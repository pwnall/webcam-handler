//! Dev-only test kit: the backend conformance battery, the corpus loader, synthetic
//! fixtures, and the container oracles. Never a normal dependency edge
//! (`scripts/gates/testkit-is-dev-only.sh`).
//!
//! The modules, and what each one is the home of:
//!
//! | Module | Home of |
//! |---|---|
//! | [`battery`] | the T1/T2 conformance suite every backend runs (design §2.11 step 4) |
//! | [`corpus`] | loading the committed device profiles (§3.2) |
//! | [`fixtures`] | the hand-authored minimal-repro profile that keeps P0 hermetic |
//! | [`images`] | synthetic image fixtures — a re-export surface over the imaging crate's generators |
//!
//! Dev-only is a property, not a convention: this crate may grow tooling dependencies
//! that must never reach a shipped binary, so the gate asserts nothing depends on it
//! outside `[dev-dependencies]` — and asserts, in the other direction, that something
//! does.
#![forbid(unsafe_code)]

mod vocabulary;

pub mod battery;
pub mod corpus;
pub mod fixtures;
pub mod images;

pub use battery::{ArmOutcome, BatteryArm, BatteryReport};
