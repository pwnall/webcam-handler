//! The webcam-handler engine.
//!
//! Pure cores (pairing planner, settle policy, sweep planner, session state machine)
//! wrapped in a thin imperative shell (camera actors, the session store, capture sinks).
//! The engine takes backends as `Box<dyn CameraBackend>` values and names none of them.
#![forbid(unsafe_code)]
