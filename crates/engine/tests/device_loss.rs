//! What a camera vanishing mid-stream has to look like (design D19; FR-W6).
//!
//! D19 states this contract **in advance of the measurement**, and that ordering is the whole
//! point of the entry: the privileged helper refuses to unload `uvcvideo` under an open node
//! (§2.13), so the event exists on this host only as the fake's scripted fault (§3.3 item 9).
//! A sibling project can produce the real one reproducibly — drop the tunnel, detach the vhci
//! port, and a camera vanishes under the driver exactly as a yanked cable does — and the one
//! thing that makes contributed evidence worth having is that the rig tests **this design's
//! claim** rather than blessing whatever the code happens to do. So the claim is written here
//! first, driven against the double, and the `hw_gone_*` recipes in
//! `crates/backends/v4l2/tests/hardware.rs` are the same sentences waiting for a rig.
//!
//! Every arm below is one sentence of D19, and the sentences are spelled entirely in
//! vocabulary that already existed: no API changed for this contract, and no error kind.

use std::io::Read;

use camino::Utf8PathBuf;
use engine::facade::Facade;
use fake::{FakeBackend, Fault};
use schema::backend::{Camera, CameraBackend};
use schema::capture::{PhotoFormat, PhotoRequest, SettlePolicy, SettleSpec, Sink, StreamRequest};
use schema::error::{Error, ErrorKind};
use schema::selector::CameraSelector;
use schema::time::Stamp;
use schema::video::RecordRequest;

/// A backend replaying the whole corpus, so "the machine" has more than one camera on it —
/// which is what makes "the listing stopped naming *that* camera" a claim rather than an
/// observation that everything went away.
fn machine() -> FakeBackend {
    let profiles = testkit::corpus::load_all()
        .expect("the corpus parses")
        .into_iter()
        .map(|(_path, profile)| profile)
        .collect();
    FakeBackend::new(profiles).expect("the corpus replays")
}

fn first(backend: &FakeBackend) -> CameraSelector {
    CameraSelector::Id(
        backend
            .enumerate()
            .expect("enumerates")
            .into_iter()
            .next()
            .expect("a camera")
            .id,
    )
}

#[test]
fn during_a_photo_the_answer_is_device_gone_and_never_a_deadline_or_a_holder() {
    // D19's first sentence, and the three kinds it is *not* are the assertion: the deadline
    // did not expire, nobody else holds the device, and the camera did not decline a
    // capability. An agent's next move differs for each of those — wait, retry, stop and tell
    // a human — so collapsing them is the defect AGENTS rule 7 names.
    // Scripted on the backend before the facade takes it: the fault queue is shared with
    // every camera the backend opens, so a fault queued here fires on the frame the facade's
    // own capture asks for.
    let backend = machine();
    let selector = first(&backend);
    backend.queue_fault(Fault::DeviceGoneMidStream);
    let facade = Facade::new(Box::new(backend));
    let refused = facade
        .photo(
            &selector,
            &PhotoRequest {
                stream: StreamRequest::default(),
                settle: SettlePolicy {
                    spec: SettleSpec::SkipFrames { frames: 0 },
                    deadline_ms: 5_000,
                },
                sink: Sink::ReturnBytes {
                    format: PhotoFormat::Png,
                },
                transform: schema::capture::Transform::default(),
                wait: false,
            },
            &mut engine::photo::WhereverTheCallerSaid,
            Stamp::epoch(),
        )
        .expect_err("the device vanished");

    assert_eq!(refused.kind(), ErrorKind::DeviceGone, "{refused}");
    for never in [
        ErrorKind::SettleTimeout,
        ErrorKind::Busy,
        ErrorKind::FormatUnsupported,
    ] {
        assert_ne!(refused.kind(), never, "{refused}");
    }
    // And the refusal names the node, because "stop and tell a human" is what this kind means
    // and a human needs to be told which device.
    let Error::DeviceGone { path } = &refused else {
        panic!("expected DeviceGone, got {refused:?}");
    };
    assert!(path.as_str().starts_with("/dev/"), "{path}");
}

#[test]
fn during_a_recording_the_take_finalizes_valid_to_the_last_frame_with_its_end_named() {
    // D19's second sentence, driven turn by turn — which is the shape the daemon records in
    // and the only shape in which the sentence is even expressible: `record_stop` collects a
    // report from a take somebody else was driving, so the loss has to arrive *between*
    // frames rather than instead of the first one.
    //
    // Three claims: the container is closed over the frames that did arrive (D7's "every
    // fault leaves a parseable file", with this build's own independent re-parser as the
    // oracle), the ending names the device failure rather than a duration or a cap, and the
    // stream statistics are carried — the gap accounting right up to the loss is exactly the
    // measurement a forwarding rig is after (D16).
    let backend = machine();
    // A camera whose negotiated format an AVI can carry, chosen by asking the enumeration
    // rather than by naming a profile: the corpus holds a greyscale camera too, and a `.avi`
    // request against that one is refused before any device could vanish (D5's container
    // pairing). Which camera has MJPG is a fact about the corpus, so it is read from it.
    let camera_id = backend
        .enumerate()
        .expect("enumerates")
        .into_iter()
        .find(|info| {
            backend
                .open_fake(&info.id)
                .ok()
                .and_then(|camera| camera.formats().ok())
                .is_some_and(|formats| {
                    formats
                        .iter()
                        .any(|format| format.pixel_format.to_string() == "MJPG")
                })
        })
        .expect("some committed camera offers MJPG")
        .id;
    let scratch = engine::paths::scratch_dir().expect("a scratch directory");
    let path = Utf8PathBuf::from_path_buf(scratch.path().join("interrupted.avi"))
        .expect("a UTF-8 scratch path");

    let mut camera = backend.open_fake(&camera_id).expect("opens");
    let request = RecordRequest {
        stream: StreamRequest::default(),
        duration_ms: Some(60_000),
        sink: Sink::ServerPath { path: path.clone() },
        wait: false,
    };
    let clock = engine::settle::MonotonicClock::new();
    let opened = engine::record::start(&mut camera, &request).expect("the stream starts");
    let mut recording = engine::record::Recording::begin(
        &opened,
        &path,
        &mut engine::record::OnDisk,
        &clock,
        Stamp::epoch(),
    )
    .expect("the file opens");

    // Frames, then the loss. Four is arbitrary and small; what matters is that it is more
    // than zero, because a file closed with nothing in it would satisfy "parseable" and
    // salvage nothing.
    let mut written = 0_u32;
    for _ in 0..4 {
        let engine::record::Turn::Frame(frame) =
            engine::record::turn(&mut camera).expect("a frame arrives")
        else {
            panic!("the fake went idle on a take it was delivering");
        };
        recording.write(&frame).expect("the frame is written");
        written += 1;
    }
    assert_eq!(written, 4);

    backend.queue_fault(Fault::DeviceGoneMidStream);
    let refused = engine::record::turn(&mut camera).expect_err("the device vanished");
    assert_eq!(refused.kind(), ErrorKind::DeviceGone, "{refused}");

    // The take finalizes, and its end names the device failure.
    let report = recording
        .finish(schema::video::RecordingEnd::DeviceFailed, &clock)
        .expect("the container closes over what arrived");
    assert_eq!(report.ended, schema::video::RecordingEnd::DeviceFailed);
    assert_eq!(report.summary.frames_written, written);
    assert_eq!(
        report.stats.frames_delivered,
        u64::from(written),
        "the stats stop at the loss rather than counting the frame that never came"
    );
    assert_eq!(
        report.stats.frames_dropped, 0,
        "nothing was dropped before the loss"
    );
    assert!(
        report.stats.intervals.is_some(),
        "four frames span intervals, and a rig measuring a forwarded link reads them"
    );

    // And the file is a file: a RIFF header, and every frame readable by the independent
    // re-parser P6a wrote from the specification before the muxer existed.
    let mut bytes = Vec::new();
    std::fs::File::open(path.as_std_path())
        .expect("the interrupted take left a file")
        .read_to_end(&mut bytes)
        .expect("the file reads");
    assert_eq!(
        bytes.get(..4),
        Some(b"RIFF".as_slice()),
        "the interrupted take did not leave a RIFF file"
    );
    let stream = imaging::avi::read::read_stream(&bytes)
        .expect("the independent re-parser reads the interrupted file");
    assert_eq!(
        u32::try_from(stream.frames.len()).unwrap_or(u32::MAX),
        written,
        "the file holds a different number of frames than the report claims"
    );
}

#[test]
fn around_the_loss_the_listing_stops_naming_the_camera_and_the_others_stay() {
    // D19's fourth sentence. The corpus gives this machine several cameras, so "the listing
    // lost one" is separable from "enumeration broke" — which is the whole reason this arm
    // replays the corpus rather than one profile.
    let backend = machine();
    let before = backend.enumerate().expect("enumerates");
    assert!(
        before.len() >= 2,
        "a one-camera machine cannot distinguish a lost camera from a lost enumeration"
    );

    let selector = first(&backend);
    let CameraSelector::Id(lost) = selector.clone() else {
        panic!("the fixture built something other than an id: {selector:?}");
    };
    let mut camera = backend.open_fake(&lost).expect("opens");
    camera
        .start_stream(&StreamRequest::default())
        .expect("starts");
    backend.queue_fault(Fault::DeviceGoneMidStream);
    let refused = camera
        .next_frame(std::time::Instant::now() + std::time::Duration::from_secs(1))
        .expect_err("the device vanished");
    assert_eq!(refused.kind(), ErrorKind::DeviceGone, "{refused}");

    let after = backend.enumerate().expect("enumerates");
    assert!(
        after.iter().all(|info| info.id != lost),
        "the listing still names {lost} after it vanished mid-stream"
    );
    assert_eq!(
        after.len(),
        before.len() - 1,
        "the loss of one camera took others with it: {after:#?}"
    );

    // And resolution follows the listing, which is the sentence a consumer actually meets:
    // the id it was holding a moment ago now names nothing. `CameraUnknown` and not
    // `DeviceGone`, because resolution is answering about a request rather than about a
    // device it is holding — which is the distinction D14 and D19 share.
    assert!(matches!(
        engine::resolve::camera(&after, &selector),
        Err(Error::CameraUnknown { .. })
    ));
}

#[test]
fn a_camera_that_did_not_vanish_is_untouched_by_one_that_did() {
    // The inverse arm, and the one that makes the three above mean something: a fake that
    // emptied its listing, or refused everything after any fault, would satisfy every
    // assertion in this file and describe a machine nobody has.
    let backend = machine();
    let cameras = backend.enumerate().expect("enumerates");
    let lost = cameras.first().expect("a camera").id.clone();
    let survivor = cameras.get(1).expect("a second camera").id.clone();

    let mut camera = backend.open_fake(&lost).expect("opens");
    camera
        .start_stream(&StreamRequest::default())
        .expect("starts");
    backend.queue_fault(Fault::DeviceGoneMidStream);
    camera
        .next_frame(std::time::Instant::now() + std::time::Duration::from_secs(1))
        .expect_err("the device vanished");

    let mut other = backend
        .open_fake(&survivor)
        .expect("the other camera still opens");
    other
        .start_stream(&StreamRequest::default())
        .expect("and still streams");
    let frame = other
        .next_frame(std::time::Instant::now() + std::time::Duration::from_secs(1))
        .expect("and still delivers frames");
    assert!(!frame.bytes.is_empty());
    assert!(
        backend
            .enumerate()
            .expect("enumerates")
            .iter()
            .any(|info| info.id == survivor),
        "the surviving camera left the listing with the lost one"
    );
}
