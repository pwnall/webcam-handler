//! The eight `calibrate_*` verbs over the wire (design D8, D9; docs/7 P4c).
//!
//! The other half of "the mutating half over RPC": `mutating_verbs.rs` has the verbs that
//! write to a *camera* or to a process, and these are the ones that write to a *document*.
//! Two laws meet in every one of them, and neither is the camera's — D8's state machine
//! decides which transitions a session may make, and D9 decides who may write the session
//! tree at all.
//!
//! ## What a session suite has to prove that a camera suite does not
//!
//! - **The engine, and only the engine.** Every verb here reaches the same `engine::lifecycle`
//!   function `webcam-handler-cli calibrate …` reaches, including the four that moved there
//!   when this daemon acquired the verbs: `session_to_update` (the token is *proof*, and the
//!   read is half of a read-modify-write), `select` (the `Selection` match records a selector
//!   whichever branch runs), `restore` (which pair set, and "no snapshot is not a failure"),
//!   and the two-way branch `calibrate_plan` keeps.
//! - **The lock is one lock, and it is not the daemonless one.** A handler that reached for
//!   `SessionStore::with_lock` would not hang: it would answer this client `StoreLocked`
//!   naming the daemon's own pid and advising it to use `webcam-handler-client` against the
//!   daemon it is already talking to. `tests/lock.rs` pins the symptom from the outside; here
//!   it is the negative — no calibrate verb ever answers that variant.
//! - **`flock` does not exclude a daemon from itself** (note **N47**). `webcam-handler-cli` is
//!   safe because D9's daemonless protocol takes the lock per operation, so two
//!   `webcam-handler-cli` processes serialize; a daemon holds one for its lifetime and gets
//!   nothing between its own request tasks, and two of these verbs (`calibrate_plan --order`
//!   and `calibrate_select`) open no camera, so not even the per-camera actor separates them.
//!   The serialization is `daemon::server`'s `Inner::sessions`, and it has a test.
//! - **A session belongs to (camera fingerprint, task)** and every mutating verb refuses on
//!   a mismatch *naming the fields that differ* (rubric B5, S:E6). A check implemented in
//!   one verb is a check the other verbs walk around, so the walk here is over all of them.
//! - **No verb answers `StoreLocked`** — see the walk's own comment. It used to be two
//!   absences: `Unimplemented` was the other, and note **N43** turned on these six being
//!   the last rows in the map. P4d deleted that variant (note **N6**), so the second claim
//!   stopped being writable here and became structural — the registry has no such member
//!   to answer with. The suite is one claim lighter and says so rather than letting a
//!   sentence stand for an assertion that is gone.
//!
//! ## Progress, and where it is now asserted
//!
//! `calibrate_sweep` emits `schema::progress::ProgressEvent`s, and at P4c they went into
//! `engine::progress::Silent` and were dropped — the T5 method's own documented answer, which
//! puts the live events on their own subscription rather than threading a watcher through a
//! call. **P4e-i attached that subscription** (`daemon::events`, `wch_subscribe_calibration`),
//! so the events these tests provoke now reach whoever is watching. Nothing in *this* binary
//! subscribes, deliberately: a sweep with no listener is still the ordinary case, and the
//! delivery claim belongs to `tests/subscriptions.rs`, which drives it over both transports.
//! What remains true here and is worth stating because no gate covers it:
//! `webcam-handler-client calibrate sweep` renders nothing until **P4f** builds the client
//! transport that consumes the stream.
//!
//! ## What is deliberately not asserted
//!
//! **The sweep's physics.** Which value a metric picks, and that every other sample scored
//! below it, is `crates/engine/tests/sweep.rs`'s claim over the same fake and the same
//! committed optimum. Re-asserting it here would be this suite checking the engine's
//! arithmetic through a socket; what a socket can break is the document, the refusals and
//! the concurrency, and those are what is below.

#[path = "support/fixture.rs"]
mod fixture;
#[path = "support/gated.rs"]
mod gated;
mod support;
#[path = "support/wire.rs"]
mod wire;

use std::collections::BTreeMap;
use std::sync::Arc;

use api::{WchRpcClient, rpc_code};
use jsonrpsee::core::client::Error as ClientError;
use schema::backend::{Camera, CameraBackend};
use schema::camera::CameraId;
use schema::control::{ControlDesc, ControlSlug, ControlValue};
use schema::error::{Error, ErrorKind};
use schema::metrics::MetricName;
use schema::report::WriteReport;
use schema::selector::CameraSelector;
use schema::session::{
    ControlStatus, Selection, Selector, Session, SessionRef, SweepRequest, SweepSpec,
};
use schema::snapshot::RestoreReport;
use uuid::Uuid;

use crate::fixture::{Ask, Fixture, SESSION_TASK, camera as nth_camera};
use crate::gated::{Blocking, Gate, lock};
use crate::wire::{Wire, refusal};

/// The task each test opens its own session under.
///
/// Derived from the test's name rather than shared, because D9 keys a slot on (camera
/// fingerprint, task) and a second open session in one slot is `SessionConflict` — which is
/// a thing this suite asserts on purpose in exactly one place and must not stumble into
/// everywhere else.
fn task(name: &str) -> String {
    format!("p4c calibrate over the wire: {name}")
}

/// The session the fixture wrote for the first camera, named the way a caller names one.
fn fixture_session() -> SessionRef {
    SessionRef::Task {
        task: SESSION_TASK.to_owned(),
    }
}

/// The first camera and the first wire.
fn ask(fixture: &Fixture) -> (CameraSelector, Wire, Ask) {
    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");
    let asked = fixture.ask();
    (asked.camera.clone(), wire, asked)
}

/// The control every suite writes to, plus a second one to put behind it in the queue.
///
/// The first is `Fixture::ask`'s — writable, and not a menu, because a menu's indices have
/// holes \[PF:2\] and a sweep over one is a different claim — so this suite sweeps the
/// control the other suites write to rather than picking its own. The second is derived the
/// same way and is only ever *queued*: what it is for is to leave the session with
/// uncalibrated work in it, which is what `apply --partial` and the queue's order are
/// about.
fn two_scalars(controls: &[ControlDesc], asked: &Ask) -> (ControlSlug, ControlSlug) {
    let second = controls
        .iter()
        .filter(|desc| desc.is_writable() && !desc.control_type.is_menu())
        .map(|desc| desc.slug.clone())
        .find(|slug| *slug != asked.control)
        .expect("the profile has a second writable scalar");
    (asked.control.clone(), second)
}

/// A sweep of `control` over three values inside its declared range.
///
/// Three rather than `SweepSpec::All`: a full sweep of an eight-bit control is 256 photos
/// and would prove nothing three do not, and what this suite is about is the document the
/// sweep leaves rather than the optimum it finds (see this file's header).
fn three_values(controls: &[ControlDesc], control: &ControlSlug) -> SweepRequest {
    let desc = engine::pairing::describe(controls, control).expect("a control this camera has");
    let mid = desc.range.min + (desc.range.max - desc.range.min) / 2;
    SweepRequest::new(
        control.clone(),
        SweepSpec::Explicit {
            values: vec![desc.range.min, mid, desc.range.max],
        },
    )
}

/// The identity-free shape of a session: what it is *about*, with what makes it *this* run
/// projected out.
///
/// Two runs of one sequence differ in their uuid, their task, and every timestamp on them —
/// including each sample's `captured_at` and the path its photo landed at, which carries the
/// session directory. Comparing whole `Session` values across two transports would therefore
/// be asserting that time had stopped; comparing this is asserting that the two pipes
/// carried the same document.
#[derive(Debug, PartialEq)]
struct Shape {
    queue: Vec<ControlSlug>,
    statuses: BTreeMap<ControlSlug, String>,
    calibrated: Vec<(ControlSlug, i64)>,
    samples: BTreeMap<ControlSlug, Vec<(i64, i64)>>,
    pairs: Vec<schema::pairing::AutomationPair>,
    has_pre_snapshot: bool,
}

impl Shape {
    fn of(session: &Session) -> Shape {
        Shape {
            queue: session.queue.clone(),
            statuses: session
                .controls
                .iter()
                .map(|(slug, control)| (slug.clone(), control.status.name().to_owned()))
                .collect(),
            calibrated: session
                .calibrated()
                .into_iter()
                .map(|(slug, value)| (slug.clone(), value))
                .collect(),
            samples: session
                .controls
                .iter()
                .map(|(slug, control)| {
                    (
                        slug.clone(),
                        control
                            .samples
                            .iter()
                            .map(|sample| (sample.requested, sample.applied))
                            .collect(),
                    )
                })
                .collect(),
            pairs: session.pairs.clone(),
            has_pre_snapshot: session.pre_snapshot.is_some(),
        }
    }
}

/// Open a session, queue two controls, sweep one of them, and choose a value for it.
///
/// The order a caller would use, and the one D8 requires: a control that never swept cannot
/// be selected, and a session with uncalibrated work cannot be applied without `--partial`.
/// One function because two transports run it and because §5's census will drive the same
/// call sites.
async fn calibrate_through<C: WchRpcClient + Sync>(
    client: &C,
    camera: &CameraSelector,
    task: &str,
    first: &ControlSlug,
    second: &ControlSlug,
    sweep: &SweepRequest,
) -> Session {
    let opened: Session = client
        .calibrate_start(
            camera.clone(),
            task.to_owned(),
            "a legible frame".to_owned(),
            vec!["sharp".to_owned()],
        )
        .await
        .expect("a free (camera, task) slot");
    let which = SessionRef::Id { id: opened.id };

    client
        .calibrate_plan(
            camera.clone(),
            which.clone(),
            vec![first.clone(), second.clone()],
            false,
        )
        .await
        .expect("two controls this camera has");
    // The queue is a document edit and opens no camera, which is the branch `crates/cli`
    // keeps and this daemon keeps with it.
    client
        .calibrate_plan(
            camera.clone(),
            which.clone(),
            vec![second.clone(), first.clone()],
            true,
        )
        .await
        .expect("a permutation of the queue");

    client
        .calibrate_sweep(camera.clone(), which.clone(), sweep.clone())
        .await
        .expect("a control with an ordered range");
    client
        .calibrate_select(
            camera.clone(),
            which.clone(),
            first.clone(),
            Selection::ByMetric {
                metric: MetricName::Sharpness,
            },
        )
        .await
        .expect("a control that swept")
}

// ------------------------------------------------------------------------------- the tests

#[tokio::test]
async fn a_session_driven_over_the_wire_is_the_one_the_engine_reads_back() {
    // Deliverable D of this sub-milestone for the calibrate half: the daemon's answer is
    // the engine's answer. Every assertion below compares what came over the socket against
    // `engine::lifecycle` reading the same state directory directly — not against a
    // rebuilt DTO, because a suite that assembled the document a third time would be
    // checking its own arithmetic.
    let fixture = Fixture::start();
    let (camera, wire, asked) = ask(&fixture);
    let controls = fixture.controls();
    let (first, second) = two_scalars(&controls, &asked);
    let sweep = three_values(&controls, &first);
    let task = task("engine parity");

    let answered = calibrate_through(&wire, &camera, &task, &first, &second, &sweep).await;
    let which = SessionRef::Id { id: answered.id };
    let fingerprint = &nth_camera(&fixture.cameras, 0).fingerprint;

    // `calibrate_select`'s answer *is* the document on disk, byte for byte through serde.
    let on_disk = engine::lifecycle::session_for(&fixture.store, fingerprint, &which)
        .expect("the daemon wrote it");
    assert_eq!(answered, on_disk);

    // And `calibrate_status` is `engine::lifecycle::status`, history included.
    let status = wire
        .calibrate_status(camera.clone(), which.clone())
        .await
        .expect("the session exists");
    assert_eq!(
        status,
        engine::lifecycle::status(&fixture.store, fingerprint, &which).expect("the store reads")
    );

    // Not vacuous, verb by verb: `start` probed the camera and recorded what it measured,
    // `plan` queued and then reordered, `sweep` took a sample per value, `select` recorded
    // a value *and* who chose it.
    assert!(!answered.pairs.is_empty(), "the probe measured nothing");
    assert_eq!(
        answered.queue,
        vec![second.clone(), first.clone()],
        "the reorder did not land"
    );
    let swept = answered
        .controls
        .get(&first)
        .expect("the swept control is on the document");
    assert_eq!(swept.samples.len(), 3);
    let ControlStatus::Calibrated {
        value, selector, ..
    } = &swept.status
    else {
        panic!("the selection did not land: {:?}", swept.status);
    };
    assert!(
        swept.samples.iter().any(|sample| sample.applied == *value),
        "the chosen value is one no sample holds"
    );
    assert_eq!(
        *selector,
        Selector::Metric {
            name: MetricName::Sharpness
        },
        "the selector a metric earned was not recorded"
    );
    // The sweep armed the record that gives the camera back (design §6, note N23).
    assert!(answered.pre_snapshot.is_some());

    // `calibrate_list` sees it, narrowed and unnarrowed, and is the engine's listing.
    for narrowed in [None, Some(camera.clone())] {
        let listed = wire
            .calibrate_list(narrowed.clone())
            .await
            .expect("listing parses nothing");
        assert_eq!(
            listed,
            engine::lifecycle::list(
                &fixture.store,
                narrowed.map(|_| fingerprint.clone()).as_ref()
            )
            .expect("the store walks")
        );
        assert!(
            listed.sessions.iter().any(|entry| entry.id == answered.id),
            "the session this test opened is not in the listing"
        );
    }
}

#[tokio::test]
async fn the_calibrate_verbs_answer_the_same_over_both_transports() {
    // One daemon, two pipes, the same sequence — but *not* the same session: opening one is
    // the first verb in the sequence and D9 keys a slot on (camera, task), so a second run
    // in one slot would be `SessionConflict` rather than a second answer. So each transport
    // gets its own task and the comparison is over `Shape`, which is the document with its
    // identity and its clock projected out (see that type for what and why).
    let fixture = Fixture::start();
    let asked = fixture.ask();
    let camera = asked.camera.clone();
    let controls = fixture.controls();
    let (first, second) = two_scalars(&controls, &asked);
    let sweep = three_values(&controls, &first);

    let [(first_name, first_wire), (second_name, second_wire)] = fixture.wires();
    let over_memory = calibrate_through(
        &first_wire,
        &camera,
        &task(first_name),
        &first,
        &second,
        &sweep,
    )
    .await;
    let over_socket = calibrate_through(
        &second_wire,
        &camera,
        &task(second_name),
        &first,
        &second,
        &sweep,
    )
    .await;

    assert_eq!(
        Shape::of(&over_memory),
        Shape::of(&over_socket),
        "{first_name} and {second_name} left different documents"
    );
    // Not vacuous: two empty documents would compare equal and say nothing.
    let shape = Shape::of(&over_memory);
    assert_eq!(shape.queue.len(), 2);
    assert_eq!(shape.calibrated.len(), 1);
    assert!(shape.has_pre_snapshot);
    assert!(!shape.pairs.is_empty());

    // And they are two sessions rather than one answered twice, which is what makes the
    // comparison above a comparison of transports.
    assert_ne!(over_memory.id, over_socket.id);
    assert_ne!(over_memory.task, over_socket.task);
}

#[tokio::test]
async fn a_second_session_in_one_slot_is_refused_and_the_first_one_is_untouched() {
    // Note N14, over the wire: D9's slot holds at most one *open* session, and the refusal
    // names the one that is already there because "resume that one" is only actionable if
    // the caller is told which one.
    let fixture = Fixture::start();
    let (camera, wire, _) = ask(&fixture);
    let task = task("one slot");

    let opened: Session = wire
        .calibrate_start(
            camera.clone(),
            task.clone(),
            "a legible frame".to_owned(),
            Vec::new(),
        )
        .await
        .expect("a free slot");

    let (code, error) = refusal(
        wire.calibrate_start(
            camera.clone(),
            task.clone(),
            "a second goal".to_owned(),
            Vec::new(),
        )
        .await,
    );
    assert_eq!(code, rpc_code(ErrorKind::SessionConflict));
    assert_eq!(error.kind(), ErrorKind::SessionConflict);
    assert!(
        error.to_string().contains(&opened.id.to_string()),
        "the refusal has to name the session that is in the way: {error}"
    );

    // The first session is exactly where it was: a refused `start` writes nothing.
    let status = wire
        .calibrate_status(camera, SessionRef::Id { id: opened.id })
        .await
        .expect("the first session is still there");
    assert_eq!(status.session, opened);
}

#[tokio::test]
async fn reordering_a_queue_opens_no_camera_and_a_non_permutation_is_refused() {
    // The branch `crates/cli`'s executor keeps and this daemon keeps with it: "reordering a
    // queue is an edit to a document, and a caller who wanted to put exposure before focus
    // should not be refused because something else currently holds the device". A daemon
    // that routed both halves of `calibrate_plan` through the camera's actor would compile,
    // would pass every test that did not count descriptors, and would break parity the day
    // somebody reordered a queue during a sweep.
    let fixture = Fixture::start();
    let (camera, wire, asked) = ask(&fixture);
    let controls = fixture.controls();
    let (first, second) = two_scalars(&controls, &asked);
    let task = task("reorder");

    let opened: Session = wire
        .calibrate_start(
            camera.clone(),
            task,
            "a legible frame".to_owned(),
            Vec::new(),
        )
        .await
        .expect("a free slot");
    let which = SessionRef::Id { id: opened.id };
    let queued: Session = wire
        .calibrate_plan(
            camera.clone(),
            which.clone(),
            vec![first.clone(), second.clone()],
            false,
        )
        .await
        .expect("two controls this camera has");
    assert_eq!(queued.queue, vec![first.clone(), second.clone()]);

    // The descriptor count is read after everything that legitimately needs the device, so
    // what the reorder does to it is the only thing left to see.
    let opens_before = fixture.backend.opens();
    let reordered: Session = wire
        .calibrate_plan(
            camera.clone(),
            which.clone(),
            vec![second.clone(), first.clone()],
            true,
        )
        .await
        .expect("a permutation of the queue");
    assert_eq!(reordered.queue, vec![second.clone(), first.clone()]);
    assert_eq!(
        fixture.backend.opens(),
        opens_before,
        "reordering a queue opened a camera"
    );

    // A list that is not a permutation is the D8 machine's refusal, and it names the
    // condition rather than shrugging.
    let (code, error) = refusal(
        wire.calibrate_plan(camera.clone(), which.clone(), vec![first.clone()], true)
            .await,
    );
    assert_eq!(code, rpc_code(ErrorKind::IllegalTransition));
    assert!(error.to_string().contains("not_a_permutation"), "{error}");

    // And a control this camera does not have is `ControlUnknown` with the planner's own
    // suggestions — the same ones `wch_get` gives for the same slug.
    let (code, error) = refusal(
        wire.calibrate_plan(camera, which, vec![asked.unknown_control.clone()], false)
            .await,
    );
    assert_eq!(code, rpc_code(ErrorKind::ControlUnknown));
    assert_eq!(
        error,
        engine::pairing::describe(&controls, &asked.unknown_control).expect_err("no such control")
    );
}

#[tokio::test]
async fn applying_needs_the_work_finished_and_restoring_puts_the_camera_back_twice_over() {
    // D8's gate on `apply` and note N23's verb after it, in the order a caller meets them —
    // and with note N20's ruling in the middle: `apply` deliberately does **not** restore
    // and does **not** consume the pre-sweep snapshot, because applying a calibration is the
    // point of having made one. A reviewer applying AGENTS rule 8 to `apply` will find it
    // "missing" a restore; it is not, and this is where that is written down as behaviour.
    let fixture = Fixture::start();
    let (camera, wire, asked) = ask(&fixture);
    let controls = fixture.controls();
    let (first, second) = two_scalars(&controls, &asked);
    let sweep = three_values(&controls, &first);
    let before = device_state(&fixture);

    let session = calibrate_through(&wire, &camera, &task("apply"), &first, &second, &sweep).await;
    let which = SessionRef::Id { id: session.id };

    // The camera really moved, which is what makes the restore below worth asserting.
    assert_ne!(device_state(&fixture), before, "the sweep wrote nothing");

    // One control is calibrated and one is still queued, so a whole-session apply is the
    // D8 machine's refusal naming what is pending.
    let (code, error) = refusal(
        wire.calibrate_apply(camera.clone(), which.clone(), false)
            .await,
    );
    assert_eq!(code, rpc_code(ErrorKind::IllegalTransition));
    assert!(error.to_string().contains("unsettled"), "{error}");

    // `--partial` is the only path around uncalibrated controls (rubric B5), and it writes
    // exactly the one control that has a value.
    let applied: WriteReport = wire
        .calibrate_apply(camera.clone(), which.clone(), true)
        .await
        .expect("partial is the way past unsettled work");
    assert_eq!(
        applied
            .writes
            .iter()
            .map(|write| write.slug.clone())
            .collect::<Vec<_>>(),
        vec![first.clone()]
    );
    // And it did not spend the snapshot: the way back is still on the document.
    let after_apply = wire
        .calibrate_status(camera.clone(), which.clone())
        .await
        .expect("the session exists");
    assert!(
        after_apply.session.pre_snapshot.is_some(),
        "apply consumed the record that gives the camera back (note N20)"
    );

    // The verb that does put it back, and the report that says what that achieved.
    let restored: RestoreReport = wire
        .calibrate_restore(camera.clone(), which.clone())
        .await
        .expect("the session carries a snapshot");
    assert!(!restored.outcomes.is_empty());
    assert!(
        restored.is_complete(),
        "the restore could not put {:?} back",
        restored.unrestored()
    );
    assert_eq!(device_state(&fixture), before, "the camera is not as found");

    // Consumed, atomically, and running it twice is **not** an error: the second time there
    // is nothing left to put back and an empty report says so.
    let after_restore = wire
        .calibrate_status(camera.clone(), which.clone())
        .await
        .expect("the session exists");
    assert!(after_restore.session.pre_snapshot.is_none());
    let again: RestoreReport = wire
        .calibrate_restore(camera, which)
        .await
        .expect("running restore twice is not an error");
    assert_eq!(
        again,
        RestoreReport {
            outcomes: Vec::new(),
            // Nothing was stranded either: the session repair runs on this path too, and an
            // empty list is what "nothing was left mid-sweep" looks like (note **N150**).
            freed: Vec::new(),
        }
    );
}

#[tokio::test]
async fn every_mutating_calibrate_verb_refuses_another_cameras_session_naming_the_fields() {
    // Rubric B5: "a session belongs to (camera fingerprint, task), and a check implemented
    // in one verb is a check the other verbs walk around". So the walk is over *all* of
    // them, with the same session id and the wrong camera each time — the shape
    // `crates/cli/tests/calibrate.rs` uses, ported to the wire.
    //
    // The refusal is one function's — `engine::lifecycle::session_to_update`, which every
    // handler routes through — and that is the point: six verbs, one check.
    let fixture = Fixture::start();
    let (camera, wire, asked) = ask(&fixture);
    let controls = fixture.controls();
    let (first, _) = two_scalars(&controls, &asked);

    // A session that genuinely belongs to the *other* camera the fixture replays. Its
    // fingerprint differs in two fields, which is what the refusal has to name.
    let others = engine::lifecycle::session_for(
        &fixture.store,
        &nth_camera(&fixture.cameras, 1).fingerprint,
        &fixture_session(),
    )
    .expect("the fixture opened one session per camera");
    let which = SessionRef::Id { id: others.id };

    let mut refusals: Vec<(&str, i32, Error)> = Vec::new();
    refusals.push({
        let (code, error) = refusal(
            wire.calibrate_plan(camera.clone(), which.clone(), vec![first.clone()], false)
                .await,
        );
        ("plan", code, error)
    });
    refusals.push({
        let (code, error) = refusal(
            wire.calibrate_plan(camera.clone(), which.clone(), Vec::new(), true)
                .await,
        );
        ("plan --order", code, error)
    });
    refusals.push({
        let (code, error) = refusal(
            wire.calibrate_sweep(
                camera.clone(),
                which.clone(),
                three_values(&controls, &first),
            )
            .await,
        );
        ("sweep", code, error)
    });
    refusals.push({
        let (code, error) = refusal(
            wire.calibrate_select(
                camera.clone(),
                which.clone(),
                first.clone(),
                Selection::ByMetric {
                    metric: MetricName::Sharpness,
                },
            )
            .await,
        );
        ("select", code, error)
    });
    refusals.push({
        let (code, error) = refusal(
            wire.calibrate_apply(camera.clone(), which.clone(), true)
                .await,
        );
        ("apply", code, error)
    });
    refusals.push({
        let (code, error) = refusal(wire.calibrate_restore(camera.clone(), which.clone()).await);
        ("restore", code, error)
    });
    // The read verb answers to the same law, which is why it is in the walk rather than
    // exempt from it.
    refusals.push({
        let (code, error) = refusal(wire.calibrate_status(camera.clone(), which.clone()).await);
        ("status", code, error)
    });

    let expected: Vec<String> = nth_camera(&fixture.cameras, 1)
        .fingerprint
        .differing_fields(&nth_camera(&fixture.cameras, 0).fingerprint)
        .into_iter()
        .map(str::to_owned)
        .collect();
    assert!(!expected.is_empty(), "the two fixtures share a fingerprint");
    for (verb, code, error) in &refusals {
        assert_eq!(*code, rpc_code(ErrorKind::FingerprintMismatch), "{verb}");
        let Error::FingerprintMismatch { fields } = error else {
            panic!("{verb} refused with something else: {error:?}");
        };
        assert_eq!(fields, &expected, "{verb} did not name the fields");
    }
    assert_eq!(refusals.len(), 7);

    // And a session nothing answers to at all is a different refusal, so the arm above is
    // measuring the fingerprint rather than "any session reference fails".
    let (_, missing) = refusal(
        wire.calibrate_apply(camera, SessionRef::Id { id: Uuid::nil() }, true)
            .await,
    );
    assert_eq!(missing.kind(), ErrorKind::IllegalTransition);
    assert!(missing.to_string().contains("no_session"), "{missing}");
}

#[tokio::test]
async fn no_calibrate_verb_answers_store_locked() {
    // An absence, so it needs a walk.
    //
    // **`StoreLocked`** is the trap `daemon::state`'s header exists for: the daemon holds one
    // advisory lock for its lifetime, and a handler that reached for D9's *daemonless*
    // protocol would answer this client `StoreLocked` naming the daemon's own pid and advising
    // it to use `webcam-handler-client` against the daemon it is already talking to. It does
    // not hang, which is why a test is the only thing that catches it.
    //
    // This walk **used to carry a second absence** and no longer does. `Unimplemented` was
    // note N43's retirement — these six verbs were the last rows in the map
    // `daemon::server::unrouted()` derived — and P4d deleted the variant itself (note N6),
    // so `assert_ne!(error.kind(), ErrorKind::Unimplemented)` is a line that no longer
    // compiles. The claim did not weaken, it moved: the D13 registry has eighteen members
    // and none of them means "not built yet", so no verb here or anywhere can answer it.
    // The loss is that this suite no longer *checks* it from the wire, which is written
    // down here rather than left to evaporate with the assertion.
    let fixture = Fixture::start();
    let (camera, wire, asked) = ask(&fixture);
    let controls = fixture.controls();
    let (first, second) = two_scalars(&controls, &asked);

    // Every verb, in both directions: a run that succeeds and a run that is refused. A walk
    // over refusals alone could pass on a build where every verb refused everything.
    let succeeded = calibrate_through(
        &wire,
        &camera,
        &task("no store locked"),
        &first,
        &second,
        &three_values(&controls, &first),
    )
    .await;
    assert!(succeeded.pre_snapshot.is_some());
    let which = SessionRef::Id { id: succeeded.id };
    wire.calibrate_apply(camera.clone(), which.clone(), true)
        .await
        .expect("one control is calibrated");
    wire.calibrate_restore(camera.clone(), which.clone())
        .await
        .expect("the session carries a snapshot");
    wire.calibrate_status(camera.clone(), which.clone())
        .await
        .expect("the session exists");
    wire.calibrate_list(Some(camera.clone()))
        .await
        .expect("the camera resolves");

    let refused = [
        refusal(
            wire.calibrate_start(
                camera.clone(),
                succeeded.task.clone(),
                "a second goal".to_owned(),
                Vec::new(),
            )
            .await,
        ),
        refusal(
            wire.calibrate_plan(
                camera.clone(),
                which.clone(),
                vec![asked.unknown_control.clone()],
                false,
            )
            .await,
        ),
        refusal(
            wire.calibrate_sweep(
                camera.clone(),
                which.clone(),
                SweepRequest::new(asked.unknown_control.clone(), SweepSpec::All),
            )
            .await,
        ),
        refusal(
            wire.calibrate_select(
                camera.clone(),
                which.clone(),
                second.clone(),
                Selection::ByValue {
                    value: 1,
                    chosen_by: schema::session::ChosenBy::Human,
                },
            )
            .await,
        ),
        refusal(
            wire.calibrate_apply(camera.clone(), which.clone(), false)
                .await,
        ),
        refusal(
            wire.calibrate_status(
                camera.clone(),
                SessionRef::Task {
                    task: task("nothing answers to this"),
                },
            )
            .await,
        ),
        refusal(
            wire.calibrate_restore(asked.unknown_camera.clone(), which.clone())
                .await,
        ),
        // A prefix two cameras answer to, which is a refusal about *which* camera rather
        // than about the session — so the walk covers both ways of naming one wrongly.
        refusal(
            wire.calibrate_status(asked.ambiguous.clone(), which.clone())
                .await,
        ),
    ];
    for (code, error) in &refused {
        assert_ne!(error.kind(), ErrorKind::StoreLocked, "{error}");
        assert_ne!(*code, rpc_code(ErrorKind::StoreLocked), "{error}");
    }
    // Not vacuous: the walk really did meet refusals, and more than one kind of them.
    assert_eq!(refused.len(), 8);
    let kinds: std::collections::BTreeSet<ErrorKind> =
        refused.iter().map(|(_, error)| error.kind()).collect();
    assert!(kinds.len() >= 3, "{kinds:?}");
}

/// The device's own state, read behind the daemon's back.
///
/// Values **and** raw flag words, for `mutating_verbs.rs`'s reason: PF:3's INACTIVE
/// coupling is half of "as you found it", and a restore that put the values back and left
/// the automation engaged has not put the camera back.
fn device_state(fixture: &Fixture) -> Vec<(ControlSlug, Option<ControlValue>, u32)> {
    fixture
        .controls()
        .into_iter()
        .map(|desc| (desc.slug, desc.current, desc.flags.raw))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_queue_edit_cannot_interleave_with_a_sweep_that_is_rewriting_the_same_session() {
    // Note **N47**. D9 gives the daemon one advisory lock for its lifetime, and `flock` does
    // not exclude its holder from itself — so two request tasks both legitimately hold
    // `&StoreLock` and nothing in the store stops them interleaving a read-modify-write.
    // `calibrate_plan --order` opens no camera, so not even the per-camera actor separates
    // it from a sweep on the same session: both would read the document, both would write a
    // draft cloned from what they read, and one client's edit would vanish while both
    // answered `Ok`. That is the exact defect `crates/cli`'s `session_to_update` doc records
    // having been found in `webcam-handler-cli`, transplanted inside one process.
    //
    // **What this test can and cannot claim.** The green direction is *certain*: the sweep
    // holds `Inner::sessions` for its whole run, so the reorder cannot have completed while
    // the sweep is provably mid-write — `is_finished()` is false because the mutex makes it
    // false, not because the scheduler happened not to get to it. The red direction is
    // measured rather than guaranteed: without the mutex the reorder has a whole sample — a
    // settle, a photo, a metric pass and a document commit — in which to land and be
    // clobbered, which is why it was verified by removing the mutex and watching both
    // assertions go red rather than by argument alone.
    let (entered, entrances) = std::sync::mpsc::sync_channel::<()>(1);
    let (release, tokens) = std::sync::mpsc::sync_channel::<()>(1);
    let gate = Arc::new(Gate {
        // Lowered: the session this test opens is created by a verb that *writes* to the
        // camera, and holding that would stop the setup rather than the sweep.
        armed: std::sync::atomic::AtomicBool::new(false),
        entered,
        release: std::sync::Mutex::new(tokens),
    });
    let fixture = Fixture::start_behind(|fake| {
        Arc::new(Blocking {
            inner: Arc::clone(fake),
            gate: Arc::clone(&gate),
        }) as Arc<dyn CameraBackend>
    });
    let asked = fixture.ask();
    let camera = asked.camera.clone();
    let controls = fixture.controls();
    let (first, second) = two_scalars(&controls, &asked);

    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");
    let opened: Session = wire
        .calibrate_start(
            camera.clone(),
            task("interleaving"),
            "a legible frame".to_owned(),
            Vec::new(),
        )
        .await
        .expect("a free slot");
    let which = SessionRef::Id { id: opened.id };
    wire.calibrate_plan(
        camera.clone(),
        which.clone(),
        vec![first.clone(), second.clone()],
        false,
    )
    .await
    .expect("two controls this camera has");

    // From here every control write waits for this test.
    gate.armed.store(true, std::sync::atomic::Ordering::Release);

    let sweeping = {
        let (wire, camera, which) = (wire.clone(), camera.clone(), which.clone());
        let request = three_values(&controls, &first);
        tokio::spawn(async move { wire.calibrate_sweep(camera, which, request).await })
    };
    let entrances = tokio::task::spawn_blocking(move || {
        entrances.recv().expect("the sweep reached the device");
        entrances
    })
    .await
    .expect("the blocking pool is alive");

    // The edit that would have interleaved, spawned while the sweep provably holds the
    // session: its first write is on the device and its pre-sweep snapshot is on disk.
    let reordering = {
        let (wire, camera, which) = (wire.clone(), camera.clone(), which.clone());
        let order = vec![second.clone(), first.clone()];
        tokio::spawn(async move { wire.calibrate_plan(camera, which, order, true).await })
    };

    // Let one write through and wait for the next. A whole sample lands in between — a
    // settle, a photo, a metric pass, and a `commit` that publishes the document from the
    // sweep's own in-memory session. That commit is what a lost update looks like.
    let release_once = release.clone();
    let entrances = tokio::task::spawn_blocking(move || {
        release_once.send(()).expect("the sweep is waiting");
        entrances.recv().expect("the sweep reached the next write");
        entrances
    })
    .await
    .expect("the blocking pool is alive");
    assert!(
        !reordering.is_finished(),
        "a queue edit landed inside a sweep's read-modify-write"
    );
    drop(entrances);

    // Lower the gate first, then hand over the one token the write that is waiting needs:
    // everything after it runs at the device's own pace, so nothing here has to guess how
    // many writes a guarded sweep makes.
    gate.armed
        .store(false, std::sync::atomic::Ordering::Release);
    release.send(()).expect("a write is waiting");

    let swept = sweeping
        .await
        .expect("the sweep task")
        .expect("a control with an ordered range");
    let reordered = reordering
        .await
        .expect("the reorder task")
        .expect("a permutation of the queue");

    // Both changes are on the document, which is what "no lost update" means: the sweep's
    // samples and the caller's order.
    assert_eq!(reordered.queue, vec![second, first.clone()]);
    let final_state = wire
        .calibrate_status(camera, which)
        .await
        .expect("the session exists");
    assert_eq!(final_state.session, reordered);
    assert_eq!(
        final_state
            .session
            .controls
            .get(&first)
            .expect("the swept control")
            .samples
            .len(),
        3,
        "the reorder republished a document without the sweep's samples"
    );
    assert_eq!(
        swept
            .controls
            .get(&first)
            .expect("the swept control")
            .samples
            .len(),
        3
    );
}

#[tokio::test]
async fn the_calibrate_half_is_really_crossing_the_socket() {
    // Every comparison in this file depends on this and none of them can see it: a
    // `Wire::Uds` that had quietly fallen back to the in-memory module would make the whole
    // suite one transport run twice, and every `assert_eq!` would still pass. It matters
    // here for a reason the other two suites do not have: what crosses is a *session
    // document*, and D9's state tree is the thing a client and a daemon have to agree about
    // when they are not the same process.
    let mut fixture = Fixture::start();
    let camera = CameraSelector::Id(nth_camera(&fixture.cameras, 0).id.clone());
    let [(_, in_memory), (_, over_uds)] = fixture.wires();

    over_uds
        .calibrate_status(camera.clone(), fixture_session())
        .await
        .expect("the socket is serving");

    fixture.handle.stop();
    fixture
        .handle
        .stopped()
        .await
        .expect("the server was asked to stop");

    // A transport failure, not a refusal the daemon sent — E3's distinction at the
    // transport layer, and the one P4f's "the daemon is not running" message is built on.
    match over_uds
        .calibrate_status(camera.clone(), fixture_session())
        .await
    {
        Err(ClientError::Transport(_)) => {}
        other => panic!("a socket nobody is listening on answered: {other:?}"),
    }

    in_memory
        .calibrate_status(camera, fixture_session())
        .await
        .expect("in-memory dispatch does not go through a socket");
}

// ------------------------------------------------- one enumeration, and one opinion with it

/// A backend that counts how many times it was asked what cameras exist.
///
/// A decorator in `Blocking`'s tradition — a counter of *this daemon's* behaviour rather than
/// a capability the fake should grow. `FakeBackend` counts opens, closes and streams because
/// those are device facts; "how many times did the daemon enumerate to answer one request" is
/// a fact about the handler, and it belongs to the suite that asserts it.
#[derive(Debug)]
struct Counting {
    inner: Arc<fake::FakeBackend>,
    enumerations: std::sync::atomic::AtomicU64,
}

impl Counting {
    fn enumerations(&self) -> u64 {
        self.enumerations.load(std::sync::atomic::Ordering::Acquire)
    }
}

impl CameraBackend for Counting {
    fn kind(&self) -> schema::backend::BackendKind {
        self.inner.kind()
    }

    fn enumerate(&self) -> schema::Result<Vec<schema::camera::CameraInfo>> {
        self.enumerations
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        self.inner.enumerate()
    }

    fn open(&self, id: &CameraId) -> schema::Result<Box<dyn Camera>> {
        self.inner.open(id)
    }

    fn watch(&self) -> schema::Result<Box<dyn schema::backend::HotplugWatch>> {
        self.inner.watch()
    }

    fn diagnose(&self) -> Vec<schema::report::ListHint> {
        self.inner.diagnose()
    }
}

#[tokio::test]
async fn a_calibrate_verb_resolves_its_camera_once_and_writes_to_what_it_resolved() {
    // `Wchd::on_camera`'s own doc gives the rule — "resolving it twice would be two
    // enumerations for one request" — and the five calibrate verbs that open a camera are the
    // ones that could most easily break it, because each needs the fingerprint *before* it
    // can build its closure. Handing the `CameraId` on to be resolved again would give a
    // request two answers to "which camera is this about": `session_to_update` would compare
    // the session against enumeration #1 and `lifecycle::draft`, `apply` and the sweep would
    // write to enumeration #2, which is the binding rubric B5 exists to make impossible.
    //
    // Enumeration is a live read every time (E2) and the second one would run *inside*
    // `editing_sessions()`, so the everyday cost is a `/sys` walk plus a `QUERYCAP` per node
    // charged to every other client waiting on the session mutex.
    let counter: Arc<std::sync::Mutex<Option<Arc<Counting>>>> =
        Arc::new(std::sync::Mutex::new(None));
    let kept = Arc::clone(&counter);
    let fixture = Fixture::start_behind(move |fake| {
        let counting = Arc::new(Counting {
            inner: Arc::clone(fake),
            enumerations: std::sync::atomic::AtomicU64::new(0),
        });
        *lock(&kept) = Some(Arc::clone(&counting));
        counting as Arc<dyn CameraBackend>
    });
    let counting = lock(&counter)
        .clone()
        .expect("the fixture built the daemon's backend");
    let (camera, wire, asked) = ask(&fixture);
    let controls = fixture.controls();
    let (first, _) = two_scalars(&controls, &asked);

    // Every one of the five verbs that opens a camera, plus the one branch that opens none
    // and still resolves: what would regress is one handler, not the set, so the walk is over
    // all of them. Each baseline is read immediately before its call, because the fixture's
    // own setup enumerates too and the claim is about one request rather than about a total.
    let opened = {
        let baseline = counting.enumerations();
        let opened = wire
            .calibrate_start(
                camera.clone(),
                task("one enumeration"),
                "a legible frame".to_owned(),
                vec!["sharp".to_owned()],
            )
            .await
            .expect("a free slot");
        assert_eq!(counting.enumerations() - baseline, 1, "calibrate_start");
        opened
    };
    let which = SessionRef::Id { id: opened.id };
    let sweep = three_values(&controls, &first);

    let baseline = counting.enumerations();
    wire.calibrate_plan(camera.clone(), which.clone(), vec![first.clone()], false)
        .await
        .expect("a control this camera has");
    assert_eq!(counting.enumerations() - baseline, 1, "calibrate_plan");

    let baseline = counting.enumerations();
    wire.calibrate_sweep(camera.clone(), which.clone(), sweep)
        .await
        .expect("a control with an ordered range");
    assert_eq!(counting.enumerations() - baseline, 1, "calibrate_sweep");

    wire.calibrate_select(
        camera.clone(),
        which.clone(),
        first.clone(),
        Selection::ByMetric {
            metric: MetricName::Sharpness,
        },
    )
    .await
    .expect("a control that swept");

    let baseline = counting.enumerations();
    wire.calibrate_apply(camera.clone(), which.clone(), false)
        .await
        .expect("a settled queue");
    assert_eq!(counting.enumerations() - baseline, 1, "calibrate_apply");

    let baseline = counting.enumerations();
    wire.calibrate_restore(camera.clone(), which.clone())
        .await
        .expect("the pre-sweep snapshot this session armed");
    assert_eq!(counting.enumerations() - baseline, 1, "calibrate_restore");

    // The branch that opens no camera at all still resolves — it needs the fingerprint to
    // find the session — so "one" is the number for every calibrate verb rather than "one for
    // the ones that open".
    let baseline = counting.enumerations();
    wire.calibrate_plan(camera, which, vec![first], true)
        .await
        .expect("the queue reordered into itself is a permutation of itself");
    assert_eq!(
        counting.enumerations() - baseline,
        1,
        "plan --order enumerated more than once"
    );
}
