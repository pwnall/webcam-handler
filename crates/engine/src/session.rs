//! The calibration state machine (design D8), as pure transitions over
//! `schema::session` values.
//!
//! Every function here takes a [`Session`] and a `now`, moves one control from one
//! [`ControlStatus`] to another, and returns the [`SessionEvent`] the store should append
//! to `log.ndjson`. Nothing reads a clock, opens a file, or knows what a camera is. The
//! shell does the writing; this module decides whether the writing is allowed.
//!
//! ## What "illegal" means here
//!
//! The status vocabulary is closed (D8), and the transitions between its variants are
//! not all reachable. Selecting a value for a control that never swept would record a
//! `Calibrated` status with no samples behind it — a calibration that looks real in
//! `session.json` and has nothing supporting it. Every such move is
//! [`schema::Error::IllegalTransition`], naming the state it refused from and the
//! operation attempted, and every legality question is answered by an exhaustive `match`
//! so a new status variant cannot quietly inherit permissions (rubric rule 6).
//!
//! A control with no entry in the session is [`ControlStatus::Untouched`]. That is why
//! there is no "unknown" state: the absence *is* the initial state, and the two cannot
//! drift apart.

use schema::control::{ControlSlug, ControlValue};
use schema::metrics::{MetricName, Preference};
use schema::session::{
    BlockedReason, ControlSession, ControlStatus, Sample, Selector, Session, SessionEvent,
    SweepSpec,
};
use schema::time::Stamp;
use schema::{Error, Result};

use crate::refusal::illegal;

/// Put a control at the end of the calibration queue, if it is not already in it.
///
/// Idempotent: the queue is an ordering, not a multiset, and enqueueing twice is a
/// caller repeating itself rather than an error worth a variant.
pub fn enqueue(session: &mut Session, control: &ControlSlug, now: Stamp) {
    if !session.queue.contains(control) {
        session.queue.push(control.clone());
        session.updated_at = now;
    }
}

/// Reorder the queue (the skill's step 10.5: calibrate exposure before focus after all).
///
/// # Errors
///
/// [`Error::IllegalTransition`] with `from = "not_a_permutation"` when `order` is not
/// exactly the current queue rearranged. Accepting a partial order would silently drop
/// the controls it omits, which reads in `session.json` as work nobody ever queued.
pub fn reorder_queue(session: &mut Session, order: &[ControlSlug], now: Stamp) -> Result<()> {
    let mut current: Vec<&ControlSlug> = session.queue.iter().collect();
    let mut requested: Vec<&ControlSlug> = order.iter().collect();
    current.sort_unstable();
    requested.sort_unstable();
    if current != requested {
        return Err(illegal("not_a_permutation", "reorder queue"));
    }
    session.queue = order.to_vec();
    session.updated_at = now;
    Ok(())
}

/// Record that an automation control was switched off so `manual` can be driven by hand.
///
/// Called once per automation partner, matching the one-write-per-partner shape of
/// [`crate::pairing::plan`]. Calling it again for the same manual control adds the
/// second partner rather than replacing the first.
///
/// # Errors
///
/// [`Error::IllegalTransition`] from [`ControlStatus::Sweeping`] (the sweep is already
/// running; changing its automation mid-flight invalidates the samples taken so far) and
/// from [`ControlStatus::Blocked`].
pub fn auto_disabled(
    session: &mut Session,
    manual: &ControlSlug,
    automation: &ControlSlug,
    parked_value: Option<i64>,
    now: Stamp,
) -> Result<SessionEvent> {
    let entry = session.controls.entry(manual.clone()).or_default();
    if !may_auto_disable(&entry.status) {
        return Err(refuse(&entry.status, "auto-disable", manual));
    }

    let mut disabled = match &entry.status {
        ControlStatus::AutoDisabled {
            automation: already,
            ..
        } => already.clone(),
        _ => Vec::new(),
    };
    if !disabled.contains(automation) {
        disabled.push(automation.clone());
    }
    entry.status = ControlStatus::AutoDisabled {
        automation: disabled,
        parked_value,
    };
    session.updated_at = now;
    Ok(SessionEvent::AutomationDisabled {
        manual: manual.clone(),
        automation: automation.clone(),
    })
}

/// Begin a sweep of `control` over `total` samples.
///
/// Legal from [`ControlStatus::Calibrated`] as well as from the earlier states: D8's
/// `precision` field exists so a coarse pass can be followed by a fine one, and a
/// refinement sweep starts from a control that is already calibrated.
///
/// # Errors
///
/// [`Error::IllegalTransition`] from [`ControlStatus::Blocked`] (the device says this
/// control cannot be calibrated, and sweeping it anyway produces a directory of
/// identical photos), from [`ControlStatus::Sweeping`] (one sweep at a time — a second
/// would interleave its samples with the first's), and when `total` is zero, which is a
/// sweep that takes no photos and reports success.
pub fn begin_sweep(
    session: &mut Session,
    control: &ControlSlug,
    plan: &SweepSpec,
    total: u32,
    now: Stamp,
) -> Result<SessionEvent> {
    if total == 0 {
        return Err(illegal(
            "no_values(sweep total=0)",
            format!("sweep {control}"),
        ));
    }
    let entry = session.controls.entry(control.clone()).or_default();
    if !may_begin_sweep(&entry.status) {
        return Err(refuse(&entry.status, "sweep", control));
    }
    entry.status = ControlStatus::Sweeping {
        plan: plan.clone(),
        done: 0,
        total,
    };
    session.updated_at = now;
    Ok(SessionEvent::SweepStarted {
        control: control.clone(),
        total,
    })
}

/// Append one sample to a running sweep.
///
/// # Errors
///
/// [`Error::IllegalTransition`] unless the control is [`ControlStatus::Sweeping`], and
/// when the sweep already holds every sample it planned — an over-run means the executor
/// and the plan disagree about how many photos this sweep takes, and the sample counts
/// in `session.json` are what a later refinement pass reads.
pub fn record_sample(
    session: &mut Session,
    control: &ControlSlug,
    sample: Sample,
    now: Stamp,
) -> Result<SessionEvent> {
    let entry = session.controls.entry(control.clone()).or_default();
    let ControlStatus::Sweeping { plan, done, total } = &entry.status else {
        return Err(refuse(&entry.status, "record a sample for", control));
    };
    if done >= total {
        return Err(illegal(
            format!("sweeping(complete {done}/{total})"),
            format!("record a sample for {control}"),
        ));
    }
    let event = SessionEvent::SampleTaken {
        control: control.clone(),
        requested: sample.requested,
        applied: sample.applied,
    };
    entry.status = ControlStatus::Sweeping {
        plan: plan.clone(),
        done: done.saturating_add(1),
        total: *total,
    };
    entry.samples.push(sample);
    session.updated_at = now;
    Ok(event)
}

/// Put a control that began a sweep and recorded nothing back where the sweep found it.
///
/// The interruption path's other half. Leaving a control mid-sweep is right when samples
/// were taken — they happened, and `select` is the way out of [`ControlStatus::Sweeping`]
/// — but a sweep that stopped before its *first* sample recorded nothing, and
/// `Sweeping { done: 0 }` is a state with no exit: `may_begin_sweep` refuses it,
/// `selectable` refuses `no_samples`, `crate::lifecycle::draft` skips anything that is
/// not `Untouched`, and `Sweeping` is never terminal, so the session's (camera, task) slot
/// never settles either. A transient availability failure — an unplug, a `SettleTimeout`
/// \[PF:11\], `ENOSPC` on the first photo — would become a permanent capability refusal
/// for that control, which is the conversion AGENTS rule 7 forbids, one layer up.
///
/// Only the status moves. The `SweepInterrupted` line (N18) is what records that the
/// attempt happened, and it is appended by the same path.
///
/// # Errors
///
/// [`Error::IllegalTransition`] from any state other than `Sweeping { done: 0 }` —
/// including a sweep that took samples, whose record must not be thrown away.
pub fn abandon_sweep(session: &mut Session, control: &ControlSlug, now: Stamp) -> Result<()> {
    let entry = session.controls.entry(control.clone()).or_default();
    let ControlStatus::Sweeping { done: 0, .. } = &entry.status else {
        return Err(refuse(&entry.status, "abandon the sweep of", control));
    };
    if !entry.samples.is_empty() {
        // The counter and the samples disagree, which is a defect somewhere else; refusing
        // is the direction that keeps the evidence.
        return Err(illegal(
            format!("sweeping(0 done, {} sample(s))", entry.samples.len()),
            format!("abandon the sweep of {control}"),
        ));
    }
    entry.status = ControlStatus::Untouched;
    session.updated_at = now;
    Ok(())
}

/// Choose a value an agent or a human picked by looking at the photos.
///
/// Selection is by *applied* value, not by requested value: the applied value is the one
/// the camera held when the photo was taken \[PF:6\], so it is the one worth writing back.
///
/// # Errors
///
/// [`Error::IllegalTransition`] when the control never swept, when the sweep recorded no
/// samples, or when no sample holds `value` — a calibration recorded against a value
/// nobody photographed is a claim with nothing behind it.
pub fn select_value(
    session: &mut Session,
    control: &ControlSlug,
    value: i64,
    selector: Selector,
    now: Stamp,
) -> Result<SessionEvent> {
    let entry = selectable(session, control)?;
    if !entry.samples.iter().any(|sample| sample.applied == value) {
        return Err(illegal(
            format!("no_sample_applied({value})"),
            format!("select {control}"),
        ));
    }
    let precision = sampled_precision(&entry.samples);
    entry.status = ControlStatus::Calibrated {
        value,
        precision,
        score: None,
        selector: selector.clone(),
    };
    session.updated_at = now;
    Ok(SessionEvent::Selected {
        control: control.clone(),
        value,
        selector,
    })
}

/// Choose the best-scoring sample under a metric.
///
/// # Errors
///
/// - `undirected_metric(<name>)` — the metric cannot rank. [`MetricName::MeanLuma`] is
///   the one: neither more nor less of it is better, so "the best" is undefined and
///   picking the largest would quietly return the brightest photo as though that were an
///   answer.
/// - `no_scores(<name>)` — no sample carries that metric. Scoring is done by the
///   executor, and a metric nobody computed cannot rank anything.
/// - the same refusals as [`select_value`] for a control that never swept.
///
/// Ties keep the earliest sample, so the choice does not depend on map iteration order.
/// Non-finite scores are not ranked: a NaN is the absence of a measurement, and
/// `f64::total_cmp` would happily sort it to the top.
pub fn select_by_metric(
    session: &mut Session,
    control: &ControlSlug,
    metric: MetricName,
    now: Stamp,
) -> Result<SessionEvent> {
    let higher_is_better = match metric.preference() {
        Preference::HigherIsBetter => true,
        Preference::LowerIsBetter => false,
        Preference::Undirected => {
            return Err(illegal(
                format!("undirected_metric({metric})"),
                format!("select {control}"),
            ));
        }
    };

    let entry = selectable(session, control)?;
    let mut best: Option<(i64, f64)> = None;
    for sample in &entry.samples {
        let Some(score) = sample
            .metrics
            .get(&metric)
            .copied()
            .filter(|s| s.is_finite())
        else {
            continue;
        };
        let improves = match best {
            None => true,
            Some((_, incumbent)) => {
                if higher_is_better {
                    score > incumbent
                } else {
                    score < incumbent
                }
            }
        };
        if improves {
            best = Some((sample.applied, score));
        }
    }

    let Some((value, score)) = best else {
        return Err(illegal(
            format!("no_scores({metric})"),
            format!("select {control}"),
        ));
    };
    let selector = Selector::Metric { name: metric };
    entry.status = ControlStatus::Calibrated {
        value,
        precision: sampled_precision(&entry.samples),
        score: Some(score),
        selector: selector.clone(),
    };
    session.updated_at = now;
    Ok(SessionEvent::Selected {
        control: control.clone(),
        value,
        selector,
    })
}

/// Set a control aside without calibrating it.
///
/// # Errors
///
/// [`Error::IllegalTransition`] from [`ControlStatus::Blocked`]: a blocked control is
/// already set aside, and the device's reason for it is more informative than the
/// operator's.
pub fn defer(session: &mut Session, control: &ControlSlug, reason: &str, now: Stamp) -> Result<()> {
    let entry = session.controls.entry(control.clone()).or_default();
    if !may_defer(&entry.status) {
        return Err(refuse(&entry.status, "defer", control));
    }
    entry.status = ControlStatus::Deferred {
        reason: reason.to_owned(),
    };
    session.updated_at = now;
    Ok(())
}

/// Record that the device will not let this control be calibrated.
///
/// Infallible, and legal from any state including [`ControlStatus::Calibrated`]: the
/// device is the only authority on itself (E2), and a control that has gone READ_ONLY
/// since the sweep is a fact about now, not a transition to argue with. The samples stay
/// on the record — they happened.
pub fn block(session: &mut Session, control: &ControlSlug, reason: BlockedReason, now: Stamp) {
    let entry = session.controls.entry(control.clone()).or_default();
    entry.status = ControlStatus::Blocked { reason };
    session.updated_at = now;
}

/// The writes that applying this session would make, in queue order.
///
/// Shaped to hand straight to [`crate::pairing::plan`]: applying a session is a guarded
/// write of its calibrated values, and the automation ordering is that module's law.
/// Controls calibrated but never queued come after the queue, in slug order, so the
/// result does not depend on map iteration.
///
/// # Errors
///
/// - `no_calibrated_controls` — the session has chosen nothing, so applying it would
///   write nothing and report success (design §2.4). `partial` says the caller meant it.
/// - `unsettled(<n> pending)` — queued controls are still untouched or mid-sweep.
///   `partial` is how a caller says "apply what is decided so far".
pub fn apply_targets(session: &Session, partial: bool) -> Result<Vec<(ControlSlug, ControlValue)>> {
    let calibrated: Vec<(ControlSlug, i64)> = session
        .calibrated()
        .into_iter()
        .map(|(slug, value)| (slug.clone(), value))
        .collect();

    if !partial {
        if calibrated.is_empty() {
            return Err(illegal("no_calibrated_controls", "apply"));
        }
        let pending = session
            .queue
            .iter()
            .filter(|slug| !is_terminal(session, slug))
            .count();
        if pending > 0 {
            return Err(illegal(format!("unsettled({pending} pending)"), "apply"));
        }
    }

    let mut ordered: Vec<(ControlSlug, ControlValue)> = Vec::with_capacity(calibrated.len());
    for slug in &session.queue {
        if let Some((_, value)) = calibrated.iter().find(|(candidate, _)| candidate == slug) {
            ordered.push((slug.clone(), ControlValue::Int(*value)));
        }
    }
    for (slug, value) in &calibrated {
        if !session.queue.contains(slug) {
            ordered.push((slug.clone(), ControlValue::Int(*value)));
        }
    }
    Ok(ordered)
}

/// The spacing a sweep actually achieved: the smallest positive gap between the values
/// the camera held.
///
/// Derived from the samples rather than taken from the caller, because the plan's stride
/// is what was *asked for* and the driver aligns writes it does not like \[PF:6\]. Zero
/// when there is only one sample — there is no spacing to report, and a consumer
/// dividing this down for a refinement pass must treat zero as "unknown" rather than as
/// a divisor.
#[must_use]
pub fn sampled_precision(samples: &[Sample]) -> i64 {
    let mut applied: Vec<i64> = samples.iter().map(|sample| sample.applied).collect();
    applied.sort_unstable();
    applied.dedup();
    applied
        .windows(2)
        .filter_map(|window| {
            let first = window.first()?;
            let second = window.get(1)?;
            i64::try_from((i128::from(*second) - i128::from(*first)).abs()).ok()
        })
        .filter(|gap| *gap > 0)
        .min()
        .unwrap_or(0)
}

/// The control's entry, once it is established that it can be selected on.
fn selectable<'a>(
    session: &'a mut Session,
    control: &ControlSlug,
) -> Result<&'a mut ControlSession> {
    let entry = session.controls.entry(control.clone()).or_default();
    if !may_select(&entry.status) {
        return Err(refuse(&entry.status, "select", control));
    }
    if entry.samples.is_empty() {
        // `Sweeping { done: 0 }` is "swept" only in name. Selecting here would record a
        // calibrated value with no photo behind it.
        return Err(illegal("no_samples", format!("select {control}")));
    }
    Ok(entry)
}

fn refuse(status: &ControlStatus, op: &str, control: &ControlSlug) -> Error {
    illegal(status.name(), format!("{op} {control}"))
}

fn is_terminal(session: &Session, control: &ControlSlug) -> bool {
    matches!(
        session.controls.get(control).map(|entry| &entry.status),
        Some(
            ControlStatus::Calibrated { .. }
                | ControlStatus::Deferred { .. }
                | ControlStatus::Blocked { .. }
        )
    )
}

// The four legality questions. Each is an exhaustive match rather than a `matches!` with
// a wildcard, so adding a status variant is a compile error in four places instead of a
// silent grant of every permission (rubric rule 6).

fn may_auto_disable(status: &ControlStatus) -> bool {
    match status {
        ControlStatus::Untouched
        | ControlStatus::AutoDisabled { .. }
        | ControlStatus::Calibrated { .. }
        | ControlStatus::Deferred { .. } => true,
        ControlStatus::Sweeping { .. } | ControlStatus::Blocked { .. } => false,
    }
}

fn may_begin_sweep(status: &ControlStatus) -> bool {
    match status {
        ControlStatus::Untouched
        | ControlStatus::AutoDisabled { .. }
        | ControlStatus::Calibrated { .. }
        | ControlStatus::Deferred { .. } => true,
        ControlStatus::Sweeping { .. } | ControlStatus::Blocked { .. } => false,
    }
}

fn may_select(status: &ControlStatus) -> bool {
    match status {
        ControlStatus::Sweeping { .. } | ControlStatus::Calibrated { .. } => true,
        ControlStatus::Untouched
        | ControlStatus::AutoDisabled { .. }
        | ControlStatus::Deferred { .. }
        | ControlStatus::Blocked { .. } => false,
    }
}

fn may_defer(status: &ControlStatus) -> bool {
    match status {
        ControlStatus::Untouched
        | ControlStatus::AutoDisabled { .. }
        | ControlStatus::Sweeping { .. }
        | ControlStatus::Calibrated { .. }
        | ControlStatus::Deferred { .. } => true,
        ControlStatus::Blocked { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use camino::Utf8PathBuf;
    use schema::ErrorKind;
    use schema::camera::CameraFingerprint;
    use schema::session::new_session;
    use uuid::Uuid;

    use super::*;

    fn slug(name: &str) -> ControlSlug {
        ControlSlug::parse(name).expect("literal slug")
    }

    fn fixture() -> Session {
        new_session(
            Uuid::nil(),
            CameraFingerprint {
                bus_path: "3-1:1.0".to_owned(),
                usb_id: None,
                card: "OBSBOT Tiny 3".to_owned(),
                driver: "uvcvideo".to_owned(),
                serial: None,
            },
            "Read text from the DUT display",
            "legible text",
            "0.1.0",
            Stamp::epoch(),
        )
    }

    fn sample(applied: i64, scores: &[(MetricName, f64)]) -> Sample {
        Sample {
            requested: applied,
            applied,
            warnings: Vec::new(),
            photo: Utf8PathBuf::from(format!("photos/focus_absolute/{applied}.jpg")),
            metrics: scores.iter().copied().collect::<BTreeMap<_, _>>(),
            captured_at: Stamp::epoch(),
        }
    }

    fn later() -> Stamp {
        Stamp::from_millis(1_000).expect("in range")
    }

    /// A control taken all the way to a finished sweep with scored samples.
    fn swept(session: &mut Session, control: &ControlSlug, scores: &[(i64, f64)]) {
        enqueue(session, control, Stamp::epoch());
        begin_sweep(
            session,
            control,
            &SweepSpec::Uniform { step: 10 },
            u32::try_from(scores.len()).expect("small fixture"),
            Stamp::epoch(),
        )
        .expect("a fresh control sweeps");
        for (applied, score) in scores {
            record_sample(
                session,
                control,
                sample(*applied, &[(MetricName::Sharpness, *score)]),
                Stamp::epoch(),
            )
            .expect("a running sweep accepts samples");
        }
    }

    // ------------------------------------------------------- the legal path

    #[test]
    fn the_legal_path_runs_from_untouched_to_applied() {
        let mut session = fixture();
        let focus = slug("focus_absolute");

        enqueue(&mut session, &focus, later());
        assert_eq!(session.queue, vec![focus.clone()]);

        let event = auto_disabled(
            &mut session,
            &focus,
            &slug("focus_automatic_continuous"),
            Some(200),
            later(),
        )
        .expect("untouched controls can have their automation disabled");
        assert_eq!(
            event,
            SessionEvent::AutomationDisabled {
                manual: focus.clone(),
                automation: slug("focus_automatic_continuous"),
            }
        );

        let event = begin_sweep(
            &mut session,
            &focus,
            &SweepSpec::Uniform { step: 10 },
            3,
            later(),
        )
        .expect("an auto-disabled control sweeps");
        assert_eq!(
            event,
            SessionEvent::SweepStarted {
                control: focus.clone(),
                total: 3
            }
        );

        for (value, score) in [(0_i64, 10.0_f64), (10, 90.0), (20, 40.0)] {
            record_sample(
                &mut session,
                &focus,
                sample(value, &[(MetricName::Sharpness, score)]),
                later(),
            )
            .expect("a running sweep accepts samples");
        }

        let event = select_by_metric(&mut session, &focus, MetricName::Sharpness, later())
            .expect("a swept control can be selected on");
        assert_eq!(
            event,
            SessionEvent::Selected {
                control: focus.clone(),
                value: 10,
                selector: Selector::Metric {
                    name: MetricName::Sharpness
                },
            }
        );
        assert_eq!(
            session.controls.get(&focus).map(|entry| &entry.status),
            Some(&ControlStatus::Calibrated {
                value: 10,
                precision: 10,
                score: Some(90.0),
                selector: Selector::Metric {
                    name: MetricName::Sharpness
                },
            })
        );

        assert!(session.is_settled());
        assert_eq!(
            apply_targets(&session, false).expect("a settled session applies"),
            vec![(focus, ControlValue::Int(10))]
        );
        assert_eq!(session.updated_at, later(), "every move stamps the session");
    }

    #[test]
    fn a_calibrated_control_can_be_swept_again_for_a_finer_pass() {
        // D8's `precision` field exists so coarse-then-fine is representable; a state
        // machine that refused this would make the field a lie.
        let mut session = fixture();
        let focus = slug("focus_absolute");
        swept(&mut session, &focus, &[(0, 1.0), (100, 2.0)]);
        select_by_metric(&mut session, &focus, MetricName::Sharpness, later()).expect("selects");
        begin_sweep(
            &mut session,
            &focus,
            &SweepSpec::Uniform { step: 1 },
            5,
            later(),
        )
        .expect("a refinement pass is legal");
    }

    #[test]
    fn a_sweep_that_recorded_nothing_is_abandoned_and_one_that_recorded_something_is_not() {
        // Both directions of the rule the interruption path relies on. Zero samples: the
        // control goes back to `Untouched`, because `Sweeping { done: 0 }` has no exit and
        // an availability failure must not become a capability refusal (rule 7). One
        // sample: refused, because the sample is evidence and `select` is the way out.
        let mut session = fixture();
        let focus = slug("focus_absolute");
        enqueue(&mut session, &focus, Stamp::epoch());
        begin_sweep(
            &mut session,
            &focus,
            &SweepSpec::Uniform { step: 10 },
            5,
            Stamp::epoch(),
        )
        .expect("a fresh control sweeps");
        abandon_sweep(&mut session, &focus, later()).expect("nothing was recorded");
        assert_eq!(session.controls[&focus].status, ControlStatus::Untouched);
        assert_eq!(session.updated_at, later());
        // …and it is sweepable again, which is the whole point.
        begin_sweep(
            &mut session,
            &focus,
            &SweepSpec::Uniform { step: 10 },
            5,
            later(),
        )
        .expect("an abandoned sweep leaves a sweepable control");
        record_sample(
            &mut session,
            &focus,
            sample(10, &[(MetricName::Sharpness, 1.0)]),
            later(),
        )
        .expect("a running sweep accepts samples");
        let err = abandon_sweep(&mut session, &focus, later())
            .expect_err("a sweep with a sample behind it is not nothing");
        assert_eq!(err.kind(), ErrorKind::IllegalTransition);
        assert_eq!(session.controls[&focus].samples.len(), 1);

        // And from every state that is not a sweep at all.
        let gamma = slug("gamma");
        assert_eq!(
            abandon_sweep(&mut session, &gamma, later())
                .expect_err("nothing was swept")
                .kind(),
            ErrorKind::IllegalTransition
        );
    }

    // -------------------------------------------------- illegal transitions

    #[test]
    fn a_control_that_never_swept_cannot_be_selected_on() {
        let mut session = fixture();
        let focus = slug("focus_absolute");
        let err = select_value(&mut session, &focus, 10, Selector::Human, later())
            .expect_err("nothing swept");
        assert_eq!(
            err,
            Error::IllegalTransition {
                from: "untouched".to_owned(),
                op: "select focus_absolute".to_owned(),
            }
        );
    }

    #[test]
    fn an_auto_disabled_control_cannot_be_selected_on_either() {
        // Disabling automation is preparation, not evidence.
        let mut session = fixture();
        let focus = slug("focus_absolute");
        auto_disabled(
            &mut session,
            &focus,
            &slug("focus_automatic_continuous"),
            None,
            later(),
        )
        .expect("legal");
        let err = select_value(&mut session, &focus, 10, Selector::Human, later())
            .expect_err("no samples");
        assert!(err.to_string().contains("auto_disabled"), "{err}");
    }

    #[test]
    fn a_sweep_that_recorded_no_samples_cannot_be_selected_on() {
        let mut session = fixture();
        let focus = slug("focus_absolute");
        begin_sweep(&mut session, &focus, &SweepSpec::All, 4, later()).expect("legal");
        let err = select_value(&mut session, &focus, 10, Selector::Human, later())
            .expect_err("no samples yet");
        assert!(err.to_string().contains("no_samples"), "{err}");
    }

    #[test]
    fn a_blocked_control_cannot_be_swept() {
        let mut session = fixture();
        let privacy = slug("privacy");
        block(
            &mut session,
            &privacy,
            BlockedReason::ReadOnly,
            Stamp::epoch(),
        );
        let err = begin_sweep(&mut session, &privacy, &SweepSpec::All, 4, later())
            .expect_err("the device said no");
        assert_eq!(
            err,
            Error::IllegalTransition {
                from: "blocked".to_owned(),
                op: "sweep privacy".to_owned(),
            }
        );
    }

    #[test]
    fn a_control_already_sweeping_cannot_start_a_second_sweep() {
        // Two sweeps interleaving their samples into one list is a sample record nobody
        // can read afterwards.
        let mut session = fixture();
        let focus = slug("focus_absolute");
        begin_sweep(&mut session, &focus, &SweepSpec::All, 4, later()).expect("legal");
        let err = begin_sweep(&mut session, &focus, &SweepSpec::All, 4, later())
            .expect_err("one at a time");
        assert!(err.to_string().contains("from state sweeping"), "{err}");
    }

    #[test]
    fn a_sweep_of_zero_samples_is_refused_rather_than_recorded() {
        let mut session = fixture();
        let err = begin_sweep(
            &mut session,
            &slug("focus_absolute"),
            &SweepSpec::All,
            0,
            later(),
        )
        .expect_err("a sweep with no samples is not a sweep");
        assert!(
            err.to_string().contains("no_values(sweep total=0)"),
            "{err}"
        );
    }

    #[test]
    fn a_sample_cannot_be_recorded_against_a_control_that_is_not_sweeping() {
        let mut session = fixture();
        let focus = slug("focus_absolute");
        let err = record_sample(&mut session, &focus, sample(10, &[]), later())
            .expect_err("no sweep is running");
        assert_eq!(
            err,
            Error::IllegalTransition {
                from: "untouched".to_owned(),
                op: "record a sample for focus_absolute".to_owned(),
            }
        );
    }

    #[test]
    fn a_sweep_cannot_record_more_samples_than_it_planned() {
        let mut session = fixture();
        let focus = slug("focus_absolute");
        begin_sweep(&mut session, &focus, &SweepSpec::All, 1, later()).expect("legal");
        record_sample(&mut session, &focus, sample(0, &[]), later()).expect("the one sample");
        let err = record_sample(&mut session, &focus, sample(1, &[]), later())
            .expect_err("the plan said one");
        assert!(err.to_string().contains("sweeping(complete 1/1)"), "{err}");
    }

    #[test]
    fn automation_cannot_be_disabled_under_a_running_sweep() {
        // Changing what governs a control mid-sweep invalidates every sample already
        // taken, and the samples do not say which side of the change they are on.
        let mut session = fixture();
        let focus = slug("focus_absolute");
        begin_sweep(&mut session, &focus, &SweepSpec::All, 4, later()).expect("legal");
        let err = auto_disabled(
            &mut session,
            &focus,
            &slug("focus_automatic_continuous"),
            None,
            later(),
        )
        .expect_err("mid-sweep");
        assert!(err.to_string().contains("from state sweeping"), "{err}");
    }

    #[test]
    fn a_blocked_control_cannot_be_deferred() {
        let mut session = fixture();
        let privacy = slug("privacy");
        block(
            &mut session,
            &privacy,
            BlockedReason::ReadOnly,
            Stamp::epoch(),
        );
        let err = defer(&mut session, &privacy, "later", later()).expect_err("already set aside");
        assert!(err.to_string().contains("from state blocked"), "{err}");

        // The inverse: an untouched control defers.
        defer(&mut session, &slug("brightness"), "no lens", later()).expect("legal");
    }

    #[test]
    fn selecting_a_value_no_sample_holds_is_refused() {
        // The failure this catches: an agent naming the value it *asked* for on a write
        // the driver clamped [PF:6], producing a calibration against a value the camera
        // never held.
        let mut session = fixture();
        let focus = slug("focus_absolute");
        swept(&mut session, &focus, &[(0, 1.0), (10, 2.0)]);
        let err = select_value(&mut session, &focus, 7, Selector::Agent, later())
            .expect_err("no sample at 7");
        assert!(err.to_string().contains("no_sample_applied(7)"), "{err}");

        // The inverse: a value a sample does hold is accepted.
        select_value(&mut session, &focus, 10, Selector::Agent, later()).expect("legal");
    }

    #[test]
    fn every_illegal_transition_reports_the_same_typed_kind() {
        // One kind, so a caller can branch on `IllegalTransition` once instead of
        // matching prose.
        let mut session = fixture();
        let focus = slug("focus_absolute");
        block(
            &mut session,
            &focus,
            BlockedReason::Disabled,
            Stamp::epoch(),
        );
        for err in [
            begin_sweep(&mut session, &focus, &SweepSpec::All, 1, later()).expect_err("blocked"),
            select_value(&mut session, &focus, 0, Selector::Human, later()).expect_err("blocked"),
            defer(&mut session, &focus, "x", later()).expect_err("blocked"),
            auto_disabled(&mut session, &focus, &slug("a"), None, later()).expect_err("blocked"),
            record_sample(&mut session, &focus, sample(0, &[]), later()).expect_err("blocked"),
        ] {
            assert_eq!(err.kind(), ErrorKind::IllegalTransition, "{err}");
        }
    }

    // ------------------------------------------------------------ selection

    #[test]
    fn an_undirected_metric_is_refused_rather_than_ranked() {
        // Mean luma has no direction. Ranking by it would silently return the brightest
        // photo as though brightest meant best.
        let mut session = fixture();
        let focus = slug("focus_absolute");
        enqueue(&mut session, &focus, later());
        begin_sweep(&mut session, &focus, &SweepSpec::All, 2, later()).expect("legal");
        for (value, luma) in [(0_i64, 0.2_f64), (10, 0.9)] {
            record_sample(
                &mut session,
                &focus,
                sample(value, &[(MetricName::MeanLuma, luma)]),
                later(),
            )
            .expect("legal");
        }
        let err = select_by_metric(&mut session, &focus, MetricName::MeanLuma, later())
            .expect_err("mean luma cannot rank");
        assert!(
            err.to_string().contains("undirected_metric(mean_luma)"),
            "{err}"
        );
        // And nothing was written: a refused selection must not half-calibrate.
        assert_eq!(
            session
                .controls
                .get(&focus)
                .map(|entry| entry.status.name()),
            Some("sweeping")
        );
    }

    #[test]
    fn a_lower_is_better_metric_picks_the_smallest_score() {
        // The bug: one comparison for every metric, so clipping is "calibrated" to the
        // most clipped sample.
        let mut session = fixture();
        let exposure = slug("exposure_time_absolute");
        enqueue(&mut session, &exposure, later());
        begin_sweep(&mut session, &exposure, &SweepSpec::All, 3, later()).expect("legal");
        for (value, clipped) in [(100_i64, 0.30_f64), (200, 0.02), (300, 0.75)] {
            record_sample(
                &mut session,
                &exposure,
                sample(value, &[(MetricName::ClippedHighlights, clipped)]),
                later(),
            )
            .expect("legal");
        }
        select_by_metric(
            &mut session,
            &exposure,
            MetricName::ClippedHighlights,
            later(),
        )
        .expect("clipping ranks");
        let Some(ControlStatus::Calibrated { value, score, .. }) =
            session.controls.get(&exposure).map(|entry| &entry.status)
        else {
            panic!("not calibrated");
        };
        assert_eq!(*value, 200);
        assert_eq!(*score, Some(0.02));
    }

    #[test]
    fn a_metric_nobody_computed_cannot_rank() {
        let mut session = fixture();
        let focus = slug("focus_absolute");
        swept(&mut session, &focus, &[(0, 1.0), (10, 2.0)]);
        let err = select_by_metric(&mut session, &focus, MetricName::RmsContrast, later())
            .expect_err("no sample carries it");
        assert!(err.to_string().contains("no_scores(rms_contrast)"), "{err}");
    }

    #[test]
    fn a_non_finite_score_is_not_a_ranking() {
        // A NaN is a measurement that failed. `f64::total_cmp` would sort it above every
        // real score and calibrate the control to whichever sample broke.
        let mut session = fixture();
        let focus = slug("focus_absolute");
        swept(&mut session, &focus, &[(0, 5.0), (10, f64::NAN), (20, 7.0)]);
        select_by_metric(&mut session, &focus, MetricName::Sharpness, later()).expect("ranks");
        let Some(ControlStatus::Calibrated { value, .. }) =
            session.controls.get(&focus).map(|entry| &entry.status)
        else {
            panic!("not calibrated");
        };
        assert_eq!(*value, 20);
    }

    #[test]
    fn a_tie_keeps_the_earliest_sample() {
        let mut session = fixture();
        let focus = slug("focus_absolute");
        swept(&mut session, &focus, &[(0, 9.0), (10, 9.0), (20, 1.0)]);
        select_by_metric(&mut session, &focus, MetricName::Sharpness, later()).expect("ranks");
        let Some(ControlStatus::Calibrated { value, .. }) =
            session.controls.get(&focus).map(|entry| &entry.status)
        else {
            panic!("not calibrated");
        };
        assert_eq!(*value, 0, "ties must not depend on iteration order");
    }

    #[test]
    fn precision_comes_from_what_the_camera_held_not_from_the_plan() {
        // PF:6 inside a sweep: the plan asked for a stride of 10 and the driver aligned
        // every write to 8. The recorded precision is 8.
        let samples = [sample(0, &[]), sample(8, &[]), sample(16, &[])];
        assert_eq!(sampled_precision(&samples), 8);
        assert_eq!(
            sampled_precision(&[sample(4, &[])]),
            0,
            "one sample, no spacing"
        );
        assert_eq!(sampled_precision(&[]), 0);
    }

    // ---------------------------------------------------------------- apply

    #[test]
    fn a_session_with_nothing_calibrated_does_not_apply_without_partial() {
        let session = fixture();
        let err = apply_targets(&session, false).expect_err("nothing chosen");
        assert_eq!(
            err,
            Error::IllegalTransition {
                from: "no_calibrated_controls".to_owned(),
                op: "apply".to_owned(),
            }
        );
        assert_eq!(
            apply_targets(&session, true).expect("partial means the caller meant it"),
            Vec::new()
        );
    }

    #[test]
    fn a_session_with_queued_work_left_does_not_apply_without_partial() {
        let mut session = fixture();
        let focus = slug("focus_absolute");
        swept(&mut session, &focus, &[(0, 1.0), (10, 2.0)]);
        select_by_metric(&mut session, &focus, MetricName::Sharpness, later()).expect("selects");
        enqueue(&mut session, &slug("exposure_time_absolute"), later());

        let err = apply_targets(&session, false).expect_err("one control still pending");
        assert!(err.to_string().contains("unsettled(1 pending)"), "{err}");

        assert_eq!(
            apply_targets(&session, true).expect("partial applies what is decided"),
            vec![(focus, ControlValue::Int(10))]
        );
    }

    #[test]
    fn apply_returns_the_queue_order_and_then_everything_else() {
        // The order matters: the operator queued exposure before focus because on this
        // camera one moves the other, and applying them the other way round undoes it.
        let mut session = fixture();
        let exposure = slug("exposure_time_absolute");
        let focus = slug("focus_absolute");
        let brightness = slug("brightness");
        swept(&mut session, &exposure, &[(100, 1.0), (200, 2.0)]);
        select_by_metric(&mut session, &exposure, MetricName::Sharpness, later()).expect("ok");
        swept(&mut session, &focus, &[(0, 1.0), (10, 2.0)]);
        select_by_metric(&mut session, &focus, MetricName::Sharpness, later()).expect("ok");
        // Calibrated but never queued — `brightness` sorts before both queued slugs, so
        // a map-order implementation would put it first.
        swept(&mut session, &brightness, &[(1, 1.0), (2, 2.0)]);
        select_by_metric(&mut session, &brightness, MetricName::Sharpness, later()).expect("ok");
        session.queue.retain(|slug| slug != &brightness);

        assert_eq!(
            apply_targets(&session, false).expect("settled"),
            vec![
                (exposure, ControlValue::Int(200)),
                (focus, ControlValue::Int(10)),
                (brightness, ControlValue::Int(2)),
            ]
        );
    }

    // ---------------------------------------------------------------- queue

    #[test]
    fn the_queue_reorders_only_into_a_permutation_of_itself() {
        let mut session = fixture();
        let focus = slug("focus_absolute");
        let exposure = slug("exposure_time_absolute");
        enqueue(&mut session, &focus, later());
        enqueue(&mut session, &exposure, later());
        enqueue(&mut session, &focus, later());
        assert_eq!(session.queue.len(), 2, "enqueue is idempotent");

        reorder_queue(&mut session, &[exposure.clone(), focus.clone()], later())
            .expect("a permutation");
        assert_eq!(session.queue, vec![exposure.clone(), focus.clone()]);

        // Dropping a control on the way through would erase queued work silently.
        let err = reorder_queue(&mut session, std::slice::from_ref(&exposure), later())
            .expect_err("not a permutation");
        assert!(err.to_string().contains("not_a_permutation"), "{err}");
        // Adding one that was never queued is the same defect from the other side.
        let err = reorder_queue(
            &mut session,
            &[exposure.clone(), focus.clone(), slug("brightness")],
            later(),
        )
        .expect_err("not a permutation");
        assert!(err.to_string().contains("not_a_permutation"), "{err}");
        assert_eq!(session.queue, vec![exposure, focus], "and nothing moved");
    }

    #[test]
    fn a_session_that_has_been_through_the_machine_still_round_trips_as_json() {
        // D9: session state is a file an agent, a human, or `jq` reads. A state machine
        // that produced a value serde cannot carry would be caught here rather than on
        // the next load.
        let mut session = fixture();
        let focus = slug("focus_absolute");
        swept(&mut session, &focus, &[(0, 1.0), (10, 2.0)]);
        select_by_metric(&mut session, &focus, MetricName::Sharpness, later()).expect("ok");
        block(
            &mut session,
            &slug("privacy"),
            BlockedReason::InactiveWithoutPartner,
            later(),
        );
        defer(&mut session, &slug("brightness"), "no target yet", later()).expect("ok");

        let json = serde_json::to_string_pretty(&session).expect("serialize");
        let back: Session = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, session);
    }
}
