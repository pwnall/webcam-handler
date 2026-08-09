//! R2 — the `vivid` virtual-driver rung (design §3.1).
//!
//! `vivid` is a kernel module presenting V4L2 capture devices with no hardware behind
//! them: the only way to exercise the real ioctl layer on a machine with no camera. It is
//! opportunistic by nature — most runners cannot load it — which makes it the rung most
//! likely to decay into green-by-absence, so `scripts/rung-vivid.sh` reports a named,
//! counted skip rather than exiting quietly.
//!
//! **What a green R2 does and does not mean.** It proves the *ioctl plumbing*: that the
//! `QUERY_EXT_CTRL` walk terminates, that menus and formats enumerate, that the decoders
//! read what a real kernel wrote. It proves nothing about device quirks — `vivid` does not
//! model INACTIVE-coupled menus with holes, or drivers that clamp silently, or values
//! outside their declared range (design §3.3 item 4). Those live on R3 and in the corpus.
//! Each test below says which of the two it is making a claim about.
//!
//! These tests deliberately assert *shapes* rather than `vivid`'s specific control set:
//! the module's controls vary across kernel versions, and pinning them would make this
//! rung fail on the next kernel for no reason worth a red run.
//!
//! wch-suite: prefix=vivid_ recipe=rung-vivid

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use schema::backend::CameraBackend;
use schema::camera::{CameraInfo, NodeKind};
use schema::capture::StreamRequest;
use schema::control::{ControlSlug, ControlType};
use testkit::battery;
use v4l2::V4l2Backend;

/// The driver name `vivid` reports through `QUERYCAP`.
const VIVID_DRIVER: &str = "vivid";

/// Every camera on this host that the `vivid` module is behind.
///
/// Selected by driver name rather than by node number: a machine running this rung may
/// also have a real camera attached (this project's development machine does), and R2
/// must make its claims about the virtual driver only.
fn vivid_cameras() -> Option<(V4l2Backend, Vec<CameraInfo>)> {
    let backend = V4l2Backend::new();
    let all = match backend.enumerate() {
        Ok(all) => all,
        Err(error) => {
            println!("SKIP: enumeration failed on this host: {error}");
            return None;
        }
    };
    let vivid: Vec<CameraInfo> = all
        .into_iter()
        .filter(|info| info.driver == VIVID_DRIVER)
        .collect();
    if vivid.is_empty() {
        println!(
            "SKIP: no camera on this host reports driver {VIVID_DRIVER:?}; load the module \
             with `sudo modprobe vivid`"
        );
        return None;
    }
    println!("{} vivid camera(s) found", vivid.len());
    Some((backend, vivid))
}

#[test]
#[ignore = "R2: needs the vivid kernel module loaded; run with `just rung-vivid`"]
fn vivid_enumeration_groups_nodes_and_classifies_them_by_capability() {
    // Plumbing, not quirks: that `QUERYCAP` reaches the kernel, that `device_caps` comes
    // back, and that the grouping rule survives a driver whose topology is not USB.
    let Some((_, cameras)) = vivid_cameras() else {
        return;
    };

    let mut bus_paths = BTreeSet::new();
    for info in &cameras {
        assert!(
            !info.card.is_empty(),
            "{}: vivid reported no card name",
            info.id
        );
        assert_eq!(info.driver, VIVID_DRIVER);
        assert!(!info.nodes.is_empty(), "{}: no nodes", info.id);
        assert!(
            bus_paths.insert(info.fingerprint.bus_path.clone()),
            "{}: two vivid cameras share bus path {}",
            info.id,
            info.fingerprint.bus_path
        );

        for node in &info.nodes {
            assert_eq!(
                node.kind,
                NodeKind::from_device_caps(node.device_caps),
                "{}: node {} is classified against its own device_caps",
                info.id,
                node.path
            );
        }

        // `vivid` is not a USB device, so the grouping falls back to whatever sysfs says
        // — the point being that a non-USB driver still enumerates rather than vanishing.
        assert!(
            info.fingerprint.usb_id.is_none(),
            "{}: vivid is not on USB, yet a USB id was derived: {:?}",
            info.id,
            info.fingerprint.usb_id
        );
        println!(
            "{}: {} node(s), bus_path {}",
            info.id,
            info.nodes.len(),
            info.fingerprint.bus_path
        );
    }
}

#[test]
#[ignore = "R2: needs the vivid kernel module loaded; run with `just rung-vivid`"]
fn vivid_controls_enumerate_and_hold_the_control_model_invariants() {
    // The `QUERY_EXT_CTRL` walk against a real kernel. `vivid` exposes a large and varied
    // control set — including compound types on recent kernels — so this is the closest
    // thing to a hardware test that a camera-less runner can run.
    let Some((backend, cameras)) = vivid_cameras() else {
        return;
    };

    let mut total = 0usize;
    for info in &cameras {
        let camera = match backend.open(&info.id) {
            Ok(camera) => camera,
            // Availability is not capability (E3): another process holding a vivid node
            // says nothing about the ioctl layer.
            Err(error @ (schema::Error::Busy { .. } | schema::Error::PermissionDenied { .. })) => {
                println!("SKIP: {} could not be opened: {error}", info.id);
                continue;
            }
            Err(error) => panic!("{}: open() failed: {error}", info.id),
        };

        let controls = camera
            .controls()
            .unwrap_or_else(|error| panic!("{}: controls() failed: {error}", info.id));
        total += controls.len();

        let mut ids = BTreeSet::new();
        for desc in &controls {
            assert!(
                ids.insert(desc.id),
                "{}: control id {} appears twice — the NEXT_CTRL walk repeated itself",
                info.id,
                desc.id
            );
            // The D2 transform, against names a real kernel produced.
            assert_eq!(
                ControlSlug::from_name(&desc.name).as_ref(),
                Some(&desc.slug),
                "{}: {:?} does not slug to {}",
                info.id,
                desc.name,
                desc.slug
            );
            // Every descriptor survives the wire, unknown types and sparse menus alike.
            let json = serde_json::to_string(desc).expect("a control serializes");
            let back: schema::control::ControlDesc =
                serde_json::from_str(&json).expect("a control deserializes");
            assert_eq!(
                &back, desc,
                "{}: {} changed on a round trip",
                info.id, desc.slug
            );

            // A menu control's items must be the ones the kernel answered for, and the
            // indices must lie inside the declared range.
            if desc.control_type.is_menu() {
                for index in desc.menu.keys() {
                    let index = i64::from(*index);
                    assert!(
                        desc.range.contains(index),
                        "{}: {} has menu index {index} outside its declared range \
                         [{}..={}]",
                        info.id,
                        desc.slug,
                        desc.range.min,
                        desc.range.max
                    );
                }
            }

            // A compound control must carry a payload size, or the read path has no
            // bound to check against [rubric B10].
            if matches!(
                desc.control_type,
                ControlType::Rect
                    | ControlType::Area
                    | ControlType::U8
                    | ControlType::U16
                    | ControlType::U32
            ) {
                assert!(
                    desc.elem_size > 0 && desc.elems > 0,
                    "{}: compound control {} declares elem_size {} × elems {}",
                    info.id,
                    desc.slug,
                    desc.elem_size,
                    desc.elems
                );
            }
        }
        println!("{}: {} control(s) enumerated", info.id, controls.len());
    }

    // Non-vacuity: `vivid` has controls, and a run that walked none did not exercise the
    // walk.
    assert!(
        total > 0,
        "no vivid camera exposed a single control; the QUERY_EXT_CTRL walk was not \
         exercised, so this rung proved nothing"
    );
}

#[test]
#[ignore = "R2: needs the vivid kernel module loaded; run with `just rung-vivid`"]
fn vivid_formats_enumerate_with_sizes_and_intervals_nested_under_them() {
    // The PF:9 nesting against a real kernel: `ENUM_FRAMESIZES` is asked per pixel
    // format and `ENUM_FRAMEINTERVALS` per size, and the enumeration terminates on
    // EINVAL rather than running to its cap.
    let Some((backend, cameras)) = vivid_cameras() else {
        return;
    };

    let mut formats_seen = 0usize;
    for info in &cameras {
        if info.capture_node().is_none() {
            println!("{}: no capture node, so no formats to enumerate", info.id);
            continue;
        }
        let camera = match backend.open(&info.id) {
            Ok(camera) => camera,
            Err(error @ (schema::Error::Busy { .. } | schema::Error::PermissionDenied { .. })) => {
                println!("SKIP: {} could not be opened: {error}", info.id);
                continue;
            }
            Err(error) => panic!("{}: open() failed: {error}", info.id),
        };

        let formats = camera
            .formats()
            .unwrap_or_else(|error| panic!("{}: formats() failed: {error}", info.id));
        formats_seen += formats.len();

        let mut seen = BTreeSet::new();
        for format in &formats {
            assert!(
                seen.insert(format.pixel_format),
                "{}: format {} enumerated twice",
                info.id,
                format.pixel_format
            );
            for entry in &format.sizes {
                // A shape this build cannot read is carried rather than dropped, and has
                // no dimensions to check — `vivid` reports none today, and if a future
                // kernel does, that is a finding rather than a failure.
                let Some((width, height)) = entry.size.max_dimensions() else {
                    println!(
                        "{}: {} lists a size shape this build cannot read: {:?}",
                        info.id, format.pixel_format, entry.size
                    );
                    continue;
                };
                assert!(
                    width > 0 && height > 0,
                    "{}: {} offers a {width}x{height} size",
                    info.id,
                    format.pixel_format
                );
            }
        }
        println!(
            "{}: {} format(s), {} size entr(ies)",
            info.id,
            formats.len(),
            formats.iter().map(|f| f.sizes.len()).sum::<usize>()
        );
    }

    assert!(
        formats_seen > 0,
        "no vivid capture node enumerated a single format; ENUM_FMT was not exercised"
    );
}

#[test]
#[ignore = "R2: needs the vivid kernel module loaded; run with `just rung-vivid`"]
fn vivid_reads_every_readable_controls_current_value() {
    // `G_EXT_CTRLS` against a real kernel, including the compound path: a payload
    // control's bytes come back through the caller-sized buffer, and `elem_size × elems`
    // is what sized it.
    let Some((backend, cameras)) = vivid_cameras() else {
        return;
    };

    let mut read = 0usize;
    let mut payloads = 0usize;
    for info in &cameras {
        let camera = match backend.open(&info.id) {
            Ok(camera) => camera,
            Err(error @ (schema::Error::Busy { .. } | schema::Error::PermissionDenied { .. })) => {
                println!("SKIP: {} could not be opened: {error}", info.id);
                continue;
            }
            Err(error) => panic!("{}: open() failed: {error}", info.id),
        };
        let controls = camera
            .controls()
            .unwrap_or_else(|error| panic!("{}: controls() failed: {error}", info.id));

        for desc in &controls {
            let Some(current) = &desc.current else {
                continue;
            };
            read += 1;
            match current {
                schema::control::ControlValue::Bytes(bytes) => {
                    payloads += 1;
                    let expected = usize::try_from(desc.elem_size)
                        .ok()
                        .and_then(|size| size.checked_mul(usize::try_from(desc.elems).ok()?));
                    assert_eq!(
                        Some(bytes.len()),
                        expected,
                        "{}: {} returned {} payload bytes for elem_size {} × elems {}",
                        info.id,
                        desc.slug,
                        bytes.len(),
                        desc.elem_size,
                        desc.elems
                    );
                }
                schema::control::ControlValue::Int(_) | schema::control::ControlValue::Text(_) => {}
            }
        }
    }

    assert!(
        read > 0,
        "no vivid control reported a current value; G_EXT_CTRLS was not exercised"
    );
    if payloads == 0 {
        println!(
            "SKIP (partial): no vivid control on this kernel carries a compound payload, \
             so the payload arm of G_EXT_CTRLS was not exercised"
        );
    } else {
        println!("{payloads} compound payload(s) read");
    }
}

// ------------------------------------------------- P2: writes and streaming, on ioctls

#[test]
#[ignore = "R2: needs the vivid kernel module loaded; run with `just rung-vivid`"]
fn vivid_a_write_goes_out_and_the_read_back_comes_from_the_driver() {
    // The `S_EXT_CTRLS` half of the ioctl plumbing, which R3 also exercises but on
    // hardware that offers this rung's whole point: **77 controls rather than 18**,
    // including types and shapes the seed cameras do not have (entry E2). What is claimed
    // is the plumbing — the write reaches the driver and the value comes back from it —
    // not any device quirk; §3.3 item 4 draws that line and the clamp behaviour stays on
    // R3 and in the corpus.
    let Some((backend, cameras)) = vivid_cameras() else {
        return;
    };

    let mut written = 0usize;
    for info in &cameras {
        let mut camera = backend
            .open(&info.id)
            .unwrap_or_else(|error| panic!("{}: could not be opened: {error}", info.id));
        let controls = camera
            .controls()
            .unwrap_or_else(|error| panic!("{}: controls() failed: {error}", info.id));

        for desc in controls
            .iter()
            .filter(|d| battery::is_perturbable(d) && !battery::is_motorized(&d.slug))
            .take(WRITES_PER_VIVID_NODE)
        {
            let Some(value) = battery::perturbation(desc) else {
                continue;
            };
            let original = desc
                .current
                .clone()
                .expect("a perturbable control has a current value");
            let applied = camera
                .set(desc.id, value.clone())
                .unwrap_or_else(|error| panic!("{}: writing {value} failed: {error}", desc.slug));
            assert_eq!(applied.requested, value, "{}: E4's pair", desc.slug);

            let back = camera
                .set(desc.id, original.clone())
                .unwrap_or_else(|error| {
                    panic!("{}: restoring {original} failed: {error}", desc.slug)
                });
            assert_eq!(
                back.applied, original,
                "{}: this test left the control somewhere else",
                desc.slug
            );
            written += 1;
        }
    }

    assert!(
        written > 0,
        "vivid exposes 77 controls; none of them was writable, which is a finding"
    );
    println!("{written} control write(s) went out and read back through the driver");
}

#[test]
#[ignore = "R2: needs the vivid kernel module loaded; run with `just rung-vivid`"]
fn vivid_a_stream_negotiates_starts_delivers_and_stops_twice_over() {
    // The streaming ioctl sequence against a driver the code has never met: `S_FMT`,
    // `REQBUFS`, `QUERYBUF` + `mmap`, `QBUF`, `STREAMON`, `DQBUF`, `STREAMOFF`,
    // `REQBUFS(0)`. Twice, because a teardown that leaks buffers is only visible from the
    // second start.
    //
    // vivid generates test patterns, so unlike R3 this arm can run on a machine with no
    // camera at all — which is exactly the hole R2 exists to narrow.
    let Some((backend, cameras)) = vivid_cameras() else {
        return;
    };

    let mut streamed = 0usize;
    for info in &cameras {
        if info.capture_node().is_none() {
            continue;
        }
        let mut camera = backend
            .open(&info.id)
            .unwrap_or_else(|error| panic!("{}: could not be opened: {error}", info.id));

        for cycle in 1..=2u32 {
            let negotiated = camera
                .start_stream(&StreamRequest::default())
                .unwrap_or_else(|error| panic!("{}: cycle {cycle} start: {error}", info.id));
            for index in 0..FRAMES_PER_VIVID_CYCLE {
                let deadline =
                    Instant::now() + Duration::from_millis(schema::limits::FRAME_DEADLINE_MS);
                let frame = camera.next_frame(deadline).unwrap_or_else(|error| {
                    panic!("{}: cycle {cycle} frame {index}: {error}", info.id)
                });
                assert_eq!(frame.pixel_format, negotiated.pixel_format);
                assert_eq!(
                    (frame.width, frame.height),
                    (negotiated.width, negotiated.height)
                );
                assert!(!frame.bytes.is_empty(), "frame {index} carries no bytes");
            }
            camera
                .stop_stream()
                .unwrap_or_else(|error| panic!("{}: cycle {cycle} stop: {error}", info.id));
        }
        streamed += 1;
        println!("{}: two stream cycles through the real ioctl path", info.id);
    }

    assert!(
        streamed > 0,
        "no vivid camera had a capture node, which vivid always provides"
    );
}

#[test]
#[ignore = "R2: needs the vivid kernel module loaded; run with `just rung-vivid`"]
fn vivid_a_second_stream_on_one_node_is_refused_as_busy_rather_than_unsupported() {
    // E3's line, on the one path that can actually produce it: V4L2 allows a single
    // streamer per node, and a second `start_stream` must say the device is *unavailable*
    // rather than that it cannot stream. The two answers send a caller in opposite
    // directions — retry, or give up.
    let Some((backend, cameras)) = vivid_cameras() else {
        return;
    };

    let mut refused = 0usize;
    for info in cameras.iter().filter(|i| i.capture_node().is_some()) {
        let mut camera = backend
            .open(&info.id)
            .unwrap_or_else(|error| panic!("{}: could not be opened: {error}", info.id));
        camera
            .start_stream(&StreamRequest::default())
            .unwrap_or_else(|error| panic!("{}: first start: {error}", info.id));

        let error = camera
            .start_stream(&StreamRequest::default())
            .expect_err("a node already streaming must refuse a second start");
        assert_eq!(
            error.kind(),
            schema::ErrorKind::Busy,
            "{}: a second start answered {error} instead of Busy",
            info.id
        );
        camera.stop_stream().expect("stop");
        // And the refusal was about the *stream*, not the camera: after the stop, the
        // same handle streams again. Without this the arm would pass on a backend that
        // wedged itself.
        camera
            .start_stream(&StreamRequest::default())
            .unwrap_or_else(|error| panic!("{}: restart after stop: {error}", info.id));
        camera.stop_stream().expect("stop");
        refused += 1;
    }

    assert!(refused > 0, "no vivid camera offered a capture node");
    println!("{refused} node(s) refused a second concurrent stream as Busy");
}

/// Enough writes per node to be a claim, few enough that 77 controls do not make the rung
/// a coffee break.
const WRITES_PER_VIVID_NODE: usize = 8;
/// Enough frames to see the driver recycle its four buffers.
const FRAMES_PER_VIVID_CYCLE: u32 = 6;

#[test]
#[ignore = "R2: needs the vivid kernel module loaded; run with `just rung-vivid`"]
fn vivid_a_calibration_sweep_sets_settles_captures_and_scores_through_the_real_ioctl_path() {
    // The P3c sweep loop meeting a real driver without touching a camera (docs/9's "Vivid
    // sweep arm" row). Everything the executor does goes out as an ioctl here — the
    // guarded write, the read-back, `S_FMT`/`REQBUFS`/`STREAMON`/`DQBUF`, the stop — and
    // the samples that come back are scored on frames a kernel produced.
    //
    // What is claimed is the *plumbing*, as everywhere on this rung: that a sweep planned
    // from a driver's own range writes, settles, captures, scores and records against that
    // driver. Not device quirks — vivid does not clamp silently or move INACTIVE bits
    // (§3.3 item 4), so `applied` equalling `requested` here is vivid being well-behaved
    // and not evidence about a camera.
    //
    // It restores what it touched and asserts the restoration, through the same persisted
    // pre-sweep snapshot a crash would use.
    let Some((backend, cameras)) = vivid_cameras() else {
        return;
    };
    let Some(info) = cameras.iter().find(|info| info.capture_node().is_some()) else {
        println!("SKIP: no vivid camera offered a capture node to sweep against");
        return;
    };
    let mut camera = backend
        .open(&info.id)
        .unwrap_or_else(|error| panic!("{}: could not be opened: {error}", info.id));

    // A source format this build can decode. Asked of `imaging` rather than listed here:
    // D6's closed source set has one home, and a second copy would be the thing that stops
    // agreeing with it.
    let decodable = imaging::decode::SourceFormat::all_pixel_formats();
    let formats = camera
        .formats()
        .unwrap_or_else(|error| panic!("{}: formats() failed: {error}", info.id));
    let Some(pixel_format) = formats
        .iter()
        .map(|entry| entry.pixel_format)
        .find(|format| decodable.contains(format))
    else {
        println!(
            "SKIP: this vivid build offers no pixel format in D6's source set; it has {:?}",
            formats
                .iter()
                .map(|entry| entry.pixel_format.to_string())
                .collect::<Vec<_>>()
        );
        return;
    };

    let controls = camera
        .controls()
        .unwrap_or_else(|error| panic!("{}: controls() failed: {error}", info.id));
    let Some(desc) = controls.iter().find(|desc| {
        battery::is_perturbable(desc)
            && !battery::is_motorized(&desc.slug)
            && desc.control_type == ControlType::Integer
            && sweep_span(desc).is_some()
    }) else {
        println!(
            "SKIP: none of this vivid build's {} controls is a writable integer with room \
             for three samples",
            controls.len()
        );
        return;
    };
    let control = desc.slug.clone();
    let original = desc
        .current
        .clone()
        .expect("a perturbable control has a current value");
    let values = sweep_span(desc).expect("the filter above already asked");

    let temp = engine::store::TempStore::new().expect("a scratch state directory");
    let lock = temp
        .store()
        .lock(engine::store::LockProtocol::PerOperation)
        .expect("nothing else holds a scratch store");
    let now = schema::Stamp::now();
    let mut session = engine::lifecycle::create(
        temp.store(),
        &lock,
        &engine::lifecycle::SessionSpec {
            id: uuid::Uuid::now_v7(),
            fingerprint: info.fingerprint.clone(),
            task: "R2 sweep".to_owned(),
            goal: "the sweep loop meets a real driver".to_owned(),
            criteria: Vec::new(),
            tool_version: schema::TOOL_VERSION.to_owned(),
        },
        now,
    )
    .expect("a free slot in a fresh scratch store");

    // Session start, as the product does it: probe the pairs empirically and record what
    // the probe found *and* what it declined (D3, PF:3).
    let found =
        engine::lifecycle::discover_pairs(temp.store(), &lock, &mut session, camera.as_mut(), now)
            .unwrap_or_else(|error| panic!("{}: pair discovery failed: {error}", info.id));
    println!(
        "{}: probe measured {} pair(s), declined {}, restore complete: {}",
        info.id,
        found.pairs.len(),
        found.skipped.len(),
        found.left_the_camera_alone()
    );

    let progress = engine::progress::Recorder::new();
    let clock = engine::settle::MonotonicClock::new();
    let outcome = engine::calibrate::run(
        &engine::calibrate::SweepContext {
            store: temp.store(),
            lock: &lock,
            clock: &clock,
            progress: &progress,
            started_at: now,
        },
        &mut session,
        camera.as_mut(),
        &engine::calibrate::SweepRequest {
            control: control.clone(),
            plan: schema::session::SweepSpec::Explicit {
                values: values.clone(),
            },
            allow_motion: false,
            stream: StreamRequest {
                pixel_format: Some(pixel_format),
                ..StreamRequest::default()
            },
            // A real driver on a busy machine is slower than a synthesizer, and the
            // default settle is `limits::DEFAULT_SETTLE_SKIP_FRAMES` frames deep.
            settle: schema::capture::SettlePolicy::default(),
            photo_format: schema::PhotoFormat::Jpeg,
        },
    )
    .unwrap_or_else(|error| panic!("{control}: the sweep failed at the driver: {error}"));

    assert_eq!(
        outcome.samples.len(),
        values.len(),
        "{control}: the sweep took a different number of samples than it planned"
    );
    let session_dir = temp.store().session_dir(&session);
    for (sample, requested) in outcome.samples.iter().zip(&values) {
        assert_eq!(sample.requested, *requested);
        // The read-back came from the driver, whatever it says: E4's pair, at the one
        // layer that could have quietly recorded the request instead.
        assert!(
            desc.range.contains(sample.applied),
            "{control}: the driver reported {} for a control declared {}..={}",
            sample.applied,
            desc.range.min,
            desc.range.max
        );
        assert!(
            sample.photo.is_relative(),
            "{control}: {} is not relative to the session directory",
            sample.photo
        );
        let photo = session_dir.join(&sample.photo);
        let bytes =
            std::fs::read(photo.as_std_path()).unwrap_or_else(|error| panic!("{photo}: {error}"));
        assert!(
            !bytes.is_empty(),
            "{photo}: the sweep recorded a photo it did not write"
        );
        // Metric *orderings* are what a hardware rung may assert; that a metric exists and
        // is a number is the weaker claim this one makes, because vivid's test pattern is
        // not a scene anybody focused.
        for (metric, score) in &sample.metrics {
            assert!(
                score.is_finite(),
                "{control}: {metric} scored {score} on a real frame"
            );
        }
    }
    assert_eq!(
        progress.sequence().first().copied(),
        Some("sweep_started"),
        "the progress stream did not open"
    );
    assert_eq!(
        progress.sequence().last().copied(),
        Some("sweep_finished"),
        "the progress stream did not close: {:?}",
        progress.sequence()
    );

    // Leave the driver as it was found, and *assert* it — a teardown nobody checks is a
    // promise (docs/8 Part C). The assertion is about **the control this arm swept**, not
    // about the whole report: vivid's `u8_pixel_array` changes shape when the format is
    // negotiated \[PF:17\], so a snapshot taken before the stream genuinely cannot go back
    // and `RestoreReport::is_complete` is honestly false. Asserting completeness here
    // would be asserting a bug in the driver's favour; the recovery is printed instead.
    let pairs = session.pairs.clone();
    let recovery = engine::lifecycle::recover(
        temp.store(),
        &lock,
        &mut session,
        camera.as_mut(),
        &pairs,
        schema::Stamp::now(),
    )
    .unwrap_or_else(|error| panic!("{control}: recovery failed: {error}"));
    // Summarised rather than `{recovery:?}`: the report holds every writable control on a
    // 77-control driver, payloads included, and a rung's output is meant to be read.
    println!(
        "{control}: swept {} value(s); recovery complete: {}; could not put back: {:?}",
        values.len(),
        recovery
            .report()
            .is_some_and(schema::snapshot::RestoreReport::is_complete),
        recovery
            .report()
            .map(|report| report
                .unrestored()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>())
            .unwrap_or_default()
    );
    let back = camera
        .controls()
        .unwrap_or_else(|error| panic!("{}: controls() failed after the sweep: {error}", info.id))
        .into_iter()
        .find(|found| found.slug == control)
        .unwrap_or_else(|| panic!("{control} disappeared during the sweep"))
        .current;
    assert_eq!(
        back,
        Some(original),
        "{control}: the sweep left the driver somewhere else"
    );
}

/// Three step-aligned values inside this control's range, or `None` when it has no room.
///
/// Small moves from the minimum rather than the extremes: this rung is about the loop
/// reaching the driver, and a sweep does not need to visit a control's corners to prove
/// that.
fn sweep_span(desc: &schema::ControlDesc) -> Option<Vec<i64>> {
    let step = desc.range.effective_step();
    let values: Vec<i64> = (0..3)
        .map(|n| desc.range.min.checked_add(step.checked_mul(n)?))
        .collect::<Option<Vec<i64>>>()?;
    values
        .iter()
        .all(|value| desc.range.contains(*value))
        .then_some(values)
}
