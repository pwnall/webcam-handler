//! A calibration session, end to end, over the synthetic profile (docs/7 P3c, gate G3).
//!
//! This is the suite the phase's headline criterion lives in: a scripted session sweeps a
//! control whose frame model has a known optimum, and `metric:sharpness` picks it.
//!
//! ## Where the expectation comes from, and where it does not
//!
//! [`FOCUS_OPTIMUM`] is a literal in this file, and the first thing every test using it
//! does is check the **committed fixture** still declares it — `focus_absolute`'s default
//! in `crates/testkit/fixtures/synthetic-basic.json`, loaded from the bytes on disk. The
//! fake's frame model answers to the same number from the other side: its blur is zero at
//! the focus control's declared default and grows either way from there
//! (`fake::frames`'s rule 1, stated in that module as a *declaration* precisely so it can
//! be depended on from outside).
//!
//! Nothing here asks the fake where its own peak is. `FakeCamera::focus_optimum` exists
//! and this suite deliberately does not call it: a test that asked the fake for the
//! answer and then asserted the sweep found it would be two halves of one implementation
//! agreeing with each other — docs/8 Part C's "the fake validating the fake", which is
//! N10's failure in a calibration costume. The fixture states the number; the fake and
//! the assertion are both answerable to it, and either one drifting from it turns this
//! suite red.
//!
//! ## Why the clock here cannot move
//!
//! Every sweep below runs on [`FrozenClock`], not on a real one. Nothing in this file is
//! about the settle deadline: the assertions are about which sample won, what the device
//! answered when it vanished, and what reached the disk. A real clock puts a second,
//! unasked-for outcome inside reach of each of them — a settle that has not converged by
//! `DEFAULT_SETTLE_DEADLINE_MS` is a `SettleTimeout`, which is a correct answer to a
//! question this file is not asking, and on a loaded machine it is the answer that arrives
//! (note N60: eleven frames, 5303 ms, an assertion expecting `DeviceGone`). A deadline that
//! cannot expire removes that outcome, so a red run here means the sweep is wrong rather
//! than that the machine was busy.
//!
//! ## What "both directions" means for a physics claim
//!
//! It is not enough that the winner is 512. `the_optimum_wins_and_every_other_sample_loses`
//! asserts the *ordering* the selection rests on — every other sampled value scored
//! strictly below the optimum — so an executor that scored the wrong frame, a metric that
//! ranked backwards, or a sweep that recorded the requested value where the applied one
//! belongs each produce a different winner and a red run rather than a coincidence.

use std::collections::BTreeMap;

use camino::{Utf8Path, Utf8PathBuf};
use engine::calibrate::{self, SweepContext, SweepRequest};
use engine::lifecycle::{self, SessionSpec};
use engine::progress::{ProgressSink, Recorder};
use engine::session;
use engine::settle::FrozenClock;
use engine::store::{LockProtocol, SessionStore, StoreLock, TempStore};
use fake::{FakeBackend, Fault};
use schema::ErrorKind;
use schema::backend::{Camera, CameraBackend};
use schema::capture::StreamRequest;
use schema::control::ControlSlug;
use schema::metrics::MetricName;
use schema::progress::{CalibrationProgress, ProgressEvent};
use schema::session::{ControlStatus, Selector, Session, SessionEvent, SweepSpec};
use schema::time::Stamp;
use uuid::Uuid;

/// The value at which the synthetic camera's frames are sharpest.
///
/// Stated here as a literal and checked against the committed fixture by
/// [`the_fixture_declares_the_optimum_this_suite_states`] before anything asserts a sweep
/// found it. See this module's header for why it is not read out of the fake.
const FOCUS_OPTIMUM: i64 = 512;

/// A sweep of `control` under `plan`, at a size this suite chose rather than one the camera
/// did.
///
/// [`SweepRequest::new`] leaves the stream unspecified, and since D5's amendment of
/// 2026-08-13 an unspecified stream resolves to the device's **largest** mode — 3840×2160 on
/// this fixture, which the fake renders and JPEG-encodes in software once per sample. That
/// is the right answer for a photograph of a device under test and thirty seconds of waste
/// for a suite whose subject is the state machine, so every sweep here names a size instead.
///
/// Naming one is the documented way out rather than a workaround: an explicit request wins
/// over the ranking, which is exactly the property `schema::capture`'s
/// `an_explicit_request_beats_the_ranking_in_both_of_its_halves` pins.
fn sweep_request(control: ControlSlug, plan: SweepSpec) -> SweepRequest {
    SweepRequest {
        stream: StreamRequest {
            width: Some(640),
            height: Some(480),
            ..StreamRequest::default()
        },
        ..SweepRequest::new(control, plan)
    }
}

/// The control the headline criterion sweeps.
const FOCUS: &str = "focus_absolute";

/// Five values spanning the fixture's declared focus range, with the optimum among them.
///
/// Explicit rather than `SweepSpec::All`: 1024 photos would prove nothing 5 do not, and
/// the four detuned values are far enough from the optimum that the ordering under test is
/// a physical separation rather than a rounding difference.
fn five_across_focus() -> SweepSpec {
    SweepSpec::Explicit {
        values: vec![0, 256, FOCUS_OPTIMUM, 768, 1_023],
    }
}

fn slug(name: &str) -> ControlSlug {
    ControlSlug::parse(name).expect("literal slug")
}

fn started() -> Stamp {
    Stamp::from_millis(1_700_000_000_000).expect("in range")
}

fn backend() -> FakeBackend {
    FakeBackend::from_profile(testkit::fixtures::synthetic_basic()).expect("this build's version")
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

/// A session on disk with `control` queued and the device's pairs probed and recorded —
/// what `calibrate start` leaves behind (docs/7 P3d wires the verb; this is the state).
fn start_session(
    store: &SessionStore,
    lock: &StoreLock,
    camera: &mut dyn Camera,
    control: &ControlSlug,
) -> Session {
    let spec = SessionSpec {
        id: Uuid::new_v7(uuid::Timestamp::from_unix(
            uuid::NoContext,
            1_700_000_000,
            0,
        )),
        fingerprint: camera.info().fingerprint.clone(),
        task: "read text from the DUT display".to_owned(),
        goal: "the DUT's serial number is legible".to_owned(),
        criteria: vec!["the serial number is readable at arm's length".to_owned()],
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

fn context<'a>(
    store: &'a SessionStore,
    lock: &'a StoreLock,
    clock: &'a FrozenClock,
    progress: &'a dyn ProgressSink,
) -> SweepContext<'a> {
    SweepContext {
        store,
        lock,
        clock,
        progress,
        started_at: started(),
    }
}

/// The sharpness score of every sample, by the value the device actually held.
fn sharpness_by_applied(session: &Session, control: &ControlSlug) -> BTreeMap<i64, f64> {
    session.controls[control]
        .samples
        .iter()
        .map(|sample| {
            (
                sample.applied,
                *sample
                    .metrics
                    .get(&MetricName::Sharpness)
                    .unwrap_or_else(|| panic!("no sharpness on {sample:?}")),
            )
        })
        .collect()
}

// ---------------------------------------------------------------- the fixture's claim

#[test]
fn the_fixture_declares_the_optimum_this_suite_states() {
    // The anchor. `FOCUS_OPTIMUM` is a number in this file, and this is what stops it
    // being a number in this file *only*: the committed document, read from its bytes,
    // has to declare the same default — which is what the fake's frame model reads to
    // decide where its blur is zero. Edit either side and this fails before any sweep runs.
    let profile = testkit::fixtures::load_synthetic_basic().expect("the committed fixture");
    let focus = profile
        .control(&slug(FOCUS))
        .expect("the fixture carries a focus control");
    assert_eq!(
        focus.default, FOCUS_OPTIMUM,
        "the committed fixture declares {} as {FOCUS}'s default; this suite is written \
         against {FOCUS_OPTIMUM}",
        focus.default
    );
    // And the range the sweep walks is the one the plan below assumes.
    assert_eq!((focus.range.min, focus.range.max), (0, 1_023));
    assert!(
        focus.range.min < FOCUS_OPTIMUM && FOCUS_OPTIMUM < focus.range.max,
        "an optimum at the edge of the range cannot be lost in both directions"
    );
}

// ---------------------------------------------------------------- the headline criterion

#[test]
fn a_scripted_session_calibrates_focus_at_the_optimum_the_fixture_declares() {
    // Gate G3's headline row: sweep → score → select, over the synthetic profile, ending
    // in `Calibrated` at the value the fixture declares — with `metric:sharpness` named on
    // the record as the thing that chose it.
    let temp = TempStore::new().expect("a temp dir");
    let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
    let backend = backend();
    let mut camera = open(&backend);
    let control = slug(FOCUS);
    let mut session = start_session(temp.store(), &lock, camera.as_mut(), &control);

    let clock = FrozenClock;
    let recorder = Recorder::new();
    let outcome = calibrate::run(
        &context(temp.store(), &lock, &clock, &recorder),
        &mut session,
        camera.as_mut(),
        &sweep_request(control.clone(), five_across_focus()),
    )
    .expect("a willing camera and a writable store");
    assert_eq!(outcome.taken(), 5);

    // Metrics rank; the selector decides — and the selector is named on the record.
    let event = lifecycle::commit(
        temp.store(),
        &lock,
        &mut session,
        started(),
        |draft, now| session::select_by_metric(draft, &control, MetricName::Sharpness, now),
    )
    .expect("a swept control selects");
    assert_eq!(
        event,
        schema::session::SessionEvent::Selected {
            control: control.clone(),
            value: FOCUS_OPTIMUM,
            selector: Selector::Metric {
                name: MetricName::Sharpness
            },
        }
    );

    let ControlStatus::Calibrated {
        value,
        precision,
        score,
        selector,
    } = session.controls[&control].status.clone()
    else {
        panic!(
            "the control did not reach Calibrated: {:?}",
            session.controls[&control].status
        );
    };
    assert_eq!(value, FOCUS_OPTIMUM, "sharpness picked the wrong sample");
    assert_eq!(
        selector,
        Selector::Metric {
            name: MetricName::Sharpness
        },
        "nothing may record a calibrated value without naming who chose it"
    );
    assert_eq!(
        precision, 255,
        "the recorded precision is the spacing the sweep achieved, not the one it planned"
    );
    assert!(
        score.is_some_and(f64::is_finite),
        "a metric selection with no score: {score:?}"
    );

    // And the same is true of the document a second process would read.
    let stored = temp
        .store()
        .load_session(&temp.store().session_dir(&session))
        .expect("readable");
    assert_eq!(stored.calibrated(), vec![(&control, FOCUS_OPTIMUM)]);
    assert!(stored.is_settled(), "the queued control is terminal");
}

#[test]
fn the_optimum_wins_and_every_other_sample_loses() {
    // The both-directions half of the physics claim. "The winner is 512" is one bit; this
    // asserts the whole ordering the selection rests on, so a wrong optimum is not merely
    // improbable but impossible to reach with this suite green.
    let temp = TempStore::new().expect("a temp dir");
    let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
    let backend = backend();
    let mut camera = open(&backend);
    let control = slug(FOCUS);
    let mut session = start_session(temp.store(), &lock, camera.as_mut(), &control);

    let clock = FrozenClock;
    calibrate::run(
        &context(temp.store(), &lock, &clock, &engine::progress::Silent),
        &mut session,
        camera.as_mut(),
        &sweep_request(control.clone(), five_across_focus()),
    )
    .expect("a willing camera");

    let scores = sharpness_by_applied(&session, &control);
    let peak = *scores
        .get(&FOCUS_OPTIMUM)
        .expect("the optimum was among the sampled values");
    for (&applied, &score) in &scores {
        if applied == FOCUS_OPTIMUM {
            continue;
        }
        assert!(
            score < peak,
            "focus {applied} scored {score}, which is not below the optimum's {peak}"
        );
    }
    // Monotone away from the optimum in both directions, which is the shape that makes
    // "sharpest" mean "in focus" rather than "happened to win".
    assert!(scores[&256] > scores[&0], "{scores:?}");
    assert!(scores[&768] > scores[&1_023], "{scores:?}");
}

// ---------------------------------------------------------------- the progress hook

/// A progress sink that records, and — after the `after`-th sample — reaches through to
/// the backend and breaks the camera.
///
/// The seam earning its keep: it proves the hook is emitted **while the sweep runs**
/// rather than assembled and flushed at the end. An executor that batched its events
/// would never sabotage anything in time, and the sweep would finish instead of stopping.
#[derive(Debug)]
struct SabotageAfter<'a> {
    backend: &'a FakeBackend,
    recorder: Recorder,
    after: u32,
}

impl ProgressSink for SabotageAfter<'_> {
    fn emit(&self, event: &ProgressEvent) {
        self.recorder.emit(event);
        if let CalibrationProgress::SampleTaken { index, .. } = event.progress
            && index == self.after
        {
            self.backend.queue_fault(Fault::DeviceGoneMidStream);
        }
    }
}

#[test]
fn an_interrupted_sweep_says_where_it_stopped_and_keeps_what_it_took() {
    let temp = TempStore::new().expect("a temp dir");
    let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
    let backend = backend();
    let mut camera = open(&backend);
    let control = slug(FOCUS);
    let mut session = start_session(temp.store(), &lock, camera.as_mut(), &control);

    let clock = FrozenClock;
    let saboteur = SabotageAfter {
        backend: &backend,
        recorder: Recorder::new(),
        after: 2,
    };
    let error = calibrate::run(
        &context(temp.store(), &lock, &clock, &saboteur),
        &mut session,
        camera.as_mut(),
        &sweep_request(control.clone(), five_across_focus()),
    )
    .expect_err("the camera disappears during the third sample");
    assert_eq!(
        error.kind(),
        ErrorKind::DeviceGone,
        "the device's own answer was reshaped on its way out: {error}"
    );

    // The stream, in order: two complete samples, a third value that was written and
    // never photographed, and one interruption. No `sweep_finished`.
    assert_eq!(
        saboteur.recorder.sequence(),
        vec![
            "sweep_started",
            "value_set",
            "sample_taken",
            "value_set",
            "sample_taken",
            "value_set",
            "sweep_interrupted",
        ]
    );
    let last = saboteur
        .recorder
        .events()
        .last()
        .expect("the stream is not empty")
        .progress
        .clone();
    let CalibrationProgress::SweepInterrupted {
        taken,
        total,
        failure,
        ref detail,
        ..
    } = last
    else {
        panic!("the sweep ended with {last:?}");
    };
    assert_eq!(
        (taken, total),
        (2, 5),
        "the count of what survived is wrong"
    );
    assert_eq!(failure, ErrorKind::DeviceGone);
    assert!(!detail.is_empty(), "an interruption nobody can read");

    // …and the durable half, which is what `calibrate status` reads after the terminal
    // that showed the live event is gone. The recorded `sample_taken` lines already say
    // *where* the sweep stopped; without this line nothing on disk says whether the camera
    // was pulled out, the sensor never settled [PF:11], or the disk filled — three
    // outcomes design keeps apart everywhere else.
    let history = temp
        .store()
        .load_log(&temp.store().session_dir(&session))
        .expect("readable");
    let stopped = history
        .iter()
        .filter_map(|entry| match &entry.event {
            SessionEvent::SweepInterrupted {
                taken,
                total,
                failure,
                detail,
                ..
            } => Some((*taken, *total, *failure, detail.clone())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        stopped,
        vec![(2, 5, ErrorKind::DeviceGone, error.to_string())],
        "the session's history does not say why the sweep stopped: {history:?}"
    );
    assert!(
        !history
            .iter()
            .any(|entry| matches!(entry.event, SessionEvent::SweepFinished { .. })),
        "a sweep that stopped claimed it finished"
    );

    // The samples that were taken stand — they happened — and the control is left
    // mid-sweep rather than forced into a terminal state, so the session is still open.
    let stored = temp
        .store()
        .load_session(&temp.store().session_dir(&session))
        .expect("readable");
    assert_eq!(
        stored.controls[&control].status,
        ControlStatus::Sweeping {
            plan: five_across_focus(),
            done: 2,
            total: 5,
        }
    );
    assert_eq!(stored.controls[&control].samples.len(), 2);
    assert!(
        !stored.is_settled(),
        "an interrupted sweep settled a control"
    );
    assert!(
        lifecycle::resume(temp.store(), &stored.fingerprint, &stored.task)
            .expect("readable")
            .is_some(),
        "an interrupted session cannot be picked up again"
    );

    // And the camera can still be put back, because the pre-sweep snapshot went to disk
    // before the first write did (design §6).
    assert!(
        stored.pre_snapshot.is_some(),
        "an interrupted sweep left a moved camera with nothing to restore it from"
    );
    let recovery = lifecycle::recover(
        temp.store(),
        &lock,
        &mut session,
        camera.as_mut(),
        &stored.pairs,
        started(),
    )
    .expect("the snapshot belongs to this camera");
    assert!(
        recovery
            .report()
            .is_some_and(schema::snapshot::RestoreReport::is_complete),
        "the camera could not be put back after an interrupted sweep: {recovery:?}"
    );
}

#[test]
fn a_sweep_that_stopped_before_its_first_sample_leaves_the_control_sweepable_again() {
    // Availability is not capability (AGENTS rule 7, rubric A4), one layer up. An
    // interruption *after* a sample is a sweep that stopped and the samples stand: `select`
    // moves the control to `Calibrated`, which is terminal, and the arm above asserts that.
    // With **zero** samples every exit was closed — `may_begin_sweep` refuses `Sweeping`,
    // `select` refuses `no_samples`, `draft` skips anything that is not `Untouched`, no
    // shipped verb produces `Deferred`, and `Sweeping` is never terminal so the (camera,
    // task) slot never settles either. An unplug, a `SettleTimeout` [PF:11] or an `ENOSPC`
    // on the first photo became a permanent refusal for that control.
    //
    // Nothing happened, so nothing is recorded: the control goes back where the sweep found
    // it, and only the `SweepInterrupted` line (N18) says the attempt was made.
    let temp = TempStore::new().expect("a temp dir");
    let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
    let backend = backend();
    let mut camera = open(&backend);
    let control = slug(FOCUS);
    let mut session = start_session(temp.store(), &lock, camera.as_mut(), &control);
    let clock = FrozenClock;

    // The fault fires inside sample 1 — after `begin_sweep` has committed `Sweeping`, and
    // before any sample exists.
    backend.queue_fault(Fault::DeviceGoneMidStream);
    let error = calibrate::run(
        &context(temp.store(), &lock, &clock, &Recorder::new()),
        &mut session,
        camera.as_mut(),
        &sweep_request(control.clone(), five_across_focus()),
    )
    .expect_err("the camera disappears during the first sample");
    assert_eq!(
        error.kind(),
        ErrorKind::DeviceGone,
        "the device's own answer was reshaped on its way out: {error}"
    );

    // On disk, which is what the next process reads.
    let stored = temp
        .store()
        .load_session(&temp.store().session_dir(&session))
        .expect("readable");
    assert_eq!(
        stored.controls.get(&control).map(|entry| &entry.status),
        Some(&ControlStatus::Untouched),
        "a sweep that took no samples left the control mid-sweep with no way out"
    );
    assert!(
        stored.controls[&control].samples.is_empty(),
        "a sweep that recorded nothing left samples behind"
    );
    // The attempt is still on the record: what was lost is a status nobody can leave, not
    // the history (N18).
    let history = temp
        .store()
        .load_log(&temp.store().session_dir(&session))
        .expect("readable");
    assert!(
        history
            .iter()
            .any(|entry| matches!(entry.event, SessionEvent::SweepInterrupted { taken: 0, .. })),
        "the session's history does not say the sweep was attempted: {history:?}"
    );

    // And the exit that was closed: the same control sweeps again on a healthy camera, and
    // the session settles the ordinary way.
    let outcome = calibrate::run(
        &context(temp.store(), &lock, &clock, &Recorder::new()),
        &mut session,
        camera.as_mut(),
        &sweep_request(control.clone(), five_across_focus()),
    )
    .expect("a control nothing was recorded against must be sweepable again");
    assert_eq!(outcome.taken(), 5);
    lifecycle::commit(
        temp.store(),
        &lock,
        &mut session,
        started(),
        |draft, now| session::select_by_metric(draft, &control, MetricName::Sharpness, now),
    )
    .expect("a swept control selects");
    assert!(
        session.is_settled(),
        "the session never settled, so its (camera, task) slot stays open forever"
    );
}

// ---------------------------------------------------------------- the refinement pass

#[test]
fn a_refinement_pass_cannot_overwrite_the_frames_the_coarse_pass_scored() {
    // D8's `precision` exists so a coarse pass can be followed by a fine one, and
    // `session::begin_sweep` is legal from `Calibrated` for exactly that reason. The fine
    // pass refines *around* the coarse winner, so the two plans overlap — and a photo named
    // only by its requested value is unique within one sweep and not across two. The first
    // pass's `Sample` would keep its metrics and point at a file the second pass rewrote:
    // the module header's "the frame that is scored is the frame that is stored", true
    // within a sweep and false across a control's history.
    //
    // On this deterministic fake the two frames are byte-identical at equal control values,
    // so the collision is invisible in the bytes. What separates the two implementations is
    // the *paths*: this asserts that no two samples of one control ever name one file, and
    // that both files are on disk.
    let temp = TempStore::new().expect("a temp dir");
    let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
    let backend = backend();
    let mut camera = open(&backend);
    let control = slug(FOCUS);
    let mut session = start_session(temp.store(), &lock, camera.as_mut(), &control);
    let clock = FrozenClock;

    let coarse = calibrate::run(
        &context(temp.store(), &lock, &clock, &Recorder::new()),
        &mut session,
        camera.as_mut(),
        &sweep_request(control.clone(), five_across_focus()),
    )
    .expect("a willing camera");
    lifecycle::commit(
        temp.store(),
        &lock,
        &mut session,
        started(),
        |draft, now| session::select_by_metric(draft, &control, MetricName::Sharpness, now),
    )
    .expect("a swept control selects");

    // The pass that D8 says must be possible: finer values around the coarse winner, two of
    // which the coarse pass already visited.
    let fine = SweepSpec::Explicit {
        values: vec![256, FOCUS_OPTIMUM, 640, 768],
    };
    let refined = calibrate::run(
        &context(temp.store(), &lock, &clock, &Recorder::new()),
        &mut session,
        camera.as_mut(),
        &sweep_request(control.clone(), fine.clone()),
    )
    .expect("a refinement pass is legal");
    assert!(
        matches!(&fine, SweepSpec::Explicit { values }
            if coarse.samples.iter().any(|s| values.contains(&s.requested))),
        "the two passes do not overlap, so this arm proves nothing"
    );

    // The document is what a second process reads, and it is where the two passes meet.
    let dir = temp.store().session_dir(&session);
    let stored = temp.store().load_session(&dir).expect("readable");
    let entry = &stored.controls[&control];
    assert_eq!(
        entry.samples.len(),
        coarse.samples.len() + refined.samples.len(),
        "a refinement pass replaced the coarse pass's samples instead of adding to them"
    );

    let mut seen: BTreeMap<Utf8PathBuf, usize> = BTreeMap::new();
    for sample in &entry.samples {
        *seen.entry(sample.photo.clone()).or_default() += 1;
    }
    let shared: Vec<&Utf8PathBuf> = seen
        .iter()
        .filter(|(_, count)| **count > 1)
        .map(|(path, _)| path)
        .collect();
    assert!(
        shared.is_empty(),
        "two samples of {control} name one file, so one of them describes a frame nobody \
         can retrieve: {shared:?}"
    );
    assert_eq!(
        seen.len(),
        entry.samples.len(),
        "the sample count and the photo count disagree"
    );
    for sample in &entry.samples {
        let on_disk = dir.join(&sample.photo);
        let bytes = std::fs::read(on_disk.as_std_path()).unwrap_or_else(|error| {
            panic!("{on_disk} is in the document and not on disk: {error}")
        });
        assert_eq!(
            &bytes[..2],
            &[0xff, 0xd8],
            "{on_disk} is not the JPEG the sample claims"
        );
    }
}

// ---------------------------------------------------------------- relocation

#[test]
fn a_session_directory_relocates_as_a_unit_with_every_sample_photo_intact() {
    // D9's promise, and the reason sample paths are relative. Moved with the plainest
    // possible copy — no path rewriting — and every photo the document names still has to
    // be there, byte for byte.
    let temp = TempStore::new().expect("a temp dir");
    let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
    let backend = backend();
    let mut camera = open(&backend);
    let control = slug("brightness");
    let mut session = start_session(temp.store(), &lock, camera.as_mut(), &control);

    let clock = FrozenClock;
    calibrate::run(
        &context(temp.store(), &lock, &clock, &engine::progress::Silent),
        &mut session,
        camera.as_mut(),
        &sweep_request(
            control.clone(),
            SweepSpec::Explicit {
                values: vec![0, 128, 255],
            },
        ),
    )
    .expect("a willing camera");

    let from = temp.store().session_dir(&session);
    let elsewhere = TempStore::new().expect("a second temp dir");
    let to = elsewhere.root().join("moved-session");
    copy_tree(&from, &to);

    let moved: Session = serde_json::from_slice(
        &std::fs::read(to.join(schema::limits::SESSION_FILE).as_std_path())
            .expect("the copied document"),
    )
    .expect("it is still this build's document");
    assert_eq!(moved.controls[&control].samples.len(), 3);
    for sample in &moved.controls[&control].samples {
        let here = to.join(&sample.photo);
        let there = from.join(&sample.photo);
        assert_eq!(
            std::fs::read(here.as_std_path()).unwrap_or_else(|e| panic!("{here}: {e}")),
            std::fs::read(there.as_std_path()).unwrap_or_else(|e| panic!("{there}: {e}")),
            "{} did not survive the move",
            sample.photo
        );
    }
}

/// Copy a directory tree, verbatim. Deliberately dumb: a copy that rewrote anything would
/// be doing the work the test is checking nobody has to do.
fn copy_tree(from: &Utf8Path, to: &Utf8PathBuf) {
    std::fs::create_dir_all(to.as_std_path()).expect("a writable scratch dir");
    for entry in std::fs::read_dir(from.as_std_path()).expect("a readable session dir") {
        let entry = entry.expect("a readable entry");
        let name = entry.file_name();
        let name = name.to_str().expect("this tree's names are UTF-8");
        let source = from.join(name);
        let target = to.join(name);
        if entry.file_type().expect("a stat-able entry").is_dir() {
            copy_tree(&source, &target);
        } else {
            std::fs::copy(source.as_std_path(), target.as_std_path()).expect("a writable copy");
        }
    }
}
