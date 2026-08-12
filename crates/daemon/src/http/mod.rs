//! The opt-in TCP transport: its security posture, its token, and the server that reads both
//! (D11, docs/7 P5a).
//!
//! | Module | Home of |
//! |---|---|
//! | [`gate`] | the token gate's policy: which parts of a request may carry a credential, and what more than one of them means |
//! | [`listener`] | the axum server, the embedded client behind it, and the graceful stop |
//! | [`posture`] | D11's bind × token matrix, decided from values before anything is bound |
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
//! ## What is still not here
//!
//! **The WebSocket endpoint and the MJPEG preview**, which are P5b: the WS transport speaking
//! the same T5 surface, `multipart/x-mixed-replace` fed by the actor's latest-frame watch,
//! slow-consumer drop semantics, and the compression layer that has to be excluded from the
//! preview route. [`listener`]'s header states what the stop does *not* yet claim about an
//! in-flight response, which is the same boundary seen from the other side.

pub mod gate;
pub mod listener;
pub mod posture;
pub mod token;

pub use listener::{Serving, bind, open, serve};
pub use posture::{INSECURE_LOOPBACK_FLAG, Posture, Reach, TokenRule};
pub use token::{TOKEN_BYTES, TOKEN_QUERY_PARAM, Token};
