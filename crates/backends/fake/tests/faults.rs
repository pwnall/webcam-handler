//! The fault menu, walked exhaustively (design §2.9).
//!
//! *A fault the compiler cannot force the fake to script is a fault nobody tests.* The
//! walk below matches on [`Fault`] rather than iterating a list of test names, so adding
//! a variant to the menu stops this build until somebody says what observing it looks
//! like.
//!
//! Each fault is also checked in the other direction, in
//! `no_fault_fires_unless_it_was_scripted`: a menu whose faults leaked into unscripted
//! runs would make every other test in this crate flaky, and a menu whose faults never
//! fire would make this file theatre.

use std::time::{Duration, Instant};

use fake::{FakeBackend, FakeCamera, Fault};
use schema::backend::{Camera, CameraBackend, HotplugEvent};
use schema::camera::PixelFormat;
use schema::capture::{Frame, StreamRequest};
use schema::control::{ControlDesc, ControlSlug, ControlValue, KnownFlag, WriteWarning};
use schema::error::Error;
use testkit::fixtures;

#[test]
fn every_fault_in_the_menu_is_observable() {
    for &fault in Fault::ALL {
        // Exhaustive on purpose: this match is the mechanism, and the loop is only here
        // so a variant that is *never dispatched* also fails.
        match fault {
            Fault::DeviceGoneMidStream => device_gone_mid_stream(),
            Fault::Busy => busy(),
            Fault::ClampOnWrite => clamp_on_write(),
            Fault::InactiveFlip => inactive_flip(),
            Fault::SettleNeverConverges => settle_never_converges(),
            Fault::FrameTimeout => frame_timeout(),
            Fault::HotplugAdd => hotplug_add(),
            Fault::HotplugRemove => hotplug_remove(),
        }
    }
}

#[test]
fn no_fault_fires_unless_it_was_scripted() {
    // The inverse of the whole file. Every observation the walk above makes must be
    // absent from a run nobody scripted, or the fake is failing on its own initiative.
    let backend = backend();
    let mut camera = backend.open_fake(&first_id(&backend)).expect("open");
    assert!(backend.pending_faults().is_empty());
    assert!(backend.held_faults().is_empty());

    // Busy, ClampOnWrite.
    let brightness = descriptor(&camera, "brightness");
    let in_range = brightness.range.min + brightness.range.effective_step();
    let applied = camera
        .set(brightness.id, ControlValue::Int(in_range))
        .expect("an in-range write");
    assert!(applied.is_exact());
    assert!(applied.warnings.is_empty());

    // InactiveFlip.
    assert_eq!(flag_words(&camera), flag_words(&camera));

    // DeviceGoneMidStream, FrameTimeout, SettleNeverConverges.
    let frames = stream_frames(&mut camera, SETTLE_PLUS_TWO);
    assert_eq!(
        frames.last().map(|f| &f.bytes),
        frames.get(frames.len() - 2).map(|f| &f.bytes),
        "an unscripted stream settles"
    );

    // HotplugAdd, HotplugRemove.
    let mut watch = backend.watch().expect("watch");
    assert_eq!(watch.next_event(Instant::now()).expect("poll"), None);
}

#[test]
fn a_held_fault_keeps_firing_and_a_queued_one_does_not() {
    let backend = backend();
    backend.queue_fault(Fault::Busy);
    assert_eq!(backend.pending_faults(), vec![Fault::Busy]);
    let id = first_id(&backend);
    assert!(backend.open_fake(&id).is_err());
    assert!(backend.pending_faults().is_empty());
    assert!(backend.open_fake(&id).is_ok(), "a one-shot fired twice");

    backend.hold_fault(Fault::SettleNeverConverges);
    assert_eq!(backend.held_faults(), vec![Fault::SettleNeverConverges]);
    backend.release_fault(Fault::SettleNeverConverges);
    assert!(backend.held_faults().is_empty());

    // Scripting several at once keeps their order, and each still fires exactly once —
    // a queue that deduplicated would silently halve a test's expectations.
    backend.queue_faults(&[Fault::FrameTimeout, Fault::Busy, Fault::FrameTimeout]);
    assert_eq!(
        backend.pending_faults(),
        vec![Fault::FrameTimeout, Fault::Busy, Fault::FrameTimeout]
    );
}

// ------------------------------------------------------------------ the observations

fn device_gone_mid_stream() {
    let backend = backend();
    let mut camera = backend.open_fake(&first_id(&backend)).expect("open");
    start(&mut camera);
    camera.next_frame(soon()).expect("a frame before the fault");

    backend.queue_fault(Fault::DeviceGoneMidStream);
    let error = camera.next_frame(soon()).expect_err("the device vanished");
    assert!(matches!(&error, Error::DeviceGone { .. }), "{error}");

    // The stream is gone with it: a device that unplugged is not a device that is between
    // frames.
    let after = camera.next_frame(soon()).expect_err("no stream is running");
    assert!(matches!(&after, Error::DeviceIo { .. }), "{after}");
}

fn busy() {
    let backend = backend();
    let id = first_id(&backend);
    backend.queue_fault(Fault::Busy);
    let error = backend.open_fake(&id).expect_err("somebody else has it");
    // E3: busy names a path and a (possibly unknown) holder — it is availability, and
    // nothing here may turn it into "this camera cannot do that".
    assert!(matches!(&error, Error::Busy { .. }), "{error}");
}

fn clamp_on_write() {
    let backend = backend();
    let mut camera = backend.open_fake(&first_id(&backend)).expect("open");
    let brightness = descriptor(&camera, "brightness");
    let in_range = brightness.range.min + brightness.range.effective_step();

    backend.queue_fault(Fault::ClampOnWrite);
    let applied = camera
        .set(brightness.id, ControlValue::Int(in_range))
        .expect("a clamp is a success [PF:6]");
    assert_eq!(applied.requested, ControlValue::Int(in_range));
    assert_eq!(applied.applied, ControlValue::Int(brightness.range.max));

    // `Adjusted`, and specifically **not** `Clamped`. The request was inside the declared
    // range, so nothing the caller can see explains the move: the driver's real range is
    // narrower than the one it publishes, and that is not deducible from the publication.
    // Reporting it as `Clamped { requested: 1, applied: 255, range: [0..255] }` would be a
    // sentence that contradicts itself — 1 is in [0..255] — and a caller reading it would
    // conclude their own value was out of bounds.
    assert_eq!(
        applied.warnings,
        vec![WriteWarning::Adjusted {
            requested: ControlValue::Int(in_range),
            applied: ControlValue::Int(brightness.range.max),
        }],
        "an unattributable adjustment must not borrow the clamp's explanation"
    );

    // The other direction, so the assertion above is not simply "some warning": a write
    // that really *was* past the maximum reports the clamp, with the range that did it.
    let beyond = brightness.range.max + 1_000;
    let clamped = camera
        .set(brightness.id, ControlValue::Int(beyond))
        .expect("an out-of-range write is a clamped success [PF:6]");
    assert_eq!(
        clamped.warnings,
        vec![WriteWarning::Clamped {
            requested: beyond,
            applied: brightness.range.max,
            range: brightness.range,
        }]
    );
}

fn inactive_flip() {
    let backend = backend();
    let profile = backend.profiles().into_iter().next().expect("one profile");
    let camera = backend.open_fake(&first_id(&backend)).expect("open");
    let partners: Vec<ControlSlug> = profile
        .invariant
        .measured_pairs
        .iter()
        .map(|pair| pair.manual.clone())
        .collect();
    assert!(!partners.is_empty(), "the fixture measured pairs");

    let before = flag_words(&camera);
    backend.queue_fault(Fault::InactiveFlip);
    let after = flag_words(&camera);

    for ((name, was), (again, now)) in before.iter().zip(&after) {
        assert_eq!(name, again);
        if partners.contains(name) {
            assert_ne!(
                was & KnownFlag::Inactive.bit(),
                now & KnownFlag::Inactive.bit(),
                "{name}'s INACTIVE bit should have flipped"
            );
        } else {
            assert_eq!(was, now, "{name} moved and is not a measured partner");
        }
    }
}

fn settle_never_converges() {
    let backend = backend();
    let mut camera = backend.open_fake(&first_id(&backend)).expect("open");

    backend.hold_fault(Fault::SettleNeverConverges);
    let unsettled = stream_frames(&mut camera, SETTLE_PLUS_TWO);
    let last = unsettled.last().expect("frames");
    let previous = unsettled.get(unsettled.len() - 2).expect("frames");
    assert_ne!(
        last.bytes, previous.bytes,
        "frames past the settle window must still be moving [PF:11]"
    );

    // Released, the same stream converges — which is what makes the fault a fault rather
    // than the fake's normal behaviour.
    backend.release_fault(Fault::SettleNeverConverges);
    let settled = stream_frames(&mut camera, SETTLE_PLUS_TWO);
    assert_eq!(
        settled.last().map(|f| &f.bytes),
        settled.get(settled.len() - 2).map(|f| &f.bytes)
    );
}

fn frame_timeout() {
    let backend = backend();
    let mut camera = backend.open_fake(&first_id(&backend)).expect("open");
    start(&mut camera);
    camera.next_frame(soon()).expect("a frame before the fault");

    backend.queue_fault(Fault::FrameTimeout);
    let error = camera.next_frame(soon()).expect_err("no frame arrived");
    // D13 has one timeout variant, and it carries how many frames did arrive — which is
    // what separates "the camera is slow" from "the camera is dead" (E3).
    let Error::SettleTimeout { frames_seen, .. } = error else {
        panic!("expected a timeout, got {error}");
    };
    assert_eq!(frames_seen, 1);
}

fn hotplug_add() {
    let backend = backend();
    let mut watch = backend.watch().expect("watch");
    backend.queue_fault(Fault::HotplugAdd);

    let event = watch.next_event(Instant::now()).expect("poll");
    assert!(
        matches!(&event, Some(HotplugEvent::Added { path }) if !path.as_str().is_empty()),
        "{event:?}"
    );
    // One shot: the node does not keep arriving.
    assert_eq!(watch.next_event(Instant::now()).expect("poll"), None);
}

fn hotplug_remove() {
    let backend = backend();
    let mut watch = backend.watch().expect("watch");
    backend.queue_fault(Fault::HotplugRemove);

    let event = watch.next_event(Instant::now()).expect("poll");
    assert!(
        matches!(&event, Some(HotplugEvent::Removed { path }) if !path.as_str().is_empty()),
        "{event:?}"
    );
}

// ------------------------------------------------------------------------- helpers

/// Two frames past the settle window, so "did it stop moving?" has two frames to compare.
const SETTLE_PLUS_TWO: u32 = fake::frames::SETTLE_FRAMES + 2;

fn backend() -> FakeBackend {
    FakeBackend::from_profile(fixtures::synthetic_basic()).expect("the fixture replays")
}

fn first_id(backend: &FakeBackend) -> schema::camera::CameraId {
    backend
        .enumerate()
        .expect("enumerate")
        .into_iter()
        .next()
        .expect("one camera")
        .id
}

fn descriptor(camera: &FakeCamera, name: &str) -> ControlDesc {
    let slug = ControlSlug::parse(name).expect("a literal slug");
    camera
        .controls()
        .expect("controls")
        .into_iter()
        .find(|desc| desc.slug == slug)
        .unwrap_or_else(|| panic!("{name} is not in the replayed control set"))
}

fn flag_words(camera: &FakeCamera) -> Vec<(ControlSlug, u32)> {
    camera
        .controls()
        .expect("controls")
        .into_iter()
        .map(|desc| (desc.slug, desc.flags.raw))
        .collect()
}

/// A deadline far enough out that only a scripted fault can beat it. Not a sleep: nothing
/// in this crate waits (N3).
fn soon() -> Instant {
    Instant::now() + Duration::from_secs(1)
}

fn start(camera: &mut FakeCamera) {
    camera
        .start_stream(&StreamRequest {
            // Small and uncompressed, so a frame-to-frame comparison is byte-exact and
            // cheap.
            pixel_format: Some(PixelFormat::YUYV),
            width: Some(320),
            height: Some(240),
            ..StreamRequest::default()
        })
        .expect("start");
}

fn stream_frames(camera: &mut FakeCamera, count: u32) -> Vec<Frame> {
    start(camera);
    let frames = (0..count)
        .map(|index| {
            camera
                .next_frame(soon())
                .unwrap_or_else(|e| panic!("frame {index}: {e}"))
        })
        .collect();
    camera.stop_stream().expect("stop");
    frames
}
