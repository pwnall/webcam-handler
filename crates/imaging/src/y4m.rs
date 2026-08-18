//! The D7 raw fallback: YUV4MPEG2, one text header and then the device's own samples.
//!
//! D7's sentence is the whole specification of this module: *"Raw fallback: Y4M (y4m crate)
//! for YUYV/GREY cameras and pipeline use (mono and 4:2:x are both in the Y4M vocabulary) —
//! enormous but exact."* A GREY camera has no MJPEG mode and the AVI muxer carries MJPEG
//! only, so without this a whole class of device — the Chicony IR camera is one — could take
//! photographs and never a recording.
//!
//! ## Every byte written here is the device's own, rearranged and not re-encoded
//!
//! GREY becomes `Cmono`, YUYV becomes planar `C422`, NV12 becomes planar `C420`, and all
//! three are **lossless rearrangements**: the samples that reach the file are the samples the
//! driver delivered, in a different order, with row padding dropped. Nothing here converts
//! colour, resamples chroma or quantises anything.
//!
//! That matters for the reason E6 makes byte fidelity the product, and the notes' Expected
//! usage item 10 says it for video specifically: *"a re-encode inserts motion artefacts
//! exactly where the agent is looking for them. A transcoding step would fabricate evidence
//! about smoothness, which is the one property being judged."* A Y4M take is enormous — a
//! 1080p YUYV frame is 4.1 MB, so [`schema::limits::MAX_RECORDING_BYTES`] is about seventeen
//! seconds of it — and that is the trade D7 already made: exact and large beats small and
//! plausible, because the recording exists to be measured.
//!
//! The one thing this module *asserts* rather than measures is colour convention. Its
//! samples are BT.601 limited range, and until 2026-08-16 this paragraph also said its 4:2:0
//! chroma siting was "unstated (`C420` rather than `C420mpeg2` or `C420jpeg`)". **That was
//! false, and note N200 carries the measurement**: `ffprobe` and `mpv` both read `C420` as
//! exactly the siting `C420jpeg` names, so the tag that was chosen for saying nothing was
//! saying *centred*. There is no spelling of 4:2:0 in this vocabulary that declines to name a
//! siting, so the siting became a capture-time input — [`Y4mParams::chroma_siting`], derived
//! by [`schema::capture::ChromaSiting::of`], defaulting to what this writer has always
//! emitted — rather than a constant under a comment claiming neutrality (owner ruling,
//! 2026-08-16).
//!
//! What the header still declines to state, it declines honestly: there is no `A`
//! pixel-aspect field, because V4L2 does not tell us the pixel aspect and `A1:1` would be a
//! claim rather than a reading (AGENTS rule 6 — represent the unknown, never correct it).
//! And V4L2's `colorspace`/`ycbcr_enc`/`quantization`/`xfer_func` signalling is still not
//! plumbed through [`schema::capture::NegotiatedStream`], which is why the *range* remains a
//! stated assumption — [`crate::decode`]'s header records the same one for the same reason.
//!
//! ## The rate, which is measured at close since P6d because an oracle said it could be
//!
//! [`crate::avi::write::AviWriter::finish`] rewrites AVI's header rate to the **measured**
//! mean interval, because D7's CFR carve-out asks for it and because AVI's rate lives in
//! fixed-width binary fields that `finish` can seek back to and overwrite. **This container now
//! does the same thing**, and both go through the one law in `crate::video::declared_interval`.
//!
//! It did not, until P6d, and the reason is worth keeping because it is what a measurement
//! replaced. A Y4M header is one line of **text**, written before the first frame, and
//! `F1000000:33333` is one byte shorter than `F1000000:133333` — so patching a measured mean
//! into it means either rewriting the whole file (gigabytes, for this container) or padding the
//! denominator to a fixed width. Note **N106** marked that padding `declared` in this
//! repository's sense and named exactly what would make it `measured`: *"the ffprobe/mpv oracle
//! rung docs/7 P6d is about to build — a third party that has never read our code, handed a
//! file whose `F` field is zero-padded, answering whether it reads the rate we meant."*
//!
//! That measurement was taken on 2026-08-15 (evidence **E17**) and both third parties read the
//! padded field as the rate it names: `F1000000:0000050000` is `20/1` to `ffprobe` and 20 fps to
//! `mpv`, `F1000000:0000033333` is `1000000/33333` and 30.0003 fps, and neither differs from the
//! unpadded spelling of the same ratio. So the denominator is zero-padded to
//! `RATE_DENOMINATOR_DIGITS` — wide enough for every `u32` interval, which is what makes it
//! *fixed*-width rather than usually-wide-enough — the file's length never moves, and
//! [`Y4mWriter::finish`] patches the digits in place exactly as the AVI muxer patches its two
//! binary homes. [`RecordingSummary::interval_source`] is [`IntervalSource::Measured`] when a
//! mean was measured, and note **N106**'s amendment records what changed.
//!
//! **The header a crash leaves is still honest**, which is why the padding is written at
//! [`Y4mWriter::begin`] with the negotiated interval in it rather than left blank for the close
//! to fill: a take that dies before `finish` leaves a file declaring what the camera was asked
//! for, labelled `Negotiated` by nothing that survives — and a reader with no summary beside it
//! gets a plausible rate rather than `F1000000:0000000000`, which is a division by zero to every
//! parser.
//!
//! The measurement is carried whichever way that goes: [`RecordingSummary::span_us`] and
//! [`RecordingSummary::frames_written`] are observations in both containers and
//! [`RecordingSummary::measured_interval_us`] is the subtraction, so an agent asking "did this
//! take 200 ms or 2 s" was answered from observed numbers even while the header could not be.
//! What P6d added is the *file alone*, handed to a player with no summary beside it, playing
//! back at the rate it was captured at.
//!
//! ## A driver is untrusted input, on this side of the boundary too
//!
//! `width`, `height`, `bytes_per_line` and `bytes.len()` all arrive from a kernel driver
//! (rubric B10). Every plane extracted below is sized from checked arithmetic and the buffer
//! is measured against it *before* a byte is read, using [`crate::decode`]'s stride and
//! plane-size functions — the same law, one home, because "what does `bytes_per_line: 0`
//! mean" must have one answer in this crate.
//!
//! The **geometry the format demands** is checked as well, and it is a different check: 4:2:2
//! halves the horizontal chroma resolution and 4:2:0 halves both, so an odd extent on a
//! subsampled axis has no plane size at all. Rather than invent a half sample this refuses at
//! [`Y4mWriter::begin`], where a caller learns it before a file exists. Mono has no such
//! constraint and takes any extent.
//!
//! ## A crash leaves a file, and so does a full disk
//!
//! Y4M needs no recovery path because it has no structure to recover: the header is complete
//! before the first frame and every frame is self-delimiting, so a file cut anywhere is a
//! valid file of however many whole frames precede the cut. That is the same promise D7 makes
//! for AVI's `movi` stream, arrived at for free, and the close-time rate patch does not spend
//! it: the only byte range `finish` revises is `RATE_DENOMINATOR_DIGITS` digits that already
//! held a legal rate. The one obligation left is not to *lie* about it, which is why a sink
//! that refuses a write poisons this writer: after a partial `write_all` this module no longer
//! knows the file's length, so it refuses every later call and names the last length it was
//! sure of.
//!
//! ## Why the `y4m` crate is not linked here
//!
//! Design §2.8 pinned `y4m = "0.8.0"` for this module and it is not used; note **N107**
//! carries the measurement. In short: `y4m::Encoder` takes the sink **by value** and exposes
//! no accessor, no `into_inner` and no `flush`, so a recording built on it could not flush its
//! sink at close (which [`crate::avi::write::AviWriter::finish`] does), could not count the
//! bytes it wrote without wrapping the sink in a shared-counter adapter the encoder then owns
//! for ever, and could not hand the sink back. Its header's colorspace tag is also emitted as
//! `write!(w, " {:?}", colorspace)` — a `Debug` derive on a `#[non_exhaustive]` enum as the
//! file-format contract — which would make this module's frozen fixture a hostage to a
//! dependency's derive output.
//!
//! ## A frame may contain a person
//!
//! Rubric A12. No refusal here quotes a sample: lengths, offsets, counts, dimensions and
//! format names only, and [`Y4mWriter`]'s `Debug` is hand-written for the reason
//! `schema::capture::Frame`'s is — a derived one would print the plane buffers.

use std::io::{Seek, SeekFrom, Write};

use schema::camera::PixelFormat;
use schema::capture::{ChromaSiting, Frame};
use schema::error::Result;
use schema::video::{CapReached, IntervalSource, RecordingSummary};

use crate::decode::{geometry, plane_bytes, stride_of};
use crate::fault::{imaging_failure, short_buffer};
use crate::video::{FrameOutcome, PROVISIONAL_INTERVAL_US, RecordingCaps};

/// The magic that opens every YUV4MPEG2 file, trailing space included.
const MAGIC: &[u8] = b"YUV4MPEG2 ";

/// What precedes every frame's samples: the marker, and the newline that ends its (empty)
/// parameter list.
///
/// Six bytes, and they are the whole per-frame overhead of this container — against AVI's
/// eight-byte chunk header, its pad byte and its sixteen-byte `idx1` entry. The size cap's
/// projection reads this constant rather than the literal, so a per-frame parameter added
/// later cannot make the projection quietly wrong.
const FRAME_MARKER: &[u8] = b"FRAME\n";

/// `FRAME_MARKER`'s length, as the projection needs it.
const FRAME_HEADER_BYTES: u64 = 6;

/// The `F` ratio's numerator, fixed for the life of the file.
///
/// The frame *rate* is `F<num>:<den>` frames per second, so pinning the numerator at one
/// million makes the denominator **be** the frame interval in microseconds. That is the same
/// trick `crate::avi::write` plays with `strh.dwRate`, for the same two reasons: the declared
/// rate then keeps the microsecond resolution the driver's timestamps arrive at, and the
/// number in the file is the number in [`RecordingSummary::declared_interval_us`] rather than
/// a rounding of it.
///
/// The ratio is deliberately **not** reduced. `F1000000:33333` is uglier than `F30:1` and it
/// is also not 30 fps — 33 333 µs is 30.0003 fps — and a container whose whole argument is
/// "enormous but exact" does not round its own metadata to look tidy.
const MICROS_PER_SECOND: u32 = 1_000_000;

/// How many decimal digits the `F` ratio's denominator is padded to, for the life of the file.
///
/// **Ten, because [`Y4mParams::negotiated_interval_us`] is a `u32` and `u32::MAX` is
/// 4 294 967 295** — so every interval this writer can be handed fits, and the field is
/// *fixed*-width rather than usually-wide-enough. A width chosen for the intervals a webcam
/// actually offers would be a width some later device walks past, and the close-time patch
/// would then have to move the rest of the file: the whole point of the padding is that
/// [`Y4mWriter::finish`] rewrites digits and never lengths.
///
/// Zero-padding a decimal field is what note **N106** marked `declared` and P6d's oracle rung
/// measured (evidence **E17**): `ffprobe` 8.0.1 and `mpv` 0.41.0 both read
/// `F1000000:0000050000` as exactly the rate `F1000000:50000` names. The format description
/// permits decimal digits and says nothing about leading zeros either way, which is why this
/// needed a measurement rather than a reading.
const RATE_DENOMINATOR_DIGITS: usize = 10;

/// The smallest header this writer can emit, and therefore the smallest `max_bytes` a Y4M
/// recording can be given.
///
/// 44 bytes: `YUV4MPEG2 W2 H2 F1000000:0000000001 Ip C420\n`, which is the magic (10) plus the
/// shortest legal extent fields (`W2 H2`, 5 with its separators), the rate this writer emits
/// (`F1000000:` and `RATE_DENOMINATOR_DIGITS` digits, 19, since both the numerator and the
/// denominator's width are fixed), the interlacing tag (` Ip`, 3), the shortest colorspace tag
/// (` C420`, 5) and the terminator. A cap below it could never be honoured — the file would be
/// over budget before a frame arrived — and refusing at [`Y4mWriter::begin`] is the difference
/// between a caller learning that immediately and learning it from a file that broke its own
/// rule.
///
/// It was 35 before P6d, when the denominator was written unpadded and the shortest rate this
/// writer could express was `F1000000:1`. The nine bytes are what a header that can be revised
/// at close costs, paid once per file.
///
/// It is a *floor*, not the header's actual length: a real geometry makes the header longer,
/// and a cap between this and that is caught by the size cap the moment the header lands. Since
/// the rate field is fixed-width, the *interval* no longer moves it at all.
/// `the_smallest_header_this_writer_emits_is_the_documented_floor` is what keeps the number and
/// the builder from drifting apart.
pub const MIN_RECORDING_BYTES: u64 = 44;

// --- The refusal vocabulary ----------------------------------------------------------
//
// Named because the tests pin them: `is_err()` cannot tell "we refused the thing we seeded"
// from "we tripped over something else on the way there" (`crate::fault::SHORT_BUFFER_PREFIX`
// records where this habit came from).

/// The sink was opened for, or handed, a format Y4M does not carry.
const NOT_RAW: &str = "the Y4M sink carries raw planes only";
/// A frame's pixel format is not the one the stream was opened in.
const FORMAT_MISMATCH: &str = "but the stream was opened in";
/// A frame's geometry is not the stream's.
const EXTENT_MISMATCH: &str = "but the stream was opened at";
/// A frame carries no bytes.
const EMPTY_FRAME: &str = "carries no bytes";
/// The geometry is zero on an axis.
const ZERO_EXTENT: &str = "a recording needs a non-degenerate frame size";
/// The geometry is odd on an axis the colorspace subsamples.
const ODD_EXTENT: &str = "has no whole chroma sample on";
/// A count does not fit the width it must be computed in.
const NOT_ADDRESSABLE: &str = "does not fit";
/// `max_bytes` is below the size of an empty recording.
const CAP_TOO_SMALL: &str = "is smaller than the empty file";
/// A negotiated interval of zero microseconds.
const ZERO_INTERVAL: &str = "a negotiated frame interval of 0 microseconds";
/// The sink refused a write, and the file's length is no longer known.
const SINK_FAILED: &str = "the sink failed after";

const BEGIN_OP: &str = "begin Y4M recording";
const FRAME_OP: &str = "write Y4M frame";
const FINISH_OP: &str = "finish Y4M recording";

/// Which of Y4M's colorspaces a device format lands in.
///
/// Three of the vocabulary's fourteen, and the three are exactly D6's raw source formats —
/// the pairing `schema::video::VideoFormat::carries` states from the other end. An exhaustive
/// match everywhere it is read, so a fourth raw format cannot be added without deciding its
/// plane layout (AGENTS rule 6's shape).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Planar {
    /// GREY: one luma plane, no chroma at all.
    Mono,
    /// YUYV: 4:2:2, so chroma is half-width and full-height.
    C422,
    /// NV12: 4:2:0, so chroma is half-width and half-height.
    C420,
}

impl Planar {
    /// The header's `C` tag for this colorspace, at the siting the recording was opened
    /// with.
    ///
    /// **Only 4:2:0 has a siting to state, and it always states one.** Mono has no chroma at
    /// all, and 4:2:2's tag carries no siting in this vocabulary — measured, not assumed:
    /// `ffprobe` 8.0.1 reports `chroma_location=unspecified` for a `C422` file (evidence
    /// **N200**). So the argument runs out at `C420`, where every spelling names a siting and
    /// the one this writer used to emit unconditionally is [`ChromaSiting::Centred`]'s
    /// (note **N200**).
    const fn tag(self, siting: ChromaSiting) -> &'static str {
        match self {
            Planar::Mono => "mono",
            Planar::C422 => "422",
            // `420` and not `420jpeg` for the same reason `Ip` is written and `A1:1` is not:
            // the two spell the same siting to every reader measured, and `420` is the one
            // every file this project has already written carries. Changing it would move
            // the frozen fixtures for no gain.
            Planar::C420 => match siting {
                ChromaSiting::Centred => "420",
                ChromaSiting::HorizontallyCosited => "420mpeg2",
            },
        }
    }

    /// Which device format this colorspace is the rearrangement of.
    const fn of(format: PixelFormat) -> Option<Self> {
        match format {
            PixelFormat::GREY => Some(Planar::Mono),
            PixelFormat::YUYV => Some(Planar::C422),
            PixelFormat::NV12 => Some(Planar::C420),
            _ => None,
        }
    }
}

/// The frame extent and the plane sizes it implies, validated once so everything derived from
/// them afterwards is infallible.
#[derive(Debug, Clone, Copy)]
struct Layout {
    width: u32,
    height: u32,
    pixel_format: PixelFormat,
    planar: Planar,
    /// The siting the header will state, from [`Y4mParams::chroma_siting`].
    ///
    /// Kept beside the colorspace rather than read from the params at the one call site, so
    /// the header and the refusal that names `C{tag}` cannot spell the same stream two ways.
    siting: ChromaSiting,
    /// The luma plane, in bytes: `width * height` for every colorspace here.
    y_bytes: usize,
    /// **One** chroma plane, in bytes. Zero for mono, which has neither.
    chroma_bytes: usize,
}

impl Layout {
    /// What one frame costs on the sink: the marker and all three planes.
    fn frame_bytes(self) -> Option<u64> {
        let planes = self
            .y_bytes
            .checked_add(self.chroma_bytes)?
            .checked_add(self.chroma_bytes)?;
        u64::try_from(planes).ok()?.checked_add(FRAME_HEADER_BYTES)
    }
}

/// The three planes of one frame, kept across frames so a recording allocates once.
///
/// A recording is a loop, and three fresh `Vec`s per frame at 120 fps is three allocations and
/// three frees per frame for buffers whose size never changes. They are cleared and refilled
/// instead; `Vec::clear` keeps the capacity, so after the first frame this is a copy into
/// memory that is already there.
#[derive(Debug, Default)]
struct Planes {
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
}

/// Everything the Y4M sink needs before the first frame.
///
/// [`crate::avi::AviParams`] plus the negotiated pixel format, which AVI does not need because
/// it carries one format and Y4M has to choose a colorspace before the header is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Y4mParams {
    /// Frame width, as negotiated with the device.
    pub width: u32,
    /// Frame height, as negotiated with the device.
    pub height: u32,
    /// The raw format the device agreed to deliver.
    pub pixel_format: PixelFormat,
    /// The negotiated frame interval in microseconds, when the negotiation named one.
    pub negotiated_interval_us: Option<u32>,
    /// Which 4:2:0 chroma siting the header states (owner ruling, 2026-08-16; note
    /// **N200**).
    ///
    /// An input rather than a constant, because there is no `C` tag that declines to name a
    /// siting: `C420` *is* [`ChromaSiting::Centred`] to every reader measured, so a writer
    /// with the tag hard-coded was making a claim while its comment said it was not.
    /// `Default` is `Centred`, which is what this writer has always emitted and what
    /// [`ChromaSiting::of`] answers for a stream nothing could be asked about. Ignored for
    /// GREY and YUYV, whose tags carry no siting at all.
    pub chroma_siting: ChromaSiting,
    /// What the recording is allowed to cost.
    pub caps: RecordingCaps,
}

/// A Y4M recording in progress.
///
/// `W` is `Write + Seek` since P6d, for exactly one call: [`Y4mWriter::finish`] seeks back to
/// the header's fixed-width rate field and patches the measured mean into it. Everything else
/// this container does is forward-only, which is why the seek costs the recording nothing —
/// and [`crate::video::Recorder`] carried the bound already, because its other arm rewrites six
/// fields the same way.
pub struct Y4mWriter<W: Write + Seek> {
    sink: W,
    layout: Layout,
    caps: RecordingCaps,
    /// Where the sink's cursor was when the header was written.
    ///
    /// The rate patch seeks to an absolute position, and a caller may hand in a sink that is
    /// already part-way through a file — `crate::avi::write::AviWriter` keeps the same field
    /// for the same reason. A writer that assumed zero would patch somebody else's bytes.
    origin: u64,
    /// Where the `F` ratio's denominator starts, as an offset from [`Self::origin`].
    ///
    /// Produced by [`header_bytes`] rather than re-derived here: an offset computed twice is
    /// two answers that agree until somebody adds a header field, and what a wrong one writes
    /// is ten digits into the middle of `W`/`H` or of the colorspace tag.
    denominator_at: u64,
    /// The interval the header declares.
    ///
    /// The negotiated (or provisional) number from `begin` until [`Y4mWriter::finish`] patches
    /// the measured mean over it — so the file a crashed take leaves declares what the camera
    /// was asked for rather than nothing.
    declared_interval_us: u32,
    /// Where that number came from.
    interval_source: IntervalSource,
    planes: Planes,
    /// How many bytes of file this writer is *sure* are on the sink.
    bytes_written: u64,
    frames_written: u32,
    first_timestamp_us: Option<i64>,
    last_timestamp_us: i64,
    /// What the delivered frames say about how they were delivered (design D16).
    ///
    /// The same accumulator the AVI muxer pushes into, and for the same reason: the two
    /// sinks carried one `count_sequence` each, identical, and a rule stated twice is a rule
    /// that can be repaired once (design §2.10).
    stats: crate::stream_stats::Accumulator,
    cap_reached: Option<CapReached>,
    sink_failed: bool,
}

impl<W: Write + Seek> std::fmt::Debug for Y4mWriter<W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // A frame may contain a person (rubric A12), and both the sink and the plane buffers
        // hold one — so neither is printable here.
        f.debug_struct("Y4mWriter")
            .field("frames_written", &self.frames_written)
            .field("bytes_written", &self.bytes_written)
            .field("dropped_frames", &self.stats.frames_dropped())
            .field("cap_reached", &self.cap_reached)
            .field("sink_failed", &self.sink_failed)
            .finish_non_exhaustive()
    }
}

impl<W: Write + Seek> Y4mWriter<W> {
    /// Open a recording: validate the parameters, then write the whole header line.
    ///
    /// When this returns, the file on the sink is already a valid Y4M of zero frames — which
    /// is what makes a crashed recording readable without a recovery path.
    ///
    /// # Errors
    ///
    /// [`schema::Error::DeviceIo`] for a pixel format outside D6's raw set, a degenerate frame
    /// extent, an extent that is odd on an axis the colorspace subsamples, a negotiated
    /// interval of zero, a `max_bytes` below [`MIN_RECORDING_BYTES`], or a sink that refused
    /// the header.
    ///
    /// The format refusal is `DeviceIo` and not
    /// [`schema::Error::FormatUnsupported`] deliberately, and the two live one layer apart:
    /// *which container carries this stream* is a capability question and
    /// `schema::video::VideoFormat::resolve` answers it with `FormatUnsupported` before
    /// [`crate::video::Recorder::begin`] ever reaches this constructor. By the time a caller
    /// is here it has already chosen Y4M, so a format this sink cannot open is a caller
    /// bypassing the pairing rather than a device that cannot oblige — AGENTS rule 7's line,
    /// with the capability claim kept where the capability is decided.
    pub fn begin(mut sink: W, params: Y4mParams) -> Result<Self> {
        let layout = layout(
            params.width,
            params.height,
            params.pixel_format,
            params.chroma_siting,
        )?;
        if params.caps.max_bytes < MIN_RECORDING_BYTES {
            return Err(imaging_failure(
                BEGIN_OP,
                format!(
                    "a cap of {} bytes {CAP_TOO_SMALL}: the shortest header this writer emits \
                     is already {MIN_RECORDING_BYTES} bytes",
                    params.caps.max_bytes
                ),
            ));
        }
        if params.negotiated_interval_us == Some(0) {
            return Err(imaging_failure(
                BEGIN_OP,
                format!("{ZERO_INTERVAL} is not a frame rate"),
            ));
        }
        // What the header says *until* `finish` measures something. A take that never reaches
        // its close leaves this number in the file, which is why it is the negotiated one and
        // not a zero waiting to be filled in.
        let (declared_interval_us, interval_source) = match params.negotiated_interval_us {
            Some(interval) => (interval, IntervalSource::Negotiated),
            None => (PROVISIONAL_INTERVAL_US, IntervalSource::Provisional),
        };

        let header = header_bytes(layout, declared_interval_us);
        let bytes_written = u64::try_from(header.bytes.len()).map_err(|_| {
            imaging_failure(BEGIN_OP, format!("the header {NOT_ADDRESSABLE} in a u64"))
        })?;
        let denominator_at = u64::try_from(header.denominator_at).map_err(|_| {
            imaging_failure(
                BEGIN_OP,
                format!("the header's rate field offset {NOT_ADDRESSABLE} in a u64"),
            )
        })?;
        // Asked *before* the header lands, because `Seek::stream_position` on a sink that has
        // already been written to would report the end of what we just wrote. A sink that
        // cannot say where it is cannot be patched at close, and learning that here costs the
        // caller nothing; learning it at `finish` would cost it the whole recording.
        let origin = sink.stream_position().map_err(|err| {
            imaging_failure(
                BEGIN_OP,
                format!("the sink could not say where the header would start: {err}"),
            )
        })?;
        sink.write_all(&header.bytes)
            .map_err(|err| imaging_failure(BEGIN_OP, format!("{SINK_FAILED} 0 bytes: {err}")))?;

        // A header longer than the whole budget is the size cap reached at frame zero rather
        // than an error: the file that exists is a legitimate Y4M of no frames, the caller
        // asked for a bound and got one, and `finish` names which bound it was. The floor
        // above catches the caps that could never work at all; this catches the ones that
        // could not work for *this* geometry.
        let cap_reached = (bytes_written > params.caps.max_bytes).then_some(CapReached::Size);

        Ok(Y4mWriter {
            sink,
            layout,
            caps: params.caps,
            origin,
            denominator_at,
            declared_interval_us,
            interval_source,
            planes: Planes::default(),
            bytes_written,
            frames_written: 0,
            first_timestamp_us: None,
            last_timestamp_us: 0,
            stats: crate::stream_stats::Accumulator::new(),
            cap_reached,
            sink_failed: false,
        })
    }

    /// Append one frame as three planes.
    ///
    /// The samples reach the sink untouched: the planes below are a *reordering* of the
    /// driver's bytes with row padding dropped, never a conversion (E6, and the module doc's
    /// first section).
    ///
    /// Caps are checked **after** the frame is validated and **before** anything is written,
    /// so a refused frame leaves the file exactly as it was and the file never exceeds its
    /// bounds. The order is `Frames`, `Span`, `Size` — `crate::avi::write`'s order, and for
    /// its reasons: the frame count is the caller's own number, the span is about what the
    /// recording *means*, and the size projection is the most derived of the three.
    ///
    /// # Errors
    ///
    /// [`schema::Error::DeviceIo`] for a pixel format that is not the stream's, a geometry
    /// that is not the stream's, an empty payload, a payload shorter than that geometry
    /// demands, or a sink that refused a write. A cap is **not** an error: it is
    /// [`FrameOutcome::Refused`].
    pub fn write_frame(&mut self, frame: &Frame) -> Result<FrameOutcome> {
        if let Some(cap) = self.cap_reached {
            return Ok(FrameOutcome::Refused(cap));
        }
        self.check_sink(FRAME_OP)?;
        self.fill_planes(frame)?;
        if let Some(cap) = self.cap_for(frame.timestamp_us) {
            self.cap_reached = Some(cap);
            return Ok(FrameOutcome::Refused(cap));
        }

        let next_count = self.frames_written.checked_add(1).ok_or_else(|| {
            imaging_failure(
                FRAME_OP,
                format!(
                    "one more than {} frames {NOT_ADDRESSABLE} in a frame count",
                    self.frames_written
                ),
            )
        })?;

        self.put_marker()?;
        self.put_planes()?;

        self.frames_written = next_count;
        self.stats.push(frame.sequence, frame.timestamp_us);
        if self.first_timestamp_us.is_none() {
            self.first_timestamp_us = Some(frame.timestamp_us);
        }
        self.last_timestamp_us = frame.timestamp_us;
        Ok(FrameOutcome::Written)
    }

    /// Patch the header's rate to what this take measured, flush the sink, and report what the
    /// recording turned out to be.
    ///
    /// D7's CFR carve-out, arriving at this container at P6d. The decision is
    /// `crate::video::declared_interval`'s and not this method's, so the two muxers cannot
    /// disagree about when a mean counts as measured; what is this method's is where the digits
    /// go. The patch is unconditional — the same digits are rewritten when nothing was measured
    /// — because a branch that skipped it would be a second opinion about whether the header
    /// and the summary agree, and the two are one claim.
    ///
    /// The file's **length does not move**: `RATE_DENOMINATOR_DIGITS` digits are overwritten
    /// with `RATE_DENOMINATOR_DIGITS` digits, which is the whole reason the field is padded.
    /// The sink is left at the end of the file rather than in the middle of the header, for
    /// `crate::avi::write::AviWriter::finish`'s reason: a caller holding another handle to it
    /// finds what it expects.
    ///
    /// The flush is not a formality: a caller writing through a `BufWriter` has bytes that are
    /// not on the disk yet, and `crate::avi::write`'s close flushes for the same reason, so a
    /// [`crate::video::Recorder`] must not have one arm that does and one that does not.
    ///
    /// # Errors
    ///
    /// [`schema::Error::DeviceIo`] when the sink refuses the seek, the patch or the flush, or
    /// when it had already failed — in which case the message names the length of the prefix
    /// that is still a readable file.
    pub fn finish(mut self) -> Result<RecordingSummary> {
        self.check_sink(FINISH_OP)?;
        let (declared_interval_us, interval_source, span_us) = crate::video::declared_interval(
            self.first_timestamp_us,
            self.last_timestamp_us,
            self.frames_written,
            self.negotiated_interval_us(),
        );
        self.patch_rate(declared_interval_us)?;
        if let Err(err) = self.sink.flush() {
            return Err(self.poison(FINISH_OP, &err));
        }
        Ok(RecordingSummary {
            frames_written: self.frames_written,
            bytes_written: self.bytes_written,
            declared_interval_us,
            interval_source,
            dropped_frames: self.stats.frames_dropped(),
            span_us,
            cap_reached: self.cap_reached,
        })
    }

    /// What the *negotiation* named, recovered from what `begin` decided.
    ///
    /// `begin` keeps the resolved pair rather than the caller's `Option`, because the header it
    /// wrote needed a number; this turns it back into the question
    /// `crate::video::declared_interval` asks. A `Provisional` header means the negotiation
    /// named nothing, and saying so is what keeps a take that measured nothing from reporting
    /// [`IntervalSource::Negotiated`] over a placeholder.
    fn negotiated_interval_us(&self) -> Option<u32> {
        match self.interval_source {
            IntervalSource::Provisional => None,
            IntervalSource::Negotiated | IntervalSource::Measured => {
                Some(self.declared_interval_us)
            }
        }
    }

    /// Overwrite the header's rate denominator in place.
    fn patch_rate(&mut self, interval_us: u32) -> Result<()> {
        let digits = denominator_digits(interval_us);
        let at = self.origin.saturating_add(self.denominator_at);
        if let Err(err) = self.sink.seek(SeekFrom::Start(at)) {
            return Err(self.poison(FINISH_OP, &err));
        }
        if let Err(err) = self.sink.write_all(digits.as_bytes()) {
            return Err(self.poison(FINISH_OP, &err));
        }
        // Back to the end, so the sink is where a caller left it rather than inside the header.
        let end = self.origin.saturating_add(self.bytes_written);
        if let Err(err) = self.sink.seek(SeekFrom::Start(end)) {
            return Err(self.poison(FINISH_OP, &err));
        }
        Ok(())
    }

    /// Refill [`Planes`] from this frame, refusing anything the stream did not agree to.
    fn fill_planes(&mut self, frame: &Frame) -> Result<()> {
        if frame.pixel_format != self.layout.pixel_format {
            return Err(imaging_failure(
                FRAME_OP,
                format!(
                    "a {} frame arrived, {FORMAT_MISMATCH} {}",
                    frame.pixel_format, self.layout.pixel_format
                ),
            ));
        }
        if frame.width != self.layout.width || frame.height != self.layout.height {
            return Err(imaging_failure(
                FRAME_OP,
                format!(
                    "a {}x{} frame arrived, {EXTENT_MISMATCH} {}x{}",
                    frame.width, frame.height, self.layout.width, self.layout.height
                ),
            ));
        }
        if frame.bytes.is_empty() {
            return Err(imaging_failure(
                FRAME_OP,
                format!("frame {} {EMPTY_FRAME}", frame.sequence),
            ));
        }

        self.planes.y.clear();
        self.planes.u.clear();
        self.planes.v.clear();
        let (width, height) = geometry(frame.width, frame.height, FRAME_OP)?;
        match self.layout.planar {
            Planar::Mono => fill_mono(
                &frame.bytes,
                width,
                height,
                frame.bytes_per_line,
                &mut self.planes,
            ),
            Planar::C422 => fill_c422(
                &frame.bytes,
                width,
                height,
                frame.bytes_per_line,
                &mut self.planes,
            ),
            Planar::C420 => fill_c420(
                &frame.bytes,
                width,
                height,
                frame.bytes_per_line,
                &mut self.planes,
            ),
        }?;

        // The planes were sized from the *stream's* layout and filled from *this frame's*
        // geometry, and the two were just asserted equal — so a disagreement here is a defect
        // in one of the three fills rather than anything a driver did. It is checked anyway,
        // because a plane that is short by a row writes a file whose frames are silently
        // misaligned from that point on, and nothing downstream would notice.
        let expected = (self.layout.y_bytes, self.layout.chroma_bytes);
        let actual = (self.planes.y.len(), self.planes.u.len());
        if actual != expected || self.planes.v.len() != self.layout.chroma_bytes {
            return Err(imaging_failure(
                FRAME_OP,
                format!(
                    "the extracted planes are {}/{}/{} bytes and this stream's are {}/{}/{}",
                    self.planes.y.len(),
                    self.planes.u.len(),
                    self.planes.v.len(),
                    self.layout.y_bytes,
                    self.layout.chroma_bytes,
                    self.layout.chroma_bytes
                ),
            ));
        }
        Ok(())
    }

    /// Which cap this frame would break, if any. Nothing is written when this is `Some`.
    fn cap_for(&self, timestamp_us: i64) -> Option<CapReached> {
        if self.frames_written >= self.caps.max_frames {
            return Some(CapReached::Frames);
        }
        if let Some(first) = self.first_timestamp_us {
            // A clock that ran backwards produces no elapsed time rather than a negative one:
            // the span cap bounds how long a recording ran, and "before it started" is not a
            // length. `crate::avi::write` takes the identical view.
            let elapsed = timestamp_us
                .checked_sub(first)
                .and_then(|delta| u64::try_from(delta).ok())
                .unwrap_or(0);
            if u128::from(elapsed) > self.caps.max_span.as_micros() {
                return Some(CapReached::Span);
            }
        }
        match self
            .layout
            .frame_bytes()
            .and_then(|cost| self.bytes_written.checked_add(cost))
        {
            // An arithmetic overflow here is a file far past any cap a caller could state, so
            // the size cap is the honest answer rather than an error.
            None => Some(CapReached::Size),
            Some(projected) if projected > self.caps.max_bytes => Some(CapReached::Size),
            Some(_) => None,
        }
    }

    /// How the frames written so far were delivered (design D16).
    ///
    /// The accumulator's own answer, handed out whole. `RecordingSummary::dropped_frames` is
    /// one number out of this same record, which is why the two can never disagree.
    #[must_use]
    pub fn stats(&self) -> schema::video::StreamStats {
        self.stats.stats()
    }

    fn put_marker(&mut self) -> Result<()> {
        self.put(FRAME_MARKER)
    }

    fn put_planes(&mut self) -> Result<()> {
        // Taken out of `self` for the write, because `put` needs `&mut self` and the planes
        // are borrowed from it. `std::mem::take` leaves an empty `Vec` behind and the buffers
        // are put back immediately, so the capacity survives the round trip and the loop still
        // allocates once per recording.
        let planes = std::mem::take(&mut self.planes);
        let result = self
            .put(&planes.y)
            .and_then(|()| self.put(&planes.u))
            .and_then(|()| self.put(&planes.v));
        self.planes = planes;
        result
    }

    /// Append bytes, and remember that they landed.
    fn put(&mut self, bytes: &[u8]) -> Result<()> {
        let count = u64::try_from(bytes.len()).map_err(|_| {
            imaging_failure(
                FRAME_OP,
                format!("a {}-byte write {NOT_ADDRESSABLE} in a u64", bytes.len()),
            )
        })?;
        match self.sink.write_all(bytes) {
            Ok(()) => {
                self.bytes_written = self.bytes_written.saturating_add(count);
                Ok(())
            }
            Err(err) => Err(self.poison(FRAME_OP, &err)),
        }
    }

    /// Record that the sink failed, and say how much file this writer was still sure of.
    ///
    /// One home for the bookkeeping, because a path that reported the failure without setting
    /// the flag would let the next call write into a file of unknown length. `write_all` does
    /// not say how many bytes landed before an error, so the number below is the last length
    /// this writer *knew*, and the prefix it names is a file a reader can still walk to its
    /// last whole frame.
    fn poison(&mut self, operation: &str, err: &std::io::Error) -> schema::error::Error {
        self.sink_failed = true;
        imaging_failure(
            operation,
            format!("{SINK_FAILED} {} bytes: {err}", self.bytes_written),
        )
    }

    /// Refuse to touch a sink that has already failed.
    fn check_sink(&self, operation: &str) -> Result<()> {
        if self.sink_failed {
            return Err(imaging_failure(
                operation,
                format!(
                    "{SINK_FAILED} {} bytes, so the file's length is no longer known; the \
                     first {} bytes are a readable Y4M of whole frames",
                    self.bytes_written, self.bytes_written
                ),
            ));
        }
        Ok(())
    }
}

/// Validate a frame extent against the colorspace it has to be described in.
///
/// Two refusals, and they are different claims. A zero axis has no pixels at all. An **odd**
/// axis has pixels and no whole chroma sample on the axis the colorspace subsamples — 4:2:2
/// pairs pixels horizontally and 4:2:0 pairs them on both axes — and the honest answer is to
/// refuse rather than to pad the frame with an invented sample or to truncate it and report a
/// size the caller did not ask for (AGENTS rule 6). Mono subsamples nothing and takes any
/// extent, which is why the check is per-colorspace rather than blanket.
fn layout(
    width: u32,
    height: u32,
    pixel_format: PixelFormat,
    siting: ChromaSiting,
) -> Result<Layout> {
    let Some(planar) = Planar::of(pixel_format) else {
        return Err(imaging_failure(
            BEGIN_OP,
            format!("{NOT_RAW}, and this stream is {pixel_format}"),
        ));
    };
    let (w, h) = geometry(width, height, BEGIN_OP).map_err(|_| {
        imaging_failure(
            BEGIN_OP,
            format!("{ZERO_EXTENT}, and this one is {width}x{height}"),
        )
    })?;
    let odd_axis = match planar {
        Planar::Mono => None,
        Planar::C422 => (width % 2 == 1).then_some("its horizontal axis"),
        Planar::C420 => match (width % 2 == 1, height % 2 == 1) {
            (true, true) => Some("either axis"),
            (true, false) => Some("its horizontal axis"),
            (false, true) => Some("its vertical axis"),
            (false, false) => None,
        },
    };
    if let Some(axis) = odd_axis {
        return Err(imaging_failure(
            BEGIN_OP,
            format!(
                "a {width}x{height} frame {ODD_EXTENT} {axis}, which C{} subsamples",
                planar.tag(siting)
            ),
        ));
    }

    let y_bytes = w.checked_mul(h).ok_or_else(|| {
        imaging_failure(
            BEGIN_OP,
            format!("a {width}x{height} luma plane {NOT_ADDRESSABLE} in this address space"),
        )
    })?;
    let chroma_bytes = match planar {
        Planar::Mono => 0,
        Planar::C422 => w / 2 * h,
        Planar::C420 => w / 2 * (h / 2),
    };
    Ok(Layout {
        width,
        height,
        pixel_format,
        planar,
        siting,
        y_bytes,
        chroma_bytes,
    })
}

/// A header line and the one offset inside it that [`Y4mWriter::finish`] revises.
///
/// Two fields rather than a `Vec<u8>` and a second function that re-derives the offset: the
/// bytes and the position are produced by the same expression, so a header field added in front
/// of the rate moves both at once. An offset computed twice is what writes ten digits into the
/// middle of `W64`.
struct Header {
    bytes: Vec<u8>,
    /// Where the `F` ratio's denominator starts, counted from the first byte of the header.
    denominator_at: usize,
}

/// The `F` ratio's denominator, zero-padded to `RATE_DENOMINATOR_DIGITS`.
///
/// One home for the spelling, because the header writes it and the close-time patch overwrites
/// it, and a patch that formatted the number differently from the header would corrupt exactly
/// the field it was revising.
fn denominator_digits(interval_us: u32) -> String {
    format!("{interval_us:0width$}", width = RATE_DENOMINATOR_DIGITS)
}

/// The whole header line, and where its rate field sits.
///
/// `YUV4MPEG2 W<w> H<h> F<num>:<den> Ip C<tag>\n`, and every field in it is something this
/// build actually knows:
///
/// - `W`/`H` are the negotiated extent.
/// - `F` is the declared interval, as the reciprocal ratio [`MICROS_PER_SECOND`] argues for,
///   with the denominator zero-padded to `RATE_DENOMINATOR_DIGITS` so that close can revise
///   it without moving a byte after it.
/// - `Ip` is progressive. Webcams deliver progressive frames and V4L2's `field` for a capture
///   buffer says so; stating it keeps a player from guessing interlaced from the geometry.
/// - `C` is the colorspace, and nothing else. There is deliberately **no** `A` pixel-aspect
///   field: V4L2 does not report a pixel aspect, the format's own default for a missing `A` is
///   "unknown", and writing `A1:1` would turn an absence into an assertion.
fn header_bytes(layout: Layout, interval_us: u32) -> Header {
    let mut out = Vec::with_capacity(64);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(
        format!(
            "W{} H{} F{}:",
            layout.width, layout.height, MICROS_PER_SECOND
        )
        .as_bytes(),
    );
    // Read off the buffer rather than added up from the field widths above, so the two cannot
    // disagree about how long `W64 H48 F1000000:` is.
    let denominator_at = out.len();
    out.extend_from_slice(denominator_digits(interval_us).as_bytes());
    out.extend_from_slice(format!(" Ip C{}\n", layout.planar.tag(layout.siting)).as_bytes());
    Header {
        bytes: out,
        denominator_at,
    }
}

/// GREY to a mono plane: drop each row's stride padding and keep every sample.
///
/// The de-strided copy [`crate::decode::decode_grey`] performs, written here rather than
/// called for one reason that is about allocation and not about the law: that function builds
/// an `ImageBuffer` and returns an owned `Vec` per call, and this one refills a buffer the
/// recording already owns. The *arithmetic* is shared — [`crate::decode::stride_of`] and
/// [`crate::decode::plane_bytes`] are the same two functions — so "what does
/// `bytes_per_line: 0` mean" still has one answer in this crate.
fn fill_mono(
    bytes: &[u8],
    width: usize,
    height: usize,
    bytes_per_line: u32,
    planes: &mut Planes,
) -> Result<()> {
    let stride = stride_of(bytes_per_line, width, FRAME_OP)?;
    let needed = plane_bytes(stride, height, width).ok_or_else(|| overflowed(width, height))?;
    if bytes.len() < needed {
        return Err(short_buffer(FRAME_OP, needed, bytes.len()));
    }
    copy_rows(bytes, stride, height, width, &mut planes.y, needed)
}

/// Packed YUYV to three planes.
///
/// One row is `Y0 U Y1 V Y0 U Y1 V …`: two pixels share one chroma pair, which is what makes
/// this 4:2:2 and what makes an odd width unrepresentable ([`layout`] refuses it, so the
/// quads below are exact). Nothing is averaged, dropped or converted — the U taken here is the
/// U the sensor produced, at the position the format puts it.
fn fill_c422(
    bytes: &[u8],
    width: usize,
    height: usize,
    bytes_per_line: u32,
    planes: &mut Planes,
) -> Result<()> {
    let row_bytes = width
        .checked_mul(2)
        .ok_or_else(|| overflowed(width, height))?;
    let stride = stride_of(bytes_per_line, row_bytes, FRAME_OP)?;
    let needed = plane_bytes(stride, height, row_bytes).ok_or_else(|| overflowed(width, height))?;
    if bytes.len() < needed {
        return Err(short_buffer(FRAME_OP, needed, bytes.len()));
    }
    for row in bytes.chunks(stride).take(height) {
        let Some(useful) = row.get(..row_bytes) else {
            return Err(short_buffer(FRAME_OP, needed, bytes.len()));
        };
        for quad in useful.chunks_exact(4) {
            let [y0, u, y1, v] = quad else {
                return Err(short_buffer(FRAME_OP, needed, bytes.len()));
            };
            planes.y.push(*y0);
            planes.y.push(*y1);
            planes.u.push(*u);
            planes.v.push(*v);
        }
    }
    Ok(())
}

/// Semi-planar NV12 to three planes.
///
/// A Y plane, then a chroma plane of `Cb Cr Cb Cr …` at half resolution on both axes. The luma
/// is copied row by row and the chroma is de-interleaved; the UV plane's stride is the Y
/// plane's, which is what V4L2 lays out and what [`crate::decode::decode_nv12`] reads.
fn fill_c420(
    bytes: &[u8],
    width: usize,
    height: usize,
    bytes_per_line: u32,
    planes: &mut Planes,
) -> Result<()> {
    let y_stride = stride_of(bytes_per_line, width, FRAME_OP)?;
    let chroma_rows = height.div_ceil(2);
    // The UV plane's useful bytes per row: one Cb and one Cr per pixel pair, which for an even
    // width is the width itself. `div_ceil` is kept rather than simplified because it is the
    // same expression `decode_nv12` uses, and the two must not describe different planes.
    let chroma_row_bytes = width
        .div_ceil(2)
        .checked_mul(2)
        .ok_or_else(|| overflowed(width, height))?;
    let y_plane = y_stride
        .checked_mul(height)
        .ok_or_else(|| overflowed(width, height))?;
    let uv_plane = plane_bytes(y_stride, chroma_rows, chroma_row_bytes)
        .ok_or_else(|| overflowed(width, height))?;
    let needed = y_plane
        .checked_add(uv_plane)
        .ok_or_else(|| overflowed(width, height))?;
    if bytes.len() < needed {
        return Err(short_buffer(FRAME_OP, needed, bytes.len()));
    }
    let Some((luma, chroma)) = bytes.split_at_checked(y_plane) else {
        return Err(short_buffer(FRAME_OP, needed, bytes.len()));
    };

    copy_rows(luma, y_stride, height, width, &mut planes.y, needed)?;
    for row in chroma.chunks(y_stride).take(chroma_rows) {
        let Some(useful) = row.get(..chroma_row_bytes) else {
            return Err(short_buffer(FRAME_OP, needed, bytes.len()));
        };
        for pair in useful.chunks_exact(2) {
            let [cb, cr] = pair else {
                return Err(short_buffer(FRAME_OP, needed, bytes.len()));
            };
            planes.u.push(*cb);
            planes.v.push(*cr);
        }
    }
    Ok(())
}

/// Copy `rows` rows of `row_bytes` useful bytes out of a strided plane.
///
/// The one place stride padding is dropped, so a driver that pads its rows and one that does
/// not produce the same file. `needed` is carried only so the refusal can name the same two
/// numbers the length check named — a second, differently-worded short-buffer message for the
/// same condition is how a test stops being able to tell them apart.
fn copy_rows(
    bytes: &[u8],
    stride: usize,
    rows: usize,
    row_bytes: usize,
    out: &mut Vec<u8>,
    needed: usize,
) -> Result<()> {
    for row in bytes.chunks(stride).take(rows) {
        let Some(useful) = row.get(..row_bytes) else {
            return Err(short_buffer(FRAME_OP, needed, bytes.len()));
        };
        out.extend_from_slice(useful);
    }
    Ok(())
}

fn overflowed(width: usize, height: usize) -> schema::error::Error {
    imaging_failure(
        FRAME_OP,
        format!("frame geometry {width}x{height} overflows this machine's address space"),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::io::Cursor;
    use std::time::Duration;

    use super::*;
    use crate::decode::Decoded;
    use crate::encode;
    use crate::fixtures;

    const WIDTH: u32 = 64;
    const HEIGHT: u32 = 48;
    /// 15 fps, as microseconds — a rate a camera is asked for, and deliberately **not**
    /// [`PROVISIONAL_INTERVAL_US`], so a writer that dropped the negotiated number and wrote
    /// the placeholder in its place cannot pass this suite.
    const NEGOTIATED_US: u32 = 66_666;

    // ---------------------------------------------------------------- the independent parser
    //
    // Derived from the YUV4MPEG2 format description and **not** from `header_bytes` or the
    // fills above: it re-derives the plane sizes from the `C` tag it read rather than asking
    // this module for them, and it knows nothing about `MAGIC`, `FRAME_MARKER` or `Layout`.
    // That is the same posture `crate::avi::read` takes towards the AVI muxer, for the same
    // reason — a parser assembled from the writer's own helpers agrees with the writer by
    // construction and can only catch a typo.
    //
    // The `y4m` crate's decoder would **not** have served: it ships in the same file as the
    // encoder we would have been checking, so it agrees with it by construction on exactly the
    // questions worth asking (see note **N107**).
    //
    // What it refuses, each of which is a way this writer could be wrong and still produce
    // something that "looks like" a Y4M:
    //
    //   * a file that does not begin with `YUV4MPEG2 ` — including one that begins with
    //     `YUV4MPEG2` and no space;
    //   * a header with no `W`, no `H` or no `F`, or with a `W`/`H` that is not a positive
    //     decimal number;
    //   * an `F` ratio that is not `<digits>:<digits>`;
    //   * a `C` tag outside the three this project writes;
    //   * a frame whose marker is not exactly `FRAME` followed by `\n` or by ` params\n`;
    //   * a frame whose payload is shorter than the plane sizes the header implies — which is
    //     the truncated-tail case, reported as such rather than silently dropped;
    //   * trailing bytes that are neither a whole frame nor nothing at all.

    #[derive(Debug, PartialEq, Eq)]
    struct ParsedY4m {
        width: usize,
        height: usize,
        rate: (u64, u64),
        colorspace: String,
        interlacing: Option<String>,
        /// Every field of the header line after the magic, in order, so a test can assert
        /// what was written *and* that nothing else was.
        fields: Vec<String>,
        frames: Vec<ParsedFrame>,
    }

    #[derive(Debug, PartialEq, Eq)]
    struct ParsedFrame {
        y: Vec<u8>,
        u: Vec<u8>,
        v: Vec<u8>,
    }

    fn parse_y4m(bytes: &[u8]) -> std::result::Result<ParsedY4m, String> {
        let rest = bytes
            .strip_prefix(b"YUV4MPEG2 ")
            .ok_or("not a YUV4MPEG2 file")?;
        let end = rest
            .iter()
            .position(|b| *b == b'\n')
            .ok_or("the header line is unterminated")?;
        let line = std::str::from_utf8(&rest[..end]).map_err(|_| "the header is not UTF-8")?;
        let fields: Vec<String> = line.split(' ').map(str::to_owned).collect();

        let mut width = None;
        let mut height = None;
        let mut rate = None;
        let mut colorspace = None;
        let mut interlacing = None;
        for field in &fields {
            let (tag, value) = field.split_at(1);
            match tag {
                "W" => width = Some(value.parse::<usize>().map_err(|_| "W is not a number")?),
                "H" => height = Some(value.parse::<usize>().map_err(|_| "H is not a number")?),
                "F" => {
                    let (num, den) = value.split_once(':').ok_or("F is not a ratio")?;
                    rate = Some((
                        num.parse::<u64>().map_err(|_| "F numerator")?,
                        den.parse::<u64>().map_err(|_| "F denominator")?,
                    ));
                }
                "C" => colorspace = Some(value.to_owned()),
                "I" => interlacing = Some(value.to_owned()),
                "A" | "X" => {}
                other => return Err(format!("unknown header field {other}")),
            }
        }
        let width = width.ok_or("the header names no width")?;
        let height = height.ok_or("the header names no height")?;
        let rate = rate.ok_or("the header names no frame rate")?;
        if width == 0 || height == 0 {
            return Err("a zero extent is not a frame".to_owned());
        }
        let colorspace = colorspace.ok_or("the header names no colorspace")?;

        // The plane sizes, re-derived from the tag rather than asked for. Written as the
        // format describes them: mono has no chroma, 422 halves the horizontal chroma
        // resolution, 420 halves both.
        let (chroma_width, chroma_height) = match colorspace.as_str() {
            "mono" => (0, 0),
            "422" => (width.div_ceil(2), height),
            // Every 4:2:0 spelling this writer can emit, and they differ only in the siting
            // they declare — the plane sizes are identical, which is exactly why the tag was
            // mistaken for a free choice. The list is written out rather than matched by
            // prefix so a tag outside it is still refused.
            "420" | "420jpeg" | "420mpeg2" | "420paldv" => (width.div_ceil(2), height.div_ceil(2)),
            other => return Err(format!("unsupported colorspace {other}")),
        };
        let y_len = width * height;
        let chroma_len = chroma_width * chroma_height;

        let mut frames = Vec::new();
        let mut cursor = &rest[end + 1..];
        while !cursor.is_empty() {
            let body = cursor
                .strip_prefix(b"FRAME")
                .ok_or("trailing bytes that are not a frame")?;
            let marker_end = body
                .iter()
                .position(|b| *b == b'\n')
                .ok_or("a frame marker is unterminated")?;
            if marker_end != 0 && body.first() != Some(&b' ') {
                return Err("a frame marker carries parameters without a separator".to_owned());
            }
            let payload = &body[marker_end + 1..];
            let total = y_len + chroma_len + chroma_len;
            if payload.len() < total {
                return Err(format!(
                    "a frame's payload is {} bytes and the header implies {total}",
                    payload.len()
                ));
            }
            frames.push(ParsedFrame {
                y: payload[..y_len].to_vec(),
                u: payload[y_len..y_len + chroma_len].to_vec(),
                v: payload[y_len + chroma_len..total].to_vec(),
            });
            cursor = &payload[total..];
        }

        Ok(ParsedY4m {
            width,
            height,
            rate,
            colorspace,
            interlacing,
            fields,
            frames,
        })
    }

    // ---------------------------------------------------------------- fixtures and helpers

    fn generous_caps() -> RecordingCaps {
        RecordingCaps {
            max_bytes: 1 << 20,
            max_frames: 1024,
            max_span: Duration::from_secs(60),
        }
    }

    fn params(pixel_format: PixelFormat, caps: RecordingCaps) -> Y4mParams {
        Y4mParams {
            width: WIDTH,
            height: HEIGHT,
            pixel_format,
            negotiated_interval_us: Some(NEGOTIATED_US),
            chroma_siting: ChromaSiting::default(),
            caps,
        }
    }

    // The synthetic sample generators, and the reason they are here rather than
    // `crate::fixtures`.
    //
    // **`fixtures::pack_yuyv` and `fixtures::pack_nv12` write neutral chroma** — 128 in every
    // Cb and every Cr — which is right for the decode round trips they were built for and
    // useless here. Measured rather than assumed: a hand-applied mutant that swapped `u` and
    // `v` in `fill_c422` passed the **entire workspace suite** (1261 tests) while this module's
    // round trip used those packers, because every byte it compared was 128 either way. So the
    // three generators below give luma, Cb and Cr disjoint value ranges *and* make every
    // sample a function of its own position — which is what turns the round trip into an
    // assertion about plane identity, sample order and row placement rather than about
    // lengths.
    //
    // 251, 61 and 61 are primes, so a sample's value does not repeat along either axis inside
    // a 64x48 frame and a plane shifted by one row or one column is a plane full of wrong
    // numbers rather than a plane that happens to match.

    /// Luma at a pixel: the whole 0–250 range, and never in either chroma's range.
    fn luma(x: u32, y: u32) -> u8 {
        u8::try_from((x * 7 + y * 13) % 251).expect("under 251")
    }

    /// Cb at a chroma position: 64–124, disjoint from `cr`'s range.
    fn cb(x: u32, y: u32) -> u8 {
        u8::try_from(64 + (x * 3 + y * 5) % 61).expect("under 125")
    }

    /// Cr at a chroma position: 160–220, disjoint from `cb`'s range.
    fn cr(x: u32, y: u32) -> u8 {
        u8::try_from(160 + (x * 11 + y * 2) % 61).expect("under 221")
    }

    /// A synthetic raw frame, generated here and never captured (design §5).
    ///
    /// The layouts are transcribed from V4L2's own descriptions of the three formats, not
    /// from the fills under test: GREY is tightly packed luma, YUYV is `Y0 Cb Y1 Cr` per pixel
    /// pair, NV12 is a luma plane followed by `Cb Cr` at half resolution on both axes.
    fn raw_frame(pixel_format: PixelFormat, sequence: u32, timestamp_us: i64) -> Frame {
        let mut bytes = Vec::new();
        match pixel_format {
            PixelFormat::YUYV => {
                for y in 0..HEIGHT {
                    for pair in 0..WIDTH / 2 {
                        bytes.push(luma(pair * 2, y));
                        bytes.push(cb(pair, y));
                        bytes.push(luma(pair * 2 + 1, y));
                        bytes.push(cr(pair, y));
                    }
                }
            }
            PixelFormat::NV12 => {
                for y in 0..HEIGHT {
                    for x in 0..WIDTH {
                        bytes.push(luma(x, y));
                    }
                }
                for y in 0..HEIGHT / 2 {
                    for x in 0..WIDTH / 2 {
                        bytes.push(cb(x, y));
                        bytes.push(cr(x, y));
                    }
                }
            }
            _ => {
                for y in 0..HEIGHT {
                    for x in 0..WIDTH {
                        bytes.push(luma(x, y));
                    }
                }
            }
        }
        Frame {
            bytes,
            pixel_format,
            width: WIDTH,
            height: HEIGHT,
            bytes_per_line: 0,
            sequence,
            timestamp_us,
        }
    }

    /// The same frame a driver would deliver with `pad` bytes of stride padding on every
    /// row, and `last_row_padded` deciding whether the final row of the buffer carries its
    /// own padding.
    ///
    /// **Built by re-padding [`raw_frame`]'s rows rather than by a second generator**, so the
    /// planes it must produce are still `expected_planes`' and a test comparing the two is
    /// comparing a padded delivery against an unpadded one rather than against a second
    /// transcription of the layout.
    ///
    /// NV12's two planes are padded to the *same* stride, because that is what V4L2 lays out
    /// and what `fill_c420` computes the chroma plane's start from. `last_row_padded` is
    /// meaningful for the packed formats only: `fill_c420` locates the chroma plane at
    /// `y_stride × height`, so a luma plane missing its final padding is a different frame
    /// rather than the same one delivered tightly.
    fn padded_raw_frame(pixel_format: PixelFormat, pad: u32, last_row_padded: bool) -> Frame {
        let tight = raw_frame(pixel_format, 0, 0);
        let row_bytes = match pixel_format {
            PixelFormat::YUYV => WIDTH * 2,
            _ => WIDTH,
        };
        let rows = usize::try_from(match pixel_format {
            // Luma rows, then the interleaved chroma plane's half-height rows.
            PixelFormat::NV12 => HEIGHT + HEIGHT / 2,
            _ => HEIGHT,
        })
        .expect("fits");
        let row_bytes = usize::try_from(row_bytes).expect("fits");
        let mut bytes = Vec::new();
        for (index, row) in tight.bytes.chunks(row_bytes).enumerate() {
            bytes.extend_from_slice(row);
            if last_row_padded || index + 1 < rows {
                // 0xff on every padding byte: out of `luma`'s range (0–250) and out of both
                // chroma ranges, so a fill that let one through lands a value no generator
                // can produce rather than one that might match by luck.
                bytes.extend(std::iter::repeat_n(
                    0xffu8,
                    usize::try_from(pad).expect("fits"),
                ));
            }
        }
        Frame {
            bytes,
            bytes_per_line: row_bytes_u32(pixel_format) + pad,
            ..tight
        }
    }

    /// One row's *useful* bytes, in the width `bytes_per_line` is stated in.
    fn row_bytes_u32(pixel_format: PixelFormat) -> u32 {
        match pixel_format {
            PixelFormat::YUYV => WIDTH * 2,
            _ => WIDTH,
        }
    }

    fn record(
        params: Y4mParams,
        frames: &[Frame],
    ) -> (Vec<u8>, Vec<FrameOutcome>, RecordingSummary) {
        let mut file = Vec::new();
        let mut writer = Y4mWriter::begin(Cursor::new(&mut file), params).expect("begin");
        let outcomes = frames
            .iter()
            .map(|frame| writer.write_frame(frame).expect("write_frame"))
            .collect();
        let summary = writer.finish().expect("finish");
        (file, outcomes, summary)
    }

    fn refusal(result: &Result<impl std::fmt::Debug>) -> String {
        match result {
            Ok(value) => panic!("expected a refusal, got {value:?}"),
            Err(err) => err.to_string(),
        }
    }

    // ---------------------------------------------------------------- the tests

    #[test]
    fn the_smallest_header_this_writer_emits_is_the_documented_floor() {
        // `MIN_RECORDING_BYTES` is a literal, and a literal that drifted from the builder
        // would let `begin` accept a cap the header cannot fit under. 2x2 C420 is the shortest
        // header this writer can produce: the magic, the fixed `F` numerator and the padded
        // denominator's width are all constant, and every other field is at its minimum.
        let smallest = layout(2, 2, PixelFormat::NV12, ChromaSiting::default())
            .expect("2x2 NV12 is a legal extent");
        let header = header_bytes(smallest, 1);
        assert_eq!(
            u64::try_from(header.bytes.len()).expect("fits"),
            MIN_RECORDING_BYTES,
            "the header is {:?}",
            header.bytes
        );
        assert_eq!(
            header.bytes,
            b"YUV4MPEG2 W2 H2 F1000000:0000000001 Ip C420\n"
        );
    }

    #[test]
    fn the_rate_field_is_fixed_width_and_the_close_patch_is_aimed_at_it() {
        // Two claims that hold the close-time rewrite up, and each one is a way it could be
        // wrong while every round trip stayed green.
        //
        // **Fixed width**: the header's length must not depend on the interval, or `finish`
        // would have to move every byte after the rate. The four intervals below are 1, 5, 8
        // and 10 digits unpadded — including `u32::MAX`, which is the number
        // `RATE_DENOMINATOR_DIGITS` is sized for.
        let extent = layout(64, 48, PixelFormat::GREY, ChromaSiting::default()).expect("legal");
        let mut lengths = BTreeSet::new();
        for interval in [1, 33_333, 66_666_666, u32::MAX] {
            let header = header_bytes(extent, interval);
            lengths.insert(header.bytes.len());
            // **Aimed at it**: the offset must name the denominator's own digits, not the
            // numerator, not the `W`/`H` in front of it and not the ` Ip` behind it. A patch
            // one byte out writes into `:` or into a space and produces a file no parser
            // reads — which the round trips would catch, and an offset that is *inside* the
            // ten digits but not at their start would not.
            let at = header.denominator_at;
            assert_eq!(
                &header.bytes[at..at + RATE_DENOMINATOR_DIGITS],
                denominator_digits(interval).as_bytes(),
                "the offset does not name the denominator of {:?}",
                String::from_utf8_lossy(&header.bytes)
            );
            assert_eq!(header.bytes.get(at - 1), Some(&b':'));
            assert_eq!(header.bytes.get(at + RATE_DENOMINATOR_DIGITS), Some(&b' '));
        }
        assert_eq!(
            lengths.len(),
            1,
            "the header's length moved with the interval: {lengths:?}"
        );
    }

    #[test]
    fn every_raw_format_round_trips_through_a_parser_that_was_not_told_our_layout() {
        // The claim this module exists to make: the samples in the file are the samples the
        // driver delivered, rearranged. The comparison is against planes rebuilt *here* from
        // the packed fixture, so a fill that dropped a row, swapped U and V, or averaged a
        // chroma pair fails — and the parse is the independent one, so a header that named
        // the wrong colorspace fails too.
        for (pixel_format, tag) in [
            (PixelFormat::GREY, "mono"),
            (PixelFormat::YUYV, "422"),
            (PixelFormat::NV12, "420"),
        ] {
            let frames = [
                raw_frame(pixel_format, 0, 0),
                raw_frame(pixel_format, 1, 50_000),
            ];
            let (file, outcomes, summary) = record(params(pixel_format, generous_caps()), &frames);
            assert!(
                outcomes.iter().all(|o| *o == FrameOutcome::Written),
                "{tag}"
            );
            assert_eq!(summary.frames_written, 2, "{tag}");
            assert_eq!(
                summary.bytes_written,
                u64::try_from(file.len()).expect("fits"),
                "{tag}: the summary's byte count is the file's own length"
            );

            let parsed = parse_y4m(&file).unwrap_or_else(|err| panic!("{tag}: {err}"));
            assert_eq!(parsed.width, usize::try_from(WIDTH).expect("fits"), "{tag}");
            assert_eq!(
                parsed.height,
                usize::try_from(HEIGHT).expect("fits"),
                "{tag}"
            );
            assert_eq!(parsed.colorspace, tag);
            assert_eq!(parsed.interlacing.as_deref(), Some("p"), "{tag}");
            assert_eq!(
                parsed.rate,
                (1_000_000, 50_000),
                "{tag}: the denominator is the mean interval in microseconds, patched at close"
            );
            assert_eq!(
                parsed.fields,
                vec![
                    "W64".to_owned(),
                    "H48".to_owned(),
                    "F1000000:0000050000".to_owned(),
                    "Ip".to_owned(),
                    format!("C{tag}"),
                ],
                "{tag}: the header carries exactly the fields this build can justify — no A"
            );
            assert_eq!(parsed.frames.len(), 2, "{tag}");

            let expected = expected_planes(pixel_format);
            for (index, frame) in parsed.frames.iter().enumerate() {
                assert_eq!(frame.y, expected.y, "{tag}: luma of frame {index}");
                assert_eq!(frame.u, expected.u, "{tag}: Cb of frame {index}");
                assert_eq!(frame.v, expected.v, "{tag}: Cr of frame {index}");
            }
        }
    }

    /// The planes a frame in `pixel_format` must produce, built straight from the sample
    /// generators.
    ///
    /// It shares **nothing** with [`fill_c422`] and its siblings — not the packed buffer, not
    /// a helper, not an index expression. Each plane is written the way the Y4M format
    /// describes it (luma in raster order; one chroma plane each, at the resolution the
    /// colorspace subsamples to) and the fills have to arrive at the same answer from the
    /// packed side.
    fn expected_planes(pixel_format: PixelFormat) -> ParsedFrame {
        let (chroma_width, chroma_height) = match pixel_format {
            PixelFormat::YUYV => (WIDTH / 2, HEIGHT),
            PixelFormat::NV12 => (WIDTH / 2, HEIGHT / 2),
            _ => (0, 0),
        };
        let mut y = Vec::new();
        for row in 0..HEIGHT {
            for column in 0..WIDTH {
                y.push(luma(column, row));
            }
        }
        let mut u = Vec::new();
        let mut v = Vec::new();
        for row in 0..chroma_height {
            for column in 0..chroma_width {
                u.push(cb(column, row));
                v.push(cr(column, row));
            }
        }
        ParsedFrame { y, u, v }
    }

    #[test]
    fn the_parser_refuses_the_ways_this_writer_could_be_wrong() {
        // A parser that accepts everything proves nothing about the files it accepted, so its
        // refusals are asserted before it is trusted with the round trip above.
        let (good, _, _) = record(
            params(PixelFormat::GREY, generous_caps()),
            &[raw_frame(PixelFormat::GREY, 0, 0)],
        );
        assert!(parse_y4m(&good).is_ok());

        assert!(parse_y4m(b"").is_err(), "an empty file is not a Y4M");
        assert!(
            parse_y4m(b"YUV4MPEG2\n").is_err(),
            "the magic ends in a space"
        );
        assert!(
            parse_y4m(b"YUV4MPEG2 W64 H48 F1:1 Cmono").is_err(),
            "unterminated"
        );
        assert!(
            parse_y4m(b"YUV4MPEG2 H48 F1:1 Cmono\n").is_err(),
            "no width"
        );
        assert!(
            parse_y4m(b"YUV4MPEG2 W64 F1:1 Cmono\n").is_err(),
            "no height"
        );
        assert!(parse_y4m(b"YUV4MPEG2 W64 H48 Cmono\n").is_err(), "no rate");
        assert!(
            parse_y4m(b"YUV4MPEG2 W64 H48 F1 Cmono\n").is_err(),
            "F is not a ratio"
        );
        assert!(
            parse_y4m(b"YUV4MPEG2 W0 H48 F1:1 Cmono\n").is_err(),
            "zero width"
        );
        assert!(
            parse_y4m(b"YUV4MPEG2 W64 H48 F1:1 C444\n").is_err(),
            "we never write 444"
        );

        // A truncated tail is reported rather than dropped, which is the case a full disk
        // produces and the one a lenient parser would hide.
        let truncated = &good[..good.len() - 1];
        assert!(
            parse_y4m(truncated).is_err(),
            "a short final frame is a short file"
        );
        // And bytes that are not a frame at all.
        let mut trailing = good.clone();
        // A marker one byte short of the real one, spelled as a prefix of it rather than
        // written out: a four-letter truncation of `FRAME` reads as a typo to `just
        // hygiene`'s spell check, and a suppression for it would outlive the line it was for.
        trailing.extend_from_slice(&b"FRAME"[..4]);
        trailing.push(b'\n');
        assert!(parse_y4m(&trailing).is_err());
    }

    #[test]
    fn the_close_rewrites_the_header_to_the_mean_this_take_measured() {
        // What note **N106**'s amendment bought, asserted at the container that gained it. Two
        // frames 50 000 µs apart is a mean, the negotiated interval was 66 666, and **the file
        // itself** — read through the parser that was not told our layout — carries 50 000.
        //
        // Three assertions and not one, because the rewrite has three ways to be half done: a
        // summary that reports the mean over a header that still says 66 666 (which is the lie
        // N106 refused to tell before the patch existed), a header patched while the label still
        // says `Negotiated`, and a patch that moved the file's length.
        let frames = [
            raw_frame(PixelFormat::GREY, 0, 0),
            raw_frame(PixelFormat::GREY, 1, 50_000),
        ];
        let (file, _, summary) = record(params(PixelFormat::GREY, generous_caps()), &frames);
        assert_eq!(summary.interval_source, IntervalSource::Measured);
        assert_eq!(summary.declared_interval_us, 50_000);
        assert_eq!(summary.span_us, Some(50_000));
        assert_eq!(summary.measured_interval_us(), Some(50_000));
        assert_ne!(
            summary.declared_interval_us, NEGOTIATED_US,
            "the header still declares what the camera was asked for"
        );
        let parsed = parse_y4m(&file).expect("parses");
        assert_eq!(parsed.rate, (1_000_000, 50_000));
        assert_eq!(
            summary.bytes_written,
            u64::try_from(file.len()).expect("fits"),
            "the patch moved the file's length"
        );
        // The patched digits are the padded spelling and nothing else: a rewrite that wrote
        // `50000` over the first five digits would leave `50000` followed by the tail of the
        // old number, which parses as some other rate entirely.
        assert!(
            file.starts_with(b"YUV4MPEG2 W64 H48 F1000000:0000050000 Ip Cmono\n"),
            "{:?}",
            String::from_utf8_lossy(&file[..48.min(file.len())])
        );

        // A take that measured nothing leaves the negotiated number where `begin` put it — the
        // patch rewrites the same digits rather than skipping, so this is the arm that says the
        // rewrite is not unconditionally `Measured`.
        let (file, _, summary) = record(params(PixelFormat::GREY, generous_caps()), &frames[..1]);
        assert_eq!(summary.interval_source, IntervalSource::Negotiated);
        assert_eq!(summary.declared_interval_us, NEGOTIATED_US);
        assert_eq!(
            parse_y4m(&file).expect("parses").rate,
            (1_000_000, u64::from(NEGOTIATED_US))
        );

        // A negotiation that named nothing is `Provisional`, and the placeholder is what the
        // header carries — labelled, never dressed up as a finding. It is the arm that catches
        // a close which recovered the negotiated interval out of `declared_interval_us` without
        // remembering that there had not been one.
        let (file, _, summary) = record(
            Y4mParams {
                negotiated_interval_us: None,
                ..params(PixelFormat::GREY, generous_caps())
            },
            &frames[..1],
        );
        assert_eq!(summary.interval_source, IntervalSource::Provisional);
        assert_eq!(summary.declared_interval_us, PROVISIONAL_INTERVAL_US);
        assert_eq!(
            parse_y4m(&file).expect("parses").rate,
            (1_000_000, u64::from(PROVISIONAL_INTERVAL_US))
        );
    }

    #[test]
    fn a_recording_written_into_the_middle_of_a_sink_patches_its_own_header() {
        // `origin`, which nothing else in this suite exercises: every other test hands in a
        // fresh `Cursor` at position zero, where a writer that patched at an absolute offset of
        // `denominator_at` would be indistinguishable from one that patched at
        // `origin + denominator_at`. `crate::avi::write` keeps the same field, and the daemon
        // hands both containers a `File` it opened — a sink whose position is zero today and is
        // nobody's promise.
        let mut file = Cursor::new(Vec::new());
        let preamble = b"................";
        file.write_all(preamble).expect("preamble");
        let mut writer =
            Y4mWriter::begin(&mut file, params(PixelFormat::GREY, generous_caps())).expect("begin");
        for frame in [
            raw_frame(PixelFormat::GREY, 0, 0),
            raw_frame(PixelFormat::GREY, 1, 50_000),
        ] {
            writer.write_frame(&frame).expect("write_frame");
        }
        let summary = writer.finish().expect("finish");
        assert_eq!(summary.declared_interval_us, 50_000);

        let bytes = file.into_inner();
        assert!(bytes.starts_with(preamble), "the preamble was overwritten");
        let parsed = parse_y4m(&bytes[preamble.len()..]).expect("parses");
        assert_eq!(parsed.rate, (1_000_000, 50_000));
        assert_eq!(parsed.frames.len(), 2);
    }

    #[test]
    fn a_compressed_frame_is_refused_by_the_sink_that_carries_planes() {
        // The mirror image of `crate::avi::write`'s "the muxer carries MJPEG only". Both
        // halves: the stream cannot be *opened* in a compressed format, and a compressed frame
        // arriving on an open raw stream is refused too — the second is the one that catches a
        // format that changed under a running recording.
        let opened = Y4mWriter::begin(
            Cursor::new(Vec::new()),
            params(PixelFormat::MJPG, generous_caps()),
        );
        let message = refusal(&opened);
        assert!(message.contains(NOT_RAW), "{message}");
        assert!(message.contains("MJPG"), "{message}");

        let mut file = Vec::new();
        let mut writer = Y4mWriter::begin(
            Cursor::new(&mut file),
            params(PixelFormat::GREY, generous_caps()),
        )
        .expect("a GREY stream opens");
        let jpeg =
            encode::jpeg(&Decoded::Gray(fixtures::gradient(WIDTH, HEIGHT)), 40).expect("encodes");
        let intruder = Frame {
            bytes: jpeg,
            pixel_format: PixelFormat::MJPG,
            width: WIDTH,
            height: HEIGHT,
            bytes_per_line: 0,
            sequence: 0,
            timestamp_us: 0,
        };
        let message = refusal(&writer.write_frame(&intruder));
        assert!(message.contains(FORMAT_MISMATCH), "{message}");
        assert!(message.contains("GREY"), "{message}");
    }

    #[test]
    fn an_odd_extent_on_a_subsampled_axis_is_refused_rather_than_padded() {
        // 4:2:2 pairs pixels horizontally and 4:2:0 pairs them on both axes, so an odd extent
        // has no whole chroma sample to write. Inventing one would put a sample in the file
        // that no sensor produced; truncating would report a size nobody asked for.
        let odd_wide = Y4mWriter::begin(
            Cursor::new(Vec::new()),
            Y4mParams {
                width: 65,
                ..params(PixelFormat::YUYV, generous_caps())
            },
        );
        assert!(refusal(&odd_wide).contains(ODD_EXTENT));

        // 4:2:2 does *not* subsample vertically, so an odd height is fine — the check is
        // per-colorspace and this is the arm that proves it is not a blanket refusal.
        assert!(
            Y4mWriter::begin(
                Cursor::new(Vec::new()),
                Y4mParams {
                    height: 49,
                    ..params(PixelFormat::YUYV, generous_caps())
                },
            )
            .is_ok()
        );

        for (width, height) in [(65, 48), (64, 49), (65, 49)] {
            let refused = Y4mWriter::begin(
                Cursor::new(Vec::new()),
                Y4mParams {
                    width,
                    height,
                    ..params(PixelFormat::NV12, generous_caps())
                },
            );
            assert!(
                refusal(&refused).contains(ODD_EXTENT),
                "{width}x{height} NV12 must be refused"
            );
        }

        // Mono subsamples nothing and takes any extent, including a wholly odd one.
        assert!(
            Y4mWriter::begin(
                Cursor::new(Vec::new()),
                Y4mParams {
                    width: 65,
                    height: 49,
                    ..params(PixelFormat::GREY, generous_caps())
                },
            )
            .is_ok()
        );
    }

    #[test]
    fn a_degenerate_extent_is_refused_before_a_header_exists() {
        for (width, height) in [(0, 48), (64, 0), (0, 0)] {
            let refused = Y4mWriter::begin(
                Cursor::new(Vec::new()),
                Y4mParams {
                    width,
                    height,
                    ..params(PixelFormat::GREY, generous_caps())
                },
            );
            assert!(
                refusal(&refused).contains(ZERO_EXTENT),
                "{width}x{height} must be refused"
            );
        }
    }

    #[test]
    fn a_frame_whose_length_disagrees_with_its_geometry_is_refused_before_a_byte_is_read() {
        // The defect rubric B10 is about, on this side of the boundary: a driver reporting
        // 64x48 while delivering one row. Trusting the geometry reads past the end of the
        // buffer; this refuses with the two numbers that disagree, in every colorspace.
        for pixel_format in [PixelFormat::GREY, PixelFormat::YUYV, PixelFormat::NV12] {
            let mut file = Vec::new();
            let mut writer = Y4mWriter::begin(
                Cursor::new(&mut file),
                params(pixel_format, generous_caps()),
            )
            .expect("begin");
            let mut short = raw_frame(pixel_format, 0, 0);
            let full = short.bytes.len();
            short.bytes.truncate(full - 1);
            let message = refusal(&writer.write_frame(&short));
            assert!(
                message.contains(crate::fault::SHORT_BUFFER_PREFIX),
                "{pixel_format}: {message}"
            );
            assert!(
                message.contains(&full.to_string()),
                "{pixel_format}: {message}"
            );

            // An empty payload is its own refusal, because "no bytes at all" and "fewer bytes
            // than the geometry demands" are different things a driver does.
            let mut empty = raw_frame(pixel_format, 0, 0);
            empty.bytes.clear();
            assert!(refusal(&writer.write_frame(&empty)).contains(EMPTY_FRAME));

            // And the exact-length frame is written, so the bound is the real one rather than
            // a blanket refusal.
            assert_eq!(
                writer
                    .write_frame(&raw_frame(pixel_format, 0, 0))
                    .expect("a whole frame is written"),
                FrameOutcome::Written
            );
        }
    }

    #[test]
    fn a_frame_whose_geometry_is_not_the_streams_is_refused() {
        let mut file = Vec::new();
        let mut writer = Y4mWriter::begin(
            Cursor::new(&mut file),
            params(PixelFormat::GREY, generous_caps()),
        )
        .expect("begin");
        let source = fixtures::gradient(32, 24);
        let wrong = Frame {
            bytes: fixtures::pack_grey(&source),
            pixel_format: PixelFormat::GREY,
            width: 32,
            height: 24,
            bytes_per_line: 0,
            sequence: 0,
            timestamp_us: 0,
        };
        let message = refusal(&writer.write_frame(&wrong));
        assert!(message.contains(EXTENT_MISMATCH), "{message}");
        assert!(message.contains("32x24"), "{message}");
        assert!(message.contains("64x48"), "{message}");
    }

    #[test]
    fn stride_padding_is_dropped_rather_than_written_into_the_file() {
        // A driver that pads its rows and one that does not must produce the same file, or a
        // recording would carry bytes the sensor never produced — and in mono those bytes are
        // luma, which is to say they are picture.
        let mut padded = Vec::new();
        for row in 0..HEIGHT {
            for column in 0..WIDTH {
                padded.push(u8::try_from((row + column) % 251).expect("under 251"));
            }
            padded.extend_from_slice(&[0xff; 8]);
        }
        let frame = Frame {
            bytes: padded,
            pixel_format: PixelFormat::GREY,
            width: WIDTH,
            height: HEIGHT,
            bytes_per_line: WIDTH + 8,
            sequence: 0,
            timestamp_us: 0,
        };
        let (file, _, _) = record(params(PixelFormat::GREY, generous_caps()), &[frame]);
        let parsed = parse_y4m(&file).expect("parses");
        let plane = &parsed.frames.first().expect("one frame").y;
        assert_eq!(
            plane.len(),
            usize::try_from(WIDTH * HEIGHT).expect("fits"),
            "the padding reached the plane"
        );
        assert!(
            !plane.windows(8).any(|window| window == [0xff; 8]),
            "a run of padding bytes survived into the file"
        );
    }

    #[test]
    fn the_header_states_the_chroma_siting_the_recording_was_opened_with() {
        // **The tag was a constant under a comment saying it asserted nothing, and it
        // asserted something** (note **N200**). `C420` is `C420jpeg`'s siting to `ffprobe` and
        // to `mpv` alike (note **N200**), so there was no neutral spelling to have chosen
        // and the writer was declaring a siting nobody had decided on. The repair makes it an
        // input, and this is the walk that keeps it one: every member of the vocabulary
        // reaches the file as its own tag, and a build that ignored the input would put two
        // members on one tag.
        //
        // The walk is over `ChromaSiting::ALL` rather than over a list written here, so a
        // third siting cannot be added to the schema without deciding what this container
        // writes for it — the `ALL`-shaped treatment §9.1 of the G6 review says is the one
        // that works.
        let mut tags = BTreeSet::new();
        for &siting in ChromaSiting::ALL {
            let params = Y4mParams {
                chroma_siting: siting,
                ..params(PixelFormat::NV12, generous_caps())
            };
            let (file, _, _) = record(params, &[raw_frame(PixelFormat::NV12, 0, 0)]);
            let parsed = parse_y4m(&file).unwrap_or_else(|err| panic!("{siting:?}: {err}"));
            assert!(
                tags.insert(parsed.colorspace.clone()),
                "{siting:?} writes {}, which another siting already writes — the input is \
                 being ignored",
                parsed.colorspace
            );
            // The planes are the same bytes whichever siting was declared: a siting is a
            // statement about where the samples *are*, not a rearrangement of them, so a
            // build that resampled on the strength of the tag would fail here.
            let expected = expected_planes(PixelFormat::NV12);
            let written = parsed.frames.first().expect("one frame");
            assert_eq!(&written.y, &expected.y, "{siting:?}");
            assert_eq!(&written.u, &expected.u, "{siting:?}");
            assert_eq!(&written.v, &expected.v, "{siting:?}");
        }
        assert_eq!(tags.len(), ChromaSiting::ALL.len());

        // The default is the tag every file this project has already written carries, which
        // is what keeps the frozen fixtures frozen and is the owner's ruling of 2026-08-16.
        assert_eq!(ChromaSiting::default(), ChromaSiting::Centred);
        let (file, _, _) = record(
            params(PixelFormat::NV12, generous_caps()),
            &[raw_frame(PixelFormat::NV12, 0, 0)],
        );
        assert_eq!(parse_y4m(&file).expect("parses").colorspace, "420");

        // And the two colorspaces that have no siting to state do not acquire one from the
        // input — measured, not assumed: `ffprobe` reads `C422` as `chroma_location=
        // unspecified` (note **N200**), and mono has no chroma at all.
        for (pixel_format, tag) in [(PixelFormat::GREY, "mono"), (PixelFormat::YUYV, "422")] {
            for &siting in ChromaSiting::ALL {
                let params = Y4mParams {
                    chroma_siting: siting,
                    ..params(pixel_format, generous_caps())
                };
                let (file, _, _) = record(params, &[raw_frame(pixel_format, 0, 0)]);
                assert_eq!(
                    parse_y4m(&file).expect("parses").colorspace,
                    tag,
                    "{tag} acquired a siting from {siting:?}"
                );
            }
        }
    }

    #[test]
    fn stride_padding_is_dropped_in_every_colorspace_and_not_only_in_mono() {
        // The test above says the same thing about *mono*, which is one of the three fills
        // and the only one whose padded-plane arithmetic is a single de-strided copy.
        // `fill_c422` walks the padded rows itself and `fill_c420` locates the chroma plane
        // at `y_stride × height` — the offset `decode_nv12`'s own comment warns about and
        // note **N130** found three surviving mutants in — and neither had an assertion
        // here (note **N202**). So a driver that pads its rows and one that does not must produce
        // the same
        // *file*, in all three, and the comparison is against the planes the tight fixture
        // is already known to produce.
        //
        // The fixture can fail for the right reason because it is `raw_frame`'s: luma, Cb and
        // Cr have disjoint ranges and every sample is a function of its own position (note
        // **N108**), so a fill that read a chroma row at the wrong offset, took the padding
        // as samples, or exchanged the two chroma planes lands values these generators cannot
        // produce rather than values that happen to match.
        for (pixel_format, tag) in [
            (PixelFormat::GREY, "mono"),
            (PixelFormat::YUYV, "422"),
            (PixelFormat::NV12, "420"),
        ] {
            let expected = expected_planes(pixel_format);
            for pad in [2, 8, 96] {
                let frame = padded_raw_frame(pixel_format, pad, true);
                let (file, outcomes, summary) =
                    record(params(pixel_format, generous_caps()), &[frame]);
                assert_eq!(outcomes, vec![FrameOutcome::Written], "{tag} pad {pad}");
                assert_eq!(summary.frames_written, 1, "{tag} pad {pad}");
                let parsed =
                    parse_y4m(&file).unwrap_or_else(|err| panic!("{tag} pad {pad}: {err}"));
                let written = parsed.frames.first().expect("one frame");
                assert_eq!(&written.y, &expected.y, "{tag} pad {pad}: luma");
                assert_eq!(&written.u, &expected.u, "{tag} pad {pad}: Cb");
                assert_eq!(&written.v, &expected.v, "{tag} pad {pad}: Cr");
                assert!(
                    !written.y.contains(&0xff)
                        && !written.u.contains(&0xff)
                        && !written.v.contains(&0xff),
                    "{tag} pad {pad}: a padding byte reached the file"
                );
            }
        }

        // And the shared rule about the *last* row, which `crate::decode::plane_bytes`
        // states once for this whole crate: its padding "is not something a driver owes us".
        // The two packed colorspaces are sized through that function, so a final row
        // delivered without its padding is a whole frame to them as well.
        for (pixel_format, tag) in [(PixelFormat::GREY, "mono"), (PixelFormat::YUYV, "422")] {
            let expected = expected_planes(pixel_format);
            let frame = padded_raw_frame(pixel_format, 8, false);
            let (file, outcomes, _) = record(params(pixel_format, generous_caps()), &[frame]);
            assert_eq!(outcomes, vec![FrameOutcome::Written], "{tag}");
            let parsed = parse_y4m(&file).unwrap_or_else(|err| panic!("{tag}: {err}"));
            let written = parsed.frames.first().expect("one frame");
            assert_eq!(
                &written.y, &expected.y,
                "{tag}: luma of an unpadded last row"
            );
            assert_eq!(&written.u, &expected.u, "{tag}: Cb of an unpadded last row");
            assert_eq!(&written.v, &expected.v, "{tag}: Cr of an unpadded last row");
        }
    }

    #[test]
    fn a_cap_below_the_header_this_geometry_needs_is_the_size_cap_at_frame_zero() {
        // The floor at `begin` catches caps no geometry could satisfy; this catches a cap that
        // *this* header does not fit under. The answer is the size cap rather than an error,
        // because a caller that asked for a bound got one — and the file that exists is a
        // legitimate Y4M of no frames.
        let header_len = header_bytes(
            layout(WIDTH, HEIGHT, PixelFormat::GREY, ChromaSiting::default()).expect("legal"),
            NEGOTIATED_US,
        )
        .bytes
        .len();
        let caps = RecordingCaps {
            max_bytes: u64::try_from(header_len).expect("fits") - 1,
            ..generous_caps()
        };
        let (file, outcomes, summary) = record(
            params(PixelFormat::GREY, caps),
            &[raw_frame(PixelFormat::GREY, 0, 0)],
        );
        assert_eq!(outcomes, vec![FrameOutcome::Refused(CapReached::Size)]);
        assert_eq!(summary.frames_written, 0);
        assert_eq!(summary.cap_reached, Some(CapReached::Size));
        assert!(
            parse_y4m(&file)
                .expect("an empty Y4M is a Y4M")
                .frames
                .is_empty()
        );

        // And a cap no header at all could satisfy is refused at `begin`, before a file
        // exists — the two mechanisms answer two different questions.
        let refused = Y4mWriter::begin(
            Cursor::new(Vec::new()),
            params(
                PixelFormat::GREY,
                RecordingCaps {
                    max_bytes: MIN_RECORDING_BYTES - 1,
                    ..generous_caps()
                },
            ),
        );
        assert!(refusal(&refused).contains(CAP_TOO_SMALL));

        // And the floor itself is admitted, so that refusal is a bound rather than a blanket.
        // The AVI writer asserts the same pair in one test
        // (`a_cap_below_the_empty_file_is_refused_at_begin`); this half was missing here, and
        // the mutation floor turned the `<` into a `<=` with the workspace still green. The
        // geometry is the smallest this writer has, because `MIN_RECORDING_BYTES` *is* that
        // header's length — `the_smallest_header_this_writer_emits_is_the_documented_floor`
        // pins the two together. A cap of exactly the floor has to buy exactly the file the
        // refusal above promises, or the message tells a caller that 44 bytes is enough while
        // refusing 44 bytes.
        let mut file = Vec::new();
        let writer = Y4mWriter::begin(
            Cursor::new(&mut file),
            Y4mParams {
                width: 2,
                height: 2,
                pixel_format: PixelFormat::NV12,
                caps: RecordingCaps {
                    max_bytes: MIN_RECORDING_BYTES,
                    ..generous_caps()
                },
                ..params(PixelFormat::NV12, generous_caps())
            },
        )
        .expect("a cap of exactly the floor is a legal cap");
        writer
            .finish()
            .expect("an empty recording fits its own floor");
        assert_eq!(
            u64::try_from(file.len()).expect("fits"),
            MIN_RECORDING_BYTES,
            "the floor bought a file that is not the floor"
        );
        assert!(
            parse_y4m(&file)
                .expect("an empty Y4M is a Y4M")
                .frames
                .is_empty()
        );
    }

    #[test]
    fn a_zero_negotiated_interval_is_refused_rather_than_written_as_a_rate() {
        // `F1000000:0` is a division by zero to every parser, and the ratio's denominator is
        // where the interval goes — so a zero interval has no header to be written into.
        let refused = Y4mWriter::begin(
            Cursor::new(Vec::new()),
            Y4mParams {
                negotiated_interval_us: Some(0),
                ..params(PixelFormat::GREY, generous_caps())
            },
        );
        assert!(refusal(&refused).contains(ZERO_INTERVAL));
    }

    #[test]
    fn a_gap_in_the_drivers_sequence_numbers_is_reported_as_dropped_frames() {
        // What stops a dropped frame reading as a slow transition (Expected usage item 10).
        let frames = [
            raw_frame(PixelFormat::GREY, 10, 0),
            raw_frame(PixelFormat::GREY, 13, 100_000),
            // A backwards sequence is a driver doing something else, not four billion drops.
            raw_frame(PixelFormat::GREY, 11, 200_000),
        ];
        let (_, _, summary) = record(params(PixelFormat::GREY, generous_caps()), &frames);
        assert_eq!(summary.dropped_frames, 2);
        assert_eq!(summary.frames_written, 3);
    }

    #[test]
    fn a_sink_that_fails_is_named_and_never_written_to_again() {
        // `write_all` does not say how many bytes landed before an error, so after a failure
        // this writer no longer knows the file's length. It says so, names the prefix that is
        // still readable, and refuses every later call rather than writing into a file whose
        // length it is guessing.
        struct ShortSink {
            budget: usize,
            at: u64,
        }
        impl Write for ShortSink {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                if self.budget == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::StorageFull,
                        "no space left on device",
                    ));
                }
                let taken = buf.len().min(self.budget);
                self.budget -= taken;
                self.at = self.at.saturating_add(u64::try_from(taken).expect("fits"));
                Ok(taken)
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        // A cursor that seeks freely and a sink that cannot write: the two faults are kept
        // apart deliberately, because this test is about what a **write** failure does and a
        // seek that also refused would let the writer be poisoned by the wrong call.
        impl Seek for ShortSink {
            fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
                self.at = match to {
                    SeekFrom::Start(at) => at,
                    SeekFrom::Current(delta) | SeekFrom::End(delta) => {
                        self.at.saturating_add_signed(delta)
                    }
                };
                Ok(self.at)
            }
        }

        let mut writer = Y4mWriter::begin(
            ShortSink {
                budget: 4096,
                at: 0,
            },
            params(PixelFormat::GREY, generous_caps()),
        )
        .expect("the header fits in the budget");
        let frame = raw_frame(PixelFormat::GREY, 0, 0);
        // 64x48 mono is 3072 bytes a frame plus six of marker, so the first fits and the
        // second runs the budget out mid-plane.
        assert_eq!(
            writer.write_frame(&frame).expect("the first frame fits"),
            FrameOutcome::Written
        );
        let message = refusal(&writer.write_frame(&frame));
        assert!(message.contains(SINK_FAILED), "{message}");

        // Every later call refuses, and names the same prefix.
        let again = refusal(&writer.write_frame(&frame));
        assert!(again.contains("readable Y4M of whole frames"), "{again}");
        assert!(refusal(&writer.finish()).contains(SINK_FAILED));
    }

    /// The stream every frozen fixture was recorded from.
    ///
    /// Restated in `crates/imaging/fixtures/README.md` so the files can be rebuilt without
    /// reading this code, exactly as the AVI fixture's table does. The interval is
    /// [`NEGOTIATED_US`] and **not** [`PROVISIONAL_INTERVAL_US`] deliberately: a writer that
    /// dropped the negotiated number and wrote the placeholder would otherwise match the
    /// frozen bytes.
    fn fixture_params(pixel_format: PixelFormat) -> Y4mParams {
        Y4mParams {
            width: 64,
            height: 48,
            pixel_format,
            negotiated_interval_us: Some(NEGOTIATED_US),
            chroma_siting: ChromaSiting::default(),
            caps: RecordingCaps {
                max_bytes: 1 << 20,
                max_frames: 64,
                max_span: Duration::from_secs(60),
            },
        }
    }

    /// The two frames every frozen fixture holds: sequences 0 and 1, 50 000 µs apart.
    ///
    /// The gap between the timestamps is what makes the fixture able to say something about the
    /// rate at all: 50 000 µs is a measurable mean against a negotiated 66 666, so these bytes
    /// pin the close-time rewrite as well as the layout — a sink that stopped patching, or that
    /// patched the negotiated number back in, moves them. The same gap was here before P6d for
    /// the opposite reason: it pinned a header that *declined* to be rewritten.
    fn fixture_frames(pixel_format: PixelFormat) -> Vec<Frame> {
        vec![
            raw_frame(pixel_format, 0, 0),
            raw_frame(pixel_format, 1, 50_000),
        ]
    }

    /// The frozen files, and the format each one pins.
    ///
    /// Two rather than three, and `crates/imaging/fixtures/README.md` records the limit: the
    /// three colorspaces differ in the `C` tag and in their plane sizes, both of which the
    /// independent parser above derives and asserts for all three, and a frozen file adds
    /// only the byte-level layout. `C422` is frozen because it is the richest layout — three
    /// non-empty planes at 4:2:2 sizes, extracted from a *packed* buffer — and `Cmono` because
    /// it is the opposite shape and the one whose two empty planes a reader could silently
    /// stop writing. `C420` sits between them on both counts.
    const FROZEN: [(&str, PixelFormat, &[u8]); 2] = [
        (
            "two-frame-64x48-c422.y4m",
            PixelFormat::YUYV,
            include_bytes!("../fixtures/y4m/two-frame-64x48-c422.y4m"),
        ),
        (
            "two-frame-64x48-mono.y4m",
            PixelFormat::GREY,
            include_bytes!("../fixtures/y4m/two-frame-64x48-mono.y4m"),
        ),
    ];

    #[test]
    fn the_frozen_fixtures_are_still_the_files_the_y4m_sink_emits() {
        // What this proves is narrow and `crates/imaging/fixtures/README.md` says so where a
        // human reading the directory will find it: a fixture produced by the code under test
        // proves nothing about *correctness* — if the sink were wrong these bytes would be
        // wrong in exactly the same way. What it pins is a **frozen format against silent
        // drift**: a header field reordered, an `A1:1` that grew back, a per-frame parameter
        // added, a plane written in the wrong order — each of those changes these bytes and
        // nothing else in the suite is looking at them.
        //
        // It also carries the privacy claim the gate cannot make about a sample plane: the
        // frames are re-generated from this module's own sample functions and compared against
        // what the independent parser reads back out of the committed bytes, so a camera frame
        // spliced in here fails the comparison (design §5, AGENTS "Hardware and privacy").
        for (name, pixel_format, frozen) in FROZEN {
            let (emitted, outcomes, summary) =
                record(fixture_params(pixel_format), &fixture_frames(pixel_format));
            assert!(
                outcomes.iter().all(|o| *o == FrameOutcome::Written),
                "{name}"
            );
            assert_eq!(
                emitted, frozen,
                "{name} is no longer the file this sink emits; if the format changed on \
                 purpose, replace the file wholesale and say in the commit what moved and why"
            );
            assert_eq!(
                summary.bytes_written,
                u64::try_from(frozen.len()).expect("fits"),
                "{name}"
            );

            // Read back through the parser that was not told our layout, and compare the
            // samples against freshly generated ones — the half that says these are generated
            // pixels rather than a picture of somebody.
            let parsed = parse_y4m(frozen).unwrap_or_else(|err| panic!("{name}: {err}"));
            let expected = expected_planes(pixel_format);
            assert_eq!(parsed.frames.len(), 2, "{name}");
            for frame in &parsed.frames {
                assert_eq!(frame.y, expected.y, "{name}: luma");
                assert_eq!(frame.u, expected.u, "{name}: Cb");
                assert_eq!(frame.v, expected.v, "{name}: Cr");
            }
            assert_eq!(
                parsed.rate,
                (1_000_000, 50_000),
                "{name}: the frozen header declares the **measured** mean, patched over the \
                 negotiated {NEGOTIATED_US} at close — which is what note N106's amendment \
                 bought and what these bytes are the record of"
            );
        }
    }

    #[test]
    fn a_refusal_never_carries_a_sample() {
        // Rubric A12 as a test: a frame may contain a person, and every plane in this module
        // is picture. The only numbers a refusal may carry are geometry and counts.
        let mut writer = Y4mWriter::begin(
            Cursor::new(Vec::new()),
            params(PixelFormat::GREY, generous_caps()),
        )
        .expect("begin");
        let mut secret = raw_frame(PixelFormat::GREY, 0, 0);
        secret.bytes = vec![0xab; 7];
        let message = refusal(&writer.write_frame(&secret));
        assert!(!message.contains("171"), "{message}");
        assert!(!message.contains("0xab"), "{message}");
        assert!(
            message.contains(crate::fault::SHORT_BUFFER_PREFIX),
            "{message}"
        );
    }
}
