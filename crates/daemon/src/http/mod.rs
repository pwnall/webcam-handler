//! The opt-in TCP transport's security posture and its token (D11, docs/7 P5a).
//!
//! | Module | Home of |
//! |---|---|
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
//! Two facts about this daemon follow from that paragraph, and they are what these two
//! modules are. **The posture is decided from values, before anything is bound** — so all
//! four cells are asserted on a machine with one interface, and the decision is a value a
//! reviewer can read rather than a branch inside a `bind` call ([`posture`]). And **the token
//! is a secret this process mints, which makes it a secret this process can leak** — so it
//! has no `Display`, a `Debug` that redacts, one named accessor, and a comparison that does
//! not leak its own answer through a stopwatch ([`token`]).
//!
//! ## What is deliberately not here
//!
//! **No transport code at all.** The axum listener, the routes, the token middleware, the
//! `rust-embed`'d assets, the WebSocket endpoint and the MJPEG preview are the rest of P5a
//! and the whole of P5b. This module is the two decisions all of that rests on, landed and
//! tested first: a listener is the place where getting the posture wrong stops being visible,
//! because by then the failure is a socket that is open rather than an expression that is
//! wrong.
//!
//! In particular, **nothing here opens, binds, reads the environment or reads a clock**.
//! [`token::Token::mint`] asks the kernel for randomness and is the one seam in the module;
//! everything else takes values and answers values (AGENTS, "pure cores take values").

pub mod posture;
pub mod token;

pub use posture::{INSECURE_LOOPBACK_FLAG, Posture, Reach, TokenRule};
pub use token::{TOKEN_BYTES, TOKEN_QUERY_PARAM, Token};
