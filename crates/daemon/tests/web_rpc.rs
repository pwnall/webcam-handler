//! The wire surface over the web listener: a real TCP socket, a real WebSocket upgrade, and
//! the *same* `Methods` value the Unix socket serves (design **D10**, **D11**; docs/7 P5b).
//!
//! `http.rs` is this suite's neighbour and they divide the transport between them.
//! That file's subject is the **credential and the matrix** — 401-without, 200-with, one test
//! per D11 cell — and it hands the listener a surface with no methods on it, because none of
//! its claims would be made truer by one. This file's subject is **what the listener carries**,
//! so it needs a real daemon, and it gets the one every other suite in this crate uses
//! (`support/fixture.rs`): two replayed cameras, a session store, a Unix socket, and the
//! `Methods` value `daemon::server::mount` produced. The web listener here is opened over
//! *that* value, exactly as `main.rs` opens it over the value it handed `uds::serve`.
//!
//! ## The claim this file exists for, and why it is a comparison
//!
//! docs/7 P5b asks for "the WS endpoint speaking the same JSON-RPC (the T5 trait has one
//! home; WS is a transport)". A file can say that; what it cannot do is *notice* when it
//! stops being true, because a second registration would answer every question a suite asked
//! it — with the same handlers, over the same engine, in the same process — right up until
//! the two drifted. So the claim is made two ways, both derived and neither a list:
//!
//! 1. **The same call answers the same thing**, asked over the TCP WebSocket and over the
//!    Unix socket in one test, with the answers compared as values
//!    ([`a_call_over_the_tcp_websocket_answers_what_the_unix_socket_answers`]).
//! 2. **Every name the daemon registers is reachable over the TCP WebSocket**, with the
//!    population read off `Fixture::methods.method_names()` — the registration itself — and
//!    the failure being `-32601`, which is the *only* thing a second and smaller registration
//!    could answer ([`no_method_the_daemon_registers_is_missing_from_the_tcp_websocket`]).
//!
//! Neither implies the other. A build with two registrations that happened to agree passes
//! (1) and fails (2) the moment they differ by one method; a build that mounted the right
//! surface on the wrong path fails both, at the handshake.
//!
//! `method_surface.rs` is the third leg and stays where it is: it walks the call surface with
//! the *generated client* over two transports and is the file that says nineteen. What this
//! file adds is that a third transport reaches the same registration — and its limit 5 names
//! this suite, because "there is no second registration path" is a claim about the whole
//! process rather than about one file.
//!
//! ## What else is here, and why each of them is not in `http.rs`
//!
//! - **The gate on an upgrade.** A `new WebSocket(url)` cannot set headers, so the credential
//!   is `?token=` (note **N74**, `daemon::http::rpc`) — and an anonymous upgrade, a near-miss
//!   token and the real token are three answers only a real handshake can distinguish, since
//!   soketto reports a refusal as a *status* where an HTTP client reports it as a response.
//!   The full refusal is read once beside it, through `support/tcp.rs`, because a status alone
//!   would not say the challenge and the body are the daemon's.
//! - **Subscriptions.** P4e-i built them and bounded them on a transport with no gate in front
//!   of it. That they arrive over this one is a property of mounting jsonrpsee's service and
//!   is therefore somebody else's crate's promise, which is exactly the kind of claim this
//!   project asserts rather than states.
//! - **The stop.** `http.rs` asserts that the *port* stops answering. This file asserts the
//!   thing that costs something: an open subscription is **ended with a reason**
//!   (`daemon::events::SHUTTING_DOWN`) rather than dropped when the socket closes — which is
//!   `crate::shutdown`'s step 3 before step 4, one transport along, and the reason
//!   `daemon::http::Serving::stopped` stops the WebSocket connections where it does.
//! - **The bound.** `limits::RPC_MAX_SUBSCRIPTIONS_PER_CONNECTION`, read rather than
//!   transcribed, over a TCP connection — because the `ConnectionGuard` and the
//!   `BoundedSubscriptions` behind it are *per builder*, and this transport has a builder of
//!   its own.
//!
//! ## Nothing here waits on a clock
//!
//! Every read ends when the daemon writes: `Ws::frame` on a real frame, `Ws::ending` on the
//! notification that ends a stream, `tcp::get` at end-of-file. A daemon that never answered
//! turns each of them into a nextest `TIMEOUT` with a test's name on it rather than a hang
//! (`.config/nextest.toml`), which is the shape "the daemon did not wedge" can actually have.

#[path = "support/fixture.rs"]
mod fixture;
mod support;
#[path = "support/tcp.rs"]
mod tcp;
#[path = "support/wire.rs"]
mod wire;
#[path = "support/ws.rs"]
mod ws;

use std::collections::BTreeSet;
use std::net::SocketAddr;

use api::WchRpcClient;
use api::rpc_code;
use daemon::http::{self, RPC_PATH, TOKEN_QUERY_PARAM};
use schema::error::ErrorKind;
use schema::limits;
use schema::progress::{CalibrationProgress, ProgressEvent};
use schema::session::{Session, SessionRef, SweepRequest, SweepSpec};
use serde_json::{Value, json};
use tokio::net::TcpStream;

use crate::fixture::{Ask, Fixture};
use crate::tcp::Answer;
use crate::wire::refusal;
use crate::ws::Ws;

// ------------------------------------------------------------------- opening the listener

/// The daemon's own web listener, over the surface the daemon already serves on `AF_UNIX`.
///
/// Two things are load-bearing and both are what `main.rs` does. The `Methods` value is
/// **cloned from the fixture's**, not built here — a clone is an `Arc` bump, so the two
/// transports answer out of one registration and (2) in this file's header is a claim about
/// this process rather than about two agreeing constructors. And the [`Shutdown`] token is the
/// **daemon's**, reached through `Wchd::shutdown`, because the whole of the stop claim below
/// is that one cancellation reaches an open subscription *and* the transport under it.
async fn serving(fixture: &Fixture) -> http::Serving {
    http::open(
        address("127.0.0.1:0"),
        false,
        fixture.methods.clone(),
        // The daemon's own fan-out, for the same reason the `Methods` value is the daemon's:
        // this listener is the one `main` opens, and a preview route over a second fan-out
        // would be a second answer to which cameras are being previewed (docs/7 P5b).
        fixture.wchd.previews(),
        fixture.wchd.shutdown().clone(),
    )
    .await
    .expect("loopback on an ephemeral port")
}

/// An address this file wrote, as a value.
fn address(text: &str) -> SocketAddr {
    text.parse().expect("a socket address the tests wrote")
}

/// The token out of the URL the daemon printed.
///
/// `http.rs`'s reason, and it is the one that matters most on this transport: a build that
/// published one secret and gated on another would pass every assertion in this file if the
/// tests reached into the `Token` value instead. There is no `None` arm because every listener
/// this suite opens is in D11's default cell.
fn secret(serving: &http::Serving) -> String {
    let url = serving.ready_to_open_url();
    url.split_once(&format!("?{TOKEN_QUERY_PARAM}="))
        .map(|(_url, token)| token.to_owned())
        .expect("D11's default cell publishes a token")
}

/// The request target a browser's `new WebSocket(url)` would carry.
///
/// The path from `daemon::http::RPC_PATH` and the credential in the query string, which is the
/// only place a WebSocket constructor can put one — that module's header is where what it
/// costs is written down.
fn wire_target(token: &str) -> String {
    format!("{RPC_PATH}?{TOKEN_QUERY_PARAM}={token}")
}

/// Open one WebSocket to the web listener at `target`, and answer what the daemon said.
///
/// `Err` is the status a refused upgrade came back with, which on this transport is a real
/// answer rather than a failure — see [`an_anonymous_upgrade_is_refused_and_so_is_a_near_miss`].
async fn upgrade(serving: &http::Serving, target: &str) -> Result<Ws<TcpStream>, u16> {
    let bound = serving.bound();
    let stream = TcpStream::connect(bound).await.expect("the listener is up");
    Ws::upgrade(stream, &bound.to_string(), target).await
}

/// The same, for a test that has already established the credential works.
async fn connected(serving: &http::Serving, token: &str) -> Ws<TcpStream> {
    upgrade(serving, &wire_target(token))
        .await
        .unwrap_or_else(|status| panic!("the daemon refused a credentialled upgrade: {status}"))
}

/// Stop both transports the way the daemon stops them, and wait for both.
///
/// The order is `main`'s: cancel the one token, join the web listener — which is where the
/// WebSocket connections are ended (`daemon::http::Serving::stopped`) — and then stop and
/// drain the Unix socket. Every test ends here, so a build that could not stop with a client
/// attached becomes a named `TIMEOUT` in whichever test left one attached rather than a
/// process nobody could explain.
async fn stop(mut fixture: Fixture, web: http::Serving) {
    fixture.wchd.shutdown().cancel();
    web.stopped().await.expect("the web listener ended");
    fixture.handle.stop();
    fixture
        .handle
        .stopped()
        .await
        .expect("the Unix socket was asked to stop");
}

/// The `result` of a JSON-RPC answer, or a failure naming what the daemon said instead.
fn result(answer: &Value, about: &str) -> Value {
    answer
        .get("result")
        .unwrap_or_else(|| panic!("{about} was refused: {answer}"))
        .clone()
}

// --------------------------------------------------------- claim 1: the same home, compared

#[tokio::test(flavor = "multi_thread")]
async fn a_call_over_the_tcp_websocket_answers_what_the_unix_socket_answers() {
    // **The headline, as a comparison rather than a sentence.** One daemon, one registration,
    // two byte streams: the same two calls are made over a WebSocket on the TCP listener and
    // over a WebSocket on the Unix socket, and the answers are compared as values.
    //
    // Two calls and not one, because they fail differently. `wch_list` reaches the backend and
    // would still agree between two registrations that shared a `Wchd`; `wch_controls` carries
    // a camera *parameter*, so it also says the two transports decode named parameters the
    // same way — which is the half a transport can get wrong on its own, since one of them
    // arrived through axum's router and the other did not.
    //
    // The client is the same type on both sides (`support/ws.rs`), which is the point: a
    // second frame reader for TCP would make this a comparison between two test clients.
    let fixture = Fixture::start();
    let ask = fixture.ask();
    let web = serving(&fixture).await;
    let token = secret(&web);

    let mut over_tcp = connected(&web, &token).await;
    let mut over_unix = Ws::connect(&fixture.socket).await;

    for (method, params) in [
        ("wch_list", json!({})),
        ("wch_controls", json!({ "camera": &ask.camera })),
    ] {
        let tcp = over_tcp.call(method, params.clone()).await;
        let unix = over_unix.call(method, params).await;
        assert_eq!(
            result(&tcp, method),
            result(&unix, method),
            "{method} answered two different things over two transports"
        );
    }

    // Non-vacuity: the thing being compared has content. Two identical refusals — or two
    // identical empty answers out of a fixture that had degraded — would satisfy the loop
    // above, and the count comes off the fixture rather than a literal.
    let listed = result(&over_tcp.call("wch_list", json!({})).await, "wch_list");
    assert_eq!(
        listed["cameras"].as_array().map(Vec::len),
        Some(fixture.cameras.len()),
        "the daemon listed something other than the cameras it replays: {listed}"
    );

    stop(fixture, web).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn no_method_the_daemon_registers_is_missing_from_the_tcp_websocket() {
    // **The derived half of D10's claim on this transport.** The population is
    // `Methods::method_names()`, read off the value `daemon::server::mount` produced — the
    // registration itself, never a list this file wrote — and every one of them is *named* on
    // a real TCP WebSocket. The failure being watched for is `-32601`, `MethodNotFound`, which
    // is the one answer a second and smaller registration behind this route could give.
    //
    // Every call is made with empty parameters on purpose. What is asserted is that the method
    // **exists here**, and a method that exists answers `-32602` (`InvalidParams`) to a request
    // with nothing in it, while a method that does not exist answers `-32601` whatever it is
    // sent. Depth belongs to `read_verbs.rs`, `mutating_verbs.rs` and `calibrate_verbs.rs`,
    // which drive the same daemon through the generated client; what belongs here is that the
    // name reached a registration at all.
    //
    // Nothing destructive can happen from it: every verb that touches a camera or signals a
    // process needs a parameter it is not being given, so the ones that actually run are the
    // ones that take none.
    let fixture = Fixture::start();
    let web = serving(&fixture).await;
    let token = secret(&web);
    let mut connection = connected(&web, &token).await;

    let registered: BTreeSet<String> = fixture.methods.method_names().map(str::to_owned).collect();
    // Derived on both sides, so a twentieth method or a third subscription lands here as a
    // failure rather than as a silence: `ROUTED` is the daemon's pinned call spelling and
    // `routed_subscriptions` is built from `api::SUBSCRIPTIONS`.
    assert_eq!(
        registered.len(),
        daemon::server::ROUTED.len() + daemon::server::routed_subscriptions().len(),
        "the registration this suite is walking is not the one the daemon pins: {registered:?}"
    );

    let mut missing = Vec::new();
    for method in &registered {
        let answer = connection.call(method, json!({})).await;
        if answer["error"]["code"] == json!(-32601) {
            missing.push(method.clone());
        }
    }
    assert!(
        missing.is_empty(),
        "the TCP WebSocket answers MethodNotFound for methods this daemon registers: \
         {missing:?} of {registered:?}"
    );

    stop(fixture, web).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_typed_refusals_are_the_same_three_over_every_transport() {
    // The other side of "one home": a surface reached three ways refuses the same three
    // absences with the same three D13 codes. `method_surface.rs`'s `discriminating_refusals`
    // is this shape over the two transports the generated client can drive; this adds the
    // third and puts them beside each other, so a WebSocket route that had acquired a
    // refusal vocabulary of its own — an HTTP status where a D13 code belongs, say — fails
    // here rather than in a browser.
    //
    // Three different codes rather than one repeated, for `method_surface.rs`'s reason: a
    // daemon that had degraded into one blanket refusal would satisfy "it refused".
    let fixture = Fixture::start();
    let ask = fixture.ask();
    let web = serving(&fixture).await;
    let token = secret(&web);
    let mut connection = connected(&web, &token).await;

    let want = [
        ErrorKind::CameraUnknown,
        ErrorKind::CameraAmbiguous,
        ErrorKind::ControlUnknown,
    ];

    // Over the TCP WebSocket, as codes on the wire.
    let over_tcp = [
        connection
            .call("wch_info", json!({ "camera": &ask.unknown_camera }))
            .await,
        connection
            .call("wch_info", json!({ "camera": &ask.ambiguous }))
            .await,
        connection
            .call(
                "wch_get",
                json!({ "camera": &ask.camera, "control": &ask.unknown_control }),
            )
            .await,
    ];
    for (answered, kind) in over_tcp.iter().zip(want) {
        assert_eq!(
            answered["error"]["code"],
            json!(rpc_code(kind)),
            "over the TCP WebSocket: {answered}"
        );
    }

    // …and over the two transports the generated client drives, through the same D13 registry
    // rather than through a code transcribed here.
    for (named_for, transport) in fixture.wires() {
        let (unknown_code, unknown) = refusal(transport.info(ask.unknown_camera.clone()).await);
        let (ambiguous_code, ambiguous) = refusal(transport.info(ask.ambiguous.clone()).await);
        let (control_code, control) = refusal(
            transport
                .get(ask.camera.clone(), ask.unknown_control.clone())
                .await,
        );

        assert_eq!(
            [unknown.kind(), ambiguous.kind(), control.kind()],
            want,
            "{named_for}: {unknown}; {ambiguous}; {control}"
        );
        assert_eq!(
            [unknown_code, ambiguous_code, control_code],
            [
                over_tcp[0]["error"]["code"].as_i64().unwrap_or_default() as i32,
                over_tcp[1]["error"]["code"].as_i64().unwrap_or_default() as i32,
                over_tcp[2]["error"]["code"].as_i64().unwrap_or_default() as i32,
            ],
            "{named_for}: a transport refused with a code the WebSocket did not"
        );
    }

    stop(fixture, web).await;
}

// -------------------------------------------------------------- claim 2: the gate is in front

#[tokio::test(flavor = "multi_thread")]
async fn an_anonymous_upgrade_is_refused_and_so_is_a_near_miss() {
    // **D11's gate, on the request that carries a camera's whole control surface.** An
    // upgrade is an ordinary HTTP request, so it meets `daemon::http::gate` before it meets
    // the router — but "so it does" is a composition argument, and this is the socket.
    //
    // Three answers, and the third is what makes the first two about the credential rather
    // than about a route that refuses everything: nothing is a `401`, an equal-length
    // one-digit-different token is a `401`, and the token the daemon printed is a WebSocket.
    // The near miss is the only candidate that reaches `Token::verify`'s comparison loop at
    // all; everything shorter or longer is refused on the length.
    let fixture = Fixture::start();
    let web = serving(&fixture).await;
    let token = secret(&web);

    let anonymous = upgrade(&web, RPC_PATH)
        .await
        .err()
        .expect("an anonymous WebSocket was upgraded onto a live camera");
    assert_eq!(anonymous, 401, "an anonymous upgrade");

    let near_miss = upgrade(&web, &wire_target(&near_miss(&token)))
        .await
        .err()
        .expect("a one-digit-wrong token opened a WebSocket");
    assert_eq!(near_miss, 401, "a near-miss token");

    // …and the credential the daemon published does open one, so the two refusals above are
    // about the digits.
    let mut opened = connected(&web, &token).await;
    let listed = opened.call("wch_list", json!({})).await;
    assert!(listed.get("result").is_some(), "{listed}");

    // The refusal in full, once, through the HTTP client — because a status code is all
    // soketto reports and the three claims a 401 has to carry are not implied by it: the
    // status a client branches on, RFC 6750's challenge, and a body that is the daemon's own
    // sentence rather than a rendering of something. The target is the wire route's, so this
    // is the same refusal on the same path.
    let refused: Answer = tcp::get(web.bound(), RPC_PATH, &[])
        .await
        .expect("the listener is up");
    assert_eq!(refused.status(), 401);
    assert_eq!(
        refused.header("WWW-Authenticate"),
        Some(daemon::http::gate::BEARER_CHALLENGE)
    );
    assert_eq!(refused.body(), daemon::http::gate::REFUSAL);

    stop(fixture, web).await;
}

/// An equal-length, one-digit-different token.
///
/// `http.rs` carries the same helper for the same reason and neither is the other's copy: what
/// they share is a *fact about the token* — 64 hex digits — and the two suites present it to
/// two different mechanisms.
fn near_miss(secret: &str) -> String {
    let mut digits: Vec<char> = secret.chars().collect();
    let first = digits.first_mut().expect("a token is not empty");
    *first = if *first == '0' { '1' } else { '0' };
    digits.into_iter().collect()
}

// ------------------------------------------------------------- claim 3: subscriptions arrive

#[tokio::test(flavor = "multi_thread")]
async fn a_subscription_delivers_over_the_tcp_websocket() {
    // **P4e-i's subscriptions, over P5b's transport.** jsonrpsee builds
    // `RpcServiceCfg::OnlyCalls` on its HTTP path and `CallsAndSubscriptions` on the upgrade
    // path, so "a browser gets the subscriptions for free" is a claim about somebody else's
    // crate — asserted here with a real sweep and a real event rather than stated in a header.
    //
    // The sweep is put on the wire with `Ws::write` and its answer collected much later with
    // `Ws::answer`, which is the only shape in which the notifications and the answer are in
    // flight together: a helper that waited for the answer first would be testing a
    // request/response pipe and calling it duplex.
    let fixture = Fixture::start();
    let ask = fixture.ask();
    let web = serving(&fixture).await;
    let token = secret(&web);
    let mut connection = connected(&web, &token).await;

    let subscription = result(
        &connection
            .call("wch_subscribe_calibration", json!({}))
            .await,
        "wch_subscribe_calibration",
    );

    // A session with one control queued, opened over the same connection — so this test also
    // establishes that a connection carrying a subscription still answers calls.
    let opened: Session = serde_json::from_value(result(
        &connection
            .call(
                "wch_calibrate_start",
                json!({
                    "camera": &ask.camera,
                    "task": SWEEP_TASK,
                    "goal": "a legible frame",
                    "criteria": [],
                }),
            )
            .await,
        "wch_calibrate_start",
    ))
    .expect("a session");
    let which = SessionRef::Id { id: opened.id };
    result(
        &connection
            .call(
                "wch_calibrate_plan",
                json!({
                    "camera": &ask.camera,
                    "session": &which,
                    "controls": [&ask.control],
                    "order": false,
                }),
            )
            .await,
        "wch_calibrate_plan",
    );

    // In flight and unanswered.
    connection
        .write(
            &json!({
                "jsonrpc": "2.0",
                "id": SWEEP_REQUEST_ID,
                "method": "wch_calibrate_sweep",
                "params": {
                    "camera": &ask.camera,
                    "session": &which,
                    "request": a_short_sweep(&fixture, &ask),
                },
            })
            .to_string(),
        )
        .await;

    // The events, decoded as the typed vocabulary a client branches on rather than as JSON
    // this file recognises the shape of. A stream that never delivered turns this into a
    // named nextest TIMEOUT rather than a hang.
    let mut seen = Vec::new();
    let ended = loop {
        let params = connection.notification().await;
        assert_eq!(
            params["subscription"], subscription,
            "a notification for somebody else's stream: {params}"
        );
        let event: ProgressEvent =
            serde_json::from_value(params["result"].clone()).expect("a progress event");
        let terminal = event.progress.is_terminal();
        seen.push(event);
        if terminal {
            break seen.last().expect("the event just pushed").clone();
        }
    };

    // What arrived is *this* sweep's, and it started before it ended: a stream that delivered
    // one terminal event and nothing else would satisfy a weaker assertion and would mean the
    // fan-out had lost the middle of the sweep.
    assert!(
        matches!(
            seen.first().map(|event| &event.progress),
            Some(CalibrationProgress::SweepStarted { .. })
        ),
        "the first event was not the start of the sweep: {seen:?}"
    );
    assert_eq!(ended.session, opened.id, "{ended:?}");
    assert!(
        seen.len() > 1,
        "a sweep that emitted only its ending: {seen:?}"
    );

    // And the call that produced them was answered on the same connection.
    let answered = connection.answer().await;
    assert_eq!(
        answered["id"],
        json!(SWEEP_REQUEST_ID),
        "the answer belonged to another request: {answered}"
    );
    let swept: Session =
        serde_json::from_value(result(&answered, "wch_calibrate_sweep")).expect("a session");
    assert_eq!(
        swept.controls[&ask.control].samples.len(),
        2,
        "one sample per swept value: {swept:?}"
    );

    // The store half of "it really ran", read through the function the daemon answers
    // `calibrate_list` from — the same claim from the other side of the socket. The fixture
    // writes one session per camera, and this test opened one more.
    let sessions = engine::lifecycle::list(&fixture.store, None).expect("the store walks");
    assert_eq!(
        sessions.sessions.len(),
        fixture.cameras.len() + 1,
        "{sessions:?}"
    );

    stop(fixture, web).await;
}

/// The task this suite's one session is opened under.
///
/// Named rather than borrowed from the fixture's, because D9 keys a session slot on (camera
/// fingerprint, task) and the fixture already owns a session under its own name — opening a
/// second under the same one is `SessionConflict` (note N14).
const SWEEP_TASK: &str = "p5b sweep over the web listener";

/// The id the sweep request carries, out of the way of `Ws::call`'s own counter.
///
/// `Ws::write` does not allocate an id and `Ws::answer` matches none, so this is what says the
/// frame collected at the end belonged to the sweep and not to something else in flight.
const SWEEP_REQUEST_ID: u32 = 9_001;

/// A sweep of two values on the fixture's writable control.
///
/// Two, because the claim is that events arrive rather than how many — the same choice
/// `subscriptions.rs` makes, and for the same reason: a sweep is minutes of camera time on
/// real hardware and this suite runs against a replayed profile.
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

// ----------------------------------------------------------------- claim 4: the stop, with a reason

#[tokio::test(flavor = "multi_thread")]
async fn a_daemon_that_is_stopping_ends_a_tcp_subscription_and_names_why() {
    // **`crate::shutdown`'s step 3, seen from a browser.** AGENTS says open streams are
    // "cancelled, never awaited, on shutdown", and *cancelled with a reason* is the half that
    // costs something: a listener that simply stopped would end this stream too — with nothing
    // at all, on a socket that closed, indistinguishable from the daemon crashing.
    //
    // It is also the assertion that pins **when** the WebSocket transport is stopped.
    // `daemon::http::Serving::stopped` asks jsonrpsee's `ServerHandle` rather than wiring it
    // to the cancellation, precisely so the reason below is written before the connection
    // goes; a build that stopped the two together would race, and this test is where it would
    // lose — the read finds a closed socket rather than a payload.
    let fixture = Fixture::start();
    let web = serving(&fixture).await;
    let token = secret(&web);
    let mut connection = connected(&web, &token).await;
    let mut subscribers = fixture.wchd.watch_subscribers();

    let subscription = result(
        &connection
            .call("wch_subscribe_calibration", json!({}))
            .await,
        "wch_subscribe_calibration",
    );
    subscribers
        .wait_for(|live| *live == 1)
        .await
        .expect("the daemon publishes its subscriber count");

    // The stop, asked for through the daemon's own token — the one the shipped
    // `serve_until_stopped` cancels at a point in its order *before* the transports are
    // stopped. Doing it with the socket still up is what makes the assertion below about the
    // cancellation rather than about a connection that went away.
    fixture.wchd.shutdown().cancel();

    assert_eq!(
        connection.ending(&subscription).await,
        json!({ "ended": daemon::events::SHUTTING_DOWN }),
        "a stream over the web listener ended without telling its client why"
    );

    // Reaped, not merely silent: the count comes back down, which is what makes the ending
    // above the end of the subscription rather than one message on a stream still holding a
    // task and a receiver.
    subscribers
        .wait_for(|live| *live == 0)
        .await
        .expect("the daemon publishes its subscriber count");

    stop(fixture, web).await;
}

// --------------------------------------------------------------- claim 5: the bound is read

#[tokio::test(flavor = "multi_thread")]
async fn one_tcp_connection_may_open_only_the_subscriptions_this_project_bounds() {
    // `limits::RPC_MAX_SUBSCRIPTIONS_PER_CONNECTION`, **read** rather than transcribed, on the
    // transport that reaches it through `daemon::server::wire_bounds` — the one function both
    // transports build their `ServerConfig` from since P5b. A build whose web listener took
    // jsonrpsee's own defaults would let 1024 subscriptions onto one browser connection and
    // fail here at the ninth.
    //
    // The refusal is a `-32006` **answer to the subscribe call**, before any handler runs, so
    // connect-and-abandon costs a client its own slots and nobody else's — which is why the
    // second connection below is part of the claim rather than a courtesy. -32006 is
    // jsonrpsee's own code; D13's block starts at -32012 (`api::codes`), so there is no
    // collision to reason about.
    let fixture = Fixture::start();
    let web = serving(&fixture).await;
    let token = secret(&web);
    let bound = usize::try_from(limits::RPC_MAX_SUBSCRIPTIONS_PER_CONNECTION).expect("small");

    let mut connection = connected(&web, &token).await;
    for opened in 0..bound {
        let answer = connection
            .call("wch_subscribe_calibration", json!({}))
            .await;
        assert!(
            answer.get("result").is_some(),
            "subscription {opened} of the bound was refused over TCP: {answer}"
        );
    }
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
        "a browser connection opened more subscriptions than this project bounds: {over}"
    );

    // …and a second connection is unaffected, which is the half that says the bound is per
    // connection rather than per daemon — and, on this transport, that the `ConnectionGuard`
    // the same builder hands out did not refuse it either.
    let mut second = connected(&web, &token).await;
    let answer = second.call("wch_subscribe_calibration", json!({})).await;
    assert!(
        answer.get("result").is_some(),
        "one browser's bound refused another: {answer}"
    );

    // Both connections still answer a verb: the refusal cost the first client one subscription
    // and neither client a socket.
    for (connection, about) in [
        (&mut connection, "past the subscription bound"),
        (&mut second, "beside a client past the bound"),
    ] {
        let listed = connection.call("wch_list", json!({})).await;
        assert!(listed.get("result").is_some(), "{about}: {listed}");
    }

    stop(fixture, web).await;
}
