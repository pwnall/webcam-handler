//! The token gate — D11's "requires a bearer token", as the one thing a request for the camera
//! passes through (docs/7 P5a; D11's 2026-08-12 amendment).
//!
//! [`super::token`] is the secret and the comparison; this is the *policy* about a request:
//! which parts of it may carry a credential, what it means when more than one does, and what
//! a request that carries none is answered with. The listener installs it over the two routes
//! that carry or drive the camera — [`super::CAMERA_BEARING_PATHS`] — and over nothing else,
//! because the owner ruled (2026-08-12) that the static assets are open-source code rather
//! than a secret and are served without authentication (note **N82**).
//! `super::listener::router` is where that is composed; its header argues both halves — why
//! the assets are outside the gate, and why the token-less cell has no gate at all rather than
//! a gate that says yes.
//!
//! **Nothing in this module changed when that ruling landed, and that is the point.** The
//! credential shapes, the rule about presenting more than one, and the refusal are what they
//! were; what moved is the set of requests that reach them. A gate narrowed by adding a
//! `if path == …` branch to [`check`] would have been the same ruling implemented in the one
//! function whose whole job is to say no.
//!
//! **Nor did it change when the second ruling landed, which is the same point twice.** Since
//! 2026-08-13 this listener has a second admission rule — [`super::provenance`], which refuses a
//! request a browser reports as coming from another origin — and it is a module beside this one
//! rather than a clause inside `admits`. The two ask different questions (*did you present the
//! credential* against *are you our own page*), are installed over different sets of paths
//! (`route_layer` over the camera routes against `layer` over everything), and are installed in
//! different numbers of D11's cells (three against four). Provenance runs **first**, so a request
//! this gate refuses is one that already claimed no foreign origin — and a cross-origin request
//! never reaches [`Token::verify`] at all, whatever credential it carries.
//!
//! ## Two forms, and both are load-bearing
//!
//! - **`?token=<token>`** — what a *navigation* can carry, and **what every request the page
//!   makes carries too**. A browser opening a link sends the headers it chose, and there is no
//!   way to attach an `Authorization` to it short of an extension, so the first request an
//!   operator makes has to authenticate with what a URL can hold; and the two things the page
//!   then reaches — a `new WebSocket(url)` and an `<img src>` — have no API for a header
//!   either, which is why P5c's client writes the token into a URL in both cases
//!   (`webcam-handler-web`'s `credential.js`, note **N74**, [`super::rpc`]'s header).
//!   [`super::token::Token::ready_to_open_url`] builds exactly that form, and
//!   [`super::TOKEN_QUERY_PARAM`] is the one spelling of the parameter's name.
//! - **`Authorization: Bearer <token>`** — what a client that *can* set a header sends: `curl`,
//!   a script, and this crate's own suites, which is also who [`REFUSAL`] recommends it to.
//!   **No part of this project's web client uses it**, and that sentence used to read the
//!   other way round — it said this was "what the page's own requests use", which was written
//!   before P5c had a page and was wrong the day it did. P5's adversarial review found it
//!   holding up a claim one module along: [`super::rpc`]'s residual 4 reasoned that a request
//!   with no query string was the page's ordinary shape, and so stripped only the query before
//!   handing the request to a service that logs it whole.
//!
//! Neither is a fallback for the other and neither takes precedence — see below, because
//! "precedence" is the word the interesting failure hides behind.
//!
//! ## The rule, stated once: **every credential the request presents must verify, and it must
//! present at least one**
//!
//! Not "the first one wins", not "the last one wins", not "the header beats the query". The
//! two questions this has to answer are a request carrying *both* forms and a request
//! carrying `?token=` *twice*, and they have the same answer for the same reason.
//!
//! A first-wins gate and a last-wins gate are different gates, and the difference is
//! reachable by anybody who can get a `&token=…` appended to a URL an operator is about to
//! open — HTTP parameter pollution, which is a live technique precisely because the layers of
//! a stack disagree about it: a proxy, a browser, a URL library and a server-side deserializer
//! each pick their own end of the string. A gate whose answer depends on that convention is a
//! gate whose answer depends on which piece of software parsed the URL last. **This one does
//! not have a convention to disagree with**: if two `token=` parameters disagree, no answer is
//! right, so the request is refused; if they agree, whichever one any layer picks is the same
//! verified token. The same argument applies letter for letter to a request that presents a
//! header and a query parameter, and to the (equally well-formed HTTP) case of two
//! `Authorization` headers.
//!
//! The cost is a request carrying one good credential and one bad one being refused, and that
//! is not a cost worth avoiding: no client this project ships sends two, and a request that
//! presents two different credentials is a request whose author does not know what it is
//! presenting.
//!
//! ## What is deliberately not decoded
//!
//! **No percent-decoding and no `+`-for-space.** The token is 64 lowercase hex digits
//! ([`super::TOKEN_BYTES`]) and every one of them is unreserved in a URL, so the form this
//! gate reads is the form [`super::token::Token::ready_to_open_url`] wrote and the form a
//! browser sends it back in. A decoder would be a second way to spell the secret — `%74` for
//! `t` — which is more ways for a candidate to be accepted, in the one place in this daemon
//! where "more ways" is the wrong direction. What it costs is that a client which
//! percent-encoded its token meets a 401; that errs closed, which is where D11 errs.
//!
//! ## What it does not claim
//!
//! **Nothing about the layers underneath.** The HTTP parser that split these headers, the
//! router that would have matched the path, and the allocator all have timing of their own;
//! [`super::token::Token::verify`]'s doc states the same residual about the comparison it
//! owns. And nothing here is a claim about *authorization*: there is one token and it opens
//! every route this gate is installed over, because what it protects is a single-user
//! machine's camera, not a multi-tenant surface (D11). Which routes those are is
//! `super::listener::router`'s decision and not this module's — a per-path opinion here
//! would be a second home for it (design §2.10), and the wrong one, since the composition is
//! read once at startup and this runs on every request.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::token::{TOKEN_QUERY_PARAM, Token};

/// The challenge a refusal carries, exactly as RFC 6750 spells the scheme.
///
/// A bare `Bearer` with no `realm` and no `error=` parameter. The realm would be a name for a
/// protection space that has one member; `error="invalid_token"` would tell a caller which of
/// "you sent nothing" and "you sent the wrong thing" happened, which is the one distinction
/// this refusal is careful not to draw.
pub const BEARER_CHALLENGE: &str = "Bearer";

/// What a refused request is told, in full.
///
/// Short, fixed, and a `const` so a test can assert the daemon says this and not something
/// else. Three things it deliberately is not: it is not the token (the refusal is read by
/// whoever failed to present one), it is not a stack trace or a `Debug` rendering of anything
/// (`super::token`'s header is about exactly that class of leak), and it is not a D13 error
/// document. D13 is the *method* vocabulary — it crosses the JSON-RPC wire with a code a
/// client branches on — and inventing an HTTP projection of it here would be a second home
/// for the error law (design §2.10) built for one refusal that has no method behind it: the
/// gate answers before the route's handler has run, and a `401` is all a client has to branch
/// on.
pub const REFUSAL: &str = "this listener serves a live camera and needs the bearer token \
                           this run printed — open the URL webcam-handler-daemon logged at \
                           startup, or send it as `Authorization: Bearer <token>`\n";

/// The middleware itself: verify, or refuse without running the route.
///
/// Installed with [`axum::middleware::from_fn_with_state`] and [`axum::Router::route_layer`],
/// so it runs **only for a request that matched a route** — which since the 2026-08-12 ruling
/// is the whole policy: the routes are the camera and the fallback is the asset table. An
/// anonymous request for `/nothing-here` is therefore answered `404` by the assets and never
/// reaches this function, and an anonymous request for [`super::PREVIEW_PATH`] is answered
/// here.
///
/// It takes no path and reads none. What this function is installed over is a fact about the
/// composition (`super::listener::router`), and a `match` on the request's path *here* would
/// be the same policy written in the place where an inverted condition serves a camera to
/// anybody who asks.
pub async fn check(State(token): State<Arc<Token>>, request: Request, next: Next) -> Response {
    if admits(request.headers(), request.uri().query(), &token) {
        next.run(request).await
    } else {
        refusal()
    }
}

/// `401`, with the challenge and the fixed body.
fn refusal() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static(BEARER_CHALLENGE),
        )],
        REFUSAL,
    )
        .into_response()
}

/// The whole policy, over values: does this request present a credential, and does everything
/// it presents verify?
///
/// A function of a [`HeaderMap`] and the query string rather than of a `Request`, so the
/// matrix of shapes below — both forms, duplicates, a wrong scheme, a parameter with no
/// value — is asserted without a socket, a router or a runtime (AGENTS, "pure cores take
/// values"). The `Request`-shaped half is [`check`], which is four lines and has nothing in
/// it to get wrong that this does not already cover.
///
/// **The fold does not short-circuit.** `&=` over every credential rather than
/// [`Iterator::all`]'s early exit: the count of credentials a request carries is public and
/// this is not where a timing claim lives ([`Token::verify`] is), but a loop that stops at the
/// first failure is one rewrite away from stopping at the first *success*, which is the
/// first-wins gate this module's header refuses.
fn admits(headers: &HeaderMap, query: Option<&str>, token: &Token) -> bool {
    let mut presented = 0_usize;
    let mut every_one_verified = true;

    // `get_all`, not `get`: two `Authorization` headers is a well-formed HTTP request, and a
    // gate that read the first would be the first-wins gate wearing a different hat.
    for value in headers.get_all(header::AUTHORIZATION) {
        presented += 1;
        every_one_verified &= bearer(value).is_some_and(|candidate| token.verify(candidate));
    }

    for candidate in query_tokens(query.unwrap_or_default()) {
        presented += 1;
        every_one_verified &= token.verify(candidate);
    }

    presented > 0 && every_one_verified
}

/// The credential out of one `Authorization` header, when it is a `Bearer` one.
///
/// `None` covers every way the header can fail to be a bearer credential, and all of them are
/// then counted as a credential that did not verify rather than as no credential at all: a
/// value that is not text (`HeaderValue` may hold arbitrary bytes), a value with no space in
/// it, and a value whose scheme is something else. A request that says `Authorization: Basic
/// …` **has** presented something, and answering it as though it had presented nothing would
/// be the gate quietly forgiving the one request that is definitely confused about how to
/// talk to this daemon.
///
/// The scheme is compared case-insensitively because RFC 9110 says auth schemes are, and the
/// credential is trimmed of the leading spaces the grammar allows more than one of. Nothing
/// is trimmed from the *end*: the HTTP parser has already stripped the optional whitespace
/// around a field value, so anything left there is part of what the caller sent, and a
/// candidate with a trailing space is not the token.
fn bearer(value: &HeaderValue) -> Option<&str> {
    let text = value.to_str().ok()?;
    let (scheme, credential) = text.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case(BEARER_CHALLENGE) {
        return None;
    }
    Some(credential.trim_start_matches(' '))
}

/// Every `token=` parameter in a query string, in the order they appear.
///
/// Hand-parsed rather than deserialized, which is the whole reason axum's `query` feature is
/// off (this crate's manifest states it): a deserializer into a map answers the
/// duplicate-parameter question by picking one, silently, and that question is this module's
/// header. Splitting on `&` and then on the first `=` is the whole grammar a query string has.
///
/// A parameter named `token` with no `=` at all is yielded as an **empty candidate** rather
/// than skipped, so `?token` is a credential that fails to verify rather than a request that
/// presented nothing. The difference matters at exactly one place — a URL whose token was
/// truncated by a mail client at the `=` should meet a refusal, not an unauthenticated 200 in
/// the cell where that would be a hole.
///
/// Names are compared whole: `?tokens=…` and `?xtoken=…` are other people's parameters and
/// are not credentials.
fn query_tokens(query: &str) -> impl Iterator<Item = &str> {
    query
        .split('&')
        .filter_map(|pair| match pair.split_once('=') {
            Some((name, value)) if name == TOKEN_QUERY_PARAM => Some(value),
            Some(_) => None,
            None if pair == TOKEN_QUERY_PARAM => Some(""),
            None => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A token, and the request shapes built around it.
    fn minted() -> Token {
        Token::mint().expect("the kernel has a CSPRNG")
    }

    /// An equal-length, one-digit-different candidate — the shape `Token::verify`'s loop is
    /// the whole reason for, spelled here so the gate is asserted to be *checking* rather
    /// than looking for a parameter's presence.
    fn near_miss(secret: &str) -> String {
        let mut digits: Vec<char> = secret.chars().collect();
        let first = digits.first_mut().expect("a token is not empty");
        *first = if *first == '0' { '1' } else { '0' };
        digits.into_iter().collect()
    }

    /// A `HeaderMap` carrying the `Authorization` values a test names, in order.
    fn authorization(values: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for value in values {
            headers.append(
                header::AUTHORIZATION,
                HeaderValue::from_str(value).expect("a header value the test wrote"),
            );
        }
        headers
    }

    #[test]
    fn the_header_form_is_admitted_and_a_near_miss_is_not() {
        // The form a client that can set a header sends — `curl`, a script, this crate's own
        // suites, and whoever read [`REFUSAL`] — which is not the page (this module's header
        // says which form that uses and why the distinction was worth a finding). Both
        // directions, and the negative one is an equal-length near miss rather than "some other
        // string": a gate that checked only that the header was present would pass the first
        // assertion and fail this one.
        let token = minted();
        let secret = token.expose_secret().to_owned();

        assert!(admits(
            &authorization(&[&format!("Bearer {secret}")]),
            None,
            &token
        ));
        assert!(!admits(
            &authorization(&[&format!("Bearer {}", near_miss(&secret))]),
            None,
            &token
        ));
    }

    #[test]
    fn a_truncated_token_is_refused_and_so_is_one_with_something_added() {
        // The shapes a copy-paste failure takes — a URL wrapped by a mail client, a selection
        // that took one character too many — and the mutant that made this test necessary: a
        // gate whose comparison is `starts_with` rather than `Token::verify` **serves a
        // prefix**, and every other negative case in this module is equal-length, so nothing
        // else here would notice. `Token::verify`'s own suite covers the same two shapes one
        // layer down; what this pins is that the gate asks it rather than deciding for itself.
        let token = minted();
        let secret = token.expose_secret().to_owned();
        let truncated = &secret[..secret.len() - 8];
        let extended = format!("{secret}0");

        for candidate in [truncated, extended.as_str(), ""] {
            assert!(
                !admits(
                    &authorization(&[&format!("Bearer {candidate}")]),
                    None,
                    &token
                ),
                "{candidate:?} was admitted through the header"
            );
            assert!(
                !admits(
                    &HeaderMap::new(),
                    Some(&format!("{TOKEN_QUERY_PARAM}={candidate}")),
                    &token
                ),
                "{candidate:?} was admitted through the query"
            );
        }
    }

    #[test]
    fn the_query_form_is_admitted_and_a_near_miss_is_not() {
        // The form a *navigation* carries, which is the one an operator uses first. The
        // parameter's name comes from the constant, so a gate that read a different
        // parameter would fail here rather than in a browser.
        let token = minted();
        let secret = token.expose_secret().to_owned();

        assert!(admits(
            &HeaderMap::new(),
            Some(&format!("{TOKEN_QUERY_PARAM}={secret}")),
            &token
        ));
        assert!(!admits(
            &HeaderMap::new(),
            Some(&format!("{TOKEN_QUERY_PARAM}={}", near_miss(&secret))),
            &token
        ));
    }

    #[test]
    fn a_request_that_presents_nothing_is_refused() {
        // What an anonymous request looks like, in all the shapes "nothing" takes: no
        // headers and no query at all, an empty query string, and a query full of somebody
        // else's parameters.
        let token = minted();

        assert!(!admits(&HeaderMap::new(), None, &token));
        assert!(!admits(&HeaderMap::new(), Some(""), &token));
        assert!(!admits(&HeaderMap::new(), Some("camera=0&fps=30"), &token));
    }

    #[test]
    fn both_forms_at_once_are_admitted_only_when_they_agree() {
        // This module's rule, at the case it was written for. A header-wins gate and a
        // query-wins gate each pass one of the last two assertions and fail the other; this
        // gate has no precedence to get wrong.
        let token = minted();
        let secret = token.expose_secret().to_owned();
        let wrong = near_miss(&secret);

        assert!(
            admits(
                &authorization(&[&format!("Bearer {secret}")]),
                Some(&format!("{TOKEN_QUERY_PARAM}={secret}")),
                &token
            ),
            "a scripted request presenting a header, sent at a URL that still carries the token"
        );
        assert!(
            !admits(
                &authorization(&[&format!("Bearer {secret}")]),
                Some(&format!("{TOKEN_QUERY_PARAM}={wrong}")),
                &token
            ),
            "a header-wins gate serves this request"
        );
        assert!(
            !admits(
                &authorization(&[&format!("Bearer {wrong}")]),
                Some(&format!("{TOKEN_QUERY_PARAM}={secret}")),
                &token
            ),
            "a query-wins gate serves this request"
        );
    }

    #[test]
    fn a_duplicated_query_parameter_is_admitted_only_when_the_copies_agree() {
        // The HTTP-parameter-pollution case. A first-wins gate serves the second of these
        // and a last-wins gate serves the third — which is the whole of why this module
        // takes neither position.
        let token = minted();
        let secret = token.expose_secret().to_owned();
        let wrong = near_miss(&secret);

        assert!(
            admits(
                &HeaderMap::new(),
                Some(&format!(
                    "{TOKEN_QUERY_PARAM}={secret}&{TOKEN_QUERY_PARAM}={secret}"
                )),
                &token
            ),
            "two copies of the same token disagree about nothing"
        );
        assert!(
            !admits(
                &HeaderMap::new(),
                Some(&format!(
                    "{TOKEN_QUERY_PARAM}={secret}&{TOKEN_QUERY_PARAM}={wrong}"
                )),
                &token
            ),
            "a first-wins gate serves this request"
        );
        assert!(
            !admits(
                &HeaderMap::new(),
                Some(&format!(
                    "{TOKEN_QUERY_PARAM}={wrong}&{TOKEN_QUERY_PARAM}={secret}"
                )),
                &token
            ),
            "a last-wins gate serves this request"
        );
    }

    #[test]
    fn two_authorization_headers_are_the_same_rule() {
        // Two `Authorization` headers is well-formed HTTP, and `HeaderMap::get` would answer
        // only the first of them. Same three cases, same three answers.
        let token = minted();
        let secret = token.expose_secret().to_owned();
        let good = format!("Bearer {secret}");
        let bad = format!("Bearer {}", near_miss(&secret));

        assert!(admits(&authorization(&[&good, &good]), None, &token));
        assert!(
            !admits(&authorization(&[&good, &bad]), None, &token),
            "a gate reading only the first header serves this request"
        );
        assert!(
            !admits(&authorization(&[&bad, &good]), None, &token),
            "a gate reading only the last header serves this request"
        );
    }

    #[test]
    fn an_authorization_header_that_is_not_a_bearer_credential_is_a_failed_credential() {
        // Not "no credential": a request that says `Basic` has presented something, and
        // treating it as anonymous would mean a request with a *wrong* credential and a
        // right query parameter was admitted by a different path than the rule above.
        let token = minted();
        let secret = token.expose_secret().to_owned();

        for value in [
            format!("Basic {secret}"),
            secret.clone(),
            "Bearer".to_owned(),
            format!("Bearer{secret}"),
        ] {
            assert!(
                !admits(&authorization(&[&value]), None, &token),
                "{value:?} was admitted"
            );
            assert!(
                !admits(
                    &authorization(&[&value]),
                    Some(&format!("{TOKEN_QUERY_PARAM}={secret}")),
                    &token
                ),
                "{value:?} beside a good query parameter was admitted"
            );
        }
    }

    #[test]
    fn the_scheme_is_case_insensitive_and_the_token_is_not() {
        // RFC 9110 makes the scheme case-insensitive; the token is hex this daemon minted
        // lowercase, and `Token::verify` compares bytes. Both halves asserted, because a
        // gate that lowercased the whole header would accept a token nobody was given and
        // one that compared the scheme byte for byte would refuse a client that is right.
        let token = minted();
        let secret = token.expose_secret().to_owned();

        assert!(admits(
            &authorization(&[&format!("bearer {secret}")]),
            None,
            &token
        ));
        assert!(admits(
            &authorization(&[&format!("BEARER {secret}")]),
            None,
            &token
        ));
        assert!(!admits(
            &authorization(&[&format!("Bearer {}", secret.to_ascii_uppercase())]),
            None,
            &token
        ));
    }

    #[test]
    fn extra_spaces_after_the_scheme_are_the_grammars_and_a_trailing_one_is_not() {
        // RFC 9110's `credentials` allows more than one space between the scheme and the
        // credential. Nothing is trimmed from the end, so a value with a trailing space is a
        // candidate of the wrong length and is refused — err closed, which is the direction
        // D11 errs everywhere else.
        let token = minted();
        let secret = token.expose_secret().to_owned();

        assert!(admits(
            &authorization(&[&format!("Bearer   {secret}")]),
            None,
            &token
        ));
        assert!(!admits(
            &authorization(&[&format!("Bearer {secret} ")]),
            None,
            &token
        ));
    }

    #[test]
    fn a_parameter_that_merely_contains_the_name_is_not_a_credential() {
        // Whole-name comparison, in both directions: a request carrying somebody else's
        // `tokens=` is anonymous (refused because it presented nothing), and one carrying a
        // real `token=` beside it is admitted (so the parser is not simply refusing anything
        // with an ampersand in it).
        let token = minted();
        let secret = token.expose_secret().to_owned();

        assert!(!admits(
            &HeaderMap::new(),
            Some(&format!("tokens={secret}&xtoken={secret}")),
            &token
        ));
        assert!(admits(
            &HeaderMap::new(),
            Some(&format!("camera=0&{TOKEN_QUERY_PARAM}={secret}&fps=30")),
            &token
        ));
    }

    #[test]
    fn a_token_parameter_with_no_value_is_a_credential_that_fails() {
        // `?token` and `?token=` are the two shapes a truncated URL takes. Both are a
        // credential that did not verify rather than a request that presented nothing, which
        // is the difference between a 401 and — in the token-less cell, where no gate is
        // installed at all — a very different reading of the same URL.
        let token = minted();

        assert!(!admits(&HeaderMap::new(), Some(TOKEN_QUERY_PARAM), &token));
        assert!(!admits(
            &HeaderMap::new(),
            Some(&format!("{TOKEN_QUERY_PARAM}=")),
            &token
        ));
    }

    #[test]
    fn a_percent_encoded_token_is_refused_rather_than_decoded() {
        // The stated residual, asserted so it stays a decision. Hex is unreserved, so the
        // only way to meet this case is a client that encoded a character it did not have
        // to; admitting it would mean the secret has more than one spelling.
        let token = minted();
        let secret = token.expose_secret().to_owned();
        // The *first* digit, whatever it is, so this is one candidate and not a search for a
        // character the token may not contain — a test that skipped itself on some tokens
        // would be a test that passes without asserting.
        let first = *secret.as_bytes().first().expect("a token is not empty");
        let encoded = format!("%{first:02X}{}", &secret[1..]);
        assert_ne!(encoded, secret);

        assert!(!admits(
            &HeaderMap::new(),
            Some(&format!("{TOKEN_QUERY_PARAM}={encoded}")),
            &token
        ));
    }

    #[test]
    fn the_refusal_says_what_to_do_and_does_not_say_the_token() {
        // The body a 401 carries, asserted as a value rather than a shape. The second half is
        // the one that matters: `super::token`'s header is about a secret that must not be
        // printable by accident, and a refusal that echoed the expected token would be the
        // most complete leak this daemon could arrange.
        let token = minted();

        assert!(REFUSAL.contains("token"), "{REFUSAL}");
        assert!(!REFUSAL.contains(token.expose_secret()), "{REFUSAL}");
        assert!(
            !REFUSAL.contains("panicked") && !REFUSAL.contains("Token {"),
            "the refusal is a sentence, not a rendering of something: {REFUSAL}"
        );
    }
}
