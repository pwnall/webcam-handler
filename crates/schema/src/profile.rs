//! The device profile (design T3).
//!
//! A JSON capture of everything a backend can enumerate about a camera, in **two
//! sections whose comparison semantics differ**:
//!
//! - the **invariant** description — identity, nodes, formats, the control set with its
//!   menus, ranges and non-volatile flags, and the automation pairs measured on the
//!   device. This is what "the corpus still resembles the device" means. Formats,
//!   controls and pairs compare exactly; the `info` half compares by
//!   [`crate::camera::CameraInfo::differing_fields`], because the one field in here that
//!   is not invariant is the `/dev/videoN` path the kernel hands out in probe order
//!   \[PF:22, note **N63**\].
//! - the **state** block — current control values and the INACTIVE-class flags, which
//!   change with use \[PF:3, PF:4\]. Re-capturing a profile after a sweep must not read as
//!   corpus drift, so this compares loosely or not at all.
//!
//! Provenance rides outside both.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::backend::BackendKind;
use crate::camera::{CameraInfo, FormatInfo};
use crate::control::{ControlDesc, ControlSlug, ControlValue, KnownFlag};
use crate::limits;
use crate::pairing::AutomationPair;
use crate::time::Stamp;

/// Where a profile came from. Never compared — a re-capture has a new timestamp by
/// definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProfileProvenance {
    /// When it was captured.
    pub captured_at: Stamp,
    /// `uname -r` of the capturing host.
    pub kernel: String,
    /// The tool version that captured it.
    pub tool_version: String,
    /// Who or what ran the capture.
    pub capturer: String,
    /// Which backend produced it. A profile captured from the fake backend would be
    /// circular corpus, so this field is what makes that visible.
    pub backend: BackendKind,
}

/// The part of a profile that should not change unless the device or the kernel does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProfileInvariant {
    /// Identity, nodes, grouping.
    pub info: CameraInfo,
    /// Formats, with sizes and intervals nested \[PF:9\].
    pub formats: Vec<FormatInfo>,
    /// The full control set, with `current` cleared and the volatile flag bits masked
    /// out — see [`invariant_control`].
    pub controls: Vec<ControlDesc>,
    /// Automation pairs discovered by probing this device \[PF:3\].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub measured_pairs: Vec<AutomationPair>,
}

/// The part that changes with use.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct ProfileState {
    /// Each control's value at capture time — including the ones outside their declared
    /// range \[PF:4\], which is exactly why this is recorded.
    #[serde(default)]
    pub values: BTreeMap<ControlSlug, ControlValue>,
    /// Each control's raw flag word at capture time. The INACTIVE bit here tracks which
    /// automation was on when the profile was taken \[PF:3\].
    #[serde(default)]
    pub flags: BTreeMap<ControlSlug, u32>,
}

/// A captured device profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DeviceProfile {
    /// The document version.
    pub schema_version: u32,
    /// Where this came from.
    pub provenance: ProfileProvenance,
    /// What does not change.
    pub invariant: ProfileInvariant,
    /// What does.
    #[serde(default)]
    pub state: ProfileState,
}

/// The flag bits that belong to the state block rather than the invariant one.
///
/// `INACTIVE` flips when an automation partner is toggled and `GRABBED` flips while
/// something streams — both are facts about *right now*, not about the device.
pub const VOLATILE_FLAG_BITS: u32 = KnownFlag::Inactive.bit() | KnownFlag::Grabbed.bit();

/// Strip a control descriptor down to its invariant part.
///
/// Clears the current value and masks the flag bits that change with use, so two
/// captures of the same untouched device compare equal even if one was taken mid-stream.
#[must_use]
pub fn invariant_control(desc: &ControlDesc) -> ControlDesc {
    use crate::control::ControlFlags;

    let mut out = desc.clone();
    out.current = None;
    out.flags = ControlFlags::from_raw(desc.flags.raw & !VOLATILE_FLAG_BITS);
    out
}

impl DeviceProfile {
    /// Whether two profiles describe the same device in the same way.
    ///
    /// Compares the invariant section only. Provenance and state are excluded by
    /// construction, which is what lets the G1 criterion — "`profile capture` reproduces
    /// the committed profile" — be true of a camera someone has been using.
    ///
    /// The `info` half goes through [`CameraInfo::differing_fields`] rather than through
    /// `==`, because one field inside this "invariant" section is not invariant:
    /// `/dev/videoN` is probe order, and a `uvcvideo` reload renumbered three of four
    /// attached cameras without any of them changing \[PF:22, note **N63**\]. That
    /// comparison is the one home for the rule; this method is its second consumer, and
    /// spelling the exclusion again here would be the second copy AGENTS forbids.
    #[must_use]
    pub fn invariant_matches(&self, other: &DeviceProfile) -> bool {
        // Destructured, not `==`, and destructured on both sides so that a new field on
        // `ProfileInvariant` stops compiling here until somebody says whether it compares
        // exactly or by rule.
        let ProfileInvariant {
            info,
            formats,
            controls,
            measured_pairs,
        } = &self.invariant;
        let ProfileInvariant {
            info: other_info,
            formats: other_formats,
            controls: other_controls,
            measured_pairs: other_pairs,
        } = &other.invariant;

        info.describes_same_device(other_info)
            && formats == other_formats
            && controls == other_controls
            && measured_pairs == other_pairs
    }

    /// The control descriptor for a slug, from the invariant section.
    #[must_use]
    pub fn control(&self, slug: &ControlSlug) -> Option<&ControlDesc> {
        self.invariant.controls.iter().find(|c| &c.slug == slug)
    }

    /// A descriptor with the state block's value and flags folded back in — what a
    /// replaying backend hands out as "the control right now".
    #[must_use]
    pub fn live_control(&self, slug: &ControlSlug) -> Option<ControlDesc> {
        use crate::control::ControlFlags;

        let mut desc = self.control(slug)?.clone();
        if let Some(value) = self.state.values.get(slug) {
            desc.current = Some(value.clone());
        }
        if let Some(raw) = self.state.flags.get(slug) {
            desc.flags = ControlFlags::from_raw(*raw);
        }
        Some(desc)
    }

    /// A profile with its state block replaced by the invariant defaults — used when a
    /// replay wants to start from a clean device rather than from whatever the capture
    /// caught.
    #[must_use]
    pub fn with_reset_state(&self) -> DeviceProfile {
        let mut out = self.clone();
        out.state = ProfileState {
            values: out
                .invariant
                .controls
                .iter()
                .filter(|c| c.control_type.is_scalar())
                .map(|c| (c.slug.clone(), ControlValue::Int(c.default)))
                .collect(),
            flags: out
                .invariant
                .controls
                .iter()
                .map(|c| (c.slug.clone(), c.flags.raw))
                .collect(),
        };
        out
    }

    /// Whether this document's version is one this build reads.
    #[must_use]
    pub fn version_is_supported(&self) -> bool {
        self.schema_version == limits::PROFILE_SCHEMA_VERSION
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{CameraFingerprint, CameraId};
    use crate::control::{ControlFlags, ControlId, ControlRange, ControlType};

    fn control(slug: &str, raw_flags: u32, current: i64) -> ControlDesc {
        ControlDesc {
            id: ControlId(1),
            name: slug.to_owned(),
            slug: ControlSlug::parse(slug).expect("literal slug"),
            control_type: ControlType::Integer,
            range: ControlRange {
                min: 0,
                max: 100,
                step: 1,
            },
            default: 50,
            flags: ControlFlags::from_raw(raw_flags),
            menu: BTreeMap::new(),
            elems: 1,
            elem_size: 4,
            dims: Vec::new(),
            current: Some(ControlValue::Int(current)),
        }
    }

    fn profile(controls: Vec<ControlDesc>) -> DeviceProfile {
        let state = ProfileState {
            values: controls
                .iter()
                .filter_map(|c| c.current.clone().map(|v| (c.slug.clone(), v)))
                .collect(),
            flags: controls
                .iter()
                .map(|c| (c.slug.clone(), c.flags.raw))
                .collect(),
        };
        DeviceProfile {
            schema_version: limits::PROFILE_SCHEMA_VERSION,
            provenance: ProfileProvenance {
                captured_at: Stamp::epoch(),
                kernel: "7.0.0-29-generic".to_owned(),
                tool_version: "0.1.0".to_owned(),
                capturer: "test".to_owned(),
                backend: BackendKind::V4l2,
            },
            invariant: ProfileInvariant {
                info: CameraInfo {
                    id: CameraId::parse("cam:test").expect("literal id"),
                    fingerprint: CameraFingerprint {
                        bus_path: "3-1:1.0".to_owned(),
                        usb_id: None,
                        card: "Test".to_owned(),
                        driver: "uvcvideo".to_owned(),
                        serial: None,
                    },
                    card: "Test".to_owned(),
                    driver: "uvcvideo".to_owned(),
                    bus_info: "usb-3-1".to_owned(),
                    nodes: Vec::new(),
                    backend: BackendKind::V4l2,
                },
                formats: Vec::new(),
                controls: controls.iter().map(invariant_control).collect(),
                measured_pairs: Vec::new(),
            },
            state,
        }
    }

    /// A profile of a two-node camera sitting at the given paths.
    ///
    /// The caps words are the OBSBOT's, measured 2026-08-11: a capture node and a
    /// metadata node, unchanged across the reload that moved them.
    fn profile_with_nodes(paths: &[&str; 2]) -> DeviceProfile {
        use crate::camera::{DeviceNode, NodeKind};

        let mut out = profile(Vec::new());
        out.invariant.info.nodes = vec![
            DeviceNode {
                path: paths[0].into(),
                kind: NodeKind::VideoCapture,
                device_caps: 0x0420_0001,
                capabilities: 0x84a0_0001,
            },
            DeviceNode {
                path: paths[1].into(),
                kind: NodeKind::MetaCapture,
                device_caps: 0x0480_0000,
                capabilities: 0x84a0_0001,
            },
        ];
        out
    }

    #[test]
    fn using_the_camera_does_not_read_as_corpus_drift() {
        // The T3 split, as the property it exists for: the same device, sampled before
        // and after somebody used it, must compare equal on the invariant section.
        let fresh = profile(vec![control("white_balance_temperature", 0x1000, 4600)]);
        // Later: automation was switched on, so INACTIVE is set and the value moved.
        let used = profile(vec![control("white_balance_temperature", 0x1010, 6500)]);

        assert!(
            fresh.invariant_matches(&used),
            "state changes must not count as drift"
        );
        assert_ne!(fresh.state, used.state, "…but the state block did change");
    }

    #[test]
    fn renumbering_the_nodes_does_not_read_as_corpus_drift_either() {
        // The second consumer of `CameraInfo::differing_fields`, and the reason it has to
        // be the *same* function: this is the arm that never ran on 2026-08-11, because
        // the enumeration arm failed first and the suite stopped. `capture` copies
        // `camera.info()` verbatim, so a `self.invariant == other.invariant` compared the
        // kernel's node names as well [PF:22, note N63].
        let committed = profile_with_nodes(&["/dev/video4", "/dev/video5"]);
        let after_reload = profile_with_nodes(&["/dev/video0", "/dev/video1"]);
        assert_ne!(
            committed.invariant.info.nodes, after_reload.invariant.info.nodes,
            "the fixture has to differ in the field under test"
        );
        assert!(
            committed.invariant_matches(&after_reload),
            "a uvcvideo reload renamed the nodes; nothing about the device moved"
        );

        // The inverse, at this layer rather than at the schema's: a node that vanished is
        // a device that changed, and this method has to carry that through.
        let mut lost = after_reload.clone();
        lost.invariant.info.nodes.pop();
        assert!(!committed.invariant_matches(&lost));

        // …and the halves that still compare exactly are still compared exactly, so
        // routing `info` through a rule cannot be mistaken for loosening the section.
        let mut refprofile = committed.clone();
        refprofile
            .invariant
            .formats
            .push(crate::camera::FormatInfo {
                pixel_format: crate::camera::PixelFormat::MJPG,
                description: "Motion-JPEG".to_owned(),
                flags: 0,
                sizes: Vec::new(),
            });
        assert!(!committed.invariant_matches(&refprofile));
    }

    #[test]
    fn a_changed_range_does_read_as_drift() {
        // The inverse direction: if the *device* changes, the corpus must notice.
        let before = profile(vec![control("brightness", 0, 50)]);
        let mut after_controls = vec![control("brightness", 0, 50)];
        after_controls[0].range = ControlRange {
            min: 0,
            max: 255,
            step: 1,
        };
        let after = profile(after_controls);
        assert!(!before.invariant_matches(&after));
    }

    #[test]
    fn live_controls_fold_the_state_block_back_in() {
        let p = profile(vec![control("zoom_continuous", 0x1000, 245)]);
        let slug = ControlSlug::parse("zoom_continuous").expect("literal slug");

        // The invariant copy has no current value and no volatile flags.
        let inv = p.control(&slug).expect("control present");
        assert_eq!(inv.current, None);

        // The live copy has both — including the out-of-range current [PF:4].
        let live = p.live_control(&slug).expect("control present");
        assert_eq!(live.current, Some(ControlValue::Int(245)));
        assert!(live.current_out_of_range());
    }

    #[test]
    fn resetting_state_returns_every_scalar_control_to_its_default() {
        let p = profile(vec![control("brightness", 0, 17)]).with_reset_state();
        let slug = ControlSlug::parse("brightness").expect("literal slug");
        assert_eq!(
            p.live_control(&slug).expect("control").current,
            Some(ControlValue::Int(50))
        );
    }

    #[test]
    fn a_foreign_version_is_recognizable_before_anything_else_is_read() {
        let mut p = profile(Vec::new());
        assert!(p.version_is_supported());
        p.schema_version = 99;
        assert!(!p.version_is_supported());
    }

    #[test]
    fn a_profile_round_trips_through_json() {
        let p = profile(vec![control("brightness", 0x1000, 50)]);
        let json = serde_json::to_string_pretty(&p).expect("serialize");
        let back: DeviceProfile = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, p);
    }
}
