//! Recording vocabulary: which container carries which frames, and what a finished take
//! turned out to be (design D7).
//!
//! ## Why this is here and not in `webcam-handler-imaging`
//!
//! [`RecordingSummary`] is the answer to `record`, so it crosses the JSON-RPC wire, lands in
//! a `--json` document and is validated against `schemas/` — and `webcam-handler-schema` is
//! where "every type that crosses a boundary" lives, because one definition makes a rename a
//! compile error in all four consumers at once. The types were born in
//! `imaging::avi` at P6a, when AVI was the only container and nothing outside that crate
//! could see them; P6b adds a second container and a wire surface, and a vocabulary with two
//! homes is the defect AGENTS' "one home per law" names. `imaging::avi` re-exports all three
//! so P6a's spellings still resolve.
//!
//! ## The container pairing is a law, and it lives here because two crates read it
//!
//! D7: *"Raw fallback: Y4M (y4m crate) for YUYV/GREY cameras and pipeline use (mono and
//! 4:2:x are both in the Y4M vocabulary) — enormous but exact. The muxer is MJPG-only by
//! design; non-MJPG `record` requests get Y4M or `FormatUnsupported { available }`."*
//!
//! [`VideoFormat::carries`] is that sentence as an exhaustive match: **AVI carries MJPG and
//! JPEG only, Y4M carries YUYV, NV12 and GREY only**, and the two sets are disjoint. The
//! engine reads it to pick a muxer, the CLI reads it to check a `--format` flag against a
//! negotiated stream, and neither may grow a copy — a second copy would let
//! `webcam-handler-cli record` and `webcam-handler-daemon` disagree about what a `.y4m` sink
//! means, which is the class design §2.10 exists to prevent.
//!
//! The pairing is **total over D6's closed source-format vocabulary** and it refuses rather
//! than guesses (AGENTS rule 6): a format neither container carries is
//! [`Error::FormatUnsupported`] naming what *is* recordable, never a silent fallback to the
//! other container. `imaging::video`'s
//! `every_d6_source_format_is_carried_by_exactly_one_container` is what holds the totality,
//! because only that crate can see both vocabularies at once —
//! `imaging::decode::SourceFormat` is D6's set and this crate knows nothing about which
//! formats can be decoded.
//!
//! ## One rate, and a summary that says where the number came from
//!
//! [`RecordingSummary::declared_interval_us`] is the frame interval the finished file
//! declares, and [`RecordingSummary::interval_source`] says whether it was **measured**,
//! merely **negotiated**, or a **placeholder**. That distinction is the payload rather than
//! the metadata: the notes' Expected usage item 10 is blunt about it — *"for a transition
//! they are the measurement, because 'did this take 200 ms or 2 s' is the question being
//! asked"* — so a caller must be able to tell a rate that was observed from one the camera
//! was asked for.
//!
//! **The two containers answer it differently, and the difference is real.** AVI's header
//! rate lives in fixed-width binary fields, so D7's CFR carve-out rewrites it at close to the
//! measured mean and the summary says [`IntervalSource::Measured`]. Y4M's header is
//! variable-width text written *before* the first frame, so the same rewrite would need a
//! fixed-width padded `F` ratio whose legality across parsers this project has not measured —
//! a Y4M take therefore declares the negotiated (or provisional) interval and its
//! `interval_source` is **never** `Measured`. Note **N106** records the asymmetry, what makes
//! the parser claim `measured` rather than `declared`, and why the summary is still enough:
//! [`RecordingSummary::span_us`] and [`RecordingSummary::frames_written`] carry the
//! measurement whichever container was used, so the mean is the caller's subtraction rather
//! than a number it has to take on trust.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::camera::PixelFormat;
use crate::error::{Error, Result};
use crate::vocabulary::closed_vocabulary;

closed_vocabulary! {
    /// Which bound ended the recording.
    ///
    /// `ALL` is generated from this definition, and the tests walk it: a cap that no
    /// recording can reach is a bound that is not a bound, and a hand-written list of the
    /// three would let one quietly stop being enforced (rubric rule 6). Since P6b the walk
    /// is two-dimensional — `CapReached::ALL` × [`VideoFormat::ALL`] — because the two
    /// containers count their own bytes and a cap enforced in one of them is not a cap.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
    #[serde(rename_all = "lowercase")]
    pub enum CapReached {
        /// The byte cap: `RecordingCaps::max_bytes` in `webcam-handler-imaging`.
        Size,
        /// The frame-count cap: `RecordingCaps::max_frames`.
        Frames,
        /// The wall-of-the-driver's-clock cap: `RecordingCaps::max_span`.
        Span,
    }
}

closed_vocabulary! {
    /// Where the frame interval in the finished file came from.
    ///
    /// The distinction D7's CFR carve-out exists to preserve, carried out of the muxer so
    /// the caller never has to guess whether the number it is reading was observed.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
    #[serde(rename_all = "lowercase")]
    pub enum IntervalSource {
        /// The mean of the delivered frame timestamps — the close-time rewrite happened.
        ///
        /// **AVI only.** A Y4M header is written before the first frame and cannot be
        /// rewritten in place, so a Y4M take never reports this; see the module doc and
        /// note **N106**.
        Measured,
        /// What the camera was asked for. For AVI: fewer than two frames arrived, or their
        /// timestamps described no usable span, so nothing was measured. For Y4M: the
        /// negotiation named an interval and the header declares it, which is as far as
        /// that container goes.
        Negotiated,
        /// Neither: the negotiation named no interval and none was measured, so the header
        /// carries `imaging::video::PROVISIONAL_INTERVAL_US`.
        Provisional,
    }
}

/// What a finished recording turned out to be.
///
/// Every field is a *measurement* of the file that was written, not a restatement of what
/// was asked for — which is the point of returning it at all. `declared_interval_us` is the
/// number now in the file's own rate field(s), and `interval_source` says whether it is a
/// finding or a placeholder.
///
/// Named for the recording rather than for AVI since P6b: the fields are the same fields
/// P6a's `AviSummary` had, and every one of them is a claim a Y4M take can make too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RecordingSummary {
    /// How many frames the file holds.
    pub frames_written: u32,
    /// The size of the finished file, every trailer the container writes at close included.
    pub bytes_written: u64,
    /// The frame interval the finished file declares.
    ///
    /// For AVI that is `avih.dwMicroSecPerFrame` and `strh.dwScale`/`dwRate`, which
    /// `imaging::avi::write::AviWriter::finish` rewrites together so a reader finding them
    /// disagreeing has caught a defect. For Y4M it is the header's `F` ratio, written before
    /// the first frame and never revised.
    pub declared_interval_us: u32,
    /// Whether that interval was measured, negotiated or provisional.
    ///
    /// **A Y4M take never answers [`IntervalSource::Measured`]**, and that is a property of
    /// the container rather than of the recording: see the module doc.
    pub interval_source: IntervalSource,
    /// Frames the driver's `sequence` numbers say never arrived.
    ///
    /// A `u64` because the gaps accumulate: each one may be as large as a `u32`, and a
    /// recording may hold as many gaps as it holds frames. Reporting it is what stops a
    /// dropped frame from reading as a slow transition (Expected usage item 10).
    pub dropped_frames: u64,
    /// The last written frame's timestamp minus the first's, when that is a duration.
    ///
    /// `None` when fewer than two frames were written, and when the driver's clock ran
    /// backwards across the take — a negative span is not a duration, and reporting it as
    /// zero would claim a measurement nobody made.
    ///
    /// Carried by **both** containers, which is what keeps the Y4M header's un-rewritable
    /// rate from costing the caller the measurement: `span_us / (frames_written - 1)` is the
    /// mean, computed by whoever wants it out of two numbers that were observed.
    pub span_us: Option<u64>,
    /// The bound that ended the recording, if one did.
    pub cap_reached: Option<CapReached>,
}

impl RecordingSummary {
    /// The mean delivered frame interval in microseconds, when the take measured one.
    ///
    /// The subtraction the module doc promises, done once here rather than in each consumer
    /// — a CLI line, a `--json` consumer and a test would otherwise each write
    /// `span / (frames - 1)` and one of them would write `span / frames`.
    ///
    /// `None` for a take that spans nothing: fewer than two frames measure no interval, a
    /// clock that ran backwards leaves no span, and a mean that truncates to zero is not an
    /// interval any more — the same three refusals `AviWriter::finish` applies before it
    /// declares [`IntervalSource::Measured`], stated once so the two cannot disagree.
    ///
    /// It is deliberately **not** the same question as [`Self::declared_interval_us`]: for a
    /// Y4M take the two differ by design, and a caller comparing them is reading exactly the
    /// asymmetry the module doc describes.
    #[must_use]
    pub fn measured_interval_us(&self) -> Option<u64> {
        let intervals = u64::from(self.frames_written.checked_sub(1)?);
        self.span_us
            .filter(|span| *span > 0)
            .and_then(|span| span.checked_div(intervals))
            .filter(|mean| *mean > 0)
    }
}

closed_vocabulary! {
    /// The container a recording lands in (design D7).
    ///
    /// Two, and they are not alternatives a caller picks on taste: each one carries a
    /// disjoint set of pixel formats, so the container is decided by what the device
    /// negotiated. See [`VideoFormat::carries`] for the law and [`VideoFormat::resolve`] for
    /// the decision every caller makes with it.
    ///
    /// The serde spelling, [`VideoFormat::as_str`] and [`VideoFormat::extension`] are the
    /// same three strings for the same reason `PhotoFormat`'s are: a `--format y4m`
    /// argument, a `"format":"y4m"` field and a `.y4m` filename must name one thing.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
    #[serde(rename_all = "lowercase")]
    pub enum VideoFormat {
        /// RIFF/AVI carrying MJPEG frames verbatim — D7's L0 container, and the product
        /// (E6): the camera's own bitstream reaches the file untouched.
        Avi,
        /// YUV4MPEG2 carrying raw planes — D7's raw fallback, "enormous but exact".
        Y4m,
    }
}

/// What [`VideoFormat::Avi`] carries, named once.
///
/// Both spellings of MJPEG and nothing else. Spelled out rather than derived from
/// [`PixelFormat::is_compressed`], which means "a bitstream we pass through" and would
/// silently start admitting H.264 the day D7's L1 layer joins that set —
/// `imaging::avi::write`'s frame check makes the identical argument one crate along, and the
/// two lists are held equal by `imaging::video`'s pairing test rather than by a shared
/// constant that would make the muxer's own guard tautological.
const AVI_FORMATS: [PixelFormat; 2] = [PixelFormat::MJPG, PixelFormat::JPEG];

/// What [`VideoFormat::Y4m`] carries: D6's raw formats, in the order Y4M's own colorspace
/// vocabulary names them — 4:2:2, 4:2:0, mono.
///
/// The set is exactly D6's source formats minus the compressed pair, which is what makes the
/// pairing total; `imaging::video` is where that identity is asserted, because this crate
/// cannot see D6's set.
const Y4M_FORMATS: [PixelFormat; 3] = [PixelFormat::YUYV, PixelFormat::NV12, PixelFormat::GREY];

impl VideoFormat {
    /// Every pixel format this container carries, and the whole of what it carries.
    ///
    /// **The one home for D7's pairing** (design §2.10). An exhaustive match, so a third
    /// container cannot be added without answering this question for it — which is AGENTS
    /// rule 6's shape applied to a decision rather than to a device's vocabulary.
    #[must_use]
    pub const fn carries(self) -> &'static [PixelFormat] {
        match self {
            VideoFormat::Avi => &AVI_FORMATS,
            VideoFormat::Y4m => &Y4M_FORMATS,
        }
    }

    /// Whether this container can carry frames in `format`.
    #[must_use]
    pub fn carries_format(self, format: PixelFormat) -> bool {
        self.carries().contains(&format)
    }

    /// The container that carries `format`, or nothing when no container in this build does.
    ///
    /// The sets are disjoint, so "the container" is well defined — and
    /// `imaging::video::tests::every_d6_source_format_is_carried_by_exactly_one_container` is
    /// what holds that, in both directions, over the vocabulary that actually decides it.
    #[must_use]
    pub fn for_pixel_format(format: PixelFormat) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|container| container.carries_format(format))
    }

    /// Every pixel format some container in this build records.
    ///
    /// The `available` list a refusal carries, derived from [`Self::ALL`] and
    /// [`Self::carries`] rather than transcribed: a caller holding an unrecordable frame
    /// needs to know which format to renegotiate to, and a hand-written list is a list that
    /// stops being true the day a container is added (rubric rule 6).
    #[must_use]
    pub fn recordable_pixel_formats() -> Vec<PixelFormat> {
        Self::ALL
            .iter()
            .flat_map(|container| container.carries().iter().copied())
            .collect()
    }

    /// The container this recording goes in, given what the caller named and what the device
    /// agreed to deliver.
    ///
    /// **The decision, once**, so the engine and the CLI cannot each grow their own copy
    /// (AGENTS "one home per law"). A caller that named nothing is answered from the
    /// negotiated format; a caller that named a container is honoured when that container can
    /// carry the stream, and refused when it cannot — never quietly redirected to the other
    /// one, because a caller who typed `out.avi` and received a Y4M has been answered a
    /// question it did not ask.
    ///
    /// # Errors
    ///
    /// [`Error::FormatUnsupported`] naming the negotiated format as `requested` and, as
    /// `available`, the formats that would have worked: everything recordable when no
    /// container was named, and *that container's* own list when one was. The narrower list
    /// is deliberate — a caller who asked for `.avi` with a YUYV stream is helped by "AVI
    /// carries MJPG, JPEG" and misled by a list that includes YUYV.
    ///
    /// It is [`Error::FormatUnsupported`] rather than [`Error::DeviceIo`] because this is a
    /// statement about what this build *can record*, which is the variant's own subject. The
    /// symmetrical-looking refusal one layer down — a frame arriving at an open sink in a
    /// format that sink is not carrying — is `DeviceIo`, because by then the container was
    /// chosen correctly and something upstream changed its mind mid-take. AGENTS rule 7 is
    /// the line between them: a capability claim and a malfunction are not the same answer.
    pub fn resolve(requested: Option<Self>, negotiated: PixelFormat) -> Result<Self> {
        match requested {
            None => Self::for_pixel_format(negotiated).ok_or_else(|| Error::FormatUnsupported {
                requested: Some(negotiated),
                available: Self::recordable_pixel_formats(),
            }),
            Some(container) if container.carries_format(negotiated) => Ok(container),
            Some(container) => Err(Error::FormatUnsupported {
                requested: Some(negotiated),
                available: container.carries().to_vec(),
            }),
        }
    }

    /// Guess the container a path is asking for by its extension.
    ///
    /// `None` for an extension this build does not write, which is a refusal its caller
    /// turns into a message naming what it does write — the shape
    /// [`crate::capture::PhotoFormat::from_extension`] already has, because a sink path is
    /// read the same way whether a photo or a recording is going into it.
    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        let lowered = ext.to_ascii_lowercase();
        Self::ALL
            .iter()
            .copied()
            .find(|format| format.extension() == lowered)
    }

    /// The extension this container writes.
    ///
    /// The same string as [`Self::as_str`] for both containers, unlike
    /// [`crate::capture::PhotoFormat`] where the format is `jpeg` and the file is `.jpg`.
    /// Kept as two functions anyway: they answer different questions, and a third container
    /// whose two spellings differ would otherwise have nowhere to say so.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            VideoFormat::Avi => "avi",
            VideoFormat::Y4m => "y4m",
        }
    }

    /// The name this container is written by, in JSON and on a command line alike.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            VideoFormat::Avi => "avi",
            VideoFormat::Y4m => "y4m",
        }
    }

    /// Parse one of those names.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|f| f.as_str() == s)
    }
}

impl fmt::Display for VideoFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_container_parses_from_the_name_it_prints_and_serializes_as() {
        // One vocabulary, one spelling — `PhotoFormat`'s test, for the container that has
        // to agree with a `--format` flag, a `"format"` field and a filename at once.
        for &format in VideoFormat::ALL {
            assert_eq!(VideoFormat::parse(format.as_str()), Some(format));
            assert_eq!(
                VideoFormat::from_extension(format.extension()),
                Some(format)
            );
            let json = serde_json::to_string(&format).expect("serialize");
            assert_eq!(json, format!("\"{}\"", format.as_str()), "{format:?}");
        }
        // The extension match is case-insensitive, because a filename is the caller's.
        assert_eq!(VideoFormat::from_extension("AVI"), Some(VideoFormat::Avi));
        assert_eq!(VideoFormat::from_extension("Y4M"), Some(VideoFormat::Y4m));
        // And a name nobody defined parses to nothing rather than to a default: the
        // inverse arm, without which this test cannot discriminate.
        assert_eq!(VideoFormat::parse("mkv"), None);
        assert_eq!(VideoFormat::parse("webm"), None);
        assert_eq!(VideoFormat::from_extension("mp4"), None);
        assert_eq!(VideoFormat::from_extension(""), None);
    }

    #[test]
    fn the_two_containers_carry_disjoint_sets_and_neither_is_empty() {
        // `for_pixel_format` says "the container", which is only well defined while the
        // sets are disjoint — and an empty set would make a container that records nothing
        // look like a container.
        for &container in VideoFormat::ALL {
            assert!(
                !container.carries().is_empty(),
                "{container} carries nothing"
            );
            for &format in container.carries() {
                assert_eq!(
                    VideoFormat::for_pixel_format(format),
                    Some(container),
                    "{format} is carried by more than one container, or by the wrong one"
                );
            }
        }
        let all = VideoFormat::recordable_pixel_formats();
        let mut deduped = all.clone();
        deduped.sort_unstable();
        deduped.dedup();
        assert_eq!(all.len(), deduped.len(), "a format is in two containers");
    }

    #[test]
    fn a_named_container_that_cannot_carry_the_stream_is_refused_rather_than_redirected() {
        // The whole point of the refusal: a caller who typed `out.avi` over a YUYV camera
        // must not silently receive a Y4M, and must be told what AVI would have taken.
        let refusal = VideoFormat::resolve(Some(VideoFormat::Avi), PixelFormat::YUYV)
            .expect_err("AVI does not carry YUYV");
        match refusal {
            Error::FormatUnsupported {
                requested,
                available,
            } => {
                assert_eq!(requested, Some(PixelFormat::YUYV));
                assert_eq!(available, vec![PixelFormat::MJPG, PixelFormat::JPEG]);
                assert!(
                    !available.contains(&PixelFormat::YUYV),
                    "the list must not offer the format that was just refused"
                );
            }
            other => panic!("wrong variant: {other:?}"),
        }

        // And the honoured direction, so the assertion above is not vacuous.
        assert_eq!(
            VideoFormat::resolve(Some(VideoFormat::Avi), PixelFormat::MJPG).expect("MJPG in AVI"),
            VideoFormat::Avi
        );
        assert_eq!(
            VideoFormat::resolve(Some(VideoFormat::Y4m), PixelFormat::GREY).expect("GREY in Y4M"),
            VideoFormat::Y4m
        );
    }

    #[test]
    fn a_request_that_named_no_container_is_answered_from_the_negotiated_format() {
        assert_eq!(
            VideoFormat::resolve(None, PixelFormat::MJPG).expect("resolves"),
            VideoFormat::Avi
        );
        assert_eq!(
            VideoFormat::resolve(None, PixelFormat::NV12).expect("resolves"),
            VideoFormat::Y4m
        );

        // A format outside every container names everything that would have worked, so an
        // unattended caller can renegotiate rather than guess (AGENTS: the primary consumer
        // has no hands).
        let h264 = PixelFormat::parse("H264").expect("four characters");
        let refusal = VideoFormat::resolve(None, h264).expect_err("no container carries H264");
        match refusal {
            Error::FormatUnsupported {
                requested,
                available,
            } => {
                assert_eq!(requested, Some(h264));
                assert_eq!(available, VideoFormat::recordable_pixel_formats());
                assert!(available.contains(&PixelFormat::MJPG), "{available:?}");
                assert!(available.contains(&PixelFormat::GREY), "{available:?}");
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn a_mean_needs_two_frames_a_forward_clock_and_a_span_longer_than_the_take() {
        // `measured_interval_us` restates `AviWriter::finish`'s three refusals, so the two
        // must agree about what "measured nothing" is. Each refusal has its own arm here
        // because they fail for different reasons and a single `None` case cannot tell
        // which one fired.
        let base = RecordingSummary {
            frames_written: 3,
            bytes_written: 1024,
            declared_interval_us: 33_333,
            interval_source: IntervalSource::Measured,
            dropped_frames: 0,
            span_us: Some(100_000),
            cap_reached: None,
        };
        assert_eq!(base.measured_interval_us(), Some(50_000));

        assert_eq!(
            RecordingSummary {
                frames_written: 1,
                ..base
            }
            .measured_interval_us(),
            None,
            "one frame spans no interval"
        );
        assert_eq!(
            RecordingSummary {
                frames_written: 0,
                ..base
            }
            .measured_interval_us(),
            None,
            "no frame at all must not underflow the interval count"
        );
        assert_eq!(
            RecordingSummary {
                span_us: None,
                ..base
            }
            .measured_interval_us(),
            None,
            "a clock that ran backwards left no span to divide"
        );
        assert_eq!(
            RecordingSummary {
                span_us: Some(0),
                ..base
            }
            .measured_interval_us(),
            None,
            "a zero span is not a measurement"
        );
        assert_eq!(
            RecordingSummary {
                frames_written: 1000,
                span_us: Some(10),
                ..base
            }
            .measured_interval_us(),
            None,
            "a mean that truncates to zero is not an interval any more"
        );
    }

    #[test]
    fn a_summary_round_trips_through_json_with_its_vocabulary_spelled_in_lower_case() {
        // The wire shape, asserted rather than assumed: this document is what a `--json`
        // consumer validates against `schemas/`, so the spelling of the two vocabularies
        // inside it is a contract and not a rendering detail.
        let summary = RecordingSummary {
            frames_written: 2,
            bytes_written: 4096,
            declared_interval_us: 33_333,
            interval_source: IntervalSource::Negotiated,
            dropped_frames: 1,
            span_us: Some(66_666),
            cap_reached: Some(CapReached::Span),
        };
        let json = serde_json::to_string(&summary).expect("serialize");
        assert!(
            json.contains("\"interval_source\":\"negotiated\""),
            "{json}"
        );
        assert!(json.contains("\"cap_reached\":\"span\""), "{json}");
        let back: RecordingSummary = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, summary);

        for &cap in CapReached::ALL {
            let rendered = serde_json::to_string(&cap).expect("serialize");
            assert_eq!(
                rendered,
                format!("\"{}\"", format!("{cap:?}").to_lowercase())
            );
        }
        for &source in IntervalSource::ALL {
            let rendered = serde_json::to_string(&source).expect("serialize");
            assert_eq!(
                rendered,
                format!("\"{}\"", format!("{source:?}").to_lowercase())
            );
        }
    }
}
