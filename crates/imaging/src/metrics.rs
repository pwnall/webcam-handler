//! The D8 metric set, as pure functions over a luma image.
//!
//! Metrics **rank**; they do not **decide** — `webcam-handler-schema::metrics` says why,
//! and this module is the arithmetic behind those names. Nothing here knows what a
//! calibration task is; a Laplacian variance is a number, and the session record names
//! whoever chose to act on it.
//!
//! ## Why the map is built from `MetricName::ALL`
//!
//! A sample's metrics are persisted as `BTreeMap<MetricName, f64>` and a missing key
//! reads as "not measured", which is indistinguishable from "measured as zero" to
//! everything downstream. [`measure_all`] therefore walks the schema's generated
//! vocabulary and dispatches through one exhaustive `match` ([`measure`]): a metric
//! added to the vocabulary stops the build here until it has an implementation, rather
//! than quietly going missing from every sample (rubric rule 6).
//!
//! ## Comparability
//!
//! Every value is scale-free where it can be — fractions and normalized luma live in
//! `0.0..=1.0` — because a sweep compares samples to each other. [`sharpness`] is the
//! exception: a Laplacian variance has units, and only its *ordering* within one sweep
//! at one resolution means anything (rubric C: assert orderings, never pixel content).

use std::collections::BTreeMap;

use image::GrayImage;
use schema::metrics::MetricName;

/// A sample at or above this counts as a blown highlight.
///
/// Not 255: sensors and JPEG rounding put a clipped highlight anywhere in the last few
/// codes, and a threshold of exactly 255 would report a visibly blown frame as clean.
pub const HIGHLIGHT_CLIP_FLOOR: u8 = 250;

/// A sample at or below this counts as a crushed shadow, for the same reason.
pub const SHADOW_CLIP_CEILING: u8 = 5;

/// The smallest axis a Laplacian can be computed over.
///
/// The kernel is 3×3. On anything narrower the result would be entirely border
/// extension — a number with no image in it — so [`sharpness`] reports `0.0` and says so
/// rather than inventing a reading.
pub const MIN_LAPLACIAN_EXTENT: u32 = 3;

/// Compute one metric.
///
/// The exhaustive `match` that anchors [`measure_all`]'s completeness.
#[must_use]
pub fn measure(name: MetricName, luma: &GrayImage) -> f64 {
    match name {
        MetricName::Sharpness => sharpness(luma),
        MetricName::ClippedHighlights => clipped_highlights(luma),
        MetricName::ClippedShadows => clipped_shadows(luma),
        MetricName::MeanLuma => mean_luma(luma),
        MetricName::RmsContrast => rms_contrast(luma),
    }
}

/// Compute every metric in the D8 set.
///
/// The population is `MetricName::ALL`, which the schema's vocabulary macro generates
/// from the enum — so this map cannot silently shrink when a metric is added.
#[must_use]
pub fn measure_all(luma: &GrayImage) -> BTreeMap<MetricName, f64> {
    MetricName::ALL
        .iter()
        .map(|&name| (name, measure(name, luma)))
        .collect()
}

/// Variance of the Laplacian: higher is sharper.
///
/// The standard focus measure. A 3×3 Laplacian is a high-pass filter, and defocus is a
/// low-pass one, so the variance of the response falls as focus is lost — which is the
/// whole reason a focus sweep can be scored without knowing what it is looking at.
///
/// Returns `0.0` for an image smaller than the kernel (see [`MIN_LAPLACIAN_EXTENT`]) and
/// for an empty one.
#[must_use]
pub fn sharpness(luma: &GrayImage) -> f64 {
    if luma.width() < MIN_LAPLACIAN_EXTENT || luma.height() < MIN_LAPLACIAN_EXTENT {
        return 0.0;
    }
    let response = imageproc::filter::laplacian_filter(luma);
    let samples = response.as_raw();
    let count = samples.len();
    if count == 0 {
        return 0.0;
    }
    let n = widen(count);
    let mean = samples.iter().map(|v| f64::from(*v)).sum::<f64>() / n;
    samples
        .iter()
        .map(|v| {
            let deviation = f64::from(*v) - mean;
            deviation * deviation
        })
        .sum::<f64>()
        / n
}

/// The fraction of samples at or above [`HIGHLIGHT_CLIP_FLOOR`].
#[must_use]
pub fn clipped_highlights(luma: &GrayImage) -> f64 {
    fraction(luma, |value| value >= HIGHLIGHT_CLIP_FLOOR)
}

/// The fraction of samples at or below [`SHADOW_CLIP_CEILING`].
#[must_use]
pub fn clipped_shadows(luma: &GrayImage) -> f64 {
    fraction(luma, |value| value <= SHADOW_CLIP_CEILING)
}

/// Mean luma, normalized to `0.0..=1.0`.
///
/// Undirected on purpose: neither brighter nor darker is better, and
/// `MetricName::MeanLuma`'s `Preference::Undirected` is what stops a selector from
/// pretending otherwise.
#[must_use]
pub fn mean_luma(luma: &GrayImage) -> f64 {
    let samples = luma.as_raw();
    if samples.is_empty() {
        return 0.0;
    }
    let total: u64 = samples.iter().map(|v| u64::from(*v)).sum();
    widen_u64(total) / (widen(samples.len()) * 255.0)
}

/// Root-mean-square contrast on normalized luma.
///
/// The standard deviation of the image, in other words — zero for a flat field, largest
/// for a half-black half-white one.
#[must_use]
pub fn rms_contrast(luma: &GrayImage) -> f64 {
    let samples = luma.as_raw();
    if samples.is_empty() {
        return 0.0;
    }
    let mean = mean_luma(luma);
    let sum_squares: f64 = samples
        .iter()
        .map(|v| {
            let deviation = f64::from(*v) / 255.0 - mean;
            deviation * deviation
        })
        .sum();
    (sum_squares / widen(samples.len())).sqrt()
}

fn fraction(luma: &GrayImage, predicate: impl Fn(u8) -> bool) -> f64 {
    let samples = luma.as_raw();
    if samples.is_empty() {
        return 0.0;
    }
    let hits = samples.iter().filter(|v| predicate(**v)).count();
    widen(hits) / widen(samples.len())
}

/// Widen a count to `f64`.
///
/// One helper carrying one suppression (rubric B11): every value reaching it is a pixel
/// count or a sum of 8-bit samples, both far below 2^53 where `f64` is exact, and
/// `usize`/`u64` have no lossless `Into<f64>` to use instead.
#[expect(
    clippy::as_conversions,
    reason = "no lossless usize->f64 conversion exists; every input here is a pixel count, far below 2^53"
)]
fn widen(count: usize) -> f64 {
    count as f64
}

#[expect(
    clippy::as_conversions,
    reason = "no lossless u64->f64 conversion exists; every input here is a sum of 8-bit samples over a pixel count, far below 2^53"
)]
fn widen_u64(total: u64) -> f64 {
    total as f64
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use super::*;
    use crate::fixtures;
    use schema::metrics::Preference;

    /// A pair of images whose ranking on `metric` is not in question, `(better, worse)`
    /// in the direction `metric.preference()` names.
    ///
    /// Exhaustive over the vocabulary, so a metric added to the schema cannot reach
    /// `measure` without a fixture pair that pins which way it points — which is the
    /// failure mode that matters: an inverted metric is green everywhere until a sweep
    /// picks the worst sample in the set.
    fn ordered_fixtures(metric: MetricName) -> Option<(GrayImage, GrayImage)> {
        let sharp = fixtures::text_like(96, 72);
        let base = fixtures::gradient(96, 72);
        match metric {
            MetricName::Sharpness => Some((
                sharp.clone(),
                fixtures::blurred(&sharp, 3.0).expect("3.0 is a positive sigma"),
            )),
            MetricName::ClippedHighlights => Some((base.clone(), fixtures::overexposed(&base))),
            MetricName::ClippedShadows => Some((base.clone(), fixtures::underexposed(&base))),
            // Undirected: no pair can be "better", which is exactly what the schema says
            // about mean luma.
            MetricName::MeanLuma => None,
            MetricName::RmsContrast => Some((
                fixtures::checkerboard(96, 72, 8),
                fixtures::flat(96, 72, 128),
            )),
        }
    }

    #[test]
    fn every_metric_points_the_way_the_schema_says_it_does() {
        let mut directed = 0usize;
        for &metric in MetricName::ALL {
            match (metric.preference(), ordered_fixtures(metric)) {
                (Preference::Undirected, None) => {}
                (Preference::HigherIsBetter, Some((better, worse))) => {
                    directed += 1;
                    let (b, w) = (measure(metric, &better), measure(metric, &worse));
                    // `partial_cmp` rather than `>`: `Some(Greater)` rules out a NaN
                    // score, which `>` would quietly report as "not better" instead of
                    // as the broken metric it is.
                    assert_eq!(
                        b.partial_cmp(&w),
                        Some(Ordering::Greater),
                        "{metric} scored {b} on the better image and {w} on the worse one"
                    );
                }
                (Preference::LowerIsBetter, Some((better, worse))) => {
                    directed += 1;
                    let (b, w) = (measure(metric, &better), measure(metric, &worse));
                    assert_eq!(
                        b.partial_cmp(&w),
                        Some(Ordering::Less),
                        "{metric} scored {b} on the better image and {w} on the worse one"
                    );
                }
                (preference, fixtures) => panic!(
                    "{metric} declares {preference:?} but its fixture pair is {:?}",
                    fixtures.map(|_| "present")
                ),
            }
        }
        // Non-vacuity: an `ordered_fixtures` that returned `None` for everything would
        // otherwise walk the whole vocabulary asserting nothing.
        assert_eq!(directed + 1, MetricName::ALL.len());
    }

    #[test]
    fn measure_all_answers_for_every_name_in_the_vocabulary() {
        let scores = measure_all(&fixtures::text_like(64, 48));
        assert_eq!(scores.len(), MetricName::ALL.len());
        for &metric in MetricName::ALL {
            let value = scores.get(&metric).copied();
            assert!(
                value.is_some(),
                "{metric} is missing from the sample record"
            );
            assert!(
                value.is_some_and(f64::is_finite),
                "{metric} produced a non-finite score"
            );
        }
    }

    #[test]
    fn a_blurred_pattern_scores_below_its_sharp_original() {
        // The focus sweep's whole premise. Asserted on a text-like fixture because a
        // blur of a linear ramp is the same ramp, which would make this vacuous.
        let sharp = fixtures::text_like(96, 72);
        let blurry = fixtures::blurred(&sharp, 2.0).expect("2.0 is a positive sigma");
        let sharp_score = sharpness(&sharp);
        let blurry_score = sharpness(&blurry);
        assert_eq!(
            sharp_score.partial_cmp(&blurry_score),
            Some(Ordering::Greater),
            "{sharp_score} vs {blurry_score}"
        );
        // And the gap is a gap, not float noise.
        assert!(sharp_score > blurry_score * 2.0);
    }

    #[test]
    fn an_inverted_sharpness_metric_fails_the_ordering_the_real_one_passes() {
        // Rubric rule 2: construct the buggy implementation and watch it fail. If a
        // sign flip in `sharpness` could still satisfy the test above, the test above
        // is not testing the direction.
        let sharp = fixtures::text_like(96, 72);
        let blurry = fixtures::blurred(&sharp, 2.0).expect("2.0 is a positive sigma");
        let inverted = |image: &GrayImage| -sharpness(image);
        assert_eq!(
            inverted(&sharp).partial_cmp(&inverted(&blurry)),
            Some(Ordering::Less),
            "the inverted metric ranked the sharp image above the blurred one, so the \
             ordering assertions above are not testing direction"
        );
    }

    #[test]
    fn an_overexposed_image_scores_above_a_correct_one_on_clipped_highlights() {
        let base = fixtures::gradient(96, 72);
        let blown = fixtures::overexposed(&base);
        let base_score = clipped_highlights(&base);
        let blown_score = clipped_highlights(&blown);
        assert_eq!(
            blown_score.partial_cmp(&base_score),
            Some(Ordering::Greater),
            "{blown_score} vs {base_score}"
        );
        // The clip metrics do not answer each other's question: blowing the highlights
        // must not also report crushed shadows.
        assert!(clipped_shadows(&blown) <= clipped_shadows(&base));
    }

    #[test]
    fn an_underexposed_image_scores_above_a_correct_one_on_clipped_shadows() {
        let base = fixtures::gradient(96, 72);
        let crushed = fixtures::underexposed(&base);
        let base_score = clipped_shadows(&base);
        let crushed_score = clipped_shadows(&crushed);
        assert_eq!(
            crushed_score.partial_cmp(&base_score),
            Some(Ordering::Greater),
            "{crushed_score} vs {base_score}"
        );
        assert!(clipped_highlights(&crushed) <= clipped_highlights(&base));
    }

    #[test]
    fn the_clip_fractions_are_fractions() {
        let white = fixtures::flat(16, 16, 255);
        assert!((clipped_highlights(&white) - 1.0).abs() < f64::EPSILON);
        assert!(clipped_shadows(&white).abs() < f64::EPSILON);
        let black = fixtures::flat(16, 16, 0);
        assert!((clipped_shadows(&black) - 1.0).abs() < f64::EPSILON);
        assert!(clipped_highlights(&black).abs() < f64::EPSILON);
    }

    #[test]
    fn mean_luma_is_normalized_and_ordered_by_exposure() {
        let mid = fixtures::flat(8, 8, 128);
        assert!((mean_luma(&mid) - 128.0 / 255.0).abs() < 1e-12);
        assert!(mean_luma(&fixtures::flat(8, 8, 0)).abs() < f64::EPSILON);
        assert!((mean_luma(&fixtures::flat(8, 8, 255)) - 1.0).abs() < f64::EPSILON);

        let base = fixtures::gradient(64, 64);
        assert!(mean_luma(&fixtures::overexposed(&base)) > mean_luma(&base));
        assert!(mean_luma(&fixtures::underexposed(&base)) < mean_luma(&base));
    }

    #[test]
    fn rms_contrast_is_zero_on_a_flat_field_and_maximal_on_a_hard_split() {
        assert!(rms_contrast(&fixtures::flat(16, 16, 200)).abs() < 1e-12);
        // A half-black half-white field has a standard deviation of exactly 0.5.
        let split = fixtures::checkerboard(16, 16, 1);
        assert!(
            (rms_contrast(&split) - 0.5).abs() < 1e-9,
            "{}",
            rms_contrast(&split)
        );
        assert!(rms_contrast(&fixtures::gradient(64, 64)) < rms_contrast(&split));
    }

    #[test]
    fn a_degenerate_image_yields_zeros_rather_than_a_panic_or_a_nan() {
        // A 0x0 frame should not reach here, but "should not" is not "cannot", and a
        // NaN score would poison every comparison in a sweep.
        let empty = fixtures::flat(0, 0, 0);
        for &metric in MetricName::ALL {
            let value = measure(metric, &empty);
            assert!(
                value.is_finite(),
                "{metric} produced {value} on an empty image"
            );
            assert!(value.abs() < f64::EPSILON, "{metric} produced {value}");
        }
        // Smaller than the Laplacian kernel: reported as zero, not as border artifacts.
        assert!(sharpness(&fixtures::checkerboard(2, 2, 1)).abs() < f64::EPSILON);
        assert!(sharpness(&fixtures::checkerboard(3, 3, 1)) > 0.0);

        // **Either** axis, not both. The square fixtures above are answered the same way
        // by `width < 3 || height < 3` and by `width < 3 && height < 3`, so P3f's mutation
        // run swapped the connective and nothing went red — a 2×64 strip would then have
        // been "measured" out of two rows of border extension.
        assert!(sharpness(&fixtures::checkerboard(2, 64, 1)).abs() < f64::EPSILON);
        assert!(sharpness(&fixtures::checkerboard(64, 2, 1)).abs() < f64::EPSILON);
        assert!(sharpness(&fixtures::checkerboard(64, 64, 1)) > 0.0);
    }

    #[test]
    fn the_laplacian_response_sums_to_zero_whatever_the_image_is() {
        // Recorded here because three surviving mutants rest on it (note N26): the mean of
        // the response is what `sharpness` subtracts, and if that mean is always exactly
        // zero then `sum / n`, `sum * n`, `sum % n` and `v - mean` versus `v + mean` are
        // all the same arithmetic. They are, and this is the fixture that says so — and
        // that turns red the day the filter's border handling changes, at which point
        // those three acceptances become killable and must be deleted.
        //
        // The population is chosen to be the hardest case for the claim: a single bright
        // pixel at each of the nine positions of a 3×3, corners and edges included, so the
        // border replication is asked the question from every direction.
        for y in 0..MIN_LAPLACIAN_EXTENT {
            for x in 0..MIN_LAPLACIAN_EXTENT {
                let mut image = GrayImage::new(MIN_LAPLACIAN_EXTENT, MIN_LAPLACIAN_EXTENT);
                image.put_pixel(x, y, image::Luma([u8::MAX]));
                let response = imageproc::filter::laplacian_filter(&image);
                let sum: i64 = response.as_raw().iter().map(|v| i64::from(*v)).sum();
                assert_eq!(sum, 0, "a dot at ({x}, {y}) summed to {sum}");
            }
        }
        // And on the fixtures the rest of this module measures.
        for image in [
            fixtures::text_like(64, 48),
            fixtures::gradient(64, 48),
            fixtures::checkerboard(64, 48, 8),
            fixtures::speckle(64, 48, 7),
        ] {
            let response = imageproc::filter::laplacian_filter(&image);
            let sum: i64 = response.as_raw().iter().map(|v| i64::from(*v)).sum();
            assert_eq!(sum, 0);
        }
    }

    #[test]
    fn sharpness_is_the_variance_of_the_laplacian_and_not_some_neighbouring_moment() {
        // Every other assertion about `sharpness` in this module is an *ordering* — sharp
        // above blurry, and rightly so, because a Laplacian variance has units and only
        // its ordering means anything to a sweep. But an ordering is cheap to satisfy:
        // P3f's mutation run divided the sum of squares by the sample count the wrong way
        // round and every ordering test in this file stayed green, because scaling a
        // metric by n² reorders nothing.
        //
        // So the statistic is stated once, independently: the response comes from the same
        // filter, and the variance is written out here rather than called. That is a second
        // implementation of the arithmetic under test, not a second copy of the code. The
        // three mutations of the *mean* that the same run reported are a different case,
        // and note N26 is where they are argued rather than covered.
        let image = fixtures::text_like(64, 48);
        let response = imageproc::filter::laplacian_filter(&image);
        let values: Vec<f64> = response.as_raw().iter().map(|v| f64::from(*v)).collect();
        let n = f64::from(u32::try_from(values.len()).expect("a fixture this size fits"));
        let mean = values.iter().sum::<f64>() / n;
        let variance = values
            .iter()
            .map(|value| (value - mean) * (value - mean))
            .sum::<f64>()
            / n;

        let measured = sharpness(&image);
        assert!(
            (measured - variance).abs() <= variance * 1e-9,
            "{measured} is not the variance {variance} of the Laplacian response"
        );

        // And the buggy implementation, so the equality above is known to be pinning
        // something. The mean is zero (the test above says why), so the arithmetic this
        // fixture *can* distinguish is the divisor: a sum of squares multiplied by the
        // sample count instead of divided by it is the same ordering and a different
        // number, which is exactly what an ordering test cannot see.
        let with_the_wrong_divisor = values.iter().map(|value| value * value).sum::<f64>() * n;
        assert!(
            with_the_wrong_divisor > variance,
            "{with_the_wrong_divisor} vs {variance}: multiplying by the sample count must \
             not give the same answer as dividing by it, or the assertion above proves \
             nothing"
        );
    }
}
