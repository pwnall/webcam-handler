//! The opt-in TCP transport: its security posture, its token, and the server that reads both
//! (D11, docs/7 P5a).
//!
//! | Module | Home of |
//! |---|---|
//! | [`gate`] | the token gate's policy: which parts of a request may carry a credential, and what more than one of them means |
//! | [`listener`] | the axum server, the embedded client behind it, and the graceful stop |
//! | [`posture`] | D11's bind × token matrix, decided from values before anything is bound |
//! | [`preview`] | the MJPEG preview: `multipart/x-mixed-replace` over the actor's latest-frame watch |
//! | [`provenance`] | the cross-origin rule: what a browser says about where a request came from, and which answers this listener admits |
//! | [`rpc`] | the WebSocket JSON-RPC endpoint: the T5 surface `crate::server::mount` produced, reached over TCP |
//! | [`token`] | the per-run bearer token: minted from the kernel, compared in constant time, printed as a URL |
//!
//! ## D11's paragraph, and what this crate does with it
//!
//! The Unix socket is always served and its auth model is the filesystem ([`crate::uds`]).
//! TCP is the other half of D11, and it is a different kind of thing:
//!
//! > TCP is **opt-in** (`--http [addr]`, default `127.0.0.1:0` → report the bound port),
//! > serves the web client (static assets + WS JSON-RPC + MJPEG preview `<img>` endpoint),
//! > and requires a bearer token: generated per run and printed as a ready-to-open URL
//! > unless configured. The full bind × token matrix, stated once …: loopback + token is the
//! > default; token-less loopback exists only behind one named explicit flag
//! > (`--http-insecure-loopback`); non-loopback **always** requires the token — there is no
//! > flag that removes it — and additionally prints a warning naming what it exposes (a live
//! > camera). Loopback + token because loopback alone is not an auth boundary on a
//! > multi-user machine. A camera is a privacy-sensitive device; the daemon's exposure
//! > posture errs closed.
//!
//! Quoted rather than restated, because docs/7 P5a asks for the matrix "enforced *as written
//! in D11* (the gate cites the paragraph, not a paraphrase)" — and because a paraphrase of a
//! security rule is a second copy of it that is one edit away from meaning something else.
//!
//! **D11 carries an amendment of 2026-08-12, and the sentence it amends is the one about the
//! three things this transport serves.** "Requires a bearer token" is now about the two of them
//! that are the camera — the WS JSON-RPC endpoint and the MJPEG preview — and not about the
//! static assets, which are this client's own source code. The matrix is untouched, its reason
//! is untouched ("a camera is a privacy-sensitive device; the daemon's exposure posture errs
//! closed"), and what changed is which resources that reason is about. See D11's amended
//! paragraph and note **N82**.
//!
//! Two facts about this daemon follow from that paragraph, and they are what the first two
//! modules in the table are. **The posture is decided from values, before anything is bound** — so all
//! four cells are asserted on a machine with one interface, and the decision is a value a
//! reviewer can read rather than a branch inside a `bind` call ([`posture`]). And **the token
//! is a secret this process mints, which makes it a secret this process can leak** — so it
//! has no `Display`, a `Debug` that redacts, one named accessor, and a comparison that does
//! not leak its own answer through a stopwatch ([`token`]).
//!
//! ## The order these landed in, which is the argument for the split
//!
//! [`posture`] and [`token`] landed **first**, with no transport at all beside them, and the
//! two modules that arrived next take their answers as values. A listener is the place where
//! getting the posture wrong stops being visible, because by then the failure is a socket
//! that is open rather than an expression that is wrong; deciding the matrix in a function
//! over a [`std::net::SocketAddr`] is what lets all four cells be asserted on a machine with
//! one interface.
//!
//! So the two halves of this module read in that direction and never the other. Nothing in
//! [`posture`] or [`token`] opens, binds, reads the environment or reads a clock —
//! [`token::Token::mint`] asks the kernel for randomness and is the one seam among them — and
//! [`listener`] decides nothing: it is handed a decided [`Posture`] and a minted [`Token`],
//! and installs [`gate`] or does not, according to what the first of those says.
//!
//! ## The third module, and what it deliberately is not
//!
//! [`rpc`] is P5b's first half: D11's "WS JSON-RPC", as **one route that calls the `Methods`
//! value [`crate::server::mount`] produced** rather than a second registration of anything.
//! It fits the arrangement above without disturbing it — it decides nothing, [`listener`]
//! merges its route beside the asset fallback, and [`gate`] covers it because it is one of the
//! two routes that carry or drive a camera. Its header carries the two things that are
//! genuinely new: why jsonrpsee's own upgrade path is used instead of axum's `ws` feature, and
//! what the `?token=` credential costs on a WebSocket rather than on a navigation (a
//! `new WebSocket(url)` can set no headers, so the URL form is not a preference).
//!
//! ## The fourth module, and the one route that is a camera
//!
//! [`preview`] is P5b's second half: design §2.6's "MJPEG preview route
//! (`multipart/x-mixed-replace`, fed from the actor's latest-frame watch channel so slow
//! clients drop frames)". It sits beside [`rpc`] in the arrangement above — it decides nothing
//! about postures, [`listener`] merges its route, and [`gate`] covers it — and it is the one
//! response this listener writes that does not end on its own, which is why [`listener`]'s
//! "what is not claimed" paragraph is now a paragraph about something that exists. The fan-out
//! itself is not here: `crate::preview` owns the watch channel, the feed registry and the
//! driver, because those are questions about cameras and this module is about a socket.
//!
//! ## The two routes that carry a camera, and everything else is a file
//!
//! [`CAMERA_BEARING_PATHS`] is the list, and it exists because of an owner ruling
//! (2026-08-12): **static assets are served without authentication** — they are open-source
//! code, not a secret — and **only the resources that carry or drive the camera are gated**.
//! Note **N82** carries the ruling, what it cost and what replaced the cost; it retires note
//! N76, whose whole subject was a client that could not fetch its own modules.
//!
//! The list is now the *policy* rather than a subset claim: `listener::router` gates the
//! routes and not the fallback, so "which paths need the token" and "which paths are routes"
//! are the same question, and this list is the answer written down. Two things hold it —
//! `crates/daemon/tests/preview.rs` drives every path in it anonymously over a real socket and
//! requires a `401`, and `scripts/gates/web-routes-are-gated.sh` requires every `.route(` in
//! this crate to name a path that is on it — because a test can only drive the paths somebody
//! named, and the route nobody named is exactly the defect the narrowing created.
//!
//! ## The sixth module, and the one rule that is not about a credential
//!
//! [`provenance`] is the owner's ruling of 2026-08-13 — *"the daemon should refuse all calls
//! tagged with cross-origin headers"* — and it is a **second admission rule beside [`gate`]**,
//! not a second copy of it. The gate asks whether a request presented this run's token;
//! provenance asks whether a browser has just reported that the request came from a page that
//! is not ours. Note **N93** is the measurement it rests on, and its load-bearing row is that
//! the preview `<img>` carries **no `Origin` at all** — so `Sec-Fetch-Site` is the primary
//! signal and `Origin` corroborates, rather than the other way round.
//!
//! It is deliberately installed differently from the gate, in both of the two ways that
//! matter, and that difference is why it is a module rather than a branch inside one:
//!
//! - **over more paths.** [`gate`] is a `Router::route_layer` over [`CAMERA_BEARING_PATHS`] and
//!   not over the asset fallback, because N82 opened the client's own source code to
//!   *anonymous* callers — which is not the same as opening it to *other origins*. Provenance
//!   is a `Router::layer`, so it covers the routes, the fallback and the catch-all `404`.
//! - **in more cells.** [`gate`] is installed in D11's three token-gated cells; provenance is
//!   installed in all four, because the token-less loopback cell is the one the ruling is
//!   about — there, nothing else stands between a foreign page and the T5 surface.
//!
//! `listener::router` composes both, in that order: provenance outside, so a cross-site request
//! is refused before any credential it carries is read.

pub mod gate;
pub mod listener;
pub mod posture;
pub mod preview;
pub mod provenance;
pub mod rpc;
pub mod token;

pub use listener::{Serving, bind, open, serve};
pub use posture::{INSECURE_LOOPBACK_FLAG, Posture, Reach, TokenRule};
pub use preview::{CAMERA_QUERY_PARAM, PREVIEW_PATH};
pub use rpc::RPC_PATH;
pub use token::{TOKEN_BYTES, TOKEN_QUERY_PARAM, Token};

/// The routes that carry or drive the camera, and therefore stay behind D11's token.
///
/// One list, `pub` because it has four readers that must agree: `crates/daemon/tests/http.rs`
/// and `crates/daemon/tests/preview.rs`, which drive every path in it anonymously and require
/// the refusal in full; [`rpc`]'s and [`preview`]'s own tests, each asserting that its route is
/// on it; and `scripts/gates/web-routes-are-gated.sh`, which reads the entries out of this
/// declaration and requires every route registered in this crate to be one of them.
///
/// The two entries are the WebSocket endpoint, which *drives* a camera (every T5 method that
/// opens one is reachable over it), and the MJPEG preview, which *carries* one — its response
/// body is a live picture of whatever the camera is pointed at, which is the whole reason the
/// ruling kept a gate at all (AGENTS: "a frame may contain a person").
///
/// **What "carries or drives the camera" means as something checkable.** Nothing can read a
/// handler and know whether a camera is behind it, so the predicate this project encodes is
/// the one that is decidable and errs closed: **a route is gated; the only thing served
/// without the token is a lookup in the embedded asset table** (`listener::router`,
/// `webcam-handler-web`). Today those are the same set — the only reason this listener has a
/// route at all is the camera — and the day one of them is not, this list is where the
/// argument has to happen rather than where it can be skipped.
pub const CAMERA_BEARING_PATHS: [&str; 2] = [rpc::RPC_PATH, preview::PREVIEW_PATH];
