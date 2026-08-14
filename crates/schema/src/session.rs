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
use crate::capture::{PhotoFormat, SettlePolicy, StreamRequest};
use crate::control::{ControlSlug, WriteWarning};
use crate::error::ErrorKind;
use crate::limits;
use crate::metrics::MetricName;
use crate::pairing::AutomationPair;
use crate::snapshot::Snapshot;
use crate::time::Stamp;
use crate::vocabulary::closed_vocabulary;

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

/// Everything one sweep needs that is not the session, the camera, or the clock.
///
/// A DTO rather than an engine struct because three callers need the same request and two of
/// them cannot see the engine: `webcam-handler-cli` parses one from a command line through the
/// shared command surface (T4), P4's `calibrate_sweep` takes one off the wire (D10), and
/// `webcam-handler-engine::calibrate::run` executes it. The engine re-exports this type rather
/// than defining a second one — a request that had to be translated at each seam is a request
/// three places can disagree about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SweepRequest {
    /// Which control to sweep.
    pub control: ControlSlug,
    /// How to derive the values.
    pub plan: SweepSpec,
    /// Whether a motorized control may be swept at all (design §5: never implicit).
    #[serde(default)]
    pub allow_motion: bool,
    /// What to ask the device's format negotiation for, once per sample.
    #[serde(default)]
    pub stream: StreamRequest,
    /// How long to let the sensor settle before each shot \[PF:11\].
    #[serde(default)]
    pub settle: SettlePolicy,
    /// Which encoding the sample photos are written in.
    #[serde(default = "default_photo_format")]
    pub photo_format: PhotoFormat,
}

/// JPEG: the verbatim path when the camera speaks MJPG (E6), which is what makes a sample
/// photo the camera's own bytes rather than our idea of them.
const fn default_photo_format() -> PhotoFormat {
    PhotoFormat::Jpeg
}

impl SweepRequest {
    /// A sweep of `control` under `plan`, with the defaults every other field has.
    ///
    /// The defaults are the schema's own ([`StreamRequest`], [`SettlePolicy`]), so a
    /// request built here and one parsed from `{}` describe the same sweep. `allow_motion`
    /// is `false` because design §5 says a plan that would move motors says so first.
    #[must_use]
    pub fn new(control: ControlSlug, plan: SweepSpec) -> SweepRequest {
        SweepRequest {
            control,
            plan,
            allow_motion: false,
            stream: StreamRequest::default(),
            settle: SettlePolicy::default(),
            photo_format: default_photo_format(),
        }
    }
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

impl Selector {
    /// The selector as design D8 spells it: `metric:<name>`, `agent`, or `human`.
    ///
    /// One home for the spelling, because three surfaces show it — the `calibrate status`
    /// table, the `Selected` log line a human greps for, and the refusal a caller reads
    /// when a metric cannot rank — and a selector that read `Metric { name: Sharpness }`
    /// in one place and `metric:sharpness` in another would be two vocabularies.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Selector::Metric { name } => format!("metric:{name}"),
            Selector::Agent => "agent".to_owned(),
            Selector::Human => "human".to_owned(),
        }
    }
}

closed_vocabulary! {
    /// Who a caller may claim to be when recording a chosen value (design D8).
    ///
    /// Two spellings, and the third selector D8 names — `metric:<name>` — is deliberately
    /// not among them: a metric is not something a caller can *claim* to be, it is
    /// something the tool computed and scored. On a command line clap's group enforced
    /// that (`--metric` conflicts with `--by`); on the wire nothing would, so this
    /// vocabulary makes the lie unrepresentable instead of merely refusable.
    ///
    /// Generated, so [`ChosenBy::selector`]'s exhaustive match forces a third claimant to
    /// be *mapped* rather than quietly accepted, and `ALL` is the whole accepted
    /// vocabulary for both the flag parser and the wire.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum ChosenBy {
        /// An agent reviewed the sample photos and picked one.
        Agent,
        /// A human did.
        Human,
    }
}

impl ChosenBy {
    /// The selector this claim records.
    ///
    /// Exhaustive, so a third claimant has to be given a [`Selector`] rather than
    /// defaulting into one — and it goes *through* `Selector` rather than carrying its own
    /// spelling, because [`Selector::label`] is the one home for how a selector is spelled.
    #[must_use]
    pub const fn selector(self) -> Selector {
        match self {
            ChosenBy::Agent => Selector::Agent,
            ChosenBy::Human => Selector::Human,
        }
    }
}

/// What `calibrate select` was told to do (design D8).
///
/// Lives here rather than in the shared command surface because both surfaces need it and
/// only one of them may name the other: `webcam-handler-cli-core` is behind the T6 purity
/// wall and can never depend on `webcam-handler-api`, so a selection the wire carries and
/// a selection a command line parses have to be one type or two that drift.
///
/// Struct variants rather than newtypes because this crate's enums are internally tagged
/// and serde cannot internally tag a newtype variant wrapping a scalar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Selection {
    /// Rank the samples by this metric and take the best.
    ByMetric {
        /// Which metric.
        metric: MetricName,
    },
    /// A value somebody chose, and who.
    ByValue {
        /// The value, as a sample's *applied* value — the one the camera actually held
        /// \[PF:6\]. Selecting a requested value would name a value no photo was taken at.
        value: i64,
        /// Who chose it. A [`ChosenBy`] rather than a [`Selector`], so a caller cannot
        /// record "a metric chose 42" without any metric having ranked anything.
        chosen_by: ChosenBy,
    },
}

/// Which session a verb is about (design D8, D9).
///
/// The two forms are not interchangeable and the type keeps them apart: a task names the
/// session for *this* camera, and a UUID names one session wherever it lives — including
/// under another camera, which is the case `calibrate apply`'s fingerprint check exists
/// for.
///
/// Here rather than in the command surface for the reason [`Selection`] is: the wire and
/// the command line name the same session, and `webcam-handler-cli-core` cannot see
/// `webcam-handler-api` (T6). Struct variants, and the `id` is a string in the emitted
/// schema, matching what [`Session::id`] already does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionRef {
    /// The newest session recorded for this camera and task.
    Task {
        /// The task, in the words `calibrate start` recorded.
        task: String,
    },
    /// The session with this id, whichever camera it belongs to.
    Id {
        /// The session's UUIDv7.
        #[schemars(with = "String")]
        id: Uuid,
    },
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
    /// A sweep stopped before its plan did, and why.
    ///
    /// The one event here whose subject is a *refusal*, and it earns its place by what a
    /// session directory looks like without it. The recorded `SampleTaken` lines bound
    /// **when** a sweep stopped — three samples of sixteen and then nothing — and say
    /// nothing at all about **why**: a camera that was unplugged, a sensor that never
    /// settled \[PF:11\], and a full disk leave byte-identical histories, and design's own
    /// rule that availability is not capability is exactly the distinction that would be
    /// lost. The live [`crate::progress::CalibrationProgress::SweepInterrupted`] carries
    /// the same fact to whoever was watching; this is the copy that survives the process,
    /// because `calibrate status` is read after the terminal is gone.
    ///
    /// Its absence is not evidence of anything: a process that was killed leaves no line
    /// either (design §6's crash story is the pre-sweep snapshot, not the log). A present
    /// line means a known reason; an absent one means the reason is not known here.
    SweepInterrupted {
        /// Which control.
        control: ControlSlug,
        /// How many samples were recorded before it stopped.
        taken: u32,
        /// How many the plan held.
        total: u32,
        /// The refusal's discriminant, so a reader can branch without parsing prose.
        failure: ErrorKind,
        /// The refusal, rendered.
        detail: String,
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

/// A session and its history — what `calibrate status` answers.
///
/// Two documents rather than one, and both of them, because they answer different
/// questions and neither can be read off the other. [`Session`] is *where the session
/// stands*: which controls are calibrated, at what value, chosen by whom. The log is
/// *what happened to get there*, and it holds the facts the document by design does not —
/// which automation controls were switched off, and, when a sweep stopped early, the
/// refusal that stopped it ([`SessionEvent::SweepInterrupted`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SessionStatus {
    /// The session document.
    pub session: Session,
    /// Its `log.ndjson`, oldest first, with a torn last line dropped (design D9).
    ///
    /// Always emitted, empty or not: this is an *answer*, and a consumer counting the
    /// history should meet a list with nothing in it rather than a missing key.
    #[serde(default)]
    pub log: Vec<LogEntry>,
}

/// One session in a listing, as the directory tree alone describes it.
///
/// **Nothing here is parsed out of a session document**, and that is the property rather
/// than a shortcut: D9 chose UUIDv7 so a listing sorts chronologically without opening a
/// file, so listing costs no parse, cannot fail on the one session whose document is
/// corrupt, and still shows a session written by a build this one cannot read. Reading a
/// session is [`SessionStatus`]'s job, and it is the one that refuses a foreign version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SessionListing {
    /// The session's identity — the directory's name.
    #[schemars(with = "String")]
    pub id: Uuid,
    /// The camera, as the fingerprint slug that names its directory.
    pub camera: String,
    /// The task, as the slug that names its directory.
    pub task_slug: String,
    /// Where it lives.
    #[schemars(with = "String")]
    pub path: Utf8PathBuf,
}

/// Every session the store holds, newest first — what `calibrate list` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SessionList {
    /// The sessions, newest first by UUIDv7 byte order. Always emitted, empty or not — an
    /// empty store answers with an empty list, not with a missing key.
    #[serde(default)]
    pub sessions: Vec<SessionListing>,
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
    fn a_sweep_request_parsed_from_nothing_but_its_two_required_fields_is_the_default_sweep() {
        // The defaults are the schema's own, so a request built in Rust and one that
        // arrived as `{"control":…,"plan":…}` describe the same sweep — which is what lets
        // `webcam-handler-cli`, P4's wire, and the executor share one type instead of three.
        let parsed: SweepRequest =
            serde_json::from_str(r#"{"control":"focus_absolute","plan":{"kind":"all"}}"#)
                .expect("deserialize");
        assert_eq!(
            parsed,
            SweepRequest::new(slug("focus_absolute"), SweepSpec::All)
        );
        assert!(
            !parsed.allow_motion,
            "motors are never implicit (design §5)"
        );
        assert_eq!(parsed.photo_format, crate::capture::PhotoFormat::Jpeg);

        let json = serde_json::to_string(&parsed).expect("serialize");
        assert_eq!(
            serde_json::from_str::<SweepRequest>(&json).expect("deserialize"),
            parsed
        );
    }

    #[test]
    fn every_selector_has_the_spelling_d8_gives_it_and_no_two_share_one() {
        let selectors = [
            Selector::Agent,
            Selector::Human,
            Selector::Metric {
                name: MetricName::Sharpness,
            },
        ];
        let mut seen = std::collections::BTreeSet::new();
        for selector in &selectors {
            assert!(
                seen.insert(selector.label()),
                "{selector:?} duplicates a spelling"
            );
        }
        assert_eq!(Selector::Agent.label(), "agent");
        assert_eq!(Selector::Human.label(), "human");
        // The metric's own name, so `metric:<name>` is one string rather than two that
        // can drift — and every metric gets a distinct one.
        for &name in MetricName::ALL {
            assert_eq!(
                Selector::Metric { name }.label(),
                format!("metric:{}", name.as_str())
            );
        }
    }

    #[test]
    fn an_interrupted_sweep_records_its_refusal_where_a_later_reader_finds_it() {
        // The durable half of the interruption. `taken`/`total` say where it stopped and
        // `failure` says why — and a device that vanished, a settle that never converged
        // and a full disk are three different answers, which is the distinction that would
        // be lost if the history only held the samples that made it.
        let entry = LogEntry {
            at: Stamp::epoch(),
            event: SessionEvent::SweepInterrupted {
                control: slug("focus_absolute"),
                taken: 3,
                total: 16,
                failure: crate::error::ErrorKind::DeviceGone,
                detail: "/dev/video0 disappeared".to_owned(),
            },
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(json.contains("\"event\":\"sweep_interrupted\""), "{json}");
        assert!(json.contains("\"failure\":\"device_gone\""), "{json}");
        assert_eq!(
            serde_json::from_str::<LogEntry>(&json).expect("deserialize"),
            entry
        );
    }

    #[test]
    fn a_status_document_carries_the_session_and_the_history_that_explains_it() {
        let status = SessionStatus {
            session: new_session(
                Uuid::nil(),
                fingerprint(),
                "focus",
                "sharp text",
                "0.1.0",
                Stamp::epoch(),
            ),
            log: vec![LogEntry {
                at: Stamp::epoch(),
                event: SessionEvent::Started {
                    goal: "sharp text".to_owned(),
                },
            }],
        };
        let json = serde_json::to_string(&status).expect("serialize");
        let back: SessionStatus = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, status);
        // A session with no history yet is a session with an empty log, not a document
        // missing a field a consumer has to special-case.
        let quiet = SessionStatus {
            log: Vec::new(),
            ..status
        };
        assert_eq!(
            serde_json::from_str::<SessionStatus>(
                &serde_json::to_string(&quiet).expect("serialize")
            )
            .expect("deserialize"),
            quiet
        );
    }

    #[test]
    fn a_listing_describes_a_session_with_nothing_a_parse_would_have_told_it() {
        // Every field here is a directory fact. That is what lets a listing show a session
        // whose document this build refuses to load.
        let list = SessionList {
            sessions: vec![SessionListing {
                id: Uuid::nil(),
                camera: "obsbot-tiny-3-3-1-1-0".to_owned(),
                task_slug: "read-text-from-the-dut-display".to_owned(),
                path: "/state/sessions/obsbot/read-text/0".into(),
            }],
        };
        let json = serde_json::to_string(&list).expect("serialize");
        let back: SessionList = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, list);
        assert_eq!(
            serde_json::from_str::<SessionList>("{}").expect("an empty store lists nothing"),
            SessionList {
                sessions: Vec::new()
            }
        );
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

    #[test]
    fn both_ways_of_naming_a_session_round_trip_and_stay_apart() {
        let by_task = SessionRef::Task {
            task: "read the DUT display".to_owned(),
        };
        let json = serde_json::to_string(&by_task).expect("serialize");
        assert_eq!(json, r#"{"kind":"task","task":"read the DUT display"}"#);
        assert_eq!(
            serde_json::from_str::<SessionRef>(&json).expect("deserialize"),
            by_task
        );

        let id = Uuid::nil();
        let by_id = SessionRef::Id { id };
        let json = serde_json::to_string(&by_id).expect("serialize");
        assert_eq!(
            serde_json::from_str::<SessionRef>(&json).expect("deserialize"),
            by_id
        );

        // The claim the type exists to make: the two forms are different questions — one
        // scoped to this camera, one to the whole store — so a document naming one must
        // never deserialize into the other.
        assert_ne!(by_task, by_id);
        assert!(serde_json::from_str::<SessionRef>(r#"{"kind":"task","id":"x"}"#).is_err());
    }

    #[test]
    fn a_selection_records_who_chose_and_cannot_claim_a_metric_did() {
        let by_metric = Selection::ByMetric {
            metric: MetricName::Sharpness,
        };
        let json = serde_json::to_string(&by_metric).expect("serialize");
        assert_eq!(
            serde_json::from_str::<Selection>(&json).expect("deserialize"),
            by_metric
        );

        for &who in ChosenBy::ALL {
            let chosen = Selection::ByValue {
                value: -108_000,
                chosen_by: who,
            };
            let json = serde_json::to_string(&chosen).expect("serialize");
            assert_eq!(
                serde_json::from_str::<Selection>(&json).expect("deserialize"),
                chosen,
                "{who:?} did not survive the wire"
            );
            // One spelling for a selector, wherever it is written: `--by agent`, the
            // `agent` a session file records, and the `"agent"` on the wire.
            assert!(
                json.contains(&format!("\"{}\"", who.selector().label())),
                "{who:?} spells itself differently on the wire: {json}"
            );
        }

        // The inverse, and the reason `ChosenBy` exists rather than `Selector`: a caller
        // cannot record "a metric chose 42" without a metric having ranked anything. D8's
        // honesty property is unrepresentable here rather than refused downstream.
        assert!(
            serde_json::from_str::<Selection>(
                r#"{"kind":"by_value","value":42,"chosen_by":"metric"}"#
            )
            .is_err()
        );
    }
}
