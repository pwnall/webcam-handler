//! Capture vocabulary: what a caller asks for, what the device agreed to, and what came
//! back (design D5, D6).
//!
//! The theme is D3's, applied to formats instead of controls: drivers adjust silently, so
//! the *negotiated* result is always reported alongside the request.

use camino::Utf8PathBuf;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::camera::{FrameInterval, PixelFormat};
use crate::limits;

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

/// How long to wait for the sensor to settle before taking a photo [PF:11].
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

/// An orientation transform (the skill's flip and rotate).
///
/// On the verbatim-JPEG sink these become an EXIF Orientation tag — zero re-encode, byte
/// fidelity preserved (E6). On PNG and re-encode sinks they are applied to pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Transform {
    /// Leave the image as the camera framed it.
    #[default]
    None,
    /// Mirror horizontally.
    HFlip,
    /// Mirror vertically.
    VFlip,
    /// Rotate a quarter turn clockwise.
    Rot90,
    /// Rotate a half turn.
    Rot180,
    /// Rotate a quarter turn counter-clockwise.
    Rot270,
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
}

/// The encoding a photo lands in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PhotoFormat {
    /// JPEG. Verbatim camera bytes when the stream is already MJPG (E6), else encoded.
    Jpeg,
    /// PNG. Always encoded, always lossless.
    Png,
    /// Binary PPM/PGM — the "give me pixels" escape hatch for tooling.
    Ppm,
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
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            PhotoFormat::Jpeg => "jpg",
            PhotoFormat::Png => "png",
            PhotoFormat::Ppm => "ppm",
        }
    }
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
        for f in [PhotoFormat::Jpeg, PhotoFormat::Png, PhotoFormat::Ppm] {
            assert_eq!(PhotoFormat::from_extension(f.extension()), Some(f));
        }
        assert_eq!(PhotoFormat::from_extension("JPEG"), Some(PhotoFormat::Jpeg));
        assert_eq!(PhotoFormat::from_extension("webp"), None);
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
