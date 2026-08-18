//! The error registry (design D13).
//!
//! Closed and typed: every variant carries what the caller needs to act on it. The
//! doctrine it enforces is E3 — *availability is not capability*. `EBUSY`, `ENODEV`,
//! `EPERM` and a settle timeout all read like "the camera can't do that" to a lazy
//! caller, so they are kept apart here and no code path converts one into another.
//!
//! What is **not** in here: a clamped write \[PF:6\]. The driver accepted it and reported
//! success, so it rides the write result as a [`crate::control::WriteWarning`]. A warning
//! with an error code is a success nobody can distinguish from a failure.

use camino::Utf8PathBuf;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::camera::{CameraId, FrameSize, PixelFormat};
use crate::control::ControlSlug;
use crate::video::VideoFormat;
use crate::vocabulary::closed_vocabulary;

/// A process holding a device open, as diagnosed from `/proc/*/fd` — the `fuser`
/// replacement. Killing one is a separate explicit command, never a side effect
/// (design §5, D10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Holder {
    /// The holding process.
    pub pid: i32,
    /// Its `comm`, when `/proc` let us read it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comm: Option<String>,
}

/// The size half of a [`Error::FormatUnsupported`]: the frame size a caller named, and
/// every size the device can actually deliver.
///
/// **A refusal has to say which knob to turn.** `FormatUnsupported` carried one slot for
/// "what was asked for" and it held a [`PixelFormat`], so a request refused for its *size*
/// had to report `requested: None` and a list of formats — which rendered as *"format
/// (unspecified) is unavailable; MJPG, YUYV would be accepted"* and told an unattended
/// caller that the half of its request that was answerable was the problem. Retrying with
/// one of the formats named produces the identical refusal, so the guide's *"fix the
/// request"* disposition becomes a loop: note **N129**'s misdirection class, at the same
/// variant, one phase later (note **N138**).
///
/// So the size that was refused and the sizes that would be accepted travel together, and
/// the presence of this payload is what says *which* half of the request could not be met.
/// See [`Error::size_unsupported`] for the two constructors that keep the two causes
/// mutually exclusive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SizeRefusal {
    /// The width the caller named.
    pub requested_width: u32,
    /// The height the caller named.
    pub requested_height: u32,
    /// Every size the device can deliver, across **every** format it enumerates — because
    /// the resolver's candidate set is device-wide, so "no mode fits" is a fact about the
    /// device rather than about whichever format a ranking happened to pick.
    ///
    /// A [`FrameSize`] rather than a `(width, height)` pair, so a **stepwise** entry
    /// arrives as the range it is: collapsing it to its maximum corner here would be the
    /// same falsehood [`FrameSize::largest_within`] exists to avoid, committed in the
    /// message that is supposed to repair the request.
    ///
    /// Entries whose shape this build cannot read (D2's [`FrameSize::Unknown`]) are left
    /// out: this list is what *would be accepted*, and a size we cannot interpret is not
    /// something a caller can ask for. The device's own enumeration still carries them —
    /// `info` is where a caller sees that the driver said something we did not understand.
    pub available: Vec<FrameSize>,
}

/// The container half of an `Error::FormatUnsupported`: the file a caller named, the format
/// the camera actually delivered, and the containers that would have taken it.
///
/// **The lever this refusal turns is the file extension, and until 2026-08-16 the payload
/// named a different one.** D7 puts `FormatUnsupported { available }` on the record path in so
/// many words, so a `record -o take.y4m` against a camera that negotiated MJPG answered
/// `requested: "MJPG"` with `available: ["YUYV", "NV12", "GREY"]` — the *container's* list.
/// Both fields mislead an unattended reader, and each in its own way:
///
/// - `requested` held the **negotiated** format. The caller never typed MJPG; a ranking chose
///   it. A reader repairing `requested` repairs a request it did not make.
/// - `available` held formats that need not exist **on that camera at all**. Measured through
///   the shipped binary over `corpus/profiles/chicony-rgb.json`, which enumerates MJPG and
///   YUYV: two of the three offered as remedies are formats that sensor has never had. That is
///   note **N129**'s misdirection — a refusal telling a caller to retry with something the
///   device does not have — surviving in the payload after the *message* was repaired for it
///   (note **N211**).
///
/// So the container case carries its own payload, `SizeRefusal`'s shape one cause along, and
/// what it names is always true and always actionable: this build's containers are a closed set
/// of two, so "which file extension would have taken these frames" is a question the schema can
/// answer without knowing anything about the camera. Build one through
/// `Error::container_unsupported`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContainerRefusal {
    /// The container the sink path's extension named.
    ///
    /// `None` when the path carried no extension, which means the caller left the container to
    /// the negotiated format (`crate::video::RecordRequest::container`) and this build writes
    /// no container that carries it. A different extension is then not the remedy, because the
    /// caller never named one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<VideoFormat>,
    /// The format the device negotiated — the thing that could not be written.
    ///
    /// **Not what the caller asked for**, and that distinction is the whole reason this field
    /// exists rather than reusing `requested`: a recording's format is chosen by D5's ranking
    /// unless the caller named one, so the name in a container refusal is usually the device's
    /// answer and not the caller's question.
    pub negotiated: PixelFormat,
    /// Every container this build writes that *would* carry `negotiated`.
    ///
    /// The remedy, and it is a fact about this build rather than about the camera — which is
    /// what makes it safe to act on unattended. Empty when no container carries the format at
    /// all, and that is the honest answer to "what should I do": nothing, this build cannot
    /// record these frames, and a second attempt with another extension meets the same wall.
    pub carried_by: Vec<VideoFormat>,
}

closed_vocabulary! {
    /// Which of D9's two locking protocols the holder of the state directory follows.
    ///
    /// Both take the same advisory lock the same way; what differs is how long it is held,
    /// and that difference is the whole of what a refused caller needs. A lock held for a
    /// process's lifetime will not be free in a moment, so retrying is pointless and the
    /// answer is another program; a lock held for one operation will be free shortly, so
    /// retrying is the whole answer.
    ///
    /// **Here rather than in `webcam-handler-engine::store`, where the locking lives**,
    /// because [`Error::StoreLocked`] carries it: a refusal that cannot say which protocol
    /// holds the lock cannot tell the caller which of those two answers applies, and
    /// `webcam-handler-schema` cannot depend on the engine. The engine re-exports this
    /// type, so the locking code still names one definition (design §2.10) — see note N40.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum LockProtocol {
        /// The daemon's: taken once at startup and held until the process exits, because
        /// the daemon owns the state directory for as long as it is running.
        HeldForLifetime,
        /// A daemonless `webcam-handler-cli`'s: taken for one mutating operation and released
        /// at its end, because a CLI that held it between invocations would lock out the
        /// daemon it does not know about.
        PerOperation,
    }
}

impl LockProtocol {
    /// The protocol's name, for the lock record and for failure messages.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            LockProtocol::HeldForLifetime => "held_for_lifetime",
            LockProtocol::PerOperation => "per_operation",
        }
    }

    /// What to tell whoever this protocol just refused — **D9's sentence, and its home**.
    ///
    /// Design D9 writes the first arm itself: a `webcam-handler-cli` finding the lock held
    /// "reports *daemon owns the state (and likely the camera) — use webcam-handler-client*
    /// rather than corrupting or blocking (D13)". It lives here, on the fact it turns on, so
    /// that it exists once: the same words reach a human through [`Error`]'s `Display` in
    /// `webcam-handler-cli`, through the wire message the daemon sends, and through
    /// `webcam-handler-client` rendering a received D13 document, none of which re-word it.
    ///
    /// The second arm is the same law read the other way, and it is why this is a `match` and
    /// not an `if`: telling somebody to start a different program because another
    /// `webcam-handler-cli` is a few milliseconds into `calibrate select` would be advice that
    /// makes their situation worse. A third protocol cannot be added without answering this
    /// question.
    #[must_use]
    pub const fn advice(self) -> &'static str {
        match self {
            LockProtocol::HeldForLifetime => {
                "daemon owns the state (and likely the camera) — use webcam-handler-client"
            }
            LockProtocol::PerOperation => "it is held for one operation and will be free shortly",
        }
    }
}

impl std::fmt::Display for LockProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

closed_vocabulary! {
    /// What the process that refused is itself doing with the camera's node.
    ///
    /// [`Error::Busy`]'s `holders` answers *who has it* by walking `/proc`, and there is one
    /// holder it deliberately never names: **this process**. A daemon that refuses its own
    /// caller knows precisely what it is doing and will not hand over its own pid, because a
    /// pid in a refusal is an invitation to signal it (`daemon::record`'s header). What was
    /// missing is the other half of that decision — *what for* — without which the refusal
    /// read as "somebody unidentified has it" and sent an unattended caller looking for a
    /// process to kill (docs/11 **M19**, note **N217**).
    ///
    /// **Every in-process producer in the workspace has an alternative here**, and that is
    /// the property rather than a tidiness. The moment one of them goes through
    /// [`Error::busy`] instead, *"held by an unidentified process"* is back — about the
    /// program the caller is talking to — and the guide's row telling a reader that an absent
    /// `this_process` means another program is false with it. Three producers shipped that
    /// way inside this repair and note **N221** is the entry for them.
    ///
    /// Every alternative says what ends it, because that is what makes `Busy`'s *retry*
    /// disposition true: the caller is told what to wait for, not merely to wait. None of them
    /// names a verb to poll with, and that is measured rather than terse — the surface this
    /// refusal reaches most often has none to offer (note **N220**).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Occupation {
        /// A recording this process is running for a caller.
        ///
        /// Bounded by the take's own duration (`limits::MAX_RECORDING_MS` at the outside), so
        /// retrying is the action that succeeds.
        Recording,
        /// A `record_start` this process is still negotiating.
        ///
        /// The slot is claimed and the stream is not up yet. It resolves either way in the
        /// time one `VIDIOC_STREAMON` takes, which is why it is told apart from
        /// [`Occupation::Recording`]: what a caller waits for is different, and so is how
        /// long.
        StartingRecording,
        /// This process's own commands for the camera, queued to the depth it allows.
        ///
        /// `engine::actor` gives each camera one thread and a bounded inbox
        /// (`limits::CAMERA_COMMAND_QUEUE_DEPTH`), so a full one is this process's work in the
        /// way and never a device that is unavailable (E3). It is the alternative a caller can
        /// do something about beyond waiting: D12's `wait` spends
        /// `limits::CAMERA_ENQUEUE_WAIT_MS` on room in exactly this queue, so it helps here
        /// and with no other alternative.
        RunningCommands,
        /// A stream this process already has open on the node.
        ///
        /// One streamer per node is the kernel's rule and this build's, so a second
        /// `start_stream` on a handle that is already streaming is refused before `S_FMT`
        /// would say `EBUSY` a moment later and tear down buffers the caller is still
        /// dequeuing from (note **N191**). It ends when the stream that is up ends.
        Streaming,
        /// A preview this process is already serving to every viewer it will.
        ///
        /// The node is streaming and what refuses is not the node: it is a count,
        /// `limits::PREVIEW_MAX_VIEWERS_PER_CAMERA`. Its own alternative because it is the one
        /// a second browser tab meets, and because what ends it is a viewer leaving rather
        /// than a stream stopping.
        StreamingPreview,
    }
}

impl Occupation {
    /// What a caller is waiting for, and what ends it — the actionable half.
    ///
    /// A sentence rather than a word because `Busy`'s whole disposition is *retry*, and a
    /// retry with no idea what it is waiting for is a loop. It lives here, on the value,
    /// for [`LockProtocol::advice`]'s reason: the same words reach a person through
    /// [`Error`]'s `Display`, an agent through the wire message, and
    /// `webcam-handler-client` rendering a received document, and none of them re-word it.
    #[must_use]
    pub const fn advice(self) -> &'static str {
        match self {
            Occupation::Recording => {
                "this process is recording it, and the take ends on its own duration — ask \
                 again once it is over"
            }
            Occupation::StartingRecording => {
                "this process is starting a recording on it — ask again in a moment"
            }
            Occupation::RunningCommands => {
                "this process is running commands on it and its queue is full — ask again in \
                 a moment, or ask to be queued rather than refused"
            }
            Occupation::Streaming => {
                "this process is already streaming it, and a node takes one stream — ask \
                 again once that one ends"
            }
            Occupation::StreamingPreview => {
                "this process is serving its preview to as many viewers as it allows — ask \
                 again once one of them leaves"
            }
        }
    }
}

closed_vocabulary! {
    /// The discriminant of [`Error`], as a value.
    ///
    /// `ALL` is generated from this definition, so a variant cannot be added without
    /// joining every completeness check that walks it (rubric rule 6: no hand lists).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum ErrorKind {
        /// See [`Error::DeviceGone`].
        DeviceGone,
        /// See [`Error::Busy`].
        Busy,
        /// See [`Error::PermissionDenied`].
        PermissionDenied,
        /// See [`Error::CameraUnknown`].
        CameraUnknown,
        /// See [`Error::CameraAmbiguous`].
        CameraAmbiguous,
        /// See [`Error::ControlUnknown`].
        ControlUnknown,
        /// See [`Error::ControlReadOnly`].
        ControlReadOnly,
        /// See [`Error::ControlInactive`].
        ControlInactive,
        /// See [`Error::FormatUnsupported`].
        FormatUnsupported,
        /// See [`Error::SettleTimeout`].
        SettleTimeout,
        /// See [`Error::FingerprintMismatch`].
        FingerprintMismatch,
        /// See [`Error::SessionConflict`].
        SessionConflict,
        /// See [`Error::IllegalTransition`].
        IllegalTransition,
        /// See [`Error::SchemaVersionForeign`].
        SchemaVersionForeign,
        /// See [`Error::StoreLocked`].
        StoreLocked,
        /// See [`Error::HolderGone`].
        HolderGone,
        /// See [`Error::DeviceIo`].
        DeviceIo,
        /// See [`Error::StorageIo`].
        StorageIo,
    }
}

/// Everything that can go wrong, as a value the caller can branch on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, thiserror::Error)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Error {
    /// The device disappeared — unplugged, or the driver unbound.
    #[error("camera {path} is gone (unplugged, or its driver unbound)")]
    DeviceGone {
        /// The node that vanished.
        #[schemars(with = "String")]
        path: Utf8PathBuf,
    },

    /// Another process holds the device. Distinct from "the camera cannot do this" (E3).
    ///
    /// Build one through [`Error::busy`] or [`Error::busy_here`]: which of the two says
    /// whether the holder is somebody else or this very process, and that is the difference
    /// between "go and find out who has it" and "wait for what I am doing".
    #[error("{path} is busy: {}", format_busy(.holders, .this_process))]
    Busy {
        /// The node in use.
        #[schemars(with = "String")]
        path: Utf8PathBuf,
        /// Who has it, as far as this user could see.
        ///
        /// Empty means *unidentified*, not *nobody*: a holder belonging to another user
        /// is invisible in `/proc` without privilege, and a process that exited between
        /// the `EBUSY` and the walk leaves nothing to find.
        holders: Vec<Holder>,
        /// What **this** process is doing with the node, when this process is the holder.
        ///
        /// A daemon refusing its own caller walked no `/proc` and needs to walk none: it
        /// knows. Until 2026-08-17 it said so by leaving `holders` empty — deliberately,
        /// and argued in `daemon::record`'s header, because *"naming it would invite a
        /// client to kill the daemon it is talking to"* — and the rendering then told the
        /// caller the node was *"held by an unidentified process"*, which is the one thing
        /// it was not. Measured through the shipped binaries: a `photo` during a take
        /// answered `{"kind":"busy","holders":[]}` and printed that sentence, so an agent's
        /// next move was to hunt for a process to kill (docs/11 **M19**, note **N217**).
        ///
        /// This field is how the daemon says "it is me, and here is what for" **without**
        /// handing over a pid — the withholding stays, and what it withholds stops reading
        /// as ignorance. `None` is every other producer, and means exactly what the empty
        /// `holders` list has always meant.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        this_process: Option<Occupation>,
    },

    /// The device node exists but we may not open it. The hint lives here, once.
    #[error("cannot open {path}: {hint}")]
    PermissionDenied {
        /// The node.
        #[schemars(with = "String")]
        path: Utf8PathBuf,
        /// What to do about it.
        hint: String,
    },

    /// No camera matched the selector — an id, a prefix, or any of D14's other spellings.
    #[error("no camera matches {requested:?}{}", crate::selector::scheme_hint(.requested))]
    CameraUnknown {
        /// What the caller asked for.
        requested: String,
    },

    /// Several cameras matched the prefix.
    #[error("{requested:?} matches {} cameras: {}", .candidates.len(), format_ids(.candidates))]
    CameraAmbiguous {
        /// What the caller asked for.
        requested: String,
        /// Which cameras it could mean.
        candidates: Vec<CameraId>,
    },

    /// No control by that slug or id.
    #[error("no control named {requested:?}{}", format_suggestions(.did_you_mean))]
    ControlUnknown {
        /// What the caller asked for.
        requested: String,
        /// The closest slugs this camera does have.
        did_you_mean: Vec<ControlSlug>,
    },

    /// The control exists and cannot be written \[PF:12\].
    #[error("control {control} is read-only on this device")]
    ControlReadOnly {
        /// Which control.
        control: ControlSlug,
    },

    /// An automation partner currently owns the control \[PF:3\]. Actionable: the
    /// variant names the control to disable.
    #[error("control {control} is inactive{}", format_automation(.automation))]
    ControlInactive {
        /// Which control.
        control: ControlSlug,
        /// The automation control to disable first, when one was discovered.
        automation: Option<ControlSlug>,
    },

    /// The requested pixel format is not one that would be accepted here.
    ///
    /// **Whose list `available` is depends on who refused, and the message must not
    /// guess.** It said "this camera offers …" until 2026-08-15, which is true of D6's
    /// source-format refusal and false of the other two callers: `engine::preview`
    /// answers with the format the negotiation actually produced, and D7's record path
    /// answers with what the *container* would have carried. Measured at the Chicony IR
    /// sensor, which offers GREY and nothing else — a `record -o x.avi` refusal read
    /// "format GREY is unavailable; this camera offers MJPG, JPEG", naming two formats
    /// that camera has never had. An unattended caller obeying it would retry
    /// `--pixel-format MJPG` against a device with no MJPG, which is the defect the D13
    /// registry exists to prevent rather than commit (note **N129**).
    ///
    /// The variant itself is right where it is: design D7 names
    /// `FormatUnsupported { available }` for the container case in so many words, so this
    /// is a rendering repair and not a re-litigation of which refusal applies. The
    /// sentence now says only what is true of every caller — these are the formats that
    /// *would* be accepted — and which lever to pull is the guide's `Do` column, where a
    /// container mismatch is answered by changing the container rather than the format.
    ///
    /// **And the same lesson had to be learnt again about the *size*** (note **N138**).
    /// A request refused because no mode could deliver the size it named had nowhere to
    /// say so: it reported `requested: None` and the sentence came out *"format
    /// (unspecified) is unavailable; MJPG, YUYV would be accepted"* — about formats, naming
    /// the caller's own format as acceptable, never mentioning size. [`SizeRefusal`] is the
    /// slot N134 said this variant would need, and `size` is now what distinguishes the two
    /// causes; build one through [`Error::format_unsupported`] or
    /// [`Error::size_unsupported`] rather than by hand, so they stay exclusive.
    ///
    /// **And a third time about the *container*** (note **N211**), which is where N129's
    /// repair had stopped: the message stopped saying "this camera offers" but `available`
    /// went on holding the container's own format list, so `record -o take.y4m` over
    /// `corpus/profiles/chicony-rgb.json` — a camera enumerating MJPG and YUYV — answered
    /// `requested: "MJPG"` with `available: ["YUYV", "NV12", "GREY"]`. Two of the three
    /// remedies are formats that sensor has never had, and the one the caller is told it
    /// asked for was chosen by D5's ranking rather than typed. `ContainerRefusal` is that
    /// cause's own payload and `container` is what distinguishes it; the door in is
    /// `Error::container_unsupported`.
    ///
    /// So the variant has three causes and one lever each — a format, a size, a file
    /// extension — and which one applies is a field rather than a sentence to parse.
    #[error("{}", format_capture_refusal(.requested, .available, .size, .container))]
    FormatUnsupported {
        /// The format that was asked for, when the caller named one **and the format is
        /// what could not be met**. `None` when the caller named none, and never a
        /// stand-in for "the size was the problem" — `size` says that outright.
        requested: Option<PixelFormat>,
        /// What would be accepted — the camera's formats, the negotiated one, or the
        /// container's, according to which of the three callers refused.
        available: Vec<PixelFormat>,
        /// The size that could not be delivered, present **exactly when** the size rather
        /// than the format is what this refusal is about.
        ///
        /// Absent for every other producer of this variant — the D6 source-format refusal,
        /// `engine::preview`'s unrenderable negotiation, and D7's container refusal all
        /// refuse a *format* or a file — which is what makes `size.is_some()` a reliable
        /// discriminator rather than a hint. `engine::preview::negotiate` reads it as one:
        /// its cap can be dropped and retried, and a format it named and the device lacks
        /// cannot.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size: Option<SizeRefusal>,
        /// The container that could not carry the stream, present **exactly when** the file
        /// rather than the camera is what this refusal is about.
        ///
        /// Exclusive with `size` for the same reason `size` is exclusive with `requested`:
        /// a refusal naming two levers is a refusal an unattended caller pulls at random.
        /// The three doors — `Error::format_unsupported`, `Error::size_unsupported`,
        /// `Error::container_unsupported` — are what keep them apart, and a value built
        /// by hand around them is a defect rather than a shortcut.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        container: Option<ContainerRefusal>,
    },

    /// Frames kept arriving but the settle policy never converged \[PF:11\].
    #[error("frames did not settle within {waited_ms} ms ({frames_seen} frames seen)")]
    SettleTimeout {
        /// How long we waited.
        waited_ms: u64,
        /// How many frames arrived in that time — zero means a different problem.
        frames_seen: u32,
    },

    /// The camera is not the one this session was recorded against.
    #[error("camera fingerprint differs from the session's in: {}", .fields.join(", "))]
    FingerprintMismatch {
        /// Which fields disagree.
        fields: Vec<String>,
    },

    /// Another session already owns this (camera, task) pair.
    #[error("session conflict: {detail}")]
    SessionConflict {
        /// What conflicts.
        detail: String,
    },

    /// The calibration state machine refused a transition (design D8).
    ///
    /// **The condition first and the instruction last**, and the order is the whole of what
    /// this rendering decides. The template was `"cannot {op} from state {from}"` when D8 was
    /// the only producer and `op` was a verb phrase — `"select"`, `"sweep privacy"` — where a
    /// trailing `from state untouched` reads. The variant now has eleven producers across five
    /// crates and most of them put a **multi-clause instruction** in `op`, because that is the
    /// field a caller acts on; appending anything to such a sentence garbles it. Measured
    /// through the shipped binaries before this changed:
    ///
    /// ```text
    /// cannot write a photo to …/x.tiff; this build writes .jpg, .png, .ppm from state unwritable_extension(tiff)
    /// ```
    ///
    /// So `from` becomes the label it has always been — a machine-shaped condition,
    /// `unwritable_extension(tiff)`, `no_session(nightly)`, `not_one_process(0)` — and `op`
    /// ends the message, where a sentence of any length can end.
    /// `an_illegal_transitions_instruction_is_the_last_thing_it_says` is what keeps a suffix
    /// from growing back (docs/11 **L29**, note **N212**).
    #[error("{from}: cannot {op}")]
    IllegalTransition {
        /// The state we were in.
        from: String,
        /// The operation attempted.
        ///
        /// A whole instruction rather than a verb, for most of the producers: the rendering
        /// above puts it last precisely so it can be one.
        op: String,
    },

    /// A persisted document was written by a version we do not understand (D9).
    #[error("document schema version {found} is not supported (this build reads {supported})")]
    SchemaVersionForeign {
        /// The version on disk.
        found: u32,
        /// The version this build reads.
        supported: u32,
    },

    /// The state directory's advisory lock is held elsewhere (D9).
    ///
    /// Both fields come from the same lock record and are `None` together: an
    /// unidentified holder is reported as unidentified, never as a plausible pid — and
    /// never as a protocol somebody would then act on.
    #[error(
        "the state directory is locked{}{}",
        format_holder(.holder),
        format_advice(.protocol)
    )]
    StoreLocked {
        /// Who holds it, when we could tell.
        holder: Option<Holder>,
        /// Which of D9's protocols the holder follows, when we could tell.
        ///
        /// The field exists so the refusal can say whether waiting is worth anything:
        /// [`LockProtocol::advice`] is the one place that turns it into words.
        protocol: Option<LockProtocol>,
    },

    /// `terminate_holder` was asked to kill a pid that no longer holds the device.
    #[error("pid {pid} no longer holds this device; refusing to signal it")]
    HolderGone {
        /// The pid named in the request.
        pid: i32,
    },

    /// The kernel refused an operation for a reason with no more specific home.
    ///
    /// Deliberately typed rather than stringly: `operation` and `errno` are what a
    /// caller (or a bug report) actually needs. See note N4.
    #[error("{operation} failed{}: {message}", format_errno(.errno))]
    DeviceIo {
        /// The ioctl or syscall attempted.
        operation: String,
        /// `errno`, when there was one.
        errno: Option<i32>,
        /// The system's description.
        message: String,
    },

    /// A filesystem operation on the state directory failed (disk full, and friends).
    #[error("{path}: {message}")]
    StorageIo {
        /// The path involved.
        #[schemars(with = "String")]
        path: Utf8PathBuf,
        /// `errno`, when there was one.
        errno: Option<i32>,
        /// The system's description.
        message: String,
    },
}

impl Error {
    /// The node is held by **somebody else**: whoever the walk could see, which may be
    /// nobody it could see.
    ///
    /// One of two doors into [`Error::Busy`], and they exist for the reason
    /// [`Error::format_unsupported`]'s three do: the two cases want different actions from
    /// an unattended caller, so a value that could claim both — a `/proc` walk *and* this
    /// process's own work — would be a refusal naming two remedies. This one is every
    /// producer that met an `EBUSY` **from the kernel**: the holder is another program, and
    /// `holders` says which when this user could tell.
    ///
    /// A refusal this process makes about its own work is [`Error::busy_here`] and never this,
    /// however empty the holder list would have been — a full command queue, a second stream
    /// on one node, a preview at its viewer cap. Each of those went through this door for one
    /// repair and rendered as *"held by an unidentified process"* (note **N221**).
    #[must_use]
    pub fn busy(path: Utf8PathBuf, holders: Vec<Holder>) -> Error {
        Error::Busy {
            path,
            holders,
            this_process: None,
        }
    }

    /// The node is held by **this process**, doing `occupation`.
    ///
    /// The daemon's own refusals: a photograph asked for during a take, a second
    /// `record_start`, a `record_stop` for a reservation still negotiating. No walk runs —
    /// there is nothing to find out — and no pid is handed over, because
    /// `daemon::record`'s header is right that a pid in a refusal is an invitation to
    /// signal it. What crosses instead is what the caller is waiting for (note **N217**).
    #[must_use]
    pub fn busy_here(path: Utf8PathBuf, occupation: Occupation) -> Error {
        Error::Busy {
            path,
            holders: Vec::new(),
            this_process: Some(occupation),
        }
    }

    /// A refusal about a **format**: what was named, when anything was, and what would be
    /// accepted instead.
    ///
    /// One of the three doors into [`Error::FormatUnsupported`], and the reason they exist is
    /// that the variant's causes must not be expressible at once. A payload carrying both a
    /// `requested` format and a [`SizeRefusal`] would be a refusal that names two
    /// levers, and an unattended caller pulling the wrong one is the failure this whole
    /// registry is built to avoid (notes **N138**, **N211**).
    #[must_use]
    pub fn format_unsupported(
        requested: Option<PixelFormat>,
        available: Vec<PixelFormat>,
    ) -> Error {
        Error::FormatUnsupported {
            requested,
            available,
            size: None,
            container: None,
        }
    }

    /// A refusal about a **size**: the frame size no mode could deliver, the sizes that
    /// would be accepted, and the formats the device has.
    ///
    /// `requested` is `None` by construction — the caller's *format*, if it named one, is
    /// on the device and is not what failed, so naming it there would send the caller to
    /// change the half of its request that was answerable. `available` is still carried
    /// because the device's formats are true and useful context; what changed is that the
    /// message no longer offers them as the remedy. See [`SizeRefusal`].
    #[must_use]
    pub fn size_unsupported(
        requested_width: u32,
        requested_height: u32,
        available_sizes: Vec<FrameSize>,
        available: Vec<PixelFormat>,
    ) -> Error {
        Error::FormatUnsupported {
            requested: None,
            available,
            size: Some(SizeRefusal {
                requested_width,
                requested_height,
                available: available_sizes,
            }),
            container: None,
        }
    }

    /// A refusal about a **container**: the file a caller named, the format the device
    /// negotiated, and the containers that would have taken it.
    ///
    /// `requested` is `None` and `available` is empty by construction, and both are the
    /// repair note **N211** records. The negotiated format is not the caller's request — D5's
    /// ranking picks it unless a `--pixel-format` was typed — so reporting it as `requested`
    /// tells a caller to repair a request it never made. And `available` held whatever the
    /// *container* carries, which is a claim about this build wearing the label of a claim
    /// about the camera: measured over `corpus/profiles/chicony-rgb.json`, a `.y4m` refusal
    /// offered NV12 and GREY to a sensor that has neither, which is N129's misdirection
    /// surviving in the payload after the message was repaired for it.
    ///
    /// `carried_by` is derived here rather than passed, because the question — *which
    /// extension would have taken these frames* — is answered by [`VideoFormat::carries`]
    /// alone, and a caller that computed it would be a second copy of D7's pairing (design
    /// §2.10). Deriving it also means the empty answer is honest by construction: a format
    /// no container in this build writes yields no remedy rather than a plausible one.
    #[must_use]
    pub fn container_unsupported(container: Option<VideoFormat>, negotiated: PixelFormat) -> Error {
        Error::FormatUnsupported {
            requested: None,
            available: Vec::new(),
            size: None,
            container: Some(ContainerRefusal {
                container,
                negotiated,
                carried_by: VideoFormat::ALL
                    .iter()
                    .copied()
                    .filter(|candidate| candidate.carries_format(negotiated))
                    .collect(),
            }),
        }
    }

    /// This error's discriminant.
    ///
    /// The match is exhaustive, so a new variant cannot be added without giving it a
    /// kind — which is what every completeness walk downstream is anchored on.
    #[must_use]
    pub fn kind(&self) -> ErrorKind {
        match self {
            Error::DeviceGone { .. } => ErrorKind::DeviceGone,
            Error::Busy { .. } => ErrorKind::Busy,
            Error::PermissionDenied { .. } => ErrorKind::PermissionDenied,
            Error::CameraUnknown { .. } => ErrorKind::CameraUnknown,
            Error::CameraAmbiguous { .. } => ErrorKind::CameraAmbiguous,
            Error::ControlUnknown { .. } => ErrorKind::ControlUnknown,
            Error::ControlReadOnly { .. } => ErrorKind::ControlReadOnly,
            Error::ControlInactive { .. } => ErrorKind::ControlInactive,
            Error::FormatUnsupported { .. } => ErrorKind::FormatUnsupported,
            Error::SettleTimeout { .. } => ErrorKind::SettleTimeout,
            Error::FingerprintMismatch { .. } => ErrorKind::FingerprintMismatch,
            Error::SessionConflict { .. } => ErrorKind::SessionConflict,
            Error::IllegalTransition { .. } => ErrorKind::IllegalTransition,
            Error::SchemaVersionForeign { .. } => ErrorKind::SchemaVersionForeign,
            Error::StoreLocked { .. } => ErrorKind::StoreLocked,
            Error::HolderGone { .. } => ErrorKind::HolderGone,
            Error::DeviceIo { .. } => ErrorKind::DeviceIo,
            Error::StorageIo { .. } => ErrorKind::StorageIo,
        }
    }

    /// A representative value of each kind.
    ///
    /// Not a test fixture living in a test: the RPC code mapping, the CLI renderer, and
    /// the schema emitter all need a walkable population, and a hand list in each of
    /// them is three lists that drift. This match is exhaustive over [`ErrorKind`], so
    /// the population is the vocabulary.
    #[must_use]
    // Six `expect`s below, all of the same shape and all of it in view: every argument is a
    // string literal in this match, so each one states a precondition rather than risking a
    // device — which is the reason docs/9's lint set carves `cfg(test)` out, applied to a
    // function that is a table of literals wearing product visibility. One expectation over the
    // whole function rather than six, because six copies of one sentence is what
    // `allow_attributes_without_reason` exists to stop.
    //
    // Bare, and not wrapped in `cfg_attr(not(test), …)` to match the crate root's `deny`, for
    // the reason `pairing.rs` states beside its own: `#[expect]` sets the level at this node
    // either way, so the wrapper changes nothing except who can notice when the suppression goes
    // stale (note **N167**).
    #[expect(
        clippy::expect_used,
        reason = "every id and slug in this table is a literal spelled in this match"
    )]
    pub fn sample(kind: ErrorKind) -> Error {
        match kind {
            ErrorKind::DeviceGone => Error::DeviceGone {
                path: "/dev/video0".into(),
            },
            ErrorKind::Busy => Error::busy(
                "/dev/video0".into(),
                vec![Holder {
                    pid: 4242,
                    comm: Some("cheese".to_owned()),
                }],
            ),
            ErrorKind::PermissionDenied => Error::PermissionDenied {
                path: "/dev/video0".into(),
                hint: "add yourself to the `video` group, then log out and back in".to_owned(),
            },
            ErrorKind::CameraUnknown => Error::CameraUnknown {
                requested: "cam:nope".to_owned(),
            },
            ErrorKind::CameraAmbiguous => Error::CameraAmbiguous {
                requested: "cam:web".to_owned(),
                candidates: vec![
                    CameraId::parse("cam:webcam").expect("literal id"),
                    CameraId::parse("cam:webcam-2").expect("literal id"),
                ],
            },
            ErrorKind::ControlUnknown => Error::ControlUnknown {
                requested: "brightnes".to_owned(),
                did_you_mean: vec![ControlSlug::parse("brightness").expect("literal slug")],
            },
            ErrorKind::ControlReadOnly => Error::ControlReadOnly {
                control: ControlSlug::parse("privacy").expect("literal slug"),
            },
            ErrorKind::ControlInactive => Error::ControlInactive {
                control: ControlSlug::parse("white_balance_temperature").expect("literal slug"),
                automation: Some(
                    ControlSlug::parse("white_balance_automatic").expect("literal slug"),
                ),
            },
            ErrorKind::FormatUnsupported => Error::format_unsupported(
                Some(PixelFormat::NV12),
                vec![PixelFormat::MJPG, PixelFormat::YUYV],
            ),
            ErrorKind::SettleTimeout => Error::SettleTimeout {
                waited_ms: 5_000,
                frames_seen: 3,
            },
            ErrorKind::FingerprintMismatch => Error::FingerprintMismatch {
                fields: vec!["card".to_owned(), "usb_id".to_owned()],
            },
            ErrorKind::SessionConflict => Error::SessionConflict {
                detail: "another session for this camera and task is already sweeping".to_owned(),
            },
            ErrorKind::IllegalTransition => Error::IllegalTransition {
                from: "untouched".to_owned(),
                op: "select".to_owned(),
            },
            ErrorKind::SchemaVersionForeign => Error::SchemaVersionForeign {
                found: 99,
                supported: 1,
            },
            ErrorKind::StoreLocked => Error::StoreLocked {
                holder: Some(Holder {
                    // `webcam-handler-`, and not `webcam-handler-daemon`, because this
                    // sample is a `comm` and a `comm` is fifteen characters: the kernel's
                    // `TASK_COMM_LEN` is 16 including the NUL, so every binary in this
                    // workspace now reports the same truncation of the shared prefix
                    // (measured, note **N90**). A sample carrying a name `/proc` never
                    // hands out would teach a client author to match on one.
                    pid: 909,
                    comm: Some("webcam-handler-".to_owned()),
                }),
                // The daemon's protocol, because it is the one D9 writes a sentence for:
                // this sample is what the OpenRPC document shows a client author, and the
                // refusal they need to render is "use webcam-handler-client", not "try
                // again" — which is also why the *protocol* and never the `comm` is what the
                // advice turns on now that the four binaries share one `comm`.
                protocol: Some(LockProtocol::HeldForLifetime),
            },
            ErrorKind::HolderGone => Error::HolderGone { pid: 4242 },
            ErrorKind::DeviceIo => Error::DeviceIo {
                operation: "VIDIOC_S_EXT_CTRLS".to_owned(),
                errno: Some(22),
                message: "Invalid argument".to_owned(),
            },
            ErrorKind::StorageIo => Error::StorageIo {
                path: "/home/you/.local/state/webcam-handler".into(),
                errno: Some(28),
                message: "No space left on device".to_owned(),
            },
        }
    }
}

/// The field name that marks a `--json` document as a failure rather than an answer.
///
/// One string, because three readers have to agree on it and only one of them is Rust:
/// [`Failure`]'s own field below, `webcam-handler-xtask`'s walk over every document a verb
/// answers with — which asserts that none of them declares a property by this name, so a
/// success document can never be mistaken for a refusal — and
/// `scripts/gates/json-validates.sh`, which reads this constant out of the tree and looks for
/// it in the documents the shipped binary actually prints. A gate that transcribed the name
/// would keep looking for a marker the product had stopped emitting, which is the shape of
/// defect docs/9's derived-population rule exists to stop.
pub const FAILURE_MARKER: &str = "failed";

/// A failed `--json` invocation, as the document standard output carries (owner ruling,
/// 2026-08-15; note **N127**).
///
/// **Why this exists.** AGENTS' opening section says the error vocabulary is read
/// unsupervised — *"`Busy` means retry, `DeviceGone` means stop and tell the human,
/// `PermissionDenied` means a setup problem — collapsing them makes the agent guess"* — and
/// note **N124** measured that the discriminant reached no command-line caller at all: both
/// roots printed one `Display` sentence to standard error, exited 1 for every one of the
/// eighteen kinds, and left standard output empty. The owner's ruling is that **the JSON is
/// the mechanism and it must be self-contained**, with distinct exit codes as redundancy
/// beside it ([`crate::error`]'s consumers reach those through `cli_core::exit_code`).
///
/// **Three fields, and each answers a different question a reader has:**
///
/// | Field | Answers |
/// |---|---|
/// | [`FAILURE_MARKER`] (`failed`) | *is this a failure?* — before anything else is parsed |
/// | `error` | *which failure, and with what?* — the D13 value in the registry's own serde spelling, payload included |
/// | `message` | *what would a human have been told?* — [`Error`]'s own `Display`, the same sentence standard error carries |
///
/// **The error is nested rather than flattened, and that is a defect avoided rather than a
/// preference.** Two variants of the registry — [`Error::DeviceIo`] and [`Error::StorageIo`]
/// — carry a `message` field of their own, so flattening the payload beside this document's
/// `message` would put two different strings under one key and let serde pick. Nesting also
/// makes the payload exactly what the wire's `data` object is (design D10), so a client
/// author reading `schemas/webcam-handler-openrpc.json` and an agent reading a command line's
/// standard output are looking at the same bytes.
///
/// **What is deliberately not here is the JSON-RPC code.** That number is a fact about the
/// wire (D10), `webcam-handler-cli-core` links no `webcam-handler-api` — the dependency wall
/// `scripts/gates/dependency-walls.sh` calls "pure", which jsonrpsee's tokio edge would
/// break — and the command line's own numeric redundancy is the exit code. Two numeric
/// registries in one document would be two things a caller could branch on and one of them
/// would be the wrong one.
///
/// **This is not an envelope.** `--json` still emits one `webcam-handler-schema` type
/// verbatim; a failure emits a *different* type from an answer, which is why the success
/// documents did not move and why `cli_core::render`'s "no envelope, no timestamp, no tool
/// version" rule is untouched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Failure {
    /// Always `true`, and the reader's first question answered without parsing anything else.
    ///
    /// Private, with [`Failure::new`] as the only constructor, because it is a constant of the
    /// shape rather than data: a `Failure` somebody could build with `failed: false` would be
    /// a refusal document announcing that nothing was refused.
    ///
    /// Refused on the way *in* as well — `refuse_a_document_that_says_it_did_not_fail` below is
    /// the deserializer — so a consumer deserializing into this type cannot end up holding one
    /// either, which is the half a constructor alone cannot give: the bytes arrive from
    /// somewhere else.
    #[serde(deserialize_with = "refuse_a_document_that_says_it_did_not_fail")]
    failed: bool,

    /// The typed error, serialized exactly as the wire's `data` object is.
    ///
    /// The discriminant is its `kind` tag in the registry's own snake_case spelling, and every
    /// actionable field the variant carries is beside it — the `available` formats of a
    /// [`Error::FormatUnsupported`], the `holders` **and** the `this_process` of a
    /// [`Error::Busy`], the `path` of a [`Error::StorageIo`]. Those payloads are the entire
    /// reason the variants carry data, and a document that dropped them would be the English
    /// sentence again wearing braces.
    ///
    /// The enumeration is illustrative and it still has to be *true*: it named `holders` alone
    /// for a day after [`Occupation`] landed beside it, which is the failure mode the whole
    /// registry exists to avoid, one document along (note **N222**).
    pub error: Error,

    /// What a person watching a terminal was told, without the program's name in front of it.
    ///
    /// [`Error`]'s own `Display` and never a second rendering (design §2.10) — the same string
    /// the wire's `message` carries and the same one `Program::error_line` prefixes on standard
    /// error, so the two channels cannot come to describe one failure differently.
    pub message: String,
}

impl Failure {
    /// The document for one error.
    ///
    /// The only constructor, so `message` is derived from `error` rather than passed beside it:
    /// a caller that could supply both could supply a sentence describing a different failure,
    /// and the whole claim this document makes is that its three fields are three views of one
    /// value.
    #[must_use]
    pub fn new(error: Error) -> Failure {
        Failure {
            failed: true,
            message: error.to_string(),
            error,
        }
    }

    /// Whether this document says a verb failed — which it always does.
    ///
    /// An accessor rather than a public field for the reason the field's own doc gives, and it
    /// exists so a consumer that deserialized one can still read the marker it branched on.
    #[must_use]
    pub const fn failed(&self) -> bool {
        self.failed
    }

    /// The discriminant, for a caller that has the document and wants the value.
    #[must_use]
    pub fn kind(&self) -> ErrorKind {
        self.error.kind()
    }
}

/// Refuse a `failed` that is not `true`.
///
/// The marker is what tells a reader this document is a refusal, so a `false` here is a
/// document that contradicts the only thing it exists to say. Deserialization refuses rather
/// than normalizing, because normalizing would make `{"failed": false}` parse into a value
/// claiming the opposite of what was written.
fn refuse_a_document_that_says_it_did_not_fail<'de, D>(
    deserializer: D,
) -> std::result::Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    if bool::deserialize(deserializer)? {
        Ok(true)
    } else {
        Err(serde::de::Error::custom(format!(
            "a failure document is marked `\"{FAILURE_MARKER}\": true`; one marked false is \
             not a failure document"
        )))
    }
}

/// The result type every fallible operation in this workspace returns.
pub type Result<T> = std::result::Result<T, Error>;

/// The name the backend traits (T1/T2) use for the same registry.
///
/// One error vocabulary, not two: rubric B2 requires every ioctl error path to map to a
/// D13 variant, which is only checkable if the backend speaks D13 directly.
pub type BackendError = Error;

/// The whole of what a [`Error::Busy`] can say about who has the node.
///
/// Three answers, in the order a caller can act on them (note **N217**):
///
/// 1. **This process, and what for.** Nothing was walked and nothing needs to be; the
///    refusal names the work rather than the pid, so the caller waits for the right thing
///    and does not go looking for something to kill.
/// 2. **Somebody else, named.** The `/proc` walk found them.
/// 3. **Somebody else, unnamed** — which is the honest end of a walk that found nobody it
///    could see, and no longer doubles as the sentence for case 1. That last clause is a
///    claim about the *producers* rather than about this function, and it held for two of
///    the five in-process ones when it was first written: a full command queue, a second
///    stream on one node and a preview at its viewer cap all still came through case 3
///    (note **N221**). It is true because [`Occupation`] now has an alternative for each.
fn format_busy(holders: &[Holder], this_process: &Option<Occupation>) -> String {
    match this_process {
        Some(occupation) => occupation.advice().to_owned(),
        None => format!("held by {}", format_holders(holders)),
    }
}

fn format_holders(holders: &[Holder]) -> String {
    if holders.is_empty() {
        // Deliberately vague, because the two reasons a walk finds nobody are
        // indistinguishable from here: `/proc` may have been unreadable, or the holder may
        // be another user's process, which this user cannot see without privilege. An
        // earlier version claimed the first, on a build where nothing had looked at
        // `/proc` at all.
        return "an unidentified process".to_owned();
    }
    holders
        .iter()
        .map(|h| match &h.comm {
            Some(comm) => format!("{comm} (pid {})", h.pid),
            None => format!("pid {}", h.pid),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_holder(holder: &Option<Holder>) -> String {
    match holder {
        Some(h) => format!(" by {}", format_holders(std::slice::from_ref(h))),
        None => String::new(),
    }
}

/// D9's advice for whoever just met a held lock, when the holder said which protocol it
/// follows.
///
/// Nothing when it did not, on the same principle `format_holder` follows: the advice turns
/// entirely on a fact we read out of the holder's record, and inventing it from a record we
/// could not read would tell somebody to go and start `webcam-handler-client` against a daemon
/// that may not exist.
fn format_advice(protocol: &Option<LockProtocol>) -> String {
    match protocol {
        Some(protocol) => format!("; {}", protocol.advice()),
        None => String::new(),
    }
}

fn format_ids(ids: &[CameraId]) -> String {
    ids.iter()
        .map(CameraId::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_suggestions(slugs: &[ControlSlug]) -> String {
    if slugs.is_empty() {
        String::new()
    } else {
        format!(
            "; did you mean {}?",
            slugs
                .iter()
                .map(ControlSlug::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

/// The half of an inactive-control refusal that says what to do about it.
///
/// **The advice named a flag no binary has**, and writing P6e's agent guide is what put the
/// sentence in front of somebody (note **N123**): it said *"or use `--guarded`"*, which is
/// design D3's spelling and was never the command surface's. The shipped verb guards by
/// default and `--no-guard` opts out, so an unattended caller following this message typed a
/// flag clap refuses and got exit 2 — the failure this project spends a whole error registry
/// avoiding, wearing the registry's own words.
///
/// The repair says what is true of **both** surfaces rather than swapping one flag name for
/// another: this message crosses the wire as a D13 `message` (design D10), where the reader
/// is a client author with no command line at all, so naming any flag here was the wider
/// mistake. `a_refusal_names_the_guard_and_never_a_flag_that_does_not_exist` is what stops it
/// coming back.
///
/// **And then it named the guard, which is the thing that refused** (note **N220**). The
/// repair above landed *"or write with the automation guard on"* — true of both surfaces and
/// still unreachable advice: `engine::pairing::plan_unguarded` never produces this refusal and
/// says so in its own `# Errors`, so the guard was already on for every caller who has ever
/// read this sentence. Both alternatives here are now a lever the caller can actually pull —
/// the automation, by name, to set to manual — and
/// `an_inactive_control_is_not_answered_with_the_guard_that_refused_it` is what stops *that*
/// coming back.
fn format_automation(automation: &Option<ControlSlug>) -> String {
    match automation {
        Some(a) => {
            format!(
                ": {a} owns it and this build could not switch it off — set {a} to manual and write again"
            )
        }
        None => " and no automation partner was discovered for it".to_owned(),
    }
}

/// The one sentence [`Error::FormatUnsupported`] renders, in whichever of its three shapes
/// the payload says applies.
///
/// **The sentence is the part of the payload a caller reads first**, and until 2026-08-16 a
/// size refusal borrowed the format one: *"format (unspecified) is unavailable; MJPG, YUYV
/// would be accepted"* for a request whose format was MJPG and whose *size* was the
/// problem. An agent following `docs/agent-guide.md`'s "fix the request" disposition retries
/// with a format the sentence just named, meets the identical refusal, and loops — note
/// **N129**'s class, at the same variant, one phase later (note **N138**). The container
/// arm is the third reading of the same lesson (note **N211**): it names an extension,
/// because an extension is the only thing the caller can change that makes the recording
/// happen, and it names no format at all, because every format it could name is a claim
/// about this build that a caller would read as a claim about its camera.
///
/// The arms name different levers on purpose: one says which formats would be taken, one
/// which sizes, one which files, and none mentions another's.
fn format_capture_refusal(
    requested: &Option<PixelFormat>,
    available: &[PixelFormat],
    size: &Option<SizeRefusal>,
    container: &Option<ContainerRefusal>,
) -> String {
    match (size, container) {
        // Not reachable through the three constructors, which is exactly why it is written
        // out: this type deserializes, so a foreign or future producer can hand us both, and
        // a renderer that dropped one would hide the half it did not choose. AGENTS rule 6 —
        // a payload-carrying arm rather than a panic or a silent preference.
        (Some(size), Some(container)) => format!(
            "{}; and {}",
            format_size_refusal(size),
            format_container_refusal(container)
        ),
        (Some(size), None) => format_size_refusal(size),
        (None, Some(container)) => format_container_refusal(container),
        (None, None) => format!(
            "format {} is unavailable; {} would be accepted",
            format_requested(requested),
            format_formats(available)
        ),
    }
}

fn format_size_refusal(size: &SizeRefusal) -> String {
    format!(
        "no mode delivers {}x{}; {} would be accepted",
        size.requested_width,
        size.requested_height,
        format_sizes(&size.available)
    )
}

/// The container arm's sentence: what could not be written, and which file would have taken
/// it instead.
///
/// Four readings, and each one is a different thing for the caller to do. A named container
/// with a carrier says *rename the file*; a named container with none says *stop, and change
/// what the camera is delivering*; an unnamed container is the path that carried no extension
/// at all, where a different extension is not the remedy because the caller never chose one.
/// The fourth — no container named but one that carries — is not produced by
/// [`crate::video::VideoFormat::resolve`], which only reaches its unnamed arm when nothing
/// carries the stream; it is written because the payload deserializes and a renderer that
/// cannot say something true about a value it was handed is a renderer that will one day say
/// something false.
fn format_container_refusal(refusal: &ContainerRefusal) -> String {
    let carriers = format_containers(&refusal.carried_by);
    match (refusal.container, refusal.carried_by.is_empty()) {
        (Some(named), false) => format!(
            "a .{} file cannot carry {} frames; {carriers} would take them",
            named.extension(),
            refusal.negotiated
        ),
        (Some(named), true) => format!(
            "a .{} file cannot carry {} frames, and no container this build writes would",
            named.extension(),
            refusal.negotiated
        ),
        (None, true) => format!(
            "no container this build writes carries {} frames",
            refusal.negotiated
        ),
        (None, false) => format!(
            "the sink named no container for {} frames; {carriers} would take them",
            refusal.negotiated
        ),
    }
}

/// Containers as the extensions a caller types, because the extension is the lever this
/// refusal is about — `.avi`, not `Avi`, so the remedy can be pasted onto a `-o` argument.
fn format_containers(containers: &[VideoFormat]) -> String {
    if containers.is_empty() {
        return "nothing".to_owned();
    }
    containers
        .iter()
        .map(|container| format!(".{}", container.extension()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_requested(requested: &Option<PixelFormat>) -> String {
    match requested {
        Some(f) => f.to_string(),
        None => "(unspecified)".to_owned(),
    }
}

fn format_formats(formats: &[PixelFormat]) -> String {
    if formats.is_empty() {
        // Just "nothing": the empty list has two causes and this sentence cannot tell them
        // apart. A capture node really enumerating no format is one; a caller that filtered
        // the enumeration down to nothing it could use — the fake, which can synthesise only
        // a few — is the other, and this string said "the capture node enumerated no
        // formats" for both until 2026-08-16, which is false for the second (note **N138**).
        // What a caller needs from it is the same either way: nothing here would be taken,
        // so stop rather than retry.
        "nothing".to_owned()
    } else {
        formats
            .iter()
            .map(PixelFormat::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The sizes a caller could ask for, rendered so the answer can be typed back at the
/// command line.
///
/// A stepwise entry is written as the range it is — `32x32..1920x1080` — rather than as its
/// maximum corner, because a caller told only the corner would ask for the one size in the
/// range this project has spent a note explaining it does not have to ask for \[PF:26\].
fn format_sizes(sizes: &[FrameSize]) -> String {
    if sizes.is_empty() {
        return "nothing".to_owned();
    }
    sizes
        .iter()
        .map(|size| match *size {
            FrameSize::Discrete { width, height } => format!("{width}x{height}"),
            FrameSize::Stepwise {
                min_width,
                max_width,
                min_height,
                max_height,
                ..
            } => format!("{min_width}x{min_height}..{max_width}x{max_height}"),
            // Kept renderable rather than unreachable: `SizeRefusal::available` filters
            // these out, and a `match` on device vocabulary still needs a payload-carrying
            // arm (AGENTS rule 6) rather than a panic that a future producer would find.
            FrameSize::Unknown { raw } => format!("a size shape this build cannot read ({raw:#x})"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_errno(errno: &Option<i32>) -> String {
    match errno {
        Some(e) => format!(" (errno {e})"),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::Sink;

    #[test]
    fn every_kind_has_a_sample_and_the_sample_reports_that_kind() {
        // The population is `ErrorKind::ALL`, generated from the vocabulary macro — so
        // this walk cannot silently shrink when a variant is added.
        for &kind in ErrorKind::ALL {
            assert_eq!(
                Error::sample(kind).kind(),
                kind,
                "sample({kind:?}) has the wrong kind"
            );
        }
    }

    #[test]
    fn every_kind_renders_something_a_human_can_act_on() {
        for &kind in ErrorKind::ALL {
            let rendered = Error::sample(kind).to_string();
            assert!(!rendered.is_empty(), "{kind:?} renders empty");
            assert!(
                !rendered.contains("{"),
                "{kind:?} leaked a format placeholder: {rendered}"
            );
            // A rendering that is just the variant name is a rendering nobody wrote.
            assert!(rendered.len() > 12, "{kind:?} renders too thin: {rendered}");
        }
    }

    #[test]
    fn an_illegal_transitions_instruction_is_the_last_thing_it_says() {
        // **The producers, driven** — not a value built here (docs/11 **L29**, note
        // **N212**). `Error::sample`'s `IllegalTransition` is `op: "select"`, the one shape
        // this variant had when D8 was its only caller and the one shape a trailing `from
        // state …` reads after, which is why `every_kind_renders_something_a_human_can_act_on`
        // stayed green while the shipped binaries printed *"cannot write a photo to
        // …/x.tiff; this build writes .jpg, .png, .ppm from state
        // unwritable_extension(tiff)"*. So the population here is every producer this crate
        // has, called: what each one puts in `op` is what a caller is supposed to do, and it
        // has to survive being rendered.
        //
        // The five below are this crate's; the other six live in `webcam-handler-engine`,
        // `webcam-handler-daemon`, `webcam-handler-cli-core` and `webcam-handler-client`, and
        // `a_refusal_ends_with_the_instruction_its_payload_carries` drives one of those through
        // the shipped binary — the template is one line and one arm of it going red is enough
        // to condemn it.
        let refusals = [
            Sink::ServerPath {
                path: "/tmp/x.tiff".into(),
            }
            .writable_format()
            .err(),
            crate::video::RecordRequest {
                sink: Sink::ServerPath {
                    path: "/tmp/take.mkv".into(),
                },
                ..record_request()
            }
            .container()
            .err(),
            crate::video::RecordRequest {
                sink: Sink::ReturnBytes {
                    format: crate::capture::PhotoFormat::Jpeg,
                },
                ..record_request()
            }
            .container()
            .err(),
            crate::video::RecordRequest {
                duration_ms: Some(crate::limits::MAX_RECORDING_MS + 1),
                ..record_request()
            }
            .budget_ms()
            .err(),
            crate::capture::SettlePolicy {
                deadline_ms: crate::limits::MAX_SETTLE_DEADLINE_MS + 1,
                ..crate::capture::SettlePolicy::default()
            }
            .within_bound()
            .err(),
        ];

        let mut seen = 0;
        for refusal in refusals {
            let refusal = refusal.expect("each call above is refused");
            let Error::IllegalTransition { from, op } = &refusal else {
                panic!("{refusal:?} is not the variant this test is about");
            };
            seen += 1;
            let rendered = refusal.to_string();
            // The property, and it is about the *end* of the message: whatever a producer
            // wrote as the instruction is a sentence of its own, so nothing may follow it.
            assert!(
                rendered.ends_with(op.as_str()),
                "the instruction is not the last thing this refusal says, so a producer's \
                 sentence runs into the state that refused it: {rendered}"
            );
            // And the condition is still in there, first, where a label goes. Losing it
            // would be the opposite defect: a message a reader cannot tell apart from the
            // same instruction refused for another reason.
            assert!(
                rendered.starts_with(from.as_str()),
                "the refusal no longer says what state it refused from: {rendered}"
            );
            // Not vacuous in the direction that matters: at least one producer here writes a
            // whole instruction rather than a verb, which is the shape the old template
            // garbled.
        }
        assert_eq!(seen, 5, "this crate's producers are five");
        assert!(
            refusal_op(
                Sink::ServerPath {
                    path: "/tmp/x.tiff".into()
                }
                .writable_format()
            )
            .contains(';'),
            "the fixture stopped being the multi-clause shape this test is about"
        );
    }

    /// A `RecordRequest` whose every field but the one under test is the ordinary one.
    ///
    /// Here rather than in each arm above so the arms differ only in the field they are
    /// about, which is what makes them readable as a list of producers.
    fn record_request() -> crate::video::RecordRequest {
        crate::video::RecordRequest {
            stream: crate::capture::StreamRequest::default(),
            duration_ms: None,
            sink: Sink::ServerPath {
                path: "/tmp/take.avi".into(),
            },
            wait: false,
        }
    }

    /// The `op` of a refusal, or a panic naming what came back instead.
    fn refusal_op<T: std::fmt::Debug>(result: Result<T>) -> String {
        match result {
            Err(Error::IllegalTransition { op, .. }) => op,
            other => panic!("{other:?} is not an illegal transition"),
        }
    }

    #[test]
    fn every_kind_round_trips_through_json() {
        for &kind in ErrorKind::ALL {
            let err = Error::sample(kind);
            let json = serde_json::to_string(&err).expect("serialize");
            let back: Error = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, err, "{kind:?} did not survive the wire");
        }
    }

    #[test]
    fn a_format_unsupported_refusal_names_its_formats_readably_in_json_and_not_only_in_prose() {
        // The failure the owner's ruling of 2026-08-14 is *about* (note **N109**), asserted
        // where it happens rather than where the type is defined. `FormatUnsupported` is
        // P6b's central refusal and the one an agent is supposed to act on — retry with a
        // format the camera has — and until this change the `message` said "format NV12 is
        // unavailable; this camera offers MJPG, YUYV" while the `data` an unattended reader
        // parses said `"available": [[77,74,80,71],[89,85,89,86]]`. Two renderings of one
        // fact, and the machine-readable one was the unreadable one.
        //
        // Asserted over `Error::sample`, because that is the value the emitted OpenRPC
        // document carries as this variant's example: an agent reading `schemas/` to learn
        // the API sees exactly this JSON before it ever makes a call.
        let json = serde_json::to_value(Error::sample(ErrorKind::FormatUnsupported))
            .expect("the sample serializes");
        assert_eq!(json["requested"], serde_json::json!("NV12"));
        assert_eq!(json["available"], serde_json::json!(["MJPG", "YUYV"]));

        // …and the two halves agree. The message is built by `format_formats`, which spells
        // a format with `PixelFormat`'s `Display`, and the wire is built by its `Serialize`,
        // which is the same function — so this asserts the property that keeps them from
        // ever drifting rather than two constants that happen to match today.
        let rendered = Error::sample(ErrorKind::FormatUnsupported).to_string();
        for spelled in ["NV12", "MJPG", "YUYV"] {
            assert!(rendered.contains(spelled), "{rendered}");
            assert!(
                serde_json::to_string(&json)
                    .expect("re-serialize")
                    .contains(spelled),
                "the JSON dropped {spelled}"
            );
        }

        // The empty case still says what it means. It said "nothing (the capture node
        // enumerated no formats)" until 2026-08-16, which named a cause this variant cannot
        // know: the fake reaches the same empty list by filtering an enumeration down to
        // the formats it can synthesise, and for that caller the sentence was false (note
        // **N138**). What is true either way is that nothing here would be taken, which is
        // also the only part a caller can act on — stop rather than retry.
        let none_at_all = Error::format_unsupported(Some(PixelFormat::MJPG), Vec::new());
        assert_eq!(
            none_at_all.to_string(),
            "format MJPG is unavailable; nothing would be accepted"
        );
    }

    #[test]
    fn a_size_refusal_names_the_size_and_never_offers_a_format_as_the_remedy() {
        // **The sentence is the part of the payload a caller reads first** (note **N138**).
        // A size refusal rendered "format (unspecified) is unavailable; MJPG, YUYV would be
        // accepted" for the two days between the two halves of the H1b repair — about
        // formats, naming the caller's own format as acceptable, never mentioning size — so
        // an agent obeying the guide's "fix the request" disposition retried the format and
        // met the identical refusal. This asserts the falsifiable halves: the size the
        // caller named is in the sentence, the sizes it could ask for are in the sentence,
        // and no format is.
        let refusal = Error::size_unsupported(
            320,
            240,
            vec![
                FrameSize::Discrete {
                    width: 640,
                    height: 480,
                },
                FrameSize::Stepwise {
                    min_width: 64,
                    max_width: 1_920,
                    step_width: 2,
                    min_height: 64,
                    max_height: 1_080,
                    step_height: 2,
                },
            ],
            vec![PixelFormat::MJPG, PixelFormat::YUYV],
        );
        let sentence = refusal.to_string();
        assert!(sentence.contains("320x240"), "{sentence}");
        assert!(sentence.contains("640x480"), "{sentence}");
        // The stepwise entry as the range it is, not as its maximum corner — the same
        // falsehood `FrameSize::largest_within` exists to avoid.
        assert!(sentence.contains("64x64..1920x1080"), "{sentence}");
        for format in ["MJPG", "YUYV"] {
            assert!(
                !sentence.contains(format),
                "the refusal offers {format} as the remedy for a size it cannot deliver: \
                 {sentence}"
            );
        }
        // And the payload carries the same two facts, so a caller never has to parse the
        // sentence to get them (AGENTS: "every variant carries what the caller needs to
        // act on it").
        let Error::FormatUnsupported {
            requested,
            size: Some(size),
            ..
        } = &refusal
        else {
            panic!("{refusal:?}");
        };
        assert_eq!(*requested, None, "{refusal:?}");
        assert_eq!((size.requested_width, size.requested_height), (320, 240));
        assert_eq!(size.available.len(), 2);

        // The other direction, so the assertions above are about the size arm rather than
        // about a renderer that never names formats.
        let format_half = Error::format_unsupported(
            Some(PixelFormat::NV12),
            vec![PixelFormat::MJPG, PixelFormat::YUYV],
        );
        assert_eq!(
            format_half.to_string(),
            "format NV12 is unavailable; MJPG, YUYV would be accepted"
        );
    }

    #[test]
    fn the_permission_hint_names_the_group() {
        // The hint lives in exactly one place (D13); this pins what it says, so a
        // rewrite that drops the actionable part fails here.
        let Error::PermissionDenied { hint, .. } = Error::sample(ErrorKind::PermissionDenied)
        else {
            panic!("sample changed shape");
        };
        assert!(
            hint.contains("video"),
            "the hint must name the group: {hint}"
        );
    }

    #[test]
    fn busy_without_holders_still_says_something_useful() {
        // The walk finds nobody for two indistinguishable reasons — `/proc` restricted, or
        // a holder belonging to another user — so the rendering commits to neither, and
        // above all must not read as "nobody has it".
        let err = Error::busy("/dev/video0".into(), Vec::new());
        let rendered = err.to_string();
        assert!(rendered.contains("unidentified process"), "{rendered}");
        assert!(
            !rendered.contains("/proc"),
            "the rendering must not claim something about /proc it did not check: {rendered}"
        );

        // The other direction: a holder the walk *did* find is named, so the empty
        // rendering is a fallback rather than the only thing this ever says.
        let named = Error::busy(
            "/dev/video0".into(),
            vec![Holder {
                pid: 4321,
                comm: Some("cheese".to_owned()),
            }],
        );
        assert!(named.to_string().contains("cheese (pid 4321)"), "{named}");
    }

    #[test]
    fn a_camera_this_process_is_holding_is_never_reported_as_held_by_a_stranger() {
        // **The sentence that sent an agent hunting for a process** (docs/11 **M19**, note
        // **N217**). The daemon refuses its own callers with an empty holder list on
        // purpose — a pid in a refusal is an invitation to signal it — and the rendering
        // turned that deliberate silence into *"held by an unidentified process"*.
        //
        // Walked over the whole vocabulary, so an alternative added later has to answer the
        // same three questions rather than inherit an answer from the two written here.
        for &occupation in Occupation::ALL {
            let refusal = Error::busy_here("/dev/video0".into(), occupation);
            let rendered = refusal.to_string();
            assert!(
                !rendered.contains("unidentified"),
                "{occupation:?}: this process knows exactly what it is doing: {rendered}"
            );
            assert!(
                !rendered.contains("held by"),
                "{occupation:?}: a caller told the node is `held by` something goes looking \
                 for it: {rendered}"
            );
            // The actionable half: what to wait for, since `Busy`'s whole disposition is
            // *retry* and a retry that does not know what it waits for is a loop.
            assert!(
                rendered.contains("ask again"),
                "{occupation:?}: the refusal says to retry and not what for: {rendered}"
            );
            // And it says *who* has it, which is this whole field's reason. Until the guide
            // arm in `webcam-handler-cli` was widened, the alternative to "ask again" this
            // assertion allowed was the literal `record_status` — a verb no surface offers,
            // pinned by the test that was supposed to be able to go red on it (note **N220**).
            assert!(
                rendered.contains("this process"),
                "{occupation:?}: the refusal says to retry and not who is in the way: \
                 {rendered}"
            );
            // And the pid is still withheld, which is the decision this repair kept.
            let Error::Busy { holders, .. } = &refusal else {
                panic!("{refusal:?}");
            };
            assert!(holders.is_empty(), "{occupation:?}: {refusal:?}");
        }

        // The other door, unchanged: a walk that found somebody names them, and a walk that
        // found nobody still says so rather than claiming this process.
        let stranger = Error::busy(
            "/dev/video0".into(),
            vec![Holder {
                pid: 4321,
                comm: Some("cheese".to_owned()),
            }],
        );
        assert!(
            stranger.to_string().contains("cheese (pid 4321)"),
            "{stranger}"
        );
        assert!(
            Error::busy("/dev/video0".into(), Vec::new())
                .to_string()
                .contains("an unidentified process")
        );
    }

    #[test]
    fn an_inactive_control_is_not_answered_with_the_guard_that_refused_it() {
        // **Note N123's finding, one reading deeper** (note **N220**). That entry took
        // *"disable white_balance_automatic first, or use `--guarded`"* — a flag no binary
        // has — and replaced the flag with *"or write with the automation guard on"*, on the
        // ground that the guard is a fact of both surfaces where the flag was a fact of one.
        // True, and still the wrong sentence: `engine::pairing::plan_unguarded` documents
        // that it never produces this refusal and its own suite holds it to that, so **every**
        // caller who reads this message wrote with the guard already on. The remedy offered
        // was the thing that had just refused, which is the disposition table's "change the
        // plan" turned into a loop.
        let refusal = Error::ControlInactive {
            control: ControlSlug::parse("white_balance_temperature").expect("literal slug"),
            automation: Some(ControlSlug::parse("white_balance_automatic").expect("slug")),
        };
        let rendered = refusal.to_string();
        assert!(
            !rendered.contains("guard"),
            "the guard is what refused, so it cannot also be the remedy: {rendered}"
        );
        // …and what it says instead is the control to act on, twice over: the one that is
        // inactive and the one that owns it. A message naming only the first would leave an
        // unattended caller with a fact and no lever.
        assert!(
            rendered.contains("white_balance_temperature")
                && rendered.contains("white_balance_automatic"),
            "{rendered}"
        );
        // The other alternative is a different fact and keeps its own sentence: nothing is
        // named because nothing was found, and inventing an automation to blame would be the
        // same defect wearing the other sign.
        let unpaired = Error::ControlInactive {
            control: ControlSlug::parse("exposure_time_absolute").expect("literal slug"),
            automation: None,
        };
        assert!(
            unpaired.to_string().contains("no automation partner"),
            "{unpaired}"
        );
    }

    #[test]
    fn a_daemons_lock_is_refused_in_the_words_d9_writes() {
        // Quoted from design D9, parenthetical and em dash included: "`webcam-handler-cli`
        // finding it held reports *daemon owns the state (and likely the camera) — use
        // webcam-handler-client* rather than corrupting or blocking (D13)". The literal is
        // here, in the test, so that rewording the constant is a red test rather than a diff
        // nobody reads; docs/7's shorter summary of the same sentence is a plan's
        // abbreviation, not a second law.
        //
        // Asserted against `sample` rather than a value built here, because that is the
        // one the generated documents carry: `Error::sample` is the walkable population
        // the OpenRPC emitter renders, so a sample that stopped naming a protocol would
        // ship a `store_locked` example with D9's advice missing from it.
        let err = Error::sample(ErrorKind::StoreLocked);
        let rendered = err.to_string();
        assert!(
            rendered.contains(
                "daemon owns the state (and likely the camera) — use webcam-handler-client"
            ),
            "{rendered}"
        );
        // And it still names who, because "somebody has it, go and use another program" is
        // half an answer: the pid is what an operator checks before believing us. The name
        // beside it is fifteen characters because a `comm` is (note **N90**), which is
        // precisely why the pid is the half that identifies anybody.
        assert!(rendered.contains("webcam-handler- (pid 909)"), "{rendered}");
    }

    #[test]
    fn a_momentary_lock_is_not_a_reason_to_go_and_start_wchc() {
        // The other direction, and the one that makes the first arm a decision rather than a
        // constant: another `webcam-handler-cli` a few milliseconds into a mutating verb will
        // be gone shortly, and sending its user off to a daemon that does not exist would make
        // their situation worse.
        //
        // The `comm` is the same fifteen characters the daemon's sample carries, and that is
        // the point rather than a copy-paste: since note **N90** every binary here truncates
        // to `webcam-handler-`, so the record's *protocol* is the only field that can tell
        // these two refusals apart, and this arm is what proves the advice reads it.
        let err = Error::StoreLocked {
            holder: Some(Holder {
                pid: 123,
                comm: Some("webcam-handler-".to_owned()),
            }),
            protocol: Some(LockProtocol::PerOperation),
        };
        let rendered = err.to_string();
        assert!(!rendered.contains("webcam-handler-client"), "{rendered}");
        assert!(rendered.contains("free shortly"), "{rendered}");

        // And an unreadable record advises nothing at all, because the advice turns
        // entirely on a fact that record carries.
        let unknown = Error::StoreLocked {
            holder: None,
            protocol: None,
        };
        assert_eq!(unknown.to_string(), "the state directory is locked");
    }

    #[test]
    fn every_locking_protocol_answers_the_question_the_refusal_asks() {
        // The population is generated from the vocabulary, so a third protocol cannot be
        // added without landing here: each one has to say something, and no two may say
        // the same thing — advice that did not depend on the protocol would be advice that
        // did not need the field, which is the defect rubric A8 names.
        for &protocol in LockProtocol::ALL {
            assert!(!protocol.advice().is_empty(), "{protocol} advises nothing");
            assert_eq!(
                LockProtocol::ALL
                    .iter()
                    .filter(|other| other.advice() == protocol.advice())
                    .count(),
                1,
                "{protocol} shares its advice with another protocol"
            );
            // The wire spelling and the name in a failure message are the same string, so
            // a lock record and a refusal cannot come to disagree about what to call it.
            assert_eq!(
                serde_json::to_string(&protocol).expect("serialize"),
                format!("\"{}\"", protocol.as_str())
            );
        }
    }

    #[test]
    fn every_kind_reaches_a_failure_document_marked_failed_and_carrying_its_own_discriminant() {
        // The owner's ruling of 2026-08-15 in one walk (note **N127**): *"reading the document
        // alone must tell a caller that this is a failure and which failure it is"*. The
        // population is `ErrorKind::ALL`, generated from the vocabulary macro, so a nineteenth
        // variant joins this without anybody remembering it.
        for &kind in ErrorKind::ALL {
            let error = Error::sample(kind);
            let document = Failure::new(error.clone());
            assert!(document.failed(), "{kind:?} is not marked as a failure");
            assert_eq!(document.kind(), kind);

            let json = serde_json::to_value(&document).expect("a failure document serializes");
            // The marker, by the constant the gate and the emitter's walk both read.
            assert_eq!(json[FAILURE_MARKER], serde_json::json!(true), "{kind:?}");
            // The discriminant, in the registry's own serde spelling — the same string the
            // wire's `data` object carries and `api::rpc_code`'s fixture pins its code
            // against. Derived from the kind rather than transcribed, so this cannot pin a
            // spelling the registry does not use.
            let wire_name = serde_json::to_value(kind).expect("a kind names itself");
            assert_eq!(json["error"]["kind"], wire_name, "{kind:?}");
            // The message is the error's own `Display` and not a second rendering (§2.10).
            assert_eq!(
                json["message"],
                serde_json::json!(error.to_string()),
                "{kind:?}"
            );

            // And it survives the trip back, which is what makes it a document a consumer can
            // hold rather than bytes it can only grep.
            let back: Failure = serde_json::from_value(json).expect("a failure document parses");
            assert_eq!(back, document, "{kind:?} changed on the way through");
        }
    }

    #[test]
    fn the_payload_a_variant_carries_reaches_the_document_and_is_not_flattened_over_its_message() {
        // **The case the ruling is about.** `FormatUnsupported` is what an unattended caller
        // retries on — ask for a format the camera has — and a document that carried only the
        // sentence would leave it parsing English to find the list. `available` is beside the
        // discriminant, as readable FourCCs since the owner's ruling of 2026-08-14 (note
        // **N109**).
        let document = Failure::new(Error::sample(ErrorKind::FormatUnsupported));
        let json = serde_json::to_value(&document).expect("serialize");
        assert_eq!(json["error"]["requested"], serde_json::json!("NV12"));
        assert_eq!(
            json["error"]["available"],
            serde_json::json!(["MJPG", "YUYV"])
        );

        // The nesting, and the collision it avoids rather than a preference: two variants
        // carry a `message` of their own, so a flattened payload would put the device's
        // sentence and this document's sentence under one key and let serde pick which
        // survived. Asserted on `StorageIo`, whose own message is deliberately unlike the
        // rendered one.
        let storage = Failure::new(Error::StorageIo {
            path: "/state".into(),
            errno: Some(28),
            message: "No space left on device".to_owned(),
        });
        let json = serde_json::to_value(&storage).expect("serialize");
        assert_eq!(
            json["error"]["message"],
            serde_json::json!("No space left on device")
        );
        assert_eq!(
            json["message"],
            serde_json::json!("/state: No space left on device")
        );
        assert_ne!(json["error"]["message"], json["message"]);
        // The path, which is what a caller acts on: this is not the camera's fault and the
        // remedy is somewhere on a filesystem.
        assert_eq!(json["error"]["path"], serde_json::json!("/state"));
        assert_eq!(json["error"]["errno"], serde_json::json!(28));
    }

    #[test]
    fn a_document_that_says_it_did_not_fail_is_refused_rather_than_read_as_a_failure() {
        // The marker is the whole of "unambiguously a failure", so a document contradicting it
        // must not parse into a value claiming the opposite of what was written. Both
        // directions: the shipped shape parses, and the same bytes with the marker turned over
        // do not.
        let real = serde_json::to_string(&Failure::new(Error::sample(ErrorKind::Busy)))
            .expect("serialize");
        serde_json::from_str::<Failure>(&real).expect("the shipped shape parses");

        let lying = real.replace(
            &format!("\"{FAILURE_MARKER}\":true"),
            &format!("\"{FAILURE_MARKER}\":false"),
        );
        assert_ne!(lying, real, "the seed did not apply: {real}");
        let refused =
            serde_json::from_str::<Failure>(&lying).expect_err("a false marker is refused");
        assert!(refused.to_string().contains(FAILURE_MARKER), "{refused}");

        // And a document with no marker at all is not one either — which is what stops an
        // answer that happens to carry `error` and `message` being read as a refusal.
        let unmarked = real.replace(&format!("\"{FAILURE_MARKER}\":true,"), "");
        assert_ne!(unmarked, real, "the seed did not apply: {real}");
        assert!(serde_json::from_str::<Failure>(&unmarked).is_err());
    }

    #[test]
    fn an_inactive_control_with_no_partner_says_so_rather_than_implying_one() {
        let err = Error::ControlInactive {
            control: ControlSlug::parse("focus_absolute").expect("literal slug"),
            automation: None,
        };
        assert!(err.to_string().contains("no automation partner"), "{err}");
    }
}
