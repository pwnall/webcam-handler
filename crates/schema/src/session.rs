//! Calibration session state (design D8, D9).
//!
//! A session belongs to a (camera fingerprint, task) pair and lives as a directory of
//! JSON an agent, a human, or `jq` can read. This module is the vocabulary; the state
//! machine that moves between these states is a pure core in
//! `webcam-handler-engine::session`, and the store that writes them is
//! `webcam-handler-engine::store`.

use std::collections::BTreeMap;

use camino::Utf8PathBuf;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::camera::CameraFingerprint;
use crate::control::{ControlSlug, WriteWarning};
use crate::limits;
use crate::metrics::MetricName;
use crate::pairing::AutomationPair;
use crate::snapshot::Snapshot;
use crate::time::Stamp;

/// How to derive the values a sweep visits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SweepSpec {
    /// Every step from min to max. Capped by [`limits::MAX_SWEEP_SAMPLES`].
    All,
    /// Every `step`-th value, aligned to the control's own step.
    Uniform {
        /// The requested spacing.
        step: i64,
    },
    /// Logarithmically spaced, for controls like exposure time whose useful range
    /// spans orders of magnitude.
    Log {
        /// How many samples.
        points: u32,
    },
    /// Exactly these values.
    Explicit {
        /// The values, in order.
        values: Vec<i64>,
    },
}

/// Who chose a calibrated value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Selector {
    /// A built-in metric ranked the samples.
    Metric {
        /// Which metric.
        name: MetricName,
    },
    /// An agent reviewed the sample photos and picked one.
    Agent,
    /// A human did.
    Human,
}

/// Why a control cannot be calibrated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BlockedReason {
    /// The device says READ_ONLY \[PF:12\].
    ReadOnly,
    /// The device says DISABLED.
    Disabled,
    /// The control is INACTIVE and no automation partner was discovered to release it.
    InactiveWithoutPartner,
    /// The control's type has no ordered range to sweep (compound, opaque, string).
    NotSweepable {
        /// The type, as text, so the message is readable without a lookup.
        control_type: String,
    },
    /// Something else, described. Free-text is deliberate: the alternative is a closed
    /// vocabulary that turns an unanticipated reason into the wrong one.
    Other {
        /// What happened.
        detail: String,
    },
}

/// Where one control stands in a session (design D8's closed vocabulary).
// No `Eq`: `Calibrated` carries a metric score, and `f64` has no total equality.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ControlStatus {
    /// Nothing has happened to it yet.
    Untouched,
    /// Its automation partner has been disabled and its value parked.
    AutoDisabled {
        /// The automation controls that were switched off.
        automation: Vec<ControlSlug>,
        /// The value the control holds while parked.
        parked_value: Option<i64>,
    },
    /// A sweep is in progress.
    Sweeping {
        /// The plan being executed.
        plan: SweepSpec,
        /// Samples taken.
        done: u32,
        /// Samples planned.
        total: u32,
    },
    /// A value has been chosen.
    Calibrated {
        /// The chosen value.
        value: i64,
        /// The final sampling step — the skill's "calibration precision". Multi-pass
        /// refinement (coarse then fine) is representable because this is recorded.
        precision: i64,
        /// The score that value earned, when a metric produced one.
        score: Option<f64>,
        /// Who chose it.
        selector: Selector,
    },
    /// Deliberately set aside.
    Deferred {
        /// Why.
        reason: String,
    },
    /// Cannot be calibrated on this device.
    Blocked {
        /// Why.
        reason: BlockedReason,
    },
}

impl ControlStatus {
    /// The status name used in errors and output.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            ControlStatus::Untouched => "untouched",
            ControlStatus::AutoDisabled { .. } => "auto_disabled",
            ControlStatus::Sweeping { .. } => "sweeping",
            ControlStatus::Calibrated { .. } => "calibrated",
            ControlStatus::Deferred { .. } => "deferred",
            ControlStatus::Blocked { .. } => "blocked",
        }
    }
}

/// One sample: a value, what the device actually took, the photo, and the scores.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Sample {
    /// The value the sweep asked for.
    pub requested: i64,
    /// The value the device actually holds — D3 applies inside sweeps too \[PF:6\], and a
    /// sample labeled with a value the camera never held would poison every comparison
    /// built on it.
    pub applied: i64,
    /// Anything notable about the write.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<WriteWarning>,
    /// The photo, as a path relative to the session directory — so a session directory
    /// relocates as a unit.
    #[schemars(with = "String")]
    pub photo: Utf8PathBuf,
    /// The metric scores.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metrics: BTreeMap<MetricName, f64>,
    /// When it was taken.
    pub captured_at: Stamp,
}

/// One control's whole story within a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ControlSession {
    /// Where it stands.
    pub status: ControlStatus,
    /// Every sample taken, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub samples: Vec<Sample>,
}

impl Default for ControlSession {
    fn default() -> Self {
        ControlSession {
            status: ControlStatus::Untouched,
            samples: Vec::new(),
        }
    }
}

/// A calibration session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Session {
    /// The document version. Present from day one; a foreign version is a typed refusal
    /// (design D9), never a best-effort parse.
    pub schema_version: u32,
    /// The session's identity — a UUIDv7, so directory listings sort chronologically.
    #[schemars(with = "String")]
    pub id: Uuid,
    /// Which camera this was recorded against.
    pub fingerprint: CameraFingerprint,
    /// The task, as the operator described it.
    pub task: String,
    /// The task through the slug transform — the directory name.
    pub task_slug: String,
    /// What the session is trying to achieve, in the operator's words.
    pub goal: String,
    /// Ordered criteria for judging a sample. Recorded because the *selector* needs
    /// them, whether that selector is a human, an agent, or a metric.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub criteria: Vec<String>,
    /// When it started.
    pub created_at: Stamp,
    /// When it last changed.
    pub updated_at: Stamp,
    /// The tool version that wrote it.
    pub tool_version: String,
    /// The control queue, in the order the operator wants them calibrated. Reorderable
    /// between sweeps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queue: Vec<ControlSlug>,
    /// Per-control state.
    #[serde(default)]
    pub controls: BTreeMap<ControlSlug, ControlSession>,
    /// Free-text notes. v1 does not pretend to model that focus and exposure interact;
    /// it lets the operator write it down.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// The automation pairs this session guards its writes with (design D3).
    ///
    /// Persisted rather than re-derived, and that is the whole point of the field: a
    /// process picking a crashed session back up has to put the camera away in the same
    /// order the sweep took it apart (D4's automation-before-manual), and re-running the
    /// discovery probe to find that out would move the camera *during* the recovery. The
    /// declared table merged with what a probe measured on this device, measured winning
    /// (E1) — so what is stored here is the answer, not the ingredients.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pairs: Vec<AutomationPair>,
    /// The control state as found, persisted **before** the first write so a crashed
    /// sweep is recoverable (design §6, gate G3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_snapshot: Option<Snapshot>,
}

impl Session {
    /// The controls that have a chosen value.
    #[must_use]
    pub fn calibrated(&self) -> Vec<(&ControlSlug, i64)> {
        self.controls
            .iter()
            .filter_map(|(slug, cs)| match &cs.status {
                ControlStatus::Calibrated { value, .. } => Some((slug, *value)),
                _ => None,
            })
            .collect()
    }

    /// Whether every queued control has reached a terminal state.
    #[must_use]
    pub fn is_settled(&self) -> bool {
        self.queue.iter().all(|slug| {
            matches!(
                self.controls.get(slug).map(|c| &c.status),
                Some(
                    ControlStatus::Calibrated { .. }
                        | ControlStatus::Deferred { .. }
                        | ControlStatus::Blocked { .. }
                )
            )
        })
    }
}

/// Something that happened, for `log.ndjson`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum SessionEvent {
    /// The session was created.
    Started {
        /// The goal recorded at creation.
        goal: String,
    },
    /// The pre-sweep snapshot was persisted.
    SnapshotTaken {
        /// How many controls it holds.
        controls: usize,
    },
    /// The automation pairs were probed empirically (design D3 layer 2, PF:3).
    ///
    /// A log line because the probe is a *write*: it toggles automation controls and puts
    /// them back, so a session's history that omitted it would omit the first thing that
    /// ever moved the camera. `skipped` is on the record for the reason the probe reports
    /// it at all — a probe silent about what it passed over reads as a probe that found
    /// nothing there.
    PairsDiscovered {
        /// How many pairs this device demonstrated.
        measured: usize,
        /// How many automation-shaped controls the probe declined to toggle.
        skipped: usize,
    },
    /// An automation control was switched off.
    AutomationDisabled {
        /// The manual control this was for.
        manual: ControlSlug,
        /// The automation control switched off.
        automation: ControlSlug,
    },
    /// A sweep began.
    SweepStarted {
        /// Which control.
        control: ControlSlug,
        /// How many samples are planned.
        total: u32,
    },
    /// A sample was recorded.
    SampleTaken {
        /// Which control.
        control: ControlSlug,
        /// The requested value.
        requested: i64,
        /// The applied value.
        applied: i64,
    },
    /// A sweep finished.
    SweepFinished {
        /// Which control.
        control: ControlSlug,
        /// How many samples were actually taken.
        samples: u32,
    },
    /// A value was chosen.
    Selected {
        /// Which control.
        control: ControlSlug,
        /// The chosen value.
        value: i64,
        /// Who chose it.
        selector: Selector,
    },
    /// The session's values were applied to a camera.
    Applied {
        /// How many controls were written.
        controls: usize,
    },
    /// The camera was put back as found.
    Restored {
        /// How many controls came back exactly.
        restored: usize,
        /// How many did not.
        unrestored: usize,
    },
}

/// One line of `log.ndjson`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LogEntry {
    /// When it happened.
    pub at: Stamp,
    /// What happened.
    #[serde(flatten)]
    pub event: SessionEvent,
}

/// The directory component a task gets in the session tree (design D9's `<task-slug>`).
///
/// One home, because two callers need it and they must agree: [`new_session`] stamps it
/// onto the document, and the store derives the *path* from the stored value — running
/// this over an already-derived slug is the identity, and running it over a
/// hand-edited one (`../../etc`) is what keeps a session file from naming a directory
/// outside the session tree.
///
/// `task` is the operator's free text, so it may slug to nothing; a task still needs a
/// directory, and `task` is that fallback.
#[must_use]
pub fn task_slug(task: &str) -> String {
    use crate::slug::{Separator, slugify};

    let s = slugify(task, Separator::Hyphen);
    if s.is_empty() { "task".to_owned() } else { s }
}

/// Build an empty session for a camera and task.
#[must_use]
pub fn new_session(
    id: Uuid,
    fingerprint: CameraFingerprint,
    task: &str,
    goal: &str,
    tool_version: &str,
    now: Stamp,
) -> Session {
    let task_slug = task_slug(task);
    Session {
        schema_version: limits::SESSION_SCHEMA_VERSION,
        id,
        fingerprint,
        task: task.to_owned(),
        task_slug,
        goal: goal.to_owned(),
        criteria: Vec::new(),
        created_at: now,
        updated_at: now,
        tool_version: tool_version.to_owned(),
        queue: Vec::new(),
        controls: BTreeMap::new(),
        notes: Vec::new(),
        pairs: Vec::new(),
        pre_snapshot: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fingerprint() -> CameraFingerprint {
        CameraFingerprint {
            bus_path: "3-1:1.0".to_owned(),
            usb_id: None,
            card: "OBSBOT Tiny 3".to_owned(),
            driver: "uvcvideo".to_owned(),
            serial: None,
        }
    }

    fn slug(s: &str) -> ControlSlug {
        ControlSlug::parse(s).expect("literal slug")
    }

    #[test]
    fn a_new_session_carries_its_schema_version_from_day_one() {
        let s = new_session(
            Uuid::nil(),
            fingerprint(),
            "Read text from the DUT display",
            "legible text",
            "0.1.0",
            Stamp::epoch(),
        );
        assert_eq!(s.schema_version, limits::SESSION_SCHEMA_VERSION);
        assert_eq!(s.task_slug, "read-text-from-the-dut-display");
    }

    #[test]
    fn a_task_that_slugs_to_nothing_still_gets_a_directory_name() {
        let s = new_session(
            Uuid::nil(),
            fingerprint(),
            "???",
            "",
            "0.1.0",
            Stamp::epoch(),
        );
        assert_eq!(s.task_slug, "task");
    }

    #[test]
    fn a_session_round_trips_through_json_with_every_status_shape() {
        let mut s = new_session(
            Uuid::nil(),
            fingerprint(),
            "focus",
            "sharp text",
            "0.1.0",
            Stamp::epoch(),
        );
        s.queue = vec![slug("focus_absolute"), slug("privacy"), slug("brightness")];
        s.controls.insert(
            slug("focus_absolute"),
            ControlSession {
                status: ControlStatus::Calibrated {
                    value: 42,
                    precision: 5,
                    score: Some(1234.5),
                    selector: Selector::Metric {
                        name: MetricName::Sharpness,
                    },
                },
                samples: vec![Sample {
                    requested: 42,
                    applied: 40,
                    warnings: vec![WriteWarning::StepAligned {
                        requested: 42,
                        applied: 40,
                        step: 5,
                    }],
                    photo: "photos/focus_absolute/42.jpg".into(),
                    metrics: BTreeMap::from([(MetricName::Sharpness, 1234.5)]),
                    captured_at: Stamp::epoch(),
                }],
            },
        );
        s.controls.insert(
            slug("privacy"),
            ControlSession {
                status: ControlStatus::Blocked {
                    reason: BlockedReason::ReadOnly,
                },
                samples: Vec::new(),
            },
        );
        s.controls.insert(
            slug("brightness"),
            ControlSession {
                status: ControlStatus::Sweeping {
                    plan: SweepSpec::Uniform { step: 8 },
                    done: 3,
                    total: 32,
                },
                samples: Vec::new(),
            },
        );

        let json = serde_json::to_string_pretty(&s).expect("serialize");
        let back: Session = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, s);

        assert_eq!(s.calibrated(), vec![(&slug("focus_absolute"), 42)]);
        assert!(!s.is_settled(), "a sweeping control is not settled");
    }

    #[test]
    fn a_settled_session_has_every_queued_control_in_a_terminal_state() {
        let mut s = new_session(
            Uuid::nil(),
            fingerprint(),
            "t",
            "g",
            "0.1.0",
            Stamp::epoch(),
        );
        s.queue = vec![slug("a"), slug("b")];
        s.controls.insert(
            slug("a"),
            ControlSession {
                status: ControlStatus::Calibrated {
                    value: 1,
                    precision: 1,
                    score: None,
                    selector: Selector::Human,
                },
                samples: Vec::new(),
            },
        );
        assert!(
            !s.is_settled(),
            "a queued control with no record is not settled"
        );
        s.controls.insert(
            slug("b"),
            ControlSession {
                status: ControlStatus::Deferred {
                    reason: "no lens".to_owned(),
                },
                samples: Vec::new(),
            },
        );
        assert!(s.is_settled());
    }

    #[test]
    fn a_sample_records_what_the_device_took_not_only_what_was_asked() {
        // PF:6 inside a sweep: the sample is labeled 40, because that is what the camera
        // held when the photo was taken.
        let sample = Sample {
            requested: 42,
            applied: 40,
            warnings: Vec::new(),
            photo: "photos/focus_absolute/42.jpg".into(),
            metrics: BTreeMap::new(),
            captured_at: Stamp::epoch(),
        };
        let json = serde_json::to_value(&sample).expect("serialize");
        assert_eq!(json["requested"], 42);
        assert_eq!(json["applied"], 40);
    }

    #[test]
    fn log_entries_flatten_so_ndjson_stays_greppable() {
        let entry = LogEntry {
            at: Stamp::epoch(),
            event: SessionEvent::SweepStarted {
                control: slug("focus_absolute"),
                total: 20,
            },
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(json.contains("\"event\":\"sweep_started\""), "{json}");
        assert!(json.contains("\"control\":\"focus_absolute\""), "{json}");
        let back: LogEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, entry);
    }
}
