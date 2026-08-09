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
