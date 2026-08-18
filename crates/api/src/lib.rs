//! The one webcam-handler wire surface (T5).
//!
//! One jsonrpsee `#[rpc(server, client)]` trait over `webcam-handler-schema` DTOs, plus
//! the single exhaustive match mapping the D13 error registry onto JSON-RPC codes.
//!
//! | Module | Home of |
//! |---|---|
//! | [`codes`] | the D13 → JSON-RPC code registry, both directions (D10, D13) |
//! | [`photo`] | D10's base64-in-JSON photo answer (D6, D10) |
//! | [`wire`] | the surface as data — the inventories, declared with the traits they describe |
//! | this file | the T5 surface itself — one method per operation the daemon routes, plus the two subscriptions |
//!
//! ## Why one declaration, and why both halves in one crate
//!
//! D10 makes the whole daemon API one `#[rpc(server, client)]` trait: the daemon implements
//! the server half, `webcam-handler-client` consumes the generated client, and the direct CLI
//! calls the same operations on the engine through T4's executor. A verb therefore exists
//! exactly once. Splitting the *surface* so the client and the server could live in different
//! crates would produce two wire surfaces, which is the thing D10 exists to prevent — and it
//! is the alternative note N5 weighed and rejected when the tokio question came up.
//!
//! P4e-i makes it **one declaration and two generated traits**: `WchRpc` carries the
//! twenty-two calls and `WchEvents` the two subscriptions, both out of one `wire_surface!`
//! invocation below. [`wire`]'s header has the three measured facts that force the split
//! and the argument that what D10 protects — one source — survives it; note **N57** is the
//! entry. The daemon merges the two registrations into the one `Methods` value it serves
//! (`daemon::server::mount`), so there is still exactly one registration path.
//!
//! ## Errors on the wire
//!
//! Every method answers with a [`codes::WireError`], which is the D13 registry and nothing
//! else: `code` from the closed range [`codes::D13_CODES`], `message` from the error's own
//! `Display`, `data` the serialized error. No `anyhow` string crosses this seam, and no
//! camera frame — or anything derived from one — reaches a message or a `data` field.
//! [`codes`] explains why the mapping is exhaustive over `ErrorKind` rather than over
//! `Error`; it is not a style choice.
//!
//! ## Paths on the wire
//!
//! D10: a relative `-o out.jpg` resolves against the **caller's** cwd, *before* the request is
//! sent, so it means the same file under `webcam-handler-cli` and `webcam-handler-client`.
//! That resolution lives once, in the shared command surface
//! (`cli_core::Command::photo_request`), which is why the methods here take an assembled
//! [`schema::capture::PhotoRequest`] rather than the flags one is built from: a server handed
//! the raw flags would have to build the sink, and the only cwd it has is its own — under
//! systemd, `/`.
//!
//! Paths travelling the *other* way are the server's and are not caller-relative:
//! `SessionListing::path` is absolute in the daemon's state directory, and a sample's
//! `photo` is relative to its session directory, which lives there too (D9). For a
//! per-user UDS daemon that is the same filesystem; a browser client (P5c) can open
//! neither, and reads the documents rather than the files.
//!
//! ## The document that describes this surface
//!
//! `xtask generate` writes `schemas/webcam-handler-openrpc.json` from [`METHODS`] and from
//! [`codes::rpc_code`], and `scripts/gates/schema-artifacts-current.sh` re-runs the
//! emitter and diffs it every CI run. Like the JSON Schema bundle beside it, that file is
//! a **generated artifact — documentation, not a second source of truth**: nothing in this
//! workspace reads it back, and the types and the trait here are what the daemon actually
//! speaks. [`wire`] explains why the trait is declared through a macro, which is what
//! keeps the document's method list from being a second list of methods.
//!
//! ## N5's wall
//!
//! This crate links tokio, because `jsonrpsee-core` activates it from both its client and
//! its server features — measured across four feature sets in note N5. The half of the
//! wall that was actually protecting something is intact and gate-asserted: no axum, no
//! hyper, no tower-http. The behavioural half — that this crate never *starts* a runtime
//! or spawns a task — is review-held, because linkage cannot see it. Nothing here runs;
//! it declares.
#![forbid(unsafe_code)]
// The wire surface is request-driven end to end: every value that reaches it was written
// by somebody else and arrived over a socket.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod codes;
pub mod photo;
pub mod wire;

use schema::backend::HotplugEvent;
use schema::capture::PhotoRequest;
use schema::control::{ControlDesc, ControlSlug, ControlWrite};
use schema::profile::DeviceProfile;
use schema::progress::ProgressEvent;
use schema::report::{
    CameraDetail, CameraList, ControlReport, DiscoveryReport, TerminationReport, WriteReport,
};
use schema::selector::CameraSelector;
use schema::session::{Selection, Session, SessionList, SessionRef, SessionStatus, SweepRequest};
use schema::snapshot::{RestoreReport, Snapshot};
use schema::video::{RecordReport, RecordRequest, RecordStatus};

use wire::wire_surface;

pub use codes::{WireError, rpc_code};
pub use photo::{Base64Bytes, PhotoResponse};

wire_surface! {
    namespace = "wch";

    /// Everything the daemon answers (design D10, T5).
    ///
    /// Declared through [`wire`]'s `wire_surface!`, which supplies the two things that are
    /// laws rather than per-method choices — `#[rpc(server, client)]` and
    /// `param_kind = map` — and emits [`METHODS`] from the same tokens, so the document
    /// xtask writes cannot describe a surface this trait does not have.
    ///
    /// The namespace is `wch` — the literal nine lines above, and a wire break rather than a
    /// spelling (note **N91**) — and jsonrpsee's default separator is `_`, so the
    /// wire names are `wch_list`, `wch_calibrate_start`, and so on. Every method takes
    /// **named** parameters, and the emitted document says so (`"paramStructure": "by-name"`):
    /// a request object costs nothing on the server, and it buys a legible OpenRPC document
    /// and a hand-written web client (P5c) that reads its own requests. The generated server
    /// also accepts a positional array — but not for every method (see [`wire::Param`]), which
    /// is why the document commits to the one shape that always works rather than to "either".
    ///
    /// Every method is `async`. The daemon's implementation hands the request to the camera's
    /// actor thread (D12) and awaits a reply; `#[method(blocking)]` would put a minutes-long
    /// sweep on a tokio blocking thread *and* still queue it behind the actor, which is two
    /// queues for one device.
    ///
    /// The Rust names are the wire names, and they follow D10's spelling rather than T4's
    /// where the two differ — `profile_capture` here, `Executor::capture_profile` there. T4 is
    /// a settled surface and is not renamed for this; the two are allowed different spellings
    /// because only one of them is the wire.
    pub trait WchRpc {
        /// Every camera, plus anything worth saying about what is missing (D1).
        ///
        /// # Errors
        ///
        /// Whatever the backend says. An empty list is not an error — D1's "an empty
        /// enumeration is diagnosed, not shrugged at" is answered by the hints in the list,
        /// not by a refusal.
        #[method(name = "list")]
        async fn list(&self) -> Result<CameraList, WireError>;

        /// One camera's identity and format tree.
        ///
        /// # Errors
        ///
        /// [`schema::Error::CameraUnknown`] or [`schema::Error::CameraAmbiguous`] for a
        /// selector that does not resolve; otherwise whatever the backend says.
        #[method(name = "info")]
        async fn info(&self, camera: CameraSelector) -> Result<CameraDetail, WireError>;

        /// One camera's control set, and the auto/manual pairs in effect for it.
        ///
        /// Read-only: the pairs it reports are the declared table filtered to this device
        /// (D3's first layer). Measuring pairs on the device is `wch_discover_pairs`,
        /// which is a different method because it writes.
        ///
        /// # Errors
        ///
        /// As `wch_info`.
        #[method(name = "controls")]
        async fn controls(&self, camera: CameraSelector) -> Result<ControlReport, WireError>;

        /// Measure this camera's auto/manual pairs by asking it (D3's second layer).
        ///
        /// **This writes to the camera.** The probe toggles automation-shaped controls, reads
        /// the control set back, and restores what it touched. It is its own method rather
        /// than a flag on `wch_controls` because the daemon has to route, permission and
        /// count it as the write it is — the plan routes the read verbs at P4b and this one
        /// with the mutating half at P4c, which is only expressible if they are two methods.
        /// The shared command surface keeps its `controls --discover-pairs` flag; a verb with
        /// a flag and an operation with a name are different units, and the engine still has
        /// exactly one probe.
        ///
        /// The answer is more than a [`ControlReport`] for the same reason: what the probe
        /// declined to touch, and what it could not put back, are facts about the run that
        /// `webcam-handler-cli` prints on standard error. A caller that could not see them
        /// would be running a write with its restoration report withheld.
        ///
        /// # Errors
        ///
        /// As `wch_info`, plus whatever the camera said when the probe could not begin —
        /// a probe that cannot record where the camera started must not start.
        #[method(name = "discover_pairs")]
        async fn discover_pairs(&self, camera: CameraSelector) -> Result<DiscoveryReport, WireError>;

        /// One control's descriptor and current value.
        ///
        /// The whole descriptor rather than the bare value: a value with no range, no flags
        /// and no menu is not renderable, and an agent reading this needs the same context a
        /// human reading the table does.
        ///
        /// # Errors
        ///
        /// As `wch_info`, plus [`schema::Error::ControlUnknown`] naming the closest
        /// slugs this camera does have.
        #[method(name = "get")]
        async fn get(
            &self,
            camera: CameraSelector,
            control: ControlSlug,
        ) -> Result<ControlDesc, WireError>;

        /// Write controls, switching automation off first unless `guarded` is false (D3).
        ///
        /// The answer carries every write the plan made — the automation switch-offs
        /// included — each with its own `{requested, applied}` pair, because requested is not
        /// applied (E4) and a clamp is a warning on the result rather than an error code
        /// (D13: a warning with an error code is a success nobody can distinguish from a
        /// failure).
        ///
        /// # Errors
        ///
        /// As `wch_info`, plus the planner's refusals —
        /// [`schema::Error::ControlReadOnly`], [`schema::Error::ControlInactive`] naming the
        /// automation to disable — and the device's.
        #[method(name = "set")]
        async fn set(
            &self,
            camera: CameraSelector,
            writes: Vec<ControlWrite>,
            guarded: bool,
        ) -> Result<WriteReport, WireError>;

        /// Every writable control's current value (D4).
        ///
        /// # Errors
        ///
        /// As `wch_info`.
        #[method(name = "snapshot")]
        async fn snapshot(&self, camera: CameraSelector) -> Result<Snapshot, WireError>;

        /// Put a snapshot back, automation before manual (D4).
        ///
        /// Takes the snapshot document itself, not a path: a client reads its own file and
        /// sends the value, so `webcam-handler-client restore` reads the caller's filesystem
        /// rather than the daemon's.
        ///
        /// # Errors
        ///
        /// As `wch_info`, plus [`schema::Error::FingerprintMismatch`] when the snapshot
        /// came from a different camera. A control that could not be put back is in the
        /// *report*, not an error — including the one that is a success, a control whose
        /// automation partner owns it now as it did then (note N9).
        #[method(name = "restore")]
        async fn restore(
            &self,
            camera: CameraSelector,
            snapshot: Snapshot,
        ) -> Result<RestoreReport, WireError>;

        /// Take one photo (D5, D6, D10).
        ///
        /// Takes an assembled [`PhotoRequest`], never the flags one is built from: the sink
        /// carries either "hand the bytes back" or an **absolute** server path, and D10 puts
        /// the relative-path resolution on the caller's side, before the request is sent. A
        /// relative `Sink::ServerPath` arriving here is a request the server must refuse
        /// rather than resolve against its own working directory, which under systemd is `/`.
        /// That rule is not restated here as prose to be remembered: it is
        /// [`schema::capture::Sink::is_addressable`], beside the variants it constrains, and
        /// the routing that landed at P4c calls it — before it resolves a camera, so a sink
        /// this server cannot address costs nobody a descriptor. Its sibling
        /// [`schema::capture::Sink::writable_format`] is asked at the same moment and is the
        /// reason a path named `.webp` is a refusal rather than JPEG bytes in a file whose
        /// name says otherwise.
        ///
        /// A `ServerPath` sink is a write primitive: any client that can call this can write a
        /// file anywhere the daemon's uid can. That is deliberate and it is exactly what D11's
        /// authentication model covers — filesystem permissions on a 0700 socket directory —
        /// and it is why the opt-in TCP listener is token-gated with no flag that removes the
        /// token off loopback.
        ///
        /// # Errors
        ///
        /// As `wch_info`, plus [`schema::Error::SettleTimeout`] \[PF:11\],
        /// [`schema::Error::FormatUnsupported`] when the camera offers nothing that was asked
        /// for, [`schema::Error::StorageIo`] from the sink, and
        /// [`schema::Error::IllegalTransition`] for a request this server will not honour at
        /// all — a `ServerPath` that is not absolute, or one whose extension names an
        /// encoding this build does not write, naming the three it does; or a
        /// `settle.deadline_ms` past 10000 (`schema::limits::MAX_SETTLE_DEADLINE_MS`), which
        /// is refused rather than clamped for `wch_record_start`'s reason one bound along: one
        /// camera is one thread (D12), so a settle is time nobody else on this daemon gets,
        /// and an agent whose settle was quietly shortened cannot tell that from a sensor
        /// that converged. All of them are answered before a camera is resolved and before
        /// the destination is opened, so a request this build was never going to honour
        /// costs nobody a descriptor and leaves no file where none was (note **N147**).
        /// Deliberately not `FormatUnsupported`: the camera declined nothing, and `.webp` is
        /// not its fault (E3, note N46). Deliberately not `SettleTimeout` either: nothing
        /// waited.
        #[method(name = "photo")]
        async fn photo(
            &self,
            camera: CameraSelector,
            request: PhotoRequest,
        ) -> Result<PhotoResponse, WireError>;

        /// Start recording `camera` to a file on the host this daemon runs on (D7, D10).
        ///
        /// **The first of three, and the only one that takes a request.** A recording is not
        /// one call, and cannot be: this trait's surface is served by a daemon that gives
        /// each open camera one thread taking one command at a time, so a take written as a
        /// single method would make `wch_record_stop` undeliverable — the stop would queue
        /// behind the recording it exists to stop, and only the take's own duration could ever
        /// end it (note **N111**). So this method starts a take and returns as soon as the
        /// container's header is on disk, and the take runs on a driver of the daemon's own.
        ///
        /// The answer is the take's first [`schema::video::RecordStatus`], which is what makes
        /// the sequence usable by a caller that has not enumerated the camera: it names the
        /// container the negotiation chose and the file the bytes are going to, both of which a
        /// request that left the extension off (see
        /// [`schema::video::RecordRequest::container`]) deliberately did not decide.
        ///
        /// **A recording is not returned as bytes.** [`schema::video::RecordRequest::sink`]
        /// takes one of D10's two sink variants and refuses the other: a take at its own cap is
        /// thirty-two times [`schema::limits::RPC_MAX_RESPONSE_BYTES`] before base64 grows it
        /// by a third, so `ReturnBytes` names an answer this build cannot send (note **N110**).
        /// The path is absolute for `wch_photo`'s reason — this daemon's working directory
        /// under systemd is `/` — and `cli_core::Command::record_request` resolves a relative
        /// `-o` against the *caller's* cwd before the request is sent (D10).
        ///
        /// # Errors
        ///
        /// As `wch_info`, plus:
        /// [`schema::Error::IllegalTransition`] for a request this build will not honour at all
        /// — a `ReturnBytes` sink, a relative path, an extension naming a container this build
        /// does not write, or a duration past [`schema::limits::MAX_RECORDING_MS`], which is
        /// refused rather than clamped because an agent whose take was quietly shortened cannot
        /// tell that from a camera that stopped (note **N46** widened the variant, note
        /// **N110**).
        /// [`schema::Error::Busy`] when this camera is **already recording** — which means
        /// *retry*, and is the honest advice: the take that is running is bounded by its own
        /// duration. It is also what a camera whose one thread is held by something else
        /// answers, unless the request set `wait` (D12).
        /// [`schema::Error::FormatUnsupported`] carrying a `container` payload when the file the
        /// path names cannot carry what the device negotiated — D7's "non-MJPG `record` requests
        /// get Y4M or `FormatUnsupported`" arriving at a caller who typed `.avi` over a GREY
        /// camera. The remedy it names is the **extension**, because the format was chosen by
        /// D5's ranking rather than typed: `container.carried_by` lists every file this build
        /// writes that would have taken those frames, and it names no pixel format, since a
        /// format this build records is not a format this camera has (note **N211**).
        /// [`schema::Error::StorageIo`] from the destination, and the device's own answer for
        /// anything it refused.
        #[method(name = "record_start")]
        async fn record_start(
            &self,
            camera: CameraSelector,
            request: RecordRequest,
        ) -> Result<RecordStatus, WireError>;

        /// What `camera`'s recording is doing, or what its last one turned out to be.
        ///
        /// **D10's whole progress mechanism**: *"progress by polling `wch_record_status` — no
        /// recording subscription in v1"*. A client polls this between `wch_record_start` and
        /// `wch_record_stop` and stops when [`schema::video::RecordStatus::is_running`] answers
        /// `false`; `schema::limits::CLIENT_RECORD_POLL_MS` is what
        /// `webcam-handler-client` polls at.
        ///
        /// A camera holds **at most one** take, and this answers about that one: the take that
        /// is running, or the one that has ended and not been collected by `wch_record_stop`.
        /// A camera holding neither answers with no take at all, which is the same answer for a
        /// camera that has never recorded and for one whose take has been collected — those are
        /// one fact rather than two, because what a caller can do about either is start a
        /// recording, and remembering the difference would be this daemon keeping something
        /// about a camera after the caller asked for it to be handed over.
        ///
        /// **It is a read.** It opens no camera, takes no lock, and never changes what a later
        /// `wch_record_stop` will answer — so a poll loop that ran twice as fast would cost the
        /// take nothing. What it does cost is a live enumeration per call (E2: a camera id is
        /// resolved against the machine every time), which is why the poll interval is a
        /// bounded constant rather than as fast as a client can ask.
        ///
        /// # Errors
        ///
        /// As `wch_info`. A camera with no recording is **not** an error — that is the answer,
        /// and a refusal there would make an agent unable to tell "nothing is recording" from
        /// "this camera is gone" (AGENTS rule 7).
        #[method(name = "record_status")]
        async fn record_status(&self, camera: CameraSelector) -> Result<RecordStatus, WireError>;

        /// End `camera`'s recording and hand over what it turned out to be (D7, D10).
        ///
        /// **Stop *and collect*, which is one verb because they are one question.** A take that
        /// is running is asked to stop, its container is closed, and the finished
        /// [`schema::video::RecordReport`] is the answer; a take that has already reached its
        /// own bound — the ordinary case, since most takes end on their duration while the
        /// caller is still polling — is simply handed over. Either way the camera's slot is
        /// emptied, so the sequence `wch_record_start` → poll → `wch_record_stop` is total for
        /// every ending a take can have, which is what AGENTS' handless primary consumer needs
        /// of a verb that spans three calls.
        ///
        /// The stop is delivered within one `schema::limits::FRAME_DEADLINE_MS`, because the
        /// driver issues one command per frame and is between commands for most of every turn
        /// (note **N111** — that is the whole reason a recording is a chain of turns).
        ///
        /// **A take that failed is answered with the device's own refusal, not with a report.**
        /// The container is still closed first, so docs/7 P6b's "every fault leaves a parseable
        /// file" holds across the wire as it does in process; what the caller gets is the
        /// `DeviceGone` or the `StorageIo` that ended it, because a refusal arriving as a
        /// successful recording is the conversion AGENTS rule 7 forbids.
        ///
        /// # Errors
        ///
        /// As `wch_info`, plus [`schema::Error::IllegalTransition`] when this camera holds no
        /// take at all — deliberately not a bland "nothing happened", because an unattended
        /// caller has to be able to tell "my recording is over" from "my `wch_record_start`
        /// never took", and those two are the same call's two answers.
        /// [`schema::Error::Busy`] when a `wch_record_start` for this camera is still
        /// negotiating, which means retry for the same reason it does there. Otherwise whatever
        /// ended the take: the device's answer, or [`schema::Error::StorageIo`] naming the
        /// file.
        #[method(name = "record_stop")]
        async fn record_stop(&self, camera: CameraSelector) -> Result<RecordReport, WireError>;

        /// One camera's full device profile (T3).
        ///
        /// `capturer` is provenance: a profile records who took it, because a transcription
        /// and a probe are different claims about a device (E1, E2).
        ///
        /// `discover_pairs` runs D3's empirical probe before the reading and folds what it
        /// measured into `invariant.measured_pairs`. It **writes to the camera** — every
        /// automation-shaped control is toggled through its positions and put back — which is
        /// why it is a parameter a caller supplies rather than something this method decides.
        /// A profile is a corpus entry, and whether the device was perturbed to produce one is
        /// a fact the document's own reader needs, so it belongs to the call (note **N239**).
        /// `false` leaves the pair set empty, which means *this capture measured none* and
        /// never *this device has none*. The probe's other two answers — what it declined, and
        /// what the restore achieved — are `wch_discover_pairs`', and a caller that needs them
        /// asks that verb.
        ///
        /// # Errors
        ///
        /// As `wch_info`, plus, when the probe was asked for, its refusal to begin: a camera
        /// whose state cannot be recorded is not probed, so nothing is written to it.
        #[method(name = "profile_capture")]
        async fn profile_capture(
            &self,
            camera: CameraSelector,
            capturer: String,
            discover_pairs: bool,
        ) -> Result<DeviceProfile, WireError>;

        /// Signal the process holding a camera's node (design §5).
        ///
        /// Names both the camera and the pid, and is never a fallback behaviour of anything
        /// else: nothing in this surface kills a process to get a device free. The answer says
        /// what was sent and whether the node was still held afterwards, because signalling is
        /// a request and a process may ignore it — E4's doctrine does not stop at the device.
        ///
        /// # Errors
        ///
        /// As `wch_info`, plus [`schema::Error::HolderGone`] when the pid no longer
        /// holds the device, which is a refusal rather than a no-op: signalling a pid that has
        /// been recycled would kill somebody else's process.
        #[method(name = "terminate_holder")]
        async fn terminate_holder(
            &self,
            camera: CameraSelector,
            pid: i32,
        ) -> Result<TerminationReport, WireError>;

        /// Open a calibration session for a camera and a task (D8).
        ///
        /// # Errors
        ///
        /// As `wch_info`, plus [`schema::Error::SessionConflict`] when the slot already
        /// holds an open session (note N14), and whatever the store refuses with —
        /// [`schema::Error::StoreLocked`] when another process owns the state directory (D9).
        #[method(name = "calibrate_start")]
        async fn calibrate_start(
            &self,
            camera: CameraSelector,
            task: String,
            goal: String,
            criteria: Vec<String>,
        ) -> Result<Session, WireError>;

        /// Queue controls for calibration, or reorder the queue.
        ///
        /// `controls` empty means every control the camera has. `order` treats the named
        /// controls as the queue's new order rather than as additions.
        ///
        /// # Errors
        ///
        /// As `wch_calibrate_start`, plus [`schema::Error::ControlUnknown`] for a slug
        /// this camera does not have and [`schema::Error::IllegalTransition`] when `order` is
        /// not a permutation of the queue.
        #[method(name = "calibrate_plan")]
        async fn calibrate_plan(
            &self,
            camera: CameraSelector,
            session: SessionRef,
            controls: Vec<ControlSlug>,
            order: bool,
        ) -> Result<Session, WireError>;

        /// Sweep one control and record a sample per value (D8).
        ///
        /// Request and response, with no progress parameter: the live events are
        /// `schema::progress::ProgressEvent`s and P4e puts them on their own subscription
        /// rather than threading a watcher through a call. Until then a client's progress bar
        /// simply does not move, which is honest; a boolean or a subscription id in this
        /// request would be a wire field with no producer and a second progress vocabulary
        /// forever.
        ///
        /// This is the one method whose latency is unbounded by design — a sweep is minutes of
        /// camera time — so a client's request timeout has to be raised or disabled at
        /// connection setup rather than left at jsonrpsee's 60-second default.
        ///
        /// # Errors
        ///
        /// As `wch_calibrate_start`, plus the planner's refusals (a control with no
        /// ordered range; a motorized control without `allow_motion`, because §5 says a plan
        /// that would move motors says so first) and whatever the camera said at the sample
        /// that stopped it. [`schema::Error::IllegalTransition`] also for a
        /// `settle.deadline_ms` past 10000 (`schema::limits::MAX_SETTLE_DEADLINE_MS`), which
        /// is answered before the camera is resolved — a sweep that reached its first sample
        /// would have armed a pre-sweep snapshot, switched an automation partner off and
        /// moved a motor before finding out this build was never going to honour the request
        /// (note **N147**).
        #[method(name = "calibrate_sweep")]
        async fn calibrate_sweep(
            &self,
            camera: CameraSelector,
            session: SessionRef,
            request: SweepRequest,
        ) -> Result<Session, WireError>;

        /// A session's document and its history.
        ///
        /// # Errors
        ///
        /// As `wch_info`, plus [`schema::Error::SchemaVersionForeign`] for a session
        /// another build wrote (D9) — a foreign document is a typed refusal, never a
        /// best-effort parse.
        #[method(name = "calibrate_status")]
        async fn calibrate_status(
            &self,
            camera: CameraSelector,
            session: SessionRef,
        ) -> Result<SessionStatus, WireError>;

        /// Record a control's chosen value and who chose it (D8).
        ///
        /// # Errors
        ///
        /// As `wch_calibrate_start`, plus the D8 machine's refusals: a control that
        /// never swept, a value no sample holds, a metric that cannot rank.
        #[method(name = "calibrate_select")]
        async fn calibrate_select(
            &self,
            camera: CameraSelector,
            session: SessionRef,
            control: ControlSlug,
            selection: Selection,
        ) -> Result<Session, WireError>;

        /// Write a session's calibrated values back to a camera, automation first (D4's
        /// ordering, D8's gate).
        ///
        /// # Errors
        ///
        /// As `wch_calibrate_start`, plus [`schema::Error::FingerprintMismatch`] naming
        /// the fields that differ when the camera is not the one the session was recorded
        /// against, and [`schema::Error::IllegalTransition`] when the session still has
        /// uncalibrated work and `partial` is false.
        #[method(name = "calibrate_apply")]
        async fn calibrate_apply(
            &self,
            camera: CameraSelector,
            session: SessionRef,
            partial: bool,
        ) -> Result<WriteReport, WireError>;

        /// Put the camera back where this session found it, from its pre-sweep snapshot
        /// (note N23).
        ///
        /// The eighth calibrate verb, and the one that makes D4's "leave the camera as you
        /// found it" true for a session rather than only for a single write. Running it twice
        /// is not an error: the second time there is nothing left to put back, and an empty
        /// report says so.
        ///
        /// # Errors
        ///
        /// As `wch_calibrate_start`, plus [`schema::Error::FingerprintMismatch`] when
        /// the camera is not the session's.
        #[method(name = "calibrate_restore")]
        async fn calibrate_restore(
            &self,
            camera: CameraSelector,
            session: SessionRef,
        ) -> Result<RestoreReport, WireError>;

        /// Every session on this machine, or one camera's, newest first.
        ///
        /// `camera` is the only optional parameter on this surface: `null`, or the key left
        /// out altogether, both mean every session. Measured against a real `RpcModule` —
        /// `{"params":{}}` and `{"params":{"camera":null}}` are answered identically —
        /// because serde resolves a missing `Option` field through `missing_field`, which
        /// visits `None`. The emitted document says the same thing by marking this parameter
        /// `required: false`, which is derived from its schema rather than written twice.
        ///
        /// # Errors
        ///
        /// As `wch_info` when a camera is named; otherwise whatever the store says. A
        /// session whose document this build cannot read still *lists* — listing parses
        /// nothing (D9).
        #[method(name = "calibrate_list")]
        async fn calibrate_list(&self, camera: Option<CameraSelector>) -> Result<SessionList, WireError>;
    }

    /// Everything the daemon *tells* a client without being asked (design D10, T5).
    ///
    /// The second trait [`wire`]'s `wire_surface!` emits, from the same declaration as
    /// `WchRpc` — see [`wire`]'s header for the three measured facts that make it two
    /// traits rather than one, and note **N57** for what that costs D10's sentence and what
    /// it preserves. The daemon merges both registrations into one `Methods`; a client
    /// reaches these over a **WebSocket upgrade** on the same Unix socket, because
    /// jsonrpsee's HTTP path serves calls only.
    ///
    /// Neither subscription takes a parameter, and that is a decision with two
    /// independent reasons rather than an omission. `schema::progress`'s own header
    /// settles the design half — "P4e's subscription is per *client* and a client may
    /// watch a daemon running more than one session", which is why the session id rides on
    /// every event and why D10's parenthetical "per-session progress" is answered by a
    /// consumer-side filter rather than by a wire field. The mechanical half is note N5's
    /// wall: jsonrpsee's generated server calls `tokio::spawn` on a subscription's
    /// params-decoding error path, and a parameterless subscription generates no such
    /// call, so this crate still starts no runtime and spawns no task.
    ///
    /// **Refusals.** A subscription that cannot be opened is rejected before it is
    /// accepted, with the same [`codes::WireError`] every method answers with — the D13
    /// registry and nothing else. After the accept there is no JSON-RPC code left to
    /// carry: what jsonrpsee can send is a `subscription_error` notification, so a stream
    /// that ends abnormally ends with the typed reason as its payload. Which stream ends
    /// on which loss is each method's own business, below.
    pub trait WchEvents {
        /// Device nodes appearing and disappearing, as the kernel reports them (D10, P4d).
        ///
        /// One `HotplugEvent` per node, never per camera: "re-enumeration decides what
        /// camera it belongs to — the event names a node, never a camera, because grouping
        /// is not a node property" (`schema::backend::HotplugEvent`). A client that wants
        /// the camera list calls `wch_list` afterwards, which enumerates live (E2).
        ///
        /// The daemon's watch is a **diff of successive readings of the node tree** (note
        /// N53), so it inherits that shape's one honest hole: a node that leaves and
        /// returns entirely between two readings is not reported at all. It opens no node,
        /// because subscribing to events is not a use of a camera (D12, N53).
        ///
        /// **A subscriber that falls too far behind is closed rather than resynced**, and
        /// that is the opposite of what `wch_subscribe_calibration` does one method down.
        /// A `HotplugEvent` is a *delta*: a gap makes a consumer's picture of the node tree
        /// wrong in a way it cannot detect, and there is no variant in this closed
        /// vocabulary that means "you missed some". Ending the stream with the count is the
        /// only answer that does not hand over a quiet lie — the client re-subscribes and
        /// re-enumerates.
        ///
        /// # Errors
        ///
        /// Whatever the backend refuses `CameraBackend::watch` with — typically
        /// [`schema::Error::DeviceIo`] when the uevent socket cannot be opened, which is a
        /// container or an LSM rather than a missing privilege \[PF:21\]. The watch is
        /// started on the first subscription rather than at startup, deliberately: a
        /// daemon that refused to *start* because it could not watch would be answering
        /// `wch_list` with nothing on a host where enumeration works perfectly (E3).
        #[subscription(name = "subscribe_events", item = HotplugEvent)]
        async fn subscribe_events(&self);

        /// Every calibration sweep's live progress, from every session (D8, D10, P3c).
        ///
        /// `schema::progress::ProgressEvent` verbatim and no second vocabulary: the same
        /// events `webcam-handler-cli calibrate sweep` renders its progress bar from, which is
        /// what docs/7's risk register asked P3c to guarantee. Each carries its session id and
        /// each in-flight variant carries `index`/`total`, so a client that connects mid-sweep
        /// can paint a truthful bar from the first event it sees.
        ///
        /// **Every session, filtered by the consumer.** The stream is per client, not per
        /// session; `ProgressEvent::session` is what a client with one session in mind
        /// matches on. A session parameter would resolve a `SessionRef` at subscribe time
        /// against a store lock this path has no business taking, and a
        /// `SessionRef::Task` subscription would silently follow whichever session
        /// occupied the slot next.
        ///
        /// **A subscriber that falls behind loses events and is told how many** — the
        /// opposite of `wch_subscribe_events`, for a reason in the payload rather than in
        /// the transport: `index`/`total` ride on every in-flight variant, so a gap is
        /// self-healing and the next event repaints a correct bar. The alternative,
        /// closing the stream, would end a client's view of a twenty-minute sweep because
        /// it was briefly slow. Nothing a subscriber does reaches the sweep: the camera is
        /// never held for a progress bar's benefit (note N17).
        ///
        /// A sweep that *fails* does not fail this stream. The refusal arrives as an
        /// event — `CalibrationProgress::SweepInterrupted`, carrying the D13 discriminant
        /// — because the sweep's own caller is the one being refused.
        ///
        /// # Errors
        ///
        /// Nothing, today: the events come from this daemon's own sweeps rather than from
        /// a device, so there is nothing here to refuse with. The method still answers a
        /// [`codes::WireError`] on rejection because that is the one shape a refusal on
        /// this surface may take.
        #[subscription(name = "subscribe_calibration", item = ProgressEvent)]
        async fn subscribe_calibration(&self);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use jsonrpsee::core::async_trait;
    use schema::error::Error;

    use super::*;

    /// A stand-in that answers nothing, so the macro's own registration can be read.
    ///
    /// **Not an implementation.** P4b writes the daemon's; this exists because a Rust
    /// trait does not reify its methods, so the only authoritative statement of what the
    /// T5 surface registers is a real [`jsonrpsee::core::server::RpcModule`] built by the
    /// generated `into_rpc()` — the same mechanism docs/9 names for P4c's method-count
    /// walk, one sub-milestone early and without a daemon. Nothing here is ever called, so
    /// the refusal each method returns is arbitrary; what is not arbitrary is that the
    /// bodies typecheck, which is what proves [`WireError`] really satisfies jsonrpsee's
    /// server error bound rather than merely converting when we ask it to.
    struct AnswersNothing;

    fn nothing(operation: &str) -> WireError {
        WireError(Error::CameraUnknown {
            requested: operation.to_owned(),
        })
    }

    #[async_trait]
    impl WchRpcServer for AnswersNothing {
        async fn list(&self) -> Result<CameraList, WireError> {
            Err(nothing("list"))
        }
        async fn info(&self, _camera: CameraSelector) -> Result<CameraDetail, WireError> {
            Err(nothing("info"))
        }
        async fn controls(&self, _camera: CameraSelector) -> Result<ControlReport, WireError> {
            Err(nothing("controls"))
        }
        async fn discover_pairs(
            &self,
            _camera: CameraSelector,
        ) -> Result<DiscoveryReport, WireError> {
            Err(nothing("discover_pairs"))
        }
        async fn get(
            &self,
            _camera: CameraSelector,
            _control: ControlSlug,
        ) -> Result<ControlDesc, WireError> {
            Err(nothing("get"))
        }
        async fn set(
            &self,
            _camera: CameraSelector,
            _writes: Vec<ControlWrite>,
            _guarded: bool,
        ) -> Result<WriteReport, WireError> {
            Err(nothing("set"))
        }
        async fn snapshot(&self, _camera: CameraSelector) -> Result<Snapshot, WireError> {
            Err(nothing("snapshot"))
        }
        async fn restore(
            &self,
            _camera: CameraSelector,
            _snapshot: Snapshot,
        ) -> Result<RestoreReport, WireError> {
            Err(nothing("restore"))
        }
        async fn photo(
            &self,
            _camera: CameraSelector,
            _request: PhotoRequest,
        ) -> Result<PhotoResponse, WireError> {
            Err(nothing("photo"))
        }
        async fn record_start(
            &self,
            _camera: CameraSelector,
            _request: RecordRequest,
        ) -> Result<RecordStatus, WireError> {
            Err(nothing("record_start"))
        }
        async fn record_status(&self, _camera: CameraSelector) -> Result<RecordStatus, WireError> {
            Err(nothing("record_status"))
        }
        async fn record_stop(&self, _camera: CameraSelector) -> Result<RecordReport, WireError> {
            Err(nothing("record_stop"))
        }
        async fn profile_capture(
            &self,
            _camera: CameraSelector,
            _capturer: String,
            _discover_pairs: bool,
        ) -> Result<DeviceProfile, WireError> {
            Err(nothing("profile_capture"))
        }
        async fn terminate_holder(
            &self,
            _camera: CameraSelector,
            _pid: i32,
        ) -> Result<TerminationReport, WireError> {
            Err(nothing("terminate_holder"))
        }
        async fn calibrate_start(
            &self,
            _camera: CameraSelector,
            _task: String,
            _goal: String,
            _criteria: Vec<String>,
        ) -> Result<Session, WireError> {
            Err(nothing("calibrate_start"))
        }
        async fn calibrate_plan(
            &self,
            _camera: CameraSelector,
            _session: SessionRef,
            _controls: Vec<ControlSlug>,
            _order: bool,
        ) -> Result<Session, WireError> {
            Err(nothing("calibrate_plan"))
        }
        async fn calibrate_sweep(
            &self,
            _camera: CameraSelector,
            _session: SessionRef,
            _request: SweepRequest,
        ) -> Result<Session, WireError> {
            Err(nothing("calibrate_sweep"))
        }
        async fn calibrate_status(
            &self,
            _camera: CameraSelector,
            _session: SessionRef,
        ) -> Result<SessionStatus, WireError> {
            Err(nothing("calibrate_status"))
        }
        async fn calibrate_select(
            &self,
            _camera: CameraSelector,
            _session: SessionRef,
            _control: ControlSlug,
            _selection: Selection,
        ) -> Result<Session, WireError> {
            Err(nothing("calibrate_select"))
        }
        async fn calibrate_apply(
            &self,
            _camera: CameraSelector,
            _session: SessionRef,
            _partial: bool,
        ) -> Result<WriteReport, WireError> {
            Err(nothing("calibrate_apply"))
        }
        async fn calibrate_restore(
            &self,
            _camera: CameraSelector,
            _session: SessionRef,
        ) -> Result<RestoreReport, WireError> {
            Err(nothing("calibrate_restore"))
        }
        async fn calibrate_list(
            &self,
            _camera: Option<CameraSelector>,
        ) -> Result<SessionList, WireError> {
            Err(nothing("calibrate_list"))
        }
    }

    /// The same stand-in for the second generated trait, and it answers even less.
    ///
    /// A `PendingSubscriptionSink` dropped without `accept` or `reject` sends jsonrpsee's
    /// own default rejection, which is exactly "answers nothing" — and it is the one body
    /// that needs no `await`, which is what keeps [`answered`]'s no-runtime claim true for
    /// this file. What is not arbitrary, and is the whole reason these bodies exist, is
    /// that they typecheck: `into_rpc()` cannot be called on a trait nothing implements,
    /// and `into_rpc()` is the only authority on what the surface registers.
    #[async_trait]
    impl WchEventsServer for AnswersNothing {
        async fn subscribe_events(
            &self,
            _sink: jsonrpsee::PendingSubscriptionSink,
        ) -> jsonrpsee::core::SubscriptionResult {
            Ok(())
        }
        async fn subscribe_calibration(
            &self,
            _sink: jsonrpsee::PendingSubscriptionSink,
        ) -> jsonrpsee::core::SubscriptionResult {
            Ok(())
        }
    }

    /// The whole T5 surface as one registration, the way the daemon serves it.
    ///
    /// `Methods::merge` verifies every incoming name against the ones already registered
    /// (`jsonrpsee-core-0.26.0/src/server/rpc_module.rs`), so a spelling that collided
    /// *across* the two traits is refused here — which the per-trait `check_name` cannot
    /// see, because it only looks inside one trait. That is the one thing the split costs
    /// and the one place it is paid.
    fn whole_surface() -> jsonrpsee::core::server::Methods {
        let mut methods: jsonrpsee::core::server::Methods =
            WchRpcServer::into_rpc(AnswersNothing).into();
        methods
            .merge(WchEventsServer::into_rpc(AnswersNothing))
            .expect("the two halves of one declaration cannot collide");
        methods
    }

    /// Drive one of jsonrpsee's futures to its answer without a runtime.
    ///
    /// `RpcModule::raw_json_request` is `async` and this crate starts no runtime and spawns
    /// no task — N5's review-held half, and it stays true in tests too. It does not have to:
    /// a direct call over [`AnswersNothing`] awaits only the method body, which returns
    /// immediately, so a single poll with the no-op waker is the whole of it. A future that
    /// ever *did* pend here would be a future waiting on a reactor, which is exactly the
    /// thing that must not appear in this crate — so the panic below is an assertion, not a
    /// shortcut.
    fn answered(
        module: &jsonrpsee::core::server::RpcModule<AnswersNothing>,
        request: &str,
    ) -> String {
        use std::task::{Context, Poll, Waker};

        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut call = std::pin::pin!(module.raw_json_request(request, 1));
        match call.as_mut().poll(&mut context) {
            Poll::Ready(answer) => answer
                .expect("the request is well-formed JSON-RPC")
                .0
                .to_string(),
            Poll::Pending => panic!("{request} pended: something in this crate wants a runtime"),
        }
    }

    #[test]
    fn an_optional_parameter_may_be_left_out_and_a_required_one_may_not() {
        // The claim the emitted document makes — `"required": !ty.admits_absence()` — put
        // to the only authority there is: the generated server. The population is
        // `METHODS`, so a method added to the surface joins this walk by existing.
        //
        // One request shape does all of it. A by-name request with no parameters at all is
        // served exactly when every parameter of the method may be left out, so the
        // expectation is computed rather than tabulated, and both directions are present
        // in one run: `wch_list` (no parameters) and `wch_calibrate_list` (one optional)
        // are answered by the method body, and the other seventeen are refused before it.
        // Named rather than inferred, because `AnswersNothing` now implements two
        // generated server traits and `into_rpc` is a method on each: this walk is over
        // the call surface, and saying so is one token.
        let module = WchRpcServer::into_rpc(AnswersNothing);
        let mut omissible = 0;
        for method in METHODS {
            let request = format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"{}","params":{{}}}}"#,
                method.name
            );
            let answer = answered(&module, &request);
            let code = serde_json::from_str::<serde_json::Value>(&answer)
                .expect("a JSON-RPC response")["error"]["code"]
                .as_i64()
                .expect("this double refuses everything, so every answer is an error");

            if method.params.iter().all(|param| param.ty.admits_absence()) {
                omissible += 1;
                assert_eq!(
                    code,
                    i64::from(rpc_code(schema::error::ErrorKind::CameraUnknown)),
                    "{} declares every parameter omissible and the server disagreed: {answer}",
                    method.name
                );
            } else {
                assert_eq!(
                    code,
                    i64::from(jsonrpsee::types::error::INVALID_PARAMS_CODE),
                    "{} has a parameter the document calls required and the server did not: \
                     {answer}",
                    method.name
                );
            }
        }
        // Not vacuous in either direction: without this, a surface whose parameters were
        // all required — or all optional — would pass while proving only one half.
        assert!(omissible > 0 && omissible < METHODS.len(), "{omissible}");
    }

    #[test]
    fn the_surface_registers_the_twenty_two_methods_and_the_two_subscriptions_and_nothing_else() {
        // Read off a real `RpcModule`, not off a list in this file: the macro's own
        // registration is the authority on what the wire carries, and a hand list here
        // would agree with itself forever (rubric rule 6).
        let methods = whole_surface();
        let mut names: Vec<&str> = methods.method_names().collect();
        names.sort_unstable();

        // Every name is namespaced. D10 says `namespace = "wch"`, and jsonrpsee's default
        // separator is `_`, so an unprefixed name here would mean a method escaped onto a
        // socket the daemon shares with nothing.
        for name in &names {
            assert!(
                name.starts_with("wch_"),
                "{name} is not in the wch namespace"
            );
        }

        // Twenty-six since P6c, and the arithmetic is note **N29**'s: twenty-two calls, plus
        // *four* names for two subscriptions, because a `#[subscription]` registers its
        // `unsubscribe` sibling as its own callback
        // (`rpc_module.rs::verify_and_register_unsubscribe`) and `method_names()` is
        // `callbacks.keys()`. D10's own method count and the registered population are two
        // different numbers about two different things, which is why only one of them is
        // arithmetic here. It is derived from `SUBSCRIPTIONS` rather than written as `26`, so
        // a third subscription moves it by two without anybody editing this line — and pinned
        // below by name, which is what catches a rename.
        assert_eq!(
            names.len(),
            METHODS.len() + 2 * SUBSCRIPTIONS.len(),
            "{names:?}"
        );

        // The spellings themselves — a *pin*, not a population, in the same tradition as
        // `fixtures/d13-rpc-codes.tsv`: a wire name is a compatibility contract in the way
        // a code is, so changing one has to be a diff somebody wrote on purpose.
        // `wch_profile_capture` is D10's spelling, not T4's `capture_profile`, and
        // `wch_discover_pairs` is its own method rather than a flag on `wch_controls`
        // because the daemon routes it with the writes. The two `wch_unsubscribe_*`
        // spellings are jsonrpsee's derivation rather than ours (see
        // [`wire::Subscription::unsubscribe`]), and pinning them here is what makes a
        // change in that derivation a diff rather than a silently renamed wire name.
        assert_eq!(
            names,
            vec![
                "wch_calibrate_apply",
                "wch_calibrate_list",
                "wch_calibrate_plan",
                "wch_calibrate_restore",
                "wch_calibrate_select",
                "wch_calibrate_start",
                "wch_calibrate_status",
                "wch_calibrate_sweep",
                "wch_controls",
                "wch_discover_pairs",
                "wch_get",
                "wch_info",
                "wch_list",
                "wch_photo",
                "wch_profile_capture",
                "wch_record_start",
                "wch_record_status",
                "wch_record_stop",
                "wch_restore",
                "wch_set",
                "wch_snapshot",
                "wch_subscribe_calibration",
                "wch_subscribe_events",
                "wch_terminate_holder",
                "wch_unsubscribe_calibration",
                "wch_unsubscribe_events",
            ]
        );
    }

    #[test]
    fn the_inventories_and_the_registration_describe_the_same_surface() {
        // Two expansions of one declaration, compared. `METHODS` and `SUBSCRIPTIONS` are
        // `wire_surface!`'s half and `method_names()` is jsonrpsee's, and the macro
        // deliberately does not own the whole of a wire name: the namespace separator is
        // the proc macro's (`rpc_macro.rs` defaults it to `_`) and so is the *unsubscribe*
        // derivation (`build_unsubscribe_method` strips `subscribe`). So this is where
        // `"wch"` + `"_"` + `"list"` and `"wch"` + `"_un"` + `"subscribe_events"` are
        // *checked* against what the module registers rather than assumed — the facts
        // about this surface that really are derived twice.
        let methods = whole_surface();
        let mut registered: Vec<&str> = methods.method_names().collect();
        registered.sort_unstable();

        let mut inventoried: Vec<&str> = METHODS.iter().map(|method| method.name).collect();
        inventoried.extend(SUBSCRIPTIONS.iter().flat_map(wire::Subscription::names));
        inventoried.sort_unstable();

        assert_eq!(
            inventoried, registered,
            "the OpenRPC document would describe a different surface than the daemon serves"
        );

        // And the two halves are disjoint, which is what lets every consumer partition the
        // registered set by `SUBSCRIPTIONS` instead of excluding names by hand. Without
        // this the equality above would still hold if a subscription's name collided with
        // a method's — `Methods::merge` would have refused it, but only because the
        // *merge* is where that is caught, and this file is where it is stated.
        let subscribed: BTreeSet<&str> = SUBSCRIPTIONS
            .iter()
            .flat_map(wire::Subscription::names)
            .collect();
        let called: BTreeSet<&str> = METHODS.iter().map(|method| method.name).collect();
        assert!(subscribed.is_disjoint(&called), "{subscribed:?}");
        assert_eq!(subscribed.len(), 2 * SUBSCRIPTIONS.len(), "{subscribed:?}");
    }

    #[test]
    fn every_subscription_carries_the_prose_and_the_item_type_a_document_needs() {
        // `every_method_carries_…`'s sibling over the population that has no `params` and
        // no `result`: what a subscription's consumer needs instead is the *item* type,
        // which is what xtask writes into `x-subscriptions`. A subscription with no
        // summary is the same hole a method with none is.
        assert!(
            !SUBSCRIPTIONS.is_empty(),
            "the surface subscribes to nothing"
        );
        for subscription in SUBSCRIPTIONS {
            let name = subscription.name;
            assert!(!subscription.summary().is_empty(), "{name} has no summary");
            assert!(
                subscription.summary().ends_with('.'),
                "{name}'s summary is not a sentence: {}",
                subscription.summary()
            );
            assert!(
                subscription.description().contains("# Errors"),
                "{name} does not document what it refuses with"
            );
            // The item is what a subscriber decodes, so it has to name a type the document
            // can carry a schema for — a `TypeRef` that named nothing would publish an
            // `x-subscriptions` row a consumer cannot use.
            assert!(
                !subscription.item.name().is_empty(),
                "{name} carries nothing"
            );
            // jsonrpsee's own precondition, which our `concat!` derivation shares: its
            // expansion panics on a subscribe name that does not start with `subscribe`.
            // Asserted rather than trusted, because the two derivations agreeing is only
            // true *because* of it.
            assert!(
                subscription.name.starts_with("wch_subscribe_"),
                "{name} would make jsonrpsee's unsubscribe derivation and ours disagree"
            );
        }
    }

    #[test]
    fn every_method_carries_the_prose_and_the_parameter_names_a_document_needs() {
        // xtask writes the OpenRPC document straight out of these, so a method with no
        // summary is a hole a consumer reads — and the emitter cannot invent one, because
        // the sentence it would invent is the sentence the author already wrote.
        for method in METHODS {
            let name = method.name;
            assert!(!method.summary().is_empty(), "{name} has no summary");
            // A summary is a sentence. House style opens every doc comment with one, and a
            // fragment here means the first line was a heading or a continuation — which
            // would put the wrong text in the document without failing anything else.
            assert!(
                method.summary().ends_with('.'),
                "{name}'s summary is not a sentence: {}",
                method.summary()
            );
            // Rustdoc's `# Errors` section is what tells a caller which D13 variants this
            // operation refuses with, and the document's error registry is only useful
            // beside it. A method that does not say is a method whose refusals are a
            // surprise.
            assert!(
                method.description().contains("# Errors"),
                "{name} does not document what it refuses with"
            );

            // Parameter *names* need no assertion here and deliberately have none: they
            // are `stringify!` of a Rust identifier, so an empty one is unrepresentable,
            // and a repeated one does not link — jsonrpsee's generated `ParamsObject`
            // declares a field per parameter, so two of a name is `E0124: field is
            // already declared` on top of rustc's own `E0415: identifier is bound more
            // than once in this parameter list` (measured by renaming `writes` to
            // `camera` in the `wire_surface!` invocation). An assertion whose false branch
            // cannot be reached is the smell rubric Part C names, and it would have
            // claimed credit for what the compiler does.
        }
    }
}
