//! The sweep planner (design D8, §2.4).
//!
//! `(ControlDesc, SweepSpec) → Vec<i64>`, total and bounded. The values come from the
//! range the *device* reported for this control — not from a table, not from the
//! caller's idea of what a focus control's range ought to be — and every one of them is
//! aligned to the control's own step, because a driver silently aligns writes it does
//! not like \[PF:6\] and a sample labeled with a value the camera never held poisons every
//! comparison built on it.
//!
//! Two invariants earn the module its tests:
//!
//! - **Bounded.** [`schema::limits::MAX_SWEEP_SAMPLES`] caps every sweep, and
//!   [`schema::limits::MAX_MOTION_SWEEP_SAMPLES`] caps one that moves motors (design §5:
//!   motors wear). A 468000-wide pan range at step 3600 plans 260 photos and a great
//!   deal of travel if nobody says otherwise. Capping widens the *stride*, so a capped
//!   plan still spans the whole range rather than stopping a quarter of the way in.
//! - **Never silently empty.** A plan with no values is a sweep that takes no photos and
//!   reports success. Every way to get there is a typed refusal
//!   ([`schema::Error::IllegalTransition`], in the convention `crate::refusal` pins):
//!   `not_sweepable(<type>)`, `empty_range(min=..,max=..)`, `motion(allow_motion=false)`,
//!   `no_values(<spec>)`.
//!
//! What is *not* here: whether the control can be written at all. READ_ONLY, DISABLED
//! and live automation are the guarded-write planner's law ([`crate::pairing`]), and a
//! second copy of that law here would be a second copy that drifts.

use std::collections::BTreeSet;

use schema::control::{ControlDesc, ControlSlug, ControlType};
use schema::session::SweepSpec;
use schema::{Result, limits};

use crate::refusal::illegal;

/// Which cap trimmed a sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleCap {
    /// [`schema::limits::MAX_SWEEP_SAMPLES`].
    Total,
    /// [`schema::limits::MAX_MOTION_SWEEP_SAMPLES`] — this control moves motors.
    Motion,
}

/// One way the planned values differ from what the spec literally asked for.
///
/// The same doctrine as D3's `{requested, applied}`, one layer up: the caller is told
/// what happened to their request instead of diffing two lists to find out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SweepAdjustment {
    /// A requested value sat outside the control's range.
    Clamped {
        /// What the spec named.
        requested: i64,
        /// What the plan holds.
        planned: i64,
    },
    /// A requested value was not a whole number of steps above the control's minimum.
    StepAligned {
        /// What the spec named.
        requested: i64,
        /// What the plan holds.
        planned: i64,
    },
    /// Values collapsed onto each other after clamping and alignment.
    Deduplicated {
        /// How many were dropped.
        dropped: usize,
    },
    /// A cap trimmed the sweep.
    Capped {
        /// How many samples the spec would have taken.
        requested: u64,
        /// How many the plan holds.
        planned: u32,
        /// The cap that fired.
        limit: u32,
        /// Which cap it was.
        cap: SampleCap,
    },
}

/// An ordered set of values to sweep, and what it took to get there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepPlan {
    /// Which control.
    pub control: ControlSlug,
    /// The values, in the order they will be written. Never empty.
    pub values: Vec<i64>,
    /// The smallest gap between consecutive values — the skill's "calibration
    /// precision", and the number a refinement pass divides down. Falls back to the
    /// control's effective step for a single-value plan.
    pub precision: i64,
    /// How the plan differs from the spec, when it does.
    pub adjustments: Vec<SweepAdjustment>,
}

impl SweepPlan {
    /// How many samples the sweep will take.
    #[must_use]
    pub fn total(&self) -> u32 {
        u32::try_from(self.values.len()).unwrap_or(u32::MAX)
    }
}

/// Whether driving this control moves a motor.
///
/// A heuristic over a name, and deliberately so: V4L2 does not say which controls have
/// mechanics behind them, and the alternative is a per-device table nobody can maintain.
/// The heuristic is safe in the direction that matters. Being *wrong* here — calling a
/// control motorized when it is not — costs a sweep of at most
/// [`schema::limits::MAX_MOTION_SWEEP_SAMPLES`] samples where a wider one was allowed,
/// and the operator can still name values explicitly. Nothing it can do produces a
/// *larger* sweep than the caller asked for, which is the only failure that wears a
/// motor out (design §5).
///
/// `pan_*` and `tilt_*` only. Focus and zoom motors exist, but §5's concern is PTZ
/// travel, and quietly capping every focus sweep at 32 samples would gut the precision
/// of the tool's main use case.
#[must_use]
pub fn is_motion_control(slug: &ControlSlug) -> bool {
    let slug = slug.as_str();
    slug == "pan" || slug == "tilt" || slug.starts_with("pan_") || slug.starts_with("tilt_")
}

/// A control type's name, for messages and for
/// [`schema::session::BlockedReason::NotSweepable`].
///
/// An exhaustive match, so a new control type cannot acquire a name by accident (rubric
/// rule 6), and `Unknown` prints its raw discriminant rather than the word "unknown"
/// alone — the discriminant is the only useful thing anyone can say about it.
#[must_use]
pub fn control_type_name(control_type: ControlType) -> String {
    match control_type {
        ControlType::Integer => "integer".to_owned(),
        ControlType::Boolean => "boolean".to_owned(),
        ControlType::Menu => "menu".to_owned(),
        ControlType::Button => "button".to_owned(),
        ControlType::Integer64 => "integer64".to_owned(),
        ControlType::ControlClass => "control_class".to_owned(),
        ControlType::String => "string".to_owned(),
        ControlType::Bitmask => "bitmask".to_owned(),
        ControlType::IntegerMenu => "integer_menu".to_owned(),
        ControlType::U8 => "u8".to_owned(),
        ControlType::U16 => "u16".to_owned(),
        ControlType::U32 => "u32".to_owned(),
        ControlType::Area => "area".to_owned(),
        ControlType::Rect => "rect".to_owned(),
        ControlType::Unknown { raw } => format!("unknown(0x{raw:04x})"),
    }
}

/// Plan a sweep of `desc` according to `spec`.
///
/// # Errors
///
/// [`schema::Error::IllegalTransition`] in every case, with `from` naming the condition:
///
/// - `not_sweepable(<type>)` — a compound, opaque or string control has no ordered range
///   to walk. `Region of Interest Rectangle` \[PF:1\] enumerates and round-trips; it does
///   not sweep.
/// - `empty_range(min=..,max=..)` — the device reported a maximum below its minimum.
///   Represented, not corrected (D2), and refused here rather than silently turned into
///   a backwards loop.
/// - `motion(allow_motion=false)` — the control moves motors and the caller did not pass
///   `--allow-motion`. Never an implicit default (design §5).
/// - `no_values(<spec>)` — the spec itself names nothing: zero log points, an empty
///   explicit list.
pub fn plan(desc: &ControlDesc, spec: &SweepSpec, allow_motion: bool) -> Result<SweepPlan> {
    let op = format!("sweep {}", desc.slug);

    if !desc.control_type.is_scalar() {
        return Err(illegal(
            format!("not_sweepable({})", control_type_name(desc.control_type)),
            op,
        ));
    }

    let (limit, cap) = if is_motion_control(&desc.slug) {
        if !allow_motion {
            return Err(illegal("motion(allow_motion=false)", op));
        }
        (
            limits::MAX_MOTION_SWEEP_SAMPLES.min(limits::MAX_SWEEP_SAMPLES),
            SampleCap::Motion,
        )
    } else {
        (limits::MAX_SWEEP_SAMPLES, SampleCap::Total)
    };

    let (min, max) = (desc.range.min, desc.range.max);
    let step = desc.range.effective_step();
    if max < min {
        // PF:4/PF:5 territory: the device contradicting itself. Such a range is reported
        // as measured everywhere else; here it is the one thing a sweep cannot work with.
        return Err(illegal(format!("empty_range(min={min},max={max})"), op));
    }

    let mut adjustments = Vec::new();
    let values = match spec {
        SweepSpec::All => strided(min, max, step, limit, cap, &mut adjustments),
        SweepSpec::Uniform { step: requested } => strided(
            min,
            max,
            round_up_to_step(*requested, step),
            limit,
            cap,
            &mut adjustments,
        ),
        SweepSpec::Log { points } => {
            if *points == 0 {
                return Err(illegal("no_values(log points=0)", op));
            }
            log_spaced(min, max, step, *points, limit, cap, &mut adjustments)
        }
        SweepSpec::Explicit { values } => {
            if values.is_empty() {
                return Err(illegal("no_values(explicit)", op));
            }
            explicit(min, max, step, values, limit, cap, &mut adjustments)
        }
    };

    // The four constructions above each yield at least one value for a non-empty range,
    // so this cannot fire today. It stays because "never silently empty" is a contract
    // rather than an observation: a fifth `SweepSpec` variant lands in the exhaustive
    // match above and could reach here, and a typed refusal is what it should get
    // rather than a sweep that takes no photos and reports success.
    if values.is_empty() {
        return Err(illegal("no_values(plan)", op));
    }

    Ok(SweepPlan {
        control: desc.slug.clone(),
        precision: precision_of(&values, step),
        values,
        adjustments,
    })
}

/// Values `min, min + stride, …` up to `max`, with the stride widened until the count
/// fits under `limit`.
///
/// Widening the stride rather than truncating the list is what keeps a capped sweep
/// spanning the whole range. A truncated sweep of a pan control photographs the left
/// quarter of the room and calls it a calibration.
fn strided(
    min: i64,
    max: i64,
    base_stride: i64,
    limit: u32,
    cap: SampleCap,
    adjustments: &mut Vec<SweepAdjustment>,
) -> Vec<i64> {
    let span = i128::from(max) - i128::from(min);
    let limit_count = i128::from(limit);
    let mut stride = i128::from(base_stride.max(1));

    let requested_count = span / stride + 1;
    let capped = requested_count > limit_count;
    if capped && limit_count > 0 {
        // The smallest whole factor that brings the count under the cap.
        let factor = requested_count.div_euclid(limit_count)
            + i128::from(requested_count.rem_euclid(limit_count) != 0);
        stride = stride.saturating_mul(factor);
    }

    let ceiling = usize::try_from(limit).unwrap_or(usize::MAX);
    let mut values = Vec::new();
    let mut next = i128::from(min);
    while next <= i128::from(max) && values.len() < ceiling {
        match i64::try_from(next) {
            Ok(value) => values.push(value),
            // Unreachable while `next <= max`; a break rather than a panic if the
            // arithmetic above ever stops being true.
            Err(_) => break,
        }
        next = next.saturating_add(stride);
    }

    note_cap(requested_count, &values, limit, cap, capped, adjustments);
    values
}

/// The requested spacing, rounded up to a whole number of the control's own steps.
///
/// A sweep cannot sample finer than the device's step, and asking for 3 on a step-7
/// control would otherwise produce values the driver silently aligns \[PF:6\] — samples
/// recorded against values the camera never held.
fn round_up_to_step(requested: i64, step: i64) -> i64 {
    let step = i128::from(step.max(1));
    let requested = i128::from(requested);
    if requested <= step {
        return i64::try_from(step).unwrap_or(i64::MAX);
    }
    let multiples = requested.div_euclid(step) + i128::from(requested.rem_euclid(step) != 0);
    i64::try_from(multiples.saturating_mul(step)).unwrap_or(i64::MAX)
}

/// Logarithmically spaced values across `[min, max]`.
///
/// Exposure time is why this exists: its useful range spans orders of magnitude, and a
/// uniform sweep of it spends most of its samples in the part that is already blown out.
///
/// The range may contain zero or be entirely negative — `Zoom, Continuous` is
/// `[-100..100]` on the OBSBOT — so the values are spaced geometrically over the range
/// *shifted* to start at 1, then shifted back. `low` is therefore at least 1 by
/// construction: there is no logarithm of a non-positive number anywhere in here, and no
/// NaN to leak into a sample list.
fn log_spaced(
    min: i64,
    max: i64,
    step: i64,
    points: u32,
    limit: u32,
    cap: SampleCap,
    adjustments: &mut Vec<SweepAdjustment>,
) -> Vec<i64> {
    let wanted = points.min(limit).max(1);
    let shift = if min <= 0 { 1 - i128::from(min) } else { 0 };
    let low = to_f64(i128::from(min) + shift);
    let high = to_f64(i128::from(max) + shift);
    let ratio = high / low;

    let mut values: Vec<i64> = Vec::new();
    for index in 0..wanted {
        let fraction = if wanted <= 1 {
            0.0
        } else {
            f64::from(index) / f64::from(wanted - 1)
        };
        let raw = i128::from(to_i64(low * ratio.powf(fraction))) - shift;
        values.push(align(min, max, step, raw));
    }

    // The sequence is non-decreasing, so equal values are adjacent: two log points that
    // land in the same step become one sample rather than a duplicate photo.
    let before = values.len();
    values.dedup();
    note_dedup(before, values.len(), adjustments);
    note_cap(
        i128::from(points),
        &values,
        limit,
        cap,
        points > limit,
        adjustments,
    );
    values
}

/// The caller's own values, clamped into range, aligned to the step, deduplicated and
/// capped.
///
/// Clamped rather than dropped: the driver would clamp the write anyway \[PF:6\], and a
/// sample recorded at the clamped value is true where a dropped value is a hole nobody
/// can see. Every change is reported as a [`SweepAdjustment`].
fn explicit(
    min: i64,
    max: i64,
    step: i64,
    requested: &[i64],
    limit: u32,
    cap: SampleCap,
    adjustments: &mut Vec<SweepAdjustment>,
) -> Vec<i64> {
    let mut seen = BTreeSet::new();
    let mut values = Vec::new();
    for &value in requested {
        let clamped = value.clamp(min, max);
        if clamped != value {
            adjustments.push(SweepAdjustment::Clamped {
                requested: value,
                planned: clamped,
            });
        }
        let aligned = align(min, max, step, i128::from(clamped));
        if aligned != clamped {
            adjustments.push(SweepAdjustment::StepAligned {
                requested: clamped,
                planned: aligned,
            });
        }
        if seen.insert(aligned) {
            values.push(aligned);
        }
    }
    note_dedup(requested.len(), values.len(), adjustments);

    let before = values.len();
    let values = subsample(values, usize::try_from(limit).unwrap_or(usize::MAX));
    note_cap(
        i128::try_from(before).unwrap_or(i128::MAX),
        &values,
        limit,
        cap,
        before > values.len(),
        adjustments,
    );
    values
}

/// Keep `limit` values spread across the list, endpoints included.
///
/// Even subsampling rather than truncation, for the same reason a capped stride widens:
/// the caller asked about the whole list, and its first `limit` entries are a different
/// question.
fn subsample(values: Vec<i64>, limit: usize) -> Vec<i64> {
    let count = values.len();
    if count <= limit || limit == 0 {
        return values;
    }
    let mut out = Vec::with_capacity(limit);
    for index in 0..limit {
        // `limit` is at most a few hundred and `count` is a `Vec` length, so the product
        // cannot overflow a `usize` on a machine that allocated the `Vec`.
        let picked = if limit == 1 {
            0
        } else {
            index * (count - 1) / (limit - 1)
        };
        if let Some(value) = values.get(picked) {
            out.push(*value);
        }
    }
    out.dedup();
    out
}

/// `value`, clamped to `[min, max]` and floored to `min + k × step`.
fn align(min: i64, max: i64, step: i64, value: i128) -> i64 {
    let step = i128::from(step.max(1));
    let clamped = value.clamp(i128::from(min), i128::from(max));
    let offset = clamped - i128::from(min);
    // `offset` is non-negative after the clamp, so this floors rather than truncating
    // toward zero — the distinction that would otherwise put a negative-range sweep one
    // step above where it belongs.
    let aligned = i128::from(min) + offset.div_euclid(step) * step;
    i64::try_from(aligned.clamp(i128::from(min), i128::from(max))).unwrap_or(min)
}

/// The smallest positive gap between consecutive values.
fn precision_of(values: &[i64], fallback: i64) -> i64 {
    values
        .windows(2)
        .filter_map(|window| {
            let first = window.first()?;
            let second = window.get(1)?;
            let gap = (i128::from(*second) - i128::from(*first)).abs();
            i64::try_from(gap).ok().filter(|gap| *gap > 0)
        })
        .min()
        .unwrap_or(fallback)
}

fn note_dedup(before: usize, after: usize, adjustments: &mut Vec<SweepAdjustment>) {
    if before > after {
        adjustments.push(SweepAdjustment::Deduplicated {
            dropped: before - after,
        });
    }
}

fn note_cap(
    requested: i128,
    values: &[i64],
    limit: u32,
    cap: SampleCap,
    fired: bool,
    adjustments: &mut Vec<SweepAdjustment>,
) {
    if !fired {
        return;
    }
    adjustments.push(SweepAdjustment::Capped {
        requested: u64::try_from(requested).unwrap_or(u64::MAX),
        planned: u32::try_from(values.len()).unwrap_or(u32::MAX),
        limit,
        cap,
    });
}

/// `i128` → `f64` for the geometric spacing.
///
/// Lossy above 2^53, and acceptable here for a stated reason: the result is rounded,
/// aligned to the control's step and clamped into the control's range before it becomes
/// a sample, so precision loss costs a slightly different sample value and can never
/// produce one outside the range. The widest control range on any device seen so far is
/// under 2^20.
fn to_f64(value: i128) -> f64 {
    value as f64
}

/// `f64` → `i64`, saturating.
///
/// Rust's float-to-integer cast saturates at the type's bounds and maps NaN to zero, so
/// this is total. Every result goes straight into [`align`], which clamps it into the
/// control's range, so saturation costs an endpoint sample rather than a bad write.
fn to_i64(value: f64) -> i64 {
    value.round() as i64
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use schema::ErrorKind;
    use schema::control::{ControlFlags, ControlId, ControlRange, ControlValue, MenuItem};

    use super::*;

    fn control(slug: &str, min: i64, max: i64, step: i64) -> ControlDesc {
        ControlDesc {
            id: ControlId(0x0098_0900),
            name: slug.to_owned(),
            slug: ControlSlug::parse(slug).expect("literal slug"),
            control_type: ControlType::Integer,
            range: ControlRange { min, max, step },
            default: min,
            flags: ControlFlags::from_raw(0),
            menu: BTreeMap::new(),
            elems: 1,
            elem_size: 4,
            dims: Vec::new(),
            current: Some(ControlValue::Int(min)),
        }
    }

    #[test]
    fn a_step_of_seven_produces_only_multiples_of_seven_from_the_minimum() {
        let desc = control("brightness", 0, 100, 7);
        let planned = plan(&desc, &SweepSpec::All, false).expect("a plannable sweep");
        assert_eq!(
            planned.values,
            vec![0, 7, 14, 21, 28, 35, 42, 49, 56, 63, 70, 77, 84, 91, 98]
        );
        assert_eq!(planned.precision, 7);
        assert!(planned.adjustments.is_empty());
        // The bug this catches: appending `max` as a final value even when it is off the
        // step grid, so the last sample is written at 100 and applied at 98.
        assert!(!planned.values.contains(&100));
    }

    #[test]
    fn a_negative_range_aligns_from_its_minimum_and_not_from_zero() {
        // Truncating division would put every value below zero one step high.
        let desc = control("zoom_continuous", -100, 100, 30);
        let planned = plan(&desc, &SweepSpec::All, false).expect("a plannable sweep");
        assert_eq!(planned.values, vec![-100, -70, -40, -10, 20, 50, 80]);
    }

    #[test]
    fn a_zero_step_does_not_divide_by_zero() {
        // Real shape: plenty of controls report step 0, and `effective_step` is the one
        // place that decides what to do about it.
        let desc = control("brightness", 0, 4, 0);
        let planned = plan(&desc, &SweepSpec::All, false).expect("a plannable sweep");
        assert_eq!(planned.values, vec![0, 1, 2, 3, 4]);
        assert_eq!(planned.precision, 1);
    }

    #[test]
    fn a_single_value_range_is_a_one_sample_sweep_not_an_error() {
        let desc = control("brightness", 5, 5, 1);
        let planned = plan(&desc, &SweepSpec::All, false).expect("min == max is a real range");
        assert_eq!(planned.values, vec![5]);
        assert_eq!(planned.precision, 1, "the fallback for a single-value plan");
    }

    #[test]
    fn a_range_whose_maximum_is_below_its_minimum_is_refused() {
        let desc = control("brightness", 10, 0, 1);
        let err = plan(&desc, &SweepSpec::All, false).expect_err("backwards range");
        assert_eq!(err.kind(), ErrorKind::IllegalTransition);
        assert!(
            err.to_string().contains("empty_range(min=10,max=0)"),
            "{err}"
        );
    }

    #[test]
    fn a_compound_control_is_refused_by_name_rather_than_swept_as_bytes() {
        // PF:1's control. It enumerates, displays and round-trips; it has no ordered
        // range, so it does not sweep.
        let mut desc = control("region_of_interest_rectangle", 0, 0, 0);
        desc.control_type = ControlType::Rect;
        let err = plan(&desc, &SweepSpec::All, false).expect_err("no ordered range");
        assert!(err.to_string().contains("not_sweepable(rect)"), "{err}");

        let mut unknown = desc;
        unknown.control_type = ControlType::Unknown { raw: 0x0fff };
        let err = plan(&unknown, &SweepSpec::All, false).expect_err("no ordered range");
        assert!(
            err.to_string().contains("not_sweepable(unknown(0x0fff))"),
            "{err}"
        );
    }

    #[test]
    fn a_menu_control_sweeps_because_its_indices_are_ordered() {
        // The inverse of the test above: `is_scalar` must not refuse everything.
        let mut desc = control("power_line_frequency", 0, 2, 1);
        desc.control_type = ControlType::Menu;
        desc.menu = BTreeMap::from([(
            0,
            MenuItem::Name {
                name: "Disabled".to_owned(),
            },
        )]);
        let planned = plan(&desc, &SweepSpec::All, false).expect("menus sweep");
        assert_eq!(planned.values, vec![0, 1, 2]);
    }

    #[test]
    fn the_total_cap_fires_and_widens_the_stride_instead_of_truncating() {
        let desc = control("exposure_time_absolute", 0, 10_000, 1);
        let planned = plan(&desc, &SweepSpec::All, false).expect("a plannable sweep");
        assert!(
            planned.total() <= limits::MAX_SWEEP_SAMPLES,
            "{} samples",
            planned.total()
        );
        assert_eq!(planned.values.first(), Some(&0));
        // Spanning the range is the point: a truncating cap would stop at 255.
        assert!(
            planned.values.last().is_some_and(|last| *last > 9_000),
            "{:?}",
            planned.values.last()
        );
        assert!(
            planned.adjustments.iter().any(|adjustment| matches!(
                adjustment,
                SweepAdjustment::Capped {
                    cap: SampleCap::Total,
                    limit,
                    ..
                } if *limit == limits::MAX_SWEEP_SAMPLES
            )),
            "{:?}",
            planned.adjustments
        );
    }

    #[test]
    fn a_motion_control_needs_permission_and_then_gets_the_smaller_cap() {
        // Design §5: motors wear, and a sweep never moves them as an implicit default.
        let desc = control("pan_absolute", -468_000, 468_000, 3_600);
        let err = plan(&desc, &SweepSpec::All, false).expect_err("motors are opt-in");
        assert!(
            err.to_string().contains("motion(allow_motion=false)"),
            "{err}"
        );

        let permitted = plan(&desc, &SweepSpec::All, true).expect("permitted");
        assert!(
            permitted.total() <= limits::MAX_MOTION_SWEEP_SAMPLES,
            "{} samples",
            permitted.total()
        );
        assert!(permitted.adjustments.iter().any(|adjustment| matches!(
            adjustment,
            SweepAdjustment::Capped {
                cap: SampleCap::Motion,
                ..
            }
        )));

        // The inverse: an equally wide control that does not move motors keeps the
        // larger cap, so the motion cap is not simply the cap.
        let still = control("exposure_time_absolute", -468_000, 468_000, 3_600);
        let still_plan = plan(&still, &SweepSpec::All, false).expect("a plannable sweep");
        assert!(still_plan.total() > limits::MAX_MOTION_SWEEP_SAMPLES);
    }

    #[test]
    fn the_motion_heuristic_reads_a_prefix_and_not_a_substring() {
        for slug in ["pan_absolute", "tilt_speed", "pan", "tilt"] {
            assert!(
                is_motion_control(&ControlSlug::parse(slug).expect("literal slug")),
                "{slug}"
            );
        }
        for slug in ["panel_backlight", "brightness", "zoom_absolute", "tilting"] {
            assert!(
                !is_motion_control(&ControlSlug::parse(slug).expect("literal slug")),
                "{slug}"
            );
        }
    }

    #[test]
    fn a_uniform_step_rounds_up_to_a_whole_number_of_device_steps() {
        // Asking for 10 on a step-7 control samples every 14, not every 10: a write at
        // 10 would be aligned to 7 by the driver and recorded against a value the camera
        // never held [PF:6].
        let desc = control("brightness", 0, 100, 7);
        let coarse = plan(&desc, &SweepSpec::Uniform { step: 10 }, false).expect("plannable");
        assert_eq!(coarse.values, vec![0, 14, 28, 42, 56, 70, 84, 98]);
        assert_eq!(coarse.precision, 14);

        // A requested step finer than the device's own is raised to it, not honored.
        let fine = plan(&desc, &SweepSpec::Uniform { step: 1 }, false).expect("plannable");
        assert_eq!(fine.precision, 7);

        // A nonsense step becomes the device's step, not a division by zero or a loop
        // that never advances.
        let zero = plan(&desc, &SweepSpec::Uniform { step: 0 }, false).expect("plannable");
        assert_eq!(zero.precision, 7);
        let negative = plan(&desc, &SweepSpec::Uniform { step: -5 }, false).expect("plannable");
        assert_eq!(negative.precision, 7);
    }

    #[test]
    fn a_log_sweep_over_a_range_containing_zero_produces_no_nan_and_no_duplicates() {
        // The bug: `ln(min)` on a range starting at -100 is NaN, NaN rounds to 0, and
        // the plan becomes a list of identical values nobody notices until the photos
        // all look the same.
        let desc = control("zoom_continuous", -100, 100, 1);
        let planned = plan(&desc, &SweepSpec::Log { points: 12 }, false).expect("plannable");
        assert!(!planned.values.is_empty());
        for window in planned.values.windows(2) {
            let first = window.first().copied().unwrap_or_default();
            let second = window.get(1).copied().unwrap_or_default();
            assert!(
                second > first,
                "{:?} is not strictly increasing",
                planned.values
            );
        }
        assert_eq!(planned.values.first(), Some(&-100));
        assert!(planned.values.iter().all(|v| (-100..=100).contains(v)));
    }

    #[test]
    fn a_log_sweep_front_loads_its_samples() {
        // What "log" is for: exposure time's useful range spans orders of magnitude, so
        // the first half of the samples must cover a small fraction of the range.
        let desc = control("exposure_time_absolute", 1, 10_000, 1);
        let logarithmic = plan(&desc, &SweepSpec::Log { points: 9 }, false).expect("plannable");
        assert_eq!(logarithmic.values.first(), Some(&1));
        assert_eq!(logarithmic.values.last(), Some(&10_000));
        let midpoint = logarithmic.values.get(4).copied().expect("nine points");
        assert!(
            midpoint < 1_000,
            "log spacing collapsed to uniform: {midpoint}"
        );

        // The inverse: a uniform sweep of the same width puts its midpoint in the middle.
        let uniform = plan(&desc, &SweepSpec::Uniform { step: 1_250 }, false).expect("plannable");
        let uniform_mid = uniform.values.get(4).copied().expect("eight points");
        assert!(uniform_mid > 1_000, "{uniform_mid}");
    }

    #[test]
    fn a_log_sweep_of_zero_points_is_a_typed_refusal_not_an_empty_plan() {
        let desc = control("exposure_time_absolute", 1, 10_000, 1);
        let err = plan(&desc, &SweepSpec::Log { points: 0 }, false).expect_err("no points");
        assert!(err.to_string().contains("no_values(log points=0)"), "{err}");

        // The inverse: one point is a real, if minimal, sweep.
        let one = plan(&desc, &SweepSpec::Log { points: 1 }, false).expect("plannable");
        assert_eq!(one.values, vec![1]);
    }

    #[test]
    fn a_log_sweep_is_capped_like_every_other() {
        let desc = control("exposure_time_absolute", 1, 1_000_000, 1);
        let planned = plan(&desc, &SweepSpec::Log { points: 5_000 }, false).expect("plannable");
        assert!(planned.total() <= limits::MAX_SWEEP_SAMPLES);
        assert!(planned.adjustments.iter().any(|adjustment| matches!(
            adjustment,
            SweepAdjustment::Capped {
                cap: SampleCap::Total,
                ..
            }
        )));
    }

    #[test]
    fn explicit_values_are_clamped_and_aligned_with_every_change_reported() {
        let desc = control("brightness", 0, 100, 7);
        let planned = plan(
            &desc,
            &SweepSpec::Explicit {
                values: vec![-20, 10, 99, 400],
            },
            false,
        )
        .expect("plannable");
        assert_eq!(planned.values, vec![0, 7, 98]);
        assert!(
            planned.adjustments.contains(&SweepAdjustment::Clamped {
                requested: -20,
                planned: 0
            }),
            "{:?}",
            planned.adjustments
        );
        assert!(
            planned.adjustments.contains(&SweepAdjustment::StepAligned {
                requested: 10,
                planned: 7
            }),
            "{:?}",
            planned.adjustments
        );
        // 99 aligns to 98, and 400 clamps to 100 which aligns to 98 as well — one
        // sample, and the collapse is reported rather than left for the caller to count.
        assert!(
            planned.adjustments.iter().any(|adjustment| matches!(
                adjustment,
                SweepAdjustment::Deduplicated { dropped: 1 }
            )),
            "{:?}",
            planned.adjustments
        );
    }

    #[test]
    fn an_empty_explicit_list_is_a_typed_refusal_not_an_empty_plan() {
        let desc = control("brightness", 0, 100, 1);
        let err = plan(&desc, &SweepSpec::Explicit { values: Vec::new() }, false)
            .expect_err("nothing to sweep");
        assert!(err.to_string().contains("no_values(explicit)"), "{err}");
    }

    #[test]
    fn a_long_explicit_list_is_subsampled_across_its_whole_extent() {
        let desc = control("brightness", 0, 100_000, 1);
        let requested: Vec<i64> = (0..1_000).map(|value| value * 100).collect();
        let planned =
            plan(&desc, &SweepSpec::Explicit { values: requested }, false).expect("plannable");
        assert!(planned.total() <= limits::MAX_SWEEP_SAMPLES);
        assert_eq!(planned.values.first(), Some(&0));
        assert_eq!(
            planned.values.last(),
            Some(&99_900),
            "subsampling must keep the endpoints"
        );
    }

    #[test]
    fn every_spec_variant_produces_a_non_empty_aligned_plan_or_a_typed_refusal() {
        // The population is every `SweepSpec` shape, and the exhaustive match in `plan`
        // is what the compiler breaks when a fifth arrives (rubric rule 6).
        let desc = control("brightness", 0, 100, 7);
        let specs = [
            SweepSpec::All,
            SweepSpec::Uniform { step: 20 },
            SweepSpec::Log { points: 6 },
            SweepSpec::Explicit {
                values: vec![0, 50, 100],
            },
        ];
        for spec in specs {
            let planned = plan(&desc, &spec, false).unwrap_or_else(|err| panic!("{spec:?}: {err}"));
            assert!(!planned.values.is_empty(), "{spec:?} planned nothing");
            assert!(
                planned
                    .values
                    .iter()
                    .all(|value| (0..=100).contains(value) && value % 7 == 0),
                "{spec:?} left the grid: {:?}",
                planned.values
            );
            assert!(planned.precision > 0, "{spec:?}");
            assert_eq!(usize::try_from(planned.total()), Ok(planned.values.len()));
            assert_eq!(planned.control, desc.slug);
        }
    }

    #[test]
    fn control_type_names_are_distinct_across_the_whole_vocabulary() {
        // The population is every named type plus the open-ended one; the match in
        // `control_type_name` is what the compiler breaks when a variant is added.
        let types = [
            ControlType::Integer,
            ControlType::Boolean,
            ControlType::Menu,
            ControlType::Button,
            ControlType::Integer64,
            ControlType::ControlClass,
            ControlType::String,
            ControlType::Bitmask,
            ControlType::IntegerMenu,
            ControlType::U8,
            ControlType::U16,
            ControlType::U32,
            ControlType::Area,
            ControlType::Rect,
            ControlType::Unknown { raw: 0x0fff },
        ];
        let mut seen = BTreeSet::new();
        for control_type in types {
            let name = control_type_name(control_type);
            assert!(!name.is_empty());
            assert!(seen.insert(name.clone()), "{name} is used twice");
        }
    }
}
