//! Taking a snapshot and putting it back (design D4).
//!
//! *Leave the camera as you found it.* Every operation that writes a control is expected
//! to be undoable, and the two halves of that are here: read every writable control's
//! value, and later write them all back in an order that survives automation.
//!
//! ## Why the order is not obvious
//!
//! Restoring `white_balance_temperature` before `white_balance_automatic` puts the manual
//! value back and then re-enables the automation that immediately overwrites it. The
//! camera ends up neither where it started nor where we left it, and nothing reports a
//! failure — the write succeeded, and the automation moved it a frame later.
//!
//! So automation goes first, then manual, and the ordering lives on
//! [`Snapshot::restore_order`] in the schema, as data on the snapshot rather than as a sort
//! in this module. This module walks that order; it never re-sorts.
//!
//! ## Two passes, and why one is not enough
//!
//! A control the device reported INACTIVE at snapshot time is owned by an automation
//! partner \[PF:3\]. Writing it in the first pass is futile — the partner overwrites it —
//! so those entries get a second pass, *after* every automation control has been restored.
//!
//! **On a device whose flag follows its automation control's value, the second pass never
//! writes**, and that is arithmetic rather than a gap: the control was INACTIVE because
//! the automation was engaged, the snapshot recorded the automation *as engaged*, so
//! restoring it engages the partner again. That is a **success** — the camera is in the
//! configuration the snapshot recorded — and it is reported
//! [`RestoreOutcome::OwnedByAutomation`] rather than as a failure. The first version of
//! this module called it unrestorable, and the very first hardware run of
//! `controls --discover-pairs` put the camera back perfectly and then announced that it
//! could not put two controls back (note N9).
//!
//! The pass *writes* only where the flag is not a function of the value we wrote — a
//! driver updating INACTIVE on its own schedule, which is exactly why PF:3 records the
//! flag diff as a measured method rather than a guarantee.
//!
//! [`UnrestorableReason::StillInactive`] is kept for the case it actually names, which is
//! the other one: a control that was *ours* when the snapshot was taken and is owned by
//! automation now. That is a change we could not undo, and telling it apart from the
//! benign case is why pass one defers on the device's present state as well as on the
//! snapshot's record of it.
//!
//! The second pass builds its writes through [`crate::pairing::plan`] and runs them
//! through [`crate::write::execute`], so "disable the partner" and "walk a plan" each
//! happen in one place in the workspace rather than two.

use std::collections::BTreeMap;

use schema::backend::Camera;
use schema::control::{ControlDesc, ControlSlug};
use schema::error::{Error, Result};
use schema::pairing::AutomationPair;
use schema::snapshot::{
    ControlRole, RestoreOutcome, RestoreReport, Snapshot, SnapshotEntry, UnrestorableReason,
};
use schema::time::Stamp;

use crate::pairing;

/// Record every writable control's current value (design D4).
///
/// `now` is an argument because the engine reads no clock.
///
/// **Only writable controls.** A read-only one cannot be changed, so recording it would
/// put an entry in the snapshot that restore could never act on — and
/// [`RestoreReport::is_complete`] would then be false for a camera nobody had touched. A
/// control with no readable value is skipped for the same reason from the other side:
/// there is nothing to record.
///
/// Volatile controls *are* recorded, and marked. Their value is the device's to choose, so
/// restore will refuse to write them — but a snapshot that omitted them would lose the
/// fact that the device has one, and a reader comparing two snapshots would see a control
/// appear and disappear.
///
/// # Errors
///
/// Whatever the camera says when asked for its controls.
pub fn take(camera: &mut dyn Camera, pairs: &[AutomationPair], now: Stamp) -> Result<Snapshot> {
    let controls = camera.controls()?;
    let entries = controls
        .iter()
        .filter(|desc| desc.is_writable())
        .filter_map(|desc| {
            Some(SnapshotEntry {
                control: desc.slug.clone(),
                id: desc.id,
                value: desc.current.clone()?,
                // The pair-aware answer, not the slug heuristic: a control called
                // `auto_something` that this device pairs with nothing is restored as
                // manual, because for restore ordering "automation" means "something
                // depends on me", and only the pair set knows that.
                role: if pairing::is_automation(&controls, pairs, &desc.slug) {
                    ControlRole::Automation
                } else {
                    ControlRole::Manual
                },
                was_inactive: desc.is_inactive(),
                was_volatile: desc.is_volatile(),
            })
        })
        .collect();

    Ok(Snapshot {
        taken_at: now,
        camera: camera.info().fingerprint.clone(),
        entries,
    })
}

/// Put a snapshot back (design D4).
///
/// The report's outcomes are in the order the writes were *attempted*:
/// [`Snapshot::restore_order`] for the first pass, then the retries, in the same relative
/// order. That is a promise the type's own documentation makes, so it is kept here rather
/// than left to the order a map happened to iterate in.
///
/// # Errors
///
/// [`Error::FingerprintMismatch`] when the snapshot came from a different camera, and
/// whatever the camera says when asked for its controls. A control that could not be put
/// back is **not** an error — it is a [`RestoreOutcome::Unrestorable`] in the report, and
/// collapsing a partial restore into a failure would throw away the part that worked.
pub fn restore(
    camera: &mut dyn Camera,
    pairs: &[AutomationPair],
    snapshot: &Snapshot,
) -> Result<RestoreReport> {
    let found = &camera.info().fingerprint;
    let differing = snapshot.camera.differing_fields(found);
    if !differing.is_empty() {
        // The *fields* rather than the two whole fingerprints: D13's variant carries the
        // disagreement, which is what a caller acts on, and a card name that differs by a
        // trailing space is far easier to see named than spotted in two structs.
        return Err(Error::FingerprintMismatch {
            fields: differing.into_iter().map(str::to_owned).collect(),
        });
    }

    let mut outcomes = Vec::with_capacity(snapshot.entries.len());
    let mut deferred: Vec<&SnapshotEntry> = Vec::new();

    // Pass one, in D4's order: automation before manual, and within each group the
    // controls that were active before the ones that were not.
    //
    // What is deferred is decided by the device's state *now*, not only by the snapshot's
    // record of it. A control an automation partner owns at this moment cannot take a
    // write whenever it became owned, so writing it here would be writing into the wind —
    // and it is the difference between the two cases (owned then, or owned only now) that
    // pass two turns into two different answers.
    let before = inactive_now(camera)?;
    for entry in snapshot.restore_order() {
        if entry.was_inactive || before.contains(entry.control.as_str()) {
            deferred.push(entry);
            continue;
        }
        outcomes.push(restore_one(camera, entry)?);
    }

    // Pass two. The device is re-read once rather than per entry: the automation controls
    // have all been written by now, so one reading answers for every deferred control, and
    // asking again per control would make the report depend on how long the loop took.
    let controls = camera.controls()?;
    let by_slug: BTreeMap<&str, &ControlDesc> =
        controls.iter().map(|d| (d.slug.as_str(), d)).collect();

    for entry in deferred {
        let Some(desc) = by_slug.get(entry.control.as_str()).copied() else {
            outcomes.push(RestoreOutcome::Unrestorable {
                control: entry.control.clone(),
                reason: UnrestorableReason::NoLongerWritable,
            });
            continue;
        };
        if desc.is_inactive() {
            let automation = partner_of(&controls, pairs, &entry.control);
            outcomes.push(if entry.was_inactive {
                // Its owner is back. The camera is in the configuration the snapshot
                // recorded, which is what D4 promises — the *value* is the automation's,
                // as it was. This is the ordinary outcome for every guarded write's
                // restore, and calling it unrestorable is what note N9 was written about.
                RestoreOutcome::OwnedByAutomation {
                    control: entry.control.clone(),
                    automation,
                }
            } else {
                // It was ours when the snapshot was taken and is not now. A real change
                // we could not undo, and naming the partner is what makes it actionable.
                RestoreOutcome::Unrestorable {
                    control: entry.control.clone(),
                    reason: UnrestorableReason::StillInactive { automation },
                }
            });
            continue;
        }
        outcomes.push(restore_one(camera, entry)?);
    }

    Ok(RestoreReport { outcomes })
}

/// The slugs the device reports INACTIVE at this instant.
///
/// # Errors
///
/// Whatever the camera says when asked for its controls.
fn inactive_now(camera: &mut dyn Camera) -> Result<std::collections::BTreeSet<String>> {
    Ok(camera
        .controls()?
        .into_iter()
        .filter(ControlDesc::is_inactive)
        .map(|desc| desc.slug.to_string())
        .collect())
}

/// Put one control back, or say why not.
///
/// # Errors
///
/// Whatever the camera says when asked for its control set. A failed *write* is not an
/// error: it becomes [`UnrestorableReason::WriteFailed`] carrying the refusal, so one
/// stubborn control cannot abandon the rest of the restore.
fn restore_one(camera: &mut dyn Camera, entry: &SnapshotEntry) -> Result<RestoreOutcome> {
    if entry.was_volatile {
        // The device chooses this value; writing it back would be pretending we restored
        // something. Reported, so `is_complete()` is false and the caller knows the camera
        // is not exactly where it was.
        return Ok(RestoreOutcome::Unrestorable {
            control: entry.control.clone(),
            reason: UnrestorableReason::Volatile,
        });
    }

    // Read before write, and it earns its ioctl: a control already holding its recorded
    // value needs no write at all, and *not writing* is the difference between a restore
    // that is invisible to the device and one that bumps every control on the camera.
    // `AlreadyCorrect` is also the honest report for it — a `Restored` with an `Applied`
    // nobody produced would be inventing a write.
    match camera.get(entry.id) {
        Ok(current) if current == entry.value => {
            return Ok(RestoreOutcome::AlreadyCorrect {
                control: entry.control.clone(),
            });
        }
        // A control that cannot be read is still worth writing: the value may well go in,
        // and the write's own read-back is where D3 reports what happened.
        Ok(_) | Err(_) => {}
    }

    match camera.set(entry.id, entry.value.clone()) {
        Ok(applied) => Ok(RestoreOutcome::Restored { applied }),
        Err(Error::ControlReadOnly { .. }) => Ok(RestoreOutcome::Unrestorable {
            control: entry.control.clone(),
            reason: UnrestorableReason::NoLongerWritable,
        }),
        Err(error) => Ok(RestoreOutcome::Unrestorable {
            control: entry.control.clone(),
            reason: UnrestorableReason::WriteFailed { error },
        }),
    }
}

/// The automation control that owns `manual` on this device, when the pair set knows one.
fn partner_of(
    controls: &[ControlDesc],
    pairs: &[AutomationPair],
    manual: &ControlSlug,
) -> Option<ControlSlug> {
    pairing::applicable(controls, pairs)
        .into_iter()
        .find(|pair| &pair.manual == manual)
        .map(|pair| pair.automation)
}

/// Write `value` to every control named, ignoring what happens — the shape a probe needs
/// when it is putting a camera back before reporting a failure.
///
/// Deliberately not public: a caller who wants a restore wants [`restore`], which reports.
#[cfg(test)]
fn force(camera: &mut dyn Camera, slug: &str, value: schema::control::ControlValue) {
    if let Ok(controls) = camera.controls()
        && let Some(desc) = controls.iter().find(|d| d.slug.as_str() == slug)
    {
        let _ = camera.set(desc.id, value);
    }
}

#[cfg(test)]
mod tests {
    use schema::control::ControlValue;
    use schema::pairing::{AutomationOff, Provenance};

    use super::*;
    use crate::double::{OnWrite, ScriptedCamera, boolean, flagged, integer};

    const READ_ONLY: u32 = 0x0004;
    const INACTIVE: u32 = 0x0010;
    const VOLATILE: u32 = 0x0080;

    fn slug(s: &str) -> ControlSlug {
        ControlSlug::parse(s).expect("literal slug")
    }

    fn white_balance_pair() -> AutomationPair {
        AutomationPair {
            manual: slug("white_balance_temperature"),
            automation: slug("white_balance_automatic"),
            off: AutomationOff::Value { value: 0 },
            provenance: Provenance::Declared,
        }
    }

    /// The seed shape: automation on, its partner INACTIVE because of it, and a control
    /// that has nothing to do with either.
    fn coupled_camera() -> ScriptedCamera {
        ScriptedCamera::new(vec![
            boolean("white_balance_automatic", 1),
            flagged(integer("white_balance_temperature", 46), INACTIVE),
            integer("brightness", 50),
        ])
        .on_write(
            "white_balance_automatic",
            OnWrite::Couple(vec!["white_balance_temperature"]),
        )
    }

    #[test]
    fn every_writable_control_is_recorded_and_the_read_only_ones_are_not() {
        let mut camera = ScriptedCamera::new(vec![
            integer("brightness", 50),
            flagged(integer("privacy", 0), READ_ONLY),
        ]);
        let snapshot = take(&mut camera, &[], Stamp::epoch()).expect("snapshots");

        assert_eq!(
            snapshot
                .entries
                .iter()
                .map(|e| e.control.as_str())
                .collect::<Vec<_>>(),
            vec!["brightness"],
            "a control restore could never write must not make a snapshot incomplete"
        );
        assert_eq!(snapshot.camera, camera.info().fingerprint);
    }

    #[test]
    fn an_automation_control_is_recorded_with_the_automation_role_and_only_when_paired() {
        let mut camera = coupled_camera();
        let snapshot =
            take(&mut camera, &[white_balance_pair()], Stamp::epoch()).expect("snapshots");
        let role = |name: &str| {
            snapshot
                .entries
                .iter()
                .find(|e| e.control.as_str() == name)
                .map(|e| e.role)
        };
        assert_eq!(
            role("white_balance_automatic"),
            Some(ControlRole::Automation)
        );
        assert_eq!(role("white_balance_temperature"), Some(ControlRole::Manual));

        // The inverse: with no pair set, the same control is manual. "Automation" for
        // restore ordering means "something depends on me", and only the pairs know that
        // — a name-shaped heuristic would answer differently and get the order wrong on a
        // device whose `auto_*` control governs nothing.
        let mut unpaired = coupled_camera();
        let plain = take(&mut unpaired, &[], Stamp::epoch()).expect("snapshots");
        assert!(
            plain.entries.iter().all(|e| e.role == ControlRole::Manual),
            "with no pairs, nothing governs anything"
        );
    }

    /// The slug each outcome is about, in report order.
    fn reported(report: &RestoreReport) -> Vec<&str> {
        report
            .outcomes
            .iter()
            .map(|o| match o {
                RestoreOutcome::Restored { applied } => applied.slug.as_str(),
                RestoreOutcome::AlreadyCorrect { control }
                | RestoreOutcome::OwnedByAutomation { control, .. }
                | RestoreOutcome::Unrestorable { control, .. } => control.as_str(),
            })
            .collect()
    }

    #[test]
    fn an_inactive_control_is_deferred_to_the_second_pass_and_reported_last() {
        // The D4 order, observed through the writes the device actually saw. The
        // automation control goes back first, and the control it owned is attempted after
        // everything else — which is the whole reason the pass exists.
        let mut camera = coupled_camera();
        let snapshot =
            take(&mut camera, &[white_balance_pair()], Stamp::epoch()).expect("snapshots");
        assert!(
            snapshot
                .entries
                .iter()
                .find(|e| e.control.as_str() == "white_balance_temperature")
                .is_some_and(|e| e.was_inactive),
            "the fact is recorded, or the second pass has nothing to select on"
        );

        force(&mut camera, "white_balance_automatic", ControlValue::Int(0));
        force(
            &mut camera,
            "white_balance_temperature",
            ControlValue::Int(90),
        );
        force(&mut camera, "brightness", ControlValue::Int(10));
        camera.writes.clear();

        let report = restore(&mut camera, &[white_balance_pair()], &snapshot).expect("restores");
        let order: Vec<&str> = camera.writes.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(
            order.first(),
            Some(&"white_balance_automatic"),
            "the automation control must go back before anything it governs: {order:?}"
        );
        // The report is in the order the writes were attempted, and the deferred entry
        // appends — whatever the pass decided to do with it.
        assert_eq!(
            reported(&report).last(),
            Some(&"white_balance_temperature"),
            "the deferred pass appends"
        );
    }

    #[test]
    fn the_second_pass_writes_when_restoring_the_automation_actually_frees_the_partner() {
        // The writing branch of pass two, on a device whose INACTIVE bit is not a function
        // of the value we wrote — see this module's header for why that is the only shape
        // that reaches it. Without this test the branch would be code nobody can turn red.
        let mut camera = ScriptedCamera::new(vec![
            boolean("white_balance_automatic", 1),
            flagged(integer("white_balance_temperature", 46), INACTIVE),
        ])
        .on_write(
            "white_balance_automatic",
            OnWrite::ClearsInactive(vec!["white_balance_temperature"]),
        );
        let snapshot =
            take(&mut camera, &[white_balance_pair()], Stamp::epoch()).expect("snapshots");

        // Both controls move, so both need writing back. Perturbing only the manual one
        // would leave the automation control already correct — and `restore_one` does not
        // write a control that is already where it belongs, so the flag would never clear
        // and this test would be asserting against a pass that never ran.
        force(&mut camera, "white_balance_automatic", ControlValue::Int(0));
        force(
            &mut camera,
            "white_balance_temperature",
            ControlValue::Int(90),
        );
        camera.writes.clear();

        let report = restore(&mut camera, &[white_balance_pair()], &snapshot).expect("restores");
        assert_eq!(
            camera
                .writes
                .iter()
                .map(|(s, v)| (s.as_str(), v.clone()))
                .collect::<Vec<_>>(),
            vec![
                ("white_balance_automatic", ControlValue::Int(1)),
                ("white_balance_temperature", ControlValue::Int(46)),
            ],
            "the freed control is written, and only after its partner"
        );
        assert!(report.is_complete(), "{report:?}");
        assert_eq!(
            camera.value_of("white_balance_temperature"),
            Some(ControlValue::Int(46))
        );
    }

    #[test]
    fn a_control_whose_owner_is_back_is_a_success_that_names_the_owner() {
        // The snapshot was taken with automation *on*, so restoring the automation value
        // re-engages it and the partner is INACTIVE again. The camera is exactly where it
        // was, and the report must say so: this is the ordinary outcome of every guarded
        // write's restore, and calling it a failure is how a field stops being read.
        let mut camera = coupled_camera();
        let snapshot =
            take(&mut camera, &[white_balance_pair()], Stamp::epoch()).expect("snapshots");
        force(&mut camera, "white_balance_automatic", ControlValue::Int(0));
        force(
            &mut camera,
            "white_balance_temperature",
            ControlValue::Int(90),
        );

        let report = restore(&mut camera, &[white_balance_pair()], &snapshot).expect("restores");
        let reported = report
            .outcomes
            .iter()
            .find(|o| {
                matches!(o, RestoreOutcome::OwnedByAutomation { control, .. }
                    if control.as_str() == "white_balance_temperature")
            })
            .expect("the deferred control is reported as owned, not as a failure");
        assert_eq!(
            reported,
            &RestoreOutcome::OwnedByAutomation {
                control: slug("white_balance_temperature"),
                automation: Some(slug("white_balance_automatic")),
            }
        );
        // And the whole restore is *complete*: the camera is in the configuration the
        // snapshot recorded. Reporting otherwise made the probe's first hardware run
        // announce a failure it had not had (note N9).
        assert!(report.is_complete(), "{report:?}");
        assert!(report.unrestored().is_empty());
    }

    #[test]
    fn a_control_already_holding_its_recorded_value_is_not_written_again() {
        // Not an optimisation: `AlreadyCorrect` is the honest report, and a `Restored`
        // carrying an `Applied` nobody produced would be inventing a write.
        let mut camera = ScriptedCamera::new(vec![integer("brightness", 50)]);
        let snapshot = take(&mut camera, &[], Stamp::epoch()).expect("snapshots");
        camera.writes.clear();

        let report = restore(&mut camera, &[], &snapshot).expect("restores");
        assert!(camera.writes.is_empty(), "nothing needed writing");
        assert_eq!(
            report.outcomes,
            vec![RestoreOutcome::AlreadyCorrect {
                control: slug("brightness")
            }]
        );
        assert!(report.is_complete());
    }

    #[test]
    fn a_volatile_control_is_recorded_and_then_refused_rather_than_written() {
        let mut camera = ScriptedCamera::new(vec![flagged(integer("exposure", 30), VOLATILE)]);
        let snapshot = take(&mut camera, &[], Stamp::epoch()).expect("snapshots");
        assert!(snapshot.entries[0].was_volatile, "the fact is recorded");

        force(&mut camera, "exposure", ControlValue::Int(70));
        camera.writes.clear();
        let report = restore(&mut camera, &[], &snapshot).expect("restores");
        assert!(
            camera.writes.is_empty(),
            "a volatile value is not ours to set"
        );
        assert_eq!(
            report.outcomes,
            vec![RestoreOutcome::Unrestorable {
                control: slug("exposure"),
                reason: UnrestorableReason::Volatile,
            }]
        );
        assert!(!report.is_complete());
    }

    #[test]
    fn a_control_that_stopped_being_writable_is_unrestorable_rather_than_silently_skipped() {
        let mut camera = ScriptedCamera::new(vec![integer("brightness", 50)]);
        let snapshot = take(&mut camera, &[], Stamp::epoch()).expect("snapshots");
        force(&mut camera, "brightness", ControlValue::Int(10));
        camera.make_read_only("brightness");

        let report = restore(&mut camera, &[], &snapshot).expect("restores");
        assert_eq!(
            report.outcomes,
            vec![RestoreOutcome::Unrestorable {
                control: slug("brightness"),
                reason: UnrestorableReason::NoLongerWritable,
            }]
        );
    }

    #[test]
    fn a_write_that_fails_during_restore_carries_its_error_and_the_rest_still_run() {
        // One stubborn control must not abandon the restore: the whole point of D4 is to
        // put back as much as can be put back, and to say what could not be.
        let mut camera =
            ScriptedCamera::new(vec![integer("brightness", 50), integer("contrast", 40)]).on_write(
                "brightness",
                OnWrite::Fail(Error::DeviceIo {
                    operation: "VIDIOC_S_EXT_CTRLS".to_owned(),
                    errno: Some(5),
                    message: "Input/output error".to_owned(),
                }),
            );
        let snapshot = take(&mut camera, &[], Stamp::epoch()).expect("snapshots");
        force(&mut camera, "contrast", ControlValue::Int(99));
        // `brightness` refuses every write including this one, so its value is unchanged
        // and the restore must still *attempt* it — which is what produces the failure.
        let mut moved =
            ScriptedCamera::new(vec![integer("brightness", 10), integer("contrast", 99)]).on_write(
                "brightness",
                OnWrite::Fail(Error::DeviceIo {
                    operation: "VIDIOC_S_EXT_CTRLS".to_owned(),
                    errno: Some(5),
                    message: "Input/output error".to_owned(),
                }),
            );

        let report = restore(&mut moved, &[], &snapshot).expect("a failed write is not an error");
        assert_eq!(report.outcomes.len(), 2);
        assert!(report.outcomes.iter().any(|o| matches!(
            o,
            RestoreOutcome::Unrestorable {
                reason: UnrestorableReason::WriteFailed { .. },
                ..
            }
        )));
        assert!(
            report
                .outcomes
                .iter()
                .any(|o| matches!(o, RestoreOutcome::Restored { .. })),
            "the control that could be restored was"
        );
    }

    #[test]
    fn a_snapshot_from_a_different_camera_is_refused_with_both_fingerprints_named() {
        let mut origin =
            ScriptedCamera::new(vec![integer("brightness", 50)]).identified("cam:a", "3-4:1.0");
        let snapshot = take(&mut origin, &[], Stamp::epoch()).expect("snapshots");

        let mut other =
            ScriptedCamera::new(vec![integer("brightness", 50)]).identified("cam:b", "3-1:1.0");
        let error = restore(&mut other, &[], &snapshot)
            .expect_err("a snapshot is not portable between cameras");
        assert_eq!(error.kind(), schema::ErrorKind::FingerprintMismatch);
        assert!(other.writes.is_empty(), "the refusal wrote nothing");
    }

    #[test]
    fn snapshot_then_perturb_then_restore_leaves_every_control_where_it_started() {
        // D4's inverse property, end to end, on the fixture with automation in it.
        let mut camera = coupled_camera();
        let before: Vec<Option<ControlValue>> = ["white_balance_automatic", "brightness"]
            .iter()
            .map(|s| camera.value_of(s))
            .collect();
        let snapshot =
            take(&mut camera, &[white_balance_pair()], Stamp::epoch()).expect("snapshots");

        force(&mut camera, "white_balance_automatic", ControlValue::Int(0));
        force(&mut camera, "brightness", ControlValue::Int(3));
        assert_ne!(camera.value_of("brightness"), before[1]);

        restore(&mut camera, &[white_balance_pair()], &snapshot).expect("restores");
        let after: Vec<Option<ControlValue>> = ["white_balance_automatic", "brightness"]
            .iter()
            .map(|s| camera.value_of(s))
            .collect();
        assert_eq!(after, before);
        assert!(
            camera.is_inactive("white_balance_temperature"),
            "and the coupling is back where it was too"
        );
    }
}
