//! Capture vocabulary: what a caller asks for, what the device agreed to, and what came
//! back (design D5, D6).
//!
//! The theme is D3's, applied to formats instead of controls: drivers adjust silently, so
//! the *negotiated* result is always reported alongside the request.

use std::fmt;

use camino::Utf8PathBuf;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::camera::{CameraId, FormatInfo, FrameInterval, PixelFormat};
use crate::limits;
use crate::time::Stamp;
use crate::vocabulary::closed_vocabulary;

/// What a caller wants from a stream. Every field is optional: "just give me something"
/// is a legitimate request, and the answer is always reported back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StreamRequest {
    /// Preferred pixel format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pixel_format: Option<PixelFormat>,
    /// Preferred width in pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    /// Preferred height in pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// Preferred frame interval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval: Option<FrameInterval>,
    /// How many buffers to queue.
    #[serde(default = "default_buffer_count")]
    pub buffer_count: u32,
}

fn default_buffer_count() -> u32 {
    limits::DEFAULT_BUFFER_COUNT
}

impl Default for StreamRequest {
    /// The same defaults serde fills in, so a struct built in Rust and one parsed from
    /// `{}` are the same value. A derived `Default` would give `buffer_count = 0` and
    /// the two would disagree.
    fn default() -> Self {
        StreamRequest {
            pixel_format: None,
            width: None,
            height: None,
            interval: None,
            buffer_count: default_buffer_count(),
        }
    }
}

impl StreamRequest {
    /// Resolve this request against the formats a device enumerates (design D5).
    ///
    /// **The one home for "what should I ask the device for".** Every backend has the same
    /// question and the same evidence — a `Vec<FormatInfo>` the device produced — so
    /// answering it twice would let the fake and the hardware disagree about what
    /// `StreamRequest::default()` means, which is precisely the resemblance E5 exists to
    /// keep. A backend that cannot use every listed format (the fake can only synthesise
    /// a few) filters the list before calling, rather than teaching this function about
    /// its limitations.
    ///
    /// The rules, and the reason for each:
    ///
    /// - **Format**: the requested one when the device offers it, else the device's first.
    ///   First, not largest: the order `VIDIOC_ENUM_FMT` returns is the driver's own
    ///   preference, and second-guessing it is how a tool ends up defaulting to a mode the
    ///   camera is worse at.
    /// - **Size**: the exact request when it is offered; else the largest offered size
    ///   that *fits inside* it, because a caller asking for 1920×1080 on a 720p camera
    ///   wants the biggest thing there is rather than a refusal; else the format's first
    ///   size, for the same "the driver ordered these" reason.
    /// - **A half-specified size names nothing.** Width alone cannot pick a height without
    ///   inventing an aspect ratio, so the size falls through to the device's first as
    ///   though nothing had been asked — and [`NegotiatedStream::diff`] then reports no
    ///   size adjustment, because none was requested in a form the answer could differ
    ///   from.
    ///
    /// `None` when `formats` is empty or lists nothing with readable dimensions — a
    /// camera that offers no size is not one this can pick from, and the caller turns that
    /// into [`crate::Error::FormatUnsupported`] with the list it does have.
    #[must_use]
    pub fn choose(&self, formats: &[FormatInfo]) -> Option<ChosenFormat> {
        let first = formats.first()?;
        let chosen = self
            .pixel_format
            .and_then(|wanted| formats.iter().find(|f| f.pixel_format == wanted))
            .unwrap_or(first);

        let offered: Vec<(u32, u32)> = chosen
            .sizes
            .iter()
            .filter_map(|entry| entry.size.max_dimensions())
            .collect();
        let default_size = offered.first().copied()?;

        let (width, height) = match (self.width, self.height) {
            (Some(width), Some(height)) if offered.contains(&(width, height)) => (width, height),
            (Some(width), Some(height)) => offered
                .iter()
                .copied()
                .filter(|(w, h)| *w <= width && *h <= height)
                .max_by_key(|(w, h)| u64::from(*w) * u64::from(*h))
                .unwrap_or(default_size),
            _ => default_size,
        };

        Some(ChosenFormat {
            pixel_format: chosen.pixel_format,
            width,
            height,
        })
    }
}

/// The format and size a [`StreamRequest`] resolves to against a device's format tree.
///
/// The interval is deliberately absent: choosing one is a *negotiation* on V4L2
/// (`S_PARM`, with its own capability bit) and a lookup in a document for the fake, so the
/// two backends genuinely do different things and pretending otherwise would put a lie in
/// a shared type. Format and size are the two fields `S_FMT` carries, and those are one
/// decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChosenFormat {
    /// The format to ask for.
    pub pixel_format: PixelFormat,
    /// The width to ask for.
    pub width: u32,
    /// The height to ask for.
    pub height: u32,
}

/// One way the device's answer differs from the request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "field", rename_all = "snake_case")]
pub enum Adjustment {
    /// The driver chose a different pixel format.
    PixelFormat {
        /// What was asked for.
        requested: PixelFormat,
        /// What the driver set.
        negotiated: PixelFormat,
    },
    /// The driver chose a different frame size.
    Size {
        /// Requested width.
        requested_width: u32,
        /// Requested height.
        requested_height: u32,
        /// Negotiated width.
        negotiated_width: u32,
        /// Negotiated height.
        negotiated_height: u32,
    },
    /// The driver chose a different frame interval.
    Interval {
        /// What was asked for.
        requested: FrameInterval,
        /// What the driver set.
        negotiated: FrameInterval,
    },
}

/// What the device actually agreed to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct NegotiatedStream {
    /// The format frames will arrive in.
    pub pixel_format: PixelFormat,
    /// Frame width.
    pub width: u32,
    /// Frame height.
    pub height: u32,
    /// Bytes per row, as the driver reports it (0 for compressed formats).
    pub bytes_per_line: u32,
    /// The driver's maximum frame size in bytes.
    pub size_image: u32,
    /// The negotiated frame interval.
    pub interval: FrameInterval,
    /// Every way this differs from what was asked. Empty means the request was honored
    /// exactly — and an empty list is a claim, so it is reported rather than assumed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub adjustments: Vec<Adjustment>,
}

impl NegotiatedStream {
    /// Whether the device gave us what we asked for.
    #[must_use]
    pub fn is_exact(&self) -> bool {
        self.adjustments.is_empty()
    }

    /// Every way an answer differs from the request that produced it (design D5).
    ///
    /// **The single home for the comparison**, so a driver's silent adjustment and a
    /// backend's chosen one are reported in the same words. *Choosing* a format is each
    /// backend's business — the V4L2 one asks the kernel, the fake picks from a document
    /// — but describing the difference is one law, and two copies of it would drift the
    /// day one of them learned about a new field.
    ///
    /// A request that named only one of width and height reports **no** size adjustment:
    /// [`Adjustment::Size`] carries both requested dimensions, and filling the unnamed one
    /// in from the answer would put a number in the caller's mouth. A half-specified size
    /// is honoured as far as it goes and the answer speaks for itself.
    #[must_use]
    pub fn diff(
        request: &StreamRequest,
        pixel_format: PixelFormat,
        width: u32,
        height: u32,
        interval: FrameInterval,
    ) -> Vec<Adjustment> {
        let mut adjustments = Vec::new();
        if let Some(requested) = request.pixel_format
            && requested != pixel_format
        {
            adjustments.push(Adjustment::PixelFormat {
                requested,
                negotiated: pixel_format,
            });
        }
        if let (Some(requested_width), Some(requested_height)) = (request.width, request.height)
            && (requested_width, requested_height) != (width, height)
        {
            adjustments.push(Adjustment::Size {
                requested_width,
                requested_height,
                negotiated_width: width,
                negotiated_height: height,
            });
        }
        if let Some(requested) = request.interval
            && requested != interval
        {
            adjustments.push(Adjustment::Interval {
                requested,
                negotiated: interval,
            });
        }
        adjustments
    }
}

/// One captured frame, copied out of the driver's buffer.
///
/// Frames never enter logs or error messages (rubric A12) — the `Debug` impl prints the
/// byte *count*, never the bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct Frame {
    /// The frame's bytes, exactly `bytesused` long.
    pub bytes: Vec<u8>,
    /// The format they are in.
    pub pixel_format: PixelFormat,
    /// Frame width.
    pub width: u32,
    /// Frame height.
    pub height: u32,
    /// Bytes per row (0 for compressed formats).
    pub bytes_per_line: u32,
    /// The driver's sequence number — gaps mean dropped frames.
    pub sequence: u32,
    /// The driver's timestamp, in microseconds on its own clock.
    pub timestamp_us: i64,
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // A frame may contain a person. Never the bytes.
        f.debug_struct("Frame")
            .field("bytes", &format_args!("<{} bytes>", self.bytes.len()))
            .field("pixel_format", &self.pixel_format)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("bytes_per_line", &self.bytes_per_line)
            .field("sequence", &self.sequence)
            .field("timestamp_us", &self.timestamp_us)
            .finish()
    }
}

/// How long to wait for the sensor to settle before taking a photo \[PF:11\].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SettleSpec {
    /// Discard this many frames first.
    SkipFrames {
        /// How many.
        frames: u32,
    },
    /// Keep discarding frames for this long.
    SettleFor {
        /// How long, in milliseconds.
        millis: u64,
    },
}

impl Default for SettleSpec {
    fn default() -> Self {
        SettleSpec::SkipFrames {
            frames: limits::DEFAULT_SETTLE_SKIP_FRAMES,
        }
    }
}

/// A settle policy: the spec plus the deadline that bounds it.
///
/// Both forms are bounded, because "skip 10 frames" on a camera delivering none is a
/// hang, and a hang is the failure mode E3 warns about wearing a disguise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SettlePolicy {
    /// What counts as settled.
    #[serde(default)]
    pub spec: SettleSpec,
    /// How long the whole settle may take.
    #[serde(default = "default_settle_deadline")]
    pub deadline_ms: u64,
}

fn default_settle_deadline() -> u64 {
    limits::DEFAULT_SETTLE_DEADLINE_MS
}

impl Default for SettlePolicy {
    fn default() -> Self {
        SettlePolicy {
            spec: SettleSpec::default(),
            deadline_ms: limits::DEFAULT_SETTLE_DEADLINE_MS,
        }
    }
}

closed_vocabulary! {
    /// An orientation transform (the skill's flip and rotate).
    ///
    /// On the verbatim-JPEG sink these become an EXIF Orientation tag — zero re-encode,
    /// byte fidelity preserved (E6). On PNG and re-encode sinks they are applied to
    /// pixels.
    ///
    /// The serde spelling and [`Transform::as_str`] are the same strings on purpose: a
    /// `--transform` argument and a `"transform"` field in a JSON document must name the
    /// same thing the same way, and the CLI reaches this vocabulary rather than deriving
    /// a second one of its own.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize, JsonSchema)]
    #[serde(rename_all = "lowercase")]
    pub enum Transform {
        /// Leave the image as the camera framed it.
        #[default]
        None,
        /// Mirror horizontally.
        #[serde(rename = "hflip")]
        HFlip,
        /// Mirror vertically.
        #[serde(rename = "vflip")]
        VFlip,
        /// Rotate a quarter turn clockwise.
        Rot90,
        /// Rotate a half turn.
        Rot180,
        /// Rotate a quarter turn counter-clockwise.
        Rot270,
    }
}

impl Transform {
    /// The EXIF Orientation tag value that expresses this transform.
    ///
    /// Values are the standard's: 1 = as shot, 2 = mirrored, 3 = 180°, 4 = flipped,
    /// 6 = 90° CW, 8 = 270° CW.
    #[must_use]
    pub const fn exif_orientation(self) -> u16 {
        match self {
            Transform::None => 1,
            Transform::HFlip => 2,
            Transform::Rot180 => 3,
            Transform::VFlip => 4,
            Transform::Rot90 => 6,
            Transform::Rot270 => 8,
        }
    }

    /// The name this transform is written by, in JSON and on a command line alike.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Transform::None => "none",
            Transform::HFlip => "hflip",
            Transform::VFlip => "vflip",
            Transform::Rot90 => "rot90",
            Transform::Rot180 => "rot180",
            Transform::Rot270 => "rot270",
        }
    }

    /// Parse one of those names.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|t| t.as_str() == s)
    }
}

impl fmt::Display for Transform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

closed_vocabulary! {
    /// The encoding a photo lands in.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
    #[serde(rename_all = "lowercase")]
    pub enum PhotoFormat {
        /// JPEG. Verbatim camera bytes when the stream is already MJPG (E6), else encoded.
        Jpeg,
        /// PNG. Always encoded, always lossless.
        Png,
        /// Binary PPM/PGM — the "give me pixels" escape hatch for tooling.
        Ppm,
    }
}

impl PhotoFormat {
    /// Guess the format a path is asking for by its extension.
    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => Some(PhotoFormat::Jpeg),
            "png" => Some(PhotoFormat::Png),
            "ppm" | "pgm" => Some(PhotoFormat::Ppm),
            _ => None,
        }
    }

    /// The extension this format writes.
    ///
    /// Not the same string as [`PhotoFormat::as_str`], and deliberately: the format is
    /// named `jpeg` and the file is named `.jpg`, because both spellings are load-bearing
    /// conventions somebody else owns.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            PhotoFormat::Jpeg => "jpg",
            PhotoFormat::Png => "png",
            PhotoFormat::Ppm => "ppm",
        }
    }

    /// The name this format is written by, in JSON and on a command line alike.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            PhotoFormat::Jpeg => "jpeg",
            PhotoFormat::Png => "png",
            PhotoFormat::Ppm => "ppm",
        }
    }

    /// Parse one of those names.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|f| f.as_str() == s)
    }
}

impl fmt::Display for PhotoFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the photo pipeline did to produce a photo's bytes (design D6/E6).
///
/// Reported alongside every photo because "the camera's own bitstream" and "a faithful
/// re-encode" are different products, and a calibration sample that was re-encoded is
/// partly ranking our codec rather than the lens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PhotoRendering {
    /// The camera's own bitstream, byte for byte (E6).
    Verbatim {
        /// The format the camera delivered, which is the format on disk.
        source: PixelFormat,
    },
    /// A compressed frame was decoded and encoded again.
    DecodedAndEncoded {
        /// What the camera delivered.
        source: PixelFormat,
        /// What was written.
        target: PhotoFormat,
    },
    /// A raw frame was converted to pixels and encoded.
    ConvertedAndEncoded {
        /// What the camera delivered.
        source: PixelFormat,
        /// What was written.
        target: PhotoFormat,
    },
}

impl PhotoRendering {
    /// Whether these bytes are the camera's, untouched.
    #[must_use]
    pub const fn is_verbatim(self) -> bool {
        matches!(self, PhotoRendering::Verbatim { .. })
    }
}

/// Where a requested orientation ended up.
///
/// "The photo is rotated" and "the photo says it is rotated" are different facts, and a
/// viewer that ignores EXIF distinguishes them — so the answer records which happened
/// rather than leaving the caller to infer it from the sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TransformApplication {
    /// Nothing was asked for.
    Identity,
    /// The pixels were rotated or mirrored before encoding.
    Pixels,
    /// The pixels were left alone and the orientation rides in EXIF, so the bitstream
    /// stays verbatim (E6).
    ExifOrientation {
        /// The tag value [`Transform::exif_orientation`] produced.
        orientation: u16,
    },
}

/// Where binary results go (design D10).
///
/// Two variants, because a daemon and an in-process CLI need different answers and
/// pretending otherwise is how `-o out.jpg` ends up meaning the server's cwd. Clients
/// resolve relative paths against their own cwd *before* sending.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Sink {
    /// Send the bytes back in the response.
    ReturnBytes {
        /// The encoding to produce.
        format: PhotoFormat,
    },
    /// Write them to an absolute path on whichever host runs the engine.
    ServerPath {
        /// The absolute destination.
        #[schemars(with = "String")]
        path: Utf8PathBuf,
    },
}

/// Everything one photo needs (design D5, D6, D10).
///
/// Assembled by the caller so `wch photo`, the daemon's `photo` method and a calibration
/// sample all ask for a photo the same way — the sweep at P3 varies the control values
/// between shots and nothing else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PhotoRequest {
    /// What to ask the device's format negotiation for.
    #[serde(default)]
    pub stream: StreamRequest,
    /// How long to let the sensor settle first \[PF:11\].
    #[serde(default)]
    pub settle: SettlePolicy,
    /// The orientation, applied to pixels or recorded in EXIF depending on the sink (E6).
    #[serde(default)]
    pub transform: Transform,
    /// Where the bytes go.
    pub sink: Sink,
}

/// Where a photo's bytes ended up — [`Sink`], answered.
///
/// The two variants pair with the two sinks, and each says the thing its caller cannot
/// otherwise learn: a path answer reports how much was written, and a bytes answer reports
/// how much is on its way. **The bytes themselves are not in this document**: `wch` streams
/// them to standard output, and D10's base64-in-JSON encoding lands at P4 with the wire
/// surface that needs it (docs/2 puts the sink DTO in `webcam-handler-api`). Carrying an
/// unused encoding here would be a dependency nobody reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PhotoDelivery {
    /// The bytes were handed back to the caller.
    Bytes {
        /// The encoding they are in.
        format: PhotoFormat,
        /// How many there are.
        byte_count: u64,
    },
    /// The bytes were written to a file.
    Path {
        /// Where.
        #[schemars(with = "String")]
        path: Utf8PathBuf,
        /// How many bytes it holds.
        byte_count: u64,
    },
}

impl PhotoDelivery {
    /// How many bytes the photo came to, whichever way it was delivered.
    #[must_use]
    pub const fn byte_count(&self) -> u64 {
        match self {
            PhotoDelivery::Bytes { byte_count, .. } | PhotoDelivery::Path { byte_count, .. } => {
                *byte_count
            }
        }
    }
}

/// What `photo` answers (design D6).
///
/// Self-describing on purpose: a saved `--json` document says which camera, when, what the
/// device actually agreed to stream, and whether the bytes are the camera's own — the four
/// things a calibration sample or a bug report needs and cannot reconstruct later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PhotoReport {
    /// Which camera took it.
    pub camera: CameraId,
    /// When.
    pub taken_at: Stamp,
    /// What the device agreed to, including every way that differs from the request (D5).
    pub negotiated: NegotiatedStream,
    /// Which of D6's three paths produced the bytes.
    pub rendering: PhotoRendering,
    /// Where the orientation request went.
    pub transform: TransformApplication,
    /// The width of the image the bytes encode, after any pixel-domain transform.
    pub width: u32,
    /// The height of the same.
    pub height: u32,
    /// How many frames were discarded before this one \[PF:11\].
    pub frames_settled: u32,
    /// Where the bytes went.
    pub delivery: PhotoDelivery,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_never_debug_prints_its_bytes() {
        // Rubric A12 as a test: a frame may contain a person, so it must be impossible
        // to leak one into a log line by formatting a struct that holds it.
        let frame = Frame {
            bytes: vec![0xde, 0xad, 0xbe, 0xef],
            pixel_format: PixelFormat::MJPG,
            width: 1920,
            height: 1080,
            bytes_per_line: 0,
            sequence: 7,
            timestamp_us: 1_234,
        };
        let rendered = format!("{frame:?}");
        assert!(rendered.contains("<4 bytes>"), "{rendered}");
        assert!(!rendered.contains("222"), "byte values leaked: {rendered}");
        assert!(!rendered.contains("0xde"), "byte values leaked: {rendered}");
    }

    #[test]
    fn transforms_map_onto_the_exif_orientation_vocabulary() {
        // Each transform gets a distinct tag, and `None` is the identity.
        assert_eq!(Transform::None.exif_orientation(), 1);
        let mut seen = std::collections::BTreeSet::new();
        for t in [
            Transform::None,
            Transform::HFlip,
            Transform::VFlip,
            Transform::Rot90,
            Transform::Rot180,
            Transform::Rot270,
        ] {
            assert!(
                seen.insert(t.exif_orientation()),
                "{t:?} duplicates a tag value"
            );
        }
    }

    #[test]
    fn photo_formats_round_trip_through_their_extensions() {
        for &f in PhotoFormat::ALL {
            assert_eq!(PhotoFormat::from_extension(f.extension()), Some(f));
        }
        assert_eq!(PhotoFormat::from_extension("JPEG"), Some(PhotoFormat::Jpeg));
        assert_eq!(PhotoFormat::from_extension("webp"), None);
    }

    #[test]
    fn every_transform_and_format_parses_from_the_name_it_prints_and_serializes_as() {
        // One vocabulary, one spelling: `--transform hflip` and `"transform":"hflip"`
        // must name the same thing, so the serde rendering and `as_str` are compared
        // against each other rather than each trusted on its own.
        for &t in Transform::ALL {
            assert_eq!(Transform::parse(t.as_str()), Some(t));
            let json = serde_json::to_string(&t).expect("serialize");
            assert_eq!(json, format!("\"{}\"", t.as_str()), "{t:?}");
        }
        for &f in PhotoFormat::ALL {
            assert_eq!(PhotoFormat::parse(f.as_str()), Some(f));
            let json = serde_json::to_string(&f).expect("serialize");
            assert_eq!(json, format!("\"{}\"", f.as_str()), "{f:?}");
        }
        // The inverse: a name nobody defined parses to nothing rather than to a default.
        assert_eq!(Transform::parse("rot45"), None);
        assert_eq!(Transform::parse("h_flip"), None);
        assert_eq!(PhotoFormat::parse("webp"), None);
    }

    #[test]
    fn the_negotiation_diff_reports_every_field_that_moved_and_no_field_that_did_not() {
        let exact = StreamRequest {
            pixel_format: Some(PixelFormat::MJPG),
            width: Some(1920),
            height: Some(1080),
            interval: Some(FrameInterval::Discrete {
                numerator: 1,
                denominator: 30,
            }),
            buffer_count: 4,
        };
        assert!(
            NegotiatedStream::diff(
                &exact,
                PixelFormat::MJPG,
                1920,
                1080,
                FrameInterval::Discrete {
                    numerator: 1,
                    denominator: 30
                }
            )
            .is_empty(),
            "an honoured request has nothing to report"
        );

        let moved = NegotiatedStream::diff(
            &exact,
            PixelFormat::YUYV,
            640,
            480,
            FrameInterval::Discrete {
                numerator: 1,
                denominator: 15,
            },
        );
        assert_eq!(
            moved,
            vec![
                Adjustment::PixelFormat {
                    requested: PixelFormat::MJPG,
                    negotiated: PixelFormat::YUYV,
                },
                Adjustment::Size {
                    requested_width: 1920,
                    requested_height: 1080,
                    negotiated_width: 640,
                    negotiated_height: 480,
                },
                Adjustment::Interval {
                    requested: FrameInterval::Discrete {
                        numerator: 1,
                        denominator: 30
                    },
                    negotiated: FrameInterval::Discrete {
                        numerator: 1,
                        denominator: 15
                    },
                },
            ]
        );
    }

    #[test]
    fn a_request_that_asked_for_nothing_is_never_reported_as_adjusted() {
        // "Just give me something" cannot have been disappointed. A diff that reported
        // adjustments here would make every default-request photo look negotiated-away.
        let anything = StreamRequest::default();
        assert!(
            NegotiatedStream::diff(
                &anything,
                PixelFormat::YUYV,
                640,
                480,
                FrameInterval::Discrete {
                    numerator: 1,
                    denominator: 15
                }
            )
            .is_empty()
        );
    }

    #[test]
    fn a_request_that_named_only_one_dimension_reports_no_size_adjustment() {
        // `Adjustment::Size` carries both requested dimensions; filling the unnamed one
        // in from the answer would put a number in the caller's mouth.
        let half = StreamRequest {
            width: Some(1920),
            ..StreamRequest::default()
        };
        assert!(
            NegotiatedStream::diff(
                &half,
                PixelFormat::MJPG,
                640,
                480,
                FrameInterval::Discrete {
                    numerator: 1,
                    denominator: 30
                }
            )
            .is_empty()
        );
    }

    /// A device offering one format at three sizes, in the order a driver lists them.
    fn seed_formats() -> Vec<FormatInfo> {
        use crate::camera::{FrameSize, FrameSizeInfo};

        let size = |width, height| FrameSizeInfo {
            size: FrameSize::Discrete { width, height },
            intervals: Vec::new(),
        };
        let format = |pixel_format, sizes| FormatInfo {
            pixel_format,
            description: format!("{pixel_format}"),
            flags: 0,
            sizes,
        };
        vec![
            // The Chicony's own MJPG ordering: 1280x720 first, then smaller entries.
            format(
                PixelFormat::MJPG,
                vec![size(1280, 720), size(320, 180), size(640, 480)],
            ),
            format(PixelFormat::YUYV, vec![size(640, 480)]),
        ]
    }

    #[test]
    fn a_request_that_asks_for_nothing_gets_the_devices_own_first_format_and_size() {
        // Deterministic on purpose, and this is the one that bit: a V4L2 node's format is
        // *persistent device state*, so resolving "anything" against the node's current
        // format made `wch photo` depend on what ran before it. Against the enumeration,
        // the same request is the same answer every time.
        let chosen = StreamRequest::default()
            .choose(&seed_formats())
            .expect("a device with formats resolves");
        assert_eq!(
            (chosen.pixel_format, chosen.width, chosen.height),
            (PixelFormat::MJPG, 1280, 720)
        );
    }

    #[test]
    fn an_offered_size_is_taken_exactly_and_an_unoffered_one_falls_to_the_largest_that_fits() {
        let formats = seed_formats();
        let exact = StreamRequest {
            pixel_format: Some(PixelFormat::MJPG),
            width: Some(640),
            height: Some(480),
            ..StreamRequest::default()
        }
        .choose(&formats)
        .expect("offered");
        assert_eq!((exact.width, exact.height), (640, 480));

        // 800x600 is not offered; 640x480 is the largest offered size inside it.
        let inside = StreamRequest {
            width: Some(800),
            height: Some(600),
            ..StreamRequest::default()
        }
        .choose(&formats)
        .expect("resolves");
        assert_eq!((inside.width, inside.height), (640, 480));

        // Nothing fits inside 3x3, so the device's first entry stands — which is what
        // makes the answer *different* from the request, and therefore reportable.
        let tiny = StreamRequest {
            width: Some(3),
            height: Some(3),
            ..StreamRequest::default()
        }
        .choose(&formats)
        .expect("resolves");
        assert_eq!((tiny.width, tiny.height), (1280, 720));
    }

    #[test]
    fn a_half_specified_size_falls_through_rather_than_inventing_the_other_half() {
        let chosen = StreamRequest {
            width: Some(640),
            ..StreamRequest::default()
        }
        .choose(&seed_formats())
        .expect("resolves");
        assert_eq!(
            (chosen.width, chosen.height),
            (1280, 720),
            "a width with no height cannot pick a height without inventing an aspect ratio"
        );
        // And the diff says nothing was adjusted, because nothing comparable was asked.
        assert!(
            NegotiatedStream::diff(
                &StreamRequest {
                    width: Some(640),
                    ..StreamRequest::default()
                },
                chosen.pixel_format,
                chosen.width,
                chosen.height,
                FrameInterval::Discrete {
                    numerator: 1,
                    denominator: 30
                },
            )
            .is_empty()
        );
    }

    #[test]
    fn a_requested_format_is_honoured_and_an_absent_one_falls_to_the_first() {
        let formats = seed_formats();
        let yuyv = StreamRequest {
            pixel_format: Some(PixelFormat::YUYV),
            ..StreamRequest::default()
        }
        .choose(&formats)
        .expect("offered");
        assert_eq!(yuyv.pixel_format, PixelFormat::YUYV);
        assert_eq!((yuyv.width, yuyv.height), (640, 480));

        // A format the device does not list: the chooser picks the device's first, and
        // reporting *that* as an adjustment is `diff`'s job, not this function's.
        let absent = StreamRequest {
            pixel_format: PixelFormat::parse("H264"),
            ..StreamRequest::default()
        }
        .choose(&formats)
        .expect("resolves");
        assert_eq!(absent.pixel_format, PixelFormat::MJPG);

        // A device with nothing to offer resolves to nothing rather than to a guess.
        assert!(StreamRequest::default().choose(&[]).is_none());
    }

    #[test]
    fn a_photo_answer_round_trips_and_reports_its_size_either_way_it_was_delivered() {
        let report = PhotoReport {
            camera: crate::camera::CameraId::parse("cam:test").expect("literal id"),
            taken_at: Stamp::epoch(),
            negotiated: NegotiatedStream {
                pixel_format: PixelFormat::MJPG,
                width: 1280,
                height: 720,
                bytes_per_line: 0,
                size_image: 1 << 20,
                interval: FrameInterval::Discrete {
                    numerator: 1,
                    denominator: 30,
                },
                adjustments: Vec::new(),
            },
            rendering: PhotoRendering::Verbatim {
                source: PixelFormat::MJPG,
            },
            transform: TransformApplication::ExifOrientation { orientation: 6 },
            width: 1280,
            height: 720,
            frames_settled: 10,
            delivery: PhotoDelivery::Path {
                path: Utf8PathBuf::from("/tmp/shot.jpg"),
                byte_count: 91_234,
            },
        };
        let json = serde_json::to_string(&report).expect("serialize");
        let back: PhotoReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, report);
        assert_eq!(back.delivery.byte_count(), 91_234);
        assert!(back.rendering.is_verbatim());

        let returned = PhotoDelivery::Bytes {
            format: PhotoFormat::Png,
            byte_count: 17,
        };
        assert_eq!(returned.byte_count(), 17);
        // The inverse of `is_verbatim`, so the predicate is not measuring nothing.
        assert!(
            !PhotoRendering::DecodedAndEncoded {
                source: PixelFormat::MJPG,
                target: PhotoFormat::Png,
            }
            .is_verbatim()
        );
    }

    #[test]
    fn a_photo_request_fills_its_defaults_the_way_the_limits_table_says() {
        let parsed: PhotoRequest =
            serde_json::from_str(r#"{"sink":{"kind":"return_bytes","format":"jpeg"}}"#)
                .expect("a sink is the only required field");
        assert_eq!(parsed.stream, StreamRequest::default());
        assert_eq!(parsed.settle, SettlePolicy::default());
        assert_eq!(parsed.transform, Transform::None);
        assert_eq!(
            parsed.sink,
            Sink::ReturnBytes {
                format: PhotoFormat::Jpeg
            }
        );
    }

    #[test]
    fn defaults_come_from_the_limits_table() {
        let policy = SettlePolicy::default();
        assert_eq!(policy.deadline_ms, limits::DEFAULT_SETTLE_DEADLINE_MS);
        assert_eq!(
            policy.spec,
            SettleSpec::SkipFrames {
                frames: limits::DEFAULT_SETTLE_SKIP_FRAMES
            }
        );
        // The Rust default and the serde default are the same value, both directions.
        assert_eq!(
            StreamRequest::default().buffer_count,
            limits::DEFAULT_BUFFER_COUNT
        );
        let from_empty: StreamRequest = serde_json::from_str("{}").expect("parse");
        assert_eq!(from_empty, StreamRequest::default());
    }

    #[test]
    fn a_negotiated_stream_reports_how_it_differs() {
        let exact = NegotiatedStream {
            pixel_format: PixelFormat::MJPG,
            width: 1920,
            height: 1080,
            bytes_per_line: 0,
            size_image: 1 << 20,
            interval: FrameInterval::Discrete {
                numerator: 1,
                denominator: 30,
            },
            adjustments: Vec::new(),
        };
        assert!(exact.is_exact());
        let adjusted = NegotiatedStream {
            adjustments: vec![Adjustment::PixelFormat {
                requested: PixelFormat::YUYV,
                negotiated: PixelFormat::MJPG,
            }],
            ..exact
        };
        assert!(!adjusted.is_exact());
    }
}
