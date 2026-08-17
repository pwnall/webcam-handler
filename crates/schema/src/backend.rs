//! The backend contract (design T1–T2) and the backend vocabulary.
//!
//! These traits are object-safe vocabulary over schema values, which is why they live
//! here rather than in the engine: it puts them *below* every backend and below the
//! engine in the dependency graph. Backends depend on schema; the engine consumes
//! `Box<dyn CameraBackend>` values the composition roots hand it and names no backend at
//! all (`scripts/gates/dependency-walls.sh` holds that claim).
//!
//! The split is: **the engine owns policy, the backend owns mechanism.** Everything above
//! [`Camera`] — guarded sets, snapshots, settle policies, sweeps, sessions, sinks — is
//! engine code shared by every backend. A backend never renders, never persists, and
//! never decides policy.

use std::fmt;
use std::time::Instant;

use camino::Utf8PathBuf;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::camera::{CameraId, CameraInfo, FormatInfo};
use crate::capture::{Frame, NegotiatedStream, StreamRequest};
use crate::control::{Applied, ControlDesc, ControlId, ControlValue};
use crate::error::{BackendError, Result};
use crate::vocabulary::closed_vocabulary;

closed_vocabulary! {
    /// Which backend a camera came from.
    ///
    /// Closed on purpose: each composition root holds one exhaustive `match` over this
    /// vocabulary constructing the backend it links, so adding a variant stops both
    /// builds until the new backend is wired (design §2.11 step 3).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum BackendKind {
        /// Real hardware through V4L2.
        V4l2,
        /// Captured device profiles, replayed. A test instrument, not a product backend.
        Fake,
    }
}

impl BackendKind {
    /// The name used on the command line (`--backend v4l2`) and in `--json` output.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            BackendKind::V4l2 => "v4l2",
            BackendKind::Fake => "fake",
        }
    }

    /// Parse a `--backend` argument.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|k| k.as_str() == s)
    }
}

impl fmt::Display for BackendKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A camera appearing or disappearing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HotplugEvent {
    /// A device node appeared. Re-enumeration decides what camera it belongs to —
    /// the event names a node, never a camera, because grouping is not a node property.
    Added {
        /// The node that appeared.
        #[schemars(with = "String")]
        path: Utf8PathBuf,
    },
    /// A device node disappeared.
    Removed {
        /// The node that vanished.
        #[schemars(with = "String")]
        path: Utf8PathBuf,
    },
}

/// A source of hotplug events.
pub trait HotplugWatch: fmt::Debug + Send {
    /// Wait for the next event, or until `deadline`.
    ///
    /// `Ok(None)` means the deadline arrived first — a normal outcome, not an error, so
    /// a caller polling on a cadence never has to interpret a timeout as a failure (E3).
    fn next_event(&mut self, deadline: Instant) -> Result<Option<HotplugEvent>>;
}

/// **T1** — the pluggability seam.
///
/// Enumeration is live, every time: capabilities are read from the device, never cached
/// across sessions as truth (E2). A committed profile is corpus and a UX hint, and is
/// always labeled as such.
pub trait CameraBackend: fmt::Debug + Send + Sync {
    /// Which backend this is.
    fn kind(&self) -> BackendKind;

    /// The backend's name. Derived from [`CameraBackend::kind`] so the two cannot
    /// disagree.
    fn name(&self) -> &'static str {
        self.kind().as_str()
    }

    /// Every camera this backend can see, right now.
    fn enumerate(&self) -> Result<Vec<CameraInfo>>;

    /// Open one camera for use.
    fn open(&self, id: &CameraId) -> Result<Box<dyn Camera>>;

    /// Subscribe to add/remove events.
    fn watch(&self) -> Result<Box<dyn HotplugWatch>>;

    /// Why [`CameraBackend::enumerate`] might not have said what the caller expected.
    ///
    /// D1: "an empty enumeration is diagnosed, not shrugged at". The diagnosis is
    /// backend-specific — only the V4L2 backend knows what a USB video-class interface
    /// with no driver looks like — so it belongs on the seam rather than in a client that
    /// would have to ask which backend it is holding (design §2.10: a second `match` on
    /// `BackendKind` is a second home). Defaulted to empty, because "I have nothing to
    /// add" is the honest answer for a backend replaying a document. See note N7.
    ///
    /// **Call it after [`CameraBackend::enumerate`], on the same thread**, and read it as
    /// *why that listing said what it did*. A hint explains a listing, so an implementation
    /// is entitled to answer from the reading its own `enumerate` just took rather than by
    /// reading the machine a second time — the V4L2 backend does exactly that, because two
    /// readings a moment apart can disagree about which cameras are there and a hint that
    /// describes the second one is attached to the first (note **N193**). Called with no
    /// listing behind it, or from a different thread, an implementation answers about now,
    /// which is a weaker answer and never a wrong one.
    ///
    /// `engine::resolve::list` is the one caller in this workspace and it makes the paired
    /// call; a caller that wants a diagnosis on its own gets the unpaired answer, which is
    /// what it asked for.
    fn diagnose(&self) -> Vec<crate::report::ListHint> {
        Vec::new()
    }
}

/// **T2** — one open camera. Blocking, object-safe, minimal.
///
/// Blocking because V4L2 ioctls and `DQBUF` block; the engine gives each camera its own
/// thread rather than pretending otherwise (design D12).
pub trait Camera: fmt::Debug + Send {
    /// What this camera is, as enumerated when it was opened.
    fn info(&self) -> &CameraInfo;

    /// Pixel formats, with sizes and intervals nested under each \[PF:9\].
    fn formats(&self) -> Result<Vec<FormatInfo>>;

    /// The full control set, including controls of types we cannot interpret \[PF:1\].
    ///
    /// Never panics on device vocabulary. That is not a style preference: the most
    /// popular V4L2 crate panics on a control type this kernel emits, and a library that
    /// can be panicked by plugging in a webcam is not a library.
    fn controls(&self) -> Result<Vec<ControlDesc>>;

    /// Read one control's current value, unvalidated \[PF:4\].
    fn get(&mut self, id: ControlId) -> Result<ControlValue>;

    /// Write one control and read it back (design D3).
    ///
    /// The read-back is the backend's obligation, not the engine's: drivers clamp
    /// out-of-range writes and report success \[PF:6\], so only the backend is positioned
    /// to report `{requested, applied}` truthfully for its own device.
    fn set(&mut self, id: ControlId, value: ControlValue) -> Result<Applied>;

    /// Negotiate a format and start streaming.
    fn start_stream(&mut self, request: &StreamRequest) -> Result<NegotiatedStream>;

    /// The stream this camera is running right now, if it is running one.
    ///
    /// **The device is the only authority on itself** (AGENTS rule 4), and "am I streaming"
    /// is the one question about a node that a caller holding a handle onto it cannot answer
    /// any other way: V4L2 has no ioctl for it, every backend already tracks it because
    /// `start_stream` has to refuse a second `STREAMON` with `EBUSY`, and a flag kept beside
    /// the handle by a layer above would be a second answer that can drift from the first.
    /// So it is asked here, of the thing that knows.
    ///
    /// The one caller is `engine::preview::while_suspended`, which stops a running stream
    /// for the length of a photo and starts it again afterwards (design **D12**; note
    /// **N83**). It asks because the *answer* is what it must restore: the negotiated stream
    /// is what the viewers were watching, so resuming asks for exactly that rather than
    /// re-deriving a request and hoping the driver answers the same way twice.
    ///
    /// Owned rather than borrowed, deliberately. A backend whose state lives behind a lock
    /// — the fake's does, because two opens are two views of one device — cannot hand out a
    /// reference into it, and this is asked once per photo rather than once per frame, so a
    /// clone of a few integers costs nothing that matters.
    fn streaming(&self) -> Option<NegotiatedStream>;

    /// Take the next frame, blocking until `deadline`.
    ///
    /// The deadline is an opaque bound the backend honors literally. Settle *policy* —
    /// which frames to skip, when to give up — lives in the engine on a steppable clock;
    /// this parameter is how that policy's decision reaches the blocking read.
    fn next_frame(&mut self, deadline: Instant) -> Result<Frame>;

    /// Stop streaming and release the buffers.
    fn stop_stream(&mut self) -> Result<()>;
}

/// A backend error, by the name the trait signatures use.
///
/// Re-exported here so `use schema::backend::*` gives a backend author everything the
/// contract mentions.
pub type Error = BackendError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_kinds_round_trip_through_their_command_line_names() {
        for &kind in BackendKind::ALL {
            assert_eq!(BackendKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(BackendKind::parse("v4l3"), None);
    }

    #[test]
    fn backend_kind_names_are_distinct() {
        let mut seen = std::collections::BTreeSet::new();
        for &kind in BackendKind::ALL {
            assert!(seen.insert(kind.as_str()), "{kind:?} duplicates a name");
        }
    }

    #[test]
    fn the_traits_are_object_safe() {
        // The whole design rests on `Box<dyn CameraBackend>` being constructible; a
        // signature change that breaks object safety must fail here rather than three
        // crates away.
        fn assert_object_safe(_: Option<&dyn CameraBackend>, _: Option<&dyn Camera>) {}
        fn assert_watch_object_safe(_: Option<&dyn HotplugWatch>) {}
        assert_object_safe(None, None);
        assert_watch_object_safe(None);
    }
}
