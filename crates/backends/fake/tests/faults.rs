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
            Fault::ControlReadDeclined => control_read_declined(),
            Fault::SettleNeverConverges => settle_never_converges(),
            Fault::FrameTimeout => frame_timeout(),
            Fault::FrameGap => frame_gap(),
            Fault::HotplugAdd => hotplug_add(),
            Fault::HotplugRemove => hotplug_remove(),
            Fault::WatchUnavailable => watch_unavailable(),
            Fault::WatchFails => watch_fails(),
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

    // ControlReadDeclined: an unscripted walk answers for every control it enumerates,
    // so `value_was_declined` is false everywhere — which is what makes the observation
    // below a fault rather than the fake's ordinary output.
    assert!(
        camera
            .controls()
            .expect("an unscripted walk")
            .iter()
            .all(|desc| !desc.value_was_declined()),
        "the fake declined a control nobody scripted"
    );

    // DeviceGoneMidStream, FrameTimeout, SettleNeverConverges.
    let frames = stream_frames(&mut camera, SETTLE_PLUS_TWO);
    assert_eq!(
        frames.last().map(|f| &f.bytes),
        frames.get(frames.len() - 2).map(|f| &f.bytes),
        "an unscripted stream settles"
    );

    // HotplugAdd, HotplugRemove, WatchUnavailable, WatchFails.
    let mut watch = backend
        .watch()
        .expect("an unscripted host gives a watch out");
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

#[test]
fn a_call_that_reported_one_fault_leaves_the_others_still_scripted() {
    // **Rubric A9's fault-menu half** (the G6 review's L35, note **N232**). `next_frame`
    // used to take all three of its faults off the queue at the top, before it knew which
    // one it would report: a test that scripted a `FrameTimeout` and a
    // `SettleNeverConverges` got the timeout, and the settle fault was *consumed by the
    // call that reported the timeout* and never fired. A scripted claim that silently never
    // happens is worse than one that fails, because the suite that made it reports green.
    //
    // Three scripted, one call, one reported — and the other two still on the queue.
    let backend = backend();
    let id = first_id(&backend);
    let mut camera = backend.open_fake(&id).expect("open");
    start(&mut camera);

    backend.queue_faults(&[
        Fault::FrameTimeout,
        Fault::SettleNeverConverges,
        Fault::DeviceGoneMidStream,
    ]);
    let error = camera
        .next_frame(soon())
        .expect_err("the timeout was scripted");
    assert!(matches!(error, Error::SettleTimeout { .. }), "{error}");
    assert_eq!(
        backend.pending_faults(),
        vec![Fault::SettleNeverConverges, Fault::DeviceGoneMidStream],
        "one call reported one fault and ate the two it did not report"
    );

    // …and the next call reports the next one, which is what makes the queue a script
    // rather than a bag.
    let error = camera
        .next_frame(soon())
        .expect_err("the device was scripted to go away");
    assert!(matches!(error, Error::DeviceGone { .. }), "{error}");
    assert_eq!(
        backend.pending_faults(),
        vec![Fault::SettleNeverConverges],
        "the settle fault is still the one nobody has spent"
    );

    // The device is gone with the stream, so the fake refuses rather than rendering — and
    // that refusal must not spend the settle fault either. This is the arm the review's
    // reading did not reach: three faults were taken before the `state.stream` check, so a
    // call that could never have produced a frame emptied the queue.
    let error = camera
        .next_frame(soon())
        .expect_err("the device is not there any more");
    assert!(matches!(error, Error::DeviceGone { .. }), "{error}");
    assert_eq!(
        backend.pending_faults(),
        vec![Fault::SettleNeverConverges],
        "a call that produced no frame at all still spent a frame fault"
    );

    // And the fault that survived all of it does what it was scripted to do, once, on the
    // first call that actually renders a frame — which needs the camera back, because a
    // device that left refuses every door into it (design D19). The return is the machine's
    // verb and not a fault, so it spends nothing off this queue, which is the second claim
    // this last stretch makes.
    backend
        .device_returns(&id, &somewhere_else())
        .expect("a camera that vanished can come back");
    let mut camera = backend.open_fake(&id).expect("it opens at its new address");
    start(&mut camera);
    camera.next_frame(soon()).expect("a frame");
    assert!(
        backend.pending_faults().is_empty(),
        "the settle fault never fired at all"
    );
}

#[test]
fn a_vanished_camera_refuses_every_door_into_it_and_takes_them_all_back() {
    // **D19's "and refuses every further operation"**, which the fake claimed in a doc
    // comment and implemented for `enumerate` alone: a camera that answered `DeviceGone` to a
    // frame and then enumerated its controls, negotiated a stream and accepted a write is a
    // shape no machine has (E5; note **N300**). A real node whose device left answers
    // `ENODEV` to whatever ioctl arrives next, and which ioctl it was does not change the
    // answer — so the claim is over the doors and not over one of them.
    //
    // Both directions in one arm, because the inverse is what makes it mean anything: every
    // door is walked once while the camera is here and answers, once while it is gone and
    // refuses `DeviceGone`, and once after it comes back and answers again. A build that
    // refused everything for ever would pass the middle third and fail the last.
    let backend = backend();
    let id = first_id(&backend);
    let brightness = {
        let camera = backend.open_fake(&id).expect("open");
        descriptor(&camera, "brightness")
    };

    // Here: every door answers. Walked through the same vocabulary as the refusal, so the
    // two thirds are the same population rather than two lists a reader compares by eye.
    let mut camera = backend.open_fake(&id).expect("open");
    for &door in Door::ALL {
        assert!(
            door.knock(&mut camera, &brightness).is_none(),
            "{door} refused a camera that is on the machine"
        );
    }

    // Gone: every door refuses with the device's own answer, and the handle a caller was
    // already holding refuses too — the descriptor outliving the device is exactly the
    // situation `ENODEV` exists for.
    backend.queue_fault(Fault::DeviceGoneMidStream);
    let refused = camera.next_frame(soon()).expect_err("the device vanished");
    assert!(matches!(refused, Error::DeviceGone { .. }), "{refused}");
    for &door in Door::ALL {
        let refused = door
            .knock(&mut camera, &brightness)
            .unwrap_or_else(|| panic!("{door} answered for a camera that is not on the machine"));
        assert!(
            matches!(refused, Error::DeviceGone { .. }),
            "{door} answered {refused} for a camera that vanished"
        );
    }

    // **The backend's own door answers the other kind, and that is not an inconsistency**
    // (note **N301**). `still_here` is what an already-open handle says, which is the
    // `ENODEV` an fd gets when its device leaves. Asking the backend for the camera again is
    // a listing miss, and D19's fourth sentence is that the listing stops naming a camera
    // that left — so the answer is the one `V4l2Backend::open` gives for an id its
    // `enumerate` does not name. The claim that both backends give it is
    // `battery::arm_enumeration`'s, because that is where both of them inherit it.
    let refused = backend
        .open_fake(&id)
        .expect_err("a camera that is not there cannot be opened");
    assert!(matches!(refused, Error::CameraUnknown { .. }), "{refused}");

    // Back: the doors open again, which is what stops the middle third from being a build
    // that simply refuses.
    backend
        .device_returns(&id, &somewhere_else())
        .expect("a camera that vanished can come back");
    let mut camera = backend.open_fake(&id).expect("open");
    for &door in Door::ALL {
        assert!(
            door.knock(&mut camera, &brightness).is_none(),
            "{door} refused a camera that came back"
        );
    }
}

#[test]
fn only_a_camera_that_left_can_come_back() {
    // The refusal on the machine's own verb, and the reason it is a refusal rather than a
    // second arrival: an `Added` event for a camera that never left is an event no machine
    // produced, and a watcher that got one would be told a lie by the double this design
    // uses to state D19's contract.
    let backend = backend();
    let id = first_id(&backend);
    let refused = backend
        .device_returns(&id, &somewhere_else())
        .expect_err("this camera never left");
    assert!(
        matches!(refused, Error::IllegalTransition { .. }),
        "{refused}"
    );

    let unknown = backend
        .device_returns(
            &schema::camera::CameraId::parse("cam:nothing-here").expect("a literal id"),
            &somewhere_else(),
        )
        .expect_err("this backend never replayed that camera");
    assert!(matches!(unknown, Error::CameraUnknown { .. }), "{unknown}");
}

// ------------------------------------------------------------------ the observations

fn device_gone_mid_stream() {
    let backend = backend();
    // Opened before the loss, because after it there is nothing left to open — which is the
    // observation this fault is named for.
    let mut watch = backend.watch().expect("a watch");
    let mut camera = backend.open_fake(&first_id(&backend)).expect("open");
    let nodes: Vec<camino::Utf8PathBuf> = camera
        .info()
        .nodes
        .iter()
        .map(|node| node.path.clone())
        .collect();
    start(&mut camera);
    camera.next_frame(soon()).expect("a frame before the fault");

    backend.queue_fault(Fault::DeviceGoneMidStream);
    let error = camera.next_frame(soon()).expect_err("the device vanished");
    assert!(matches!(&error, Error::DeviceGone { .. }), "{error}");

    // The device is gone with the stream: a device that unplugged is not a device that is
    // between frames, and the difference is what an unattended agent branches on.
    let after = camera
        .next_frame(soon())
        .expect_err("the device is not there");
    assert!(matches!(&after, Error::DeviceGone { .. }), "{after}");

    // And the machine said so, naming **every** node that left (design D19; note **N301**).
    // Read off the watch this test opened *before* the loss, so the event is one the machine
    // produced rather than one anybody scripted: `pending_faults` is empty here and the walk's
    // other half, `no_fault_fires_unless_it_was_scripted`, is what keeps that true.
    //
    // One per node because that is the machine's shape: the kernel emits a uevent per
    // interface and `v4l2::hotplug::Tracker::rescan` queues one `Removed` per path that left
    // the tree, so a double that announced once for a two-node camera would leave a node-level
    // consumer believing the metadata node was still there.
    let mut announced = Vec::new();
    for _ in &nodes {
        announced.push(
            watch
                .next_event(soon())
                .expect("the watch is working")
                .expect("a camera left and the watch was told about fewer nodes than left"),
        );
    }
    assert_eq!(
        announced,
        nodes
            .iter()
            .map(|path| HotplugEvent::Removed { path: path.clone() })
            .collect::<Vec<_>>(),
        "a camera with {} node(s) announced {announced:?}",
        nodes.len()
    );
    assert!(
        nodes.len() > 1,
        "the profile this walk replays owns one node, so 'one removal per node' is untested \
         here: {nodes:?}"
    );
    // And nothing further about a camera that has already left. The deadline is already spent,
    // so this answers immediately and the arm costs nothing.
    let again = watch
        .next_event(Instant::now())
        .expect("the watch is still working");
    assert_eq!(
        again, None,
        "one camera leaving announced more than one removal per node: {again:?}"
    );
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

fn control_read_declined() {
    // AGENTS rule 7 through a whole backend, which is the arm the V4L2 half cannot have:
    // there is no ioctl seam under `crates/backends/v4l2/`, so `walked_current`'s claim —
    // one control the driver declined is carried valueless and the enumeration runs to
    // the end — was a direct equality on a free function and nothing else (note **N195**).
    let backend = backend();
    let camera = backend.open_fake(&first_id(&backend)).expect("open");
    let before = camera.controls().expect("a walk");
    assert!(before.len() > 1, "a one-control fixture proves nothing");

    backend.queue_fault(Fault::ControlReadDeclined);
    let after = camera.controls().expect("the walk still answers");

    // The whole camera is still described. A backend that ended the walk on the refusal
    // would answer "what can this camera do" with "something went wrong reading one
    // knob", which is E3's conversion at the level D2 exists to protect.
    assert_eq!(
        after.iter().map(|d| &d.slug).collect::<Vec<_>>(),
        before.iter().map(|d| &d.slug).collect::<Vec<_>>(),
        "the walk dropped or reordered controls"
    );

    // And exactly one control is carried valueless, in the population a reader can name.
    let declined: Vec<&ControlSlug> = after
        .iter()
        .filter(|desc| desc.value_was_declined())
        .map(|desc| &desc.slug)
        .collect();
    assert_eq!(declined.len(), 1, "{declined:?}");
    let name = declined[0].clone();
    assert!(
        before
            .iter()
            .any(|desc| desc.slug == name && desc.current.is_some()),
        "{name} had no value before the fault either, so nothing was declined"
    );
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

fn frame_gap() {
    // Design D16's driven inverse: `Frame::sequence`'s "gaps mean dropped frames" is a
    // contract consumers are now invited to aggregate over, and this is the fault that lets
    // a consumer's gap accounting be tested against something rather than reasoned about.
    let backend = backend();
    let mut camera = backend.open_fake(&first_id(&backend)).expect("open");
    start(&mut camera);
    let before = camera.next_frame(soon()).expect("a frame before the fault");

    backend.queue_fault(Fault::FrameGap);
    let after = camera.next_frame(soon()).expect("a frame after the gap");

    // The sequence skips the frames that never arrived...
    assert_eq!(
        after.sequence - before.sequence,
        fake::FRAME_GAP_FRAMES + 1,
        "a gap of {} lost frames advances the sequence by that many plus this frame",
        fake::FRAME_GAP_FRAMES
    );
    // ...and the clock moves on by *that many intervals*, which is what tells a lost run from
    // a stall: a stall stretches one interval and skips no sequence number, and a lost run
    // does both together. Asserted against the interval this stream really has — measured
    // from two frames the fault did not touch — rather than as `> 0`, which is true of a
    // stall as well and which an adversarial reader measured as unfalsifiable: deleting the
    // fake's clock advance left the whole workspace green.
    let gap_span = after.timestamp_us - before.timestamp_us;
    let next = camera.next_frame(soon()).expect("a frame after the gap");
    let one_interval = next.timestamp_us - after.timestamp_us;
    assert!(
        one_interval > 0,
        "this stream has no interval to compare a gap against"
    );
    assert_eq!(
        gap_span,
        one_interval * i64::from(fake::FRAME_GAP_FRAMES + 1),
        "the clock advanced {gap_span} µs across a gap of {} lost frame(s) and one interval \
         is {one_interval} µs — a lost run advances the clock by the frames it lost, and a \
         stall would advance it by one",
        fake::FRAME_GAP_FRAMES
    );

    // One shot: the frame after the gap follows the one before it.
    assert_eq!(next.sequence - after.sequence, 1);
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

fn watch_unavailable() {
    let backend = backend();
    backend.queue_fault(Fault::WatchUnavailable);

    let error = backend.watch().expect_err("this host has no watch to give");
    // `DeviceIo` and not `DeviceGone`: E3 keeps "the machine will not give me a watch"
    // apart from "the camera is gone", and this backend's cameras are all still here —
    // which is asserted rather than argued, one line down.
    assert!(matches!(&error, Error::DeviceIo { .. }), "{error}");
    assert!(!backend.enumerate().expect("enumeration").is_empty());

    // One shot: the next caller gets a watch, which is what makes a daemon's "the next
    // subscriber starts a fresh watch" reachable.
    backend.watch().expect("a watch after the refusal");
}

fn watch_fails() {
    let backend = backend();
    let mut watch = backend.watch().expect("watch");
    // A watch that was handed out fine and then stops, which is the direction with a
    // consumer behind it: the daemon's watch thread ends its subscribers' streams over it
    // (note **N59**).
    backend.queue_fault(Fault::WatchFails);

    let error = watch
        .next_event(Instant::now())
        .expect_err("the watch stopped working");
    assert!(matches!(&error, Error::DeviceIo { .. }), "{error}");
    // Checked before the events, so a failure is not overtaken by a queued arrival.
    backend.queue_faults(&[Fault::WatchFails, Fault::HotplugAdd]);
    assert!(
        watch.next_event(Instant::now()).is_err(),
        "a queued arrival was answered by a watch that had failed"
    );
}

#[test]
fn the_watch_waits_for_a_scripted_event_and_gives_the_deadline_back_when_none_comes() {
    // The behaviour P4e-i corrected, both directions (note **N57**). `HotplugWatch` says
    // `next_event` blocks until an event or until the deadline; this fake used to answer
    // `Ok(None)` instantly whatever it was given, which made a caller that loops — the
    // daemon's watch thread — a spin at 100% of a core, and made
    // `testkit::battery`'s "the deadline is honored" arm vacuous.
    //
    // **Nothing here sleeps.** The waiting arm ends when *this test* scripts a fault, which
    // is a signal from the subject; the deadline arm is the trait's own bound, driven with
    // a deadline that has already passed so that "it returned rather than blocked" needs no
    // clock at all and no duration to elapse.
    let backend = backend();
    let mut watch = backend.watch().expect("watch");

    // A generous budget nothing reaches: the wait ends on the notification, so this bound
    // is one-sided and a build that ignored it would fail by *timing out*, not by racing.
    let generous = Instant::now() + Duration::from_secs(60);
    let waiting = std::thread::spawn(move || (watch.next_event(generous), watch));
    backend.queue_fault(Fault::HotplugAdd);
    let (event, mut watch) = waiting.join().expect("the watching thread");
    assert!(
        matches!(event.expect("poll"), Some(HotplugEvent::Added { .. })),
        "a scripted event did not end the wait"
    );

    // And the other direction, with no fault to find: an already-spent deadline is a zero
    // wait rather than a block or a panic, which is the same shape
    // `v4l2::watch`'s own suite pins for the real one.
    let began = Instant::now();
    assert_eq!(watch.next_event(Instant::now()).expect("poll"), None);
    assert!(
        began.elapsed() < Duration::from_secs(30),
        "a spent deadline was waited out: {:?}",
        began.elapsed()
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
/// Every door into an open camera that can refuse (design T2's `Camera`).
///
/// A closed vocabulary with an exhaustive `match`, the shape [`Fault`]'s own walk uses and
/// for the same reason: a list of calls written inline is a claim about "every door" that a
/// reader has to re-check, and the walk above is exactly that claim (note **N301**). Adding a
/// member here without giving it a knock stops this build.
///
/// It covers the seven fallible methods and not the two infallible ones — `info` hands back a
/// copy taken at open time and `streaming` answers `Option`, so neither has a refusal to make.
/// The day `Camera` grows a method the compiler asks the question in ten places at once, every
/// `impl Camera for` in this workspace included, and this vocabulary is where the answer goes.
#[derive(Debug, Clone, Copy)]
enum Door {
    Formats,
    Controls,
    Get,
    Set,
    StartStream,
    NextFrame,
    StopStream,
}

impl Door {
    const ALL: &'static [Door] = &[
        Door::Formats,
        Door::Controls,
        Door::Get,
        Door::Set,
        Door::StartStream,
        Door::NextFrame,
        Door::StopStream,
    ];

    /// Knock, and hand back the refusal if there was one.
    fn knock(self, camera: &mut FakeCamera, brightness: &ControlDesc) -> Option<Error> {
        match self {
            Door::Formats => camera.formats().err(),
            Door::Controls => camera.controls().err(),
            Door::Get => camera.get(brightness.id).err(),
            Door::Set => camera
                .set(brightness.id, ControlValue::Int(brightness.range.min))
                .err(),
            Door::StartStream => camera.start_stream(&StreamRequest::default()).err(),
            Door::NextFrame => camera.next_frame(soon()).err(),
            Door::StopStream => camera.stop_stream().err(),
        }
    }

    /// The trait method this door is, spelled as the trait spells it.
    fn name(self) -> &'static str {
        match self {
            Door::Formats => "formats",
            Door::Controls => "controls",
            Door::Get => "get",
            Door::Set => "set",
            Door::StartStream => "start_stream",
            Door::NextFrame => "next_frame",
            Door::StopStream => "stop_stream",
        }
    }
}

impl std::fmt::Display for Door {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

fn soon() -> Instant {
    Instant::now() + Duration::from_secs(1)
}

/// A different port on the same machine, for a camera coming back (design D19).
///
/// Written down rather than derived, because the address is the *rig's* to choose: the
/// partner rig picks the vhci port it re-attaches on, and a stated input is what an
/// assertion about the answer is allowed to rest on (N252).
fn somewhere_else() -> fake::Reattachment {
    fake::Reattachment::At {
        bus_path: "3-7:1.0".to_owned(),
        bus_info: "usb-0000:00:14.0-7".to_owned(),
        first_node: 40,
    }
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
