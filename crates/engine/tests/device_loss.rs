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
//! `crates/backends/v4l2/tests/hardware.rs` are the same sentences waiting for a rig — **one
//! `hw_gone_*` recipe per sentence**, which it was not until 2026-08-20: neither committed
//! recipe opened a hotplug watch and nothing could re-attach, so two clauses had no producer
//! on either side of the rig (notes **N299**, **N300**). The recipes are named rather than
//! counted, here and in N299, because a count of code in prose is a claim something has to
//! reconcile and nothing reconciles this one (N153, N158; note **N301**).
//!
//! Every arm below is one sentence of D19, and the sentences are spelled entirely in
//! vocabulary that already existed: no API changed for this contract, and no error kind.
//!
//! **The sentence that is not here** is the preview's — the feed ending, the viewers'
//! streams closing, the slot reaped rather than stranded. That is a claim about what the
//! *daemon* does with the device's answer rather than about what the device does, so it is
//! driven where the daemon is, by
//! `a_device_that_failed_mid_take_reaches_the_collector_and_never_the_readers_as_a_success`,
//! and the `g8` criterion's selection names that arm beside this binary so "every sentence"
//! is a claim something reconciles rather than a prose count.

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
    //
    // **The ending is supplied here, and that is not the claim this arm makes about it.**
    // `engine::record::run` chooses `RecordingEnd::DeviceFailed` itself and then *discards*
    // the report, because a report answering a request that failed would be a `DeviceGone`
    // reported as a successful recording (rule 7, notes **N115**, **N173**) — so no caller of
    // the shipped composition can obtain the stats at all, and this arm is driven turn by turn
    // to get at them. That the code and not a test picks the ending is asserted where the code
    // picks it: `engine::record`'s own
    // `a_device_that_vanished_mid_recording_leaves_an_indexed_file_and_the_devices_own_words`,
    // and the daemon's
    // `a_take_the_device_refused_is_collected_as_that_refusal_and_never_as_a_report`. Both are
    // in this criterion's selection, so the pair is one claim rather than two files that could
    // drift. D19's bullet was amended to say which caller holds the report, because it named
    // `record_stop` and `record_stop` is the one caller that answers the refusal instead.
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

#[test]
fn around_the_loss_a_watcher_is_told_the_removal_within_the_hotplug_bounds() {
    // D19's fourth sentence, second clause — and until this arm existed it was the one part
    // of the contract neither side of the rig could measure (note **N300**). The fake's loss
    // and the fake's hotplug channel were unconnected: `DeviceGoneMidStream` answered a frame
    // and touched no watch, while `HotplugEvent::Removed` came only from a separate scripted
    // `Fault::HotplugRemove` naming whichever node the backend listed first. So a consumer
    // that had been told to re-enumerate on a removal was never told anything by the event
    // this design exists to describe.
    //
    // **The bound is the assertion and not a wait**, and it is
    // `limits::HOTPLUG_MAX_DEFERRAL_MS` (note **N301**). That is the ceiling this project
    // states on how long a hotplug reading may be deferred, so it is what D19's phrase "the
    // hotplug bounds" names; `HOTPLUG_WATCH_DEADLINE_MS` is how long the daemon's loop parks
    // per turn and its own doc says in bold that it is **not** a bound on delivery, so a
    // twin asserted against it would hold a real stack delivering at 1.5 s to a sentence D19
    // does not make. `HotplugWatch::next_event`'s contract is "block until an event or until
    // this deadline", so a build that produced no removal answers `Ok(None)` at the end of it
    // rather than hanging. Nothing here sleeps: the fake's watch is woken by the announcement
    // itself.
    let backend = machine();
    let cameras = backend.enumerate().expect("enumerates");
    let lost = cameras.first().expect("a camera").clone();
    let nodes: Vec<camino::Utf8PathBuf> = lost.nodes.iter().map(|node| node.path.clone()).collect();

    // Opened before the loss, because a watcher that subscribed afterwards would be asking
    // about a machine that had already changed — and because that is the order a consumer of
    // this daemon runs in.
    let mut watch = backend.watch().expect("this backend gives out a watch");

    let mut camera = backend.open_fake(&lost.id).expect("opens");
    camera
        .start_stream(&StreamRequest::default())
        .expect("starts");
    backend.queue_fault(Fault::DeviceGoneMidStream);
    let refused = camera
        .next_frame(std::time::Instant::now() + std::time::Duration::from_secs(1))
        .expect_err("the device vanished");
    assert_eq!(refused.kind(), ErrorKind::DeviceGone, "{refused}");

    // **One removal per node the camera owned**, which is the shape the real tracker has:
    // `v4l2::hotplug::Tracker::rescan` diffs the node tree and queues a `Removed` for every
    // path that left it, and every profile in `corpus/` owns two nodes or four. A double that
    // announced once for the camera would hand a node-level consumer a tree the machine never
    // had, and the count was asserted the other way here until 2026-08-20 (note **N301**).
    let bound = std::time::Duration::from_millis(schema::limits::HOTPLUG_MAX_DEFERRAL_MS);
    let deadline = std::time::Instant::now() + bound;
    let mut announced = Vec::new();
    for _ in &nodes {
        announced.push(
            watch
                .next_event(deadline)
                .expect("the watch is working")
                .expect(
                    "a camera left the machine and the watch was told about fewer of its \
                     nodes than left, inside the bound",
                ),
        );
    }
    assert_eq!(
        announced,
        nodes
            .iter()
            .map(|path| schema::backend::HotplugEvent::Removed { path: path.clone() })
            .collect::<Vec<_>>(),
        "the watcher was told about the wrong nodes, or the wrong direction"
    );
    assert!(
        nodes.len() > 1,
        "the first corpus camera owns one node, so 'one per node' is untested here: {nodes:?}"
    );

    // **And no more than that**, which is the inverse arm: a fake that announced on every
    // refused frame would tell a consumer the camera left over and over, and a consumer that
    // re-enumerated on each would be doing D19's work every time. The last read is given an
    // already-spent deadline, so it answers immediately and the arm costs nothing.
    let again = watch
        .next_event(std::time::Instant::now())
        .expect("the watch is still working");
    assert_eq!(
        again, None,
        "one camera leaving announced more than one removal per node it owned: {again:?}"
    );
}

#[test]
fn a_watch_opened_after_the_loss_is_told_nothing_about_it() {
    // **A watch reports what happened after it opened, and never what happened before**
    // (note **N301**). The real watch is *primed* from the node tree when it opens —
    // `v4l2::hotplug::Tracker::primed`'s own doc says "a watch on a machine that already has
    // ten cameras does not announce ten arrivals" — so a node that was already absent when a
    // subscriber arrived is never announced to it. A double whose event queue any later watch
    // could drain would tell that subscriber about a departure that had already happened,
    // which is a fake capability no real stack exhibits \[PF:17, note **N136**\].
    //
    // It is a sequence a real consumer meets rather than a curiosity: `daemon::events` runs
    // its watch thread only while somebody is listening, so "the camera left while nobody was
    // subscribed, then a subscriber arrived" is the ordinary shape of an agent reconnecting.
    // The arm above is the other direction — a watch opened *before* the loss is told — so
    // the two together say the priming is a cursor and not a mute.
    let backend = machine();
    let cameras = backend.enumerate().expect("enumerates");
    let lost = cameras.first().expect("a camera").clone();

    let mut camera = backend.open_fake(&lost.id).expect("opens");
    camera
        .start_stream(&StreamRequest::default())
        .expect("starts");
    backend.queue_fault(Fault::DeviceGoneMidStream);
    let refused = camera
        .next_frame(std::time::Instant::now() + std::time::Duration::from_secs(1))
        .expect_err("the device vanished");
    assert_eq!(refused.kind(), ErrorKind::DeviceGone, "{refused}");
    drop(camera);

    // Subscribed afterwards, and told nothing for the whole of a poll turn — the deadline the
    // daemon's own watch thread hands in, which is the right bound for sitting through a
    // quiet stretch even though it is the wrong one for a delivery claim.
    let mut watch = backend.watch().expect("this backend gives out a watch");
    let quiet = watch
        .next_event(
            std::time::Instant::now()
                + std::time::Duration::from_millis(schema::limits::HOTPLUG_WATCH_DEADLINE_MS),
        )
        .expect("the watch is working");
    assert_eq!(
        quiet, None,
        "a watch opened after the camera left was told about a departure it could not have \
         seen: {quiet:?}"
    );

    // And the listing is where that subscriber learns what it missed, which is the thing a
    // real consumer does and the reason the silence is correct rather than a loss.
    let after = backend.enumerate().expect("enumerates");
    assert!(
        after.iter().all(|other| other.id != lost.id),
        "the camera that left is still in the listing"
    );
}

#[test]
fn a_camera_that_stayed_puts_nothing_on_the_watch() {
    // The other half of the inverse, and the one that stops the arm above from passing on a
    // fake that announces a removal whenever anybody opens a watch: a machine where nothing
    // left says nothing, for the whole of the same bound.
    let backend = machine();
    let mut watch = backend.watch().expect("this backend gives out a watch");
    let selector = first(&backend);
    let CameraSelector::Id(present) = selector else {
        panic!("the fixture built something other than an id");
    };
    let mut camera = backend.open_fake(&present).expect("opens");
    camera
        .start_stream(&StreamRequest::default())
        .expect("starts");
    camera
        .next_frame(std::time::Instant::now() + std::time::Duration::from_secs(1))
        .expect("a frame from a camera that is still here");

    let bound = std::time::Duration::from_millis(schema::limits::HOTPLUG_WATCH_DEADLINE_MS);
    let quiet = watch
        .next_event(std::time::Instant::now() + bound)
        .expect("the watch is working");
    assert_eq!(
        quiet, None,
        "a machine nothing left from announced something: {quiet:?}"
    );
}

/// What a capture stamps around it: this test, on this host, this minute.
///
/// The backend is named `Fake` because it is, which is the field that stops a document taken
/// here from ever being mistaken for corpus.
fn capture_context() -> engine::profile::CaptureContext {
    engine::profile::CaptureContext {
        captured_at: Stamp::epoch(),
        kernel: "hermetic".to_owned(),
        tool_version: env!("CARGO_PKG_VERSION").to_owned(),
        capturer: "device_loss.rs".to_owned(),
        backend: schema::backend::BackendKind::Fake,
    }
}

#[test]
fn a_later_return_is_a_new_arrival_whose_fingerprint_says_it_is_the_same_device() {
    // D19's last sentence, and the one the tree could not express at all until the fake grew
    // a way for a camera to come back (note **N300**): `gone` was set once and never cleared,
    // so "a later return" had no producer on either side of the rig.
    //
    // The claim is D14 and D15's split doing its job, so it is asserted through D15's own
    // projection rather than by reading fields: two profiles captured by the shipped capture
    // path, one before the loss and one after the return, compared with identity held to one
    // side. **Same device, different address** — the whole sentence, in the vocabulary the
    // design says carries it.
    let backend = machine();
    let cameras = backend.enumerate().expect("enumerates");
    let lost = cameras.first().expect("a camera").clone();

    let before = {
        let mut camera = backend.open_fake(&lost.id).expect("opens");
        engine::profile::capture(&mut camera, &capture_context())
            .expect("the camera describes itself")
    };
    let paths_before: Vec<camino::Utf8PathBuf> =
        lost.nodes.iter().map(|node| node.path.clone()).collect();

    let mut watch = backend.watch().expect("this backend gives out a watch");
    let mut camera = backend.open_fake(&lost.id).expect("opens");
    camera
        .start_stream(&StreamRequest::default())
        .expect("starts");
    backend.queue_fault(Fault::DeviceGoneMidStream);
    camera
        .next_frame(std::time::Instant::now() + std::time::Duration::from_secs(1))
        .expect_err("the device vanished");
    drop(camera);
    // The removals, read off so that what the return puts on the watch is the next event and
    // not a previous one — one per node the camera owned, which is what the arm above holds
    // the loss to.
    for _ in &lost.nodes {
        watch
            .next_event(std::time::Instant::now())
            .expect("the watch is working")
            .expect("the loss announced fewer removals than the camera had nodes");
    }

    // Somewhere else on the same machine. The address is stated rather than derived: the rig
    // picks the port it re-attaches on, and this test is standing in for the rig.
    let returned = backend
        .device_returns(
            &lost.id,
            &fake::Reattachment::At {
                bus_path: "9-9:1.0".to_owned(),
                bus_info: "usb-0000:00:14.0-9".to_owned(),
                first_node: 90,
            },
        )
        .expect("a camera that vanished can come back");

    // **A new arrival**, on the watch, naming every node it came back on — the mirror of the
    // departure, and one event per node for the same reason (note **N301**).
    let deadline = std::time::Instant::now()
        + std::time::Duration::from_millis(schema::limits::HOTPLUG_MAX_DEFERRAL_MS);
    let mut announced = Vec::new();
    for _ in &returned.nodes {
        announced.push(
            watch
                .next_event(deadline)
                .expect("the watch is working")
                .expect("a camera arrived and the watch was told about fewer of its nodes"),
        );
    }
    assert_eq!(
        announced,
        returned
            .nodes
            .iter()
            .map(|node| schema::backend::HotplugEvent::Added {
                path: node.path.clone()
            })
            .collect::<Vec<_>>(),
        "the arrival named the wrong nodes, or the wrong direction"
    );
    let again = watch
        .next_event(std::time::Instant::now())
        .expect("the watch is still working");
    assert_eq!(
        again, None,
        "one camera arriving announced more than one arrival per node: {again:?}"
    );

    // The listing has it again, and resolution finds it under the id a consumer was holding —
    // which is D1's rule rather than a shortcut: an id is derived from card names, and the
    // card name did not move.
    let after_listing = backend.enumerate().expect("enumerates");
    assert_eq!(
        after_listing.len(),
        cameras.len(),
        "the machine came back a different size: {after_listing:#?}"
    );
    let resolved = engine::resolve::camera(&after_listing, &CameraSelector::Id(lost.id.clone()))
        .expect("the id a consumer was holding names the camera that came back");
    assert_eq!(resolved.id, lost.id);

    // And D15's answer: the description did not move, the address did.
    let after = {
        let mut camera = backend
            .open_fake(&lost.id)
            .expect("it opens at its new address");
        engine::profile::capture(&mut camera, &capture_context())
            .expect("the camera describes itself")
    };
    let comparison = before.compare(&after);
    assert!(
        comparison.device_matches(),
        "a camera that came back described itself as a different device: {comparison}"
    );
    assert_eq!(
        comparison.identity,
        vec!["fingerprint.bus_path".to_owned(), "bus_info".to_owned()],
        "the identity half did not name the address, or named more than the address: {comparison}"
    );

    // Every node path moved and the comparison named none of them, which is PF:22 and not an
    // omission: node numbering is probe-order bookkeeping, and a comparison that called it a
    // difference would report a moved camera every time a driver reloaded.
    let paths_after: Vec<camino::Utf8PathBuf> = returned
        .nodes
        .iter()
        .map(|node| node.path.clone())
        .collect();
    assert_eq!(paths_after.len(), paths_before.len());
    assert!(
        paths_before
            .iter()
            .zip(paths_after.iter())
            .all(|(was, now)| was != now),
        "the fixture did not move the node paths, so PF:22's exclusion is untested here: \
         {paths_before:?} against {paths_after:?}"
    );
}

#[test]
fn a_camera_that_came_back_where_it_was_moved_no_identity_at_all() {
    // The inverse of the arm above, and what stops it passing on a build that reports an
    // identity delta for any two captures: the same loss, the same return, into the socket the
    // camera came out of — and D15 finds nothing to say. It is also the case an operator
    // actually produces, and the one that leaves a session store's fingerprint-keyed directory
    // where it was.
    let backend = machine();
    let lost = backend
        .enumerate()
        .expect("enumerates")
        .first()
        .expect("a camera")
        .clone();
    let before = {
        let mut camera = backend.open_fake(&lost.id).expect("opens");
        engine::profile::capture(&mut camera, &capture_context())
            .expect("the camera describes itself")
    };

    let mut camera = backend.open_fake(&lost.id).expect("opens");
    camera
        .start_stream(&StreamRequest::default())
        .expect("starts");
    backend.queue_fault(Fault::DeviceGoneMidStream);
    camera
        .next_frame(std::time::Instant::now() + std::time::Duration::from_secs(1))
        .expect_err("the device vanished");
    drop(camera);
    backend
        .device_returns(&lost.id, &fake::Reattachment::WhereItWas)
        .expect("a camera that vanished can come back");

    let after = {
        let mut camera = backend.open_fake(&lost.id).expect("opens");
        engine::profile::capture(&mut camera, &capture_context())
            .expect("the camera describes itself")
    };
    let comparison = before.compare(&after);
    assert!(comparison.device_matches(), "{comparison}");
    assert!(
        comparison.identity.is_empty(),
        "a camera that came back where it was reported an address change: {comparison}"
    );
}
