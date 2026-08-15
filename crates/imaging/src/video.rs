//! One entry point for both containers, and the vocabulary they share (design D7).
//!
//! [`Recorder`] is what the engine holds for the length of a take: `begin`, `write_frame` per
//! delivered frame, `finish`. Everything below it is two independent muxers —
//! [`crate::avi::write::AviWriter`] for MJPEG remuxed verbatim, [`crate::y4m::Y4mWriter`] for
//! raw planes — and *nothing* is shared between their byte accounting. That is deliberate and
//! it is the expensive choice: the two count different things (an AVI carries a chunk header,
//! a pad byte and a 16-byte index entry per frame; a Y4M carries six bytes of `FRAME\n` and
//! nothing at close), and a shared accounting layer would have to be told about both, which
//! is how one of them quietly stops enforcing a cap. What pays for the duplication is
//! `both_containers_report_every_cap_in_the_vocabulary`, which walks
//! [`schema::video::CapReached::ALL`] against [`schema::video::VideoFormat::ALL`] and drives
//! each container to each bound — so a cap that stops being enforced in one of them goes red
//! rather than being caught by review.
//!
//! ## An exhaustive match, not a trait object
//!
//! [`Recorder`] is an enum with one arm per container and every method matches on it, so a
//! third container is a **compile error here** rather than a `Box<dyn VideoSink>` that a new
//! implementation silently joins. That is AGENTS rule 6's shape moved from a device's
//! vocabulary to ours: the set of containers is closed, this build knows all of them, and the
//! thing that must not be possible is adding one without answering
//! [`schema::video::VideoFormat::carries`]'s question for it.
//!
//! The `Seek` bound is on the enum and **Y4M does not use it**: a Y4M file is written forward
//! and never revised, while `AviWriter::finish` seeks back to rewrite six header fields. The
//! bound sits on the enum anyway because the enum is one type and a caller holding a
//! [`Recorder`] does not yet know which arm it will be — a per-arm bound would have to be
//! expressed as two types, which is exactly the trait object this module is refusing. The
//! cost is real and small: an engine writing Y4M to a sink it cannot seek has to say so, and
//! the sink it actually has is a file.
//!
//! ## A recording is bounded by values, not by constants
//!
//! [`RecordingCaps`] has **no `Default`**, deliberately. docs/7 puts "duration/size caps from
//! `limits`" in P6b and `webcam-handler-schema::limits`' own header says a constant nobody
//! reads is a defect (rubric A8), so this crate takes the bounds as values and the caller owns
//! which constants they came from. The consequence is the one worth having: a caller cannot
//! forget to bound a recording, because there is no bound to forget — the type will not
//! construct without all three.
//!
//! The one relation this crate *does* assert about `limits` is the ceiling below, because it
//! is the one neither crate can check alone: `schema` cannot see the AVI container's 32-bit
//! index, and the container cannot see the policy cap.
//!
//! ## A cap is an outcome; a malformed frame is an error
//!
//! `write_frame` answers a cap with `Ok(`[`FrameOutcome::Refused`]`)` and a bad frame with
//! `Err`. The split is AGENTS rule 7 in another costume — "availability is not capability" —
//! because a recording that stopped at its size cap did exactly what it was told to, while a
//! frame whose geometry disagrees with the stream's is a defect somewhere upstream.
//! Collapsing the two would make an agent unable to tell "your recording is as long as you
//! allowed" from "your camera is misbehaving", and the primary consumer has no hands to
//! investigate with.
//!
//! Both containers obey that split and both obey the same ordering — `Frames`, `Span`, `Size`
//! — and once any cap is reached the recording is over: a later, smaller frame is refused
//! too, because a recording with a hole in the middle answers the "how long did this take"
//! question wrongly.
//!
//! ## One rate law, since P6d, because both containers can now honour it
//!
//! `declared_interval` is D7's CFR carve-out — *"the header's rate is the negotiated frame
//! interval, rewritten at close to the measured mean interval"* — as one function both muxers
//! call. It did not live here before P6d and could not have: a Y4M header was variable-width
//! text, so that container declined the rewrite and note **N106** recorded the asymmetry. The
//! oracle rung measured what N106 said would retire it (a zero-padded `F` ratio is read
//! correctly by both `ffprobe` and `mpv`), the Y4M header's rate field became fixed-width, and
//! the two containers stopped differing on the one question they had ever differed on. A law
//! two callers obey belongs in one place (AGENTS' "one home per law"), and the asymmetry that
//! justified two copies of it is gone.

use std::io::{Seek, Write};
use std::time::Duration;

use schema::camera::PixelFormat;
use schema::capture::Frame;
use schema::error::Result;
use schema::limits;
use schema::video::{CapReached, IntervalSource, RecordingSummary, VideoFormat};

use crate::avi::AviParams;
use crate::avi::write::{AviWriter, MAX_RECORDING_BYTES as AVI_MAX_RECORDING_BYTES};
use crate::y4m::{Y4mParams, Y4mWriter};

// The relation `schema::limits::MAX_RECORDING_BYTES`' own doc argues and cannot check: the
// policy cap must sit at or below the *format's* ceiling, which is `u32::MAX` because an
// `idx1` entry's `dwChunkOffset` is 32 bits. Checked here because this is the one module that
// can see both numbers — `webcam-handler-schema` does not depend on this crate, and
// `crate::avi::write` has no business knowing what policy the daemon ships. A compile failure
// is the one red nothing can skip, which is what makes this an assertion rather than a
// paragraph (`schema::limits`' own `CAMERA_IDLE_SWEEP_MS` tradition).
//
// It is also `MAX_RECORDING_BYTES`' only reader today, and that is the point: a policy cap
// above the container's limit would be discovered as a wrapped index at frame 200 000.
const _: () = assert!(limits::MAX_RECORDING_BYTES <= AVI_MAX_RECORDING_BYTES);

/// The frame interval a recording declares when nothing else named one.
///
/// 33 333 µs — 30 fps, the mode every webcam in this project offers and the one a caller who
/// expressed no preference will almost certainly get. It is a *placeholder*, and
/// [`schema::video::IntervalSource::Provisional`] is how the summary refuses to pretend
/// otherwise; in AVI the close-time rewrite replaces it the moment two frames have been
/// delivered, and in Y4M it is what the header says for the whole take.
///
/// Not zero, which would be the honest "we do not know": a zero frame interval means infinite
/// frame rate to every player, it would force AVI's `strh.dwScale` to zero as well to keep the
/// two homes agreeing, and it is not expressible at all as a Y4M `F` ratio. A meaningless
/// number that parses everywhere is worse than a plausible one that is labelled.
///
/// It lived in `crate::avi::write` at P6a, when AVI was the only container; it is here since
/// P6b because both containers declare it and a second copy of a placeholder is a second
/// placeholder. `crate::avi::write` re-exports it, so P6a's spelling still resolves.
pub const PROVISIONAL_INTERVAL_US: u32 = 33_333;

/// The interval a finished file declares, where that number came from, and the span behind it.
///
/// **D7's CFR carve-out, once**, called by both muxers at close. Three answers rather than one
/// because they are three different claims about the same header field, and
/// [`schema::video::IntervalSource`] exists so a caller never has to guess which it is holding:
///
/// - a **mean** was measured across the delivered frame timestamps
///   ([`IntervalSource::Measured`]);
/// - nothing could be measured and the negotiation had named a number
///   ([`IntervalSource::Negotiated`]);
/// - neither, so the header carries [`PROVISIONAL_INTERVAL_US`] and says so
///   ([`IntervalSource::Provisional`]).
///
/// Three things make a take measure nothing, and each of them returns `None` for the span
/// rather than zero, because reporting zero would claim a measurement nobody made: **fewer
/// than two frames** span no interval at all, a **driver clock that ran backwards** spans
/// nothing that is a duration, and a **mean that truncates to zero** — more frames than
/// microseconds — has stopped being an interval. The last one matters to a file rather than to
/// arithmetic: a zero rate is meaningless to every player, and AVI would have to zero
/// `strh.dwScale` to keep its two homes agreeing.
///
/// [`schema::video::RecordingSummary::measured_interval_us`] answers the neighbouring question
/// — the mean a *caller* computes out of the two observed numbers — and applies the same three
/// refusals; its own doc says so. This one additionally decides what the **header** says, which
/// is why it also carries the negotiated fallback that a caller's subtraction has no business
/// knowing about.
pub(crate) fn declared_interval(
    first_timestamp_us: Option<i64>,
    last_timestamp_us: i64,
    frames_written: u32,
    negotiated_interval_us: Option<u32>,
) -> (u32, IntervalSource, Option<u64>) {
    let span_us = match first_timestamp_us {
        Some(first) if frames_written >= 2 => last_timestamp_us
            .checked_sub(first)
            .and_then(|delta| u64::try_from(delta).ok()),
        _ => None,
    };
    let intervals = u64::from(frames_written.saturating_sub(1));
    let measured = span_us
        .filter(|span| *span > 0)
        .and_then(|span| span.checked_div(intervals))
        .filter(|mean| *mean > 0)
        .and_then(|mean| u32::try_from(mean).ok());
    match (measured, negotiated_interval_us) {
        (Some(mean), _) => (mean, IntervalSource::Measured, span_us),
        (None, Some(negotiated)) => (negotiated, IntervalSource::Negotiated, span_us),
        (None, None) => (
            PROVISIONAL_INTERVAL_US,
            IntervalSource::Provisional,
            span_us,
        ),
    }
}

/// What a recording is allowed to cost.
///
/// Three bounds rather than one because they fail differently: `max_bytes` protects the disk,
/// `max_frames` protects the index and the memory it lives in, and `max_span` protects the
/// caller from a camera that is still delivering long after the thing being filmed stopped
/// happening. Whichever binds first ends the recording, and
/// [`RecordingSummary::cap_reached`] names it.
///
/// No `Default` (see the module doc): the values belong to the caller, and P6b is where they
/// meet `webcam-handler-schema::limits`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordingCaps {
    /// The largest the finished file may be, **every trailer included**.
    ///
    /// Counted against the whole file rather than the frame payloads because AVI's `idx1`,
    /// written at close, is 16 bytes per frame, and a cap that stopped at the last frame would
    /// be walked past by the close itself. Y4M writes nothing at close and still measures the
    /// same thing, so the field means one thing in both containers.
    pub max_bytes: u64,
    /// How many frames may be written.
    pub max_frames: u32,
    /// How far the last frame's timestamp may be from the first frame's.
    ///
    /// Measured on the driver's clock, from the first frame **written** — this crate reads no
    /// clock of its own (design §2.10), so wall-clock time is the caller's question.
    pub max_span: Duration,
}

/// What happened to a frame handed to a recorder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameOutcome {
    /// It is on the sink.
    Written,
    /// A cap was reached, so it was **not** written and neither will anything after it.
    Refused(CapReached),
}

/// Everything a recorder needs before the first frame, whichever container it turns out to be.
///
/// The union of what the two containers ask for, which is [`crate::avi::AviParams`] plus the
/// negotiated pixel format — AVI infers its own format (it carries one) while Y4M has to pick
/// a colorspace before the header is written. Carried here rather than in each container's own
/// params because [`Recorder::begin`] is the one call the engine makes, and a caller that had
/// to build a different value per container would be reading the pairing law itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordingParams {
    /// Frame width, as negotiated with the device.
    pub width: u32,
    /// Frame height, as negotiated with the device.
    pub height: u32,
    /// The format the device agreed to deliver — what the frames will arrive in.
    ///
    /// The *negotiated* format and not the requested one (D5: requested is not applied), which
    /// matters here because it is what decides the container.
    pub pixel_format: PixelFormat,
    /// The negotiated frame interval in microseconds, when the negotiation named one.
    ///
    /// An `Option` because the negotiation may not have produced a number at all:
    /// [`crate::avi::interval_micros`] refuses to invent one for a stepwise range or for a
    /// `v4l2_frmivaltypes` value this build cannot read (AGENTS rule 6). A recording still
    /// happens — see [`PROVISIONAL_INTERVAL_US`] — and the summary says the interval was
    /// provisional.
    pub negotiated_interval_us: Option<u32>,
    /// What the recording is allowed to cost.
    pub caps: RecordingCaps,
}

/// A recording in progress, in whichever container the frames belong in.
///
/// See the module doc for why this is an enum rather than a trait object, and why the `Seek`
/// bound is here when only one arm uses it.
pub enum Recorder<W: Write + Seek> {
    /// MJPEG frames remuxed into RIFF/AVI, verbatim (D7 L0, E6).
    Avi(AviWriter<W>),
    /// Raw planes in YUV4MPEG2 — D7's "enormous but exact" fallback.
    Y4m(Y4mWriter<W>),
}

impl<W: Write + Seek> std::fmt::Debug for Recorder<W> {
    /// Hand-written rather than derived, for the bound rather than for the content: both
    /// writers implement `Debug` for **every** `W`, because each one prints counts and never
    /// its sink (rubric A12 — a frame may contain a person). A `#[derive(Debug)]` here would
    /// add a `W: Debug` bound of its own and make `Recorder<File>` unprintable, which is a
    /// narrower impl than either arm already has.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Recorder::Avi(writer) => f.debug_tuple("Avi").field(writer).finish(),
            Recorder::Y4m(writer) => f.debug_tuple("Y4m").field(writer).finish(),
        }
    }
}

impl<W: Write + Seek> Recorder<W> {
    /// Open a recording in `format`, writing to `sink`.
    ///
    /// The pairing is checked first and it is checked in **one** place —
    /// [`schema::video::VideoFormat::resolve`], which is D7's law's only home — so a container
    /// that cannot carry the negotiated stream is refused before a byte of header is written
    /// and before a caller has a half-built file to clean up. The muxer below still checks the
    /// format of every frame it is handed, and that is not the same check: this one is about
    /// the *stream*, that one is about a frame that arrived after the stream was opened.
    ///
    /// # Errors
    ///
    /// [`schema::Error::FormatUnsupported`] when `format` cannot carry
    /// [`RecordingParams::pixel_format`], naming what it could have carried; then whatever the
    /// chosen container refuses at `begin` — a degenerate or oversized frame extent, a
    /// negotiated interval of zero, a `max_bytes` outside what the container can address, or a
    /// sink that refused the header.
    pub fn begin(format: VideoFormat, sink: W, params: RecordingParams) -> Result<Self> {
        // The answer is discarded because `format` is the caller's decision; what is wanted is
        // the refusal, and asking `resolve` for it rather than `carries_format` keeps the
        // refusal's *wording* in one place too.
        let _resolved = VideoFormat::resolve(Some(format), params.pixel_format)?;
        match format {
            VideoFormat::Avi => AviWriter::begin(
                sink,
                AviParams {
                    width: params.width,
                    height: params.height,
                    negotiated_interval_us: params.negotiated_interval_us,
                    caps: params.caps,
                },
            )
            .map(Recorder::Avi),
            VideoFormat::Y4m => Y4mWriter::begin(
                sink,
                Y4mParams {
                    width: params.width,
                    height: params.height,
                    pixel_format: params.pixel_format,
                    negotiated_interval_us: params.negotiated_interval_us,
                    caps: params.caps,
                },
            )
            .map(Recorder::Y4m),
        }
    }

    /// Which container this recording is in.
    #[must_use]
    pub const fn format(&self) -> VideoFormat {
        match self {
            Recorder::Avi(_) => VideoFormat::Avi,
            Recorder::Y4m(_) => VideoFormat::Y4m,
        }
    }

    /// Append one frame.
    ///
    /// # Errors
    ///
    /// [`schema::Error::DeviceIo`] naming the container's own step for a frame the open sink
    /// cannot carry — a pixel format that is not the stream's, a geometry that is not the
    /// stream's, a payload whose length disagrees with that geometry — or for a sink that
    /// refused a write. A cap is **not** an error: it is [`FrameOutcome::Refused`].
    pub fn write_frame(&mut self, frame: &Frame) -> Result<FrameOutcome> {
        match self {
            Recorder::Avi(writer) => writer.write_frame(frame),
            Recorder::Y4m(writer) => writer.write_frame(frame),
        }
    }

    /// Close the recording and report what it turned out to be.
    ///
    /// # Errors
    ///
    /// [`schema::Error::DeviceIo`] when the sink refuses a write, a seek or a flush, or when
    /// it had already failed — in which case the message names the length of the prefix that
    /// is still readable.
    pub fn finish(self) -> Result<RecordingSummary> {
        match self {
            Recorder::Avi(writer) => writer.finish(),
            Recorder::Y4m(writer) => writer.finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use schema::video::IntervalSource;

    use super::*;
    use crate::decode::SourceFormat;
    use crate::encode;
    use crate::fixtures;

    const WIDTH: u32 = 16;
    const HEIGHT: u32 = 8;
    /// 15 fps, and deliberately **not** [`PROVISIONAL_INTERVAL_US`]: a container that dropped
    /// the negotiated interval and wrote the placeholder instead would satisfy every
    /// assertion in a suite that negotiated 33 333 everywhere.
    const NEGOTIATED_US: u32 = 66_666;

    /// A frame in `format`, carrying whatever bytes that format's geometry demands.
    fn frame(format: PixelFormat, sequence: u32, timestamp_us: i64) -> Frame {
        let source = fixtures::gradient(WIDTH, HEIGHT);
        let bytes = match format {
            PixelFormat::YUYV => fixtures::pack_yuyv(&source),
            PixelFormat::NV12 => fixtures::pack_nv12(&source),
            PixelFormat::GREY => fixtures::pack_grey(&source),
            _ => encode::jpeg(&crate::decode::Decoded::Gray(source), 40).expect("encodes"),
        };
        Frame {
            bytes,
            pixel_format: format,
            width: WIDTH,
            height: HEIGHT,
            bytes_per_line: 0,
            sequence,
            timestamp_us,
        }
    }

    /// The format each container is exercised with, which is the first one it carries.
    fn representative(container: VideoFormat) -> PixelFormat {
        *container
            .carries()
            .first()
            .expect("a container carries at least one format")
    }

    fn params(container: VideoFormat, caps: RecordingCaps) -> RecordingParams {
        RecordingParams {
            width: WIDTH,
            height: HEIGHT,
            pixel_format: representative(container),
            negotiated_interval_us: Some(NEGOTIATED_US),
            caps,
        }
    }

    /// Bounds no test trips by accident, so a cap assertion is always deliberate.
    fn generous_caps() -> RecordingCaps {
        RecordingCaps {
            max_bytes: 1 << 20,
            max_frames: 1024,
            max_span: Duration::from_secs(60),
        }
    }

    /// Run a whole recording over an in-memory sink.
    fn record(
        container: VideoFormat,
        caps: RecordingCaps,
        frames: &[Frame],
    ) -> (Vec<u8>, Vec<FrameOutcome>, RecordingSummary) {
        let mut file = Vec::new();
        let mut recorder =
            Recorder::begin(container, Cursor::new(&mut file), params(container, caps))
                .expect("begin");
        assert_eq!(recorder.format(), container);
        let outcomes = frames
            .iter()
            .map(|frame| recorder.write_frame(frame).expect("write_frame"))
            .collect();
        let summary = recorder.finish().expect("finish");
        (file, outcomes, summary)
    }

    #[test]
    fn every_d6_source_format_is_carried_by_exactly_one_container() {
        // The totality claim `schema::video`'s module doc makes and cannot check: D6's set
        // lives in `imaging::decode` and the pairing lives in `webcam-handler-schema`, so this
        // crate is the only place both are visible. Without it the pairing could quietly stop
        // covering a format — a new D6 source format would be recordable by nothing, and the
        // only symptom would be a `FormatUnsupported` on a camera that works.
        for &source in SourceFormat::ALL {
            let format = source.pixel_format();
            let carriers: Vec<VideoFormat> = VideoFormat::ALL
                .iter()
                .copied()
                .filter(|container| container.carries_format(format))
                .collect();
            assert_eq!(
                carriers.len(),
                1,
                "{format} is carried by {carriers:?}; D7's pairing is total and disjoint"
            );
            assert_eq!(
                VideoFormat::for_pixel_format(format),
                carriers.first().copied()
            );
            assert!(VideoFormat::resolve(None, format).is_ok(), "{format}");
        }

        // Both directions: nothing outside D6's set is recordable either, or the pairing
        // would be promising a container for a frame this build cannot even read.
        for &format in &VideoFormat::recordable_pixel_formats() {
            assert!(
                SourceFormat::from_pixel_format(format).is_some(),
                "{format} is recordable and not decodable, so the two vocabularies disagree"
            );
        }
        assert_eq!(
            VideoFormat::recordable_pixel_formats().len(),
            SourceFormat::ALL.len(),
            "the recordable set and D6's source set are the same set"
        );
    }

    #[test]
    fn both_containers_report_every_cap_in_the_vocabulary() {
        // What pays for the two containers counting their own bytes rather than sharing an
        // accounting layer (see the module doc). Two dimensions, walked rather than listed: a
        // cap that stops being enforced in one container, or a container that stops enforcing
        // one cap, goes red here and nowhere else.
        //
        // Every arm drives the *same* frame sequence and varies only the cap under test, so a
        // refusal is attributable to the bound that was tightened.
        let mut checked = 0;
        for &container in VideoFormat::ALL {
            let format = representative(container);
            let frames: Vec<Frame> = (0..4)
                .map(|index| frame(format, index, i64::from(index) * i64::from(NEGOTIATED_US)))
                .collect();
            // The size one frame costs this container, measured rather than predicted: a
            // one-frame recording's own byte count is the only number that works for both.
            let (_, _, one) = record(container, generous_caps(), &frames[..1]);
            let (_, _, two) = record(container, generous_caps(), &frames[..2]);
            let per_frame = two.bytes_written - one.bytes_written;

            for &cap in CapReached::ALL {
                let caps = match cap {
                    // Two frames fit; the third is refused.
                    CapReached::Frames => RecordingCaps {
                        max_frames: 2,
                        ..generous_caps()
                    },
                    // The span is measured from the first frame written, so a budget of one
                    // interval admits frames 0 and 1 and refuses frame 2.
                    CapReached::Span => RecordingCaps {
                        max_span: Duration::from_micros(u64::from(NEGOTIATED_US)),
                        ..generous_caps()
                    },
                    // Room for two frames and not the third, derived from what this
                    // container actually charged for one.
                    CapReached::Size => RecordingCaps {
                        max_bytes: two.bytes_written + per_frame - 1,
                        ..generous_caps()
                    },
                };
                let (_, outcomes, summary) = record(container, caps, &frames);
                assert_eq!(
                    summary.cap_reached,
                    Some(cap),
                    "{container} did not report {cap:?}"
                );
                assert_eq!(
                    outcomes[0],
                    FrameOutcome::Written,
                    "{container}/{cap:?} refused a frame that was inside every bound"
                );
                assert_eq!(
                    outcomes[3],
                    FrameOutcome::Refused(cap),
                    "{container}/{cap:?} kept writing after the cap was reached"
                );
                assert_eq!(
                    summary.frames_written, 2,
                    "{container}/{cap:?} wrote the wrong number of frames"
                );
                checked += 1;
            }
        }
        assert_eq!(
            checked,
            VideoFormat::ALL.len() * CapReached::ALL.len(),
            "the walk skipped a container/cap pair"
        );
    }

    #[test]
    fn a_container_that_cannot_carry_the_stream_is_refused_before_a_header_is_written() {
        // The pairing law, enforced at the one call the engine makes. The sink is checked as
        // well as the answer: a refusal that had already written a header would leave the
        // caller a file it never asked for.
        let mut file = Vec::new();
        let refused = Recorder::begin(
            VideoFormat::Y4m,
            Cursor::new(&mut file),
            RecordingParams {
                pixel_format: PixelFormat::MJPG,
                ..params(VideoFormat::Y4m, generous_caps())
            },
        );
        assert!(matches!(
            refused,
            Err(schema::Error::FormatUnsupported { .. })
        ));
        assert!(
            file.is_empty(),
            "a refused recording wrote {} bytes",
            file.len()
        );

        let mut file = Vec::new();
        let refused = Recorder::begin(
            VideoFormat::Avi,
            Cursor::new(&mut file),
            RecordingParams {
                pixel_format: PixelFormat::YUYV,
                ..params(VideoFormat::Avi, generous_caps())
            },
        );
        assert!(matches!(
            refused,
            Err(schema::Error::FormatUnsupported { .. })
        ));
        assert!(file.is_empty());
    }

    #[test]
    fn every_container_declares_the_mean_it_measured_and_says_so() {
        // D7's CFR carve-out over `VideoFormat::ALL`, and **one branch** since P6d — which is
        // the shape note **N106**'s amendment predicted for the day the Y4M header could be
        // rewritten. It was a two-armed `match` for two phases: AVI declared the measured mean
        // and Y4M declared the negotiated interval, because a variable-width text header had
        // nothing to patch. It is written as a walk rather than as two tests so that a third
        // container has to answer the question rather than inherit an answer, and so that a
        // container which quietly stopped rewriting its header goes red here.
        //
        // Every take delivers two frames 50 000 µs apart against a negotiated 66 666, so each
        // one *has* a mean and each one's declared interval is distinguishable from what it was
        // asked for. `NEGOTIATED_US` is asserted absent for that reason: a container that
        // ignored its own measurement would declare 66 666 and satisfy every other line here.
        for &container in VideoFormat::ALL {
            let format = representative(container);
            let frames = [frame(format, 0, 0), frame(format, 1, 50_000)];
            let (_, _, summary) = record(container, generous_caps(), &frames);
            assert_eq!(summary.span_us, Some(50_000), "{container}");
            assert_eq!(
                summary.measured_interval_us(),
                Some(50_000),
                "{container} must carry the measurement as well as declare it"
            );
            assert_eq!(
                summary.interval_source,
                IntervalSource::Measured,
                "{container} measured a mean and did not say so"
            );
            assert_eq!(summary.declared_interval_us, 50_000, "{container}");
            assert_ne!(
                summary.declared_interval_us, NEGOTIATED_US,
                "{container} declared what it was asked for rather than what it saw"
            );
        }
    }

    #[test]
    fn a_take_that_measured_nothing_declares_what_it_was_asked_for_in_either_container() {
        // The other half of the same law, and the one that keeps the assertion above from being
        // satisfiable by a container that answers `Measured` unconditionally. A single frame
        // spans no interval, so there is nothing to measure and nothing for either close to
        // patch in — and the label says `Negotiated` rather than dressing 66 666 up as a
        // finding (`schema::video::IntervalSource`'s whole reason for existing).
        for &container in VideoFormat::ALL {
            let format = representative(container);
            let (_, _, summary) = record(container, generous_caps(), &[frame(format, 0, 0)]);
            assert_eq!(summary.span_us, None, "{container}");
            assert_eq!(summary.measured_interval_us(), None, "{container}");
            assert_eq!(
                summary.interval_source,
                IntervalSource::Negotiated,
                "{container} claimed a measurement over one frame"
            );
            assert_eq!(summary.declared_interval_us, NEGOTIATED_US, "{container}");

            // And a negotiation that named nothing is `Provisional` in both, so the placeholder
            // is never mistakable for either of the other two answers.
            let mut file = Vec::new();
            let mut recorder = Recorder::begin(
                container,
                Cursor::new(&mut file),
                RecordingParams {
                    negotiated_interval_us: None,
                    ..params(container, generous_caps())
                },
            )
            .expect("begin");
            recorder
                .write_frame(&frame(format, 0, 0))
                .expect("write_frame");
            let summary = recorder.finish().expect("finish");
            assert_eq!(
                summary.interval_source,
                IntervalSource::Provisional,
                "{container}"
            );
            assert_eq!(
                summary.declared_interval_us, PROVISIONAL_INTERVAL_US,
                "{container}"
            );
        }
    }

    #[test]
    fn a_recording_of_nothing_still_produces_a_file_in_either_container() {
        // `begin` writes a complete header in both containers, which is what makes a crashed
        // recording recoverable (D7's promise) — and what makes a zero-frame take a file
        // rather than an empty one.
        for &container in VideoFormat::ALL {
            let (file, outcomes, summary) = record(container, generous_caps(), &[]);
            assert!(outcomes.is_empty());
            assert_eq!(summary.frames_written, 0, "{container}");
            assert_eq!(summary.span_us, None, "{container}");
            assert_eq!(summary.cap_reached, None, "{container}");
            assert!(!file.is_empty(), "{container} wrote no header");
            assert_eq!(
                summary.bytes_written,
                u64::try_from(file.len()).expect("fits"),
                "{container}'s summary disagrees with the file's own length"
            );
        }
    }
}
