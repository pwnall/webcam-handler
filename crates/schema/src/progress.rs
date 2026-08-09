//! The calibration progress stream (design D8; docs/7 P3c and P4e).
//!
//! A sweep is the one operation in this tool that takes minutes: a photo per value, each
//! one waiting for a sensor to settle \[PF:11\]. Something has to say what it is doing
//! while it does it, and there are two consumers with one answer between them — `wch`
//! renders a progress bar from it now, and P4e's `subscribe_calibration` puts it on the
//! wire.
//!
//! **That is why this lives in the schema crate and not in the engine.** docs/7's risk
//! register states the consequence in as many words: *if the hook is not schema-shaped
//! from the start, P4 re-plumbs it.* A callback shaped for a terminal — a closure, a
//! `&dyn Fn(&str)`, a pre-rendered line — is a shape only a terminal can consume, and the
//! phase that needs to serialize it would have to invent the type these events already
//! are. So an event is a DTO with the derives every other DTO carries, and the transport
//! is somebody else's problem in both directions.
//!
//! ## What it is not
//!
//! Not `log.ndjson`. [`crate::session::SessionEvent`] is the *durable* record of what
//! happened to the device, and it is written under the store's lock, atomically, in a
//! file an operator reads afterwards. This is the *live* stream: it is allowed to be
//! dropped when nobody is listening, it carries per-sample detail the log deliberately
//! does not (metrics, photo paths), and nothing reads it back. Two vocabularies because
//! they answer to two different failures — a lost progress line costs a repaint, and a
//! lost log line costs the history.

use std::collections::BTreeMap;

use camino::Utf8PathBuf;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::control::{ControlSlug, WriteWarning};
use crate::error::ErrorKind;
use crate::metrics::MetricName;
use crate::session::SweepSpec;
use crate::time::Stamp;

/// What a calibration sweep is doing, as it does it.
///
/// The `index`/`total` pair rides on every in-flight variant rather than being tracked by
/// the consumer, because a subscriber that connects mid-sweep (P4e) has no earlier events
/// to count and a progress bar that starts at zero halfway through a sweep is a lie the
/// transport would have introduced.
// No `Eq`: `SampleTaken` carries metric scores, and `f64` has no total equality.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "progress", rename_all = "snake_case")]
pub enum CalibrationProgress {
    /// A sweep began, and this is how big it is.
    SweepStarted {
        /// Which control.
        control: ControlSlug,
        /// The plan it is executing.
        plan: SweepSpec,
        /// How many samples it will take.
        total: u32,
    },
    /// The camera has been moved to the next value; the sensor is settling.
    ///
    /// Emitted before the capture rather than after it because the capture is the slow
    /// part: a bar that only advances on [`CalibrationProgress::SampleTaken`] sits still
    /// for the seconds that actually pass. It carries `{requested, applied}` for the same
    /// reason every other layer does — the driver may have taken something else \[PF:6\],
    /// and a viewer watching a sweep should see that when it happens, not when the
    /// session file is read afterwards.
    ValueSet {
        /// Which control.
        control: ControlSlug,
        /// Which sample this is, counting from one.
        index: u32,
        /// How many the sweep will take.
        total: u32,
        /// The value the sweep asked for.
        requested: i64,
        /// The value the device actually holds.
        applied: i64,
        /// Anything notable about the write.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        warnings: Vec<WriteWarning>,
    },
    /// A photo was taken and scored.
    SampleTaken {
        /// Which control.
        control: ControlSlug,
        /// Which sample this is, counting from one.
        index: u32,
        /// How many the sweep will take.
        total: u32,
        /// The value the sweep asked for — which is what the photo is named after.
        requested: i64,
        /// The value the device held when the shutter fell \[PF:6\].
        applied: i64,
        /// The photo, relative to the session directory (D9).
        #[schemars(with = "String")]
        photo: Utf8PathBuf,
        /// What the metrics made of it. Metrics *rank*; they do not decide (D8).
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        metrics: BTreeMap<MetricName, f64>,
    },
    /// Every planned sample was taken.
    ///
    /// Note what it does **not** say: nothing was selected. A sweep produces a ranking and
    /// stops there, and the `Selected` fact belongs to whoever chose (D8).
    SweepFinished {
        /// Which control.
        control: ControlSlug,
        /// How many samples it took.
        samples: u32,
    },
    /// The sweep stopped before its plan did.
    ///
    /// The samples already recorded stand — they happened — so `taken` is the count on
    /// disk and the control is left mid-sweep rather than forced into a terminal state.
    SweepInterrupted {
        /// Which control.
        control: ControlSlug,
        /// How many samples were recorded before it stopped.
        taken: u32,
        /// How many the plan held.
        total: u32,
        /// The refusal's discriminant, so a consumer can branch without parsing prose.
        failure: ErrorKind,
        /// The refusal, rendered.
        detail: String,
    },
}

impl CalibrationProgress {
    /// The control this event is about.
    ///
    /// Every variant has one, and the exhaustive `match` is what keeps that true: an event
    /// about no control would be an event a per-control view could not place.
    #[must_use]
    pub const fn control(&self) -> &ControlSlug {
        match self {
            CalibrationProgress::SweepStarted { control, .. }
            | CalibrationProgress::ValueSet { control, .. }
            | CalibrationProgress::SampleTaken { control, .. }
            | CalibrationProgress::SweepFinished { control, .. }
            | CalibrationProgress::SweepInterrupted { control, .. } => control,
        }
    }

    /// The event name, for output and for tests that assert a sequence.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            CalibrationProgress::SweepStarted { .. } => "sweep_started",
            CalibrationProgress::ValueSet { .. } => "value_set",
            CalibrationProgress::SampleTaken { .. } => "sample_taken",
            CalibrationProgress::SweepFinished { .. } => "sweep_finished",
            CalibrationProgress::SweepInterrupted { .. } => "sweep_interrupted",
        }
    }

    /// Whether no further event for this control follows from this sweep.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            CalibrationProgress::SweepFinished { .. }
                | CalibrationProgress::SweepInterrupted { .. }
        )
    }
}

/// One progress event, addressed to a session and stamped.
///
/// The session id is on every event rather than established once at subscription time,
/// because P4e's subscription is per *client* and a client may watch a daemon running
/// more than one session. A stream whose events had to be correlated by arrival order
/// would be a stream that could not be multiplexed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProgressEvent {
    /// Which session.
    #[schemars(with = "String")]
    pub session: Uuid,
    /// When it happened.
    pub at: Stamp,
    /// What happened.
    #[serde(flatten)]
    pub progress: CalibrationProgress,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slug(s: &str) -> ControlSlug {
        ControlSlug::parse(s).expect("literal slug")
    }

    fn event(progress: CalibrationProgress) -> ProgressEvent {
        ProgressEvent {
            session: Uuid::nil(),
            at: Stamp::epoch(),
            progress,
        }
    }

    /// Every variant, so the round-trip below is a claim about the vocabulary rather than
    /// about whichever variant somebody happened to type out.
    fn every_variant() -> Vec<CalibrationProgress> {
        vec![
            CalibrationProgress::SweepStarted {
                control: slug("focus_absolute"),
                plan: SweepSpec::Uniform { step: 64 },
                total: 16,
            },
            CalibrationProgress::ValueSet {
                control: slug("focus_absolute"),
                index: 1,
                total: 16,
                requested: 42,
                applied: 40,
                warnings: vec![WriteWarning::StepAligned {
                    requested: 42,
                    applied: 40,
                    step: 5,
                }],
            },
            CalibrationProgress::SampleTaken {
                control: slug("focus_absolute"),
                index: 1,
                total: 16,
                requested: 42,
                applied: 40,
                photo: "photos/focus_absolute/42.jpg".into(),
                metrics: BTreeMap::from([(MetricName::Sharpness, 1234.5)]),
            },
            CalibrationProgress::SweepFinished {
                control: slug("focus_absolute"),
                samples: 16,
            },
            CalibrationProgress::SweepInterrupted {
                control: slug("focus_absolute"),
                taken: 3,
                total: 16,
                failure: ErrorKind::DeviceGone,
                detail: "/dev/video0 disappeared".to_owned(),
            },
        ]
    }

    #[test]
    fn every_progress_event_round_trips_through_json() {
        // The whole reason this type lives in the schema crate: P4e serializes it, and a
        // shape that only survived in memory would be re-plumbed there.
        for progress in every_variant() {
            let event = event(progress);
            let json = serde_json::to_string(&event).expect("serialize");
            assert!(
                json.contains(&format!("\"progress\":\"{}\"", event.progress.name())),
                "{json}"
            );
            assert!(json.contains("\"session\":"), "{json}");
            let back: ProgressEvent = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, event);
        }
    }

    #[test]
    fn exactly_the_two_ending_events_are_terminal_and_every_event_names_its_control() {
        let terminal: Vec<&'static str> = every_variant()
            .iter()
            .filter(|progress| progress.is_terminal())
            .map(CalibrationProgress::name)
            .collect();
        assert_eq!(terminal, vec!["sweep_finished", "sweep_interrupted"]);
        // Both directions: the in-flight ones are not terminal, so a consumer closing on
        // `is_terminal` does not close on the first sample.
        assert!(
            every_variant()
                .iter()
                .filter(|progress| !progress.is_terminal())
                .count()
                == 3
        );
        for progress in every_variant() {
            assert_eq!(progress.control().as_str(), "focus_absolute");
        }
    }
}
