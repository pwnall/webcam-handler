//! Deterministic synthetic images.
//!
//! Three consumers, one generator set: this crate's own metric and codec tests, the fake
//! backend's frame synthesis (§2.3 — the fake's frames respond to control values, and
//! these are what they are made of), and `corpus/images/`.
//!
//! Everything here is **pure and deterministic**: same arguments, same bytes, on every
//! machine and every run. Where randomness would be convenient there is [`Lcg`], a
//! seeded generator written here rather than pulled in, because a fixture that differs
//! between runs turns a metric ordering test into a coin flip.
//!
//! ## Provenance is not optional
//!
//! `corpus/images/` holds generated fixtures only, and the gate that enforces it
//! (docs/9 `no-frame-bytes-in-repo.sh`) tells a synthetic fixture from a camera frame by
//! the `generated-by` marker this module stamps. **Any image file written from these
//! generators goes through [`provenanced_png_gray8`] or [`provenanced_png_rgb8`]** — a
//! bare [`crate::encode::png_gray8`] produces a file the gate must reject, and rightly
//! so, because it cannot tell it from a frame with a person in it.
//!
//! This module writes no files. It returns bytes; the caller owns the sink.

use image::{GrayImage, ImageBuffer, Luma, Rgb, RgbImage};
use schema::error::Result;

use crate::encode::png_with_text;

/// The `tEXt` keyword the corpus provenance gate looks for.
pub const PROVENANCE_KEYWORD: &str = "generated-by";

/// The value that keyword carries: this crate generated the pixels, no camera did.
pub const GENERATED_BY: &str = "webcam-handler-imaging";

/// The `tEXt` keyword naming which generator produced a fixture.
///
/// Not what the gate keys on, but what a human reading `corpus/images/` needs in order
/// to regenerate a fixture rather than guess at it.
pub const FIXTURE_KEYWORD: &str = "fixture";

/// Encode luma as a PNG carrying the corpus provenance marker.
///
/// `fixture` names the generator call that produced the pixels — "checkerboard-64-8",
/// say — so a committed fixture can be regenerated instead of trusted.
///
/// # Errors
///
/// As [`crate::encode::png_gray8`].
pub fn provenanced_png_gray8(image: &GrayImage, fixture: &str) -> Result<Vec<u8>> {
    png_with_text(
        image.as_raw(),
        image.width(),
        image.height(),
        png::ColorType::Grayscale,
        &[
            (PROVENANCE_KEYWORD, GENERATED_BY),
            (FIXTURE_KEYWORD, fixture),
        ],
    )
}

/// Encode RGB as a PNG carrying the corpus provenance marker.
///
/// # Errors
///
/// As [`crate::encode::png_rgb8`].
pub fn provenanced_png_rgb8(image: &RgbImage, fixture: &str) -> Result<Vec<u8>> {
    png_with_text(
        image.as_raw(),
        image.width(),
        image.height(),
        png::ColorType::Rgb,
        &[
            (PROVENANCE_KEYWORD, GENERATED_BY),
            (FIXTURE_KEYWORD, fixture),
        ],
    )
}

/// A square-wave checkerboard: maximum contrast, one dominant spatial frequency.
///
/// The sharpness fixture. `cell` is the square size in pixels; a `cell` of zero has no
/// meaning and is read as one, which is the degenerate checkerboard rather than a
/// division by zero.
#[must_use]
pub fn checkerboard(width: u32, height: u32, cell: u32) -> GrayImage {
    let cell = cell.max(1);
    ImageBuffer::from_fn(width, height, |x, y| {
        let dark = ((x / cell) + (y / cell)).is_multiple_of(2);
        Luma([if dark { 255 } else { 0 }])
    })
}

/// A uniform field.
///
/// The zero-contrast, zero-detail baseline every "more than nothing" assertion needs a
/// floor against.
#[must_use]
pub fn flat(width: u32, height: u32, value: u8) -> GrayImage {
    ImageBuffer::from_pixel(width, height, Luma([value]))
}

/// A horizontal 0→255 ramp.
///
/// The exposure fixture, and deliberately *not* a sharpness fixture: a Gaussian blur of
/// a linear ramp is the same ramp, so a sharp-versus-blurred ordering asserted on this
/// image would be asserting nothing.
#[must_use]
pub fn gradient(width: u32, height: u32) -> GrayImage {
    ImageBuffer::from_fn(width, height, |x, _| Luma([ramp(x, width)]))
}

/// Dark strokes on white at roughly the spatial frequency of small text.
///
/// The fixture that stands in for "text legible on the DUT display" — the task D8 was
/// written for. Its energy sits where a focus sweep moves it, so a blur of this image
/// scores far below the original.
#[must_use]
pub fn text_like(width: u32, height: u32) -> GrayImage {
    ImageBuffer::from_fn(width, height, |x, y| {
        let within_line = y % 12;
        let on_a_glyph_row = within_line < 7;
        let word_gap = (x / 5) % 6 == 5;
        let stroke = x.is_multiple_of(3) || x % 7 == 1;
        let ink = on_a_glyph_row && stroke && !word_gap;
        Luma([if ink { 16 } else { 235 }])
    })
}

/// Eight vertical colour bars, the broadcast order.
///
/// The colour fixture: it exercises the three-channel paths (RGB PNG, colour JPEG,
/// RGB→luma) with something whose luma ordering is obvious by eye.
#[must_use]
pub fn colour_bars(width: u32, height: u32) -> RgbImage {
    ImageBuffer::from_fn(width, height, |x, _| {
        let bar = (x * 8) / width.max(1);
        Rgb(match bar {
            0 => [255, 255, 255],
            1 => [255, 255, 0],
            2 => [0, 255, 255],
            3 => [0, 255, 0],
            4 => [255, 0, 255],
            5 => [255, 0, 0],
            6 => [0, 0, 255],
            _ => [0, 0, 0],
        })
    })
}

/// Uniform noise from a seeded generator.
///
/// Broadband detail with no structure — useful to the fake backend as sensor grain, and
/// useful here as an image whose sharpness is high for a reason unrelated to edges.
#[must_use]
pub fn speckle(width: u32, height: u32, seed: u64) -> GrayImage {
    let mut rng = Lcg::new(seed);
    let mut image = GrayImage::new(width, height);
    for pixel in image.pixels_mut() {
        pixel.0 = [rng.next_u8()];
    }
    image
}

/// Blur an image with a Gaussian kernel.
///
/// `None` when `sigma` is not a positive finite number: `imageproc` panics on that, and
/// this crate does not panic. A caller with a literal sigma unwraps the `Option` at the
/// call site, where the value is visible.
#[must_use]
pub fn blurred(image: &GrayImage, sigma: f32) -> Option<GrayImage> {
    if !sigma.is_finite() || sigma <= 0.0 {
        return None;
    }
    Some(imageproc::filter::gaussian_blur_f32(image, sigma))
}

/// Scale every sample by `numerator / denominator`, clipping at both ends.
///
/// Integer arithmetic, so the result is bit-identical everywhere. A `denominator` of
/// zero is read as one — the only total reading of a degenerate ratio.
#[must_use]
pub fn exposure_scaled(image: &GrayImage, numerator: u32, denominator: u32) -> GrayImage {
    let denominator = denominator.max(1);
    let mut out = GrayImage::new(image.width(), image.height());
    for (destination, source) in out.pixels_mut().zip(image.pixels()) {
        let scaled = u32::from(source.0[0]) * numerator / denominator;
        destination.0 = [u8::try_from(scaled.min(255)).unwrap_or(u8::MAX)];
    }
    out
}

/// The gain [`overexposed`] applies, as `numerator / denominator`.
pub const OVEREXPOSURE_GAIN: (u32, u32) = (5, 2);

/// The gain [`underexposed`] applies.
pub const UNDEREXPOSURE_GAIN: (u32, u32) = (1, 5);

/// An image driven far enough up that a large fraction of it clips white.
#[must_use]
pub fn overexposed(image: &GrayImage) -> GrayImage {
    exposure_scaled(image, OVEREXPOSURE_GAIN.0, OVEREXPOSURE_GAIN.1)
}

/// An image driven far enough down that a large fraction of it clips black.
#[must_use]
pub fn underexposed(image: &GrayImage) -> GrayImage {
    exposure_scaled(image, UNDEREXPOSURE_GAIN.0, UNDEREXPOSURE_GAIN.1)
}

/// Pack luma as a GREY frame: the identity, tightly packed.
#[must_use]
pub fn pack_grey(image: &GrayImage) -> Vec<u8> {
    image.as_raw().clone()
}

/// Pack luma as a YUYV 4:2:2 frame with neutral chroma.
///
/// Limited range, BT.601 — the same convention [`crate::decode::decode_yuyv`] reads, so
/// a round trip through this pair is a real round trip rather than two disagreeing
/// definitions of black. An odd width pads the trailing pair by repeating its last
/// pixel, which is what the format leaves no other option for.
#[must_use]
pub fn pack_yuyv(image: &GrayImage) -> Vec<u8> {
    let width = image.width();
    let pairs = width.div_ceil(2);
    let capacity = pairs.saturating_mul(4).saturating_mul(image.height());
    let mut out = Vec::with_capacity(usize::try_from(capacity).unwrap_or(0));
    for y in 0..image.height() {
        for pair in 0..pairs {
            let first = limited_luma(sample(image, pair * 2, y));
            let second = limited_luma(sample(image, pair * 2 + 1, y));
            out.extend_from_slice(&[first, NEUTRAL_CHROMA, second, NEUTRAL_CHROMA]);
        }
    }
    out
}

/// Pack luma as an NV12 frame with neutral chroma.
///
/// Same range convention as [`pack_yuyv`]. The chroma plane is `ceil(height / 2)` rows
/// of `ceil(width / 2) * 2` bytes, which is what V4L2 lays out after the Y plane.
#[must_use]
pub fn pack_nv12(image: &GrayImage) -> Vec<u8> {
    let width = image.width();
    let height = image.height();
    let chroma_row = width.div_ceil(2).saturating_mul(2);
    let chroma_rows = height.div_ceil(2);
    let chroma_bytes = usize::try_from(chroma_row.saturating_mul(chroma_rows)).unwrap_or(0);
    let mut out = Vec::with_capacity(image.as_raw().len().saturating_add(chroma_bytes));
    for pixel in image.pixels() {
        out.push(limited_luma(pixel.0[0]));
    }
    out.extend(std::iter::repeat_n(NEUTRAL_CHROMA, chroma_bytes));
    out
}

/// The chroma sample that means "no colour" in an 8-bit YUV frame.
const NEUTRAL_CHROMA: u8 = 128;

// ------------------------------------------------------- the fixture that has a colour
//
// **Everything above this line writes [`NEUTRAL_CHROMA`], and note N108 is the bill for it.**
// A round trip that compares every byte of every plane constrains nothing about *which*
// plane is which when every chroma sample is the same number: a mutant that swapped Cb and
// Cr in the Y4M sink passed the entire workspace suite, and the same mutant applied to
// `decode::decode_yuyv`/`decode_nv12` — the product path that turns a camera's raw frame into
// a photograph — passed all 1381 tests on 2026-08-15. The generators below are what
// `imaging::decode`'s orientation is asserted against, and their three properties are each
// load-bearing rather than tidy.
//
// **Disjoint value ranges.** Luma is 58–100, Cb is 108–126, Cr is 140–156, and no two of
// those overlap. A swap therefore has to change a *value*; it cannot merely move one. The
// 14-code gap between the chroma ranges is the floor on how far it moves — 22 codes of red
// and 28 of blue at the worst pixel, which no rounding tolerance can absorb.
//
// **Every sample is a function of its own position, with prime moduli** (43, 19, 17) whose
// multipliers are coprime to them. A plane shifted by one row or one column is then a plane
// full of wrong numbers rather than one that happens to match, and inside any frame narrower
// than the modulus no value repeats along an axis.
//
// **The decoded RGB clips at neither end, in either chroma orientation** — the property
// `imaging::decode`'s tests assert directly, because a channel pinned at 0 or 255 is a
// channel that is constant along the dimension under test, which is N108's class exactly.
// The window is tight: BT.601 limited range turns a chroma 20 codes below neutral into 40
// codes of blue subtracted from the luma term, so the luma range is bounded below by the
// blue that must survive it and above by the red that must not saturate. It is what stops
// `imaging::y4m`'s test generators being reused here: their ranges (luma 0–250, Cb 64–124,
// Cr 160–220) drive R past 255 and B below 0 over most of their domain, and they cannot
// simply be re-centred because they also produce two committed byte-exact fixtures. Two
// generators, two obligations, and the obligations are written down in both places rather
// than one set of numbers pressed into serving both.

/// The lowest luma this fixture emits; the range is 43 codes wide from here.
pub const COLOUR_YUV_LUMA_BASE: u8 = 58;

/// The lowest Cb this fixture emits; the range is 19 codes wide from here.
pub const COLOUR_YUV_CB_BASE: u8 = 108;

/// The lowest Cr this fixture emits; the range is 17 codes wide from here.
pub const COLOUR_YUV_CR_BASE: u8 = 140;

const COLOUR_YUV_LUMA_MODULUS: u32 = 43;
const COLOUR_YUV_CB_MODULUS: u32 = 19;
const COLOUR_YUV_CR_MODULUS: u32 = 17;

/// Luma at a pixel: 58–100, below both chroma ranges and above the blue they subtract.
#[must_use]
pub fn colour_yuv_luma(x: u32, y: u32) -> u8 {
    position_sample(COLOUR_YUV_LUMA_BASE, COLOUR_YUV_LUMA_MODULUS, x, 7, y, 13)
}

/// Cb at a *chroma* position: 108–126, disjoint from [`colour_yuv_cr`]'s range.
///
/// The coordinates are the chroma grid's, not the pixel grid's — halved horizontally for
/// 4:2:2 and on both axes for 4:2:0 — because that is the grid V4L2 lays these formats out
/// on, and a caller stating an expectation has to say which sample covers which pixel.
#[must_use]
pub fn colour_yuv_cb(x: u32, y: u32) -> u8 {
    position_sample(COLOUR_YUV_CB_BASE, COLOUR_YUV_CB_MODULUS, x, 3, y, 5)
}

/// Cr at a chroma position: 140–156, disjoint from [`colour_yuv_cb`]'s range.
#[must_use]
pub fn colour_yuv_cr(x: u32, y: u32) -> u8 {
    position_sample(COLOUR_YUV_CR_BASE, COLOUR_YUV_CR_MODULUS, x, 11, y, 2)
}

/// `base + (x·`x_step` + y·`y_step`) mod `modulus``, total for every coordinate.
///
/// Wrapping rather than checked arithmetic because a fixture generator that refused a large
/// coordinate would be a fixture generator with a failure mode, and there is nothing to
/// report it to; the value stays inside the declared range whatever the coordinates were.
fn position_sample(base: u8, modulus: u32, x: u32, x_step: u32, y: u32, y_step: u32) -> u8 {
    let offset = x
        .wrapping_mul(x_step)
        .wrapping_add(y.wrapping_mul(y_step))
        .wrapping_rem(modulus);
    // `base + offset` is under 256 for all three sets of constants above, so the conversion
    // is exact; the fallback exists only to keep this crate's panic ban whole.
    u8::try_from(u32::from(base).wrapping_add(offset)).unwrap_or(u8::MAX)
}

/// Pack the colour fixture as a YUYV 4:2:2 frame, tightly packed.
///
/// The layout is V4L2's own description of `V4L2_PIX_FMT_YUYV` and not this crate's decoder's:
/// one pixel pair is `Y0 Cb Y1 Cr`, so the two pixels of a pair share the chroma sample at
/// chroma column `pair`. An odd width pads the trailing pair, which is what the format leaves
/// no other option for.
#[must_use]
pub fn pack_yuyv_colour(width: u32, height: u32) -> Vec<u8> {
    let pairs = width.div_ceil(2);
    let capacity = pairs.saturating_mul(4).saturating_mul(height);
    let mut out = Vec::with_capacity(usize::try_from(capacity).unwrap_or(0));
    for y in 0..height {
        for pair in 0..pairs {
            out.push(colour_yuv_luma(pair * 2, y));
            out.push(colour_yuv_cb(pair, y));
            out.push(colour_yuv_luma(pair * 2 + 1, y));
            out.push(colour_yuv_cr(pair, y));
        }
    }
    out
}

/// Pack the colour fixture as an NV12 4:2:0 frame, tightly packed.
///
/// V4L2's `V4L2_PIX_FMT_NV12`: a full luma plane, then one interleaved `Cb Cr` plane at half
/// resolution on **both** axes, so one chroma pair covers a 2x2 block of pixels. The chroma
/// plane is `ceil(height / 2)` rows of `ceil(width / 2) * 2` bytes, which is the layout
/// [`crate::decode::decode_nv12`] computes its buffer requirement from.
#[must_use]
pub fn pack_nv12_colour(width: u32, height: u32) -> Vec<u8> {
    let chroma_columns = width.div_ceil(2);
    let chroma_rows = height.div_ceil(2);
    let capacity = width
        .saturating_mul(height)
        .saturating_add(chroma_columns.saturating_mul(2).saturating_mul(chroma_rows));
    let mut out = Vec::with_capacity(usize::try_from(capacity).unwrap_or(0));
    for y in 0..height {
        for x in 0..width {
            out.push(colour_yuv_luma(x, y));
        }
    }
    for y in 0..chroma_rows {
        for x in 0..chroma_columns {
            out.push(colour_yuv_cb(x, y));
            out.push(colour_yuv_cr(x, y));
        }
    }
    out
}

/// Full-range luma to BT.601 limited range: 0 → 16, 255 → 235.
fn limited_luma(value: u8) -> u8 {
    let scaled = (u32::from(value) * 219 + 127) / 255 + 16;
    u8::try_from(scaled.min(255)).unwrap_or(u8::MAX)
}

/// One sample, clamped to the image — the padding rule the packers share.
fn sample(image: &GrayImage, x: u32, y: u32) -> u8 {
    let x = x.min(image.width().saturating_sub(1));
    let y = y.min(image.height().saturating_sub(1));
    image.get_pixel_checked(x, y).map_or(0, |pixel| pixel.0[0])
}

/// `x`'s position along `width`, as a 0–255 ramp.
fn ramp(x: u32, width: u32) -> u8 {
    let last = width.saturating_sub(1);
    if last == 0 {
        return 0;
    }
    u8::try_from((x * 255) / last).unwrap_or(u8::MAX)
}

/// A seeded linear congruential generator.
///
/// Written here rather than depended on: fixtures must be identical across machines and
/// across releases of somebody else's crate, and this is thirty bits of arithmetic. The
/// multiplier and increment are Knuth's MMIX constants.
#[derive(Debug, Clone)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// Start from a seed. Every seed gives a different, reproducible stream.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Lcg { state: seed }
    }

    /// The next byte, taken from the high bits where an LCG's period is longest.
    pub fn next_u8(&mut self) -> u8 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        // A `u64` shifted right by 56 has at most eight significant bits, so the
        // conversion cannot fail; the fallback exists only to keep the panic ban whole.
        u8::try_from(self.state >> 56).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_generator_is_reproducible() {
        // A fixture that differs between runs turns a metric ordering test into a coin
        // flip, and the corpus into something nobody can regenerate.
        assert_eq!(checkerboard(16, 16, 4), checkerboard(16, 16, 4));
        assert_eq!(gradient(16, 4), gradient(16, 4));
        assert_eq!(text_like(32, 24), text_like(32, 24));
        assert_eq!(colour_bars(16, 4), colour_bars(16, 4));
        assert_eq!(speckle(16, 16, 7), speckle(16, 16, 7));
        assert_ne!(speckle(16, 16, 7), speckle(16, 16, 8));
        assert_eq!(pack_yuyv_colour(16, 8), pack_yuyv_colour(16, 8));
        assert_eq!(pack_nv12_colour(16, 8), pack_nv12_colour(16, 8));
    }

    #[test]
    fn the_colour_fixtures_three_sample_ranges_never_overlap() {
        // The property note **N108** charges for, asserted over the whole domain rather than
        // trusted from the constants: a swap of two planes has to change a value, and a plane
        // read at the wrong offset has to produce a number from the wrong band. Walked over a
        // coordinate span wider than every modulus, so each generator's whole range is seen.
        let mut luma = (u8::MAX, u8::MIN);
        let mut cb = (u8::MAX, u8::MIN);
        let mut cr = (u8::MAX, u8::MIN);
        for y in 0..64 {
            for x in 0..64 {
                luma = (
                    luma.0.min(colour_yuv_luma(x, y)),
                    luma.1.max(colour_yuv_luma(x, y)),
                );
                cb = (cb.0.min(colour_yuv_cb(x, y)), cb.1.max(colour_yuv_cb(x, y)));
                cr = (cr.0.min(colour_yuv_cr(x, y)), cr.1.max(colour_yuv_cr(x, y)));
            }
        }
        assert_eq!(luma, (58, 100), "the luma band moved");
        assert_eq!(cb, (108, 126), "the Cb band moved");
        assert_eq!(cr, (140, 156), "the Cr band moved");
        assert!(luma.1 < cb.0, "luma reaches into the Cb band");
        assert!(cb.1 < cr.0, "Cb reaches into the Cr band");
        // The gap is the floor on how far a swapped sample moves, and 14 codes of chroma is
        // 22 of red and 28 of blue — an order of magnitude past any rounding tolerance.
        assert!(
            cr.0 - cb.1 >= 14,
            "the chroma bands are too close to tell apart"
        );
    }

    #[test]
    fn a_colour_sample_never_repeats_along_a_row_or_a_column_of_its_own_plane() {
        // Position-dependence is the other half of N108's repair: a plane shifted by one row
        // or one column must come out full of wrong numbers rather than matching by luck.
        for step in 1..16u32 {
            assert_ne!(colour_yuv_luma(0, 0), colour_yuv_luma(step, 0));
            assert_ne!(colour_yuv_luma(0, 0), colour_yuv_luma(0, step));
            assert_ne!(colour_yuv_cb(0, 0), colour_yuv_cb(step, 0));
            assert_ne!(colour_yuv_cb(0, 0), colour_yuv_cb(0, step));
            assert_ne!(colour_yuv_cr(0, 0), colour_yuv_cr(step, 0));
            assert_ne!(colour_yuv_cr(0, 0), colour_yuv_cr(0, step));
        }
    }

    #[test]
    fn the_colour_packers_produce_the_lengths_their_formats_define() {
        // Odd extents on both axes, because that is where the two formats' padding rules
        // differ and where a length computed from `width * height` alone comes out short.
        assert_eq!(pack_yuyv_colour(7, 5).len(), 4 * 4 * 5);
        assert_eq!(pack_nv12_colour(7, 5).len(), 7 * 5 + 8 * 3);
        assert_eq!(pack_yuyv_colour(16, 8).len(), 16 * 8 * 2);
        assert_eq!(pack_nv12_colour(16, 8).len(), 16 * 8 + 16 * 4);
    }

    #[test]
    fn the_colour_packers_put_each_sample_where_its_format_puts_it() {
        // The layouts, read back out of the bytes: `Y0 Cb Y1 Cr` per YUYV pair, and a whole
        // luma plane followed by interleaved `Cb Cr` for NV12. Stated here so that a packer
        // that quietly changed its own layout is a red test in this module rather than a
        // silent change of meaning in `imaging::decode`'s expectations.
        let yuyv = pack_yuyv_colour(4, 2);
        assert_eq!(
            &yuyv[..8],
            &[
                colour_yuv_luma(0, 0),
                colour_yuv_cb(0, 0),
                colour_yuv_luma(1, 0),
                colour_yuv_cr(0, 0),
                colour_yuv_luma(2, 0),
                colour_yuv_cb(1, 0),
                colour_yuv_luma(3, 0),
                colour_yuv_cr(1, 0),
            ]
        );
        let nv12 = pack_nv12_colour(4, 2);
        assert_eq!(
            &nv12[..4],
            &[
                colour_yuv_luma(0, 0),
                colour_yuv_luma(1, 0),
                colour_yuv_luma(2, 0),
                colour_yuv_luma(3, 0)
            ]
        );
        assert_eq!(
            &nv12[8..12],
            &[
                colour_yuv_cb(0, 0),
                colour_yuv_cr(0, 0),
                colour_yuv_cb(1, 0),
                colour_yuv_cr(1, 0)
            ]
        );
    }

    #[test]
    fn the_generated_by_chunk_is_in_the_bytes() {
        // The corpus gate reads the file, not our intentions.
        let bytes =
            provenanced_png_gray8(&checkerboard(8, 8, 2), "checkerboard-8-2").expect("encode");
        assert!(
            contains(&bytes, PROVENANCE_KEYWORD.as_bytes()),
            "no {PROVENANCE_KEYWORD} keyword"
        );
        assert!(
            contains(&bytes, GENERATED_BY.as_bytes()),
            "no {GENERATED_BY} value"
        );
        assert!(contains(&bytes, b"checkerboard-8-2"), "no fixture name");

        // The inverse: the plain encoder produces a file the gate must reject, which is
        // why fixtures never take that path.
        let bare = crate::encode::png_gray8(&checkerboard(8, 8, 2)).expect("encode");
        assert!(!contains(&bare, PROVENANCE_KEYWORD.as_bytes()));
    }

    #[test]
    fn the_rgb_fixture_writer_carries_the_same_marker() {
        let bytes = provenanced_png_rgb8(&colour_bars(16, 4), "colour-bars-16x4").expect("encode");
        assert!(contains(&bytes, GENERATED_BY.as_bytes()));
    }

    #[test]
    fn a_text_chunk_survives_an_independent_png_reader() {
        // Present-in-the-bytes is necessary but not sufficient: the chunk must also be
        // a well-formed tEXt chunk, which only a real parser can say.
        let bytes = provenanced_png_gray8(&gradient(8, 8), "gradient-8x8").expect("encode");
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes.as_slice()));
        let reader = decoder.read_info().expect("header");
        let found = reader
            .info()
            .uncompressed_latin1_text
            .iter()
            .find(|chunk| chunk.keyword == PROVENANCE_KEYWORD)
            .map(|chunk| chunk.text.clone());
        assert_eq!(found.as_deref(), Some(GENERATED_BY));
    }

    #[test]
    fn a_degenerate_cell_size_is_read_as_one_rather_than_dividing_by_zero() {
        let image = checkerboard(4, 4, 0);
        assert_eq!(image.dimensions(), (4, 4));
        assert_eq!(image, checkerboard(4, 4, 1));
    }

    #[test]
    fn exposure_scaling_clips_rather_than_wrapping() {
        let bright = overexposed(&gradient(256, 1));
        assert_eq!(bright.as_raw().iter().copied().max(), Some(255));
        let dark = underexposed(&gradient(256, 1));
        assert_eq!(dark.as_raw().iter().copied().max(), Some(51));
        // A wrapping implementation would put a black pixel next to a white one here.
        assert!(bright.as_raw().windows(2).all(|w| w[1] >= w[0]));
    }

    #[test]
    fn blurring_refuses_a_sigma_imageproc_would_panic_on() {
        let source = checkerboard(8, 8, 2);
        assert!(blurred(&source, 0.0).is_none());
        assert!(blurred(&source, -1.0).is_none());
        assert!(blurred(&source, f32::NAN).is_none());
        assert!(blurred(&source, 1.5).is_some());
    }

    #[test]
    fn the_packers_produce_the_lengths_the_formats_define() {
        let image = gradient(7, 5);
        assert_eq!(pack_grey(&image).len(), 7 * 5);
        // Odd width rounds up to whole YUYV pairs.
        assert_eq!(pack_yuyv(&image).len(), 4 * 4 * 5);
        assert_eq!(pack_nv12(&image).len(), 7 * 5 + 8 * 3);
    }

    #[test]
    fn limited_range_packing_puts_black_and_white_where_bt601_says() {
        assert_eq!(limited_luma(0), 16);
        assert_eq!(limited_luma(255), 235);
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }
}
