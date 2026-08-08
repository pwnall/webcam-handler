//! The fake camera backend.
//!
//! Replays captured device profiles (T3), scripts the fault menu, and synthesizes frames
//! whose content responds to control values. A capability no real device exhibits is a bug
//! in this crate, not a feature of it (E5).
#![forbid(unsafe_code)]
