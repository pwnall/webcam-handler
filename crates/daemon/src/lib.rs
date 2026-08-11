//! `wchd` internals, as a library so integration tests can drive a real server.
//!
//! | Module | Home of |
//! |---|---|
//! | `events` | the two subscriptions' event sources, the fan-out, and its bounds (D10, P4e-i) |
//! | [`logging`] | design §2.6's `tracing` fmt layer, and the rule that no frame reaches it |
//! | [`server`] | the server half of `webcam-handler-api`'s T5 surface (D10) |
//! | [`shutdown`] | the order the daemon stops in, and the signal and supervisor seams (P4e-ii) |
//! | [`state`] | D9's state-directory lock, held for the process's lifetime |
//! | [`uds`] | the Unix-socket transport — the one piece of transport code we own (D11) |
//!
//! The binary beside this library is the daemon's **composition root** (design §2.11):
//! the process's edges — argument handling, the state-directory lock, the subscriber, the
//! backend factory match — live there, and everything here takes what it needs as a
//! parameter. That is what makes an integration test able to run a real `wchd` in its own
//! process without a socket path, a state directory or an environment variable of the
//! machine's leaking into it.
#![forbid(unsafe_code)]
// The daemon is request-driven end to end: every value that reaches it was written by
// somebody else and arrived over a socket, and a panic on this path takes the camera's
// owner down with it.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod events;
pub mod logging;
pub mod server;
pub mod shutdown;
pub mod state;
pub mod uds;
