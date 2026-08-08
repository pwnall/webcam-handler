//! The pass-through-versus-re-encode decision (E6) — the single home for it (§2.10).
//!
//! Pure: a frame and a requested sink go in, the bytes to write and a record of what
//! happened to them come out. The engine owns the file, the daemon owns the response
//! body, and neither owns this decision — which is what stops a second, subtly different
//! copy of it appearing behind the RPC surface.
//!
//! ## What E6 actually promises
//!
//! A `.jpg` sink fed an MJPG frame gets **the camera's own bitstream, unmodified**. Not
//! "re-encoded at high quality", not "decoded and written back": the same bytes. Two
//! reasons, and the second is the load-bearing one:
//!
//! 1. It is what the user asked for. The camera's JPEG is the product.
//! 2. Calibration compares samples on a sharpness metric. A decode/re-encode round trip
//!    adds ringing that varies with content, so a sweep that re-encodes is partly
//!    ranking its own codec. [`PhotoRendering`] is reported alongside the bytes so a
//!    sample record can say which it got.
//!
//! ## Where a transform goes
//!
//! On the verbatim path an orientation request cannot touch pixels without destroying
//! the promise above, so it becomes an EXIF Orientation tag ([`crate::exif`] writes it).
//! On every other path it is applied to pixels before encoding. [`TransformApplication`]
//! records which happened, because "the photo is rotated" and "the photo says it is
//! rotated" are different facts and a viewer that ignores EXIF distinguishes them.

use image::{ImageBuffer, Pixel, imageops};
use schema::capture::{Frame, PhotoFormat, PhotoRendering, Transform, TransformApplication};
use schema::error::Result;

use crate::decode::{Decoded, decode_frame};
use crate::encode::{self, DEFAULT_JPEG_QUALITY};
use crate::fault::imaging_failure;

/// A rendered photo: the bytes to write, and what was done to get them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Photo {
    /// The bytes destined for the sink.
    pub bytes: Vec<u8>,
    /// Which of the three paths produced them.
    pub rendering: PhotoRendering,
    /// Where the orientation request went.
    pub transform: TransformApplication,
    /// Width of the image these bytes encode, after any pixel-domain transform.
    pub width: u32,
    /// Height of the same.
    pub height: u32,
}

/// Render a frame into the bytes a sink should receive.
///
/// # Errors
///
/// [`schema::Error::FormatUnsupported`] for a source outside the D6 set,
/// and [`schema::Error::DeviceIo`] naming the step for a frame that
/// cannot be decoded or encoded.
pub fn render(frame: &Frame, format: PhotoFormat, transform: Transform) -> Result<Photo> {
    render_with_quality(frame, format, transform, DEFAULT_JPEG_QUALITY)
}

/// Render a frame, choosing the JPEG quality used on re-encode paths.
///
/// The quality is not reachable from the verbatim path by construction — there is no
/// encoder there to give it to.
///
/// # Errors
///
/// As [`render`].
pub fn render_with_quality(
    frame: &Frame,
    format: PhotoFormat,
    transform: Transform,
    jpeg_quality: u8,
) -> Result<Photo> {
    if format == PhotoFormat::Jpeg && frame.pixel_format.is_compressed() {
        return verbatim(frame, transform);
    }

    let decoded = transform_pixels(&decode_frame(frame)?, transform);
    let bytes = match format {
        PhotoFormat::Jpeg => encode::jpeg(&decoded, jpeg_quality)?,
        PhotoFormat::Png => encode::png(&decoded)?,
        PhotoFormat::Ppm => encode::netpbm(&decoded)?,
    };
    let rendering = if frame.pixel_format.is_compressed() {
        PhotoRendering::DecodedAndEncoded {
            source: frame.pixel_format,
            target: format,
        }
    } else {
        PhotoRendering::ConvertedAndEncoded {
            source: frame.pixel_format,
            target: format,
        }
    };
    Ok(Photo {
        bytes,
        rendering,
        transform: if transform == Transform::None {
            TransformApplication::Identity
        } else {
            TransformApplication::Pixels
        },
        width: decoded.width(),
        height: decoded.height(),
    })
}

/// The E6 path: the camera's bytes, cloned and not otherwise touched.
fn verbatim(frame: &Frame, transform: Transform) -> Result<Photo> {
    // The one check on this path, and it changes no bytes: a buffer that is not a JPEG
    // must not be written to a `.jpg`. A driver handing back an unwritten buffer under
    // an MJPG format is a real failure mode, and a zero-byte photo file is a worse
    // answer to it than a typed error.
    if !frame.bytes.starts_with(&[0xff, 0xd8]) {
        return Err(imaging_failure(
            "pass through camera JPEG",
            format!(
                "the driver delivered {} bytes under {} that do not begin with the JPEG SOI marker",
                frame.bytes.len(),
                frame.pixel_format
            ),
        ));
    }
    Ok(Photo {
        bytes: frame.bytes.clone(),
        rendering: PhotoRendering::Verbatim {
            source: frame.pixel_format,
        },
        transform: if transform == Transform::None {
            TransformApplication::Identity
        } else {
            TransformApplication::ExifOrientation {
                orientation: transform.exif_orientation(),
            }
        },
        width: frame.width,
        height: frame.height,
    })
}

/// Apply an orientation transform to pixels.
///
/// The `match` is exhaustive over [`Transform`], so a new orientation cannot be added to
/// the schema without an implementation here.
#[must_use]
pub fn transform_pixels(image: &Decoded, transform: Transform) -> Decoded {
    match image {
        Decoded::Rgb(rgb) => Decoded::Rgb(oriented(rgb, transform)),
        Decoded::Gray(gray) => Decoded::Gray(oriented(gray, transform)),
    }
}

fn oriented<P>(
    image: &ImageBuffer<P, Vec<P::Subpixel>>,
    transform: Transform,
) -> ImageBuffer<P, Vec<P::Subpixel>>
where
    P: Pixel + 'static,
{
    match transform {
        Transform::None => image.clone(),
        Transform::HFlip => imageops::flip_horizontal(image),
        Transform::VFlip => imageops::flip_vertical(image),
        Transform::Rot90 => imageops::rotate90(image),
        Transform::Rot180 => imageops::rotate180(image),
        Transform::Rot270 => imageops::rotate270(image),
    }
}

#[cfg(test)]
mod tests {
    use schema::camera::PixelFormat;

    use super::*;
    use crate::decode;
    use crate::fixtures;

    fn mjpg_frame(width: u32, height: u32) -> Frame {
        let source = fixtures::text_like(width, height);
        let bytes = encode::jpeg(&Decoded::Gray(source), 88).expect("encode fixture");
        Frame {
            bytes,
            pixel_format: PixelFormat::MJPG,
            width,
            height,
            bytes_per_line: 0,
            sequence: 3,
            timestamp_us: 1_000,
        }
    }

    fn yuyv_frame(width: u32, height: u32) -> Frame {
        Frame {
            bytes: fixtures::pack_yuyv(&fixtures::gradient(width, height)),
            pixel_format: PixelFormat::YUYV,
            width,
            height,
            bytes_per_line: 0,
            sequence: 4,
            timestamp_us: 2_000,
        }
    }

    #[test]
    fn a_jpg_sink_fed_an_mjpg_frame_returns_the_camera_bytes_unmodified() {
        // E6, asserted the only way that means anything: the output compared against
        // the input byte for byte. A "high quality re-encode" passes every test that
        // only checks decodability.
        let frame = mjpg_frame(64, 48);
        let photo = render(&frame, PhotoFormat::Jpeg, Transform::None).expect("render");
        assert_eq!(
            photo.bytes, frame.bytes,
            "the bitstream was not passed through"
        );
        assert!(photo.rendering.is_verbatim());
        assert_eq!(photo.transform, TransformApplication::Identity);
        assert_eq!((photo.width, photo.height), (64, 48));
    }

    #[test]
    fn a_png_sink_fed_the_same_frame_must_not_return_them_unmodified() {
        // The other direction of the same claim: if this passed, `is_verbatim` would be
        // measuring nothing.
        let frame = mjpg_frame(64, 48);
        let photo = render(&frame, PhotoFormat::Png, Transform::None).expect("render");
        assert_ne!(photo.bytes, frame.bytes);
        assert!(!photo.rendering.is_verbatim());
        assert_eq!(
            photo.rendering,
            PhotoRendering::DecodedAndEncoded {
                source: PixelFormat::MJPG,
                target: PhotoFormat::Png,
            }
        );
        assert!(photo.bytes.starts_with(&[0x89, b'P', b'N', b'G']));
    }

    #[test]
    fn a_transform_on_the_verbatim_path_leaves_the_bitstream_alone() {
        // The whole point of D6's split: rotating a `.jpg` photo must not cost a
        // re-encode, so the rotation becomes a tag and the bytes stay the camera's.
        let frame = mjpg_frame(64, 48);
        let photo = render(&frame, PhotoFormat::Jpeg, Transform::Rot90).expect("render");
        assert_eq!(photo.bytes, frame.bytes);
        assert_eq!(
            photo.transform,
            TransformApplication::ExifOrientation { orientation: 6 }
        );
        // And the dimensions reported are the frame's, not the rotated ones — nothing
        // was rotated.
        assert_eq!((photo.width, photo.height), (64, 48));
    }

    #[test]
    fn a_transform_on_an_encoded_path_moves_pixels_and_swaps_the_axes() {
        let frame = mjpg_frame(64, 48);
        let photo = render(&frame, PhotoFormat::Png, Transform::Rot90).expect("render");
        assert_eq!(photo.transform, TransformApplication::Pixels);
        assert_eq!((photo.width, photo.height), (48, 64));
    }

    #[test]
    fn a_raw_frame_is_converted_and_encoded_on_every_sink() {
        let frame = yuyv_frame(32, 16);
        for (format, target) in [
            (PhotoFormat::Jpeg, PhotoFormat::Jpeg),
            (PhotoFormat::Png, PhotoFormat::Png),
            (PhotoFormat::Ppm, PhotoFormat::Ppm),
        ] {
            let photo = render(&frame, format, Transform::None).expect("render");
            assert_eq!(
                photo.rendering,
                PhotoRendering::ConvertedAndEncoded {
                    source: PixelFormat::YUYV,
                    target,
                }
            );
            assert!(!photo.bytes.is_empty());
        }
    }

    #[test]
    fn a_grey_frame_reaches_a_png_sink_as_grayscale() {
        // D6's "grayscale is not optional": the Chicony IR camera's only format, on the
        // sink that always re-encodes.
        let frame = Frame {
            bytes: fixtures::pack_grey(&fixtures::checkerboard(32, 32, 4)),
            pixel_format: PixelFormat::GREY,
            width: 32,
            height: 32,
            bytes_per_line: 32,
            sequence: 1,
            timestamp_us: 0,
        };
        let photo = render(&frame, PhotoFormat::Png, Transform::None).expect("render");
        let reader = png::Decoder::new(std::io::Cursor::new(photo.bytes.as_slice()))
            .read_info()
            .expect("header");
        assert_eq!(reader.info().color_type, png::ColorType::Grayscale);
    }

    #[test]
    fn a_grey_frame_on_a_jpg_sink_is_re_encoded_rather_than_passed_through() {
        // GREY is not a bitstream; there is nothing to pass through. The record must
        // say so rather than claiming byte fidelity it does not have.
        let frame = Frame {
            bytes: fixtures::pack_grey(&fixtures::gradient(16, 16)),
            pixel_format: PixelFormat::GREY,
            width: 16,
            height: 16,
            bytes_per_line: 16,
            sequence: 1,
            timestamp_us: 0,
        };
        let photo = render(&frame, PhotoFormat::Jpeg, Transform::None).expect("render");
        assert!(!photo.rendering.is_verbatim());
        assert!(photo.bytes.starts_with(&[0xff, 0xd8]));
    }

    #[test]
    fn a_frame_in_a_format_we_cannot_read_is_refused_on_every_sink() {
        let frame = Frame {
            bytes: vec![0u8; 64],
            pixel_format: PixelFormat::parse("H264").expect("fourcc"),
            width: 8,
            height: 8,
            bytes_per_line: 0,
            sequence: 1,
            timestamp_us: 0,
        };
        for format in [PhotoFormat::Jpeg, PhotoFormat::Png, PhotoFormat::Ppm] {
            assert!(render(&frame, format, Transform::None).is_err());
        }
    }

    #[test]
    fn a_frame_that_is_not_a_jpeg_never_reaches_a_jpg_sink_verbatim() {
        // A driver returning an unwritten buffer under MJPG. Writing it verbatim would
        // produce a zero-content `.jpg` nobody can open, with no error anywhere.
        let frame = Frame {
            bytes: vec![0u8; 4096],
            pixel_format: PixelFormat::MJPG,
            width: 64,
            height: 48,
            bytes_per_line: 0,
            sequence: 1,
            timestamp_us: 0,
        };
        let err = render(&frame, PhotoFormat::Jpeg, Transform::None).expect_err("not a JPEG");
        assert!(err.to_string().contains("SOI"), "{err}");
    }

    #[test]
    fn a_pixel_transform_is_the_orientation_it_claims_to_be() {
        // A rot90 that is really a rot270 satisfies every dimension assertion above.
        // Pin the actual movement: the top-left pixel of a rot90 comes from the
        // bottom-left of the original.
        let mut source = fixtures::flat(4, 2, 0);
        if let Some(pixel) = source.get_pixel_mut_checked(0, 1) {
            pixel.0 = [255];
        }
        let rotated = transform_pixels(&Decoded::Gray(source), Transform::Rot90);
        let Decoded::Gray(image) = rotated else {
            panic!("a grey image must stay grey through a transform");
        };
        assert_eq!(image.dimensions(), (2, 4));
        assert_eq!(image.get_pixel_checked(0, 0).map(|p| p.0[0]), Some(255));
    }

    #[test]
    fn a_verbatim_photo_still_decodes_to_the_frame_it_came_from() {
        // Byte fidelity is not an excuse for writing something unreadable.
        let frame = mjpg_frame(48, 32);
        let photo = render(&frame, PhotoFormat::Jpeg, Transform::None).expect("render");
        let decoded = decode::decode_jpeg(&photo.bytes, 48, 32).expect("decode");
        assert_eq!((decoded.width(), decoded.height()), (48, 32));
    }
}
