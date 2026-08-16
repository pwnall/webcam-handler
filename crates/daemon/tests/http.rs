//! The opt-in TCP transport, end to end: a real socket, a real axum server, a hand-written
//! client (D11, docs/7 P5a).
//!
//! `uds.rs` is this suite's model — it asserts that a byte stream over `AF_UNIX` carries a
//! JSON-RPC call and its answer — and this is the same altitude about the other transport:
//! that a byte stream over TCP carries an HTTP request, that D11's matrix decides whether it
//! is answered, and that the server starts and stops on demand. What the client *shows* a
//! person is P5c's, and what the WebSocket and the preview do is P5b's.
//!
//! ## Which requests the token is for, since 2026-08-12
//!
//! The owner ruled that **static assets are served without authentication** — the client is
//! open-source code, not a secret — and that only the resources which carry or drive the camera
//! stay behind D11's token: `daemon::http::CAMERA_BEARING_PATHS`, which is the WebSocket
//! endpoint and the MJPEG preview (note **N82**). So the pair this file is built out of is no
//! longer "401 without, 200 with" over one path. It is three answers to the *same* anonymous
//! request, and a build can get any one of them wrong on its own:
//!
//!   * an asset is **200** — the ruling is a requirement rather than a permission, so a listener
//!     that kept the gate over `/index.html` fails here;
//!   * a path that names no asset is **404**, which used to be a 401 and is the visible half of
//!     the property note N75 gave up;
//!   * a camera-bearing route is **401**, in full — status, RFC 6750's challenge, and the
//!     daemon's fixed body.
//!
//! Every test that presents a credential presents it on a **gated** route, because a credential
//! presented to an ungated one proves nothing about a gate.
//!
//! ## The second admission rule, since 2026-08-13
//!
//! The owner ruled again: *"the daemon should refuse all calls tagged with cross-origin headers.
//! I see no good reason to accept those."* `daemon::http::provenance` is that rule, and the pair
//! above is no longer the whole story either — this listener now has **two** ways to say no, and
//! a suite that could not tell them apart would let one quietly become the other.
//!
//! What the tests below are shaped by is note **N93**, which measured what the pinned Chromium
//! sends per request the shipped client makes. Three consequences, and each is a test:
//!
//!   * **the preview `<img>` carries no `Origin` at all** — a media tag performs a no-CORS load —
//!     so `Sec-Fetch-Site` is the primary signal and `Origin` corroborates.
//!     [`every_request_the_shipped_page_makes_is_still_served`] replays the measured table over
//!     the wire, and it is the assertion that stops the rule being too strict;
//!   * **absence must be admitted**, because `webcam-handler-client`, `curl` and the agent
//!     harness AGENTS calls this project's primary consumer send neither header.
//!     [`a_request_claiming_no_provenance_is_admitted_because_the_primary_consumer_sends_none`]
//!     is the test that goes red if the rule is ever strengthened into "require a same-origin
//!     header", which would lock out the consumer that matters most;
//!   * **the rule covers more than the gate does**, because it is installed with `Router::layer`
//!     rather than `route_layer` and in all four of D11's cells rather than three.
//!     [`a_request_a_browser_tagged_cross_site_is_refused_on_every_path_this_listener_serves`]
//!     quantifies over a population derived the way the composition derives it, and
//!     [`the_token_less_cell_is_the_one_this_rule_is_for`] drives the cell that has no gate at
//!     all — which is the cell the ruling is about.
//!
//! The two refusals are asserted to be **different answers**: `401` with RFC 6750's challenge
//! for a missing credential, `403` with no challenge for a request no credential could fix
//! ([`is_the_refusal`] and [`is_the_cross_origin_refusal`], and
//! [`every_refused_shape_gets_the_same_answer`] between them).
//!
//! What this file cannot establish is that a **real** browser tags a **real** foreign request the
//! way `provenance` expects: every header here was typed by this suite. That half is
//! `crates/daemon/tests/browser/`'s claim "a page in another origin cannot reach the camera,
//! token and all", which drives a sandboxed iframe in the pinned Chromium and reads the status
//! this daemon answered it with.
//!
//! ## What the suite drives, and what that is worth
//!
//! Two entry points, deliberately, because they establish different things.
//!
//! [`Web::opened`] calls `daemon::http::open` — **the function `webcam-handler-daemon`'s
//! composition root calls**, which decides the posture from the address, mints the token only
//! where D11 gates it, binds, and serves. Everything after that in `main.rs` is: log what it
//! answers, join it. So a test that opens a listener this way and then opens the URL it
//! published is asserting about the shipped daemon and not about a rehearsal of it.
//!
//! Both hand in a **surface with no methods on it** ([`no_methods`]), because the subject
//! here is the gate and the matrix. What the WebSocket endpoint on the same listener carries,
//! that it carries the registration `daemon::server::mount` produced and no second one, and
//! that the gate covers an upgrade as readily as it covers a `GET`, is `web_rpc.rs`'s — over
//! a real `Wchd` and beside the Unix socket serving the same value, which is the only
//! arrangement in which "the same home" is a comparison rather than a sentence.
//!
//! [`Web::with_posture`] calls `daemon::http::serve` with a posture decided elsewhere, which
//! is the only way D11's two non-loopback cells can be reached on a machine with one
//! interface: the socket is bound to loopback and the posture is decided about
//! `192.168.1.10:8080`. **What those two tests establish is that the gate is installed because
//! the posture said so** — and nothing about the composition root having decided that posture
//! from the right address. That half is covered where it lives: `main.rs`'s clap suite pins
//! what `--http` and `--http-insecure-loopback` parse to, and `daemon::http::posture`'s own
//! suite pins what those two values decide, over all four cells and both address families.
//!
//! ## The token comes out of the URL the daemon printed
//!
//! Every test that presents a credential takes it from [`Web::token`], which parses
//! `Serving::ready_to_open_url` — the line an operator copies. Reaching into the `Token` value
//! instead would let a build publish one secret and check another and still pass the whole
//! suite, which is the one bug a "401 without, 200 with" pair is supposed to be unable to
//! survive.
//!
//! ## The two things a credential can leak into, which are not the gate
//!
//! P5's adversarial review found both, and neither is reachable from a test that only asks what
//! a route answers — so both are here, beside the gate they are about.
//!
//! **The daemon's own log.** jsonrpsee logs the whole `http::Request` at `TRACE` on its own
//! target, request line *and headers*, so `RUST_LOG=trace` decides whether a credential this
//! listener accepts ends up in a journal that outlives the run. `daemon::http::rpc` takes both
//! forms off the request before the wire surface sees it, and
//! [`neither_form_of_the_credential_reaches_the_daemons_own_log`] is what can go red on that —
//! the one test in this workspace that reads `tracing` output back through a **global**
//! subscriber, because the line is written from a task `axum::serve` spawned and a thread-local
//! default cannot see it.
//!
//! **The descriptors behind the listener.** `limits::DAEMON_MAX_CONNECTIONS` exists so a client
//! that leaks connections is refused rather than able to exhaust the daemon's file descriptors,
//! "which on this process are also the camera's" — and jsonrpsee's cap of the same name is not
//! that bound (`daemon::uds` measured it: 128 idle connections were all accepted with the cap at
//! 32, because its permit is per in-flight *request*). So this transport takes a permit at
//! accept, and [`connections_are_bounded_at_the_number_this_project_chose_on_this_transport_too`]
//! is the TCP twin of `uds.rs`'s test of the same name: connections that send **nothing**, which
//! is what makes the bound the only thing between a local process and the camera's descriptors —
//! no byte is sent, so no gate, no route and no handler is ever reached.
//!
//! Nothing here waits on a clock: the client's read ends at end-of-file (`support/tcp.rs`),
//! and the stop is a cancellation followed by a join.

mod support;
#[path = "support/tcp.rs"]
mod tcp;

use std::net::SocketAddr;
use std::sync::Arc;

use daemon::http::{
    self, CAMERA_BEARING_PATHS, PREVIEW_PATH, Posture, RPC_PATH, TOKEN_QUERY_PARAM, Token,
    TokenRule, gate, provenance,
};
use daemon::shutdown::Shutdown;
use daemon::uds::{self, SocketDir};
use engine::paths::TempRuntimeDir;
use engine::store::{LockProtocol, StoreLock, TempStore};
use schema::limits;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

use crate::support::{body, call};
use crate::tcp::Answer;

/// A running web listener and the token it published, thrown away with the value.
struct Web {
    serving: http::Serving,
    shutdown: Shutdown,
}

impl Web {
    /// Everything `--http` costs the composition root, driven the way `main` drives it.
    ///
    /// The address is always loopback with port zero, because what a test may assume about
    /// the machine it runs on is that it can bind loopback. The *posture* cells that need
    /// another address are [`Web::with_posture`]'s.
    async fn opened(insecure_loopback: bool) -> Web {
        let shutdown = Shutdown::new();
        let serving = http::open(
            address("127.0.0.1:0"),
            insecure_loopback,
            no_methods(),
            no_cameras(),
            shutdown.clone(),
        )
        .await
        .expect("loopback on an ephemeral port");
        Web { serving, shutdown }
    }

    /// A listener bound to loopback, serving a posture decided about somewhere else.
    ///
    /// The suite header says what this establishes and what it does not. The token is minted
    /// here rather than inside, because [`http::serve`] takes it as a value — that is the
    /// seam, and it is also what makes the two disagreeing arrangements refusable
    /// (`daemon::http::listener`'s unit tests drive those).
    async fn with_posture(posture: Posture) -> Web {
        let shutdown = Shutdown::new();
        let listener = http::bind(address("127.0.0.1:0"))
            .await
            .expect("loopback on an ephemeral port");
        let token = match posture.token() {
            TokenRule::Required => Some(Arc::new(Token::mint().expect("the kernel has a CSPRNG"))),
            TokenRule::NotRequired => None,
        };
        let serving = http::serve(
            listener,
            posture,
            token,
            no_methods(),
            no_cameras(),
            shutdown.clone(),
        )
        .expect("a posture and a token that agree");
        Web { serving, shutdown }
    }

    /// Where the kernel actually put it.
    fn bound(&self) -> SocketAddr {
        self.serving.bound()
    }

    /// The line an operator copies.
    fn url(&self) -> String {
        self.serving.ready_to_open_url()
    }

    /// The token that URL carries, which is the only credential this suite ever presents.
    ///
    /// `None` in D11's token-less cell, where the URL has no query at all — and that is
    /// asserted rather than assumed by the tests that call this, because a `None` here and a
    /// gate that was never installed are two different claims.
    fn token(&self) -> Option<String> {
        let url = self.url();
        url.split_once(&format!("?{TOKEN_QUERY_PARAM}="))
            .map(|(_url, token)| token.to_owned())
    }

    /// The token, for a test that has already established there is one.
    fn secret(&self) -> String {
        self.token()
            .expect("this cell of D11's matrix is token-gated")
    }

    /// A `GET` presenting nothing at all — what a stranger sends.
    async fn anonymous(&self, target: &str) -> Answer {
        tcp::get(self.bound(), target, &[])
            .await
            .expect("the listener is up")
    }

    /// The same, with the method spelled out: what a stranger sends who is not sending a `GET`.
    async fn anonymous_with(&self, method: &str, target: &str) -> Answer {
        tcp::request(method, self.bound(), target, &[])
            .await
            .expect("the listener is up")
    }

    /// A `GET` presenting the token the way a client that can set a header does — `curl`, a
    /// script, and this suite, which is who `gate::REFUSAL` recommends it to.
    ///
    /// **Not the way the page does it**: P5c's client has no `Authorization` anywhere, because
    /// the two things it reaches (`new WebSocket(url)` and `<img src>`) cannot carry one, so it
    /// writes the token into a URL for both (`credential.js`, note **N74**). The daemon accepts
    /// this form regardless, and it is a form a build can lose on its own, which is why the
    /// suite drives it.
    async fn bearing(&self, target: &str, token: &str) -> Answer {
        tcp::get(
            self.bound(),
            target,
            &[("Authorization", &format!("Bearer {token}"))],
        )
        .await
        .expect("the listener is up")
    }

    /// A `GET` carrying the headers a browser stamps on a request, whatever they are.
    ///
    /// The one entry point for `daemon::http::provenance`'s subject. Every field is spelled as a
    /// literal by the caller and never read out of the daemon's own constants, which is the
    /// inverse of how this file treats the token and is deliberate: `Sec-Fetch-Site` and
    /// `Origin` are the **browser's** vocabulary, fixed by the Fetch and HTML specifications, so
    /// a suite that read them out of `daemon::http` would keep passing against a build that
    /// looked up a header no browser sends.
    async fn tagged(&self, target: &str, fields: &[(&str, &str)]) -> Answer {
        tcp::get(self.bound(), target, fields)
            .await
            .expect("the listener is up")
    }

    /// Stop the way the daemon stops it, and wait for the task to end.
    ///
    /// The cancellation is the same one every subscription watches; the join is what turns
    /// "the listener ended" from a maybe into a fact (`daemon::http::listener`).
    async fn stop(self) {
        self.shutdown.cancel();
        self.serving.stopped().await.expect("the server task ended");
    }
}

/// An address this file wrote, as a value.
fn address(text: &str) -> SocketAddr {
    text.parse().expect("a socket address the tests wrote")
}

/// The wire surface this suite hands the listener: none.
///
/// See the header. The listener takes the T5 surface as a value, so a suite whose subject is
/// the credential can hand it an empty one and still be driving the function
/// `webcam-handler-daemon` calls — and a suite that handed it a real one would be claiming, in
/// this file, something `web_rpc.rs` claims properly. What it does **not** mean is that the
/// endpoint is absent: `daemon::http::rpc`'s route is registered either way, which is why
/// [`the_wire_route_is_gated_and_the_page_beside_it_is_not`] can ask about it here.
fn no_methods() -> jsonrpsee_server::Methods {
    jsonrpsee_server::Methods::new()
}

/// The preview fan-out this suite hands the listener: one with no camera behind it.
///
/// [`no_methods`]'s argument, one route along. This file's subject is the credential, and the
/// credential is checked before the route's handler runs — so an anonymous `GET /preview?camera=…`
/// is a `401` whether or not any camera exists, which is exactly what
/// [`every_camera_bearing_route_is_behind_the_gate`] establishes. What a preview *does* once
/// the token is right is `crates/daemon/tests/preview.rs`'s claim, over a real `Wchd` and the
/// fake backend.
fn no_cameras() -> daemon::preview::Previews {
    daemon::preview::Previews::new(
        Arc::new(engine::actor::Cameras::new(Arc::new(
            fake::FakeBackend::new(Vec::new()).expect("a backend replaying no cameras"),
        ))),
        engine::settle::MonotonicClock::new(),
        Shutdown::new(),
    )
}

/// An equal-length, one-digit-different token.
///
/// The candidate a gate that merely looks for the parameter's presence would accept, and the
/// only shape that reaches `Token::verify`'s loop at all — everything shorter or longer is
/// refused by the length check.
fn near_miss(secret: &str) -> String {
    let mut digits: Vec<char> = secret.chars().collect();
    let first = digits.first_mut().expect("a token is not empty");
    *first = if *first == '0' { '1' } else { '0' };
    digits.into_iter().collect()
}

/// `path` with the `?token=…` a navigation carries.
///
/// A path parameter rather than a fixed `/`, because since the 2026-08-12 ruling `/` needs no
/// credential and a test that presented one there would be asserting about a gate that is not
/// installed on it (note **N82**).
fn with_token(path: &str, secret: &str) -> String {
    format!("{path}?{TOKEN_QUERY_PARAM}={secret}")
}

/// Assert an answer is the gate's refusal, in full.
///
/// Three claims and none implies the others: the status a client branches on, the challenge
/// RFC 6750 asks for, and a body that is the daemon's fixed sentence rather than a rendering
/// of something. The body is compared against the constant, so a build that started answering
/// with a stack trace — or with the token — fails here rather than in a review.
fn is_the_refusal(answer: &Answer, about: &str) {
    assert_eq!(answer.status(), 401, "{about}: {}", answer.body());
    assert_eq!(
        answer.header("WWW-Authenticate"),
        Some(gate::BEARER_CHALLENGE),
        "{about}: a 401 with no challenge"
    );
    assert_eq!(answer.body(), gate::REFUSAL, "{about}");
}

/// Every method a stranger can put on the request line of a camera-bearing path.
///
/// **The population is methods and not one method** (note **N185**), and that is what makes the
/// claim survive the next edit to a route. D11's gate is a `route_layer`, so it wraps the route's
/// whole `MethodRouter` and refuses a *route* rather than a verb — which means a build that has
/// grown a second method on `/preview` (G6/M23 added `HEAD`) needs no new assertion here, and a
/// build that had somehow put one method outside the layer fails without anybody having guessed
/// which. Every name RFC 9110 §9.3 defines, less `CONNECT`, whose request target is an authority
/// rather than a path and which is therefore not a request for one of these routes at all.
const EVERY_METHOD: [&str; 8] = [
    "GET", "HEAD", "POST", "PUT", "DELETE", "PATCH", "OPTIONS", "TRACE",
];

/// Assert an answer is the token gate's refusal to a method that carries no body back.
///
/// `HEAD` is the reason this exists: RFC 9110 §9.3.2 forbids content in its answer, so hyper
/// drops the body and the refusal arrives as its status and its challenge alone. Asserting
/// [`gate::REFUSAL`] there would fail on a daemon that is behaving exactly as it must, and
/// dropping the body assertion for *every* method would give up the claim that the two refusals
/// this listener writes are still distinguishable ([`is_the_cross_origin_refusal`] is the other).
fn is_the_refusal_to(method: &str, answer: &Answer, about: &str) {
    assert_eq!(
        answer.status(),
        401,
        "{about}: {method} was not refused: {}",
        answer.body()
    );
    assert_eq!(
        answer.header("WWW-Authenticate"),
        Some(gate::BEARER_CHALLENGE),
        "{about}: a 401 to {method} with no challenge"
    );
    if method == "HEAD" {
        assert!(
            answer.body().is_empty(),
            "{about}: a HEAD answer carried content: {}",
            answer.body()
        );
    } else {
        assert_eq!(answer.body(), gate::REFUSAL, "{about}: to {method}");
    }
}

/// Assert an answer is the cross-origin rule's refusal, in full.
///
/// Four claims, and the last two are what stop this being a restatement of [`is_the_refusal`]:
/// the status a client branches on, the daemon's fixed sentence, the **absence** of RFC 6750's
/// challenge — a `WWW-Authenticate` here would be an invitation to retry with a credential that
/// cannot help — and that the body is not the gate's, so a build whose two refusals had merged
/// into one fails rather than looking careful.
fn is_the_cross_origin_refusal(answer: &Answer, about: &str) {
    assert_eq!(answer.status(), 403, "{about}: {}", answer.body());
    assert_eq!(answer.body(), provenance::REFUSAL, "{about}");
    assert_eq!(
        answer.header("WWW-Authenticate"),
        None,
        "{about}: a refusal no credential can lift offered a challenge"
    );
    assert_ne!(answer.body(), gate::REFUSAL, "{about}");
}

/// Assert an answer is the client's page.
fn is_the_page(answer: &Answer, about: &str) {
    assert_eq!(answer.status(), 200, "{about}: {}", answer.body());
    assert_eq!(
        answer.header("Content-Type"),
        Some("text/html; charset=utf-8"),
        "{about}: served as something a browser will not render"
    );
    assert!(
        answer.body().starts_with("<!doctype html>"),
        "{about}: {}",
        answer.body()
    );
}

#[tokio::test]
async fn an_anonymous_asset_is_served_and_an_anonymous_camera_route_is_not() {
    // **The owner's 2026-08-12 ruling, as the three answers one anonymous client gets**, in the
    // order a browser meets them. The first is a requirement and not a permission: a listener
    // that kept the gate over the page would fail here, which is the direction this test was
    // written to fail in.
    //
    // The second is the visible half of what the narrowing cost. A path this daemon does not
    // serve is now **404 and not 401**, so a stranger can map the asset table — which is a
    // directory in a public repository, and is why the property note N75 argued for stopped
    // being worth its price rather than merely lapsing.
    //
    // The third is driven over `CAMERA_BEARING_PATHS` rather than over two literals: the list
    // is the daemon's own, so a route that is on it and lost its gate fails here, and a route
    // that is on it and does not exist fails here too — an unrouted path answers the asset
    // table's 404, not a 401.
    //
    // **And over every method, not over `GET`** (note **N185**). What the ruling gated is a set
    // of routes, and D11's gate is a `route_layer` — it wraps the route's whole `MethodRouter`,
    // so it has no way to admit one verb and refuse another. That is a property worth asserting
    // rather than assuming, because the arrangement it rests on is invisible from a request:
    // until G6 both halves of this pair drove `GET` alone, `HEAD` was added to `/preview` in the
    // same review (note **N179**), and nothing in this workspace would have noticed a build that
    // answered it from outside the layer.
    let web = Web::opened(false).await;

    is_the_page(&web.anonymous("/").await, "the page, with no credential");
    is_the_page(&web.anonymous("/index.html").await, "the page by name");
    assert_eq!(
        web.anonymous("/nothing-here").await.status(),
        404,
        "a path that names no asset answered something other than the asset table's 404"
    );
    let mut refused = 0_usize;
    for path in CAMERA_BEARING_PATHS {
        is_the_refusal(&web.anonymous(path).await, path);
        for method in EVERY_METHOD {
            is_the_refusal_to(method, &web.anonymous_with(method, path).await, path);
            refused += 1;
        }
    }
    assert_eq!(
        refused,
        CAMERA_BEARING_PATHS.len() * EVERY_METHOD.len(),
        "the method population and the path population did not both quantify over something"
    );

    web.stop().await;
}

#[tokio::test]
async fn every_asset_this_build_embeds_is_served_to_a_request_presenting_nothing() {
    // The same claim over the whole population rather than over the one file P5a shipped, and
    // the population comes from `webcam-handler-web`'s own table (`web::paths`) rather than
    // from a list here. P5c adds modules to that directory; each one joins this test by
    // existing, which is the only arrangement in which "the client loads with no credential"
    // keeps meaning what it means today — a module graph is one `GET` per module, and one 401
    // in it is a client that does not run (note **N76**, retired by the ruling this asserts).
    let web = Web::opened(false).await;

    let mut served = 0_usize;
    for name in web::paths() {
        let answer = web.anonymous(&format!("/{name}")).await;
        assert_eq!(answer.status(), 200, "/{name} was not served anonymously");
        served += 1;
    }
    assert!(
        served > 0,
        "the asset table is empty and this test is about nothing"
    );

    web.stop().await;
}

#[tokio::test]
async fn the_token_opens_a_gated_route_in_either_form() {
    // docs/9's "Token enforcement" row, at the routes the token is now for. Both forms, because
    // they are two mechanisms a build can lose one at a time: the header is what a client that
    // can set one sends — `curl`, a script, the sentence this daemon's own 401 ends with — and
    // the query parameter is what a URL can carry, which is all a `new WebSocket(url)` and an
    // `<img src>` have, and therefore all P5c's client ever uses (note **N74**).
    //
    // "Past the gate" is asserted as an answer only the route behind it gives. `/rpc` answers
    // **405** to a `GET` that is not an upgrade, which is jsonrpsee's answer and could have come
    // from nowhere else; the preview answers **400** to a request that names no camera, which is
    // its own. Neither is the gate's 401 and neither is the asset table's 404, so a build that
    // registered either route elsewhere fails here rather than looking authenticated.
    let web = Web::opened(false).await;
    let secret = web.secret();

    for (path, past) in [(RPC_PATH, 405), (PREVIEW_PATH, 400)] {
        assert_eq!(
            web.bearing(path, &secret).await.status(),
            past,
            "{path} with the header form"
        );
        assert_eq!(
            web.anonymous(&with_token(path, &secret)).await.status(),
            past,
            "{path} with the query form"
        );
    }

    web.stop().await;
}

#[tokio::test]
async fn a_near_miss_token_is_refused_over_the_wire() {
    // The gate is *checking*, not looking for a parameter. An equal-length, one-digit-different
    // token is the only candidate that reaches `Token::verify`'s comparison loop, and it is
    // presented in both forms because they are two code paths into the same answer — on **both**
    // gated routes, because since the ruling there are two of them and one gate serving a near
    // miss is one camera served to whoever guessed a digit.
    let web = Web::opened(false).await;
    let secret = web.secret();
    let wrong = near_miss(&secret);
    // A **prefix**, which is the shape a wrapped or truncated URL arrives in — and the shape a
    // gate that compared with `starts_with` instead of `Token::verify` would serve while
    // refusing every equal-length candidate here.
    let truncated = &secret[..secret.len() - 8];

    for path in CAMERA_BEARING_PATHS {
        is_the_refusal(&web.bearing(path, &wrong).await, path);
        is_the_refusal(&web.anonymous(&with_token(path, &wrong)).await, path);
        is_the_refusal(&web.anonymous(&with_token(path, truncated)).await, path);
    }

    // ... and the token these are all near misses *of* still gets past, so this is a statement
    // about the digits and not about a route that refuses everything.
    assert_eq!(
        web.bearing(RPC_PATH, &secret).await.status(),
        405,
        "the real token"
    );

    web.stop().await;
}

#[tokio::test]
async fn every_answer_carries_the_no_referrer_policy() {
    // The token rides the document's URL, so the URL a browser holds for this page **is** the
    // key to a camera — and a `Referer` is that URL, handed to whatever the page links out to.
    // Same-origin leakage back here is harmless; a link out is not, and P5c's client is the one
    // that will have links in it.
    //
    // Four shapes, because the header is applied to the outermost layer and a build that
    // stamped it inside the gate — or on the asset half alone — would pass a test that only
    // asked for the page. The 401 is the interesting member: it is the answer to a request whose
    // URL may have carried the token in the first place.
    let web = Web::opened(false).await;
    let secret = web.secret();

    for (target, about) in [
        ("/", "the page"),
        ("/nothing-here", "the asset table's 404"),
        (RPC_PATH, "the gate's refusal"),
    ] {
        assert_eq!(
            web.anonymous(target).await.header("Referrer-Policy"),
            Some("no-referrer"),
            "{about}"
        );
    }
    assert_eq!(
        web.bearing(RPC_PATH, &secret)
            .await
            .header("Referrer-Policy"),
        Some("no-referrer"),
        "an answer from behind the gate"
    );

    web.stop().await;
}

#[tokio::test]
async fn a_request_carrying_two_credentials_is_served_only_when_they_agree() {
    // `daemon::http::gate`'s rule, over the wire rather than over a `HeaderMap`: every
    // credential a request presents must verify. The two shapes are the ones HTTP allows and
    // clients do not send — a header beside a query parameter, and the same parameter twice —
    // and each is served by exactly one of the two gates this project refused to build (first
    // wins, last wins). On a gated route, because that is where the rule has consequences.
    let web = Web::opened(false).await;
    let secret = web.secret();
    let wrong = near_miss(&secret);

    assert_eq!(
        web.bearing(&with_token(RPC_PATH, &secret), &secret)
            .await
            .status(),
        405,
        "a scripted request presenting a header, sent at a URL that still carries the token"
    );
    is_the_refusal(
        &web.bearing(&with_token(RPC_PATH, &wrong), &secret).await,
        "a good header beside a bad query parameter",
    );
    is_the_refusal(
        &web.anonymous(&format!(
            "{RPC_PATH}?{TOKEN_QUERY_PARAM}={secret}&{TOKEN_QUERY_PARAM}={wrong}"
        ))
        .await,
        "a good query parameter followed by a bad one",
    );
    is_the_refusal(
        &web.anonymous(&format!(
            "{RPC_PATH}?{TOKEN_QUERY_PARAM}={wrong}&{TOKEN_QUERY_PARAM}={secret}"
        ))
        .await,
        "a bad query parameter followed by a good one",
    );

    // Two `Authorization` headers, which is well-formed HTTP and which `HeaderMap::get` would
    // answer with only the first of.
    let answered = tcp::get(
        web.bound(),
        RPC_PATH,
        &[
            ("Authorization", &format!("Bearer {secret}")),
            ("Authorization", &format!("Bearer {wrong}")),
        ],
    )
    .await
    .expect("the listener is up");
    is_the_refusal(&answered, "two Authorization headers that disagree");

    web.stop().await;
}

#[tokio::test]
async fn the_wire_route_is_gated_and_the_page_beside_it_is_not() {
    // P5b's endpoint, asked the question this file is about and no other: **is it inside the
    // wall?** Since the 2026-08-12 ruling the wall has two sides and both are asserted here,
    // against the same listener in the same run, because "the camera routes are gated" and
    // "the assets are not" are the two halves a build can get wrong in opposite directions.
    //
    // Anonymously the wire route is the daemon's refusal, in full. With the token it is **405
    // and not 404**, which is two facts at once: the request got past the gate, and `/rpc` is a
    // *route* rather than a name the asset fallback failed to find — jsonrpsee answers "POST is
    // required" to a `GET` that is not an upgrade, and that answer could only have come from the
    // wire surface. A build that registered the route on some other path would answer 404 here,
    // from the asset table, and would now do it **without** a 401 in front of it, which is the
    // failure the ruling made louder rather than quieter.
    let web = Web::opened(false).await;
    let secret = web.secret();

    is_the_refusal(
        &web.anonymous(RPC_PATH).await,
        "the wire route, anonymously",
    );
    assert_eq!(
        web.bearing(RPC_PATH, &secret).await.status(),
        405,
        "the wire route answered as an asset that is not there"
    );
    is_the_page(
        &web.anonymous("/").await,
        "the page beside it, with no credential at all",
    );

    web.stop().await;
}

// ------------------------------------------- the cross-origin rule (owner ruling, 2026-08-13)

/// Every path this listener can answer, as a population derived from the daemon's own tables.
///
/// Four parts, and each is a different half of the composition — which is the whole reason a
/// rule installed with `Router::layer` needs a population and the token gate did not. The asset
/// table comes from `webcam-handler-web`'s own list, so a module P5c or anybody else adds joins
/// this claim by existing; `/` is the same table reached by the one path that is a lookup rather
/// than a name; `CAMERA_BEARING_PATHS` is the routes; and `/nothing-here` is the catch-all
/// `404`, which is served by neither a route nor a named asset and is precisely the answer
/// `Router::route_layer` would not have covered.
fn every_path_this_listener_answers() -> Vec<String> {
    let mut paths = vec!["/".to_owned(), "/nothing-here".to_owned()];
    paths.extend(web::paths().map(|name| format!("/{name}")));
    paths.extend(CAMERA_BEARING_PATHS.iter().map(|path| (*path).to_owned()));
    paths
}

#[tokio::test]
async fn every_request_the_shipped_page_makes_is_still_served() {
    // **The assertion that stops the cross-origin rule being too strict**, and it is note
    // **N93**'s measured table replayed over a real socket: the headers here are the ones the
    // pinned Chromium was observed to send, per request the shipped client makes.
    //
    // Two rows carry **no `Origin` at all** — the stylesheet and the preview `<img>` — because a
    // media tag performs a no-CORS load, and that is the finding the whole design turns on. A
    // rule spelled "require a same-origin `Origin`" passes every negative test in this file and
    // fails here, on the one route whose response body is a live camera.
    //
    // `Sec-Fetch-Site` and `Origin` are spelled as literals rather than read out of
    // `daemon::http` (see `Web::tagged`), so a build that looked up a header no browser sends
    // would be red here rather than quietly admitting everything.
    let web = Web::opened(false).await;
    let secret = web.secret();
    let ours = format!("http://{bound}", bound = web.bound());

    // The navigation an operator makes first: no initiator, so `none` — the value this rule has
    // to admit and the one a page cannot cause.
    is_the_page(
        &web.tagged(
            &with_token("/", &secret),
            &[("Sec-Fetch-Dest", "document"), ("Sec-Fetch-Site", "none")],
        )
        .await,
        "the page, opened from the URL the daemon printed",
    );

    // Every subresource the client has, from the asset crate's own table. The ES modules carry
    // an `Origin` and the stylesheet does not; both were measured, and both must be served.
    let mut subresources = 0_usize;
    for name in web::paths() {
        let module = name.ends_with(".js");
        let mut fields = vec![
            ("Sec-Fetch-Dest", if module { "script" } else { "style" }),
            ("Sec-Fetch-Site", "same-origin"),
        ];
        if module {
            fields.push(("Origin", ours.as_str()));
        }
        let answer = web.tagged(&format!("/{name}"), &fields).await;
        assert_eq!(
            answer.status(),
            200,
            "/{name} was refused a request the shipped page makes: {}",
            answer.body()
        );
        subresources += 1;
    }
    assert!(
        subresources > 0,
        "the asset table is empty and this test is about nothing"
    );

    // The preview `<img>`: `same-origin`, and **no `Origin`**. Past both layers, so the answer is
    // the preview's own complaint that no camera was named rather than a 401 or a 403.
    assert_eq!(
        web.tagged(
            &with_token(PREVIEW_PATH, &secret),
            &[
                ("Sec-Fetch-Dest", "image"),
                ("Sec-Fetch-Site", "same-origin")
            ]
        )
        .await
        .status(),
        400,
        "the preview <img> — the request that carries no Origin and a live camera"
    );

    // The WebSocket upgrade: an `Origin` and no Fetch Metadata at all, which is what the
    // handshake sends. 405 is jsonrpsee's answer to a `GET` that is not an upgrade, so this
    // provably reached the wire surface.
    assert_eq!(
        web.tagged(&with_token(RPC_PATH, &secret), &[("Origin", ours.as_str())])
            .await
            .status(),
        405,
        "the WebSocket upgrade's own Origin was treated as somebody else's"
    );

    web.stop().await;
}

#[tokio::test]
async fn a_request_a_browser_tagged_cross_site_is_refused_on_every_path_this_listener_serves() {
    // **The ruling, over the whole listener**: *"the daemon should refuse all calls tagged with
    // cross-origin headers"*. The population is derived rather than listed
    // (`every_path_this_listener_answers`) because this rule is installed with `Router::layer`
    // and therefore claims something about paths no route table names — the asset fallback and
    // the catch-all `404` included. A build that installed it with `route_layer`, the way the
    // token gate is installed, passes on the two camera routes and fails on everything else.
    //
    // **Both cells**, because the rule is not the gate's and does not share its installation:
    // `--http-insecure-loopback` removes the token and must not remove this.
    //
    // **And the credential does not help**, which is the ordering claim: in the gated cell the
    // same request is sent again carrying this run's real token, and it still meets the 403 — so
    // the cross-origin layer is outside the gate rather than beside it, and a foreign page that
    // somehow obtained the token gets no further than one that did not.
    for insecure_loopback in [false, true] {
        let web = Web::opened(insecure_loopback).await;
        let cell = if insecure_loopback {
            "the token-less cell"
        } else {
            "the default cell"
        };

        let mut refused = 0_usize;
        for path in every_path_this_listener_answers() {
            is_the_cross_origin_refusal(
                &web.tagged(&path, &[("Sec-Fetch-Site", "cross-site")]).await,
                &format!("{cell}: {path}, tagged cross-site"),
            );
            if let Some(secret) = web.token() {
                is_the_cross_origin_refusal(
                    &web.tagged(
                        &with_token(&path, &secret),
                        &[("Sec-Fetch-Site", "cross-site")],
                    )
                    .await,
                    &format!("{cell}: {path}, tagged cross-site and carrying this run's token"),
                );
            }
            refused += 1;
        }
        assert!(
            refused > 3,
            "{cell}: the population is {refused} path(s), which is fewer than the assets, the \
             two routes and the 404 this rule has to cover"
        );

        // …and `same-site` is refused too, which `cross-site` alone does not imply: a page at
        // another origin under the same registrable domain is still another origin.
        is_the_cross_origin_refusal(
            &web.tagged(PREVIEW_PATH, &[("Sec-Fetch-Site", "same-site")])
                .await,
            &format!("{cell}: the preview, tagged same-site"),
        );

        web.stop().await;
    }
}

#[tokio::test]
async fn a_foreign_origin_is_refused_and_so_is_a_null_one() {
    // The corroborating signal, on the route where it is the *only* one a browser sends: a
    // `new WebSocket(url)` carries an `Origin` and no Fetch Metadata (note **N93**), and a
    // WebSocket handshake is not subject to CORS — which is P5's review lens 1 in one sentence,
    // and the reason the ruling was asked for at all.
    //
    // `Origin: null` is the row worth its own line. A sandboxed iframe, a `file://` document and
    // some redirect chains all send it, and it is one careless `unwrap_or_default` away from
    // being read as the request that presented nothing — which this daemon admits, because that
    // is what `webcam-handler-client` looks like.
    let web = Web::opened(false).await;
    let secret = web.secret();
    let bound = web.bound();

    for origin in [
        "http://evil.example".to_owned(),
        "null".to_owned(),
        // Same authority, other scheme — a different origin, and one no page of ours can be
        // fulfilled at, since this listener speaks no TLS.
        format!("https://{bound}"),
        // Same host, other port.
        format!(
            "http://127.0.0.1:{port}",
            port = bound.port().wrapping_add(1)
        ),
    ] {
        for path in [RPC_PATH, PREVIEW_PATH, "/"] {
            is_the_cross_origin_refusal(
                &web.tagged(path, &[("Origin", &origin)]).await,
                &format!("{path}, from {origin}"),
            );
            is_the_cross_origin_refusal(
                &web.tagged(&with_token(path, &secret), &[("Origin", &origin)])
                    .await,
                &format!("{path}, from {origin}, carrying this run's token"),
            );
        }
    }

    // …and this listener's own origin still gets through, so the assertions above are about the
    // authority rather than about a rule that refuses every `Origin` — which would refuse the
    // client's own module graph and its WebSocket.
    assert_eq!(
        web.tagged(
            &with_token(RPC_PATH, &secret),
            &[("Origin", &format!("http://{bound}"))]
        )
        .await
        .status(),
        405,
        "the listener's own origin was refused"
    );

    web.stop().await;
}

#[tokio::test]
async fn a_request_claiming_no_provenance_is_admitted_because_the_primary_consumer_sends_none() {
    // **The deliberate weakening, asserted so it stays deliberate** (note **N93**).
    // `webcam-handler-client`, `curl` and the AI agent harness AGENTS names as this project's
    // *primary* consumer send neither header. The rule is therefore "refuse when a browser tells
    // us it is cross-site", never "require a same-origin header" — and this is the test that
    // goes red if somebody strengthens it into the second thing, which would lock out every
    // consumer that has no hands.
    //
    // Both cells, and every kind of answer this listener gives: a file, a route behind the gate,
    // and the 404. A request presenting nothing is what the rest of this suite sends, so a
    // failure here is also a failure of most of the file — which is the point: this is the one
    // test that says so on purpose.
    for insecure_loopback in [false, true] {
        let web = Web::opened(insecure_loopback).await;

        is_the_page(&web.anonymous("/").await, "the page, claiming nothing");
        assert_eq!(web.anonymous("/nothing-here").await.status(), 404);

        let (rpc, preview) = match web.token() {
            Some(secret) => (
                web.anonymous(&with_token(RPC_PATH, &secret)).await,
                web.anonymous(&with_token(PREVIEW_PATH, &secret)).await,
            ),
            None => (
                web.anonymous(RPC_PATH).await,
                web.anonymous(PREVIEW_PATH).await,
            ),
        };
        assert_eq!(rpc.status(), 405, "the wire route: {}", rpc.body());
        assert_eq!(preview.status(), 400, "the preview: {}", preview.body());

        web.stop().await;
    }
}

#[tokio::test]
async fn the_token_less_cell_is_the_one_this_rule_is_for() {
    // **The cell the ruling is about.** D11's `--http-insecure-loopback` installs no gate at
    // all, so before this rule a page at any origin in the operator's browser could open a
    // WebSocket to `127.0.0.1` and drive the whole T5 surface — a handshake is not subject to
    // CORS — and could paint the preview into an `<img>` it was not allowed to read. The owner's
    // second sentence bounds what this fixes rather than how far it reaches: *"`--http-insecure-
    // loopback` has `insecure` in it, so we'll fix all the obvious security holes, and accept
    // that we may miss some more subtle ones."*
    //
    // Three shapes at the two camera routes, and the control that makes them mean something: the
    // same requests untagged still reach the routes behind them, so this cell is still the cell
    // that serves without a credential.
    let web = Web::opened(true).await;
    assert_eq!(web.serving.posture().token(), TokenRule::NotRequired);
    assert_eq!(web.token(), None, "a token-less listener published one");

    for path in CAMERA_BEARING_PATHS {
        for (about, fields) in [
            (
                "a page in another site",
                vec![("Sec-Fetch-Site", "cross-site")],
            ),
            ("a foreign origin", vec![("Origin", "http://evil.example")]),
            (
                "a sandboxed iframe",
                vec![("Sec-Fetch-Site", "cross-site"), ("Origin", "null")],
            ),
        ] {
            is_the_cross_origin_refusal(
                &web.tagged(path, &fields).await,
                &format!("{path}, from {about}, in the cell with no token"),
            );
        }
    }
    for (path, past) in [(RPC_PATH, 405), (PREVIEW_PATH, 400)] {
        assert_eq!(
            web.anonymous(path).await.status(),
            past,
            "{path} in the token-less cell no longer answers a request that claims nothing"
        );
    }

    web.stop().await;
}

#[tokio::test]
async fn every_refused_shape_gets_the_same_answer() {
    // The refusal says which rule refused and nothing else. Four shapes that trip two different
    // signals — Fetch Metadata twice, `Origin` twice — and one answer byte for byte, so a caller
    // cannot learn from the response which header to drop. It is asserted as an equality between
    // the answers rather than as four comparisons against the constant, because a build that
    // grew a second refusal body would satisfy the second and fail this.
    //
    // And it is **not** the gate's answer: a `403` with no challenge, where the gate's is a
    // `401` with RFC 6750's. A client can tell "you are not our page" from "you did not present
    // the token", which is the one distinction this daemon's refusals do draw — because the
    // second is fixable by the caller and the first is not.
    let web = Web::opened(false).await;

    let mut answers = Vec::new();
    for fields in [
        vec![("Sec-Fetch-Site", "cross-site")],
        vec![("Sec-Fetch-Site", "same-site")],
        vec![("Origin", "http://evil.example")],
        vec![("Origin", "null")],
    ] {
        let answer = web.tagged(RPC_PATH, &fields).await;
        is_the_cross_origin_refusal(&answer, &format!("{fields:?}"));
        answers.push((answer.status(), answer.body().to_owned()));
    }
    let first = answers.first().expect("four shapes were driven").clone();
    for answer in &answers {
        assert_eq!(
            *answer, first,
            "two refused shapes were answered differently"
        );
    }

    // The gate's refusal, beside it, on the same route in the same run.
    is_the_refusal(
        &web.anonymous(RPC_PATH).await,
        "a request that claims nothing and presents nothing",
    );
    assert_ne!(provenance::REFUSAL, gate::REFUSAL);

    web.stop().await;
}

#[tokio::test]
async fn the_cross_origin_refusal_carries_the_no_referrer_policy_too() {
    // `every_answer_carries_the_no_referrer_policy`'s claim, at the answer that arrived after
    // it was written. The policy is applied outermost so it lands on **every** response this
    // listener writes, and a `403` is a response to a request whose URL may have carried the
    // token — which is exactly the case the header exists for.
    let web = Web::opened(false).await;

    assert_eq!(
        web.tagged(
            &with_token("/", &web.secret()),
            &[("Sec-Fetch-Site", "cross-site")]
        )
        .await
        .header("Referrer-Policy"),
        Some("no-referrer"),
        "the cross-origin refusal"
    );

    web.stop().await;
}

#[tokio::test]
async fn d11_loopback_without_the_flag_requires_the_token() {
    // **D11's first cell**: "loopback + token is the default". Reached through the composition
    // root's own function with no flag at all, which is what `webcam-handler-daemon --http`
    // does.
    let web = Web::opened(false).await;

    assert_eq!(web.serving.posture().token(), TokenRule::Required);
    assert_eq!(
        web.serving.posture().warning(),
        None,
        "warning about a socket only this machine can reach trains operators to ignore warnings"
    );
    is_the_refusal(
        &web.anonymous(PREVIEW_PATH).await,
        "the default cell, anonymously",
    );
    assert_eq!(
        web.bearing(PREVIEW_PATH, &web.secret()).await.status(),
        400,
        "the default cell, with the token: the preview asks which camera"
    );
    is_the_page(
        &web.anonymous("/").await,
        "the asset half of the default cell",
    );

    web.stop().await;
}

#[tokio::test]
async fn d11_loopback_with_the_named_flag_is_the_one_token_less_cell() {
    // **D11's second cell**: "token-less loopback exists only behind one named explicit flag".
    // The claim is stronger than "a request without a token is served" — it is that **no gate
    // is installed at all**, so the page comes back to a request carrying no credential in
    // either form, and the URL the daemon printed carries no token to leak.
    //
    // This is the cell the 2026-08-12 ruling leaves exactly as it was (note **N75**, whose
    // absent-rather-than-permissive half the ruling does not touch), so the camera-bearing
    // routes are asserted here too: in this one cell they answer *their own* answers to a
    // request presenting nothing, which is the difference between a gate that was narrowed and
    // a gate that was removed.
    let web = Web::opened(true).await;

    assert_eq!(web.serving.posture().token(), TokenRule::NotRequired);
    assert_eq!(web.token(), None, "a token-less listener published one");
    assert_eq!(web.url(), format!("http://{bound}/", bound = web.bound()));

    is_the_page(
        &web.anonymous("/").await,
        "the token-less cell, with no credentials at all",
    );
    is_the_page(&web.anonymous("/index.html").await, "the asset by name");
    // The router is still a router: this cell removes the gate, not the 404.
    assert_eq!(web.anonymous("/nothing-here").await.status(), 404);
    for (path, past) in [(RPC_PATH, 405), (PREVIEW_PATH, 400)] {
        assert_eq!(
            web.anonymous(path).await.status(),
            past,
            "{path} in the token-less cell answered something other than the route behind it"
        );
    }

    web.stop().await;
}

#[tokio::test]
async fn d11_a_non_loopback_bind_requires_the_token_with_no_flag_given() {
    // **D11's third cell**: "non-loopback **always** requires the token … and additionally
    // prints a warning naming what it exposes (a live camera)". The posture is injected — the
    // suite header says what that establishes and what it does not — and the socket is on
    // loopback, because a test may not assume this machine has an interface to bind.
    let web = Web::with_posture(Posture::of(address("192.168.1.10:8080"), false)).await;

    let warning = web
        .serving
        .posture()
        .warning()
        .expect("D11 requires a warning here");
    assert!(warning.contains("camera"), "{warning}");
    assert!(warning.contains("192.168.1.10:8080"), "{warning}");

    is_the_refusal(
        &web.anonymous(PREVIEW_PATH).await,
        "a non-loopback bind, anonymously",
    );
    assert_eq!(
        web.bearing(PREVIEW_PATH, &web.secret()).await.status(),
        400,
        "with the token"
    );

    web.stop().await;
}

#[tokio::test]
async fn d11_a_non_loopback_bind_requires_the_token_even_with_the_flag() {
    // **D11's fourth cell**, and the one the paragraph is emphatic about: "there is no flag
    // that removes it". The listener still *serves* — D11 prescribes a token and a warning,
    // not a refusal — and the warning names the flag that did nothing, because an operator who
    // typed it is otherwise left guessing why they met a 401.
    let web = Web::with_posture(Posture::of(address("0.0.0.0:8080"), true)).await;

    assert_eq!(web.serving.posture().token(), TokenRule::Required);
    let warning = web
        .serving
        .posture()
        .warning()
        .expect("D11 requires a warning here");
    assert!(
        warning.contains(daemon::http::INSECURE_LOOPBACK_FLAG),
        "{warning}"
    );

    is_the_refusal(
        &web.anonymous(RPC_PATH).await,
        "a wildcard bind with the insecure flag, anonymously",
    );
    is_the_refusal(
        &web.anonymous(&with_token(RPC_PATH, "")).await,
        "an empty token parameter, which is what a truncated URL carries",
    );
    assert_eq!(
        web.bearing(RPC_PATH, &web.secret()).await.status(),
        405,
        "with the token"
    );

    web.stop().await;
}

#[tokio::test]
async fn the_url_the_daemon_prints_carries_the_bound_port_and_opens_the_page() {
    // D11's "default `127.0.0.1:0` → report the bound port", asserted as the property that
    // makes the line worth printing: the URL an operator copies can be **opened**. It is taken
    // apart and used as a request rather than merely matched against a pattern, so a build
    // that printed the requested address would fail here by sending this test to port zero.
    //
    // **What this can no longer catch on its own is a build that published one secret and gated
    // on another**, because since the 2026-08-12 ruling the page is served to a request with no
    // credential at all and would come back whatever the URL carried. So the token the URL
    // carries is presented to a *gated* route at the end, which is where a published secret the
    // gate does not hold is a 401 rather than a page.
    let web = Web::opened(false).await;

    let bound = web.bound();
    assert_ne!(bound.port(), 0, "the requested port reached the URL");
    let url = web.url();
    assert_eq!(
        url,
        format!(
            "http://{bound}/?{TOKEN_QUERY_PARAM}={secret}",
            secret = web.secret()
        )
    );

    // Opened the way a browser opens it: everything after the authority is the request target,
    // and the address it names is the one this test connects to.
    let (authority, target) = url
        .trim_start_matches("http://")
        .split_once('/')
        .expect("a URL with a path");
    assert_eq!(authority, bound.to_string());
    let answered = tcp::get(address(authority), &format!("/{target}"), &[])
        .await
        .expect("the listener is up");
    is_the_page(&answered, "the URL the daemon printed");

    // ... and the credential that URL carries is the one the gate holds.
    assert_eq!(
        web.anonymous(&with_token(RPC_PATH, &web.secret()))
            .await
            .status(),
        405,
        "the published token did not open a gated route"
    );

    web.stop().await;
}

/// The daemon's **always-on** transport, started beside the opt-in one.
///
/// One test needs it, for one claim that cannot be made any other way:
/// `limits::DAEMON_MAX_CONNECTIONS` is a bound *per transport* — `daemon::http::rpc` says so and
/// D11's "opt-in" is the reason — so "a browser that leaks sockets cannot exhaust the transport
/// `webcam-handler-client` uses" is a statement about two counters, and a suite with one socket
/// cannot tell one counter from two.
///
/// It serves the same empty surface [`no_methods`] hands the web listener: what is being asked
/// of it is whether it **answers at all** while the other transport is at its cap, and a
/// registration with methods on it would not make that answer truer. `-32601` is what an empty
/// registration says, and it is a document that crossed the socket both ways.
struct Unix {
    handle: uds::Serving,
    dir: SocketDir,
    _lock: StoreLock,
    _store: TempStore,
    _runtime: TempRuntimeDir,
}

impl Unix {
    /// Bind a private socket directory and serve it. Must be called inside a runtime.
    fn start() -> Unix {
        let store = TempStore::new().expect("a state directory");
        let lock = store
            .store()
            .lock(LockProtocol::HeldForLifetime)
            .expect("nothing else holds a throw-away state directory");
        let runtime = TempRuntimeDir::new().expect("a runtime directory");
        let dir = SocketDir::prepare(&runtime.env()).expect("a fresh, private socket directory");
        let listener = dir.bind(&lock).expect("nothing is in the way");

        Unix {
            handle: uds::serve(listener, no_methods()),
            dir,
            _lock: lock,
            _store: store,
            _runtime: runtime,
        }
    }

    fn socket(&self) -> camino::Utf8PathBuf {
        self.dir.socket_path()
    }

    /// Stop it the way `daemon::shutdown` does, and wait for the accept loop and its
    /// connections — a listener nobody joined is a listener whose ending nothing waited for.
    async fn stop(mut self) {
        self.handle.stop();
        self.handle
            .stopped()
            .await
            .expect("the Unix socket was asked to stop");
    }
}

/// Send one `GET` on a connection **the test already opened**, and read to end-of-file.
///
/// [`tcp::get`] connects, sends and parses in one call, and each of those is wrong here. The
/// subject is what happens to a connection whose accept the test can reason about, so opening
/// it and using it have to be two steps; and a connection that was *refused* has no HTTP answer
/// to parse, which `Answer::of` panics on — turning the very outcome this test is looking for
/// into a failure that reads like a broken helper.
///
/// `Connection: close` for `support::call`'s reason: the server ends the stream, so the read
/// finishes at end-of-file rather than on a clock.
async fn ask(stream: &mut TcpStream, bound: SocketAddr, target: &str) -> std::io::Result<String> {
    let framed = format!(
        "GET {target} HTTP/1.1\r\n\
         Host: {bound}\r\n\
         Connection: close\r\n\
         \r\n"
    );
    stream.write_all(framed.as_bytes()).await?;
    let mut answered = String::new();
    stream.read_to_string(&mut answered).await?;
    Ok(answered)
}

#[tokio::test]
async fn connections_are_bounded_at_the_number_this_project_chose_on_this_transport_too() {
    // **`uds.rs`'s `connections_are_bounded_at_the_number_this_project_chose`, on the transport
    // that did not have the bound.** `limits::DAEMON_MAX_CONNECTIONS`'s doc says what it is for
    // — "so a client that leaks connections is refused rather than able to exhaust the daemon's
    // file descriptors, which on this process are also the camera's" — and jsonrpsee's
    // `max_connections` is not it: that permit is taken per in-flight HTTP *request* and
    // released with the response. `daemon::uds` measured that before this listener existed
    // (with the cap at 32, 128 idle connections were all accepted and held) and grew an
    // owned-permit semaphore at accept; the web listener was spawned as a bare `axum::serve`
    // and never got one, which P5's adversarial review found.
    //
    // **Every held connection sends nothing, and that is the whole shape of the exposure.** A
    // connection that sends no byte reaches no gate — `daemon::http::gate` is a `route_layer`,
    // which is per request — takes no jsonrpsee permit, and matches no route, so it is refused
    // by nothing and costs a descriptor for as long as it is held. It is also completely
    // unauthenticated for the same reason: there is no credential in a connection that never
    // spoke.
    //
    // **The connection past the bound is the one that asks for something**, because "refused"
    // has to be observable: a bounded listener has already dropped it, so the request meets a
    // closed socket, while an unbounded one answers it with the page. Both outcomes are
    // immediate and neither waits on a clock — which is what makes this test go red rather than
    // time out on the build it was written against.
    let unix = Unix::start();
    let web = Web::opened(false).await;
    let bound = web.bound();
    let cap = usize::try_from(limits::DAEMON_MAX_CONNECTIONS).expect("a small bound");

    // At the bound, idle. `connect` returns when the handshake completes, and the kernel's
    // accept queue is FIFO, so by the time the listener looks at the connection below it has
    // taken a permit for every one of these — no ordering here is left to a scheduler.
    let mut held = Vec::with_capacity(cap);
    for connection in 0..cap {
        held.push(
            TcpStream::connect(bound)
                .await
                .unwrap_or_else(|err| panic!("connection {connection} was refused: {err}")),
        );
    }

    // One past it. Either an immediate end-of-stream or a reset, depending on how far the write
    // got before the daemon dropped its end — `uds.rs` says the same thing about the same
    // moment. What matters is that no answer came back: the refusal *is* the connection going
    // away, because there is no request yet for a `503` to be an answer to.
    let mut past = TcpStream::connect(bound)
        .await
        .expect("the kernel completes a handshake into the listener's backlog");
    match ask(&mut past, bound, "/").await {
        Ok(answered) => assert!(
            answered.is_empty(),
            "a connection past this transport's bound was served: {status}",
            // The status line alone: what is being said is that *something* came back, and a
            // failure carrying the client's whole entry page buries that under the page.
            status = answered.lines().next().unwrap_or_default()
        ),
        Err(err) => assert_eq!(err.kind(), std::io::ErrorKind::ConnectionReset, "{err}"),
    }

    // **And the daemon's always-on transport is untouched**, which is the half of
    // `daemon::http::rpc`'s claim that was true and is worth keeping: the two bounds are two
    // counters, so a browser that leaks sockets cannot cost `webcam-handler-client` the one
    // socket this daemon always serves. A build that shared one semaphore between the
    // transports passes every assertion above and fails here.
    let answered = call(
        &unix.socket(),
        r#"{"jsonrpc":"2.0","id":1,"method":"nothing_is_registered_here","params":{}}"#,
    )
    .await
    .expect("the Unix socket is listening");
    assert!(answered.starts_with("HTTP/1.1 200 OK"), "{answered}");
    assert!(body(&answered).contains("-32601"), "{}", body(&answered));

    // …and a connection the listener accepted *before* the bound was reached is still served,
    // so what happened above is a bound and not a listener that stopped answering. This is also
    // what says the held connections were accepted rather than left in the kernel's backlog.
    let first = held.first_mut().expect("the bound is not zero");
    let served = ask(first, bound, "/")
        .await
        .expect("a connection the listener accepted inside the bound");
    assert!(served.starts_with("HTTP/1.1 200 OK"), "{served}");

    web.stop().await;
    unix.stop().await;
}

/// Everything this **process** logs, as bytes a test can read back.
///
/// Three things about it are load-bearing, and the first two are why
/// `daemon::logging::capturing` — the workspace's other test writer — could not be used here
/// even if an integration test could see it (it is `#[cfg(test)]` in the library, so it does
/// not exist in the artifact this binary links).
///
/// **The subscriber is global, not thread-local.** The line under test is written by
/// jsonrpsee's `TowerService::call`, which runs in the task `axum::serve` spawned for the
/// connection — never on the thread that installed a default. `tracing::subscriber::set_default`
/// captures the installing thread and nothing else, so a capture built that way would read an
/// empty buffer and pass whatever the daemon logged. That is the shape of test this project
/// bans by name, and it is the reason this is the only global subscriber in the workspace.
///
/// **The level is `TRACE`.** `tracing_subscriber::fmt`'s default is `INFO` and the event in
/// question is a `TRACE` on somebody else's target, so a capture at the default would be the
/// same passing-without-asserting test wearing a filter.
///
/// **It is installed once per process**, which under `cargo nextest` — what `just ci` runs — is
/// once per test. Under `cargo test` the whole binary shares it, and the only consequence is
/// that the buffer carries the other tests' lines as well: every assertion below is about *this*
/// listener's secret, which is minted per run and appears in no other test's request.
#[derive(Clone, Debug, Default)]
struct CapturedLog(Arc<std::sync::Mutex<Vec<u8>>>);

impl CapturedLog {
    /// Install this as the process's one subscriber, at `TRACE`, and hand it back.
    fn installed_globally() -> CapturedLog {
        let captured = CapturedLog::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(captured.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("this is the only test in this binary that installs a subscriber");
        captured
    }

    /// Everything written so far, as an operator reading the journal would have.
    fn rendered(&self) -> String {
        let bytes = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

impl std::io::Write for CapturedLog {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CapturedLog {
    type Writer = CapturedLog;

    fn make_writer(&'a self) -> CapturedLog {
        self.clone()
    }
}

#[tokio::test]
async fn neither_form_of_the_credential_reaches_the_daemons_own_log() {
    // **The residual `daemon::http::rpc`'s header claims is closed, asserted at the only place
    // it can be observed: the bytes the daemon writes.** jsonrpsee logs the whole
    // `http::Request` at `TRACE` — `Debug` of the request line *and* the header map — so a
    // daemon under `RUST_LOG=trace` decides, in this one line, whether the key to somebody's
    // camera is in a journal that outlives the process. Under systemd that journal is
    // persistent and readable by `systemd-journal` and `adm`, which is the multi-user boundary
    // D11's "loopback alone is not an auth boundary" is about.
    //
    // **Both forms, because the gate accepts both and stripping one is not a fix.** The query
    // form is what a `new WebSocket(url)` carries (note **N74**), and the header form is what
    // this daemon's own `401` recommends (`daemon::http::gate::REFUSAL`) and what every client
    // that can set headers sends — `curl`, a script, and this suite. Both are driven at a
    // camera-bearing route, because a credential presented to an ungated one never reaches the
    // wire surface that does the logging.
    //
    // **The non-vacuity assertion is the request's own `Host` header**, and it is what makes
    // this test able to fail. The capture is global and at `TRACE` (see [`CapturedLog`]) for
    // reasons that are easy to get subtly wrong, and every one of those mistakes produces an
    // empty buffer, which satisfies "the secret is not in it". So the buffer has to be shown to
    // carry the *sibling header* of the one being watched: `host` is in the same rendering of
    // the same map, and a build that logs it logs `authorization` beside it unless something
    // took that header off.
    let captured = CapturedLog::installed_globally();
    let web = Web::opened(false).await;
    let secret = web.secret();
    let bound = web.bound();

    // Past the gate in both forms — 405 is jsonrpsee's answer to a `GET` that is not an upgrade,
    // so each request provably reached the service that does the logging.
    assert_eq!(
        web.anonymous(&with_token(RPC_PATH, &secret)).await.status(),
        405,
        "the query form did not reach the wire surface"
    );
    assert_eq!(
        web.bearing(RPC_PATH, &secret).await.status(),
        405,
        "the header form did not reach the wire surface"
    );

    web.stop().await;

    let rendered = captured.rendered();
    assert!(
        rendered.contains("jsonrpsee"),
        "nothing from the wire surface's own target reached this capture, so it could not \
         have seen the line the credential rides on:\n{rendered}"
    );
    assert!(
        rendered.contains(&bound.to_string()),
        "the request's own Host header is not in this capture, so nothing here would notice a \
         credential beside it:\n{rendered}"
    );
    assert!(
        !rendered.contains(&secret),
        "the daemon wrote the token this run minted into its own log"
    );
}

#[tokio::test]
async fn the_listener_stops_when_the_shutdown_token_is_cancelled() {
    // The lifecycle claim, and the reason it is a *join*: the composition root waits for this
    // task, so "the web listener ended" is a fact rather than a consequence of the process
    // exiting. A build whose server ignored the cancellation hangs here and becomes a named
    // nextest TIMEOUT rather than a green test — which is the shape of "the process does not
    // hang" that a test can actually have.
    let web = Web::opened(false).await;
    let bound = web.bound();

    // Alive first, so the assertion after the stop is about the stop.
    is_the_page(&web.anonymous("/").await, "before the stop was asked for");

    web.stop().await;

    // And nothing is listening on that port any more. The connection is refused by the kernel,
    // which is what a port with nobody behind it does.
    let refused = tokio::net::TcpStream::connect(bound)
        .await
        .expect_err("something is still listening");
    assert_eq!(refused.kind(), std::io::ErrorKind::ConnectionRefused);
}
