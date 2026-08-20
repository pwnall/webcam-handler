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

use camino::Utf8PathBuf;
use fake::FakeBackend;
use schema::backend::CameraBackend;
use schema::camera::{NodeKind, PixelFormat, SinkFidelity};
use schema::capture::{ChoiceReason, StreamRequest};
use schema::control::{ControlType, ControlValue, KnownFlag};
use schema::profile::{DeviceProfile, DeviceVerdict};
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
    //
    // **Asked through the product's own comparison since D15**, rather than through a list
    // of fields written out here. This suite carried that list from P1 until v3 —
    // fingerprint, card, bus_info, nodes, four asserts — and it was a private copy of
    // exactly the rule `CameraInfo::differing_fields` states, which is the second-copy
    // defect design §2.10 names. The promotion runs the other way too: what the product
    // could not have known is that a *replay* must preserve node paths verbatim, because no
    // kernel renumbered anything between the capture and this loop. So that one claim stays
    // here, beside the projection rather than instead of it, and it is the only field-level
    // assertion left.
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
        assert_eq!(
            captured.differing_fields(&replayed),
            vec!["backend".to_owned()],
            "{path}: the fake rewrote something other than the backend it is allowed to \
             rewrite (the id is excluded by the comparison itself, which is where that \
             exclusion is argued)"
        );
        let paths: Vec<&str> = captured
            .nodes
            .iter()
            .map(|node| node.path.as_str())
            .collect();
        let replayed_paths: Vec<&str> = replayed
            .nodes
            .iter()
            .map(|node| node.path.as_str())
            .collect();
        assert_eq!(
            paths, replayed_paths,
            "{path}: a replay renumbers nothing, so the node paths are the captured ones"
        );
    }
}

#[test]
fn every_committed_profile_is_the_same_device_as_its_identity_rewritten_self() {
    // D15's corpus arm, positive half. Identity is where the device *is* — a forwarded bus,
    // another port, another machine legitimately move it — and description is what the
    // device *is*, which must not move. So every committed profile, given somebody else's
    // identity, must still compare device-equal to itself.
    //
    // The rewrite is deliberately total: every `info` field the comparison looks at is
    // replaced, not one of them. A rewrite of a single field would pass against a
    // comparison that had quietly started reading `formats` out of `info`.
    for (path, profile) in corpus::load_all().expect("the corpus parses") {
        let mut forwarded = profile.clone();
        let info = &mut forwarded.invariant.info;
        info.id = schema::camera::CameraId::parse("cam:somewhere-else").expect("a literal id");
        info.fingerprint.bus_path = "9-9:1.9".to_owned();
        info.fingerprint.usb_id = Some(schema::camera::UsbId {
            vendor: 0xdead,
            product: 0xbeef,
        });
        info.fingerprint.serial = Some("A-DIFFERENT-SERIAL".to_owned());
        info.bus_info = "usb-9-9".to_owned();
        info.backend = schema::backend::BackendKind::Fake;

        let comparison = profile.compare(&forwarded);
        assert!(
            comparison.device_matches(),
            "{path}: an identity rewrite moved the device half — {comparison}"
        );
        assert!(
            !comparison.identity.is_empty(),
            "{path}: the identity half reported nothing, so this arm rewrote nothing and \
             proves nothing"
        );
        assert!(
            comparison.device.sections().is_empty(),
            "{path}: {} section(s) disagree",
            comparison.device.sections().len()
        );
    }
}

#[test]
fn no_two_committed_profiles_are_the_same_device_and_each_names_why() {
    // D15's corpus arm, negative half — the one that makes the positive half mean
    // something. A `device_matches` that answered `true` for everything would pass the
    // arm above and be worthless, so every *pair* of distinct profiles is walked and each
    // one must disagree about something the answer can name.
    //
    // Note which pair is the hardest: the Chicony's RGB and IR halves are one physical
    // device, share a USB id and a serial \[PF:13, PF:8\], and differ only in what they can
    // do. That is exactly the pair a comparison keyed on identity would call equal.
    let profiles = corpus::load_all().expect("the corpus parses");
    assert!(
        profiles.len() >= 2,
        "a one-profile corpus cannot exhibit a mutual negative"
    );

    let mut pairs = 0usize;
    for (path_a, a) in &profiles {
        for (path_b, b) in &profiles {
            if path_a >= path_b {
                continue;
            }
            pairs += 1;
            let comparison = a.compare(b);
            assert!(
                !comparison.device_matches(),
                "{path_a} and {path_b} describe the same device: {comparison}"
            );
            let sections = comparison.device.sections();
            assert!(
                !sections.is_empty(),
                "{path_a} vs {path_b}: device_matches said no and no section said why"
            );
            // Both directions, because a comparison that reported a difference one way and
            // not the other would be a diff nobody could rely on.
            assert_eq!(
                sections,
                b.compare(a).device.sections(),
                "{path_a} vs {path_b}: the two orders disagree about which sections differ"
            );
        }
    }
    assert_eq!(
        pairs,
        profiles.len() * (profiles.len() - 1) / 2,
        "the walk skipped pairs"
    );
}

/// The first committed profile that carries at least one format **and** at least one
/// control, with the population named when the corpus has none.
///
/// The subject is chosen **by the shape the arm needs** rather than pinned to whichever file
/// sorts first, and the two are different things: the arm below perturbs a format and a
/// control, so a control-less capture — the Chicony IR camera enumerates three controls and
/// a future one may enumerate none — would make it permanently red for a fact about a real
/// device rather than for a defect. Choosing by shape also closes the other end, which is
/// what the `if let` this replaced left open: a corpus in which *nothing* has both cannot
/// go quietly green, because there is no subject and this says so with the count it looked
/// at (the shape `binary(selectors)`'s arms use — fail loudly when the corpus stops
/// exhibiting what an arm pins). Note **N287** records both ends and the measurement behind
/// each.
fn the_first_profile_that_carries_a_format_and_a_control() -> (Utf8PathBuf, DeviceProfile) {
    let profiles = corpus::load_all().expect("the corpus parses");
    let considered = profiles.len();
    profiles
        .into_iter()
        .find(|(_, profile)| {
            !profile.invariant.formats.is_empty() && !profile.invariant.controls.is_empty()
        })
        .unwrap_or_else(|| {
            panic!(
                "none of the {considered} committed profile(s) carries both a format and a \
                 control, so no committed capture can exhibit a format-tree difference with a \
                 second finding beside it"
            )
        })
}

#[test]
fn a_formats_only_difference_is_the_one_the_owners_ruling_licenses() {
    // The distinction D15 puts on the answer, driven over a real captured document rather
    // than a constructed one: a camera's advertised format tree is invariant within a
    // connection and nowhere else (owner ruling, 2026-08-13; \[PF:23\], note **N89**), so a
    // consumer needs to know when *that* is the only thing that moved.
    let (path, profile) = the_first_profile_that_carries_a_format_and_a_control();

    let mut fewer_modes = profile.clone();
    assert!(
        fewer_modes.invariant.formats.pop().is_some(),
        "{path}: a profile with no formats cannot exhibit a format-tree-only difference"
    );
    let comparison = profile.compare(&fewer_modes);
    assert!(!comparison.device_matches(), "{path}");
    assert!(
        comparison.device_differs_only_in_the_format_tree(),
        "{path}: dropping a pixel format moved something besides the format tree — {comparison}"
    );
    assert_eq!(
        comparison.verdict(),
        DeviceVerdict::OnlyTheFormatTree,
        "{path}: the verdict the document carries disagrees with the predicate that reads it"
    );

    // And with anything beside it the permission goes away, because the ruling is about a
    // device re-deciding what it advertises and not about two findings at once. Unconditional,
    // the way the `formats.pop()` above already is: this claim sat inside an `if let` over the
    // control list until 2026-08-20, so three assertions would have stopped running — silently,
    // with the arm still reporting PASS — the day a control-less capture sorted first (note
    // **N287**).
    let Some(control) = fewer_modes.invariant.controls.first_mut() else {
        panic!(
            "{path}: a profile with no controls cannot exhibit a format difference with a \
             second finding beside it, and this subject was chosen for having one"
        )
    };
    control.range.max = control.range.max.saturating_add(1);
    let both = profile.compare(&fewer_modes);
    assert!(
        !both.device_differs_only_in_the_format_tree(),
        "{path}: a control moved too and the format-tree permission still applied"
    );
    assert_eq!(
        both.verdict(),
        DeviceVerdict::Differs,
        "{path}: two findings at once were given a verdict that licenses one"
    );
    assert!(
        !both.device.controls.is_empty(),
        "{path}: the control that moved is not named"
    );
}

#[test]
fn a_compound_controls_element_count_is_a_device_difference_because_pf17_is_not_absorbed() {
    // PF:17's consequence for D15's device half, asserted rather than left to a design
    // sentence that said the opposite. `vivid`'s `u8_pixel_array` reshapes from `elems=300
    // dims=[15, 20]` to `elems=240 dims=[12, 20]` across an `S_FMT` on one file descriptor —
    // the grid is `ceil(height/16) × ceil(width/16)` — and `profile::invariant_control` clears
    // only `current` and the volatile flag bits, so `elems`, `elem_size` and `dims` stay in the
    // invariant section and D15's *description* half reports a `controls` difference for a
    // device that did not change shape.
    //
    // That is the tree's deliberate behaviour and PF:17's **Retires when:** clause has not
    // fired: retiring it needs a device on which the reshape does not happen *and* a
    // per-control statement of which descriptor fields may move, and this rig has measured
    // neither. So docs/12's T3 records the rule as unabsorbed with PF:17 carrying the status,
    // and this arm is what makes that a fact something checks.
    //
    // **What reddens it is the half of PF:17's fix that D15 actually stands on**, and the two
    // halves are not the same edit — measured on 2026-08-20 by making each one and running
    // this arm. Teaching `invariant_control` to strip `elems` and `dims` leaves this green,
    // because a committed profile is compared *as it was loaded* and that function runs at
    // capture; what reddens it is `differing_control_slugs` comparing descriptors with the
    // payload's shape masked out. So a future fix that moves only the capture side would leave
    // two profiles captured a year apart still disagreeing, and this arm is what says so
    // (note **N288**).
    let profiles = corpus::load_all().expect("the corpus parses");
    let considered = profiles.len();
    let (path, profile, index) = profiles
        .into_iter()
        .find_map(|(path, profile)| {
            let index = profile
                .invariant
                .controls
                .iter()
                .position(|desc| desc.elems > 1 && desc.dims.len() > 1)?;
            Some((path, profile, index))
        })
        .unwrap_or_else(|| {
            panic!(
                "none of the {considered} committed profile(s) carries a multi-dimensional \
                 compound control, so PF:17's reshape has no captured subject and this arm \
                 would be asserting nothing"
            )
        });

    // The reshape PF:17 measured, in its own shape: one row fewer off the leading dimension,
    // and the element count down by the product of the dimensions that row held. Derived from
    // the descriptor's own `dims` rather than from anything under test.
    let mut reshaped = profile.clone();
    let desc = &mut reshaped.invariant.controls[index];
    let slug = desc.slug.clone();
    let row: u32 = desc.dims[1..].iter().product();
    desc.dims[0] -= 1;
    desc.elems -= row;
    assert_ne!(
        profile.invariant.controls[index], reshaped.invariant.controls[index],
        "{path}: the fixture has to differ in the fields under test"
    );
    assert_eq!(
        profile.invariant.controls[index].current, reshaped.invariant.controls[index].current,
        "{path}: this arm is about the payload's shape and not about its value"
    );

    let comparison = profile.compare(&reshaped);
    assert_eq!(
        comparison.device.controls,
        vec![slug.clone()],
        "{path}: a compound control that reshaped is named by its slug and nothing else moved"
    );
    assert_eq!(
        comparison.device.sections(),
        vec!["controls"],
        "{path}: {comparison}"
    );
    assert_eq!(
        comparison.verdict(),
        DeviceVerdict::Differs,
        "{path}: {slug} reshaped and the comparison licensed it"
    );
    assert!(
        !comparison.device_matches(),
        "{path}: the invariant split treats a payload's shape as identity, so this is a device \
         difference until PF:17 retires — {comparison}"
    );
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
        claim: "a measured automation pair, whose two controls this document both carries",
        // **Not** "some captured flag word has the INACTIVE bit set", which is what this
        // row asked for until 2026-08-17 and is what the G6 review's finding **M30** is
        // about. That bit is a photograph of one moment — it says an automation held a
        // control while somebody took a capture — and PF:3's finding is that INACTIVE
        // tracks pairing *live, in both directions*. A static bit cannot carry a claim
        // about what happens when something changes.
        //
        // `measured_pairs` is the field that can, and it is the field the fake's whole
        // coupling model reads (`fake::camera::apply_coupling`): a profile with an empty
        // pair set replays as a device that couples nothing, so before this row moved,
        // every `--backend fake` run in the workspace was against a device exhibiting none
        // of the behaviour three attached cameras were measured exhibiting the same day
        // \[E18\]. The *assertion* half §3.2 asks for is the test below; this predicate is
        // the *representability* half, and it requires both ends of the pair to be present
        // because a pair naming a control the document does not carry is a recipe nothing
        // can follow.
        exhibited_by: |p| {
            p.invariant.measured_pairs.iter().any(|pair| {
                pair.provenance == schema::pairing::Provenance::Measured
                    && p.control(&pair.manual).is_some()
                    && p.control(&pair.automation).is_some()
            })
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
fn the_coupling_a_profile_measured_is_replayed_live_and_in_both_directions() {
    // PF:3's finding, asserted from the corpus rather than from prose — the second half of
    // what §3.2 asks of a profile-shaped finding, and the half the G6 review's **M30** found
    // missing. The row above establishes that a committed document *carries* a measured
    // pair; this establishes that replaying that document produces the behaviour the pair
    // describes, which is the only thing that makes the fake resemble the devices it stands
    // in for (AGENTS: "its claims … are asserted against the probe record of the profile it
    // replays").
    //
    // **Both directions, and the second one is not decoration.** A model that set INACTIVE
    // when automation engaged and never cleared it would pass a one-directional check and
    // would strand every guarded write in the workspace: D3's whole plan is *switch the
    // automation off, then write the manual control*, and a partner whose bit never clears
    // is a control the planner can free and the device still refuses. The three cameras
    // measured on 2026-08-17 all cleared it \[E18\].
    //
    // The `off` recipe comes from the pair the device itself produced, so this test knows no
    // control names: a corpus whose pairs are spelled differently on the next device is
    // still driven correctly, and a recipe that resolves to nothing is a red test rather
    // than a silently skipped one.
    let profiles = corpus::load_all().expect("the corpus parses");
    let mut asserted = 0usize;

    for (path, profile) in &profiles {
        for pair in &profile.invariant.measured_pairs {
            let backend = FakeBackend::from_profile(profile.clone()).expect("replays");
            let id = backend
                .enumerate()
                .expect("enumerates")
                .into_iter()
                .next()
                .unwrap_or_else(|| panic!("{path} replayed no camera"))
                .id;
            let mut camera = backend.open(&id).expect("opens");

            let describe = |camera: &dyn schema::backend::Camera, slug| {
                camera
                    .controls()
                    .expect("the fake enumerates")
                    .into_iter()
                    .find(|desc| &desc.slug == slug)
                    .unwrap_or_else(|| panic!("{path}: {slug} is named by a pair and absent"))
            };

            let automation = describe(camera.as_ref(), &pair.automation);
            let off = pair.off.resolve(&automation).unwrap_or_else(|| {
                panic!(
                    "{path}: the recipe {:?} resolves to nothing against the very control it \
                     was measured on, so this document cannot describe its own device",
                    pair.off
                )
            });
            // Any position that is not "off". Taken from the control's own vocabulary — its
            // menu, or its range — rather than assumed to be 1, because `auto_exposure` is a
            // menu whose engaged positions are 1 and 3 on this hardware and a boolean's are
            // 0 and 1 (design D3's third probe rule, PF:2).
            let engaged = engaged_position(&automation, off).unwrap_or_else(|| {
                panic!(
                    "{path}: {} has no position other than its measured off value {off}, so \
                     nothing here could ever engage",
                    pair.automation
                )
            });

            camera
                .set(automation.id, ControlValue::Int(engaged))
                .expect("the fake takes a write to a control its own profile carries");
            assert!(
                describe(camera.as_ref(), &pair.manual).is_inactive(),
                "{path}: {} engaged at {engaged} and {} did not go INACTIVE — PF:3 says a \
                 device's automation takes its partner over, and a replay that does not is a \
                 fake claiming a capability no real device lacks",
                pair.automation,
                pair.manual
            );

            camera
                .set(automation.id, ControlValue::Int(off))
                .expect("the fake takes the off value its own pair names");
            assert!(
                !describe(camera.as_ref(), &pair.manual).is_inactive(),
                "{path}: {} switched off at {off} and {} stayed INACTIVE — the direction D3's \
                 guarded write depends on, so a partner that never comes back is a control \
                 nothing can drive by hand",
                pair.automation,
                pair.manual
            );

            asserted += 1;
        }
    }

    // Not vacuous, and it fails for the reason M30 named rather than for a missing file: a
    // corpus in which no document carries a measured pair leaves the fake's coupling model
    // — the thing every guarded-write test in this workspace runs against — asserted by
    // nothing at all.
    assert!(
        asserted > 0,
        "no committed profile carries a measured automation pair, so PF:3's coupling is \
         replayed by nothing and the fake's model has no corpus behind it (§3.2)"
    );
    println!("PF:3: {asserted} measured pair(s) driven both ways through the fake");
}

/// A position for `automation` that is not its measured `off` value, in the control's own
/// vocabulary.
///
/// A menu's alternatives are its indices — "a menu is not a switch" is D3's third probe rule
/// and PF:2 says the indices have holes — and a plain integer's are its range. `None` means
/// the control has exactly one position, which the caller reports as the finding it is.
fn engaged_position(automation: &schema::control::ControlDesc, off: i64) -> Option<i64> {
    if !automation.menu.is_empty() {
        return automation
            .menu
            .keys()
            .map(|index| i64::from(*index))
            .find(|index| *index != off);
    }
    [automation.range.min, automation.range.max]
        .into_iter()
        .find(|candidate| *candidate != off)
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
///
/// **A row here moves when a device changes what it advertises, and one just did.**
/// `obsbot-tiny3` read `MJPG 1920x1440` from the day N85 landed until 2026-08-13, because the
/// corpus was captured while the camera had stopped offering 3840×2160 \[PF:23\]. The tree
/// came back whole on a replug, the corpus was re-captured to it under the owner's ruling of
/// that day — advertised support may change at each plug event — and the largest mode moved
/// with it. That is not a re-ranking and nothing about D5 was touched: the rule asked the same
/// question and the *device* gave a different answer. It is recorded here rather than left as
/// a number that changed, because this row is the one place in the workspace where a plug
/// event visibly reaches the product — `webcam-handler-cli photo` with no flags takes a 4K
/// frame off this camera today and took a 1920×1440 one last week, and a reader comparing two
/// sessions' output deserves to be told why rather than to go looking for the commit that
/// re-ranked something.
const RANKED_DEFAULT: &[(&str, [u8; 4], u32, u32)] = &[
    ("chicony-ir", *b"GREY", 640, 360),
    ("chicony-rgb", *b"MJPG", 2592, 1944),
    ("dell-u3224kb", *b"MJPG", 3840, 2160),
    ("logitech-brio", *b"MJPG", 4096, 2160),
    ("obsbot-tiny3", *b"MJPG", 3840, 2160),
    // The one virtual device in the corpus, committed at P9b as the workbench's 77-control
    // layout fixture (D20 sizes the two-pane shell against it rather than against the
    // 18-control common case). Its row is the surprising one, and it is recorded as measured
    // rather than as expected: vivid advertises **83 formats including both `GREY` and
    // `YUYV`**, and D5's key ranks `Lossiness::Lossless` above `ChromaSubsampled`, so an
    // unspecified photo request on this device answers *monochrome* at 4K.
    //
    // That is the rule working as written, and it is also the first time this project has
    // owned a device where the rule can be questioned: `chicony-ir` offers `GREY` alone, so
    // until now "lossless" and "the whole signal" were the same sentence. On a colour source
    // they are not — dropping chroma entirely loses more than keeping half of it — and note
    // **N261** puts the question to the owner rather than answering it here. Nothing about
    // D5 is touched by this commit; what changed is that the corpus can now see it.
    ("vivid", *b"GREY", 3840, 2160),
];

#[test]
fn every_committed_profile_resolves_an_unspecified_request_to_its_largest_mode() {
    // The ruling, run over the five real format trees this project has captured. The chooser
    // is a pure function over values — no device, no I/O — so the documents go through it
    // directly and the answer is the one `webcam-handler-cli photo` with no flags would get.
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
            .unwrap_or_else(|error| {
                panic!("{stem} offers no format with a readable size: {error}")
            });
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
    // Two honest exceptions, both named: the Chicony IR sensor has no compressed format at
    // all, so there is nothing for the ruling to prefer — and neither does `vivid`, whose 83
    // formats are every raw layout the kernel can synthesize and not one bitstream. The
    // virtual device joining this list is worth a sentence rather than a number: it means the
    // set is no longer "the one IR camera", and PF:26's reading of the corpus rests on the
    // *cameras* in it, so a reader checking that reading should skip the row that is not one.
    let profiles = corpus::load_all().expect("the corpus parses");
    let mut without_compressed = Vec::new();

    for (path, profile) in &profiles {
        let stem = path.file_stem().expect("a named file");
        let chosen = StreamRequest::default()
            .choose(&profile.invariant.formats)
            .unwrap_or_else(|error| {
                panic!("{stem} offers no format with a readable size: {error}")
            });
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
        vec!["chicony-ir", "vivid"],
        "the set of profiles with no compressed format at all has changed; PF:26's \
         reading of the corpus and N85's cost estimate both rest on it, and both are \
         statements about the *cameras* — a virtual driver joining this list changes what \
         the list means as well as how long it is"
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
