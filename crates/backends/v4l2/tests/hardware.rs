//! R3 — the real-hardware rung (design §3.1, docs/7 G1).
//!
//! Every test here carries the ignore attribute by construction: shared CI has no camera,
//! and that is this plan's largest honest hole (docs/9's recorded limits). `just smoke-hw`
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
//! the failure docs/8 Part C names. Which controls a write arm may touch is not decided
//! here: [`testkit::battery::is_perturbable`] and [`testkit::battery::is_motorized`] are
//! the same predicates the conformance battery uses, so "may this test move the camera
//! somebody is pointing at a person" has one answer in the workspace. Exactly one arm here
//! moves a motor — P3e's bounded PTZ sweep — and it carries the `hw_motion_` prefix, which
//! `smoke-hw` includes by default (owner ruling, 2026-08-08) and excludes under
//! WCH_NO_MOTION=1 as a named, counted skip. Every other arm in this file excludes
//! motorized controls by asking [`testkit::battery::is_motorized`], so renaming the motion
//! arm out of its prefix cannot quietly make the rest of the file move the camera.
//!
//! wch-suite: prefix=hw_ recipe=smoke-hw

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use schema::backend::{Camera, CameraBackend};
use schema::camera::NodeKind;
use schema::capture::StreamRequest;
use schema::control::{ControlDesc, ControlSlug, ControlType, ControlValue, KnownFlag};
use schema::profile::DeviceProfile;
use schema::session::{ControlStatus, Selector, SessionEvent, SweepSpec};
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
/// touched, and docs/8 Part C says a restore nobody checked is a promise rather than a
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
        if freed.is_empty() {
            // Not a pass. The arm's whole subject is that the flag tracks the automation
            // control, and a toggle that moved nothing made no claim about it — counting
            // it as an observation would be a skip wearing a pass (docs/8 Part C).
            println!(
                "SKIP (partial): {}: switching {} off freed no control's INACTIVE bit, so \
                 PF:3 was not exercised on this device",
                info.id, desc.slug
            );
        } else {
            observed += 1;
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

// ---------------------------------------------------------------- P3: calibration (D8)

/// A control this rung may run a whole calibration session over.
///
/// Named in preference order rather than found by predicate, because "a brightness-class
/// control" is what docs/7 P3e asks for and the three UVC controls that move luma directly
/// are the population. Everything else about the choice is asked of the same predicates the
/// battery uses, so a calibration arm can touch nothing a write arm may not.
fn brightness_class_target(controls: &[ControlDesc]) -> Option<&ControlDesc> {
    const BRIGHTNESS_CLASS: [&str; 3] = ["brightness", "gamma", "gain"];
    BRIGHTNESS_CLASS.iter().find_map(|name| {
        controls.iter().find(|desc| {
            desc.slug.as_str() == *name
                && battery::is_perturbable(desc)
                && !battery::is_motorized(&desc.slug)
                && !desc.is_inactive()
                && desc.control_type == ControlType::Integer
                && desc.range.max > desc.range.min
        })
    })
}

/// A session on a scratch store, started the way the product starts one: create, then probe
/// the automation pairs empirically and persist the merge (D3, N16).
///
/// The scratch store is the caller's to keep alive — dropping it removes the tree, photos
/// included, which is [design §5](../../../../docs/6-claude-fable-design-v2.md): a frame may
/// contain a person and test captures live in gitignored scratch directories.
fn start_session(
    store: &engine::store::SessionStore,
    lock: &engine::store::StoreLock,
    camera: &mut dyn Camera,
    info: &schema::camera::CameraInfo,
    task: &str,
    goal: &str,
    now: schema::time::Stamp,
) -> schema::session::Session {
    let mut session = engine::lifecycle::create(
        store,
        lock,
        &engine::lifecycle::SessionSpec {
            id: uuid::Uuid::now_v7(),
            fingerprint: info.fingerprint.clone(),
            task: task.to_owned(),
            goal: goal.to_owned(),
            criteria: vec!["the operator's own eye on the sample photos".to_owned()],
            tool_version: schema::TOOL_VERSION.to_owned(),
        },
        now,
    )
    .unwrap_or_else(|error| {
        panic!(
            "{}: a fresh scratch store refused a session: {error}",
            info.id
        )
    });

    let found = engine::lifecycle::discover_pairs(store, lock, &mut session, camera, now)
        .unwrap_or_else(|error| panic!("{}: pair discovery failed: {error}", info.id));
    println!(
        "{}: probe measured {} pair(s), declined {}, left the camera alone: {}",
        info.id,
        found.pairs.len(),
        found.skipped.len(),
        found.left_the_camera_alone()
    );
    session
}

/// One control's current value, read back off the device.
fn current_of(camera: &mut dyn Camera, slug: &ControlSlug) -> Option<ControlValue> {
    values_of(camera).get(slug.as_str()).cloned()
}

/// Every sample's photo read back off disk, decoded, and its metrics checked for being
/// numbers in their own domain.
///
/// The *content* is never asserted (lighting varies); what is asserted is that the file the
/// sample points at exists, decodes, and that the scores recorded beside it are finite and
/// — for the three metrics that are fractions — inside `0.0..=1.0`. A NaN or a 7.3 in a
/// clipped-highlight fraction is a defect in the imaging half, and a sweep is where it
/// would first meet a real frame.
fn check_samples(
    session_dir: &camino::Utf8Path,
    control: &ControlSlug,
    range: &schema::control::ControlRange,
    plan: &engine::sweep::SweepPlan,
    samples: &[schema::session::Sample],
) {
    assert_eq!(
        samples.len(),
        plan.values.len(),
        "{control}: the sweep took a different number of samples than it planned"
    );
    for (sample, requested) in samples.iter().zip(&plan.values) {
        assert_eq!(sample.requested, *requested, "{control}: sample order");
        // The read-back came off the device, whatever it says [PF:6].
        assert!(
            range.contains(sample.applied),
            "{control}: the driver reported {} for a control declared {}..={}",
            sample.applied,
            range.min,
            range.max
        );
        assert!(
            sample.photo.is_relative(),
            "{control}: {} is not relative to the session directory",
            sample.photo
        );
        let photo = session_dir.join(&sample.photo);
        let bytes = std::fs::read(photo.as_std_path())
            .unwrap_or_else(|error| panic!("{control}: {photo} could not be read: {error}"));
        let decoded = image::load_from_memory(&bytes)
            .unwrap_or_else(|error| panic!("{control}: {photo} does not decode: {error}"));
        assert!(
            decoded.width() > 0 && decoded.height() > 0,
            "{control}: {photo} decoded to nothing"
        );
        for (metric, score) in &sample.metrics {
            assert!(
                score.is_finite(),
                "{control}: {metric} scored {score} on a real frame"
            );
        }
        for metric in [
            schema::metrics::MetricName::ClippedHighlights,
            schema::metrics::MetricName::ClippedShadows,
            schema::metrics::MetricName::MeanLuma,
        ] {
            let score = sample.metrics.get(&metric).copied().unwrap_or_else(|| {
                panic!("{control}: sample {} carries no {metric}", sample.applied)
            });
            assert!(
                (0.0..=1.0).contains(&score),
                "{control}: {metric} is a fraction and scored {score}"
            );
        }
    }
}

/// The scores one metric earned across a sweep, paired with the value the camera held.
fn scores(
    samples: &[schema::session::Sample],
    metric: schema::metrics::MetricName,
) -> Vec<(i64, f64)> {
    samples
        .iter()
        .filter_map(|sample| {
            sample
                .metrics
                .get(&metric)
                .copied()
                .map(|score| (sample.applied, score))
        })
        .collect()
}

#[test]
#[ignore = "R3: needs a camera attached; run with `just smoke-hw`"]
fn hw_a_calibration_session_sweeps_a_brightness_control_selects_applies_and_restores() {
    // G3's headline criterion against real optics: start → plan → sweep → select → apply,
    // and then the camera goes back where the session found it. Everything the executor
    // does reaches a driver here — the guarded write, the read-back, `S_FMT`/`REQBUFS`/
    // `STREAMON`/`DQBUF` per sample, the photo, the atomic session writes — and the frames
    // that are scored came out of a sensor rather than a synthesizer.
    //
    // What is claimed about the *pictures* is deliberately narrow (design §3.1): that each
    // one decodes, that its scores are numbers in their own domain, and one **ordering** —
    // driving a brightness-class control from the bottom of its range to the top makes the
    // frame brighter. Nothing here reads a pixel and nothing asserts a particular value:
    // lighting varies, and the selector's job is to rank what it was given, which is the
    // property asserted instead.
    //
    // Nothing is written into the tree: the session lives in a scratch store that is
    // removed when this test ends, because a sample photo is a camera frame (rubric A12).
    let Some((backend, cameras)) = attached() else {
        return;
    };

    let mut sessions = 0usize;
    for info in &cameras {
        if info.capture_node().is_none() {
            println!(
                "SKIP (partial): {} has no capture node, so a sweep has nothing to photograph",
                info.id
            );
            continue;
        }
        let mut camera = backend
            .open(&info.id)
            .unwrap_or_else(|error| panic!("{}: could not be opened: {error}", info.id));
        let controls = camera
            .controls()
            .unwrap_or_else(|error| panic!("{}: controls() failed: {error}", info.id));
        let Some(desc) = brightness_class_target(&controls).cloned() else {
            println!(
                "SKIP (partial): {} exposes no sweepable brightness-class control",
                info.id
            );
            continue;
        };
        let control = desc.slug.clone();

        let temp = engine::store::TempStore::new().expect("a scratch state directory");
        let lock = temp
            .store()
            .lock(engine::store::LockProtocol::PerOperation)
            .expect("nothing else holds a scratch store");
        let started = schema::time::Stamp::now();
        let mut session = start_session(
            temp.store(),
            &lock,
            camera.as_mut(),
            info,
            "R3 brightness calibration",
            "a sample an operator would call correctly exposed",
            started,
        );

        // `plan` with nothing named is the draft over the *whole* control set (N19): the
        // sweepable controls are queued and the rest carry the device's own reason. The
        // claim that can fail is the one N19 exists for — a control the device enumerates is
        // recorded, never omitted — and it is asserted against the device's enumeration
        // rather than against a list.
        engine::lifecycle::draft(
            temp.store(),
            &lock,
            &mut session,
            camera.as_mut(),
            &[],
            schema::time::Stamp::now(),
        )
        .unwrap_or_else(|error| panic!("{}: the draft failed: {error}", info.id));
        for enumerated in &controls {
            // Queued *or* recorded with a reason. Those are the two halves of N19's
            // sentence, and "absent" is the third possibility it exists to forbid.
            assert!(
                session.queue.contains(&enumerated.slug)
                    || session.controls.contains_key(&enumerated.slug),
                "{}: {} is on the device and the draft neither queued it nor said why not",
                info.id,
                enumerated.slug
            );
        }
        assert!(
            session.queue.contains(&control),
            "{}: the draft did not queue {control}",
            info.id
        );
        let blocked: Vec<String> = session
            .controls
            .iter()
            .filter_map(|(slug, entry)| match &entry.status {
                ControlStatus::Blocked { reason } => Some(format!("{slug} ({reason:?})")),
                _ => None,
            })
            .collect();
        println!(
            "{}: draft covered {} of the device's {} control(s) — {} queued, {} blocked: {}",
            info.id,
            session.queue.len() + blocked.len(),
            controls.len(),
            session.queue.len(),
            blocked.len(),
            if blocked.is_empty() {
                "—".to_owned()
            } else {
                blocked.join(", ")
            }
        );

        // Five values across the declared range, which the planner aligns to the control's
        // own step. Derived from the device rather than written down: `brightness` is
        // 0..=255 on one seed camera and 0..=100 on the other.
        let span = desc.range.max.saturating_sub(desc.range.min);
        let stride = (span / 4).max(desc.range.effective_step());
        let before = values_of(camera.as_mut());
        let progress = engine::progress::Recorder::new();
        let clock = engine::settle::MonotonicClock::new();
        let outcome = engine::calibrate::run(
            &engine::calibrate::SweepContext {
                store: temp.store(),
                lock: &lock,
                clock: &clock,
                progress: &progress,
                started_at: schema::time::Stamp::now(),
            },
            &mut session,
            camera.as_mut(),
            &schema::session::SweepRequest {
                control: control.clone(),
                plan: SweepSpec::Uniform { step: stride },
                allow_motion: false,
                stream: StreamRequest::default(),
                settle: schema::capture::SettlePolicy::default(),
                photo_format: schema::capture::PhotoFormat::Jpeg,
            },
        )
        .unwrap_or_else(|error| panic!("{control}: the sweep failed on {}: {error}", info.id));

        let session_dir = temp.store().session_dir(&session);
        check_samples(
            &session_dir,
            &control,
            &desc.range,
            &outcome.plan,
            &outcome.samples,
        );
        assert!(
            outcome.samples.len() >= 3,
            "{control}: {} sample(s) is too few to show an ordering",
            outcome.samples.len()
        );
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
        match session.controls.get(&control).map(|entry| &entry.status) {
            Some(ControlStatus::Sweeping { done, total, .. }) => assert_eq!(
                (*done, *total),
                (outcome.taken(), outcome.plan.total()),
                "{control}: a finished sweep must leave every planned sample recorded"
            ),
            other => panic!("{control}: a finished sweep left the control {other:?}"),
        }

        // The one ordering. `mean_luma` is exactly the metric with no *preference* (it
        // cannot rank on its own — a selector asking it to is refused), which is why it is
        // the honest one to make a physical claim with: brighter is not "better", it is what
        // this control does.
        let luma = scores(&outcome.samples, schema::metrics::MetricName::MeanLuma);
        let dimmest = luma
            .iter()
            .min_by_key(|(value, _)| *value)
            .copied()
            .unwrap_or_else(|| panic!("{control}: no sample carries a mean luma"));
        let brightest = luma
            .iter()
            .max_by_key(|(value, _)| *value)
            .copied()
            .unwrap_or_else(|| panic!("{control}: no sample carries a mean luma"));
        // …with one condition on making it at all, and the condition is about light rather
        // than about the control. A sensor that received none photographs black at every
        // brightness — the offset has nothing to lift — and "this control does not brighten
        // the frame" would then be a claim about a lens cap. `clipped_shadows == 1.0` for
        // *every* sample is that case and nothing else; anything short of it, and the
        // ordering is asserted.
        let lightless = scores(
            &outcome.samples,
            schema::metrics::MetricName::ClippedShadows,
        )
        .iter()
        .all(|(_, score)| *score >= 1.0);
        if lightless {
            println!(
                "SKIP (partial): {}: every {control} sample is fully clipped to black, so \
                 this camera received no light and the brightness ordering was not claimed",
                info.id
            );
        } else {
            assert!(
                brightest.1 > dimmest.1,
                "{}: {control} at {} measured mean luma {:.4} and at {} measured {:.4} — a \
                 brightness-class control that does not brighten the frame is a finding about \
                 this device, not a test to tune around",
                info.id,
                dimmest.0,
                dimmest.1,
                brightest.0,
                brightest.1
            );
        }
        // The whole curve of every metric, not just the endpoints of one: a rung's output is
        // meant to be read, and what the scores did *between* the ends is where the next
        // reader learns something without re-running.
        for &name in schema::metrics::MetricName::ALL {
            println!(
                "{}: swept {control} — {name} {}",
                info.id,
                scores(&outcome.samples, name)
                    .iter()
                    .map(|(value, score)| format!("{value}:{score:.4}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }

        // Metrics rank; they do not decide (D8). `rms_contrast` is the directed metric with
        // something to say about a brightness sweep — a frame driven to either end of the
        // range loses tonal separation — and the expectation is computed here from the
        // recorded scores, then the selector is checked against it. Never the other way
        // round: a test that asks the ranker where its own winner is proves only that it
        // agrees with itself (N10).
        let metric = schema::metrics::MetricName::RmsContrast;
        let ranked = scores(&outcome.samples, metric);
        let expected = ranked
            .iter()
            .copied()
            .reduce(|best, next| if next.1 > best.1 { next } else { best })
            .unwrap_or_else(|| panic!("{control}: no sample carries {metric}"));
        engine::lifecycle::commit(
            temp.store(),
            &lock,
            &mut session,
            schema::time::Stamp::now(),
            |draft, now| engine::session::select_by_metric(draft, &control, metric, now),
        )
        .unwrap_or_else(|error| panic!("{control}: select failed: {error}"));
        let selector = Selector::Metric { name: metric };
        match session.controls.get(&control).map(|entry| &entry.status) {
            Some(ControlStatus::Calibrated {
                value,
                precision,
                score,
                selector: recorded,
            }) => {
                assert_eq!(
                    *value, expected.0,
                    "{control}: the metric ranked {} best and the record says {value}",
                    expected.0
                );
                assert_eq!(*score, Some(expected.1), "{control}: the recorded score");
                assert_eq!(recorded, &selector, "{control}: the recorded selector");
                assert!(
                    *precision > 0,
                    "{control}: a multi-sample sweep achieved no spacing"
                );
            }
            other => panic!("{control}: select left the control {other:?}"),
        }
        // Whether the metric *ranked* anything, said out loud. A directed metric that scores
        // every sample identically still produces a `Calibrated` record — the tie-break
        // keeps the earliest sample — and that record reads exactly like a choice somebody
        // made. It is the shape docs/8 Part C calls a skip wearing a pass, and the only
        // defence is counting it: this is where a real scene fails to discriminate and says
        // so, rather than in a green line nobody reads.
        if ranked.iter().any(|(_, score)| *score != expected.1) {
            assert!(
                ranked
                    .iter()
                    .any(|(value, score)| *value != expected.0 && *score < expected.1),
                "{control}: {metric} ranked {} best without any sample scoring below it: {ranked:?}",
                expected.0
            );
        } else {
            println!(
                "SKIP (partial): {}: {metric} scored every {control} sample {:.4}, so the \
                 selection was a tie-break rather than a ranking on this device and scene",
                info.id, expected.1
            );
        }
        // The selector identity survived to `log.ndjson`, which is the copy that outlives
        // the process (D9).
        let log = engine::lifecycle::history(temp.store(), &session)
            .unwrap_or_else(|error| panic!("{control}: the log could not be read: {error}"));
        assert!(
            log.iter().any(|entry| matches!(
                &entry.event,
                SessionEvent::Selected { control: chosen, value, selector: recorded }
                    if chosen == &control && *value == expected.0 && recorded == &selector
            )),
            "{control}: no Selected line names {} and {}",
            expected.0,
            selector.label()
        );

        // `--partial` is the only path around uncalibrated controls, and the refusal must
        // write nothing. Which branch runs is a fact about this device's control set, and
        // both branches assert.
        //
        // "Settled" is restated here rather than asked of the engine because
        // `session::is_terminal` is private to it. That is a prediction, not a second home
        // for the rule: if the two ever disagree, this arm fails at its `expect_err` below
        // instead of passing quietly, which is the direction a duplicated rule should fail
        // in.
        let pending = session
            .queue
            .iter()
            .filter(|slug| {
                !matches!(
                    session.controls.get(*slug).map(|entry| &entry.status),
                    Some(ControlStatus::Calibrated { .. } | ControlStatus::Blocked { .. })
                        | Some(ControlStatus::Deferred { .. })
                )
            })
            .count();
        if pending > 0 {
            let held = current_of(camera.as_mut(), &control);
            let refusal = engine::lifecycle::apply(
                temp.store(),
                &lock,
                &mut session,
                camera.as_mut(),
                false,
                schema::time::Stamp::now(),
            )
            .expect_err("a session with pending controls must refuse apply without --partial");
            assert_eq!(
                refusal.kind(),
                schema::ErrorKind::IllegalTransition,
                "{control}: apply refused with {refusal}"
            );
            assert_eq!(
                current_of(camera.as_mut(), &control),
                held,
                "{control}: the refused apply still wrote to the camera"
            );
            println!(
                "{}: apply refused with {pending} control(s) pending, and wrote nothing",
                info.id
            );
        } else {
            println!(
                "SKIP (partial): {}: every queued control is settled, so the --partial \
                 refusal was not exercised on this device",
                info.id
            );
        }

        let report = engine::lifecycle::apply(
            temp.store(),
            &lock,
            &mut session,
            camera.as_mut(),
            true,
            schema::time::Stamp::now(),
        )
        .unwrap_or_else(|error| panic!("{control}: apply --partial failed: {error}"));
        // The applied set is exactly the calibrated controls plus D4's automation
        // switch-offs — the queued-but-uncalibrated ones absent.
        let written: BTreeSet<String> = report
            .writes
            .iter()
            .map(|applied| applied.slug.to_string())
            .collect();
        let mut expected_written: BTreeSet<String> = session
            .calibrated()
            .into_iter()
            .map(|(slug, _)| slug.to_string())
            .collect();
        expected_written.extend(report.disabled_automation.iter().map(ToString::to_string));
        assert_eq!(
            written, expected_written,
            "{control}: apply wrote a set that is not the calibrated controls plus the \
             automation it had to switch off"
        );
        assert_eq!(
            current_of(camera.as_mut(), &control),
            Some(ControlValue::Int(expected.0)),
            "{control}: apply did not leave the camera holding the chosen value"
        );
        // N20: applying a calibration is not borrowing the camera, so the record of what it
        // looked like beforehand is still there.
        assert!(
            session.pre_snapshot.is_some(),
            "{control}: apply consumed the pre-sweep snapshot"
        );
        println!(
            "{}: applied {control}={} (selector {}, score {:.4}), {} write(s), automation off: {:?}",
            info.id,
            expected.0,
            selector.label(),
            expected.1,
            report.writes.len(),
            report.disabled_automation
        );

        // And back. This is the same call an ordinary session end and a crash recovery both
        // make (§6), reading the snapshot that was persisted before the sweep's first write.
        let volatile: BTreeSet<String> = session
            .pre_snapshot
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .entries
                    .iter()
                    .filter(|entry| entry.was_volatile)
                    .map(|entry| entry.control.to_string())
                    .collect()
            })
            .unwrap_or_default();
        let pairs = session.pairs.clone();
        let recovery = engine::lifecycle::recover(
            temp.store(),
            &lock,
            &mut session,
            camera.as_mut(),
            &pairs,
            schema::time::Stamp::now(),
        )
        .unwrap_or_else(|error| panic!("{control}: the restore failed: {error}"));
        let restore = recovery
            .report()
            .unwrap_or_else(|| panic!("{control}: the session carried no snapshot to put back"));
        assert!(
            restore.is_complete(),
            "{}: the restore reported itself incomplete: {:?}",
            info.id,
            restore.unrestored()
        );
        assert!(
            session.pre_snapshot.is_none(),
            "{control}: a complete recovery must consume the snapshot"
        );
        let after = values_of(camera.as_mut());
        for (slug, was) in &before {
            if volatile.contains(slug) {
                continue;
            }
            assert_eq!(
                after.get(slug),
                Some(was),
                "{}: {slug} is {:?} and the sweep found it at {was}",
                info.id,
                after.get(slug)
            );
        }
        sessions += 1;
        println!(
            "{}: calibration session {} — {} sample(s), restore complete, {} control(s) back \
             where the sweep found them",
            info.id,
            session.id,
            outcome.samples.len(),
            before.len()
        );
    }

    if sessions == 0 {
        println!(
            "SKIP: no attached camera offered a brightness-class control and a capture node, \
             so no calibration session ran"
        );
    }
}

/// Values a motion sweep may visit: a few whole steps either side of where the motor is
/// now, clamped into the declared range.
///
/// Deliberately far smaller than [`schema::limits::MAX_MOTION_SWEEP_SAMPLES`] allows.
/// The cap is the ceiling §5 puts on a *caller*; what a test should spend is the least
/// travel that still proves the loop, and the same paragraph's "motors wear" is why. That
/// the cap is real on this device's own range is asserted separately, by the planner,
/// without moving anything.
fn bounded_motion_values(desc: &ControlDesc, from: i64) -> Vec<i64> {
    let step = desc.range.effective_step();
    let mut values: Vec<i64> = (-2i64..=2)
        .filter_map(|k| {
            step.checked_mul(k)
                .and_then(|offset| from.checked_add(offset))
        })
        .filter(|value| desc.range.contains(*value))
        .collect();
    values.dedup();
    values
}

#[test]
#[ignore = "R3: needs a camera attached; run with `just smoke-hw`"]
fn hw_motion_a_bounded_ptz_sweep_returns_the_motor_to_where_it_started() {
    // The one arm in this workspace that moves a physical motor. §5's two halves meet here:
    // in *testing* motors run by default (owner ruling, 2026-08-08) because an untested
    // motor is untested code and the OBSBOT is a PTZ device; `WCH_NO_MOTION=1` excludes this
    // prefix as a named, counted skip for runs where the camera is pointed at someone.
    //
    // Three things are asserted and they are different claims:
    //
    // 1. **The product never moves a motor implicitly.** `sweep::plan` refuses the same plan
    //    without `allow_motion`, on this device's own descriptor.
    // 2. **The motion cap is real on this device's range.** A full-range plan is bounded by
    //    `limits::MAX_MOTION_SWEEP_SAMPLES`, and the planner *says* which cap trimmed it.
    //    Asserted from the descriptor, so it costs no travel.
    // 3. **The motor goes back, and that is checked at the device.** Not from the restore
    //    report alone — a report is a claim, and the position is what the device says.
    //
    // What "the position" means here is narrower than it looks, and \[PF:18\] is why: the
    // OBSBOT acknowledges an absolute pan the instant the ioctl returns, so the value read
    // back is the position the head was *commanded* to, not one anybody measured. That is
    // the strongest statement V4L2 offers on this device — nothing in the control surface
    // reports mechanism state — and the assertion is written knowing it rather than
    // pretending otherwise. It is still the assertion worth making: a restore that never
    // issued the write fails it.
    let Some((backend, cameras)) = attached() else {
        return;
    };

    let mut swept = 0usize;
    for info in &cameras {
        if info.capture_node().is_none() {
            continue;
        }
        let mut camera = backend
            .open(&info.id)
            .unwrap_or_else(|error| panic!("{}: could not be opened: {error}", info.id));
        let controls = camera
            .controls()
            .unwrap_or_else(|error| panic!("{}: controls() failed: {error}", info.id));

        // The motion predicate is the *planner's* — `engine::sweep::is_motion_control` is
        // what decides which cap applies and whether `--allow-motion` is needed, so a suite
        // that used a different rule could sweep a motor the product would have refused.
        let Some(desc) = controls
            .iter()
            .find(|desc| {
                engine::sweep::is_motion_control(&desc.slug)
                    && battery::is_perturbable(desc)
                    && !desc.is_inactive()
                    && desc.control_type == ControlType::Integer
                    && desc.range.max > desc.range.min
            })
            .cloned()
        else {
            println!(
                "SKIP (partial): {} exposes no writable pan/tilt control, so nothing here \
                 has a motor to move",
                info.id
            );
            continue;
        };
        let control = desc.slug.clone();
        let original = desc
            .current
            .clone()
            .unwrap_or_else(|| panic!("{control}: a perturbable control has a current value"));
        let home = original
            .as_int()
            .unwrap_or_else(|| panic!("{control}: a scalar control read back as {original}"));

        // (1) Never implicit.
        let refusal = engine::sweep::plan(&desc, &SweepSpec::All, false)
            .expect_err("a motorized control must refuse a sweep without allow_motion");
        assert_eq!(
            refusal.kind(),
            schema::ErrorKind::IllegalTransition,
            "{control}: the motion refusal came back as {refusal}"
        );

        // (2) The cap binds on the range this device actually declares.
        let step = desc.range.effective_step();
        let full = engine::sweep::plan(&desc, &SweepSpec::All, true)
            .unwrap_or_else(|error| panic!("{control}: a full-range plan was refused: {error}"));
        let unbounded =
            (i128::from(desc.range.max) - i128::from(desc.range.min)) / i128::from(step) + 1;
        assert!(
            full.total() <= schema::limits::MAX_MOTION_SWEEP_SAMPLES,
            "{control}: a full-range plan holds {} samples against a motion cap of {}",
            full.total(),
            schema::limits::MAX_MOTION_SWEEP_SAMPLES
        );
        if unbounded > i128::from(schema::limits::MAX_MOTION_SWEEP_SAMPLES) {
            assert!(
                full.adjustments.iter().any(|adjustment| matches!(
                    adjustment,
                    engine::sweep::SweepAdjustment::Capped { limit, cap, .. }
                        if *limit == schema::limits::MAX_MOTION_SWEEP_SAMPLES
                            && *cap == engine::sweep::SampleCap::Motion
                )),
                "{control}: every step of {}..={} is {unbounded} samples and the plan holds \
                 {} without saying the motion cap trimmed it: {:?}",
                desc.range.min,
                desc.range.max,
                full.total(),
                full.adjustments
            );
            println!(
                "{}: {control} declares {}..={} step {step} — {unbounded} samples at full \
                 stride, bounded to {} by the motion cap [limits::MAX_MOTION_SWEEP_SAMPLES]",
                info.id,
                desc.range.min,
                desc.range.max,
                full.total()
            );
        } else {
            assert_eq!(
                i128::from(full.total()),
                unbounded,
                "{control}: a range that fits under the cap must plan every step of itself"
            );
            println!(
                "SKIP (partial): {}: {control}'s whole range is {unbounded} samples, under \
                 the motion cap of {}, so the cap was not exercised on this device",
                info.id,
                schema::limits::MAX_MOTION_SWEEP_SAMPLES
            );
        }

        // (3) The sweep itself: a handful of steps either side of home.
        let values = bounded_motion_values(&desc, home);
        assert!(
            values.len() >= 3,
            "{control}: {} value(s) around {home} is too few to move a motor and come back",
            values.len()
        );
        let travel = values
            .last()
            .copied()
            .unwrap_or(home)
            .saturating_sub(values.first().copied().unwrap_or(home));

        let temp = engine::store::TempStore::new().expect("a scratch state directory");
        let lock = temp
            .store()
            .lock(engine::store::LockProtocol::PerOperation)
            .expect("nothing else holds a scratch store");
        let mut session = start_session(
            temp.store(),
            &lock,
            camera.as_mut(),
            info,
            "R3 bounded PTZ sweep",
            "the sweep loop drives a real motor and gives it back",
            schema::time::Stamp::now(),
        );

        let before = values_of(camera.as_mut());
        let progress = engine::progress::Recorder::new();
        let clock = engine::settle::MonotonicClock::new();
        let outcome = engine::calibrate::run(
            &engine::calibrate::SweepContext {
                store: temp.store(),
                lock: &lock,
                clock: &clock,
                progress: &progress,
                started_at: schema::time::Stamp::now(),
            },
            &mut session,
            camera.as_mut(),
            &schema::session::SweepRequest {
                control: control.clone(),
                plan: SweepSpec::Explicit {
                    values: values.clone(),
                },
                allow_motion: true,
                stream: StreamRequest::default(),
                settle: schema::capture::SettlePolicy::default(),
                photo_format: schema::capture::PhotoFormat::Jpeg,
            },
        )
        .unwrap_or_else(|error| panic!("{control}: the motion sweep failed: {error}"));

        check_samples(
            &temp.store().session_dir(&session),
            &control,
            &desc.range,
            &outcome.plan,
            &outcome.samples,
        );
        assert!(
            outcome.plan.total() <= schema::limits::MAX_MOTION_SWEEP_SAMPLES,
            "{control}: the executed plan is not bounded by the motion cap"
        );
        // Non-vacuity: a sweep that wrote five values while the motor stayed put would pass
        // every assertion above and have moved nothing.
        let held: BTreeSet<i64> = outcome
            .samples
            .iter()
            .map(|sample| sample.applied)
            .collect();
        assert!(
            held.len() > 1,
            "{control}: every sample came back as the same position {held:?} — the motor did \
             not move"
        );
        // Requested against applied, per sample, in the transcript: whether a motor lands on
        // the position it was sent to is a fact about this device and D3's whole subject
        // \[PF:6\], and printing it is how the next reader learns it without re-running.
        println!(
            "{}: {control} requested -> applied {}",
            info.id,
            outcome
                .samples
                .iter()
                .map(|sample| format!("{}->{}", sample.requested, sample.applied))
                .collect::<Vec<_>>()
                .join(" ")
        );
        assert_eq!(
            progress.sequence().last().copied(),
            Some("sweep_finished"),
            "the progress stream did not close: {:?}",
            progress.sequence()
        );

        let pairs = session.pairs.clone();
        let recovery = engine::lifecycle::recover(
            temp.store(),
            &lock,
            &mut session,
            camera.as_mut(),
            &pairs,
            schema::time::Stamp::now(),
        )
        .unwrap_or_else(|error| panic!("{control}: the restore failed: {error}"));
        let restore = recovery
            .report()
            .unwrap_or_else(|| panic!("{control}: the session carried no snapshot to put back"));
        assert!(
            restore.is_complete(),
            "{}: the restore reported itself incomplete: {:?}",
            info.id,
            restore.unrestored()
        );

        // The headline: the motor is where it started, read off the device rather than
        // taken from the report.
        assert_eq!(
            current_of(camera.as_mut(), &control),
            Some(original.clone()),
            "{}: this test left {control} somewhere other than {original}",
            info.id
        );
        let after = values_of(camera.as_mut());
        assert_eq!(
            after.get(control.as_str()),
            before.get(control.as_str()),
            "{}: {control} did not come back",
            info.id
        );
        swept += 1;
        println!(
            "{}: moved {control} through {:?} — {} sample(s), {travel} units of travel \
             ({} step(s)), and back to {original}",
            info.id,
            values,
            outcome.samples.len(),
            travel / step
        );
    }

    if swept == 0 {
        println!("SKIP: no attached camera exposes a motorized control this arm could sweep");
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
