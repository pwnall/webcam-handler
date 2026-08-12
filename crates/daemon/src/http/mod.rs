//! The opt-in TCP transport: its security posture, its token, and the server that reads both
//! (D11, docs/7 P5a).
//!
//! | Module | Home of |
//! |---|---|
//! | [`gate`] | the token gate's policy: which parts of a request may carry a credential, and what more than one of them means |
//! | [`listener`] | the axum server, the embedded client behind it, and the graceful stop |
//! | [`posture`] | D11's bind × token matrix, decided from values before anything is bound |
//! | [`preview`] | the MJPEG preview: `multipart/x-mixed-replace` over the actor's latest-frame watch |
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
//! merges its route beside the asset fallback, and [`gate`] covers it because it covers
//! everything. Its header carries the two things that are genuinely new: why jsonrpsee's own
//! upgrade path is used instead of axum's `ws` feature, and what the `?token=` credential
//! costs on a WebSocket rather than on a navigation (note **N76** is the constraint it works
//! under; a `new WebSocket(url)` can set no headers, so the URL form is not a preference).
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
//! ## The two routes that carry a camera, named
//!
//! [`CAMERA_BEARING_PATHS`] is the list, and it exists because of an owner ruling
//! (2026-08-12): **static assets are served without authentication** — they are open-source
//! code, not a secret — and **only the resources that carry or drive the camera are gated**.
//! The piece that moves the gate from `Router::layer` over everything to those two routes is a
//! separate one and is deliberately not landed here; what is landed here is the list it will
//! move the gate over, so that "which routes stay gated" is a value in this crate rather than
//! a memory in a commit message. Today the gate still covers every route, so the list is a
//! *subset* claim rather than the whole policy — and the suite drives every path in it
//! anonymously, which is a claim that survives the split.

pub mod gate;
pub mod listener;
pub mod posture;
pub mod preview;
pub mod rpc;
pub mod token;

pub use listener::{Serving, bind, open, serve};
pub use posture::{INSECURE_LOOPBACK_FLAG, Posture, Reach, TokenRule};
pub use preview::{CAMERA_QUERY_PARAM, PREVIEW_PATH};
pub use rpc::RPC_PATH;
pub use token::{TOKEN_BYTES, TOKEN_QUERY_PARAM, Token};

/// The routes that carry or drive the camera, and therefore stay behind D11's token.
///
/// One list, `pub` because it has three readers that must agree: this crate's own suite, which
/// drives every path in it anonymously and requires a `401`; [`preview`]'s own tests, which
/// assert that the preview is on it; and the piece that moves the gate onto exactly these
/// routes when the owner's 2026-08-12 ruling is implemented.
///
/// **Being on this list is not what makes a route gated today** — `listener::router` wraps
/// the whole router, so the gate covers these two and everything else besides — and the
/// distinction is worth keeping honest rather than blurring. What the list is for is that
/// after the split, "the preview is gated" must not depend on where the route happens to sit
/// in a router; it depends on the route being named here, and on a test that reads this list
/// rather than a copy of it.
///
/// The two entries are the WebSocket endpoint, which *drives* a camera (every T5 method that
/// opens one is reachable over it), and the MJPEG preview, which *carries* one — its response
/// body is a live picture of whatever the camera is pointed at, which is the whole reason the
/// ruling kept a gate at all (AGENTS: "a frame may contain a person").
pub const CAMERA_BEARING_PATHS: [&str; 2] = [rpc::RPC_PATH, preview::PREVIEW_PATH];
