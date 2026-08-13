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
use std::fmt;

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

/// Which sections of two profiles' invariant halves disagree.
///
/// [`DeviceProfile::invariant_matches`] used to be the whole answer: one bool over four
/// sections, and every caller that got `false` had the same recourse, which was to go and
/// read a `{:#?}` dump of both sides. That stopped being enough on **2026-08-13**, when the
/// owner ruled that a camera's advertised support **may change each time the camera is
/// plugged in**, and that we need not worry about it changing while the camera stays
/// connected. Under that ruling a disagreement is no longer one kind of event: a fresh
/// capture that offers different modes than the corpus is the device exercising a licence
/// the owner has granted it, and a fresh capture whose *identity*, control set or measured
/// pairs moved is still the corpus being wrong about the device or the device being wrong
/// about itself.
///
/// **The split is measured, not assumed.** \[PF:23\] is unusually specific about where the
/// variation lives. When the OBSBOT Tiny 3 stopped advertising 3840×2160 and 120 fps its
/// `CameraInfo` half was identical — `differing_fields` answered `[]` — and its control set
/// was identical, "all 24 controls, byte for byte"; only the format tree moved. When the
/// whole tree came back two days later, on an enumeration the journal can point at, it came
/// back the same way and by the same amount. Two observations in opposite directions, and in
/// both of them exactly one of the four sections is the one that moved. So `formats` gets a
/// predicate of its own and the other three do not, and the day a control set is measured
/// moving across a replug is the day that stops being the right shape — which is why
/// [`Self::is_only_the_format_tree`] is written as a question about *this* value rather than
/// as a general "is this difference tolerable".
///
/// **A `Vec<&'static str>` of section names was the smaller change and it loses the thing
/// that matters.** `info` disagrees by *field* — [`CameraInfo::differing_fields`] is the one
/// home for that rule \[PF:22, note **N63**\] — so a flat list of section names either
/// throws those field names away or smuggles them into a string nobody can match on. This
/// value carries both: the sections as flags, and the `info` fields as the names that
/// function produced, so a caller can print the useful sentence and still branch on the
/// cheap question.
///
/// **Not a wire type, and deliberately not.** It derives neither `Serialize` nor
/// `JsonSchema`. It is the answer to a comparison between two documents, computed fresh on
/// whichever side is asking; putting it in `schemas/` would turn an internal verdict into
/// something this project promises to keep answering the same way, and the verdict is
/// exactly the thing PF:23 says may have to change when the next device teaches us
/// something.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvariantDifference {
    /// The `info` fields that describe different devices, in the order and the spelling
    /// [`CameraInfo::differing_fields`] produced. Empty when the two halves describe the
    /// same camera in the same shape — which is not the same as being equal, because
    /// `/dev/videoN` is excluded there \[PF:22\].
    info: Vec<String>,
    /// Whether the format tree differs, at any of its three depths: a pixel format, a size
    /// under a pixel format, or a frame interval under a size. One flag rather than three,
    /// because the ruling this type serves is about the tree and not about a level of it.
    formats: bool,
    /// Whether the control set differs, with `current` cleared and the volatile bits masked
    /// \[see [`invariant_control`]\].
    controls: bool,
    /// Whether the automation pairs D3's probe measured differ \[PF:3\].
    measured_pairs: bool,
}

impl InvariantDifference {
    /// How the two sections anybody asks about by name are spelled, in one place, because
    /// [`fmt::Display`] and [`Self::is_only_the_format_tree`] both have to recognise one of
    /// them in the list [`Self::sections`] returns.
    const INFO: &'static str = "info";
    const FORMATS: &'static str = "formats";

    /// The sections that disagree, named, in the order [`ProfileInvariant`] declares them.
    ///
    /// The value-shaped answer, so a test can assert *which* sections a comparison found
    /// rather than pattern-matching on prose — the same reason
    /// [`crate::pairing::AutomationPair`]'s consumers get values and not sentences. It is
    /// also the **one** walk of these fields: [`fmt::Display`] renders this list and
    /// [`Self::is_only_the_format_tree`] compares against it, so "which of these counts as a
    /// disagreement, and in what order" is settled here and nowhere else (design §2.10).
    #[must_use]
    pub fn sections(&self) -> Vec<&'static str> {
        // Destructured for the same reason `DeviceProfile::invariant_difference`
        // destructures `ProfileInvariant`, and with more riding on it: a field added to this
        // struct and not named here would be a section that silently never disagrees *and* a
        // section the format-tree predicate would keep waving through, because that
        // predicate is written over this list precisely so it cannot be told about a section
        // separately. The compiler is the only thing that reliably asks.
        let Self {
            info,
            formats,
            controls,
            measured_pairs,
        } = self;

        let mut out = Vec::new();
        if !info.is_empty() {
            out.push(Self::INFO);
        }
        if *formats {
            out.push(Self::FORMATS);
        }
        if *controls {
            out.push("controls");
        }
        if *measured_pairs {
            out.push("measured_pairs");
        }
        out
    }

    /// Whether the format tree is the **only** thing that disagrees.
    ///
    /// The predicate the owner's 2026-08-13 ruling turns on, and the only reason this type
    /// exists rather than a bool. A caller answering `true` here is looking at the shape
    /// \[PF:23\] measured twice — once shrinking, once returning whole — and may treat it as
    /// a fact about the device. A caller answering `false` is looking at something no
    /// measurement licenses, **including a formats difference with anything else beside
    /// it**: the ruling is about a device re-deciding what it advertises when its rail comes
    /// up, and a run in which the control set moved *as well* is not that observation, it is
    /// two findings at once and the second one is unexplained.
    ///
    /// **Written as an equality against [`Self::sections`] rather than as a conjunction over
    /// the fields**, and the difference is not style. `formats && info.is_empty() &&
    /// !controls && !measured_pairs` is the obvious spelling and it fails *open*: the day
    /// somebody adds a fifth section to [`ProfileInvariant`], that conjunction keeps
    /// answering `true` for a difference in it, and a hardware arm silently extends the
    /// owner's ruling to a section the owner has never been asked about. The equality
    /// answers `false` for anything it does not recognise, which is the direction a
    /// permission should fail in, and it costs one `Vec` on a path that already allocated
    /// one for the `info` field names.
    #[must_use]
    pub fn is_only_the_format_tree(&self) -> bool {
        self.sections() == [Self::FORMATS]
    }
}

impl fmt::Display for InvariantDifference {
    /// Every disagreeing section, and underneath `info` the fields that disagreed.
    ///
    /// The fields are named for the reason [`crate::pairing::AutomationPair`]'s consumers
    /// name their partner control: a section name on its own tells a reader that something
    /// moved and gives them nowhere to look, and this string's whole job is to be the line
    /// somebody reads in a transcript a week later, on a desk that no longer has the device
    /// on it.
    ///
    /// Built over [`Self::sections`] rather than by walking the fields a second time, so the
    /// membership rule and the order have one home (design §2.10); `info` is the only arm
    /// with anything to add, and it has to recognise itself by name to add it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let named: Vec<String> = self
            .sections()
            .into_iter()
            .map(|section| {
                if section == Self::INFO {
                    format!("{section} ({})", self.info.join(", "))
                } else {
                    section.to_owned()
                }
            })
            .collect();
        f.write_str(&named.join(", "))
    }
}

impl DeviceProfile {
    /// Which sections of the invariant half disagree with `other`'s, or `None` when none of
    /// them do.
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
    ///
    /// This answers with a value rather than a bool because its two hardware consumers no
    /// longer want the same thing from it — see [`InvariantDifference`] for the owner's
    /// 2026-08-13 ruling and for the two PF:23 measurements that say the format tree is the
    /// section a replug moves. [`Self::invariant_matches`] is still here, and still the
    /// right call for anybody who only wants the bool.
    #[must_use]
    pub fn invariant_difference(&self, other: &DeviceProfile) -> Option<InvariantDifference> {
        // Destructured, not `==`, and destructured on both sides so that a new field on
        // `ProfileInvariant` stops compiling here until somebody says whether it compares
        // exactly or by rule — and, since 2026-08-13, until somebody also says whether a run
        // may *decline* over it the way it may decline over `formats`. A field added to the
        // struct and forgotten here would be a section that silently never disagrees, which
        // is the failure this destructuring was put in to prevent and which now has a second
        // way to be wrong.
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

        let difference = InvariantDifference {
            info: info.differing_fields(other_info),
            formats: formats != other_formats,
            controls: controls != other_controls,
            measured_pairs: measured_pairs != other_pairs,
        };
        // Asked through `sections()` rather than re-spelling "is any of the four set",
        // because "which of these four count as a disagreement" is one rule and this is the
        // place it would quietly grow a second copy.
        (!difference.sections().is_empty()).then_some(difference)
    }

    /// Whether two profiles describe the same device in the same way.
    ///
    /// The bool [`Self::invariant_difference`] used to be, kept because most of this
    /// method's callers — the engine's own round-trip tests, and every unit arm below — want
    /// nothing more than that and reading a `None` as "matches" at each of them would be the
    /// rule stated over and over.
    #[must_use]
    pub fn invariant_matches(&self, other: &DeviceProfile) -> bool {
        self.invariant_difference(other).is_none()
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

    /// A profile of a camera offering one MJPG format with the given modes, each mode a
    /// discrete size and the whole-number frame rates available *at that size*.
    ///
    /// The shape is the OBSBOT's, because that is the device PF:23 measured: sizes nested
    /// under a pixel format and intervals nested under a size, so a mode can vanish at
    /// either of the two inner levels.
    fn profile_offering(modes: &[(u32, u32, &[u32])]) -> DeviceProfile {
        use crate::camera::{FormatInfo, FrameInterval, FrameSize, FrameSizeInfo, PixelFormat};

        let mut out = profile(Vec::new());
        out.invariant.formats = vec![FormatInfo {
            pixel_format: PixelFormat::MJPG,
            description: "Motion-JPEG".to_owned(),
            // V4L2_FMT_FLAG_COMPRESSED.
            flags: 0x0001,
            sizes: modes
                .iter()
                .map(|&(width, height, rates)| FrameSizeInfo {
                    size: FrameSize::Discrete { width, height },
                    intervals: rates
                        .iter()
                        .map(|&fps| FrameInterval::Discrete {
                            numerator: 1,
                            denominator: fps,
                        })
                        .collect(),
                })
                .collect(),
        }];
        out
    }

    #[test]
    fn a_mode_the_device_stopped_advertising_reads_as_drift() {
        // PF:23 as a property rather than as a transcript. Between 2026-08-08 and
        // 2026-08-11 the OBSBOT Tiny 3 stopped advertising 3840×2160 at all and stopped
        // offering 120 fps at the two sizes it kept — one loss at the *size* level and
        // one at the *interval* level, from a device whose `CameraInfo` half was
        // byte-identical across the two captures. Only the format tree moved.
        //
        // Both arms pass today, because `invariant_matches` compares `formats` with `==`.
        // They are written down anyway, for the reason N63 gives for writing down its
        // over-correction: the obvious next change to this method is a `formats` diff
        // that *names* what moved, the way the `info` half got one, and such a diff has a
        // characteristic wrong shape — it compares the size list and forgets that the
        // intervals hang off it. The second arm is the one that catches that.
        //
        // 2026-08-13 took half of that step and not this half: `invariant_difference` names
        // the *section*, so a reader is told the format tree moved, and it still compares
        // the tree with `==`, so nothing here says which size or which interval. The warning
        // above therefore still stands as written, against the diff that has not been built.
        let modes: &[(u32, u32, &[u32])] = &[(1920, 1080, &[120, 60, 30]), (3840, 2160, &[30])];
        let committed = profile_offering(modes);

        // Non-vacuity first: two captures of an unchanged device still match. Without
        // this, a builder that made every profile differ would satisfy both refusals.
        assert!(
            committed.invariant_matches(&profile_offering(modes)),
            "the same modes twice are not drift"
        );

        // The size that vanished.
        let no_4k = profile_offering(&[(1920, 1080, &[120, 60, 30])]);
        assert!(
            !committed.invariant_matches(&no_4k),
            "a size the device stopped offering is the device changing shape"
        );

        // The interval that vanished, at a size the device kept. The size *list* is
        // identical on both sides here, which is what makes this the arm a comparison
        // written one level too shallow would let through.
        let no_120 = profile_offering(&[(1920, 1080, &[60, 30]), (3840, 2160, &[30])]);
        let sizes = |p: &DeviceProfile| -> Vec<crate::camera::FrameSize> {
            p.invariant
                .formats
                .iter()
                .flat_map(|f| f.sizes.iter().map(|entry| entry.size))
                .collect()
        };
        assert_eq!(
            sizes(&committed),
            sizes(&no_120),
            "this fixture has to differ from the committed one *only* in its intervals"
        );
        assert!(
            !committed.invariant_matches(&no_120),
            "a frame rate the device stopped offering is drift too, and it hides one \
             level deeper than the size does"
        );
    }

    // ------------------- which section moved, under the owner's 2026-08-13 ruling
    //
    // The ruling is that a camera's advertised support may change each time it is plugged
    // in, and PF:23 is what says *where* that shows up: twice now — once shrinking, once
    // returning whole — the OBSBOT Tiny 3's format tree moved while its identity and its 24
    // controls did not. A hardware arm may therefore decline over a formats-only difference
    // and must not decline over any other, so the difference between "only the format tree"
    // and "the format tree and something else" is a load-bearing distinction and gets tests
    // over values. These are the arms that survive the corpus being re-captured to today's
    // tree: after that the hardware arm goes green and stops exercising the decline at all,
    // and the next plug event that reaches it will be somebody else's session.

    #[test]
    fn a_formats_only_difference_is_named_as_one_and_not_as_a_match() {
        // The device fact, as a value. The two sides differ in the tree and in nothing
        // else, which is exactly PF:23's two observations, and all three of the questions a
        // caller can ask have to agree about that.
        let modes: &[(u32, u32, &[u32])] = &[(1920, 1080, &[120, 60, 30]), (3840, 2160, &[30])];
        let committed = profile_offering(&[(1920, 1080, &[60, 30])]);
        let fresh = profile_offering(modes);

        // Non-vacuity, first and in the same shape the arm above uses: the builder has to be
        // capable of producing two profiles that agree, or every refusal below is free.
        assert_eq!(
            committed.invariant_difference(&profile_offering(&[(1920, 1080, &[60, 30])])),
            None,
            "two captures of an unchanged device disagree in no section at all"
        );

        let difference = committed
            .invariant_difference(&fresh)
            .expect("a tree that gained a size and a rate is a difference");
        assert_eq!(difference.sections(), ["formats"]);
        assert!(difference.is_only_the_format_tree());
        assert_eq!(difference.to_string(), "formats");
        // And the bool the old callers read is still the old bool: a difference is a
        // difference, whatever a hardware arm then decides to do about it. `is_none()` over
        // a value is only a refactor if this stays true.
        assert!(!committed.invariant_matches(&fresh));
    }

    #[test]
    fn a_control_set_that_moved_is_not_the_ruling_the_format_tree_gets() {
        // The inverse, and the one that matters most: the ruling is about a device
        // re-deciding what it *advertises*, and PF:23 measured the control set holding still
        // through both of those events — "all 24 controls, byte for byte". A difference that
        // reaches the controls is therefore outside what anybody has licensed, and a
        // predicate that answered `true` here would convert an unexplained finding into a
        // green run with a skip line on it.
        let before = profile(vec![control("brightness", 0, 50)]);
        let mut moved = vec![control("brightness", 0, 50)];
        moved[0].range = ControlRange {
            min: 0,
            max: 255,
            step: 1,
        };
        let after = profile(moved.clone());

        let difference = before
            .invariant_difference(&after)
            .expect("a control whose range moved is a difference");
        assert_eq!(difference.sections(), ["controls"]);
        assert!(
            !difference.is_only_the_format_tree(),
            "a control set that moved is not a format tree that moved"
        );

        // …and the same refusal when the format tree moved *as well*, which is the shape a
        // predicate written as "no controls difference unless" would still wave through in
        // one direction. Two findings in one capture is not the observation PF:23 recorded;
        // it is that observation with an unexplained one sitting beside it.
        let mut both = profile_offering(&[(1920, 1080, &[60, 30])]);
        both.invariant.controls = moved.iter().map(invariant_control).collect();
        let mut only_formats = profile_offering(&[(1920, 1080, &[120, 60, 30])]);
        only_formats.invariant.controls = before.invariant.controls.clone();

        let difference = only_formats
            .invariant_difference(&both)
            .expect("a tree and a control set that both moved is a difference");
        assert_eq!(difference.sections(), ["formats", "controls"]);
        assert!(
            !difference.is_only_the_format_tree(),
            "the format tree moving does not license whatever moved with it"
        );
        assert_eq!(difference.to_string(), "formats, controls");
    }

    #[test]
    fn an_identity_difference_names_the_fields_it_found_and_is_not_the_format_tree_either() {
        // The third section, and the reason this value is not a list of section names: the
        // `info` half disagrees by *field*, `CameraInfo::differing_fields` is the one home
        // for which fields count \[PF:22, note N63\], and a caller printing "info" alone
        // would be telling a reader that a camera changed and giving them nowhere to look.
        let committed = profile_with_nodes(&["/dev/video4", "/dev/video5"]);
        let mut lost = profile_with_nodes(&["/dev/video0", "/dev/video1"]);
        lost.invariant.info.nodes.pop();

        let difference = committed
            .invariant_difference(&lost)
            .expect("a camera that lost a node is a difference");
        assert_eq!(difference.sections(), ["info"]);
        assert!(!difference.is_only_the_format_tree());
        assert_eq!(
            difference.to_string(),
            "info (nodes.len)",
            "the section on its own is not an answer a reader can act on"
        );

        // The renumbering that started all of this is still not a difference at all, so
        // routing the `info` half through a rule survived being given a richer answer type.
        assert_eq!(
            committed.invariant_difference(&profile_with_nodes(&["/dev/video0", "/dev/video1"])),
            None
        );

        // And identity beside a moved tree is still not the ruling, which needs saying
        // separately: the two arms above both leave `formats` false, so neither of them can
        // tell `formats && info.is_empty() && …` from `formats && …`. This is the only place
        // the `info` conjunct is load-bearing, and a camera whose identity moved is the one
        // difference nobody may wave through — the corpus is then not describing this device
        // at all, and whatever its format tree says is an answer to a different question.
        let mut renamed = profile_offering(&[(1920, 1080, &[60, 30])]);
        renamed.invariant.info.card = "Some Other Camera".to_owned();
        let difference = profile_offering(&[(1920, 1080, &[120, 60, 30])])
            .invariant_difference(&renamed)
            .expect("a different camera offering a different tree is a difference");
        assert_eq!(difference.sections(), ["info", "formats"]);
        assert!(
            !difference.is_only_the_format_tree(),
            "a format tree read off a camera that is not the committed one is not evidence \
             about the committed one"
        );
        assert_eq!(difference.to_string(), "info (card), formats");
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
