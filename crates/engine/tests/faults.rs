//! The engine's answer to every fault the fake can script (design §2.9, docs/7 G2).
//!
//! The backend conformance battery asks whether a *backend* behaves; this asks whether the
//! **engine** does, when the backend behaves badly on purpose. Those are different
//! questions and only the first one had a suite: `BatteryArm::FaultMenu` is an
//! unconditional declared skip, because T1/T2 expose no fault-scripting seam for a
//! backend-agnostic suite to reach. So the fault half of G2's criterion is carried here and
//! in the fake's own `tests/faults.rs`, not by the battery arm of that name.
//!
//! The walk is an exhaustive `match` over [`Fault`], for the reason the fake's own menu
//! test gives: a fault the compiler cannot force somebody to answer is a fault nobody
//! answers. Adding a variant to the menu stops this build until the engine has a named
//! behaviour for it.

use fake::{FakeBackend, Fault};
use schema::ErrorKind;
use schema::backend::{Camera, CameraBackend};
use schema::camera::CameraId;
use schema::capture::{
    PhotoFormat, PhotoRequest, SettlePolicy, SettleSpec, Sink, StreamRequest, Transform,
};
use schema::control::{ControlSlug, ControlValue};
use schema::time::Stamp;
use testkit::fixtures;

#[test]
fn every_fault_in_the_menu_has_a_named_engine_behaviour() {
    for &fault in Fault::ALL {
        match fault {
            Fault::ClampOnWrite => clamp_surfaces_as_a_warning(),
            Fault::InactiveFlip => an_inactive_flip_does_not_become_an_unguarded_write(),
            Fault::SettleNeverConverges => a_stream_that_never_settles_times_out(),
            Fault::FrameTimeout => a_frame_that_never_arrives_times_out(),
            Fault::DeviceGoneMidStream => a_device_that_vanishes_is_device_gone(),
            Fault::Busy => a_busy_camera_refuses_without_claiming_it_cannot(),
            // Hotplug is the backend's channel and the engine has no consumer for it at
            // all: `CameraBackend::watch` is forwarded by `engine::actor::Cameras` and read
            // by `daemon::events`, one crate out, where P4e-i's `subscribe_events` drives
            // all four of these. Named rather than silently omitted, and the exhaustive
            // match is what would force this arm to grow a body if the engine ever became a
            // consumer.
            Fault::HotplugAdd
            | Fault::HotplugRemove
            | Fault::WatchUnavailable
            | Fault::WatchFails => {
                println!(
                    "SKIP: {fault} is the daemon's to observe (daemon::events); the engine \
                     forwards the watch and consumes none of it"
                );
            }
        }
    }
}

// --------------------------------------------------------------------- the behaviours

fn clamp_surfaces_as_a_warning() {
    // PF:6 travelling the whole way up: the device took something else, the write
    // *succeeded*, and the report carries both values. An engine that turned this into an
    // error would convert "the device adjusted it" into "the device refused" — the E3
    // confusion D13 exists to prevent.
    let backend = backend();
    let mut camera = open(&backend);
    let target = writable_scalar(camera.as_mut());

    backend.queue_fault(Fault::ClampOnWrite);
    let report = engine::write::set(
        camera.as_mut(),
        &[],
        &[(target.clone(), ControlValue::Int(1))],
        false,
    )
    .expect("a clamp is a success, not a refusal");

    assert!(!report.is_exact(), "the adjustment reached the report");
    let inexact = report.inexact();
    assert_eq!(inexact.len(), 1);
    assert_eq!(inexact[0].slug, target);
    assert!(
        !inexact[0].warnings.is_empty(),
        "an adjustment with no warning is a silent one"
    );
}

fn an_inactive_flip_does_not_become_an_unguarded_write() {
    // The flag moves between the plan and the write — a device changing its mind under
    // us. What must *not* happen is a manual write going out under live automation
    // because the planner read a stale flag: the guard's promise is about the order of two
    // writes, and the plan is built from a reading taken at plan time.
    let backend = backend();
    let mut camera = open(&backend);
    let profile = backend.profiles().into_iter().next().expect("one profile");
    let Some(pair) = profile.invariant.measured_pairs.first().cloned() else {
        println!("SKIP: the fixture measures no pair, so there is no guard to test");
        return;
    };

    backend.queue_fault(Fault::InactiveFlip);
    let outcome = engine::write::set(
        camera.as_mut(),
        std::slice::from_ref(&pair),
        &[(pair.manual.clone(), ControlValue::Int(1))],
        true,
    );

    match outcome {
        // Either the plan switched the partner off first…
        Ok(report) => {
            let wrote_manual = report
                .writes
                .iter()
                .position(|a| a.slug == pair.manual)
                .expect("the target was written");
            // `expect`, not `if let`: a plan that reached the manual write with no
            // switch-off in front of it *is* the unguarded write this arm is named after,
            // and an `if let` would let the defect walk past the only assertion that
            // could catch it. The camera's automation partner is engaged in this fixture
            // — the profile's `measured_pairs` says so — so the switch-off is obligatory,
            // and its absence must be a failure rather than a skipped branch.
            let switch_off = report
                .writes
                .iter()
                .position(|a| a.slug == pair.automation)
                .unwrap_or_else(|| {
                    panic!(
                        "the manual write went out with no switch-off in front of it: {:?}",
                        report.writes
                    )
                });
            assert!(
                switch_off < wrote_manual,
                "the switch-off must precede the manual write: {:?}",
                report.writes
            );
        }
        // …or it refused, naming the control that owns the target. Both are guarded
        // answers; a manual write with neither is not.
        Err(error) => assert!(
            matches!(
                error.kind(),
                ErrorKind::ControlInactive | ErrorKind::ControlReadOnly
            ),
            "a flip must not produce an untyped failure: {error}"
        ),
    }
}

fn a_stream_that_never_settles_times_out() {
    // PF:11's condition, held rather than queued: every frame differs, so no settle
    // policy converges. The engine must give up at *its* deadline and say how many frames
    // it saw — the number that separates "the camera is slow" from "the camera is dead".
    let backend = backend();
    let mut camera = open(&backend);
    backend.hold_fault(Fault::SettleNeverConverges);

    // A `SettleFor` policy whose duration exceeds its deadline: both bounds are honoured
    // and the tighter one wins, which is the configuration a caller reaches by asking for
    // more settling than they allowed time for.
    let error = engine::capture::grab(
        camera.as_mut(),
        &StreamRequest::default(),
        SettlePolicy {
            spec: SettleSpec::SettleFor { millis: 60_000 },
            deadline_ms: 0,
        },
        &engine::settle::SteppedClock::new(0),
    )
    .expect_err("a stream that never settles must not return a photo");
    assert_eq!(error.kind(), ErrorKind::SettleTimeout);
    backend.release_fault(Fault::SettleNeverConverges);
}

fn a_frame_that_never_arrives_times_out() {
    // The device answers `next_frame` with a timeout rather than a frame. The engine's
    // own policy decides whether that is fatal, and with a spent deadline it is.
    let backend = backend();
    let mut camera = open(&backend);
    backend.queue_fault(Fault::FrameTimeout);

    let error = engine::capture::grab(
        camera.as_mut(),
        &StreamRequest::default(),
        SettlePolicy {
            spec: SettleSpec::SkipFrames { frames: 0 },
            deadline_ms: 0,
        },
        &engine::settle::SteppedClock::new(0),
    )
    .expect_err("no frame arrived before the deadline");
    assert_eq!(error.kind(), ErrorKind::SettleTimeout);
}

fn a_device_that_vanishes_is_device_gone() {
    // E3, at the top of the stack: a camera somebody unplugged mid-capture has said
    // nothing about what it can do. The refusal must arrive as `DeviceGone` and not as a
    // settle timeout, because the caller's next move differs — re-enumerate, or wait.
    let backend = backend();
    let mut camera = open(&backend);
    backend.queue_fault(Fault::DeviceGoneMidStream);

    let error = engine::photo::take(
        camera.as_mut(),
        &photo_request(),
        &mut engine::photo::WhereverTheCallerSaid,
        &engine::settle::SteppedClock::new(0),
        Stamp::epoch(),
    )
    .expect_err("the device vanished");
    assert_eq!(
        error.kind(),
        ErrorKind::DeviceGone,
        "an unplugged camera must not be reported as one that cannot settle: {error}"
    );
}

fn a_busy_camera_refuses_without_claiming_it_cannot() {
    // The other half of E3: a device somebody else holds is *unavailable*, not incapable.
    // `FormatUnsupported` would send a caller to buy a different camera.
    let backend = backend();
    let id = first_id(&backend);
    backend.queue_fault(Fault::Busy);
    let error = backend.open(&id).expect_err("somebody else holds it");
    assert_eq!(error.kind(), ErrorKind::Busy);
    assert_ne!(error.kind(), ErrorKind::FormatUnsupported);
}

// ------------------------------------------------------------------------- helpers

fn backend() -> FakeBackend {
    FakeBackend::from_profile(fixtures::synthetic_basic()).expect("the fixture replays")
}

fn first_id(backend: &FakeBackend) -> CameraId {
    backend
        .enumerate()
        .expect("enumerate")
        .into_iter()
        .next()
        .expect("one camera")
        .id
}

fn open(backend: &FakeBackend) -> Box<dyn Camera> {
    let id = first_id(backend);
    backend.open(&id).expect("opens")
}

/// A control this suite may write: scalar, writable, and not owned by automation.
fn writable_scalar(camera: &mut dyn Camera) -> ControlSlug {
    camera
        .controls()
        .expect("controls")
        .into_iter()
        .find(|desc| {
            desc.is_writable()
                && desc.control_type == schema::control::ControlType::Integer
                && !desc.is_inactive()
                && desc.current.is_some()
        })
        .map(|desc| desc.slug)
        .expect("the fixture exposes a writable integer control")
}

fn photo_request() -> PhotoRequest {
    PhotoRequest {
        stream: StreamRequest::default(),
        settle: SettlePolicy {
            spec: SettleSpec::SkipFrames { frames: 0 },
            deadline_ms: 5_000,
        },
        transform: Transform::None,
        sink: Sink::ReturnBytes {
            format: PhotoFormat::Jpeg,
        },
        // These faults are the device's, and D12's flag is about a queue in front of it.
        wait: false,
    }
}
