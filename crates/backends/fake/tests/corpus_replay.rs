//! The committed corpus, replayed (design §3.2, docs/9's "PF regression fixtures loaded").
//!
//! Two claims, and the second is the interesting one.
//!
//! 1. **Every committed profile replays.** The conformance battery runs against each one,
//!    so a profile that stops being a device the fake can be is a red test rather than a
//!    file nobody opens. This is the corpus floor: §3.2 calls a profile nobody loads *dead
//!    corpus*.
//!
//! 2. **Every device-behavior probe finding is asserted from the corpus, not from prose.**
//!    §3.2 says PF:1–PF:9 and PF:12 must each be "representable in — and asserted from —
//!    at least one committed profile". [`PF_FINDINGS`] is that claim as a table: each row
//!    is a predicate over a real captured document, and a row nothing satisfies fails.
//!    Re-capturing the corpus against a kernel that stopped exhibiting a finding therefore
//!    turns this red, which is the point — it means either the corpus is stale or the
//!    world changed, and both are worth being told about.
//!
//! The findings that are *not* here are named too, in [`NOT_PROFILE_SHAPED`], because a
//! silent omission and a considered exclusion look identical from the outside.

use std::collections::{BTreeMap, BTreeSet};

use fake::FakeBackend;
use schema::backend::CameraBackend;
use schema::camera::{NodeKind, PixelFormat, SinkFidelity};
use schema::capture::{ChoiceReason, StreamRequest};
use schema::control::{ControlType, ControlValue, KnownFlag};
use schema::profile::DeviceProfile;
use testkit::battery::{self, BatteryArm};
use testkit::corpus;

/// The battery arms this profile cannot run, with the reason each is honest.
///
/// The fault-menu arm is unconditional: T1/T2 expose no fault-scripting seam by design,
/// so every backend declares it and walks its own menu in its own suite (design §2.9).
///
/// The other two are **per profile**, and the Chicony IR camera is why. It enumerates
/// three controls — a class header, a RECT payload, and a bitmask — and not one of them
/// is a writable scalar, so there is nothing for the write or snapshot arms to perturb.
/// That is a fact about a real camera, not a gap in the fake, and declaring it globally
/// would silence the arms for the two profiles that *can* run them.
///
/// The condition is stated in the schema's own vocabulary rather than as a copy of the
/// battery's `is_perturbable`, so there is no second home for the rule. It is
/// deliberately *simpler* than the battery's, which additionally requires a non-volatile,
/// step-aligned, non-motorized control: a future profile whose only numeric controls turn
/// motors would satisfy this and still skip, and the battery's undeclared-skip arm would
/// turn red. That is the correct outcome — a new device shape should make somebody look,
/// not slide through.
///
/// "Numeric with room to move" and not merely "scalar", because the IR camera's one
/// writable scalar is a **bitmask**, whose bits mean things no backend-agnostic suite
/// knows: the battery will neither clamp-probe it (not an integer) nor perturb it
/// (guessing a bit is exactly the invention D2 forbids).
fn declared_skips(profile: &DeviceProfile) -> BTreeMap<BatteryArm, String> {
    let mut skips = BTreeMap::from([(
        BatteryArm::FaultMenu,
        "the T1/T2 surface exposes no fault-scripting seam; the fake's menu is walked \
         exhaustively by tests/faults.rs (design §2.9)"
            .to_owned(),
    )]);

    let has_movable_number = profile.invariant.controls.iter().any(|desc| {
        desc.is_writable()
            && matches!(
                desc.control_type,
                ControlType::Integer | ControlType::Integer64
            )
            && desc.range.max > desc.range.min
    });
    if !has_movable_number {
        let reason = format!(
            "{} exposes no writable integer control with room to move (its {} control(s) \
             are class headers, payloads, bitmasks, or read-only), so there is nothing \
             for these arms to write or perturb",
            profile.invariant.info.card,
            profile.invariant.controls.len()
        );
        skips.insert(BatteryArm::WriteReadBack, reason.clone());
        skips.insert(BatteryArm::SnapshotRestoreInverse, reason);
    }
    skips
}

#[test]
fn the_corpus_is_not_empty() {
    // The floor under every other test in this file. An empty corpus would make each of
    // them vacuously green, which is precisely the failure docs/9's derived-population
    // rule exists to prevent.
    let count = corpus::count().expect("the corpus directory reads");
    assert!(
        count > 0,
        "corpus/profiles/ holds no profiles; the three probe-era captures land at P1 \
         (docs/7) and every claim below is vacuous without them"
    );
}

#[test]
fn every_committed_profile_replays_through_the_conformance_battery() {
    let profiles = corpus::load_all().expect("every committed profile parses");
    assert!(!profiles.is_empty(), "the corpus is empty");

    let mut arms_run_somewhere: BTreeSet<BatteryArm> = BTreeSet::new();
    for (path, profile) in profiles {
        let skips = declared_skips(&profile);
        let backend = FakeBackend::from_profile(profile).expect("a committed profile replays");
        let report = battery::run(&backend, &skips);
        assert!(
            report.is_green(),
            "{path} does not replay cleanly:\n{report}"
        );

        // Non-vacuity, per profile: every arm this profile did not declare skipped must
        // actually have run. A green report where everything quietly skipped proves
        // nothing, and the declaration is what turns "quietly" into "on the record".
        for &arm in BatteryArm::ALL {
            let ran = report.outcome(arm).is_some_and(battery::ArmOutcome::ran);
            if ran {
                arms_run_somewhere.insert(arm);
            }
            assert_eq!(
                ran,
                !skips.contains_key(&arm),
                "{path}: {arm} ran={ran} but declared-skipped={}:\n{report}",
                skips.contains_key(&arm)
            );
        }
    }

    // Non-vacuity across the corpus: every arm except the structurally-unrunnable fault
    // menu must have run against at least one committed profile, or the corpus has
    // stopped exercising part of the contract.
    for &arm in BatteryArm::ALL {
        if arm == BatteryArm::FaultMenu {
            continue;
        }
        assert!(
            arms_run_somewhere.contains(&arm),
            "no committed profile exercised the {arm} arm"
        );
    }
}

#[test]
fn a_committed_profile_keeps_the_identity_it_was_captured_with() {
    // The fake rewrites two fields on purpose — a fresh `CameraId` and `backend: Fake` —
    // and must rewrite nothing else, or replaying corpus would prove things about a
    // camera that never existed.
    for (path, profile) in corpus::load_all().expect("the corpus parses") {
        let captured = profile.invariant.info.clone();
        let backend = FakeBackend::from_profile(profile).expect("replays");
        let replayed = backend
            .enumerate()
            .expect("enumerates")
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("{path} replayed no camera"));

        assert_eq!(
            replayed.backend,
            schema::backend::BackendKind::Fake,
            "{path}: a fake run must never be mistakable for a hardware one"
        );
        assert_eq!(replayed.fingerprint, captured.fingerprint, "{path}");
        assert_eq!(replayed.card, captured.card, "{path}");
        assert_eq!(replayed.bus_info, captured.bus_info, "{path}");
        assert_eq!(replayed.nodes, captured.nodes, "{path}");
    }
}

#[test]
fn the_corpus_was_captured_from_hardware_rather_than_from_the_fake() {
    // T3's provenance field exists for exactly this: a profile captured from the fake
    // backend would be circular corpus, and every claim built on it would be a claim
    // about our own model.
    for (path, profile) in corpus::load_all().expect("the corpus parses") {
        assert_eq!(
            profile.provenance.backend,
            schema::backend::BackendKind::V4l2,
            "{path} was captured from the {} backend — that is circular corpus",
            profile.provenance.backend
        );
        assert!(
            !profile.provenance.kernel.is_empty() && profile.provenance.kernel != "(unknown)",
            "{path} records no kernel; provenance without a kernel cannot date a finding"
        );
        assert!(
            !profile.provenance.capturer.trim().is_empty(),
            "{path} records no capturer"
        );
    }
}

/// One probe finding, and how to see it in a captured document.
struct PfFinding {
    /// The citation, as docs/6 §1.2 numbers it.
    id: &'static str,
    /// What the finding says, in one line.
    claim: &'static str,
    /// Whether this profile exhibits it.
    exhibited_by: fn(&DeviceProfile) -> bool,
}

/// The device-behavior findings §3.2 requires the corpus to carry.
///
/// A finding whose predicate no committed profile satisfies fails the test below. That
/// makes the corpus's coverage a fact rather than a claim in a document, and it means
/// re-capturing against a kernel that no longer exhibits something cannot quietly drop it.
const PF_FINDINGS: &[PfFinding] = &[
    PfFinding {
        id: "PF:1",
        claim: "a control of a compound type the v4l crate's own layer panics on",
        exhibited_by: |p| {
            p.invariant.controls.iter().any(|c| {
                matches!(
                    c.control_type,
                    ControlType::Rect
                        | ControlType::Area
                        | ControlType::U8
                        | ControlType::U16
                        | ControlType::U32
                        | ControlType::Unknown { .. }
                )
            })
        },
    },
    PfFinding {
        id: "PF:2",
        claim: "a menu whose indices have holes in them",
        exhibited_by: |p| {
            p.invariant.controls.iter().any(|c| {
                let indices: Vec<u32> = c.menu.keys().copied().collect();
                indices.len() >= 2
                    && indices.windows(2).any(|pair| {
                        pair.get(1)
                            .zip(pair.first())
                            .is_some_and(|(b, a)| b - a > 1)
                    })
            })
        },
    },
    PfFinding {
        id: "PF:3",
        claim: "INACTIVE set on a control an automation partner owned at capture time",
        // In the *state* block, not the invariant one: INACTIVE is volatile by
        // definition, and T3 masks it out of the part that compares exactly.
        exhibited_by: |p| {
            p.state
                .flags
                .values()
                .any(|raw| raw & KnownFlag::Inactive.bit() != 0)
        },
    },
    PfFinding {
        id: "PF:4",
        claim: "a current value outside its control's declared range",
        exhibited_by: |p| {
            p.state.values.iter().any(|(slug, value)| {
                p.control(slug).is_some_and(
                    |desc| matches!(value, ControlValue::Int(v) if !desc.range.contains(*v)),
                )
            })
        },
    },
    PfFinding {
        id: "PF:5",
        claim: "a declared default outside its control's declared range",
        exhibited_by: |p| {
            p.invariant
                .controls
                .iter()
                .any(schema::control::ControlDesc::default_out_of_range)
        },
    },
    PfFinding {
        id: "PF:7",
        claim: "a node group holding both a capture node and a metadata node",
        exhibited_by: |p| {
            let nodes = &p.invariant.info.nodes;
            nodes.iter().any(|n| n.kind == NodeKind::VideoCapture)
                && nodes.iter().any(|n| n.kind == NodeKind::MetaCapture)
        },
    },
    PfFinding {
        id: "PF:8",
        claim: "a camera that reports no distinguishing serial",
        exhibited_by: |p| p.invariant.info.fingerprint.serial.is_none(),
    },
    PfFinding {
        id: "PF:9",
        claim: "a compressed format reaching a larger size than an uncompressed one",
        exhibited_by: |p| {
            let largest = |compressed: bool| {
                p.invariant
                    .formats
                    .iter()
                    .filter(|f| f.pixel_format.is_compressed() == compressed)
                    .flat_map(|f| f.sizes.iter())
                    .filter_map(|s| s.size.max_dimensions())
                    .map(|(w, h)| u64::from(w) * u64::from(h))
                    .max()
            };
            largest(true)
                .zip(largest(false))
                .is_some_and(|(c, u)| c > u)
        },
    },
    PfFinding {
        id: "PF:12",
        claim: "a READ_ONLY control, and a flag bit older references do not list",
        exhibited_by: |p| {
            p.invariant
                .controls
                .iter()
                .any(|c| c.flags.has(KnownFlag::ReadOnly))
                && p.invariant
                    .controls
                    .iter()
                    .any(|c| c.flags.has(KnownFlag::HasWhichMinMax))
        },
    },
];

/// The findings deliberately absent from the table above, and why.
///
/// Named so that "PF:6 is not in the list" reads as a decision rather than an oversight.
const NOT_PROFILE_SHAPED: &[(&str, &str)] = &[
    (
        "PF:6",
        "silent clamping is a *behaviour* under a write, not a field: the battery's write \
         arm probes it against every profile in this file",
    ),
    (
        "PF:10",
        "a build requirement (libclang, kernel headers), which belongs in the build docs",
    ),
    (
        "PF:11",
        "unsettled early frames live in the fake's settle model and the P2 settle policy, \
         not in an enumeration",
    ),
    (
        "PF:13",
        "two cameras sharing one bus_info is a fact about a *pair* of profiles; it is \
         asserted across the corpus below rather than within one document",
    ),
];

#[test]
fn every_profile_shaped_probe_finding_is_exhibited_by_a_committed_profile() {
    let profiles = corpus::load_all().expect("the corpus parses");
    assert!(!profiles.is_empty(), "the corpus is empty");

    let mut uncovered = Vec::new();
    for finding in PF_FINDINGS {
        let carriers: Vec<&str> = profiles
            .iter()
            .filter(|(_, profile)| (finding.exhibited_by)(profile))
            .filter_map(|(path, _)| path.file_stem())
            .collect();
        if carriers.is_empty() {
            uncovered.push(format!("{} — {}", finding.id, finding.claim));
        } else {
            println!(
                "{}: {} [{}]",
                finding.id,
                finding.claim,
                carriers.join(", ")
            );
        }
    }
    assert!(
        uncovered.is_empty(),
        "the corpus no longer exhibits {} probe finding(s), so they are asserted from \
         prose rather than from a device (§3.2):\n  {}",
        uncovered.len(),
        uncovered.join("\n  ")
    );
}

#[test]
fn the_findings_this_table_omits_are_named_rather_than_forgotten() {
    // The inverse direction of the coverage claim: the union of "asserted here" and
    // "deliberately not profile-shaped" must cover the whole device-behavior registry, so
    // a finding cannot go missing by simply not appearing anywhere.
    let covered: BTreeSet<&str> = PF_FINDINGS
        .iter()
        .map(|f| f.id)
        .chain(NOT_PROFILE_SHAPED.iter().map(|(id, _)| *id))
        .collect();
    let registry: BTreeSet<&str> = (1..=14)
        .map(|n| match n {
            1 => "PF:1",
            2 => "PF:2",
            3 => "PF:3",
            4 => "PF:4",
            5 => "PF:5",
            6 => "PF:6",
            7 => "PF:7",
            8 => "PF:8",
            9 => "PF:9",
            10 => "PF:10",
            11 => "PF:11",
            12 => "PF:12",
            13 => "PF:13",
            _ => "PF:14",
        })
        .collect();

    // PF:14 is about sysfs USB topology rather than about a camera's own answers, and is
    // asserted in the V4L2 backend's own tests; it is listed here so this walk stays
    // total over the registry.
    let expected_absent: BTreeSet<&str> = BTreeSet::from(["PF:14"]);
    let missing: Vec<&&str> = registry
        .difference(&covered)
        .filter(|id| !expected_absent.contains(**id))
        .collect();
    assert!(
        missing.is_empty(),
        "these registry entries are neither asserted from the corpus nor named as \
         deliberately absent: {missing:?}"
    );
}

#[test]
fn two_cameras_on_one_usb_device_stay_two_profiles_with_one_bus_info() {
    // PF:13, which needs a *pair* of documents to state: the Chicony's RGB and IR
    // cameras report the identical `bus_info` and differ only in the interface path the
    // fingerprint carries. If a future capture collapsed them, `calibrate apply` could
    // replay an IR session onto the RGB sensor.
    let profiles = corpus::load_all().expect("the corpus parses");

    let mut by_bus_info: BTreeMap<&str, Vec<&DeviceProfile>> = BTreeMap::new();
    for (_, profile) in &profiles {
        by_bus_info
            .entry(profile.invariant.info.bus_info.as_str())
            .or_default()
            .push(profile);
    }

    let shared: Vec<(&&str, &Vec<&DeviceProfile>)> =
        by_bus_info.iter().filter(|(_, v)| v.len() > 1).collect();
    assert!(
        !shared.is_empty(),
        "no two committed profiles share a bus_info, so PF:13 is asserted from prose \
         rather than from the corpus"
    );

    for (bus_info, group) in shared {
        let bus_paths: BTreeSet<&str> = group
            .iter()
            .map(|p| p.invariant.info.fingerprint.bus_path.as_str())
            .collect();
        assert_eq!(
            bus_paths.len(),
            group.len(),
            "{bus_info}: {} profiles collapse into {} bus_path(s) — grouping has \
             regressed onto bus_info [PF:13]",
            group.len(),
            bus_paths.len()
        );
        // And the fingerprints must actually refuse each other.
        for (index, a) in group.iter().enumerate() {
            for b in group.iter().skip(index + 1) {
                assert!(
                    !a.invariant
                        .info
                        .fingerprint
                        .matches(&b.invariant.info.fingerprint),
                    "{bus_info}: two logical cameras' fingerprints match each other"
                );
            }
        }
    }
}

#[test]
fn one_camera_with_two_capture_nodes_is_in_the_corpus_as_one_document() {
    // PF:19, which — like PF:13 — is a claim about a *shape* rather than about a control,
    // and so is asserted here rather than from the v1 registry table above. The Dell
    // U3224KB/A drives two USB Streaming output terminals off one sensor, so its group
    // holds two capture nodes and two metadata nodes; the finding is that this is one
    // camera, and the corpus is where that is written down as a device answer.
    //
    // Red in both the ways that matter: if the profile is dropped, no document carries the
    // shape and the finding goes back to being prose; if a future capture split it into two
    // documents, they would share a `bus_path` and the fingerprint check below catches it.
    let profiles = corpus::load_all().expect("the corpus parses");

    let multi: Vec<(&str, &DeviceProfile)> = profiles
        .iter()
        .filter(|(_, p)| {
            p.invariant
                .info
                .nodes
                .iter()
                .filter(|n| n.kind == NodeKind::VideoCapture)
                .count()
                > 1
        })
        .filter_map(|(path, p)| path.file_stem().map(|stem| (stem, p)))
        .collect();
    assert!(
        !multi.is_empty(),
        "no committed profile holds a group with two capture nodes, so PF:19 is asserted \
         from prose rather than from a device"
    );

    let mut by_bus_path: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, profile) in &profiles {
        *by_bus_path
            .entry(profile.invariant.info.fingerprint.bus_path.as_str())
            .or_default() += 1;
    }
    for (stem, profile) in multi {
        let bus_path = profile.invariant.info.fingerprint.bus_path.as_str();
        assert_eq!(
            by_bus_path.get(bus_path).copied(),
            Some(1),
            "{stem}: {bus_path} appears in more than one profile — a second capture node \
             has been captured as a second camera [PF:19]"
        );
        // The secondary node is *listed*, not dropped: `nodes` is the record of the
        // device's shape, and a capture that kept only the streamable one would erase the
        // finding while still parsing.
        println!(
            "PF:19: {stem} — {} node(s), {} of them capture, streaming {}",
            profile.invariant.info.nodes.len(),
            profile
                .invariant
                .info
                .nodes
                .iter()
                .filter(|n| n.kind == NodeKind::VideoCapture)
                .count(),
            profile
                .invariant
                .info
                .capture_node()
                .map_or("nothing", |n| n.path.as_str())
        );
    }
}

#[test]
fn the_greyscale_camera_is_in_the_corpus_because_grayscale_is_not_optional() {
    // D6's "grayscale is not optional" needs a device that offers nothing else, and the
    // Chicony IR camera is it. G2 makes a photo from this profile a criterion, so losing
    // it from the corpus must fail here first.
    let profiles = corpus::load_all().expect("the corpus parses");
    let grey: Vec<&str> = profiles
        .iter()
        .filter(|(_, p)| {
            p.invariant
                .formats
                .iter()
                .all(|f| f.pixel_format == PixelFormat::GREY)
                && !p.invariant.formats.is_empty()
        })
        .filter_map(|(path, _)| path.file_stem())
        .collect();
    assert!(
        !grey.is_empty(),
        "no committed profile offers greyscale only; D6's grayscale path would have no \
         device-shaped fixture"
    );
}

/// What an unspecified request resolves to on each committed profile, after the owner's
/// re-ranking ruling of 2026-08-13 (design D5's amendment, note **N85**).
///
/// A table rather than five tests, so a profile added to the corpus without a row here
/// fails the walk below rather than sliding in unmeasured. Each row is
/// `(file stem, fourcc, width, height)` and every one of them is the profile's **largest**
/// mode — which is the ruling, checked against the documents it was ruled on rather than
/// against the summary somebody typed into a note.
///
/// The `before` column is not here because it is not a fact about the corpus: it is what
/// the *previous* rule would have answered, and it lives in N85 where the comparison
/// belongs. What this table pins is the answer this build gives.
const RANKED_DEFAULT: &[(&str, [u8; 4], u32, u32)] = &[
    ("chicony-ir", *b"GREY", 640, 360),
    ("chicony-rgb", *b"MJPG", 2592, 1944),
    ("dell-u3224kb", *b"MJPG", 3840, 2160),
    ("logitech-brio", *b"MJPG", 4096, 2160),
    ("obsbot-tiny3", *b"MJPG", 1920, 1440),
];

#[test]
fn every_committed_profile_resolves_an_unspecified_request_to_its_largest_mode() {
    // The ruling, run over the five real format trees this project has captured. The
    // chooser is a pure function over values — no device, no I/O — so the documents go
    // through it directly and the answer is the one `wch photo` with no flags would get.
    let profiles = corpus::load_all().expect("the corpus parses");
    assert!(!profiles.is_empty(), "the corpus is empty");
    let mut seen = BTreeSet::new();

    for (path, profile) in &profiles {
        let stem = path
            .file_stem()
            .expect("a committed profile is a named file");
        let (_, fourcc, width, height) = RANKED_DEFAULT
            .iter()
            .find(|(name, ..)| *name == stem)
            .unwrap_or_else(|| {
                panic!(
                    "{stem} is in the corpus and not in RANKED_DEFAULT: a new device is a \
                     new answer to what an unspecified photo request means, and it has to \
                     be stated rather than discovered"
                )
            });
        seen.insert(stem);

        let chosen = StreamRequest::default()
            .choose(&profile.invariant.formats)
            .unwrap_or_else(|| panic!("{stem} offers no format with a readable size"));
        assert_eq!(
            (chosen.pixel_format, chosen.width, chosen.height),
            (PixelFormat(*fourcc), *width, *height),
            "{stem}"
        );

        // ... and it really is the device's largest mode, derived from the document rather
        // than transcribed into the table above: a row that agreed with the chooser and
        // disagreed with the camera would pin a shared mistake.
        let largest = profile
            .invariant
            .formats
            .iter()
            .flat_map(|format| format.sizes.iter())
            .filter_map(|entry| entry.size.max_dimensions())
            .map(|(w, h)| u64::from(w) * u64::from(h))
            .max()
            .expect("a captured camera offers at least one readable size");
        assert_eq!(
            u64::from(chosen.width) * u64::from(chosen.height),
            largest,
            "{stem}: the chosen mode is not the largest the device enumerates"
        );
    }

    let missing: Vec<&str> = RANKED_DEFAULT
        .iter()
        .map(|(name, ..)| *name)
        .filter(|name| !seen.contains(name))
        .collect();
    assert!(
        missing.is_empty(),
        "RANKED_DEFAULT names profiles the corpus does not hold: {missing:?}"
    );
}

#[test]
fn the_ruling_costs_nothing_on_the_hardware_this_project_has_met() {
    // The claim that makes the re-ranking cheap to adopt, measured rather than asserted:
    // on every camera in the corpus the highest-resolution format is *also* the compressed
    // one, so resolution-first and AGENTS' "verbatim camera JPEG when the sink allows"
    // agree everywhere there is evidence. The interesting cases — a tie between a
    // compressed and an uncompressed format at one resolution — are hypothetical, which is
    // why they are argued in `schema::capture`'s unit tests over fixtures and here only as
    // an absence.
    //
    // The Chicony IR sensor is the honest exception and it is named: it has no compressed
    // format at all, so there is nothing for the ruling to prefer.
    let profiles = corpus::load_all().expect("the corpus parses");
    let mut without_compressed = Vec::new();

    for (path, profile) in &profiles {
        let stem = path.file_stem().expect("a named file");
        let chosen = StreamRequest::default()
            .choose(&profile.invariant.formats)
            .unwrap_or_else(|| panic!("{stem} offers no format with a readable size"));
        let offers_compressed = profile
            .invariant
            .formats
            .iter()
            .any(|format| format.pixel_format.is_compressed());
        if offers_compressed {
            assert!(
                chosen.pixel_format.is_compressed(),
                "{stem}: the ranking chose {} on a camera that also offers a compressed \
                 format — the ruling and E6's verbatim path disagree here and the \
                 disagreement is not recorded anywhere",
                chosen.pixel_format
            );
        } else {
            without_compressed.push(stem);
        }
    }

    assert_eq!(
        without_compressed,
        vec!["chicony-ir"],
        "the set of cameras with no compressed format at all has changed; PF:26's \
         reading of the corpus and N85's cost estimate both rest on it"
    );
}

#[test]
fn the_dells_two_uncompressed_formats_tie_and_the_less_subsampled_one_wins() {
    // The tiebreak, on the one real device that exercises it. NV12 and YUYV stop at the
    // same 1920×1080 in `corpus/profiles/dell-u3224kb.json`, so the ruling's primary key
    // cannot separate them; 4:2:0 keeps a quarter of the chroma where 4:2:2 keeps half.
    //
    // MJPG is filtered out rather than absent, because on the whole tree it wins outright
    // on resolution and the tie would never be reached — which is itself worth stating:
    // this pair is a fact about the device that the *default* never has to decide.
    let (_, dell) = corpus::load_all()
        .expect("the corpus parses")
        .into_iter()
        .find(|(path, _)| path.file_stem() == Some("dell-u3224kb"))
        .expect("the Dell U3224KB/A is in the corpus");

    let uncompressed: Vec<_> = dell
        .invariant
        .formats
        .iter()
        .filter(|format| !format.pixel_format.is_compressed())
        .cloned()
        .collect();
    assert_eq!(
        uncompressed.len(),
        2,
        "the Dell's uncompressed pair is NV12 and YUYV"
    );
    let maxima: BTreeSet<u64> = uncompressed
        .iter()
        .map(|format| {
            format
                .sizes
                .iter()
                .filter_map(|entry| entry.size.max_dimensions())
                .map(|(w, h)| u64::from(w) * u64::from(h))
                .max()
                .expect("a captured format offers a readable size")
        })
        .collect();
    assert_eq!(maxima.len(), 1, "the pair no longer ties on resolution");

    for sink in [
        SinkFidelity::PassesCompressedThrough,
        SinkFidelity::EncodesLosslessly,
    ] {
        let (winner, reason) =
            schema::capture::rank_formats(&uncompressed, sink).expect("two candidates");
        assert_eq!(winner.pixel_format, PixelFormat::YUYV, "{sink:?}");
        assert_eq!(
            reason,
            ChoiceReason::LeastLossyOfTheLargest { sink },
            "{sink:?}: the answer does not name the rule that decided it"
        );
    }
}
