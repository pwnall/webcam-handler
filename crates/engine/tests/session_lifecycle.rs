//! The session lifecycle from outside the engine (design D8, D9, docs/7 P3b).
//!
//! Everything here is reached the way `wch` will reach it — through the crate's public
//! surface, with the store's lock taken per operation — because two of the three
//! properties under test are only visible from there:
//!
//! - **The lock is a lock in both orderings.** A lifecycle operation that meets a held
//!   lock refuses and names the holder; a peer that arrives while a lifecycle operation
//!   holds it is refused and takes it as soon as the operation ends. Both arms use the
//!   real `flock` over the real state directory — two opens of one file are two
//!   contenders even inside one process, so there is no second process to synchronise
//!   with and therefore no sleep (the crash suite is where a second process earns its
//!   keep).
//! - **A refusal survives the trip through the shell.** The state machine's
//!   `IllegalTransition` carries a `from` and an `op`; a shell that reshaped either would
//!   leave a caller matching on prose, and a shell that swallowed the refusal would leave
//!   a session claiming work it never did.
//! - **The D8 gates hold on a session that came off disk.** A session that has been
//!   through `session.json` and back is the only kind a second process ever sees, and it
//!   is the one that has to refuse `apply` when nothing is calibrated.
//!
//! Every refusal here has its twin: the same operation, on a session the transition is
//! legal for, has to succeed. A suite that only asserts the sad path cannot tell "it
//! refuses when nothing swept" from "it refuses".

use engine::lifecycle::{self, SessionSpec};
use engine::session;
use engine::store::{LockProtocol, TempStore};
use schema::camera::CameraFingerprint;
use schema::control::{ControlSlug, ControlValue};
use schema::metrics::MetricName;
use schema::session::{Sample, Selector, SweepSpec};
use schema::time::Stamp;
use schema::{Error, ErrorKind};
use std::collections::BTreeMap;
use uuid::Uuid;

fn fingerprint() -> CameraFingerprint {
    CameraFingerprint {
        bus_path: "3-1:1.0".to_owned(),
        usb_id: None,
        card: "OBSBOT Tiny 3".to_owned(),
        driver: "uvcvideo".to_owned(),
        serial: None,
    }
}

fn spec(millis: u64, task: &str) -> SessionSpec {
    SessionSpec {
        id: Uuid::new_v7(uuid::Timestamp::from_unix(
            uuid::NoContext,
            millis / 1_000,
            u32::try_from((millis % 1_000) * 1_000_000).expect("sub-second nanos fit"),
        )),
        fingerprint: fingerprint(),
        task: task.to_owned(),
        goal: "legible text".to_owned(),
        criteria: Vec::new(),
        tool_version: "0.1.0".to_owned(),
    }
}

fn slug(name: &str) -> ControlSlug {
    ControlSlug::parse(name).expect("literal slug")
}

fn now() -> Stamp {
    Stamp::from_millis(1_000).expect("in range")
}

fn sample(applied: i64, score: f64) -> Sample {
    Sample {
        requested: applied,
        applied,
        warnings: Vec::new(),
        photo: format!("photos/brightness/{applied}.jpg").into(),
        metrics: BTreeMap::from([(MetricName::Sharpness, score)]),
        captured_at: Stamp::epoch(),
    }
}

// ---------------------------------------------------------------------- the lock

#[test]
fn a_lifecycle_operation_refuses_a_lock_somebody_else_holds_and_names_who() {
    // Holder first, taker second. D9 is explicit that a `wch` meeting a held lock reports
    // it rather than corrupting or blocking, and the report has to be actionable: an
    // unnamed holder tells an operator to go looking through `ps`.
    let temp = TempStore::new().expect("a temp dir");
    let daemon = temp
        .peer()
        .lock(LockProtocol::HeldForLifetime)
        .expect("nobody holds it yet");

    let err = temp
        .store()
        .with_lock(|lock| lifecycle::create(temp.store(), lock, &spec(1_000, "focus"), now()))
        .expect_err("the daemon holds the state directory");
    let Error::StoreLocked { holder, protocol } = &err else {
        panic!("a held lock is a StoreLocked refusal, not {err}");
    };
    let holder = holder.as_ref().expect("the holder wrote its record");
    assert_eq!(
        holder.pid,
        i32::try_from(std::process::id()).expect("a pid fits"),
        "the refusal named somebody else's pid"
    );
    // The holder took it the way a daemon does, so the refusal carries the answer D9 gives
    // a `wch` that meets one: this lock will not be free in a moment, and `wchc` is the
    // program that can reach the state through the process holding it.
    assert_eq!(*protocol, Some(LockProtocol::HeldForLifetime));
    assert!(
        err.to_string()
            .contains("daemon owns the state (and likely the camera) — use wchc"),
        "{err}"
    );

    // Nothing was created on the way to the refusal.
    assert!(
        temp.store()
            .sessions_for(&fingerprint())
            .expect("listable")
            .is_empty(),
        "a refused operation left a session directory behind"
    );

    // The inverse: with the lock released, the same call goes through.
    drop(daemon);
    let session = temp
        .store()
        .with_lock(|lock| lifecycle::create(temp.store(), lock, &spec(1_000, "focus"), now()))
        .expect("the lock is free");
    assert_eq!(
        temp.store()
            .sessions_for(&fingerprint())
            .expect("listable")
            .len(),
        1,
        "the session that was created is the one on disk"
    );
    assert_eq!(session.task_slug, "focus");
}

#[test]
fn a_peer_is_refused_while_a_lifecycle_operation_holds_the_lock_and_takes_it_afterwards() {
    // Taker first, holder second: the ordering that proves the lifecycle *takes* the lock
    // rather than merely accepting one. An operation that quietly ran without it would
    // leave this arm green only because nothing was ever contended.
    let temp = TempStore::new().expect("a temp dir");

    let session = temp
        .store()
        .with_lock(|lock| {
            let session = lifecycle::create(temp.store(), lock, &spec(1_000, "focus"), now())?;
            let err = temp
                .peer()
                .lock(LockProtocol::PerOperation)
                .expect_err("the lifecycle operation is holding it");
            assert_eq!(err.kind(), ErrorKind::StoreLocked, "{err}");
            Ok(session)
        })
        .expect("the operation itself succeeded");

    // And the moment it ends, the lock is free — including for the protocol the daemon
    // uses, which is the one that would be locked out for a process lifetime.
    let peer = temp
        .peer()
        .lock(LockProtocol::HeldForLifetime)
        .expect("released with the operation");
    assert_eq!(peer.protocol(), LockProtocol::HeldForLifetime);
    drop(peer);

    assert_eq!(
        lifecycle::resume(temp.store(), &fingerprint(), "focus")
            .expect("readable")
            .map(|found| found.id),
        Some(session.id),
        "the session written under the lock is the one that resumes"
    );
}

// ---------------------------------------------------------------------- D8's refusals

#[test]
fn selecting_before_any_sweep_is_refused_through_the_engine_and_a_swept_control_is_not() {
    let temp = TempStore::new().expect("a temp dir");
    let control = slug("brightness");
    let mut session = temp
        .store()
        .with_lock(|lock| lifecycle::create(temp.store(), lock, &spec(1_000, "focus"), now()))
        .expect("free");

    // Sad: nothing has swept, and the refusal names the state it refused from and the
    // operation it refused — both intact after the trip through the shell.
    let err = temp
        .store()
        .with_lock(|lock| {
            lifecycle::commit(temp.store(), lock, &mut session, now(), |draft, at| {
                session::select_value(draft, &control, 10, Selector::Agent, at)
            })
        })
        .expect_err("no samples exist");
    assert_eq!(
        err,
        Error::IllegalTransition {
            from: "untouched".to_owned(),
            op: "select brightness".to_owned(),
        }
    );

    // The document is untouched, which is the difference between a refusal and a
    // half-applied transition.
    let stored = lifecycle::resume(temp.store(), &fingerprint(), "focus")
        .expect("readable")
        .expect("still open");
    assert_eq!(stored, session);
    assert!(
        stored.controls.is_empty(),
        "a refusal wrote a control entry"
    );

    // The inverse: sweep the same control, record a sample, and the same selection lands.
    temp.store()
        .with_lock(|lock| {
            lifecycle::commit(temp.store(), lock, &mut session, now(), |draft, at| {
                session::begin_sweep(draft, &control, &SweepSpec::All, 1, at)
            })?;
            lifecycle::commit(temp.store(), lock, &mut session, now(), |draft, at| {
                session::record_sample(draft, &control, sample(10, 4.0), at)
            })?;
            lifecycle::commit(temp.store(), lock, &mut session, now(), |draft, at| {
                session::select_value(draft, &control, 10, Selector::Agent, at)
            })
        })
        .expect("a sampled value can be chosen");
    assert_eq!(session.calibrated(), vec![(&control, 10)]);
}

#[test]
fn a_session_off_disk_with_nothing_calibrated_does_not_apply_without_partial() {
    // The D8 gate `calibrate apply` has to pass, asked of a session that has been through
    // `session.json` and back — which is the only kind a second process ever holds. The
    // camera-side replay is P3d's verb; the refusal is here because it is the state
    // machine's, and a shell that lost it would apply nothing and report success.
    let temp = TempStore::new().expect("a temp dir");
    let control = slug("brightness");
    let mut session = temp
        .store()
        .with_lock(|lock| lifecycle::create(temp.store(), lock, &spec(1_000, "focus"), now()))
        .expect("free");

    let resumed = lifecycle::resume(temp.store(), &fingerprint(), "focus")
        .expect("readable")
        .expect("still open");
    assert_eq!(
        session::apply_targets(&resumed, false).expect_err("nothing has been chosen"),
        Error::IllegalTransition {
            from: "no_calibrated_controls".to_owned(),
            op: "apply".to_owned(),
        }
    );
    assert_eq!(
        session::apply_targets(&resumed, true).expect("--partial says the caller meant it"),
        Vec::new(),
        "partial is the only path around an uncalibrated session"
    );

    // The inverse: calibrate one control through the shell, resume again, and the same
    // call answers with the write it would make.
    temp.store()
        .with_lock(|lock| {
            lifecycle::commit_state(temp.store(), lock, &mut session, now(), |draft, at| {
                session::enqueue(draft, &control, at);
                Ok(())
            })?;
            lifecycle::commit(temp.store(), lock, &mut session, now(), |draft, at| {
                session::begin_sweep(draft, &control, &SweepSpec::All, 2, at)
            })?;
            for (value, score) in [(10_i64, 4.0_f64), (20, 9.0)] {
                lifecycle::commit(temp.store(), lock, &mut session, now(), |draft, at| {
                    session::record_sample(draft, &control, sample(value, score), at)
                })?;
            }
            lifecycle::commit(temp.store(), lock, &mut session, now(), |draft, at| {
                session::select_by_metric(draft, &control, MetricName::Sharpness, at)
            })
        })
        .expect("a swept control selects");

    let settled = temp
        .store()
        .load_session(&temp.store().session_dir(&session))
        .expect("readable");
    assert_eq!(
        session::apply_targets(&settled, false).expect("every queued control is terminal"),
        vec![(control, ControlValue::Int(20))]
    );
    assert!(
        !lifecycle::is_open(&settled),
        "a session whose queue is all terminal is finished work, not work in progress"
    );
}
