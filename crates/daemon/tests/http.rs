//! The opt-in TCP transport, end to end: a real socket, a real axum server, a hand-written
//! client (D11, docs/7 P5a).
//!
//! `uds.rs` is this suite's model — it asserts that a byte stream over `AF_UNIX` carries a
//! JSON-RPC call and its answer — and this is the same altitude about the other transport:
//! that a byte stream over TCP carries an HTTP request, that D11's matrix decides whether it
//! is answered, and that the server starts and stops on demand. What the client *shows* a
//! person is P5c's, and what the WebSocket and the preview do is P5b's.
//!
//! ## What the suite drives, and what that is worth
//!
//! Two entry points, deliberately, because they establish different things.
//!
//! [`Web::opened`] calls `daemon::http::open` — **the function `wchd`'s composition root
//! calls**, which decides the posture from the address, mints the token only where D11 gates
//! it, binds, and serves. Everything after that in `main.rs` is: log what it answers, join it.
//! So a test that opens a listener this way and then opens the URL it published is asserting
//! about the shipped daemon and not about a rehearsal of it.
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
//! Nothing here waits on a clock: the client's read ends at end-of-file (`support/tcp.rs`),
//! and the stop is a cancellation followed by a join.

#[path = "support/tcp.rs"]
mod tcp;

use std::net::SocketAddr;
use std::sync::Arc;

use daemon::http::{self, Posture, TOKEN_QUERY_PARAM, Token, TokenRule, gate};
use daemon::shutdown::Shutdown;

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
        let serving = http::open(address("127.0.0.1:0"), insecure_loopback, shutdown.clone())
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
        let serving = http::serve(listener, posture, token, shutdown.clone())
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

    /// A `GET` presenting the token the way the page's own requests will (P5b).
    async fn bearing(&self, target: &str, token: &str) -> Answer {
        tcp::get(
            self.bound(),
            target,
            &[("Authorization", &format!("Bearer {token}"))],
        )
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

/// The `?token=…` a navigation carries.
fn query(secret: &str) -> String {
    format!("/?{TOKEN_QUERY_PARAM}={secret}")
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
async fn an_anonymous_request_is_refused_and_the_token_serves_the_page_in_either_form() {
    // docs/9's "Token enforcement" row — 401-without/200-with — and both forms of the token,
    // because they are two different mechanisms and a build can lose either one on its own:
    // the header is what code sends, the query parameter is what a *navigation* can carry, and
    // an operator's first request is a navigation.
    let web = Web::opened(false).await;
    let secret = web.secret();

    is_the_refusal(&web.anonymous("/").await, "a request with no credential");
    is_the_page(
        &web.bearing("/", &secret).await,
        "the header form, which is what the page's own requests will use",
    );
    is_the_page(
        &web.anonymous(&query(&secret)).await,
        "the query form, which is what a browser opening a link can carry",
    );

    web.stop().await;
}

#[tokio::test]
async fn an_anonymous_request_for_the_asset_route_is_refused_whatever_it_asks_for() {
    // The asset the gate protects is the page that drives the camera, so the asset route is
    // the route that matters — a gate installed over everything *except* the file a browser
    // actually loads would pass a test that only ever asked for `/`.
    //
    // The third case is the one that says the gate is outside the router rather than inside
    // it: a path this daemon does not serve is answered **401 and not 404** while it is
    // anonymous, so a stranger cannot map the surface by watching which paths answer
    // differently.
    let web = Web::opened(false).await;
    let secret = web.secret();

    for target in ["/", "/index.html", "/nothing-here"] {
        is_the_refusal(&web.anonymous(target).await, target);
    }

    // And with the token the router answers normally, which is what makes the assertions above
    // about the *gate* rather than about a listener that refuses everything.
    is_the_page(
        &web.bearing("/index.html", &secret).await,
        "the page by name",
    );
    assert_eq!(
        web.bearing("/nothing-here", &secret).await.status(),
        404,
        "an authenticated request for a path this build does not serve"
    );

    web.stop().await;
}

#[tokio::test]
async fn a_near_miss_token_is_refused_over_the_wire() {
    // The gate is *checking*, not looking for a parameter. An equal-length, one-digit-different
    // token is the only candidate that reaches `Token::verify`'s comparison loop, and it is
    // presented in both forms because they are two code paths into the same answer.
    let web = Web::opened(false).await;
    let wrong = near_miss(&web.secret());

    is_the_refusal(&web.bearing("/", &wrong).await, "a near miss in the header");
    is_the_refusal(
        &web.anonymous(&query(&wrong)).await,
        "a near miss in the query",
    );

    // And a **prefix**, which is the shape a wrapped or truncated URL arrives in — and the
    // shape a gate that compared with `starts_with` instead of `Token::verify` would serve
    // while refusing every equal-length candidate above.
    let secret = web.secret();
    let truncated = &secret[..secret.len() - 8];
    is_the_refusal(
        &web.anonymous(&query(truncated)).await,
        "a truncated token in the query",
    );

    // ... and the token these are all near misses *of* still works, so this is a statement
    // about the digits and not about a daemon that has stopped serving.
    is_the_page(&web.bearing("/", &secret).await, "the real token");

    web.stop().await;
}

#[tokio::test]
async fn a_request_carrying_two_credentials_is_served_only_when_they_agree() {
    // `daemon::http::gate`'s rule, over the wire rather than over a `HeaderMap`: every
    // credential a request presents must verify. The two shapes are the ones HTTP allows and
    // clients do not send — a header beside a query parameter, and the same parameter twice —
    // and each is served by exactly one of the two gates this project refused to build (first
    // wins, last wins).
    let web = Web::opened(false).await;
    let secret = web.secret();
    let wrong = near_miss(&secret);

    is_the_page(
        &web.bearing(&query(&secret), &secret).await,
        "a page request made from a URL that still carries the token",
    );
    is_the_refusal(
        &web.bearing(&query(&wrong), &secret).await,
        "a good header beside a bad query parameter",
    );
    is_the_refusal(
        &web.anonymous(&format!(
            "/?{TOKEN_QUERY_PARAM}={secret}&{TOKEN_QUERY_PARAM}={wrong}"
        ))
        .await,
        "a good query parameter followed by a bad one",
    );
    is_the_refusal(
        &web.anonymous(&format!(
            "/?{TOKEN_QUERY_PARAM}={wrong}&{TOKEN_QUERY_PARAM}={secret}"
        ))
        .await,
        "a bad query parameter followed by a good one",
    );

    // Two `Authorization` headers, which is well-formed HTTP and which `HeaderMap::get` would
    // answer with only the first of.
    let answered = tcp::get(
        web.bound(),
        "/",
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
async fn d11_loopback_without_the_flag_requires_the_token() {
    // **D11's first cell**: "loopback + token is the default". Reached through the composition
    // root's own function with no flag at all, which is what `wchd --http` does.
    let web = Web::opened(false).await;

    assert_eq!(web.serving.posture().token(), TokenRule::Required);
    assert_eq!(
        web.serving.posture().warning(),
        None,
        "warning about a socket only this machine can reach trains operators to ignore warnings"
    );
    is_the_refusal(&web.anonymous("/").await, "the default cell, anonymously");
    is_the_page(&web.bearing("/", &web.secret()).await, "the default cell");

    web.stop().await;
}

#[tokio::test]
async fn d11_loopback_with_the_named_flag_is_the_one_token_less_cell() {
    // **D11's second cell**: "token-less loopback exists only behind one named explicit flag".
    // The claim is stronger than "a request without a token is served" — it is that **no gate
    // is installed at all**, so the page comes back to a request carrying no credential in
    // either form, and the URL the daemon printed carries no token to leak.
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
        &web.anonymous("/").await,
        "a non-loopback bind, anonymously",
    );
    is_the_page(&web.bearing("/", &web.secret()).await, "with the token");

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
        &web.anonymous("/").await,
        "a wildcard bind with the insecure flag, anonymously",
    );
    is_the_refusal(
        &web.anonymous(&query("")).await,
        "an empty token parameter, which is what a truncated URL carries",
    );
    is_the_page(&web.bearing("/", &web.secret()).await, "with the token");

    web.stop().await;
}

#[tokio::test]
async fn the_url_the_daemon_prints_carries_the_bound_port_and_opens_the_page() {
    // D11's "default `127.0.0.1:0` → report the bound port", asserted as the property that
    // makes the line worth printing: the URL an operator copies can be **opened**. It is taken
    // apart and used as a request rather than merely matched against a pattern, so a build
    // that printed the requested address would fail here by sending this test to port zero —
    // and one that printed a token the gate does not accept would fail with a 401.
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

    web.stop().await;
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
    is_the_page(
        &web.bearing("/", &web.secret()).await,
        "before the stop was asked for",
    );

    web.stop().await;

    // And nothing is listening on that port any more. The connection is refused by the kernel,
    // which is what a port with nobody behind it does.
    let refused = tokio::net::TcpStream::connect(bound)
        .await
        .expect_err("something is still listening");
    assert_eq!(refused.kind(), std::io::ErrorKind::ConnectionRefused);
}
