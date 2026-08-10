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

/// The daemon's Unix socket inside the runtime directory (design D11):
/// `<XDG runtime dir>/webcam-handler/wchd.sock`.
///
/// Here rather than in the daemon because `wchc` has to resolve the same path to connect
/// (P4f), and two string literals is the drift this module exists to prevent. The
/// *directory* is the security boundary — `connect(2)` checks search permission on every
/// component and write permission on the socket inode, and the socket file itself is
/// created with `0777 & ~umask` — so the 0700 mode D11 names belongs to the directory,
/// which is where `webcam-handler-daemon::uds` asserts it.
pub const DAEMON_SOCKET_FILE: &str = "wchd.sock";

/// The longest Unix socket path the kernel will bind.
///
/// `sockaddr_un::sun_path` is 108 bytes on Linux and the path inside it is
/// NUL-terminated, so 107 bytes is the real bound. It is checked before `bind` rather
/// than inferred from `ENAMETOOLONG` afterwards, because the path is composed from
/// `$XDG_RUNTIME_DIR` and a refusal that names the limit is the difference between "the
/// daemon is broken" and "your `$TMPDIR` is too deep" — which is exactly what a test run
/// under a scratch `$TMPDIR` (`scripts/mutants.sh` exports one) would otherwise look
/// like.
pub const MAX_UNIX_SOCKET_PATH_BYTES: usize = 107;

/// The most connections the daemon serves at once.
///
/// jsonrpsee's own default is 100 and this is not it: a per-user daemon behind a 0700
/// directory has one `wchc` invocation, maybe a browser tab, and whatever a script is
/// doing. The cap exists so a client that leaks connections is refused rather than able
/// to exhaust the daemon's file descriptors, which on this process are also the camera's.
pub const DAEMON_MAX_CONNECTIONS: u32 = 32;

/// The largest JSON-RPC request body the daemon reads.
///
/// Requests on this surface are small: a control id, a handful of writes, a sweep
/// request, a sink path. A megabyte is three orders of magnitude of headroom and still
/// bounds what a caller can make the daemon buffer before it has decided anything.
pub const RPC_MAX_REQUEST_BYTES: u32 = 1024 * 1024;

/// The largest JSON-RPC response body the daemon writes.
///
/// jsonrpsee's default is 10 MB and a photo does not fit in it: D10 answers a capture as
/// base64 in the JSON result, a 4K MJPG frame off the OBSBOT is ~8 MB \[PF:9\], and
/// base64 is four bytes per three. Set here, from the bound the picture actually has,
/// rather than left to be discovered as a wire failure by the sub-milestone that routes
/// `wch_photo`.
pub const RPC_MAX_RESPONSE_BYTES: u32 = 64 * 1024 * 1024;

/// The most calls one JSON-RPC batch may carry.
///
/// jsonrpsee defaults to unlimited, which is an unbounded loop over caller-supplied input
/// on the one socket the daemon always serves. Nothing this project ships batches — `wch`
/// and `wchc` run one verb per invocation — so the bound is small on purpose: it keeps
/// the protocol feature available without making it a lever.
pub const RPC_MAX_BATCH: u32 = 16;

/// How many `accept` calls may fail in a row before the daemon stops accepting.
///
/// An accept error is usually about one client (`ECONNABORTED`: it hung up between
/// `connect` and `accept`), so retrying is right. But `EMFILE` is about *us* and does not
/// clear on its own, and a loop that retries it immediately is a spin at 100% of a core
/// that no log level makes obvious. Consecutive failures are what distinguishes the two,
/// and the daemon gives up rather than spinning.
pub const MAX_CONSECUTIVE_ACCEPT_FAILURES: u32 = 64;

/// How long an open camera may go unused before the next idle sweep closes it.
///
/// D12's "the daemon never opens a camera until first use and closes on idle
/// (configurable), so `wchd` running does not itself block other applications from the
/// webcam" — this is the *default* that makes "configurable" have something to configure
/// (`engine::actor::Cameras::with_idle_timeout` is the override).
///
/// Thirty seconds is chosen from the two failures it sits between. Too short and every
/// `wchc get` in a shell loop pays a fresh `open` and the driver's first-frame settle
/// \[PF:11\]; too long and a person who ran one command cannot start a video call without
/// stopping the daemon, which is precisely the complaint D12 exists to answer. A human
/// working at a terminal issues their next command inside thirty seconds, and has stopped
/// caring about the camera long before they switch applications.
pub const CAMERA_IDLE_CLOSE_MS: u64 = 30_000;

/// How often the daemon asks every open camera whether it has gone idle.
///
/// [`CAMERA_IDLE_CLOSE_MS`] is the deadline and this is the thing that checks it: an idle
/// close nobody asks about never happens, and the pass is what turns
/// `engine::actor::Idle`'s two numbers into a descriptor going away.
///
/// Deliberately shorter than the timeout, and by a whole factor rather than a hair. Since
/// `engine::actor::Idle::expired` reaches its deadline with `>=`, the first pass at or
/// after the deadline is the one that closes: a camera therefore closes **within one
/// cadence of its deadline** — thirty to thirty-five seconds after the last command, not
/// "some multiple of thirty".
///
/// The cost is one mutex read per *actor* every five seconds, plus one acknowledgement per
/// camera that is actually about to close. Per actor rather than per open camera, because
/// nothing is removed from the registry except a dead thread — a machine whose eight
/// cameras have each been used once and closed keeps eight entries — and per *actor* is
/// also the accounting P4d's reaping will be argued against. It is a mutex and not a queue
/// round trip because `engine::actor::CameraActor::sweep` answers from the published state
/// unless the camera is open, unused and quiescent.
pub const CAMERA_IDLE_SWEEP_MS: u64 = 5_000;

// The relation between the two, checked where both numbers are rather than asserted from a
// distance. A cadence at least as long as the timeout would leave a camera open for up to
// cadence-plus-timeout while the documentation above promises thirty seconds, and a zero
// cadence is a spin — `tokio::time::interval` refuses one by panicking, which on a daemon's
// startup path is not an available failure mode. A compile failure is the one red nothing
// can skip.
const _: () = assert!(CAMERA_IDLE_SWEEP_MS > 0 && CAMERA_IDLE_SWEEP_MS < CAMERA_IDLE_CLOSE_MS);

/// How many commands one camera's actor queues before refusing with
/// [`crate::Error::Busy`].
///
/// A bound, not a buffer. One camera is one blocking thread (D12), so everything past the
/// command being executed is already waiting on a device that can only do one thing at a
/// time, and a deeper queue buys a caller nothing but a longer wait before the same
/// answer. Eight is enough that a browser tab's poll and a `wchc` invocation arriving
/// together never collide; a caller past it is told the camera is busy — which is the
/// refusal D12 names — rather than made to wait for a queue nobody bounded.
pub const CAMERA_COMMAND_QUEUE_DEPTH: usize = 8;

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

/// How many calibration progress events a channel sink queues for a consumer that has
/// stopped reading.
///
/// Bounded because the alternatives are both worse than dropping: an unbounded queue lets
/// a stalled subscriber grow the sweep's memory without limit, and a blocking send lets it
/// *stop the sweep* — a camera held at one value because a progress bar went away. The
/// events past this bound are dropped and counted, never silently lost (rubric rule 3).
pub const PROGRESS_QUEUE_DEPTH: usize = 256;

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
