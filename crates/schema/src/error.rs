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
        /// See [`Error::Unimplemented`].
        Unimplemented,
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
    #[error("the state directory is locked{}", format_holder(.holder))]
    StoreLocked {
        /// Who holds it, when we could tell.
        holder: Option<Holder>,
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

    /// The operation exists in the contract and this build does not perform it yet.
    ///
    /// Emphatically **not** a capability answer (E3): the device was never asked. A
    /// half-built backend has to say something when a trait method it has not reached
    /// is called, and every other choice is worse — a panic breaks the "plugging in a
    /// webcam cannot panic the library" rule, and reusing [`Error::DeviceIo`] would
    /// blame the kernel for our own schedule. See note N6.
    #[error("{operation} is not implemented in this build; it lands at {arrives_in}")]
    Unimplemented {
        /// The trait method or verb that was called.
        operation: String,
        /// The phase that lands it, in docs/2's spelling (`P2`).
        arrives_in: String,
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
            Error::Unimplemented { .. } => ErrorKind::Unimplemented,
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
                    pid: 909,
                    comm: Some("wchd".to_owned()),
                }),
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
            ErrorKind::Unimplemented => Error::Unimplemented {
                operation: "Camera::start_stream".to_owned(),
                arrives_in: "P2".to_owned(),
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

fn format_automation(automation: &Option<ControlSlug>) -> String {
    match automation {
        Some(a) => format!(": disable {a} first, or use `--guarded`"),
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
    fn an_unfinished_operation_blames_the_build_rather_than_the_device() {
        // N6's whole reason for existing: a caller reading this must not conclude the
        // camera cannot do it, and a bug report must not be filed against the kernel.
        let Error::Unimplemented { .. } = Error::sample(ErrorKind::Unimplemented) else {
            panic!("sample changed shape");
        };
        let rendered = Error::sample(ErrorKind::Unimplemented).to_string();
        assert!(
            rendered.contains("this build"),
            "the rendering must name us, not the device: {rendered}"
        );
        assert!(
            rendered.contains("P2"),
            "the rendering must say when it arrives: {rendered}"
        );
        // And it is its own kind — no code path may fold it into a device or capability
        // answer (E3).
        assert_ne!(
            Error::sample(ErrorKind::Unimplemented).kind(),
            ErrorKind::DeviceIo
        );
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
