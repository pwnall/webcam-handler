//! Shared vocabulary for webcam-handler.
//!
//! Every type that crosses a boundary lives here, because the same types serve four
//! masters: the library API, the JSON-RPC wire, `--json` CLI output, and on-disk session
//! state. One definition means a rename is a compile error in all four at once rather
//! than a silent divergence in three.
//!
//! The modules, and the design decision each one is the home of:
//!
//! | Module | Home of |
//! |---|---|
//! | [`control`] | the control model — represent, don't reject (D2) |
//! | [`camera`] | camera identity, grouping, and the format tree (D1) |
//! | [`capture`] | stream requests, negotiated results, frames, settle policy (D5/D6) |
//! | [`video`] | the recording answer and D7's container-to-pixel-format pairing |
//! | [`pairing`] | the declared auto/manual table and its provenance (D3) |
//! | [`snapshot`] | snapshot/restore vocabulary and ordering (D4) |
//! | [`session`] | calibration session state (D8/D9) |
//! | [`progress`] | the live calibration progress stream the CLI renders and P4e transports (D8) |
//! | [`metrics`] | the metric names sessions persist (D8) |
//! | [`profile`] | the captured device profile (T3) |
//! | [`backend`] | the `CameraBackend`/`Camera` traits and `BackendKind` (T1/T2) |
//! | [`error`] | the closed error registry (D13) |
//! | [`limits`] | every cap and deadline, and D9's path layout (rubric A14) |
//! | [`paths`] | the runtime directory the daemon's socket lives in, and the [`paths::Env`] seam (D11/T6) |
//! | [`slug`] | the pinned slug transform (D2) |
//! | [`vocabulary`] | the generator every closed vocabulary and fault menu is built from |
//!
//! ## The doctrine these types encode
//!
//! - **The device is the only authority on itself** (E2). Nothing here validates a
//!   device's claims against a specification. A default outside its own range and a
//!   current value outside it are both representable, because both are real \[PF:4, PF:5\].
//! - **Requested is not applied** (E4). Every write returns [`control::Applied`], and no
//!   type in this crate collapses the pair.
//! - **Represent the unknown** (D2). [`control::ControlType::Unknown`] and
//!   [`control::ControlFlags::unknown_bits`] carry what this version cannot name.
//! - **Availability is not capability** (E3). [`error::Error`] keeps `Busy`,
//!   `PermissionDenied`, `DeviceGone` and `SettleTimeout` apart.
#![forbid(unsafe_code)]

pub mod vocabulary;

pub mod backend;
pub mod camera;
pub mod capture;
pub mod control;
pub mod error;
pub mod limits;
pub mod metrics;
pub mod pairing;
pub mod paths;
pub mod profile;
pub mod progress;
pub mod report;
pub mod session;
pub mod slug;
pub mod snapshot;
pub mod time;
pub mod video;

pub use backend::{BackendKind, Camera, CameraBackend, HotplugEvent, HotplugWatch};
pub use camera::{CameraFingerprint, CameraId, CameraInfo, PixelFormat};
pub use capture::{
    Frame, NegotiatedStream, PhotoFormat, PhotoReport, PhotoRequest, SettlePolicy, Sink,
    StreamRequest, Transform,
};
pub use control::{
    Applied, ControlDesc, ControlId, ControlSlug, ControlValue, ControlWrite, WriteWarning,
};
pub use error::{BackendError, Error, ErrorKind, Result};
pub use profile::DeviceProfile;
pub use report::{
    CameraDetail, CameraList, ControlReport, DiscoveryReport, ListHint, TerminationReport,
    WriteReport,
};
pub use session::{Selection, Session, SessionList, SessionRef, SessionStatus, SweepRequest};
pub use snapshot::{RestoreReport, Snapshot};
pub use time::Stamp;
pub use video::{CapReached, IntervalSource, RecordingSummary, VideoFormat};

/// The version of this crate, for provenance blocks and `--json` output.
pub const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    /// The schema crate is `#![forbid(unsafe_code)]`, and every other crate except
    /// `webcam-handler-v4l2` is too. That claim is gate-enforced rather than
    /// review-held — see `scripts/gates/unsafe-scope.sh`.
    #[test]
    fn the_tool_version_is_populated() {
        assert!(!super::TOOL_VERSION.is_empty());
    }
}
