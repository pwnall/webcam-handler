//! The muxer: MJPEG frames in, one AVI file out, and nothing in between them.
//!
//! Three calls — [`AviWriter::begin`], [`AviWriter::write_frame`], [`AviWriter::finish`] —
//! and the whole of D7 L0 lives in what each one is allowed to touch. `begin` writes every
//! byte a reader needs and no byte it will have to revisit; `write_frame` only appends;
//! `finish` is the only thing that seeks backwards. That split is not tidiness, it is the
//! crash-recovery promise made mechanical.
//!
//! ## A sink, not a `Vec`
//!
//! [`AviWriter`] takes a `W: Write + Seek` rather than returning bytes because a recording
//! cannot be buffered whole — bounding its size is the point — the close-time header
//! rewrite needs seek-back, and the crash-recovery promise needs the frames already on the
//! sink when the crash happens. Purity survives: this crate still opens no path and reads
//! no clock (design §2.10), because the engine owns the sink and hands it in. The
//! alternative — returning byte chunks plus a patch for the caller to apply — was rejected
//! because it would put the CFR rewrite's correctness in two crates, and AGENTS' "one home
//! per law" forbids that.
//!
//! Buffering is the caller's: this module writes to the sink it is handed, because the
//! caller is the one who knows whether that sink is a file, a pipe or a `Vec`.
//!
//! ## Everything a reader needs is written before the first frame
//!
//! D7 L0 says a crash "leaves a recoverable `movi` stream (documented)", and
//! [`super::read::recover_frames`] is the implementation of that promise. It can only work
//! if the header list is complete and final the moment the first frame lands, so `begin`
//! writes the whole `LIST hdrl` — `avih`, `strh`, `strf` — with every size field that is
//! knowable already correct. Nothing written before a frame depends on a later fixup to be
//! parseable.
//!
//! Two size fields are genuinely unknowable then: the `RIFF` size and the `LIST movi` size.
//! They are written as a documented placeholder of **zero**, argued at `SIZE_PLACEHOLDER`
//! itself: a crashed recording then declares "the `RIFF` chunk holds nothing", which every
//! naive reader treats as immediate end-of-data, where `0xFFFF_FFFF` would invite one to read
//! four gigabytes past the end of the file.
//!
//! `avih.dwFlags` starts at zero for the same reason: `AVIF_HASINDEX` is set by `finish`,
//! after `idx1` is actually on the sink. A file that crashed mid-recording must not claim
//! an index it does not have — a reader that believed it would seek into frame data.
//!
//! ## The pad byte, and the size field it is not in
//!
//! RIFF pads an odd-length chunk to an even boundary with a single zero byte. That byte is
//! **not** counted in the chunk's own size field, is **not** counted in the `idx1` entry's
//! `dwChunkLength`, and **is** counted in the enclosing `LIST movi` size. Getting that
//! backwards is the most common muxer bug there is, which is why the reader beside this
//! module was written first and independently: it refuses all three mistakes, and the tests
//! below go through it rather than through any layout helper of ours.
//!
//! ## One rate, two homes, both rewritten at close
//!
//! D7's CFR carve-out, and the notes' sharper version of it: "a recording that silently
//! reports nominal fps while delivering something else is answering the agent's question
//! wrongly … it must not make a dropped frame look like a slow transition". So `finish`
//! rewrites `avih.dwMicroSecPerFrame` **and** `strh.dwScale`/`dwRate`, and
//! [`super::RecordingSummary`] reports which of the two things happened: a mean was measured, or
//! it was not and the header still carries what the camera was asked for.
//!
//! The mean is `(last − first) / (frames − 1)` over the delivered timestamps, truncating.
//! Truncation biases the declared duration short by under one microsecond per frame
//! interval — a millisecond over a thousand frames — and the formula is the one D7 names,
//! so it is written the way it is stated rather than improved on quietly.
//!
//! ## A driver is untrusted input, and so is a caller's cap
//!
//! `width`, `height`, `sequence`, `timestamp_us` and `bytes.len()` all arrive from a kernel
//! driver (rubric B10). Every one of them is validated before a size field is derived from
//! it: a frame whose geometry disagrees with the stream's is refused, an empty payload is
//! refused, a payload too long for a 32-bit chunk size is refused, a clock that runs
//! backwards produces neither a negative span nor a panic. Every integer that reaches the
//! file goes through `try_from`; there is no `as` in this module.
//!
//! The caps get the same treatment. `idx1` offsets are 32 bits, so a `movi` list that grows
//! past 4 GiB silently wraps every offset after that point — this project does not ship
//! that, and [`MAX_RECORDING_BYTES`] is refused at `begin` rather than discovered at frame
//! 200 000.
//!
//! ## A sink that fails is the one thing this module cannot repair
//!
//! `std::io::Write::write_all` does not say how many bytes landed before an error, so after
//! a failed write this module no longer knows the length of the file it is writing. It
//! records that, refuses every later call, and names the last length it was sure of — the
//! prefix a reader can still walk. Repairing the tail would need to shorten the file, and a
//! `Write + Seek` sink cannot be truncated through this API; the recovery path is what
//! answers that case, which is exactly the case it was built for.
//!
//! ## A frame may contain a person
//!
//! Rubric A12. No refusal here quotes a payload byte — lengths, offsets, counts, dimensions
//! and format names only — and [`AviWriter`]'s `Debug` is hand-written for the same reason
//! `schema::capture::Frame`'s is: a derived one would print the recording.

use std::io::{Seek, SeekFrom, Write};

use schema::camera::PixelFormat;
use schema::capture::Frame;
use schema::error::Result;

use super::{AviParams, CapReached, FrameOutcome, IntervalSource, RecordingCaps, RecordingSummary};
use crate::fault::imaging_failure;

// The placeholder interval, re-exported so `imaging::avi::write::PROVISIONAL_INTERVAL_US`
// still resolves. It was defined here at P6a, when this was the only container that could
// need one; `crate::y4m` declares the same placeholder for the same reason, and a second copy
// of a placeholder is a second placeholder — so the definition and its argument live in
// `crate::video` since P6b.
pub use crate::video::PROVISIONAL_INTERVAL_US;

/// The largest `max_bytes` this muxer accepts, refused at [`AviWriter::begin`].
///
/// `u32::MAX`. An `idx1` entry's `dwChunkOffset` is 32 bits and is measured from the `movi`
/// FourCC, so a file that grows past this cannot be indexed at all — every offset past the
/// boundary wraps, and the resulting file is one whose own reader rejects it. The bound is
/// checked against the whole file rather than against `movi` alone because the file is
/// larger than `movi`, which makes it the conservative comparison of the two.
pub const MAX_RECORDING_BYTES: u64 = 4_294_967_295;

/// The smallest `max_bytes` this muxer accepts: what a recording of nothing costs.
///
/// The header list is complete before the first frame (D7's recovery promise), so a file
/// exists at this size the instant `begin` returns. A cap below it could never be honoured
/// — the file would be over budget before a frame arrived — and refusing at `begin` is the
/// difference between a caller learning that immediately and learning it from a file that
/// broke its own rule.
pub const MIN_RECORDING_BYTES: u64 = HEADER_BYTES;

/// The largest frame extent this container can describe.
///
/// `i16::MAX`. `strh`'s `rcFrame` is four 16-bit shorts, so a frame wider than 32 767
/// cannot be written into the header at all — the limit belongs to the format and is not a
/// policy of ours. It is far above anything a webcam produces (8K is 7 680 wide), so the
/// refusal it drives is a corruption check rather than a restriction.
pub const MAX_EXTENT: u32 = 32_767;

/// What `finish` patches over, and what a crashed file is therefore left holding.
///
/// Zero, and this is a decision rather than a habit. A crashed recording declares "the
/// `RIFF` chunk holds nothing", which every naive reader treats as immediate end-of-data;
/// `0xFFFF_FFFF` would instead invite one to read four gigabytes past the end of the file.
/// Zero can also never be a *correct* value for either field — the `RIFF` size covers at
/// least the header list and the `movi` size covers at least its own FourCC — so a
/// placeholder that survived to disk is unambiguous rather than merely unlikely.
const SIZE_PLACEHOLDER: u32 = 0;

/// `dwRate`, fixed for the life of the file.
///
/// The frame interval is a rational `dwScale`/`dwRate` seconds, and pinning the denominator
/// at one million makes `dwScale` *be* the interval in microseconds. The two homes of the
/// rate then agree by construction rather than by rounding, and the measured mean keeps
/// microsecond resolution — which is the resolution the driver's timestamps arrive at.
const MICROS_PER_SECOND: u32 = 1_000_000;

/// `AVIF_HASINDEX` in `avih.dwFlags`, set by `finish` once `idx1` is on the sink.
const AVIF_HASINDEX: u32 = 0x0000_0010;

/// `AVIIF_KEYFRAME` in an `idx1` entry's `dwFlags`.
///
/// On every entry, because every MJPEG frame is a complete JPEG: there are no P-frames to
/// depend on a predecessor, so every frame is a seek target. A reader that trusted a
/// keyframe flag we had left clear would refuse to seek anywhere in a file that is seekable
/// everywhere.
const AVIIF_KEYFRAME: u32 = 0x0000_0010;

/// `24`, for `strf.biBitCount`.
///
/// A `BITMAPINFOHEADER` must state a colour depth even when the payload is a compressed
/// bitstream that carries its own. 24 is what MJPEG AVI files state, and what players
/// expect to find; nothing in this crate reads it back.
const BIT_COUNT: u16 = 24;

// --- The specification's literals, derived here and not shared -----------------------
//
// `read.rs` keeps its own copies private on purpose (see the module doc on `crate::avi`);
// these are this module's, transcribed from the RIFF/AVI specification independently.

const RIFF: [u8; 4] = *b"RIFF";
const AVI_FORM: [u8; 4] = *b"AVI ";
const LIST: [u8; 4] = *b"LIST";
const HDRL: [u8; 4] = *b"hdrl";
const AVIH: [u8; 4] = *b"avih";
const STRL: [u8; 4] = *b"strl";
const STRH: [u8; 4] = *b"strh";
const STRF: [u8; 4] = *b"strf";
const MOVI: [u8; 4] = *b"movi";
const IDX1: [u8; 4] = *b"idx1";
const VIDS: [u8; 4] = *b"vids";
const MJPG: [u8; 4] = *b"MJPG";
/// Stream 0, compressed DIB: the only chunk id this muxer emits inside `movi`.
const CHUNK_00DC: [u8; 4] = *b"00dc";

/// A chunk header is a FourCC and a `DWORD` size.
const CHUNK_HEADER_BYTES: u64 = 8;
/// A FourCC — a chunk id, a form type, or a list type.
const FOURCC_BYTES: u64 = 4;
/// `MainAVIHeader`.
const AVIH_PAYLOAD_BYTES: u64 = 56;
/// `AVIStreamHeader`, `rcFrame` included.
const STRH_PAYLOAD_BYTES: u64 = 56;
/// `BITMAPINFOHEADER`.
const STRF_PAYLOAD_BYTES: u64 = 40;
/// Four `DWORD`s: `dwChunkId`, `dwFlags`, `dwChunkOffset`, `dwChunkLength`.
const IDX1_ENTRY_BYTES: u64 = 16;
/// The `LIST strl` payload: its list type, then `strh` and `strf` with their headers.
const STRL_SIZE: u64 = FOURCC_BYTES
    + CHUNK_HEADER_BYTES
    + STRH_PAYLOAD_BYTES
    + CHUNK_HEADER_BYTES
    + STRF_PAYLOAD_BYTES;
/// The `LIST hdrl` payload: its list type, `avih`, and the whole `LIST strl`.
///
/// Final the moment `begin` writes it — nothing here waits for close, which is what
/// [`super::read::recover_frames`] needs in order to find `movi` in a crashed file.
const HDRL_SIZE: u64 =
    FOURCC_BYTES + CHUNK_HEADER_BYTES + AVIH_PAYLOAD_BYTES + CHUNK_HEADER_BYTES + STRL_SIZE;

// --- Where `finish` seeks ------------------------------------------------------------
//
// Every offset below is a sum of the fixed structure sizes above, stated as that sum so it
// can be read rather than trusted. They are absolute because `begin` writes a header of one
// fixed shape — there is exactly one stream, and no optional chunk — and the test
// `the_header_offsets_are_the_fields_they_name` reads each one back out of a real file.

/// `RIFF`'s size field.
const RIFF_SIZE_AT: u64 = 4;
/// `LIST hdrl` — `RIFF` + its size + `AVI `.
const HDRL_AT: u64 = CHUNK_HEADER_BYTES + FOURCC_BYTES;
/// The `avih` payload — `hdrl`'s own header and list type, then `avih`'s header.
const AVIH_AT: u64 = HDRL_AT + CHUNK_HEADER_BYTES + FOURCC_BYTES + CHUNK_HEADER_BYTES;
const AVIH_MICRO_SEC_AT: u64 = AVIH_AT;
const AVIH_MAX_BYTES_PER_SEC_AT: u64 = AVIH_AT + 4;
const AVIH_FLAGS_AT: u64 = AVIH_AT + 12;
const AVIH_TOTAL_FRAMES_AT: u64 = AVIH_AT + 16;
const AVIH_SUGGESTED_BUFFER_AT: u64 = AVIH_AT + 28;
/// `LIST strl`, immediately after the `avih` payload.
const STRL_AT: u64 = AVIH_AT + AVIH_PAYLOAD_BYTES;
/// The `strh` payload.
const STRH_AT: u64 = STRL_AT + CHUNK_HEADER_BYTES + FOURCC_BYTES + CHUNK_HEADER_BYTES;
const STRH_SCALE_AT: u64 = STRH_AT + 20;
const STRH_RATE_AT: u64 = STRH_AT + 24;
const STRH_LENGTH_AT: u64 = STRH_AT + 32;
const STRH_SUGGESTED_BUFFER_AT: u64 = STRH_AT + 36;
/// The `strf` payload.
const STRF_AT: u64 = STRH_AT + STRH_PAYLOAD_BYTES + CHUNK_HEADER_BYTES;
/// `LIST movi`, immediately after the `strf` payload.
const MOVI_LIST_AT: u64 = STRF_AT + STRF_PAYLOAD_BYTES;
/// `LIST movi`'s size field.
const MOVI_SIZE_AT: u64 = MOVI_LIST_AT + 4;
/// The `movi` FourCC, which is what an `idx1` offset is measured from.
const MOVI_FOURCC_AT: u64 = MOVI_LIST_AT + CHUNK_HEADER_BYTES;
/// The whole header: everything `begin` writes, and where the first frame chunk lands.
const HEADER_BYTES: u64 = MOVI_FOURCC_AT + FOURCC_BYTES;

// --- The refusal vocabulary ----------------------------------------------------------
//
// Named because the tests pin them: `is_err()` cannot tell "we refused the thing we seeded"
// from "we tripped over something else on the way there" (`crate::fault::SHORT_BUFFER_PREFIX`
// records where this habit came from).

/// The muxer was handed something other than a JPEG bitstream.
const NOT_MJPEG: &str = "the muxer carries MJPEG only";
/// A frame's geometry is not the stream's.
const EXTENT_MISMATCH: &str = "but the stream was opened at";
/// A frame carries no bytes.
const EMPTY_FRAME: &str = "carries no bytes";
/// A payload, an offset or a count does not fit the field it must go in.
const NOT_ADDRESSABLE: &str = "does not fit";
/// The geometry is zero on an axis.
const ZERO_EXTENT: &str = "a recording needs a non-degenerate frame size";
/// The geometry is larger than the header can describe.
const EXTENT_TOO_LARGE: &str = "larger than this container can describe";
/// `max_bytes` is outside what this muxer will accept.
const CAP_UNADDRESSABLE: &str = "cannot be indexed by a 32-bit idx1 offset";
/// `max_bytes` is below the size of an empty recording.
const CAP_TOO_SMALL: &str = "is smaller than the empty file";
/// A negotiated interval of zero microseconds.
const ZERO_INTERVAL: &str = "a negotiated frame interval of 0 microseconds";
/// The sink refused a write, and the file's length is no longer known.
const SINK_FAILED: &str = "the sink failed after";

const BEGIN_OP: &str = "begin AVI recording";
const FRAME_OP: &str = "write AVI frame";
const FINISH_OP: &str = "finish AVI recording";

/// The frame extent, validated once so that everything derived from it afterwards is
/// infallible.
#[derive(Clone, Copy)]
struct Geometry {
    width: u32,
    height: u32,
    /// `rcFrame.right`, which is why [`MAX_EXTENT`] is what it is.
    right: i16,
    /// `rcFrame.bottom`.
    bottom: i16,
}

/// One `idx1` entry, held until close.
///
/// Eight bytes a frame in memory, against sixteen on disk — so `max_bytes` bounds this
/// vector twice over and no separate limit is needed.
#[derive(Clone, Copy)]
struct IndexEntry {
    /// Where the chunk header sits, relative to the `movi` FourCC.
    movi_offset: u32,
    /// The payload length, pad byte excluded.
    payload_bytes: u32,
}

/// A recording in progress.
///
/// `W` is the caller's sink. See the module doc for why this is not a `Vec<u8>`, and for
/// what happens when the sink itself fails.
pub struct AviWriter<W: Write + Seek> {
    sink: W,
    /// Where the file starts in the sink, so a sink handed over mid-stream still gets a
    /// correct file — every seek in `finish` is relative to this.
    origin: u64,
    geometry: Geometry,
    caps: RecordingCaps,
    negotiated_interval_us: Option<u32>,
    /// How many bytes of file this writer is *sure* are on the sink.
    bytes_written: u64,
    frames_written: u32,
    largest_frame: u32,
    index: Vec<IndexEntry>,
    first_timestamp_us: Option<i64>,
    last_timestamp_us: i64,
    last_sequence: Option<u32>,
    dropped_frames: u64,
    cap_reached: Option<CapReached>,
    sink_failed: bool,
}

impl<W: Write + Seek> std::fmt::Debug for AviWriter<W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // A frame may contain a person (rubric A12), and the sink holds every frame in the
        // recording — so neither the sink nor the index is printable here.
        f.debug_struct("AviWriter")
            .field("frames_written", &self.frames_written)
            .field("bytes_written", &self.bytes_written)
            .field("dropped_frames", &self.dropped_frames)
            .field("cap_reached", &self.cap_reached)
            .field("sink_failed", &self.sink_failed)
            .finish_non_exhaustive()
    }
}

impl<W: Write + Seek> AviWriter<W> {
    /// Open a recording: validate the parameters, then write the whole header list.
    ///
    /// When this returns, the file on the sink is already something
    /// [`super::read::recover_frames`] can walk — that is the point of doing all of it here
    /// rather than at close.
    ///
    /// # Errors
    ///
    /// [`schema::Error::DeviceIo`] for a degenerate or oversized frame extent, a
    /// negotiated interval of zero, a `max_bytes` above [`MAX_RECORDING_BYTES`] or below
    /// [`MIN_RECORDING_BYTES`], or a sink that refused the header.
    pub fn begin(mut sink: W, params: AviParams) -> Result<Self> {
        let geometry = geometry(params.width, params.height)?;
        let caps = params.caps;
        if caps.max_bytes > MAX_RECORDING_BYTES {
            return Err(imaging_failure(
                BEGIN_OP,
                format!(
                    "a cap of {} bytes {CAP_UNADDRESSABLE}, which tops out at \
                     {MAX_RECORDING_BYTES}",
                    caps.max_bytes
                ),
            ));
        }
        if caps.max_bytes < MIN_RECORDING_BYTES {
            return Err(imaging_failure(
                BEGIN_OP,
                format!(
                    "a cap of {} bytes {CAP_TOO_SMALL}: a recording of no frames is already \
                     {MIN_RECORDING_BYTES} bytes of header",
                    caps.max_bytes
                ),
            ));
        }
        if params.negotiated_interval_us == Some(0) {
            return Err(imaging_failure(
                BEGIN_OP,
                format!("{ZERO_INTERVAL} is not a frame rate"),
            ));
        }
        let interval = params
            .negotiated_interval_us
            .unwrap_or(PROVISIONAL_INTERVAL_US);
        let origin = sink
            .stream_position()
            .map_err(|err| imaging_failure(BEGIN_OP, format!("{SINK_FAILED} 0 bytes: {err}")))?;
        let header = header_bytes(geometry, interval);
        let bytes_written = u64::try_from(header.len()).map_err(|_| {
            imaging_failure(BEGIN_OP, format!("the header {NOT_ADDRESSABLE} in a u64"))
        })?;
        sink.write_all(&header)
            .map_err(|err| imaging_failure(BEGIN_OP, format!("{SINK_FAILED} 0 bytes: {err}")))?;
        Ok(AviWriter {
            sink,
            origin,
            geometry,
            caps,
            negotiated_interval_us: params.negotiated_interval_us,
            bytes_written,
            frames_written: 0,
            largest_frame: 0,
            index: Vec::new(),
            first_timestamp_us: None,
            last_timestamp_us: 0,
            last_sequence: None,
            dropped_frames: 0,
            cap_reached: None,
            sink_failed: false,
        })
    }

    /// Append one frame, verbatim.
    ///
    /// The bytes reach the sink untouched (E6): no decode, no re-encode, no scaling, ever.
    /// Nothing about the payload is parsed either — a JPEG this module refused to believe
    /// would be a frame the camera produced and the file did not carry, and the muxer is
    /// not the place that adjudicates that.
    ///
    /// Caps are checked **before** the write, so a refused frame leaves the file exactly as
    /// it was and the file never exceeds its bounds. The order is `Frames`, `Span`, `Size`:
    /// the frame count is the caller's own number and does not depend on what the camera
    /// delivered, the span is about what the recording *means*, and the size projection is
    /// the most derived of the three (it includes the index that close will write). Once
    /// any cap is reached the recording is over — a later, smaller frame is refused too,
    /// because a recording with a hole in the middle answers the "how long did this take"
    /// question wrongly.
    ///
    /// # Errors
    ///
    /// [`schema::Error::DeviceIo`] for a pixel format that is not MJPEG, a geometry that is
    /// not the stream's, an empty payload, a payload too long for a 32-bit chunk size, or a
    /// sink that refused a write. A cap is **not** an error: it is
    /// [`FrameOutcome::Refused`].
    pub fn write_frame(&mut self, frame: &Frame) -> Result<FrameOutcome> {
        if let Some(cap) = self.cap_reached {
            return Ok(FrameOutcome::Refused(cap));
        }
        self.check_sink(FRAME_OP)?;
        let payload_bytes = self.check_frame(frame)?;
        if let Some(cap) = self.cap_for(payload_bytes, frame.timestamp_us) {
            self.cap_reached = Some(cap);
            return Ok(FrameOutcome::Refused(cap));
        }

        let next_count = self.frames_written.checked_add(1).ok_or_else(|| {
            imaging_failure(
                FRAME_OP,
                format!(
                    "one more than {} frames {NOT_ADDRESSABLE} in dwTotalFrames",
                    self.frames_written
                ),
            )
        })?;
        let movi_offset = self
            .bytes_written
            .checked_sub(MOVI_FOURCC_AT)
            .and_then(|offset| u32::try_from(offset).ok())
            .ok_or_else(|| {
                imaging_failure(
                    FRAME_OP,
                    format!(
                        "a frame {} bytes into the file {NOT_ADDRESSABLE} in an idx1 offset",
                        self.bytes_written
                    ),
                )
            })?;

        self.put(&CHUNK_00DC, FRAME_OP)?;
        self.put(&payload_bytes.to_le_bytes(), FRAME_OP)?;
        self.put(&frame.bytes, FRAME_OP)?;
        if payload_bytes % 2 == 1 {
            // Outside the chunk's own size field and outside the index entry's length;
            // inside the enclosing list's size. Zero, so that a *missing* pad is visible to
            // a reader at all.
            self.put(&[0], FRAME_OP)?;
        }

        self.index.push(IndexEntry {
            movi_offset,
            payload_bytes,
        });
        self.frames_written = next_count;
        self.largest_frame = self.largest_frame.max(payload_bytes);
        self.count_sequence(frame.sequence);
        if self.first_timestamp_us.is_none() {
            self.first_timestamp_us = Some(frame.timestamp_us);
        }
        self.last_timestamp_us = frame.timestamp_us;
        Ok(FrameOutcome::Written)
    }

    /// Write `idx1`, patch every field close is the first to know, and report the result.
    ///
    /// The order matters and is the order `read_stream` refuses every partial version of:
    /// the index lands on the sink first, and only then does `avih.dwFlags` claim one.
    ///
    /// # Errors
    ///
    /// [`schema::Error::DeviceIo`] when the sink refuses a write or a seek, or when it had
    /// already failed — in which case the message names the length of the prefix that is
    /// still a recoverable `movi` stream.
    pub fn finish(mut self) -> Result<RecordingSummary> {
        self.check_sink(FINISH_OP)?;
        // Before `idx1`: the `movi` list ends where the index begins.
        let movi_size = self
            .bytes_written
            .checked_sub(MOVI_FOURCC_AT)
            .ok_or_else(|| {
                imaging_failure(
                    FINISH_OP,
                    format!(
                        "the file is {} bytes, shorter than its own header",
                        self.bytes_written
                    ),
                )
            })?;
        let has_index = self.frames_written > 0;
        if has_index {
            self.write_index()?;
        }
        let (declared_interval_us, interval_source, span_us) = self.declared_interval();
        // What the `RIFF` size field covers is everything after itself.
        let riff_size = self.bytes_written.saturating_sub(CHUNK_HEADER_BYTES);
        let max_bytes_per_sec = self.max_bytes_per_sec(declared_interval_us);

        self.patch(RIFF_SIZE_AT, size_field(riff_size, "the RIFF size")?)?;
        self.patch(MOVI_SIZE_AT, size_field(movi_size, "the LIST movi size")?)?;
        self.patch(AVIH_MICRO_SEC_AT, declared_interval_us)?;
        self.patch(AVIH_MAX_BYTES_PER_SEC_AT, max_bytes_per_sec)?;
        self.patch(AVIH_FLAGS_AT, if has_index { AVIF_HASINDEX } else { 0 })?;
        self.patch(AVIH_TOTAL_FRAMES_AT, self.frames_written)?;
        self.patch(AVIH_SUGGESTED_BUFFER_AT, self.largest_frame)?;
        // Both homes of the rate, `dwRate` included even though it never changes: the
        // close-time rewrite owns the whole rational rather than half of it.
        self.patch(STRH_SCALE_AT, declared_interval_us)?;
        self.patch(STRH_RATE_AT, MICROS_PER_SECOND)?;
        self.patch(STRH_LENGTH_AT, self.frames_written)?;
        self.patch(STRH_SUGGESTED_BUFFER_AT, self.largest_frame)?;

        // Leave the sink at the end of the file rather than in the middle of the header,
        // so a caller holding another handle to it finds what it expects.
        let end = self.origin.saturating_add(self.bytes_written);
        self.seek_to(end)?;
        self.sink
            .flush()
            .map_err(|err| self.poison(FINISH_OP, &err))?;

        Ok(RecordingSummary {
            frames_written: self.frames_written,
            bytes_written: self.bytes_written,
            declared_interval_us,
            interval_source,
            dropped_frames: self.dropped_frames,
            span_us,
            cap_reached: self.cap_reached,
        })
    }

    /// The frame's payload length, once the frame is one this stream can carry.
    fn check_frame(&self, frame: &Frame) -> Result<u32> {
        // Spelled out rather than `PixelFormat::is_compressed`, which means "a bitstream we
        // pass through" and would silently start accepting H.264 the day D7's L1 layer adds
        // it to that set. This muxer carries MJPEG, and the two spellings of MJPEG are the
        // whole of what it carries.
        if frame.pixel_format != PixelFormat::MJPG && frame.pixel_format != PixelFormat::JPEG {
            return Err(imaging_failure(
                FRAME_OP,
                format!("{NOT_MJPEG}, and this frame is {}", frame.pixel_format),
            ));
        }
        if frame.width != self.geometry.width || frame.height != self.geometry.height {
            return Err(imaging_failure(
                FRAME_OP,
                format!(
                    "a {}x{} frame arrived, {EXTENT_MISMATCH} {}x{}",
                    frame.width, frame.height, self.geometry.width, self.geometry.height
                ),
            ));
        }
        if frame.bytes.is_empty() {
            return Err(imaging_failure(
                FRAME_OP,
                format!("frame {} {EMPTY_FRAME}", frame.sequence),
            ));
        }
        u32::try_from(frame.bytes.len()).map_err(|_| {
            imaging_failure(
                FRAME_OP,
                format!(
                    "a {}-byte frame {NOT_ADDRESSABLE} in a 32-bit chunk size field",
                    frame.bytes.len()
                ),
            )
        })
    }

    /// Which cap this frame would break, if any. Nothing is written when this is `Some`.
    fn cap_for(&self, payload_bytes: u32, timestamp_us: i64) -> Option<CapReached> {
        if self.frames_written >= self.caps.max_frames {
            return Some(CapReached::Frames);
        }
        if let Some(first) = self.first_timestamp_us {
            // A clock that ran backwards produces no elapsed time rather than a negative
            // one: the span cap bounds how long a recording ran, and "before it started" is
            // not a length. The mean interval takes the same view at close.
            let elapsed = timestamp_us
                .checked_sub(first)
                .and_then(|delta| u64::try_from(delta).ok())
                .unwrap_or(0);
            if u128::from(elapsed) > self.caps.max_span.as_micros() {
                return Some(CapReached::Span);
            }
        }
        match self.projected_bytes(payload_bytes) {
            // An arithmetic overflow here is a file far past `MAX_RECORDING_BYTES`, so the
            // size cap is the honest answer rather than an error.
            None => Some(CapReached::Size),
            Some(projected) if projected > self.caps.max_bytes => Some(CapReached::Size),
            Some(_) => None,
        }
    }

    /// What the finished file would weigh if this frame were written: the bytes so far, the
    /// chunk, its pad byte, and the `idx1` that close will write for every frame including
    /// this one.
    fn projected_bytes(&self, payload_bytes: u32) -> Option<u64> {
        let payload = u64::from(payload_bytes);
        let frames = u64::from(self.frames_written).checked_add(1)?;
        let index = IDX1_ENTRY_BYTES
            .checked_mul(frames)?
            .checked_add(CHUNK_HEADER_BYTES)?;
        self.bytes_written
            .checked_add(CHUNK_HEADER_BYTES)?
            .checked_add(payload)?
            .checked_add(payload % 2)?
            .checked_add(index)
    }

    /// Count what the driver's sequence numbers say never arrived.
    ///
    /// Only a forward gap is a drop. A repeated or backwards sequence number is a driver
    /// doing something else — and a `u32` that wrapped, which at 30 fps takes four and a
    /// half years — so it is not counted rather than counted as four billion.
    fn count_sequence(&mut self, sequence: u32) {
        if let Some(previous) = self.last_sequence
            && let Some(gap) = sequence.checked_sub(previous)
        {
            self.dropped_frames = self
                .dropped_frames
                .saturating_add(u64::from(gap.saturating_sub(1)));
        }
        self.last_sequence = Some(sequence);
    }

    /// The interval the header will declare, where it came from, and the span behind it.
    ///
    /// Forwarded to `crate::video::declared_interval` since P6d, where the arithmetic and its
    /// three refusals now live for both containers. It was this method's own until then, and it
    /// could be: a Y4M header could not be rewritten at close, so there was nothing to share it
    /// with (note **N106**, now amended). The forwarder stays rather than the call being
    /// inlined into `finish`, so a reader of this muxer still meets "the interval the header
    /// will declare" where the header is written.
    fn declared_interval(&self) -> (u32, IntervalSource, Option<u64>) {
        crate::video::declared_interval(
            self.first_timestamp_us,
            self.last_timestamp_us,
            self.frames_written,
            self.negotiated_interval_us,
        )
    }

    /// `avih.dwMaxBytesPerSec`: the whole file over the duration it declares.
    ///
    /// Zero when no frame was written, which is the field's conventional "unstated" — a
    /// recording of nothing has no data rate. A rate too large for the field saturates
    /// rather than wrapping to zero, because understating it is a player's problem and
    /// zeroing it silently is a reader's.
    fn max_bytes_per_sec(&self, declared_interval_us: u32) -> u32 {
        let duration_us =
            u64::from(declared_interval_us).checked_mul(u64::from(self.frames_written));
        match duration_us {
            Some(duration) if duration > 0 => self
                .bytes_written
                .checked_mul(u64::from(MICROS_PER_SECOND))
                .map(|scaled| scaled / duration)
                .and_then(|rate| u32::try_from(rate).ok())
                .unwrap_or(u32::MAX),
            _ => 0,
        }
    }

    /// Write `idx1`: one entry per frame, in file order.
    fn write_index(&mut self) -> Result<()> {
        let entries = std::mem::take(&mut self.index);
        let payload_bytes = u64::try_from(entries.len())
            .ok()
            .and_then(|count| IDX1_ENTRY_BYTES.checked_mul(count))
            .ok_or_else(|| {
                imaging_failure(
                    FINISH_OP,
                    format!("an index of {} entries {NOT_ADDRESSABLE}", entries.len()),
                )
            })?;
        self.put(&IDX1, FINISH_OP)?;
        self.put(
            &size_field(payload_bytes, "the idx1 size")?.to_le_bytes(),
            FINISH_OP,
        )?;
        let mut record = Vec::with_capacity(16);
        for entry in &entries {
            record.clear();
            record.extend_from_slice(&CHUNK_00DC);
            record.extend_from_slice(&AVIIF_KEYFRAME.to_le_bytes());
            record.extend_from_slice(&entry.movi_offset.to_le_bytes());
            // The payload length, pad byte excluded — which is what makes a pad byte
            // wrongly counted *inside* a chunk's size field detectable at all.
            record.extend_from_slice(&entry.payload_bytes.to_le_bytes());
            self.put(&record, FINISH_OP)?;
        }
        Ok(())
    }

    /// Append bytes, and remember that they landed.
    fn put(&mut self, bytes: &[u8], operation: &str) -> Result<()> {
        let count = u64::try_from(bytes.len()).map_err(|_| {
            imaging_failure(
                operation,
                format!("a {}-byte write {NOT_ADDRESSABLE} in a u64", bytes.len()),
            )
        })?;
        match self.sink.write_all(bytes) {
            Ok(()) => {
                self.bytes_written = self.bytes_written.saturating_add(count);
                Ok(())
            }
            Err(err) => Err(self.poison(operation, &err)),
        }
    }

    /// Overwrite one `DWORD` in the header. Never changes the file's length.
    fn patch(&mut self, at: u64, value: u32) -> Result<()> {
        self.seek_to(self.origin.saturating_add(at))?;
        self.sink
            .write_all(&value.to_le_bytes())
            .map_err(|err| self.poison(FINISH_OP, &err))
    }

    fn seek_to(&mut self, position: u64) -> Result<()> {
        self.sink
            .seek(SeekFrom::Start(position))
            .map(|_| ())
            .map_err(|err| self.poison(FINISH_OP, &err))
    }

    /// Record that the sink failed, and say how much file this writer was still sure of.
    ///
    /// One home for the bookkeeping, because a path that reported the failure without
    /// setting the flag would let the next call write into a file of unknown length.
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
                     first {} bytes are a recoverable movi stream",
                    self.bytes_written, self.bytes_written
                ),
            ));
        }
        Ok(())
    }
}

/// Validate a frame extent, once, for the whole recording.
fn geometry(width: u32, height: u32) -> Result<Geometry> {
    if width == 0 || height == 0 {
        return Err(imaging_failure(
            BEGIN_OP,
            format!("{ZERO_EXTENT}, and this one is {width}x{height}"),
        ));
    }
    let (Ok(right), Ok(bottom)) = (i16::try_from(width), i16::try_from(height)) else {
        return Err(imaging_failure(
            BEGIN_OP,
            format!(
                "a {width}x{height} frame is {EXTENT_TOO_LARGE}: strh's rcFrame is four \
                 16-bit shorts, so {MAX_EXTENT} is the largest extent it can state"
            ),
        ));
    };
    Ok(Geometry {
        width,
        height,
        right,
        bottom,
    })
}

/// A byte count as a `DWORD` size field, or a refusal naming it.
fn size_field(value: u64, what: &str) -> Result<u32> {
    u32::try_from(value).map_err(|_| {
        imaging_failure(
            FINISH_OP,
            format!("{what} is {value} bytes, which {NOT_ADDRESSABLE} in a 32-bit field"),
        )
    })
}

/// The whole header list, as bytes: `RIFF`/`AVI `, `LIST hdrl` (`avih` + `LIST strl` of
/// `strh` + `strf`), and the `LIST movi` header the first frame will follow.
///
/// One linear transcription of the layout, so that the offsets `finish` seeks to can be
/// read off it. Sizes that close will patch are [`SIZE_PLACEHOLDER`]; everything else is
/// final the moment this is written, which is what
/// [`super::read::recover_frames`] depends on.
fn header_bytes(geometry: Geometry, interval_us: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);

    out.extend_from_slice(&RIFF);
    push_u32(&mut out, SIZE_PLACEHOLDER);
    out.extend_from_slice(&AVI_FORM);

    // LIST hdrl — its size is knowable now and never changes.
    out.extend_from_slice(&LIST);
    push_size(&mut out, HDRL_SIZE);
    out.extend_from_slice(&HDRL);

    // avih
    out.extend_from_slice(&AVIH);
    push_size(&mut out, AVIH_PAYLOAD_BYTES);
    push_u32(&mut out, interval_us); // dwMicroSecPerFrame — rewritten at close
    push_u32(&mut out, 0); // dwMaxBytesPerSec — rewritten at close
    push_u32(&mut out, 0); // dwPaddingGranularity: this muxer pads to 2, per RIFF
    push_u32(&mut out, 0); // dwFlags — AVIF_HASINDEX is set at close, not here
    push_u32(&mut out, 0); // dwTotalFrames — rewritten at close
    push_u32(&mut out, 0); // dwInitialFrames: no audio to pre-roll (design §1)
    push_u32(&mut out, 1); // dwStreams
    push_u32(&mut out, 0); // dwSuggestedBufferSize — rewritten at close
    push_u32(&mut out, geometry.width);
    push_u32(&mut out, geometry.height);
    for _ in 0..4 {
        push_u32(&mut out, 0); // dwReserved
    }

    // LIST strl
    out.extend_from_slice(&LIST);
    push_size(&mut out, STRL_SIZE);
    out.extend_from_slice(&STRL);

    // strh
    out.extend_from_slice(&STRH);
    push_size(&mut out, STRH_PAYLOAD_BYTES);
    out.extend_from_slice(&VIDS); // fccType
    out.extend_from_slice(&MJPG); // fccHandler
    push_u32(&mut out, 0); // dwFlags
    push_u16(&mut out, 0); // wPriority
    push_u16(&mut out, 0); // wLanguage
    push_u32(&mut out, 0); // dwInitialFrames
    push_u32(&mut out, interval_us); // dwScale — rewritten at close
    push_u32(&mut out, MICROS_PER_SECOND); // dwRate — rewritten at close
    push_u32(&mut out, 0); // dwStart
    push_u32(&mut out, 0); // dwLength — rewritten at close
    push_u32(&mut out, 0); // dwSuggestedBufferSize — rewritten at close
    push_u32(&mut out, 0); // dwQuality: the encoder's, and there is no encoder here
    push_u32(&mut out, 0); // dwSampleSize: 0 means variable-size samples, which frames are
    push_i16(&mut out, 0); // rcFrame.left
    push_i16(&mut out, 0); // rcFrame.top
    push_i16(&mut out, geometry.right);
    push_i16(&mut out, geometry.bottom);

    // strf — a BITMAPINFOHEADER
    out.extend_from_slice(&STRF);
    push_size(&mut out, STRF_PAYLOAD_BYTES);
    push_size(&mut out, STRF_PAYLOAD_BYTES); // biSize
    push_u32(&mut out, geometry.width); // biWidth
    push_u32(&mut out, geometry.height); // biHeight, bottom-up like every MJPEG AVI
    push_u16(&mut out, 1); // biPlanes
    push_u16(&mut out, BIT_COUNT);
    out.extend_from_slice(&MJPG); // biCompression
    // biSizeImage is advisory for a compressed stream; the uncompressed extent saturates
    // rather than wrapping, because a wrapped size reads as a small image.
    push_u32(
        &mut out,
        geometry
            .width
            .saturating_mul(geometry.height)
            .saturating_mul(3),
    );
    push_u32(&mut out, 0); // biXPelsPerMeter
    push_u32(&mut out, 0); // biYPelsPerMeter
    push_u32(&mut out, 0); // biClrUsed
    push_u32(&mut out, 0); // biClrImportant

    // LIST movi — its size is the one close patches, and the frames follow the FourCC.
    out.extend_from_slice(&LIST);
    push_u32(&mut out, SIZE_PLACEHOLDER);
    out.extend_from_slice(&MOVI);

    out
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_i16(out: &mut Vec<u8>, value: i16) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Push one of the layout constants as a `DWORD` size field.
///
/// Every value reaching this is a sum of the fixed structure sizes at the top of the module
/// — a few hundred bytes — so the saturating fallback is unreachable. It exists because the
/// crate bans `as` and a header builder that cannot fail is worth more than one that
/// returns a `Result` nobody can trip.
fn push_size(out: &mut Vec<u8>, value: u64) {
    push_u32(out, u32::try_from(value).unwrap_or(u32::MAX));
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::time::Duration;

    use super::*;
    use crate::avi::read::{read_stream, recover_frames};
    use crate::decode::Decoded;
    use crate::encode;
    use crate::fixtures::{self, Lcg};

    const WIDTH: u32 = 64;
    const HEIGHT: u32 = 48;
    /// 15 fps, as microseconds — a rate a camera is asked for, and **not**
    /// [`PROVISIONAL_INTERVAL_US`].
    ///
    /// The value is 66 666 rather than the obvious 33 333 for one reason, and it is the
    /// reason this constant carries a paragraph. `PROVISIONAL_INTERVAL_US` is 33 333, so a
    /// suite that negotiated 33 333 everywhere cannot tell "the camera's number reached the
    /// header" from "our placeholder reached the header": a muxer that dropped
    /// `AviParams::negotiated_interval_us` on the floor and wrote the placeholder in its
    /// place would satisfy every assertion in this module, the frozen-fixture comparison
    /// included. That is precisely "a recording that silently reports nominal fps while
    /// delivering something else" — the defect D7's CFR carve-out and the notes' item 10
    /// exist to prevent — and `interval_source` naming the variant is only half the claim,
    /// because the *number* the variant carries is the half a caller reads.
    /// `a_negotiated_interval_is_the_camera_s_number_and_not_the_placeholder` is the
    /// assertion that holds the two apart, and it can only hold them apart while these two
    /// constants differ.
    const NEGOTIATED_US: u32 = 66_666;

    /// Bounds no test trips by accident, so a cap assertion is always deliberate.
    fn generous_caps() -> RecordingCaps {
        RecordingCaps {
            max_bytes: 1 << 20,
            max_frames: 1024,
            max_span: Duration::from_secs(60),
        }
    }

    fn params(caps: RecordingCaps) -> AviParams {
        AviParams {
            width: WIDTH,
            height: HEIGHT,
            negotiated_interval_us: Some(NEGOTIATED_US),
            caps,
        }
    }

    /// A synthetic MJPEG frame, generated here and never captured (design §5).
    ///
    /// The round-trip and fixture tests carry real JPEG bitstreams so that E6's
    /// byte-fidelity claim is made about the thing a camera actually produces. The
    /// accounting tests below hand the muxer arbitrary bytes instead, deliberately: it is
    /// the *lengths* that are under test there, and this muxer never parses a payload.
    fn mjpeg(quality: u8) -> Vec<u8> {
        encode::jpeg(&Decoded::Gray(fixtures::text_like(WIDTH, HEIGHT)), quality)
            .expect("a 64x48 fixture encodes")
    }

    fn frame(bytes: Vec<u8>, sequence: u32, timestamp_us: i64) -> Frame {
        Frame {
            bytes,
            pixel_format: PixelFormat::MJPG,
            width: WIDTH,
            height: HEIGHT,
            bytes_per_line: 0,
            sequence,
            timestamp_us,
        }
    }

    /// A payload of exactly `len` bytes, deterministic and not a JPEG.
    fn payload(len: usize, seed: u64) -> Vec<u8> {
        let mut lcg = Lcg::new(seed);
        (0..len).map(|_| lcg.next_u8()).collect()
    }

    /// Run a whole recording over an in-memory sink.
    fn record(
        params: AviParams,
        frames: &[Frame],
    ) -> (Vec<u8>, Vec<FrameOutcome>, RecordingSummary) {
        let mut file = Vec::new();
        let mut writer = AviWriter::begin(Cursor::new(&mut file), params).expect("begin");
        let outcomes = frames
            .iter()
            .map(|frame| writer.write_frame(frame).expect("write_frame"))
            .collect();
        let summary = writer.finish().expect("finish");
        (file, outcomes, summary)
    }

    /// The `DWORD` at an absolute file offset.
    fn dword(bytes: &[u8], at: u64) -> u32 {
        let at = usize::try_from(at).expect("a test file this size fits");
        u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
    }

    fn refusal(result: &schema::error::Result<impl std::fmt::Debug>) -> String {
        match result {
            Ok(value) => panic!("expected a refusal, got {value:?}"),
            Err(err) => err.to_string(),
        }
    }

    #[test]
    fn the_canonical_recording_parses_and_every_frame_is_byte_identical() {
        // E6: MJPEG frames are remuxed verbatim — "a re-encode inserts motion artefacts
        // exactly where the agent is looking for them". Checked through `read::read_stream`,
        // which was written from the specification before this muxer existed, so agreement
        // here is agreement between two independent readings of the format.
        let payloads = [mjpeg(30), mjpeg(70), mjpeg(50)];
        let frames: Vec<Frame> = payloads
            .iter()
            .enumerate()
            .map(|(index, bytes)| {
                let step = i64::try_from(index).expect("three frames fit");
                frame(
                    bytes.clone(),
                    100 + u32::try_from(index).expect("fits"),
                    step * 33_333,
                )
            })
            .collect();
        let (file, outcomes, summary) = record(params(generous_caps()), &frames);

        assert!(
            outcomes
                .iter()
                .all(|outcome| *outcome == FrameOutcome::Written)
        );
        assert_eq!(summary.frames_written, 3);
        assert_eq!(
            summary.bytes_written,
            u64::try_from(file.len()).expect("fits"),
            "the summary's byte count is the file's own length"
        );
        assert_eq!(summary.dropped_frames, 0);
        assert_eq!(summary.cap_reached, None);

        let stream = read_stream(&file).expect("our own output parses");
        assert_eq!(stream.frames.len(), 3);
        for (parsed, original) in stream.frames.iter().zip(&payloads) {
            assert_eq!(
                parsed.bytes,
                original.as_slice(),
                "a frame changed on the way in"
            );
        }
        assert_eq!(stream.headers.width, WIDTH);
        assert_eq!(stream.headers.height, HEIGHT);
        assert_eq!(stream.headers.compression, *b"MJPG");
        assert_eq!(stream.headers.handler, *b"MJPG");
        assert_eq!(stream.headers.bit_count, 24);
        assert_eq!(stream.headers.total_frames, 3);
        assert_eq!(stream.headers.stream_length, 3);
        assert!(stream.headers.has_index());
        assert_eq!(stream.index.len(), 3);
        assert!(
            stream.index.iter().all(|entry| entry.flags == 0x10),
            "every MJPEG frame is a keyframe"
        );
        // The first frame chunk sits four bytes into `movi`, which is the number every
        // index offset in the file is measured against.
        assert_eq!(stream.frames[0].movi_offset, 4);
        assert_eq!(stream.frames[0].chunk_id, *b"00dc");
    }

    #[test]
    fn the_header_offsets_are_the_fields_they_name() {
        // `finish` seeks to absolute offsets. If the header builder ever moved a field, the
        // close-time rewrite would patch whatever now sits there — silently, because the
        // sizes would still add up. These are the specification's structure sizes added up,
        // stated once, and read back out of a real file.
        assert_eq!(AVIH_AT, 32);
        assert_eq!(STRH_AT, 108);
        assert_eq!(STRF_AT, 172);
        assert_eq!(MOVI_FOURCC_AT, 220);
        assert_eq!(HEADER_BYTES, 224);
        assert_eq!(MIN_RECORDING_BYTES, HEADER_BYTES);
        assert_eq!(MAX_RECORDING_BYTES, u64::from(u32::MAX));
        assert_eq!(
            MAX_EXTENT,
            u32::try_from(i16::MAX).expect("i16::MAX is positive")
        );

        let (file, _, summary) = record(params(generous_caps()), &[frame(mjpeg(40), 1, 0)]);
        assert_eq!(&file[0..4], b"RIFF");
        assert_eq!(&file[8..12], b"AVI ");
        assert_eq!(
            dword(&file, RIFF_SIZE_AT),
            u32::try_from(file.len() - 8).expect("fits")
        );
        assert_eq!(&file[12..16], b"LIST");
        assert_eq!(
            dword(&file, HDRL_AT + 4),
            u32::try_from(HDRL_SIZE).expect("fits")
        );
        assert_eq!(
            dword(&file, AVIH_MICRO_SEC_AT),
            summary.declared_interval_us
        );
        assert_eq!(dword(&file, AVIH_FLAGS_AT), AVIF_HASINDEX);
        assert_eq!(dword(&file, AVIH_TOTAL_FRAMES_AT), 1);
        assert_eq!(
            dword(&file, AVIH_SUGGESTED_BUFFER_AT),
            u32::try_from(mjpeg(40).len()).expect("fits")
        );
        assert_eq!(dword(&file, STRH_SCALE_AT), summary.declared_interval_us);
        assert_eq!(dword(&file, STRH_RATE_AT), MICROS_PER_SECOND);
        assert_eq!(dword(&file, STRH_LENGTH_AT), 1);
        assert_eq!(&file[212..216], b"LIST");
        assert_eq!(&file[220..224], b"movi");
        // The `movi` size covers its own FourCC, every chunk header, every payload and
        // every pad byte — and stops where `idx1` starts.
        let idx1_at = 224 + 8 + mjpeg(40).len() + mjpeg(40).len() % 2;
        assert_eq!(&file[idx1_at..idx1_at + 4], b"idx1");
        assert_eq!(
            dword(&file, MOVI_SIZE_AT),
            u32::try_from(idx1_at - 220).expect("fits")
        );
    }

    #[test]
    fn an_odd_payload_is_padded_outside_its_own_size_field_and_inside_the_lists() {
        // The most common muxer bug there is, in all three of its places: the chunk's size
        // field must exclude the pad, the index entry's length must exclude it, and the
        // enclosing `LIST movi` size must include it. `read_stream` refuses each mistake
        // separately, and the offsets below are computed here rather than read back.
        let odd = payload(5, 1);
        let even = payload(4, 2);
        let frames = [frame(odd.clone(), 0, 0), frame(even.clone(), 1, 33_333)];
        let (file, _, _) = record(params(generous_caps()), &frames);
        let stream = read_stream(&file).expect("parses");

        assert_eq!(stream.frames[0].bytes, odd.as_slice());
        assert_eq!(stream.frames[1].bytes, even.as_slice());
        // 4 (the `movi` FourCC) then 8 header + 5 payload + 1 pad.
        assert_eq!(stream.frames[0].movi_offset, 4);
        assert_eq!(stream.frames[1].movi_offset, 18);
        // The chunk's own size field, and the index's copy of it, both exclude the pad.
        assert_eq!(dword(&file, 224 + 4), 5);
        assert_eq!(stream.index[0].size, 5);
        assert_eq!(stream.index[1].size, 4);
        // And the pad byte is zero, which is what makes a *missing* pad detectable: the
        // next chunk's FourCC otherwise sits exactly where the pad belongs.
        assert_eq!(file[224 + 8 + 5], 0);
    }

    #[test]
    fn the_measured_mean_replaces_the_negotiated_interval_in_both_of_its_homes() {
        // D7's CFR carve-out, and the notes' reason for it: "a recording that silently
        // reports nominal fps while delivering something else is answering the agent's
        // question wrongly". Asked for 30 fps, delivered at 20.
        let frames: Vec<Frame> = (0..4)
            .map(|index: i64| {
                frame(
                    payload(64, 3),
                    u32::try_from(index).expect("fits"),
                    index * 50_000,
                )
            })
            .collect();
        let (file, _, summary) = record(params(generous_caps()), &frames);

        assert_eq!(summary.interval_source, IntervalSource::Measured);
        assert_eq!(summary.declared_interval_us, 50_000);
        assert_eq!(summary.span_us, Some(150_000));

        let headers = read_stream(&file).expect("parses").headers;
        assert_eq!(
            headers.micro_sec_per_frame, 50_000,
            "avih was not rewritten"
        );
        assert_eq!(headers.stream_scale, 50_000, "strh was not rewritten");
        assert_eq!(headers.stream_rate, MICROS_PER_SECOND);
        assert_ne!(
            headers.micro_sec_per_frame, NEGOTIATED_US,
            "the negotiated interval survived the close"
        );
    }

    #[test]
    fn a_rate_left_stale_in_one_of_its_two_homes_is_a_file_the_reader_refuses() {
        // The test that would catch only one of the two being patched. It mutates our own
        // correct output back to the pre-close value in `avih` alone — which is exactly the
        // file a muxer that forgot `strh` would have written — and the reader must say so.
        // Without this, `the_measured_mean_…` above would still pass on a muxer that wrote
        // the measured mean into both homes *and* on one that wrote it into neither.
        let frames: Vec<Frame> = (0..4)
            .map(|index: i64| {
                frame(
                    payload(64, 4),
                    u32::try_from(index).expect("fits"),
                    index * 50_000,
                )
            })
            .collect();
        let (mut file, _, _) = record(params(generous_caps()), &frames);
        let at = usize::try_from(AVIH_MICRO_SEC_AT).expect("fits");
        file[at..at + 4].copy_from_slice(&NEGOTIATED_US.to_le_bytes());

        let message = refusal(&read_stream(&file));
        assert!(message.contains("dwScale"), "{message}");
        assert!(message.contains(&NEGOTIATED_US.to_string()), "{message}");
        assert!(message.contains("50000"), "{message}");
    }

    #[test]
    fn dropped_frames_are_counted_from_sequence_gaps() {
        // "It must not make a dropped frame look like a slow transition" (notes, item 10).
        // The timestamps below are evenly spaced on purpose: the gap is only visible in the
        // sequence numbers, so a muxer inferring drops from timing would report none.
        let frames: Vec<Frame> = [0u32, 1, 5, 6, 9]
            .iter()
            .enumerate()
            .map(|(index, sequence)| {
                frame(
                    payload(32, 5),
                    *sequence,
                    i64::try_from(index).expect("fits") * 33_333,
                )
            })
            .collect();
        let (_, _, summary) = record(params(generous_caps()), &frames);
        // 1→5 loses three, 6→9 loses two.
        assert_eq!(summary.dropped_frames, 5);
        assert_eq!(summary.frames_written, 5);
    }

    #[test]
    fn a_repeated_or_backwards_sequence_number_is_not_a_dropped_frame() {
        // A driver that repeats or rewinds a sequence number has done something other than
        // drop a frame, and counting it as `u32::MAX` drops would turn one odd frame into a
        // recording that claims four billion losses.
        let frames = [
            frame(payload(16, 6), 7, 0),
            frame(payload(16, 6), 7, 33_333),
            frame(payload(16, 6), 3, 66_666),
            frame(payload(16, 6), 4, 99_999),
        ];
        let (_, _, summary) = record(params(generous_caps()), &frames);
        assert_eq!(summary.dropped_frames, 0);
        assert_eq!(summary.frames_written, 4);
    }

    #[test]
    fn every_cap_refuses_at_the_right_frame_and_leaves_a_file_that_still_parses() {
        // The population is `CapReached::ALL`, generated from the enum: a cap nothing can
        // reach is a bound that is not a bound, and a hand-written list of the three would
        // let one quietly stop being enforced (rubric rule 6).
        let body = payload(37, 7);
        let two_frame_len = {
            let frames = [frame(body.clone(), 0, 0), frame(body.clone(), 1, 50_000)];
            let (file, _, _) = record(params(generous_caps()), &frames);
            u64::try_from(file.len()).expect("fits")
        };

        for &cap in CapReached::ALL {
            let caps = match cap {
                CapReached::Frames => RecordingCaps {
                    max_frames: 2,
                    ..generous_caps()
                },
                CapReached::Size => RecordingCaps {
                    // Exactly the file two frames make, index included.
                    max_bytes: two_frame_len,
                    ..generous_caps()
                },
                CapReached::Span => RecordingCaps {
                    max_span: Duration::from_micros(50_000),
                    ..generous_caps()
                },
            };
            let frames = [
                frame(body.clone(), 0, 0),
                frame(body.clone(), 1, 50_000),
                frame(body.clone(), 2, 100_000),
            ];
            let (file, outcomes, summary) = record(params(caps), &frames);

            assert_eq!(
                outcomes,
                vec![
                    FrameOutcome::Written,
                    FrameOutcome::Written,
                    FrameOutcome::Refused(cap)
                ],
                "{cap:?} did not refuse the third frame"
            );
            assert_eq!(summary.cap_reached, Some(cap));
            assert_eq!(summary.frames_written, 2);
            assert!(
                u64::try_from(file.len()).expect("fits") <= caps.max_bytes,
                "{cap:?} left a {}-byte file over its {} cap",
                file.len(),
                caps.max_bytes
            );
            let stream = read_stream(&file).expect("a capped recording is still a file");
            assert_eq!(stream.frames.len(), 2, "{cap:?}");
            assert_eq!(stream.index.len(), 2, "{cap:?}");
        }
    }

    #[test]
    fn once_a_cap_is_reached_a_smaller_later_frame_is_refused_too() {
        // A recording that resumed after its cap would have a hole in the middle, and the
        // whole point of the timing record is that the frames in the file are the frames
        // that arrived, in order, with the gaps counted rather than closed.
        //
        // Both caps here are ones a *later* frame would slip past on its own: a small frame
        // still fits under a size cap a large one broke, and a frame stamped earlier is
        // still inside a span cap a late one left. `max_frames` cannot show this — it only
        // ever grows — so a test written against it would pass on a muxer with no
        // once-refused-always-refused rule at all.
        let caps = RecordingCaps {
            max_bytes: 500,
            ..generous_caps()
        };
        let frames = [
            frame(payload(200, 8), 0, 0),
            frame(payload(200, 8), 1, 33_333),
            frame(payload(2, 8), 2, 66_666),
        ];
        let (_, outcomes, summary) = record(params(caps), &frames);
        assert_eq!(
            outcomes,
            vec![
                FrameOutcome::Written,
                FrameOutcome::Refused(CapReached::Size),
                FrameOutcome::Refused(CapReached::Size)
            ],
            "a two-byte frame fits under the cap, and must still be refused"
        );
        assert_eq!(summary.frames_written, 1);

        let caps = RecordingCaps {
            max_span: Duration::from_micros(50_000),
            ..generous_caps()
        };
        let frames = [
            frame(payload(64, 8), 0, 0),
            frame(payload(64, 8), 1, 100_000),
            frame(payload(64, 8), 2, 20_000),
        ];
        let (_, outcomes, summary) = record(params(caps), &frames);
        assert_eq!(
            outcomes,
            vec![
                FrameOutcome::Written,
                FrameOutcome::Refused(CapReached::Span),
                FrameOutcome::Refused(CapReached::Span)
            ],
            "a frame stamped inside the span must still be refused after one outside it"
        );
        assert_eq!(summary.frames_written, 1);
    }

    #[test]
    fn the_size_cap_counts_the_index_the_close_will_write() {
        // `idx1` is sixteen bytes a frame and is written after the last frame, so a cap
        // that stopped at the frames would be walked past by the close itself. One byte
        // under what two frames cost: the second frame must not be written.
        let body = payload(37, 9);
        let two_frame_len = {
            let frames = [frame(body.clone(), 0, 0), frame(body.clone(), 1, 50_000)];
            let (file, _, _) = record(params(generous_caps()), &frames);
            u64::try_from(file.len()).expect("fits")
        };
        let caps = RecordingCaps {
            max_bytes: two_frame_len - 1,
            ..generous_caps()
        };
        let frames = [frame(body.clone(), 0, 0), frame(body.clone(), 1, 50_000)];
        let (file, outcomes, summary) = record(params(caps), &frames);

        assert_eq!(
            outcomes,
            vec![
                FrameOutcome::Written,
                FrameOutcome::Refused(CapReached::Size)
            ],
            "the projection did not count the index"
        );
        assert_eq!(summary.frames_written, 1);
        assert!(u64::try_from(file.len()).expect("fits") <= caps.max_bytes);
        read_stream(&file).expect("parses");
    }

    #[test]
    fn a_cap_no_index_offset_can_address_is_refused_at_begin() {
        // A `movi` list past 4 GiB wraps every index offset after the boundary, and the
        // file's own reader then rejects it. Refused where the caller can still do
        // something about it, not at frame 200 000.
        let caps = RecordingCaps {
            max_bytes: MAX_RECORDING_BYTES + 1,
            ..generous_caps()
        };
        let result = AviWriter::begin(Cursor::new(Vec::new()), params(caps));
        let message = refusal(&result);
        assert!(message.contains(CAP_UNADDRESSABLE), "{message}");
        assert!(
            message.contains(&MAX_RECORDING_BYTES.to_string()),
            "{message}"
        );

        // And the boundary itself is accepted, so the refusal is a bound rather than a
        // blanket.
        let caps = RecordingCaps {
            max_bytes: MAX_RECORDING_BYTES,
            ..generous_caps()
        };
        AviWriter::begin(Cursor::new(Vec::new()), params(caps)).expect("the boundary is legal");
    }

    #[test]
    fn a_cap_below_the_empty_file_is_refused_at_begin() {
        // The header list is complete before the first frame, so a cap under it could never
        // be honoured — the file would be over budget the moment `begin` returned.
        let caps = RecordingCaps {
            max_bytes: MIN_RECORDING_BYTES - 1,
            ..generous_caps()
        };
        let message = refusal(&AviWriter::begin(Cursor::new(Vec::new()), params(caps)));
        assert!(message.contains(CAP_TOO_SMALL), "{message}");
        assert!(
            message.contains(&MIN_RECORDING_BYTES.to_string()),
            "{message}"
        );

        let caps = RecordingCaps {
            max_bytes: MIN_RECORDING_BYTES,
            ..generous_caps()
        };
        let mut file = Vec::new();
        let writer =
            AviWriter::begin(Cursor::new(&mut file), params(caps)).expect("the boundary is legal");
        writer
            .finish()
            .expect("an empty recording fits its own floor");
        assert_eq!(
            u64::try_from(file.len()).expect("fits"),
            MIN_RECORDING_BYTES
        );
    }

    #[test]
    fn a_recording_of_no_frames_is_a_file_the_reader_accepts() {
        // Zero frames is a real outcome — a camera that delivered nothing before the caller
        // stopped — and it must not be a corrupt file. No frames means no index, and no
        // index means `AVIF_HASINDEX` stays clear: a file that claims an index it does not
        // have sends a reader seeking into nothing.
        let (file, _, summary) = record(params(generous_caps()), &[]);
        assert_eq!(summary.frames_written, 0);
        assert_eq!(summary.span_us, None);
        assert_eq!(summary.declared_interval_us, NEGOTIATED_US);
        assert_eq!(summary.interval_source, IntervalSource::Negotiated);

        let stream = read_stream(&file).expect("an empty recording is still a file");
        assert!(stream.frames.is_empty());
        assert!(stream.index.is_empty());
        assert!(!stream.headers.has_index());
        assert_eq!(stream.headers.total_frames, 0);
        assert_eq!(stream.headers.stream_length, 0);
        assert_eq!(dword(&file, AVIH_FLAGS_AT), 0);
        assert_eq!(
            dword(&file, AVIH_MAX_BYTES_PER_SEC_AT),
            0,
            "no frames, no data rate"
        );
    }

    #[test]
    fn a_negotiated_interval_is_the_camera_s_number_and_not_the_placeholder() {
        // The distinction the whole `IntervalSource` vocabulary exists to make, asserted
        // about the *number* rather than about the label. Nothing else in this module can
        // make it: every other test that asks for an interval asks for `NEGOTIATED_US`, and
        // a muxer that ignored `AviParams::negotiated_interval_us` entirely and wrote
        // `PROVISIONAL_INTERVAL_US` instead would pass all of them if the two were equal.
        // They are not equal, and this is where that is load-bearing rather than incidental.
        assert_ne!(
            NEGOTIATED_US, PROVISIONAL_INTERVAL_US,
            "the suite cannot tell a negotiated interval from a placeholder while these agree"
        );

        // Both homes of the rate, in a closed file, at a rate the camera named.
        let (file, _, summary) = record(params(generous_caps()), &[frame(mjpeg(50), 0, 7)]);
        assert_eq!(summary.declared_interval_us, NEGOTIATED_US);
        assert_eq!(summary.interval_source, IntervalSource::Negotiated);
        assert_eq!(dword(&file, AVIH_MICRO_SEC_AT), NEGOTIATED_US);
        assert_eq!(dword(&file, STRH_SCALE_AT), NEGOTIATED_US);

        // And in the header `begin` wrote, which is the only copy a **crashed** recording
        // ever has: `finish` never ran, so nothing rewrote either home. D7's recovery
        // promise is that such a file is readable, and a readable file that declares 30 fps
        // for a camera negotiated at 15 answers the agent's question wrongly — the one
        // failure D7's carve-out is about. Nothing else asserts what `begin` put there,
        // because every other path overwrites it at close.
        let mut crashed = Vec::new();
        {
            let mut writer = AviWriter::begin(Cursor::new(&mut crashed), params(generous_caps()))
                .expect("begin");
            writer
                .write_frame(&frame(mjpeg(50), 0, 0))
                .expect("write_frame");
        }
        assert_eq!(dword(&crashed, AVIH_MICRO_SEC_AT), NEGOTIATED_US);
        assert_eq!(dword(&crashed, STRH_SCALE_AT), NEGOTIATED_US);
        assert_eq!(dword(&crashed, STRH_RATE_AT), MICROS_PER_SECOND);
    }

    #[test]
    fn a_one_frame_recording_declares_the_negotiated_interval_because_a_mean_needs_two_points() {
        let (file, _, summary) = record(params(generous_caps()), &[frame(mjpeg(50), 0, 12_345)]);
        assert_eq!(summary.frames_written, 1);
        assert_eq!(summary.interval_source, IntervalSource::Negotiated);
        assert_eq!(summary.declared_interval_us, NEGOTIATED_US);
        assert_eq!(summary.span_us, None, "one instant is not a span");

        let headers = read_stream(&file).expect("parses").headers;
        assert_eq!(headers.micro_sec_per_frame, NEGOTIATED_US);
        assert_eq!(headers.stream_scale, NEGOTIATED_US);
    }

    #[test]
    fn every_interval_source_is_reachable_and_says_which_one_it_is() {
        // Walks the generated `ALL`: a source no recording can produce is a distinction
        // that is not being made, and this is the distinction D7's rewrite exists for.
        for &source in IntervalSource::ALL {
            let (negotiated, frames) = match source {
                IntervalSource::Measured => (
                    Some(NEGOTIATED_US),
                    vec![
                        frame(payload(8, 10), 0, 0),
                        frame(payload(8, 10), 1, 40_000),
                    ],
                ),
                IntervalSource::Negotiated => {
                    (Some(NEGOTIATED_US), vec![frame(payload(8, 10), 0, 0)])
                }
                IntervalSource::Provisional => (None, vec![frame(payload(8, 10), 0, 0)]),
            };
            let params = AviParams {
                width: WIDTH,
                height: HEIGHT,
                negotiated_interval_us: negotiated,
                caps: generous_caps(),
            };
            let (file, _, summary) = record(params, &frames);
            assert_eq!(summary.interval_source, source);
            let expected = match source {
                IntervalSource::Measured => 40_000,
                IntervalSource::Negotiated => NEGOTIATED_US,
                IntervalSource::Provisional => PROVISIONAL_INTERVAL_US,
            };
            assert_eq!(summary.declared_interval_us, expected, "{source:?}");
            let headers = read_stream(&file).expect("parses").headers;
            assert_eq!(headers.micro_sec_per_frame, expected, "{source:?}");
            assert_eq!(headers.stream_scale, expected, "{source:?}");
        }
    }

    #[test]
    fn a_clock_that_ran_backwards_measures_nothing_rather_than_a_negative_interval() {
        // A driver's clock is untrusted input like everything else it reports. The answer
        // has to be total: no panic, no negative interval, no wrapped `u32` — and the
        // recording still has to end with a file in it.
        let frames = [
            frame(payload(16, 11), 0, 5_000_000),
            frame(payload(16, 11), 1, 4_000_000),
            frame(payload(16, 11), 2, 3_000_000),
        ];
        let (file, outcomes, summary) = record(params(generous_caps()), &frames);
        assert!(
            outcomes
                .iter()
                .all(|outcome| *outcome == FrameOutcome::Written)
        );
        assert_eq!(summary.span_us, None, "a backwards clock spans no duration");
        assert_eq!(summary.interval_source, IntervalSource::Negotiated);
        assert_eq!(summary.declared_interval_us, NEGOTIATED_US);
        read_stream(&file).expect("parses");

        // The extreme of the same shape, which would overflow a subtraction rather than
        // merely reverse it.
        let frames = [
            frame(payload(16, 11), 0, i64::MAX),
            frame(payload(16, 11), 1, i64::MIN),
        ];
        let (file, _, summary) = record(params(generous_caps()), &frames);
        assert_eq!(summary.span_us, None);
        read_stream(&file).expect("parses");
    }

    #[test]
    fn timestamps_that_do_not_move_measure_nothing_rather_than_a_zero_interval() {
        // A zero frame interval is meaningless to every player, and `strh.dwRate` of zero
        // is a file `read_stream` refuses outright. Two shapes reach it: a clock that never
        // moved, and a span shorter than the number of intervals in it.
        let flat = [
            frame(payload(16, 12), 0, 900),
            frame(payload(16, 12), 1, 900),
            frame(payload(16, 12), 2, 900),
        ];
        let (file, _, summary) = record(params(generous_caps()), &flat);
        assert_eq!(summary.span_us, Some(0));
        assert_eq!(summary.interval_source, IntervalSource::Negotiated);
        read_stream(&file).expect("parses");

        let crowded = [
            frame(payload(16, 12), 0, 0),
            frame(payload(16, 12), 1, 0),
            frame(payload(16, 12), 2, 1),
        ];
        let (file, _, summary) = record(params(generous_caps()), &crowded);
        assert_eq!(
            summary.span_us,
            Some(1),
            "the span is a fact even when the mean is not"
        );
        assert_eq!(
            summary.interval_source,
            IntervalSource::Negotiated,
            "one microsecond over two intervals truncates to a zero rate"
        );
        assert_eq!(summary.declared_interval_us, NEGOTIATED_US);
        read_stream(&file).expect("parses");
    }

    #[test]
    fn a_mean_too_large_for_the_header_falls_back_to_the_negotiated_interval() {
        // `dwMicroSecPerFrame` is 32 bits — about 71 minutes. A span longer than that
        // between two frames is not a frame rate this container can state, and stating a
        // truncated one would be a number the caller could not tell from a measurement.
        let frames = [
            frame(payload(16, 13), 0, 0),
            frame(payload(16, 13), 1, 5_000_000_000),
        ];
        // A span cap that admits the gap, so the fallback under test is the header field's
        // own width rather than a cap refusal.
        let caps = RecordingCaps {
            max_span: Duration::from_secs(10_000),
            ..generous_caps()
        };
        let (file, _, summary) = record(params(caps), &frames);
        assert_eq!(
            summary.span_us,
            Some(5_000_000_000),
            "the span is still reported"
        );
        assert_eq!(summary.interval_source, IntervalSource::Negotiated);
        assert_eq!(summary.declared_interval_us, NEGOTIATED_US);
        read_stream(&file).expect("parses");
    }

    #[test]
    fn a_crash_leaves_every_complete_frame_recoverable_and_the_file_refused() {
        // D7 L0's documented product property: a crash "leaves a recoverable `movi`
        // stream". The writer is dropped without `finish`, which is exactly what a killed
        // process leaves behind — the placeholder sizes are still on the sink and no index
        // was ever written.
        let payloads = [mjpeg(30), payload(41, 14), mjpeg(60)];
        let mut file = Vec::new();
        {
            let mut writer =
                AviWriter::begin(Cursor::new(&mut file), params(generous_caps())).expect("begin");
            for (index, bytes) in payloads.iter().enumerate() {
                let sequence = u32::try_from(index).expect("fits");
                writer
                    .write_frame(&frame(
                        bytes.clone(),
                        sequence,
                        i64::from(sequence) * 33_333,
                    ))
                    .expect("write_frame");
            }
        }

        // Strict parsing must refuse: the `RIFF` size is still the placeholder.
        let message = refusal(&read_stream(&file));
        assert!(message.contains("RIFF size field declares 0"), "{message}");
        assert_eq!(dword(&file, RIFF_SIZE_AT), SIZE_PLACEHOLDER);
        assert_eq!(dword(&file, MOVI_SIZE_AT), SIZE_PLACEHOLDER);
        assert_eq!(
            dword(&file, AVIH_FLAGS_AT),
            0,
            "no index was written, so none is claimed"
        );

        // Recovery is total, and gets every frame.
        let recovered = recover_frames(&file);
        assert_eq!(recovered.len(), 3);
        for (frame, original) in recovered.iter().zip(&payloads) {
            assert_eq!(frame.bytes, original.as_slice());
        }

        // And at every truncation point, every *complete* frame comes back — never a
        // half-written one. The boundaries are computed here rather than read back.
        let mut boundary = usize::try_from(HEADER_BYTES).expect("fits");
        let mut chunks = Vec::new();
        for bytes in &payloads {
            let payload_end = boundary + 8 + bytes.len();
            boundary = payload_end + bytes.len() % 2;
            chunks.push((payload_end, boundary));
        }
        for (complete, (payload_end, chunk_end)) in chunks.iter().enumerate() {
            for (cut, expected) in [
                (*chunk_end, complete + 1),
                // One byte short of the payload: the frame is not all there, and half a
                // JPEG must never come back as a frame.
                (*payload_end - 1, complete),
            ] {
                let truncated = recover_frames(&file[..cut]);
                assert_eq!(
                    truncated.len(),
                    expected,
                    "a file cut at {cut} bytes yielded {} frames",
                    truncated.len()
                );
                assert!(
                    read_stream(&file[..cut]).is_err(),
                    "a truncated file is not a file"
                );
            }
            // The pad byte is the one thing a complete frame is allowed to be missing: the
            // crash landed between the payload write and the pad write, and every byte of
            // the frame itself is on the sink.
            if payload_end != chunk_end {
                assert_eq!(recover_frames(&file[..*payload_end]).len(), complete + 1);
            }
        }
        // Cut inside the header: nothing to recover, and still no panic.
        assert!(recover_frames(&file[..100]).is_empty());
        assert!(recover_frames(&[]).is_empty());
    }

    #[test]
    fn the_size_fields_agree_with_the_bytes_written_over_many_frame_length_sequences() {
        // docs/7 P6a's chunk-accounting criterion: "the muxer never emits a size field that
        // disagrees with bytes written, proven by re-parsing our own output". The lengths
        // are pseudo-random so that the odd/even pattern, and therefore the pad byte, lands
        // everywhere in the sequence — an accounting bug that only shows up on the third
        // odd frame is exactly the kind a hand-written fixture misses.
        let mut lcg = Lcg::new(0x5EED);
        let mut takes = 0;
        for _ in 0..32 {
            let count = usize::from(lcg.next_u8() % 6) + 1;
            let lengths: Vec<usize> = (0..count)
                .map(|_| usize::from(lcg.next_u8()) % 97 + 1)
                .collect();
            let frames: Vec<Frame> = lengths
                .iter()
                .enumerate()
                .map(|(index, len)| {
                    let index = u32::try_from(index).expect("fits");
                    frame(
                        payload(*len, u64::from(index) + 1),
                        index,
                        i64::from(index) * 20_000,
                    )
                })
                .collect();
            let (file, _, summary) = record(params(generous_caps()), &frames);

            // The independent reader checks every size field against the bytes present.
            let stream = read_stream(&file).expect("our own output parses");
            assert_eq!(stream.frames.len(), count);
            assert_eq!(
                summary.bytes_written,
                u64::try_from(file.len()).expect("fits")
            );

            // And the offsets, recomputed here from the layout rather than read back, so
            // that a muxer and a reader agreeing on the wrong number would still fail.
            let mut offset = 4u32;
            let mut movi = 4u64;
            for (index, len) in lengths.iter().enumerate() {
                let len32 = u32::try_from(*len).expect("fits");
                assert_eq!(stream.frames[index].movi_offset, offset, "frame {index}");
                assert_eq!(stream.index[index].offset, offset, "index {index}");
                assert_eq!(stream.index[index].size, len32, "index {index}");
                assert_eq!(stream.frames[index].bytes.len(), *len);
                let chunk = 8 + len32 + len32 % 2;
                offset += chunk;
                movi += u64::from(chunk);
            }
            assert_eq!(u64::from(dword(&file, MOVI_SIZE_AT)), movi);
            assert_eq!(
                u64::from(dword(&file, RIFF_SIZE_AT)),
                u64::try_from(file.len()).expect("fits") - 8
            );
            takes += 1;
        }
        assert_eq!(takes, 32, "the sweep must not be empty");
    }

    #[test]
    fn a_frame_that_is_not_mjpeg_is_refused_rather_than_remuxed() {
        // E6 and D7 L0: the muxer is MJPG-only by design, and a YUYV frame written into an
        // `00dc` chunk would produce a file that claims MJPG and carries pixels.
        let mut file = Vec::new();
        let mut writer =
            AviWriter::begin(Cursor::new(&mut file), params(generous_caps())).expect("begin");
        let mut yuyv = frame(payload(64, 15), 0, 0);
        yuyv.pixel_format = PixelFormat::YUYV;
        let message = refusal(&writer.write_frame(&yuyv));
        assert!(message.contains(NOT_MJPEG), "{message}");
        assert!(message.contains("YUYV"), "{message}");

        // Both spellings of MJPEG are accepted — `schema::camera` calls them "the two
        // spellings of the same thing", and a camera that reports `JPEG` is not offering a
        // different payload.
        let mut jpeg = frame(mjpeg(40), 0, 0);
        jpeg.pixel_format = PixelFormat::JPEG;
        assert_eq!(
            writer.write_frame(&jpeg).expect("JPEG"),
            FrameOutcome::Written
        );
        writer.finish().expect("finish");
        assert_eq!(read_stream(&file).expect("parses").frames.len(), 1);
    }

    #[test]
    fn a_frame_whose_geometry_is_not_the_streams_is_refused() {
        // The header states one size for the whole file (AVI has no per-frame geometry), so
        // a frame of another size would be described by a header that is wrong about it —
        // and a driver renegotiating mid-stream is a thing that happens.
        let mut file = Vec::new();
        let mut writer =
            AviWriter::begin(Cursor::new(&mut file), params(generous_caps())).expect("begin");
        let mut wrong = frame(payload(64, 16), 0, 0);
        wrong.width = WIDTH * 2;
        let message = refusal(&writer.write_frame(&wrong));
        assert!(message.contains(EXTENT_MISMATCH), "{message}");
        assert!(message.contains("128x48"), "{message}");
        assert!(message.contains("64x48"), "{message}");
    }

    #[test]
    fn an_empty_frame_payload_is_refused_rather_than_written_as_a_zero_length_chunk() {
        let mut file = Vec::new();
        let mut writer =
            AviWriter::begin(Cursor::new(&mut file), params(generous_caps())).expect("begin");
        let message = refusal(&writer.write_frame(&frame(Vec::new(), 12, 0)));
        assert!(message.contains(EMPTY_FRAME), "{message}");
        assert!(message.contains("12"), "{message}");
    }

    #[test]
    fn a_degenerate_or_oversized_frame_size_is_refused_at_begin() {
        for (width, height) in [(0, HEIGHT), (WIDTH, 0), (0, 0)] {
            let params = AviParams {
                width,
                height,
                negotiated_interval_us: Some(NEGOTIATED_US),
                caps: generous_caps(),
            };
            let message = refusal(&AviWriter::begin(Cursor::new(Vec::new()), params));
            assert!(message.contains(ZERO_EXTENT), "{message}");
        }

        // `rcFrame` is four 16-bit shorts, so the format itself stops here. The boundary is
        // accepted, one past it is not — a header this muxer cannot fill honestly is a file
        // it must not open.
        let at_limit = AviParams {
            width: MAX_EXTENT,
            height: MAX_EXTENT,
            negotiated_interval_us: Some(NEGOTIATED_US),
            caps: generous_caps(),
        };
        AviWriter::begin(Cursor::new(Vec::new()), at_limit).expect("the boundary is legal");
        let past_limit = AviParams {
            width: MAX_EXTENT + 1,
            ..at_limit
        };
        let message = refusal(&AviWriter::begin(Cursor::new(Vec::new()), past_limit));
        assert!(message.contains(EXTENT_TOO_LARGE), "{message}");
        assert!(message.contains("32767"), "{message}");
    }

    #[test]
    fn a_negotiated_interval_of_zero_is_refused_at_begin() {
        // `strh.dwRate` of zero is a file the reader refuses, and a zero interval in the
        // other home is meaningless to every player. A caller that computed a zero has a
        // bug, and substituting the provisional value quietly would hide it.
        let params = AviParams {
            width: WIDTH,
            height: HEIGHT,
            negotiated_interval_us: Some(0),
            caps: generous_caps(),
        };
        let message = refusal(&AviWriter::begin(Cursor::new(Vec::new()), params));
        assert!(message.contains(ZERO_INTERVAL), "{message}");
    }

    /// A sink that accepts `budget` bytes and then refuses, the way a full disk does.
    struct ShortSink<'a> {
        inner: Cursor<&'a mut Vec<u8>>,
        budget: usize,
    }

    impl std::io::Write for ShortSink<'_> {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if self.budget == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::StorageFull,
                    "no space left on device",
                ));
            }
            let take = buf.len().min(self.budget);
            self.budget -= take;
            self.inner.write(&buf[..take])
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.inner.flush()
        }
    }

    impl Seek for ShortSink<'_> {
        fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(from)
        }
    }

    #[test]
    fn a_sink_that_fails_refuses_every_later_call_and_names_the_recoverable_prefix() {
        // P6b's disk-full case, from this side of the boundary. `write_all` does not say
        // how many bytes landed, so after a failure the file's length is genuinely unknown
        // — and a `finish` that patched size fields on top of that guess would produce the
        // one thing this whole module exists to prevent: a size field that disagrees with
        // the bytes written. It refuses instead, and names the prefix recovery can walk.
        let mut file = Vec::new();
        let good = payload(40, 17);
        let budget = usize::try_from(HEADER_BYTES).expect("fits") + 8 + good.len() + 20;
        let mut writer = AviWriter::begin(
            ShortSink {
                inner: Cursor::new(&mut file),
                budget,
            },
            params(generous_caps()),
        )
        .expect("begin");
        assert_eq!(
            writer
                .write_frame(&frame(good.clone(), 0, 0))
                .expect("the first frame fits"),
            FrameOutcome::Written
        );
        let message = refusal(&writer.write_frame(&frame(payload(80, 18), 1, 33_333)));
        assert!(message.contains(SINK_FAILED), "{message}");

        // Poisoned: no later call touches the sink again.
        let message = refusal(&writer.write_frame(&frame(good.clone(), 2, 66_666)));
        assert!(message.contains(SINK_FAILED), "{message}");
        let message = refusal(&writer.finish());
        assert!(message.contains(SINK_FAILED), "{message}");
        assert!(message.contains("recoverable movi stream"), "{message}");

        // And the promise the message makes is true: the frame that landed is recoverable.
        let recovered = recover_frames(&file);
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].bytes, good.as_slice());
    }

    #[test]
    fn the_debug_rendering_never_contains_the_recording() {
        // Rubric A12: a frame may contain a person, and a derived `Debug` on a writer would
        // print the sink — which is the whole recording.
        let mut file = Vec::new();
        let mut writer =
            AviWriter::begin(Cursor::new(&mut file), params(generous_caps())).expect("begin");
        let marker = vec![0xAB; 64];
        let mut recognisable = frame(marker.clone(), 0, 0);
        recognisable.bytes = marker;
        writer.write_frame(&recognisable).expect("write_frame");
        let rendered = format!("{writer:?}");
        assert!(rendered.contains("frames_written: 1"), "{rendered}");
        assert!(!rendered.contains("171"), "{rendered}");
        assert!(!rendered.contains("0xab"), "{rendered}");
        assert!(!rendered.contains("sink:"), "{rendered}");
    }

    #[test]
    fn the_suggested_buffer_size_and_data_rate_describe_the_file_that_was_written() {
        // Both are advisory fields, and both are the muxer's own measurement of what it
        // produced — a player that trusts an understated buffer size reads half a frame.
        let big = payload(500, 19);
        let small = payload(20, 20);
        let frames = [
            frame(small.clone(), 0, 0),
            frame(big.clone(), 1, 100_000),
            frame(small, 2, 200_000),
        ];
        let (file, _, summary) = record(params(generous_caps()), &frames);
        let headers = read_stream(&file).expect("parses").headers;
        assert_eq!(
            headers.suggested_buffer_size, 500,
            "the largest frame in the file"
        );
        assert_eq!(dword(&file, AVIH_SUGGESTED_BUFFER_AT), 500);

        // 100 ms a frame over three frames is 300 ms of declared duration.
        let expected = u32::try_from(summary.bytes_written * 1_000_000 / 300_000).expect("fits");
        assert_eq!(dword(&file, AVIH_MAX_BYTES_PER_SEC_AT), expected);
        assert_eq!(summary.declared_interval_us, 100_000);
    }

    #[test]
    fn a_recording_starts_where_the_sink_was_positioned() {
        // The engine owns the sink, and a sink handed over mid-stream must still receive a
        // correct file: every seek `finish` performs is relative to where the file began,
        // not to where the sink began.
        let mut file = Vec::new();
        let prefix = b"not an AVI";
        {
            let mut cursor = Cursor::new(&mut file);
            cursor.write_all(prefix).expect("prefix");
            let mut writer = AviWriter::begin(cursor, params(generous_caps())).expect("begin");
            writer
                .write_frame(&frame(mjpeg(35), 0, 0))
                .expect("write_frame");
            writer
                .write_frame(&frame(mjpeg(45), 1, 33_333))
                .expect("write_frame");
            writer.finish().expect("finish");
        }
        assert_eq!(&file[..prefix.len()], prefix, "the prefix was overwritten");
        let stream = read_stream(&file[prefix.len()..]).expect("the file after the prefix parses");
        assert_eq!(stream.frames.len(), 2);
    }

    /// The committed AVI, frozen against silent format drift.
    ///
    /// What it proves is narrow and worth being exact about: that the bytes this muxer
    /// emits for a known input have not changed. It proves nothing about *correctness* —
    /// a fixture produced by the code under test cannot — which is `read.rs`'s job here
    /// and the ffprobe/mpv oracles' job at P6d. `crates/imaging/fixtures/README.md`
    /// carries the same warning where a human reading the directory will find it.
    const FROZEN: &[u8] = include_bytes!("../../fixtures/avi/two-frame-64x48.avi");

    #[test]
    fn the_frozen_fixture_is_still_the_file_the_muxer_emits() {
        // A format change that no other test notices — a field reordered, a flag set at
        // begin instead of close, a `dwQuality` that grew a value — moves these bytes.
        let frames = fixture_frames();
        let (file, _, summary) = record(fixture_params(), &frames);
        assert_eq!(summary.bytes_written, 1852);
        assert_eq!(
            summary.declared_interval_us, 50_000,
            "the close-time rewrite is in there"
        );
        assert_eq!(summary.interval_source, IntervalSource::Measured);
        if file.as_slice() != FROZEN {
            let first = file
                .iter()
                .zip(FROZEN)
                .position(|(emitted, frozen)| emitted != frozen);
            panic!(
                "the muxer no longer emits the frozen fixture: {} bytes against {}, first \
                 difference at {first:?}. If the change is deliberate, regenerate the file \
                 the way crates/imaging/fixtures/README.md describes and say what moved.",
                file.len(),
                FROZEN.len()
            );
        }

        // And the committed bytes carry nothing but pixels this repository generated — the
        // privacy claim in the README, asserted rather than promised (design §5). Since the
        // P6a review `no-frame-bytes-in-repo.sh` sniffs the container too, and holds it to
        // its directory, its provenance sidecar and its declared frame extent; what that
        // gate cannot say is that these particular pixels were *generated*, which is what
        // the comparison below says.
        let stream = read_stream(FROZEN).expect("the frozen fixture parses");
        assert_eq!(stream.frames.len(), 2);
        assert_eq!(stream.frames[0].bytes, frames[0].bytes.as_slice());
        assert_eq!(stream.frames[1].bytes, frames[1].bytes.as_slice());
        // The lengths pin the generator arguments the README names, and the odd one pins
        // the pad path into the frozen bytes.
        assert_eq!(stream.frames[0].bytes.len(), 409);
        assert_eq!(stream.frames[1].bytes.len(), 1162);
        assert_eq!(stream.headers.micro_sec_per_frame, 50_000);
        assert_eq!(stream.headers.stream_scale, 50_000);
        // The numbers the README's "what it pins" row states, so the two cannot drift.
        assert_eq!(stream.index[0].offset, 4);
        assert_eq!(stream.index[1].offset, 422);
        assert!(stream.headers.has_index());
    }

    fn fixture_params() -> AviParams {
        AviParams {
            width: 64,
            height: 48,
            negotiated_interval_us: Some(33_333),
            caps: RecordingCaps {
                max_bytes: 1 << 20,
                max_frames: 64,
                max_span: Duration::from_secs(60),
            },
        }
    }

    fn fixture_frames() -> Vec<Frame> {
        let odd = encode::jpeg(&Decoded::Gray(fixtures::checkerboard(64, 48, 8)), 15)
            .expect("the checkerboard fixture encodes");
        let even = encode::jpeg(&Decoded::Gray(fixtures::text_like(64, 48)), 30)
            .expect("the text-like fixture encodes");
        vec![frame(odd, 0, 0), frame(even, 1, 50_000)]
    }
}
