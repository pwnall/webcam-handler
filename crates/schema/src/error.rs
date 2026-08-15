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

use crate::camera::{CameraId, PixelFormat};
use crate::control::ControlSlug;
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
    #[error("{path} is busy: held by {}", format_holders(.holders))]
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

    /// No camera matched the id or prefix.
    #[error("no camera matches {requested:?}")]
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

    /// The requested pixel format is not one this camera offers.
    #[error("format {} is unavailable; this camera offers {}", format_requested(.requested), format_formats(.available))]
    FormatUnsupported {
        /// What was asked for, when the caller named something.
        requested: Option<PixelFormat>,
        /// What the camera does offer.
        available: Vec<PixelFormat>,
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
    #[error("cannot {op} from state {from}")]
    IllegalTransition {
        /// The state we were in.
        from: String,
        /// The operation attempted.
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
    pub fn sample(kind: ErrorKind) -> Error {
        match kind {
            ErrorKind::DeviceGone => Error::DeviceGone {
                path: "/dev/video0".into(),
            },
            ErrorKind::Busy => Error::Busy {
                path: "/dev/video0".into(),
                holders: vec![Holder {
                    pid: 4242,
                    comm: Some("cheese".to_owned()),
                }],
            },
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
            ErrorKind::FormatUnsupported => Error::FormatUnsupported {
                requested: Some(PixelFormat::NV12),
                available: vec![PixelFormat::MJPG, PixelFormat::YUYV],
            },
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

/// The result type every fallible operation in this workspace returns.
pub type Result<T> = std::result::Result<T, Error>;

/// The name the backend traits (T1/T2) use for the same registry.
///
/// One error vocabulary, not two: rubric B2 requires every ioctl error path to map to a
/// D13 variant, which is only checkable if the backend speaks D13 directly.
pub type BackendError = Error;

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
fn format_automation(automation: &Option<ControlSlug>) -> String {
    match automation {
        Some(a) => format!(": disable {a} first, or write with the automation guard on"),
        None => " and no automation partner was discovered for it".to_owned(),
    }
}

fn format_requested(requested: &Option<PixelFormat>) -> String {
    match requested {
        Some(f) => f.to_string(),
        None => "(unspecified)".to_owned(),
    }
}

fn format_formats(formats: &[PixelFormat]) -> String {
    if formats.is_empty() {
        "nothing (the capture node enumerated no formats)".to_owned()
    } else {
        formats
            .iter()
            .map(PixelFormat::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
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

        // The empty case still says what it means, because a camera whose capture node
        // enumerated nothing is a different problem from one that lacks the format asked
        // for, and an empty array is where that distinction would quietly go.
        let none_at_all = Error::FormatUnsupported {
            requested: Some(PixelFormat::MJPG),
            available: Vec::new(),
        };
        assert!(
            none_at_all.to_string().contains("enumerated no formats"),
            "{none_at_all}"
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
        let err = Error::Busy {
            path: "/dev/video0".into(),
            holders: Vec::new(),
        };
        let rendered = err.to_string();
        assert!(rendered.contains("unidentified process"), "{rendered}");
        assert!(
            !rendered.contains("/proc"),
            "the rendering must not claim something about /proc it did not check: {rendered}"
        );

        // The other direction: a holder the walk *did* find is named, so the empty
        // rendering is a fallback rather than the only thing this ever says.
        let named = Error::Busy {
            path: "/dev/video0".into(),
            holders: vec![Holder {
                pid: 4321,
                comm: Some("cheese".to_owned()),
            }],
        };
        assert!(named.to_string().contains("cheese (pid 4321)"), "{named}");
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
    fn an_inactive_control_with_no_partner_says_so_rather_than_implying_one() {
        let err = Error::ControlInactive {
            control: ControlSlug::parse("focus_absolute").expect("literal slug"),
            automation: None,
        };
        assert!(err.to_string().contains("no automation partner"), "{err}");
    }
}
