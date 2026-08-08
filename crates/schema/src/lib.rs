//! Shared vocabulary for webcam-handler.
//!
//! Every type that crosses a boundary lives here: the control model (design D2), camera
//! identity (D1), the error registry (D13), session state (D8), the limits table, and the
//! backend traits (T1/T2) that let the engine take a camera backend as a value.
#![forbid(unsafe_code)]
