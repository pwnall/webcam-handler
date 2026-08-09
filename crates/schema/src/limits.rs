//! Bounded everything (rubric A14).
//!
//! Every loop over device behavior has a deadline or a cap, and every cap lives here.
//! A constant nobody reads is a defect (rubric A8), so this module grows with the code
//! that consumes it rather than ahead of it.
//!
//! Design §2.10 also makes this module the home of **path layouts**, so D9's session
//! tree is spelled once: `webcam-handler-engine::store` composes the names below into
//! paths, and the CLI and the daemon read the same constants rather than repeating the
//! strings.

/// The persisted session document version (design D9). Bumped when the on-disk shape
/// changes; a foreign version is [`crate::Error::SchemaVersionForeign`], never a
/// best-effort parse.
pub const SESSION_SCHEMA_VERSION: u32 = 1;

/// The session tree's root inside the state directory (design D9):
/// `<state dir>/sessions/<fingerprint-slug>/<task-slug>/<uuidv7>/`.
pub const SESSIONS_DIR: &str = "sessions";

/// The session document inside a session directory (design D9).
pub const SESSION_FILE: &str = "session.json";

/// The append-only event log inside a session directory (design D9).
pub const SESSION_LOG_FILE: &str = "log.ndjson";

/// The photo tree inside a session directory: `photos/<control-slug>/` (design D9).
pub const SESSION_PHOTOS_DIR: &str = "photos";

/// The one advisory lock at the state directory's root (design D9).
///
/// A plain name rather than a dotfile: D9's whole posture is that the state directory is
/// inspectable, and an operator wondering who owns it should see the file that answers.
pub const STORE_LOCK_FILE: &str = "lock";

/// The device-profile document version (design T3).
pub const PROFILE_SCHEMA_VERSION: u32 = 1;

/// Frames to discard before a photo, by default.
///
/// Frames immediately after `STREAMON` are dark and miscolored until AE and AWB
/// converge \[PF:11\]; ten was enough on both seed cameras \[PF:9\].
pub const DEFAULT_SETTLE_SKIP_FRAMES: u32 = 10;

/// How long a settle policy may wait before giving up with
/// [`crate::Error::SettleTimeout`].
pub const DEFAULT_SETTLE_DEADLINE_MS: u64 = 5_000;

/// How long a single `DQBUF` may block before the deadline logic gets a turn.
pub const FRAME_DEADLINE_MS: u64 = 2_000;

/// The most times a settle loop may go round before giving up.
///
/// The **deadline** is the real bound; this is the backstop for the one case the deadline
/// cannot cover — a round that neither consumes a frame nor advances the clock. That
/// happens when a backend returns "no frame" without waiting *and* the clock is not
/// moving, which is a test's `SteppedClock` today and would be a wedged monotonic clock in
/// the field. Either way, spinning is the one outcome that helps nobody: the loop gives up
/// and reports the settle timeout it would eventually have reported anyway.
///
/// Generous by design. At 30 fps a `SettleFor` policy running the full
/// [`DEFAULT_SETTLE_DEADLINE_MS`] sees ~150 frames, so this leaves an order of magnitude
/// before a legitimate settle could reach it.
pub const MAX_SETTLE_ROUNDS: u32 = 4_096;

/// Buffers to request from the driver for a capture stream.
pub const DEFAULT_BUFFER_COUNT: u32 = 4;

/// The most processes a `Busy` refusal names.
///
/// The walk that finds them reads the whole process table, and a refusal listing four
/// hundred processes is less readable than one listing none. Four is enough to say "these
/// have it" and short enough to read in a terminal.
pub const MAX_HOLDERS_REPORTED: usize = 4;

/// The most buffers one stream may ask the driver to allocate.
///
/// `buffer_count` arrives from a caller (a CLI flag, an RPC field), and every buffer is a
/// driver-side allocation of a whole frame — 8 MB each at 4K MJPG on the OBSBOT. A request
/// for a thousand is not a caller who wants smooth capture, and the driver would be the
/// one to run out of memory.
pub const MAX_BUFFERS_PER_STREAM: u32 = 32;

/// The most samples one sweep may take.
///
/// A sweep of a 468000-wide pan range at step 3600 would otherwise plan 260 photos and
/// a great deal of motor travel.
pub const MAX_SWEEP_SAMPLES: u32 = 256;

/// The most samples a sweep that *moves motors* may take (design §5: motors wear).
pub const MAX_MOTION_SWEEP_SAMPLES: u32 = 32;

/// The largest control payload we will read back from a device.
///
/// `elem_size × elems` is device-supplied, and a lying driver is attacker-shaped input
/// (rubric B10): the product is checked against this before anything is allocated.
pub const MAX_CONTROL_PAYLOAD_BYTES: usize = 64 * 1024;

/// The most menu indices `VIDIOC_QUERYMENU` is asked about for one control.
///
/// Menus are sparse and enumeration walks `min..=max` tolerating holes \[PF:2\]; a driver
/// reporting a menu range of `0..=i64::MAX` must not become an infinite loop.
pub const MAX_MENU_INDICES: u32 = 4_096;

/// The most device nodes one camera group may contain before we stop believing the
/// grouping. Real groups hold two \[PF:7\].
pub const MAX_NODES_PER_CAMERA: usize = 16;

/// The most controls one device may enumerate.
///
/// `QUERY_EXT_CTRL` with `NEXT_CTRL` walks in strictly increasing id order and ends with
/// `EINVAL`, so a well-behaved driver terminates on its own. A driver that never says
/// `EINVAL` would otherwise walk the whole 32-bit id space; the seed cameras expose 18
/// and 24.
pub const MAX_CONTROLS_PER_DEVICE: u32 = 1_024;

/// The most pixel formats one capture node may enumerate. The seed cameras offer two.
pub const MAX_FORMATS_PER_NODE: u32 = 128;

/// The most frame sizes one pixel format may enumerate. The Chicony's MJPG offers nine.
pub const MAX_FRAME_SIZES_PER_FORMAT: u32 = 256;

/// The most frame intervals one size may enumerate. The OBSBOT's 1080p offers ten.
pub const MAX_FRAME_INTERVALS_PER_SIZE: u32 = 256;
