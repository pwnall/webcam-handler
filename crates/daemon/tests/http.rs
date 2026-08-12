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
//! Nothing here waits on a clock: the client's read ends at end-of-file (`support/tcp.rs`),
//! and the stop is a cancellation followed by a join.

#[path = "support/tcp.rs"]
mod tcp;

use std::net::SocketAddr;
use std::sync::Arc;

use daemon::http::{
    self, CAMERA_BEARING_PATHS, PREVIEW_PATH, Posture, RPC_PATH, TOKEN_QUERY_PARAM, Token,
    TokenRule, gate,
};
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

/// The wire surface this suite hands the listener: none.
///
/// See the header. The listener takes the T5 surface as a value, so a suite whose subject is
/// the credential can hand it an empty one and still be driving the function `wchd` calls —
/// and a suite that handed it a real one would be claiming, in this file, something
/// `web_rpc.rs` claims properly. What it does **not** mean is that the endpoint is absent:
/// `daemon::http::rpc`'s route is registered either way, which is why
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
    let web = Web::opened(false).await;

    is_the_page(&web.anonymous("/").await, "the page, with no credential");
    is_the_page(&web.anonymous("/index.html").await, "the page by name");
    assert_eq!(
        web.anonymous("/nothing-here").await.status(),
        404,
        "a path that names no asset answered something other than the asset table's 404"
    );
    for path in CAMERA_BEARING_PATHS {
        is_the_refusal(&web.anonymous(path).await, path);
    }

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
    // they are two mechanisms a build can lose one at a time: the header is what code sends —
    // the page's own `fetch` and its `<img>` — and the query parameter is what a URL can carry,
    // which is all a `new WebSocket(url)` has (note **N74**).
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
        "the page's own request, made from a URL that still carries the token"
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
