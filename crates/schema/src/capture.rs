//! Capture vocabulary: what a caller asks for, what the device agreed to, and what came
//! back (design D5, D6).
//!
//! The theme is D3's, applied to formats instead of controls: drivers adjust silently, so
//! the *negotiated* result is always reported alongside the request.

use std::fmt;

use camino::Utf8PathBuf;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::camera::{CameraId, FormatInfo, FrameInterval, Lossiness, PixelFormat, SinkFidelity};
use crate::error::{Error, Result};
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
    /// What the destination these frames are going to can do with them (D5's 2026-08-13
    /// amendment).
    ///
    /// **Not a caller's field, which is why it is `#[serde(default)]` and
    /// `#[serde(skip)]`.** Every other field here is something somebody asked for; this one
    /// is *derived* — [`PhotoRequest::stream_for_sink`] computes it from the sink the same
    /// request carries, at the moment the request reaches a device. Serializing it would
    /// put a second, contradictable answer to "can this destination take a camera
    /// bitstream" on the wire beside the sink that already answers it, and a client that
    /// set `png` here and `.jpg` there would have said two things.
    ///
    /// It rides on the request rather than beside it because the T1 `start_stream` takes
    /// one [`StreamRequest`] and the backends are what call [`StreamRequest::choose`] — so
    /// this is the only seat from which the chooser can be told, and the alternative is a
    /// second parameter on the backend trait for a bit only one of its callers has.
    ///
    /// Because it is derived at the point of use rather than sent, `webcam-handler-cli` and
    /// `webcam-handler-client` get the same answer for the same command without the wire
    /// carrying anything: both roots reach [`PhotoRequest::stream_for_sink`], one in process
    /// and one after the daemon has parsed the document.
    #[serde(default, skip)]
    pub sink_fidelity: SinkFidelity,
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
            sink_fidelity: SinkFidelity::default(),
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
    /// - **Format**: the requested one when the device offers it, else the **best-ranked**
    ///   one — see [`rank_formats`] for the ranking and the owner ruling that replaced
    ///   "the device's first" with it on 2026-08-13. An explicit request is untouched by
    ///   the ranking: a caller who names a format is answered, and the ranking exists only
    ///   for the request that named none.
    /// - **Size**: the largest thing the *chosen* format offers that fits inside the
    ///   request — which is the exact request when the device offers it, and the biggest
    ///   available frame when a caller asks a 720p camera for 1080p. A **stepwise** entry
    ///   is asked about as the range it is rather than as its maximum, so a device that
    ///   can deliver the exact request delivers it. Nothing fitting falls back to the
    ///   format's largest size, which is also what an unspecified request gets.
    /// - **Largest, not first.** Until 2026-08-13 an unspecified size took the format's
    ///   *first* size entry at its maximum, on the same "the driver ordered these"
    ///   argument the format rule used. \[PF:26\] measured what that costs: the BRIO's MJPG
    ///   list starts at 640×480 and ends at 4096×2160, so picking the format for its 8.8
    ///   megapixels and then streaming its first entry would have been the same defect one
    ///   level down.
    /// - **A half-specified size names nothing.** Width alone cannot pick a height without
    ///   inventing an aspect ratio, so the size falls through as though nothing had been
    ///   asked — and [`NegotiatedStream::diff`] then reports no size adjustment, because
    ///   none was requested in a form the answer could differ from.
    ///
    /// `None` when `formats` is empty or lists nothing with readable dimensions — a
    /// camera that offers no size is not one this can pick from, and the caller turns that
    /// into [`crate::Error::FormatUnsupported`] with the list it does have.
    #[must_use]
    pub fn choose(&self, formats: &[FormatInfo]) -> Option<ChosenFormat> {
        let requested = self
            .pixel_format
            .and_then(|wanted| formats.iter().find(|f| f.pixel_format == wanted));
        let (chosen, reason) = match requested {
            Some(named) => (named, ChoiceReason::Requested),
            None => rank_formats(formats, self.sink_fidelity)?,
        };

        // The chosen format at its largest — what "just give me something" resolves to,
        // and the fallback when nothing the caller asked for fits. `max_by_key` keeps the
        // *last* maximum, so the index goes into the key reversed: two entries of equal
        // area must resolve to the same one on every run, or two photographs an hour apart
        // differ where the device did not (Expected usage item 2).
        let default_size = chosen
            .sizes
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| Some((index, entry.size.max_dimensions()?)))
            .max_by_key(|(index, (w, h))| (area(*w, *h), std::cmp::Reverse(*index)))
            .map(|(_, size)| size)?;

        let (width, height) = match (self.width, self.height) {
            (Some(width), Some(height)) => chosen
                .sizes
                .iter()
                .enumerate()
                // `largest_within` rather than a comparison against `max_dimensions`: a
                // **stepwise** entry offers a whole range, and asking only about its
                // maximum makes a device that can deliver 640x480 exactly report
                // 1920x1080 and call it an adjustment.
                .filter_map(|(index, entry)| {
                    Some((index, entry.size.largest_within(width, height)?))
                })
                .max_by_key(|(index, (w, h))| (area(*w, *h), std::cmp::Reverse(*index)))
                .map_or(default_size, |(_, size)| size),
            _ => default_size,
        };

        Some(ChosenFormat {
            pixel_format: chosen.pixel_format,
            width,
            height,
            reason,
        })
    }

    /// This request, told what the destination its frames are going to can carry.
    ///
    /// How [`StreamRequest::sink_fidelity`] is set on every path that takes a photograph, so
    /// the derivation is one line rather than a struct literal repeated per caller. Two
    /// callers supply the argument: [`PhotoRequest::stream_for_sink`] from a [`Sink`], and a
    /// calibration sweep from the [`PhotoFormat`] its samples are written in — the same rule
    /// read from the two shapes a destination comes in. The field stays public because a
    /// test that wants to state a destination outright should not have to build a photo
    /// request around it.
    #[must_use]
    pub fn for_sink(&self, sink_fidelity: SinkFidelity) -> StreamRequest {
        StreamRequest {
            sink_fidelity,
            ..self.clone()
        }
    }
}

/// Pixels, in the width that will not overflow when a driver enumerates something silly.
fn area(width: u32, height: u32) -> u64 {
    u64::from(width) * u64::from(height)
}

/// The format a device should be asked for when the caller named none (design D5, amended
/// by the owner's ruling of 2026-08-13), and why that one.
///
/// **A pure function over values.** It takes the format tree a device enumerated and
/// answers with a borrow into it; there is no device, no I/O and no order of operations, so
/// every committed profile in `corpus/` can be run through it in a unit test and the fake
/// and the hardware cannot disagree about what `StreamRequest::default()` means.
///
/// ## The ruling
///
/// > "Let's re-rank the formats offered by the device and ignore the ordering. Our intended
/// > usage benefits from higher-quality photos, even if they cost more bandwidth or
/// > latency. So, let's re-rank — higher-resolution formats are preferred to lower-
/// > resolution formats, and less lossy encodings are preferred to more lossy encodings."
///
/// It overturns D5's original rule, which took the device's first enumerated format on the
/// argument that "the order `VIDIOC_ENUM_FMT` returns is the driver's own preference, and
/// second-guessing it is how a tool ends up defaulting to a mode the camera is worse at".
/// That argument was sound and \[PF:26\] is the measurement that charges for it: the BRIO
/// enumerates YUYV first at 640×480, so the default photograph was a re-encoded VGA from a
/// camera offering verbatim 4096×2160 — 3.5% of its pixels. The driver's order is not
/// deleted; it is **demoted to the last tiebreak**, where it still decides between
/// candidates nothing else separates.
///
/// ## The keys, outermost first
///
/// 1. **A size this build can read**, because a format whose every size entry is a
///    `v4l2_frmsizetypes` value we cannot interpret is one we cannot ask for.
/// 2. **A FourCC this build can name.** Ahead of resolution deliberately, and this is the
///    one place the ranking departs from the ruling's own wording: the ruling is about
///    which *photograph* is better, and a format `imaging::decode` cannot decode produces
///    no photograph at all, only a
///    [`FormatUnsupported`](crate::Error::FormatUnsupported). Nothing would have preferred
///    it on merit either — [`Lossiness::rank_for`] cannot say an unrecognised encoding is
///    good — so the honest reading is that an unknown format is not a candidate while a
///    known one exists. It is ranked rather than filtered, so a device offering nothing
///    else still resolves to something deterministic. The hazard this answers is real and
///    the ruling made it likelier: `H264` and `HEVC` modes are typically offered at a
///    device's *largest* sizes, so resolution-first without this key would prefer exactly
///    the formats this build cannot turn into a file.
/// 3. **Pixels**, the ruling's primary key: a format's resolution is the largest its size
///    list offers, across every entry rather than at the first one.
/// 4. **Fidelity**, the ruling's secondary key — [`Lossiness::rank_for`], which is also
///    where `sink` gets its vote. The owner's sentence listed resolution first and
///    lossiness second and admits the other reading; this takes resolution as primary and
///    lossiness as the tiebreak, and says so here because a future reader deserves to know
///    which was taken. The Dell exercises it: `NV12` and `YUYV` tie at 1920×1080, and 4:2:0
///    keeps a quarter of the chroma where 4:2:2 keeps half, so `YUYV` wins that pair.
/// 5. **The driver's enumeration order**, D5's original rule, kept last so the answer is a
///    function of the device rather than of a `Vec`'s iteration accident.
///
/// `None` only for an empty list — every non-empty one has a best element, because every
/// key is total.
#[must_use]
pub fn rank_formats(
    formats: &[FormatInfo],
    sink: SinkFidelity,
) -> Option<(&FormatInfo, ChoiceReason)> {
    let key = |format: &FormatInfo| {
        let lossiness = Lossiness::of(format.pixel_format);
        let pixels = format
            .sizes
            .iter()
            .filter_map(|entry| entry.size.max_dimensions())
            .map(|(w, h)| area(w, h))
            .max();
        (
            pixels.is_some(),
            lossiness.is_named(),
            pixels.unwrap_or(0),
            lossiness.rank_for(sink),
        )
    };

    // `max_by_key` keeps the last maximum; the reversed index makes it keep the first,
    // which is what leaves the driver's own order as the final tiebreak.
    let (index, best) = formats
        .iter()
        .enumerate()
        .max_by_key(|(index, format)| (key(format), std::cmp::Reverse(*index)))?;

    // Why it won, decided by how many others reached the same rank — so the answer names
    // the rule that actually fired rather than the rule that usually fires.
    let winning = key(best);
    let tied_on_size = formats.iter().filter(|f| key(f).2 == winning.2).count();
    let tied_outright = formats.iter().filter(|f| key(f) == winning).count();
    let reason = if tied_on_size == 1 {
        ChoiceReason::MostPixels
    } else if tied_outright == 1 {
        ChoiceReason::LeastLossyOfTheLargest { sink }
    } else {
        ChoiceReason::FirstOfEqualsInDriverOrder { index }
    };
    Some((best, reason))
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
    /// Which rule picked the format.
    pub reason: ChoiceReason,
}

/// Why [`StreamRequest::choose`] picked the format it did (design D5, amended 2026-08-13).
///
/// The ruling that re-ranked the formats has a tiebreak whose answer depends on where the
/// frames are going, and \[PF:26\]'s complaint about the rule it replaced was that the
/// answer said nothing about the choice: "the document is honest, but nothing in the answer
/// says 'this camera also offers 4096×2160 MJPG and you did not ask for it'". So the
/// chooser's answer names the rule that fired. It is beside [`ChosenFormat`] and not on the
/// wire, because it explains a *decision this program made* rather than anything the device
/// agreed to — what the device agreed to is [`NegotiatedStream`], and the difference from
/// the request is [`Adjustment`], and both of those stay exactly as they were.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoiceReason {
    /// The caller named this format and the device offers it. The ranking never ran.
    Requested,
    /// It offers more pixels than any other format the device lists.
    MostPixels,
    /// It ties for most pixels with at least one other format, and keeps more of what the
    /// sensor produced than every format it ties with — for the destination these frames
    /// are going to, which is what `sink` records.
    ///
    /// An unrecognised FourCC loses here too: [`Lossiness::rank_for`] ranks what it cannot
    /// name below what it can, in both destinations.
    LeastLossyOfTheLargest {
        /// The destination whose vote decided it.
        sink: SinkFidelity,
    },
    /// It ties on every key the ranking can measure, and the driver enumerated it first —
    /// D5's original rule, kept as the last tiebreak so the answer is a function of the
    /// device rather than of iteration order.
    FirstOfEqualsInDriverOrder {
        /// Where in the device's enumeration it sits.
        index: usize,
    },
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

impl SinkFidelity {
    /// What a photo in this encoding can do with the frame it is made from (design D5,
    /// amended 2026-08-13).
    ///
    /// One home for the map, beside the encodings it reads, with the argument for each arm
    /// in D6's own terms:
    ///
    /// - **JPEG** is the verbatim path. An `MJPG` frame destined for a `.jpg` sink is
    ///   written byte for byte (E6), and the EXIF stamp is a header splice that leaves the
    ///   scan alone \[PF:16\] — so the camera's own bitstream reaches the file with nothing
    ///   of ours in it.
    /// - **PNG** and **PPM** always encode, and both encode losslessly: a frame is decoded
    ///   or converted to pixels and those pixels are written whole. Whatever loss a photo
    ///   in one of these carries is loss the *camera* applied before we saw the frame,
    ///   which is exactly why an uncompressed source is the better one here.
    ///
    /// An exhaustive match, so a fourth encoding cannot be added without answering this
    /// question for it — a lossy encoding that could not pass a camera bitstream through
    /// would be a third answer rather than a second copy of one of these.
    #[must_use]
    pub const fn of(format: PhotoFormat) -> Self {
        match format {
            PhotoFormat::Jpeg => SinkFidelity::PassesCompressedThrough,
            PhotoFormat::Png | PhotoFormat::Ppm => SinkFidelity::EncodesLosslessly,
        }
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

impl Sink {
    /// Whether this sink names somewhere the engine can actually address.
    ///
    /// D10 says a [`Sink::ServerPath`] is absolute and that clients resolve `-o out.jpg`
    /// against their **own** cwd before sending. `cli_core::Command::photo_request` is the one
    /// place that resolution happens, so a sink `webcam-handler-cli` built always satisfies
    /// this — which is exactly why the rule needs a predicate rather than a paragraph. The
    /// moment a `Sink` can arrive off a socket (P4c routes `wch_photo`) it can arrive
    /// relative, and the daemon's cwd under systemd is `/`:
    /// `{"kind":"server_path","path":"out.jpg"}` would silently write `/out.jpg` as the
    /// daemon's uid, or refuse naming a path the caller never asked for.
    ///
    /// A predicate beside the variants, and not a validating constructor, for the reason
    /// `api::PhotoResponse::bytes_match_the_delivery` is one: the document is built by
    /// somebody else's code, and a type that could not *represent* the malformed request
    /// could not refuse it either. `ReturnBytes` is always addressable — there is nowhere
    /// for it to be wrong.
    ///
    /// **Its consumer landed at P4c**: `daemon::server::addressable` asks this before the
    /// `photo` handler resolves a camera, so a request no build was going to honour costs
    /// nobody a descriptor, and the refusal it raises is `Error::IllegalTransition` naming the
    /// path (notes N34 and N46). `webcam-handler-cli` still cannot produce a sink that fails
    /// it, which is why the both-directions test for the rule lives here rather than there.
    #[must_use]
    pub fn is_addressable(&self) -> bool {
        match self {
            Sink::ReturnBytes { .. } => true,
            Sink::ServerPath { path } => path.is_absolute(),
        }
    }

    /// The encoding this sink asks for, or a refusal naming what this build writes.
    ///
    /// One home for a rule that used to have its two halves in two crates. The *decision* —
    /// `.png` means PNG, and a path with no extension at all is a JPEG — lived in
    /// `engine::photo::sink_format`, and the *refusal* for an extension this build cannot
    /// write lived in `cli_core::Command::photo_request`, where it ran while parsing a command
    /// line. The engine's comment said as much out loud: "an unknown one never reaches here —
    /// the CLI refuses it while building the sink". That sentence stopped being true the
    /// moment a `Sink` could arrive off a socket, because `webcam-handler-daemon` links no
    /// `cli-core`: `{"kind":"server_path","path":"/tmp/x.webp"}` produced JPEG bytes in a file
    /// named `.webp`, and a delivery reporting a path whose extension lies about its contents.
    /// Both surfaces call this now, so the refusal holds wherever a request comes from.
    ///
    /// A path with **no** extension is a JPEG rather than a refusal, and that arm is kept
    /// exactly as it was: it is a filename the caller chose and we do not get to rename it.
    ///
    /// This and [`Sink::is_addressable`] stay two questions — *in what encoding* and
    /// *where* — because they have different answers about who may ask. Every caller has to
    /// know the encoding; only a caller that can receive a path from somewhere else has to
    /// check that it is absolute.
    ///
    /// # Errors
    ///
    /// [`Error::IllegalTransition`] when the path's extension names an encoding this build
    /// does not write, naming both the extension that was typed and the three that are
    /// written — the list derived from [`PhotoFormat::ALL`] rather than spelled out.
    /// Deliberately **not** [`Error::FormatUnsupported`]: that variant is the camera saying
    /// what it cannot offer, and `.webp` is not the camera's fault (E3). Note **N46**
    /// records the pick and the one it shares it with.
    pub fn writable_format(&self) -> Result<PhotoFormat> {
        match self {
            Sink::ReturnBytes { format } => Ok(*format),
            Sink::ServerPath { path } => match path.extension() {
                None => Ok(PhotoFormat::Jpeg),
                Some(extension) => {
                    PhotoFormat::from_extension(extension).ok_or_else(|| Error::IllegalTransition {
                        from: format!("unwritable_extension({extension})"),
                        op: format!(
                            "write a photo to {path}; this build writes {}",
                            PhotoFormat::ALL
                                .iter()
                                .map(|format| format!(".{}", format.extension()))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    })
                }
            },
        }
    }
}

/// Everything one photo needs (design D5, D6, D10).
///
/// Assembled by the caller so `webcam-handler-cli photo`, the daemon's `photo` method and a
/// calibration sample all ask for a photo the same way — the sweep at P3 varies the control
/// values between shots and nothing else.
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
    /// Whether a request that finds the camera's command queue full waits for its turn.
    ///
    /// D12's flag: *"a second capture request queues or is refused with `Busy` per its
    /// `wait` flag"*. `false` — the default, and the shape every client sent before this
    /// field existed — is [`crate::Error::Busy`] the moment
    /// [`crate::limits::CAMERA_COMMAND_QUEUE_DEPTH`] commands are already waiting. `true`
    /// waits for room, bounded by [`crate::limits::CAMERA_ENQUEUE_WAIT_MS`], and takes the
    /// *same* refusal when that budget is spent: the flag changes when the answer arrives,
    /// never what it is.
    ///
    /// `#[serde(default)]` like its three siblings, so a request written before this field
    /// existed still parses and still means what it meant. `false` rather than `true` for
    /// the same reason: it is the behaviour every caller has already met, and a default that
    /// silently turned a prompt refusal into ten seconds of latency would be a change to
    /// requests nobody rewrote.
    ///
    /// It is on the *request* rather than beside it because the T4 executor surface takes
    /// one `&PhotoRequest` and the T5 method takes one assembled request (design §2.10), so
    /// a value here reaches every caller without a second parameter on three signatures.
    /// The flag is meaningful only where something else can be holding the camera's one
    /// thread, which today is the daemon; note **N42** records why it has no command-line
    /// spelling yet.
    #[serde(default)]
    pub wait: bool,
}

impl PhotoRequest {
    /// This request's stream, told what its sink can carry (design D5, amended 2026-08-13).
    ///
    /// **The one place [`StreamRequest::sink_fidelity`] is written.** A photo request is the
    /// only value in this vocabulary that holds a stream and a destination at once, so it is
    /// the only value that can answer the tiebreak's question — and answering it here rather
    /// than in each caller is what keeps `webcam-handler-cli`, `webcam-handler-daemon` and a
    /// calibration sweep from asking one device three subtly different questions (design
    /// §2.10).
    ///
    /// A sink naming an encoding this build cannot write has no destination to reason
    /// about. It gets the default, and it gets refused a moment later by
    /// [`Sink::writable_format`] — every caller of this asks that too, before a camera is
    /// opened, so the fallback here is never the last word on such a request. Guessing is
    /// safe precisely because the guess never reaches a device.
    #[must_use]
    pub fn stream_for_sink(&self) -> StreamRequest {
        self.stream.for_sink(
            self.sink
                .writable_format()
                .map_or_else(|_| SinkFidelity::default(), SinkFidelity::of),
        )
    }
}

/// Where a photo's bytes ended up — [`Sink`], answered.
///
/// The two variants pair with the two sinks, and each says the thing its caller cannot
/// otherwise learn: a path answer reports how much was written, and a bytes answer reports how
/// much is on its way. **The bytes themselves are not in this document**: `webcam-handler-cli`
/// streams them to standard output, and D10's base64-in-JSON encoding lives with the wire
/// surface that needs it — `webcam-handler-api`'s `photo::Base64Bytes`, beside the report
/// rather than inside this enum. Carrying an unused encoding here would be a dependency nobody
/// reads, and it would put base64 in every session file and `--json` document too.
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
            ..StreamRequest::default()
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

    /// A frame size entry with no intervals — the shape every fixture here needs.
    fn size(width: u32, height: u32) -> crate::camera::FrameSizeInfo {
        crate::camera::FrameSizeInfo {
            size: crate::camera::FrameSize::Discrete { width, height },
            intervals: Vec::new(),
        }
    }

    /// One enumerated format, described the way a driver describes it.
    fn format(pixel_format: PixelFormat, sizes: Vec<crate::camera::FrameSizeInfo>) -> FormatInfo {
        FormatInfo {
            pixel_format,
            description: format!("{pixel_format}"),
            flags: 0,
            sizes,
        }
    }

    /// The Chicony RGB camera's shape: MJPG with a size list that does not start at its
    /// largest, and a small YUYV beside it.
    ///
    /// The out-of-order size list is the fixture's whole job. It used to end at 1280×720,
    /// which is where the real device's list *begins* — and while it did, this fixture
    /// could not tell "the format's first size entry" from "the format's largest size",
    /// so the rule D5's amendment replaced and the rule that replaced it produced the same
    /// answer here and three tests below passed either way. `corpus/profiles/chicony-rgb.json`
    /// tops out at 2592×1944, so that entry is now in the list and the two rules disagree.
    fn seed_formats() -> Vec<FormatInfo> {
        vec![
            format(
                PixelFormat::MJPG,
                vec![
                    size(1280, 720),
                    size(320, 180),
                    size(640, 480),
                    size(2592, 1944),
                ],
            ),
            format(PixelFormat::YUYV, vec![size(640, 480)]),
        ]
    }

    #[test]
    fn a_request_that_asks_for_nothing_gets_the_best_ranked_format_at_its_largest_size() {
        // Deterministic on purpose, and this is the one that bit: a V4L2 node's format is
        // *persistent device state*, so resolving "anything" against the node's current format
        // made `webcam-handler-cli photo` depend on what ran before it. Against the
        // enumeration, the same request is the same answer every time.
        //
        // *Which* answer changed on 2026-08-13. It was "the device's first format at its
        // first size entry"; it is now the ranking's — most pixels, ties broken by
        // fidelity — and this fixture would answer 1280×720 under the old rule.
        let chosen = StreamRequest::default()
            .choose(&seed_formats())
            .expect("a device with formats resolves");
        assert_eq!(
            (chosen.pixel_format, chosen.width, chosen.height),
            (PixelFormat::MJPG, 2592, 1944)
        );
        assert_eq!(chosen.reason, ChoiceReason::MostPixels);
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

        // Nothing fits inside 3x3, so the format's largest entry stands — which is what
        // makes the answer *different* from the request, and therefore reportable. It was
        // the format's *first* entry until 2026-08-13; the fallback moved with the default
        // because they are one rule, and a fallback that stayed on the first entry would
        // have made "nothing fits" a quieter way of asking for the old behaviour.
        let tiny = StreamRequest {
            width: Some(3),
            height: Some(3),
            ..StreamRequest::default()
        }
        .choose(&formats)
        .expect("resolves");
        assert_eq!((tiny.width, tiny.height), (2592, 1944));
    }

    #[test]
    fn a_stepwise_size_is_asked_about_as_the_range_it_is_and_not_as_its_maximum() {
        // No seed camera is stepwise, which is why the first version of this function got
        // it wrong and nothing noticed: it built its candidate list from
        // `max_dimensions()`, so a device offering 32..1920 in steps of 2 was treated as
        // offering exactly one size — its largest — and a request for 640x480 that the
        // device can deliver *exactly* came back as 1920x1080 with an adjustment on it.
        use crate::camera::{FrameSize, FrameSizeInfo};

        let stepwise = vec![FormatInfo {
            pixel_format: PixelFormat::MJPG,
            description: "MJPG".to_owned(),
            flags: 0,
            sizes: vec![FrameSizeInfo {
                size: FrameSize::Stepwise {
                    min_width: 32,
                    max_width: 1920,
                    step_width: 2,
                    min_height: 32,
                    max_height: 1080,
                    step_height: 2,
                },
                intervals: Vec::new(),
            }],
        }];

        let exact = StreamRequest {
            width: Some(640),
            height: Some(480),
            ..StreamRequest::default()
        };
        let chosen = exact.choose(&stepwise).expect("resolves");
        assert_eq!(
            (chosen.width, chosen.height),
            (640, 480),
            "a size inside the declared range and on its grid is deliverable"
        );
        assert!(
            NegotiatedStream::diff(
                &exact,
                chosen.pixel_format,
                chosen.width,
                chosen.height,
                FrameInterval::Discrete {
                    numerator: 1,
                    denominator: 30
                },
            )
            .is_empty(),
            "and it is not an adjustment"
        );

        // Off the grid, the answer rounds *down* to it — a driver takes the grid it
        // declared, and reporting an off-grid size as agreed is a claim `S_FMT` refutes.
        let odd = StreamRequest {
            width: Some(641),
            height: Some(481),
            ..StreamRequest::default()
        }
        .choose(&stepwise)
        .expect("resolves");
        assert_eq!((odd.width, odd.height), (640, 480));

        // Above the range, clamped to the maximum; below the minimum, nothing fits and the
        // device's own first entry stands.
        let big = StreamRequest {
            width: Some(4096),
            height: Some(2160),
            ..StreamRequest::default()
        }
        .choose(&stepwise)
        .expect("resolves");
        assert_eq!((big.width, big.height), (1920, 1080));

        let tiny = StreamRequest {
            width: Some(8),
            height: Some(8),
            ..StreamRequest::default()
        }
        .choose(&stepwise)
        .expect("resolves");
        assert_eq!(
            (tiny.width, tiny.height),
            (1920, 1080),
            "nothing in a 32-pixel-minimum range fits inside 8x8"
        );
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
            (2592, 1944),
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
    fn a_requested_format_is_honoured_and_an_absent_one_falls_to_the_ranking() {
        let formats = seed_formats();
        let yuyv = StreamRequest {
            pixel_format: Some(PixelFormat::YUYV),
            ..StreamRequest::default()
        }
        .choose(&formats)
        .expect("offered");
        assert_eq!(yuyv.pixel_format, PixelFormat::YUYV);
        assert_eq!((yuyv.width, yuyv.height), (640, 480));
        assert_eq!(
            yuyv.reason,
            ChoiceReason::Requested,
            "a named format is not a ranked one, and the answer says which happened"
        );

        // A format the device does not list: the chooser ranks, and reporting *that* as an
        // adjustment is `diff`'s job, not this function's.
        let absent = StreamRequest {
            pixel_format: PixelFormat::parse("H264"),
            ..StreamRequest::default()
        }
        .choose(&formats)
        .expect("resolves");
        assert_eq!(absent.pixel_format, PixelFormat::MJPG);
        assert_eq!(absent.reason, ChoiceReason::MostPixels);

        // ... and it *is* reported, which is the half the ruling had to leave alone: the
        // chooser answering something other than what was named is exactly the case D5's
        // "the negotiated result is always surfaced" exists for, and the two functions are
        // asserted together here because separately each one looks correct.
        let asked = StreamRequest {
            pixel_format: PixelFormat::parse("H264"),
            ..StreamRequest::default()
        };
        assert_eq!(
            NegotiatedStream::diff(
                &asked,
                absent.pixel_format,
                absent.width,
                absent.height,
                FrameInterval::Discrete {
                    numerator: 1,
                    denominator: 30
                },
            ),
            vec![Adjustment::PixelFormat {
                requested: PixelFormat::parse("H264").expect("four characters"),
                negotiated: PixelFormat::MJPG,
            }],
            "a request answered with a different format and told nothing about it is a \
             photograph whose caller believes it is looking at H.264"
        );

        // A device with nothing to offer resolves to nothing rather than to a guess.
        assert!(StreamRequest::default().choose(&[]).is_none());
        assert!(rank_formats(&[], SinkFidelity::default()).is_none());
    }

    /// The Dell U3224KB/A's shape, in its own enumeration order: NV12 first, then YUYV at
    /// the same maximum, then MJPG well above both.
    ///
    /// Literal rather than loaded, because this crate is the bottom of the stack and the
    /// corpus loader lives in `webcam-handler-testkit` three layers up. The numbers are
    /// `corpus/profiles/dell-u3224kb.json`'s, and the corpus walk in
    /// `webcam-handler-fake`'s `corpus_replay` runs the real document through the same
    /// function so this fixture cannot quietly stop describing the device.
    fn dell_shaped_formats() -> Vec<FormatInfo> {
        vec![
            format(
                PixelFormat::NV12,
                vec![size(640, 480), size(1280, 720), size(1920, 1080)],
            ),
            format(
                PixelFormat::YUYV,
                vec![size(640, 480), size(1280, 720), size(1920, 1080)],
            ),
            format(
                PixelFormat::MJPG,
                vec![size(640, 480), size(1920, 1080), size(3840, 2160)],
            ),
        ]
    }

    #[test]
    fn the_ranking_takes_the_most_pixels_and_ignores_the_drivers_order() {
        // The whole of the owner's primary key, on the device that made the case: the Dell
        // enumerates NV12 first at 640×480 and D5's original rule took exactly that
        // [PF:26]. 8.3 megapixels of MJPG are three formats away and the ranking finds
        // them.
        let chosen = StreamRequest::default()
            .choose(&dell_shaped_formats())
            .expect("resolves");
        assert_eq!(
            (chosen.pixel_format, chosen.width, chosen.height),
            (PixelFormat::MJPG, 3840, 2160)
        );
        assert_eq!(chosen.reason, ChoiceReason::MostPixels);
    }

    #[test]
    fn a_tie_on_pixels_is_broken_by_how_much_of_the_sensor_survives() {
        // The Dell's real tie, isolated: NV12 and YUYV stop at the same 1920×1080, so the
        // primary key cannot separate them and the secondary one must. 4:2:0 keeps a
        // quarter of the chroma where 4:2:2 keeps half, so YUYV wins — and it wins from
        // *second* place in the driver's order, which is the demotion D5's amendment
        // performs.
        let pair: Vec<FormatInfo> = dell_shaped_formats()
            .into_iter()
            .filter(|f| f.pixel_format != PixelFormat::MJPG)
            .collect();
        assert_eq!(pair.len(), 2, "the pair is NV12 and YUYV");

        let (winner, reason) =
            rank_formats(&pair, SinkFidelity::PassesCompressedThrough).expect("two candidates");
        assert_eq!(winner.pixel_format, PixelFormat::YUYV);
        assert_eq!(
            reason,
            ChoiceReason::LeastLossyOfTheLargest {
                sink: SinkFidelity::PassesCompressedThrough
            }
        );

        // And the same pair the other way round, so the answer is the formats' and not the
        // list's: reversing the driver's order must not reverse the winner.
        let reversed: Vec<FormatInfo> = pair.into_iter().rev().collect();
        let (winner, _) =
            rank_formats(&reversed, SinkFidelity::PassesCompressedThrough).expect("two candidates");
        assert_eq!(
            winner.pixel_format,
            PixelFormat::YUYV,
            "the tiebreak is about chroma, not about position"
        );
    }

    #[test]
    fn the_tie_between_a_compressed_and_an_uncompressed_format_is_decided_by_the_destination() {
        // Question 2 of the ruling, and the case no camera this project has met has: a
        // device offering MJPG and YUYV at the *same* maximum. "Less lossy" alone picks
        // YUYV, which must then be encoded — so the camera's own bytes are lost on a JPEG
        // sink and gained on a PNG one. Measuring the loss over the whole path from sensor
        // to file rather than over the driver's buffer is what makes one rule out of the
        // two answers.
        let tied = vec![
            format(PixelFormat::MJPG, vec![size(1920, 1080)]),
            format(PixelFormat::YUYV, vec![size(1920, 1080)]),
        ];

        let jpeg = StreamRequest::default()
            .for_sink(SinkFidelity::PassesCompressedThrough)
            .choose(&tied)
            .expect("resolves");
        assert_eq!(
            jpeg.pixel_format,
            PixelFormat::MJPG,
            "a JPEG sink takes the camera's own bitstream byte for byte (E6); encoding \
             YUYV into it would insert artefacts the camera's own encoder did not"
        );

        let png = StreamRequest::default()
            .for_sink(SinkFidelity::EncodesLosslessly)
            .choose(&tied)
            .expect("resolves");
        assert_eq!(
            png.pixel_format,
            PixelFormat::YUYV,
            "a PNG sink encodes losslessly, so the uncompressed source arrives with the \
             sensor's own samples and the compressed one arrives already quantised"
        );

        // Both answers name the rule that fired, and each names the destination that voted.
        assert_eq!(
            jpeg.reason,
            ChoiceReason::LeastLossyOfTheLargest {
                sink: SinkFidelity::PassesCompressedThrough
            }
        );
        assert_eq!(
            png.reason,
            ChoiceReason::LeastLossyOfTheLargest {
                sink: SinkFidelity::EncodesLosslessly
            }
        );

        // The destination changes nothing when the resolutions differ, because it only
        // ever votes on a tie: a bigger MJPG beats a smaller YUYV into a PNG file too.
        let untied = vec![
            format(PixelFormat::MJPG, vec![size(3840, 2160)]),
            format(PixelFormat::YUYV, vec![size(1920, 1080)]),
        ];
        for &sink in SinkFidelity::ALL {
            let chosen = StreamRequest::default()
                .for_sink(sink)
                .choose(&untied)
                .expect("resolves");
            assert_eq!(
                (chosen.pixel_format, chosen.reason),
                (PixelFormat::MJPG, ChoiceReason::MostPixels),
                "{sink:?} got a vote it should not have had"
            );
        }
    }

    #[test]
    fn a_photo_requests_stream_is_told_what_its_own_sink_can_carry() {
        // The one place the derivation happens, both arms, from the sink the request
        // already carries — so a `.png` and a `.jpg` photo of the same camera rank its
        // formats differently without either caller arranging anything.
        let of = |sink: Sink| {
            PhotoRequest {
                stream: StreamRequest::default(),
                settle: SettlePolicy::default(),
                transform: Transform::None,
                sink,
                wait: false,
            }
            .stream_for_sink()
            .sink_fidelity
        };
        assert_eq!(
            of(Sink::ServerPath {
                path: "/tmp/a.jpg".into()
            }),
            SinkFidelity::PassesCompressedThrough
        );
        assert_eq!(
            of(Sink::ServerPath {
                path: "/tmp/a.png".into()
            }),
            SinkFidelity::EncodesLosslessly
        );
        assert_eq!(
            of(Sink::ReturnBytes {
                format: PhotoFormat::Ppm
            }),
            SinkFidelity::EncodesLosslessly
        );
        // An encoding this build cannot write has no destination to reason about. It takes
        // the default and is refused by `writable_format` before a camera is opened, which
        // is asserted beside the refusal itself rather than inferred here.
        assert_eq!(
            of(Sink::ServerPath {
                path: "/tmp/a.webp".into()
            }),
            SinkFidelity::default()
        );
        assert!(
            Sink::ServerPath {
                path: "/tmp/a.webp".into()
            }
            .writable_format()
            .is_err()
        );

        // Every field but the derived one survives the trip, so a request cannot lose its
        // caller's size on the way to a device.
        let asked = PhotoRequest {
            stream: StreamRequest {
                pixel_format: Some(PixelFormat::YUYV),
                width: Some(640),
                height: Some(480),
                interval: Some(FrameInterval::Discrete {
                    numerator: 1,
                    denominator: 30,
                }),
                buffer_count: 7,
                sink_fidelity: SinkFidelity::PassesCompressedThrough,
            },
            settle: SettlePolicy::default(),
            transform: Transform::None,
            sink: Sink::ReturnBytes {
                format: PhotoFormat::Png,
            },
            wait: false,
        };
        let derived = asked.stream_for_sink();
        assert_eq!(derived.sink_fidelity, SinkFidelity::EncodesLosslessly);
        assert_eq!(
            StreamRequest {
                sink_fidelity: SinkFidelity::PassesCompressedThrough,
                ..derived
            },
            asked.stream
        );
    }

    #[test]
    fn an_explicit_request_beats_the_ranking_in_both_of_its_halves() {
        // The ruling re-ranks the *unspecified* request and nothing else. A caller that
        // names a format and a size gets them off the device that would otherwise have
        // answered 3840×2160 MJPG — which is the whole reason `--format` and `--size` were
        // an adequate answer to [PF:26] on the day it was measured.
        let formats = dell_shaped_formats();
        let named = StreamRequest {
            pixel_format: Some(PixelFormat::NV12),
            width: Some(1280),
            height: Some(720),
            ..StreamRequest::default()
        };
        let chosen = named.choose(&formats).expect("offered");
        assert_eq!(
            (chosen.pixel_format, chosen.width, chosen.height),
            (PixelFormat::NV12, 1280, 720)
        );
        assert_eq!(chosen.reason, ChoiceReason::Requested);
        // ... and an honoured request is not an adjustment, so nothing is reported.
        assert!(
            NegotiatedStream::diff(
                &named,
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

        // The destination gets no vote over a named format either: a PNG sink would rank
        // YUYV above MJPG on a tie, and this caller asked for neither.
        let over_png = StreamRequest {
            sink_fidelity: SinkFidelity::EncodesLosslessly,
            ..named
        }
        .choose(&formats)
        .expect("offered");
        assert_eq!(over_png.pixel_format, PixelFormat::NV12);

        // The inverse, so the assertions above are not measuring a chooser that ignores
        // its arguments: with the format unnamed, the same device answers differently.
        let unnamed = StreamRequest {
            pixel_format: None,
            ..named
        }
        .choose(&formats)
        .expect("resolves");
        assert_eq!(unnamed.pixel_format, PixelFormat::MJPG);
        assert_ne!(unnamed.reason, ChoiceReason::Requested);
    }

    #[test]
    fn a_fourcc_this_build_has_never_heard_of_ranks_last_and_is_still_answerable() {
        // AGENTS rule 6, at the ranking: an unknown format is carried, ranked and never
        // panicked over. It ranks *below* every format this build can name, ahead of
        // resolution, and this fixture is why the key is ordered that way — `H264` at 4K
        // beside `MJPG` at 1080p is the shape a device with a hardware encoder has, and
        // preferring it would trade a photograph for a `FormatUnsupported`.
        let h264 = PixelFormat::parse("H264").expect("four characters");
        let with_known = vec![
            format(h264, vec![size(3840, 2160)]),
            format(PixelFormat::MJPG, vec![size(1920, 1080)]),
        ];
        let chosen = StreamRequest::default()
            .choose(&with_known)
            .expect("resolves");
        assert_eq!(
            (chosen.pixel_format, chosen.width, chosen.height),
            (PixelFormat::MJPG, 1920, 1080),
            "a format this build cannot decode is not the better photograph at any size"
        );

        // But it is not dropped: a device that offers nothing else still resolves, at that
        // format's largest size, rather than answering `None` and losing the camera.
        let alone = vec![format(h264, vec![size(1280, 720), size(3840, 2160)])];
        let only = StreamRequest::default().choose(&alone).expect("resolves");
        assert_eq!(
            (only.pixel_format, only.width, only.height),
            (h264, 3840, 2160)
        );

        // And the classification carries the FourCC rather than a placeholder.
        assert_eq!(Lossiness::of(h264), Lossiness::Unknown { fourcc: h264 });
        assert!(!Lossiness::of(h264).is_named());
        for &sink in SinkFidelity::ALL {
            assert_eq!(
                Lossiness::of(h264).rank_for(sink),
                (0, 0),
                "an unrecognised encoding cannot be claimed to be good for {sink:?}"
            );
        }
    }

    #[test]
    fn a_device_with_one_format_answers_with_it() {
        // The Chicony IR sensor: GREY 640×360 and nothing else. A ranking is only
        // interesting where there is something to rank, and the honest answer for a camera
        // with one format is that format — reported as ranked rather than as requested,
        // because nobody asked for it.
        let grey = vec![format(PixelFormat::GREY, vec![size(640, 360)])];
        let chosen = StreamRequest::default().choose(&grey).expect("resolves");
        assert_eq!(
            (chosen.pixel_format, chosen.width, chosen.height),
            (PixelFormat::GREY, 640, 360)
        );
        assert_eq!(chosen.reason, ChoiceReason::MostPixels);
        assert_eq!(Lossiness::of(PixelFormat::GREY), Lossiness::Lossless);
    }

    #[test]
    fn two_formats_the_ranking_cannot_separate_fall_back_to_the_drivers_own_order() {
        // D5's original rule, in the place it was demoted to. Two entries of the same
        // format at the same size cannot be told apart by anything the ranking measures,
        // and the answer must still be the same one on every run — Expected usage item 2
        // is about two photographs an hour apart differing only where the device does.
        let twins = vec![
            format(PixelFormat::MJPG, vec![size(1920, 1080)]),
            format(PixelFormat::MJPG, vec![size(1920, 1080)]),
        ];
        let chosen = StreamRequest::default().choose(&twins).expect("resolves");
        assert_eq!(
            chosen.reason,
            ChoiceReason::FirstOfEqualsInDriverOrder { index: 0 },
            "the driver enumerated them and it is the only thing left to ask"
        );

        // The same claim one level down, over sizes: two entries of equal area resolve to
        // the earlier one rather than to whichever `max_by_key` saw last.
        let square = vec![format(
            PixelFormat::MJPG,
            vec![size(1440, 1080), size(1080, 1440)],
        )];
        let chosen = StreamRequest::default().choose(&square).expect("resolves");
        assert_eq!((chosen.width, chosen.height), (1440, 1080));
    }

    #[test]
    fn a_format_whose_sizes_this_build_cannot_read_loses_to_one_it_can() {
        // D2's `FrameSize::Unknown` reaching the ranking. The entry is real and carried, so
        // the format is a candidate; but nothing can be asked of a size whose shape we
        // cannot interpret, so a format we *can* size beats it however it is spelled.
        let unreadable = FormatInfo {
            pixel_format: PixelFormat::MJPG,
            description: "MJPG".to_owned(),
            flags: 0,
            sizes: vec![crate::camera::FrameSizeInfo {
                size: crate::camera::FrameSize::Unknown { raw: 99 },
                intervals: Vec::new(),
            }],
        };
        let formats = vec![
            unreadable.clone(),
            format(PixelFormat::YUYV, vec![size(320, 240)]),
        ];
        let chosen = StreamRequest::default().choose(&formats).expect("resolves");
        assert_eq!(
            (chosen.pixel_format, chosen.width, chosen.height),
            (PixelFormat::YUYV, 320, 240)
        );

        // With nothing else on offer there is no size to ask for, which is the one thing
        // the chooser answers `None` to — and the caller turns that into
        // `FormatUnsupported` with the list it does have.
        assert!(StreamRequest::default().choose(&[unreadable]).is_none());
    }

    #[test]
    fn the_fidelity_scale_orders_the_formats_this_project_has_met() {
        // The scale itself, both destinations, because `rank_for` is where the ruling's
        // second clause actually lives and a table nothing reads is a table that drifts.
        let of = Lossiness::of;
        assert_eq!(of(PixelFormat::MJPG), Lossiness::Compressed);
        assert_eq!(of(PixelFormat::JPEG), Lossiness::Compressed);
        assert_eq!(
            of(PixelFormat::YUYV),
            Lossiness::ChromaSubsampled { chroma_percent: 50 }
        );
        assert_eq!(
            of(PixelFormat::NV12),
            Lossiness::ChromaSubsampled { chroma_percent: 25 }
        );

        let through = SinkFidelity::PassesCompressedThrough;
        assert!(of(PixelFormat::MJPG).rank_for(through) > of(PixelFormat::GREY).rank_for(through));
        assert!(of(PixelFormat::GREY).rank_for(through) > of(PixelFormat::YUYV).rank_for(through));
        assert!(of(PixelFormat::YUYV).rank_for(through) > of(PixelFormat::NV12).rank_for(through));

        let lossless = SinkFidelity::EncodesLosslessly;
        assert!(
            of(PixelFormat::GREY).rank_for(lossless) > of(PixelFormat::YUYV).rank_for(lossless)
        );
        assert!(
            of(PixelFormat::YUYV).rank_for(lossless) > of(PixelFormat::NV12).rank_for(lossless)
        );
        assert!(
            of(PixelFormat::NV12).rank_for(lossless) > of(PixelFormat::MJPG).rank_for(lossless)
        );

        // The inversion, stated as the one difference between the two columns.
        assert!(of(PixelFormat::MJPG).rank_for(through) > of(PixelFormat::YUYV).rank_for(through));
        assert!(
            of(PixelFormat::MJPG).rank_for(lossless) < of(PixelFormat::YUYV).rank_for(lossless)
        );

        // And every photo encoding this build writes maps onto one of the two.
        assert_eq!(SinkFidelity::of(PhotoFormat::Jpeg), through);
        assert_eq!(SinkFidelity::of(PhotoFormat::Png), lossless);
        assert_eq!(SinkFidelity::of(PhotoFormat::Ppm), lossless);
        assert_eq!(SinkFidelity::ALL.len(), 2);
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
    fn a_server_path_sink_has_to_be_absolute_and_a_bytes_sink_cannot_be_wrong() {
        // D10's rule, as a predicate rather than a paragraph. Both directions, because a check
        // that only ever sees absolute paths is a check that cannot discriminate — and
        // `webcam-handler-cli` only ever produces absolute ones, so this arm is the only place
        // the relative case exists before P4c routes a socket into it.
        assert!(
            Sink::ServerPath {
                path: "/tmp/out.jpg".into()
            }
            .is_addressable()
        );
        for relative in ["out.jpg", "./out.jpg", "../out.jpg", "sub/dir/out.jpg", ""] {
            assert!(
                !Sink::ServerPath {
                    path: relative.into()
                }
                .is_addressable(),
                "{relative:?} would be resolved against the daemon's cwd, which is /"
            );
        }
        // `ReturnBytes` carries no destination, so there is nothing for it to get wrong.
        // Asserted rather than assumed: a predicate that answered `false` here would
        // refuse every `webcam-handler-client photo` that asked for its bytes back.
        assert!(
            Sink::ReturnBytes {
                format: PhotoFormat::Jpeg
            }
            .is_addressable()
        );
    }

    #[test]
    fn a_sinks_format_comes_from_its_extension_and_an_unwritable_one_is_refused() {
        // Moved here from `engine::photo::sink_format`, whose comment used to answer the
        // unknown-extension case with "the CLI refuses it while building the sink" — true
        // until a socket could build one, and the whole of debt D-1. The three accepted
        // arms are the ones that were already asserted; the fourth is the one that used to
        // fall through to `unwrap_or(PhotoFormat::Jpeg)` and write JPEG bytes into a file
        // named `.webp`.
        for (path, expected) in [
            ("/tmp/a.jpg", PhotoFormat::Jpeg),
            ("/tmp/a.jpeg", PhotoFormat::Jpeg),
            ("/tmp/a.JPG", PhotoFormat::Jpeg),
            ("/tmp/a.png", PhotoFormat::Png),
            ("/tmp/a.ppm", PhotoFormat::Ppm),
            ("/tmp/a.pgm", PhotoFormat::Ppm),
            // No extension: the caller named this file and we do not get to rename it.
            ("/tmp/photo", PhotoFormat::Jpeg),
        ] {
            assert_eq!(
                Sink::ServerPath { path: path.into() }
                    .writable_format()
                    .unwrap_or_else(|err| panic!("{path}: {err}")),
                expected,
                "{path}"
            );
        }

        // The refusal, and what it has to say. Not `FormatUnsupported`: the camera offered
        // nothing and was not asked (E3) — the request named an encoding this build does
        // not write.
        let error = Sink::ServerPath {
            path: "/tmp/x.webp".into(),
        }
        .writable_format()
        .expect_err("webp is not one of the three");
        assert_eq!(error.kind(), crate::ErrorKind::IllegalTransition);
        assert_ne!(error.kind(), crate::ErrorKind::FormatUnsupported);
        let rendered = error.to_string();
        assert!(rendered.contains("webp"), "the extension typed: {rendered}");
        assert!(rendered.contains("/tmp/x.webp"), "the path: {rendered}");
        for format in PhotoFormat::ALL {
            assert!(
                rendered.contains(format.extension()),
                "the formats it does write: {rendered}"
            );
        }

        // A `ReturnBytes` sink names its encoding outright, so there is nothing to guess
        // and nothing to refuse — every format is one this build writes.
        for &format in PhotoFormat::ALL {
            assert_eq!(
                Sink::ReturnBytes { format }
                    .writable_format()
                    .expect("a format this build names is a format this build writes"),
                format
            );
        }
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
