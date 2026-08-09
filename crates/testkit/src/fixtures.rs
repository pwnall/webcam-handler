//! The hand-authored minimal-repro device profile.
//!
//! `corpus/profiles/` stays uniformly tool-captured (design §3.2), and real captures do
//! not exist until P1 — so P0 needs one document that carries every PF-derived edge at
//! once and lives with the testkit instead of in the corpus. This is it
//! (`crates/testkit/fixtures/synthetic-basic.json`), and docs/7 P0 commissions it by
//! name.
//!
//! **Why the JSON is generated rather than typed.** A hand-typed fixture drifts from the
//! type the moment a field is renamed, and a fixture that no longer deserializes is
//! usually deleted rather than fixed. So the document is *serialized from*
//! [`synthetic_basic`], and the test `the_committed_document_matches_the_constructor`
//! fails when the two disagree — with `WCH_BLESS_FIXTURES=1` in the environment to
//! rewrite it deliberately. The committed bytes still matter: they are what proves the
//! *serde contract* has not changed, which a Rust-only fixture could never show.
//!
//! The edges this document carries, each cited where it is built below:
//!
//! | Edge | Control |
//! |---|---|
//! | sparse menu with a hole \[PF:2\] | `auto_exposure`, items {1, 3} |
//! | default outside its declared range \[PF:5\] | `power_line_frequency`, range 0..=2, default 3 |
//! | current outside its declared range \[PF:4\] | `zoom_continuous`, range -100..=100, current 245 |
//! | unknown control type with an opaque payload \[PF:1\] | `region_of_interest_rectangle` |
//! | READ_ONLY control \[PF:12\] | `privacy` |
//! | live INACTIVE coupling \[PF:3\] | `white_balance_automatic` / `white_balance_temperature` |
//! | per-format maximum sizes \[PF:9\] | MJPG to 3840×2160, YUYV to 640×480 |

use std::collections::BTreeMap;

use camino::Utf8PathBuf;
use schema::backend::BackendKind;
use schema::camera::{
    CameraFingerprint, CameraId, CameraInfo, DeviceNode, FormatInfo, FrameInterval, FrameSize,
    FrameSizeInfo, NodeKind, PixelFormat, UsbId,
};
use schema::control::{
    ControlDesc, ControlFlags, ControlId, ControlRange, ControlSlug, ControlType, ControlValue,
    MenuItem,
};
use schema::error::Result;
use schema::limits;
use schema::pairing::{AutomationOff, AutomationPair, Provenance};
use schema::profile::{
    DeviceProfile, ProfileInvariant, ProfileProvenance, ProfileState, invariant_control,
};
use schema::time::Stamp;

/// The committed document's path, relative to this crate's manifest directory.
const SYNTHETIC_BASIC_FILE: &str = "fixtures/synthetic-basic.json";

/// Where the committed copy of [`synthetic_basic`] lives.
#[must_use]
pub fn synthetic_basic_path() -> Utf8PathBuf {
    Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(SYNTHETIC_BASIC_FILE)
}

/// Load the committed document from disk.
///
/// Tests that mean "the fixture on disk carries this edge" must go through here; calling
/// [`synthetic_basic`] instead would assert the constructor against itself.
pub fn load_synthetic_basic() -> Result<DeviceProfile> {
    crate::corpus::load_path(&synthetic_basic_path())
}

/// The document, as Rust.
///
/// Every value here is a fixture with a citation, not a plausible number: the ranges,
/// menu holes and out-of-range values are the ones the design-phase probes measured
/// (docs/6 §1.2), transplanted onto one synthetic device so a single load exercises them
/// all.
#[must_use]
pub fn synthetic_basic() -> DeviceProfile {
    let controls = synthetic_basic_controls();
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
            tool_version: schema::TOOL_VERSION.to_owned(),
            capturer: "hand-authored in crates/testkit/src/fixtures.rs — no device was \
                       probed for this document"
                .to_owned(),
            // Nothing real produced it. `Fake` is the honest answer, and it is what keeps
            // this document from ever being mistaken for corpus (§3.2).
            backend: BackendKind::Fake,
        },
        invariant: ProfileInvariant {
            info: synthetic_basic_info(),
            formats: synthetic_basic_formats(),
            // The invariant section carries no current values and no volatile flag bits;
            // those live in the state block. Deriving it here rather than writing it out
            // means the T3 split cannot be got wrong in the fixture (design T3).
            controls: controls.iter().map(invariant_control).collect(),
            measured_pairs: synthetic_basic_pairs(),
        },
        state,
    }
}

/// The camera this document describes.
///
/// `info.backend` is `V4l2` because the document is written to look like what a capture
/// off real hardware would record — which is also what makes the fake's rewrite of this
/// field observable. The *provenance* block above is where "nobody captured this" is
/// recorded.
fn synthetic_basic_info() -> CameraInfo {
    CameraInfo {
        id: CameraId::parse("cam:synthetic-basic").expect("a literal, non-empty id"),
        fingerprint: CameraFingerprint {
            // The USB *interface* path, never `bus_info` — two logical cameras on one
            // device share the latter [PF:13].
            bus_path: "3-4:1.0".to_owned(),
            usb_id: Some(UsbId {
                vendor: 0x1d6b,
                product: 0x0001,
            }),
            card: "Synthetic Basic Camera".to_owned(),
            driver: "uvcvideo".to_owned(),
            // PF:8's common case: no distinguishing serial.
            serial: None,
        },
        card: "Synthetic Basic Camera".to_owned(),
        driver: "uvcvideo".to_owned(),
        bus_info: "usb-0000:00:14.0-4".to_owned(),
        nodes: vec![
            // The measured `device_caps` words from PF:13's node table. The capture node
            // is told from the metadata node by these, never by numbering [PF:7].
            DeviceNode {
                path: "/dev/video0".into(),
                kind: NodeKind::from_device_caps(0x0420_0001),
                device_caps: 0x0420_0001,
                capabilities: 0x84a0_0001,
            },
            DeviceNode {
                path: "/dev/video1".into(),
                kind: NodeKind::from_device_caps(0x04a0_0000),
                device_caps: 0x04a0_0000,
                capabilities: 0x84a0_0001,
            },
        ],
        backend: BackendKind::V4l2,
    }
}

/// MJPG and YUYV with different maxima, which is why sizes nest under formats \[PF:9\].
fn synthetic_basic_formats() -> Vec<FormatInfo> {
    fn discrete(width: u32, height: u32, fps: u32) -> FrameSizeInfo {
        FrameSizeInfo {
            size: FrameSize::Discrete { width, height },
            intervals: vec![FrameInterval::Discrete {
                numerator: 1,
                denominator: fps,
            }],
        }
    }

    vec![
        FormatInfo {
            pixel_format: PixelFormat::MJPG,
            description: "Motion-JPEG".to_owned(),
            // V4L2_FMT_FLAG_COMPRESSED.
            flags: 0x0001,
            sizes: vec![
                discrete(640, 480, 30),
                discrete(1920, 1080, 30),
                discrete(3840, 2160, 30),
            ],
        },
        FormatInfo {
            pixel_format: PixelFormat::YUYV,
            description: "YUYV 4:2:2".to_owned(),
            flags: 0,
            // The whole point of the nesting: uncompressed tops out three orders of
            // magnitude below MJPG on the same cable [PF:9].
            sizes: vec![discrete(320, 240, 30), discrete(640, 480, 30)],
        },
    ]
}

/// The measured automation pairs. `measured`, not `declared`: on a real device these come
/// from toggling each automation control and diffing INACTIVE across the set \[PF:3\], and
/// the fake's coupling model reads exactly this list (E5 — it may not invent coupling the
/// profile never recorded).
fn synthetic_basic_pairs() -> Vec<AutomationPair> {
    vec![
        AutomationPair {
            manual: slug("exposure_time_absolute"),
            automation: slug("auto_exposure"),
            // By item *name*: `Manual Mode` is index 1 here and that is a coincidence,
            // not a contract [PF:2].
            off: AutomationOff::MenuItemNamed {
                patterns: vec!["manual".to_owned()],
            },
            provenance: Provenance::Measured,
        },
        AutomationPair {
            manual: slug("white_balance_temperature"),
            automation: slug("white_balance_automatic"),
            off: AutomationOff::Value { value: 0 },
            provenance: Provenance::Measured,
        },
        AutomationPair {
            manual: slug("focus_absolute"),
            automation: slug("focus_automatic_continuous"),
            off: AutomationOff::Value { value: 0 },
            provenance: Provenance::Measured,
        },
    ]
}

/// The control set, with `current` and the live flag word filled in — the *state* view.
/// [`synthetic_basic`] splits it into the invariant and state sections.
fn synthetic_basic_controls() -> Vec<ControlDesc> {
    // Most integer controls on this kernel carry HAS_WHICH_MIN_MAX; the fixture carries
    // it too, so nothing downstream can assume a clean zero flag word [PF:12].
    const HAS_WHICH_MIN_MAX: u32 = 0x1000;
    const READ_ONLY: u32 = 0x0004;
    const INACTIVE: u32 = 0x0010;
    const HAS_PAYLOAD: u32 = 0x0100;

    vec![
        integer(
            0x0098_0900,
            "Brightness",
            ControlRange {
                min: 0,
                max: 255,
                step: 1,
            },
            128,
            128,
            HAS_WHICH_MIN_MAX,
        ),
        // PF:2 — index 2 does not exist. `VIDIOC_QUERYMENU` returns EINVAL on the hole,
        // and a Vec would have invented an item there.
        menu(
            0x009a_0901,
            "Auto Exposure",
            ControlRange {
                min: 0,
                max: 3,
                step: 1,
            },
            3,
            3,
            &[(1, "Manual Mode"), (3, "Aperture Priority Mode")],
            HAS_WHICH_MIN_MAX,
        ),
        // Automation is on above, so the manual partner is INACTIVE right now [PF:3].
        integer(
            0x009a_0902,
            "Exposure Time, Absolute",
            ControlRange {
                min: 1,
                max: 10_000,
                step: 1,
            },
            156,
            156,
            HAS_WHICH_MIN_MAX | INACTIVE,
        ),
        boolean(
            0x0098_090c,
            "White Balance, Automatic",
            1,
            1,
            HAS_WHICH_MIN_MAX,
        ),
        // The PF:3 seed case: 0x1000 → 0x1010 when its automation partner is switched on,
        // and back when it is switched off. The fixture is captured with it on.
        integer(
            0x0098_091a,
            "White Balance Temperature",
            ControlRange {
                min: 2_800,
                max: 6_500,
                step: 10,
            },
            4_600,
            4_600,
            HAS_WHICH_MIN_MAX | INACTIVE,
        ),
        // PF:5 — the menu declares 0..=2 and the default is 3. Reported, never corrected.
        menu(
            0x0098_0918,
            "Power Line Frequency",
            ControlRange {
                min: 0,
                max: 2,
                step: 1,
            },
            3,
            2,
            &[(0, "Disabled"), (1, "50 Hz"), (2, "60 Hz")],
            0,
        ),
        // PF:4 — the current value sits 145 past the declared maximum. Replayed as-is.
        integer(
            0x009a_090d,
            "Zoom, Continuous",
            ControlRange {
                min: -100,
                max: 100,
                step: 1,
            },
            0,
            245,
            HAS_WHICH_MIN_MAX,
        ),
        integer(
            0x009a_090a,
            "Focus, Absolute",
            ControlRange {
                min: 0,
                max: 1_023,
                step: 1,
            },
            512,
            512,
            HAS_WHICH_MIN_MAX,
        ),
        boolean(
            0x009a_090c,
            "Focus, Automatic Continuous",
            1,
            0,
            HAS_WHICH_MIN_MAX,
        ),
        // PF:12 — the Chicony's Privacy control cannot be written, and the tool honors
        // that rather than working around it (§5).
        boolean(0x0098_0910, "Privacy", 0, 0, READ_ONLY),
        // PF:1 — the control type that panics the popular V4L2 crate's enumeration. Here
        // it is a type this build does not name, carrying its payload byte-exact. The raw
        // discriminant is deliberately one no kernel has issued: a fixture using 0x0107
        // would decode to `Rect` and stop exercising the `Unknown` path.
        ControlDesc {
            id: ControlId(0x0098_1ae1),
            name: "Region of Interest Rectangle".to_owned(),
            slug: slug("region_of_interest_rectangle"),
            control_type: ControlType::Unknown { raw: 0x0fff },
            range: ControlRange {
                min: 0,
                max: 0,
                step: 0,
            },
            default: 0,
            flags: ControlFlags::from_raw(HAS_PAYLOAD),
            menu: BTreeMap::new(),
            elems: 1,
            elem_size: 16,
            dims: Vec::new(),
            current: Some(ControlValue::Bytes(vec![
                0, 0, 0, 0, 0, 0, 0, 0, 0x80, 0x07, 0, 0, 0x38, 0x04, 0, 0,
            ])),
        },
    ]
}

fn slug(text: &str) -> ControlSlug {
    ControlSlug::parse(text).expect("a literal, non-empty slug")
}

fn derived_slug(name: &str) -> ControlSlug {
    ControlSlug::from_name(name).expect("every fixture control name slugs to something")
}

fn integer(
    id: u32,
    name: &str,
    range: ControlRange,
    default: i64,
    current: i64,
    raw_flags: u32,
) -> ControlDesc {
    ControlDesc {
        id: ControlId(id),
        name: name.to_owned(),
        slug: derived_slug(name),
        control_type: ControlType::Integer,
        range,
        default,
        flags: ControlFlags::from_raw(raw_flags),
        menu: BTreeMap::new(),
        elems: 1,
        elem_size: 4,
        dims: Vec::new(),
        current: Some(ControlValue::Int(current)),
    }
}

fn boolean(id: u32, name: &str, default: i64, current: i64, raw_flags: u32) -> ControlDesc {
    ControlDesc {
        control_type: ControlType::Boolean,
        ..integer(
            id,
            name,
            ControlRange {
                min: 0,
                max: 1,
                step: 1,
            },
            default,
            current,
            raw_flags,
        )
    }
}

fn menu(
    id: u32,
    name: &str,
    range: ControlRange,
    default: i64,
    current: i64,
    items: &[(u32, &str)],
    raw_flags: u32,
) -> ControlDesc {
    ControlDesc {
        control_type: ControlType::Menu,
        menu: items
            .iter()
            .map(|(index, item)| {
                (
                    *index,
                    MenuItem::Name {
                        name: (*item).to_owned(),
                    },
                )
            })
            .collect(),
        ..integer(id, name, range, default, current, raw_flags)
    }
}

/// [`synthetic_basic`] as it is committed: pretty-printed with a trailing newline, so the
/// document is diffable and `git` does not complain about its last line.
///
/// This is the one spelling of "the fixture, as bytes"; the drift test compares the file
/// against it rather than re-deciding how to format JSON.
#[must_use]
pub fn synthetic_basic_json() -> String {
    let mut text =
        serde_json::to_string_pretty(&synthetic_basic()).expect("a DeviceProfile serializes");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use schema::camera::{CAP_META_CAPTURE, CAP_VIDEO_CAPTURE};
    use schema::control::KnownFlag;

    use super::*;

    /// The variable this whole module exists to hold still: the bytes on disk and the
    /// type in the crate must be the same document.
    #[test]
    fn the_committed_document_matches_the_constructor() {
        let path = synthetic_basic_path();
        let expected = synthetic_basic_json();

        if std::env::var_os("WCH_BLESS_FIXTURES").is_some() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("create the fixtures directory");
            }
            std::fs::write(&path, &expected).expect("write the fixture");
        }

        let found = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{path} is missing or unreadable: {error}"));
        assert_eq!(
            found, expected,
            "{path} has drifted from fixtures::synthetic_basic(). Re-run with \
             WCH_BLESS_FIXTURES=1 once you have decided the new document is what you meant."
        );
    }

    /// A fixture nobody reads is not coverage (docs/8 rule 3). This is the test that
    /// reads it — from disk, so the serde contract is what is being asserted and not the
    /// constructor's memory of it.
    #[test]
    fn the_committed_document_carries_every_pf_edge() {
        let profile = load_synthetic_basic().expect("the committed fixture loads");

        // PF:2 — a sparse menu with a hole where index 2 would be.
        let auto_exposure = profile
            .control(&slug("auto_exposure"))
            .expect("auto_exposure is present");
        assert_eq!(
            auto_exposure.menu.keys().copied().collect::<Vec<_>>(),
            vec![1, 3],
            "the hole at index 2 is the fixture"
        );
        assert_eq!(
            auto_exposure.menu_index_by_name(|n| n.contains("Manual")),
            Some(1)
        );

        // PF:5 — a default outside its own declared range.
        let power_line = profile
            .control(&slug("power_line_frequency"))
            .expect("power_line_frequency is present");
        assert_eq!(power_line.range.max, 2);
        assert_eq!(power_line.default, 3);
        assert!(power_line.default_out_of_range());

        // PF:4 — a current value outside its own declared range, in the state block.
        let zoom = profile
            .live_control(&slug("zoom_continuous"))
            .expect("zoom_continuous is present");
        assert_eq!(zoom.current, Some(ControlValue::Int(245)));
        assert!(zoom.current_out_of_range());

        // PF:1 — an unknown control type carrying an opaque payload.
        let roi = profile
            .live_control(&slug("region_of_interest_rectangle"))
            .expect("region_of_interest_rectangle is present");
        assert!(matches!(roi.control_type, ControlType::Unknown { .. }));
        assert!(!roi.control_type.is_scalar());
        assert!(
            matches!(&roi.current, Some(ControlValue::Bytes(bytes)) if bytes.len() == 16),
            "the payload must survive as bytes: {:?}",
            roi.current
        );

        // PF:12 — a READ_ONLY control.
        let privacy = profile
            .control(&slug("privacy"))
            .expect("privacy is present");
        assert!(privacy.flags.has(KnownFlag::ReadOnly));
        assert!(!privacy.is_writable());

        // PF:3 — a boolean automation control paired to a manual one, captured with the
        // automation on and the partner's INACTIVE bit set.
        let pair = profile
            .invariant
            .measured_pairs
            .iter()
            .find(|p| p.automation == slug("white_balance_automatic"))
            .expect("the white balance pair was measured");
        assert_eq!(pair.manual, slug("white_balance_temperature"));
        assert_eq!(pair.provenance, Provenance::Measured);
        let automation = profile
            .live_control(&pair.automation)
            .expect("the automation control is present");
        assert_eq!(automation.current, Some(ControlValue::Int(1)));
        let manual = profile
            .live_control(&pair.manual)
            .expect("the manual control is present");
        assert!(
            manual.is_inactive(),
            "the manual partner must carry INACTIVE while automation is on"
        );
        // …and the invariant section must not, because INACTIVE is state (T3).
        let invariant_manual = profile.control(&pair.manual).expect("present");
        assert!(!invariant_manual.is_inactive());

        // PF:9 — two formats with different maximum sizes, which is why sizes nest.
        let max_for = |format: PixelFormat| {
            profile
                .invariant
                .formats
                .iter()
                .find(|f| f.pixel_format == format)
                .map(|f| {
                    f.sizes
                        .iter()
                        .filter_map(|s| s.size.max_dimensions())
                        .max()
                        .unwrap_or((0, 0))
                })
        };
        assert_eq!(max_for(PixelFormat::MJPG), Some((3840, 2160)));
        assert_eq!(max_for(PixelFormat::YUYV), Some((640, 480)));

        // PF:7 — a capture node and a metadata node, told apart by device_caps.
        let info = &profile.invariant.info;
        let capture = info.capture_node().expect("a capture node");
        assert_ne!(capture.device_caps & CAP_VIDEO_CAPTURE, 0);
        let metadata = info
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::MetaCapture)
            .expect("a metadata node");
        assert_ne!(metadata.device_caps & CAP_META_CAPTURE, 0);
        assert_eq!(metadata.device_caps & CAP_VIDEO_CAPTURE, 0);
    }

    #[test]
    fn every_control_slug_is_derivable_from_its_kernel_name() {
        // Names are load-bearing on both sides of the ioctl (rubric A11): a fixture whose
        // slug is not what the D2 transform produces would teach the wrong spelling.
        for desc in synthetic_basic_controls() {
            assert_eq!(
                ControlSlug::from_name(&desc.name),
                Some(desc.slug.clone()),
                "{:?} does not slug to {}",
                desc.name,
                desc.slug
            );
        }
    }

    #[test]
    fn every_measured_pair_names_controls_the_document_has() {
        // A pair naming a control that is not in the set would make the fake couple
        // nothing while claiming to (E5).
        let profile = synthetic_basic();
        for pair in &profile.invariant.measured_pairs {
            assert!(
                profile.control(&pair.manual).is_some(),
                "{} is not in the control set",
                pair.manual
            );
            let automation = profile
                .control(&pair.automation)
                .unwrap_or_else(|| panic!("{} is not in the control set", pair.automation));
            assert!(
                pair.off.resolve(automation).is_some(),
                "{}'s off value does not resolve against its own descriptor",
                pair.automation
            );
        }
    }
}
