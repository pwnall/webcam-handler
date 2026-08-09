//! Assembling a device profile from an open camera (design T3).
//!
//! One home for the T3 split. `wch profile capture` writes what this produces; the
//! hardware rung compares a live capture against the committed corpus by calling the same
//! function; the daemon's `profile_capture` method will call it at P4. A second copy would
//! be a second opinion about which fields are invariant, and the whole value of the corpus
//! rests on that answer being one answer.
//!
//! The split, restated where it is implemented:
//!
//! - **invariant** — identity, nodes, formats, and the control set with `current` cleared
//!   and the volatile flag bits masked out. This is what "the corpus still resembles the
//!   device" means, and it compares exactly.
//! - **state** — the current values and the raw flag words, which change with use \[PF:3,
//!   PF:4\]. Re-capturing after somebody used the camera must not read as corpus drift.
//!
//! Provenance rides outside both and is never compared.

use schema::backend::{BackendKind, Camera};
use schema::error::Result;
use schema::limits;
use schema::profile::{
    DeviceProfile, ProfileInvariant, ProfileProvenance, ProfileState, invariant_control,
};
use schema::time::Stamp;

/// Who and what took a capture. Supplied by the caller because none of it is the engine's
/// to know: the clock, the tool's version, and the person are all facts about the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureContext {
    /// When. Passed in rather than read here, because the engine reads no clock.
    pub captured_at: Stamp,
    /// The capturing host's kernel release.
    pub kernel: String,
    /// The tool version that took it.
    pub tool_version: String,
    /// Who or what ran the capture.
    pub capturer: String,
    /// Which backend produced it. A profile captured from the fake backend would be
    /// circular corpus, and this field is what makes that visible.
    pub backend: BackendKind,
}

/// Capture everything `camera` can enumerate about itself.
///
/// # Errors
///
/// Whatever the camera says when asked for its formats or controls. A capture that could
/// not read part of the device is not a partial profile — it is not a profile.
pub fn capture(camera: &mut dyn Camera, context: &CaptureContext) -> Result<DeviceProfile> {
    let info = camera.info().clone();
    let formats = camera.formats()?;
    let controls = camera.controls()?;

    Ok(DeviceProfile {
        schema_version: limits::PROFILE_SCHEMA_VERSION,
        provenance: ProfileProvenance {
            captured_at: context.captured_at,
            kernel: context.kernel.clone(),
            tool_version: context.tool_version.clone(),
            capturer: context.capturer.clone(),
            backend: context.backend,
        },
        invariant: ProfileInvariant {
            info,
            formats,
            controls: controls.iter().map(invariant_control).collect(),
            // Always empty, and the comment used to say `--discover-pairs` would fill it.
            // It does not: the probe answers a `controls` report, and `profile capture`
            // does not run one — a capture that wrote to the camera would stop being the
            // read-only operation the corpus is built from, and every committed profile
            // would then depend on a probe having been run first.
            //
            // Empty is therefore the honest answer here: it says "this capture measured
            // nothing", not "this device has no pairs". Wiring a probe into `capture`
            // needs a decision about whether a corpus entry may perturb its subject, and
            // that decision belongs to whoever needs measured pairs in a profile.
            measured_pairs: Vec::new(),
        },
        state: ProfileState {
            values: controls
                .iter()
                .filter_map(|desc| desc.current.clone().map(|v| (desc.slug.clone(), v)))
                .collect(),
            flags: controls
                .iter()
                .map(|desc| (desc.slug.clone(), desc.flags.raw))
                .collect(),
        },
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Instant;

    use schema::backend::Camera;
    use schema::camera::{
        CameraFingerprint, CameraId, CameraInfo, DeviceNode, FormatInfo, NodeKind,
    };
    use schema::control::{
        Applied, ControlDesc, ControlFlags, ControlId, ControlRange, ControlSlug, ControlType,
        ControlValue, KnownFlag,
    };
    use schema::error::Error;

    use super::*;

    /// A camera that answers with exactly what it was built with. Not the fake backend:
    /// this module's subject is the *split*, and a double that returns fixed values keeps
    /// the test about that rather than about a replay engine.
    #[derive(Debug)]
    struct StubCamera {
        info: CameraInfo,
        controls: Vec<ControlDesc>,
    }

    impl Camera for StubCamera {
        fn info(&self) -> &CameraInfo {
            &self.info
        }

        fn formats(&self) -> Result<Vec<FormatInfo>> {
            Ok(Vec::new())
        }

        fn controls(&self) -> Result<Vec<ControlDesc>> {
            Ok(self.controls.clone())
        }

        fn get(&mut self, _id: ControlId) -> Result<ControlValue> {
            Err(unsupported())
        }

        fn set(&mut self, _id: ControlId, _value: ControlValue) -> Result<Applied> {
            Err(unsupported())
        }

        fn start_stream(
            &mut self,
            _request: &schema::capture::StreamRequest,
        ) -> Result<schema::capture::NegotiatedStream> {
            Err(unsupported())
        }

        fn next_frame(&mut self, _deadline: Instant) -> Result<schema::capture::Frame> {
            Err(unsupported())
        }

        fn stop_stream(&mut self) -> Result<()> {
            Err(unsupported())
        }
    }

    fn unsupported() -> Error {
        Error::Unimplemented {
            operation: "StubCamera".to_owned(),
            arrives_in: "never".to_owned(),
        }
    }

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

    fn stub(controls: Vec<ControlDesc>) -> StubCamera {
        StubCamera {
            info: CameraInfo {
                id: CameraId::parse("cam:test").expect("literal id"),
                fingerprint: CameraFingerprint {
                    bus_path: "3-4:1.0".to_owned(),
                    usb_id: None,
                    card: "Test".to_owned(),
                    driver: "uvcvideo".to_owned(),
                    serial: None,
                },
                card: "Test".to_owned(),
                driver: "uvcvideo".to_owned(),
                bus_info: "usb-1".to_owned(),
                nodes: vec![DeviceNode {
                    path: "/dev/video0".into(),
                    kind: NodeKind::VideoCapture,
                    device_caps: 0x0420_0001,
                    capabilities: 0x84a0_0001,
                }],
                backend: BackendKind::V4l2,
            },
            controls,
        }
    }

    fn context() -> CaptureContext {
        CaptureContext {
            captured_at: Stamp::epoch(),
            kernel: "7.0.0-29-generic".to_owned(),
            tool_version: "0.1.0".to_owned(),
            capturer: "test".to_owned(),
            backend: BackendKind::V4l2,
        }
    }

    #[test]
    fn the_invariant_section_carries_no_current_value_and_no_volatile_flag() {
        // The T3 split, at the point it is made. INACTIVE is set here because an
        // automation partner held the control when the capture was taken [PF:3], and it
        // must not reach the section that compares exactly.
        let mut camera = stub(vec![control("white_balance_temperature", 0x1010, 4600)]);
        let profile = capture(&mut camera, &context()).expect("captures");

        let slug = ControlSlug::parse("white_balance_temperature").expect("literal slug");
        let invariant = profile
            .control(&slug)
            .expect("the control is in the profile");
        assert_eq!(invariant.current, None);
        assert!(!invariant.flags.has(KnownFlag::Inactive));
        assert!(invariant.flags.has(KnownFlag::HasWhichMinMax));

        // …and the state block carries both.
        assert_eq!(
            profile.state.values.get(&slug),
            Some(&ControlValue::Int(4600))
        );
        assert_eq!(profile.state.flags.get(&slug), Some(&0x1010));
    }

    #[test]
    fn using_the_camera_between_two_captures_does_not_read_as_corpus_drift() {
        // The property the split exists for, end to end: capture, use the camera,
        // capture again, and the invariant sections must still compare equal.
        let fresh = capture(
            &mut stub(vec![control("white_balance_temperature", 0x1000, 4600)]),
            &context(),
        )
        .expect("captures");
        let after_use = capture(
            &mut stub(vec![control("white_balance_temperature", 0x1010, 6500)]),
            &context(),
        )
        .expect("captures");

        assert!(fresh.invariant_matches(&after_use));
        assert_ne!(fresh.state, after_use.state, "…but the state did move");
    }

    #[test]
    fn a_changed_device_does_read_as_drift() {
        // The inverse direction. If the *device* changes, the corpus must notice.
        let before =
            capture(&mut stub(vec![control("brightness", 0, 50)]), &context()).expect("captures");
        let mut moved = stub(vec![control("brightness", 0, 50)]);
        moved.controls[0].range.max = 255;
        let after = capture(&mut moved, &context()).expect("captures");
        assert!(!before.invariant_matches(&after));
    }

    #[test]
    fn a_control_with_no_readable_value_is_absent_from_the_state_block_not_zero_in_it() {
        // A button, a class header, or a write-only control has no current value, and
        // recording a 0 for it would be inventing device state.
        let mut valueless = control("user_controls", 0x44, 0);
        valueless.control_type = ControlType::ControlClass;
        valueless.current = None;
        let profile = capture(
            &mut stub(vec![valueless, control("brightness", 0, 50)]),
            &context(),
        )
        .expect("captures");

        assert_eq!(profile.state.values.len(), 1);
        assert!(
            !profile
                .state
                .values
                .contains_key(&ControlSlug::parse("user_controls").expect("literal slug"))
        );
        // It is still in the invariant section: the control exists, it just has no value.
        assert_eq!(profile.invariant.controls.len(), 2);
        // And its flags are recorded, because flags are readable even when values are not.
        assert_eq!(profile.state.flags.len(), 2);
    }

    #[test]
    fn the_capturing_backend_is_recorded_so_circular_corpus_is_visible() {
        let mut fake_context = context();
        fake_context.backend = BackendKind::Fake;
        let profile = capture(&mut stub(Vec::new()), &fake_context).expect("captures");
        assert_eq!(profile.provenance.backend, BackendKind::Fake);
        assert_eq!(profile.schema_version, limits::PROFILE_SCHEMA_VERSION);
    }
}
