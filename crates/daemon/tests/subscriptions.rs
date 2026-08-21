//! The two things the daemon says without being asked, over both transports (docs/7 P4e-i).
//!
//! `method_surface.rs` walks the population a `ClientT` can *call*; this walks the one it
//! cannot. Since P4e-i the daemon registers twenty-three names, four of which belong to two
//! subscriptions — a subscribe and the `unsubscribe` jsonrpsee registers beside it — and no
//! transport of ours can reach either half through `api::WchRpcClient`, because
//! `SubscriptionClientT::subscribe` answers a type whose only constructor is private to
//! `jsonrpsee-core` (note **N57**). So the two suites partition the surface between them,
//! both sides derived from the one `wire_surface!` declaration, and a third subscription
//! joins both walks by existing.
//!
//! ## The story this file is the proof of
//!
//! *A client can watch, and nothing a client does can wedge the daemon.* Nine claims, and
//! each one is a test below:
//!
//! 1. **Both signals are really delivered, over both transports** — a hotplug event from
//!    the daemon's own watch thread and a progress event from a real sweep, decoded from
//!    frames that crossed a real WebSocket on the daemon's own `AF_UNIX` socket.
//! 2. **A subscriber that stops reading costs counted drops and never the daemon** — the
//!    bound is `limits::WS_MESSAGE_BUFFER_CAPACITY`, read rather than transcribed; a second
//!    subscriber on the same source is unaffected; and every event the slow one did not get
//!    is *counted* rather than lost.
//! 3. **Disconnect mid-sweep**: the sweep continues and the subscription is reaped, both
//!    asserted — docs/7's own sentence, and rubric B6's "neither cancels the sweep nor leaks
//!    the subscription".
//! 4. **Events emitted with nobody subscribed** are dropped and counted, which is P4e-i's
//!    decision and not an accident.
//! 5. **One connection may open only the subscriptions this project bounds** —
//!    `limits::RPC_MAX_SUBSCRIPTIONS_PER_CONNECTION`, refused before any handler runs, and
//!    a second connection unaffected by the first one's refusal.
//! 6. **The hostile directions, one test apiece**, and every one of them ends with the same
//!    question asked of the daemon: a client that subscribes and vanishes; one that names a
//!    session at subscribe time, and one that sweeps a session that does not exist; one that
//!    subscribes twice to the same sweep; a subscription that outlives its session and
//!    carries the next one's failure; a session that ends under a watcher; a connection that
//!    dies in the middle of a message.
//! 7. **The message buffer a real connection gives a subscription is the number this project
//!    chose** — read back off the sink, because `ServerConfig` cannot be read back and claim
//!    2's exactness lives on a dispatch that takes the buffer as an argument (rubric A8).
//! 8. **The hotplug watch's own two failure directions**, reachable at last because the
//!    fake's fault menu grew the variants for them: a host that cannot give a watch out
//!    refuses the *subscribe call* with a D13 code, and a watch that stops under an open
//!    subscription **ends that subscriber's stream with a reason** rather than leaving it
//!    accepted, counted and silently blind — plus the thread's own lifecycle, which is the
//!    one thing `limits::HOTPLUG_WATCH_DEADLINE_MS` participates in.
//! 9. **A client that floods the daemon's waiting arm** with `wch_photo {"wait": true}` on
//!    one connection is bounded at `limits::CAMERA_ENQUEUE_WAITERS` and refused past it —
//!    the bound this sub-milestone's own transport removed, because a WebSocket connection
//!    can hold arbitrarily many calls in flight where HTTP/1.1 could not (note **N59**).
//!
//! And one claim that is P4e-ii's rather than P4e-i's, because it is about these streams and
//! there is one honest place to assert it: **a daemon that is stopping ends every open
//! subscription with a reason** (`daemon::events::SHUTTING_DOWN`) rather than closing the
//! socket under it, and the hotplug thread still goes down with its last reader without a
//! token of its own. `daemon::shutdown` owns the order that makes the first true; this is the
//! half of it a client can see.
//!
//! ## Answering afterwards is half of every claim
//!
//! A wedged daemon does not fail an assertion — it fails to return — so each hostile
//! direction closes by asking the daemon a verb it has to answer: **over the connection that
//! did the damage, wherever the damage left one, and over a connection opened afterwards,
//! always** ([`still_answers`], [`a_fresh_client_is_served`]). The two are not the same
//! claim: a daemon that leaked a permit, a task or a lock keeps serving the client that
//! already holds a connection while refusing the next one, and one whose accept loop is gone
//! does the opposite. The plain `POST /` in the second is the third direction — the upgrade
//! and the calls share one listener, so a subscription that damaged the socket shows up on
//! the transport that never subscribes.
//!
//! ## Nothing here sleeps, and here is what each wait is instead
//!
//! - `Ws::frame` and `Subscription::next` end when the **daemon writes**. A subscription
//!   that never delivered turns those into a nextest `TIMEOUT` with a test's name on it
//!   rather than a hang (`.config/nextest.toml`).
//! - "The sweep is inside a write" is `support/gated.rs`'s announcement — an observation
//!   rather than a duration, in `daemon::server`'s own `Announcing` tradition.
//! - "The subscription was reaped" is `Wchd::watch_subscribers().wait_for(…)`, a change on a
//!   `watch` channel the daemon publishes. A counter can be read but not waited for, and a
//!   test that polled one would be a test with a sleep in it under another name.
//! - Scripting a hotplug event wakes the fake's watch through a `Condvar` (note **N57**),
//!   so an event a test asks for arrives when it asks rather than a deadline later.
//!
//! ## Two waits, and why the arithmetic needs both
//!
//! A slow subscriber can lose an event at either of two bounds — its connection's buffer or
//! the fan-out it shares — and which one it meets depends on how far its task has got.
//! Left there, "delivered plus dropped equals emitted" would be a measurement of the
//! scheduler. Two signals remove it:
//!
//! - the *reading* subscriber taking the last event says the **producer** finished;
//! - `Wchd::watch_losses()` reaching the overflow says the **stalled subscriber's own task**
//!   finished, because a loss is published only once the bound has refused an event.
//!
//! Only after both is the stalled subscriber read — reading it earlier would free a slot per
//! read and let its task send what it would otherwise have dropped. With them, the split is
//! exact and asserted: `dropped` is the overflow, `missed` and `unheard` are zero, and the
//! prefix the slow subscriber kept is the oldest events in order.
//!
//! ## What this file deliberately cannot claim
//!
//! **The instant a subscription was reaped, relative to the sweep's next emit.** What the
//! disconnect test observes is that the reaping happened before the sweep ended and that the
//! sweep ended whole, which is the pair of facts docs/7's sentence is about.
//!
//! **Which policy a lagging subscriber meets.** Forcing a real fan-out lag means keeping a
//! subscription's task from running while `SUBSCRIPTION_BROADCAST_DEPTH` events go past it,
//! which is a fact about the scheduler wearing a fact about the queue. The two policies'
//! difference is a fold with a unit test on it (`daemon::events::lag_verdict`), which is
//! where a decision no fixture can force belongs.

#[path = "support/fixture.rs"]
mod fixture;
#[path = "support/gated.rs"]
mod gated;
#[path = "support/subscribe.rs"]
mod subscribe;
mod support;
#[path = "support/wire.rs"]
mod wire;
#[path = "support/ws.rs"]
mod ws;

use std::collections::BTreeMap;
use std::sync::Arc;

use api::{WchRpcClient, rpc_code};
use fake::Fault;
use schema::backend::{CameraBackend, HotplugEvent};
use schema::camera::CameraId;
use schema::error::ErrorKind;
use schema::limits;
use schema::metrics::MetricName;
use schema::progress::{CalibrationProgress, ProgressEvent};
use schema::session::{Selection, Session, SessionRef, SweepRequest, SweepSpec};
use serde_json::json;

use crate::fixture::{Ask, Fixture};
use crate::gated::{Blocking, Gate};
use crate::subscribe::Watching;
use crate::wire::{Wire, refusal};
use crate::ws::Ws;

// ------------------------------------------------------------------ opening a subscription

/// Open `name` over one transport, with `buffer` as the connection's message buffer.
///
/// The buffer is a *parameter* rather than a default because it is the bound under test:
/// the in-memory dispatch takes it as an argument and a real connection takes it from
/// `limits::WS_MESSAGE_BUFFER_CAPACITY`, so passing the constant in is what makes the two
/// transports one experiment rather than two.
async fn open(fixture: &Fixture, named_for: &str, name: &str, buffer: usize) -> Watching {
    match named_for {
        "in-memory" => Watching::InMemory(
            fixture
                .methods
                .subscribe(name, jsonrpsee::core::EmptyServerParams::new(), buffer)
                .await
                .unwrap_or_else(|err| panic!("{name} over {named_for}: {err}")),
        ),
        _ => {
            let mut connection = Ws::connect(&fixture.socket).await;
            let answer = connection.call(name, json!({})).await;
            let subscription = answer
                .get("result")
                .unwrap_or_else(|| panic!("{name} over {named_for} was refused: {answer}"))
                .clone();
            Watching::Ws {
                connection: Box::new(connection),
                subscription,
            }
        }
    }
}

/// The bound a real connection runs under, as the `usize` both transports want.
fn buffer() -> usize {
    usize::try_from(limits::WS_MESSAGE_BUFFER_CAPACITY).expect("a small bound")
}

/// A sweep of two values on the fixture's writable control.
///
/// Two, because the claim is that events arrive rather than how many: a sweep is minutes of
/// camera time on real hardware and this suite runs against a replayed profile.
fn a_short_sweep(fixture: &Fixture, ask: &Ask) -> SweepRequest {
    let controls = fixture.controls();
    let desc =
        engine::pairing::describe(&controls, &ask.control).expect("a control this camera has");
    SweepRequest::new(
        ask.control.clone(),
        SweepSpec::Explicit {
            values: vec![desc.range.min, desc.range.max],
        },
    )
}

/// Open a session on the fixture's camera and queue its one control.
async fn a_planned_session(wire: &Wire, ask: &Ask, task: &str) -> SessionRef {
    let opened: Session = wire
        .calibrate_start(
            ask.camera.clone(),
            task.to_owned(),
            "a legible frame".to_owned(),
            Vec::new(),
        )
        .await
        .expect("a free slot");
    let which = SessionRef::Id { id: opened.id };
    wire.calibrate_plan(
        ask.camera.clone(),
        which.clone(),
        vec![ask.control.clone()],
        false,
    )
    .await
    .expect("a control this camera has");
    which
}

/// Read events until this sweep says it has stopped, and hand back the one that said so.
///
/// `CalibrationProgress::is_terminal` is per *control* and per *sweep* — `schema::progress`
/// says so in as many words, and its own test warns a consumer against closing on the first
/// sample — so this ends at the end of a sweep and never at the end of a stream. That
/// distinction is the subject of two tests below: a terminal event is not a close, and the
/// subscription that received one still carries the next session.
///
/// Each read ends when the daemon writes. A sweep whose terminal event never arrived is a
/// nextest `TIMEOUT` with a test's name on it rather than a hang — **watched**: against a
/// hand-applied `daemon::events::forward` that returns after its first delivery, the two
/// tests that use this become named TIMEOUTs at 180s (`.config/nextest.toml`'s 60s × 3) and
/// two of their siblings fail outright.
async fn until_the_sweep_stops(watching: &mut Watching) -> ProgressEvent {
    loop {
        let event: ProgressEvent = watching
            .next()
            .await
            .expect("the stream ended before the sweep did");
        if event.progress.is_terminal() {
            return event;
        }
    }
}

// ------------------------------------------------ the daemon is still answering afterwards

/// Ask the daemon two questions on `connection`, and require two *different* answers.
///
/// Every hostile direction below ends here, because that is the whole of P4e-i's story:
/// *nothing a client does can wedge the daemon* — asked of the connection that just did it.
/// Two questions rather than one for `method_surface.rs`'s `discriminating_refusals` reason:
/// a daemon that had degraded into answering everything with one blanket refusal would
/// satisfy "it answered". The refusal's code is derived from the D13 registry through
/// `api::codes::rpc_code` rather than transcribed, so a code that moves moves this with it.
///
/// **Watched red**: expecting any other D13 code here fails every one of the seven hostile
/// directions plus the disconnect test, in milliseconds — which is what says the second
/// question is really asked and really answered by the daemon rather than by this helper.
async fn still_answers(connection: &mut Ws<tokio::net::UnixStream>, ask: &Ask, after: &str) {
    still_answers_about(connection, ask, after, FIXTURE_CAMERAS, None).await;
}

/// The id behind the selector every suite here asks its questions with.
///
/// The `Ask` carries a *spelling* on purpose — a verb takes the selector a caller typed — and
/// what a listing answers with is an id, so the two meet here rather than at each caller.
fn asked_id(ask: &Ask) -> &CameraId {
    match &ask.camera {
        schema::selector::CameraSelector::Id(id) => id,
        other => {
            panic!("the fixture asks about a camera by something other than its id: {other:?}")
        }
    }
}

/// The camera ids one `wch_list` answer names, parsed.
///
/// Parsed rather than looked for in the printed answer, because the fixture's second camera is
/// the first one renamed (`support::fixture::second_camera` appends to the card) and its id
/// therefore carries the first one's as a prefix: over this machine a substring search cannot
/// tell a listing that lost the first camera from one that lost the second, which is the whole
/// distinction the caller below is asking about.
fn listed_ids(listed: &serde_json::Value, after: &str) -> Vec<CameraId> {
    listed["result"]["cameras"]
        .as_array()
        .unwrap_or_else(|| panic!("{after}: the daemon answered no listing at all: {listed}"))
        .iter()
        .map(|camera| {
            let spelled = camera["id"]
                .as_str()
                .unwrap_or_else(|| panic!("{after}: a listed camera with no id: {camera}"));
            CameraId::parse(spelled).unwrap_or_else(|| {
                panic!("{after}: the daemon listed `{spelled}`, which is not an id")
            })
        })
        .collect()
}

/// What one listing must say: how many cameras, and which machine they are on.
///
/// `FIXTURE_CAMERAS - 1` is equally true of a machine that lost the camera the fault was aimed
/// at and of one that lost the other, and D19 is a claim about the first: "`list` stops naming
/// the camera". So every listing this file reads back is asked for the identity too, in
/// whichever direction the caller is in a position to claim — the two loss callers pass the id
/// they killed and must not find it, and every other caller passes `None` and must still find
/// the camera the suite asks its questions about. Both arms are driven from this file, so
/// neither is a branch that only ever reads as a pass (notes **N160**, **N231**).
///
/// The arity is asserted **here** rather than beside the one caller that reads a listing off a
/// WebSocket, for two reasons. It keeps one home for where a listing's cameras live in the
/// answer — this function and its caller were reading `result.cameras` out of the same document
/// twice. And it leaves the plain `POST /` arm below a claim that goes red when a listing is the
/// wrong size: until this helper replaced it, that arm asserted the asked camera's spelling was
/// in the body, so a loss caller that now passes `gone` would otherwise be left holding an
/// absence and nothing else on the one transport those lines exist to speak for.
fn the_listing_is_about_the_machine_that_is_left(
    listed: &serde_json::Value,
    ask: &Ask,
    after: &str,
    cameras: usize,
    gone: Option<&CameraId>,
) {
    let named = listed_ids(listed, after);
    assert_eq!(
        named.len(),
        cameras,
        "{after}: the daemon stopped listing its cameras: {listed}"
    );
    match gone {
        Some(gone) => assert!(
            !named.contains(gone),
            "{after}: {gone} vanished mid-stream and the daemon's next listing still names it: \
             {named:?}"
        ),
        None => assert!(
            named.contains(asked_id(ask)),
            "{after}: the daemon stopped listing the camera this suite asks about: {named:?}"
        ),
    }
}

/// How many cameras the fixture backend replays.
///
/// Named rather than repeated, because one caller below expects a different number and the
/// difference is a claim rather than an adjustment: a take that met `DeviceGoneMidStream`
/// leaves a machine with one fewer camera on it (design D19), and a fake that went on listing
/// a device it had just told a caller was gone would be a shape no machine has (E5).
const FIXTURE_CAMERAS: usize = 2;

/// [`still_answers`], for a caller that knows the machine has changed under it.
///
/// `gone` is the camera that left, when one did: how many cameras a listing names and *which*
/// ones it names are two claims, and only the second is D19's.
async fn still_answers_about(
    connection: &mut Ws<tokio::net::UnixStream>,
    ask: &Ask,
    after: &str,
    cameras: usize,
    gone: Option<&CameraId>,
) {
    let listed = connection.call("wch_list", json!({})).await;
    the_listing_is_about_the_machine_that_is_left(&listed, ask, after, cameras, gone);

    let refused = connection
        .call("wch_info", json!({ "camera": &ask.unknown_camera }))
        .await;
    assert_eq!(
        refused["error"]["code"],
        json!(rpc_code(ErrorKind::CameraUnknown)),
        "{after}: the daemon stopped discriminating between its refusals: {refused}"
    );
}

/// The same, over a connection opened *after* the damage — and over the other transport.
///
/// "The same connection" and "a fresh connection" are two claims and neither implies the
/// other: a daemon that leaked a permit, a task or a lock would keep answering the client
/// that already holds a connection while refusing the next one, and a daemon whose accept
/// loop had gone would do the opposite. The plain `POST /` at the end is the third — the
/// upgrade and the calls share one listener (`daemon::uds`), so a subscription that damaged
/// the socket would be visible on the transport that never subscribes.
async fn a_fresh_client_is_served(fixture: &Fixture, ask: &Ask, after: &str) {
    a_fresh_client_is_served_about(fixture, ask, after, FIXTURE_CAMERAS, None).await;
}

/// [`a_fresh_client_is_served`], for a caller that knows the machine has changed under it.
async fn a_fresh_client_is_served_about(
    fixture: &Fixture,
    ask: &Ask,
    after: &str,
    cameras: usize,
    gone: Option<&CameraId>,
) {
    let mut fresh = Ws::connect(&fixture.socket).await;
    still_answers_about(&mut fresh, ask, after, cameras, gone).await;

    let answered = support::call(
        &fixture.socket,
        r#"{"jsonrpc":"2.0","id":1,"method":"wch_list","params":{}}"#,
    )
    .await
    .expect("the daemon is listening");
    // The third transport is asked the same question rather than searched for a spelling: a
    // `contains` over this body said the vanished camera was still there and read as the
    // daemon still answering, because the surviving camera's id carries the vanished one as a
    // prefix. Parsed, the two are told apart.
    let body = support::body(&answered);
    let served: serde_json::Value = serde_json::from_str(body).unwrap_or_else(|err| {
        panic!("{after}: the transport that never subscribes stopped answering: {err}: {body}")
    });
    the_listing_is_about_the_machine_that_is_left(&served, ask, after, cameras, gone);
}

// --------------------------------------------------------------------------- claim 1: both

#[tokio::test(flavor = "multi_thread")]
async fn every_subscription_the_daemon_registers_delivers_over_both_transports() {
    // **The walk**, and its population is `api::SUBSCRIPTIONS` — the second inventory
    // `wire_surface!` emits from the same declaration as the trait, so a third subscription
    // lands here as a failure rather than as a silence. The `match` below is the part a
    // Rust test cannot derive (nothing can invent the arrangement that makes an event
    // happen), and its fallback arm is what turns "a registered subscription nobody drives"
    // into a red test — which is exactly `method_surface.rs`'s shape for the call surface.
    //
    // Proven red by adding a third row to `wire_surface!` and running this: it fails at the
    // fallback naming the row, before any assertion below.
    let fixture = Fixture::start();
    let ask = fixture.ask();

    for (named_for, wire) in fixture.wires() {
        for subscription in api::SUBSCRIPTIONS {
            let mut watching = open(&fixture, named_for, subscription.name, buffer()).await;

            match subscription.name {
                "wch_subscribe_events" => {
                    // The daemon's own watch thread is the producer: subscribing started it
                    // (`daemon::events::Hotplug` — lazily, because a backend that cannot
                    // watch must not be a daemon that cannot start), and scripting the fault
                    // wakes it. Nothing here waits on a duration: the fake's watch honours
                    // its deadline through a `Condvar` a scripted fault ends (note N57).
                    fixture.backend.queue_fault(Fault::HotplugAdd);
                    let event: HotplugEvent = watching.next().await.expect("the watch delivered");
                    let HotplugEvent::Added { path } = &event else {
                        panic!("{named_for}: the wrong direction crossed the wire: {event:?}");
                    };
                    // A node, never a camera — `HotplugEvent`'s own contract, and the thing
                    // a transport could most easily "help" with (note N53).
                    assert!(path.as_str().starts_with("/dev/"), "{named_for}: {path}");
                }
                "wch_subscribe_calibration" => {
                    // A real sweep, over the same daemon, from the other transport — so
                    // this also says the stream is per *client* rather than per connection:
                    // the sweep is issued on a `Wire` and watched on a subscription that
                    // knows nothing about it.
                    let which = a_planned_session(&wire, &ask, &format!("watch-{named_for}")).await;
                    let sweeping = {
                        let (wire, camera, request) = (
                            wire.clone(),
                            ask.camera.clone(),
                            a_short_sweep(&fixture, &ask),
                        );
                        tokio::spawn(
                            async move { wire.calibrate_sweep(camera, which, request).await },
                        )
                    };
                    let event: ProgressEvent = watching.next().await.expect("the sweep emitted");
                    // Every in-flight variant carries the session it belongs to, which is
                    // what makes one stream multiplexable over many sessions
                    // (`schema::progress`).
                    assert_eq!(
                        event.session,
                        sweeping
                            .await
                            .expect("the sweep task")
                            .expect("a control with an ordered range")
                            .id,
                        "{named_for}: an event named a session this client did not sweep"
                    );
                }
                unknown => panic!(
                    "{named_for}: {unknown} is registered and this suite does not drive it — \
                     a subscription with no delivery test is the failure this walk exists for"
                ),
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn one_websocket_connection_carries_a_subscription_and_a_call_at_the_same_time() {
    // The transport claim P4b deferred, made where it is now true: the daemon's socket
    // carries HTTP/1.1 *and* the upgrade on the same connection, and jsonrpsee's HTTP path
    // is calls-only — so a subscription that reached the daemon at all reached it through
    // the upgrade `tests/uds.rs` asserts and this one uses.
    //
    // What is new here rather than there: the upgraded connection is **duplex**. A call's
    // answer and somebody's notification are in flight together, which is the fact
    // `support/subscribe.rs` matches request ids for and the one P4f's client transport has
    // to be built against.
    let fixture = Fixture::start();
    let ask = fixture.ask();

    let mut connection = Ws::connect(&fixture.socket).await;
    let opened = connection.call("wch_subscribe_events", json!({})).await;
    let subscription = opened
        .get("result")
        .unwrap_or_else(|| panic!("the upgrade did not carry a subscription: {opened}"))
        .clone();

    // A plain call on the same connection, while the subscription is open.
    fixture.backend.queue_fault(Fault::HotplugRemove);
    let listed = connection.call("wch_list", json!({})).await;
    assert_eq!(
        listed["result"]["cameras"]
            .as_array()
            .expect("a camera list")
            .len(),
        2,
        "{listed}"
    );

    // And the notification the call did not disturb.
    let mut watching = Watching::Ws {
        connection: Box::new(connection),
        subscription,
    };
    let event: HotplugEvent = watching.next().await.expect("the watch delivered");
    assert!(
        matches!(event, HotplugEvent::Removed { .. }),
        "the wrong direction crossed the wire: {event:?}"
    );

    // The plain HTTP transport is untouched by any of it — a `POST /` on its own connection,
    // through the client every other suite in this crate uses.
    let answered = support::call(
        &fixture.socket,
        r#"{"jsonrpc":"2.0","id":9,"method":"wch_list","params":{}}"#,
    )
    .await
    .expect("the daemon is listening");
    assert!(
        support::body(&answered).contains(&ask.camera.to_string()),
        "{}",
        support::body(&answered)
    );
}

// ------------------------------------------------------------- claim 2: the slow subscriber

#[tokio::test(flavor = "multi_thread")]
async fn a_subscriber_that_stops_reading_costs_counted_drops_and_never_the_daemon() {
    // **The claim this sub-milestone is named for.** It is a claim about a *bound*, so the
    // bound is `schema::limits`' and is read here rather than transcribed — moving the
    // constant moves this test with it, which is the shape `uds.rs`'s batch and body bounds
    // already use.
    //
    // Three things have to be true at once and none of them implies the others: the
    // producer finished, another subscriber lost nothing, and what the slow one lost was
    // counted. A daemon that blocked its sweep on a stalled socket would fail the first by
    // never finishing — which nextest turns into a named TIMEOUT rather than a hang.
    //
    // **Watched red**, against a hand-applied mutant that swaps `daemon::events`' `try_send`
    // for the `send().await` beside it: the run becomes a named `TIMEOUT` at 180s
    // (`.config/nextest.toml`'s 60s × 3) rather than a hang, which is the shape that config
    // exists to give and the reason this inverse is safe to write down.
    //
    // The in-memory dispatch, because the bound has to be the *only* buffer in the path: a
    // real socket puts the kernel's own send buffer between the daemon and a reader that has
    // stopped, so "the connection is full" over `AF_UNIX` is a fact about `SO_SNDBUF`
    // rather than about `WS_MESSAGE_BUFFER_CAPACITY`. `Methods::subscribe` takes the buffer
    // as an argument, which is what makes the arithmetic below exact.
    let fixture = Fixture::start();
    let ask = fixture.ask();
    let (_, wire) = fixture.wires().into_iter().next().expect("a transport");

    // Two subscribers on one source, which is what "fan-out" means: `stalled` never reads,
    // `keeping_up` reads everything.
    //
    // **They are given different buffers on purpose, and that is what makes the arithmetic
    // below exact rather than a measurement of the scheduler.** `stalled`'s is the bound
    // under test. `keeping_up`'s is the whole script, because a *reading* subscriber whose
    // buffer were also the bound would lose events whenever the daemon's task got ahead of
    // this test's loop — and "did the other subscriber lose anything" would then be a
    // question about which task ran first. Sizing it out of the experiment is what leaves
    // one variable in it.
    let over_the_bound = buffer() + OVERFLOW;
    let mut stalled = open(&fixture, "in-memory", "wch_subscribe_events", buffer()).await;
    let mut keeping_up = open(
        &fixture,
        "in-memory",
        "wch_subscribe_events",
        over_the_bound,
    )
    .await;
    assert_eq!(fixture.wchd.subscriptions().live, 2);

    // The loss count, taken **before** anything is emitted, so what is waited for below is a
    // change this test caused. A number that can be read but not waited for would leave the
    // assertions racing the daemon's own tasks.
    let mut losses = fixture.wchd.watch_losses();

    // More events than one connection's buffer can hold. The producer is the daemon's watch
    // thread and the script is this test's, so "the producer finished" is observable: the
    // last event reaches `keeping_up`, and `keeping_up` is the rendezvous.
    for _ in 0..over_the_bound {
        fixture.backend.queue_fault(Fault::HotplugAdd);
    }
    for delivered in 0..over_the_bound {
        let event: HotplugEvent = keeping_up
            .next()
            .await
            .unwrap_or_else(|| panic!("event {delivered} never reached the subscriber reading"));
        assert!(matches!(event, HotplugEvent::Added { .. }), "{event:?}");
    }

    // **The second rendezvous, and the one that makes the split exact.** The reading
    // subscriber having seen every event says the *producer* finished; it says nothing about
    // how far the stalled subscriber's own task has got, and reading that subscriber before
    // its task had filled its buffer would free a slot per read and turn `dropped` into a
    // number about task scheduling. So this waits for the daemon to say it has finished
    // losing — a change on a `watch`, not a poll of a counter.
    // `>=` and not `==`: a `watch` carries the latest value rather than every value, so a
    // waiter for one exact number is a waiter that can be stepped over. The exactness is
    // asserted below, where reading a number cannot hang.
    losses
        .wait_for(|lost| *lost >= u64::try_from(OVERFLOW).expect("a small bound"))
        .await
        .expect("the daemon publishes what it could not deliver");

    // **The wedge assertion, and it is a positive one**: a wedged daemon does not fail an
    // assertion, it fails to return. `ask.ambiguous` and `ask.unknown_control` are here for
    // the same reason `method_surface.rs`'s `discriminating_refusals` exists — a daemon
    // answering everything with one refusal would satisfy "it answered".
    assert_eq!(
        wire.list()
            .await
            .expect("the daemon is still serving")
            .cameras
            .len(),
        2
    );
    let (_, ambiguous) = refusal(wire.info(ask.ambiguous.clone()).await);
    assert_eq!(ambiguous.kind(), ErrorKind::CameraAmbiguous, "{ambiguous}");
    let (_, unknown_camera) = refusal(wire.info(ask.unknown_camera.clone()).await);
    assert_eq!(unknown_camera.kind(), ErrorKind::CameraUnknown);
    let (_, unknown_control) = refusal(
        wire.get(ask.camera.clone(), ask.unknown_control.clone())
            .await,
    );
    assert_eq!(unknown_control.kind(), ErrorKind::ControlUnknown);

    // The bound did its job on the subscriber that stopped, and it did it *at the bound*:
    // everything past one connection's buffer is dropped, and every drop is **counted**
    // rather than silently lost (rubric rule 3). The itemisation is asserted against the total
    // this test just waited on, which is what keeps the two bookkeepings from drifting.
    let hotplug = fixture.wchd.subscriptions().hotplug;
    assert_eq!(
        hotplug.dropped,
        u64::try_from(OVERFLOW).expect("a small bound"),
        "the connection buffer did not refuse exactly what would not fit: {hotplug:?}"
    );
    assert_eq!(hotplug.lost(), hotplug.dropped, "{hotplug:?}");
    // Nothing was lost at the *fan-out*: `SUBSCRIPTION_BROADCAST_DEPTH` is deliberately
    // deeper than one connection's buffer, so the hop that refuses is the private one and a
    // slow subscriber never costs another its events (`schema::limits`).
    assert_eq!(hotplug.missed, 0, "{hotplug:?}");
    assert_eq!(hotplug.unheard, 0, "{hotplug:?}");

    // And the conservation law, exactly: every event the producer emitted either reached
    // the stalled subscriber or was counted. What it kept is the **oldest** prefix, in
    // order — a gap it can detect rather than a scatter it cannot.
    let mut kept = 0_u64;
    for expected in 0..buffer() {
        let event: HotplugEvent = stalled.next().await.unwrap_or_else(|| {
            panic!("the stalled subscriber held fewer than its bound: {expected}")
        });
        assert!(matches!(event, HotplugEvent::Added { .. }), "{event:?}");
        kept += 1;
    }
    assert_eq!(
        kept + hotplug.lost(),
        u64::try_from(over_the_bound).expect("a small bound"),
        "an event was neither delivered nor counted: kept {kept}, {hotplug:?}"
    );

    // And nothing the slow subscriber did reached the one that was reading, which is the
    // whole point of a fan-out: `keeping_up` took every event above, in order, before any
    // of this.
    drop(keeping_up);
    drop(stalled);
}

/// How far past the connection buffer the stalled subscriber is driven.
///
/// Enough that the overflow cannot be a rounding error and small enough that the reading
/// subscriber's loop is a handful of frames. It is a test's own number rather than a
/// `limits` one: what `schema::limits` owns is the bound, and this is how far past it this
/// fixture goes.
const OVERFLOW: usize = 8;

// ------------------------------------------------------------- claim 3: disconnect mid-sweep

#[tokio::test(flavor = "multi_thread")]
async fn a_client_that_vanishes_mid_sweep_is_reaped_and_the_sweep_finishes_without_it() {
    // docs/7's own sentence — "the sweep continues, the subscription is reaped, both
    // asserted" — and rubric B6's spelling of it: "neither cancels the sweep nor leaks the
    // subscription". **Two claims and neither implies the other.** A daemon that abandoned
    // the sweep when its watcher left would pass the reaping assertion; one that never
    // noticed the disconnect would pass the sweep assertion while leaking a subscription per
    // client that walked away. D8's sweep holds a camera for minutes and has a pre-sweep
    // snapshot on disk (note N23), so abandoning it is the more expensive of the two.
    //
    // Every step below is a signal. The gate announces that a write has begun, the
    // `watch` channel announces that the count changed, and the store is read afterwards
    // through the same `engine::lifecycle` function the daemon answers `calibrate_status`
    // from — which is `method_surface.rs`'s and N47's idiom for "the same claim from the
    // other side of the socket".
    let (entered, entrances) = std::sync::mpsc::sync_channel::<()>(1);
    let (release, tokens) = std::sync::mpsc::sync_channel::<()>(1);
    let gate = Arc::new(Gate {
        // Lowered: `calibrate_start`'s probe writes to the camera, and holding that would
        // stop the setup rather than the sweep.
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
    let ask = fixture.ask();
    let (_, wire) = fixture.wires().into_iter().next().expect("a transport");
    let which = a_planned_session(&wire, &ask, "vanishing").await;

    // 1. Subscribe on its own **connection**, so dropping it drops nothing else. It has to
    //    be the WebSocket: a disconnect is a fact about a socket, and the in-memory
    //    dispatch has none to break.
    let mut subscribers = fixture.wchd.watch_subscribers();
    let watching = open(&fixture, "uds", "wch_subscribe_calibration", buffer()).await;
    subscribers
        .wait_for(|live| *live == 1)
        .await
        .expect("the daemon publishes its subscriber count");

    // 2. Start the sweep on the other transport and let it reach the device.
    gate.armed.store(true, std::sync::atomic::Ordering::Release);
    let sweeping = {
        let (wire, camera, which, request) = (
            wire.clone(),
            ask.camera.clone(),
            which.clone(),
            a_short_sweep(&fixture, &ask),
        );
        tokio::spawn(async move { wire.calibrate_sweep(camera, which, request).await })
    };
    let entrances = tokio::task::spawn_blocking(move || {
        entrances.recv().expect("the sweep reached the device");
        entrances
    })
    .await
    .expect("the blocking pool is alive");

    // 3. The client vanishes. No unsubscribe: the case is a broken pipe, not a polite exit.
    drop(watching);

    // 4. The daemon says so. This is a wait on an *event* — the count changing — rather
    //    than a poll of a counter, which is the difference between a deterministic
    //    assertion and a sleep nobody wrote down.
    subscribers
        .wait_for(|live| *live == 0)
        .await
        .expect("the daemon publishes its subscriber count");
    assert_eq!(
        fixture.wchd.subscriptions().calibration.subscribers,
        0,
        "a subscription outlived its connection"
    );

    // 5. Let the sweep finish at the device's pace, and assert it *completed*.
    gate.armed
        .store(false, std::sync::atomic::Ordering::Release);
    drop(entrances);
    release.send(()).expect("a write is waiting");
    let swept = sweeping
        .await
        .expect("the sweep task")
        .expect("a control with an ordered range");
    assert_eq!(
        swept.controls[&ask.control].samples.len(),
        2,
        "the sweep stopped when its watcher did"
    );

    // …and that the document on disk agrees, read the way `calibrate_status` reads it. A
    // build that cancelled the sweep on disconnect shows fewer samples here, a non-terminal
    // per-control status, and a control left at a swept value with its snapshot unspent.
    let on_disk = engine::lifecycle::status(
        &fixture.store,
        &crate::fixture::camera(&fixture.cameras, 0).fingerprint,
        &which,
    )
    .expect("the store walks");
    assert_eq!(on_disk.session, swept);

    // 6. And the events the sweep kept emitting into nothing were counted, not lost — which
    //    is what makes "reaped" a positive observation rather than the absence of a crash.
    let calibration = fixture.wchd.subscriptions().calibration;
    assert!(
        calibration.unheard > 0,
        "a sweep with no listener reported no drops: {calibration:?}"
    );

    // 7. The daemon is still a daemon: a client that broke a pipe mid-sweep cost the socket
    //    nothing, which is this sub-milestone's whole story asked of the case docs/7 names.
    a_fresh_client_is_served(&fixture, &ask, "after a watcher vanished mid-sweep").await;

    // 8. The server stops, which is the assertion a leaked bridging task turns into a
    //    TIMEOUT rather than a failure — and the reason P4e-ii inherits this fixture rather
    //    than building one.
    let mut fixture = fixture;
    fixture.handle.stop();
    fixture
        .handle
        .stopped()
        .await
        .expect("the server was asked to stop");
}

// ------------------------------------------------------ claim 4: nobody is listening

#[tokio::test(flavor = "multi_thread")]
async fn a_sweep_with_nobody_watching_drops_its_events_and_counts_them() {
    // **P4e-i's decision, both directions.** Nothing is buffered for a subscriber who has
    // not arrived — keeping a `Receiver` parked so nothing is "lost" would buffer a whole
    // sweep for nobody, which is the unbounded growth `limits::SUBSCRIPTION_BROADCAST_DEPTH`'s
    // doc rejects — and the drop is a *number*, because a silence is what rubric
    // rule 3 forbids.
    //
    // The second half is what makes it a decision rather than a bug: the same sweep, with a
    // subscriber attached, adds nothing to the count. A daemon that counted every event as
    // unheard would pass the first assertion alone.
    let fixture = Fixture::start();
    let ask = fixture.ask();
    let (_, wire) = fixture.wires().into_iter().next().expect("a transport");

    let unwatched = a_planned_session(&wire, &ask, "unwatched").await;
    wire.calibrate_sweep(ask.camera.clone(), unwatched, a_short_sweep(&fixture, &ask))
        .await
        .expect("a control with an ordered range");
    let alone = fixture.wchd.subscriptions().calibration;
    assert!(
        alone.unheard > 0,
        "a sweep nobody watched reported nothing dropped: {alone:?}"
    );
    assert_eq!(alone.subscribers, 0);

    // Now with somebody listening, over the wire.
    let mut watching = open(&fixture, "uds", "wch_subscribe_calibration", buffer()).await;
    let watched = a_planned_session(&wire, &ask, "watched").await;
    let sweeping = {
        let (wire, camera, request) = (
            wire.clone(),
            ask.camera.clone(),
            a_short_sweep(&fixture, &ask),
        );
        tokio::spawn(async move { wire.calibrate_sweep(camera, watched, request).await })
    };
    let event: ProgressEvent = watching.next().await.expect("the sweep emitted");
    let swept = sweeping
        .await
        .expect("the sweep task")
        .expect("a control with an ordered range");
    assert_eq!(event.session, swept.id);

    let watched_counts = fixture.wchd.subscriptions().calibration;
    assert_eq!(
        watched_counts.unheard, alone.unheard,
        "an event with a subscriber attached was counted as unheard: {watched_counts:?}"
    );
}

// ------------------------------------------------------- claim 5: the per-connection bound

#[tokio::test(flavor = "multi_thread")]
async fn one_connection_may_open_only_the_subscriptions_this_project_bounds() {
    // `limits::RPC_MAX_SUBSCRIPTIONS_PER_CONNECTION`, the second of the two numbers note
    // **N38** says P4e owns — read here rather than transcribed, so moving the constant
    // moves this test with it. jsonrpsee refuses the *subscribe call* before the handler
    // runs, which is why the refusal is a `-32006` answer and not a closed connection:
    // connect-and-abandon costs a client its own slots and nobody else's.
    //
    // -32006 is jsonrpsee's own code. D13's block starts at -32012 (`api::codes`), so there
    // is no collision to reason about — the same argument `uds.rs` makes about the batch
    // bound's -32010.
    //
    // Only over the real socket: the bound is a property of a *connection*, and
    // `Methods::subscribe` dispatches with a mock permit
    // (`jsonrpsee-core-0.26.0/src/server/rpc_module.rs`), so the in-memory arm cannot see
    // it. Stated rather than worked around.
    let fixture = Fixture::start();
    let bound = usize::try_from(limits::RPC_MAX_SUBSCRIPTIONS_PER_CONNECTION).expect("small");

    let mut connection = Ws::connect(&fixture.socket).await;
    for opened in 0..bound {
        let answer = connection
            .call("wch_subscribe_calibration", json!({}))
            .await;
        assert!(
            answer.get("result").is_some(),
            "subscription {opened} of the bound was refused: {answer}"
        );
    }
    // Every one of them is live, which is what makes the refusal below about the bound
    // rather than about a subscription that ended on its own.
    let mut subscribers = fixture.wchd.watch_subscribers();
    subscribers
        .wait_for(|live| *live == bound)
        .await
        .expect("the daemon publishes its subscriber count");

    let over = connection
        .call("wch_subscribe_calibration", json!({}))
        .await;
    assert_eq!(
        over["error"]["code"],
        json!(-32006),
        "a connection opened more subscriptions than this project bounds: {over}"
    );

    // …and a second connection is unaffected, which is the half that says the bound is per
    // connection rather than per daemon.
    let mut second = Ws::connect(&fixture.socket).await;
    let answer = second.call("wch_subscribe_calibration", json!({})).await;
    assert!(
        answer.get("result").is_some(),
        "one client's bound refused another client: {answer}"
    );

    // And both connections still answer a verb: the refusal cost the first client one
    // subscription and neither client a socket.
    let ask = fixture.ask();
    still_answers(&mut connection, &ask, "past the subscription bound").await;
    still_answers(&mut second, &ask, "beside a client past the bound").await;
    a_fresh_client_is_served(&fixture, &ask, "after a client hit the subscription bound").await;
}

// ---------------------------------------- claim 6: the hostile directions, one test apiece

#[tokio::test(flavor = "multi_thread")]
async fn a_client_that_subscribes_and_vanishes_costs_its_subscriptions_and_nothing_else() {
    // The plainest hostile direction there is, and the one every other reaping claim rests
    // on: a client opens everything this daemon offers and then simply stops existing. No
    // unsubscribe and no close frame — a broken pipe, which is the case a polite exit would
    // not test.
    //
    // The population is `api::SUBSCRIPTIONS` rather than two names written here, so a third
    // subscription is opened, counted and reaped by this test the day it lands.
    //
    // **Watched red** against a hand-applied `daemon::events::Counted::drop` that does not
    // decrement — the reap that a subscription's receiver owes on every one of the four ways
    // it can end. The wait below then never finishes and nextest names it: `TIMEOUT` at
    // 180s. That is the shape a leaked subscription has, and the reason this waits on a
    // `watch` rather than reading a counter: a read would have passed against that mutant on
    // whichever schedule happened to look right.
    let fixture = Fixture::start();
    let ask = fixture.ask();
    let mut subscribers = fixture.wchd.watch_subscribers();

    let mut connection = Ws::connect(&fixture.socket).await;
    for subscription in api::SUBSCRIPTIONS {
        let answer = connection.call(subscription.name, json!({})).await;
        assert!(
            answer.get("result").is_some(),
            "{}: {answer}",
            subscription.name
        );
    }
    subscribers
        .wait_for(|live| *live == api::SUBSCRIPTIONS.len())
        .await
        .expect("the daemon publishes its subscriber count");

    drop(connection);

    // Waited for rather than polled: the count is a `watch`, so this ends when the daemon
    // says the last receiver is gone.
    subscribers
        .wait_for(|live| *live == 0)
        .await
        .expect("the daemon publishes its subscriber count");
    let activity = fixture.wchd.subscriptions();
    assert_eq!(
        (
            activity.hotplug.subscribers,
            activity.calibration.subscribers
        ),
        (0, 0),
        "a subscription outlived the connection that opened it: {activity:?}"
    );

    // **And the next client is served the same surface**, which is the assertion that makes
    // the reaping structural rather than cosmetic: the hotplug watch the first client
    // started is a thread that ends with its last reader (`daemon::events::Hotplug`), so a
    // second subscriber either finds it still running or starts a fresh one — and a build
    // that got that handover wrong delivers nothing here rather than failing an assertion,
    // which nextest turns into a named TIMEOUT.
    let mut second = Ws::connect(&fixture.socket).await;
    let reopened = second.call("wch_subscribe_events", json!({})).await;
    let subscription = reopened
        .get("result")
        .unwrap_or_else(|| panic!("the second client was refused a subscription: {reopened}"))
        .clone();
    fixture.backend.queue_fault(Fault::HotplugAdd);
    let mut watching = Watching::Ws {
        connection: Box::new(second),
        subscription,
    };
    let event: HotplugEvent = watching.next().await.expect("the watch delivered");
    assert!(matches!(event, HotplugEvent::Added { .. }), "{event:?}");

    still_answers(
        watching.connection(),
        &ask,
        "watching after a client vanished",
    )
    .await;
    a_fresh_client_is_served(&fixture, &ask, "after a client vanished").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_named_at_subscribe_time_is_not_a_filter_and_an_unknown_one_refuses_only_its_sweep()
 {
    // **The hostile direction "a client subscribes to a session that does not exist", and
    // what it turns out to be.** Neither subscription takes a parameter — the stream is per
    // *client*, which `schema::progress`'s header settles and `crates/api` records — so
    // "subscribe to session X" is not a request this wire can carry. What a client that
    // read D10's parenthetical "per-session progress" would actually send is a `session`
    // key, and what it gets back is a subscription to **every** session. That is worth
    // pinning rather than assuming: a client that believed it had a server-side filter
    // would drop other sessions' events on the floor and never know.
    //
    // The session-shaped half of the direction is where it belongs — on the *sweep*, which
    // is the verb that takes a `SessionRef` — and the claim is that its refusal is the
    // sweep's alone: the stream is neither closed nor fed a phantom event.
    let fixture = Fixture::start();
    let ask = fixture.ask();
    let (_, wire) = fixture.wires().into_iter().next().expect("a transport");

    let phantom = uuid::Uuid::now_v7();
    let mut connection = Ws::connect(&fixture.socket).await;
    let opened = connection
        .call(
            "wch_subscribe_calibration",
            json!({ "session": SessionRef::Id { id: phantom } }),
        )
        .await;
    let subscription = opened
        .get("result")
        .unwrap_or_else(|| panic!("a subscription that named a session was refused: {opened}"))
        .clone();

    // Sweeping a session nothing answers to is `IllegalTransition` naming what was asked
    // for (`engine::lifecycle::session_for`: "a lookup that invented an empty session would
    // let a sweep write to a camera with nothing recording it").
    let (_, refused) = refusal(
        wire.calibrate_sweep(
            ask.camera.clone(),
            SessionRef::Id { id: phantom },
            a_short_sweep(&fixture, &ask),
        )
        .await,
    );
    assert_eq!(refused.kind(), ErrorKind::IllegalTransition, "{refused}");
    assert_eq!(
        fixture.wchd.subscriptions().calibration.subscribers,
        1,
        "a refused sweep took a subscription with it"
    );

    // The stream is still every session's, and the first thing it carries is a session this
    // client never named. That ordering is also what says the refused sweep emitted
    // *nothing*: an event for the phantom would have arrived ahead of this one.
    let which = a_planned_session(&wire, &ask, "not the one that was named").await;
    let sweeping = {
        let (wire, camera, request) = (
            wire.clone(),
            ask.camera.clone(),
            a_short_sweep(&fixture, &ask),
        );
        tokio::spawn(async move { wire.calibrate_sweep(camera, which, request).await })
    };
    let mut watching = Watching::Ws {
        connection: Box::new(connection),
        subscription,
    };
    let event: ProgressEvent = watching.next().await.expect("the sweep emitted");
    let swept = sweeping
        .await
        .expect("the sweep task")
        .expect("a control with an ordered range");
    assert_eq!(
        event.session, swept.id,
        "the first event named neither the session that swept nor the one asked for"
    );
    assert_ne!(
        event.session, phantom,
        "a session that does not exist emitted an event"
    );

    still_answers(
        watching.connection(),
        &ask,
        "after a sweep named a session that does not exist",
    )
    .await;
    a_fresh_client_is_served(&fixture, &ask, "after an unknown session was swept").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn one_client_watching_one_sweep_twice_is_told_twice_and_neither_stream_starves_the_other() {
    // **"A client subscribes twice to the same session."** With a per-client stream that is
    // two subscriptions on one connection watching one sweep, and the claim is that the
    // daemon fans out to both rather than binding a source to a subscriber: two ids, two
    // deliveries of the same session's events, and one connection's bookkeeping counting
    // both.
    //
    // It reads the *connection* rather than two `Watching`s, because telling two
    // subscriptions apart on one socket is precisely what a client that subscribed twice
    // has to do — `support/subscribe.rs` says why a `Watching` cannot.
    let fixture = Fixture::start();
    let ask = fixture.ask();
    let (_, wire) = fixture.wires().into_iter().next().expect("a transport");
    let which = a_planned_session(&wire, &ask, "watched twice").await;

    let mut subscribers = fixture.wchd.watch_subscribers();
    let mut connection = Ws::connect(&fixture.socket).await;
    let mut ids = Vec::new();
    for opened in 0..2 {
        let answer = connection
            .call("wch_subscribe_calibration", json!({}))
            .await;
        ids.push(
            answer
                .get("result")
                .unwrap_or_else(|| panic!("subscription {opened} was refused: {answer}"))
                .to_string(),
        );
    }
    assert_ne!(
        ids[0], ids[1],
        "one connection got one id for two subscriptions, which no client could tell apart"
    );
    subscribers
        .wait_for(|live| *live == 2)
        .await
        .expect("the daemon publishes its subscriber count");

    let sweeping = {
        let (wire, camera, request) = (
            wire.clone(),
            ask.camera.clone(),
            a_short_sweep(&fixture, &ask),
        );
        tokio::spawn(async move { wire.calibrate_sweep(camera, which, request).await })
    };

    // Read until each id has carried an event. Every read ends when the daemon writes, so a
    // build that delivered to one subscription and not the other never reaches the assertion
    // below — it is a named TIMEOUT, which is the shape `.config/nextest.toml` gives a wedge.
    let mut seen: BTreeMap<String, ProgressEvent> = BTreeMap::new();
    while seen.len() < ids.len() {
        let params = connection.notification().await;
        let id = params["subscription"].to_string();
        let event: ProgressEvent =
            serde_json::from_value(params["result"].clone()).expect("a notification decodes");
        seen.entry(id).or_insert(event);
    }
    let swept = sweeping
        .await
        .expect("the sweep task")
        .expect("a control with an ordered range");
    for id in &ids {
        let event = seen
            .get(id)
            .unwrap_or_else(|| panic!("subscription {id} was told nothing: {seen:?}"));
        assert_eq!(
            event.session, swept.id,
            "one of a client's two streams carried another session"
        );
    }

    still_answers(&mut connection, &ask, "with one sweep watched twice").await;

    // And both go together when the connection does: a client that subscribed twice costs
    // the daemon two slots and gives back two.
    drop(connection);
    subscribers
        .wait_for(|live| *live == 0)
        .await
        .expect("the daemon publishes its subscriber count");
    a_fresh_client_is_served(&fixture, &ask, "after a client watched one sweep twice").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_subscription_outlives_the_session_it_watched_and_carries_the_next_ones_failure() {
    // **"A subscription outliving its session."** The stream is per client, so a session
    // ending is not a stream ending — and the event that says a sweep is over is *terminal
    // for that control*, never for the subscription. `schema::progress`'s own test warns a
    // consumer against closing on `is_terminal`; this is the transport's half of that
    // warning, and a daemon that ended the stream on it would fail here rather than in a
    // client somebody writes later.
    //
    // The second session **fails**, on purpose: a sweep that is refused reports it as an
    // event (`SweepInterrupted`, carrying the D13 discriminant) rather than by closing the
    // stream, because the sweep's own caller is the one being refused. That is the only
    // place in this suite where a failure and a stream meet.
    let fixture = Fixture::start();
    let ask = fixture.ask();
    let (_, wire) = fixture.wires().into_iter().next().expect("a transport");

    let mut watching = open(&fixture, "uds", "wch_subscribe_calibration", buffer()).await;

    let first = a_planned_session(&wire, &ask, "the session that ends").await;
    let sweeping = {
        let (wire, camera, request) = (
            wire.clone(),
            ask.camera.clone(),
            a_short_sweep(&fixture, &ask),
        );
        tokio::spawn(async move { wire.calibrate_sweep(camera, first, request).await })
    };
    let ended = until_the_sweep_stops(&mut watching).await;
    let finished = sweeping
        .await
        .expect("the sweep task")
        .expect("a control with an ordered range");
    assert_eq!(ended.session, finished.id);
    assert!(
        matches!(ended.progress, CalibrationProgress::SweepFinished { .. }),
        "the first sweep did not finish: {ended:?}"
    );
    assert_eq!(
        fixture.wchd.subscriptions().calibration.subscribers,
        1,
        "a terminal event closed the subscription that saw it"
    );

    // A second session, on the same camera and the same subscription — and one that cannot
    // finish, because the device goes away mid-stream (E3: nothing is coming, which is not
    // a settle timeout).
    let second = a_planned_session(&wire, &ask, "the session after it").await;
    fixture.backend.queue_fault(Fault::DeviceGoneMidStream);
    let (_, refused) = refusal(
        wire.calibrate_sweep(
            ask.camera.clone(),
            second.clone(),
            a_short_sweep(&fixture, &ask),
        )
        .await,
    );

    let interrupted = until_the_sweep_stops(&mut watching).await;
    // Named as the session it belongs to, on a subscription opened before that session
    // existed — which is what "the stream is per client" means at the only place it can be
    // observed. The refusal above is compared with `SessionRef` destructured rather than
    // with the id printed, because the ref is what the verb was given.
    let SessionRef::Id { id: after } = second else {
        panic!("the fixture opened a session by task rather than by id: {second:?}");
    };
    assert_eq!(
        interrupted.session, after,
        "the second session's events arrived under another session's id"
    );
    assert_ne!(
        interrupted.session, finished.id,
        "the session that had already ended emitted the second session's interruption"
    );
    let CalibrationProgress::SweepInterrupted { failure, .. } = interrupted.progress else {
        panic!("a sweep that was refused did not say so on the stream: {interrupted:?}");
    };
    // The discriminant on the event is the one the caller was refused with — compared
    // rather than transcribed, because "a consumer can branch without parsing prose" is only
    // true if the two agree.
    assert_eq!(failure, refused.kind(), "{refused}");

    // One camera fewer than the fixture started with, and *that* camera, which are two
    // assertions rather than an adjustment: the sweep above met `DeviceGoneMidStream`, and a
    // device that vanished mid-stream has left the machine (design D19) — so the daemon's next
    // listing must stop naming it. A fake that answered `DeviceGone` to a frame and went on
    // enumerating the camera would be a shape no machine has, and a consumer's
    // re-enumeration — the thing D19 says a caller does next — would find the camera and carry
    // on. The id is handed in because an arity on its own is equally true of a daemon that
    // dropped the camera nothing happened to.
    let vanished = asked_id(&ask).clone();
    still_answers_about(
        watching.connection(),
        &ask,
        "after two sessions ended under one subscription",
        FIXTURE_CAMERAS - 1,
        Some(&vanished),
    )
    .await;
    a_fresh_client_is_served_about(
        &fixture,
        &ask,
        "after a watched sweep was interrupted",
        FIXTURE_CAMERAS - 1,
        Some(&vanished),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_camera_that_vanished_under_a_photo_tells_the_watchers_it_left() {
    // **D19's watcher sentence, over the wire** — the clause that says a `subscribe_events`
    // watcher gets the removal within the hotplug bounds. The engine twin
    // (`engine/tests/device_loss.rs`) drives it against the backend seam; this drives it
    // through `wch_subscribe_events`, because between the two there is a watch thread, a
    // fan-out and a WebSocket, and a removal that stopped at any of them would leave an agent
    // holding a stale listing with nothing to tell it otherwise (note **N300**).
    //
    // The loss arrives on the verb an agent actually runs: a photo, refused `DeviceGone`, on
    // the camera whose node the notification then names.
    let fixture = Fixture::start();
    let ask = fixture.ask();
    let vanishing = crate::fixture::camera(&fixture.cameras, 0).clone();

    // Subscribed on its own connection, so the notification and the photo's answer are never
    // two frames on one socket racing this test's reader.
    let mut watching = open(&fixture, "ws", "wch_subscribe_events", buffer()).await;
    let mut running = fixture.wchd.watch_hotplug();
    running
        .wait_for(|running| *running)
        .await
        .expect("the daemon publishes whether it is watching");

    fixture.backend.queue_fault(Fault::DeviceGoneMidStream);
    let mut connection = Ws::connect(&fixture.socket).await;
    let request = schema::capture::PhotoRequest {
        stream: schema::capture::StreamRequest::default(),
        settle: schema::capture::SettlePolicy::default(),
        transform: schema::capture::Transform::None,
        sink: schema::capture::Sink::ReturnBytes {
            format: schema::capture::PhotoFormat::Jpeg,
        },
        wait: false,
    };
    let refused = connection
        .call(
            "wch_photo",
            json!({
                "camera": &ask.camera,
                "request": serde_json::to_value(&request).expect("a photo request serializes"),
            }),
        )
        .await;
    assert_eq!(
        refused["error"]["code"],
        json!(rpc_code(ErrorKind::DeviceGone)),
        "a camera that vanished mid-stream answered something else: {refused}"
    );

    // The removals, on the subscription, naming every node that left. One per node, because
    // that is what the kernel emits and what `v4l2::hotplug::Tracker::rescan` turns it into,
    // and because a subscriber that heard about the capture node alone would still be holding
    // a metadata node this machine no longer has (note **N301**). `Watching::next` ends when
    // the daemon writes, so a build that produced no event turns into a named nextest TIMEOUT
    // rather than a hang — the same bound every other delivery claim in this file runs under.
    let mut told: Vec<HotplugEvent> = Vec::new();
    for _ in &vanishing.nodes {
        told.push(
            watching
                .next()
                .await
                .expect("a camera left and a watcher was told about fewer of its nodes"),
        );
    }
    assert_eq!(
        told,
        vanishing
            .nodes
            .iter()
            .map(|node| HotplugEvent::Removed {
                path: node.path.clone()
            })
            .collect::<Vec<HotplugEvent>>(),
        "the watcher was told about the wrong nodes, or the wrong direction: {told:?}"
    );
    assert!(
        vanishing.nodes.len() > 1,
        "the fixture camera owns one node, so 'one removal per node' is untested here"
    );

    // And the listing the watcher would re-read agrees with what it was told, which is the
    // point of the event: the camera the removals named is the camera the listing stopped
    // naming, one fewer than before, and the daemon still answering.
    still_answers_about(
        &mut connection,
        &ask,
        "after a camera vanished under a photo",
        FIXTURE_CAMERAS - 1,
        Some(&vanishing.id),
    )
    .await;
    a_fresh_client_is_served_about(
        &fixture,
        &ask,
        "after a camera vanished under a photo",
        FIXTURE_CAMERAS - 1,
        Some(&vanishing.id),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_that_ends_under_a_watcher_ends_only_the_session() {
    // **"A session that ends while somebody watches."** The whole of D8's arc — sweep,
    // select, apply, restore — run under an open subscription, so that what ends is a
    // session with nothing left to say rather than a sweep that stopped. The claim is the
    // negative one: none of it reaches the stream. The subscriber is still live afterwards,
    // its connection still answers, and the terminal event it received was an event and not
    // a close (which is the direction the sibling test above then drives, by putting a
    // *second* session down the same stream).
    let fixture = Fixture::start();
    let ask = fixture.ask();
    let (_, wire) = fixture.wires().into_iter().next().expect("a transport");

    let mut watching = open(&fixture, "uds", "wch_subscribe_calibration", buffer()).await;
    let which = a_planned_session(&wire, &ask, "ends under a watcher").await;
    let sweeping = {
        let (wire, camera, which, request) = (
            wire.clone(),
            ask.camera.clone(),
            which.clone(),
            a_short_sweep(&fixture, &ask),
        );
        tokio::spawn(async move { wire.calibrate_sweep(camera, which, request).await })
    };
    let ended = until_the_sweep_stops(&mut watching).await;
    let swept = sweeping
        .await
        .expect("the sweep task")
        .expect("a control with an ordered range");
    assert_eq!(ended.session, swept.id);
    assert_eq!(
        ended.progress.control(),
        &ask.control,
        "the terminal event named another control"
    );

    // The rest of the session's life, while the same client watches: a value chosen, the
    // choice written back to the camera, and the pre-sweep snapshot spent (note N23). After
    // this the session has nothing left that D8 can do to it.
    wire.calibrate_select(
        ask.camera.clone(),
        which.clone(),
        ask.control.clone(),
        Selection::ByMetric {
            metric: MetricName::Sharpness,
        },
    )
    .await
    .expect("a control that swept");
    wire.calibrate_apply(ask.camera.clone(), which.clone(), false)
        .await
        .expect("a settled queue");
    wire.calibrate_restore(ask.camera.clone(), which.clone())
        .await
        .expect("the pre-sweep snapshot this session armed");

    assert_eq!(
        fixture.wchd.subscriptions().calibration.subscribers,
        1,
        "the session ending took its watcher with it"
    );
    still_answers(
        watching.connection(),
        &ask,
        "after the watched session ended",
    )
    .await;
    a_fresh_client_is_served(&fixture, &ask, "after a watched session ended").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_connection_that_dies_in_the_middle_of_a_message_takes_only_its_own_subscriptions() {
    // **"A subscriber on a connection that dies mid-message."** Both directions are in
    // flight when the socket goes: the daemon is writing notifications this client has
    // never read — more of them than one connection's buffer holds, so the drop path is
    // live — and the client's last write is *half a JSON-RPC document*, which is what a
    // process that died between two writes leaves in the daemon's reader.
    //
    // A whole WebSocket message carrying a fragment of JSON, rather than a torn frame: the
    // frame layer is soketto's on both sides of this socket (`daemon::uds` mounts
    // jsonrpsee-server's own), so a hand-torn frame would be a test of soketto. What is
    // this daemon's own is what it does with a message it cannot parse from a peer that is
    // already gone.
    let fixture = Fixture::start();
    let ask = fixture.ask();
    let mut subscribers = fixture.wchd.watch_subscribers();

    let mut connection = Ws::connect(&fixture.socket).await;
    let opened = connection.call("wch_subscribe_events", json!({})).await;
    assert!(opened.get("result").is_some(), "{opened}");
    subscribers
        .wait_for(|live| *live == 1)
        .await
        .expect("the daemon publishes its subscriber count");

    // Events this client will never collect, queued before it stops being a client.
    for _ in 0..buffer() + OVERFLOW {
        fixture.backend.queue_fault(Fault::HotplugAdd);
    }
    connection
        .write(r#"{"jsonrpc":"2.0","id":9,"method":"wch_li"#)
        .await;
    drop(connection);

    subscribers
        .wait_for(|live| *live == 0)
        .await
        .expect("the daemon publishes its subscriber count");
    assert_eq!(
        fixture.wchd.subscriptions().hotplug.subscribers,
        0,
        "a subscription outlived the connection that died under it"
    );

    a_fresh_client_is_served(&fixture, &ask, "after a connection died mid-message").await;
}

// ---------------------------------------------- claim 7: the two bounds nothing else reads

#[tokio::test(flavor = "multi_thread")]
async fn a_real_connections_message_buffer_is_the_number_this_project_chose() {
    // **A bound this daemon sets and nothing reads is a bound that silently becomes
    // jsonrpsee's default again** (rubric A8) — which is the objection note **N38** made
    // when P4b turned this surface off rather than inherit two numbers, and the gap the
    // P4e-i review found: the backpressure test above drives `Methods::subscribe`, whose
    // buffer is an *argument*, so it says nothing about what `daemon::uds::serve` configures.
    // `ServerConfig` cannot be read back, so the configured number is observed where it
    // becomes visible — the sink a real connection hands a subscription
    // (`SubscriptionSink::max_capacity`, published as `StreamActivity::buffer`).
    //
    // **Watched red**: `.set_message_buffer_capacity(1024)` — jsonrpsee's own default, the
    // exact regression `http_only()` existed to prevent — passed all 109 daemon tests before
    // this test existed and fails this one in milliseconds.
    let fixture = Fixture::start();
    let ask = fixture.ask();
    assert_eq!(
        fixture.wchd.subscriptions().hotplug.buffer,
        0,
        "a daemon that has accepted no subscription reported a connection's buffer"
    );

    let mut connection = Ws::connect(&fixture.socket).await;
    let opened = connection.call("wch_subscribe_events", json!({})).await;
    let subscription = opened
        .get("result")
        .unwrap_or_else(|| panic!("the upgrade did not carry a subscription: {opened}"))
        .clone();
    // One delivered event is the rendezvous: the buffer is recorded after `accept`, and the
    // accept is what makes the subscription id this test already holds — so reading a
    // notification is the cheapest signal that the handler is past that line. Not a poll.
    fixture.backend.queue_fault(Fault::HotplugAdd);
    let mut watching = Watching::Ws {
        connection: Box::new(connection),
        subscription,
    };
    let _event: HotplugEvent = watching.next().await.expect("the watch delivered");

    assert_eq!(
        fixture.wchd.subscriptions().hotplug.buffer,
        buffer(),
        "a real connection's subscription does not run under the buffer this project chose"
    );
    // Per stream, and per *connection*: the other stream has accepted nothing, and the
    // in-memory dispatch below reports the buffer it was handed rather than the constant —
    // which is what says this field is an observation and not a second copy of `limits`.
    assert_eq!(fixture.wchd.subscriptions().calibration.buffer, 0);

    let elsewhere = buffer() + OVERFLOW;
    let mut memory = open(
        &fixture,
        "in-memory",
        "wch_subscribe_calibration",
        elsewhere,
    )
    .await;
    let (_, wire) = fixture.wires().into_iter().next().expect("a transport");
    let which = a_planned_session(&wire, &ask, "buffered").await;
    let sweeping = {
        let (wire, camera, request) = (
            wire.clone(),
            ask.camera.clone(),
            a_short_sweep(&fixture, &ask),
        );
        tokio::spawn(async move { wire.calibrate_sweep(camera, which, request).await })
    };
    let _progress: ProgressEvent = memory.next().await.expect("the sweep emitted");
    assert_eq!(
        fixture.wchd.subscriptions().calibration.buffer,
        elsewhere,
        "the published buffer is a constant rather than what the connection gave"
    );
    sweeping
        .await
        .expect("the sweep task")
        .expect("a control with an ordered range");

    a_fresh_client_is_served(&fixture, &ask, "after a subscription reported its buffer").await;
}

// ------------------------------- claim 8: the hotplug watch's own two failure directions

#[tokio::test(flavor = "multi_thread")]
async fn a_backend_that_cannot_watch_refuses_the_subscribe_call_rather_than_accepting_it() {
    // `CameraBackend::watch` can fail — a container without `NETLINK_KOBJECT_UEVENT`, an
    // LSM, a backend with no watch to give — and `daemon::events::Hotplug`'s header rests a
    // decision on it: the watch is started **lazily**, so that host answers every other verb
    // instead of refusing to start. That decision is only worth something if the refusal it
    // moves to the subscribe call is a refusal a client can actually see, and until note
    // **N59** nothing in the workspace could reach it — `fake::Fault` had eight variants and
    // none made `watch()` or `next_event` fail.
    //
    // Refused **before** the accept, which is the one moment a subscription can still carry
    // a JSON-RPC code: after it, jsonrpsee has only a `subscription_error` notification,
    // which carries no code at all. So this is a D13 refusal like every other refusal on
    // this surface.
    let fixture = Fixture::start();
    let ask = fixture.ask();
    fixture.backend.queue_fault(Fault::WatchUnavailable);

    let mut connection = Ws::connect(&fixture.socket).await;
    let refused = connection.call("wch_subscribe_events", json!({})).await;
    assert_eq!(
        refused["error"]["code"],
        json!(rpc_code(ErrorKind::DeviceIo)),
        "a host with no watch to give did not refuse with the code the registry gives it: \
         {refused}"
    );
    // Nothing was attached and nothing is running: a refusal that had counted a subscriber
    // would leak one per client on a host that can never watch.
    assert_eq!(fixture.wchd.subscriptions().live, 0, "{refused}");
    assert!(!*fixture.wchd.watch_hotplug().borrow());

    // …and the fault was this host's answer *once*, so the next subscriber gets a watch —
    // which is what makes "the next subscriber starts a fresh watch" a claim rather than a
    // hope, on the same connection the refusal arrived on.
    let opened = connection.call("wch_subscribe_events", json!({})).await;
    assert!(
        opened.get("result").is_some(),
        "a host that refused one watch refused the next: {opened}"
    );
    still_answers(&mut connection, &ask, "after a watch was refused").await;
    a_fresh_client_is_served(&fixture, &ask, "after a watch was refused").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_watch_that_stops_ends_the_streams_reading_it_and_names_why() {
    // **The failure a first draft of `daemon::events` got wrong, and its comment asserted
    // the opposite of** (note **N59**). A watch thread whose `next_event` fails used to log
    // "subscribers were told" and clear its flag — but nothing closed the fan-out, because
    // `broadcast` gives a `Sender` no close and this one lives as long as the daemon. Every
    // open `wch_subscribe_events` stream stayed accepted, stayed counted, and received
    // nothing for the rest of the process's life: rubric A4's shape (a transient failure
    // leaving a client in a state no verb can leave) behind a log line claiming otherwise.
    //
    // Three things are asserted and none implies the others: what the stream already had is
    // delivered *before* the ending, the ending names a reason a client can branch on, and
    // the subscription is reaped rather than merely silent.
    let fixture = Fixture::start();
    let ask = fixture.ask();
    let mut subscribers = fixture.wchd.watch_subscribers();
    let mut running = fixture.wchd.watch_hotplug();

    let mut connection = Ws::connect(&fixture.socket).await;
    let opened = connection.call("wch_subscribe_events", json!({})).await;
    let subscription = opened
        .get("result")
        .unwrap_or_else(|| panic!("the upgrade did not carry a subscription: {opened}"))
        .clone();
    running
        .wait_for(|running| *running)
        .await
        .expect("the daemon publishes whether it is watching");

    // One real event first, and read, so that what follows is a stream that was **working**
    // being ended rather than one that never delivered. The two are scripted in turn rather
    // than together because the fake answers a failed watch before a queued arrival, which
    // is the honest order for a watch that has stopped; that the *fan-out* delivers what it
    // already holds before a terminal is `daemon::events`' own unit test
    // (`a_fresh_fan_out_has_nobody_listening_and_says_so_by_counting`), where both can be
    // put in the queue without a thread between them.
    fixture.backend.queue_fault(Fault::HotplugAdd);

    let delivered = connection.notification().await;
    assert_eq!(delivered["subscription"], subscription, "{delivered}");
    let event: HotplugEvent =
        serde_json::from_value(delivered["result"].clone()).expect("an event decodes");
    assert!(matches!(event, HotplugEvent::Added { .. }), "{event:?}");

    // And now the watch under it stops.
    fixture.backend.queue_fault(Fault::WatchFails);

    // The ending, with its reason. `params.error` rather than `params.result` is jsonrpsee's
    // `subscription_error` notification — the only carrier left after an accept — and the
    // payload is typed rather than prose, so a client branches instead of parsing a
    // sentence. `Ws::ending` is what reads it, so "this stream ended, naming why" is one
    // helper here and in `signals.rs` rather than two spellings of one frame shape.
    assert_eq!(
        connection.ending(&subscription).await,
        json!({ "ended": daemon::events::WATCH_STOPPED }),
        "a stranded subscriber was told nothing"
    );

    // Reaped, not merely silent: the count comes back down and the thread is gone. A build
    // that only logged would sit here for ever, which nextest turns into a named TIMEOUT.
    subscribers
        .wait_for(|live| *live == 0)
        .await
        .expect("the daemon publishes its subscriber count");
    running
        .wait_for(|running| !*running)
        .await
        .expect("the daemon publishes whether it is watching");
    assert_eq!(fixture.wchd.subscriptions().hotplug.subscribers, 0);

    // And the retry the module's comment promises is real: the next subscriber starts a
    // fresh watch and sees events again, on the connection whose stream just ended.
    let reopened = connection.call("wch_subscribe_events", json!({})).await;
    let subscription = reopened
        .get("result")
        .unwrap_or_else(|| {
            panic!(
                "a client could not re-subscribe after a watch failed: \
                                   {reopened}"
            )
        })
        .clone();
    running
        .wait_for(|running| *running)
        .await
        .expect("the daemon publishes whether it is watching");
    fixture.backend.queue_fault(Fault::HotplugRemove);
    let again = connection.notification().await;
    assert_eq!(again["subscription"], subscription, "{again}");
    assert!(
        again.get("result").is_some(),
        "the fresh watch delivered nothing: {again}"
    );

    still_answers(&mut connection, &ask, "after a watch stopped").await;
    a_fresh_client_is_served(&fixture, &ask, "after a watch stopped").await;
}

/// Open `name` on `connection` and hand back the subscription id the daemon assigned.
///
/// The id is what a notification is matched against, and on a duplex connection carrying two
/// subscriptions that matching is the whole assertion — "a stream ended" is not the claim,
/// "*this* stream ended, naming why" is.
async fn subscribed(connection: &mut Ws<tokio::net::UnixStream>, name: &str) -> serde_json::Value {
    let opened = connection.call(name, json!({})).await;
    opened
        .get("result")
        .unwrap_or_else(|| panic!("{name} was refused: {opened}"))
        .clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_daemon_that_is_stopping_ends_every_open_subscription_and_names_why() {
    // **P4e-ii's step 3, from the outside** (`daemon::shutdown`'s header owns the order; this
    // is the half of it a client can see). AGENTS says open streams are "cancelled, never
    // awaited, on shutdown", and *cancelled with a reason* is the part that costs something:
    // a daemon that simply stopped its transport would end both streams below too — with
    // nothing at all, on a socket that closed, indistinguishable from the daemon crashing or
    // the machine losing its network. What the token buys is the payload.
    //
    // Both subscriptions, on **one** connection, because the claim is about the daemon rather
    // than about either stream: one cancellation reaches every open subscription, and the two
    // have different lag policies, different producers and different bodies underneath.
    let fixture = Fixture::start();
    let mut subscribers = fixture.wchd.watch_subscribers();
    let mut running = fixture.wchd.watch_hotplug();

    let mut connection = Ws::connect(&fixture.socket).await;
    let hotplug = subscribed(&mut connection, "wch_subscribe_events").await;
    running
        .wait_for(|running| *running)
        .await
        .expect("the daemon publishes whether it is watching");
    let calibration = subscribed(&mut connection, "wch_subscribe_calibration").await;
    subscribers
        .wait_for(|live| *live == 2)
        .await
        .expect("the daemon publishes its subscriber count");

    // The stop, asked for through the daemon's own token — which is what the shipped
    // `serve_until_stopped` cancels, at a point in its order *before* it stops the transport.
    // Doing it here, with the socket still up, is what makes the assertions below about the
    // cancellation rather than about a connection that went away.
    fixture.wchd.shutdown().cancel();

    for (subscription, stream) in [
        (&hotplug, "wch_subscribe_events"),
        (&calibration, "wch_subscribe_calibration"),
    ] {
        // `params.error` is jsonrpsee's `subscription_error` notification, the only carrier
        // left after an accept, and the payload is typed rather than prose so a client
        // branches instead of parsing a sentence — and branches on *this* reason rather than
        // on `WATCH_STOPPED`, because "re-subscribe now" and "this daemon is going away" are
        // different advice. Each stream is asked for **by id**: the two end in whichever
        // order the runtime gets to them, and `Ws::ending` queues the one that is not being
        // waited for rather than dropping it (which is what makes the second call answer).
        assert_eq!(
            connection.ending(subscription).await,
            json!({ "ended": daemon::events::SHUTTING_DOWN }),
            "{stream} ended without telling its client why"
        );
    }

    // Reaped, not merely silent: both counts come back down, which is what makes the endings
    // above the end of the subscriptions rather than one message on a stream still holding a
    // task and a receiver.
    subscribers
        .wait_for(|live| *live == 0)
        .await
        .expect("the daemon publishes its subscriber count");
    assert_eq!(fixture.wchd.subscriptions().live, 0);

    // And the hotplug **thread** goes down with them, without a token of its own — the
    // argument `daemon::events::Hotplug`'s header makes and P4e-ii deliberately did not
    // replace. Cancelling the subscribers is what ends it: each cancelled subscription drops
    // its `Attached`, that drops the `broadcast::Receiver`, and the thread's next turn — at
    // most one `limits::HOTPLUG_WATCH_DEADLINE_MS` away — finds no readers left. This is the
    // assertion that keeps that paragraph honest now that there is a second way for a
    // subscription to end.
    running
        .wait_for(|running| !*running)
        .await
        .expect("the watch thread put the watch down after its cancelled readers left");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hotplug_watch_runs_only_while_somebody_is_listening() {
    // `daemon::events::Hotplug`'s header makes two claims about a thread — **started by the
    // first subscriber** and **ended when the last one goes** — and until note **N59** the
    // second was a paragraph: `running` was private, `StreamActivity::subscribers` counts
    // receivers rather than threads, and so `limits::HOTPLUG_WATCH_DEADLINE_MS` bounded
    // nothing any test could see (a 3600× change left the whole workspace green, measured).
    //
    // **This is the one test that number participates in.** The thread notices its last
    // reader has gone on the turn the deadline gives it — no fault is scripted here, on
    // purpose, because a scripted wake would end the wait and take the bound out of the
    // experiment — so the wait below is bounded by that constant and by nothing else. It is
    // still not a sleep: what it waits for is the daemon publishing that the thread has
    // left, and a build whose deadline stopped arriving is a named nextest TIMEOUT.
    let fixture = Fixture::start();
    let ask = fixture.ask();
    let mut running = fixture.wchd.watch_hotplug();
    let mut subscribers = fixture.wchd.watch_subscribers();
    assert!(
        !*running.borrow_and_update(),
        "a daemon nobody has subscribed to is already watching"
    );

    let watching = open(&fixture, "uds", "wch_subscribe_events", buffer()).await;
    running
        .wait_for(|running| *running)
        .await
        .expect("the daemon publishes whether it is watching");

    drop(watching);
    subscribers
        .wait_for(|live| *live == 0)
        .await
        .expect("the daemon publishes its subscriber count");
    running
        .wait_for(|running| !*running)
        .await
        .expect("the watch thread put the watch down after its last reader left");

    // And back up for the next one, which is what makes the first half a *lifecycle* rather
    // than a one-way door: a daemon that never watched again would pass every assertion
    // above.
    let _second = open(&fixture, "uds", "wch_subscribe_events", buffer()).await;
    running
        .wait_for(|running| *running)
        .await
        .expect("a fresh subscriber started a fresh watch");

    a_fresh_client_is_served(&fixture, &ask, "after a watch was put down and taken up").await;
}

// -------------------------------- claim 9: a client that floods the daemon's waiting arm

/// How many capture requests are sent past the waiter bound.
///
/// Enough that "some were refused" cannot be a rounding error, and small enough that the
/// answers this test reads back are a handful of frames. Like [`OVERFLOW`] it is this
/// fixture's number rather than `schema::limits`': what the workspace owns is the bound.
const PAST_THE_WAITERS: usize = 4;

#[tokio::test(flavor = "multi_thread")]
async fn a_flood_of_waiting_captures_is_bounded_and_never_the_daemon() {
    // **The hostile direction P4e-i's own transport opened, and the one this suite's story
    // is named against** (note **N59**). D12's `wait: true` parks a *blocking-pool* thread
    // by construction, and until this bound existed the number of those was capped only by
    // tokio's pool: the arithmetic `daemon::server` and note N56 both claimed —
    // "`DAEMON_MAX_CONNECTIONS` against tokio's own pool" — was a fact about HTTP/1.1
    // answering one request per connection, and P4e-i lifted `http_only()` in the same
    // sub-milestone. jsonrpsee's WebSocket transport `tokio::spawn`s a task per inbound
    // message and caps nothing per connection, so **one** connection can hold thousands of
    // captures in flight, and every other verb — `wch_list` included — queues behind them
    // for the same pool.
    //
    // Every wait here is a signal. The gate says a sweep is inside a write; a `Busy` answer
    // says the command queue is full; `Wchd::watch_waiting_captures` says the waiters have
    // arrived. Nothing sleeps and nothing polls.
    //
    // **Watched red** against a hand-applied mutant that widens the permit pool past the
    // flood (`Semaphore::new(4096)`): the requests past the bound park instead of being
    // refused, so the run takes a whole `limits::CAMERA_ENQUEUE_WAIT_MS` to get their
    // answers and then fails at the count — `36` parked where this daemon bounds `32`,
    // measured at 10.01s. The delay is itself the finding: those four requests each held a
    // pool thread for ten seconds, and a client that keeps sending has no bound at all.
    let (entered, entrances) = std::sync::mpsc::sync_channel::<()>(1);
    let (release, tokens) = std::sync::mpsc::sync_channel::<()>(1);
    let gate = Arc::new(Gate {
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
    let ask = fixture.ask();
    let (_, wire) = fixture.wires().into_iter().next().expect("a transport");
    let which = a_planned_session(&wire, &ask, "flooded").await;

    // 1. A sweep occupies the camera's one actor thread, held inside a control write. Every
    //    capture below therefore meets a device that is provably busy rather than one this
    //    test hopes is busy.
    gate.armed.store(true, std::sync::atomic::Ordering::Release);
    let sweeping = {
        let (wire, camera, request) = (
            wire.clone(),
            ask.camera.clone(),
            a_short_sweep(&fixture, &ask),
        );
        tokio::spawn(async move { wire.calibrate_sweep(camera, which, request).await })
    };
    let entrances = tokio::task::spawn_blocking(move || {
        entrances.recv().expect("the sweep reached the device");
        entrances
    })
    .await
    .expect("the blocking pool is alive");

    // 2. Fill the command queue from **one** connection, which is the whole point: this is
    //    the concurrency the WebSocket surface allows and HTTP/1.1 did not.
    //    `CAMERA_COMMAND_QUEUE_DEPTH * 2` requests meet `CAMERA_COMMAND_QUEUE_DEPTH` free
    //    seats — the sweep is running rather than queued — so exactly half are refused, and
    //    reading exactly that many `Busy` answers is what makes "the queue is full" an
    //    observation. It stays full: nothing drains while the gate holds the thread.
    let mut connection = Ws::connect(&fixture.socket).await;
    let seats = limits::CAMERA_COMMAND_QUEUE_DEPTH;
    for _ in 0..seats * 2 {
        capture(&mut connection, &ask, false).await;
    }
    for refused in 0..seats {
        let answer = connection.answer().await;
        assert_eq!(
            answer["error"]["code"],
            json!(rpc_code(ErrorKind::Busy)),
            "refusal {refused} of a full command queue was not D12's: {answer}"
        );
    }

    // 3. Now the flood. Every one of these finds the queue full and parks a pool thread,
    //    and the daemon publishes that they have — which is the rendezvous that makes step 4
    //    about the bound rather than about scheduling.
    let bound = usize::try_from(limits::CAMERA_ENQUEUE_WAITERS).expect("a small bound");
    let mut waiting = fixture.wchd.watch_waiting_captures();
    for _ in 0..bound {
        capture(&mut connection, &ask, true).await;
    }
    waiting
        .wait_for(|parked| *parked == bound)
        .await
        .expect("the daemon publishes how many captures are waiting");

    // 4. Past the bound, on the same connection. Each one is answered **now**, with the
    //    refusal the flag already promised — the flag degrades to its own `false` rather
    //    than growing a nineteenth D13 variant meaning "too many waiters".
    for _ in 0..PAST_THE_WAITERS {
        capture(&mut connection, &ask, true).await;
    }
    for refused in 0..PAST_THE_WAITERS {
        let answer = connection.answer().await;
        assert_eq!(
            answer["error"]["code"],
            json!(rpc_code(ErrorKind::Busy)),
            "waiter {refused} past the bound was not refused the way the flag promises: \
             {answer}"
        );
    }
    assert_eq!(
        *waiting.borrow(),
        bound,
        "more captures were parked than this daemon bounds"
    );

    // 5. **The wedge assertion, and it is a positive one.** The client that did the flooding
    //    is still served on the connection it did it from, and so is one that arrives after
    //    — the two are different claims (see this suite's header), and the second reaches
    //    the daemon through `Wchd::offload`, the very pool the flood was parking threads in.
    still_answers(&mut connection, &ask, "while the waiting arm is full").await;
    a_fresh_client_is_served(&fixture, &ask, "while the waiting arm is full").await;

    // 6. And the sweep the flood was queued behind finished whole, which says the bound
    //    refused callers rather than commands.
    gate.armed
        .store(false, std::sync::atomic::Ordering::Release);
    drop(entrances);
    release.send(()).expect("a write is waiting");
    let swept = sweeping
        .await
        .expect("the sweep task")
        .expect("a control with an ordered range");
    assert_eq!(swept.controls[&ask.control].samples.len(), 2);

    // The waiters take the seats the sweep freed and drain, which is what says step 4
    // refused the ones past the bound and not the ones inside it.
    waiting
        .wait_for(|parked| *parked == 0)
        .await
        .expect("the daemon publishes how many captures are waiting");
    drop(connection);
}

/// One `wch_photo` on this connection, with D12's flag set to `wait`, and no answer read.
///
/// Written and not awaited, because what this suite is about is a client with many requests
/// in flight at once — a helper that read an answer could not express the case, since what
/// makes it a case is that nobody is reading. The request is built from the committed
/// `schema::capture::PhotoRequest` rather than typed as JSON, so a field that moves moves
/// this with it.
async fn capture(connection: &mut Ws<tokio::net::UnixStream>, ask: &Ask, wait: bool) {
    let request = schema::capture::PhotoRequest {
        stream: schema::capture::StreamRequest::default(),
        settle: schema::capture::SettlePolicy::default(),
        transform: schema::capture::Transform::None,
        // Nothing on disk: a flood of captures to a path would be a test of the filesystem.
        sink: schema::capture::Sink::ReturnBytes {
            format: schema::capture::PhotoFormat::Jpeg,
        },
        wait,
    };
    let params = json!({
        "camera": &ask.camera,
        "request": serde_json::to_value(&request).expect("a photo request serializes"),
    });
    let message = json!({
        "jsonrpc": "2.0",
        // One id for every request in a flood: this connection never matches an answer to a
        // request, and a counter here would be a number nothing reads.
        "id": 0,
        "method": "wch_photo",
        "params": params,
    });
    connection.write(&message.to_string()).await;
}
