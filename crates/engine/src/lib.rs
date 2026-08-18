//! The webcam-handler engine.
//!
//! Pure cores (pairing planner, settle policy, sweep planner, session state machine)
//! wrapped in a thin imperative shell (camera actors, the session store, capture sinks).
//! The engine takes backends as `Box<dyn CameraBackend>` values and names none of them.
//!
//! Every pure core here is a fold over values: it reads no clock, opens no file, touches
//! no global. That is what makes the hard parts testable without a camera — the settle
//! policy's deadline is an argument, the pairing planner's device state is a slice of
//! [`schema::ControlDesc`], and the session state machine's "now" arrives as a
//! [`schema::Stamp`]. Where a seam is unavoidable it is a trait with a real
//! implementation and a scriptable double in the same module ([`schema::paths::Env`],
//! [`settle::Clock`]).
//!
//! | Module | Home of |
//! |---|---|
//! | [`actor`] | one blocking thread per open camera, and the registry that keeps it one (D12) |
//! | [`paths`] | D9's state directory, ours because `directories` is banned (note N2); the runtime half is [`schema::paths`] |
//! | [`pairing`] | the guarded-write planner and its inverse validator (D3) |
//! | [`settle`] | the settle policy state machine (D5) |
//! | [`sweep`] | the sweep planner (D8) |
//! | [`session`] | the calibration state machine (D8) |
//! | [`lifecycle`] | wiring that machine to the store: create, resume, pair discovery, the persisted pre-sweep snapshot, recovery (D8, D9, §6) |
//! | [`calibrate`] | executing a sweep: guarded set, settle, capture, score, record (D8) |
//! | [`progress`] | the seam a running sweep reports through, real and doubled (§2.9) |
//! | [`profile`] | assembling a T3 device profile, and the invariant/state split |
//! | [`resolve`] | turning a typed id or prefix into the camera it names (D1) |
//! | [`mod@write`] | executing a write plan, and reporting one that stopped part way (D3) |
//! | [`snapshot`] | taking a snapshot and putting it back, in D4's order |
//! | [`capture`] | start, settle, one frame, stop — with the stop on every exit (D5) |
//! | [`preview`] | the other capture shape: one frame per command off a stream that stays running (D12, §2.6) |
//! | [`photo`] | the photo assembly and the file sinks (D6, D10) |
//! | [`record`] | the third capture shape: a stream that runs for a duration into a container, in turns the actor can interleave (D7, D10) |
//! | [`discover`] | empirical pair discovery, by toggling and diffing INACTIVE (D3, PF:3) |
//! | [`store`] | the session directory, atomic state writes, and the one fd-lock (D9) |
#![forbid(unsafe_code)]
// docs/9's "device/request-driven paths" lint set. Every value that reaches this crate came
// from a device or from a caller's request, and every path here acts on one, so the whole crate
// is inside it. `not(test)` because a test asserting an invariant with `.expect("literal
// fixture")` is stating a precondition, not risking a device; docs/9 writes the same carve-out.
//
// `as_conversions` is deliberately not in this set — see `lint-posture.sh`, which walks for the
// four and records which crates add the fifth and why.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

#[cfg(test)]
mod double;
mod refusal;

pub mod actor;
pub mod calibrate;
pub mod capture;
pub mod discover;
pub mod facade;
pub mod lifecycle;
pub mod pairing;
pub mod paths;
pub mod photo;
pub mod preview;
pub mod profile;
pub mod progress;
pub mod record;
pub mod resolve;
pub mod session;
pub mod settle;
pub mod snapshot;
pub mod store;
pub mod sweep;
pub mod write;
