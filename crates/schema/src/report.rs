//! What the read verbs answer with.
//!
//! `--json` emits schema types verbatim (design §2.7), and "verbatim" is only checkable if
//! the shape has a name in the emitted bundle — so each read verb's answer is a type here
//! rather than an object assembled in a renderer. The same types are what the T5 wire
//! surface returns at P4: one home for the shape, two transports over it.
//!
//! These carry no rendering decisions. A table is one view of [`ControlReport`] and
//! `--json` is another, and neither is allowed to know something the other does not.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::camera::{CameraId, CameraInfo, FormatInfo};
use crate::control::ControlDesc;
use crate::vocabulary::closed_vocabulary;

closed_vocabulary! {
    /// Why a listing might not say what the user expected.
    ///
    /// D1: "an empty enumeration is diagnosed, not shrugged at". Hints are *data*, not
    /// prose the CLI prints and the daemon forgets — an agent reading `--json` gets the
    /// same diagnosis a human reading the table does.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum HintKind {
        /// A USB device presents a video-class interface and no driver has bound a V4L2
        /// node to it — the skill's `lsusb` triage, built in \[PF:14\].
        DriverlessUsbVideoDevice,
    }
}

/// One diagnosis attached to a listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ListHint {
    /// What kind of problem this is.
    pub kind: HintKind,
    /// What it is about — a USB device path such as `1-2`.
    pub subject: String,
}

impl ListHint {
    /// The sentence a human reads. Lives here rather than in a renderer so the CLI and
    /// the daemon cannot describe the same finding differently.
    #[must_use]
    pub fn message(&self) -> String {
        match self.kind {
            HintKind::DriverlessUsbVideoDevice => format!(
                "USB device {} presents a video-class interface with no V4L2 driver bound; \
                 the camera is plugged in and nothing is driving it",
                self.subject
            ),
        }
    }
}

/// What `list` answers.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct CameraList {
    /// The cameras, in enumeration order.
    pub cameras: Vec<CameraInfo>,
    /// Anything worth saying about what is *not* in the list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<ListHint>,
}

/// What `info` answers: one camera and the formats its capture node offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CameraDetail {
    /// Identity, nodes, grouping.
    pub info: CameraInfo,
    /// Formats, with sizes and the intervals available at each size nested under them
    /// \[PF:9\]. Empty for a camera with no capture node, which is a real shape.
    pub formats: Vec<FormatInfo>,
}

/// What `controls` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ControlReport {
    /// Which camera these came from — so a saved `--json` document is self-describing.
    pub camera: CameraId,
    /// Every control the device enumerated, including the ones this build cannot
    /// interpret \[PF:1\].
    pub controls: Vec<ControlDesc>,
}

impl ControlReport {
    /// The controls whose declared default or current value sits outside their declared
    /// range \[PF:4, PF:5\].
    ///
    /// Reported rather than corrected, and computed here so the table and `--json` agree
    /// about which rows deserve a mark.
    #[must_use]
    pub fn self_contradicting(&self) -> Vec<&ControlDesc> {
        self.controls
            .iter()
            .filter(|desc| desc.default_out_of_range() || desc.current_out_of_range())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_hint_kind_renders_a_sentence_naming_its_subject() {
        // The population is the generated `ALL`, so a new hint cannot be added without
        // getting a message.
        for &kind in HintKind::ALL {
            let hint = ListHint {
                kind,
                subject: "1-2".to_owned(),
            };
            let message = hint.message();
            assert!(message.contains("1-2"), "{kind:?} renders {message:?}");
            assert!(message.len() > 20, "{kind:?} renders too thin: {message}");
        }
    }

    #[test]
    fn an_empty_listing_still_round_trips_and_omits_an_empty_hint_list() {
        let empty = CameraList::default();
        let json = serde_json::to_string(&empty).expect("serialize");
        assert_eq!(json, r#"{"cameras":[]}"#);
        assert_eq!(
            serde_json::from_str::<CameraList>(&json).expect("deserialize"),
            empty
        );
    }

    #[test]
    fn a_listing_with_a_hint_carries_it_across_the_wire() {
        let list = CameraList {
            cameras: Vec::new(),
            hints: vec![ListHint {
                kind: HintKind::DriverlessUsbVideoDevice,
                subject: "1-2".to_owned(),
            }],
        };
        let json = serde_json::to_string(&list).expect("serialize");
        let back: CameraList = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, list);
        assert_eq!(back.hints[0].subject, "1-2");
    }

    #[test]
    fn a_control_report_marks_the_rows_that_contradict_themselves() {
        use std::collections::BTreeMap;

        use crate::control::{
            ControlFlags, ControlId, ControlRange, ControlSlug, ControlType, ControlValue,
        };

        let control = |name: &str, default: i64, current: i64| ControlDesc {
            id: ControlId(1),
            name: name.to_owned(),
            slug: ControlSlug::from_name(name).expect("literal name"),
            control_type: ControlType::Integer,
            range: ControlRange {
                min: 0,
                max: 100,
                step: 1,
            },
            default,
            flags: ControlFlags::from_raw(0),
            menu: BTreeMap::new(),
            elems: 1,
            elem_size: 4,
            dims: Vec::new(),
            current: Some(ControlValue::Int(current)),
        };

        let report = ControlReport {
            camera: CameraId::parse("cam:test").expect("literal id"),
            controls: vec![
                control("Ordinary", 50, 50),
                // PF:5's shape: a default outside the declared range.
                control("Odd Default", 300, 50),
                // PF:4's shape: a current outside it.
                control("Odd Current", 50, 245),
            ],
        };
        let flagged: Vec<&str> = report
            .self_contradicting()
            .into_iter()
            .map(|d| d.name.as_str())
            .collect();
        assert_eq!(flagged, vec!["Odd Default", "Odd Current"]);
    }
}
