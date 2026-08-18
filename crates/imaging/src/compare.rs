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
//! [`read`] accepts exactly what [`crate::encode`] writes — every member of
//! [`schema::capture::PhotoFormat`], walked — because "a photograph this tool produced" is
//! the only population `photo diff` has any business accepting, and a reader that took less
//! than the writer emits would refuse the tool's own output.

use std::collections::BTreeMap;

use image::GrayImage;
use schema::capture::PhotoFormat;
use schema::error::Result;
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
/// The crate's one refusal home ([`crate::fault::imaging_failure`], `DeviceIo` with no
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
        // Through the `image` crate's own decoders, which are the ones `crate::encode` writes
        // with: a round trip that used a second implementation would be measuring the two
        // implementations against each other rather than the file.
        PhotoFormat::Jpeg | PhotoFormat::Png => image::load_from_memory(bytes)
            .map(|decoded| decoded.to_luma8())
            .map_err(|refused| {
                crate::fault::imaging_failure(
                    READ_OP,
                    format!("{} decode failed: {refused}", format.as_str()),
                )
            }),
        PhotoFormat::Ppm => read_netpbm(bytes),
    }
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
                // The same Rec. 601 luma the rest of this crate uses, in integers: a second
                // set of coefficients here would make a PPM's metrics disagree with the same
                // picture's PNG.
                let r = u32::from(*rgb.first().unwrap_or(&0));
                let g = u32::from(*rgb.get(1).unwrap_or(&0));
                let b = u32::from(*rgb.get(2).unwrap_or(&0));
                u8::try_from((r * 299 + g * 587 + b * 114) / 1000).unwrap_or(u8::MAX)
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
