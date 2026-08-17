//! docs/9's T5 method-count walk: every method the daemon registers, driven over the fake.
//!
//! docs/7 P4c's headline proof obligation, and the one row docs/9 Part 2 still carried
//! unstruck: "the registered `RpcModule`'s `method_names()` — built from the real server,
//! which is what the compiler enforces — compared against the integration-test inventory;
//! derived from the running registration, never a hand list", failing on "a wire method with
//! no test". The plan's other sentence for this sub-milestone — "Every method exercised over
//! the fake" — is the same claim from the other side, so it is the same test.
//!
//! ## Why a walk rather than a match
//!
//! A Rust trait does not reify its methods, so there is no `match` whose non-exhaustiveness
//! a compiler could refuse. docs/9 says so where it commissions this row, and note **N28**
//! says which population it means: not `api::METHODS` (which `crates/api` already compares
//! against a module built over a double) but *this daemon's own registration* — the
//! `Methods` value both wires serve, built by the generated `into_rpc()` over a real
//! `Wchd`.
//!
//! ## Both sides are derived, which is the whole point
//!
//! - **Registered:** `Fixture::methods.method_names()`, read off the value the fixture
//!   serves. Nothing here lists a method name.
//! - **Exercised:** whatever the *generated client* actually sent, recorded at the transport
//!   by [`Recording`] as each call passes through. The name comes from `#[method(name = …)]`'s
//!   expansion, so this file writes down no spelling of its own; it writes down what it saw.
//!
//! ## The registered set is **partitioned**, never filtered by hand (P4e-i)
//!
//! Since the subscriptions landed, `method_names()` carries four names no `ClientT` can
//! send: jsonrpsee registers an `unsubscribe` callback of its own beside every
//! `#[subscription]` (`rpc_module.rs::verify_and_register_unsubscribe`), and the generated
//! client reaches the subscribe half through `SubscriptionClientT` — which `Wire` cannot
//! implement, because `jsonrpsee_core::client::Subscription`'s only constructor is private
//! (note **N57**). So the comparison below subtracts `api::SUBSCRIPTIONS`' names from the
//! registered set rather than naming them here, and `tests/subscriptions.rs` is the walk
//! that drives *that* population. Two walks over two populations, each derived from the one
//! declaration in `crates/api`, and a third subscription joins both by existing.
//!
//! A hand list on either side would agree with itself forever (rubric rule 6). What is
//! unavoidably hand-written is the *sequence of calls* in [`every_method`] — a Rust test
//! cannot walk a trait and invent arguments for it — and that is precisely what the
//! comparison protects: a twentieth method that nobody calls here leaves `exercised` one
//! short and the count stops, which is the row's stated failure mode.
//!
//! ## Why it is one test
//!
//! `cargo-nextest` runs every test in its own process, so there is no cross-test
//! aggregation: twenty-two separate `#[test]`s recording into a process-global set would each
//! see one name. The whole surface is therefore driven by one test, twice — once per
//! transport — and the per-verb *behaviour* lives in the sibling suites, which is where the
//! depth is: `read_verbs.rs` for the six that read, `mutating_verbs.rs` for the ones that
//! write to a camera or signal a process, `calibrate_verbs.rs` for the eight that write a
//! session document.
//!
//! ## The honest limits, stated because a count that measured nothing would still be green
//!
//! 1. **It counts calls, not depth.** A method reached by one call with one shallow
//!    assertion satisfies the walk. Two things push back and neither is a proof: every
//!    field of [`Surface`] is read by an assertion below, and an unread field is a
//!    `dead_code` warning, which this workspace compiles as an error — so an answer nobody
//!    looked at does not build. Depth is the sibling suites' and the review's.
//! 2. **It is one suite's inventory, not the workspace's.** Because of the process rule
//!    above, the walk sees what *this* test drove. A verb exercised thoroughly elsewhere and
//!    not here still stops the count — the intended direction — but this row is not a
//!    workspace-wide coverage claim and must not be read as one.
//! 3. **It cannot see "registered but still refusing".** A build where every method
//!    answered a refusal would pass this walk, because they were called and they answered.
//!    The walk is only meaningful paired with `daemon::server`'s own
//!    `the_pinned_routing_is_the_whole_wire_surface_and_nothing_answers_unimplemented` and
//!    with `calibrate_verbs.rs`'s `no_calibrate_verb_answers_store_locked` (note **N43**).
//!    The refusal that used to make this concrete — `Error::Unimplemented`, a method saying
//!    "not built yet" — no longer exists in the registry (note **N6**, retired at P4d), so
//!    the limit is now about *any* refusal rather than about that one. The success
//!    assertions below are the third leg: a method that refused would not produce the
//!    answer this file reads.
//! 4. **It cannot catch a rename.** The client and the server are two expansions of *one*
//!    declaration, so a changed `#[method(name = …)]` moves both sides together and this
//!    comparison stays green. That is `crates/api`'s pinned-spelling test's job
//!    (`the_surface_registers_the_twenty_two_methods_and_the_two_subscriptions_and_nothing_else`),
//!    and it is why that pin is a list on purpose.
//! 5. **It cannot see a method on the trait that nothing registered**, because there is no
//!    second registration path: `into_rpc()` is the one, D10 exists to keep it that way, and
//!    neither `uds.rs` nor `http::rpc` registers a `wch_`-prefixed name beside it. Removing a
//!    method from the
//!    trait is a *compile* failure here rather than a count failure, which is the stronger
//!    direction and the reason it is not asserted.
//!
//!    **What this file cannot see at all is a second registration behind a *third* transport**
//!    (P5b's WebSocket route on the TCP listener), because the two wires here are handed
//!    `Fixture::methods` by construction and would agree with themselves. `web_rpc.rs` is that
//!    claim's home and makes it the only way it can be made — over a real socket, with the
//!    population read off `method_names()` and the failure being `-32601`.
//! 6. **Non-vacuity depends on the fixture.** A refusal records its method name just as an
//!    answer does, so a badly degraded fixture that refused everything would keep this green
//!    on the count alone. Hence [`discriminating_refusals`]: the same client, over the same
//!    wire, asking for three things that are not there and getting three *different* typed
//!    refusals is the cheapest proof that the answers above are answers rather than a stub.

#[path = "support/fixture.rs"]
mod fixture;
mod support;
#[path = "support/wire.rs"]
mod wire;

use std::collections::BTreeSet;
use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};

use api::{PhotoResponse, WchRpcClient, rpc_code};
use jsonrpsee::core::DeserializeOwned;
use jsonrpsee::core::client::{BatchResponse, ClientT, Error as ClientError};
use jsonrpsee::core::params::BatchRequestBuilder;
use jsonrpsee::core::traits::ToRpcParams;
use schema::capture::{
    PhotoFormat, PhotoRequest, SettlePolicy, SettleSpec, Sink, StreamRequest, Transform,
};
use schema::control::{ControlDesc, ControlSlug, ControlValue, ControlWrite};
use schema::error::{Error, ErrorKind};
use schema::metrics::MetricName;
use schema::profile::DeviceProfile;
use schema::report::{CameraDetail, CameraList, ControlReport, DiscoveryReport, WriteReport};
use schema::session::{
    Selection, Session, SessionList, SessionRef, SessionStatus, SweepRequest, SweepSpec,
};
use schema::snapshot::{RestoreReport, Snapshot};
use schema::video::{RecordReport, RecordRequest, RecordStatus};

use crate::fixture::{Ask, Fixture, SESSION_TASK};
use crate::wire::{Wire, refusal};

// ------------------------------------------------------- the exercised half, as a recording

/// A wire, plus a record of every method name that crossed it.
///
/// The seam this whole file rests on, and it is deliberately the *thinnest* thing that can
/// be one: it inserts the name it was handed and forwards. The name is the generated
/// client's — `#[method(name = …)]`'s expansion, which is also what the server registered —
/// so the exercised side of the comparison is derived from the same declaration as the
/// registered side rather than transcribed beside it.
///
/// Wrapping [`Wire`] rather than replacing it matters for the same reason `Wire` itself is
/// one enum over two transports: a second encoder here would make the comparison a
/// comparison between two clients.
#[derive(Debug, Clone)]
struct Recording {
    inner: Wire,
    seen: Arc<Mutex<BTreeSet<String>>>,
}

impl Recording {
    /// Start recording over one transport.
    fn over(inner: Wire) -> Recording {
        Recording {
            inner,
            seen: Arc::new(Mutex::new(BTreeSet::new())),
        }
    }

    /// Every method name that has crossed this wire so far.
    fn seen(&self) -> BTreeSet<String> {
        lock(&self.seen).clone()
    }
}

/// Take a lock, treating poison as the panic it is rather than as a second failure.
///
/// A poisoned mutex here means a test thread panicked while holding it, and the useful
/// failure is that panic; `unwrap_or_else(PoisonError::into_inner)` keeps this from
/// replacing it with a confusing one. `daemon::server`'s own tests carry the same helper for
/// the same reason.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl ClientT for Recording {
    fn request<R, Params>(
        &self,
        method: &str,
        params: Params,
    ) -> impl Future<Output = Result<R, ClientError>> + Send
    where
        R: DeserializeOwned,
        Params: ToRpcParams + Send,
    {
        // Recorded *before* the call and regardless of its outcome: a refusal is an
        // exercise of the method that refused, and `terminate_holder` below is answered by
        // a refusal on purpose. Recording only successes would make the count a count of
        // happy paths.
        lock(&self.seen).insert(method.to_owned());
        self.inner.request(method, params)
    }

    async fn notification<Params>(&self, method: &str, params: Params) -> Result<(), ClientError>
    where
        Params: ToRpcParams + Send,
    {
        // Forwarded rather than refused here, so that the refusal a caller meets is the
        // transport's own sentence and not a second one this file invented.
        self.inner.notification(method, params).await
    }

    async fn batch_request<'a, R>(
        &self,
        batch: BatchRequestBuilder<'a>,
    ) -> Result<BatchResponse<'a, R>, ClientError>
    where
        R: DeserializeOwned + fmt::Debug + 'a,
    {
        self.inner.batch_request(batch).await
    }
}

// ---------------------------------------------------------------- one answer per method

/// One answer per method on the T5 surface, named the way the method is named.
///
/// Twenty-two fields, and every one is read by [`assert_every_answer_is_an_answer`] —
/// which is not a convention this file keeps by discipline: an unread field is `dead_code`,
/// and this workspace compiles warnings as errors, so an answer nobody looked at fails the
/// build. That is the structural half of limit 1 in this file's header.
#[derive(Debug)]
struct Surface {
    list: CameraList,
    info: CameraDetail,
    controls: ControlReport,
    get: ControlDesc,
    set: WriteReport,
    snapshot: Snapshot,
    photo: PhotoResponse,
    /// The three P6c added, and the only trio in this walk where one call's answer is the
    /// next call's precondition: a status is about a take a start made, and a stop collects
    /// it. Driven in that order and nowhere else in this file.
    record_start: RecordStatus,
    record_status: RecordStatus,
    record_stop: RecordReport,
    discover_pairs: DiscoveryReport,
    profile_capture: DeviceProfile,
    restore: RestoreReport,
    /// The one method whose exercise is a refusal — see [`every_method`].
    terminate_holder: (i32, Error),
    calibrate_start: Session,
    calibrate_plan: Session,
    calibrate_sweep: Session,
    calibrate_status: SessionStatus,
    calibrate_select: Session,
    calibrate_apply: WriteReport,
    calibrate_restore: RestoreReport,
    calibrate_list: SessionList,
}

/// Drive every method the daemon registers, once, in an order a caller could use.
///
/// Generic over the **generated** client trait, so the method names, the parameter names and
/// the response types all come from `webcam-handler-api`'s declaration: this is the property
/// that makes the recorded set derived rather than transcribed, and it makes a rename in the
/// trait a compile failure here rather than a literal that quietly stops matching.
///
/// The order is load-bearing in three places and arbitrary everywhere else. `snapshot` comes
/// before the writes and `restore` after them, so a second run of this function over the
/// second transport starts where the first one did rather than in its residue. The calibrate
/// seven run in D8's own order — a control that never swept cannot be selected, and a
/// session with uncalibrated work cannot be applied — and `calibrate_restore` last, because
/// it spends the pre-sweep snapshot that `calibrate_sweep` armed (note N23).
///
/// Each call is deliberately the *shallowest* one that produces a real answer. Depth belongs
/// to the sibling suites named in this file's header; what belongs here is that the method
/// was driven over the fake and answered as itself.
async fn every_method<C: WchRpcClient + Sync>(
    client: &C,
    ask: &Ask,
    sweep: &SweepRequest,
    recording: &camino::Utf8Path,
    named_for: &str,
) -> Surface {
    let camera = ask.camera.clone();
    let list = client.list().await.expect("the fake enumerates");
    let info = client
        .info(camera.clone())
        .await
        .expect("the camera resolves and opens");
    let controls = client
        .controls(camera.clone())
        .await
        .expect("the camera answers");
    let get = client
        .get(camera.clone(), ask.control.clone())
        .await
        .expect("a control this camera has");

    let snapshot = client
        .snapshot(camera.clone())
        .await
        .expect("every writable control reads back");
    let set = client
        .set(camera.clone(), vec![a_write(&get)], true)
        .await
        .expect("an unpaired scalar has nothing to switch off");
    let photo = client
        .photo(camera.clone(), a_photo())
        .await
        .expect("the fake synthesizes a frame");
    // P6c's three, in the one order that works: a take, a question about it, and the
    // collection that empties the camera's slot. The take asks for the **shortest** duration
    // this build records, because what a census needs is that the three answered as
    // themselves, and a shorter take is a deterministic one: the driver takes one turn and
    // ends on its budget, so `record_stop` collects rather than waits. Depth is
    // `mutating_verbs.rs`'s.
    let record_start = client
        .record_start(camera.clone(), a_recording(recording))
        .await
        .expect("a camera with no take on it");
    let record_status = client
        .record_status(camera.clone())
        .await
        .expect("the camera resolves");
    let record_stop = client
        .record_stop(camera.clone())
        .await
        .expect("the take this call started");

    let discover_pairs = client
        .discover_pairs(camera.clone())
        .await
        .expect("the probe writes and puts the camera back");
    let profile_capture = client
        .profile_capture(camera.clone(), capturer(named_for))
        .await
        .expect("a capture reads the whole device");
    let restore = client
        .restore(camera.clone(), snapshot.clone())
        .await
        .expect("the snapshot came from this camera");

    // The one method whose exercise over the fake is a refusal, and the reason is the verb's
    // own: `terminate_holder` signals a process, and the only pid this test may be wrong
    // about is its own — which is alive, is signallable, and does not hold this camera's
    // node, so the answer is `HolderGone` and nothing is sent. `mutating_verbs.rs` drives
    // the other direction with a forked child holding a doctored node and asserts the
    // signal really arrived; what this file needs is that the method answered.
    let me = i32::try_from(std::process::id()).expect("a pid Linux can represent");
    let terminate_holder = refusal(client.terminate_holder(camera.clone(), me).await);

    let calibrate_start = client
        .calibrate_start(
            camera.clone(),
            census_task(named_for),
            "a legible frame".to_owned(),
            vec!["sharp".to_owned()],
        )
        .await
        .expect("a free (camera, task) slot");
    let which = SessionRef::Id {
        id: calibrate_start.id,
    };
    let calibrate_plan = client
        .calibrate_plan(
            camera.clone(),
            which.clone(),
            vec![sweep.control.clone()],
            false,
        )
        .await
        .expect("a control this camera has");
    let calibrate_sweep = client
        .calibrate_sweep(camera.clone(), which.clone(), sweep.clone())
        .await
        .expect("a control with an ordered range");
    let calibrate_status = client
        .calibrate_status(camera.clone(), which.clone())
        .await
        .expect("the session this run opened");
    let calibrate_select = client
        .calibrate_select(
            camera.clone(),
            which.clone(),
            sweep.control.clone(),
            Selection::ByMetric {
                metric: MetricName::Sharpness,
            },
        )
        .await
        .expect("a control that swept");
    // `partial: false`, because the queue holds exactly the one control that swept and was
    // chosen: there is no uncalibrated work to walk around, which is the state D8 wants an
    // apply to be made from.
    let calibrate_apply = client
        .calibrate_apply(camera.clone(), which.clone(), false)
        .await
        .expect("a settled queue");
    let calibrate_restore = client
        .calibrate_restore(camera.clone(), which.clone())
        .await
        .expect("the pre-sweep snapshot this session armed");
    let calibrate_list = client
        .calibrate_list(Some(camera))
        .await
        .expect("the camera resolves");

    Surface {
        list,
        info,
        controls,
        get,
        set,
        snapshot,
        photo,
        record_start,
        record_status,
        record_stop,
        discover_pairs,
        profile_capture,
        restore,
        terminate_holder,
        calibrate_start,
        calibrate_plan,
        calibrate_sweep,
        calibrate_status,
        calibrate_select,
        calibrate_apply,
        calibrate_restore,
        calibrate_list,
    }
}

/// The task this census opens its session under, named for the transport that opened it.
///
/// Derived from the transport's name and spelled once, because two runs of [`every_method`]
/// against one camera would otherwise collide in one (camera, task) slot — which D9 refuses
/// with `SessionConflict` (note N14) — and because the assertion that the session came back
/// has to name the same task the call sent.
fn census_task(named_for: &str) -> String {
    format!("p4c method census: {named_for}")
}

/// Provenance for the profile this census captures, naming the transport that asked.
///
/// `capturer` is a free string a socket can put anything in, and a profile records who took
/// it (E1, E2). A census that wrote "test" into one would be leaving a document behind whose
/// provenance says nothing.
fn capturer(named_for: &str) -> String {
    format!("the P4c method census, over the {named_for} wire")
}

/// A write that lands somewhere else in the control's declared range.
///
/// Derived from the descriptor the census just read rather than named, and step-aligned off
/// the low end, so "the write changed something" is a property of this device rather than a
/// literal that stops being true when the fixture does.
fn a_write(desc: &ControlDesc) -> ControlWrite {
    let resting = desc
        .current
        .as_ref()
        .and_then(ControlValue::as_int)
        .expect("the fixture's writable scalar reads back");
    let elsewhere = if resting == desc.range.min {
        desc.range.min + desc.range.step
    } else {
        desc.range.min
    };
    assert_ne!(elsewhere, resting, "the write would not change anything");
    ControlWrite {
        control: desc.slug.clone(),
        value: ControlValue::Int(elsewhere),
    }
}

/// A photo that hands its bytes back and settles immediately.
///
/// `ReturnBytes` rather than a `ServerPath` because a census should write nothing it does
/// not have to: the sink variants and their two refusals are `mutating_verbs.rs`'s claim.
/// `SkipFrames { frames: 0 }` because nothing here is about settling — the fake reports a
/// deadline as spent rather than sleeping through it, so the deadline is a bound and not a
/// wait either way.
fn a_photo() -> PhotoRequest {
    PhotoRequest {
        stream: StreamRequest::default(),
        settle: SettlePolicy {
            spec: SettleSpec::SkipFrames { frames: 0 },
            deadline_ms: 5_000,
        },
        transform: Transform::None,
        sink: Sink::ReturnBytes {
            format: PhotoFormat::Jpeg,
        },
        // The census drives every method once against an idle camera, so there is never a
        // queue to wait for; D12's flag is exercised where it can go red, in
        // `mutating_verbs.rs`.
        wait: false,
    }
}

/// A recording of nothing at all, into `path`.
///
/// One millisecond on purpose: it is the shortest budget this build accepts, and
/// `engine::record::drive` checks the bound *before* each turn, so the take is over after one
/// and the census never waits on a camera. That is exactly the shallowest call that produces
/// a real answer — a negotiated stream, a chosen container and a file on disk — which is this
/// walk's rule for every method in it.
///
/// A budget of **zero** was what this said until 2026-08-17, and it wrote a container header
/// with no frames in it and answered as a success — which is the outcome note **N213** made
/// into a refusal, at both spellings, because an unattended caller cannot tell it from a
/// camera that delivered nothing.
///
/// A `ServerPath` and not a `ReturnBytes`, because a recording has no second variant: note
/// **N110** narrows this verb to one of D10's two, and asking for the other is a refusal
/// `mutating_verbs.rs` asserts rather than a shape this file could drive.
fn a_recording(path: &camino::Utf8Path) -> RecordRequest {
    RecordRequest {
        stream: StreamRequest::default(),
        duration_ms: Some(1),
        sink: Sink::ServerPath {
            path: path.to_owned(),
        },
        // The census drives one verb at a time against an idle camera, so there is never a
        // queue to wait for; D12's flag is exercised where it can go red, in
        // `mutating_verbs.rs`.
        wait: false,
    }
}

/// A sweep of `control` over the two ends of its declared range.
///
/// Two samples rather than a full sweep: what a census needs is that the verb ran, recorded
/// samples and left a session a selection can be made from. Which value a metric picks, and
/// that every other sample scored below it, is `crates/engine/tests/sweep.rs`'s claim over
/// the same fake and the same committed optimum.
fn a_short_sweep(controls: &[ControlDesc], control: &ControlSlug) -> SweepRequest {
    let desc = engine::pairing::describe(controls, control).expect("a control this camera has");
    SweepRequest::new(
        control.clone(),
        SweepSpec::Explicit {
            values: vec![desc.range.min, desc.range.max],
        },
    )
}

// ------------------------------------------------------------- what the answers have to say

/// Every field of [`Surface`], read.
///
/// Shallow on purpose and *complete* on purpose: one assertion per method, so the census
/// cannot be satisfied by a surface that answered twenty-two times with nothing in it. The
/// compiler enforces the completeness (see [`Surface`]); this function is where the
/// enforcement is spent on something worth reading.
fn assert_every_answer_is_an_answer(answers: &Surface, ask: &Ask, seen: &str) {
    assert_eq!(answers.list.cameras.len(), 2, "{seen}: the fixture's two");
    assert!(!answers.info.formats.is_empty(), "{seen}");
    assert!(!answers.controls.controls.is_empty(), "{seen}");
    assert!(!answers.controls.pairs.is_empty(), "{seen}");
    assert_eq!(answers.get.slug, ask.control, "{seen}");

    // A write report names every write the plan made, and the census's write is unpaired,
    // so the plan is the one write and nothing was switched off to make it stick.
    assert_eq!(answers.set.writes.len(), 1, "{seen}: {:?}", answers.set);
    assert!(answers.set.disabled_automation.is_empty(), "{seen}");
    assert!(!answers.snapshot.entries.is_empty(), "{seen}");

    // Note N34's predicate from the side `webcam-handler-client` will call it from (P4f). The
    // daemon has already refused to send an answer that fails it; this is the client half, and
    // it is the assertion that makes a truncated payload visible here rather than in a file.
    assert!(answers.photo.bytes_match_the_delivery(), "{seen}");
    assert!(
        answers
            .photo
            .bytes
            .as_ref()
            .is_some_and(|bytes| bytes.as_slice().get(..2) == Some(&[0xff, 0xd8][..])),
        "{seen}: a JPEG SOI survives base64"
    );

    // P6c's three, each read for the one thing only it can say. The status is the *same*
    // take the start answered with — a build whose registry lost it between two calls would
    // answer about a camera with no take at all — and the report is the file the start named,
    // which is what says the bytes went where the request asked rather than somewhere this
    // daemon chose.
    let started = answers
        .record_start
        .take
        .as_ref()
        .unwrap_or_else(|| panic!("{seen}: a started take carries a take"));
    let polled = answers
        .record_status
        .take
        .as_ref()
        .unwrap_or_else(|| panic!("{seen}: a take that has not been collected is still there"));
    assert_eq!(started.path, polled.path, "{seen}");
    assert_eq!(started.format, polled.format, "{seen}");
    assert_eq!(answers.record_stop.path, started.path, "{seen}");
    assert_eq!(answers.record_stop.format, started.format, "{seen}");
    assert!(
        answers.record_stop.path.exists(),
        "{seen}: the recording named a file nothing wrote"
    );

    assert!(
        !answers.discover_pairs.controls.controls.is_empty(),
        "{seen}"
    );
    // The probe writes, so the report saying it put the camera back is the half of AGENTS
    // rule 8 a census can see. `mutating_verbs.rs` reads the device behind the daemon's
    // back for the other half.
    assert!(answers.discover_pairs.restored.is_complete(), "{seen}");

    assert_eq!(
        answers.profile_capture.provenance.capturer,
        capturer(seen),
        "{seen}: a profile records who took it"
    );
    assert!(!answers.profile_capture.state.values.is_empty(), "{seen}");

    assert_eq!(
        answers.restore.outcomes.len(),
        answers.snapshot.entries.len(),
        "{seen}: one outcome per snapshotted control"
    );

    // The refusal, as the code that arrived and the D13 error a client recovers — asserted
    // as itself rather than as "an error", because `HolderGone` is the one answer that says
    // nothing was signalled.
    let me = i32::try_from(std::process::id()).expect("a pid Linux can represent");
    assert_eq!(
        answers.terminate_holder,
        (
            rpc_code(ErrorKind::HolderGone),
            Error::HolderGone { pid: me }
        ),
        "{seen}"
    );

    assert_eq!(answers.calibrate_start.task, census_task(seen), "{seen}");
    assert_eq!(
        answers.calibrate_plan.queue,
        vec![ask.control.clone()],
        "{seen}"
    );
    assert_eq!(
        answers.calibrate_sweep.controls[&ask.control].samples.len(),
        2,
        "{seen}: one sample per swept value"
    );
    assert_eq!(
        answers.calibrate_status.session.id, answers.calibrate_start.id,
        "{seen}"
    );
    assert_eq!(
        answers.calibrate_select.calibrated().len(),
        1,
        "{seen}: the control that swept was chosen"
    );
    assert!(!answers.calibrate_apply.writes.is_empty(), "{seen}");
    // A sweep arms a pre-sweep snapshot and `calibrate_restore` spends it, so an empty
    // report here would mean the sweep left nothing to give back (note N23, N24).
    assert!(!answers.calibrate_restore.outcomes.is_empty(), "{seen}");
    assert!(!answers.calibrate_list.sessions.is_empty(), "{seen}");
}

/// Three questions about things that are not there, asked over the same wire.
///
/// Limit 6 in this file's header: a refusal records its method name as readily as an answer
/// does, so a census over a fixture that had degraded into refusing everything would still
/// count twenty-two. These three are the cheapest proof that it has not — the same client
/// getting three *different* typed refusals for three different absences, which a stub
/// cannot produce.
async fn discriminating_refusals<C: WchRpcClient + Sync>(client: &C, ask: &Ask, named_for: &str) {
    let (_, unknown_camera) = refusal(client.info(ask.unknown_camera.clone()).await);
    let (_, ambiguous) = refusal(client.info(ask.ambiguous.clone()).await);
    let (_, unknown_control) = refusal(
        client
            .get(ask.camera.clone(), ask.unknown_control.clone())
            .await,
    );

    let kinds = [
        unknown_camera.kind(),
        ambiguous.kind(),
        unknown_control.kind(),
    ];
    assert_eq!(
        kinds,
        [
            ErrorKind::CameraUnknown,
            ErrorKind::CameraAmbiguous,
            ErrorKind::ControlUnknown
        ],
        "{named_for}: {unknown_camera}; {ambiguous}; {unknown_control}"
    );
}

// ------------------------------------------------------------------------------- the walk

#[tokio::test]
async fn every_method_the_daemon_registers_is_exercised_over_the_fake() {
    let mut fixture = Fixture::start();
    let ask = fixture.ask();
    let sweep = a_short_sweep(&fixture.controls(), &ask.control);

    // The registered side: the very `Methods` value both wires serve, built by the
    // generated `into_rpc()` over a real `Wchd` (`support/fixture.rs`). Not `api::METHODS`,
    // which is a different population and already compared in `crates/api` against a module
    // built over a double — note N28 draws the line and this is the daemon's side of it.
    let registered: BTreeSet<String> = fixture.methods.method_names().map(str::to_owned).collect();
    assert!(!registered.is_empty(), "the daemon registered nothing");

    // The half of the registration this suite's client cannot reach, derived from
    // `api::SUBSCRIPTIONS` rather than written out — see this file's header. Asserted to be
    // a *subset* first, because subtracting a name nothing registered would silently make
    // the comparison below weaker rather than fail.
    let subscribed: BTreeSet<String> = api::SUBSCRIPTIONS
        .iter()
        .flat_map(api::wire::Subscription::names)
        .map(str::to_owned)
        .collect();
    assert!(
        subscribed.is_subset(&registered),
        "an unsubscribe spelling nothing registered: {subscribed:?} against {registered:?}"
    );
    let called: BTreeSet<String> = registered.difference(&subscribed).cloned().collect();

    // Where the census's recordings go: a throw-away directory outside the repository, for
    // AGENTS' reason — a frame may contain a person, and a test capture belongs in a
    // gitignored scratch dir even when the frames in it are synthesised from a committed
    // profile.
    let scratch = engine::paths::TempRuntimeDir::new().expect("a throw-away directory");

    for (named_for, transport) in fixture.wires() {
        let client = Recording::over(transport);
        let recording = scratch.base().join(format!("{named_for}.avi"));
        let answers = every_method(&client, &ask, &sweep, &recording, named_for).await;
        assert_every_answer_is_an_answer(&answers, &ask, named_for);
        discriminating_refusals(&client, &ask, named_for).await;

        let exercised = client.seen();
        // **This equality is the row.** It carries both directions and only one of them can
        // reach this line, which was measured rather than reasoned about: a daemon whose
        // registration is missing a method this suite drives fails *earlier*, at the call, as
        // `Call(ErrorObject { code: MethodNotFound })` out of the `expect` in
        // `every_method` — because a client that names an unregistered method gets
        // `-32601` rather than an answer. So this assertion's live direction is docs/9's
        // own — a registered method with no exercise — and the other is a sentence about
        // what the set equality means rather than a failure anybody will read.
        //
        // There used to be a loop under it that dropped each registered name from
        // `exercised` in turn and asserted the shortened set differed, sold as the row's
        // inverse arm. It could not fail for any input: after the equality above, `exercised`
        // *is* `registered`, so removing a member always succeeds and a proper subset is
        // never equal to its superset. A tautology under a comparison is worse than nothing,
        // because it reads as a second guard. The red-ability that is actually there was
        // demonstrated by deleting a call from `every_method` and watching this line fail.
        assert_eq!(
            exercised, called,
            "{named_for}: the daemon registers a method this suite does not drive, \
             or drives one it does not register"
        );

        // Not vacuous, and this is the assertion that says so rather than a loop that could
        // not have failed: two empty sets compare equal, so the count is only a count if the
        // number is the one `crates/api` pins the trait at. Twenty-two is the *call* surface;
        // the four names beside it are two subscriptions' worth, which is note N29's
        // accounting ("P4e grows the registered population by four rather than two").
        assert_eq!(called.len(), 22, "{named_for}: {called:?}");
        assert_eq!(
            registered.len(),
            22 + 2 * api::SUBSCRIPTIONS.len(),
            "{named_for}: {registered:?}"
        );
    }

    // The store half of "it really ran": each transport opened its own session, and they
    // are on disk beside the two the fixture wrote. Read through `engine::lifecycle`, which
    // is the function the daemon answers `calibrate_list` from — so this is the same claim
    // from the other side of the socket rather than a second reading of the same answer.
    let sessions = engine::lifecycle::list(&fixture.store, None).expect("the store walks");
    assert_eq!(sessions.sessions.len(), 4, "{sessions:?}");

    // And the accept loop outlived all twenty-two methods twice over: `stopped()` answers
    // *why* it stopped, so `Ok` here is "because this test asked" rather than "because a
    // handler took the server down with it", which is a failure mode a census that only
    // read answers could not tell from success.
    fixture.handle.stop();
    fixture
        .handle
        .stopped()
        .await
        .expect("the server was asked to stop");
}

#[tokio::test]
async fn the_recorder_writes_down_what_crossed_the_wire_and_nothing_else() {
    // The seam this file's headline claim rests on, tested rather than assumed: if
    // `Recording` recorded a name the client never sent — or missed one it did — the walk
    // above would be comparing the registered surface against a list this file made up,
    // which is exactly the hand list rubric rule 6 forbids.
    //
    // Three calls, because three things have to be true of the recording: it starts empty,
    // an answered call adds its own name once, and a *refused* call adds its name too —
    // `terminate_holder`'s only exercise over the fake is a refusal, so a recorder that
    // wrote down successes only would leave the walk one short.
    let fixture = Fixture::start();
    let ask = fixture.ask();
    let [(_, transport), _] = fixture.wires();
    let client = Recording::over(transport);

    assert!(client.seen().is_empty(), "nothing has crossed yet");

    client.list().await.expect("the fake enumerates");
    assert_eq!(
        client.seen(),
        BTreeSet::from(["wch_list".to_owned()]),
        "one call, one name, and the name is the trait's"
    );

    // The session the fixture opened for this camera, named the way a caller names one.
    client
        .calibrate_status(
            ask.camera.clone(),
            SessionRef::Task {
                task: SESSION_TASK.to_owned(),
            },
        )
        .await
        .expect("the fixture wrote one session per camera");
    let refused = refusal(client.info(ask.unknown_camera.clone()).await).1;
    assert_eq!(refused.kind(), ErrorKind::CameraUnknown, "{refused}");

    assert_eq!(
        client.seen(),
        BTreeSet::from([
            "wch_calibrate_status".to_owned(),
            "wch_info".to_owned(),
            "wch_list".to_owned(),
        ]),
        "an answer and a refusal are both exercises, and neither is recorded twice"
    );
}
