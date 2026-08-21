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
//! **Both containers answer it the same way since P6d, and they did not before.** AVI's header
//! rate lives in fixed-width binary fields, so D7's CFR carve-out has always rewritten it at
//! close to the measured mean. Y4M's header is variable-width text written *before* the first
//! frame, so the same rewrite needed a fixed-width padded `F` ratio — and whether a padded
//! ratio is read correctly by the parsers that matter was **unmeasured**, so note **N106**
//! recorded the asymmetry and named the measurement that would end it. P6d's oracle rung took
//! it (evidence **E17**): `ffprobe` and `mpv` both read `F1000000:0000050000` as exactly the
//! rate it names. The Y4M denominator is now zero-padded to a fixed width and patched at close,
//! so **either container may answer [`IntervalSource::Measured`]** and the vocabulary means the
//! same thing in both.
//!
//! What has not changed is that the summary is enough on its own:
//! [`RecordingSummary::span_us`] and [`RecordingSummary::frames_written`] carry the measurement
//! whichever container was used, so the mean is the caller's subtraction rather than a number it
//! has to take on trust. What P6d added is the *file*, handed to a player with no summary beside
//! it, playing at the rate it was captured at.
//!
//! ## The request, and the three questions it answers about itself
//!
//! [`RecordRequest`] is what a caller hands `webcam-handler-engine::record`, and its three
//! predicates — [`RecordRequest::server_path`], [`RecordRequest::container`] and
//! [`RecordRequest::budget_ms`] — are the shape [`crate::capture::Sink::writable_format`]
//! already has one verb along, and for its reason: **the rule lives beside the variants it
//! constrains, so a request built by a socket meets it as surely as one built by a command
//! line.** `webcam-handler-daemon` links no `cli-core`, so a rule enforced while parsing a
//! command line is a rule the wire does not have (debt D-1, note **N46**).
//!
//! Each of the three refuses rather than repairs, and each is
//! [`Error::IllegalTransition`] — the variant note **N46** widened to mean *"the request
//! names something this build will not do"*:
//!
//! - **a recording's bytes go to a path, never back in the answer** ([`RecordRequest::
//!   server_path`], note **N110**);
//! - **the path's extension names the container**, and one this build does not write is
//!   refused rather than filled with something else under a name that lies about it
//!   ([`RecordRequest::container`] — the `.webp` defect, one container along);
//! - **a duration past [`crate::limits::MAX_RECORDING_MS`] is refused rather than clamped**
//!   ([`RecordRequest::budget_ms`]), because an agent that asked for too much can ask for
//!   less and an agent whose recording was quietly shortened cannot tell that from a camera
//!   that stopped.
//!
//! ## A take is also a thing you ask about while it runs (P6c)
//!
//! [`RecordReport`] is what a *finished* take turned out to be, and it is the only answer a
//! caller that holds the camera ever needs — `webcam-handler-cli record` blocks and gets one.
//! A caller on the other end of a socket does not hold the camera: D10 puts three methods on
//! the wire and says *"progress by polling `record_status` — no recording subscription in
//! v1"*, so there has to be a document that describes a take **that has not finished**.
//! [`RecordStatus`] is it, and [`TakeStatus`] is what it carries.
//!
//! The two documents are not one type with optional fields, and the split is the container's
//! own: a summary counts bytes that do not exist until the container is closed
//! ([`TakeStatus::frames_written`] says which number is honest mid-take and which is not), and
//! a report names an ending that a running take does not have. What they share is asserted
//! rather than assumed — a finished take's `frames_written` and its
//! [`RecordingSummary::frames_written`] are the same number, counted on two sides.
//!
//! ## The answer carries two clocks, because P6d subtracts one from the other
//!
//! [`RecordReport`] holds [`RecordingSummary::span_us`] — measured on the *driver's* frame
//! timestamps — beside [`RecordReport::wall_clock_ms`], measured on the engine's own
//! monotonic clock. That pair is docs/7 **P6d**'s declared-vs-wall-clock duration bound
//! ("the D7/§3.3 CFR limitation, bounded rather than wished away"), so the two numbers live
//! in one document rather than being recovered from a log; [`RecordReport::wall_clock_ms`]'s
//! own doc says what a difference between them means.

use std::fmt;

use camino::{Utf8Path, Utf8PathBuf};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::camera::{CameraId, PixelFormat};
use crate::capture::{NegotiatedStream, Sink, StreamRequest};
use crate::error::{Error, Result};
use crate::limits;
use crate::time::Stamp;
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
        /// **Either container**, since P6d. It was AVI's alone for two phases, because a Y4M
        /// header is written before the first frame and its rate field was variable-width text;
        /// note **N106** and its amendment record what the padded field cost and what measured
        /// it (evidence **E17**).
        Measured,
        /// What the camera was asked for: fewer than two frames arrived, or their timestamps
        /// described no usable span, so nothing was measured and the header kept the number the
        /// negotiation named.
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
    /// disagreeing has caught a defect. For Y4M it is the header's `F` ratio, whose denominator
    /// is zero-padded to a fixed width so that `imaging::y4m::Y4mWriter::finish` can rewrite it
    /// in place without moving a byte after it.
    pub declared_interval_us: u32,
    /// Whether that interval was measured, negotiated or provisional.
    ///
    /// The same three answers in both containers since P6d; see the module doc for what used to
    /// make Y4M the exception and what ended it.
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
    /// Carried by **both** containers: `span_us / (frames_written - 1)` is the mean, computed by
    /// whoever wants it out of two numbers that were observed. That is why the Y4M header's
    /// un-rewritable rate cost the caller nothing for the two phases it lasted, and it is still
    /// the number to read — a header declares `frames_written × declared_interval_us`, one frame
    /// period more than this, because a constant-rate container shows its last frame for an
    /// interval no gap between timestamps can contain (note **N120**).
    pub span_us: Option<u64>,
    /// The bound that ended the recording, if one did.
    pub cap_reached: Option<CapReached>,
}

/// How a stream *delivered*, as distinct from what it delivered (design D16; FR-W4).
///
/// USB-over-IP's characteristic failures — added latency, isochronous bandwidth collapse,
/// dropped frames — are visible only in two fields every frame already carries:
/// [`crate::capture::Frame::sequence`], whose gaps mean dropped frames, and
/// [`crate::capture::Frame::timestamp_us`], the driver's own clock. Thermal throttling and a
/// contended hub look the same way on an ordinary rig. This is what an accumulator over
/// those two numbers can say about a take, and **nothing here is a judgement**: the stats
/// rank and report exactly as D8's metrics do, and deciding whether a stream was "healthy"
/// belongs to the consumer whose tolerance it is.
///
/// Every quantity is an integer in microseconds. `imaging::stream_stats::Accumulator`
/// computes it; the recording path fills [`Self::wall_clock_skew_us`], which is the one field
/// no pure core could produce because the engine is the only layer that reads a clock.
///
/// # The frame contract these numbers are read out of
///
/// Stated here, and here only, because this is the surface a `--json` consumer holds: the two
/// frame fields are not wire types — a [`crate::capture::Frame`] never leaves the process —
/// so their semantics would otherwise live in rustdoc that no committed artifact carries and
/// no gate can see (note **N290**). Every backend owes a caller these three sentences. Two of
/// them are claims a stream either honours or breaks, and `testkit::battery::FrameLedger` is
/// the one place they are asserted — pushed into by the conformance battery's stream arm and
/// by both real-device stream arms, so the claim rides whichever backend is in front of it.
/// The third is a *reading* rather than a claim: the counters below are where it is answered,
/// and nothing in this tree refuses a stream for it (note **N298**).
///
/// - **A gap means dropped frames.** [`crate::capture::Frame::sequence`] advances by one for
///   each frame the driver delivered, so a sequence that jumps from *s* to *s + n* is
///   *n - 1* frames the driver had and this link did not — the arithmetic written out
///   because a consumer implementing it off this sentence must land on the same number the
///   tool reports (note **N298**). Those frames are [`Self::frames_dropped`], summed over
///   every jump; the jumps themselves are [`Self::gap_events`], counted as runs.
/// - **A reset is not a gap.** A sequence number that repeats or goes backwards is a driver
///   restarting its counter, not the four billion frames a wrapping subtraction would invent;
///   it is [`Self::sequence_resets`] and it joins no drop count (AGENTS rule 6).
/// - **Both numbers are per stream, and the clock is the driver's own.**
///   [`crate::capture::Frame::timestamp_us`] is that clock in microseconds, not this host's.
///   A frame that arrives no later than the one before it contributes no interval and is
///   counted as [`Self::clock_reversals`] — a finding about the link rather than a broken
///   backend, which is why it is a number here and not a refusal anywhere (D16's last
///   bullet: deciding what a reading means belongs to the consumer). A `STREAMON` may
///   legitimately restart both fields, so no claim here reaches across one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StreamStats {
    /// How many frames this accumulator was given.
    ///
    /// **Which side of the cap that is, stated rather than left to be discovered** (note
    /// **N292**): for a take it is the frames the *container took*, because both muxers push
    /// from inside their commit path and a frame a cap refused is refused here too — so a
    /// take that ended on [`RecordingSummary::cap_reached`] reached this process with one
    /// frame more than this counts, and the summary's own `cap_reached` is what says one did.
    /// A consumer aggregating a live stream in process pushes every frame it receives and
    /// gets the literal reading. The difference is that one frame at the end of a capped take
    /// and nothing at all otherwise, and a forwarding path judged on this number needs to
    /// know which reading it is holding.
    pub frames_delivered: u64,
    /// How many the driver's sequence numbers say never did.
    ///
    /// Summed forward gaps, each one counted as the frames it hides: a jump from *s* to
    /// *s + n* contributes *n - 1*, so a link that lost nothing contributes nothing. A
    /// repeated or backwards sequence number is not a gap — it is a driver doing something
    /// else, counted as [`Self::sequence_resets`] rather than as the four billion frames a
    /// wrapping subtraction would invent.
    pub frames_dropped: u64,
    /// How many *runs* of dropped frames there were.
    ///
    /// Beside [`Self::frames_dropped`] because one gap of sixty and sixty gaps of one are the
    /// same count and different failures: the first is a stall, the second is a link losing
    /// one frame in every two.
    pub gap_events: u32,
    /// Frames whose sequence number did not advance.
    ///
    /// A `u32` sequence wraps after four and a half years at 30 fps, so in practice this is a
    /// driver restarting its counter — recorded rather than discarded (AGENTS rule 6), and
    /// deliberately not folded into the drop count.
    pub sequence_resets: u32,
    /// Frames whose timestamp was not later than the one before it.
    ///
    /// A negative interval is not a duration, so it joins no statistic below; the count is
    /// here because a clock that ran backwards is a finding about the host, not noise.
    pub clock_reversals: u32,
    /// What the intervals between consecutive frames looked like, when there were any.
    ///
    /// `None` for a take of fewer than two usable frames — which spans no interval at all,
    /// and reporting zeros there would claim a measurement nobody made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intervals: Option<IntervalStats>,
    /// The driver's span against the caller's own clock, when a caller filled it in.
    ///
    /// `None` from a pure accumulator, which sees no wall clock by construction. The
    /// recording path fills it — the take's own start and stop stamps against the span the
    /// driver's timestamps describe — and a consumer aggregating a live stream in process
    /// fills it from its own stamps or leaves it absent. Public precisely so it can.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_clock_skew_us: Option<i64>,
}

/// The interval distribution of one stream, in microseconds (design D16).
///
/// **Two tiers, and the split is the bound.** `mean`, `min` and `max` are streaming moments
/// and cover every interval the stream produced. The order statistics — `p50`, `p99` and the
/// jitter — are exact over the intervals the accumulator *retained*, which is every one of
/// them up to `crate::limits::MAX_RECORDING_FRAMES` and the first `retained` of them after
/// that. `retained < observed` is how the answer states its own truncation, which is the
/// only honest way to bound a percentile: a stream longer than the recording cap is a stream
/// this tool has already declined to record whole.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct IntervalStats {
    /// How many intervals the stream produced.
    pub observed: u64,
    /// How many of them the accumulator kept exactly.
    pub retained: u32,
    /// The mean interval, over every one observed.
    pub mean_us: u64,
    /// The shortest, over every one observed.
    pub min_us: u64,
    /// The longest, over every one observed.
    pub max_us: u64,
    /// The median, over the retained ones.
    pub p50_us: u64,
    /// The 99th percentile, over the retained ones — the number a stall shows up in.
    pub p99_us: u64,
    /// Mean absolute deviation from the mean, over the retained ones.
    ///
    /// Mean absolute deviation rather than a standard deviation because this is integer
    /// arithmetic end to end: a variance in microseconds squared overflows a `u64` at a
    /// four-second interval and needs a square root to become a number anybody reads.
    pub jitter_us: u64,
}

impl IntervalStats {
    /// Whether the order statistics cover only part of the stream.
    ///
    /// Derived rather than carried as a field: two spellings of one fact are two things that
    /// can come to disagree, and the two counts are already on the answer.
    #[must_use]
    pub fn is_truncated(&self) -> bool {
        u64::from(self.retained) < self.observed
    }
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
    /// interval any more — the same three refusals `imaging::video::declared_interval` applies
    /// before either container declares [`IntervalSource::Measured`], stated once so the two
    /// cannot disagree.
    ///
    /// It is deliberately **not** the same question as [`Self::declared_interval_us`]: that is
    /// what the *file's own header* says, and a take whose `interval_source` is
    /// [`IntervalSource::Negotiated`] or [`IntervalSource::Provisional`] has a header that
    /// carries a number this method did not produce. The two agree exactly when the close had
    /// something to measure.
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
    /// [`Error::FormatUnsupported`] carrying a [`crate::error::ContainerRefusal`]: the
    /// container the path named, the format the device negotiated, and every container this
    /// build writes that *would* have carried it.
    ///
    /// **It answered in pixel formats until 2026-08-16 and that was the misdirection**
    /// (note **N211**). `requested` held the *negotiated* format, which D5's ranking chooses
    /// unless a caller typed one, so the refusal told a caller to repair a request it had not
    /// made; and `available` held `carries()`, a fact about this build, in the slot every
    /// other producer of this variant fills with a fact about the camera. Over
    /// `corpus/profiles/chicony-rgb.json` — MJPG and YUYV — a `.y4m` request answered
    /// `available: ["YUYV", "NV12", "GREY"]`, offering two formats that sensor has never had.
    /// The lever here is the **file extension**, so that is what the payload names and what
    /// the sentence says; a caller that wants a different *format* asks the camera for one,
    /// and `info` is where the camera answers.
    ///
    /// It is [`Error::FormatUnsupported`] rather than [`Error::DeviceIo`] because this is a
    /// statement about what this build *can record*, which is the variant's own subject. The
    /// symmetrical-looking refusal one layer down — a frame arriving at an open sink in a
    /// format that sink is not carrying — is `DeviceIo`, because by then the container was
    /// chosen correctly and something upstream changed its mind mid-take. AGENTS rule 7 is
    /// the line between them: a capability claim and a malfunction are not the same answer.
    pub fn resolve(requested: Option<Self>, negotiated: PixelFormat) -> Result<Self> {
        match requested {
            // The unnamed arm refuses only when *nothing* carries the stream, so the refusal
            // it builds has an empty remedy — which is the honest answer rather than a thin
            // one: no extension helps, and the caller has to change what the camera delivers.
            None => Self::for_pixel_format(negotiated)
                .ok_or_else(|| Error::container_unsupported(None, negotiated)),
            Some(container) if container.carries_format(negotiated) => Ok(container),
            Some(container) => Err(Error::container_unsupported(Some(container), negotiated)),
        }
    }

    /// What a destination in this container can do with a frame (design D5, amended
    /// 2026-08-13).
    ///
    /// **The map from a container to a destination's capability, beside the containers it
    /// reads** — [`crate::camera::SinkFidelity::of`]'s counterpart for the verb that writes
    /// many frames instead of one, and in the same place for the same reason: the question
    /// is about what *this build's* encodings can carry, so it belongs with them.
    ///
    /// - **AVI** is the verbatim path. It carries MJPEG and nothing else, so a camera's own
    ///   bitstream is remuxed into it byte for byte (E6) and a compressed format is the only
    ///   candidate that arrives with nothing this program did in it.
    /// - **Y4M** carries raw planes and cannot take a compressed frame at all
    ///   ([`VideoFormat::carries`] is the law and [`VideoFormat::resolve`] is the refusal).
    ///   Whatever samples arrive are written whole, so the format that never met the
    ///   camera's quantiser is the better one — which is what
    ///   [`crate::camera::SinkFidelity::EncodesLosslessly`] means.
    ///
    /// An exhaustive match, so a third container cannot be added without answering this for
    /// it, and `every_container_says_what_its_destination_can_do_with_a_frame` walks
    /// [`Self::ALL`] against [`Self::carries`] so the two cannot drift.
    #[must_use]
    pub const fn sink_fidelity(self) -> crate::camera::SinkFidelity {
        match self {
            VideoFormat::Avi => crate::camera::SinkFidelity::PassesCompressedThrough,
            VideoFormat::Y4m => crate::camera::SinkFidelity::EncodesLosslessly,
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

closed_vocabulary! {
    /// Why a recording stopped.
    ///
    /// [`RecordingSummary::cap_reached`] answers a narrower question — *which bound the
    /// container refused a frame at* — and it is `None` for every recording that ended for
    /// any of the other reasons below. An agent reading only that field cannot tell a take
    /// that ran its whole duration from one whose camera went silent after two frames, and
    /// those are different findings about the device under test: the first is a recording,
    /// the second is a defect. So the ending is carried as its own closed vocabulary, walked
    /// by `engine::record`'s `every_ending_in_the_vocabulary_is_one_a_recording_can_reach` —
    /// because an ending nothing can produce is a word in a report rather than an answer.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum RecordingEnd {
        /// The wall-clock budget the request asked for was spent.
        ///
        /// The ordinary ending, and the one [`RecordRequest::budget_ms`] names. Measured on
        /// the engine's own monotonic clock rather than on the driver's frame timestamps —
        /// see [`RecordReport::wall_clock_ms`] for why a recording is bounded by both.
        Duration,
        /// One of the bounds `engine::record::caps` assembles out of
        /// [`crate::limits`] refused a frame.
        ///
        /// [`RecordingSummary::cap_reached`] names which one; it is `Some` exactly when the
        /// ending is this.
        Cap,
        /// The device stopped delivering.
        ///
        /// [`crate::limits::RECORDING_MAX_EMPTY_TURNS`] consecutive turns brought no frame,
        /// which is the bound that covers the case the duration cannot: a driver answering
        /// promptly and delivering nothing, on a clock that is not moving. AGENTS rule 7 is
        /// why it is an *ending* rather than an error — the recording it produced is real
        /// and its frames are the device's own; what is being reported is that there were no
        /// more of them.
        DeviceQuiet,
        /// The caller ended it.
        ///
        /// What `record_stop` produces (docs/7 P6c): a recording driven one turn at a time
        /// stops when its caller says so, and nothing about the device or the bounds is
        /// involved.
        Stopped,
        /// The device refused mid-take, and the caller is holding that refusal.
        ///
        /// The container is still closed — a recording that left a half-written file because
        /// the camera was unplugged would fail docs/7 P6b's "every fault leaves a parseable
        /// file" — so this ending exists to say that the file is finished and the take is
        /// not. `engine::record::run` produces it and then answers with the *device's* own
        /// error rather than with the report, because a `DeviceGone` that arrived as a
        /// successful recording would be exactly the conversion AGENTS rule 7 forbids.
        DeviceFailed,
    }
}

/// Everything one recording needs (design D7, D10).
///
/// [`crate::capture::PhotoRequest`]'s counterpart, and deliberately smaller than it. There is
/// no `settle` field: the frames immediately after `STREAMON` are dark until AE converges
/// \[PF:11\], and a *photo* discards them because one dark frame is the whole answer — a
/// recording's are visible in the file beside everything that follows, and discarding them
/// would move the recording's start away from where the caller put it. An agent filming a
/// transition asked for the moment it asked for. Note **N111** records the decision.
///
/// There is no `format` field either, and that is [`RecordRequest::container`]'s paragraph:
/// the sink path's extension names the container, exactly as it names a photo's encoding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RecordRequest {
    /// What to ask the device's format negotiation for.
    ///
    /// The negotiated answer decides the container when the sink path named none, so this
    /// field and [`RecordRequest::container`] are the two halves of one question — which is
    /// why [`VideoFormat::resolve`] takes both and refuses rather than picking a winner.
    #[serde(default)]
    pub stream: StreamRequest,
    /// How long to record, in milliseconds. `None` is
    /// [`crate::limits::DEFAULT_RECORDING_MS`].
    ///
    /// Read through [`RecordRequest::budget_ms`], which is where the default and the cap
    /// live; a caller that read this field directly would be reading a request rather than a
    /// decision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Where the file goes.
    ///
    /// A [`Sink::ServerPath`] and nothing else — see [`RecordRequest::server_path`] for the
    /// refusal and note **N110** for why this verb narrows D10's two-variant DTO rather than
    /// changing it.
    pub sink: Sink,
    /// Whether a request that finds the camera's command queue full waits for its turn.
    ///
    /// D12's flag, in the same shape and for the same reasons
    /// [`crate::capture::PhotoRequest::wait`] carries it — that field's doc is the argument
    /// and this one is not a second copy of it. What is worth saying twice is which *call* it
    /// bounds: a recording is a chain of turns (note **N111**), and only the **first** of them
    /// negotiates a stream on a camera somebody else may be holding. So this flag decides how
    /// `record_start` meets a busy camera and nothing else — the turns that follow are issued
    /// by a driver that already owns the take, and a recording that had to re-queue per frame
    /// would be a recording bounded by other clients rather than by its own duration.
    ///
    /// `#[serde(default)]` for [`crate::capture::PhotoRequest::wait`]'s reason: `false` is the
    /// behaviour every caller written before this field existed already meets, and a default
    /// that turned a prompt [`Error::Busy`] into ten seconds of latency would change requests
    /// nobody rewrote.
    #[serde(default)]
    pub wait: bool,
}

impl RecordRequest {
    /// The path this recording is written to, or a refusal naming why there has to be one.
    ///
    /// **A recording is not returned as bytes** (note **N110**). D10 says binary results
    /// cross the wire via a two-variant sink DTO, and this verb uses one of the two: a
    /// recording is bounded by [`crate::limits::MAX_RECORDING_BYTES`], which is two orders of
    /// magnitude past [`crate::limits::RPC_MAX_RESPONSE_BYTES`] before base64 adds a third,
    /// so [`Sink::ReturnBytes`] names an answer this build cannot send. The refusal says so
    /// and names the remedy, because AGENTS' primary consumer has no hands to work it out
    /// with.
    ///
    /// The second half is [`Sink::is_addressable`]'s rule, asked here rather than only at the
    /// daemon: a relative path from a socket would be resolved against the daemon's own
    /// working directory, which under systemd is `/`. One rule asked twice is not two rules
    /// (note **N46**).
    ///
    /// # Errors
    ///
    /// [`Error::IllegalTransition`] for a [`Sink::ReturnBytes`] sink and for a relative
    /// [`Sink::ServerPath`].
    pub fn server_path(&self) -> Result<&Utf8Path> {
        match &self.sink {
            Sink::ReturnBytes { .. } => Err(Error::IllegalTransition {
                from: "a recording asked for as bytes in the answer".to_owned(),
                op: format!(
                    "return a recording of up to {} bytes in a JSON-RPC result bounded at {} \
                     before base64 grows it by a third; name an absolute server path instead",
                    limits::MAX_RECORDING_BYTES,
                    limits::RPC_MAX_RESPONSE_BYTES
                ),
            }),
            Sink::ServerPath { path } if !path.is_absolute() => Err(Error::IllegalTransition {
                from: format!("a relative recording path ({path})"),
                op: "write a recording to a path this engine cannot resolve; send an absolute \
                     one, resolved against the caller's own working directory"
                    .to_owned(),
            }),
            Sink::ServerPath { path } => Ok(path.as_path()),
        }
    }

    /// The container the *request* names, read off the sink path's extension.
    ///
    /// `Ok(None)` when the path carries no extension at all, which means the negotiated
    /// stream decides ([`VideoFormat::resolve`]'s `None` arm). That case is the one AGENTS'
    /// primary consumer needs: an agent that has not enumerated a camera's formats does not
    /// know whether it is about to get MJPG or GREY, and a verb where you must know that to
    /// name the file is a verb needing a call sequence. `record -o /tmp/take` records
    /// whatever the camera gives and the report says which container it was.
    ///
    /// An extension this build does **not** write is a refusal rather than a container
    /// chosen behind the caller's back, and it is the same defect
    /// [`Sink::writable_format`] refuses one verb along: `/tmp/take.mkv` filled with a Y4M
    /// would be a file whose name lies about its contents (debt D-1, note **N46**).
    ///
    /// # Errors
    ///
    /// [`Error::IllegalTransition`] naming the extension that was typed and the ones this
    /// build writes, derived from [`VideoFormat::ALL`] rather than transcribed. Deliberately
    /// **not** [`Error::FormatUnsupported`]: that variant is the *camera* saying what it
    /// cannot offer (E3), and `.mkv` is not the camera's fault. Plus whatever
    /// [`RecordRequest::server_path`] refuses, since a sink with no path has no extension to
    /// read.
    pub fn container(&self) -> Result<Option<VideoFormat>> {
        let path = self.server_path()?;
        match path.extension() {
            None => Ok(None),
            Some(extension) => VideoFormat::from_extension(extension)
                .map(Some)
                .ok_or_else(|| Error::IllegalTransition {
                    from: format!("unwritable_extension({extension})"),
                    op: format!(
                        "record to {path}; this build writes {}",
                        VideoFormat::ALL
                            .iter()
                            .map(|format| format!(".{}", format.extension()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                }),
        }
    }

    /// How long this recording may run, in milliseconds.
    ///
    /// The one home for [`crate::limits::DEFAULT_RECORDING_MS`] and
    /// [`crate::limits::MAX_RECORDING_MS`], so `webcam-handler-cli` and
    /// `webcam-handler-daemon` cannot answer "how long is a recording with no `--duration`"
    /// differently.
    ///
    /// **A duration past the cap is refused, never clamped.** The two answers are
    /// indistinguishable to the consumer that matters: an agent that asked for five minutes
    /// and silently received two cannot tell that from a camera that stopped after two, and
    /// the whole point of a recording is to be measured. An agent that is *told* the cap can
    /// ask again for something inside it.
    ///
    /// **And a duration of zero is refused for the same reason, which the cap's argument
    /// always covered and the code did not** (docs/11 **L30**, note **N213**). A budget of
    /// `0` runs no turn at all: the file is opened, the header is written, and the answer
    /// says `frames_written: 0` with `ended: "duration"` and exits `0` — a *successful*
    /// recording of nothing, which is the one outcome an unattended caller cannot tell from
    /// a camera that delivered nothing. One millisecond is enough to record a frame
    /// (measured: `--duration 1ms` over `corpus/profiles/chicony-rgb.json` writes one), so
    /// the floor is the smallest number this field can hold and not a policy of its own —
    /// which is why it is written here rather than added to `crate::limits`.
    ///
    /// # Errors
    ///
    /// [`Error::IllegalTransition`] naming both the duration asked for and the bound it
    /// missed, at either end.
    pub fn budget_ms(&self) -> Result<u64> {
        match self.duration_ms {
            None => Ok(limits::DEFAULT_RECORDING_MS),
            Some(asked) if asked > limits::MAX_RECORDING_MS => Err(Error::IllegalTransition {
                from: format!("a {asked} ms recording"),
                op: format!(
                    "record for longer than the {} ms this build will; ask for a duration at \
                     or under it",
                    limits::MAX_RECORDING_MS
                ),
            }),
            Some(0) => Err(Error::IllegalTransition {
                from: "a 0 ms recording".to_owned(),
                op: "record for no time at all; that writes a container header and no frames \
                     and answers as a success, so ask for at least 1 ms"
                    .to_owned(),
            }),
            Some(asked) => Ok(asked),
        }
    }

    /// This request's stream, told what the container it named can carry (design D5, amended
    /// 2026-08-13).
    ///
    /// **The one place [`crate::capture::StreamRequest::sink_fidelity`] is written for a
    /// recording**, and [`crate::capture::PhotoRequest::stream_for_sink`]'s shape one verb
    /// along: a record request is the only value in this vocabulary holding a stream and a
    /// destination at once, so it is the only value that can answer the tiebreak's question.
    /// Deriving it here rather than in each caller is what keeps `webcam-handler-cli` and
    /// `webcam-handler-daemon` from asking one device two subtly different questions, and it
    /// costs the wire nothing because the field is derived at the point of use rather than
    /// sent.
    ///
    /// **A request that named no extension gets the default**, and that is the right answer
    /// rather than a fallback: the negotiated format is what decides the container in that
    /// case ([`RecordRequest::container`]), so there is no destination to reason about and
    /// every recordable format is admissible.
    ///
    /// # Errors
    ///
    /// Whatever [`RecordRequest::container`] refuses: a sink that is not a path, a relative
    /// one, or an extension this build does not write.
    pub fn stream_for_container(&self) -> Result<crate::capture::StreamRequest> {
        Ok(self.stream.for_sink(
            self.container()?
                .map_or_else(Default::default, VideoFormat::sink_fidelity),
        ))
    }
}

/// What one recording turned out to be (design D7, D10).
///
/// [`crate::capture::PhotoReport`]'s counterpart. Every field is a measurement of what
/// happened rather than a restatement of the request — `negotiated` is D5's "requested is not
/// applied" for a stream that ran for seconds rather than for one frame, `format` is the
/// container the pairing chose, and `summary` is what the container itself counted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RecordReport {
    /// The camera that was recorded.
    pub camera: CameraId,
    /// When the recording started, on the wall clock.
    ///
    /// The caller's [`Stamp`], as `PhotoReport::taken_at` is: the engine reads no clock
    /// (design §2.10), and the monotonic one this report's [`RecordReport::wall_clock_ms`]
    /// comes from is a different question — conflating them is how an NTP step becomes a
    /// duration.
    pub started_at: Stamp,
    /// The file that was written.
    #[schemars(with = "String")]
    pub path: Utf8PathBuf,
    /// The container it is in.
    ///
    /// Reported rather than assumed, because a request that named no extension let the
    /// negotiated stream decide — so this is the answer to a question the caller may have
    /// left open (see [`RecordRequest::container`]).
    pub format: VideoFormat,
    /// What the device agreed to deliver, with every difference from the request (D5).
    pub negotiated: NegotiatedStream,
    /// What the container counted.
    pub summary: RecordingSummary,
    /// How the take's frames were *delivered* (design D16).
    ///
    /// Beside [`Self::summary`] rather than inside it, because they answer two questions
    /// about one take: the summary is what the **file** turned out to be, and this is what
    /// the **link** turned out to be. A consumer proving a camera-forwarding path reads this
    /// one — `frames_dropped` here and [`RecordingSummary::dropped_frames`] are two readings
    /// of one accumulator, so they cannot disagree, and everything else here (the gap runs,
    /// the interval distribution, the skew against this report's own wall clock) exists
    /// nowhere else in the document.
    ///
    /// **Required on the wire, and that is the whole of its shape** (note **N291**). A
    /// `#[serde(default)]` here would make "this daemon never measured delivery" and "this
    /// link delivered nothing" the same document, because the default [`StreamStats`] is
    /// byte-identical to the answer a [`RecordingEnd::DeviceQuiet`] take produces — the
    /// conversion AGENTS rule 7 forbids, arriving through a serde attribute rather than
    /// through a `match`. An `Option` would be the other wrong answer: no producer in this
    /// tree can construct the `None`, so it would be an arm every consumer must branch on and
    /// nothing can ever reach. An answer that arrives without this field is a version skew,
    /// and a version skew is refused by name.
    pub stats: StreamStats,
    /// How long the recording took on the engine's monotonic clock, in milliseconds.
    ///
    /// **Beside [`RecordingSummary::span_us`] rather than instead of it, because they are
    /// two measurements and the difference between them is a finding.** `span_us` is the last
    /// written frame's timestamp minus the first's, on the *driver's* clock and in
    /// microseconds; this is the engine's own elapsed time across the whole take, in
    /// milliseconds. `webcam-handler-imaging` reads no clock of its own — its
    /// `RecordingCaps::max_span` doc says wall-clock time is the caller's question — and this
    /// engine is that caller.
    ///
    /// Three things make the two disagree, and a caller can tell them apart from the rest of
    /// this document: a **driver clock that ran slow or fast** (the span is short or long
    /// against a wall clock that is not), **frames dropped before they reached us**
    /// ([`RecordingSummary::dropped_frames`] is non-zero), and **time spent in our own loop**
    /// — the wall clock includes the header write, the settle-free start, the close-time
    /// index and every turn that brought no frame, none of which are inside the span. The
    /// wall clock is therefore always the larger of the two for an honest take, and a span
    /// that *exceeds* it is a driver clock this host does not share.
    ///
    /// **The file's own declared duration is a third number and it is not the span.** A
    /// constant-rate container declares `frames_written × declared_interval_us`, while the
    /// interval is the mean of `frames_written - 1` gaps — so a healthy take's file legitimately
    /// claims **one frame period more** video than this wall clock, and a comparison written the
    /// obvious way goes red on a camera that is working (note **N120**; measured at 1102 ms
    /// declared against 1016 ms of wall clock on a 8 fps take). The bound docs/7 P6d asks for is
    /// therefore two-sided: at most one frame period above, and `MAX_WALL_CLOCK_OVERHANG_MS`
    /// below, with `crates/backends/v4l2/tests/hardware.rs` carrying the measurement.
    ///
    /// Milliseconds rather than microseconds because that is the resolution the engine's
    /// clock actually has (`engine::settle::Millis`), and a `wall_clock_us` filled in by
    /// multiplying by a thousand would be three digits of precision nobody measured.
    ///
    /// This pair is docs/7 **P6d**'s criterion — the declared-vs-wall-clock duration bound,
    /// measured on a real capture — which is why both live in the one document a `--json`
    /// consumer already has.
    pub wall_clock_ms: u64,
    /// Why it stopped.
    pub ended: RecordingEnd,
}

/// What one camera's recording is doing (design D10; docs/7 P6c).
///
/// **The answer to `record_status`, and D10's whole progress mechanism**: *"progress by
/// polling `record_status` — no recording subscription in v1"*. A recording runs for seconds
/// or minutes on a camera the caller does not hold, so the only thing a caller can do between
/// `record_start` and `record_stop` is ask; this is the shape of the asking.
///
/// It is a **struct with an optional take** rather than a three-armed enum, and the shape is
/// the claim: a camera has at most one take, so "is there one" and "what is it doing" are one
/// question with one answer and [`RecordStatus::is_running`] is the one predicate every
/// consumer branches on. A client's poll loop, `webcam-handler-cli`'s renderer and the
/// daemon's own refusals all read that one function, which is what stops three copies of "has
/// it finished?" from disagreeing about a take whose driver has stopped but whose container is
/// still closing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RecordStatus {
    /// The camera this is about, resolved (D1) rather than echoed back.
    pub camera: CameraId,
    /// The take this camera holds, or `None` when it holds none.
    ///
    /// `None` is a camera that has never recorded **and** a camera whose take has been
    /// collected by `record_stop`. Those are deliberately one answer rather than two: what a
    /// caller can do about either is identical — start a recording — and a vocabulary that
    /// distinguished them would be this daemon remembering something about a camera after the
    /// caller asked for it to be forgotten.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub take: Option<TakeStatus>,
}

impl RecordStatus {
    /// Whether a recording is running on this camera right now.
    ///
    /// **The one predicate**, for the module-level reason above. A take that has reached an
    /// ending is not running even though it is still here to be collected, which is exactly
    /// the distinction a polling client needs: it stops polling when this answers `false` and
    /// calls `record_stop` to collect what the answer is about.
    ///
    /// [`TakeStatus::ended`] alone, and the [`TakeStatus::failed`] beside it is deliberately
    /// **not** asked. Every way a take can stop fills the ending in — a refusal is
    /// [`RecordingEnd::DeviceFailed`], and a take whose *container* refused keeps whichever
    /// ending its loop reached — so a `failed` without an `ended` is a state nothing produces,
    /// and a second condition that cannot change an answer is dead code wearing a guard. That
    /// is not a deduction: it is what a hand-applied mutant found by deleting the condition and
    /// watching the whole workspace stay green (note **N115**, mutant M8), and the repair is
    /// the deletion rather than a test about a state that cannot exist.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.take.as_ref().is_some_and(|take| take.ended.is_none())
    }
}

/// One take, while it runs and after it ends (design D7, D10).
///
/// Every field is a *measurement*, in [`RecordingSummary`]'s tradition, and the two are not
/// the same document: this one exists while the container is still open, so it carries what
/// can be known mid-take and nothing that cannot. `frames_written` is here and
/// `bytes_written` is not — the container's byte count includes trailers written at close
/// (`imaging::avi::write::AviWriter::finish` writes the whole `idx1` there), so a running
/// take's file size is not the recording's size and reporting it as one would be a number
/// that changes meaning when the take ends.
///
/// The finished take's full accounting is [`RecordReport`], which `record_stop` answers with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TakeStatus {
    /// The file it is being written to.
    #[schemars(with = "String")]
    pub path: Utf8PathBuf,
    /// The container, decided by [`VideoFormat::resolve`] when the take started.
    ///
    /// Reported from the first status onwards because a request that named no extension left
    /// it to the negotiated stream, and an agent that has not enumerated the camera learns
    /// here which file it is about to have (see [`RecordRequest::container`]).
    pub format: VideoFormat,
    /// What the device agreed to deliver, with every difference from the request (D5).
    pub negotiated: NegotiatedStream,
    /// When it started, on the wall clock.
    pub started_at: Stamp,
    /// How long it may run, from [`RecordRequest::budget_ms`].
    ///
    /// Beside [`TakeStatus::elapsed_ms`] so a caller can draw a bar without holding the
    /// request it sent — AGENTS' primary consumer has no hands, and a progress mechanism that
    /// needs the client to remember what it asked for is one a restarted client cannot use.
    pub budget_ms: u64,
    /// How long it has run, on the daemon's monotonic clock.
    ///
    /// Monotonic rather than `Stamp::now() - started_at`, because those are two clocks and
    /// subtracting one from the other is how an NTP step becomes a duration
    /// ([`RecordReport::wall_clock_ms`] argues the same pair).
    pub elapsed_ms: u64,
    /// How many frames the container has accepted so far.
    ///
    /// Frames *written*, not frames delivered: a frame a cap refused is not in the file, and a
    /// count that included it would tell an agent its recording holds a frame it can never
    /// read. It ends equal to [`RecordingSummary::frames_written`], which is a cross-check a
    /// test can make rather than a coincidence.
    pub frames_written: u32,
    /// Why it stopped — `None` while it is still running.
    pub ended: Option<RecordingEnd>,
    /// The D13 discriminant of the refusal that ended it, when one did.
    ///
    /// A **kind and not the error**, for `schema::progress::CalibrationProgress`'s reason one
    /// stream along: a status document is read by a poller that wants to know whether to stop
    /// polling, and the error itself belongs to the caller that asked for the take — which is
    /// `record_stop`, and which answers with the device's own [`Error`] unchanged. Putting the
    /// whole error here as well would give one refusal two homes and let a client act on the
    /// copy that is easier to reach rather than on the one the verb returned.
    ///
    /// **It does not follow [`TakeStatus::ended`], and the two are separate fields because of
    /// that rather than in spite of it.** A take the *device* refused ends
    /// [`RecordingEnd::DeviceFailed`] and carries the device's kind here. A take that ran its
    /// duration and whose **container could not be closed** ends
    /// [`RecordingEnd::Duration`] — because that is what happened, and rewriting the ending
    /// because a flush refused would lose the one fact the vocabulary carries — and carries
    /// the disk's kind here. An ending is a fact about the recording and a refusal is a fact
    /// about the whatever refused; AGENTS rule 7 is the line between them, and folding one
    /// into the other is the collapse it forbids.
    ///
    /// What is always true is the other direction: `failed` is `Some` only for a take that has
    /// **ended**, because nothing fills it in until [`TakeStatus::ended`] does.
    pub failed: Option<crate::error::ErrorKind>,
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
        // must not silently receive a Y4M, and must be told which file would have taken the
        // frames. **The remedy is the extension** (note **N211**): the format was chosen by
        // D5's ranking rather than typed, so a payload that named formats would be answering
        // a question the caller did not ask with facts about a build rather than a camera.
        let refusal = VideoFormat::resolve(Some(VideoFormat::Avi), PixelFormat::YUYV)
            .expect_err("AVI does not carry YUYV");
        match refusal {
            Error::FormatUnsupported {
                requested,
                available,
                // A container refuses a *file*, never a size — which is what makes
                // `size.is_some()` a reliable discriminator for the one caller that acts on
                // it (note **N138**).
                size: None,
                container: Some(container),
            } => {
                assert_eq!(
                    requested, None,
                    "the negotiated format is not what the caller asked for, and reporting it \
                     as `requested` sends the caller to repair a request it never made"
                );
                assert!(
                    available.is_empty(),
                    "a container refusal that also lists formats offers a caller a lever it \
                     cannot pull and this build's list where the camera's belongs: {available:?}"
                );
                assert_eq!(container.container, Some(VideoFormat::Avi));
                assert_eq!(container.negotiated, PixelFormat::YUYV);
                assert_eq!(container.carried_by, vec![VideoFormat::Y4m]);
                assert!(
                    !container.carried_by.contains(&VideoFormat::Avi),
                    "the remedy must not offer the container that was just refused"
                );
            }
            other => panic!("wrong variant: {other:?}"),
        }

        // And the sentence, which is the half a caller reads first: it names the extension to
        // type and no pixel format at all.
        let message = VideoFormat::resolve(Some(VideoFormat::Avi), PixelFormat::YUYV)
            .expect_err("AVI does not carry YUYV")
            .to_string();
        assert!(message.contains(".y4m"), "{message}");
        assert!(
            !message.contains("MJPG") && !message.contains("JPEG"),
            "the refusal offers formats this camera may never have had: {message}"
        );

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

        // A format outside every container is refused with an **empty** remedy, and that is
        // the answer rather than a gap in it (note **N211**): no extension helps, so naming
        // one would be false, and naming the formats this build does record would offer a
        // caller a list its camera need not intersect — N129's misdirection, which is the
        // defect this payload exists to stop committing.
        let h264 = PixelFormat::parse("H264").expect("four characters");
        let refusal = VideoFormat::resolve(None, h264).expect_err("no container carries H264");
        match &refusal {
            Error::FormatUnsupported {
                requested,
                available,
                size: None,
                container: Some(container),
            } => {
                assert_eq!(*requested, None);
                assert!(available.is_empty(), "{available:?}");
                assert_eq!(
                    container.container, None,
                    "the caller named no container, so no extension is the thing it got wrong"
                );
                assert_eq!(container.negotiated, h264);
                assert!(
                    container.carried_by.is_empty(),
                    "a container was offered for a format no container carries: {:?}",
                    container.carried_by
                );
            }
            other => panic!("wrong variant: {other:?}"),
        }
        // The sentence says which frames could not be written and stops, because "stop" is
        // what a caller with no lever has to do.
        let message = refusal.to_string();
        assert!(message.contains("H264"), "{message}");
        assert!(
            !message.contains(".avi") && !message.contains(".y4m"),
            "the refusal offers an extension that would meet the same wall: {message}"
        );
    }

    /// A device offering MJPG and YUYV at exactly the same resolution.
    ///
    /// **The tie is the fixture's whole job.** D5's ranking asks about a readable size, a
    /// named FourCC and pixels before it asks about fidelity, so the destination's vote only
    /// decides candidates nothing else separates — and a fixture whose formats differed on
    /// resolution would be answered before [`crate::camera::SinkFidelity`] was consulted and
    /// could not tell a derived one from a defaulted one.
    fn a_device_whose_formats_tie_on_pixels() -> Vec<crate::camera::FormatInfo> {
        let sizes = || {
            vec![crate::camera::FrameSizeInfo {
                size: crate::camera::FrameSize::Discrete {
                    width: 1280,
                    height: 720,
                },
                intervals: Vec::new(),
            }]
        };
        [PixelFormat::MJPG, PixelFormat::YUYV]
            .into_iter()
            .map(|pixel_format| crate::camera::FormatInfo {
                pixel_format,
                description: format!("{pixel_format}"),
                flags: 0,
                sizes: sizes(),
            })
            .collect()
    }

    #[test]
    fn a_recording_asks_the_device_for_what_the_container_it_named_can_carry() {
        // **`StreamRequest::sink_fidelity` had three producers and the recording path was
        // not one of them** (the G6 review's L18; note **N206**), so a `.y4m` request reached D5's
        // ranking
        // claiming a destination that takes a camera bitstream byte for byte — which is the
        // one thing the raw container cannot do. On a camera whose MJPG and YUYV modes tie,
        // that ranked MJPG, and `VideoFormat::resolve` then refused the recording over a
        // format the file could have carried.
        //
        // The derivation is `VideoFormat::sink_fidelity` and the one place it is applied is
        // `RecordRequest::stream_for_container`, which is `PhotoRequest::stream_for_sink`'s
        // shape one verb along — the datum derived at the point of use rather than sent, so
        // both roots reach the same answer with nothing added to the wire.
        let formats = a_device_whose_formats_tie_on_pixels();

        let raw = to_path("/tmp/take.y4m");
        let chosen = raw
            .stream_for_container()
            .expect("the extension names a container")
            .choose(&formats)
            .expect("the device offers something");
        assert_eq!(
            chosen.pixel_format,
            PixelFormat::YUYV,
            "a Y4M destination was ranked as one that passes a compressed bitstream through"
        );
        // And the resolution the caller never named is untouched: the destination's vote is
        // the tiebreak D5 gives it and not a second opinion about size.
        assert_eq!((chosen.width, chosen.height), (1280, 720));
        // The whole point, end to end: the container the caller typed can carry what the
        // ranking chose.
        assert_eq!(
            VideoFormat::resolve(raw.container().expect("an extension"), chosen.pixel_format)
                .expect("the container carries what was ranked"),
            VideoFormat::Y4m
        );

        // The other direction, so the assertion above is about the derivation rather than
        // about a preference for raw formats: an AVI destination *is* the verbatim path, and
        // the compressed candidate is the one that reaches it with nothing of ours in it.
        let avi = to_path("/tmp/take.avi");
        assert_eq!(
            avi.stream_for_container()
                .expect("the extension names a container")
                .choose(&formats)
                .expect("the device offers something")
                .pixel_format,
            PixelFormat::MJPG
        );

        // A path with no extension leaves the container to the negotiated format, so there
        // is no destination to reason about and the default stands — which is the answer
        // every `record -o /tmp/take` has always had.
        let unnamed = to_path("/tmp/take");
        assert_eq!(
            unnamed
                .stream_for_container()
                .expect("no extension is not a refusal")
                .sink_fidelity,
            crate::camera::SinkFidelity::default()
        );

        // An explicit request still wins over all of it (D5), because the destination's vote
        // is a tiebreak among the formats the ranking is choosing from and a named format is
        // not chosen at all.
        let mut named = to_path("/tmp/take.y4m");
        named.stream.pixel_format = Some(PixelFormat::MJPG);
        assert_eq!(
            named
                .stream_for_container()
                .expect("the extension names a container")
                .choose(&formats)
                .expect("MJPG is on this device")
                .pixel_format,
            PixelFormat::MJPG,
            "the ranking overrode a format the caller named"
        );
    }

    #[test]
    fn every_container_says_what_its_destination_can_do_with_a_frame() {
        // The map walked over `VideoFormat::ALL`, so a third container cannot be added
        // without answering the question for it — and the two answers have to differ, or the
        // datum would be decoration.
        let answers: Vec<crate::camera::SinkFidelity> = VideoFormat::ALL
            .iter()
            .map(|container| container.sink_fidelity())
            .collect();
        assert_eq!(answers.len(), VideoFormat::ALL.len());
        assert_eq!(
            VideoFormat::Avi.sink_fidelity(),
            crate::camera::SinkFidelity::PassesCompressedThrough
        );
        assert_eq!(
            VideoFormat::Y4m.sink_fidelity(),
            crate::camera::SinkFidelity::EncodesLosslessly
        );

        // Said from the other end, so the map cannot drift from the thing that justifies it:
        // a container that passes a compressed bitstream through is exactly one that carries
        // a compressed format, and one that does not carries none.
        for &container in VideoFormat::ALL {
            let carries_compressed = container.carries().iter().any(|format| {
                matches!(
                    crate::camera::Lossiness::of(*format),
                    crate::camera::Lossiness::Compressed
                )
            });
            assert_eq!(
                carries_compressed,
                container.sink_fidelity() == crate::camera::SinkFidelity::PassesCompressedThrough,
                "{container} carries {:?} and answers {:?}",
                container.carries(),
                container.sink_fidelity()
            );
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

    /// A request to `path`, with no duration named.
    fn to_path(path: &str) -> RecordRequest {
        RecordRequest {
            stream: StreamRequest::default(),
            duration_ms: None,
            sink: Sink::ServerPath { path: path.into() },
            wait: false,
        }
    }

    #[test]
    fn a_recording_asked_for_as_bytes_is_refused_naming_the_bound_and_the_remedy() {
        // Note **N110**: D10's sink DTO has two variants and this verb takes one of them.
        // The refusal has to carry both halves an unattended caller acts on — why the answer
        // cannot come back inline, and what to send instead — because the alternative is an
        // agent retrying the same request.
        let refusal = RecordRequest {
            stream: StreamRequest::default(),
            duration_ms: None,
            sink: Sink::ReturnBytes {
                format: crate::capture::PhotoFormat::Jpeg,
            },
            wait: false,
        }
        .server_path()
        .expect_err("a recording does not come back in the answer");
        assert_eq!(refusal.kind(), crate::error::ErrorKind::IllegalTransition);
        let rendered = refusal.to_string();
        assert!(
            rendered.contains(&limits::MAX_RECORDING_BYTES.to_string()),
            "the refusal must name the bound it is about: {rendered}"
        );
        assert!(
            rendered.contains("absolute server path"),
            "the refusal must name the remedy: {rendered}"
        );

        // A relative path is the other half of the same question (note N46), and an absolute
        // one is served — so the arm above refuses a *shape* rather than refusing sinks.
        assert!(to_path("take.avi").server_path().is_err());
        assert_eq!(
            to_path("/tmp/take.avi").server_path().expect("absolute"),
            Utf8Path::new("/tmp/take.avi")
        );
    }

    #[test]
    fn the_sink_paths_extension_names_the_container_and_one_this_build_cannot_write_is_refused() {
        // The `.webp` defect one container along (debt D-1, note N46): a `/tmp/take.mkv`
        // filled with a Y4M is a file whose name lies about its contents, and a *reader* of
        // that file has no way to find out. Both directions, and the no-extension arm, which
        // is the one AGENTS' handless consumer depends on.
        for &container in VideoFormat::ALL {
            let path = format!("/tmp/take.{}", container.extension());
            assert_eq!(
                to_path(&path).container().expect("a writable extension"),
                Some(container)
            );
        }
        assert_eq!(
            to_path("/tmp/take").container().expect("no extension"),
            None,
            "a path with no extension lets the negotiated stream decide"
        );

        let refusal = to_path("/tmp/take.mkv")
            .container()
            .expect_err("this build writes no Matroska");
        let rendered = refusal.to_string();
        for &container in VideoFormat::ALL {
            assert!(
                rendered.contains(&format!(".{}", container.extension())),
                "the refusal must name what this build does write: {rendered}"
            );
        }
        assert!(rendered.contains("mkv"), "{rendered}");
    }

    #[test]
    fn a_duration_past_the_cap_is_refused_rather_than_clamped_and_the_default_is_the_limits_one() {
        // The clamp that is not taken, and why: an agent that asked for five minutes and
        // silently received two cannot tell that from a camera that stopped after two, and
        // the recording exists to be measured. Both directions, and the boundary itself,
        // because a refusal at the cap rather than past it is a different bound.
        assert_eq!(
            to_path("/tmp/take.avi").budget_ms().expect("the default"),
            limits::DEFAULT_RECORDING_MS
        );

        let at_the_cap = RecordRequest {
            duration_ms: Some(limits::MAX_RECORDING_MS),
            ..to_path("/tmp/take.avi")
        };
        assert_eq!(
            at_the_cap
                .budget_ms()
                .expect("a duration at the cap is inside it"),
            limits::MAX_RECORDING_MS
        );

        let past_it = RecordRequest {
            duration_ms: Some(limits::MAX_RECORDING_MS + 1),
            ..to_path("/tmp/take.avi")
        };
        let rendered = past_it
            .budget_ms()
            .expect_err("a millisecond past the cap is past it")
            .to_string();
        assert!(
            rendered.contains(&limits::MAX_RECORDING_MS.to_string()),
            "an agent that is not told the cap cannot ask again inside it: {rendered}"
        );
    }

    #[test]
    fn a_request_and_a_report_round_trip_through_json_with_their_vocabulary_spelled_out() {
        // The wire shape of the two documents `record` carries, asserted rather than
        // assumed: `schemas/` is what a `--json` consumer validates against, so the spelling
        // of the ending vocabulary inside a report is a contract.
        let request = RecordRequest {
            duration_ms: Some(2_000),
            ..to_path("/tmp/take.avi")
        };
        let json = serde_json::to_string(&request).expect("serialize");
        let back: RecordRequest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, request);
        // A request that named no duration omits the field rather than sending a null, which
        // is what `skip_serializing_if` is for and what a hand-written client will send.
        let bare = serde_json::to_string(&to_path("/tmp/take.avi")).expect("serialize");
        assert!(!bare.contains("duration_ms"), "{bare}");
        let back: RecordRequest = serde_json::from_str(&bare).expect("deserialize");
        assert_eq!(back.duration_ms, None);

        for &ended in RecordingEnd::ALL {
            let rendered = serde_json::to_string(&ended).expect("serialize");
            let expected = format!("{ended:?}")
                .chars()
                .enumerate()
                .flat_map(|(index, ch)| {
                    let mut out = Vec::new();
                    if ch.is_uppercase() && index > 0 {
                        out.push('_');
                    }
                    out.extend(ch.to_lowercase());
                    out
                })
                .collect::<String>();
            assert_eq!(rendered, format!("\"{expected}\""), "{ended:?}");
        }
    }

    /// A take of `path`, running, with nothing measured yet.
    fn running_take(path: &str) -> TakeStatus {
        TakeStatus {
            path: path.into(),
            format: VideoFormat::Avi,
            negotiated: NegotiatedStream {
                pixel_format: PixelFormat::MJPG,
                width: 64,
                height: 48,
                bytes_per_line: 0,
                size_image: 4096,
                interval: crate::camera::FrameInterval::Discrete {
                    numerator: 1,
                    denominator: 30,
                },
                adjustments: Vec::new(),
            },
            started_at: Stamp::epoch(),
            budget_ms: limits::DEFAULT_RECORDING_MS,
            elapsed_ms: 0,
            frames_written: 0,
            ended: None,
            failed: None,
        }
    }

    #[test]
    fn a_camera_is_recording_exactly_while_its_take_has_not_reached_an_ending() {
        // The one predicate a polling client branches on (D10: "progress by polling
        // `record_status`"), in all four of its states — because a build that answered
        // `true` for a finished take would make a client poll for ever, and one that
        // answered `false` for a running take would make it collect a recording that is
        // still being written.
        let camera = CameraId::parse("cam:test").expect("a literal id");
        let idle = RecordStatus {
            camera: camera.clone(),
            take: None,
        };
        assert!(!idle.is_running(), "a camera with no take is not recording");

        let running = RecordStatus {
            camera: camera.clone(),
            take: Some(running_take("/tmp/take.avi")),
        };
        assert!(running.is_running());

        let ended = RecordStatus {
            camera: camera.clone(),
            take: Some(TakeStatus {
                ended: Some(RecordingEnd::Duration),
                ..running_take("/tmp/take.avi")
            }),
        };
        assert!(
            !ended.is_running(),
            "a take that ended is not still running"
        );

        // A take the device refused, and one whose *container* refused after an ordinary
        // ending. Both stop the poll loop, and they are two arms rather than one because the
        // two fill `ended` in differently — `DeviceFailed` for the first, whatever the loop
        // reached for the second — which is the asymmetry `TakeStatus::failed`'s own doc
        // argues and the reason `is_running` reads only `ended`.
        let refused = RecordStatus {
            camera: camera.clone(),
            take: Some(TakeStatus {
                ended: Some(RecordingEnd::DeviceFailed),
                failed: Some(crate::error::ErrorKind::DeviceGone),
                ..running_take("/tmp/take.avi")
            }),
        };
        assert!(!refused.is_running());

        let unclosable = RecordStatus {
            camera,
            take: Some(TakeStatus {
                ended: Some(RecordingEnd::Duration),
                failed: Some(crate::error::ErrorKind::StorageIo),
                ..running_take("/tmp/take.avi")
            }),
        };
        assert!(!unclosable.is_running());
    }

    #[test]
    fn a_status_round_trips_through_json_and_a_camera_with_no_take_omits_the_field() {
        // The wire shape of the document a poll loop reads, asserted rather than assumed:
        // `schemas/` is what a `--json` consumer validates against, and the *absence* of
        // `take` is what tells a hand-written client that this camera holds nothing —
        // a `"take":null` would be a third thing for it to handle.
        let camera = CameraId::parse("cam:test").expect("a literal id");
        let idle = RecordStatus {
            camera: camera.clone(),
            take: None,
        };
        let json = serde_json::to_string(&idle).expect("serialize");
        assert!(!json.contains("take"), "{json}");
        assert_eq!(
            serde_json::from_str::<RecordStatus>(&json).expect("deserialize"),
            idle
        );

        let running = RecordStatus {
            camera,
            take: Some(TakeStatus {
                frames_written: 7,
                elapsed_ms: 233,
                ..running_take("/tmp/take.avi")
            }),
        };
        let json = serde_json::to_string(&running).expect("serialize");
        assert!(json.contains("\"frames_written\":7"), "{json}");
        assert!(json.contains("\"format\":\"avi\""), "{json}");
        assert_eq!(
            serde_json::from_str::<RecordStatus>(&json).expect("deserialize"),
            running
        );
    }

    #[test]
    fn a_request_that_did_not_ask_to_wait_still_parses_from_a_document_written_before_the_flag() {
        // D12's flag, `#[serde(default)]` for `PhotoRequest::wait`'s reason: a request
        // written before the field existed still parses and still means what it meant.
        // Asserted rather than trusted, because `#[serde(default)]` removed from one field
        // is a change nothing else in this workspace would notice.
        let older = r#"{"sink":{"kind":"server_path","path":"/tmp/take.avi"}}"#;
        let parsed: RecordRequest = serde_json::from_str(older).expect("deserialize");
        assert!(!parsed.wait, "the default is a prompt refusal, not a wait");
        assert_eq!(parsed.sink, to_path("/tmp/take.avi").sink);

        let waiting = RecordRequest {
            wait: true,
            ..to_path("/tmp/take.avi")
        };
        let json = serde_json::to_string(&waiting).expect("serialize");
        assert!(json.contains("\"wait\":true"), "{json}");
        assert_eq!(
            serde_json::from_str::<RecordRequest>(&json).expect("deserialize"),
            waiting
        );
    }

    #[test]
    fn a_report_with_no_stream_stats_is_refused_rather_than_read_as_a_zeroed_take() {
        // AGENTS rules 6 and 7, and the inverse of the test above: `RecordRequest::wait` is a
        // *request* field a caller may legitimately omit, and this is an *answer* field no
        // producer omits. `#[serde(default)]` on it would let a document that carries no
        // delivery accounting deserialize into one that carries an accounting of nothing —
        // and a `DeviceQuiet` take really does answer all-zeros, so the two would be the same
        // document and a consumer proving a forwarded camera could not tell them apart.
        //
        // Asserted rather than trusted, because a `#[serde(default)]` added to one field is a
        // change nothing else in this workspace would notice (note **N291**).
        let full = RecordReport {
            camera: CameraId::parse("cam:test").expect("a literal id"),
            started_at: Stamp::epoch(),
            path: "/tmp/take.avi".into(),
            format: VideoFormat::Avi,
            negotiated: running_take("/tmp/take.avi").negotiated,
            summary: RecordingSummary {
                frames_written: 0,
                bytes_written: 0,
                declared_interval_us: 33_333,
                interval_source: IntervalSource::Negotiated,
                span_us: None,
                dropped_frames: 0,
                cap_reached: None,
            },
            // Exactly what a take that delivered nothing answers, which is the point: this
            // value and the serde default are indistinguishable, so only the field's
            // *presence* can tell a reader which of them it is looking at.
            stats: StreamStats::default(),
            wall_clock_ms: 240,
            ended: RecordingEnd::DeviceQuiet,
        };

        let doc = serde_json::to_string(&full).expect("serialize");
        assert!(
            doc.contains("\"stats\""),
            "the answer always carries its stats: {doc}"
        );
        assert_eq!(
            serde_json::from_str::<RecordReport>(&doc).expect("deserialize"),
            full
        );

        // The inverse, driven from the same document rather than hand-written, so the
        // fixture and the thing under test cannot drift apart.
        let mut value: serde_json::Value = serde_json::from_str(&doc).expect("a document");
        assert!(
            value
                .as_object_mut()
                .expect("an object")
                .remove("stats")
                .is_some(),
            "the fixture must actually have had the key this arm removes"
        );
        let refusal = serde_json::from_value::<RecordReport>(value)
            .expect_err("a report with no stats parsed as a take that delivered nothing");
        assert!(
            refusal.to_string().contains("stats"),
            "the refusal must name the field a consumer has to go and look for: {refusal}"
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
