//! The webcam-handler engine.
//!
//! Pure cores (pairing planner, settle policy, sweep planner, session state machine)
//! wrapped in a thin imperative shell (camera actors, the session store, capture sinks).
//! The engine takes backends as `Box<dyn CameraBackend>` values and names none of them.
//!
//! Every module here is a fold over values: it reads no clock, opens no file, touches no
//! global. That is what makes the hard parts testable without a camera — the settle
//! policy's deadline is an argument, the pairing planner's device state is a slice of
//! [`schema::ControlDesc`], and the session state machine's "now" arrives as a
//! [`schema::Stamp`]. Where a seam is unavoidable it is a trait with a real
//! implementation and a scriptable double in the same module ([`paths::Env`],
//! [`settle::Clock`]).
//!
//! | Module | Home of |
//! |---|---|
//! | [`paths`] | the two XDG directories, ours because `directories` is banned (note N2) |
//! | [`pairing`] | the guarded-write planner and its inverse validator (D3) |
//! | [`settle`] | the settle policy state machine (D5) |
//! | [`sweep`] | the sweep planner (D8) |
//! | [`session`] | the calibration state machine (D8) |
#![forbid(unsafe_code)]

mod refusal;

pub mod pairing;
pub mod paths;
pub mod session;
pub mod settle;
pub mod sweep;
