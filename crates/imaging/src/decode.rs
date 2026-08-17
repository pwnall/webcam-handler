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
use schema::vocabulary::closed_vocabulary;
use yuv::{
    YuvBiPlanarImage, YuvConversionMode, YuvPackedImage, YuvRange, YuvStandardMatrix,
    yuv_nv12_to_rgb, yuyv422_to_rgb,
};
use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::bytestream::ZCursor;
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;

use crate::fault::{imaging_failure, short_buffer};

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
        return Err(Error::format_unsupported(
            Some(pixel_format),
            SourceFormat::all_pixel_formats(),
        ));
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

    let plane = packed_422_plane(bytes, stride, h, width, height)?;
    let mut rgb = rgb_buffer(w, h, YUYV_OP)?;
    let rgb_stride = row_stride(w, YUYV_OP)?;
    let packed = YuvPackedImage {
        yuy: plane.as_ref(),
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
///
/// `pub(crate)` since P6b: [`crate::y4m`] rearranges the same raw formats into planes rather
/// than into pixels, and the arithmetic that decides *how many bytes a driver owes us* is one
/// law (AGENTS "one home per law"). A second copy in the Y4M sink would be a second answer to
/// "what does `bytes_per_line: 0` mean", and the two would drift on the day one of them
/// learned about a new format.
pub(crate) fn geometry(width: u32, height: u32, operation: &str) -> Result<(usize, usize)> {
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
///
/// `pub(crate)` for [`geometry`]'s reason.
pub(crate) fn stride_of(bytes_per_line: u32, row_bytes: usize, operation: &str) -> Result<usize> {
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
///
/// `pub(crate)` for [`geometry`]'s reason.
pub(crate) fn plane_bytes(stride: usize, rows: usize, row_bytes: usize) -> Option<usize> {
    let full = stride.checked_mul(rows.checked_sub(1)?)?;
    full.checked_add(row_bytes)
}

/// The packed 4:2:2 plane `yuv`'s converter will accept, out of the buffer a driver
/// delivered.
///
/// **Where two rules about the same buffer are reconciled, and the one that wins is ours.**
/// [`plane_bytes`] is this crate's law — full strides for every row but the last, whose
/// padding "is not something a driver owes us" — and `decode_grey` and `decode_nv12` both
/// hold callers to exactly that. The `yuv` crate's `check_yuv_packed422` holds them to
/// something else: it compares the packed buffer's length against `stride × height` with
/// `!=`, so a final row delivered without its padding is *short* by that comparison and a
/// `bytesused` covering trailing bytes is *long* by it, and both come back as
/// `PackedFrameSizeMismatch` — a refusal in a dependency's words for a frame this crate had
/// already decided was well formed.
///
/// So the length the dependency wants is produced here rather than demanded of the driver.
/// The common case borrows: a tightly packed frame, and a padded one whose last row carries
/// its padding, are already `stride × height` and are handed straight through. A longer
/// buffer is borrowed too, at its first `stride × height` bytes. Only the padding-free final
/// row costs a copy, and what it copies into is the *padding* — bytes the converter walks
/// past to reach the next row and never reads as a sample, since every row's useful span is
/// `width` pixels wide. They are zeroed rather than left uninitialised or filled from
/// anywhere else, because a frame may contain a person (rubric A12) and padding that
/// borrowed from a neighbouring row would be picture.
fn packed_422_plane(
    bytes: &[u8],
    stride: usize,
    rows: usize,
    width: u32,
    height: u32,
) -> Result<Cow<'_, [u8]>> {
    let padded = stride
        .checked_mul(rows)
        .ok_or_else(|| overflowed(YUYV_OP, width, height))?;
    match bytes.len().cmp(&padded) {
        std::cmp::Ordering::Equal => Ok(Cow::Borrowed(bytes)),
        // Unreachable while `padded >= bytes.len()` is false, and written as a `get` rather
        // than an index because a device-derived length decides it (rubric B10).
        std::cmp::Ordering::Greater => bytes
            .get(..padded)
            .map(Cow::Borrowed)
            .ok_or_else(|| short_buffer(YUYV_OP, padded, bytes.len())),
        std::cmp::Ordering::Less => {
            let mut owned = Vec::with_capacity(padded);
            owned.extend_from_slice(bytes);
            owned.resize(padded, 0);
            Ok(Cow::Owned(owned))
        }
    }
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
                // A source-format refusal is about the format, so the size slot stays
                // empty — the discriminator `engine::preview` reads (note **N138**) — and so
                // does the container slot, since a decoder names no file (note **N211**).
                size: None,
                container: None,
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

    // ------------------------------------------------- the chroma orientation (note N130)
    //
    // **Everything below this comment exists because nothing above it could tell Cb from Cr.**
    // Note **N108** recorded the class at P6b and left this half open: the three round trips
    // in this module run on neutral chroma, so `decode_yuyv` and `decode_nv12` could hand the
    // `yuv` crate's converter the two chroma planes the wrong way round and produce
    // colour-inverted photographs with nothing in the workspace noticing — the D8 metrics are
    // computed on luma, and luma is unaffected by the swap. Measured rather than argued:
    // `yuyv422_to_rgb` → `yvyu422_to_rgb` and `yuv_nv12_to_rgb` → `yuv_nv21_to_rgb`, the two
    // one-word edits that spell exactly that defect, passed **1381 of 1381 tests** on
    // 2026-08-15.
    //
    // The repair is a colour fixture (`imaging::fixtures`, whose section comment argues the
    // three properties its samples need) plus an expectation this file derives from BT.601
    // rather than from a run of the code under test — because a fixture whose expectations
    // came out of the implementation cannot catch the implementation being wrong, which is
    // the whole of N108.

    /// The largest per-channel difference between the derivation below and what
    /// `imaging::decode` produced that this project reads as agreement.
    ///
    /// **Two, and the number is an error budget rather than a shrug.** Both sides compute the
    /// same BT.601 matrix; the `yuv` crate does it in 13-bit fixed point, so its coefficients
    /// differ from the exact ratios by under `1/8192` — worth 0.004 of a code over this
    /// fixture's luma span — and it finishes with one rounded right shift, worth half a code.
    /// Two therefore covers rounding twice over and nothing else. What it must not cover is
    /// the defect: a swapped chroma pair moves red by at least 22 codes and blue by at least
    /// 28, and [`assert_decoded_colour_is_what_bt601_says`] asserts that separation at every
    /// pixel rather than leaving it as a claim in this comment.
    const CHROMA_ROUNDING_TOLERANCE: i32 = 2;

    /// BT.601 limited-range Y'CbCr to full-range 8-bit R'G'B', derived from the standard.
    ///
    /// Rec. ITU-R BT.601 fixes the luma coefficients `Kr = 0.299` and `Kb = 0.114`, so
    /// `Kg = 1 - Kr - Kb = 0.587`, and its 8-bit studio quantization puts Y' on \[16, 235\] —
    /// 219 codes — and Cb/Cr on \[16, 240\] — 224 codes — offset about 128. Undo the
    /// quantization first:
    ///
    /// ```text
    /// y  = (Y' -  16) * 255 / 219
    /// cb = (Cb - 128) * 255 / 224
    /// cr = (Cr - 128) * 255 / 224
    /// ```
    ///
    /// then invert the forward transform `Y = Kr·R + Kg·G + Kb·B`, `Cb = (B - Y) / 2(1 - Kb)`,
    /// `Cr = (R - Y) / 2(1 - Kr)`. The first two invert by inspection; substituting them into
    /// the first equation and using `1 - Kr - Kb = Kg` gives the third:
    ///
    /// ```text
    /// R = y + 2(1 - Kr)·cr
    /// B = y + 2(1 - Kb)·cb
    /// G = y - (2·Kr(1 - Kr) / Kg)·cr - (2·Kb(1 - Kb) / Kg)·cb
    /// ```
    ///
    /// The coefficients are written as those expressions and not as the decimals they come to,
    /// so that a reader checks an identity rather than a transcription — and so that no number
    /// here could have been copied from `yuv`'s tables or from a run of [`decode_yuyv`]. The
    /// range and the matrix are the ones this module *passes* ([`YuvRange::Limited`],
    /// [`YuvStandardMatrix::Bt601`]), not the ones this test would prefer: the module's own
    /// "Colour conventions" section is what makes them the right question to ask.
    fn bt601_limited_to_rgb(luma: u8, cb: u8, cr: u8) -> [u8; 3] {
        const KR: f64 = 0.299;
        const KB: f64 = 0.114;
        const KG: f64 = 1.0 - KR - KB;

        let y = (f64::from(luma) - 16.0) * 255.0 / 219.0;
        let cb = (f64::from(cb) - 128.0) * 255.0 / 224.0;
        let cr = (f64::from(cr) - 128.0) * 255.0 / 224.0;

        let red = y + 2.0 * (1.0 - KR) * cr;
        let blue = y + 2.0 * (1.0 - KB) * cb;
        let green = y - (2.0 * KR * (1.0 - KR) / KG) * cr - (2.0 * KB * (1.0 - KB) / KG) * cb;
        [to_channel(red), to_channel(green), to_channel(blue)]
    }

    /// Round to the nearest 8-bit code, saturating at both ends.
    ///
    /// The saturation is a total function and never a licence: the tests below assert that no
    /// channel of this fixture reaches either end, in either chroma orientation, because a
    /// channel pinned at 0 or 255 is constant along the dimension under test and therefore
    /// cannot discriminate — N108's class, arriving at the expectation instead of the fixture.
    fn to_channel(value: f64) -> u8 {
        // Clamped into 0..=255 and rounded to an integer on the line above, so the conversion
        // is exact. `as` is confined to test code, which the crate root's `not(test)` lint
        // block deliberately does not reach.
        value.round().clamp(0.0, 255.0) as u8
    }

    /// The (Y, Cb, Cr) triple `imaging::fixtures`' colour generators put at every pixel, in
    /// raster order.
    ///
    /// `vertical_subsampling` is **1** for YUYV, whose 4:2:2 halves only the horizontal axis,
    /// and **2** for NV12's 4:2:0, which halves both. Written from V4L2's descriptions of the
    /// two formats — the same descriptions the packers' doc comments cite — so that the two
    /// decoders are asked the same question about a chroma sample's *coverage* and not only
    /// about its identity: a build that shared one chroma pair across the wrong two pixels
    /// would be as wrong as one that swapped the planes, and is a different edit.
    fn colour_fixture_triples(
        width: u32,
        height: u32,
        vertical_subsampling: u32,
    ) -> Vec<(u8, u8, u8)> {
        let mut triples = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let (cx, cy) = (x / 2, y / vertical_subsampling);
                triples.push((
                    fixtures::colour_yuv_luma(x, y),
                    fixtures::colour_yuv_cb(cx, cy),
                    fixtures::colour_yuv_cr(cx, cy),
                ));
            }
        }
        triples
    }

    /// Assert a decoded frame is what BT.601 says the fixture's samples are — and that the
    /// fixture could have said otherwise.
    ///
    /// Three claims per pixel, and the second and third are what make the first worth making:
    ///
    /// 1. every channel agrees with [`bt601_limited_to_rgb`] to within
    ///    [`CHROMA_ROUNDING_TOLERANCE`];
    /// 2. no channel of either the correct or the chroma-swapped expectation reaches 0 or 255,
    ///    so no pixel is a dead assertion held there by saturation;
    /// 3. the swapped expectation differs from the correct one, in red *and* in blue, by more
    ///    than the tolerance — so this pixel would have gone red under the swap rather than
    ///    relying on some other pixel to notice.
    fn assert_decoded_colour_is_what_bt601_says(rgb: &RgbImage, triples: &[(u8, u8, u8)]) {
        assert_eq!(
            rgb.pixels().len(),
            triples.len(),
            "the decode produced a different number of pixels than the fixture has samples"
        );
        for (index, (pixel, &(luma, cb, cr))) in rgb.pixels().zip(triples).enumerate() {
            let expected = bt601_limited_to_rgb(luma, cb, cr);
            let swapped = bt601_limited_to_rgb(luma, cr, cb);
            for (channel, name) in [(0usize, "red"), (1, "green"), (2, "blue")] {
                let (got, want) = (i32::from(pixel.0[channel]), i32::from(expected[channel]));
                assert!(
                    (got - want).abs() <= CHROMA_ROUNDING_TOLERANCE,
                    "pixel {index} ({luma}, {cb}, {cr}): {name} decoded as {got}, BT.601 \
                     limited range says {want}"
                );
                assert!(
                    want > 0 && want < 255 && expected[channel] != 0 && swapped[channel] != 0,
                    "pixel {index}: {name} saturates, so this pixel cannot tell a swap apart"
                );
                assert!(
                    swapped[channel] != 255 && expected[channel] != 255,
                    "pixel {index}: {name} saturates white, so this pixel cannot tell a swap apart"
                );
            }
            for (channel, name) in [(0usize, "red"), (2, "blue")] {
                let moved = i32::from(expected[channel]) - i32::from(swapped[channel]);
                assert!(
                    moved.abs() > CHROMA_ROUNDING_TOLERANCE,
                    "pixel {index}: exchanging Cb and Cr moves {name} by {moved}, which this \
                     test's tolerance would absorb — the fixture has stopped discriminating"
                );
            }
        }
    }

    #[test]
    fn a_yuyv_frame_puts_cr_on_red_and_cb_on_blue_rather_than_the_other_way_round() {
        // `decode_yuyv` chooses the orientation by which of the `yuv` crate's four packed
        // 4:2:2 entry points it calls, and `yvyu422_to_rgb` sits one identifier away from
        // `yuyv422_to_rgb`. That one-word edit is a camera whose every recorded colour is
        // inverted about the neutral axis — reds become blues — in photographs whose whole
        // claim is that their samples are the device's own. It passed every test in this
        // workspace until this one existed (note N130).
        let bytes = fixtures::pack_yuyv_colour(32, 16);
        let rgb = decode_yuyv(&bytes, 32, 16, 0).expect("the colour fixture decodes");
        assert_decoded_colour_is_what_bt601_says(&rgb, &colour_fixture_triples(32, 16, 1));
    }

    #[test]
    fn an_nv12_frame_reads_its_interleaved_plane_as_cb_then_cr_over_the_right_two_by_two_block() {
        // The same defect, a different edit: NV12's chroma is one interleaved plane, so the
        // orientation is `yuv_nv12_to_rgb` against `yuv_nv21_to_rgb`, and the sample covers a
        // 2x2 block rather than a horizontal pair. Both halves are asserted — a build that had
        // the planes right and the block wrong would still be a build that colours the wrong
        // pixels.
        let bytes = fixtures::pack_nv12_colour(32, 16);
        let rgb = decode_nv12(&bytes, 32, 16, 0).expect("the colour fixture decodes");
        assert_decoded_colour_is_what_bt601_says(&rgb, &colour_fixture_triples(32, 16, 2));
    }

    #[test]
    fn a_stride_padded_colour_frame_decodes_the_same_colours_as_a_tightly_packed_one() {
        // The orientation claim above has to survive the thing a driver actually does. A
        // padded Y plane moves where the chroma plane starts, and `decode_nv12` computes that
        // start from the *stride* rather than from the useful bytes — get that wrong and every
        // chroma sample is read from the wrong offset, which on this fixture is a colour error
        // rather than the length error the existing short-buffer tests would catch.
        let packed = fixtures::pack_nv12_colour(16, 8);
        let stride = 24;
        let mut padded = Vec::new();
        for row in packed.chunks(16).take(8) {
            padded.extend_from_slice(row);
            padded.extend(std::iter::repeat_n(0xffu8, stride - 16));
        }
        for row in packed[16 * 8..].chunks(16) {
            padded.extend_from_slice(row);
            padded.extend(std::iter::repeat_n(0xffu8, stride - 16));
        }
        let rgb = decode_nv12(&padded, 16, 8, u32::try_from(stride).expect("small"))
            .expect("the padded colour fixture decodes");
        assert_decoded_colour_is_what_bt601_says(&rgb, &colour_fixture_triples(16, 8, 2));
    }

    #[test]
    fn every_raw_decoder_takes_the_buffer_plane_bytes_says_a_driver_owes_it() {
        // [`plane_bytes`] is the one law: *full strides for every row but the last, which
        // needs only its useful bytes* — "padding after the final row is not something a
        // driver owes us". `decode_grey` and `decode_nv12` obeyed it and `decode_yuyv` did
        // not, because the `yuv` crate's `check_yuv_packed422` compares the packed buffer's
        // length against `stride × height` with `!=` rather than with `<`. So one decoder
        // refused a frame its two siblings accepted, and it refused it with the dependency's
        // own words rather than with ours (note **N201**).
        //
        // Both directions of that equality are asserted here, because the defect is the
        // equality and not either side of it: a last row delivered without its padding is
        // *shorter* than `stride × height`, and a driver that reports a `bytesused` covering
        // trailing bytes hands us one that is *longer*. `decode_grey` and `decode_nv12` take
        // both today, so this is the shared rule read from all three ends.
        let width = 16u32;
        let height = 8u32;
        let row_bytes = usize::try_from(width).expect("small") * 2;
        let stride = row_bytes + 8;

        let packed = fixtures::pack_yuyv_colour(width, height);
        let expected = colour_fixture_triples(width, height, 1);

        let mut clipped = Vec::new();
        for (index, row) in packed.chunks(row_bytes).enumerate() {
            clipped.extend_from_slice(row);
            // Every row but the last is padded out to the stride; the last one stops at its
            // useful bytes, which is exactly `plane_bytes`' claim about what a driver owes.
            if index + 1 < usize::try_from(height).expect("small") {
                clipped.extend(std::iter::repeat_n(0xffu8, stride - row_bytes));
            }
        }
        assert_eq!(
            clipped.len(),
            plane_bytes(stride, usize::try_from(height).expect("small"), row_bytes)
                .expect("the fixture's geometry is addressable"),
            "the fixture is not the length the shared rule computes"
        );
        let rgb = decode_yuyv(
            &clipped,
            width,
            height,
            u32::try_from(stride).expect("small"),
        )
        .expect("a last row without padding is a frame");
        assert_decoded_colour_is_what_bt601_says(&rgb, &expected);

        // The other side: a buffer longer than the geometry demands is a driver being
        // generous, not a driver being wrong. `decode_grey` and `decode_nv12` already take
        // one.
        let mut generous = clipped.clone();
        generous.extend(std::iter::repeat_n(0xffu8, stride * 3));
        let rgb = decode_yuyv(
            &generous,
            width,
            height,
            u32::try_from(stride).expect("small"),
        )
        .expect("trailing bytes are not a defect");
        assert_decoded_colour_is_what_bt601_says(&rgb, &expected);

        let grey = fixtures::pack_grey(&fixtures::gradient(4, 4));
        let mut grey_generous = grey.clone();
        grey_generous.extend_from_slice(&[0xff; 9]);
        assert!(
            decode_grey(&grey_generous, 4, 4, 0).is_ok(),
            "the sibling this rule is shared with"
        );
        let mut nv12_generous = fixtures::pack_nv12(&fixtures::gradient(4, 4));
        nv12_generous.extend_from_slice(&[0xff; 9]);
        assert!(
            decode_nv12(&nv12_generous, 4, 4, 0).is_ok(),
            "the other sibling"
        );
    }

    #[test]
    fn a_gray_fixture_survives_the_yuyv_round_trip() {
        // **Kept deliberately, and note N130 says why.** This asserts something the colour
        // tests above do not: that a grey frame comes back grey, luma preserved end to end
        // through the packer this crate ships and the `image` buffer it lands in. What it
        // cannot assert is which chroma plane is which — every byte it compares is 128 either
        // way — and it was read as covering that for two phases, which is the entire finding
        // of N108. The two live side by side; neither is the other's replacement.
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
        // Kept for its sibling's reason: it is the luma claim, and it is not the chroma one.
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
