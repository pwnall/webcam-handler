//! Finding out which controls this device pairs, by asking it (design D3 layer 2, PF:3).
//!
//! The declared table in `schema::pairing` is a *nomination*: it says what UVC devices
//! generally do. This is the layer that turns a nomination into evidence — toggle an
//! automation-shaped control, read the control set again, and whatever control's INACTIVE
//! bit moved is one this device really does govern. What comes out carries
//! `Provenance::Measured`, and measured beats declared (E1).
//!
//! ## The probe changes the camera, so it puts it back
//!
//! Every toggle is a write, and a probe that left a camera with its white balance on
//! manual would be a probe nobody could run twice. So it snapshots first, restores last,
//! and reports what the restore achieved — because a restore nobody checked is a promise
//! (docs/3 Part C). The restore runs even when the probe fails part way, for the same
//! reason [`crate::capture`]'s stream guard does.
//!
//! ## What it declines to touch, it names
//!
//! §5: motors wear, and nothing here moves one. A candidate it skips is *listed* with the
//! reason, because a probe silent about what it passed over reads as a probe that found
//! nothing there — and "this camera has no pairs" and "this probe did not look at half of
//! them" are answers a caller would act on very differently.

use schema::backend::Camera;
use schema::control::{ControlDesc, ControlSlug, ControlValue};
use schema::error::Result;
use schema::pairing::{AutomationPair, looks_like_automation};
use schema::snapshot::RestoreReport;
use schema::time::Stamp;

use crate::{pairing, snapshot};

/// Control-name fragments that mean a motor moves (design §5).
///
/// The same list the conformance battery uses, and it is here rather than imported because
/// `webcam-handler-testkit` is a *dev*-dependency: the product may not link it. Two copies
/// of one rule is the shape §2.10 calls a defect, so this one is pinned by a test that
/// compares the two lists — the duplication is allowed to exist and not allowed to drift.
const MOTORIZED_FRAGMENTS: &[&str] = &["pan", "tilt", "zoom", "focus", "roll"];

/// What a probe found, declined, and put back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovery {
    /// The pairs this device demonstrated, sorted, every one `Provenance::Measured`.
    pub pairs: Vec<AutomationPair>,
    /// The automation-shaped controls the probe did not toggle, and why.
    pub skipped: Vec<(ControlSlug, String)>,
    /// What putting the camera back achieved. Reported rather than assumed.
    pub restored: RestoreReport,
}

impl Discovery {
    /// Whether the probe left the camera where it found it.
    #[must_use]
    pub fn left_the_camera_alone(&self) -> bool {
        self.restored.is_complete()
    }
}

/// Toggle each automation-shaped control and record which manual controls it frees
/// \[PF:3\].
///
/// `now` is an argument because the engine reads no clock.
///
/// The candidate set is [`looks_like_automation`] — the slug heuristic, and this is the
/// *only* question it answers. Being wrong here costs one extra toggle; being narrow here
/// costs a pair nobody finds, so the predicate leans inclusive and the evidence, not the
/// name, decides what ends up in the result.
///
/// # Errors
///
/// Whatever the camera says when asked for its controls, or when the pre-probe snapshot
/// could not be taken. A probe that cannot record where the camera started must not start:
/// it would be a set of writes with no way back.
pub fn pairs(camera: &mut dyn Camera, now: Stamp) -> Result<Discovery> {
    // Before anything is written. A failure here is a refusal to begin, which is the
    // correct direction: the alternative is a camera perturbed with no record of where it
    // was.
    let before_all = snapshot::take(camera, &[], now)?;
    let controls = camera.controls()?;

    let mut found: Vec<AutomationPair> = Vec::new();
    let mut skipped: Vec<(ControlSlug, String)> = Vec::new();

    for candidate in controls.iter().filter(|d| looks_like_automation(d)) {
        match probe_one(camera, candidate, &controls) {
            Ok(pairs) => found.extend(pairs),
            Err(reason) => skipped.push((candidate.slug.clone(), reason)),
        }
    }

    // Unconditionally, and after everything — including after a candidate that failed
    // mid-toggle. AGENTS.md rule 8, and the reason the report carries the outcome.
    let restored = snapshot::restore(camera, &found, &before_all)?;

    found.sort_by(|a, b| {
        (a.manual.as_str(), a.automation.as_str()).cmp(&(b.manual.as_str(), b.automation.as_str()))
    });
    found.dedup();
    Ok(Discovery {
        pairs: found,
        skipped,
        restored,
    })
}

/// Toggle one candidate and read what moved.
///
/// `Err` is a *skip* with its reason, not a failure of the probe: a candidate that cannot
/// be toggled is one this device will not let us learn about, and the run carries on.
fn probe_one(
    camera: &mut dyn Camera,
    candidate: &ControlDesc,
    controls: &[ControlDesc],
) -> std::result::Result<Vec<AutomationPair>, String> {
    if is_motorized(&candidate.slug) {
        return Err("§5: a motor moves when this control is written".to_owned());
    }
    let Some(current) = candidate.current.as_ref().and_then(ControlValue::as_int) else {
        return Err("no readable current value, so the toggle could not be undone".to_owned());
    };
    let Some(other) = opposite(candidate, current) else {
        return Err(
            "no second value to toggle to; a control with one position governs nothing \
             observable"
                .to_owned(),
        );
    };

    // The toggle. Its *own* undo happens here rather than being left to the whole-probe
    // restore: the next candidate must be read against a device in its resting state, or
    // one control's coupling would be attributed to the next control's toggle.
    camera
        .set(candidate.id, ControlValue::Int(other))
        .map_err(|error| format!("the toggle was refused: {error}"))?;
    let after = camera
        .controls()
        .map_err(|error| format!("the control set could not be re-read: {error}"))?;
    let moved = pairing::inactive_diff(controls, &after);
    let undo = camera.set(candidate.id, ControlValue::Int(current));

    if let Err(error) = undo {
        return Err(format!(
            "toggled to {other} and could not be put back: {error}"
        ));
    }

    // Which of the two values *frees* the partner is what the pair needs to record: the
    // "off" recipe is the position at which the manual control is writable. If toggling
    // to `other` freed it, `other` is off; otherwise the original was.
    let freed_by_other = after
        .iter()
        .any(|desc| moved.contains(&desc.slug) && !desc.is_inactive());
    let off = if freed_by_other { other } else { current };

    Ok(moved
        .iter()
        .filter_map(|manual| pairing::measured_pair(candidate, off, manual))
        .collect())
}

/// The other value to try, for a control that has one.
///
/// Booleans flip. Menus take the first index that is not the current one — every index, so
/// a sparse menu's holes are skipped by construction \[PF:2\] rather than by arithmetic
/// that assumes `current + 1` exists.
fn opposite(desc: &ControlDesc, current: i64) -> Option<i64> {
    if desc.control_type.is_menu() {
        desc.menu
            .keys()
            .map(|index| i64::from(*index))
            .find(|index| *index != current)
    } else {
        Some(i64::from(current == 0))
    }
}

/// Whether writing this control turns a motor (design §5).
fn is_motorized(slug: &ControlSlug) -> bool {
    let slug = slug.as_str();
    MOTORIZED_FRAGMENTS.iter().any(|f| slug.contains(f))
}

#[cfg(test)]
mod tests {
    use fake::FakeBackend;
    use schema::backend::CameraBackend;
    use schema::pairing::{AutomationOff, Provenance};

    use super::*;
    use crate::double::{OnWrite, ScriptedCamera, boolean, integer};

    fn slug(s: &str) -> ControlSlug {
        ControlSlug::parse(s).expect("literal slug")
    }

    #[test]
    fn the_motor_rule_is_the_same_list_the_conformance_battery_uses() {
        // The duplication this test exists to freeze. `webcam-handler-testkit` is a
        // dev-dependency, so the product cannot import its copy — but §5's rule having
        // two spellings that drift is exactly the defect §2.10 names, and a shared
        // fixture that fails on divergence is the cheapest thing that prevents it.
        for fragment in MOTORIZED_FRAGMENTS {
            let name = format!("{fragment}_absolute");
            assert!(
                is_motorized(&slug(&name)),
                "{name} is motorized here but the list disagrees"
            );
            assert!(
                testkit::battery::is_motorized(&slug(&name)),
                "{name} is motorized here and not in the battery's list — §5 has drifted"
            );
        }
        // And the inverse, so the two lists agreeing is not the trivial "everything is a
        // motor".
        for calm in ["brightness", "white_balance_automatic", "contrast"] {
            assert!(!is_motorized(&slug(calm)));
            assert!(!testkit::battery::is_motorized(&slug(calm)));
        }
    }

    #[test]
    fn toggling_an_automation_control_finds_the_manual_control_it_freezes() {
        let mut camera = ScriptedCamera::new(vec![
            boolean("white_balance_automatic", 0),
            integer("white_balance_temperature", 46),
            integer("brightness", 50),
        ])
        .on_write(
            "white_balance_automatic",
            OnWrite::Couple(vec!["white_balance_temperature"]),
        );

        let found = pairs(&mut camera, Stamp::epoch()).expect("probes");
        assert_eq!(
            found.pairs,
            vec![AutomationPair {
                manual: slug("white_balance_temperature"),
                automation: slug("white_balance_automatic"),
                // Toggling to 1 engaged the automation, so 0 — where we found it — is off.
                off: AutomationOff::Value { value: 0 },
                provenance: Provenance::Measured,
            }]
        );
        assert!(
            found.left_the_camera_alone(),
            "the probe must put the camera back: {:?}",
            found.restored
        );
        assert_eq!(
            camera.value_of("white_balance_automatic"),
            Some(ControlValue::Int(0))
        );
        assert!(!camera.is_inactive("white_balance_temperature"));
    }

    #[test]
    fn a_control_that_freezes_nothing_produces_no_pair_rather_than_a_guess() {
        // The inverse, and the one that matters: a device with an automation-*shaped*
        // control that governs nothing must yield an empty result, not a plausible pair
        // with `Measured` on it. A hallucinated pair goes into the corpus and outranks the
        // declared table forever (E1).
        let mut camera = ScriptedCamera::new(vec![
            boolean("white_balance_automatic", 0),
            integer("white_balance_temperature", 46),
        ]);
        let found = pairs(&mut camera, Stamp::epoch()).expect("probes");
        assert!(found.pairs.is_empty(), "{:?}", found.pairs);
        assert!(found.skipped.is_empty(), "nothing was declined either");
        assert!(found.left_the_camera_alone());
    }

    #[test]
    fn a_motorized_candidate_is_declined_by_name_and_the_reason_is_in_the_report() {
        // §5, and the "say what you skipped" half: `focus_automatic_continuous` matches
        // the automation heuristic and moves a lens, so the probe passes over it and the
        // caller can see that it did.
        let mut camera = ScriptedCamera::new(vec![
            boolean("focus_automatic_continuous", 1),
            integer("focus_absolute", 30),
        ]);
        let found = pairs(&mut camera, Stamp::epoch()).expect("probes");
        assert!(found.pairs.is_empty());
        assert_eq!(found.skipped.len(), 1);
        assert_eq!(found.skipped[0].0, slug("focus_automatic_continuous"));
        assert!(
            found.skipped[0].1.contains("§5"),
            "{:?}",
            found.skipped[0].1
        );
        assert!(
            camera.writes.is_empty(),
            "a declined candidate is not written"
        );
    }

    #[test]
    fn a_candidate_the_device_refuses_is_named_in_the_skipped_list_and_the_run_continues() {
        let mut camera = ScriptedCamera::new(vec![
            boolean("white_balance_automatic", 0),
            boolean("auto_exposure_bias", 0),
            integer("white_balance_temperature", 46),
            integer("exposure_time_absolute", 156),
        ])
        .on_write(
            "white_balance_automatic",
            OnWrite::Fail(schema::Error::DeviceIo {
                operation: "VIDIOC_S_EXT_CTRLS".to_owned(),
                errno: Some(22),
                message: "Invalid argument".to_owned(),
            }),
        )
        .on_write(
            "auto_exposure_bias",
            OnWrite::Couple(vec!["exposure_time_absolute"]),
        );

        let found = pairs(&mut camera, Stamp::epoch()).expect("probes");
        assert_eq!(found.skipped.len(), 1);
        assert_eq!(found.skipped[0].0, slug("white_balance_automatic"));
        assert_eq!(
            found
                .pairs
                .iter()
                .map(|p| p.manual.as_str())
                .collect::<Vec<_>>(),
            vec!["exposure_time_absolute"],
            "one candidate refusing must not end the probe"
        );
    }

    #[test]
    fn a_discovered_pair_outranks_the_declared_table_it_collides_with() {
        // E1 end to end: the declared recipe for this pair is a plain value, the probe
        // measured a different one, and the merge keeps the measurement.
        let measured = AutomationPair {
            manual: slug("white_balance_temperature"),
            automation: slug("white_balance_automatic"),
            off: AutomationOff::Value { value: 1 },
            provenance: Provenance::Measured,
        };
        let merged = pairing::merge(schema::pairing::declared_pairs(), vec![measured.clone()]);
        assert!(merged.contains(&measured));
        assert!(
            !merged.iter().any(|p| p.manual == measured.manual
                && p.automation == measured.automation
                && p.provenance == Provenance::Declared),
            "the declared row for the same two controls must have been replaced"
        );
    }

    #[test]
    fn the_probe_finds_the_pairs_the_replayed_profile_declares_it_has() {
        // Against the fake rather than the scripted double, because the subject here is
        // whether the probe agrees with a device that *has* real coupling — and the
        // synthetic profile's `measured_pairs` block is an independent statement of the
        // answer, written when the fixture was authored rather than by this test.
        let profile = testkit::fixtures::synthetic_basic();
        let expected: Vec<ControlSlug> = profile
            .invariant
            .measured_pairs
            .iter()
            .map(|p| p.manual.clone())
            .filter(|manual| !is_motorized(manual))
            .collect();
        assert!(!expected.is_empty(), "the fixture measures pairs");

        let backend = FakeBackend::from_profile(profile).expect("replays");
        let id = backend
            .enumerate()
            .expect("enumerate")
            .into_iter()
            .next()
            .expect("one camera")
            .id;
        let mut camera = backend.open(&id).expect("opens");

        let found = pairs(camera.as_mut(), Stamp::epoch()).expect("probes");
        for manual in &expected {
            assert!(
                found.pairs.iter().any(|p| &p.manual == manual),
                "the probe missed {manual}, which the profile says this device couples; \
                 found {:?}",
                found.pairs
            );
        }
        assert!(
            found
                .pairs
                .iter()
                .all(|p| p.provenance == Provenance::Measured),
            "everything a probe returns is measured, by definition"
        );
    }

    #[test]
    fn a_profile_with_the_coupling_removed_yields_nothing_from_the_same_probe() {
        // The other direction of the test above, over the same code path: without the
        // coupling the probe must find nothing, or "it found the pairs" would be a
        // sentence about a function that always finds pairs.
        let mut profile = testkit::fixtures::synthetic_basic();
        profile.invariant.measured_pairs.clear();
        let backend = FakeBackend::from_profile(profile).expect("replays");
        let id = backend
            .enumerate()
            .expect("enumerate")
            .into_iter()
            .next()
            .expect("one camera")
            .id;
        let mut camera = backend.open(&id).expect("opens");

        let found = pairs(camera.as_mut(), Stamp::epoch()).expect("probes");
        assert!(
            found.pairs.is_empty(),
            "a device that couples nothing must yield no pairs: {:?}",
            found.pairs
        );
    }
}
