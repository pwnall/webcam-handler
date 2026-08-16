//! Snapshot and restore (design D4): leave the camera as you found it.
//!
//! Restore order is load-bearing. Automation controls go back first, then manual ones —
//! otherwise re-enabling automation immediately overwrites the manual value we just
//! restored, and the camera ends up in a state that is neither where it started nor
//! where we left it. Controls the device reported INACTIVE get a second pass, after
//! their automation partner has been dealt with.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::camera::CameraFingerprint;
use crate::control::{Applied, ControlId, ControlSlug, ControlValue};
use crate::time::Stamp;

/// What part a control plays in restore ordering.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ControlRole {
    /// An automation control — restored first, so it is in place before the manual
    /// values it governs.
    Automation,
    /// Everything else.
    Manual,
}

/// One control's recorded value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SnapshotEntry {
    /// Which control.
    pub control: ControlSlug,
    /// Its kernel id, so restore does not depend on the slug transform being stable
    /// across versions.
    pub id: ControlId,
    /// The value at snapshot time.
    pub value: ControlValue,
    /// Its role in restore ordering, decided from the pairing set when the snapshot was
    /// taken.
    pub role: ControlRole,
    /// Whether the device reported it INACTIVE at snapshot time \[PF:3\] — such a control
    /// is restored on the second pass.
    pub was_inactive: bool,
    /// Whether the device reported it VOLATILE. A volatile value cannot be meaningfully
    /// restored, and restore says so rather than pretending it did.
    pub was_volatile: bool,
}

/// Every writable control's value at one instant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Snapshot {
    /// When it was taken.
    pub taken_at: Stamp,
    /// Which camera it came from — restoring a snapshot onto a different camera is a
    /// [`crate::Error::FingerprintMismatch`], not a surprise.
    pub camera: CameraFingerprint,
    /// The recorded values.
    pub entries: Vec<SnapshotEntry>,
}

impl Snapshot {
    /// The entries in restore order: automation first, then manual, and within each
    /// group the controls that were active before those that were INACTIVE.
    ///
    /// Stable and total — every entry appears exactly once. The property is what the
    /// reordering test checks: shuffling the input must not change the output.
    #[must_use]
    pub fn restore_order(&self) -> Vec<&SnapshotEntry> {
        let mut out: Vec<&SnapshotEntry> = self.entries.iter().collect();
        out.sort_by_key(|e| {
            (
                match e.role {
                    ControlRole::Automation => 0u8,
                    ControlRole::Manual => 1,
                },
                u8::from(e.was_inactive),
                e.control.as_str().to_owned(),
            )
        });
        out
    }
}

/// Why one control could not be put back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UnrestorableReason {
    /// The control has been taken over by automation that did **not** hold it when the
    /// snapshot was taken.
    ///
    /// A real change we could not undo, and distinct from
    /// [`RestoreOutcome::OwnedByAutomation`], which is the same *flag* meaning the
    /// opposite thing: there, the owner is the one that was there before.
    StillInactive {
        /// The automation partner, when one is known.
        automation: Option<ControlSlug>,
    },
    /// The control reports VOLATILE: its value is the device's to choose.
    Volatile,
    /// The control has gone read-only, or vanished, since the snapshot.
    NoLongerWritable,
    /// The write failed. The error rides along so the caller can act on it.
    WriteFailed {
        /// What went wrong.
        error: crate::Error,
    },
}

/// One control's restore outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum RestoreOutcome {
    /// Written, with the usual `{requested, applied}` pair (D3 applies inside restore
    /// too — a restore can be clamped like any other write).
    Restored {
        /// What was written and what stuck.
        applied: Applied,
    },
    /// Already holding the recorded value; nothing was written.
    AlreadyCorrect {
        /// Which control.
        control: ControlSlug,
    },
    /// The automation that owned this control when the snapshot was taken owns it again,
    /// so its value is that automation's to choose — exactly as it was.
    ///
    /// **A success**, and the distinction is not pedantry. Restoring an automation control
    /// to "on" re-engages the manual control it governs, so on any device whose INACTIVE
    /// flag follows the automation's value this is the *ordinary* outcome for every
    /// guarded write's restore. Measured on the seed hardware the day
    /// `controls --discover-pairs` first ran: the probe put the camera back perfectly and
    /// then reported two controls it "could not put back", because the vocabulary had no
    /// way to say "its owner is back". See note N9.
    OwnedByAutomation {
        /// Which control.
        control: ControlSlug,
        /// The automation control that holds it, when the pair set names one.
        automation: Option<ControlSlug>,
    },
    /// Could not be put back, and why.
    Unrestorable {
        /// Which control.
        control: ControlSlug,
        /// The reason.
        reason: UnrestorableReason,
    },
}

/// What a whole restore did — to the camera, and to the session document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RestoreReport {
    /// One entry per snapshotted control, in the order they were attempted.
    pub outcomes: Vec<RestoreOutcome>,
    /// The controls a dead sweep had stranded, given back by this call (note **N139**).
    ///
    /// **A restore repairs the session as well as the camera, and this is the half that has
    /// nothing to do with the snapshot** (note **N150**). A process killed between
    /// `begin_sweep` and its first sample leaves a control in `Sweeping { done: 0 }`, which
    /// every shipped verb refuses and no snapshot describes; `calibrate restore` puts it back
    /// to `Untouched` and names it here. Empty is the ordinary answer and means nothing was
    /// stranded — not that nothing was looked at.
    ///
    /// It is on the report rather than only in `log.ndjson` because `calibrate restore
    /// --json` answers *this* document and exits 0. A run that changed a control's status on
    /// disk and answered `{"outcomes":[]}` would be telling an unattended caller that nothing
    /// happened (rubric A8).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub freed: Vec<ControlSlug>,
}

impl RestoreReport {
    /// Whether the camera is back where the snapshot found it.
    ///
    /// [`RestoreOutcome::OwnedByAutomation`] counts as complete: the control's owner is
    /// the one that owned it before, so the camera *is* in the configuration that was
    /// recorded, even though no value was written. Counting it as incomplete would make
    /// every guarded write's restore look like a failure, which is how a field stops being
    /// read (note N9).
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.outcomes.iter().all(|o| match o {
            RestoreOutcome::Restored { applied } => applied.is_exact(),
            RestoreOutcome::AlreadyCorrect { .. } | RestoreOutcome::OwnedByAutomation { .. } => {
                true
            }
            RestoreOutcome::Unrestorable { .. } => false,
        })
    }

    /// The controls that did not come back.
    #[must_use]
    pub fn unrestored(&self) -> Vec<&ControlSlug> {
        self.outcomes
            .iter()
            .filter_map(|o| match o {
                RestoreOutcome::Unrestorable { control, .. } => Some(control),
                RestoreOutcome::Restored { applied } if !applied.is_exact() => Some(&applied.slug),
                RestoreOutcome::Restored { .. }
                | RestoreOutcome::AlreadyCorrect { .. }
                | RestoreOutcome::OwnedByAutomation { .. } => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(slug: &str, role: ControlRole, inactive: bool) -> SnapshotEntry {
        SnapshotEntry {
            control: ControlSlug::parse(slug).expect("literal slug"),
            id: ControlId(1),
            value: ControlValue::Int(1),
            role,
            was_inactive: inactive,
            was_volatile: false,
        }
    }

    fn snapshot(entries: Vec<SnapshotEntry>) -> Snapshot {
        Snapshot {
            taken_at: Stamp::epoch(),
            camera: CameraFingerprint {
                bus_path: "3-1:1.0".to_owned(),
                usb_id: None,
                card: "Test".to_owned(),
                driver: "uvcvideo".to_owned(),
                serial: None,
            },
            entries,
        }
    }

    #[test]
    fn automation_controls_are_restored_before_manual_ones() {
        // The buggy reordering this test exists to catch: restoring
        // `white_balance_temperature` before `white_balance_automatic` lets the
        // re-enabled automation immediately overwrite it.
        let snap = snapshot(vec![
            entry("white_balance_temperature", ControlRole::Manual, true),
            entry("brightness", ControlRole::Manual, false),
            entry("white_balance_automatic", ControlRole::Automation, false),
        ]);
        let order: Vec<&str> = snap
            .restore_order()
            .iter()
            .map(|e| e.control.as_str())
            .collect();
        assert_eq!(
            order,
            vec![
                "white_balance_automatic",
                "brightness",
                "white_balance_temperature"
            ]
        );
    }

    #[test]
    fn restore_order_is_stable_under_input_shuffling_and_loses_nothing() {
        let entries = vec![
            entry("zoom_absolute", ControlRole::Manual, false),
            entry("auto_exposure", ControlRole::Automation, false),
            entry("exposure_time_absolute", ControlRole::Manual, true),
            entry("brightness", ControlRole::Manual, false),
        ];
        let forward = snapshot(entries.clone());
        let mut reversed = entries;
        reversed.reverse();
        let backward = snapshot(reversed);

        let a: Vec<&str> = forward
            .restore_order()
            .iter()
            .map(|e| e.control.as_str())
            .collect();
        let b: Vec<&str> = backward
            .restore_order()
            .iter()
            .map(|e| e.control.as_str())
            .collect();
        assert_eq!(a, b, "restore order must not depend on snapshot order");
        assert_eq!(a.len(), 4, "restore order must be total");
    }

    #[test]
    fn a_report_with_an_unrestorable_control_is_not_complete() {
        let report = RestoreReport {
            outcomes: vec![
                RestoreOutcome::AlreadyCorrect {
                    control: ControlSlug::parse("brightness").expect("literal slug"),
                },
                RestoreOutcome::Unrestorable {
                    control: ControlSlug::parse("focus_absolute").expect("literal slug"),
                    reason: UnrestorableReason::StillInactive { automation: None },
                },
            ],
            freed: Vec::new(),
        };
        assert!(!report.is_complete());
        assert_eq!(report.unrestored().len(), 1);

        // The inverse: an all-clean report is complete.
        let clean = RestoreReport {
            outcomes: vec![RestoreOutcome::AlreadyCorrect {
                control: ControlSlug::parse("brightness").expect("literal slug"),
            }],
            freed: Vec::new(),
        };
        assert!(clean.is_complete());
        assert!(clean.unrestored().is_empty());
    }

    #[test]
    fn a_clamped_restore_counts_as_incomplete() {
        // A restore is a write, so D3 applies: the driver can clamp it, and a clamped
        // restore did not put the camera back.
        let applied = Applied {
            control: ControlId(1),
            slug: ControlSlug::parse("white_balance_temperature").expect("literal slug"),
            requested: ControlValue::Int(9_000),
            applied: ControlValue::Int(6_500),
            warnings: Vec::new(),
        };
        let report = RestoreReport {
            outcomes: vec![RestoreOutcome::Restored { applied }],
            freed: Vec::new(),
        };
        assert!(!report.is_complete());
        assert_eq!(report.unrestored().len(), 1);
    }
}
