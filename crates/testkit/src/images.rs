//! Synthetic image fixtures, for tests that want one import.
//!
//! **`webcam-handler-imaging` owns the generators**; this module is a re-export surface
//! plus the one adapter that does not belong there — turning a generated image into a
//! [`schema::capture::Frame`], which is a capture-layer type a pure imaging fixture has
//! no business constructing.
//!
//! Keeping the generators in one crate is not tidiness: `corpus/images/` fixtures, the
//! imaging crate's own metric-ordering tests, and the fake backend's frame synthesis all
//! have to be the *same* pixels, or a sharpness ordering proven in one place stops
//! meaning anything in another (design §2.3, §3.2).
//!
//! Anything written to `corpus/images/` goes through
//! [`imaging::fixtures::provenanced_png_gray8`] or its RGB twin — the no-frame-bytes gate
//! reads the `generated-by` marker those stamp, and cannot tell a bare PNG from a frame
//! with a person in it (§5).

use image::GrayImage;
use schema::camera::PixelFormat;
use schema::capture::Frame;
use schema::error::{Error, Result};

pub use imaging::fixtures::{
    GENERATED_BY, Lcg, PROVENANCE_KEYWORD, blurred, checkerboard, colour_bars, exposure_scaled,
    gradient, overexposed, pack_grey, pack_nv12, pack_yuyv, provenanced_png_gray8,
    provenanced_png_rgb8, speckle, text_like, underexposed,
};

/// Pack a generated image as a captured frame in `format`.
///
/// The geometry fields are what a driver would have reported for these bytes, so a frame
/// built here exercises the same validation path a real one does — including
/// `bytes_per_line`, which the decode layer trusts and must therefore be told the truth
/// about.
///
/// # Errors
///
/// [`Error::FormatUnsupported`] for a format the packers do not produce.
pub fn frame(image: &GrayImage, format: PixelFormat) -> Result<Frame> {
    let (bytes, bytes_per_line) = if format == PixelFormat::GREY {
        (pack_grey(image), image.width())
    } else if format == PixelFormat::YUYV {
        // Whole 4:2:2 pairs; an odd width pads by repeating its last pixel.
        (
            pack_yuyv(image),
            image.width().div_ceil(2).saturating_mul(4),
        )
    } else if format == PixelFormat::NV12 {
        // The stride of the *luma* plane; the chroma plane follows it.
        (pack_nv12(image), image.width())
    } else {
        return Err(Error::format_unsupported(
            Some(format),
            vec![PixelFormat::GREY, PixelFormat::YUYV, PixelFormat::NV12],
        ));
    };

    Ok(Frame {
        bytes,
        pixel_format: format,
        width: image.width(),
        height: image.height(),
        bytes_per_line,
        sequence: 0,
        timestamp_us: 0,
    })
}

/// The same scene sharp and blurred, for a metric-ordering test.
///
/// Returned as a pair so the two are the same pixels apart from the blur — a comparison
/// between two *different* images would be measuring the images, not the blur.
#[must_use]
pub fn sharp_and_blurred(width: u32, height: u32, sigma: f32) -> Option<(GrayImage, GrayImage)> {
    let sharp = text_like(width, height);
    let soft = blurred(&sharp, sigma)?;
    Some((sharp, soft))
}

#[cfg(test)]
mod tests {
    use imaging::metrics;

    use super::*;

    #[test]
    fn a_packed_frame_reports_the_geometry_its_bytes_actually_have() {
        // The failure this catches: a frame whose `bytes_per_line` disagrees with its
        // buffer, which is a truncated read one decode away.
        let image = gradient(64, 48);
        for format in [PixelFormat::GREY, PixelFormat::YUYV] {
            let frame = frame(&image, format).unwrap_or_else(|e| panic!("{format}: {e}"));
            assert_eq!(frame.width, 64);
            assert_eq!(frame.height, 48);
            assert_eq!(
                frame.bytes.len(),
                usize::try_from(frame.bytes_per_line * frame.height).expect("fits"),
                "{format} stride disagrees with its buffer"
            );
        }
        // NV12's stride describes the luma plane only, so the buffer is half again as
        // large. Asserted rather than assumed, because getting it backwards is silent.
        let nv12 = frame(&image, PixelFormat::NV12).expect("pack NV12");
        assert_eq!(nv12.bytes.len(), 64 * 48 + 64 * 24);
        assert_eq!(nv12.bytes_per_line, 64);
    }

    #[test]
    fn a_format_the_packers_do_not_produce_is_a_typed_refusal() {
        let image = gradient(8, 8);
        assert!(matches!(
            frame(&image, PixelFormat::MJPG),
            Err(Error::FormatUnsupported { .. })
        ));
    }

    #[test]
    fn the_sharp_and_blurred_pair_orders_the_way_a_calibration_sweep_needs() {
        let (sharp, soft) = sharp_and_blurred(96, 72, 2.0).expect("a positive sigma");
        assert!(
            metrics::sharpness(&sharp) > metrics::sharpness(&soft),
            "the blurred half must score below the sharp one"
        );
        // The inverse: a sigma imaging refuses produces no pair at all, rather than a
        // pair of identical images that would make the ordering above vacuous.
        assert!(sharp_and_blurred(96, 72, 0.0).is_none());
    }
}
