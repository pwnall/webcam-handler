//! Pixels to bytes — the D6 sink encodings.
//!
//! PNG (lossless, always encoded), JPEG (re-encode, for the paths E6 cannot pass
//! through), and binary PPM/PGM (the "give me pixels" escape hatch for tooling).
//!
//! ## Why the `image` crate's own JPEG encoder
//!
//! `jpeg-encoder` and `mozjpeg`/`turbojpeg` all carry the IJG term, which design §8
//! item 1 keeps dormant: the ban is on the cargo-deny list by name, and the default
//! stack never needs them. The `image` crate ships its own MIT/Apache baseline encoder,
//! which is the one this module calls. It is not the fastest JPEG encoder in existence
//! and does not need to be — E6 means the common photo path re-encodes nothing at all.
//!
//! ## Why PNG goes through the `png` crate rather than `image`
//!
//! One home for PNG (rubric A5). The provenance marker that
//! [`crate::fixtures`] stamps into generated fixtures is a `tEXt` chunk, and the `image`
//! crate's PNG encoder does not expose text chunks — so a second encoder would have
//! appeared the moment the corpus gate needed one. Everything PNG in this crate is
//! [`png_rgb8`] and [`png_gray8`], and both are [`png_with_text`] with an empty chunk
//! list.

use image::codecs::jpeg::JpegEncoder;
use image::{GrayImage, RgbImage};
use schema::error::Result;

use crate::decode::Decoded;
use crate::fault::imaging_failure;

/// The quality the JPEG re-encode path uses when a caller expresses no preference.
///
/// High enough that a re-encoded calibration sample is not scored on its own compression
/// artifacts, low enough that a 4K frame is not larger than the MJPG original. Nothing
/// on the E6 pass-through path reads it, which is the point: it only applies where a
/// re-encode was unavoidable anyway.
pub const DEFAULT_JPEG_QUALITY: u8 = 92;

const PNG_OP: &str = "encode PNG";
const JPEG_OP: &str = "encode JPEG";

/// Encode a decoded frame as PNG, keeping its colour model.
///
/// # Errors
///
/// [`schema::Error::DeviceIo`] naming the encode step when the geometry
/// is degenerate.
pub fn png(image: &Decoded) -> Result<Vec<u8>> {
    match image {
        Decoded::Rgb(rgb) => png_rgb8(rgb),
        Decoded::Gray(gray) => png_gray8(gray),
    }
}

/// Encode 8-bit RGB as PNG.
///
/// # Errors
///
/// As [`png()`].
pub fn png_rgb8(image: &RgbImage) -> Result<Vec<u8>> {
    png_with_text(
        image.as_raw(),
        image.width(),
        image.height(),
        png::ColorType::Rgb,
        &[],
    )
}

/// Encode 8-bit luma as PNG.
///
/// # Errors
///
/// As [`png()`].
pub fn png_gray8(image: &GrayImage) -> Result<Vec<u8>> {
    png_with_text(
        image.as_raw(),
        image.width(),
        image.height(),
        png::ColorType::Grayscale,
        &[],
    )
}

/// Encode a PNG carrying `tEXt` chunks.
///
/// The one PNG writer in this crate. `text` entries are `(keyword, value)`; PNG's own
/// rules bound the keyword to 1–79 Latin-1 characters, and the `png` crate enforces
/// them — a rejected keyword is an error here rather than a chunk quietly missing from
/// the file, which is what the corpus provenance gate depends on.
///
/// # Errors
///
/// As [`png()`], plus a rejected keyword.
pub fn png_with_text(
    pixels: &[u8],
    width: u32,
    height: u32,
    color: png::ColorType,
    text: &[(&str, &str)],
) -> Result<Vec<u8>> {
    if width == 0 || height == 0 {
        return Err(imaging_failure(
            PNG_OP,
            format!("cannot encode a {width}x{height} image"),
        ));
    }
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(color);
    encoder.set_depth(png::BitDepth::Eight);
    for (keyword, value) in text {
        encoder
            .add_text_chunk((*keyword).to_owned(), (*value).to_owned())
            .map_err(|err| imaging_failure(PNG_OP, err.to_string()))?;
    }
    let mut writer = encoder
        .write_header()
        .map_err(|err| imaging_failure(PNG_OP, err.to_string()))?;
    writer
        .write_image_data(pixels)
        .map_err(|err| imaging_failure(PNG_OP, err.to_string()))?;
    writer
        .finish()
        .map_err(|err| imaging_failure(PNG_OP, err.to_string()))?;
    Ok(out)
}

/// Re-encode a decoded frame as JPEG.
///
/// Grayscale stays grayscale — the encoder writes a single-component JPEG rather than
/// three identical ones, so an IR frame does not triple in size on its way to disk.
///
/// # Errors
///
/// [`schema::Error::DeviceIo`] when the geometry is degenerate or exceeds
/// JPEG's 65535-pixel axis limit.
pub fn jpeg(image: &Decoded, quality: u8) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut out, quality);
    let result = match image {
        Decoded::Rgb(rgb) => encoder.encode_image(rgb),
        Decoded::Gray(gray) => encoder.encode_image(gray),
    };
    result.map_err(|err| imaging_failure(JPEG_OP, err.to_string()))?;
    Ok(out)
}

/// Encode a decoded frame as binary Netpbm: P6 for RGB, P5 for luma.
///
/// The two share a header shape, and [`schema::capture::PhotoFormat::Ppm`]
/// covers both because a `.pgm` sink fed a colour frame and a `.ppm` sink fed a grey one
/// both have exactly one sensible answer.
///
/// # Errors
///
/// As [`png()`].
pub fn netpbm(image: &Decoded) -> Result<Vec<u8>> {
    let (magic, width, height, pixels) = match image {
        Decoded::Rgb(rgb) => ("P6", rgb.width(), rgb.height(), rgb.as_raw().as_slice()),
        Decoded::Gray(gray) => ("P5", gray.width(), gray.height(), gray.as_raw().as_slice()),
    };
    if width == 0 || height == 0 {
        return Err(imaging_failure(
            "encode Netpbm",
            format!("cannot encode a {width}x{height} image"),
        ));
    }
    let header = format!("{magic}\n{width} {height}\n255\n");
    let mut out = Vec::with_capacity(header.len() + pixels.len());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(pixels);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode;
    use crate::fixtures;

    #[test]
    fn a_png_round_trips_through_the_independent_reader() {
        let source = fixtures::checkerboard(16, 12, 4);
        let bytes = png_gray8(&source).expect("encode");
        assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']), "not a PNG");

        let decoder = png::Decoder::new(std::io::Cursor::new(bytes.as_slice()));
        let mut reader = decoder.read_info().expect("header");
        let mut pixels = vec![0u8; reader.output_buffer_size().expect("size")];
        let info = reader.next_frame(&mut pixels).expect("frame");
        assert_eq!((info.width, info.height), (16, 12));
        assert_eq!(info.color_type, png::ColorType::Grayscale);
        assert_eq!(&pixels[..info.buffer_size()], source.as_raw().as_slice());
    }

    #[test]
    fn an_rgb_png_keeps_three_channels() {
        let source = fixtures::colour_bars(12, 4);
        let bytes = png_rgb8(&source).expect("encode");
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes.as_slice()));
        let mut reader = decoder.read_info().expect("header");
        let mut pixels = vec![0u8; reader.output_buffer_size().expect("size")];
        let info = reader.next_frame(&mut pixels).expect("frame");
        assert_eq!(info.color_type, png::ColorType::Rgb);
        assert_eq!(&pixels[..info.buffer_size()], source.as_raw().as_slice());
    }

    #[test]
    fn a_degenerate_image_is_refused_rather_than_written_as_a_broken_file() {
        let empty = GrayImage::new(0, 0);
        assert!(png_gray8(&empty).is_err());
        assert!(netpbm(&Decoded::Gray(empty)).is_err());
    }

    #[test]
    fn a_grayscale_jpeg_stays_single_component() {
        // Re-encoding an IR frame as colour would triple it on disk and give the
        // sharpness metric chroma noise to chew on.
        let source = fixtures::text_like(48, 24);
        let bytes = jpeg(&Decoded::Gray(source), DEFAULT_JPEG_QUALITY).expect("encode");
        let decoded = decode::decode_jpeg(&bytes, 48, 24).expect("decode");
        assert!(matches!(decoded, Decoded::Gray(_)));
    }

    #[test]
    fn a_jpeg_re_encode_preserves_the_image_within_its_lossy_budget() {
        let source = fixtures::gradient(32, 32);
        let bytes = jpeg(&Decoded::Gray(source.clone()), 95).expect("encode");
        let decoded = decode::decode_jpeg(&bytes, 32, 32).expect("decode");
        let luma = decoded.luma().into_owned();
        assert_eq!(luma.dimensions(), source.dimensions());
        let worst = source
            .as_raw()
            .iter()
            .zip(luma.as_raw())
            .map(|(a, b)| (i32::from(*a) - i32::from(*b)).abs())
            .max()
            .unwrap_or(0);
        assert!(worst <= 12, "quality 95 drifted by {worst}");
    }

    #[test]
    fn netpbm_writes_the_header_the_format_specifies() {
        let gray = netpbm(&Decoded::Gray(fixtures::gradient(4, 2))).expect("encode");
        assert!(gray.starts_with(b"P5\n4 2\n255\n"), "{:?}", &gray[..12]);
        assert_eq!(gray.len(), 11 + 8);

        let rgb = netpbm(&Decoded::Rgb(fixtures::colour_bars(4, 2))).expect("encode");
        assert!(rgb.starts_with(b"P6\n4 2\n255\n"));
        assert_eq!(rgb.len(), 11 + 4 * 2 * 3);
    }

    #[test]
    fn a_text_chunk_lands_in_the_file_and_a_bad_keyword_is_refused() {
        let source = fixtures::gradient(8, 8);
        let bytes = png_with_text(
            source.as_raw(),
            8,
            8,
            png::ColorType::Grayscale,
            &[("marker", "present")],
        )
        .expect("encode");
        assert!(
            bytes.windows(6).any(|w| w == b"marker"),
            "the tEXt chunk did not reach the file"
        );

        // The inverse: PNG keywords are bounded, and a rejection must surface rather
        // than silently drop the chunk the corpus gate is looking for.
        let too_long = "k".repeat(80);
        assert!(
            png_with_text(
                source.as_raw(),
                8,
                8,
                png::ColorType::Grayscale,
                &[(too_long.as_str(), "value")],
            )
            .is_err()
        );
    }
}
