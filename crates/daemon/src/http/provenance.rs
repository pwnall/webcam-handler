//! The cross-origin rule — **refuse a request a browser reports as coming from somewhere else**
//! (owner ruling, 2026-08-13; the measurement it rests on is note **N93**).
//!
//! > "I think the daemon should refuse all calls tagged with cross-origin headers. I see no good
//! > reason to accept those."
//!
//! [`super::gate`] is this module's sibling and its model. That one asks *did you present the
//! credential this run minted*; this one asks *did a browser just tell us you are not our own
//! page*. They are two admission rules and not two halves of one, which is why this is a module
//! beside it rather than three more lines inside [`super::gate::check`] — see "One home per law"
//! below, because the argument is the whole reason for the file.
//!
//! ## What a browser actually says, measured, and the row the rule is built on
//!
//! Note **N93** read the headers off every request the shipped client makes, against the pinned
//! Chromium and a live `webcam-handler-daemon`:
//!
//! | request | `Sec-Fetch-Dest` | `Sec-Fetch-Site` | `Origin` |
//! |---|---|---|---|
//! | `/` (the page) | `document` | `none` | **absent** |
//! | `/app.css` | `style` | `same-origin` | **absent** |
//! | the nine ES modules | `script` | `same-origin` | present |
//! | [`super::RPC_PATH`] (the WebSocket upgrade) | — | — | present |
//! | **[`super::PREVIEW_PATH`] (the `<img>`)** | `image` | `same-origin` | **absent** |
//!
//! **The last row is the whole design.** An `<img src>` performs a *no-CORS* load: the response
//! is opaque — renderable, unreadable from script, canvas-tainting — and the request carries **no
//! `Origin` at all**. So on the one route whose response body *is* a live camera, the page's own
//! request and a foreign page's `<img src>` are indistinguishable by `Origin`, because neither
//! has one. An `Origin`-only rule would defend [`super::rpc`] and do nothing whatever for
//! [`super::preview`] — and, worse than nothing, would leave the frames being *fetched and
//! displayed*, unreadable but visible, which is a privacy breach with no exfiltration attached
//! to it and therefore no error anywhere for anybody to notice.
//!
//! Hence: **Fetch Metadata is the primary signal and `Origin` corroborates**, and this module is
//! named for the question rather than for the weaker of the two headers that answer it. A module
//! called `origin` would be a name arguing for the reading N93 measured to be wrong.
//!
//! ## The rule, stated once
//!
//! - **`Sec-Fetch-Site`, when present, must be `same-origin` or `none`.** `cross-site` and
//!   `same-site` are refused, and so is any value this build does not recognise — see
//!   `site_is_ours` for why the unknown arm errs closed here rather than carrying its payload
//!   forward the way AGENTS rule 6 asks of *device* vocabulary.
//! - **`Origin`, when present, must be this request's own authority over `http`.** `null` — a
//!   sandboxed iframe, a `file://` document, some redirects — is refused rather than read as
//!   absent, which it very nearly looks like. `origin_is_ours` argues the comparison.
//! - **A request that presents neither is admitted.**
//!
//! ## Absence is admitted, and that is a deliberate weakening
//!
//! `webcam-handler-client`, `curl` and the AI agent harness AGENTS names as this project's
//! **primary** consumer send neither header. A rule spelled *"require a same-origin header"*
//! locks out every consumer that matters most — the one with no hands, driving this daemon
//! unattended — so the rule is necessarily "refuse when a browser tells us it is cross-site;
//! admit when nothing is claimed".
//!
//! N93 writes that down rather than leaving it to be discovered: **anything that can forge or
//! omit these headers is unaffected.** What it buys is exactly the thing the ruling asked for —
//! *a page in another origin, in the operator's own browser, cannot drive the camera* — and it
//! buys nothing against a local process, which is what D11's token is for.
//!
//! ## Why `Sec-Fetch-Site: none` is admitted, which is the value worth thinking twice about
//!
//! `none` means **no initiator**: a typed URL, a bookmark, a link opened from another
//! application. It is what the operator's first navigation to
//! [`super::token::Token::ready_to_open_url`] sends, measured in N93's first row, so admitting it
//! is a product requirement. It is also *safe*, and for a reason stronger than "we need it": a
//! document cannot cause a `none`. Every request a page initiates is classified against that
//! page — `same-origin`, `same-site` or `cross-site` — so `none` is precisely the class a
//! foreign page cannot produce. The attacker this rule is about is a page, and a page has an
//! initiator by definition.
//!
//! ## `Origin` is compared against the request's own authority, not against the bound address
//!
//! "The listener's own origin" is not a value this daemon has. It has a *bound address*, and in
//! two of D11's four cells that address is the wrong thing to compare against: `--http
//! 0.0.0.0:8080` binds `0.0.0.0:8080`, which is an address no client ever types, so a rule
//! anchored on it would refuse every request in the cell D11 is most careful about. And an
//! operator who types `http://localhost:<port>/` instead of copying the `127.0.0.1` URL the
//! daemon printed is reaching the same listener at a different authority, legitimately.
//!
//! So the comparison is `Origin` against the authority **this request was addressed to** — the
//! `Host` header, or the request target's authority when it carries one. A browser sets both
//! from the URL it was given and a page cannot forge either, which is the only property the
//! comparison needs. See `origin_is_ours` for the two mechanical details (the scheme, and
//! absolute-form request targets).
//!
//! ## Why `403` and not [`super::gate`]'s `401`
//!
//! The gate answers `401` with RFC 6750's `WWW-Authenticate: Bearer` challenge and a body that
//! tells the caller where to find the token, because there a credential is exactly what is
//! missing and exactly what would fix it. **Here a credential would not help and inviting one
//! would be a lie**: a foreign page presenting this run's token is still a foreign page, and this
//! layer sits *outside* the gate precisely so that it never gets as far as asking. `401` also
//! carries a challenge, and a challenge is an instruction to retry — which a browser will do,
//! with the same headers, forever.
//!
//! `403` is RFC 9110's answer for a request the server understood and refuses to authorize,
//! "regardless of any credentials the request may carry", which is this rule word for word.
//!
//! [`REFUSAL`] is one fixed sentence for **every** refused shape — cross-site, same-site, a
//! foreign `Origin`, `Origin: null`, an unrecognised token — and it echoes nothing from the
//! request: no path, no origin, no header name, no credential. A refusal that said *which* signal
//! tripped would be this daemon teaching a caller which header to drop.
//!
//! ## One home per law, and why it is not inside the gate
//!
//! Design §2.10, and three concrete reasons rather than a preference.
//!
//! **It covers more than the gate does.** [`super::gate`] is installed with
//! `Router::route_layer` over [`super::CAMERA_BEARING_PATHS`] and deliberately not over the
//! asset fallback (owner ruling 2026-08-12, note **N82**) — the client's own source code is
//! served to *anonymous* callers, which is not the same as serving it to *other origins*. A
//! foreign page has no business fetching this daemon's modules, its stylesheet or its 404
//! either. So this rule is installed with `Router::layer`, which maps over the path router, the
//! fallback and the catch-all: the exact tool note **N75** argued for and the gate then gave up,
//! wanted here in its original form for its original reason.
//!
//! **It applies in all four of D11's cells, and the gate applies in three.** The token-less
//! loopback cell — `--http-insecure-loopback` — installs no gate at all, and that is the cell
//! the ruling is about: P5's review lens 1 measured that a foreign page can drive the whole T5
//! surface there, because a WebSocket handshake is not subject to CORS and the token that would
//! otherwise stop it is absent by design. A rule living inside `gate::admits` would be absent
//! from the one cell that needs it most. The owner's second sentence bounds the ambition here
//! rather than the coverage: *"`--http-insecure-loopback` has `insecure` in it, so we'll fix all
//! the obvious security holes, and accept that we may miss some more subtle ones."*
//!
//! **They are different questions and share no vocabulary.** `super::gate::admits` takes a
//! token and counts credentials; this takes an authority and reads a browser's report. Folding
//! them would give one function two reasons to say no and one caller no way to tell which — and
//! `gate`'s own header spends a section on why its answer must not depend on precedence between
//! two things.
//!
//! ## What this does not close
//!
//! Stated here as well as in N93, because a reader of the code should not have to find the note
//! to learn what the code does not do.
//!
//! - **A local process is unaffected.** It sends no headers, so it is admitted — exactly as
//!   `webcam-handler-client` is, by design. In the token-less cell that remains open, and D11's
//!   token is what closes it.
//! - **A header-forging non-browser client is unaffected**, by construction.
//! - **DNS rebinding is not closed.** A page served from a name that resolves to this host sends
//!   a matching `Host` *and* `Origin` and is classified `same-origin` by the browser, so neither
//!   signal fires. Closing it needs an allowlist of authorities, which would refuse
//!   `http://localhost:<port>/` and every non-loopback cell's real name — a bigger decision than
//!   this ruling, and the owner's to take.
//! - **This does not stop an `<img>` from *rendering* an opaque response.** It stops the
//!   request. The two are different and the difference is the argument for reading
//!   `Sec-Fetch-Site` at all.
//!
//! ## What it does not claim
//!
//! Nothing about timing: unlike [`super::token::Token::verify`] there is no secret here, and
//! every comparison in this module is over values the caller sent and already knows.

use axum::extract::Request;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// The Fetch Metadata header this rule reads first.
///
/// **Private, and the suites spell it themselves** — which is the opposite of
/// [`super::TOKEN_QUERY_PARAM`]'s arrangement, for the reason that inverts it. The token's
/// parameter name is *this project's* invention, so a test that typed it could disagree with the
/// daemon and pass; this header's name is the **browser's**, fixed by the Fetch specification, so
/// a test that read it out of this constant would keep passing against a build that looked up the
/// wrong header. `crates/daemon/tests/http.rs` therefore sends `Sec-Fetch-Site` as a literal, and
/// that literal is what makes the constant checkable.
///
/// `Sec-Fetch-Dest` is deliberately **not** read. It says what the request is *for* — `image`,
/// `script`, `document` — which is a fact about the caller's intent and not about where the
/// caller came from; a rule that also demanded `Sec-Fetch-Dest: image` on the preview would be
/// this daemon deciding that `curl` may not fetch a frame, which is the opposite of what AGENTS
/// says its primary consumer is.
const SEC_FETCH_SITE: HeaderName = HeaderName::from_static("sec-fetch-site");

/// What a refused request is told, in full.
///
/// One sentence, fixed, and a `const` so a test can assert this daemon says **this** and the
/// same thing for every refused shape. Three things it deliberately is not: it is not a
/// challenge (see the module header on `403` against `401` — there is no credential that would
/// help), it does not name which of the two signals tripped, and it echoes nothing the caller
/// sent. `super::gate::REFUSAL`'s third clause applies here unchanged: a refusal is a sentence,
/// not a rendering of something.
pub const REFUSAL: &str = "this listener serves a live camera and refuses requests a browser \
                           reports as coming from another origin\n";

/// The middleware itself: admit, or refuse without running anything.
///
/// Installed with [`axum::middleware::from_fn`] and `Router::layer` in
/// `super::listener::router`, which is what puts it over the routes, the asset fallback
/// **and** the catch-all — see the module header for why that is the exact inverse of the
/// gate's `route_layer` and why the inversion is the point.
///
/// It is applied **outside** [`super::gate::check`], so a request a browser tagged as
/// cross-site is refused before any credential it carries is read. That ordering is the reason
/// the refusal can honestly say a credential would not have helped.
///
/// It takes no path and reads none, for [`super::gate::check`]'s reason: what this is installed
/// over is a fact about the composition, and a `match` on the path here would be the same policy
/// written in the one place where an inverted condition serves a camera to a stranger's page.
pub async fn check(request: Request, next: Next) -> Response {
    // Scoped so the borrow ends before the request is moved into the route.
    let admitted = {
        let authority = addressed_to(&request);
        admits(request.headers(), authority)
    };
    if admitted {
        next.run(request).await
    } else {
        refusal()
    }
}

/// `403`, with the fixed body and nothing else.
///
/// No `WWW-Authenticate`, deliberately: that header is an invitation to retry with a credential,
/// and this is the one refusal in this daemon that a credential cannot lift.
fn refusal() -> Response {
    (StatusCode::FORBIDDEN, REFUSAL).into_response()
}

/// The authority this request was addressed to, which is what an `Origin` is compared against.
///
/// Two shapes, and the order between them is RFC 9112 §3.2.2's rather than a preference: a
/// request target in **absolute form** (`GET http://host/path HTTP/1.1`, and every HTTP/2
/// request, whose `:authority` hyper puts here) carries the authority itself, and an origin
/// server "MUST ignore the received Host header field" when it does. Origin-form targets — every
/// request a browser sends on this transport — carry no authority, and the `Host` header is the
/// one the client used.
///
/// A caller that sends an absolute-form target and a matching `Origin` therefore matches itself.
/// That is not a hole this function opens: no browser sends absolute form, and a client that
/// writes its own request line could as easily send no `Origin` at all — which is admitted, by
/// design, and is the residual N93 records rather than one introduced here.
fn addressed_to(request: &Request) -> Option<&str> {
    request
        .uri()
        .authority()
        .map(axum::http::uri::Authority::as_str)
        .or_else(|| {
            request
                .headers()
                .get(header::HOST)
                .and_then(|value| value.to_str().ok())
        })
}

/// The whole policy, over values: does anything in this request say it came from somewhere else?
///
/// A function of a [`HeaderMap`] and an authority rather than of a `Request`, so N93's measured
/// table and every hostile shape beside it are asserted without a socket, a router or a runtime
/// (AGENTS, "pure cores take values"). [`check`] is the `Request`-shaped half and has nothing in
/// it this does not already cover.
///
/// **`get_all` and a non-short-circuiting fold**, both for `super::gate::admits`'s reasons. Two
/// `Origin` headers is a well-formed HTTP request and a browser never sends one; a rule that read
/// the first would be a first-wins rule, and which copy any layer of a stack picks is exactly the
/// disagreement `gate`'s header refuses to have. Every value present must be one this listener
/// owns, so there is no precedence to get wrong — and `&=` rather than [`Iterator::all`] so that
/// a later edit cannot turn "stops at the first failure" into "stops at the first success".
fn admits(headers: &HeaderMap, authority: Option<&str>) -> bool {
    let mut admitted = true;

    for value in headers.get_all(SEC_FETCH_SITE) {
        admitted &= site_is_ours(value);
    }
    for value in headers.get_all(header::ORIGIN) {
        admitted &= origin_is_ours(value, authority);
    }

    admitted
}

/// Is this `Sec-Fetch-Site` one of the two a request of our own carries?
///
/// `same-origin` is what the page's own subresources send — the stylesheet, the modules and the
/// preview `<img>`, all measured in N93 — and `none` is what a navigation with no initiator
/// sends, which is the operator opening the URL this daemon printed. The module header argues at
/// length why `none` cannot be produced by the page this rule exists to refuse.
///
/// **Everything else is refused, including a value this build has never heard of.** AGENTS rule 6
/// asks that unknown *device* vocabulary be carried as data rather than corrected, and that rule
/// is about a camera that knows more than we do; this is a security decision about a value a
/// caller chose, where the two ways to be wrong are not symmetric. A new Fetch Metadata value
/// admitted by default is a hole that arrives with somebody else's browser release; a new value
/// refused by default is a client that meets a `403` and a finding here. D11 errs closed and so
/// does this.
///
/// The comparison is byte-for-byte against the lowercase spellings, because these are structured
/// -header tokens: the specification defines them lowercase, every browser sends them lowercase,
/// and a case-folding comparison here would be one more spelling of a value that has exactly one.
fn site_is_ours(value: &HeaderValue) -> bool {
    matches!(value.as_bytes(), b"same-origin" | b"none")
}

/// Is this `Origin` the authority the request was addressed to, over `http`?
///
/// Serialized origins are `scheme "://" host [ ":" port ]` — RFC 6454 — with no path and no
/// trailing slash, and a browser includes the port exactly when the scheme's default is not in
/// use. The `Host` header of the same request is built from the same URL by the same browser, so
/// on a same-origin request the two authorities are byte-identical, and comparing them is one
/// comparison rather than a URL parser.
///
/// Four ways to fail, and each is a shape somebody actually sends:
///
/// - **`null`** — a sandboxed iframe, a `file://` document, a redirect that lost its origin.
///   It has no `://`, so it fails the scheme split, which is the answer this rule needs: `null`
///   is an origin that *is* present and *is* foreign, and reading it as absence would admit the
///   one attacker shape that announces itself.
/// - **another scheme** — `https://` at this authority is a different origin, and this listener
///   speaks no TLS at all (AGENTS: no TLS features anywhere in the dependency wall), so no page
///   of ours can be fulfilled over one.
/// - **another authority** — the whole point.
/// - **no authority to compare against** — a request with an `Origin` and no `Host` is a request
///   claiming a provenance this daemon cannot check, and an unanswerable question is refused
///   rather than assumed. hyper answers `400` to an HTTP/1.1 request with no `Host` before this
///   layer runs, so this arm is reachable only by a client the router already dislikes.
///
/// The host comparison is ASCII-case-insensitive because host names are; the scheme's is for the
/// same reason.
fn origin_is_ours(value: &HeaderValue, authority: Option<&str>) -> bool {
    let Ok(origin) = value.to_str() else {
        // `HeaderValue` may hold bytes that are not text at all. A credential-shaped comparison
        // would be wrong here: this is not "no origin", it is an origin nobody can read.
        return false;
    };
    let Some(authority) = authority else {
        return false;
    };
    let Some((scheme, host)) = origin.split_once("://") else {
        return false;
    };
    scheme.eq_ignore_ascii_case("http") && host.eq_ignore_ascii_case(authority)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The authority every request in this module's tests was addressed to.
    ///
    /// A loopback address with a port, because that is what D11's default cell binds and what
    /// every measured row in N93 was taken against.
    const HOST: &str = "127.0.0.1:41927";

    /// A header map carrying the fields a test names, in order, repeats kept.
    fn tagged(fields: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in fields {
            headers.append(
                HeaderName::from_bytes(name.as_bytes()).expect("a header name the test wrote"),
                HeaderValue::from_str(value).expect("a header value the test wrote"),
            );
        }
        headers
    }

    /// This listener's own origin, as a browser would serialize it.
    fn ours() -> String {
        format!("http://{HOST}")
    }

    #[test]
    fn every_request_the_shipped_client_makes_is_admitted() {
        // **Note N93's measured table, as the assertion that stops this rule being too strict.**
        // Each row is one request the pinned Chromium makes against a live daemon, with the
        // headers it was measured to send — including the two rows that carry no `Origin` at
        // all, which is the shape a rule built on `Origin` alone would have had no opinion
        // about. A build that demanded a same-origin header rather than refusing a cross-site
        // one fails on the first two rows; one that refused `none` fails on the navigation the
        // operator makes first.
        for (about, fields) in [
            (
                "the page, opened from the URL the daemon printed",
                vec![("sec-fetch-dest", "document"), ("sec-fetch-site", "none")],
            ),
            (
                "the stylesheet, which carries no Origin",
                vec![
                    ("sec-fetch-dest", "style"),
                    ("sec-fetch-site", "same-origin"),
                ],
            ),
            (
                "an ES module, which does",
                vec![
                    ("sec-fetch-dest", "script"),
                    ("sec-fetch-site", "same-origin"),
                    ("origin", &ours()),
                ],
            ),
            (
                "the WebSocket upgrade, which carries an Origin and no Fetch Metadata",
                vec![("origin", &ours())],
            ),
            (
                "the preview <img>, which carries neither an Origin nor a readable one",
                vec![
                    ("sec-fetch-dest", "image"),
                    ("sec-fetch-site", "same-origin"),
                ],
            ),
        ] {
            let fields: Vec<(&str, &str)> = fields.iter().map(|(n, v)| (*n, *v)).collect();
            assert!(admits(&tagged(&fields), Some(HOST)), "{about} was refused");
        }
    }

    #[test]
    fn a_request_claiming_nothing_is_admitted() {
        // The consumer AGENTS calls primary: an AI agent harness driving
        // `webcam-handler-client`, plus `curl` and this crate's own suites. None of them sends
        // either header, and a rule spelled "require a same-origin header" would lock out every
        // one of them — the weakening N93 records deliberately rather than leaving to be found.
        assert!(admits(&HeaderMap::new(), Some(HOST)));
        assert!(
            admits(&HeaderMap::new(), None),
            "a request with no Host either is still a request claiming nothing"
        );
    }

    #[test]
    fn a_browser_that_says_cross_site_or_same_site_is_refused() {
        // The ruling, at the two values it names. `same-site` is the one worth stating: a page
        // at another origin under the same registrable domain is still another origin, and a
        // rule that only refused `cross-site` would admit it.
        for site in ["cross-site", "same-site"] {
            assert!(
                !admits(&tagged(&[("sec-fetch-site", site)]), Some(HOST)),
                "{site} was admitted"
            );
        }
    }

    #[test]
    fn a_fetch_metadata_value_this_build_does_not_know_is_refused() {
        // The unknown arm, err-closed and asserted so it stays a decision (see `site_is_ours`).
        // The last two shapes are the ones a careless implementation admits: an empty value, and
        // a spelling that differs only in case from one this listener owns.
        for site in ["", "SAME-ORIGIN", "None", "same-origin, none", "cross_site"] {
            assert!(
                !admits(&tagged(&[("sec-fetch-site", site)]), Some(HOST)),
                "{site:?} was admitted"
            );
        }
    }

    #[test]
    fn a_foreign_origin_is_refused_and_this_listeners_own_is_not() {
        // Both directions, because a rule that refused every `Origin` would refuse the ES
        // modules and the WebSocket upgrade — half the client — and would pass every negative
        // assertion in this file.
        assert!(admits(&tagged(&[("origin", &ours())]), Some(HOST)));
        for origin in [
            "http://evil.example",
            "http://evil.example:41927",
            // The authority as a prefix and as a suffix of ours, which is what a comparison
            // written with `starts_with` or `ends_with` would admit.
            "http://127.0.0.1:41927.evil.example",
            "http://evil.example/http://127.0.0.1:41927",
        ] {
            assert!(
                !admits(&tagged(&[("origin", origin)]), Some(HOST)),
                "{origin} was admitted"
            );
        }
    }

    #[test]
    fn origin_null_is_refused_rather_than_read_as_absence() {
        // A sandboxed iframe, a `file://` document, and some redirect chains. It is the one
        // foreign origin that announces itself, and it is one careless `unwrap_or("")` away from
        // being read as the request that presented nothing — which is admitted.
        assert!(!admits(&tagged(&[("origin", "null")]), Some(HOST)));
        assert!(
            !admits(
                &tagged(&[("sec-fetch-site", "cross-site"), ("origin", "null")]),
                Some(HOST)
            ),
            "the shape a sandboxed iframe actually sends"
        );
    }

    #[test]
    fn an_origin_differing_only_in_scheme_or_port_is_refused() {
        // Two near misses, which are the shapes a comparison of *host* rather than of
        // *authority* admits. This listener speaks no TLS, so the `https` row is not a
        // theoretical origin — it is one no page of ours can ever be fulfilled at.
        for origin in [
            "https://127.0.0.1:41927",
            "http://127.0.0.1:41928",
            "http://127.0.0.1",
        ] {
            assert!(
                !admits(&tagged(&[("origin", origin)]), Some(HOST)),
                "{origin} was admitted"
            );
        }
    }

    #[test]
    fn an_origin_with_no_authority_to_compare_it_against_is_refused() {
        // A request that claims a provenance this daemon cannot check. Refused rather than
        // assumed, which is where D11 errs everywhere else.
        assert!(!admits(&tagged(&[("origin", &ours())]), None));
    }

    #[test]
    fn presenting_a_header_twice_is_the_same_rule_as_presenting_it_once() {
        // Well-formed HTTP that no browser sends, and the parameter-pollution question
        // `super::gate`'s header is about, one layer along: a first-wins rule serves the second
        // shape here and a last-wins rule serves the third. This one has no precedence to
        // disagree about — every value present must be one this listener owns.
        assert!(admits(
            &tagged(&[
                ("sec-fetch-site", "same-origin"),
                ("sec-fetch-site", "same-origin")
            ]),
            Some(HOST)
        ));
        assert!(!admits(
            &tagged(&[
                ("sec-fetch-site", "same-origin"),
                ("sec-fetch-site", "cross-site")
            ]),
            Some(HOST)
        ));
        assert!(!admits(
            &tagged(&[
                ("sec-fetch-site", "cross-site"),
                ("sec-fetch-site", "same-origin")
            ]),
            Some(HOST)
        ));
        assert!(!admits(
            &tagged(&[("origin", &ours()), ("origin", "http://evil.example")]),
            Some(HOST)
        ));
        assert!(!admits(
            &tagged(&[("origin", "http://evil.example"), ("origin", &ours())]),
            Some(HOST)
        ));
    }

    #[test]
    fn an_origin_that_is_not_text_is_refused() {
        // `HeaderValue` may hold arbitrary bytes. This is not "no origin": it is an origin
        // nobody can read, and the two must not have the same answer.
        let mut headers = HeaderMap::new();
        headers.append(
            header::ORIGIN,
            HeaderValue::from_bytes(&[0xff, 0xfe]).expect("bytes a header value may hold"),
        );
        assert!(!admits(&headers, Some(HOST)));
    }

    #[test]
    fn the_refusal_names_no_part_of_the_request() {
        // The body a `403` carries, asserted as a value. It must be the same sentence for every
        // refused shape, so it can mention neither of the two signals by name — a refusal that
        // said which header tripped would tell a caller which one to drop.
        assert!(
            !REFUSAL.contains("Origin") && !REFUSAL.contains("origin:"),
            "{REFUSAL}"
        );
        assert!(!REFUSAL.contains("Sec-Fetch"), "{REFUSAL}");
        assert!(!REFUSAL.contains("token"), "{REFUSAL}");
        assert!(
            !REFUSAL.contains("panicked") && !REFUSAL.contains('{'),
            "the refusal is a sentence, not a rendering of something: {REFUSAL}"
        );
    }
}
