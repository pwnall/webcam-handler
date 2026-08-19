//! Two photographs, measured against each other (design D17; FR-W3).
//!
//! "Same scene, two paths" — two units of one model, before and after a firmware update, one
//! camera direct and one forwarded — and the answer is a record rather than a verdict:
//! [`schema::metrics::PhotoComparison`] carries the D8 metric set on each side, the
//! per-metric delta, and an SSIM corroborator. Metrics rank, SSIM corroborates, and neither
//! decides — the same sentence D8 has carried since P0, one consumer over.
//!
//! ## The core is total
//!
//! [`photos`] cannot fail. Two images of different sizes still have well-defined per-metric
//! scalars, so they are still computed and reported; SSIM is what needs a pixel-for-pixel
//! correspondence, so *it* answers
//! [`schema::metrics::SsimUnavailable::DimensionsDiffer`] with both shapes on it. That is
//! D2's doctrine applied to a comparison: an unattended consumer branches on data, never on
//! a parsed refusal, and D17 therefore adds no error kind.
//!
//! **Nothing here resizes, ever.** A comparison that resampled one side to match the other
//! would be measuring an artifact it introduced itself, and reporting it as a difference
//! between two cameras (E6's rule, one layer up from the codec it was written about).
//!
//! ## Where SSIM comes from
//!
//! `image-compare`'s `MSSIMSimple` — SSIM over 8×8 windows, averaged — adopted at P8b under
//! design §2.8's conditional: its own `image` edge is `default-features = false`, so nothing
//! it brings re-opens the `avif` door that drags rav1e through workspace-wide feature
//! unification. The measurement that cleared the condition is note **N260**;
//! `feature-posture.sh` is the standing backstop that makes the trap impossible to re-open
//! quietly.
//!
//! ## Reading the files
//!
//! [`read`] accepts exactly what the *product* writes — every member of
//! [`schema::capture::PhotoFormat`], walked — because "a photograph this tool produced" is
//! the only population `photo diff` has any business accepting, and a reader that took less
//! than the writer emits would refuse the tool's own output. What the product writes is
//! [`crate::encode`] **and the stamp one layer up**: `engine::photo::take` puts an EXIF
//! Orientation on every JPEG it produces, the verbatim sink included, because on that sink
//! the orientation *is* the transform (D6, E6). A reader tested against the inner half alone
//! is a reader that drops the tag, which is one frame written to two sinks arriving as two
//! shapes (note **N267**).
//!
//! So the tag is honoured here, and the consequence is stated rather than left to be worked
//! out: [`schema::metrics::PhotoMeasurements`]'s `width` and `height` are the **displayed**
//! shape of a photograph, not its stored raster's. Applying an orientation is a permutation
//! of pixels and never a resample, so the "nothing here resizes, ever" paragraph above still
//! holds of it.
//!
//! ## One colour model
//!
//! Every format read here reaches luma through `decode::luma_sample`, which is BT.601 in
//! integers and the same arithmetic the YUV decode path uses (named rather than linked
//! because it is private to this crate). `image`'s own `to_luma8` is Rec. 709 and answers up
//! to 33 codes differently on saturated colour, so a build that used it would report a
//! difference between a scene's PNG and the same scene's PPM that no camera produced — and
//! would disagree with the mean luma the calibration stream reported for that same
//! photograph (note **N266**). `luma-has-one-home.sh` is the standing check that no second
//! definition lands.

use std::collections::BTreeMap;

use image::GrayImage;
use schema::capture::PhotoFormat;
use schema::error::Result;
use schema::limits;
use schema::metrics::{MetricName, PhotoComparison, PhotoMeasurements, Ssim, SsimUnavailable};

/// The step named in every refusal this module makes.
const READ_OP: &str = "read a photograph for comparison";

/// Compare two decoded photographs.
///
/// Total by construction: every field of the answer is defined for every pair of images, and
/// the one quantity that needs equal shapes says so on itself.
#[must_use]
pub fn photos(a: &GrayImage, b: &GrayImage) -> PhotoComparison {
    let a_metrics = crate::metrics::measure_all(a);
    let b_metrics = crate::metrics::measure_all(b);

    // Over `MetricName::ALL` rather than over either map's keys: the delta's population is
    // the vocabulary, so a metric missing from one side would be a bug this walk exposes
    // instead of a key that quietly vanishes from the document.
    let delta: BTreeMap<MetricName, f64> = MetricName::ALL
        .iter()
        .map(|name| {
            let before = a_metrics.get(name).copied().unwrap_or_default();
            let after = b_metrics.get(name).copied().unwrap_or_default();
            (*name, after - before)
        })
        .collect();

    PhotoComparison {
        a: PhotoMeasurements {
            width: a.width(),
            height: a.height(),
            metrics: a_metrics,
        },
        b: PhotoMeasurements {
            width: b.width(),
            height: b.height(),
            metrics: b_metrics,
        },
        delta,
        ssim: ssim(a, b),
    }
}

/// The structural-similarity corroborator, or a stated reason there is none.
fn ssim(a: &GrayImage, b: &GrayImage) -> Ssim {
    if a.dimensions() != b.dimensions() {
        // Checked here rather than left to the implementation's own refusal, because the
        // answer this design wants carries *both shapes* and a refusal carries neither.
        return Ssim::Unavailable {
            reason: SsimUnavailable::DimensionsDiffer {
                a: [a.width(), a.height()],
                b: [b.width(), b.height()],
            },
        };
    }
    match image_compare::gray_similarity_structure(&image_compare::Algorithm::MSSIMSimple, a, b) {
        Ok(similarity) => Ssim::Measured {
            score: similarity.score,
        },
        Err(refused) => Ssim::Unavailable {
            reason: SsimUnavailable::ComputationFailed {
                message: refused.to_string(),
            },
        },
    }
}

/// Decode a photograph this build could have written, to luma.
///
/// The population is [`PhotoFormat::ALL`] and the dispatch is by the bytes' own magic rather
/// than by a filename, for the reason every other sniff in this crate gives: a caller that
/// renamed a file is still holding the file, and an extension is a claim while a magic number
/// is the thing itself.
///
/// # Errors
///
/// The crate's one refusal home (`fault::imaging_failure`, `DeviceIo` with no
/// errno — D13 has no imaging-local variant), naming what was found: bytes in no format this
/// build writes, and bytes whose own decoder refused them.
pub fn read(bytes: &[u8]) -> Result<GrayImage> {
    let Some(format) = sniff(bytes) else {
        return Err(crate::fault::imaging_failure(
            READ_OP,
            format!(
                "not a photograph this build writes ({}); the first bytes are {}",
                PhotoFormat::ALL
                    .iter()
                    .map(|format| format.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                first_bytes(bytes)
            ),
        ));
    };
    match format {
        // The *bitstream* readers are the `image` crate's, which are the ones `crate::encode`
        // writes with: a round trip that used a second implementation would be measuring the
        // two implementations against each other rather than the file. The *colour model* is
        // this crate's, and the decoders are built per format rather than sniffed a second
        // time by `load_from_memory` so that the orientation can be read off each one before
        // it is consumed — see the module header for both.
        //
        // PNG takes the budget at construction as well as through [`oriented_luma`], because
        // that is the only door the `png` crate's own internal buffers can be bounded through
        // (`PngDecoder::set_limits` leaves them alone, by its own comment) and
        // `PngDecoder::new` is `with_limits(…, Limits::no_limits())`.
        PhotoFormat::Jpeg => image::codecs::jpeg::JpegDecoder::new(std::io::Cursor::new(bytes))
            .map_err(|err| refused(format, &err))
            .and_then(|decoder| oriented_luma(format, decoder)),
        PhotoFormat::Png => {
            image::codecs::png::PngDecoder::with_limits(std::io::Cursor::new(bytes), budget())
                .map_err(|err| refused(format, &err))
                .and_then(|decoder| oriented_luma(format, decoder))
        }
        PhotoFormat::Ppm => read_netpbm(bytes),
    }
}

/// One refusal home for both compressed formats, so the two arms cannot drift into two
/// spellings of "that file would not decode".
fn refused(format: PhotoFormat, refused: &image::ImageError) -> schema::Error {
    crate::fault::imaging_failure(
        READ_OP,
        format!("{} decode failed: {refused}", format.as_str()),
    )
}

/// What this build is willing to allocate for one photograph's raster
/// ([`limits::MAX_PHOTO_DECODE_BYTES`]), in the shape the `image` crate's decoders take it.
///
/// Only the allocation ceiling is set. The two *dimension* ceilings stay unset on purpose: a
/// photograph is refused here for what it would cost, not for how it is shaped, and a camera
/// that one day delivers a very wide, very short frame is a device this build represents
/// rather than an input it declines (rule 6).
fn budget() -> image::Limits {
    let mut budget = image::Limits::no_limits();
    budget.max_alloc = Some(limits::MAX_PHOTO_DECODE_BYTES);
    budget
}

/// One decoder's picture, bounded, turned the way its own metadata says, and measured through
/// the crate's one luma home.
///
/// All three halves are here rather than in the two arms above so that a third compressed
/// format cannot join the reader on some of them. The orientation is read *before* the decoder
/// is consumed because that is the only moment it exists: `image` exposes the tag on
/// `ImageDecoder` and its default is `NoTransforms`, which is what a photograph carrying no
/// EXIF at all answers — so an unstamped file is not turned by this.
///
/// **The size is checked before anything is allocated**, and by this build rather than by the
/// decoder, because the number deciding the allocation came out of the file's header and the
/// allocator's answer to a number it cannot serve is `abort` — no `Failure` document, no exit
/// code, nothing on standard output at all (note **N268**). `image`'s own
/// `ImageReader::decode` reserves against `Limits::default()` for the same reason; building
/// the decoders per format is what took this build off that path, so the reservation comes
/// back explicitly and says what it refused.
fn oriented_luma(format: PhotoFormat, mut decoder: impl image::ImageDecoder) -> Result<GrayImage> {
    let declared = decoder.total_bytes();
    if declared > limits::MAX_PHOTO_DECODE_BYTES {
        let (width, height) = decoder.dimensions();
        return Err(crate::fault::imaging_failure(
            READ_OP,
            format!(
                "the {} header declares {width}x{height}, which is {declared} bytes of raster \
                 and past this build's decode budget of {} bytes",
                format.as_str(),
                limits::MAX_PHOTO_DECODE_BYTES
            ),
        ));
    }
    decoder
        .set_limits(budget())
        .map_err(|err| refused(format, &err))?;
    let orientation = decoder.orientation().map_err(|err| refused(format, &err))?;
    let mut decoded =
        image::DynamicImage::from_decoder(decoder).map_err(|err| refused(format, &err))?;
    decoded.apply_orientation(orientation);
    // `to_rgb8` and then this crate's own conversion, never `to_luma8`: `image`'s is Rec. 709
    // and this crate's is BT.601 (note **N266**). A grey source costs one transient copy and
    // no accuracy — `luma_sample(v, v, v)` is `(256 · v) >> 8`, which is `v`.
    Ok(crate::decode::rgb_to_luma(&decoded.to_rgb8()))
}

/// Which of this build's photo formats these bytes are, by magic number.
fn sniff(bytes: &[u8]) -> Option<PhotoFormat> {
    match bytes {
        [0xFF, 0xD8, 0xFF, ..] => Some(PhotoFormat::Jpeg),
        [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, ..] => Some(PhotoFormat::Png),
        // P5 is binary PGM and P6 is binary PPM; `crate::encode` writes one or the other
        // depending on whether the frame was grey, and `PhotoFormat::Ppm` covers both.
        [b'P', b'5' | b'6', ..] => Some(PhotoFormat::Ppm),
        _ => None,
    }
}

/// The first few bytes, for a refusal that says what it actually found.
fn first_bytes(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "nothing — the file is empty".to_owned();
    }
    let shown: Vec<String> = bytes
        .iter()
        .take(4)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    shown.join(" ")
}

/// Binary PGM (`P5`) and PPM (`P6`), to luma.
///
/// Written here rather than taken from `image`'s `pnm` feature, and the reason is the same
/// one that keeps `image` on `default-features = false` at all: a feature door opened for one
/// reader is opened workspace-wide by cargo's feature unification, and this is forty lines of
/// header parsing over a format `crate::encode` already writes by hand. The two halves are
/// therefore a matched pair in this crate, which is what makes the round-trip test below a
/// test of the pair rather than of somebody else's decoder.
fn read_netpbm(bytes: &[u8]) -> Result<GrayImage> {
    let refused = |what: &str| {
        crate::fault::imaging_failure(READ_OP, format!("malformed binary Netpbm: {what}"))
    };

    let mut cursor = 0_usize;
    let magic = header_token(bytes, &mut cursor).ok_or_else(|| refused("no magic number"))?;
    let channels = match magic.as_slice() {
        b"P5" => 1_usize,
        b"P6" => 3,
        _ => return Err(refused("magic is neither P5 nor P6")),
    };
    let width = header_number(bytes, &mut cursor).ok_or_else(|| refused("no width"))?;
    let height = header_number(bytes, &mut cursor).ok_or_else(|| refused("no height"))?;
    let max = header_number(bytes, &mut cursor).ok_or_else(|| refused("no maximum value"))?;
    if max != 255 {
        // Sixteen-bit Netpbm is a real format and this build never writes one, so it is
        // refused by name rather than misread as eight-bit and reported as a difference.
        return Err(refused(
            "only an 8-bit maximum value (255) is written by this build",
        ));
    }
    if width == 0 || height == 0 {
        return Err(refused("a zero extent"));
    }

    // Exactly one whitespace byte separates the header from the raster, by the format's own
    // rule — and the pixels start there, not at the next non-space, because a raster's first
    // pixel may legitimately *be* a space's byte value.
    let pixels = bytes.get(cursor..).ok_or_else(|| refused("no raster"))?;
    let expected = usize::try_from(width)
        .ok()
        .and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)))
        .and_then(|count| count.checked_mul(channels))
        .ok_or_else(|| refused("a raster larger than this machine can address"))?;
    if pixels.len() < expected {
        return Err(refused(&format!(
            "the raster is {} bytes and the header describes {expected}",
            pixels.len()
        )));
    }

    let luma: Vec<u8> = if channels == 1 {
        pixels.get(..expected).unwrap_or_default().to_vec()
    } else {
        pixels
            .get(..expected)
            .unwrap_or_default()
            .chunks_exact(3)
            .map(|rgb| {
                // The crate's one luma home, called rather than restated: a second set of
                // coefficients here would make a PPM's metrics disagree with the same
                // picture's PNG, which is the sentence this line used to carry while being
                // the second set (note **N266**).
                crate::decode::luma_sample(
                    *rgb.first().unwrap_or(&0),
                    *rgb.get(1).unwrap_or(&0),
                    *rgb.get(2).unwrap_or(&0),
                )
            })
            .collect()
    };

    GrayImage::from_raw(width, height, luma).ok_or_else(|| refused("the raster did not fit"))
}

/// One whitespace-delimited header token, skipping `#` comments, advancing `cursor` past the
/// single whitespace byte that follows it.
fn header_token(bytes: &[u8], cursor: &mut usize) -> Option<Vec<u8>> {
    loop {
        while bytes
            .get(*cursor)
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            *cursor += 1;
        }
        if bytes.get(*cursor) == Some(&b'#') {
            while bytes.get(*cursor).is_some_and(|byte| *byte != b'\n') {
                *cursor += 1;
            }
            continue;
        }
        break;
    }
    let start = *cursor;
    while bytes
        .get(*cursor)
        .is_some_and(|byte| !byte.is_ascii_whitespace())
    {
        *cursor += 1;
    }
    if *cursor == start {
        return None;
    }
    let token = bytes.get(start..*cursor)?.to_vec();
    // Exactly one whitespace byte is consumed, which is the format's rule and the reason the
    // raster can start with a byte that looks like a space.
    if bytes.get(*cursor).is_some_and(u8::is_ascii_whitespace) {
        *cursor += 1;
    }
    Some(token)
}

/// One header token, as a number.
fn header_number(bytes: &[u8], cursor: &mut usize) -> Option<u32> {
    let token = header_token(bytes, cursor)?;
    std::str::from_utf8(&token).ok()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;
    use schema::capture::Transform;

    #[test]
    fn every_metric_in_the_vocabulary_gets_a_delta() {
        // The walk is `MetricName::ALL`, so a sixth metric joins the comparison by existing.
        // Asserted rather than assumed, because "covered by construction" is the sentence
        // this suite exists to distrust.
        let base = fixtures::checkerboard(32, 32, 4);
        let comparison = photos(&base, &base);
        assert_eq!(comparison.delta.len(), MetricName::ALL.len());
        for name in MetricName::ALL {
            let delta = comparison.delta.get(name).copied().expect("every metric");
            assert!(
                delta.abs() < 1e-12,
                "{name:?} moved between an image and itself: {delta}"
            );
            assert!(comparison.a.metrics.contains_key(name));
            assert!(comparison.b.metrics.contains_key(name));
        }
    }

    #[test]
    fn a_delta_is_the_second_image_minus_the_first_and_reverses_with_the_arguments() {
        // Both orders, because a sign convention nobody asserted is a sign convention half
        // the consumers will read backwards.
        // A gradient rather than a checkerboard: exposure scaling is multiplicative, and a
        // picture of nothing but 0 and 255 has no midtone for it to move.
        let base = fixtures::gradient(32, 32);
        let brighter = fixtures::overexposed(&base);
        let forward = photos(&base, &brighter);
        let backward = photos(&brighter, &base);

        let name = MetricName::MeanLuma;
        let up = forward.delta.get(&name).copied().expect("mean luma");
        let down = backward.delta.get(&name).copied().expect("mean luma");
        assert!(up > 0.0, "the brighter image must raise the mean luma");
        assert!(
            (up + down).abs() < 1e-12,
            "the two orders must be negatives"
        );
        assert_eq!(forward.a.metrics, backward.b.metrics);
    }

    #[test]
    fn ssim_ranks_a_blurred_image_below_the_original_against_a_sharp_pair() {
        // The same both-directions shape every metric in this crate already carries: a claim
        // that SSIM "works" is unfalsifiable, and a claim that it *orders* two known pairs is
        // not.
        let base = fixtures::checkerboard(64, 64, 8);
        let blurred = fixtures::blurred(&base, 2.0).expect("a 64x64 image blurs");

        let identical = photos(&base, &base).ssim.score().expect("equal sizes");
        let against_blur = photos(&base, &blurred).ssim.score().expect("equal sizes");

        assert!(
            (identical - 1.0).abs() < 1e-6,
            "an image against itself must score 1.0, got {identical}"
        );
        assert!(
            against_blur < identical,
            "a blurred copy must score below an identical one: {against_blur} vs {identical}"
        );
        assert!(
            against_blur > 0.0,
            "a blurred copy of the same scene is not unrelated to it: {against_blur}"
        );
    }

    #[test]
    fn a_dimension_mismatch_is_represented_rather_than_refused() {
        // D17's shape, and the reason this core is total: the scalars are still there, the
        // deltas are still there, and the one quantity that needed equal shapes says so with
        // both shapes on it.
        let small = fixtures::checkerboard(16, 16, 4);
        let large = fixtures::checkerboard(32, 32, 4);
        let comparison = photos(&small, &large);

        assert_eq!(comparison.a.width, 16);
        assert_eq!(comparison.b.width, 32);
        assert_eq!(comparison.delta.len(), MetricName::ALL.len());
        match &comparison.ssim {
            Ssim::Unavailable {
                reason: SsimUnavailable::DimensionsDiffer { a, b },
            } => {
                assert_eq!(*a, [16, 16]);
                assert_eq!(*b, [32, 32]);
            }
            other => panic!("expected a represented mismatch, got {other:?}"),
        }
        assert!(comparison.ssim.score().is_none());
    }

    #[test]
    fn every_format_this_build_writes_is_a_format_the_comparison_reads() {
        // The population is `PhotoFormat::ALL`, walked, so a fourth sink format cannot land
        // without a reader — which is the defect a hand-listed pair of formats would hide.
        let base = fixtures::checkerboard(24, 24, 4);
        let decoded = crate::decode::Decoded::Gray(base.clone());
        for format in PhotoFormat::ALL {
            let bytes = match format {
                PhotoFormat::Png => crate::encode::png(&decoded),
                PhotoFormat::Jpeg => crate::encode::jpeg(&decoded, 90),
                PhotoFormat::Ppm => crate::encode::netpbm(&decoded),
            }
            .unwrap_or_else(|err| panic!("{format:?} encodes: {err}"));
            let read_back =
                read(&bytes).unwrap_or_else(|err| panic!("{format:?} must read back: {err}"));
            assert_eq!(
                read_back.dimensions(),
                base.dimensions(),
                "{format:?} round-tripped at another size"
            );
            assert_eq!(
                sniff(&bytes),
                Some(*format),
                "{format:?}'s own bytes are not sniffed as {format:?}"
            );
        }
    }

    /// The BT.601 luma of each bar of [`fixtures::colour_bars`], worked out from the
    /// definition rather than read off the code under test (N252).
    ///
    /// The one home is `decode::luma_sample`, whose arithmetic is `(77·r + 150·g + 29·b) >> 8`
    /// — the coefficients the YUV decode path already uses, so a photograph and the stream it
    /// came from describe one brightness. Each row below is that expression evaluated by hand
    /// for one of the eight saturated bars the fixture paints:
    ///
    /// | bar | RGB             | 77·r + 150·g + 29·b | >> 8 |
    /// |-----|-----------------|---------------------|------|
    /// | 0   | (255, 255, 255) | 65 280              | 255  |
    /// | 1   | (255, 255,   0) | 57 885              | 226  |
    /// | 2   | (  0, 255, 255) | 45 645              | 178  |
    /// | 3   | (  0, 255,   0) | 38 250              | 149  |
    /// | 4   | (255,   0, 255) | 27 030              | 105  |
    /// | 5   | (255,   0,   0) | 19 635              |  76  |
    /// | 6   | (  0,   0, 255) |  7 395              |  28  |
    /// | 7   | (  0,   0,   0) |      0              |   0  |
    ///
    /// Rec. 709 — the luma `image`'s own `to_luma8` computes — answers 255, 237, 201, 182, 73,
    /// 54, 18, 0 for the same eight bars, so the table separates the two definitions by up to
    /// 33 codes and cannot be satisfied by both.
    const COLOUR_BAR_LUMA: [u8; 8] = [255, 226, 178, 149, 105, 76, 28, 0];

    /// How far a JPEG bar centre may sit from the table.
    ///
    /// The measurement first, because it is what the number is headroom *for*: at quality 95 this
    /// fixture's bars are eight pixels wide and therefore DCT-block aligned, so every block is one
    /// constant colour and decodes DC-exact — the worst deviation from the table over the **whole
    /// raster**, boundaries and centres alike, is 0 codes on all three formats. Nothing rings, and
    /// the three codes buy nothing today. They are carried because the encoder's quality,
    /// quantizer and subsampling are not this arm's subject: a change to any of them would move
    /// the boundaries first, and an arm red for a quantizer is an arm that reads as green about a
    /// colour model. Centres are sampled for the same reason and not because the boundaries were
    /// measured to be worse. The tolerance is deliberately an order of magnitude below the 33
    /// codes that separate BT.601 from Rec. 709 on these bars: no tolerance this arm could carry
    /// and still be worth having would admit a second colour model.
    const JPEG_TOLERANCE: i32 = 3;

    #[test]
    fn one_colour_scene_reads_the_same_luma_out_of_every_format_this_build_writes() {
        // The sibling of the round-trip walk below, on a fixture that can tell the crate's
        // colour models apart: a grey fixture is a fixed point of every luma definition there
        // is, so the walk that already exists cannot go red on a second one.
        //
        // The population is `PhotoFormat::ALL` for that walk's reason — a fourth sink format
        // joins this claim by existing.
        let bars = fixtures::colour_bars(64, 16);
        let decoded = crate::decode::Decoded::Rgb(bars);
        let mut rasters: BTreeMap<PhotoFormat, GrayImage> = BTreeMap::new();
        for format in PhotoFormat::ALL {
            let bytes = match format {
                PhotoFormat::Png => crate::encode::png(&decoded),
                PhotoFormat::Jpeg => crate::encode::jpeg(&decoded, 95),
                PhotoFormat::Ppm => crate::encode::netpbm(&decoded),
            }
            .unwrap_or_else(|err| panic!("{format:?} encodes: {err}"));
            let read_back =
                read(&bytes).unwrap_or_else(|err| panic!("{format:?} must read back: {err}"));

            for (bar, expected) in COLOUR_BAR_LUMA.iter().enumerate() {
                // The centre of the bar, because a bar boundary is where a lossy codec would
                // ring first if the encoder changed, and this claim is about the colour model
                // rather than about the quantizer — see [`JPEG_TOLERANCE`] for what today's
                // encoder actually does, which is not ring at all.
                let x = u32::try_from(bar * 8 + 4).expect("eight bars of eight pixels");
                let seen = read_back.get_pixel(x, 8).0[0];
                let tolerance = if *format == PhotoFormat::Jpeg {
                    JPEG_TOLERANCE
                } else {
                    0
                };
                let apart = i32::from(seen).abs_diff(i32::from(*expected));
                assert!(
                    i32::try_from(apart).is_ok_and(|apart| apart <= tolerance),
                    "the {format:?} arm read bar {bar} as {seen} where BT.601 says {expected}; a \
                     picture measured through two colour models is a difference the camera never \
                     produced"
                );
            }
            rasters.insert(*format, read_back);
        }

        // And the two lossless sinks agree exactly, not merely within a tolerance: PNG goes
        // through `image`'s decoder and PPM through this crate's own header walk, so this is
        // the two readers meeting at one colour model rather than one reader agreeing with
        // itself.
        let png = rasters.get(&PhotoFormat::Png).expect("the PNG arm ran");
        let ppm = rasters.get(&PhotoFormat::Ppm).expect("the Netpbm arm ran");
        assert_eq!(
            png.as_raw(),
            ppm.as_raw(),
            "the same scene read out of a PNG and a PPM is two different pictures"
        );
    }

    /// A ramp in **both** axes, so all four corners are distinct and their ordering names
    /// which of the eight dihedral permutations a reader performed.
    ///
    /// A checkerboard cannot tell a quarter turn from the identity, and a single-axis ramp
    /// cannot tell one from a transpose — which is the misread a shape assertion is blind to,
    /// because a transpose and a quarter turn produce the same extents. Non-square for the
    /// same family of reasons: on a square fixture the defect and the repair agree.
    fn two_axis_ramp(width: u32, height: u32) -> GrayImage {
        image::ImageBuffer::from_fn(width, height, |x, y| {
            // Sixteen codes between the closest two corners, which is far more than a
            // quality-95 JPEG of a smooth ramp moves any of them.
            image::Luma([u8::try_from(20 + x * 4 + y * 3).unwrap_or(u8::MAX)])
        })
    }

    /// The four corners' brightnesses in rank order, which is what an ordering claim about a
    /// lossy round trip can honestly assert.
    fn corner_ranks(image: &GrayImage) -> Vec<usize> {
        let (right, bottom) = (image.width() - 1, image.height() - 1);
        let samples: Vec<u8> = [(0, 0), (right, 0), (0, bottom), (right, bottom)]
            .iter()
            .map(|(x, y)| image.get_pixel(*x, *y).0[0])
            .collect();
        let mut order: Vec<usize> = (0..samples.len()).collect();
        order.sort_by_key(|corner| samples[*corner]);
        order
    }

    #[test]
    fn a_photograph_this_build_stamped_is_read_at_the_shape_its_orientation_tag_describes() {
        // The verbatim sink applies a transform as an EXIF tag and moves no pixel (D6, E6), so
        // a reader that drops the tag hands `photo diff` two shapes for one frame and refuses
        // it a similarity score.
        let upright = two_axis_ramp(24, 36);
        let stamped = stamped_jpeg(&upright, Transform::Rot90);

        // The expectation is the definition of a quarter turn, corroborated by
        // `photo::transform_pixels` — a different function in a different module, asserted over
        // `Transform::ALL` by its own suite — and never by asking `read` what it thinks. Both
        // halves of it are borrowed: the extents, and **which way round the picture came out**.
        let crate::decode::Decoded::Gray(turned) = crate::photo::transform_pixels(
            &crate::decode::Decoded::Gray(upright.clone()),
            Transform::Rot90,
        ) else {
            panic!("a grey fixture turns into a grey image");
        };
        assert_eq!((turned.width(), turned.height()), (36, 24));

        // JPEG first, and then the same claim through a PNG carrying the same tag: `image`'s
        // `PngDecoder` reads `eXIf`, so a helper per format would be a hole that reopens on
        // the day somebody writes one, and the shared helper is a claim nothing could go red
        // on while no PNG in this tree carried an orientation.
        for (what, bytes) in [
            ("JPEG", stamped.clone()),
            (
                "PNG",
                png_carrying_exif(&upright, &tiff_payload_of(&stamped)),
            ),
        ] {
            let read_back = read(&bytes).expect("a stamped photograph reads");
            assert_eq!(
                read_back.dimensions(),
                (36, 24),
                "a rot90 {what} this build stamped read back at the raster's shape rather than \
                 at the shape its own Orientation tag describes"
            );
            assert_eq!(
                corner_ranks(&read_back),
                corner_ranks(&turned),
                "a rot90 {what} came back turned a different way: its four corners rank {:?} \
                 where the quarter turn `photo::transform_pixels` computes ranks {:?}, and a \
                 transpose has the same extents as a quarter turn",
                corner_ranks(&read_back),
                corner_ranks(&turned)
            );
        }
    }

    /// The TIFF payload out of a stamped JPEG's EXIF APP1 segment, by an independent marker
    /// walk: the segment length is big-endian and counts itself, and the six bytes after it
    /// are the `Exif\0\0` signature the payload starts after.
    ///
    /// Written here rather than borrowed from `crate::exif`, because this is the fixture
    /// builder for a claim about that module's output.
    fn tiff_payload_of(jpeg: &[u8]) -> Vec<u8> {
        let mut cursor = 2;
        while cursor + 4 <= jpeg.len() {
            let marker = (jpeg[cursor], jpeg[cursor + 1]);
            let length = usize::from(u16::from_be_bytes([jpeg[cursor + 2], jpeg[cursor + 3]]));
            let body = jpeg
                .get(cursor + 4..cursor + 2 + length)
                .expect("a segment this build wrote declares its own length");
            if marker == (0xff, 0xe1) && body.starts_with(b"Exif\0\0") {
                return body[6..].to_vec();
            }
            cursor += 2 + length;
        }
        panic!("the stamped fixture carries no EXIF APP1 segment to lift a payload out of");
    }

    /// The same TIFF payload written into a PNG's `eXIf` chunk, through the `image` crate's
    /// own encoder — the writer whose decoder this arm is about.
    fn png_carrying_exif(image: &GrayImage, tiff: &[u8]) -> Vec<u8> {
        use image::ImageEncoder as _;

        let mut bytes = Vec::new();
        let mut encoder = image::codecs::png::PngEncoder::new(&mut bytes);
        encoder
            .set_exif_metadata(tiff.to_vec())
            .expect("PNG carries EXIF");
        encoder
            .write_image(
                image.as_raw(),
                image.width(),
                image.height(),
                image::ExtendedColorType::L8,
            )
            .expect("the fixture encodes");
        bytes
    }

    #[test]
    fn an_upright_or_unstamped_photograph_is_not_turned_by_the_reader() {
        // The inverse of the arm above, and what stops the repair from over-rotating: three
        // inputs that differ from it in exactly one tag, or in having no tag at all. Without
        // this arm "apply the orientation unconditionally" would satisfy the forward claim.
        let upright = fixtures::checkerboard(24, 36, 4);

        let stamped = stamped_jpeg(&upright, Transform::None);
        assert_eq!(
            read(&stamped)
                .expect("an upright photograph reads")
                .dimensions(),
            (24, 36),
            "an Orientation-1 photograph came back transposed, so the reader turns what nobody \
             asked it to"
        );

        let bare = crate::encode::jpeg(&crate::decode::Decoded::Gray(upright.clone()), 95)
            .expect("the fixture encodes");
        assert_eq!(
            read(&bare).expect("an unstamped JPEG reads").dimensions(),
            (24, 36),
            "a photograph carrying no EXIF at all came back transposed"
        );

        let png = crate::encode::png(&crate::decode::Decoded::Gray(upright))
            .expect("the fixture encodes");
        assert_eq!(
            read(&png).expect("a PNG reads").dimensions(),
            (24, 36),
            "a PNG carrying no orientation at all came back transposed"
        );
    }

    /// The fixture the two orientation arms are about: what `engine::photo::take` writes on the
    /// verbatim sink, built here from the two halves that produce it — `encode::jpeg` and the
    /// stamp one layer up. The reader was only ever tested against the inner half, which is the
    /// seam the defect lived in.
    ///
    /// Small on purpose: `exif::stamp_jpeg` refuses a segment past
    /// `limits::MAX_EXIF_APP1_BYTES` \[PF:16, N203\], and a fixture near that bound would make
    /// the arms above red for a reason that is not theirs.
    fn stamped_jpeg(image: &GrayImage, transform: Transform) -> Vec<u8> {
        let bytes = crate::encode::jpeg(&crate::decode::Decoded::Gray(image.clone()), 95)
            .expect("the fixture encodes");
        crate::exif::stamp_jpeg(
            &bytes,
            &crate::exif::CaptureMetadata {
                captured_at: schema::time::Stamp::epoch(),
                camera: schema::camera::CameraFingerprint {
                    bus_path: "3-4:1.0".to_owned(),
                    usb_id: None,
                    card: "a synthetic fixture".to_owned(),
                    driver: "uvcvideo".to_owned(),
                    serial: None,
                },
                negotiated: schema::capture::NegotiatedStream {
                    pixel_format: schema::camera::PixelFormat::MJPG,
                    width: image.width(),
                    height: image.height(),
                    bytes_per_line: 0,
                    size_image: 1 << 16,
                    interval: schema::camera::FrameInterval::Discrete {
                        numerator: 1,
                        denominator: 30,
                    },
                    adjustments: Vec::new(),
                },
                transform,
                controls: BTreeMap::new(),
            },
        )
        .expect("a small JPEG stamps")
    }

    /// A PNG that declares a raster and carries none of it: an IHDR saying `width` by
    /// `height` in 8-bit RGBA, one deflate stream of nothing, and the end marker.
    ///
    /// Built here as bytes rather than committed to `corpus/images/`, which holds
    /// photographs this build could have taken: a header describing 200 000 pixels on a side
    /// is not one of those, and seventy bytes of it are cheaper to read here than to look up.
    /// The CRCs are computed rather than transcribed, because the `png` crate checks them and
    /// a fixture it refuses for the wrong reason would be an arm green for the wrong reason.
    fn png_declaring(width: u32, height: u32) -> Vec<u8> {
        fn crc32(bytes: &[u8]) -> u32 {
            let mut crc = u32::MAX;
            for byte in bytes {
                crc ^= u32::from(*byte);
                for _ in 0..8 {
                    // The reflected CRC-32 polynomial, which is what PNG's `Cyclic Redundancy
                    // Check` clause names.
                    crc = if crc & 1 == 1 {
                        (crc >> 1) ^ 0xedb8_8320
                    } else {
                        crc >> 1
                    };
                }
            }
            !crc
        }
        fn chunk(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut out = Vec::new();
            let length = u32::try_from(body.len()).expect("a chunk this test builds is short");
            out.extend_from_slice(&length.to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(body);
            let mut checked = kind.to_vec();
            checked.extend_from_slice(body);
            out.extend_from_slice(&crc32(&checked).to_be_bytes());
            out
        }
        let mut header = Vec::new();
        header.extend_from_slice(&width.to_be_bytes());
        header.extend_from_slice(&height.to_be_bytes());
        // 8 bits per channel, colour type 6 (RGBA), deflate, no filter, no interlace.
        header.extend_from_slice(&[8, 6, 0, 0, 0]);

        let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        out.extend_from_slice(&chunk(b"IHDR", &header));
        // An empty deflate stream: the zlib header, one final stored block of length zero,
        // and the Adler-32 of nothing.
        out.extend_from_slice(&chunk(
            b"IDAT",
            &[
                0x78, 0x01, 0x01, 0x00, 0x00, 0xff, 0xff, 0x00, 0x00, 0x00, 0x01,
            ],
        ));
        out.extend_from_slice(&chunk(b"IEND", &[]));
        out
    }

    /// A JPEG that declares a raster and carries none of it: the quantization and Huffman
    /// tables a decoder needs to accept the header at all, an SOF0 saying `width` by
    /// `height` in three components, and a scan header with no scan behind it.
    fn jpeg_declaring(width: u16, height: u16) -> Vec<u8> {
        let mut out = vec![0xff, 0xd8];
        let mut quantization = vec![0x00];
        quantization.extend_from_slice(&[1; 64]);
        out.extend_from_slice(&[0xff, 0xdb]);
        out.extend_from_slice(
            &u16::try_from(quantization.len() + 2)
                .expect("one quantization table is 67 bytes")
                .to_be_bytes(),
        );
        out.extend_from_slice(&quantization);

        let mut frame = vec![0x08];
        frame.extend_from_slice(&height.to_be_bytes());
        frame.extend_from_slice(&width.to_be_bytes());
        frame.push(3);
        for component in 1..=3u8 {
            frame.extend_from_slice(&[component, 0x11, 0x00]);
        }
        out.extend_from_slice(&[0xff, 0xc0]);
        out.extend_from_slice(
            &u16::try_from(frame.len() + 2)
                .expect("one frame header is 17 bytes")
                .to_be_bytes(),
        );
        out.extend_from_slice(&frame);

        for class in [0x00, 0x10] {
            let mut table = vec![class];
            table.extend_from_slice(&[1]);
            table.extend_from_slice(&[0; 15]);
            table.push(0);
            out.extend_from_slice(&[0xff, 0xc4]);
            out.extend_from_slice(
                &u16::try_from(table.len() + 2)
                    .expect("one Huffman table is 20 bytes")
                    .to_be_bytes(),
            );
            out.extend_from_slice(&table);
        }

        let scan = [3u8, 1, 0x00, 2, 0x00, 3, 0x00, 0x00, 0x3f, 0x00];
        out.extend_from_slice(&[0xff, 0xda]);
        out.extend_from_slice(
            &u16::try_from(scan.len() + 2)
                .expect("one scan header is 12 bytes")
                .to_be_bytes(),
        );
        out.extend_from_slice(&scan);
        out.extend_from_slice(&[0xff, 0xd9]);
        out
    }

    #[test]
    fn a_photograph_declaring_more_raster_than_this_build_will_allocate_is_refused() {
        // `photo diff` takes two paths a caller named, and a compressed file's *header* is
        // what says how big its raster is — a caller-supplied number reaching an allocator,
        // which AGENTS bounds at the door. The allocator's answer to a number it cannot serve
        // is `abort`: no `Failure` document, no exit code from the registry, nothing on
        // standard output at all, which is the one failure shape a `--json` run must not have
        // (note **N268**).
        //
        // Both compressed formats, because the bound has to live where both decoders are
        // built and a hole in one of them is the whole hole.
        let past = u64::from(u32::MAX);
        for (format, bytes) in [
            (PhotoFormat::Jpeg, jpeg_declaring(u16::MAX, u16::MAX)),
            (PhotoFormat::Png, png_declaring(200_000, 200_000)),
        ] {
            assert!(
                bytes.len() < 1_024,
                "the {format:?} fixture is {} bytes and is meant to be a header with no raster \
                 behind it",
                bytes.len()
            );
            let refusal = read(&bytes).expect_err(
                "a photograph declaring more raster than this build will allocate was decoded \
                 rather than refused, so the bound is whatever the allocator does next",
            );
            let message = format!("{refusal}");
            assert!(
                message.contains(format.as_str())
                    && message.contains(&limits::MAX_PHOTO_DECODE_BYTES.to_string()),
                "the refusal must name the format and the budget it exceeded, so an unattended \
                 reader can tell a file that is too big from a machine that is out of memory: \
                 {message}"
            );
            assert!(
                past > limits::MAX_PHOTO_DECODE_BYTES,
                "this arm's fixtures declare rasters past the budget only while the budget is \
                 below {past} bytes"
            );
        }
    }

    #[test]
    fn a_photograph_inside_this_builds_decode_budget_is_not_refused_for_its_size() {
        // The bound from the other side (N255), and what stops it from being set below the
        // product's own output: a raster four thousand pixels on a side is twice the largest
        // any camera in `corpus/` delivers and is comfortably inside the budget, so the reader
        // must get past the size check and fail on the *pixels this fixture does not carry*
        // instead. A build whose budget had been tightened onto the product would refuse it
        // here with the sentence above.
        let bytes = png_declaring(4_096, 4_096);
        let declared = 4_096u64 * 4_096 * 4;
        assert!(
            declared < limits::MAX_PHOTO_DECODE_BYTES,
            "this arm needs a raster inside the budget and {declared} is not"
        );
        let refusal = read(&bytes).expect_err("the fixture carries no pixels, so it cannot decode");
        let message = format!("{refusal}");
        assert!(
            !message.contains(&limits::MAX_PHOTO_DECODE_BYTES.to_string()),
            "a photograph this build could have taken was refused as too large: {message}"
        );
    }

    #[test]
    fn bytes_in_no_format_this_build_writes_are_refused_naming_what_was_found() {
        for (bytes, expected) in [
            (b"".as_slice(), "the file is empty"),
            (b"GIF89a".as_slice(), "not a photograph this build writes"),
            (b"\x00\x01\x02\x03".as_slice(), "the first bytes are"),
        ] {
            let refusal = read(bytes).expect_err("refused").to_string();
            assert!(
                refusal.contains(expected),
                "the refusal does not say {expected:?}: {refusal}"
            );
        }
    }

    #[test]
    fn a_truncated_or_lying_netpbm_header_is_refused_rather_than_half_read() {
        // The malformed fixtures each validator owes (AGENTS "Writing tests"), one per branch
        // the reader can refuse on.
        for (bytes, expected) in [
            (b"P5".to_vec(), "no width"),
            (b"P5 4 ".to_vec(), "no height"),
            (b"P5 4 4 ".to_vec(), "no maximum value"),
            (b"P5 4 4 65535 ".to_vec(), "only an 8-bit maximum"),
            (b"P5 0 4 255 ".to_vec(), "a zero extent"),
            (b"P5 4 4 255 short".to_vec(), "the raster is"),
            (
                b"P7 4 4 255 ".to_vec(),
                "not a photograph this build writes",
            ),
        ] {
            let refusal = read(&bytes).expect_err("refused").to_string();
            assert!(
                refusal.contains(expected),
                "the refusal does not say {expected:?}: {refusal}"
            );
        }
    }

    #[test]
    fn a_netpbm_comment_and_a_raster_that_starts_with_a_space_both_survive() {
        // Two shapes the parser must allow, and the second is why exactly one whitespace byte
        // is consumed after the header: a raster whose first pixel is 0x20 is a legal file,
        // and a reader that skipped whitespace would eat the picture's first pixel.
        let mut bytes = b"P5\n# written by a comment-writing tool\n2 2\n255\n".to_vec();
        bytes.extend_from_slice(&[b' ', 10, 20, 30]);
        let image = read(&bytes).expect("a commented header reads");
        assert_eq!(image.dimensions(), (2, 2));
        assert_eq!(image.as_raw().first().copied(), Some(b' '));
    }
}
