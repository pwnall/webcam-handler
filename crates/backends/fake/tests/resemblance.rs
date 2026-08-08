//! E5: a stand-in resembles what it imitates.
//!
//! Every behavioural claim the fake makes is checked here **against the profile it
//! replays**, never against a second copy of the expectation written out by hand. If the
//! profile says a control's range stops at 6500, the clamp stops at 6500 because this
//! file read 6500 out of the document. A fake drifting from its profile is a test failure
//! in the fake, not a green run.
//!
//! The hermetic fixture (`crates/testkit/fixtures/synthetic-basic.json`) is the document
//! under replay at P0; at P1 the same assertions run over the tool-captured corpus, which
//! is what makes the profile-driven form of these tests worth the extra indirection.

use std::time::{Duration, Instant};

use fake::{FakeBackend, FakeCamera};
use imaging::metrics;
use schema::backend::{BackendKind, Camera, CameraBackend};
use schema::camera::{NodeKind, PixelFormat};
use schema::capture::{Adjustment, StreamRequest};
use schema::control::{
    ControlDesc, ControlSlug, ControlType, ControlValue, KnownFlag, WriteWarning,
};
use schema::error::Error;
use schema::profile::DeviceProfile;
use testkit::fixtures;

fn backend() -> FakeBackend {
    FakeBackend::from_profile(fixtures::synthetic_basic()).expect("the fixture replays")
}

fn open_first(backend: &FakeBackend) -> FakeCamera {
    let info = backend
        .enumerate()
        .expect("enumerate")
        .into_iter()
        .next()
        .expect("the fixture describes one camera");
    backend.open_fake(&info.id).expect("open")
}

fn slug(text: &str) -> ControlSlug {
    ControlSlug::parse(text).expect("a literal, non-empty slug")
}

fn descriptor(camera: &FakeCamera, name: &str) -> ControlDesc {
    camera
        .controls()
        .expect("controls")
        .into_iter()
        .find(|desc| desc.slug == slug(name))
        .unwrap_or_else(|| panic!("{name} is not in the replayed control set"))
}

/// Every control's raw flag word, so "nothing else moved" is checkable.
fn flag_words(camera: &FakeCamera) -> Vec<(ControlSlug, u32)> {
    camera
        .controls()
        .expect("controls")
        .into_iter()
        .map(|desc| (desc.slug, desc.flags.raw))
        .collect()
}

#[test]
fn the_clamp_stops_where_the_profile_says_the_range_stops() {
    // PF:6, driven by the document: for every scalar control the profile declares, a
    // write past its maximum succeeds and applies *that control's* maximum — not a
    // number this test knows.
    let backend = backend();
    let profile = backend.profiles().into_iter().next().expect("one profile");
    let mut camera = open_first(&backend);

    let mut probed = 0;
    for declared in &profile.invariant.controls {
        if !declared.is_writable()
            || declared.control_type.is_menu()
            || !declared.control_type.is_scalar()
        {
            continue;
        }
        probed += 1;
        let beyond = declared.range.max + 1_000;
        let applied = camera
            .set(declared.id, ControlValue::Int(beyond))
            .unwrap_or_else(|e| {
                panic!("{}: an out-of-range write must succeed: {e}", declared.slug)
            });

        assert_eq!(
            applied.applied,
            ControlValue::Int(declared.range.max),
            "{} clamped to the wrong value",
            declared.slug
        );
        assert_eq!(applied.requested, ControlValue::Int(beyond));
        assert!(
            applied.warnings.iter().any(|warning| matches!(
                warning,
                WriteWarning::Clamped { range, applied, .. }
                    if *range == declared.range && *applied == declared.range.max
            )),
            "{}: the clamp warning must carry the declared range, got {:?}",
            declared.slug,
            applied.warnings
        );
    }
    assert!(probed >= 4, "only {probed} controls exercised the clamp");
}

#[test]
fn a_write_off_the_step_is_aligned_with_the_warning() {
    let backend = backend();
    let mut camera = open_first(&backend);
    let white_balance = descriptor(&camera, "white_balance_temperature");
    let step = white_balance.range.effective_step();
    assert!(step > 1, "the fixture's step must be worth aligning to");

    let off_step = white_balance.range.min + step + 1;
    let applied = camera
        .set(white_balance.id, ControlValue::Int(off_step))
        .expect("a misaligned write is a success with a warning");

    assert_eq!(
        applied.applied,
        ControlValue::Int(white_balance.range.min + step)
    );
    assert!(
        applied.warnings.iter().any(|warning| matches!(
            warning,
            WriteWarning::StepAligned { step: reported, .. } if *reported == step
        )),
        "{:?}",
        applied.warnings
    );
}

#[test]
fn toggling_a_measured_automation_flips_exactly_its_partner_and_nothing_else() {
    // PF:3, both directions and live. "Nothing else" is the load-bearing half: a fake
    // that flipped every INACTIVE bit at once would pass a test that only looked at the
    // partner.
    let backend = backend();
    let profile = backend.profiles().into_iter().next().expect("one profile");
    let mut camera = open_first(&backend);

    let pair = profile
        .invariant
        .measured_pairs
        .iter()
        .find(|pair| pair.automation == slug("white_balance_automatic"))
        .expect("the fixture measured this pair");
    let automation = descriptor(&camera, pair.automation.as_str());
    let off = pair
        .off
        .resolve(&automation)
        .expect("the profile's off value resolves");

    let before = flag_words(&camera);
    camera
        .set(automation.id, ControlValue::Int(off))
        .expect("switching automation off");
    let after_off = flag_words(&camera);

    for ((name, was), (again, now)) in before.iter().zip(&after_off) {
        assert_eq!(name, again, "the control set changed shape");
        if name == &pair.manual {
            assert_ne!(was, now, "{name} should have lost its INACTIVE bit");
            assert_eq!(was & !KnownFlag::Inactive.bit(), *now);
        } else {
            assert_eq!(was, now, "{name}'s flags moved and should not have");
        }
    }

    // And back: the coupling is not a one-way trip.
    camera
        .set(automation.id, ControlValue::Int(off + 1))
        .expect("switching automation on");
    assert_eq!(flag_words(&camera), before);
}

#[test]
fn a_menu_hole_is_refused_the_way_a_driver_refuses_it() {
    // PF:2 — the Chicony's `Auto Exposure` has items {1, 3} and `VIDIOC_QUERYMENU`
    // returns EINVAL on index 2. A fake that accepted the hole would let a planner ship
    // an index no device takes.
    let backend = backend();
    let mut camera = open_first(&backend);
    let auto_exposure = descriptor(&camera, "auto_exposure");

    let hole = (auto_exposure.range.min..=auto_exposure.range.max)
        .find(|index| {
            u32::try_from(*index).is_ok_and(|index| !auto_exposure.menu.contains_key(&index))
        })
        .expect("the fixture's menu has a hole");
    let error = camera
        .set(auto_exposure.id, ControlValue::Int(hole))
        .expect_err("a menu hole must be refused");
    assert!(
        matches!(
            &error,
            Error::DeviceIo {
                errno: Some(22),
                ..
            }
        ),
        "expected an EINVAL-shaped refusal, got {error}"
    );

    // The other direction: an index the menu does have is accepted exactly.
    let real = auto_exposure
        .menu
        .keys()
        .next()
        .map(|index| i64::from(*index))
        .expect("the menu has items");
    let applied = camera
        .set(auto_exposure.id, ControlValue::Int(real))
        .expect("a real menu index is accepted");
    assert!(applied.is_exact());
    assert!(applied.warnings.is_empty());
}

#[test]
fn a_read_only_control_refuses_the_write() {
    // PF:12 — the Chicony's `Privacy`. The tool honors it and never works around it (§5).
    let backend = backend();
    let mut camera = open_first(&backend);
    let privacy = descriptor(&camera, "privacy");
    assert!(privacy.flags.has(KnownFlag::ReadOnly));

    let error = camera
        .set(privacy.id, ControlValue::Int(1))
        .expect_err("a read-only control must refuse");
    assert!(
        matches!(&error, Error::ControlReadOnly { control } if control == &privacy.slug),
        "{error}"
    );
    assert_eq!(
        camera.get(privacy.id).expect("read"),
        privacy.current.expect("the fixture gives it a value"),
        "a refused write must not have moved it"
    );
}

#[test]
fn out_of_range_currents_and_defaults_are_replayed_verbatim() {
    // PF:4 and PF:5 — the device is the only authority on itself, including when it
    // disagrees with itself.
    let backend = backend();
    let profile = backend.profiles().into_iter().next().expect("one profile");
    let mut camera = open_first(&backend);

    let zoom = descriptor(&camera, "zoom_continuous");
    let recorded = profile
        .state
        .values
        .get(&zoom.slug)
        .expect("the state block records it");
    assert_eq!(zoom.current.as_ref(), Some(recorded));
    assert!(zoom.current_out_of_range());
    assert_eq!(camera.get(zoom.id).expect("read"), *recorded);

    let power_line = descriptor(&camera, "power_line_frequency");
    let declared = profile
        .control(&power_line.slug)
        .expect("the invariant section has it");
    assert_eq!(power_line.default, declared.default);
    assert!(power_line.default_out_of_range());
}

#[test]
fn an_unknown_control_round_trips_its_opaque_payload() {
    // PF:1 — a type this build cannot interpret still enumerates, reads and writes.
    let backend = backend();
    let mut camera = open_first(&backend);
    let roi = descriptor(&camera, "region_of_interest_rectangle");
    assert!(matches!(roi.control_type, ControlType::Unknown { .. }));

    let Some(ControlValue::Bytes(original)) = roi.current.clone() else {
        panic!("the fixture gives it a payload");
    };
    let mut rewritten = original.clone();
    rewritten.reverse();
    let applied = camera
        .set(roi.id, ControlValue::Bytes(rewritten.clone()))
        .expect("an opaque payload is writable");
    assert_eq!(applied.applied, ControlValue::Bytes(rewritten.clone()));
    assert_eq!(
        camera.get(roi.id).expect("read"),
        ControlValue::Bytes(rewritten)
    );

    // A payload of the wrong size is refused, because a driver checks `elem_size × elems`.
    let error = camera
        .set(roi.id, ControlValue::Bytes(vec![0; original.len() + 1]))
        .expect_err("a mis-sized payload must be refused");
    assert!(matches!(error, Error::DeviceIo { .. }), "{error}");
}

#[test]
fn enumeration_relabels_the_backend_and_keeps_everything_else() {
    let backend = backend();
    let profile = fixtures::synthetic_basic();
    let enumerated = backend
        .enumerate()
        .expect("enumerate")
        .into_iter()
        .next()
        .expect("one camera");

    // The one field the fake must not replay: a fake run is never mistakable for a
    // hardware one, whatever the captured document said.
    assert_eq!(profile.invariant.info.backend, BackendKind::V4l2);
    assert_eq!(enumerated.backend, BackendKind::Fake);
    // Everything that describes the device is the document's.
    assert_eq!(enumerated.fingerprint, profile.invariant.info.fingerprint);
    assert_eq!(enumerated.card, profile.invariant.info.card);
    assert_eq!(enumerated.nodes, profile.invariant.info.nodes);
    assert_eq!(
        backend.enumerate().expect("enumerate")[0].id,
        enumerated.id,
        "enumeration must be stable across calls"
    );
}

#[test]
fn two_copies_of_one_profile_are_two_cameras_with_distinct_ids() {
    // The id is re-derived through D1's collision rules rather than replayed, or a
    // multi-camera fake would hand out one id that means two devices.
    let backend = FakeBackend::new(vec![
        fixtures::synthetic_basic(),
        fixtures::synthetic_basic(),
    ])
    .expect("two profiles replay");
    let cameras = backend.enumerate().expect("enumerate");
    assert_eq!(cameras.len(), 2);
    assert_ne!(cameras[0].id, cameras[1].id);
    assert_eq!(cameras[1].id.body(), format!("{}-2", cameras[0].id.body()));
}

#[test]
fn a_second_handle_sees_the_first_handles_writes() {
    // Two opens are two views of one device, not two devices. A fake that cloned its
    // state per handle would make the engine's exclusive-streaming rule (D12) look
    // unnecessary.
    let backend = backend();
    let info = backend.enumerate().expect("enumerate").remove(0);
    let mut first = backend.open_fake(&info.id).expect("open");
    let second = backend.open_fake(&info.id).expect("open again");

    let brightness = descriptor(&first, "brightness");
    let moved = brightness.range.min + brightness.range.effective_step();
    first
        .set(brightness.id, ControlValue::Int(moved))
        .expect("write");
    assert_eq!(
        descriptor(&second, "brightness").current,
        Some(ControlValue::Int(moved))
    );
}

#[test]
fn the_focus_optimum_is_the_profiles_declared_default() {
    // The independence this test exists for (docs/3 Part C, "the fake validating the
    // fake"): the expectation is read out of the *document*, and the model is then asked
    // whether it agrees.
    let backend = backend();
    let profile = backend.profiles().into_iter().next().expect("one profile");
    let declared = profile
        .control(&slug("focus_absolute"))
        .expect("the fixture has a focus control");
    let camera = open_first(&backend);

    assert_eq!(camera.focus_optimum(), Some(declared.default));
}

#[test]
fn frames_are_sharpest_at_the_focus_optimum_stated_by_the_profile() {
    let backend = backend();
    let profile = backend.profiles().into_iter().next().expect("one profile");
    let focus = profile
        .control(&slug("focus_absolute"))
        .expect("the fixture has a focus control")
        .clone();
    let mut camera = open_first(&backend);

    let sharpness_at = |camera: &mut FakeCamera, value: i64| {
        camera
            .set(focus.id, ControlValue::Int(value))
            .expect("focus is writable");
        metrics::sharpness(&steady_frame(camera).luma())
    };

    let peak = sharpness_at(&mut camera, focus.default);
    for detuned in [focus.range.min, focus.range.max] {
        let score = sharpness_at(&mut camera, detuned);
        assert!(
            score < peak,
            "focus {detuned} scored {score} against the optimum's {peak}"
        );
    }
}

#[test]
fn frames_respond_to_the_brightness_control() {
    let backend = backend();
    let mut camera = open_first(&backend);
    let brightness = descriptor(&camera, "brightness");

    let luma_at = |camera: &mut FakeCamera, value: i64| {
        camera
            .set(brightness.id, ControlValue::Int(value))
            .expect("brightness is writable");
        metrics::mean_luma(&steady_frame(camera).luma())
    };

    let dark = luma_at(&mut camera, brightness.range.min);
    let bright = luma_at(&mut camera, brightness.range.max);
    assert!(dark < bright, "{dark} should be darker than {bright}");
}

#[test]
fn negotiation_reports_every_way_it_differed_from_the_request() {
    let backend = backend();
    let mut camera = open_first(&backend);

    // Exactly what was asked for: no adjustments, and an empty list is a claim (D5).
    let exact = camera
        .start_stream(&StreamRequest {
            pixel_format: Some(PixelFormat::MJPG),
            width: Some(1_920),
            height: Some(1_080),
            ..StreamRequest::default()
        })
        .expect("MJPG 1920x1080 is on the profile");
    assert!(exact.is_exact(), "{:?}", exact.adjustments);
    camera.stop_stream().expect("stop");

    // PF:9 as a negotiation: YUYV stops at 640x480 on this device even though MJPG goes
    // to 4K, so asking for 1920x1080 in YUYV must come back adjusted rather than obeyed.
    let adjusted = camera
        .start_stream(&StreamRequest {
            pixel_format: Some(PixelFormat::YUYV),
            width: Some(1_920),
            height: Some(1_080),
            ..StreamRequest::default()
        })
        .expect("YUYV negotiates down");
    assert_eq!(adjusted.pixel_format, PixelFormat::YUYV);
    assert_eq!((adjusted.width, adjusted.height), (640, 480));
    assert!(
        adjusted
            .adjustments
            .iter()
            .any(|a| matches!(a, Adjustment::Size { .. })),
        "{:?}",
        adjusted.adjustments
    );
    camera.stop_stream().expect("stop");

    // A format the device does not offer at all is a typed refusal naming what it does.
    let error = camera
        .start_stream(&StreamRequest {
            pixel_format: Some(PixelFormat::NV12),
            ..StreamRequest::default()
        })
        .expect_err("NV12 is not on the profile");
    assert!(
        matches!(&error, Error::FormatUnsupported { available, .. } if !available.is_empty()),
        "{error}"
    );
}

#[test]
fn a_camera_with_no_capture_node_is_listed_and_refused() {
    // D1: a metadata-only group is a real shape. It enumerates, and streaming it is a
    // typed refusal rather than a surprise.
    let mut profile = fixtures::synthetic_basic();
    profile
        .invariant
        .info
        .nodes
        .retain(|node| node.kind != NodeKind::VideoCapture);
    let backend = FakeBackend::from_profile(profile).expect("replays");
    let mut camera = open_first(&backend);

    assert!(camera.info().capture_node().is_none());
    let error = camera
        .start_stream(&StreamRequest::default())
        .expect_err("a metadata-only camera cannot stream");
    assert!(
        matches!(&error, Error::FormatUnsupported { available, .. } if available.is_empty()),
        "{error}"
    );
    // Availability is not capability (E3): it still enumerates its controls.
    assert!(!camera.controls().expect("controls").is_empty());
}

#[test]
fn a_second_stream_on_one_node_is_busy_and_stopping_twice_is_not() {
    // V4L2 allows one streamer per node and says EBUSY (D12); `STREAMOFF` on a stopped
    // stream is a no-op. Both are the device's manners, not this backend's opinion.
    let backend = backend();
    let mut camera = open_first(&backend);
    camera
        .start_stream(&StreamRequest::default())
        .expect("the first stream");
    let error = camera
        .start_stream(&StreamRequest::default())
        .expect_err("the second must be refused");
    assert!(matches!(&error, Error::Busy { .. }), "{error}");

    camera.stop_stream().expect("stop");
    camera
        .stop_stream()
        .expect("stopping twice is not an error");
    camera
        .start_stream(&StreamRequest::default())
        .expect("the node is free again");
    camera.stop_stream().expect("stop");
}

#[test]
fn a_write_to_an_inactive_control_is_accepted_the_way_hardware_accepts_it() {
    // Deliberately *not* a refusal. Real hardware takes the write and lets the automation
    // overwrite it on the next frame [PF:3]; inventing a `ControlInactive` error here
    // would be a capability no device has, and would hide that avoiding the futile write
    // is the engine's job (D3's guarded set).
    let backend = backend();
    let mut camera = open_first(&backend);
    let manual = descriptor(&camera, "white_balance_temperature");
    assert!(manual.is_inactive(), "the fixture captures it INACTIVE");

    let target = manual.range.min + manual.range.effective_step();
    let applied = camera
        .set(manual.id, ControlValue::Int(target))
        .expect("an inactive control still takes the write");
    assert_eq!(applied.applied, ControlValue::Int(target));
    assert!(
        descriptor(&camera, "white_balance_temperature").is_inactive(),
        "and it is still inactive afterwards"
    );
}

#[test]
fn unknown_cameras_and_controls_are_named_refusals() {
    let backend = backend();
    let missing = schema::camera::CameraId::parse("cam:nope").expect("a literal id");
    assert!(matches!(
        backend.open_fake(&missing),
        Err(Error::CameraUnknown { .. })
    ));

    let mut camera = open_first(&backend);
    let error = camera
        .get(schema::control::ControlId(0xdead_beef))
        .expect_err("no such control");
    // The refusal lists what the camera does have, because a caller who mistyped a slug
    // needs the alternatives more than the error code (D13).
    assert!(
        matches!(&error, Error::ControlUnknown { did_you_mean, .. } if !did_you_mean.is_empty()),
        "{error}"
    );
}

#[test]
fn a_profile_from_a_foreign_schema_version_is_refused_before_it_is_replayed() {
    let mut profile: DeviceProfile = fixtures::synthetic_basic();
    profile.schema_version += 1;
    let error = FakeBackend::from_profile(profile).expect_err("a foreign version is refused");
    assert!(
        matches!(error, Error::SchemaVersionForeign { .. }),
        "{error}"
    );
}

/// Stream until the sensor has settled and return the last frame, decoded.
///
/// \[PF:11\] — the first frames are unsettled by design, so a test comparing image content
/// across control values has to look past them or it is comparing the settle ramp.
fn steady_frame(camera: &mut FakeCamera) -> imaging::decode::Decoded {
    let request = StreamRequest {
        pixel_format: Some(PixelFormat::YUYV),
        width: Some(320),
        height: Some(240),
        ..StreamRequest::default()
    };
    camera.start_stream(&request).expect("start");
    let mut last = None;
    for _ in 0..=fake::frames::SETTLE_FRAMES {
        last = Some(
            camera
                .next_frame(Instant::now() + Duration::from_secs(1))
                .expect("a frame"),
        );
    }
    camera.stop_stream().expect("stop");
    imaging::decode::decode_frame(&last.expect("at least one frame")).expect("decode")
}
