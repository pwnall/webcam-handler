//! The session lifecycle: create, resume, the persisted pre-sweep snapshot, and recovery
//! (design D8, D9, D4, §6).
//!
//! [`crate::session`] is the pure D8 state machine and [`crate::store`] is D9's one home
//! for state writes. This module is the seam between them, and it exists so that the two
//! laws neither module can state alone are stated exactly once:
//!
//! - **A refusal writes nothing.** Every transition runs against a *draft* of the session;
//!   the document on disk and the caller's own value are replaced only after the pure core
//!   has said yes and the store has said yes. So an [`Error::IllegalTransition`] arrives
//!   with its `from` and its `op` intact and leaves `session.json` byte-for-byte as it was
//!   — and no path here turns a refusal into a silent no-op, because every one of them
//!   returns it.
//! - **The snapshot is on disk before the camera moves.** [`sweep_write`] is the one door
//!   from a sweep to a control write, and it persists [`Session::pre_snapshot`] first. The
//!   ordering is the whole mitigation design §6 records: a sweep that dies mid-flight has
//!   left a camera mis-set, and the only thing that can put it back is a record written
//!   *before* the first write. A snapshot persisted afterwards documents a camera nobody
//!   can restore.
//!
//! ## What "the same session" means
//!
//! D8 says a session belongs to a (camera fingerprint, task) pair; D9's layout puts many
//! UUIDs under one task slug. Both are true, and the reconciliation is that a task slug
//! holds a *history*: at most one of those sessions is **open** at a time, and the rest
//! are finished work kept for `calibrate list` to show.
//!
//! [`is_open`] is the definition, and it is deliberately generous at the start and strict
//! at the end: a session with an empty queue is open (it was created a moment ago and has
//! nothing in it yet), and a session with a queue is open until every queued control has
//! reached one of D8's terminal states. So `calibrate start` twice against one camera and
//! task is [`Error::SessionConflict`] naming the session already there — the second run is
//! a `resume`, not a new directory — while starting a *new* session for a task whose last
//! one is settled is ordinary and allowed.
//!
//! [`resume`] looks at the newest session in the slot and no further. A document it cannot
//! read is a refusal rather than a reason to hand back an older one: quietly resuming the
//! session *before* the one the operator meant is how a tool loses somebody's work. The
//! same refusal reaches [`create`], which is conservative in the direction that matters —
//! this build cannot tell whether a session it cannot read is still open, and two live
//! sessions sweeping one camera is the defect [`Error::SessionConflict`] exists to prevent.
//!
//! ## What is not here
//!
//! Executing a sweep — set, settle, capture, score, record — is P3c's, and it will call
//! [`sweep_write`] for its writes. Replaying a session's calibrated values against a
//! camera is P3d's `calibrate apply`; the D8 gate that verb has to pass through
//! ([`crate::session::apply_targets`]) is pure and already lives with the state machine.

use schema::backend::Camera;
use schema::camera::CameraFingerprint;
use schema::control::{ControlSlug, ControlValue};
use schema::pairing::AutomationPair;
use schema::report::WriteReport;
use schema::session::{LogEntry, Session, SessionEvent, new_session};
use schema::snapshot::RestoreReport;
use schema::time::Stamp;
use schema::{Error, Result};
use uuid::Uuid;

use crate::discover::Discovery;
use crate::store::{SessionRef, SessionStore, StoreLock};
use crate::{discover, pairing, snapshot, write};

// ------------------------------------------------------------------ create and resume

/// Everything a new session needs that the tool cannot derive.
///
/// A struct rather than eight arguments: `id` and `now` come from the caller because the
/// engine reads no clock (a UUIDv7 *is* a timestamp), and the rest is the operator's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSpec {
    /// The session's identity. UUIDv7, so the directory listing sorts chronologically.
    pub id: Uuid,
    /// The camera it is recorded against.
    pub fingerprint: CameraFingerprint,
    /// The task, in the operator's words.
    pub task: String,
    /// What the session is trying to achieve.
    pub goal: String,
    /// Ordered criteria for judging a sample (D8: recorded because the *selector* needs
    /// them).
    pub criteria: Vec<String>,
    /// The tool version writing the document.
    pub tool_version: String,
}

/// Start a session, writing it through the store and opening its log.
///
/// # Errors
///
/// [`Error::SessionConflict`] when the (camera, task) slot already holds an open session —
/// its id is in the detail, because "resume that one" is only actionable if the caller is
/// told which one. Whatever [`resume`] refuses with when the newest session in the slot
/// cannot be read, and whatever the store refuses with.
pub fn create(
    store: &SessionStore,
    lock: &StoreLock,
    spec: &SessionSpec,
    now: Stamp,
) -> Result<Session> {
    if let Some(open) = resume(store, &spec.fingerprint, &spec.task)? {
        return Err(Error::SessionConflict {
            detail: format!(
                "session {} is still open for this camera and task ({}); resume it, or \
                 finish it before starting another",
                open.id, open.task
            ),
        });
    }

    let mut session = new_session(
        spec.id,
        spec.fingerprint.clone(),
        &spec.task,
        &spec.goal,
        &spec.tool_version,
        now,
    );
    session.criteria = spec.criteria.clone();

    let dir = store.save_session(lock, &session)?;
    store.append_log(
        lock,
        &dir,
        &LogEntry {
            at: now,
            event: SessionEvent::Started {
                goal: session.goal.clone(),
            },
        },
    )?;
    Ok(session)
}

/// The open session for a (camera, task) pair, if there is one.
///
/// The newest session in the slot and no further — see this module's header for why
/// looking past it would be worse than answering `None`.
///
/// # Errors
///
/// [`Error::SchemaVersionForeign`] when the newest session was written by another build,
/// and [`Error::StorageIo`] when it cannot be read: this build cannot say whether a
/// document it cannot read is open, and guessing is how two sessions end up sweeping one
/// camera.
pub fn resume(
    store: &SessionStore,
    fingerprint: &CameraFingerprint,
    task: &str,
) -> Result<Option<Session>> {
    let Some(newest) = newest_in_slot(store, fingerprint, task)? else {
        return Ok(None);
    };
    let session = store.load_session(&newest.dir)?;
    Ok(is_open(&session).then_some(session))
}

/// Whether a session still has work in it (this module's header defines the term).
#[must_use]
pub fn is_open(session: &Session) -> bool {
    // The empty queue is *open*, not settled. `Session::is_settled` answers `true` for it
    // — vacuously, every one of no controls is terminal — and taking that as "finished"
    // would make `calibrate start` succeed twice in a row and leave two empty session
    // directories where one session was meant.
    session.queue.is_empty() || !session.is_settled()
}

/// The newest session recorded for this (camera, task), whatever state it is in.
///
/// No document is parsed: the store lists newest-first by UUIDv7 byte order, which is
/// what that version was chosen for.
fn newest_in_slot(
    store: &SessionStore,
    fingerprint: &CameraFingerprint,
    task: &str,
) -> Result<Option<SessionRef>> {
    // The *derived* slug on both sides. The store names its directories through the one
    // transform, so asking with the operator's free text — "Read text from the DUT
    // display" — has to go through the same transform or it never matches.
    let slug = schema::session::task_slug(task);
    Ok(store
        .sessions_for(fingerprint)?
        .into_iter()
        .find(|found| found.task_slug == slug))
}

// ------------------------------------------------------------------ transitions

/// Run one D8 transition and persist what it produced.
///
/// The one door from the pure state machine to the store, so "a refusal writes nothing"
/// is a property of one function instead of a habit of eight. `now` is handed to the
/// transition rather than captured by the caller, so the stamp on the document and the
/// stamp on the log entry cannot disagree.
///
/// # Errors
///
/// Whatever the transition refuses with — [`Error::IllegalTransition`], `from` and `op`
/// unchanged — in which case neither `session` nor its document has moved. Whatever the
/// store refuses with: a failure writing the document leaves both where they were, and a
/// failure appending the log line leaves the document (and `session`) advanced, because
/// the state landing is what the next call has to agree with.
pub fn commit(
    store: &SessionStore,
    lock: &StoreLock,
    session: &mut Session,
    now: Stamp,
    transition: impl FnOnce(&mut Session, Stamp) -> Result<SessionEvent>,
) -> Result<SessionEvent> {
    let mut draft = session.clone();
    let event = transition(&mut draft, now)?;
    persist(store, lock, session, draft, now, Some(&event))?;
    Ok(event)
}

/// The same, for the D8 transitions that report no event.
///
/// Queueing, reordering, deferring and blocking change the document and touch no camera,
/// and [`SessionEvent`] has no variant for them — `log.ndjson` is the record of what
/// happened *to the device*, and inventing an event for a queue edit would put a line in
/// it that no camera ever saw.
///
/// # Errors
///
/// As [`commit`].
pub fn commit_state(
    store: &SessionStore,
    lock: &StoreLock,
    session: &mut Session,
    now: Stamp,
    transition: impl FnOnce(&mut Session, Stamp) -> Result<()>,
) -> Result<()> {
    let mut draft = session.clone();
    transition(&mut draft, now)?;
    persist(store, lock, session, draft, now, None)
}

/// Publish a draft: the document first, then the caller's value, then the log line.
///
/// The order is chosen for what a crash between the steps leaves behind. Document then
/// log: a crash there loses one line of `log.ndjson`, which is the torn-tail case D9
/// already drops on load. Log then document would be worse — an event recorded for a
/// state change that never landed, which is a session history that lies.
///
/// `*session` is replaced *before* the append, so a caller whose log write failed is
/// holding the state the disk holds rather than the state before it.
fn persist(
    store: &SessionStore,
    lock: &StoreLock,
    session: &mut Session,
    draft: Session,
    now: Stamp,
    event: Option<&SessionEvent>,
) -> Result<()> {
    let dir = store.save_session(lock, &draft)?;
    *session = draft;
    if let Some(event) = event {
        store.append_log(
            lock,
            &dir,
            &LogEntry {
                at: now,
                event: event.clone(),
            },
        )?;
    }
    Ok(())
}

// ------------------------------------------------------------------ pair discovery

/// Ask the camera which of its controls it pairs, and record the answer on the session
/// (design D3 layer 2, PF:3).
///
/// **Run at session start, before anything sweeps.** The probe is a sequence of *writes*
/// — it toggles each automation-shaped control and diffs the INACTIVE flags across the
/// set — so it has to happen while the camera is still where the operator left it, and it
/// snapshots and restores itself for exactly that reason. Running it later would mean
/// toggling automation in the middle of a calibration, and the pairs it measured would
/// describe a device three sweeps into somebody's session.
///
/// What lands on the document is the *merge*: the declared UVC table (D3 layer 1),
/// narrowed to the controls this device actually has, with everything the probe measured
/// laid over it. Measured beats declared (E1), and the merge is stored rather than its
/// ingredients because the caller that most needs it is the one that cannot re-run the
/// probe — a recovery after a crash, putting the camera back in D4's automation-first
/// order, on a process that never saw this camera before.
///
/// The [`Discovery`] is returned rather than reduced to its pairs: what the probe declined
/// to touch and whether it put the camera back are facts about the *run*, and a caller
/// that hides them is reporting "this camera has no pairs" when the truth is "this probe
/// did not look at half of them".
///
/// # Errors
///
/// Whatever the camera says when asked for its controls, or when the probe's own
/// pre-flight snapshot cannot be taken — a probe that cannot record where the camera
/// started does not start. Whatever the store refuses with; in that case the probe has
/// already run and put the camera back, and the pairs are simply not on the document yet.
pub fn discover_pairs(
    store: &SessionStore,
    lock: &StoreLock,
    session: &mut Session,
    camera: &mut dyn Camera,
    now: Stamp,
) -> Result<Discovery> {
    let found = discover::pairs(camera, now)?;
    let controls = camera.controls()?;
    let merged = pairing::applicable(
        &controls,
        &pairing::merge(schema::pairing::declared_pairs(), found.pairs.clone()),
    );

    let measured = found.pairs.len();
    let skipped = found.skipped.len();
    let mut draft = session.clone();
    draft.pairs = merged;
    draft.updated_at = now;
    persist(
        store,
        lock,
        session,
        draft,
        now,
        Some(&SessionEvent::PairsDiscovered { measured, skipped }),
    )?;
    Ok(found)
}

// ------------------------------------------------------------------ the pre-sweep snapshot

/// What arming the pre-sweep snapshot did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum PreSnapshot {
    /// Read from the camera and persisted by this call.
    Taken {
        /// How many controls it holds.
        controls: usize,
    },
    /// Already on disk from an earlier call; the camera was not read again.
    AlreadyPersisted {
        /// How many controls it holds.
        controls: usize,
    },
}

impl PreSnapshot {
    /// How many controls the persisted snapshot holds, however it got there.
    #[must_use]
    pub const fn controls(self) -> usize {
        match self {
            PreSnapshot::Taken { controls } | PreSnapshot::AlreadyPersisted { controls } => {
                controls
            }
        }
    }
}

/// Persist the camera's state as found, once per session (design §6, gate G3).
///
/// **Once per session, not once per sweep.** The record has to describe the camera as the
/// *operator* left it, and by the second sweep the first one's writes are on the device:
/// a snapshot taken then would restore the camera to the middle of a calibration nobody
/// asked for. So a session that already carries one keeps it, and this call does not read
/// the camera at all.
///
/// Callers do not usually reach for this: [`sweep_write`] calls it, which is how the
/// ordering stops being something a caller can get wrong.
///
/// # Errors
///
/// Whatever the camera says when asked for its controls, and whatever the store refuses
/// with. A failure here means **nothing has been written to the camera**, which is the
/// point of doing it first.
pub fn arm_pre_snapshot(
    store: &SessionStore,
    lock: &StoreLock,
    session: &mut Session,
    camera: &mut dyn Camera,
    pairs: &[AutomationPair],
    now: Stamp,
) -> Result<PreSnapshot> {
    if let Some(already) = &session.pre_snapshot {
        return Ok(PreSnapshot::AlreadyPersisted {
            controls: already.entries.len(),
        });
    }

    let taken = snapshot::take(camera, pairs, now)?;
    let controls = taken.entries.len();
    let mut draft = session.clone();
    draft.pre_snapshot = Some(taken);
    draft.updated_at = now;
    persist(
        store,
        lock,
        session,
        draft,
        now,
        Some(&SessionEvent::SnapshotTaken { controls }),
    )?;
    Ok(PreSnapshot::Taken { controls })
}

/// Write controls as part of a sweep — the one door, and the reason the ordering holds.
///
/// Arms the pre-sweep snapshot, then performs the write **guarded** (D3 applies inside
/// sweeps too \[PF:6\], and the automation partner has to be switched off before a manual
/// value can be driven at all). The two steps are one function because separating them
/// would put "persist first" back into a caller's hands, and a caller that got it wrong
/// would leave no trace until the day a sweep crashed.
///
/// # Errors
///
/// The store's, when the snapshot cannot be persisted — and in that case the camera has
/// not been touched. The planner's ([`Error::ControlUnknown`], [`Error::ControlReadOnly`],
/// [`Error::ControlInactive`]) or the device's, from the first write that failed.
pub fn sweep_write(
    store: &SessionStore,
    lock: &StoreLock,
    session: &mut Session,
    camera: &mut dyn Camera,
    pairs: &[AutomationPair],
    targets: &[(ControlSlug, ControlValue)],
    now: Stamp,
) -> Result<WriteReport> {
    // Bound rather than discarded so that "the snapshot is on disk" is a step with a name
    // in the one function that has to do it first. Which of the two answers it gave —
    // taken now, or already there — is not a sweep's business; that it happened is.
    let _armed: PreSnapshot = arm_pre_snapshot(store, lock, session, camera, pairs, now)?;
    write::set(camera, pairs, targets, true)
}

// ------------------------------------------------------------------ recovery

/// What putting the persisted snapshot back achieved.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub enum Recovery {
    /// The session carries no unconsumed snapshot: either it never wrote to the camera,
    /// or an earlier restore already put it back.
    NothingPersisted,
    /// The camera is back where the snapshot found it, and the session no longer carries
    /// it.
    Restored {
        /// Per-control outcomes, in D4's attempted order.
        report: RestoreReport,
    },
    /// Some of it could not go back. **The snapshot is kept**, so the next attempt still
    /// has it.
    Incomplete {
        /// Per-control outcomes, including the ones that name what could not be undone.
        report: RestoreReport,
    },
}

impl Recovery {
    /// The report, when a restore was attempted.
    #[must_use]
    pub const fn report(&self) -> Option<&RestoreReport> {
        match self {
            Recovery::NothingPersisted => None,
            Recovery::Restored { report } | Recovery::Incomplete { report } => Some(report),
        }
    }
}

/// Put the persisted pre-sweep snapshot back, and consume it (design D4, §6).
///
/// This is *both* halves of "leave the camera as you found it": the end of an ordinary
/// session runs it, and so does the next process after a crash. The only difference is
/// which process is holding the session, and that is exactly why the snapshot lives in
/// the document rather than in memory.
///
/// **What counts as recovered is N9's vocabulary, unchanged.**
/// [`schema::snapshot::RestoreOutcome::OwnedByAutomation`] — the control is INACTIVE
/// because its automation owner is back in charge, exactly as at snapshot time — is
/// *complete*: on any device whose INACTIVE flag follows its automation control \[PF:3\]
/// it is the ordinary result of every guarded write's restore. `StillInactive` is the
/// other case, a control that was ours at snapshot time and is automation-owned now, and
/// it is **not** complete. Reading those two the other way round would leave every
/// recovered session carrying a snapshot forever, and every ordinary session announcing a
/// failure it did not have.
///
/// **Consuming is the last step, and it is one atomic write.** The camera goes back
/// first; only then does the document stop carrying the snapshot. A crash in between
/// leaves the snapshot on disk and the camera already restored, and the next attempt puts
/// back what is already there — every outcome `AlreadyCorrect`, and the record consumed.
/// A crash *during* the consumption cannot leave a half-consumed session, because
/// `write_json_atomic` publishes by rename: the document either carries the snapshot or
/// does not.
///
/// # Errors
///
/// [`Error::FingerprintMismatch`] when the snapshot belongs to another camera, naming the
/// fields that differ, and whatever the camera says when asked for its controls. Whatever
/// the store refuses with when the consumption cannot be written — in which case the
/// camera *is* back and the snapshot is still on disk, which is the safe direction: the
/// next attempt is a no-op restore and a retried consumption.
pub fn recover(
    store: &SessionStore,
    lock: &StoreLock,
    session: &mut Session,
    camera: &mut dyn Camera,
    pairs: &[AutomationPair],
    now: Stamp,
) -> Result<Recovery> {
    let Some(persisted) = session.pre_snapshot.clone() else {
        return Ok(Recovery::NothingPersisted);
    };

    let report = snapshot::restore(camera, pairs, &persisted)?;
    if !report.is_complete() {
        // Consuming here would record a recovery that did not happen — a session that
        // thinks it put the camera back while a control it moved is still where the sweep
        // left it.
        return Ok(Recovery::Incomplete { report });
    }

    let unrestored = report.unrestored().len();
    let restored = report.outcomes.len().saturating_sub(unrestored);
    let mut draft = session.clone();
    draft.pre_snapshot = None;
    draft.updated_at = now;
    persist(
        store,
        lock,
        session,
        draft,
        now,
        Some(&SessionEvent::Restored {
            restored,
            unrestored,
        }),
    )?;
    Ok(Recovery::Restored { report })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use camino::Utf8PathBuf;
    use schema::ErrorKind;
    use schema::backend::CameraBackend;
    use schema::control::ControlValue;
    use schema::metrics::MetricName;
    use schema::pairing::{AutomationOff, Provenance};
    use schema::session::{Sample, Selector, SweepSpec};
    use schema::snapshot::{RestoreOutcome, UnrestorableReason};
    use schema::{limits, session as schema_session};

    use super::*;
    use crate::double::{OnWrite, ScriptedCamera, boolean, flagged, integer};
    use crate::session;
    use crate::store::{LockProtocol, StoreFault, TempStore};

    const INACTIVE: u32 = 0x0010;
    const VOLATILE: u32 = 0x0080;

    fn slug(name: &str) -> ControlSlug {
        ControlSlug::parse(name).expect("literal slug")
    }

    fn fingerprint() -> CameraFingerprint {
        CameraFingerprint {
            bus_path: "3-4:1.0".to_owned(),
            usb_id: None,
            card: "Scripted".to_owned(),
            driver: "scripted".to_owned(),
            serial: None,
        }
    }

    /// The identity `ScriptedCamera` hands out, so a snapshot taken from one restores to
    /// it — a fingerprint typed by hand here would only ever prove that two literals match.
    fn scripted_fingerprint(camera: &ScriptedCamera) -> CameraFingerprint {
        camera.info().fingerprint.clone()
    }

    fn spec(id: u64, task: &str) -> SessionSpec {
        SessionSpec {
            id: uuid_v7(id),
            fingerprint: fingerprint(),
            task: task.to_owned(),
            goal: "legible text".to_owned(),
            criteria: vec!["the DUT's serial number is readable".to_owned()],
            tool_version: "0.1.0".to_owned(),
        }
    }

    fn uuid_v7(millis: u64) -> Uuid {
        Uuid::new_v7(uuid::Timestamp::from_unix(
            uuid::NoContext,
            millis / 1_000,
            u32::try_from((millis % 1_000) * 1_000_000).expect("sub-second nanos fit"),
        ))
    }

    fn later() -> Stamp {
        Stamp::from_millis(1_000).expect("in range")
    }

    fn white_balance_pair() -> AutomationPair {
        AutomationPair {
            manual: slug("white_balance_temperature"),
            automation: slug("white_balance_automatic"),
            off: AutomationOff::Value { value: 0 },
            provenance: Provenance::Declared,
        }
    }

    /// Automation on, its partner INACTIVE because of it, and a control that has nothing
    /// to do with either — the shape note N9 is about.
    fn coupled_camera() -> ScriptedCamera {
        ScriptedCamera::new(vec![
            boolean("white_balance_automatic", 1),
            flagged(integer("white_balance_temperature", 46), INACTIVE),
            integer("brightness", 50),
        ])
        .on_write(
            "white_balance_automatic",
            OnWrite::Couple(vec!["white_balance_temperature"]),
        )
    }

    fn sample(applied: i64, score: f64) -> Sample {
        Sample {
            requested: applied,
            applied,
            warnings: Vec::new(),
            photo: Utf8PathBuf::from(format!("photos/brightness/{applied}.jpg")),
            metrics: BTreeMap::from([(MetricName::Sharpness, score)]),
            captured_at: Stamp::epoch(),
        }
    }

    /// The bytes of a session's document, for "and nothing was written" assertions.
    fn document(store: &SessionStore, session: &Session) -> Vec<u8> {
        std::fs::read(
            store
                .session_dir(session)
                .join(limits::SESSION_FILE)
                .as_std_path(),
        )
        .expect("the session was written")
    }

    // ---------------------------------------------------------------- create and resume

    #[test]
    fn a_created_session_lands_on_disk_with_its_opening_event() {
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let spec = spec(1_000, "Read text from the DUT display");

        let session = create(temp.store(), &lock, &spec, later()).expect("a free slot");
        assert_eq!(session.schema_version, limits::SESSION_SCHEMA_VERSION);
        assert_eq!(session.criteria, spec.criteria);
        assert_eq!(session.task_slug, "read-text-from-the-dut-display");
        assert_eq!(session.pre_snapshot, None, "nothing has moved yet");

        let dir = temp.store().session_dir(&session);
        assert_eq!(
            temp.store().load_session(&dir).expect("readable"),
            session,
            "the document on disk is the session that was returned"
        );
        assert_eq!(
            temp.store().load_log(&dir).expect("readable"),
            vec![LogEntry {
                at: later(),
                event: SessionEvent::Started {
                    goal: "legible text".to_owned(),
                },
            }],
            "a session that opened no log is a session with no history"
        );
    }

    #[test]
    fn a_second_session_for_one_camera_and_task_conflicts_until_the_first_one_settles() {
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let mut first = create(temp.store(), &lock, &spec(1_000, "focus"), later()).expect("free");

        // Sad: the slot is taken, and the refusal names what is in it.
        let err = create(temp.store(), &lock, &spec(2_000, "focus"), later())
            .expect_err("the first session is still open");
        assert_eq!(err.kind(), ErrorKind::SessionConflict);
        assert!(
            err.to_string().contains(&first.id.to_string()),
            "a conflict nobody can act on: {err}"
        );

        // A *different* task on the same camera is a different session, not a conflict.
        create(temp.store(), &lock, &spec(3_000, "exposure"), later())
            .expect("another task is another slot");
        // And so is the same task on a different camera.
        let mut elsewhere = spec(4_000, "focus");
        elsewhere.fingerprint.bus_path = "1-6:1.0".to_owned();
        create(temp.store(), &lock, &elsewhere, later()).expect("another camera is another slot");

        // Take the first session to a settled state: queued, swept, chosen.
        let control = slug("brightness");
        commit_state(temp.store(), &lock, &mut first, later(), |s, now| {
            session::enqueue(s, &control, now);
            Ok(())
        })
        .expect("queueing is legal");
        commit(temp.store(), &lock, &mut first, later(), |s, now| {
            session::begin_sweep(s, &control, &SweepSpec::All, 1, now)
        })
        .expect("a fresh control sweeps");
        commit(temp.store(), &lock, &mut first, later(), |s, now| {
            session::record_sample(s, &control, sample(30, 9.0), now)
        })
        .expect("a running sweep accepts samples");
        commit(temp.store(), &lock, &mut first, later(), |s, now| {
            session::select_by_metric(s, &control, MetricName::Sharpness, now)
        })
        .expect("a swept control selects");

        assert!(!is_open(&first), "every queued control is terminal");
        assert_eq!(
            resume(temp.store(), &fingerprint(), "focus").expect("readable"),
            None,
            "a settled session is history, not work in progress"
        );
        let successor = create(temp.store(), &lock, &spec(5_000, "focus"), later())
            .expect("a settled slot takes a new session");

        // And the slot now holds two: the finished one and the new one. Which of them
        // answers is the whole of "newest first" — resuming the *older* session would hand
        // an operator somebody's finished work and let a third `start` succeed beside it.
        assert_eq!(
            temp.store()
                .sessions_for(&fingerprint())
                .expect("listable")
                .iter()
                .filter(|found| found.task_slug == "focus")
                .count(),
            2,
            "a task slug holds a history"
        );
        assert_eq!(
            resume(temp.store(), &fingerprint(), "focus")
                .expect("readable")
                .map(|found| found.id),
            Some(successor.id),
            "resume looked past the newest session in the slot"
        );
        assert_eq!(
            create(temp.store(), &lock, &spec(6_000, "focus"), later())
                .expect_err("the successor is open")
                .kind(),
            ErrorKind::SessionConflict
        );
    }

    #[test]
    fn a_session_resumes_by_fingerprint_and_task_with_everything_it_was_carrying() {
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let mut camera = coupled_camera();
        let mut spec = spec(1_000, "Read text from the DUT display");
        spec.fingerprint = scripted_fingerprint(&camera);

        let mut session = create(temp.store(), &lock, &spec, later()).expect("free");
        let control = slug("brightness");
        commit_state(temp.store(), &lock, &mut session, later(), |s, now| {
            session::enqueue(s, &control, now);
            Ok(())
        })
        .expect("legal");
        let armed = arm_pre_snapshot(
            temp.store(),
            &lock,
            &mut session,
            &mut camera,
            &[white_balance_pair()],
            later(),
        )
        .expect("a readable camera and a writable store");
        assert_eq!(armed, PreSnapshot::Taken { controls: 3 });

        let resumed = resume(temp.store(), &spec.fingerprint, &spec.task)
            .expect("readable")
            .expect("still open");
        assert_eq!(resumed, session, "resume is the document, not a summary");
        assert!(
            resumed.pre_snapshot.is_some(),
            "the snapshot has to survive the round trip, or a crash has nothing to restore"
        );

        // Both directions on the lookup itself: neither a task nobody started nor a camera
        // nobody used resumes anything.
        assert_eq!(
            resume(temp.store(), &spec.fingerprint, "a task nobody started").expect("readable"),
            None
        );
        let mut elsewhere = spec.fingerprint.clone();
        elsewhere.card = "Integrated Camera".to_owned();
        elsewhere.bus_path = "1-6:1.0".to_owned();
        assert_eq!(
            resume(temp.store(), &elsewhere, &spec.task).expect("readable"),
            None
        );
    }

    #[test]
    fn a_session_written_by_another_build_refuses_rather_than_resuming_the_one_before_it() {
        // Conservative in the direction that matters: this build cannot tell whether a
        // document it cannot read is still open, and answering `None` would let a second
        // session start against a camera the first one may be sweeping.
        let mut temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        create(temp.store(), &lock, &spec(1_000, "focus"), later()).expect("free");

        let arranged = temp
            .store_mut()
            .arrange(StoreFault::ForeignSchemaVersion)
            .expect("scriptable");
        // A real document, one version ahead, written where the newest session is.
        let ahead = create(temp.store(), &lock, &spec(2_000, "exposure"), later())
            .expect("writing it is not what refuses");
        assert_eq!(
            resume(temp.store(), &fingerprint(), "exposure")
                .expect_err("this build cannot read it")
                .kind(),
            ErrorKind::SchemaVersionForeign
        );
        assert_eq!(
            create(temp.store(), &lock, &spec(3_000, "exposure"), later())
                .expect_err("and it will not start a second session past it")
                .kind(),
            ErrorKind::SchemaVersionForeign
        );
        drop(arranged);
        temp.store_mut().clear_faults();

        // The healthy twin: the same document at this build's version resumes.
        temp.store()
            .save_session(&lock, &ahead)
            .expect("a writable store");
        assert_eq!(
            resume(temp.store(), &fingerprint(), "exposure")
                .expect("readable now")
                .map(|s| s.id),
            Some(ahead.id)
        );
    }

    // ---------------------------------------------------------------- transitions

    #[test]
    fn a_refused_transition_keeps_its_from_and_op_and_writes_nothing() {
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let mut session =
            create(temp.store(), &lock, &spec(1_000, "focus"), later()).expect("free");
        let control = slug("focus_absolute");

        let before = document(temp.store(), &session);
        let before_log = temp
            .store()
            .load_log(&temp.store().session_dir(&session))
            .expect("readable");
        let unchanged = session.clone();

        let err = commit(temp.store(), &lock, &mut session, later(), |s, now| {
            session::select_value(s, &control, 10, Selector::Human, now)
        })
        .expect_err("nothing has swept");
        assert_eq!(
            err,
            Error::IllegalTransition {
                from: "untouched".to_owned(),
                op: "select focus_absolute".to_owned(),
            },
            "the shell must not reshape the refusal it carries"
        );
        assert_eq!(
            document(temp.store(), &session),
            before,
            "a refused transition rewrote the document"
        );
        assert_eq!(
            temp.store()
                .load_log(&temp.store().session_dir(&session))
                .expect("readable"),
            before_log,
            "a refused transition logged an event"
        );
        assert_eq!(
            session, unchanged,
            "a refused transition moved the caller's own session"
        );

        // The inverse: the same operation on a well-formed session goes through, and the
        // document and the log both show it.
        commit(temp.store(), &lock, &mut session, later(), |s, now| {
            session::begin_sweep(s, &control, &SweepSpec::All, 1, now)
        })
        .expect("a fresh control sweeps");
        commit(temp.store(), &lock, &mut session, later(), |s, now| {
            session::record_sample(s, &control, sample(10, 1.0), now)
        })
        .expect("a running sweep accepts samples");
        let event = commit(temp.store(), &lock, &mut session, later(), |s, now| {
            session::select_value(s, &control, 10, Selector::Human, now)
        })
        .expect("a sampled value can be chosen");
        assert_eq!(
            event,
            SessionEvent::Selected {
                control: control.clone(),
                value: 10,
                selector: Selector::Human,
            }
        );
        let dir = temp.store().session_dir(&session);
        assert_eq!(
            temp.store().load_session(&dir).expect("readable"),
            session,
            "the document and the caller's session agree after a commit"
        );
        assert!(
            temp.store()
                .load_log(&dir)
                .expect("readable")
                .contains(&LogEntry { at: later(), event }),
            "the event reached the log"
        );
    }

    #[test]
    fn a_store_that_refuses_the_write_leaves_the_session_where_it_was() {
        // The other half of "a refusal writes nothing": the transition was legal and the
        // *store* said no. The caller's session must not advance past a document that was
        // never written, or the next commit persists a state whose predecessor is missing.
        let mut temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let mut session =
            create(temp.store(), &lock, &spec(1_000, "focus"), later()).expect("free");
        let control = slug("focus_absolute");
        let before = session.clone();

        let arranged = temp
            .store_mut()
            .arrange(StoreFault::DiskFull)
            .expect("scriptable");
        let err = commit(temp.store(), &lock, &mut session, later(), |s, now| {
            session::begin_sweep(s, &control, &SweepSpec::All, 1, now)
        })
        .expect_err("no space");
        assert_eq!(err.kind(), ErrorKind::StorageIo);
        assert_eq!(
            session, before,
            "the caller's session ran ahead of the disk"
        );
        drop(arranged);
        temp.store_mut().clear_faults();

        commit(temp.store(), &lock, &mut session, later(), |s, now| {
            session::begin_sweep(s, &control, &SweepSpec::All, 1, now)
        })
        .expect("a healthy store takes the same transition");
    }

    #[test]
    fn the_document_goes_down_before_the_line_that_describes_it() {
        // `persist`'s ordering, made observable by a log append that cannot succeed while
        // the document write can: `log.ndjson` is a *directory*, so `O_APPEND` on it is
        // `EISDIR` for every user including root — a real refusal from the real kernel,
        // not a scripted one, and one that does not depend on who is running the suite.
        //
        // The order matters for what a crash between the two steps leaves: a lost log line
        // is the torn tail D9 already drops, and a logged event whose state never landed is
        // a session history that lies. An implementation that appended first fails here,
        // because the document would still hold the state before the transition.
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let mut session =
            create(temp.store(), &lock, &spec(1_000, "focus"), later()).expect("free");
        let control = slug("brightness");
        let dir = temp.store().session_dir(&session);
        let log = dir.join(limits::SESSION_LOG_FILE);
        std::fs::remove_file(log.as_std_path()).expect("`create` opened the log");
        std::fs::create_dir(log.as_std_path()).expect("the session directory is ours");

        let err = commit(temp.store(), &lock, &mut session, later(), |s, now| {
            session::begin_sweep(s, &control, &SweepSpec::All, 1, now)
        })
        .expect_err("the log cannot be appended to");
        assert_eq!(err.kind(), ErrorKind::StorageIo);

        let stored = temp.store().load_session(&dir).expect("readable");
        assert_eq!(
            stored
                .controls
                .get(&control)
                .map(|entry| entry.status.name()),
            Some("sweeping"),
            "the state change was not on disk when its log line was attempted"
        );
        assert_eq!(
            stored, session,
            "the caller's session must track the disk once the document has landed"
        );
    }

    // ---------------------------------------------------------------- pair discovery

    #[test]
    fn discovery_records_the_merged_pair_set_on_the_session_and_it_survives_a_resume() {
        // D3's two layers, resolved once and written down. The session carries the
        // *answer* because the caller that most needs it — a recovery on a process that
        // never saw this camera — cannot re-run a probe that writes to the device.
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let backend = fake::FakeBackend::from_profile(testkit::fixtures::synthetic_basic())
            .expect("this build's version");
        let info = backend
            .enumerate()
            .expect("the fake enumerates what it replays")
            .into_iter()
            .next()
            .expect("one profile is one camera");
        let mut camera = backend.open(&info.id).expect("nothing holds a fake");
        let mut spec = spec(1_000, "focus");
        spec.fingerprint = camera.info().fingerprint.clone();
        let mut session = create(temp.store(), &lock, &spec, later()).expect("free");
        assert!(session.pairs.is_empty(), "nothing has been probed yet");

        let found = discover_pairs(temp.store(), &lock, &mut session, camera.as_mut(), later())
            .expect("a readable camera and a writable store");
        assert!(
            found.left_the_camera_alone(),
            "the probe did not put the camera back: {:?}",
            found.restored
        );

        // A pair the probe demonstrated: toggling `white_balance_automatic` moved
        // `white_balance_temperature`'s INACTIVE bit [PF:3], so what lands on the session
        // says `Measured` — and measured beats declared (E1).
        let balance = session
            .pairs
            .iter()
            .find(|pair| pair.manual.as_str() == "white_balance_temperature")
            .unwrap_or_else(|| panic!("no white balance pair in {:?}", session.pairs));
        assert_eq!(balance.automation.as_str(), "white_balance_automatic");
        assert_eq!(balance.provenance, Provenance::Measured);

        // And a pair the probe declined to demonstrate is still *there*, still marked for
        // what it is. `focus_automatic_continuous` moves a motor and §5 says nothing here
        // does that, so the probe names it in `skipped` and the declared table supplies the
        // relationship — a session that dropped it would refuse to sweep focus at all.
        let focus = session
            .pairs
            .iter()
            .find(|pair| pair.manual.as_str() == "focus_absolute")
            .unwrap_or_else(|| panic!("no focus pair in {:?}", session.pairs));
        assert_eq!(focus.automation.as_str(), "focus_automatic_continuous");
        assert_eq!(
            focus.provenance,
            Provenance::Declared,
            "a pair nobody measured must not claim it was measured"
        );
        assert!(
            found
                .skipped
                .iter()
                .any(
                    |(control, reason)| control.as_str() == "focus_automatic_continuous"
                        && !reason.is_empty()
                ),
            "the probe passed over a candidate without saying so: {:?}",
            found.skipped
        );

        // Every recorded pair names controls this device actually has: the declared table
        // lists both spellings of the white-balance automation control, and a session
        // carrying a pair for a control that is not there would plan writes nobody can make.
        let present: Vec<ControlSlug> = camera
            .controls()
            .expect("readable")
            .into_iter()
            .map(|desc| desc.slug)
            .collect();
        for pair in &session.pairs {
            assert!(
                present.contains(&pair.manual) && present.contains(&pair.automation),
                "{pair:?} names a control this camera does not have"
            );
        }

        // It is on the document and in the history, not only in memory.
        let dir = temp.store().session_dir(&session);
        assert_eq!(
            temp.store().load_session(&dir).expect("readable").pairs,
            session.pairs,
            "the pairs never reached the document a second process reads"
        );
        assert!(
            temp.store()
                .load_log(&dir)
                .expect("readable")
                .iter()
                .any(|entry| matches!(entry.event, SessionEvent::PairsDiscovered { .. })),
            "the probe wrote to the camera and left no line saying so"
        );
        assert_eq!(
            resume(temp.store(), &spec.fingerprint, &spec.task)
                .expect("readable")
                .map(|found| found.pairs),
            Some(session.pairs.clone())
        );
    }

    #[test]
    fn a_camera_with_nothing_to_pair_records_an_empty_set_rather_than_the_table() {
        // The inverse, and it is the one that matters for the merge: the declared table is
        // a *nomination* about UVC devices in general, and narrowing it to this device is
        // what keeps a session from planning a switch-off for a control that is not there.
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let mut camera = ScriptedCamera::new(vec![integer("brightness", 50)]);
        let mut spec = spec(1_000, "brightness");
        spec.fingerprint = scripted_fingerprint(&camera);
        let mut session = create(temp.store(), &lock, &spec, later()).expect("free");

        let found = discover_pairs(temp.store(), &lock, &mut session, &mut camera, later())
            .expect("a readable camera");
        assert!(found.pairs.is_empty());
        assert!(
            session.pairs.is_empty(),
            "a camera with one manual control was given pairs: {:?}",
            session.pairs
        );
        assert!(
            temp.store()
                .load_log(&temp.store().session_dir(&session))
                .expect("readable")
                .iter()
                .any(|entry| entry.event
                    == SessionEvent::PairsDiscovered {
                        measured: 0,
                        skipped: 0
                    }),
            "finding nothing is a result, and it is recorded like one"
        );
    }

    // ---------------------------------------------------------------- the pre-sweep snapshot

    #[test]
    fn the_pre_sweep_snapshot_reaches_the_disk_before_the_first_write_reaches_the_camera() {
        // The ordering design §6 rests on, proven by taking the disk away: with the store
        // refusing, a `sweep_write` must leave the camera *untouched*. An implementation
        // that wrote first and persisted afterwards passes every other test in this file
        // and fails this one, because the camera would carry a write nothing can undo.
        let mut temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let mut camera = coupled_camera();
        let mut spec = spec(1_000, "focus");
        spec.fingerprint = scripted_fingerprint(&camera);
        let mut session = create(temp.store(), &lock, &spec, later()).expect("free");

        let arranged = temp
            .store_mut()
            .arrange(StoreFault::DiskFull)
            .expect("scriptable");
        let err = sweep_write(
            temp.store(),
            &lock,
            &mut session,
            &mut camera,
            &[white_balance_pair()],
            &[(slug("brightness"), ControlValue::Int(0))],
            later(),
        )
        .expect_err("the snapshot cannot be persisted");
        assert_eq!(err.kind(), ErrorKind::StorageIo);
        assert!(
            camera.writes.is_empty(),
            "the camera moved before the snapshot was on disk: {:?}",
            camera.writes
        );
        assert_eq!(session.pre_snapshot, None);
        drop(arranged);
        temp.store_mut().clear_faults();

        // The healthy twin: the snapshot is on disk, and *then* the write lands.
        let report = sweep_write(
            temp.store(),
            &lock,
            &mut session,
            &mut camera,
            &[white_balance_pair()],
            &[(slug("brightness"), ControlValue::Int(0))],
            later(),
        )
        .expect("a writable store and a willing camera");
        assert_eq!(report.writes.len(), 1);
        assert!(!camera.writes.is_empty());

        let persisted = temp
            .store()
            .load_session(&temp.store().session_dir(&session))
            .expect("readable")
            .pre_snapshot
            .expect("the document carries the snapshot a crash would need");
        assert_eq!(
            persisted.entries.len(),
            3,
            "every writable control belongs in it: {persisted:?}"
        );
        assert!(
            persisted
                .entries
                .iter()
                .any(|entry| entry.control.as_str() == "brightness"
                    && entry.value == ControlValue::Int(50)),
            "the snapshot records the value the sweep is about to overwrite: {persisted:?}"
        );
    }

    #[test]
    fn the_pre_sweep_snapshot_is_taken_once_for_the_session_not_once_per_sweep() {
        // By the second sweep the first one's writes are on the camera. A snapshot taken
        // then records a calibration in progress and restores the operator's camera to the
        // middle of it.
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let mut camera = coupled_camera();
        let mut spec = spec(1_000, "focus");
        spec.fingerprint = scripted_fingerprint(&camera);
        let mut session = create(temp.store(), &lock, &spec, later()).expect("free");

        let first = arm_pre_snapshot(
            temp.store(),
            &lock,
            &mut session,
            &mut camera,
            &[white_balance_pair()],
            later(),
        )
        .expect("readable");
        assert!(matches!(first, PreSnapshot::Taken { .. }), "{first:?}");
        let recorded = session.pre_snapshot.clone().expect("taken");

        // The sweep moves the camera, and a second sweep arms again.
        write::set(
            &mut camera,
            &[white_balance_pair()],
            &[(slug("brightness"), ControlValue::Int(7))],
            true,
        )
        .expect("a willing camera");
        let second = arm_pre_snapshot(
            temp.store(),
            &lock,
            &mut session,
            &mut camera,
            &[white_balance_pair()],
            later(),
        )
        .expect("readable");
        assert_eq!(
            second,
            PreSnapshot::AlreadyPersisted {
                controls: recorded.entries.len()
            },
            "the second sweep re-read the camera"
        );
        assert_eq!(
            session.pre_snapshot.as_ref(),
            Some(&recorded),
            "the record of the camera as found was overwritten mid-calibration"
        );
        assert_eq!(
            temp.store()
                .load_log(&temp.store().session_dir(&session))
                .expect("readable")
                .iter()
                .filter(|entry| matches!(entry.event, SessionEvent::SnapshotTaken { .. }))
                .count(),
            1,
            "one snapshot, one event"
        );
    }

    // ---------------------------------------------------------------- recovery

    #[test]
    fn recovery_puts_the_camera_back_and_the_session_stops_carrying_the_snapshot() {
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let mut camera = ScriptedCamera::new(vec![integer("brightness", 50)]);
        let mut spec = spec(1_000, "focus");
        spec.fingerprint = scripted_fingerprint(&camera);
        let mut session = create(temp.store(), &lock, &spec, later()).expect("free");

        sweep_write(
            temp.store(),
            &lock,
            &mut session,
            &mut camera,
            &[],
            &[(slug("brightness"), ControlValue::Int(3))],
            later(),
        )
        .expect("a willing camera");
        assert_eq!(camera.value_of("brightness"), Some(ControlValue::Int(3)));

        let recovery = recover(temp.store(), &lock, &mut session, &mut camera, &[], later())
            .expect("the snapshot belongs to this camera");
        let Recovery::Restored { report } = &recovery else {
            panic!("a camera that took every write back is recovered: {recovery:?}");
        };
        assert!(report.is_complete(), "{report:?}");
        assert_eq!(camera.value_of("brightness"), Some(ControlValue::Int(50)));

        let dir = temp.store().session_dir(&session);
        assert_eq!(session.pre_snapshot, None, "the snapshot was not consumed");
        assert_eq!(
            temp.store()
                .load_session(&dir)
                .expect("readable")
                .pre_snapshot,
            None,
            "the document still carries a snapshot that has already been put back"
        );
        assert!(
            temp.store()
                .load_log(&dir)
                .expect("readable")
                .iter()
                .any(|entry| entry.event
                    == SessionEvent::Restored {
                        restored: 1,
                        unrestored: 0
                    }),
            "the restore left no record"
        );

        // And a session with nothing left to put back says so rather than restoring twice.
        assert_eq!(
            recover(temp.store(), &lock, &mut session, &mut camera, &[], later())
                .expect("nothing to do"),
            Recovery::NothingPersisted
        );
    }

    #[test]
    fn a_control_whose_automation_owner_is_back_counts_as_recovered() {
        // Note N9, at the level that consumes it: the snapshot was taken with automation
        // *on*, so restoring the automation control re-engages it and the partner is
        // INACTIVE again — the camera is exactly where it was. Reading that as incomplete
        // would leave every recovered session carrying its snapshot forever, and every
        // ordinary sweep announcing a failure it did not have.
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let mut camera = coupled_camera();
        let mut spec = spec(1_000, "white balance");
        spec.fingerprint = scripted_fingerprint(&camera);
        let mut session = create(temp.store(), &lock, &spec, later()).expect("free");

        // A guarded write of the manual control: the planner switches the automation off
        // first, which is what frees the partner and what the restore has to undo.
        sweep_write(
            temp.store(),
            &lock,
            &mut session,
            &mut camera,
            &[white_balance_pair()],
            &[(slug("white_balance_temperature"), ControlValue::Int(90))],
            later(),
        )
        .expect("a willing camera");
        assert_eq!(
            camera.value_of("white_balance_automatic"),
            Some(ControlValue::Int(0)),
            "the guarded write did not disable the automation"
        );

        let recovery = recover(
            temp.store(),
            &lock,
            &mut session,
            &mut camera,
            &[white_balance_pair()],
            later(),
        )
        .expect("the snapshot belongs to this camera");
        let Recovery::Restored { report } = &recovery else {
            panic!("an owned control is a complete restore, not an incomplete one: {recovery:?}");
        };
        assert!(
            report.outcomes.iter().any(|outcome| matches!(
                outcome,
                RestoreOutcome::OwnedByAutomation { control, .. }
                    if control.as_str() == "white_balance_temperature"
            )),
            "the deferred control was not reported as owned: {report:?}"
        );
        assert_eq!(
            session.pre_snapshot, None,
            "an owned control blocked the consumption"
        );
        assert!(
            temp.store()
                .load_log(&temp.store().session_dir(&session))
                .expect("readable")
                .iter()
                .any(|entry| entry.event
                    == SessionEvent::Restored {
                        restored: 3,
                        unrestored: 0
                    }),
            "the owned control was counted as a failure in the log"
        );
        assert!(
            camera.is_inactive("white_balance_temperature"),
            "and the coupling is back where it was"
        );
    }

    #[test]
    fn a_restore_that_could_not_finish_keeps_the_snapshot_for_the_next_attempt() {
        // The other side of N9: a control this tool cannot put back leaves the camera
        // somewhere the snapshot does not describe, and consuming the record there would
        // be a session claiming a recovery it did not perform.
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let mut camera = ScriptedCamera::new(vec![
            integer("brightness", 50),
            flagged(integer("exposure", 30), VOLATILE),
        ]);
        let mut spec = spec(1_000, "focus");
        spec.fingerprint = scripted_fingerprint(&camera);
        let mut session = create(temp.store(), &lock, &spec, later()).expect("free");

        sweep_write(
            temp.store(),
            &lock,
            &mut session,
            &mut camera,
            &[],
            &[(slug("brightness"), ControlValue::Int(3))],
            later(),
        )
        .expect("a willing camera");

        let recovery = recover(temp.store(), &lock, &mut session, &mut camera, &[], later())
            .expect("attempted");
        let Recovery::Incomplete { report } = &recovery else {
            panic!("a volatile control cannot be put back: {recovery:?}");
        };
        assert!(report.outcomes.iter().any(|outcome| matches!(
            outcome,
            RestoreOutcome::Unrestorable {
                reason: UnrestorableReason::Volatile,
                ..
            }
        )));
        assert!(
            session.pre_snapshot.is_some(),
            "the snapshot was consumed by a restore that did not finish"
        );
        assert!(
            temp.store()
                .load_session(&temp.store().session_dir(&session))
                .expect("readable")
                .pre_snapshot
                .is_some(),
            "and the document forgot it too"
        );
        assert_eq!(
            camera.value_of("brightness"),
            Some(ControlValue::Int(50)),
            "what *could* be put back still was"
        );
    }

    #[test]
    fn a_consumption_the_store_refuses_leaves_the_snapshot_to_be_retried() {
        // The crash-during-recovery window, as far as a hermetic test can open it: the
        // camera is back and the record of it is not yet gone. The safe direction, and the
        // reason the camera goes first — the retry is a no-op restore and a second attempt
        // at the one write that failed.
        let mut temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let mut camera = ScriptedCamera::new(vec![integer("brightness", 50)]);
        let mut spec = spec(1_000, "focus");
        spec.fingerprint = scripted_fingerprint(&camera);
        let mut session = create(temp.store(), &lock, &spec, later()).expect("free");
        sweep_write(
            temp.store(),
            &lock,
            &mut session,
            &mut camera,
            &[],
            &[(slug("brightness"), ControlValue::Int(3))],
            later(),
        )
        .expect("a willing camera");

        let arranged = temp
            .store_mut()
            .arrange(StoreFault::DiskFull)
            .expect("scriptable");
        let err = recover(temp.store(), &lock, &mut session, &mut camera, &[], later())
            .expect_err("the consumption cannot be written");
        assert_eq!(err.kind(), ErrorKind::StorageIo);
        assert_eq!(
            camera.value_of("brightness"),
            Some(ControlValue::Int(50)),
            "the camera goes back first, and it did"
        );
        assert!(
            session.pre_snapshot.is_some(),
            "a session that failed to record its recovery must still carry the snapshot"
        );
        drop(arranged);
        temp.store_mut().clear_faults();

        let retried = recover(temp.store(), &lock, &mut session, &mut camera, &[], later())
            .expect("the retry");
        let Recovery::Restored { report } = &retried else {
            panic!("putting back what is already there is a complete restore: {retried:?}");
        };
        assert!(
            report
                .outcomes
                .iter()
                .all(|outcome| matches!(outcome, RestoreOutcome::AlreadyCorrect { .. })),
            "the retry wrote to a camera that was already back: {report:?}"
        );
        assert_eq!(session.pre_snapshot, None);
    }

    #[test]
    fn a_snapshot_from_another_camera_refuses_and_keeps_the_record() {
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let mut camera = ScriptedCamera::new(vec![integer("brightness", 50)]);
        let mut spec = spec(1_000, "focus");
        spec.fingerprint = scripted_fingerprint(&camera);
        let mut session = create(temp.store(), &lock, &spec, later()).expect("free");
        let armed = arm_pre_snapshot(temp.store(), &lock, &mut session, &mut camera, &[], later())
            .expect("readable");
        assert_eq!(armed.controls(), 1);

        let mut other =
            ScriptedCamera::new(vec![integer("brightness", 10)]).identified("cam:b", "1-6:1.0");
        let err = recover(temp.store(), &lock, &mut session, &mut other, &[], later())
            .expect_err("a snapshot is not portable between cameras");
        assert_eq!(err.kind(), ErrorKind::FingerprintMismatch);
        assert!(other.writes.is_empty(), "the refusal wrote to the camera");
        assert!(
            session.pre_snapshot.is_some(),
            "a refused recovery consumed the record it never used"
        );
    }

    #[test]
    fn the_task_slug_a_session_resumes_by_is_the_one_the_store_named_its_directory_with() {
        // One transform, asked the same question twice. A resume that slugged the
        // operator's free text differently from the store would answer `None` for every
        // task whose name has a capital letter in it.
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let task = "Read text from the DUT display";
        let session = create(temp.store(), &lock, &spec(1_000, task), later()).expect("free");

        let dir = temp.store().session_dir(&session);
        assert!(
            dir.as_str()
                .contains(&format!("/{}/", schema_session::task_slug(task))),
            "{dir} is not under the slug the transform derives"
        );
        assert!(
            resume(
                temp.store(),
                &fingerprint(),
                "read  TEXT from the DUT display"
            )
            .expect("readable")
            .is_some(),
            "free text that slugs to the same directory has to find the same session"
        );
        assert!(
            resume(temp.store(), &fingerprint(), "read text from the DUT")
                .expect("readable")
                .is_none(),
            "and text that slugs to a different directory must not"
        );
    }
}
