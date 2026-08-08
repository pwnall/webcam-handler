//! The one webcam-handler wire surface (T5).
//!
//! One jsonrpsee `#[rpc(server, client)]` trait over schema DTOs, plus the single
//! exhaustive match mapping the D13 error registry onto JSON-RPC codes.
#![forbid(unsafe_code)]
