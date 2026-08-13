//! Frame bytes to pixels — the D6 source-format set, stated closed.
//!
//! MJPG/JPEG, YUYV, NV12 and GREY, and nothing else: the Chicony IR camera makes
//! grayscale non-optional and the OBSBOT makes MJPG the common case, while NV12 rides
//! along because the `yuv` crate covers it next to YUYV. Anything else is
//! [`Error::FormatUnsupported`] naming what this crate *can* read, because a caller
//! holding an unreadable frame needs to know which format to renegotiate to.
//!
//! ## Why every entry point re-derives the geometry
//!
//! `width`, `height` and `bytes_per_line` come from the driver, and `bytes.len()` comes
//! from the driver's `bytesused`. Nothing checks them against each other on the way here.
//! A frame that claims 1920×1080 YUYV in 4 kB is a truncated read in any implementation
//! that trusts the header, so each function computes the bytes the geometry demands, in
//! checked arithmetic, and refuses before it indexes anything (rubric B10, applied to the
//! pure side).
//!
//! ## Colour conventions
//!
//! YUYV and NV12 are decoded as BT.601 limited range, which is what UVC cameras emit and
//! what V4L2 defaults to for these formats. V4L2 *can* signal `colorspace` and
//! `quantization` per format; we do not plumb those yet, so this is a stated assumption
//! rather than a measured fact — a camera that signals full range will decode slightly
//! flat, and the fix is to carry the signalled quantization through
//! `NegotiatedStream`.

use std::borrow::Cow;

use image::{GrayImage, ImageBuffer, RgbImage};
use schema::camera::PixelFormat;
use schema::capture::Frame;
use schema::error::{Error, Result};
use yuv::{
    YuvBiPlanarImage, YuvConversionMode, YuvPackedImage, YuvRange, YuvStandardMatrix,
    yuv_nv12_to_rgb, yuyv422_to_rgb,
};
use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::bytestream::ZCursor;
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;

use crate::fault::{imaging_failure, short_buffer};
use crate::vocabulary::closed_vocabulary;

closed_vocabulary! {
    /// A pixel format this crate can turn into pixels (design D6 closes the set).
    ///
    /// `ALL` is generated from this definition, so a format cannot be added without
    /// joining every walk over the set — including the one that populates
    /// [`Error::FormatUnsupported`]'s `available` list.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub enum SourceFormat {
        /// Motion JPEG: a JPEG bitstream per frame.
        Mjpg,
        /// JPEG, which the kernel spells differently from MJPG but we decode the same.
        Jpeg,
        /// Packed YUV 4:2:2.
        Yuyv,
        /// Planar YUV 4:2:0 with an interleaved chroma plane.
        Nv12,
        /// 8-bit greyscale, already luma.
        Grey,
    }
}

impl SourceFormat {
    /// The fourcc this variant names.
    #[must_use]
    pub const fn pixel_format(self) -> PixelFormat {
        match self {
            SourceFormat::Mjpg => PixelFormat::MJPG,
            SourceFormat::Jpeg => PixelFormat::JPEG,
            SourceFormat::Yuyv => PixelFormat::YUYV,
            SourceFormat::Nv12 => PixelFormat::NV12,
            SourceFormat::Grey => PixelFormat::GREY,
        }
    }

    /// Recognize a fourcc, or `None` when this crate cannot decode it.
    #[must_use]
    pub fn from_pixel_format(format: PixelFormat) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|candidate| candidate.pixel_format() == format)
    }

    /// Every format this crate can decode, as fourccs.
    ///
    /// Derived from [`Self::ALL`], which the vocabulary macro derives from the enum.
    #[must_use]
    pub fn all_pixel_formats() -> Vec<PixelFormat> {
        Self::ALL
            .iter()
            .copied()
            .map(SourceFormat::pixel_format)
            .collect()
    }
}

/// A decoded frame, in the colour model the source actually carried.
///
/// A grayscale source stays grayscale rather than being widened to three identical
/// channels: the Chicony IR camera is a seed device, its frames are luma, and tripling
/// them costs three times the memory to say the same thing. Callers that need RGB ask
/// for it with [`Decoded::to_rgb8`]; callers that need luma — which is every metric —
/// ask with [`Decoded::luma`] and pay no copy when the source was already grey.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decoded {
    /// Three interleaved 8-bit channels.
    Rgb(RgbImage),
    /// One 8-bit luma channel.
    Gray(GrayImage),
}

impl Decoded {
    /// Frame width in pixels.
    #[must_use]
    pub fn width(&self) -> u32 {
        match self {
            Decoded::Rgb(image) => image.width(),
            Decoded::Gray(image) => image.width(),
        }
    }

    /// Frame height in pixels.
    #[must_use]
    pub fn height(&self) -> u32 {
        match self {
            Decoded::Rgb(image) => image.height(),
            Decoded::Gray(image) => image.height(),
        }
    }

    /// A luma view of the frame, borrowed when the frame is already luma.
    ///
    /// Every D8 metric wants one of these, and calibration computes five metrics per
    /// sample: converting once per metric would be four wasted conversions per photo.
    #[must_use]
    pub fn luma(&self) -> Cow<'_, GrayImage> {
        match self {
            Decoded::Gray(image) => Cow::Borrowed(image),
            Decoded::Rgb(image) => Cow::Owned(rgb_to_luma(image)),
        }
    }

    /// An RGB view of the frame, borrowed when the frame is already RGB.
    #[must_use]
    pub fn to_rgb8(&self) -> Cow<'_, RgbImage> {
        match self {
            Decoded::Rgb(image) => Cow::Borrowed(image),
            Decoded::Gray(image) => Cow::Owned(luma_to_rgb(image)),
        }
    }
}

/// BT.601 luma, in integers.
///
/// The same coefficients the YUV decode path uses, so a YUYV frame's luma round-trips
/// through RGB to within rounding rather than drifting to a different definition of
/// "brightness" — which would make two samples in one sweep incomparable.
fn rgb_to_luma(image: &RgbImage) -> GrayImage {
    // Built by zipping two same-length pixel iterators rather than by `from_raw`, so
    // there is no length invariant to assert and no `Option` to unwrap.
    let mut out = GrayImage::new(image.width(), image.height());
    for (destination, source) in out.pixels_mut().zip(image.pixels()) {
        let [r, g, b] = source.0;
        let weighted = 77 * u32::from(r) + 150 * u32::from(g) + 29 * u32::from(b);
        // 77 + 150 + 29 == 256, so the sum is at most 255 << 8 and the shift is exact.
        destination.0 = [u8::try_from(weighted >> 8).unwrap_or(u8::MAX)];
    }
    out
}

fn luma_to_rgb(image: &GrayImage) -> RgbImage {
    let mut out = RgbImage::new(image.width(), image.height());
    for (destination, source) in out.pixels_mut().zip(image.pixels()) {
        let value = source.0[0];
        destination.0 = [value, value, value];
    }
    out
}

/// Decode a captured frame using the geometry the driver reported with it.
///
/// # Errors
///
/// [`Error::FormatUnsupported`] for a format outside the D6 set, and
/// [`Error::DeviceIo`] naming the decode step for a frame whose bytes do not match its
/// geometry or whose bitstream will not parse.
pub fn decode_frame(frame: &Frame) -> Result<Decoded> {
    decode(
        &frame.bytes,
        frame.pixel_format,
        frame.width,
        frame.height,
        frame.bytes_per_line,
    )
}

/// Decode frame bytes in a named format.
///
/// For the compressed formats `width` and `height` bound the decode rather than describe
/// it: the bitstream carries its own dimensions (it is the authority on itself, E2), and
/// these cap how large a frame a lying JPEG header can make us allocate. For the raw
/// formats they *are* the geometry, together with `bytes_per_line` — which is `0` when
/// the driver means "tightly packed".
///
/// # Errors
///
/// As [`decode_frame`].
pub fn decode(
    bytes: &[u8],
    pixel_format: PixelFormat,
    width: u32,
    height: u32,
    bytes_per_line: u32,
) -> Result<Decoded> {
    let Some(source) = SourceFormat::from_pixel_format(pixel_format) else {
        return Err(Error::FormatUnsupported {
            requested: Some(pixel_format),
            available: SourceFormat::all_pixel_formats(),
        });
    };
    match source {
        SourceFormat::Mjpg | SourceFormat::Jpeg => decode_jpeg(bytes, width, height),
        SourceFormat::Yuyv => decode_yuyv(bytes, width, height, bytes_per_line).map(Decoded::Rgb),
        SourceFormat::Nv12 => decode_nv12(bytes, width, height, bytes_per_line).map(Decoded::Rgb),
        SourceFormat::Grey => decode_grey(bytes, width, height, bytes_per_line).map(Decoded::Gray),
    }
}

/// Headroom on the JPEG allocation cap, in pixels.
///
/// One MCU. A camera whose bitstream is a few pixels larger than the size it negotiated
/// is doing something odd, not something dangerous, and refusing its photo outright
/// would be a capability claim made from a rounding difference (E3). The cap still stops
/// the failure mode it exists for — a header claiming thousands of pixels more than the
/// negotiation — by orders of magnitude.
pub const JPEG_BOUND_SLACK: u32 = 16;

const JPEG_OP: &str = "decode JPEG frame";
const YUYV_OP: &str = "decode YUYV frame";
const NV12_OP: &str = "decode NV12 frame";
const GREY_OP: &str = "decode GREY frame";

/// Decode a JPEG (or MJPG) bitstream.
///
/// `max_width`/`max_height` are the negotiated frame size and act as an **allocation
/// cap**, not as a dimension check: the bitstream is the authority on its own size (E2),
/// and the returned image carries whatever the header said. What the cap prevents is a
/// header claiming 16384×16384 turning a 4 kB frame into an 800 MB allocation. It is
/// applied with [`JPEG_BOUND_SLACK`] pixels of headroom so a camera that rounds its own
/// dimensions is decoded rather than refused.
///
/// A JPEG whose components say grayscale decodes to [`Decoded::Gray`]. Widening it to
/// RGB would triple the buffer to store the same numbers, and every metric would convert
/// it straight back.
///
/// # Errors
///
/// [`Error::DeviceIo`] when the buffer is not a JPEG, exceeds the allocation cap, or
/// will not parse.
pub fn decode_jpeg(bytes: &[u8], max_width: u32, max_height: u32) -> Result<Decoded> {
    // Cheapest possible rejection of a buffer that is not a JPEG at all — an empty
    // `bytesused`, or a driver that handed back an unwritten buffer.
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return Err(imaging_failure(
            JPEG_OP,
            format!(
                "buffer of {} bytes does not begin with the JPEG SOI marker",
                bytes.len()
            ),
        ));
    }
    let (bound_width, bound_height) = bounds(
        max_width.saturating_add(JPEG_BOUND_SLACK),
        max_height.saturating_add(JPEG_BOUND_SLACK),
        JPEG_OP,
    )?;
    let base = DecoderOptions::default()
        .set_max_width(bound_width)
        .set_max_height(bound_height);

    // The output colourspace is bound when the headers are parsed, so the input
    // colourspace has to be known before the decoding pass starts. Parsing the headers
    // twice costs a few microseconds and buys a grayscale path that is not a guess.
    let mut probe = JpegDecoder::new_with_options(ZCursor::new(bytes), base);
    probe
        .decode_headers()
        .map_err(|err| imaging_failure(JPEG_OP, err.to_string()))?;
    let grayscale = probe.input_colorspace() == Some(ColorSpace::Luma);
    drop(probe);

    let colorspace = if grayscale {
        ColorSpace::Luma
    } else {
        ColorSpace::RGB
    };
    let mut decoder = JpegDecoder::new_with_options(
        ZCursor::new(bytes),
        base.jpeg_set_out_colorspace(colorspace),
    );
    decoder
        .decode_headers()
        .map_err(|err| imaging_failure(JPEG_OP, err.to_string()))?;
    let (info, size) = match (decoder.info(), decoder.output_buffer_size()) {
        (Some(info), Some(size)) => (info, size),
        // Unreachable while `decode_headers` returned `Ok`; kept typed because the only
        // alternative the dependency offers is its own `unwrap`.
        _ => {
            return Err(imaging_failure(
                JPEG_OP,
                "the decoder reported no image information after parsing headers",
            ));
        }
    };
    let mut pixels = vec![0u8; size];
    decoder
        .decode_into(&mut pixels)
        .map_err(|err| imaging_failure(JPEG_OP, err.to_string()))?;

    let width = u32::from(info.width);
    let height = u32::from(info.height);
    if grayscale {
        ImageBuffer::from_raw(width, height, pixels)
            .map(Decoded::Gray)
            .ok_or_else(|| buffer_mismatch(JPEG_OP, width, height))
    } else {
        ImageBuffer::from_raw(width, height, pixels)
            .map(Decoded::Rgb)
            .ok_or_else(|| buffer_mismatch(JPEG_OP, width, height))
    }
}

/// Convert a packed YUYV 4:2:2 frame to RGB8.
///
/// # Errors
///
/// [`Error::DeviceIo`] for a zero dimension, a `bytes_per_line` below one row, or a
/// buffer shorter than the geometry demands.
pub fn decode_yuyv(bytes: &[u8], width: u32, height: u32, bytes_per_line: u32) -> Result<RgbImage> {
    let (w, h) = geometry(width, height, YUYV_OP)?;
    // Two pixels share one chroma pair, so an odd width still occupies a whole pair.
    let row_bytes = w
        .checked_next_multiple_of(2)
        .and_then(|pairs| pairs.checked_mul(2))
        .ok_or_else(|| overflowed(YUYV_OP, width, height))?;
    let stride = stride_of(bytes_per_line, row_bytes, YUYV_OP)?;
    let needed =
        plane_bytes(stride, h, row_bytes).ok_or_else(|| overflowed(YUYV_OP, width, height))?;
    if bytes.len() < needed {
        return Err(short_buffer(YUYV_OP, needed, bytes.len()));
    }

    let mut rgb = rgb_buffer(w, h, YUYV_OP)?;
    let rgb_stride = row_stride(w, YUYV_OP)?;
    let packed = YuvPackedImage {
        yuy: bytes,
        yuy_stride: as_u32(stride, YUYV_OP)?,
        width,
        height,
    };
    yuyv422_to_rgb(
        &packed,
        &mut rgb,
        rgb_stride,
        YuvRange::Limited,
        YuvStandardMatrix::Bt601,
    )
    .map_err(|err| imaging_failure(YUYV_OP, err.to_string()))?;
    ImageBuffer::from_raw(width, height, rgb).ok_or_else(|| buffer_mismatch(YUYV_OP, width, height))
}

/// Convert a planar NV12 frame (Y plane then an interleaved UV plane) to RGB8.
///
/// # Errors
///
/// As [`decode_yuyv`].
pub fn decode_nv12(bytes: &[u8], width: u32, height: u32, bytes_per_line: u32) -> Result<RgbImage> {
    let (w, h) = geometry(width, height, NV12_OP)?;
    let y_stride = stride_of(bytes_per_line, w, NV12_OP)?;
    // 4:2:0 halves both axes, and an odd axis still costs a whole chroma sample.
    let chroma_rows = h.div_ceil(2);
    let chroma_row_bytes = w
        .div_ceil(2)
        .checked_mul(2)
        .ok_or_else(|| overflowed(NV12_OP, width, height))?;

    // The UV plane starts where the Y plane's *stride* ends, not where its last useful
    // byte does — the padding of the final Y row is part of the Y plane.
    let y_plane = y_stride
        .checked_mul(h)
        .ok_or_else(|| overflowed(NV12_OP, width, height))?;
    let uv_plane = plane_bytes(y_stride, chroma_rows, chroma_row_bytes)
        .ok_or_else(|| overflowed(NV12_OP, width, height))?;
    let needed = y_plane
        .checked_add(uv_plane)
        .ok_or_else(|| overflowed(NV12_OP, width, height))?;
    if bytes.len() < needed {
        return Err(short_buffer(NV12_OP, needed, bytes.len()));
    }
    let Some((y, uv)) = bytes.split_at_checked(y_plane) else {
        return Err(short_buffer(NV12_OP, needed, bytes.len()));
    };

    let mut rgb = rgb_buffer(w, h, NV12_OP)?;
    let rgb_stride = row_stride(w, NV12_OP)?;
    let planar = YuvBiPlanarImage {
        y_plane: y,
        y_stride: as_u32(y_stride, NV12_OP)?,
        uv_plane: uv,
        uv_stride: as_u32(y_stride, NV12_OP)?,
        width,
        height,
    };
    yuv_nv12_to_rgb(
        &planar,
        &mut rgb,
        rgb_stride,
        YuvRange::Limited,
        YuvStandardMatrix::Bt601,
        YuvConversionMode::default(),
    )
    .map_err(|err| imaging_failure(NV12_OP, err.to_string()))?;
    ImageBuffer::from_raw(width, height, rgb).ok_or_else(|| buffer_mismatch(NV12_OP, width, height))
}

/// Read a GREY frame as luma.
///
/// GREY is already 8-bit luma, so "decoding" is dropping each row's stride padding. The
/// Chicony IR camera offers nothing else, which is why this path exists at all.
///
/// # Errors
///
/// As [`decode_yuyv`].
pub fn decode_grey(
    bytes: &[u8],
    width: u32,
    height: u32,
    bytes_per_line: u32,
) -> Result<GrayImage> {
    let (w, h) = geometry(width, height, GREY_OP)?;
    let stride = stride_of(bytes_per_line, w, GREY_OP)?;
    let needed = plane_bytes(stride, h, w).ok_or_else(|| overflowed(GREY_OP, width, height))?;
    if bytes.len() < needed {
        return Err(short_buffer(GREY_OP, needed, bytes.len()));
    }

    let mut packed = Vec::with_capacity(needed);
    for row in bytes.chunks(stride).take(h) {
        let Some(useful) = row.get(..w) else {
            return Err(short_buffer(GREY_OP, needed, bytes.len()));
        };
        packed.extend_from_slice(useful);
    }
    ImageBuffer::from_raw(width, height, packed)
        .ok_or_else(|| buffer_mismatch(GREY_OP, width, height))
}

/// Reject a degenerate frame and widen the dimensions for byte arithmetic.
fn geometry(width: u32, height: u32, operation: &str) -> Result<(usize, usize)> {
    if width == 0 || height == 0 {
        return Err(imaging_failure(
            operation,
            format!("frame geometry is {width}x{height}; a zero-sized frame has no pixels"),
        ));
    }
    bounds(width, height, operation)
}

fn bounds(width: u32, height: u32, operation: &str) -> Result<(usize, usize)> {
    match (usize::try_from(width), usize::try_from(height)) {
        (Ok(w), Ok(h)) => Ok((w, h)),
        _ => Err(overflowed(operation, width, height)),
    }
}

/// The driver's row stride, or the packed stride when it reports `0`.
///
/// A stride *below* one row of pixels is a driver contradicting itself; believing it
/// would read one row's data as the next row's.
fn stride_of(bytes_per_line: u32, row_bytes: usize, operation: &str) -> Result<usize> {
    if bytes_per_line == 0 {
        return Ok(row_bytes);
    }
    let stride = usize::try_from(bytes_per_line).map_err(|_| {
        imaging_failure(
            operation,
            format!("bytes_per_line {bytes_per_line} does not fit this machine's address space"),
        )
    })?;
    if stride < row_bytes {
        return Err(imaging_failure(
            operation,
            format!("bytes_per_line is {stride} but one row of pixels needs {row_bytes}"),
        ));
    }
    Ok(stride)
}

/// The bytes a plane occupies: full strides for every row but the last, which needs only
/// its useful bytes. Padding after the final row is not something a driver owes us.
fn plane_bytes(stride: usize, rows: usize, row_bytes: usize) -> Option<usize> {
    let full = stride.checked_mul(rows.checked_sub(1)?)?;
    full.checked_add(row_bytes)
}

fn rgb_buffer(width: usize, height: usize, operation: &str) -> Result<Vec<u8>> {
    let len = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(3))
        .ok_or_else(|| {
            imaging_failure(
                operation,
                format!("{width}x{height} RGB overflows a buffer length"),
            )
        })?;
    Ok(vec![0u8; len])
}

fn row_stride(width: usize, operation: &str) -> Result<u32> {
    let bytes = width.checked_mul(3).ok_or_else(|| {
        imaging_failure(operation, format!("an RGB row of {width} pixels overflows"))
    })?;
    as_u32(bytes, operation)
}

fn as_u32(value: usize, operation: &str) -> Result<u32> {
    u32::try_from(value)
        .map_err(|_| imaging_failure(operation, format!("{value} does not fit a 32-bit stride")))
}

fn overflowed(operation: &str, width: u32, height: u32) -> Error {
    imaging_failure(
        operation,
        format!("frame geometry {width}x{height} overflows this machine's address space"),
    )
}

fn buffer_mismatch(operation: &str, width: u32, height: u32) -> Error {
    imaging_failure(
        operation,
        format!("the decoded buffer does not match the {width}x{height} geometry it was built for"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    /// Limited-range BT.601 white and black, as a YUYV pixel pair.
    const WHITE_Y: u8 = 235;
    const BLACK_Y: u8 = 16;
    const NEUTRAL_C: u8 = 128;

    fn yuyv_pair(first: u8, second: u8) -> [u8; 4] {
        [first, NEUTRAL_C, second, NEUTRAL_C]
    }

    #[test]
    fn the_supported_set_is_the_d6_set_and_nothing_else() {
        let formats = SourceFormat::all_pixel_formats();
        assert!(formats.contains(&PixelFormat::MJPG));
        assert!(formats.contains(&PixelFormat::JPEG));
        assert!(formats.contains(&PixelFormat::YUYV));
        assert!(formats.contains(&PixelFormat::NV12));
        assert!(formats.contains(&PixelFormat::GREY));
        assert_eq!(formats.len(), SourceFormat::ALL.len());
        assert_eq!(
            SourceFormat::from_pixel_format(PixelFormat::parse("H264").expect("fourcc")),
            None
        );
    }

    #[test]
    fn the_format_ranking_never_prefers_a_format_this_crate_cannot_decode() {
        // D5's amendment of 2026-08-13 sorts a FourCC the schema cannot classify behind
        // every one it can, *ahead of resolution*, and the argument for that ordering is
        // this crate: a format outside D6's set produces no photograph, only a
        // `FormatUnsupported`. The argument holds only while the two sets are the same
        // set, and nothing but this test says so — the schema knows nothing about which
        // crate decodes what, and this crate knows nothing about the ranking.
        //
        // Both directions, because a check that only ever sees agreement cannot
        // discriminate: every format this crate reads is one the schema names, and a
        // format the schema does not name is one this crate refuses.
        for &source in SourceFormat::ALL {
            let format = source.pixel_format();
            assert!(
                schema::camera::Lossiness::of(format).is_named(),
                "{format} decodes here and ranks as unknown in the schema, so the ranking \
                 would sort a decodable format to the back"
            );
        }
        for name in ["H264", "HEVC", "RGB3", "Y16 "] {
            let format = PixelFormat::parse(name).expect("four characters");
            assert_eq!(SourceFormat::from_pixel_format(format), None, "{name}");
            assert!(
                !schema::camera::Lossiness::of(format).is_named(),
                "{name} ranks as a known format in the schema and cannot be decoded here, \
                 so the ranking could prefer it and hand a caller a refusal"
            );
        }
    }

    #[test]
    fn an_unsupported_format_names_what_we_can_read_instead() {
        // The actionable half: a caller holding an H.264 frame needs to know which
        // format to renegotiate to, not merely that this one failed.
        let h264 = PixelFormat::parse("H264").expect("fourcc");
        let err = decode(&[0u8; 16], h264, 4, 4, 0).expect_err("H264 is outside D6");
        match err {
            Error::FormatUnsupported {
                requested,
                available,
            } => {
                assert_eq!(requested, Some(h264));
                assert!(available.contains(&PixelFormat::MJPG), "{available:?}");
                assert!(available.contains(&PixelFormat::GREY), "{available:?}");
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    /// Assert the refusal came from *this crate's* length check, before the bytes were
    /// touched.
    ///
    /// The distinction is the whole point and it is not visible in `is_err()`: the `yuv`
    /// crate validates too, so a decode that deleted its own check still returns an
    /// error — one that happens to quote the same two numbers. A mutation that removed
    /// the check in `decode_yuyv` survived a test asserting only on those numbers.
    fn assert_our_own_length_check_refused(err: &Error, operation: &str) {
        match err {
            Error::DeviceIo {
                operation: named,
                errno,
                message,
            } => {
                assert_eq!(named, operation);
                assert_eq!(*errno, None, "no syscall was involved");
                assert!(
                    message.starts_with(crate::fault::SHORT_BUFFER_PREFIX),
                    "the refusal came from somewhere else: {message}"
                );
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn a_short_yuyv_buffer_is_a_typed_error_not_a_panic_or_a_short_read() {
        // The defect this test exists for: a driver reporting 640x480 while delivering
        // one row. Trusting the geometry reads 613 kB past the end of the buffer.
        let one_row = vec![0u8; 640 * 2];
        let err = decode_yuyv(&one_row, 640, 480, 0).expect_err("one row is not a frame");
        assert_our_own_length_check_refused(&err, YUYV_OP);
        let rendered = err.to_string();
        assert!(rendered.contains("614400"), "{rendered}");
        assert!(rendered.contains("1280"), "{rendered}");

        // And the exact-length buffer decodes, so the bound is the real one and not a
        // blanket refusal.
        let whole = vec![0u8; 640 * 480 * 2];
        assert!(decode_yuyv(&whole, 640, 480, 0).is_ok());
        // One byte short still fails: the check is `<`, not "roughly".
        let one_short = vec![0u8; 640 * 480 * 2 - 1];
        let err = decode_yuyv(&one_short, 640, 480, 0).expect_err("one byte short");
        assert_our_own_length_check_refused(&err, YUYV_OP);
    }

    #[test]
    fn a_short_buffer_is_refused_in_every_raw_format() {
        let grey = decode_grey(&[0u8; 15], 4, 4, 0).expect_err("one byte short");
        assert_our_own_length_check_refused(&grey, GREY_OP);
        assert!(decode_grey(&[0u8; 16], 4, 4, 0).is_ok());

        // NV12 needs its chroma plane, which a Y-plane-sized buffer does not have.
        let nv12 = decode_nv12(&[0u8; 16], 4, 4, 0).expect_err("no chroma plane");
        assert_our_own_length_check_refused(&nv12, NV12_OP);
        assert!(decode_nv12(&[0u8; 24], 4, 4, 0).is_ok());
    }

    #[test]
    fn a_stride_below_one_row_is_refused_rather_than_believed() {
        // A driver contradicting itself: 640 pixels of luma cannot live in 320 bytes.
        // Believing it would read the next row's data as this row's.
        let buffer = vec![0u8; 640 * 480];
        let err = decode_grey(&buffer, 640, 480, 320).expect_err("stride is short");
        assert!(err.to_string().contains("bytes_per_line"), "{err}");
    }

    #[test]
    fn stride_padding_is_dropped_rather_than_decoded() {
        // 2x2 luma in a 4-byte stride: the padding bytes must not reach the image.
        let padded = vec![10, 20, 0xff, 0xff, 30, 40, 0xff, 0xff];
        let image = decode_grey(&padded, 2, 2, 4).expect("padded rows decode");
        assert_eq!(image.as_raw(), &[10, 20, 30, 40]);
    }

    #[test]
    fn a_zero_sized_frame_is_refused() {
        assert!(decode_grey(&[], 0, 4, 0).is_err());
        assert!(decode_grey(&[], 4, 0, 0).is_err());
    }

    #[test]
    fn yuyv_limited_range_extremes_land_on_black_and_white() {
        // The expectation is the BT.601 limited-range definition, stated independently
        // of the conversion library: Y=16 is black, Y=235 is white, neutral chroma.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&yuyv_pair(BLACK_Y, BLACK_Y));
        bytes.extend_from_slice(&yuyv_pair(WHITE_Y, WHITE_Y));
        let rgb = decode_yuyv(&bytes, 2, 2, 4).expect("2x2 yuyv decodes");
        let raw = rgb.as_raw();
        assert!(raw.iter().take(6).all(|&c| c <= 2), "black row: {raw:?}");
        assert!(raw.iter().skip(6).all(|&c| c >= 253), "white row: {raw:?}");
    }

    #[test]
    fn a_gray_fixture_survives_the_yuyv_round_trip() {
        let source = fixtures::gradient(16, 8);
        let packed = fixtures::pack_yuyv(&source);
        let rgb = decode_yuyv(&packed, 16, 8, 0).expect("packed fixture decodes");
        let recovered = Decoded::Rgb(rgb).luma().into_owned();
        for (before, after) in source.as_raw().iter().zip(recovered.as_raw()) {
            let drift = i32::from(*before) - i32::from(*after);
            assert!(drift.abs() <= 3, "luma drifted by {drift}");
        }
    }

    #[test]
    fn a_gray_fixture_survives_the_nv12_round_trip() {
        let source = fixtures::gradient(16, 8);
        let packed = fixtures::pack_nv12(&source);
        let rgb = decode_nv12(&packed, 16, 8, 0).expect("packed fixture decodes");
        let recovered = Decoded::Rgb(rgb).luma().into_owned();
        for (before, after) in source.as_raw().iter().zip(recovered.as_raw()) {
            let drift = i32::from(*before) - i32::from(*after);
            assert!(drift.abs() <= 3, "luma drifted by {drift}");
        }
    }

    #[test]
    fn a_grayscale_jpeg_decodes_as_luma_not_as_three_copies_of_it() {
        // The Chicony IR camera's shape. A decoder that only knows YCbCr either fails
        // here or silently produces a green cast.
        let source = fixtures::checkerboard(32, 32, 8);
        let jpeg = crate::encode::jpeg(&Decoded::Gray(source), 92).expect("encode");
        let decoded = decode_jpeg(&jpeg, 32, 32).expect("grayscale jpeg decodes");
        match decoded {
            Decoded::Gray(image) => assert_eq!(image.dimensions(), (32, 32)),
            Decoded::Rgb(_) => panic!("a grayscale JPEG must not widen to RGB"),
        }
    }

    #[test]
    fn a_colour_jpeg_decodes_as_rgb() {
        let source = fixtures::colour_bars(24, 8);
        let jpeg = crate::encode::jpeg(&Decoded::Rgb(source), 92).expect("encode");
        let decoded = decode_jpeg(&jpeg, 24, 8).expect("colour jpeg decodes");
        match decoded {
            Decoded::Rgb(image) => assert_eq!(image.dimensions(), (24, 8)),
            Decoded::Gray(_) => panic!("a colour JPEG must not collapse to luma"),
        }
    }

    #[test]
    fn a_buffer_that_is_not_a_jpeg_is_refused_before_the_decoder_sees_it() {
        let err = decode_jpeg(&[0u8; 64], 640, 480).expect_err("zeroed buffer is not a JPEG");
        assert!(err.to_string().contains("SOI"), "{err}");
        assert!(decode_jpeg(&[], 640, 480).is_err());
    }

    #[test]
    fn a_jpeg_far_larger_than_the_negotiated_frame_is_refused_not_allocated_for() {
        // A header claiming vastly more than the negotiation is how a small buffer turns
        // into a huge allocation. The cap is what stops that, and it is not a dimension
        // check: the slack below is decoded, not refused.
        let source = fixtures::gradient(64, 64);
        let jpeg = crate::encode::jpeg(&Decoded::Gray(source), 80).expect("encode");
        assert!(decode_jpeg(&jpeg, 64, 64).is_ok());
        assert!(
            decode_jpeg(&jpeg, 32, 32).is_err(),
            "a 64x64 bitstream must not decode into a 32x32 negotiation"
        );
        // A camera a few pixels off its own negotiated size is odd, not dangerous, and
        // refusing its photo would be a capability claim made from a rounding
        // difference (E3).
        assert!(
            decode_jpeg(&jpeg, 64 - JPEG_BOUND_SLACK, 64 - JPEG_BOUND_SLACK).is_ok(),
            "the slack is headroom and must be usable"
        );
        assert!(decode_jpeg(&jpeg, 64 - JPEG_BOUND_SLACK - 1, 64,).is_err());
    }

    #[test]
    fn decode_dispatches_on_the_frame_the_driver_described() {
        let frame = Frame {
            bytes: fixtures::pack_grey(&fixtures::gradient(8, 4)),
            pixel_format: PixelFormat::GREY,
            width: 8,
            height: 4,
            bytes_per_line: 8,
            sequence: 1,
            timestamp_us: 0,
        };
        let decoded = decode_frame(&frame).expect("grey frame decodes");
        assert_eq!((decoded.width(), decoded.height()), (8, 4));
        assert!(matches!(decoded, Decoded::Gray(_)));
    }

    #[test]
    fn a_luma_view_of_a_gray_frame_costs_no_copy() {
        let gray = Decoded::Gray(fixtures::gradient(8, 8));
        assert!(matches!(gray.luma(), Cow::Borrowed(_)));
        let rgb = Decoded::Rgb(fixtures::colour_bars(8, 8));
        assert!(matches!(rgb.luma(), Cow::Owned(_)));
        assert!(matches!(rgb.to_rgb8(), Cow::Borrowed(_)));
        assert!(matches!(gray.to_rgb8(), Cow::Owned(_)));
    }

    #[test]
    fn luma_of_a_grey_widening_is_the_original_luma() {
        let source = fixtures::gradient(9, 5);
        let widened = Decoded::Gray(source.clone()).to_rgb8().into_owned();
        let back = Decoded::Rgb(widened).luma().into_owned();
        assert_eq!(back.as_raw(), source.as_raw());
    }

    #[test]
    fn an_error_message_never_carries_frame_bytes() {
        // Rubric A12 as a test: the only numbers in a decode refusal are geometry.
        let secret = vec![0xab; 7];
        let err = decode_grey(&secret, 640, 480, 0).expect_err("short");
        let rendered = err.to_string();
        assert!(!rendered.contains("171"), "{rendered}");
        assert!(!rendered.contains("0xab"), "{rendered}");
    }
}
