//! Running a sweep against a camera: guarded set → settle → capture → score → record
//! (design D8, docs/7 P3c).
//!
//! Everything this module needs already exists and is already tested. [`crate::sweep`]
//! derives the values, [`crate::lifecycle`] is the one door to the store,
//! [`crate::capture`] gets the frame, [`crate::photo`] writes it, `imaging::metrics`
//! scores it, and [`crate::session`] decides whether the transition was legal. What is
//! here is the *order*, and the order is the part that can be got wrong quietly.
//!
//! ## One sample row, and `applied` is not decoration
//!
//! Each value produces one [`Sample`]: `{requested, applied, warnings, photo, metrics,
//! captured_at}`. D3 applies inside sweeps too \[PF:6\] — a driver clamps or aligns a
//! write and reports success — so the value the sample is *labelled* with is the one read
//! back off the device, not the one the plan asked for. A sample labelled with a value the
//! camera never held poisons every comparison built on it, and every comparison in this
//! phase is built on it: [`crate::session::select_by_metric`] chooses by `applied`,
//! [`crate::session::sampled_precision`] measures the spacing the sweep *achieved*, and
//! `calibrate apply` writes back what was chosen.
//!
//! The photo, on the other hand, is named after the **requested** value. Two requests the
//! driver collapses onto one value are two samples, and naming both files after the value
//! the device took would leave the first sample pointing at the second sample's picture.
//! The plan's values are deduplicated, so a requested value is unique within a sweep; an
//! applied value is not.
//!
//! *Within a sweep* is the whole of that guarantee, and a control's history is longer than
//! one sweep: D8's `precision` exists so a coarse pass can be followed by a fine one, and
//! the fine pass refines *around* a value the coarse pass already photographed. So the
//! photo directory carries the pass as well — `photos/<control>/<from>/<requested>.<ext>`,
//! where `from` is the sample count the control already had (note N22). Without it the
//! second pass overwrites the first's overlapping frames while the first's samples still
//! name them.
//!
//! ## The frame that is scored is the frame that is stored
//!
//! One [`crate::capture::grab`] per sample, measured and written. Two captures would be
//! two moments — a sensor drifts, a scene moves — and a sample whose numbers and whose
//! picture come from different frames is a measurement of nothing in particular. That is
//! why [`crate::photo::from_capture`] exists.
//!
//! ## Metrics rank; they do not decide
//!
//! A finished sweep leaves its control in [`schema::session::ControlStatus::Sweeping`]
//! with every planned sample recorded. It does **not** become `Calibrated`: choosing is a
//! separate act with a named selector (D8), and a sweep that quietly picked its own
//! winner would be a `Calibrated` record whose `selector` nobody could believe.
//!
//! ## Writes go through the one door
//!
//! [`crate::lifecycle::sweep_write`], every time. It persists the pre-sweep snapshot
//! before the first write reaches the camera, which is the whole of design §6's crash
//! story; a second door here would be a second chance to get that ordering wrong.

use schema::backend::Camera;
use schema::capture::{PhotoRequest, Sink, Transform};
use schema::control::{ControlDesc, ControlSlug, ControlValue};
use schema::pairing::AutomationPair;
use schema::progress::{CalibrationProgress, ProgressEvent};
use schema::session::{Sample, Session, SessionEvent};
use schema::time::Stamp;
use schema::{Applied, Error, Result};
use uuid::Uuid;

use crate::progress::ProgressSink;
use crate::settle::{Clock, Millis};
use crate::store::{SessionStore, StoreLock};
use crate::sweep::SweepPlan;
use crate::{capture, lifecycle, photo, session, sweep};

/// Everything one sweep needs that is not the session, the camera, or the clock.
///
/// The schema's, not this module's: the same request is typed on a command line (T4),
/// arrives on P4's wire (D10) and is executed here, and a type each seam translated would
/// be a request three places can disagree about.
pub use schema::session::SweepRequest;

/// What a sweep produced.
#[derive(Debug, Clone, PartialEq)]
pub struct SweepOutcome {
    /// Which control it swept.
    pub control: ControlSlug,
    /// The plan it executed, with every adjustment the planner made to the spec.
    pub plan: SweepPlan,
    /// The samples, in the order they were taken. Also on the session, and returned here
    /// so a caller does not have to go back through the document to see what it just did.
    pub samples: Vec<Sample>,
}

impl SweepOutcome {
    /// How many samples it took.
    #[must_use]
    pub fn taken(&self) -> u32 {
        u32::try_from(self.samples.len()).unwrap_or(u32::MAX)
    }
}

/// Where a sweep's state goes, what time it is, and who is watching.
///
/// Bundled rather than passed one by one because all five outlive any single sweep and
/// none of them is a decision the sweep makes: a caller assembles this once and runs every
/// control in a session against it.
pub struct SweepContext<'a> {
    /// D9's one home for state writes.
    pub store: &'a SessionStore,
    /// Held for the duration, because every write inside a sweep is a state write.
    pub lock: &'a StoreLock,
    /// The monotonic clock the settle policy runs on — stepped in tests, never slept on
    /// (note N3).
    pub clock: &'a dyn Clock,
    /// Where progress goes. [`crate::progress::Silent`] for a caller that wants none.
    pub progress: &'a dyn ProgressSink,
    /// The wall time the sweep begins.
    ///
    /// Each sample's `captured_at` is this plus the monotonic time `clock` has measured
    /// since. The engine reads no clock, and this is what that costs and buys: one wall
    /// reading per sweep, so an NTP step in the middle of a twenty-minute pan sweep cannot
    /// make sample 40 older than sample 39.
    pub started_at: Stamp,
}

impl std::fmt::Debug for SweepContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Hand-written because two of the five fields are trait objects whose `Debug` says
        // nothing a reader wants; the store's root and the start time are what identify a
        // context in a log line.
        f.debug_struct("SweepContext")
            .field("store", &self.store.root())
            .field("lock", &self.lock.protocol())
            .field("started_at", &self.started_at)
            .finish_non_exhaustive()
    }
}

/// Execute a sweep (design D8's `execution is guarded-set → settle → capture → score →
/// record`).
///
/// The pairs are the **session's** ([`Session::pairs`]), not a parameter. A sweep guarding
/// its writes against one pair set while the recovery that has to undo them reads another
/// is the failure D4's ordering exists to prevent, and the only way for the two to agree
/// across a crash is for the list to be on disk.
///
/// # Errors
///
/// The planner's, when the control cannot be swept ([`Error::IllegalTransition`] naming
/// the condition); [`Error::ControlUnknown`] when the device has no such control; the D8
/// machine's, when the control is not in a state a sweep may start from; and whatever the
/// camera or the store said at the sample that stopped it. A sweep that stops part way
/// leaves the samples it already took on the record — they happened — and the control
/// mid-sweep rather than in a terminal state.
pub fn run(
    cx: &SweepContext<'_>,
    session: &mut Session,
    camera: &mut dyn Camera,
    request: &SweepRequest,
) -> Result<SweepOutcome> {
    let (store, lock, clock) = (cx.store, cx.lock, cx.clock);
    let stream = Stream {
        session: session.id,
        started_at: cx.started_at,
        origin: clock.now_ms(),
        sink: cx.progress,
    };

    let desc = describe(camera, &request.control)?;
    // The bound on every sweep comes from here: `sweep::plan` reads
    // `limits::MAX_SWEEP_SAMPLES` and `limits::MAX_MOTION_SWEEP_SAMPLES` and widens the
    // stride rather than truncating the range. Nothing in this module re-derives it.
    let plan = sweep::plan(&desc, &request.plan, request.allow_motion)?;
    let total = plan.total();
    let pairs = session.pairs.clone();
    // Which pass this is, read before the sweep starts: the count of samples the control
    // already carries is the index this pass's first sample will take, and it is what keeps
    // this pass's photos off the previous one's (note N22). `begin_sweep` resets `done` and
    // leaves `samples`, so reading it afterwards would answer the same — but the number is
    // a property of the history rather than of the transition, and it is read where it is
    // true.
    let pass_from = session
        .controls
        .get(&request.control)
        .map_or(0, |entry| entry.samples.len());

    lifecycle::commit(store, lock, session, stream.at(clock), |draft, now| {
        session::begin_sweep(draft, &request.control, &request.plan, total, now)
    })?;
    stream.emit(
        clock,
        CalibrationProgress::SweepStarted {
            control: request.control.clone(),
            plan: request.plan.clone(),
            total,
        },
    );

    let session_dir = store.session_dir(session);
    let photo_dir_rel = crate::store::SessionStore::photo_dir_rel(&request.control, pass_from)?;
    store.create_photo_dir(lock, &session_dir, &request.control, pass_from)?;

    let mut samples: Vec<Sample> = Vec::with_capacity(plan.values.len());
    for (index, &value) in plan.values.iter().enumerate() {
        let index = u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX);
        let step = Step {
            request,
            pairs: &pairs,
            photo_dir_rel: &photo_dir_rel,
            session_dir: &session_dir,
            index,
            total,
            value,
        };
        match one_sample(cx, session, camera, &step, &stream) {
            Ok(sample) => samples.push(sample),
            Err(error) => {
                // The samples already recorded stand, and the control stays mid-sweep:
                // this is a sweep that stopped, not a sweep that failed to happen.
                //
                // Unless nothing was recorded at all, in which case it *is* a sweep that
                // failed to happen, and `Sweeping { done: 0 }` is a state with no exit
                // (`session::abandon_sweep` names them). Best effort by the same reasoning
                // as the log line below: the sweep already has a refusal to report, and the
                // store's would displace it.
                let taken = u32::try_from(samples.len()).unwrap_or(u32::MAX);
                if samples.is_empty() {
                    let _ = lifecycle::commit_state(
                        store,
                        lock,
                        session,
                        stream.at(clock),
                        |draft, now| session::abandon_sweep(draft, &request.control, now),
                    );
                }
                stream.emit(
                    clock,
                    CalibrationProgress::SweepInterrupted {
                        control: request.control.clone(),
                        taken,
                        total,
                        failure: error.kind(),
                        detail: error.to_string(),
                    },
                );
                // And the durable half, for the reader who arrives after the terminal is
                // gone: `calibrate status` shows a control stopped at 3 of 16, and without
                // this line nothing on disk says whether the camera was unplugged, the
                // sensor never settled, or the disk filled.
                //
                // Best effort **by construction**. The sweep already has a refusal to
                // report, and answering the caller with the store's instead would say "the
                // disk is full" to somebody whose camera was pulled out — the one
                // conversion AGENTS rule 7 forbids. A store that cannot take this line
                // cannot take the next operation's either, and the caller meets it there.
                let _ = lifecycle::note(
                    store,
                    lock,
                    session,
                    stream.at(clock),
                    SessionEvent::SweepInterrupted {
                        control: request.control.clone(),
                        taken,
                        total,
                        failure: error.kind(),
                        detail: error.to_string(),
                    },
                );
                return Err(error);
            }
        }
    }

    let taken = u32::try_from(samples.len()).unwrap_or(u32::MAX);
    lifecycle::commit(store, lock, session, stream.at(clock), |draft, now| {
        draft.updated_at = now;
        Ok(SessionEvent::SweepFinished {
            control: request.control.clone(),
            samples: taken,
        })
    })?;
    stream.emit(
        clock,
        CalibrationProgress::SweepFinished {
            control: request.control.clone(),
            samples: taken,
        },
    );

    Ok(SweepOutcome {
        control: request.control.clone(),
        plan,
        samples,
    })
}

/// One value's worth of work, so the interruption path has one place to catch.
struct Step<'a> {
    request: &'a SweepRequest,
    pairs: &'a [AutomationPair],
    photo_dir_rel: &'a camino::Utf8Path,
    session_dir: &'a camino::Utf8Path,
    index: u32,
    total: u32,
    value: i64,
}

/// The addressing every progress event shares, and the clock arithmetic behind its stamp.
struct Stream<'a> {
    session: Uuid,
    started_at: Stamp,
    origin: Millis,
    sink: &'a dyn ProgressSink,
}

impl Stream<'_> {
    /// Wall time now: where the sweep started, plus what the monotonic clock has counted.
    fn at(&self, clock: &dyn Clock) -> Stamp {
        let elapsed = clock.now_ms().saturating_sub(self.origin);
        i64::try_from(elapsed)
            .ok()
            .and_then(|elapsed| self.started_at.as_millis().checked_add(elapsed))
            .and_then(Stamp::from_millis)
            // A clock that has run past the representable range is not a reason to stop a
            // sweep; the sweep's own start is the honest fallback and it is monotone.
            .unwrap_or(self.started_at)
    }

    fn emit(&self, clock: &dyn Clock, progress: CalibrationProgress) {
        self.sink.emit(&ProgressEvent {
            session: self.session,
            at: self.at(clock),
            progress,
        });
    }
}

/// Set the value, wait for the sensor, take the picture, score it, record it.
fn one_sample(
    cx: &SweepContext<'_>,
    session: &mut Session,
    camera: &mut dyn Camera,
    step: &Step<'_>,
    stream: &Stream<'_>,
) -> Result<Sample> {
    let (store, lock, clock) = (cx.store, cx.lock, cx.clock);
    let control = &step.request.control;

    // The one door (P3b): it persists the pre-sweep snapshot before the first write of the
    // session reaches the camera, and it plans the write *guarded* — an automation partner
    // that owns this control has to be switched off before a manual value can be driven at
    // all (D3, and D3 applies inside sweeps too [PF:6]).
    let report = lifecycle::sweep_write(
        store,
        lock,
        session,
        camera,
        step.pairs,
        &[(control.clone(), ControlValue::Int(step.value))],
        stream.at(clock),
    )?;
    let written = applied_value(&report.writes, control)?;

    stream.emit(
        clock,
        CalibrationProgress::ValueSet {
            control: control.clone(),
            index: step.index,
            total: step.total,
            requested: step.value,
            applied: written.applied,
            warnings: written.warnings.clone(),
        },
    );

    // Before the stream starts, for the reason `photo::controls_in_effect` documents: these
    // are the values the photo is *of*, and reading them afterwards would report whatever
    // the device holds by then.
    let controls = photo::controls_in_effect(camera);
    let captured = capture::grab(camera, &step.request.stream, step.request.settle, clock)?;

    // The frame that is scored is the frame that is stored. One decode, from the frame the
    // device just delivered — not from the file, which on the verbatim path is the same
    // bytes and on the re-encode path is not the sample the sweep took.
    let luma = imaging::decode::decode_frame(&captured.frame)?
        .luma()
        .into_owned();
    let metrics = imaging::metrics::measure_all(&luma);

    let photo_rel = step.photo_dir_rel.join(format!(
        "{}.{}",
        step.value,
        step.request.photo_format.extension()
    ));
    let captured_at = stream.at(clock);
    photo::from_capture(
        camera,
        &captured,
        &PhotoRequest {
            stream: step.request.stream.clone(),
            settle: step.request.settle,
            transform: Transform::None,
            sink: Sink::ServerPath {
                path: step.session_dir.join(&photo_rel),
            },
            // `false`, and not because a sweep is impatient: this photo is taken from
            // *inside* the camera's actor thread, so there is no queue in front of it and
            // nothing for D12's flag to choose between (note N42).
            wait: false,
        },
        // The destination is the session tree's own, under a directory this process made
        // (D9) — never a path a client named, which is what note N51's non-blocking open is
        // for. There is nothing here for a client to swap.
        &mut photo::WhereverTheCallerSaid,
        controls,
        captured_at,
    )?;

    let sample = Sample {
        requested: step.value,
        applied: written.applied,
        warnings: written.warnings,
        // Relative, because D9 says a session directory relocates as a unit: an absolute
        // path here would make every sample in the document wrong the first time somebody
        // moved the tree or looked at it from another host.
        photo: photo_rel,
        metrics: metrics.clone(),
        captured_at,
    };
    lifecycle::commit(store, lock, session, captured_at, |draft, now| {
        session::record_sample(draft, control, sample.clone(), now)
    })?;

    stream.emit(
        clock,
        CalibrationProgress::SampleTaken {
            control: control.clone(),
            index: step.index,
            total: step.total,
            requested: sample.requested,
            applied: sample.applied,
            photo: sample.photo.clone(),
            metrics,
        },
    );
    Ok(sample)
}

/// What the device took, as an integer.
///
/// Read out of the write *report* rather than assumed from the request. The report is the
/// backend's answer and the backend is the only thing that read the control back (§2.10);
/// a sweep that filled this in from the planned value would record a sweep of the values
/// it asked for, which is the one thing PF:6 says a sweep may not do.
///
/// Its two refusals have no *producer* in this workspace: a T2-conforming backend reports
/// every control it wrote, and the sweep planner refuses a control whose value is not
/// scalar before the write is planned at all. P3d deferred the question of whether that
/// makes them dead code. It does not, and this is the ruling: they are the contract check
/// on a value that arrives from *outside* the engine — the trait has real and fake
/// implementations today and a third at P4 — and a function that took `writes` on faith
/// would record a sample labelled with a number nobody measured. Reaching them through a
/// backend would mean teaching a double to violate T2, which makes the double a worse
/// model of a device (AGENTS: "a fake capability no real device exhibits is a bug in the
/// fake"). So they are exercised where they live: this is a pure function of a report, and
/// [`tests::a_report_that_does_not_name_the_control_or_answers_with_a_payload_is_refused`]
/// hands it both malformed reports directly.
fn applied_value(writes: &[Applied], control: &ControlSlug) -> Result<Written> {
    let Some(applied) = writes.iter().find(|write| write.slug == *control) else {
        // The plan is built from this control, so the plan's own report must contain it.
        // If it does not, the write and the record are about different things and there is
        // no sample to be had.
        return Err(crate::refusal::illegal(
            "no_write_recorded",
            format!("record a sample for {control}"),
        ));
    };
    let Some(value) = applied.applied.as_int() else {
        // The planner already refused non-scalar controls, so this is the *device*
        // answering a scalar write with a payload. Represented rather than unwrapped
        // (D2), and refused rather than turned into a number nobody measured.
        return Err(crate::refusal::illegal(
            format!("non_scalar_readback({})", applied.applied),
            format!("record a sample for {control}"),
        ));
    };
    Ok(Written {
        applied: value,
        warnings: applied.warnings.clone(),
    })
}

/// The half of an [`Applied`] a sample keeps.
#[derive(Debug)]
struct Written {
    applied: i64,
    warnings: Vec<schema::control::WriteWarning>,
}

/// The descriptor for the control a sweep names.
///
/// Enumerated live, never taken from a session document: the device is the only authority
/// on its own range (E2), and a sweep planned from a stale range would walk values this
/// camera no longer has.
fn describe(camera: &dyn Camera, control: &ControlSlug) -> Result<ControlDesc> {
    let controls = camera.controls()?;
    controls
        .iter()
        .find(|desc| desc.slug == *control)
        .cloned()
        .ok_or_else(|| Error::ControlUnknown {
            requested: control.as_str().to_owned(),
            did_you_mean: crate::pairing::suggestions(&controls, control.as_str()),
        })
}

#[cfg(test)]
mod tests {
    use fake::{FakeBackend, Fault};
    use schema::ErrorKind;
    use schema::backend::CameraBackend;
    use schema::camera::CameraFingerprint;
    use schema::capture::{SettlePolicy, SettleSpec, StreamRequest};
    use schema::control::WriteWarning;
    use schema::metrics::MetricName;
    use schema::session::{ControlStatus, SweepSpec};
    use testkit::fixtures;

    use super::*;
    use crate::lifecycle::SessionSpec;
    use crate::progress::{Recorder, Silent};
    use crate::settle::MonotonicClock;
    use crate::store::{LockProtocol, StoreLock, TempStore};

    fn slug(name: &str) -> ControlSlug {
        ControlSlug::parse(name).expect("literal slug")
    }

    /// The fake replaying the committed synthetic profile — the one document that carries
    /// every PF-derived edge, and the device every sweep test here runs against.
    fn backend() -> FakeBackend {
        FakeBackend::from_profile(fixtures::synthetic_basic()).expect("this build's version")
    }

    fn open(backend: &FakeBackend) -> Box<dyn Camera> {
        let info = backend
            .enumerate()
            .expect("the fake enumerates what it replays")
            .into_iter()
            .next()
            .expect("one profile is one camera");
        backend.open(&info.id).expect("nothing holds a fake")
    }

    fn fingerprint(camera: &dyn Camera) -> CameraFingerprint {
        camera.info().fingerprint.clone()
    }

    fn started() -> Stamp {
        Stamp::from_millis(1_700_000_000_000).expect("in range")
    }

    /// A session on disk, with `control` queued and the device's pairs recorded — the
    /// state `calibrate start` leaves behind.
    ///
    /// Each call gets its own task slug and its own UUIDv7, so a test that needs two
    /// sessions is not fighting [`Error::SessionConflict`] (one open session per task).
    fn session_for(
        store: &SessionStore,
        lock: &StoreLock,
        camera: &mut dyn Camera,
        control: &ControlSlug,
    ) -> Session {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NTH: AtomicU64 = AtomicU64::new(0);
        let nth = NTH.fetch_add(1, Ordering::Relaxed);

        let spec = SessionSpec {
            id: Uuid::new_v7(uuid::Timestamp::from_unix(
                uuid::NoContext,
                1_700_000_000 + nth,
                0,
            )),
            fingerprint: fingerprint(camera),
            task: format!("read text from the DUT display {nth}"),
            goal: "legible text".to_owned(),
            criteria: vec!["the serial number is readable".to_owned()],
            tool_version: "0.1.0".to_owned(),
        };
        let mut session = lifecycle::create(store, lock, &spec, started()).expect("a free slot");
        lifecycle::discover_pairs(store, lock, &mut session, camera, started())
            .expect("a readable camera and a writable store");
        lifecycle::commit_state(store, lock, &mut session, started(), |draft, now| {
            session::enqueue(draft, control, now);
            Ok(())
        })
        .expect("queueing is legal");
        session
    }

    /// Five focus values with the fixture's declared optimum among them, so a sweep is
    /// nine seconds shorter than `SweepSpec::All` and still spans the range.
    fn five_across_focus() -> SweepSpec {
        SweepSpec::Explicit {
            values: vec![0, 256, 512, 768, 1_023],
        }
    }

    fn context<'a>(
        temp: &'a TempStore,
        lock: &'a StoreLock,
        clock: &'a MonotonicClock,
        progress: &'a dyn ProgressSink,
    ) -> SweepContext<'a> {
        SweepContext {
            store: temp.store(),
            lock,
            clock,
            progress,
            started_at: started(),
        }
    }

    // ------------------------------------------------------------------ the sample row

    #[test]
    fn a_sample_records_what_the_device_took_and_names_its_photo_after_what_was_asked() {
        // PF:6 inside a sweep. `Fault::ClampOnWrite` makes the first write land somewhere
        // else, which is the only way to tell a sweep that records `applied` from one that
        // records `requested` twice — on a well-behaved control the two are equal and the
        // defect is invisible.
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let backend = backend();
        let mut camera = open(&backend);
        let control = slug("focus_absolute");
        let mut session = session_for(temp.store(), &lock, camera.as_mut(), &control);

        backend.queue_fault(Fault::ClampOnWrite);
        let clock = MonotonicClock::new();
        let outcome = run(
            &context(&temp, &lock, &clock, &Silent),
            &mut session,
            camera.as_mut(),
            &SweepRequest::new(control.clone(), five_across_focus()),
        )
        .expect("a willing camera and a writable store");

        let first = outcome.samples.first().expect("five samples");
        assert_eq!(
            first.requested, 0,
            "the plan asked for the bottom of the range"
        );
        assert_eq!(
            first.applied, 1_023,
            "the sample was labelled with the value the sweep asked for, not the one the \
             device took: {first:?}"
        );
        assert!(
            first
                .warnings
                .iter()
                .any(|warning| matches!(warning, WriteWarning::Adjusted { .. })),
            "the move was recorded without saying it happened: {:?}",
            first.warnings
        );
        // And the photo is named after the *request*, so two values the driver collapses
        // onto one do not overwrite each other's picture — under the pass this sweep is,
        // so the *next* pass's overlapping values do not overwrite this one's either.
        assert_eq!(first.photo, "photos/focus_absolute/0/0.jpg");

        // The inverse, from the same run: every other write was exact, so the pair is not
        // equal by construction.
        for sample in outcome.samples.iter().skip(1) {
            assert_eq!(
                sample.requested, sample.applied,
                "an unfaulted write moved: {sample:?}"
            );
            assert!(sample.warnings.is_empty(), "{sample:?}");
        }
    }

    #[test]
    fn every_sample_photo_is_a_relative_path_that_resolves_under_the_session_directory() {
        // D9: a session directory relocates as a unit. An absolute path here would make
        // every sample in the document wrong the first time the tree moved.
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let backend = backend();
        let mut camera = open(&backend);
        let control = slug("brightness");
        let mut session = session_for(temp.store(), &lock, camera.as_mut(), &control);

        let clock = MonotonicClock::new();
        let outcome = run(
            &context(&temp, &lock, &clock, &Silent),
            &mut session,
            camera.as_mut(),
            &SweepRequest::new(
                control.clone(),
                SweepSpec::Explicit {
                    values: vec![0, 128, 255],
                },
            ),
        )
        .expect("a willing camera");

        let dir = temp.store().session_dir(&session);
        for sample in &outcome.samples {
            assert!(
                sample.photo.is_relative(),
                "{} is not relative to the session directory",
                sample.photo
            );
            assert!(
                sample.photo.starts_with("photos/brightness"),
                "{} is not under the control's photo directory",
                sample.photo
            );
            let on_disk = dir.join(&sample.photo);
            let bytes = std::fs::read(on_disk.as_std_path())
                .unwrap_or_else(|error| panic!("{on_disk} could not be read: {error}"));
            assert_eq!(
                &bytes[..2],
                &[0xff, 0xd8],
                "{on_disk} is not the JPEG the sample claims"
            );
        }
        // And the document says the same thing the outcome does, which is what a second
        // process reads.
        let stored = temp.store().load_session(&dir).expect("readable");
        let recorded: Vec<&camino::Utf8Path> = stored.controls[&control]
            .samples
            .iter()
            .map(|sample| sample.photo.as_path())
            .collect();
        assert_eq!(
            recorded,
            outcome
                .samples
                .iter()
                .map(|sample| sample.photo.as_path())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_finished_sweep_leaves_the_control_swept_and_chooses_nothing() {
        // "Metrics rank; they do not decide" (D8), as a property of the executor: a sweep
        // that auto-selected would produce a `Calibrated` record whose `selector` names a
        // metric nobody asked to be trusted, and rubric B5 forbids exactly that path.
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let backend = backend();
        let mut camera = open(&backend);
        let control = slug("brightness");
        let mut session = session_for(temp.store(), &lock, camera.as_mut(), &control);

        let clock = MonotonicClock::new();
        let outcome = run(
            &context(&temp, &lock, &clock, &Silent),
            &mut session,
            camera.as_mut(),
            &SweepRequest::new(
                control.clone(),
                SweepSpec::Explicit {
                    values: vec![0, 255],
                },
            ),
        )
        .expect("a willing camera");
        assert_eq!(outcome.taken(), 2);

        let status = &session.controls[&control].status;
        assert_eq!(
            status,
            &ControlStatus::Sweeping {
                plan: SweepSpec::Explicit {
                    values: vec![0, 255]
                },
                done: 2,
                total: 2,
            },
            "the sweep chose a value nobody selected"
        );
        // Every sample carries the whole metric set, because a missing key reads as "not
        // measured" and is indistinguishable from zero downstream.
        for sample in &outcome.samples {
            for &metric in MetricName::ALL {
                assert!(
                    sample.metrics.contains_key(&metric),
                    "{metric} is missing from {sample:?}"
                );
            }
        }
        // And the inverse: selecting is available, and it is what produces `Calibrated`.
        lifecycle::commit(
            temp.store(),
            &lock,
            &mut session,
            started(),
            |draft, now| session::select_by_metric(draft, &control, MetricName::RmsContrast, now),
        )
        .expect("a swept control selects");
        assert!(matches!(
            session.controls[&control].status,
            ControlStatus::Calibrated { .. }
        ));
    }

    #[test]
    fn a_sweep_of_an_automation_owned_control_switches_the_owner_off_first() {
        // D3 applies inside sweeps too [PF:6]. `white_balance_temperature` is INACTIVE on
        // this fixture because `white_balance_automatic` is on, so the *only* way to sweep
        // it is to switch the owner off — and the pair set that says which control that is
        // comes off the session, where the probe wrote it.
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let device = backend();
        let mut camera = open(&device);
        let control = slug("white_balance_temperature");
        let mut session = session_for(temp.store(), &lock, camera.as_mut(), &control);
        assert!(
            session.pairs.iter().any(|pair| pair.manual == control),
            "the probe found no owner for the control this test sweeps: {:?}",
            session.pairs
        );

        let clock = MonotonicClock::new();
        let outcome = run(
            &context(&temp, &lock, &clock, &Silent),
            &mut session,
            camera.as_mut(),
            &SweepRequest::new(
                control.clone(),
                SweepSpec::Explicit {
                    values: vec![2_800, 6_500],
                },
            ),
        )
        .expect("a guarded sweep releases the control it is about to drive");
        assert_eq!(
            outcome
                .samples
                .iter()
                .map(|sample| sample.applied)
                .collect::<Vec<_>>(),
            vec![2_800, 6_500],
            "the values did not stick, which is what an unguarded write looks like"
        );

        // The inverse, on a fresh session and a fresh *device*: with no pair set recorded,
        // the same sweep is refused rather than quietly writing to a control its owner
        // still holds. A second backend rather than a second handle — two handles onto one
        // camera are two views of one device (T2), so the sweep above would have left the
        // automation switched off and there would be nothing to release.
        let untouched = backend();
        let mut fresh = open(&untouched);
        let mut unguarded = session_for(temp.store(), &lock, fresh.as_mut(), &control);
        unguarded.pairs.clear();
        let error = run(
            &context(&temp, &lock, &clock, &Silent),
            &mut unguarded,
            fresh.as_mut(),
            &SweepRequest::new(
                control,
                SweepSpec::Explicit {
                    values: vec![2_800],
                },
            ),
        )
        .expect_err("nothing can release a control whose owner is unknown");
        assert_eq!(error.kind(), ErrorKind::ControlInactive);
    }

    #[test]
    fn a_report_that_does_not_name_the_control_or_answers_with_a_payload_is_refused() {
        // The two contract checks in `applied_value`, closed here rather than deferred a
        // third time. No backend in this workspace produces either report — that is the
        // point of a contract check — so they are driven where they are decidable: this is
        // a pure function of a `WriteReport`'s writes, and a malformed report is a value a
        // test can simply construct.
        let control = slug("focus_absolute");
        let other = slug("brightness");

        // A report about a different control: the write and the record would be about two
        // different things, and there is no sample to be had.
        let elsewhere = vec![Applied {
            control: schema::control::ControlId(1),
            slug: other.clone(),
            requested: ControlValue::Int(7),
            applied: ControlValue::Int(7),
            warnings: Vec::new(),
        }];
        let error = applied_value(&elsewhere, &control).expect_err("nothing wrote this control");
        assert_eq!(error.kind(), ErrorKind::IllegalTransition);
        assert!(
            error.to_string().contains("no_write_recorded"),
            "the refusal did not name the condition: {error}"
        );
        assert!(
            applied_value(&[], &control).is_err(),
            "an empty report answered for a control nobody wrote"
        );

        // A scalar control answering with a payload (D2's "represent the unknown"): refused
        // rather than turned into a number nobody measured.
        let payload = vec![Applied {
            control: schema::control::ControlId(2),
            slug: control.clone(),
            requested: ControlValue::Int(7),
            applied: ControlValue::Bytes(vec![0, 1, 2, 3]),
            warnings: Vec::new(),
        }];
        let error = applied_value(&payload, &control).expect_err("a payload is not a value");
        assert_eq!(error.kind(), ErrorKind::IllegalTransition);
        assert!(
            error.to_string().contains("non_scalar_readback"),
            "the refusal did not name the condition: {error}"
        );

        // And the twin, so neither arm is passing for the wrong reason: a conforming report
        // answers, carrying what the device took rather than what was asked for.
        let honest = vec![Applied {
            control: schema::control::ControlId(3),
            slug: control.clone(),
            requested: ControlValue::Int(7),
            applied: ControlValue::Int(6),
            warnings: vec![WriteWarning::Adjusted {
                requested: ControlValue::Int(7),
                applied: ControlValue::Int(6),
            }],
        }];
        let written = applied_value(&honest, &control).expect("a conforming report");
        assert_eq!(written.applied, 6);
        assert_eq!(written.warnings.len(), 1);
    }

    // ------------------------------------------------------------------ the settle

    #[test]
    fn a_sample_is_taken_after_the_sensor_has_settled_and_not_before() {
        // PF:11, and the mutant this catches is "the sweep skipped the settle". The
        // expectation is not a number from a model: it is a frame taken off *this camera*
        // with the settle explicitly disabled, which is what an unsettled sample would
        // look like. The settled sample has to be brighter than that.
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let backend = backend();
        let mut camera = open(&backend);
        let control = slug("brightness");
        let mut session = session_for(temp.store(), &lock, camera.as_mut(), &control);
        let clock = MonotonicClock::new();

        // The unsettled reading, measured rather than assumed: the first frame off a fresh
        // stream at the same control value the sweep will use.
        crate::write::set(
            camera.as_mut(),
            &session.pairs,
            &[(control.clone(), ControlValue::Int(255))],
            true,
        )
        .expect("a willing camera");
        let unsettled = capture::grab(
            camera.as_mut(),
            &StreamRequest::default(),
            SettlePolicy {
                spec: SettleSpec::SkipFrames { frames: 0 },
                deadline_ms: 5_000,
            },
            &clock,
        )
        .expect("a frame");
        assert_eq!(
            unsettled.frames_settled, 0,
            "the control arm settled anyway"
        );
        let unsettled_luma = imaging::metrics::mean_luma(
            &imaging::decode::decode_frame(&unsettled.frame)
                .expect("a decodable frame")
                .luma(),
        );

        let outcome = run(
            &context(&temp, &lock, &clock, &Silent),
            &mut session,
            camera.as_mut(),
            &SweepRequest::new(control.clone(), SweepSpec::Explicit { values: vec![255] }),
        )
        .expect("a willing camera");
        let settled_luma = outcome.samples[0].metrics[&MetricName::MeanLuma];
        assert!(
            settled_luma > unsettled_luma,
            "the sweep's sample is no brighter than an unsettled frame ({settled_luma} vs \
             {unsettled_luma}): the settle was skipped"
        );
    }

    #[test]
    fn one_sample_is_one_capture_and_the_frame_it_scores_is_the_frame_it_stores() {
        // The module header states it as a law, and a deterministic synthesizer cannot
        // prove it from the frames: two captures of one scene are byte-identical, so the
        // metrics and the photo agree either way. What separates the two implementations
        // is how many times the sensor was started — a second capture is a second moment
        // on real hardware and twice the wall clock on any.
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let backend = backend();
        let mut camera = open(&backend);
        let control = slug("brightness");
        let mut session = session_for(temp.store(), &lock, camera.as_mut(), &control);
        assert_eq!(
            backend.streams_started(),
            0,
            "starting a session streamed from the camera"
        );

        let clock = MonotonicClock::new();
        run(
            &context(&temp, &lock, &clock, &Silent),
            &mut session,
            camera.as_mut(),
            &SweepRequest::new(
                control,
                SweepSpec::Explicit {
                    values: vec![0, 128, 255],
                },
            ),
        )
        .expect("a willing camera");
        assert_eq!(
            backend.streams_started(),
            3,
            "three samples started the sensor a different number of times"
        );
    }

    // ------------------------------------------------------------------ the plan's bounds

    #[test]
    fn a_control_with_no_ordered_range_is_refused_by_the_planner_rather_than_swept_anyway() {
        // The executor asks [`crate::sweep::plan`] and takes its answer: the caps, the
        // motion rule (§5) and the not-sweepable rule are that module's law, and a second
        // copy here would be a second copy that drifts. `region_of_interest_rectangle` is
        // the fixture's PF:1 control — it enumerates and round-trips, and it does not
        // sweep.
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let backend = backend();
        let mut camera = open(&backend);
        let control = slug("region_of_interest_rectangle");
        let mut session = session_for(temp.store(), &lock, camera.as_mut(), &control);
        let clock = MonotonicClock::new();

        let error = run(
            &context(&temp, &lock, &clock, &Silent),
            &mut session,
            camera.as_mut(),
            &SweepRequest::new(control.clone(), SweepSpec::All),
        )
        .expect_err("a compound control has no ordered range to walk");
        assert_eq!(error.kind(), ErrorKind::IllegalTransition);
        assert!(
            error.to_string().contains("not_sweepable"),
            "the refusal did not name the condition: {error}"
        );
        // Nothing was recorded: a refused sweep must not leave a control mid-sweep.
        assert!(!session.controls.contains_key(&control));
    }

    #[test]
    fn a_control_this_camera_does_not_have_is_refused_with_the_closest_names() {
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let backend = backend();
        let mut camera = open(&backend);
        let control = slug("focus_absoluteish");
        let mut session = session_for(temp.store(), &lock, camera.as_mut(), &control);
        let clock = MonotonicClock::new();

        let error = run(
            &context(&temp, &lock, &clock, &Silent),
            &mut session,
            camera.as_mut(),
            &SweepRequest::new(control, SweepSpec::All),
        )
        .expect_err("no such control");
        assert_eq!(error.kind(), ErrorKind::ControlUnknown);
        assert!(
            error.to_string().contains("focus_absolute"),
            "a refusal nobody can act on: {error}"
        );
    }

    // ------------------------------------------------------------------ the progress hook

    #[test]
    fn a_clean_sweep_emits_started_then_a_pair_per_sample_then_finished() {
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let backend = backend();
        let mut camera = open(&backend);
        let control = slug("brightness");
        let mut session = session_for(temp.store(), &lock, camera.as_mut(), &control);

        let recorder = Recorder::new();
        let clock = MonotonicClock::new();
        run(
            &context(&temp, &lock, &clock, &recorder),
            &mut session,
            camera.as_mut(),
            &SweepRequest::new(
                control.clone(),
                SweepSpec::Explicit {
                    values: vec![0, 128],
                },
            ),
        )
        .expect("a willing camera");

        assert_eq!(
            recorder.sequence(),
            vec![
                "sweep_started",
                "value_set",
                "sample_taken",
                "value_set",
                "sample_taken",
                "sweep_finished",
            ]
        );
        // Every event names the session it belongs to, because P4e's subscription is per
        // client and a client may watch more than one.
        assert!(
            recorder
                .events()
                .iter()
                .all(|event| event.session == session.id)
        );
        // And the indices count the samples rather than restarting or repeating.
        let indices: Vec<u32> = recorder
            .events()
            .iter()
            .filter_map(|event| match event.progress {
                CalibrationProgress::SampleTaken { index, total, .. } => {
                    assert_eq!(total, 2);
                    Some(index)
                }
                _ => None,
            })
            .collect();
        assert_eq!(indices, vec![1, 2]);
    }
}
