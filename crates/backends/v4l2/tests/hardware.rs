//! R3 — the real-hardware rung (design §3.1, docs/2 G1).
//!
//! Every test here carries the ignore attribute by construction: shared CI has no camera,
//! and that is this plan's largest honest hole (docs/4's recorded limits). `just smoke-hw`
//! runs them on a machine that has one, and the results are recorded as evidence in
//! `docs/implementation-notes.md`.
//!
//! (The attribute is named around rather than written out, because
//! `ignored-suites-have-recipes.sh` scans for the token and would read this paragraph as a
//! declaration — the same accommodation `unsafe-scope.sh` documents for its own token.)
//!
//! **What this rung asserts, and what it deliberately does not.** Invariants and
//! *orderings*, never pixel content and never a specific camera: lighting varies, and a
//! test that only passes on the author's desk is a test that fails on everyone else's. So
//! "the attached device matches a committed profile" is written as "*if* a camera here has
//! a fingerprint the corpus knows, its enumeration must still match" — a machine with a
//! different camera runs the PF:1 arm and reports a named skip for the corpus arm, rather
//! than failing for having the wrong hardware.
//!
//! **Writes, and §5's motor rule.** P2 adds arms that change the camera's state, and
//! every one of them puts it back and *asserts* that it did — a restore by assumption is
//! the failure docs/3 Part C names. Which controls a write arm may touch is not decided
//! here: [`testkit::battery::is_perturbable`] and [`testkit::battery::is_motorized`] are
//! the same predicates the conformance battery uses, so "may this test move the camera
//! somebody is pointing at a person" has one answer in the workspace. Nothing on this
//! rung moves a motor at all; the `hw_motion_` prefix `smoke-hw` excludes by default is
//! reserved for the sweeps that do.
//!
//! wch-suite: prefix=hw_ recipe=smoke-hw

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use schema::backend::{Camera, CameraBackend};
use schema::camera::NodeKind;
use schema::capture::StreamRequest;
use schema::control::{ControlDesc, ControlType, ControlValue, KnownFlag};
use schema::profile::DeviceProfile;
use testkit::{battery, corpus};
use v4l2::V4l2Backend;

/// Enumerate, or report why this host cannot take part.
///
/// A machine with no camera is not a failure — it is a machine this rung has nothing to
/// say about. The skip is printed rather than silent, because `just smoke-hw` counts what
/// it ran and a quiet return would read as a pass.
fn attached() -> Option<(V4l2Backend, Vec<schema::camera::CameraInfo>)> {
    let backend = V4l2Backend::new();
    match backend.enumerate() {
        Ok(cameras) if cameras.is_empty() => {
            println!("SKIP: no camera is attached to this host");
            None
        }
        Ok(cameras) => Some((backend, cameras)),
        Err(error) => {
            println!("SKIP: this host's cameras could not be enumerated: {error}");
            None
        }
    }
}

/// The committed profile whose fingerprint matches `info`, if the corpus has one.
fn committed_for(info: &schema::camera::CameraInfo) -> Option<(String, DeviceProfile)> {
    corpus::load_all()
        .ok()?
        .into_iter()
        .find(|(_, profile)| {
            profile
                .invariant
                .info
                .fingerprint
                .matches(&info.fingerprint)
        })
        .map(|(path, profile)| (path.file_stem().unwrap_or("?").to_owned(), profile))
}

#[test]
#[ignore = "R3: needs a camera attached; run with `just smoke-hw`"]
fn hw_controls_enumerate_on_every_node_without_panicking() {
    // The PF:1 regression test, forever. The most popular V4L2 crate panics enumerating
    // the Chicony's `Region of Interest Rectangle`, and "a library you can crash by
    // plugging in a webcam is not a library" is the sentence this rung exists to keep
    // true.
    //
    // One camera, one node: `open` picks the capture node, so this walks the control set
    // of every *camera*, not of every node. Nodes that implement no control ioctl are
    // covered by `hw_a_node_that_implements_no_control_ioctl_answers_empty_rather_than_erroring`
    // in the crate itself, which is where reaching them is possible — an earlier version
    // of this comment claimed "including the metadata nodes", which was never true of the
    // code beneath it.
    let Some((backend, cameras)) = attached() else {
        return;
    };

    let mut examined = 0usize;
    let mut compound_seen = false;
    for info in &cameras {
        let camera = backend
            .open(&info.id)
            .unwrap_or_else(|error| panic!("{}: could not be opened: {error}", info.id));

        let controls = camera
            .controls()
            .unwrap_or_else(|error| panic!("{}: controls() failed: {error}", info.id));
        examined += 1;

        for desc in &controls {
            // Every control must round-trip, including the ones this build cannot
            // interpret. A device that enumerates is a device whose answers we can carry.
            let json = serde_json::to_string(desc).expect("a control serializes");
            let back: schema::control::ControlDesc =
                serde_json::from_str(&json).expect("a control deserializes");
            assert_eq!(
                &back, desc,
                "{}: {} changed on a round trip",
                info.id, desc.slug
            );

            if matches!(
                desc.control_type,
                ControlType::Rect
                    | ControlType::Area
                    | ControlType::U8
                    | ControlType::U16
                    | ControlType::U32
                    | ControlType::Unknown { .. }
            ) {
                compound_seen = true;
                println!(
                    "{}: {} is type {:?} (elem_size {}) and enumerated without panicking \
                     [PF:1]",
                    info.id, desc.slug, desc.control_type, desc.elem_size
                );
            }
        }
        println!("{}: {} control(s) enumerated", info.id, controls.len());
    }

    assert!(examined > 0, "no camera was examined");
    // Non-vacuity for the PF:1 claim specifically: on hardware that has no compound
    // control, this test proves that enumeration works but not that it survives the type
    // that used to break it. Say so rather than implying the stronger claim.
    if !compound_seen {
        println!(
            "SKIP (partial): no attached camera exposes a compound control type, so the \
             PF:1 arm of this test did not exercise the type that panics `v4l`"
        );
    }
}

#[test]
#[ignore = "R3: needs a camera attached; run with `just smoke-hw`"]
fn hw_enumeration_matches_the_committed_profile() {
    // Drift is a finding either way: the corpus is stale, or the kernel changed
    // behaviour, and both are worth knowing (design §3.1 R3).
    let Some((_, cameras)) = attached() else {
        return;
    };

    let mut matched = 0usize;
    let mut unknown = Vec::new();
    for info in &cameras {
        let Some((name, profile)) = committed_for(info) else {
            unknown.push(info.card.clone());
            continue;
        };
        matched += 1;

        assert_eq!(
            profile.invariant.info.nodes, info.nodes,
            "{name}: the attached camera's node set differs from the committed profile"
        );
        assert_eq!(
            profile.invariant.info.card, info.card,
            "{name}: card name drift"
        );
        assert_eq!(
            profile.invariant.info.bus_info, info.bus_info,
            "{name}: bus_info drift"
        );
        println!("{name}: enumeration matches the committed profile");
    }

    if !unknown.is_empty() {
        println!(
            "SKIP (partial): {} attached camera(s) have no committed profile: {}",
            unknown.len(),
            unknown.join(", ")
        );
    }
    // A host whose cameras are not in the corpus is a host this arm has nothing to say
    // about, which the module doc promises and an `assert!(matched > 0)` broke: it turned
    // "different hardware" into a red run. The claim is conditional by design — *if* a
    // camera here is one the corpus knows, its enumeration must still match.
    if matched == 0 {
        println!(
            "SKIP: no attached camera matches a committed profile, so this arm made no \
             claim on this host"
        );
    }
}

#[test]
#[ignore = "R3: needs a camera attached; run with `just smoke-hw`"]
fn hw_profile_capture_reproduces_the_committed_invariant_section() {
    // G1's criterion, as a test rather than as a transcript: a fresh capture must equal
    // the committed one in the invariant section, and *differ* in provenance. The state
    // block is excluded by construction — the camera has been used since.
    let Some((backend, cameras)) = attached() else {
        return;
    };

    let mut compared = 0usize;
    for info in &cameras {
        let Some((name, committed)) = committed_for(info) else {
            continue;
        };
        let mut camera = backend
            .open(&info.id)
            .unwrap_or_else(|error| panic!("{}: could not be opened: {error}", info.id));

        let fresh = engine::profile::capture(
            camera.as_mut(),
            &engine::profile::CaptureContext {
                captured_at: schema::time::Stamp::now(),
                kernel: std::fs::read_to_string("/proc/sys/kernel/osrelease")
                    .map(|t| t.trim().to_owned())
                    .unwrap_or_else(|_| "(unknown)".to_owned()),
                tool_version: env!("CARGO_PKG_VERSION").to_owned(),
                capturer: "hw_profile_capture_reproduces_the_committed_invariant_section"
                    .to_owned(),
                backend: backend.kind(),
            },
        )
        .unwrap_or_else(|error| panic!("{name}: capture failed: {error}"));

        assert!(
            fresh.invariant_matches(&committed),
            "{name}: a fresh capture's invariant section differs from the committed one.\n\
             Either the corpus is stale or the kernel changed behaviour — both are \
             findings, and neither is fixed by re-capturing without saying why.\n\
             committed: {:#?}\nfresh: {:#?}",
            committed.invariant,
            fresh.invariant
        );
        assert_ne!(
            fresh.provenance, committed.provenance,
            "{name}: a re-capture must carry its own provenance"
        );
        compared += 1;
        println!("{name}: a fresh capture reproduces the committed invariant section");
    }

    if compared == 0 {
        println!("SKIP: no attached camera matches a committed profile, so nothing was compared");
    }
}

#[test]
#[ignore = "R3: needs a camera attached; run with `just smoke-hw`"]
fn hw_nodes_group_by_interface_and_capture_nodes_are_found_by_capability() {
    // PF:7 and PF:13 on the live tree rather than on a fixture. The measured trap: two
    // logical cameras that report the same `bus_info`.
    let Some((_, cameras)) = attached() else {
        return;
    };

    let mut bus_paths = BTreeSet::new();
    let mut by_bus_info: std::collections::BTreeMap<&str, usize> =
        std::collections::BTreeMap::new();
    for info in &cameras {
        assert!(
            bus_paths.insert(info.fingerprint.bus_path.clone()),
            "{}: two cameras share the bus path {} — grouping has collapsed",
            info.id,
            info.fingerprint.bus_path
        );
        *by_bus_info.entry(info.bus_info.as_str()).or_default() += 1;

        for node in &info.nodes {
            assert_eq!(
                node.kind,
                NodeKind::from_device_caps(node.device_caps),
                "{}: node {} is classified against its own device_caps",
                info.id,
                node.path
            );
        }
        // The claim that can fail: exactly one node in a group may be the capture node,
        // and if any node carries the VIDEO_CAPTURE bit then `capture_node()` must find
        // one. `if let Some(..)` alone asserted nothing — it was true of a camera with no
        // capture node at all, which is the case worth catching.
        let carrying: Vec<&schema::camera::DeviceNode> = info
            .nodes
            .iter()
            .filter(|node| node.device_caps & schema::camera::CAP_VIDEO_CAPTURE != 0)
            .collect();
        assert_eq!(
            info.capture_node().is_some(),
            !carrying.is_empty(),
            "{}: {} node(s) carry VIDEO_CAPTURE but capture_node() answered {:?}",
            info.id,
            carrying.len(),
            info.capture_node().map(|n| n.path.as_str())
        );
        assert!(
            carrying.len() <= 1,
            "{}: {} nodes claim VIDEO_CAPTURE; the group is two cameras, not one",
            info.id,
            carrying.len()
        );
    }

    if let Some((bus_info, count)) = by_bus_info.iter().find(|(_, count)| **count > 1) {
        println!(
            "PF:13 confirmed live: {count} cameras report bus_info {bus_info}, and the \
             interface path is what tells them apart"
        );
    } else {
        println!(
            "SKIP (partial): no two attached cameras share a bus_info, so PF:13's \
             counter-example is not exercised on this host"
        );
    }
}

// ---------------------------------------------------------------- P2: the write path

/// A control this rung may perturb: safe by the battery's own rule, and not a motor.
///
/// Returned with the value to write, so a caller cannot pick a control and then invent a
/// perturbation for it that §5 would not allow.
fn perturbable_target(controls: &[ControlDesc]) -> Option<(ControlDesc, ControlValue)> {
    controls.iter().find_map(|desc| {
        if !battery::is_perturbable(desc) || battery::is_motorized(&desc.slug) {
            return None;
        }
        // An INACTIVE control is an automation partner's to write [PF:3]; driving one
        // here would be testing the guarded-set rule the engine owns, from the wrong
        // layer, and the value would not stick.
        if desc.is_inactive() {
            return None;
        }
        battery::perturbation(desc).map(|value| (desc.clone(), value))
    })
}

/// Write `value`, then put `desc` back where it was and assert that it went.
///
/// The restoration is the assertion, not the cleanup: §5 says a suite restores what it
/// touched, and docs/3 Part C says a restore nobody checked is a promise rather than a
/// fact.
fn write_and_restore(
    camera: &mut dyn Camera,
    desc: &ControlDesc,
    value: ControlValue,
) -> schema::control::Applied {
    let original = desc
        .current
        .clone()
        .expect("a perturbable control has a current value");
    let applied = camera
        .set(desc.id, value)
        .unwrap_or_else(|error| panic!("{}: the write failed: {error}", desc.slug));

    let back = camera
        .set(desc.id, original.clone())
        .unwrap_or_else(|error| panic!("{}: restoring {original} failed: {error}", desc.slug));
    assert_eq!(
        back.applied, original,
        "{}: this test left the camera holding {} instead of {original}",
        desc.slug, back.applied
    );
    applied
}

#[test]
#[ignore = "R3: needs a camera attached; run with `just smoke-hw`"]
fn hw_a_write_reads_back_and_reports_what_the_driver_actually_took() {
    // D3 and E4 against a real driver for the first time: the write goes out through
    // `S_EXT_CTRLS`, the value comes back through `G_EXT_CTRLS`, and both halves of the
    // pair survive to the caller. P1's notes record that "no control on any attached
    // camera was written" — this is the arm that changes that.
    let Some((backend, cameras)) = attached() else {
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

        let Some((desc, value)) = perturbable_target(&controls) else {
            println!(
                "SKIP (partial): {} exposes no safely perturbable control",
                info.id
            );
            continue;
        };
        let applied = write_and_restore(camera.as_mut(), &desc, value.clone());
        written += 1;

        assert_eq!(
            applied.control, desc.id,
            "{}: wrong control reported",
            desc.slug
        );
        assert_eq!(applied.slug, desc.slug);
        assert_eq!(
            applied.requested, value,
            "{}: the request must survive the round trip verbatim (E4)",
            desc.slug
        );
        assert_eq!(
            applied.applied, value,
            "{}: a one-step write inside the declared range was adjusted to {} — that is \
             a finding about this device, not a test failure to paper over",
            desc.slug, applied.applied
        );
        println!(
            "{}: {} {} -> {} (read back from the device)",
            info.id,
            desc.slug,
            desc.current
                .as_ref()
                .map_or_else(|| "?".to_owned(), ToString::to_string),
            applied.applied
        );
    }

    if written == 0 {
        println!("SKIP: no attached camera offered a control this arm could safely write");
    }
}

#[test]
#[ignore = "R3: needs a camera attached; run with `just smoke-hw`"]
fn hw_a_write_past_the_range_is_clamped_and_the_clamp_is_a_warning_not_an_error() {
    // PF:6 on real hardware. The P1 notes list this explicitly under "not established by
    // any of the above — the hardware twin arrives at P2 with the write path", so the
    // interesting outcome is either direction: a driver that clamps confirms the finding,
    // and a driver that *refuses* is a new one worth a PF entry.
    let Some((backend, cameras)) = attached() else {
        return;
    };

    let mut probed = 0usize;
    for info in &cameras {
        let mut camera = backend
            .open(&info.id)
            .unwrap_or_else(|error| panic!("{}: could not be opened: {error}", info.id));
        let controls = camera
            .controls()
            .unwrap_or_else(|error| panic!("{}: controls() failed: {error}", info.id));

        let candidate = controls.iter().find(|desc| {
            battery::is_perturbable(desc)
                && !battery::is_motorized(&desc.slug)
                && !desc.is_inactive()
                && matches!(
                    desc.control_type,
                    ControlType::Integer | ControlType::Integer64
                )
                && desc.range.max > desc.range.min
                && desc.range.max.checked_add(1_000).is_some()
        });
        let Some(desc) = candidate.cloned() else {
            println!(
                "SKIP (partial): {} exposes no non-motorized integer control to probe \
                 clamping on (design §5 keeps motors off their limits)",
                info.id
            );
            continue;
        };

        let beyond = desc.range.max + 1_000;
        let applied = write_and_restore(camera.as_mut(), &desc, ControlValue::Int(beyond));
        probed += 1;

        assert_eq!(applied.requested, ControlValue::Int(beyond));
        let took = applied
            .applied
            .as_int()
            .unwrap_or_else(|| panic!("{}: a scalar read back as {}", desc.slug, applied.applied));
        assert!(
            desc.range.contains(took),
            "{}: a write of {beyond} landed on {took}, outside the declared range \
             [{}..={}]",
            desc.slug,
            desc.range.min,
            desc.range.max
        );
        assert!(
            !applied.warnings.is_empty(),
            "{}: the driver moved {beyond} to {took} and said nothing about it — a silent \
             adjustment is the fact E4 exists to keep",
            desc.slug
        );
        println!(
            "{}: PF:6 live — {} took {took} for a write of {beyond}, warnings {:?}",
            info.id, desc.slug, applied.warnings
        );
    }

    if probed == 0 {
        println!("SKIP: no attached camera offered a control this arm could probe");
    }
}

#[test]
#[ignore = "R3: needs a camera attached; run with `just smoke-hw`"]
fn hw_switching_an_automation_control_moves_its_partners_inactive_bit() {
    // PF:3, live and in both directions. The finding is that a manual control's INACTIVE
    // flag tracks whether its automation partner owns it *right now*, which is what makes
    // the flag diff a usable pair-discovery method (D3 layer 2) — and what makes a
    // guarded write necessary in the first place.
    let Some((backend, cameras)) = attached() else {
        return;
    };

    let mut observed = 0usize;
    for info in &cameras {
        let mut camera = backend
            .open(&info.id)
            .unwrap_or_else(|error| panic!("{}: could not be opened: {error}", info.id));
        let controls = camera
            .controls()
            .unwrap_or_else(|error| panic!("{}: controls() failed: {error}", info.id));

        // A boolean automation control, off a motor, that is currently *on*: switching it
        // off is the direction that frees a partner, and it is also the direction that is
        // safe to leave the camera in for the microseconds before the restore.
        let candidate = controls.iter().find(|desc| {
            schema::pairing::looks_like_automation(desc)
                && !battery::is_motorized(&desc.slug)
                && desc.control_type == schema::control::ControlType::Boolean
                && desc.current.as_ref().and_then(ControlValue::as_int) == Some(1)
        });
        let Some(desc) = candidate.cloned() else {
            println!(
                "SKIP (partial): {} has no enabled non-motorized boolean automation \
                 control to toggle",
                info.id
            );
            continue;
        };

        let inactive_before = inactive_slugs(&controls);
        camera
            .set(desc.id, ControlValue::Int(0))
            .unwrap_or_else(|error| panic!("{}: switching off failed: {error}", desc.slug));
        let during = camera.controls().expect("controls after the switch-off");
        let inactive_during = inactive_slugs(&during);

        // Put it back before asserting anything: a failed assertion must not leave the
        // camera with its automation off.
        let back = camera
            .set(desc.id, ControlValue::Int(1))
            .unwrap_or_else(|error| panic!("{}: switching back on failed: {error}", desc.slug));
        assert_eq!(
            back.applied,
            ControlValue::Int(1),
            "{}: this test left {} switched off",
            info.id,
            desc.slug
        );
        let after = camera.controls().expect("controls after the restore");
        assert_eq!(
            inactive_slugs(&after),
            inactive_before,
            "{}: the INACTIVE set did not come back to where it started",
            info.id
        );

        let freed: Vec<&str> = inactive_before
            .difference(&inactive_during)
            .map(String::as_str)
            .collect();
        observed += 1;
        if freed.is_empty() {
            println!(
                "{}: switching {} off freed no control's INACTIVE bit — this device does \
                 not couple through that flag [PF:3 does not hold here]",
                info.id, desc.slug
            );
        } else {
            println!(
                "{}: PF:3 live — switching {} off freed {}",
                info.id,
                desc.slug,
                freed.join(", ")
            );
        }
    }

    if observed == 0 {
        println!("SKIP: no attached camera offered an automation control this arm could toggle");
    }
}

#[test]
#[ignore = "R3: needs a camera attached; run with `just smoke-hw`"]
fn hw_a_read_only_control_refuses_the_write_rather_than_pretending() {
    // PF:12 — the Chicony's `Privacy` is READ_ONLY, and §5 says the hardware privacy
    // control is honored rather than worked around. The refusal must be the typed
    // capability answer, not a permission problem with the device (E3).
    let Some((backend, cameras)) = attached() else {
        return;
    };

    let mut refused = 0usize;
    for info in &cameras {
        let camera = backend
            .open(&info.id)
            .unwrap_or_else(|error| panic!("{}: could not be opened: {error}", info.id));
        let controls = camera
            .controls()
            .unwrap_or_else(|error| panic!("{}: controls() failed: {error}", info.id));
        let mut camera = camera;

        for desc in controls
            .iter()
            .filter(|d| d.flags.has(KnownFlag::ReadOnly) && d.control_type.is_scalar())
        {
            let error = camera
                .set(desc.id, ControlValue::Int(desc.default))
                .expect_err("a read-only control must refuse the write");
            assert_eq!(
                error,
                schema::Error::ControlReadOnly {
                    control: desc.slug.clone()
                },
                "{}: {} refused with the wrong variant",
                info.id,
                desc.slug
            );
            refused += 1;
            println!("{}: {} is read-only and said so", info.id, desc.slug);
        }
    }

    if refused == 0 {
        println!("SKIP: no attached camera exposes a read-only scalar control");
    }
}

/// The slugs whose INACTIVE bit is set right now.
fn inactive_slugs(controls: &[ControlDesc]) -> BTreeSet<String> {
    controls
        .iter()
        .filter(|desc| desc.is_inactive())
        .map(|desc| desc.slug.to_string())
        .collect()
}

#[test]
#[ignore = "R3: needs a camera attached; run with `just smoke-hw`"]
fn hw_a_stream_negotiates_delivers_frames_and_stops_twice_over() {
    // The whole capture path against real drivers: `S_FMT` negotiation, mmap buffers,
    // `DQBUF` bounded by a deadline, and teardown. Twice, because "stop released
    // everything" is only observable from the second start — the failure this catches is
    // a `REQBUFS(0)` that never happens, which leaves the node busy for everybody.
    //
    // What is asserted is the frame's agreement with its own negotiated header, never its
    // content: lighting varies and a test that reads pixels is a test that fails on
    // somebody else's desk.
    let Some((backend, cameras)) = attached() else {
        return;
    };

    let mut streamed = 0usize;
    for info in &cameras {
        if info.capture_node().is_none() {
            println!(
                "SKIP (partial): {} has no capture node, so it is listed but not streamable",
                info.id
            );
            continue;
        }
        let mut camera = backend
            .open(&info.id)
            .unwrap_or_else(|error| panic!("{}: could not be opened: {error}", info.id));

        for cycle in 1..=2u32 {
            let negotiated = match camera.start_stream(&StreamRequest::default()) {
                Ok(negotiated) => negotiated,
                // E3: a camera somebody else holds has not told us it cannot stream.
                Err(
                    error @ (schema::Error::Busy { .. } | schema::Error::PermissionDenied { .. }),
                ) => {
                    println!("SKIP (partial): {} could not be streamed: {error}", info.id);
                    break;
                }
                Err(error) => panic!("{}: start_stream on cycle {cycle} failed: {error}", info.id),
            };

            for index in 0..FRAMES_PER_HARDWARE_CYCLE {
                let deadline =
                    Instant::now() + Duration::from_millis(schema::limits::FRAME_DEADLINE_MS);
                let frame = camera.next_frame(deadline).unwrap_or_else(|error| {
                    panic!(
                        "{}: frame {index} on cycle {cycle} failed: {error}",
                        info.id
                    )
                });

                assert_eq!(
                    frame.pixel_format, negotiated.pixel_format,
                    "{}: a frame arrived in a format the stream did not negotiate",
                    info.id
                );
                assert_eq!(
                    (frame.width, frame.height),
                    (negotiated.width, negotiated.height),
                    "{}: a frame's size disagrees with the negotiated one",
                    info.id
                );
                assert!(
                    !frame.bytes.is_empty(),
                    "{}: frame {index} carries no bytes",
                    info.id
                );
                if frame.pixel_format.is_compressed() {
                    // PF:9 checked this by hand during the design probe; here it is the
                    // standing assertion, and it is what makes E6's verbatim JPEG sink a
                    // claim about a real bitstream.
                    assert!(
                        frame.bytes.starts_with(&[0xff, 0xd8]),
                        "{}: frame {index} is {} and does not start with a JPEG SOI marker",
                        info.id,
                        frame.pixel_format
                    );
                }
            }

            camera.stop_stream().unwrap_or_else(|error| {
                panic!("{}: stop on cycle {cycle} failed: {error}", info.id)
            });
            if cycle == 2 {
                streamed += 1;
                println!(
                    "{}: streamed {} at {}x{} ({}), two cycles, {} frames each",
                    info.id,
                    negotiated.pixel_format,
                    negotiated.width,
                    negotiated.height,
                    negotiated.interval.fps().map_or_else(
                        || "no rate reported".to_owned(),
                        |fps| format!("{fps:.0} fps")
                    ),
                    FRAMES_PER_HARDWARE_CYCLE
                );
            }
        }

        // Stopping a stopped stream is not an error, and a caller unwinding from a
        // failure must not have to know how far it got.
        camera
            .stop_stream()
            .unwrap_or_else(|error| panic!("{}: a redundant stop failed: {error}", info.id));
    }

    if streamed == 0 {
        println!("SKIP: no attached camera could be streamed");
    }
}

#[test]
#[ignore = "R3: needs a camera attached; run with `just smoke-hw`"]
fn hw_a_stream_honours_a_size_the_camera_offers_and_reports_one_it_does_not() {
    // D5 in both directions on real hardware: a size the device lists must come back
    // exactly and report no adjustment, and a size it does not list must come back
    // *different* and say so. A negotiator that always reported "exact" would pass the
    // first arm alone.
    let Some((backend, cameras)) = attached() else {
        return;
    };

    let mut checked = 0usize;
    for info in &cameras {
        if info.capture_node().is_none() {
            continue;
        }
        let mut camera = backend
            .open(&info.id)
            .unwrap_or_else(|error| panic!("{}: could not be opened: {error}", info.id));
        let formats = camera
            .formats()
            .unwrap_or_else(|error| panic!("{}: formats() failed: {error}", info.id));

        let offered = formats.iter().find_map(|format| {
            format.sizes.iter().find_map(|entry| {
                entry
                    .size
                    .max_dimensions()
                    .map(|wh| (format.pixel_format, wh))
            })
        });
        let Some((pixel_format, (width, height))) = offered else {
            println!("SKIP (partial): {} offers no readable frame size", info.id);
            continue;
        };

        let exact = camera
            .start_stream(&StreamRequest {
                pixel_format: Some(pixel_format),
                width: Some(width),
                height: Some(height),
                ..StreamRequest::default()
            })
            .unwrap_or_else(|error| panic!("{}: a size it listed was refused: {error}", info.id));
        camera.stop_stream().expect("stop");
        assert_eq!(
            (exact.width, exact.height),
            (width, height),
            "{}: asked for a size the device lists and got another",
            info.id
        );
        assert!(
            exact.is_exact(),
            "{}: an honoured request reported adjustments {:?}",
            info.id,
            exact.adjustments
        );

        // Three pixels wide is not a frame size any UVC camera offers, so the driver must
        // adjust — and the adjustment must be *reported*, which is the half a silent
        // negotiator gets wrong.
        let adjusted = camera
            .start_stream(&StreamRequest {
                pixel_format: Some(pixel_format),
                width: Some(3),
                height: Some(3),
                ..StreamRequest::default()
            })
            .unwrap_or_else(|error| {
                panic!(
                    "{}: a tiny size errored instead of adjusting: {error}",
                    info.id
                )
            });
        camera.stop_stream().expect("stop");
        assert!(
            !adjusted.is_exact(),
            "{}: 3x3 came back as {}x{} and was reported exact",
            info.id,
            adjusted.width,
            adjusted.height
        );
        checked += 1;
        println!(
            "{}: D5 live — 3x3 negotiated to {}x{}, reported as {:?}",
            info.id, adjusted.width, adjusted.height, adjusted.adjustments
        );
    }

    if checked == 0 {
        println!("SKIP: no attached camera could be asked to negotiate a size");
    }
}

/// Enough frames to see the driver recycle its buffers, and few enough not to make
/// `just smoke-hw` a coffee break. The default buffer count is four.
const FRAMES_PER_HARDWARE_CYCLE: u32 = 6;

#[test]
#[ignore = "R3: needs a camera attached; run with `just smoke-hw`"]
fn hw_a_snapshot_perturb_restore_round_trip_leaves_every_control_where_it_started() {
    // D4's inverse property on real hardware, and the arm that makes every other
    // write-touching test on this rung safe to run: if this is broken, "restores what it
    // touched" is a promise nothing keeps.
    //
    // Compared against a *re-read* of the device rather than against the snapshot's own
    // values, because the snapshot is what is under test — checking it against itself
    // would pass on a restore that did nothing.
    let Some((backend, cameras)) = attached() else {
        return;
    };

    let mut round_trips = 0usize;
    for info in &cameras {
        let mut camera = backend
            .open(&info.id)
            .unwrap_or_else(|error| panic!("{}: could not be opened: {error}", info.id));
        let controls = camera
            .controls()
            .unwrap_or_else(|error| panic!("{}: controls() failed: {error}", info.id));
        let pairs = engine::pairing::applicable(&controls, &schema::pairing::declared_pairs());

        let Some((target, perturbed)) = perturbable_target(&controls) else {
            println!(
                "SKIP (partial): {} exposes no safely perturbable control",
                info.id
            );
            continue;
        };

        let before = values_of(camera.as_mut());
        let snapshot = engine::snapshot::take(camera.as_mut(), &pairs, schema::time::Stamp::now())
            .unwrap_or_else(|error| panic!("{}: snapshot failed: {error}", info.id));
        assert!(
            !snapshot.entries.is_empty(),
            "{}: a snapshot of nothing restores nothing",
            info.id
        );

        camera
            .set(target.id, perturbed.clone())
            .unwrap_or_else(|error| panic!("{}: perturbing failed: {error}", info.id));
        assert_ne!(
            values_of(camera.as_mut()).get(target.slug.as_str()),
            before.get(target.slug.as_str()),
            "{}: the perturbation did not move {} — this arm would pass vacuously",
            info.id,
            target.slug
        );

        let report = engine::snapshot::restore(camera.as_mut(), &pairs, &snapshot)
            .unwrap_or_else(|error| panic!("{}: restore failed: {error}", info.id));
        let after = values_of(camera.as_mut());

        // Every control the snapshot recorded is back. Volatile ones are excluded by the
        // report, not by this comparison: their value is the device's to choose, and
        // demanding it be identical would be asserting the device is not what it says.
        let volatile: BTreeSet<String> = snapshot
            .entries
            .iter()
            .filter(|entry| entry.was_volatile)
            .map(|entry| entry.control.to_string())
            .collect();
        for entry in &snapshot.entries {
            if volatile.contains(entry.control.as_str()) {
                continue;
            }
            let slug = entry.control.as_str();
            assert_eq!(
                after.get(slug),
                before.get(slug),
                "{}: {slug} is {:?} and started at {:?}",
                info.id,
                after.get(slug),
                before.get(slug)
            );
        }
        assert!(
            report.is_complete(),
            "{}: the restore reported itself incomplete: {:?}",
            info.id,
            report.unrestored()
        );
        round_trips += 1;
        println!(
            "{}: snapshot({}) → perturb {} → restore, every control back",
            info.id,
            snapshot.entries.len(),
            target.slug
        );
    }

    if round_trips == 0 {
        println!("SKIP: no attached camera offered a control this arm could perturb");
    }
}

#[test]
#[ignore = "R3: needs a camera attached; run with `just smoke-hw`"]
fn hw_a_photo_decodes_at_the_negotiated_size_and_an_mjpg_one_is_the_cameras_own_bytes() {
    // The whole P2 stack on real hardware in one arm: negotiate, settle, capture, render,
    // stamp. What is asserted is the photo's agreement with its *own* report — never its
    // content, because lighting varies and a test that reads pixels fails on somebody
    // else's desk.
    //
    // E6's byte-fidelity claim is checked where it can be: an MJPG source must produce a
    // verbatim rendering, and the stamped file's entropy-coded scan must still be the
    // camera's. Nothing is written into the tree (rubric A12) — a frame may contain a
    // person, and on this rung it will.
    let Some((backend, cameras)) = attached() else {
        return;
    };

    let scratch = tempfile::tempdir().expect("a scratch directory");
    let mut taken = 0usize;
    for info in &cameras {
        if info.capture_node().is_none() {
            println!("SKIP (partial): {} has no capture node", info.id);
            continue;
        }
        let mut camera = backend
            .open(&info.id)
            .unwrap_or_else(|error| panic!("{}: could not be opened: {error}", info.id));

        let path = camino::Utf8PathBuf::from_path_buf(
            scratch
                .path()
                .join(format!("{}.jpg", info.fingerprint.bus_path)),
        )
        .expect("a utf-8 scratch dir");
        let photo = engine::photo::take(
            camera.as_mut(),
            &schema::capture::PhotoRequest {
                stream: StreamRequest::default(),
                settle: schema::capture::SettlePolicy::default(),
                transform: schema::capture::Transform::None,
                sink: schema::capture::Sink::ServerPath { path: path.clone() },
            },
            &engine::settle::MonotonicClock::new(),
            schema::time::Stamp::now(),
        )
        .unwrap_or_else(|error| panic!("{}: the photo failed: {error}", info.id));
        let report = photo.report;

        let bytes = std::fs::read(&path).expect("the photo was written");
        assert_eq!(
            u64::try_from(bytes.len()).expect("fits"),
            report.delivery.byte_count(),
            "{}: the reported byte count is not the file's",
            info.id
        );

        // Decodable at the size the report claims — the one assertion that catches a
        // frame handed on with the wrong dimensions, which is how a decoder reads past a
        // buffer.
        let decoded = image::load_from_memory(&bytes)
            .unwrap_or_else(|error| panic!("{}: the photo does not decode: {error}", info.id));
        assert_eq!(
            (decoded.width(), decoded.height()),
            (report.width, report.height),
            "{}: the photo's size disagrees with its own report",
            info.id
        );

        if report.negotiated.pixel_format.is_compressed() {
            assert!(
                report.rendering.is_verbatim(),
                "{}: an MJPG source must pass through, not re-encode [E6]: {:?}",
                info.id,
                report.rendering
            );
        }
        taken += 1;
        println!(
            "{}: {} {}x{} → {} bytes, {}",
            info.id,
            report.negotiated.pixel_format,
            report.width,
            report.height,
            report.delivery.byte_count(),
            if report.rendering.is_verbatim() {
                "the camera's own bytes [E6]"
            } else {
                "re-encoded"
            }
        );
    }

    if taken == 0 {
        println!("SKIP: no attached camera could take a photo");
    }
}

/// Every control's current value, by slug — the reading a restore is compared against.
fn values_of(camera: &mut dyn Camera) -> std::collections::BTreeMap<String, ControlValue> {
    camera
        .controls()
        .map(|controls| {
            controls
                .into_iter()
                .filter_map(|desc| Some((desc.slug.to_string(), desc.current?)))
                .collect()
        })
        .unwrap_or_default()
}
