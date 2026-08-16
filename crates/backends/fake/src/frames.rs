//! Frame synthesis that responds to control values (design T3, §2.9).
//!
//! The fake's frames are not noise with a timestamp. Writing `brightness` moves the
//! luma; writing `focus_absolute` blurs a text-like test pattern, sharpest at one value
//! and monotonically worse either side of it. That is what makes the whole calibration
//! loop — sweep, score, rank, select — testable with no camera in the room (design G3).
//!
//! ## The model, stated so a test can state it independently
//!
//! Two rules, and both are *declarations*, not emergent behaviour:
//!
//! 1. **The focus optimum is the focus control's declared default.** Detune is measured
//!    against the wider side of the control's range, so the blur reaches
//!    [`MAX_BLUR_SIGMA`] exactly at whichever end is further away.
//! 2. **Brightness is a linear gain** from [`BRIGHTNESS_GAIN_AT_MINIMUM_PERCENT`] at the
//!    control's minimum to that plus [`BRIGHTNESS_GAIN_SPAN_PERCENT`] at its maximum.
//!
//! Rule 1 is what keeps a calibration test off docs/8 Part C's "the fake validating the
//! fake" list: the expectation comes from the *profile* — read that control's default —
//! and never from asking the synthesizer where its own peak is.
//!
//! ## Early frames are unsettled \[PF:11\]
//!
//! Frames immediately after `STREAMON` are dark until AE converges, so the first
//! [`SETTLE_FRAMES`] frames ramp up and then hold steady — which is exactly what the
//! default settle policy skips past. Under [`crate::Fault::SettleNeverConverges`] the
//! ramp restarts forever instead of finishing, so a policy waiting for a stable exposure
//! waits until its deadline.

use image::GrayImage;
use imaging::decode::Decoded;
use imaging::fixtures::{blurred, exposure_scaled, pack_grey, pack_nv12, pack_yuyv, text_like};
use schema::camera::PixelFormat;
use schema::control::ControlRange;
use schema::error::{Error, Result};
use schema::limits;

/// The control whose value the focus model reads.
pub const FOCUS_CONTROL_SLUG: &str = "focus_absolute";

/// The control whose value the exposure model reads.
pub const BRIGHTNESS_CONTROL_SLUG: &str = "brightness";

/// The Gaussian sigma applied at maximum detune from the focus optimum.
///
/// Large enough that the sharpness metric separates the extremes by orders of magnitude,
/// which is what makes a ranking test assert an ordering rather than a rounding
/// difference.
pub const MAX_BLUR_SIGMA: f32 = 6.0;

/// The exposure gain, in percent, at the brightness control's minimum.
pub const BRIGHTNESS_GAIN_AT_MINIMUM_PERCENT: u32 = 25;

/// How much the exposure gain grows, in percent, across the brightness control's range.
pub const BRIGHTNESS_GAIN_SPAN_PERCENT: u32 = 175;

/// How many frames the sensor takes to settle after `STREAMON` \[PF:11\].
///
/// The default settle policy skips exactly this many
/// ([`schema::limits::DEFAULT_SETTLE_SKIP_FRAMES`]), so the fake and the policy agree by
/// construction rather than by coincidence.
pub const SETTLE_FRAMES: u32 = limits::DEFAULT_SETTLE_SKIP_FRAMES;

/// The quality the MJPG path encodes at. Below the imaging crate's default, because a
/// synthetic frame is a fixture and not a photograph.
pub const JPEG_QUALITY: u8 = 85;

/// The pixel formats this synthesizer can emit.
///
/// D6's closed source-format set, minus the ones a *generator* has no meaning for. A
/// profile offering anything else has that format refused at `start_stream` with
/// [`schema::Error::FormatUnsupported`] naming this list — the fake does not invent a
/// bitstream it could not also decode.
pub const SYNTHESIZABLE_FORMATS: &[PixelFormat] = &[
    PixelFormat::MJPG,
    PixelFormat::JPEG,
    PixelFormat::YUYV,
    PixelFormat::GREY,
    PixelFormat::NV12,
];

/// Whether the synthesizer can produce this format.
#[must_use]
pub fn can_synthesize(format: PixelFormat) -> bool {
    SYNTHESIZABLE_FORMATS.contains(&format)
}

/// A control's value together with the range it was declared over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Knob {
    pub(crate) value: i64,
    pub(crate) range: ControlRange,
    pub(crate) default: i64,
}

/// Everything the scene depends on.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Scene {
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// The focus control, when the replayed profile has one.
    pub(crate) focus: Option<Knob>,
    /// The brightness control, when the replayed profile has one.
    pub(crate) brightness: Option<Knob>,
    /// How many frames this stream has already delivered.
    pub(crate) frame_index: u32,
    /// Whether [`crate::Fault::SettleNeverConverges`] is in force.
    pub(crate) unsettled: bool,
}

/// The value at which this control's frames are sharpest: its declared default.
///
/// Stated here, once, so the fake's optimum is a property of the model and not a
/// discovery a test has to make by asking the model.
#[must_use]
pub fn focus_optimum(focus: &ControlRange, default: i64) -> i64 {
    default.clamp(focus.min, focus.max)
}

/// Render the scene as a luma image.
pub(crate) fn render(scene: &Scene) -> GrayImage {
    // Text-like, because D8's motivating task is "read text from the DUT display" and a
    // focus sweep has to move something a sharpness metric can see.
    let base = text_like(scene.width.max(1), scene.height.max(1));
    let focused = match scene.focus.and_then(blur_sigma) {
        Some(sigma) => blurred(&base, sigma).unwrap_or(base),
        None => base,
    };
    let (numerator, denominator) = exposure_gain(scene);
    exposure_scaled(&focused, numerator, denominator)
}

/// Encode a rendered scene in the negotiated format.
///
/// # Errors
///
/// [`schema::Error::FormatUnsupported`] for a format outside
/// [`SYNTHESIZABLE_FORMATS`] — which `start_stream` has already refused, so reaching it
/// means the negotiation and the synthesizer disagree.
pub(crate) fn encode(image: &GrayImage, format: PixelFormat) -> Result<Vec<u8>> {
    if format == PixelFormat::MJPG || format == PixelFormat::JPEG {
        // Real JPEG bytes, SOI marker and all: an MJPG frame from this backend goes
        // through the same decode path a camera's would (E6's premise).
        imaging::encode::jpeg(&Decoded::Gray(image.clone()), JPEG_QUALITY)
    } else if format == PixelFormat::YUYV {
        Ok(pack_yuyv(image))
    } else if format == PixelFormat::GREY {
        Ok(pack_grey(image))
    } else if format == PixelFormat::NV12 {
        Ok(pack_nv12(image))
    } else {
        Err(Error::format_unsupported(
            Some(format),
            SYNTHESIZABLE_FORMATS.to_vec(),
        ))
    }
}

/// The blur applied at this focus setting, or `None` at the optimum.
///
/// Integer arithmetic until the last step: the detune is computed in thousandths so the
/// result is identical on every machine, and only then scaled into a sigma.
fn blur_sigma(focus: Knob) -> Option<f32> {
    let optimum = focus_optimum(&focus.range, focus.default);
    let detune = focus.value.saturating_sub(optimum).unsigned_abs();
    // Measured against the wider side, so the blur reaches its maximum at whichever end
    // of the range is further from the optimum and not before.
    let span = optimum
        .saturating_sub(focus.range.min)
        .max(focus.range.max.saturating_sub(optimum))
        .unsigned_abs()
        .max(1);
    let permille = u16::try_from(detune.saturating_mul(1_000) / span)
        .unwrap_or(u16::MAX)
        .min(1_000);
    if permille == 0 {
        return None;
    }
    Some(MAX_BLUR_SIGMA * f32::from(permille) / 1_000.0)
}

/// The exposure gain as a `numerator / denominator` ratio, settle ramp included.
fn exposure_gain(scene: &Scene) -> (u32, u32) {
    let base = match scene.brightness {
        Some(knob) => brightness_gain_percent(knob),
        None => 100,
    };
    let settle_frames = SETTLE_FRAMES.max(1);
    let ramp = if scene.unsettled {
        // Never arrives: the exposure keeps walking the ramp instead of finishing it, so
        // a settle policy waiting for two like frames waits forever [PF:11].
        1 + scene.frame_index % settle_frames
    } else {
        scene.frame_index.saturating_add(1).min(settle_frames)
    };
    (base.saturating_mul(ramp), settle_frames.saturating_mul(100))
}

/// The gain this brightness setting asks for, in percent.
fn brightness_gain_percent(knob: Knob) -> u32 {
    let span = knob.range.max.saturating_sub(knob.range.min);
    if span <= 0 {
        return 100;
    }
    let offset = knob
        .value
        .clamp(knob.range.min, knob.range.max)
        .saturating_sub(knob.range.min);
    let span = u64::try_from(span).unwrap_or(1).max(1);
    let offset = u64::try_from(offset).unwrap_or(0);
    let grown = u64::from(BRIGHTNESS_GAIN_SPAN_PERCENT).saturating_mul(offset) / span;
    u32::try_from(u64::from(BRIGHTNESS_GAIN_AT_MINIMUM_PERCENT).saturating_add(grown))
        .unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use imaging::metrics;

    use super::*;

    fn range(min: i64, max: i64) -> ControlRange {
        ControlRange { min, max, step: 1 }
    }

    fn scene_at(focus: i64) -> Scene {
        Scene {
            width: 160,
            height: 120,
            focus: Some(Knob {
                value: focus,
                range: range(0, 1_023),
                default: 512,
            }),
            brightness: None,
            // Past the settle ramp, so the comparison is about focus and nothing else.
            frame_index: SETTLE_FRAMES,
            unsettled: false,
        }
    }

    #[test]
    fn the_optimum_is_the_declared_default_and_nothing_is_blurred_there() {
        // The independently-stateable half of the model: a calibration test reads the
        // default out of the profile and expects the peak there.
        assert_eq!(focus_optimum(&range(0, 1_023), 512), 512);
        assert_eq!(blur_sigma(scene_at(512).focus.expect("focus knob")), None);
        // An optimum outside its own range is clamped rather than believed [PF:5].
        assert_eq!(focus_optimum(&range(0, 2), 3), 2);
    }

    #[test]
    fn blur_grows_with_distance_from_the_optimum_in_both_directions() {
        let knob = |value| scene_at(value).focus.expect("focus knob");
        let near = blur_sigma(knob(562)).expect("detuned");
        let far = blur_sigma(knob(1_023)).expect("detuned");
        assert!(near < far, "{near} should be gentler than {far}");
        // The same distance the other side of the optimum blurs the same amount.
        let below = blur_sigma(knob(462)).expect("detuned");
        assert!((below - near).abs() < 0.01, "{below} vs {near}");
        // The maximum lands at whichever end is furthest from the optimum — here the
        // bottom, 512 steps away against the top's 511.
        let bottom = blur_sigma(knob(0)).expect("detuned");
        assert!((bottom - MAX_BLUR_SIGMA).abs() < f32::EPSILON, "{bottom}");
        assert!(
            far <= bottom,
            "{far} must not exceed the widest detune {bottom}"
        );
    }

    #[test]
    fn sharpness_peaks_at_the_optimum_and_falls_off_either_side() {
        // The property the calibration loop depends on, asserted on the metric a sweep
        // would actually score with.
        let sharpness = |focus| metrics::sharpness(&render(&scene_at(focus)));
        let peak = sharpness(512);
        for detuned in [0, 256, 768, 1_023] {
            assert!(
                sharpness(detuned) < peak,
                "focus {detuned} scored {} against the optimum's {peak}",
                sharpness(detuned)
            );
        }
        // Monotone on the way out, not just lower at the extremes.
        assert!(sharpness(600) > sharpness(700));
        assert!(sharpness(400) > sharpness(300));
    }

    #[test]
    fn brightness_moves_the_luma_monotonically() {
        let luma = |value| {
            let scene = Scene {
                brightness: Some(Knob {
                    value,
                    range: range(0, 255),
                    default: 128,
                }),
                ..scene_at(512)
            };
            metrics::mean_luma(&render(&scene))
        };
        let dark = luma(0);
        let middle = luma(128);
        let bright = luma(255);
        assert!(dark < middle, "{dark} < {middle}");
        assert!(middle < bright, "{middle} < {bright}");
    }

    #[test]
    fn the_first_frames_are_dark_and_then_the_exposure_holds_still() {
        // PF:11 as a property: the settle ramp exists, converges, and stops moving.
        let at = |frame_index| {
            let scene = Scene {
                frame_index,
                ..scene_at(512)
            };
            metrics::mean_luma(&render(&scene))
        };
        assert!(at(0) < at(SETTLE_FRAMES / 2));
        assert!(at(SETTLE_FRAMES / 2) < at(SETTLE_FRAMES));
        assert!((at(SETTLE_FRAMES) - at(SETTLE_FRAMES * 3)).abs() < f64::EPSILON);
    }

    #[test]
    fn an_unsettled_stream_never_holds_still() {
        // The inverse of the test above, which is the whole point of the fault: the
        // exposure keeps moving no matter how long a policy waits.
        let at = |frame_index| {
            let scene = Scene {
                frame_index,
                unsettled: true,
                ..scene_at(512)
            };
            metrics::mean_luma(&render(&scene))
        };
        // It cycles instead of converging: the same point in the ramp gives the same
        // exposure however long the stream has been running…
        assert!((at(SETTLE_FRAMES) - at(SETTLE_FRAMES * 3)).abs() < f64::EPSILON);
        // …and the very next frame is somewhere else again, forever.
        assert!(at(SETTLE_FRAMES * 3) < at(SETTLE_FRAMES * 3 + 1));
    }

    #[test]
    fn every_synthesizable_format_encodes_and_nothing_else_does() {
        let image = text_like(32, 24);
        for &format in SYNTHESIZABLE_FORMATS {
            assert!(can_synthesize(format));
            let bytes = encode(&image, format).unwrap_or_else(|e| panic!("{format}: {e}"));
            assert!(!bytes.is_empty(), "{format} encoded to nothing");
            if format.is_compressed() {
                assert_eq!(&bytes[..2], &[0xff, 0xd8], "{format} lacks a JPEG SOI");
            }
        }
        // Both directions: a format outside the set is refused, naming what is offered.
        // H264 is the deferred D7 L1 format: real, and not something a generator can emit.
        let unsupported = PixelFormat::parse("H264").expect("a four-character name");
        assert!(!can_synthesize(unsupported));
        assert!(matches!(
            encode(&image, unsupported),
            Err(Error::FormatUnsupported { .. })
        ));
    }
}
