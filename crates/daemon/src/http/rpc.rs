//! The WebSocket JSON-RPC endpoint: the *same* T5 surface the Unix socket serves, reached
//! over the opt-in TCP listener (design **D10**, **D11**, §2.6; docs/7 P5b).
//!
//! docs/7 states the whole constraint in one line — "the WS endpoint speaking the same
//! JSON-RPC (the T5 trait has one home; WS is a transport)" — and everything below is what it
//! takes to mean that literally.
//!
//! ## The mechanism, and why it is not a dispatcher
//!
//! `mount` is handed a [`Methods`] **value** and stores it. It builds none, registers
//! nothing, and declares no method name: the value it holds is the one
//! [`crate::server::mount`] produced from the two generated `into_rpc()`s, cloned — and a
//! `Methods` clone is an `Arc` bump, so "the same registration" is a fact about a pointer
//! rather than about two call sites agreeing. That is D10's "one wire surface, one home"
//! spent on a second transport instead of copied for one, and it is the property
//! `crates/daemon/tests/web_rpc.rs` asserts by walking `method_names()` off the registration
//! and requiring that **none** of them answers `-32601` over this route.
//!
//! What actually answers a frame is jsonrpsee's own tower service —
//! `Server::builder().set_config(…).to_service_builder()`, then `build(methods, stop_handle)`
//! per connection — which is the same five names [`crate::uds`] uses and the same
//! `RpcServiceCfg::CallsAndSubscriptions` an upgrade gets there. The route is four lines:
//! take the request axum matched, hand it to that service, hand the answer back. There is no
//! frame loop here, no `Message` type, no method `match`, and nothing that could disagree
//! with `POST /` on the Unix socket about what a JSON-RPC request means.
//!
//! **Why jsonrpsee's upgrade path rather than axum's `ws` feature.** `axum::extract::ws`
//! gives a handler an upgraded socket and axum's own `Message` type — a *frame layer*, and a
//! frame layer under a JSON-RPC endpoint is the second dispatcher D10 exists to prevent: some
//! code would then have to read a text frame, decide whether it is a call, a batch, a
//! notification or an `unsubscribe`, route it, and invent a subscription sink. jsonrpsee
//! already has all of that, keyed to the registration this daemon mounts. So the feature
//! stays off (this crate's manifest says so), and the upgrade is jsonrpsee's: its service
//! reads the handshake with soketto, takes hyper's `OnUpgrade` extension out of the request
//! axum handed it, and spawns the connection task itself. `axum::serve` performs the handoff
//! for it — every connection is served with `serve_connection_with_upgrades`, which is not
//! behind a feature — so the only axum-shaped thing here is a route.
//!
//! ## Subscriptions come with the transport, and that is asserted rather than assumed
//!
//! jsonrpsee's HTTP path builds `RpcServiceCfg::OnlyCalls`, so a `wch_subscribe_*` over a
//! plain `POST` is answered `-32603`; the **upgrade** path builds
//! `CallsAndSubscriptions` with a `BoundedSubscriptions` and a `MethodSink`. Mounting the
//! service therefore gives a browser the two subscriptions P4e-i built, on the same
//! `crate::events` fan-out the Unix socket's subscribers sit on, for no extra code — but "for
//! free" is a claim about somebody else's crate, so `tests/web_rpc.rs` opens one over a real
//! TCP WebSocket and reads a real event out of it rather than saying so here.
//!
//! ## What this route answers besides an upgrade
//!
//! A plain `POST` with `content-type: application/json`, because `enable_http` is on and this
//! is jsonrpsee's whole service. That is a **consequence of mounting it** rather than a
//! decision, exactly as [`crate::uds`]'s header records the same consequence for `AF_UNIX`
//! ("a caller speaks `POST /` … That is a consequence of mounting jsonrpsee's own service
//! rather than a choice"). Declining it would mean this module reading upgrade headers and
//! answering some requests itself, which is a routing policy invented against a client that
//! does not exist. It carries no new surface — the same `Methods`, behind the same gate —
//! and calls-only is what it can do, since `OnlyCalls` refuses every subscription.
//!
//! The route is registered with `any`, so **which methods mean what is jsonrpsee's answer**
//! and not a table here: a `GET` that is not an upgrade meets its `405`, and a `POST` without
//! the JSON content type meets its `415`. A method table in the router would be a second
//! opinion about a protocol this module does not own.
//!
//! ## The credential: what a `new WebSocket(url)` can carry, which is a URL and nothing else
//!
//! A browser's `new WebSocket(url)` **cannot set request headers** — the same fact about
//! browsers that note **N76** records for a navigation, in the one API where it is not even
//! arguable, since the WebSocket constructor takes a URL and a subprotocol list and nothing
//! else. So the credential this endpoint can be opened with is `?token=`, which
//! [`super::gate`] already accepts and note **N74** already has the rule for. Nothing here
//! widens the gate, and nothing here invents a second credential shape: the upgrade request is
//! an ordinary HTTP request, so it meets the gate before it meets this handler, and an
//! anonymous upgrade is a `401`.
//!
//! **This is one of the two routes the owner's 2026-08-12 ruling keeps gated**, and it is the
//! "drives" half of "carries or drives the camera": every T5 method that opens a camera is
//! reachable over this socket, so an anonymous upgrade is a control surface for somebody
//! else's webcam. The other half is [`super::preview`], which carries the frames themselves.
//! [`super::CAMERA_BEARING_PATHS`] is the list both are on, [`RPC_PATH`] comes from this
//! module rather than being transcribed into it, and note **N82** carries the ruling. What
//! changed with it is only what is *outside*: an anonymous `GET /` used to meet the same `401`
//! this route answers and now meets the page.
//!
//! ### What the query form costs *for a WebSocket*, stated rather than left implied
//!
//! `Token::ready_to_open_url`'s doc prices the URL form for a **navigation**: the secret
//! lands in the browser's history, in omnibox suggestions, and in the terminal scrollback of
//! whoever started the daemon. A WebSocket URL is not a navigation and pays none of those —
//! it never enters history, it is never an omnibox suggestion, and a WebSocket handshake
//! sends `Origin` and never `Referer`, so it cannot leak sideways the way a document request
//! can. What it *is* in:
//!
//! 1. **The page's own `location.search`**, which is where the client reads it from and is
//!    not a new exposure — every script running in that origin could already read it.
//! 2. **The browser's devtools network panel**, for as long as the tab is open, and any HAR
//!    export taken from it. That is the residual with teeth, and it is the operator's
//!    machine, which is the machine the camera is on.
//! 3. **Any intermediary's access log**, because the token is in the request line. D11
//!    defaults this listener to loopback, where there is no intermediary; note **N79** names
//!    the reverse-proxy shape as the thing that would change that, and it would change this
//!    with it.
//! 4. **This daemon's own logs — and that one is closed here, in both of the shapes the gate
//!    accepts.** jsonrpsee's service logs the whole `http::Request` at `TRACE` on its own
//!    target — the `Debug` of the request line **and of the header map** — so `RUST_LOG=trace`
//!    decides whether the key to somebody's camera is written into a journal that outlives the
//!    process. Under systemd that journal is persistent and readable by `systemd-journal` and
//!    `adm`, which is exactly the multi-user boundary D11's "loopback alone is not an auth
//!    boundary" is about, and `/rpc` is the route that *drives* the camera. The Unix socket
//!    never could: its request target is `/` and nothing that speaks to it presents a
//!    credential at all.
//!
//!    `without_the_credential` takes **both** forms off the request, after the gate has read
//!    it and before the service sees it. That it is both is P5's adversarial review's finding,
//!    and the way it was missed is worth keeping rather than quietly fixing: this residual was
//!    written when the request was rewritten by a function that took a [`Uri`] — and **a `Uri`
//!    cannot express a header**, so no fixture in this module could reach the half that was
//!    missing, and the test that covered it was named "the credential", singular, about a
//!    function that handled one of two. Measured at `RUST_LOG=trace` before the repair:
//!    `GET /rpc?token=…` arrived at the log as `uri: /rpc` with `{host, accept}` and
//!    `GET /rpc` with `Authorization: Bearer …` arrived as
//!    `headers {"authorization": "Bearer <the whole token>"}`. The signature is the repair and
//!    the extra line is a consequence of it.
//!
//!    `crates/daemon/tests/http.rs` is where that can go red: it drives both forms at a real
//!    socket and reads this process's own `tracing` output back through a global subscriber,
//!    because the claim is about bytes the daemon **writes** rather than about a value it
//!    holds — and because the line is written from a task `axum::serve` spawned, where a
//!    thread-local capture sees nothing and passes.
//!
//! **What this endpoint's shape said about note N76's question, kept now that the question is
//! retired.** N76 left two candidates for how the client's ES modules would authenticate, one
//! of them a session cookie; the owner's 2026-08-12 ruling dissolved the question by serving
//! the modules unauthenticated (note **N82**), so no cookie was ever weighed on its merits.
//! The fact this route contributed is worth keeping anyway, because it is the argument against
//! the candidate that is still one plausible edit away: **a WebSocket handshake is not subject
//! to CORS.** Any page in any origin may open a socket to `127.0.0.1` and the browser will not
//! stop it; what stops it here is that it cannot produce the token. A credential the browser
//! attached *by itself* — which is what a cookie is — would be attached to exactly that
//! cross-origin socket, and the gate would admit it. No cookie is read, written or accepted
//! anywhere in this daemon.
//!
//! ## Stopping
//!
//! An upgraded connection stops belonging to axum the moment hyper hands the socket over, so
//! `axum::serve`'s graceful shutdown neither waits for it nor ends it — what ends it is
//! jsonrpsee's [`ServerHandle`], and [`super::listener::Serving`] holds the one this module
//! makes. It is **not** stopped at the instant the daemon's token is cancelled, and that is
//! the ordering `crate::shutdown`'s header spends a paragraph on: a cancelled subscription
//! ends with [`crate::events::SHUTTING_DOWN`] in a payload the client can branch on, and a
//! transport stopped in the same instant closes the connection with that reason still in
//! flight. So the cancellation reaches the subscriptions (step 3) and the transport is
//! stopped where the composition root joins the listener, after the ordered teardown has run
//! — which is step 4's position, one transport along.

use std::sync::Arc;

use axum::BoxError;
use axum::Router;
use axum::extract::{Request, State};
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use jsonrpsee_server::{
    Methods, Server, ServerHandle, StopHandle, TowerServiceBuilder, stop_channel,
};
use tower::Service;
use tower::layer::util::Identity;

/// The path the WebSocket JSON-RPC endpoint answers on.
///
/// One spelling, `pub` because it has four readers that must agree: the route below;
/// [`super::CAMERA_BEARING_PATHS`], which is what keeps this route behind D11's token now that
/// the assets are not; the suite that opens a socket at it; and P5c's client, which builds
/// `ws://…/rpc?token=…` from the URL it was opened at. A literal in the page and a literal here
/// would be the pair that stops matching; a literal in the list and a literal here would be a
/// gate over a path nothing serves.
///
/// It is a path and not a scheme: a browser reaches this with `ws://` while an operator
/// reaches the page with `http://`, and the two are the same listener and the same origin.
pub const RPC_PATH: &str = "/rpc";

/// The builder jsonrpsee hands out, with both of its middleware slots empty.
///
/// Named because it lives in a struct field. `Identity` is `tower`'s no-op layer and is what
/// `Server::builder()` starts with in both slots; this daemon adds neither an HTTP nor an RPC
/// middleware, because everything it would put in one is already somewhere better — the token
/// gate is axum's layer over this route and the preview ([`super::gate`],
/// `super::listener::router`), and the D13 error vocabulary is the handlers'
/// (`crate::server`).
type WireServiceBuilder = TowerServiceBuilder<Identity, Identity>;

/// The route, and the handle whose `stop` ends the connections it accepted.
///
/// Two values that must not come apart: a router registered without its handle would leave
/// upgraded connections that nothing can end, and a handle whose route was never registered
/// would be a stop for a transport that does not exist. [`super::listener::serve`] takes both
/// out of one call.
#[derive(Debug)]
pub(super) struct Mounted {
    /// The one route, state already applied.
    pub(super) route: Router,
    /// Held by [`super::listener::Serving`] for the listener's life — **dropping it stops the
    /// server**, because jsonrpsee's stop channel is a `watch::Sender` and every
    /// [`StopHandle`] is one of its receivers. That is the safety net under the ordering this
    /// module's header argues, not a substitute for it.
    pub(super) ending: ServerHandle,
}

/// Mount `methods` — the value [`crate::server::mount`] produced — as a WebSocket endpoint.
///
/// Takes the surface as a parameter and builds none, which is the whole of this module's
/// claim to D10: there is one registration in this process and this is a second reader of it.
/// A function that took a `Wchd` and called `into_rpc()` itself would be the second
/// registration path D10 exists to prevent, and it would compile.
///
/// The bounds come from [`crate::server::wire_bounds`], the same function [`crate::uds`]
/// reads, so a browser's WebSocket runs under this project's numbers rather than jsonrpsee's —
/// [`schema::limits::RPC_MAX_SUBSCRIPTIONS_PER_CONNECTION`] and
/// [`schema::limits::WS_MESSAGE_BUFFER_CAPACITY`] included.
///
/// **What this builder's `ConnectionGuard` is, said correctly.** It is *per builder*, so this
/// transport counts separately from the Unix socket — and what it counts is **in-flight
/// requests**, not connections: the permit is taken inside `TowerService::call` and released
/// with the response, which [`crate::uds`]'s header measured (128 idle connections were all
/// accepted with the cap at 32). This doc used to read that as "this transport gets a bound of
/// its own … a browser that leaks sockets cannot exhaust the transport `webcam-handler-client`
/// uses", which was two claims resting on one guard that only carries the first of them; P5's
/// adversarial review found the second one holding up nothing at all, because
/// [`super::listener::serve`] was a bare `axum::serve` with no accept-time bound.
///
/// It is true now, and it is [`super::listener`]'s to make true: that module takes an owned
/// permit at **accept**, out of a semaphore of its own over
/// [`schema::limits::DAEMON_MAX_CONNECTIONS`], exactly as [`crate::uds`] does for the socket
/// this daemon always serves. Two counters, one per transport, which is the direction D11's
/// "opt-in" argues for everywhere else.
pub(super) fn mount(methods: Methods) -> Mounted {
    let (stop, ending) = stop_channel();
    let builder = Server::builder()
        .set_config(crate::server::wire_bounds())
        .to_service_builder();

    let wire = Arc::new(Wire {
        builder,
        methods,
        stop,
    });
    let route = Router::new()
        // `any`, so the method policy is jsonrpsee's — see this module's header. The gate is
        // **not** applied here: `super::listener::router` is the one place that decides which
        // routes are behind D11's token, and a route that installed its own would be a second
        // answer to that question in the file least likely to be read beside the first.
        .route(RPC_PATH, any(upgrade))
        .with_state(wire);

    Mounted { route, ending }
}

/// Everything the route needs to answer one request, and nothing it could decide with.
///
/// Three values, all of them somebody else's: the registration (`crate::server::mount`), the
/// bounds (`crate::server::wire_bounds`, inside the builder) and the stop channel. There is
/// no policy in this struct, which is the point — a field here would be a place for the WS
/// transport to differ from the Unix one.
#[derive(Debug)]
struct Wire {
    /// Cloned per connection. The clone shares the `ConnectionGuard` and the connection-id
    /// counter, which is why it is held rather than rebuilt: a builder made per request would
    /// hand out a fresh semaphore every time, and the request bound in
    /// [`crate::server::wire_bounds`] would be a number nothing counted. (What bounds
    /// *connections* on this transport is [`super::listener`]'s accept-time permit — see
    /// [`mount`].)
    builder: WireServiceBuilder,
    /// The one registration this process has. Cloned per connection; the clone is an `Arc`
    /// bump, so every connection answers out of the same map the Unix socket answers out of.
    methods: Methods,
    /// jsonrpsee's side of the stop. Cloned per connection, and each connection task watches
    /// its own clone.
    stop: StopHandle,
}

/// One request, answered by the wire surface.
///
/// An upgrade becomes a `101` and a connection task jsonrpsee owns; a `POST` becomes a
/// JSON-RPC answer; anything else becomes jsonrpsee's refusal. This function decides none of
/// that — see the module header for why the choosing lives one crate away.
///
/// The service is built **per request** rather than kept, because `build` is what allocates a
/// connection id and jsonrpsee keys a subscription on `(connection, subscription)`: a service
/// cloned across connections would hand two clients one id, and an `unsubscribe` could then
/// name a stream belonging to somebody else.
async fn upgrade(State(wire): State<Arc<Wire>>, request: Request) -> Response {
    let mut service = wire
        .builder
        .clone()
        .build(wire.methods.clone(), wire.stop.clone());

    // The tower contract, honoured rather than assumed. jsonrpsee's `poll_ready` is
    // unconditionally `Ready(Ok(()))` today; calling it anyway costs one line and means this
    // route does not depend on that staying true — which is the same reason `crate::uds`'s
    // header lists the five jsonrpsee names a bump would break. The qualified path is not
    // decoration: `TowerService` is `Service<Request<B>>` for *every* body type, so the
    // request axum hands out is what says which impl this is.
    let ready = std::future::poll_fn(|cx| <_ as Service<Request>>::poll_ready(&mut service, cx));
    if let Err(err) = ready.await {
        return unavailable(&err);
    }

    // The credential is stripped **here**, after the gate has read it and before the service
    // logs it — see this module's header, residual 4. The gate is a `route_layer` over this
    // route, so it has already run by the time this handler is called.
    match service.call(without_the_credential(request)).await {
        Ok(answer) => answer.map(axum::body::Body::new).into_response(),
        Err(err) => unavailable(&err),
    }
}

/// What a caller is told when the wire surface could not answer at all.
///
/// Not a D13 error document, for [`super::gate::REFUSAL`]'s reason: D13 is the *method*
/// vocabulary and it crosses the JSON-RPC wire with a code a client branches on, and this
/// failure happened before any method was named — there is nothing for a client to branch on
/// but the status. `503` and not `500` because the honest reading of a tower service that
/// refused is "this transport is not serving right now"; the detail goes to the log, where an
/// operator can see it, and not to the socket, where a stranger could.
fn unavailable(err: &BoxError) -> Response {
    tracing::error!(
        error = %err,
        path = RPC_PATH,
        "the web listener's JSON-RPC service could not answer a request"
    );
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "the wire surface is not serving\n",
    )
        .into_response()
}

/// The same request with **every** credential it presented taken off it — the query string and
/// every `Authorization` header — for the wire surface to log and answer.
///
/// It takes and returns the [`Request`] because that is the shape of the rule, and the
/// signature is the load-bearing half of this repair rather than a tidying of it: what stood
/// here took a [`Uri`], and a `Uri` cannot express a header, so the module's whole fixture
/// vocabulary was unable to state the case that was leaking (this module's header, residual 4).
/// A function that can be handed both shapes is one a test can be wrong about **out loud**.
///
/// **Nothing downstream needs either of them.** The gate is a `route_layer` over this route
/// (`super::listener::router`), so it has read the credential and decided before this handler
/// runs; jsonrpsee performs no authorization of any kind and never reads `Authorization`; and
/// the value it does read out of an upgrade — the soketto handshake's `Sec-WebSocket-Key` and
/// friends — is untouched here. So this is a removal and not a redaction.
///
/// **`remove` and not `HeaderValue::set_sensitive`.** The `http` crate offers a flag that makes
/// a value's `Debug` print `Sensitive`, which would close the *rendering* this residual is
/// about and leave the secret in the request for everything else — a middleware added later, a
/// jsonrpsee that one day echoed a header, a panic payload. Removing the value is the property
/// worth having: after this line the credential is not in the request at all, so no future
/// reader can be careless with it.
///
/// It is unconditional, and that is deliberate: there is no branch here to invert, and a
/// request that presented nothing loses nothing. `HeaderMap::remove` takes the entry **and
/// every extra value on it**, which matters because [`super::gate`] admits a request carrying
/// two `Authorization` headers when both verify — a repair that took only the first would leave
/// the second in the log for the one client shape the gate went out of its way to allow.
fn without_the_credential(mut request: Request) -> Request {
    if let Some(target) = without_the_query(request.uri()) {
        *request.uri_mut() = target;
    }
    request.headers_mut().remove(header::AUTHORIZATION);
    request
}

/// The same request target with its query string removed, or `None` when there is nothing to
/// remove and nothing this can safely rebuild.
///
/// Half of [`without_the_credential`]'s job and never the whole of it — see that function for
/// why the distinction is written down rather than left to a reader. A pure function over a
/// [`Uri`] rather than four lines inside its caller, so the case that cannot be rewritten has
/// somewhere to be asserted. Two `None`s, and they are different facts:
///
/// - **no query at all**, which is every request that carries its credential in a header — the
///   form this daemon's own `401` recommends, and the one a client that can set headers sends
///   (the page cannot: `super::gate`'s header has what each form is for). Rewriting such a
///   target would be a copy for nothing;
/// - **a target with no path to stand on**, which is the authority-form request line
///   `CONNECT host:443` — [`Uri::path`] answers `""` for it and `""` is not a URI. Leaving it
///   alone is right rather than merely safe: there is no query string in an authority-form
///   target, so there is no credential in *this* half to strip, and the header half is taken
///   off it either way.
///
/// Only the path is kept. The scheme and authority of a request target this daemon receives
/// are absent in origin-form (which is what every browser sends), and jsonrpsee routes on
/// neither.
fn without_the_query(uri: &Uri) -> Option<Uri> {
    uri.query()?;
    uri.path().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A request target this file wrote, as a value.
    fn target(text: &str) -> Uri {
        text.parse().expect("a request target the tests wrote")
    }

    /// The secret every request below presents, in whichever form the case is about.
    ///
    /// Short and obviously not a real token: what is asserted is that a *substring the client
    /// sent* is gone from the request, and a 64-digit fixture would say nothing more.
    const SECRET: &str = "c0ffee";

    /// A request at `target` presenting `credentials` as `Authorization` headers, in order.
    ///
    /// A list rather than one value, because two `Authorization` headers is well-formed HTTP
    /// and [`super::super::gate`] admits such a request when both verify — so it is a shape
    /// this route really can be handed. The `Host` and the handshake header are on every
    /// request this builds: they are what makes the "only the credential went" half of the
    /// claim below assertable.
    fn presenting(target: &str, credentials: &[&str]) -> Request {
        let mut request = Request::builder()
            .method("GET")
            .uri(target)
            .header(header::HOST, "127.0.0.1:9")
            .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==");
        for credential in credentials {
            request = request.header(header::AUTHORIZATION, *credential);
        }
        request
            .body(axum::body::Body::empty())
            .expect("a request the tests wrote")
    }

    /// The request as the wire surface's `TRACE` line renders it.
    ///
    /// `Debug`, and not a walk of the parts, because `Debug` is literally what jsonrpsee logs
    /// (`tracing::trace!(target: LOG_TARGET, "{:?}", request)`) and therefore literally what
    /// this residual is about. A test that inspected `uri()` and `headers()` separately would
    /// be asserting about the two places a token is known to hide today rather than about the
    /// string that reaches the journal.
    fn as_logged(request: &Request) -> String {
        format!("{request:?}")
    }

    #[test]
    fn every_credential_the_gate_accepts_is_off_the_request_the_wire_surface_is_handed() {
        // **The residual this closes is a token in a `RUST_LOG=trace` journal, and the gate
        // accepts two forms**, so the population here is both of them and every combination
        // this daemon will admit: the query parameter a `new WebSocket(url)` carries, the
        // header a client that can set one sends, both at once (which the gate admits when
        // they agree), and the duplicate-header shape it also admits. The predecessor of this
        // test asserted the first row only — over a `Uri`, which cannot hold the other three —
        // and every row below the first is what P5's review found being logged verbatim.
        for (about, request) in [
            ("the query form", presenting("/rpc?token=c0ffee", &[])),
            (
                "the query form beside somebody else's parameter",
                presenting("/rpc?camera=0&token=c0ffee", &[]),
            ),
            ("the header form", presenting("/rpc", &["Bearer c0ffee"])),
            (
                "both forms at once, which the gate admits when they agree",
                presenting("/rpc?token=c0ffee", &["Bearer c0ffee"]),
            ),
            (
                "two headers, which the gate admits when they agree",
                presenting("/rpc", &["Bearer c0ffee", "Bearer c0ffee"]),
            ),
        ] {
            let stripped = without_the_credential(request);
            assert!(
                !as_logged(&stripped).contains(SECRET),
                "{about}: the wire surface would log {}",
                as_logged(&stripped)
            );
            // …and the two mechanisms, named, so a failure says which half stopped working
            // rather than only that the string is still there.
            assert_eq!(stripped.uri().query(), None, "{about}");
            assert_eq!(
                stripped
                    .headers()
                    .get_all(header::AUTHORIZATION)
                    .iter()
                    .count(),
                0,
                "{about}: a credential the gate would have accepted is still on the request"
            );
        }
    }

    #[test]
    fn nothing_but_the_credential_is_taken_off_the_request() {
        // The other direction, and it is not decoration: this route's whole job is to hand
        // jsonrpsee a request it can *upgrade*, and the handshake is read off headers beside
        // the one being removed. A repair that cleared the header map — or rebuilt the target
        // as `/` — would pass the test above and answer every browser a `400`.
        let stripped = without_the_credential(presenting("/rpc?token=c0ffee", &["Bearer c0ffee"]));

        assert_eq!(stripped.method(), "GET");
        assert_eq!(stripped.uri().path(), RPC_PATH);
        assert_eq!(
            stripped
                .headers()
                .get(header::HOST)
                .map(|host| host.as_bytes()),
            Some("127.0.0.1:9".as_bytes()),
            "a header that is not a credential was taken off the request"
        );
        assert_eq!(
            stripped
                .headers()
                .get("sec-websocket-key")
                .map(|key| key.as_bytes()),
            Some("dGhlIHNhbXBsZSBub25jZQ==".as_bytes()),
            "the handshake this route exists to serve was taken off the request"
        );
    }

    #[test]
    fn a_request_that_presented_nothing_is_left_exactly_as_it_arrived() {
        // The unconditional half, over the shape it is unconditional *for*: an anonymous
        // request never reaches this handler (the gate refuses it), but a request in D11's
        // token-less cell does, and there is no gate in front of it at all (note **N75**).
        // Nothing to remove and nothing removed.
        let untouched = without_the_credential(presenting("/rpc", &[]));

        assert_eq!(untouched.uri(), &target("/rpc"));
        assert!(!as_logged(&untouched).contains(SECRET));
        assert_eq!(untouched.headers().len(), 2, "{:?}", untouched.headers());
    }

    #[test]
    fn a_target_with_nothing_to_strip_is_left_exactly_as_it_arrived() {
        // The first `None`, and the reason it is a `None` rather than a copy: a request whose
        // credential is in a header has no query at all, so rewriting its target would be work
        // for nothing — and the header half of the strip happens to it either way.
        assert_eq!(without_the_query(&target("/rpc")), None);
        assert_eq!(without_the_query(&target("/")), None);
    }

    #[test]
    fn an_authority_form_target_has_no_path_to_stand_on_and_is_left_alone() {
        // The second `None`, and the arm that exists because `Uri::path` answers `""` for an
        // authority-form request line — `CONNECT host:443`, which hyper will hand to a router
        // like any other request. `""` does not parse as a `Uri`, so a rewrite would have to
        // decide what to do about it; there is no query string in this form and therefore no
        // credential to strip, so leaving it alone is the answer rather than the fallback.
        let connect = target("example.invalid:443");
        assert_eq!(connect.path(), "", "the shape this arm is about has moved");
        assert_eq!(connect.query(), None);
        assert_eq!(without_the_query(&connect), None);

        // And the arm really is reachable rather than argued: a target with a path that will
        // not stand alone answers `None` even when it *does* carry a query.
        assert_eq!(
            "".parse::<Uri>().ok(),
            None,
            "an empty target parses after all, and this arm has stopped being about anything"
        );
    }

    #[test]
    fn the_path_is_one_spelling_and_it_is_a_path() {
        // Four readers have to agree on it (see the constant). A value that stopped being a
        // path — a full URL, or a name with no leading slash — would register a route axum
        // never matches, which is a listener that answers 404 to its own client.
        assert!(RPC_PATH.starts_with('/'), "{RPC_PATH}");
        assert_eq!(target(RPC_PATH).path(), RPC_PATH);
    }

    #[test]
    fn this_route_is_on_the_list_that_keeps_it_gated() {
        // Since the owner's 2026-08-12 ruling the gate is over the camera-bearing routes and
        // not over everything (note **N82**), so a route that fell off this list would be a
        // JSON-RPC surface for somebody's camera served to anybody who asked. The same
        // assertion is made one route along, in `super::preview`, because the two are on the
        // list for two different reasons — this one *drives* a camera and that one *carries*
        // one — and a list that lost either is a different defect.
        assert!(
            super::super::CAMERA_BEARING_PATHS.contains(&RPC_PATH),
            "the wire route is not on the list of paths that stay gated"
        );
    }
}
