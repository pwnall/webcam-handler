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
//! Nothing here writes a control or moves a motor. P1 is the read path; §5's motor rule
//! and the `hw_motion_` prefix arrive with the writes that need them (P2).
//!
//! wch-suite: prefix=hw_ recipe=smoke-hw

use std::collections::BTreeSet;

use schema::backend::CameraBackend;
use schema::camera::NodeKind;
use schema::control::ControlType;
use schema::profile::DeviceProfile;
use testkit::corpus;
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
