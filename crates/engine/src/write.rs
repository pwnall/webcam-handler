//! Executing a write plan (design D3, §2.4).
//!
//! [`crate::pairing`] decides *what* to write and in what order; this module walks that
//! decision, one [`Camera::set`] per entry. The split is the same one the whole engine is
//! built on — a pure core that can be tested exhaustively over values, and a shell thin
//! enough that reading it is most of verifying it.
//!
//! ## The engine trusts the read-back and never repeats it
//!
//! `Applied` arrives from the backend already carrying `{requested, applied}` and its
//! warnings, because only the thing holding the device can say what the device took
//! (§2.10). Nothing here re-reads, re-clamps, or re-derives a warning: a second opinion
//! about what happened would be a second opinion the caller has no way to reconcile.
//!
//! ## A plan that stops part way says where
//!
//! The failure that matters is the guarded one. A plan is `[disable auto_exposure, set
//! exposure_time_absolute]`, and if the second write fails the camera is left with its
//! automation off — a state the caller did not ask for and cannot undo without knowing it
//! happened. So [`PartialWrite`] carries the writes that *did* land, and the caller can
//! put them back (D4). Returning a bare error would make that impossible, which is why
//! this module's failure type is not [`schema::Error`].

use std::fmt;

use schema::backend::Camera;
use schema::control::{ControlSlug, ControlValue, ControlWrite};
use schema::error::{Error, Result};
use schema::pairing::AutomationPair;
use schema::report::WriteReport;

use crate::pairing::{self, WritePlan};

/// A plan that stopped at one of its writes.
///
/// Not a [`schema::Error`], and that is the point: an error says what went wrong, and a
/// caller unwinding a guarded write also needs to know *what already happened*. The
/// underlying refusal is still here, and [`From`] extracts it for callers that only want
/// that.
///
/// Returned boxed. It carries a whole [`WriteReport`] on top of the error, which makes the
/// `Err` arm several times the size of the `Ok` one — and every successful write would pay
/// for it in stack. The failure is the rare path; it can afford the allocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialWrite {
    /// The writes that landed before the failure, in order.
    pub done: WriteReport,
    /// The control the plan could not write.
    pub failed: ControlSlug,
    /// Where in the plan it sat, so a message can say "the second of three".
    pub index: usize,
    /// Why.
    pub error: Error,
}

impl fmt::Display for PartialWrite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "write {} of the plan failed at {}: {} ({} earlier write(s) stand)",
            self.index.saturating_add(1),
            self.failed,
            self.error,
            self.done.writes.len()
        )
    }
}

impl From<PartialWrite> for Error {
    fn from(partial: PartialWrite) -> Error {
        partial.error
    }
}

impl From<Box<PartialWrite>> for Error {
    fn from(partial: Box<PartialWrite>) -> Error {
        partial.error
    }
}

/// Walk `plan`, writing each entry to `camera`.
///
/// # Errors
///
/// [`PartialWrite`] at the first write the device refused, carrying everything that
/// landed before it.
pub fn execute(
    camera: &mut dyn Camera,
    plan: &WritePlan,
) -> std::result::Result<WriteReport, Box<PartialWrite>> {
    let mut writes = Vec::with_capacity(plan.writes.len());
    for (index, planned) in plan.writes.iter().enumerate() {
        match camera.set(planned.id, planned.value.clone()) {
            Ok(applied) => writes.push(applied),
            Err(error) => {
                return Err(Box::new(PartialWrite {
                    done: report(camera, writes, plan),
                    failed: planned.control.clone(),
                    index,
                    error,
                }));
            }
        }
    }
    Ok(report(camera, writes, plan))
}

/// Set `targets`, switching automation off first unless the caller said not to (D3).
///
/// `guarded` chooses between [`pairing::plan`] and [`pairing::plan_unguarded`], and
/// **nothing else differs**: both resolve slugs the same way, refuse an unknown control
/// the same way, and refuse a read-only one the same way. `--guarded` and `--no-guard`
/// disagreeing about anything but guarding would be two verbs wearing one name.
///
/// # Errors
///
/// The planner's refusals — [`Error::ControlUnknown`], [`Error::ControlReadOnly`],
/// [`Error::ControlInactive`], [`Error::IllegalTransition`] — or the device's, from the
/// first write that failed. The partial-write detail is flattened here: a caller that
/// needs it calls [`execute`] over a plan it made itself, which is what the calibration
/// sweep does at P3.
pub fn set(
    camera: &mut dyn Camera,
    pairs: &[AutomationPair],
    targets: &[(ControlSlug, ControlValue)],
    guarded: bool,
) -> Result<WriteReport> {
    let controls = camera.controls()?;
    let plan = if guarded {
        pairing::plan(&controls, pairs, targets)?
    } else {
        pairing::plan_unguarded(&controls, targets)?
    };
    execute(camera, &plan).map_err(Error::from)
}

/// Set the values a caller asked for, against the pair set **this device is in now**.
///
/// [`set`] takes the pair set as a parameter, because the calibration sweep has one it
/// computed for its own reasons. Every *other* caller has the same three lines in front of it
/// — read the controls, ask [`pairing::in_effect`] which relationships this device actually
/// exhibits, and turn the wire's [`ControlWrite`]s into the planner's targets — and that
/// composition is a rule rather than plumbing: which pair set a write plans against decides
/// whether an automation control is switched off first, and two copies of it are two opinions
/// about what a camera's automation looks like. It had two authors as `webcam-handler-cli set`
/// and `wch_set` landed, which is what this exists to stop (AGENTS "One home per law").
///
/// `Vec::new()` for the measured pairs is part of the rule and not an omission: measuring
/// pairs writes to the camera, so a `set` that measured would be performing D3's probe on
/// its way to a write nobody asked to be preceded by one. `wch_discover_pairs` is the verb
/// that measures (note N30), and what it produces is carried in the session document a
/// calibration plans against.
///
/// The [`ControlWrite`] conversion is here rather than at each root for note **N35**'s
/// reason: the wire spelling stops before [`crate::pairing`], and where it stops is a fact
/// with one home.
///
/// # Errors
///
/// As [`set`], plus whatever the camera says when asked for its controls.
pub fn set_requested(
    camera: &mut dyn Camera,
    writes: &[ControlWrite],
    guarded: bool,
) -> Result<WriteReport> {
    let pairs = pairing::in_effect(&camera.controls()?, Vec::new());
    let targets: Vec<(ControlSlug, ControlValue)> =
        writes.iter().map(ControlWrite::target).collect();
    set(camera, &pairs, &targets, guarded)
}

/// Assemble the report, taking the disabled-automation list from the *plan*.
///
/// From the plan rather than from the writes, and the difference shows up exactly when it
/// matters: a switch-off that was itself adjusted still switched something off, and a
/// caller restoring the camera afterwards needs it in the list.
fn report(camera: &dyn Camera, writes: Vec<schema::Applied>, plan: &WritePlan) -> WriteReport {
    WriteReport {
        camera: camera.info().id.clone(),
        disabled_automation: plan.disabled_automation(),
        writes,
    }
}

#[cfg(test)]
mod tests {
    use schema::ErrorKind;
    use schema::control::ControlValue;
    use schema::pairing::{AutomationOff, Provenance};

    use super::*;
    use crate::double::{OnWrite, ScriptedCamera, boolean, integer};

    fn slug(s: &str) -> ControlSlug {
        ControlSlug::parse(s).expect("literal slug")
    }

    /// The seed shape: a manual control an automation control owns.
    fn white_balance_pair() -> AutomationPair {
        AutomationPair {
            manual: slug("white_balance_temperature"),
            automation: slug("white_balance_automatic"),
            off: AutomationOff::Value { value: 0 },
            provenance: Provenance::Declared,
        }
    }

    fn camera() -> ScriptedCamera {
        ScriptedCamera::new(vec![
            boolean("white_balance_automatic", 1),
            integer("white_balance_temperature", 46),
            integer("brightness", 50),
        ])
    }

    #[test]
    fn a_guarded_set_switches_the_automation_off_before_the_manual_write() {
        let mut camera = camera();
        let report = set(
            &mut camera,
            &[white_balance_pair()],
            &[(slug("white_balance_temperature"), ControlValue::Int(32))],
            true,
        )
        .expect("a plannable write");

        assert_eq!(
            camera
                .writes
                .iter()
                .map(|(s, _)| s.as_str())
                .collect::<Vec<_>>(),
            vec!["white_balance_automatic", "white_balance_temperature"],
            "the order is the whole promise of --guarded"
        );
        assert_eq!(report.writes.len(), 2);
        assert_eq!(
            report.disabled_automation,
            vec![slug("white_balance_automatic")]
        );
        assert!(report.is_exact());
    }

    #[test]
    fn an_unguarded_set_writes_what_it_was_asked_and_disables_nothing() {
        // The inverse of the test above over the same fixture, so the difference between
        // the two paths is visible as a difference in output rather than argued for.
        let mut camera = camera();
        let report = set(
            &mut camera,
            &[white_balance_pair()],
            &[(slug("white_balance_temperature"), ControlValue::Int(32))],
            false,
        )
        .expect("an unguarded write is always plannable");

        assert_eq!(
            camera
                .writes
                .iter()
                .map(|(s, _)| s.as_str())
                .collect::<Vec<_>>(),
            vec!["white_balance_temperature"]
        );
        assert!(report.disabled_automation.is_empty());
    }

    #[test]
    fn the_guarded_and_unguarded_paths_refuse_an_unknown_control_the_same_way() {
        // The one-home claim, made mechanical: slug resolution and its did-you-mean list
        // live in the planner, so both paths must produce the identical refusal.
        let expected = Error::ControlUnknown {
            requested: "brightnes".to_owned(),
            did_you_mean: vec![slug("brightness")],
        };
        for guarded in [true, false] {
            let mut camera = camera();
            let error = set(
                &mut camera,
                &[white_balance_pair()],
                &[(slug("brightnes"), ControlValue::Int(1))],
                guarded,
            )
            .expect_err("no such control");
            assert_eq!(error, expected, "guarded={guarded}");
            assert!(camera.writes.is_empty(), "a refusal wrote nothing");
        }
    }

    #[test]
    fn the_guarded_and_unguarded_paths_refuse_a_read_only_control_the_same_way() {
        for guarded in [true, false] {
            let mut camera =
                ScriptedCamera::new(vec![crate::double::flagged(integer("privacy", 0), 0x0004)]);
            let error = set(
                &mut camera,
                &[],
                &[(slug("privacy"), ControlValue::Int(1))],
                guarded,
            )
            .expect_err("read-only");
            assert_eq!(
                error,
                Error::ControlReadOnly {
                    control: slug("privacy")
                },
                "guarded={guarded}"
            );
        }
    }

    #[test]
    fn a_clamped_write_reaches_the_caller_as_a_warning_and_not_as_an_error() {
        // PF:6 travelling up through the engine unchanged. An engine that turned the
        // adjustment into an error would convert "the device took something else" into
        // "the device refused", which is the confusion E3 and D13 exist to prevent.
        let mut camera = camera().on_write("brightness", OnWrite::Apply(100));
        let report = set(
            &mut camera,
            &[],
            &[(slug("brightness"), ControlValue::Int(5_000))],
            true,
        )
        .expect("a clamp is a success");

        assert!(!report.is_exact());
        let inexact = report.inexact();
        assert_eq!(inexact.len(), 1);
        assert_eq!(inexact[0].requested, ControlValue::Int(5_000));
        assert_eq!(inexact[0].applied, ControlValue::Int(100));
        assert!(
            !inexact[0].warnings.is_empty(),
            "an adjustment with no warning is a silent one"
        );
    }

    #[test]
    fn a_plan_that_fails_at_its_second_write_names_it_and_hands_back_the_first() {
        // The guarded failure that matters: the automation is off and the manual write
        // did not land, so the camera is somewhere the caller did not ask for. Without
        // the partial report they cannot put it back.
        let mut camera = camera().on_write(
            "white_balance_temperature",
            OnWrite::Fail(Error::DeviceGone {
                path: "/dev/video0".into(),
            }),
        );
        let controls = camera.controls().expect("controls");
        let plan = pairing::plan(
            &controls,
            &[white_balance_pair()],
            &[(slug("white_balance_temperature"), ControlValue::Int(32))],
        )
        .expect("plannable");

        let partial = execute(&mut camera, &plan).expect_err("the second write fails");
        assert_eq!(partial.index, 1);
        assert_eq!(partial.failed, slug("white_balance_temperature"));
        assert_eq!(partial.error.kind(), ErrorKind::DeviceGone);
        assert_eq!(
            partial
                .done
                .writes
                .iter()
                .map(|a| a.slug.as_str())
                .collect::<Vec<_>>(),
            vec!["white_balance_automatic"],
            "the switch-off landed and the caller has to know"
        );
        assert_eq!(
            partial.done.disabled_automation,
            vec![slug("white_balance_automatic")]
        );
        assert!(partial.to_string().contains("white_balance_temperature"));

        // And the plan stopped: nothing after the failing write was attempted.
        assert_eq!(camera.writes.len(), 1);
    }

    #[test]
    fn a_partial_write_still_carries_the_refusal_for_a_caller_that_only_wants_that() {
        let partial = PartialWrite {
            done: WriteReport {
                camera: schema::CameraId::parse("cam:test").expect("literal id"),
                writes: Vec::new(),
                disabled_automation: Vec::new(),
            },
            failed: slug("brightness"),
            index: 0,
            error: Error::busy("/dev/video0".into(), Vec::new()),
        };
        assert_eq!(Error::from(partial).kind(), ErrorKind::Busy);
    }

    #[test]
    fn an_empty_plan_writes_nothing_and_reports_an_empty_success() {
        let mut camera = camera();
        let report = execute(&mut camera, &WritePlan::default()).expect("an empty plan");
        assert!(report.writes.is_empty());
        assert!(report.is_exact(), "nothing is exactly what was asked");
        assert!(camera.writes.is_empty());
    }

    #[test]
    fn several_targets_are_written_in_the_order_the_caller_gave_them() {
        let mut camera = camera();
        set(
            &mut camera,
            &[],
            &[
                (slug("brightness"), ControlValue::Int(10)),
                (slug("white_balance_temperature"), ControlValue::Int(20)),
            ],
            false,
        )
        .expect("plannable");
        assert_eq!(
            camera
                .writes
                .iter()
                .map(|(s, v)| (s.as_str(), v.clone()))
                .collect::<Vec<_>>(),
            vec![
                ("brightness", ControlValue::Int(10)),
                ("white_balance_temperature", ControlValue::Int(20)),
            ]
        );
    }
}
