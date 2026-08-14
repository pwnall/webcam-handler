//! An AVI reader written from the specification, so that it can disagree with our muxer.
//!
//! ## Two jobs, and they want opposite things
//!
//! [`read_stream`] is **strict**: every structural claim the file makes is checked against
//! the bytes actually present, and the first disagreement is a refusal naming the offset
//! and both numbers. It exists to fail on our own muxer's output the day that output stops
//! being right — docs/7 P6a's chunk-accounting criterion is "the muxer never emits a size
//! field that disagrees with bytes written, proven by re-parsing our own output", and a
//! re-parser that shrugs proves nothing.
//!
//! [`recover_frames`] is **total**: it never fails and never panics, because it is the
//! implementation of a documented product promise. D7 L0 says a crash "leaves a recoverable
//! `movi` stream (documented)", and a documented recovery with nothing that can perform it
//! is prose. It reads `hdrl` — complete before the first frame, by construction — to find
//! where `movi` begins, then walks frame chunks to end-of-file trusting **no** container
//! size field and **no** index. A file truncated in the middle of frame 7 yields six
//! frames.
//!
//! ## A size field is a claim, not a fact
//!
//! Every list in RIFF states its own length, and a muxer that patches one of those numbers
//! at close and forgets another leaves a file that is internally inconsistent in exactly
//! one place. So no walk here is driven by the number it is checking. The `movi` walk is
//! driven by chunk ids and chunk sizes and runs until it meets something that is not a
//! stream chunk (`idx1`, normally) or the end of the file; only then is the distance it
//! covered compared with the `LIST movi` size field. Had the walk been bounded by that
//! field, the comparison would have been a tautology — which is how a size-field check
//! quietly stops being one.
//!
//! ## The pad byte is where muxers die
//!
//! RIFF pads odd-length chunks to an even boundary. The pad byte is **not** counted in the
//! chunk's own size field and **is** counted in the enclosing list's, and getting that
//! backwards is the single most common muxer bug. Both halves are checked: the pad must be
//! present and it must be zero (the specification pads with a null byte), which is what
//! makes a *missing* pad detectable at all — otherwise the first byte of the next chunk's
//! FourCC sits where the pad belongs and the walk continues happily one byte out of phase.
//!
//! The other half — a pad byte counted **inside** the chunk's size field — is invisible to
//! the walk, because a 5-byte chunk claiming 6 bytes ends exactly where a correct one does.
//! It is caught by `idx1`, whose `dwChunkLength` excludes the pad by specification, so the
//! index and the chunk header disagree by one. That cross-check is the reason this module
//! validates the index against the chunks rather than merely parsing it — and it is also
//! why a crash-truncated file, which has no index, cannot be checked for that particular
//! defect by anything.
//!
//! ## One rate, two homes
//!
//! D7's CFR carve-out puts the frame interval in `avih.dwMicroSecPerFrame` **and** in
//! `strh.dwScale`/`dwRate`, and rewrites both at close to the measured mean. A writer that
//! rewrites one and forgets the other produces a file that answers "how long did this take"
//! two different ways, and the notes are blunt about the cost: a recording that reports
//! nominal fps while delivering something else "must not make a dropped frame look like a
//! slow transition". So the two are compared, with a tolerance of exactly one microsecond
//! (`RATE_TOLERANCE_US`, whose own paragraph argues the number) — enough to absorb the
//! difference between a writer that rounded and the truncating integer division below, and
//! nowhere near the thousands of microseconds by which a stale header misses.
//!
//! ## A frame may contain a person
//!
//! Rubric A12. No refusal in this module quotes a payload byte; they carry offsets, counts,
//! lengths, dimensions and **this reader's own vocabulary**. That last qualification is the
//! load-bearing one and it was learned the hard way: a FourCC is header data only while the
//! walk that found it is synchronised, and a size field is precisely what desynchronises
//! one — a `00dc` claiming 206 bytes where it holds 409 puts the next "chunk id" 206 bytes
//! into a JPEG. So a refusal about a chunk a walk found names the chunk when the id is one
//! of the specification's literals below and names its offset when it is not (`is_named`,
//! `located`), and the pad-byte refusal reports that the byte is not zero rather than
//! what it is. [`AviFrame`] and [`AviStream`] hand-write `Debug` for the same reason
//! `schema::capture::Frame` does — a derived one would print the frame.

use schema::error::Result;

use crate::fault::imaging_failure;

/// The `operation` every refusal in this module carries (design D13, rubric A5).
///
/// There is no imaging-local error type; `Error::DeviceIo`'s `operation` field is where the
/// step that refused gets named, and `crate::fault` is the one place that decision lives.
const READ_OP: &str = "read AVI";

// --- The specification's literals ---------------------------------------------------
//
// Derived here from the RIFF/AVI specification and deliberately **private**: the muxer must
// derive its own, or the two halves of P6a stop being independent (see the module doc on
// `crate::avi`).

/// The outermost chunk id of every RIFF file.
const RIFF: [u8; 4] = *b"RIFF";
/// The form type that makes a RIFF file an AVI. The trailing space is part of it.
const AVI_FORM: [u8; 4] = *b"AVI ";
/// The chunk id whose payload opens with a list type FourCC.
const LIST: [u8; 4] = *b"LIST";
/// The header list: everything a reader needs before the first frame.
const HDRL: [u8; 4] = *b"hdrl";
/// The main AVI header chunk.
const AVIH: [u8; 4] = *b"avih";
/// One stream's header list.
const STRL: [u8; 4] = *b"strl";
/// The stream header inside `strl`.
const STRH: [u8; 4] = *b"strh";
/// The stream format inside `strl` — a `BITMAPINFOHEADER` for video.
const STRF: [u8; 4] = *b"strf";
/// The list that holds the frames.
const MOVI: [u8; 4] = *b"movi";
/// The index, written at close.
const IDX1: [u8; 4] = *b"idx1";
/// `strh.fccType` for a video stream.
const VIDS: [u8; 4] = *b"vids";
/// The only compression this reader accepts (D7 L0: the muxer is MJPG-only by design).
const MJPG: [u8; 4] = *b"MJPG";
/// Stream 0's compressed-DIB chunk: the frame chunk our muxer writes.
const CHUNK_00DC: [u8; 4] = *b"00dc";
/// Stream 0's uncompressed-DIB chunk. Accepted as a frame chunk because the specification
/// says it is one; nothing this project writes uses it.
const CHUNK_00DB: [u8; 4] = *b"00db";

/// Every chunk id this reader has a name for, and therefore the only ids it will quote.
///
/// The list is the module's own vocabulary, not a guess at the format's: a refusal that
/// echoes one of these is echoing a constant declared ten lines above it, while a refusal
/// that echoes anything else is echoing four bytes it read out of the file — which, inside
/// `movi`, is four bytes of a frame (rubric A12; see [`located`] for why no walk can prove
/// otherwise).
const NAMED_CHUNKS: [[u8; 4]; 8] = [RIFF, LIST, AVIH, STRH, STRF, IDX1, CHUNK_00DC, CHUNK_00DB];

/// `MainAVIHeader` is a fixed 56 bytes.
const AVIH_BYTES: usize = 56;
/// `AVIStreamHeader` is a fixed 56 bytes, `rcFrame` included.
const STRH_BYTES: usize = 56;
/// `BITMAPINFOHEADER` is a fixed 40 bytes. A longer `strf` (a palette follows) is legal and
/// its tail is ignored; a shorter one is refused.
const STRF_BYTES: usize = 40;
/// Every `idx1` entry is four little-endian `DWORD`s.
const IDX1_ENTRY_BYTES: usize = 16;
/// A chunk header is a FourCC and a `DWORD` size.
const CHUNK_HEADER_BYTES: usize = 8;

/// `AVIF_HASINDEX` in `avih.dwFlags`: the file carries an `idx1`.
///
/// Set only at close, once the index is actually on the sink — a file that crashed
/// mid-recording must not claim an index it does not have, which is why this reader
/// refuses both directions of the disagreement.
const AVIF_HASINDEX: u32 = 0x0000_0010;

/// How far apart the two homes of the frame interval may be before it is a refusal.
///
/// One microsecond, and not zero: `dwScale`/`dwRate` is a rational and
/// `dwMicroSecPerFrame` is an integer, so a writer that rounded and a reader that
/// truncates can legitimately land one apart (29.97 fps is the classic case —
/// 1001/30000 s is 33366.67 µs, and both neighbours are defensible). And not more than
/// one: the defect this catches is a header rewritten in one place and stale in the other,
/// which misses by the difference between two frame rates — thousands of microseconds,
/// never one.
const RATE_TOLERANCE_US: u64 = 1;

// --- The refusal vocabulary ---------------------------------------------------------
//
// Named rather than inlined because the tests pin them: an assertion of `is_err()` cannot
// tell "we caught the corruption we seeded" from "we tripped over something else on the way
// there", and every refusal below has a test that seeds exactly its corruption
// (`crate::fault::SHORT_BUFFER_PREFIX` records where this habit came from).

/// The file does not open with a `RIFF` chunk.
const NOT_RIFF: &str = "the file does not begin with a RIFF chunk";
/// The RIFF form type is something other than `AVI `.
const NOT_AVI_FORM: &str = "the RIFF form type is";
/// The `RIFF` size field and the bytes present disagree.
const RIFF_SIZE_MISMATCH: &str = "the RIFF size field declares";
/// A chunk's size field puts its end past its enclosing list or past the file.
const CHUNK_OVERRUN: &str = "which runs past";
/// An odd-length chunk's pad byte is missing, non-zero, or outside its list.
const BAD_PAD: &str = "must be followed by a zero pad byte";
/// The `LIST movi` size field and the chunks inside it disagree.
const MOVI_SIZE_MISMATCH: &str = "the LIST movi size field declares";
/// A chunk inside `movi` belongs to a stream this reader does not carry.
const FOREIGN_STREAM: &str = "belongs to a stream this reader does not carry";
/// A chunk inside `movi` is stream 0's, and is not one of stream 0's frame chunks.
///
/// Separate from [`FOREIGN_STREAM`] because they are different defects and the wrong one
/// sends a maintainer hunting a second `LIST strl` that is not there: `00pc`, a palette
/// change, is stream 0's and legal in a video stream, and calling it evidence of a second
/// stream contradicts its own first two characters.
const NOT_A_FRAME_CHUNK: &str = "is not a frame chunk";
/// A structure the file cannot be read without is absent.
const MISSING: &str = "the file has no";
/// A fixed-size header chunk is shorter than the specification's structure.
const SHORT_HEADER: &str = "is shorter than the specification's";
/// `avih.dwTotalFrames` or `strh.dwLength` disagrees with the chunks found.
const FRAME_COUNT_MISMATCH: &str = "frame chunks are present but";
/// The two homes of the frame interval disagree.
const RATE_MISMATCH: &str = "microseconds per frame, but dwScale";
/// `strh.dwRate` is zero, so the stream's interval is not a number.
const ZERO_RATE: &str = "strh.dwRate is zero";
/// An `idx1` entry does not name the frame that sits at its own position in the index.
const INDEX_OUT_OF_ORDER: &str = "is not where its own frame begins";
/// An `idx1` entry's length disagrees with the chunk's own size field.
const INDEX_LENGTH_MISMATCH: &str = "declares a length of";
/// An `idx1` entry names a different chunk id than the chunk at that offset.
const INDEX_ID_MISMATCH: &str = "but the chunk there is";
/// The index holds a different number of entries than the `movi` list holds frames.
const INDEX_COUNT_MISMATCH: &str = "index entries but";
/// The `idx1` payload is not a whole number of entries.
const INDEX_RAGGED: &str = "is not a whole number of 16-byte entries";
/// `AVIF_HASINDEX` is set and there is no `idx1`.
const HASINDEX_WITHOUT_INDEX: &str = "AVIF_HASINDEX is set but the file has no idx1";
/// There is an `idx1` and `AVIF_HASINDEX` is clear.
const INDEX_WITHOUT_HASINDEX: &str = "the file has an idx1 but AVIF_HASINDEX is clear";
/// The stream is not a video stream.
const NOT_VIDEO: &str = "the stream type is";
/// `biCompression` is not `MJPG`.
const NOT_MJPG: &str = "biCompression is";
/// The headers claim a zero width or height.
const ZERO_EXTENT: &str = "the headers claim a";
/// `avih` and the `BITMAPINFOHEADER` disagree about the frame size.
const EXTENT_MISMATCH: &str = "but the BITMAPINFOHEADER says";
/// The file carries more than one stream.
const SECOND_STREAM: &str = "this reader carries one video stream";
/// `avih.dwStreams` disagrees with the number of `strl` lists found.
const STREAM_COUNT_MISMATCH: &str = "avih.dwStreams says";
/// A run of bytes too short to be a chunk header sits at the end of a list.
const TRAILING_BYTES: &str = "trailing bytes";
/// An offset or a length does not fit where it must.
const NOT_ADDRESSABLE: &str = "does not fit";

/// The header values a reader needs, from the three chunks that carry them.
///
/// Flat rather than nested because a reader's questions cross the structures: "does the
/// rate agree with itself" reads `avih` and `strh`, "does the geometry agree with itself"
/// reads `avih` and `strf`. [`read_stream`] has already checked both by the time a caller
/// sees this, so the fields are here to be *reported* — by an oracle test, or by whatever
/// P6d ends up comparing against ffprobe.
///
/// `width`/`height` are `avih`'s `dwWidth`/`dwHeight`; the `BITMAPINFOHEADER`'s copy is
/// validated against them and then dropped, because two names for one number is how the two
/// stop matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AviHeaders {
    /// `avih.dwMicroSecPerFrame` — the declared constant frame interval (D7's CFR
    /// carve-out).
    pub micro_sec_per_frame: u32,
    /// `avih.dwMaxBytesPerSec`.
    pub max_bytes_per_sec: u32,
    /// `avih.dwFlags`. [`AviHeaders::has_index`] is the one bit this module interprets.
    pub flags: u32,
    /// `avih.dwTotalFrames`, checked against the frames actually present.
    pub total_frames: u32,
    /// `avih.dwWidth`.
    pub width: u32,
    /// `avih.dwHeight`.
    pub height: u32,
    /// `strh.dwRate` — the denominator of the stream's frame interval.
    pub stream_rate: u32,
    /// `strh.dwScale` — its numerator.
    pub stream_scale: u32,
    /// `strh.dwLength`, checked against the frames actually present.
    pub stream_length: u32,
    /// `strh.dwSuggestedBufferSize` — the largest frame a player should expect. Reported,
    /// not enforced: a player that trusts it and a muxer that understates it is a
    /// player's bug to have.
    pub suggested_buffer_size: u32,
    /// `strh.fccHandler`. Advisory by specification, so it is surfaced and not enforced;
    /// `compression` is the field that decides what the payload is.
    pub handler: [u8; 4],
    /// `strf.biCompression`, which must be `MJPG`.
    pub compression: [u8; 4],
    /// `strf.biBitCount`.
    pub bit_count: u16,
}

impl AviHeaders {
    /// Whether `avih.dwFlags` claims an `idx1`.
    ///
    /// [`read_stream`] has already refused any file where this disagrees with the index's
    /// actual presence, so on a parsed stream it is a fact rather than a claim.
    #[must_use]
    pub fn has_index(&self) -> bool {
        self.flags & AVIF_HASINDEX != 0
    }
}

/// One frame chunk, borrowed out of the file.
///
/// The bytes are the payload exactly: no pad byte, no chunk header, nothing decoded. E6's
/// byte-fidelity claim is testable precisely because this is a borrow — a copy would let a
/// transformation hide in the copying.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AviFrame<'a> {
    /// The frame's payload — for our files, one whole JPEG.
    pub bytes: &'a [u8],
    /// Where the chunk *header* starts, relative to the `movi` FourCC. This is the number
    /// an `idx1` entry carries, so the first frame in a file is at 4.
    pub movi_offset: u32,
    /// The chunk id, normally `00dc`.
    pub chunk_id: [u8; 4],
}

impl std::fmt::Debug for AviFrame<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // A frame may contain a person. Never the bytes (rubric A12) — the same reason
        // `schema::capture::Frame` hand-writes this.
        f.debug_struct("AviFrame")
            .field("bytes", &format_args!("<{} bytes>", self.bytes.len()))
            .field("movi_offset", &self.movi_offset)
            .field("chunk_id", &format_args!("{}", fourcc_name(self.chunk_id)))
            .finish()
    }
}

/// One `idx1` entry, as it appears on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AviIndexEntry {
    /// `dwChunkId`.
    pub chunk_id: [u8; 4],
    /// `dwFlags` — `AVIIF_KEYFRAME` (0x10) on every frame of an MJPEG file, since every
    /// MJPEG frame is a keyframe. Surfaced rather than enforced: a container that can
    /// carry P-frames is not wrong to say so, and this reader validates structure.
    pub flags: u32,
    /// `dwChunkOffset`, relative to the `movi` FourCC.
    pub offset: u32,
    /// `dwChunkLength` — the payload length, pad byte excluded.
    pub size: u32,
}

/// A whole parsed AVI: its headers, its frames in file order, and its index.
///
/// `index` is empty when the file has none, which [`AviHeaders::has_index`] agrees with by
/// the time this value exists.
#[derive(Clone, PartialEq, Eq)]
pub struct AviStream<'a> {
    /// Everything the header chunks declared.
    pub headers: AviHeaders,
    /// The frames, in the order they appear in `movi` — which is also increasing
    /// `movi_offset` order, and which is the order [`read_stream`] holds `idx1` to: entry
    /// *i* of the index is frame *i* of this vector, or the file was refused.
    pub frames: Vec<AviFrame<'a>>,
    /// The `idx1` entries, in file order.
    pub index: Vec<AviIndexEntry>,
}

impl std::fmt::Debug for AviStream<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Counts, never payloads (rubric A12): a derived `Debug` here would print every
        // frame in the recording.
        f.debug_struct("AviStream")
            .field("headers", &self.headers)
            .field("frames", &format_args!("<{} frames>", self.frames.len()))
            .field("index", &format_args!("<{} entries>", self.index.len()))
            .finish()
    }
}

/// Parse a whole AVI file, refusing anything that disagrees with itself.
///
/// The order is structure first, then semantics: the RIFF envelope, then the chunk walk
/// (which is where overruns, pad bytes and the `movi` accounting are caught), then the
/// cross-checks between header fields, the frames and the index. A file that survives all
/// of it is one whose every size field is backed by bytes.
///
/// # Errors
///
/// [`schema::Error::DeviceIo`] naming the offset and the two numbers that
/// disagree, for: a missing or foreign RIFF envelope; a `RIFF` size field that does not
/// match the file length; a chunk running past its enclosing list or past end-of-file; a
/// missing or non-zero pad byte after an odd-length chunk; a `LIST movi` size field that
/// its own chunks do not account for; a missing `hdrl`, `avih`, `strh`, `strf` or `movi`; a
/// header chunk shorter than its fixed structure; a second stream; `avih.dwTotalFrames` or
/// `strh.dwLength` disagreeing with the frames found; the two homes of the frame interval
/// disagreeing by more than one microsecond; an `idx1` entry whose offset, id or length is
/// not the frame at that entry's own position; an index whose entry count is not the frame
/// count; `AVIF_HASINDEX` disagreeing with the index's presence; a non-`vids` stream; a
/// `biCompression` other than `MJPG`; or a declared width or height of zero.
pub fn read_stream(bytes: &[u8]) -> Result<AviStream<'_>> {
    let total = bytes.len();
    let envelope = CHUNK_HEADER_BYTES.saturating_add(4);
    if total < envelope {
        return Err(refuse(format!(
            "{NOT_RIFF}: it holds {total} bytes, fewer than the {envelope} a RIFF envelope needs"
        )));
    }
    if fourcc_at(bytes, 0)? != RIFF {
        return Err(refuse(format!(
            "{NOT_RIFF}: offset 0 holds `{}`",
            fourcc_name(fourcc_at(bytes, 0)?)
        )));
    }
    let form = fourcc_at(bytes, 8)?;
    if form != AVI_FORM {
        return Err(refuse(format!(
            "{NOT_AVI_FORM} `{}` at offset 8, not `AVI `",
            fourcc_name(form)
        )));
    }
    let riff_size = addressable(u32_at(bytes, 4)?, "the RIFF size field")?;
    // What the size field covers is everything after it, so the arithmetic a correct file
    // satisfies is `size == len - 8`. Stated as a subtraction rather than an addition so a
    // placeholder of `u32::MAX` reports the numbers rather than overflowing.
    let bytes_present = total.saturating_sub(CHUNK_HEADER_BYTES);
    if riff_size != bytes_present {
        return Err(refuse(format!(
            "{RIFF_SIZE_MISMATCH} {riff_size} bytes after offset 8 but {bytes_present} are present"
        )));
    }

    let mut hdrl: Option<HeaderList> = None;
    let mut movi: Option<(usize, Vec<AviFrame<'_>>)> = None;
    let mut index: Option<Vec<AviIndexEntry>> = None;

    let mut cursor = envelope;
    while cursor < total {
        let chunk = chunk_at(bytes, cursor, total, "the end of the file")?;
        if chunk.id == LIST {
            let list_type = list_type_of(&chunk)?;
            if list_type == HDRL {
                hdrl = Some(parse_header_list(bytes, &chunk)?);
            } else if list_type == MOVI {
                // Deliberately not `chunk.next`: the `movi` walk is driven by the chunks
                // inside it, and only afterwards compared with this size field.
                let (frames, end) = walk_movi(bytes, chunk.payload_at, chunk.size, total)?;
                movi = Some((chunk.payload_at, frames));
                cursor = end;
                continue;
            }
        } else if chunk.id == IDX1 {
            index = Some(parse_index(chunk.payload, chunk.start)?);
        }
        cursor = chunk.next;
    }

    let headers = hdrl.ok_or_else(|| refuse(format!("{MISSING} LIST hdrl")))?;
    let (movi_at, frames) = movi.ok_or_else(|| refuse(format!("{MISSING} LIST movi")))?;
    let index = index.unwrap_or_default();
    let has_idx1 = headers.main.flags & AVIF_HASINDEX != 0;

    check_stream_shape(&headers)?;
    check_frame_counts(&headers, frames.len(), movi_at)?;
    check_rate_agreement(&headers)?;
    if has_idx1 && index.is_empty() {
        return Err(refuse(format!(
            "{HASINDEX_WITHOUT_INDEX} (avih at offset {})",
            headers.main.at
        )));
    }
    if !has_idx1 && !index.is_empty() {
        return Err(refuse(format!(
            "{INDEX_WITHOUT_HASINDEX} ({} entries, avih at offset {})",
            index.len(),
            headers.main.at
        )));
    }
    if !index.is_empty() {
        check_index(&index, &frames, movi_at)?;
    }

    Ok(AviStream {
        headers: headers.published(),
        frames,
        index,
    })
}

/// Walk `movi` to end-of-file and return every **complete** frame chunk.
///
/// The crash-recovery half of D7 L0, and the reason `hdrl` is written whole before the
/// first frame: the only thing this needs to trust is the header list, whose size field was
/// final the moment it was written. Everything after it — the `RIFF` size, the `LIST movi`
/// size, the index — is either a placeholder a crash never patched or absent entirely, so
/// none of it is read.
///
/// Returns an empty vector rather than an error for anything it cannot make sense of: a
/// recovery path that refuses is a recovery path that did not happen.
#[must_use]
pub fn recover_frames(bytes: &[u8]) -> Vec<AviFrame<'_>> {
    let mut frames = Vec::new();
    let Some(movi_at) = find_movi(bytes) else {
        return frames;
    };
    let Some(mut cursor) = movi_at.checked_add(4) else {
        return frames;
    };
    loop {
        let Some(id) = try_fourcc(bytes, cursor) else {
            return frames;
        };
        if !is_stream_chunk(id) {
            return frames;
        }
        let Some(size) = try_u32(bytes, cursor.saturating_add(4)) else {
            return frames;
        };
        let Some(payload_at) = cursor.checked_add(CHUNK_HEADER_BYTES) else {
            return frames;
        };
        let Ok(size) = usize::try_from(size) else {
            return frames;
        };
        let Some(payload_end) = payload_at.checked_add(size) else {
            return frames;
        };
        // A frame is complete when its whole payload is on the sink. The pad byte may be
        // missing on the last one — the crash landed between the two writes — and a frame
        // whose bytes are all there is a frame.
        let Some(payload) = bytes.get(payload_at..payload_end) else {
            return frames;
        };
        let Some(offset) = cursor
            .checked_sub(movi_at)
            .and_then(|d| u32::try_from(d).ok())
        else {
            return frames;
        };
        if id == CHUNK_00DC || id == CHUNK_00DB {
            frames.push(AviFrame {
                bytes: payload,
                movi_offset: offset,
                chunk_id: id,
            });
        }
        let Some(next) = payload_end.checked_add(size % 2) else {
            return frames;
        };
        cursor = next;
    }
}

// --- The walk -----------------------------------------------------------------------

/// One chunk, located and bounds-checked.
struct Chunk<'a> {
    /// Its FourCC.
    id: [u8; 4],
    /// Where the FourCC sits.
    start: usize,
    /// Where the payload starts.
    payload_at: usize,
    /// The declared payload size — pad byte excluded, per the specification.
    size: usize,
    /// The payload itself.
    payload: &'a [u8],
    /// Where the next chunk starts: past the payload, and past the pad byte if there is
    /// one.
    next: usize,
}

/// Read the chunk at `at`, refusing anything that does not fit inside `limit`.
///
/// `limit` is the end of the enclosing list, or the end of the file at the top level —
/// which is what makes "runs past its enclosing list" and "runs past end-of-file" the same
/// check with two different messages, rather than two checks that can drift apart.
fn chunk_at<'a>(bytes: &'a [u8], at: usize, limit: usize, edge: &str) -> Result<Chunk<'a>> {
    let room = limit.saturating_sub(at);
    if room < CHUNK_HEADER_BYTES {
        return Err(refuse(format!(
            "{room} {TRAILING_BYTES} at offset {at}, too few for a chunk header before {edge} \
             at offset {limit}"
        )));
    }
    let id = fourcc_at(bytes, at)?;
    let here = located(id, at);
    let size = addressable(
        u32_at(bytes, at.saturating_add(4))?,
        &format!("the size field of {here}"),
    )?;
    let payload_at = at.saturating_add(CHUNK_HEADER_BYTES);
    let pad = size % 2;
    let payload_end = payload_at
        .checked_add(size)
        .ok_or_else(|| overflowed(&here, size))?;
    let next = payload_end
        .checked_add(pad)
        .ok_or_else(|| overflowed(&here, size))?;
    if next > limit {
        let padding = if pad == 1 { " plus a pad byte" } else { "" };
        return Err(refuse(format!(
            "{here} declares {size} bytes{padding}, {CHUNK_OVERRUN} {edge} at offset {limit}"
        )));
    }
    if pad == 1 {
        let byte = bytes
            .get(payload_end)
            .copied()
            .ok_or_else(|| overflowed(&here, size))?;
        if byte != 0 {
            // The offset, and never the value. When the size field is the thing that is
            // wrong — which is the case this check exists for — `payload_end` lands inside
            // the payload, so the byte here is a frame byte and a refusal travels to logs
            // and to the wire (rubric A12). The offset carries every diagnosis the value
            // would: what a reader needs to know is that the byte at this address is not a
            // pad, not what it is instead.
            return Err(refuse(format!(
                "{here} declares an odd {size} bytes and {BAD_PAD}, but the byte at offset \
                 {payload_end} is not zero"
            )));
        }
    }
    let payload = bytes
        .get(payload_at..payload_end)
        .ok_or_else(|| overflowed(&here, size))?;
    Ok(Chunk {
        id,
        start: at,
        payload_at,
        size,
        payload,
        next,
    })
}

/// The list type a `LIST` chunk opens with.
fn list_type_of(chunk: &Chunk<'_>) -> Result<[u8; 4]> {
    let head = chunk.payload.get(..4).ok_or_else(|| {
        refuse(format!(
            "the LIST chunk at offset {} declares {} bytes, too few for a list type",
            chunk.start, chunk.size
        ))
    })?;
    <[u8; 4]>::try_from(head).map_err(|_| {
        refuse(format!(
            "the LIST chunk at offset {} has no readable list type",
            chunk.start
        ))
    })
}

/// Walk the chunks inside `movi`, then hold its size field to what they add up to.
///
/// The walk is bounded by the **file**, not by `declared`. It stops when it meets a chunk
/// id that is not a stream chunk — which is where `idx1` lives on a finished file, and
/// where the file simply ends on a crashed one. Only then is `declared` compared with the
/// distance covered, which is what keeps the comparison from being a restatement of the
/// walk's own bound.
fn walk_movi<'a>(
    bytes: &'a [u8],
    movi_at: usize,
    declared: usize,
    total: usize,
) -> Result<(Vec<AviFrame<'a>>, usize)> {
    let mut frames = Vec::new();
    let mut cursor = movi_at.saturating_add(4);
    while total.saturating_sub(cursor) >= CHUNK_HEADER_BYTES {
        let Some(id) = try_fourcc(bytes, cursor) else {
            break;
        };
        if !is_stream_chunk(id) {
            break;
        }
        let chunk = chunk_at(bytes, cursor, total, "the end of the file")?;
        if id != CHUNK_00DC && id != CHUNK_00DB {
            // Two defects wear this shape and they are not the same finding. A stream
            // number other than 0 is a second stream, which design §1 makes a non-goal; a
            // chunk of stream 0 whose type this reader does not carry — `00pc`, a palette
            // change, legal in a video stream — is not evidence of a second stream at all,
            // and saying so would send a maintainer looking for a `LIST strl` that is not
            // there. Neither branch quotes the id: see `located`.
            let [hi, lo, _, _] = id;
            let stream_zero = hi == b'0' && lo == b'0';
            return Err(refuse(if stream_zero {
                format!(
                    "the chunk at offset {cursor} {NOT_A_FRAME_CHUNK}: this reader carries \
                     `00dc` and `00db` (design §1: no audio)"
                )
            } else {
                format!(
                    "the chunk at offset {cursor} {FOREIGN_STREAM} — {SECOND_STREAM} \
                     (design §1: no audio)"
                )
            }));
        }
        let offset = cursor.saturating_sub(movi_at);
        let movi_offset = u32::try_from(offset).map_err(|_| {
            refuse(format!(
                "the frame chunk at offset {cursor} is {offset} bytes into movi, which \
                 {NOT_ADDRESSABLE} in an idx1 entry's 32-bit offset"
            ))
        })?;
        frames.push(AviFrame {
            bytes: chunk.payload,
            movi_offset,
            chunk_id: id,
        });
        cursor = chunk.next;
    }
    let accounted = cursor.saturating_sub(movi_at);
    if accounted != declared {
        // Where the walk stopped, because the size field is not always the thing that is
        // wrong. A `JUNK` chunk or a `LIST rec ` grouping inside `movi` is spec-legal and
        // this reader deliberately does not carry either (see `is_stream_chunk`); the walk
        // ends there and the accounting then falls short of a size field that is perfectly
        // correct. Reporting only the two numbers points at the innocent field and sends a
        // reader hunting a muxer accounting bug that does not exist.
        let stopped = match try_fourcc(bytes, cursor) {
            Some(id) if is_named(id) => {
                format!(", where the walk stopped at chunk `{}`", fourcc_name(id))
            }
            Some(_) => ", where the walk stopped at a chunk id this reader does not carry".into(),
            None => ", which is where the file ends".into(),
        };
        return Err(refuse(format!(
            "{MOVI_SIZE_MISMATCH} {declared} bytes at offset {movi_at} but its chunks account \
             for {accounted} up to offset {cursor}{stopped}"
        )));
    }
    Ok((frames, cursor))
}

/// Whether a chunk id inside `movi` names stream data at all.
///
/// The specification spells a stream chunk `##xx`, two decimal digits of stream number
/// followed by a two-letter type. Anything else — `idx1`, or the `JUNK` padding and
/// `LIST rec ` groupings RIFF permits and this reader deliberately does not carry — ends
/// the walk, and the `movi` size check is what then decides whether ending there was right.
///
/// Not carrying them is the adversary's posture rather than an oversight: this module
/// exists to disagree with our muxer, our muxer writes neither, and a walk that skipped
/// unrecognised chunks would stop noticing one that appeared. What it must do instead is
/// *diagnose* the refusal correctly, which is why [`walk_movi`]'s size-mismatch message
/// names the offset the walk stopped at — the `LIST movi` size field in that case is
/// innocent.
fn is_stream_chunk(id: [u8; 4]) -> bool {
    let [hi, lo, _, _] = id;
    hi.is_ascii_digit() && lo.is_ascii_digit()
}

// --- The header list ----------------------------------------------------------------

/// `avih`, located.
#[derive(Clone, Copy)]
struct MainHeader {
    at: usize,
    micro_sec_per_frame: u32,
    max_bytes_per_sec: u32,
    flags: u32,
    total_frames: u32,
    streams: u32,
    width: u32,
    height: u32,
}

/// `strh`, located.
#[derive(Clone, Copy)]
struct StreamHeader {
    at: usize,
    stream_type: [u8; 4],
    handler: [u8; 4],
    scale: u32,
    rate: u32,
    length: u32,
    suggested_buffer_size: u32,
}

/// `strf`, located — a `BITMAPINFOHEADER`.
#[derive(Clone, Copy)]
struct StreamFormat {
    at: usize,
    width: i32,
    height: i32,
    bit_count: u16,
    compression: [u8; 4],
}

/// The three header chunks, once all three have been found.
struct HeaderList {
    main: MainHeader,
    stream: StreamHeader,
    format: StreamFormat,
}

impl HeaderList {
    /// The public projection: the fields, without the offsets the checks needed.
    fn published(&self) -> AviHeaders {
        AviHeaders {
            micro_sec_per_frame: self.main.micro_sec_per_frame,
            max_bytes_per_sec: self.main.max_bytes_per_sec,
            flags: self.main.flags,
            total_frames: self.main.total_frames,
            width: self.main.width,
            height: self.main.height,
            stream_rate: self.stream.rate,
            stream_scale: self.stream.scale,
            stream_length: self.stream.length,
            suggested_buffer_size: self.stream.suggested_buffer_size,
            handler: self.stream.handler,
            compression: self.format.compression,
            bit_count: self.format.bit_count,
        }
    }
}

/// Parse `LIST hdrl`: one `avih`, then one `LIST strl` of `strh` + `strf`.
fn parse_header_list(bytes: &[u8], list: &Chunk<'_>) -> Result<HeaderList> {
    let end = list.payload_at.saturating_add(list.size);
    let mut cursor = list.payload_at.saturating_add(4);
    let mut main = None;
    let mut stream = None;
    let mut format = None;
    let mut stream_lists = 0u32;
    while cursor < end {
        let chunk = chunk_at(bytes, cursor, end, "the LIST hdrl")?;
        if chunk.id == AVIH {
            main = Some(parse_avih(&chunk)?);
        } else if chunk.id == LIST && list_type_of(&chunk)? == STRL {
            stream_lists = stream_lists.saturating_add(1);
            if stream_lists > 1 {
                return Err(refuse(format!(
                    "a second LIST strl at offset {} — {SECOND_STREAM} (design §1: no audio)",
                    chunk.start
                )));
            }
            let (header, fmt) = parse_stream_list(bytes, &chunk)?;
            stream = Some(header);
            format = Some(fmt);
        }
        cursor = chunk.next;
    }
    let main = main.ok_or_else(|| refuse(format!("{MISSING} avih chunk inside LIST hdrl")))?;
    let stream = stream.ok_or_else(|| refuse(format!("{MISSING} strh chunk inside LIST strl")))?;
    let format = format.ok_or_else(|| refuse(format!("{MISSING} strf chunk inside LIST strl")))?;
    if main.streams != stream_lists {
        return Err(refuse(format!(
            "{STREAM_COUNT_MISMATCH} {} but {stream_lists} LIST strl are present (avih at \
             offset {})",
            main.streams, main.at
        )));
    }
    Ok(HeaderList {
        main,
        stream,
        format,
    })
}

/// Parse `LIST strl`: `strh` then `strf`.
fn parse_stream_list(bytes: &[u8], list: &Chunk<'_>) -> Result<(StreamHeader, StreamFormat)> {
    let end = list.payload_at.saturating_add(list.size);
    let mut cursor = list.payload_at.saturating_add(4);
    let mut header = None;
    let mut format = None;
    while cursor < end {
        let chunk = chunk_at(bytes, cursor, end, "the LIST strl")?;
        if chunk.id == STRH {
            header = Some(parse_strh(&chunk)?);
        } else if chunk.id == STRF {
            format = Some(parse_strf(&chunk)?);
        }
        cursor = chunk.next;
    }
    let header = header.ok_or_else(|| refuse(format!("{MISSING} strh chunk inside LIST strl")))?;
    let format = format.ok_or_else(|| refuse(format!("{MISSING} strf chunk inside LIST strl")))?;
    Ok((header, format))
}

fn parse_avih(chunk: &Chunk<'_>) -> Result<MainHeader> {
    let head = fixed_header(chunk, AVIH_BYTES, "MainAVIHeader")?;
    Ok(MainHeader {
        at: chunk.payload_at,
        micro_sec_per_frame: u32_at(head, 0)?,
        max_bytes_per_sec: u32_at(head, 4)?,
        // 8 is dwPaddingGranularity, which nothing here reads: this module verifies
        // padding by looking at the bytes rather than by believing a field about them.
        flags: u32_at(head, 12)?,
        total_frames: u32_at(head, 16)?,
        // 20 is dwInitialFrames (audio pre-roll; no audio, design §1).
        streams: u32_at(head, 24)?,
        // 28 is dwSuggestedBufferSize, whose per-stream copy in `strh` is the one reported.
        width: u32_at(head, 32)?,
        height: u32_at(head, 36)?,
    })
}

fn parse_strh(chunk: &Chunk<'_>) -> Result<StreamHeader> {
    let head = fixed_header(chunk, STRH_BYTES, "AVIStreamHeader")?;
    Ok(StreamHeader {
        at: chunk.payload_at,
        stream_type: fourcc_at(head, 0)?,
        handler: fourcc_at(head, 4)?,
        // 8 dwFlags, 12 wPriority + wLanguage, 16 dwInitialFrames.
        scale: u32_at(head, 20)?,
        rate: u32_at(head, 24)?,
        // 28 dwStart.
        length: u32_at(head, 32)?,
        suggested_buffer_size: u32_at(head, 36)?,
        // 40 dwQuality, 44 dwSampleSize, 48..56 rcFrame.
    })
}

fn parse_strf(chunk: &Chunk<'_>) -> Result<StreamFormat> {
    let head = fixed_header(chunk, STRF_BYTES, "BITMAPINFOHEADER")?;
    Ok(StreamFormat {
        at: chunk.payload_at,
        // 0 biSize, which this module does not believe: the chunk's own size is what
        // bounds the read, and a biSize disagreeing with it decides nothing here.
        width: i32_at(head, 4)?,
        height: i32_at(head, 8)?,
        // 12 biPlanes.
        bit_count: u16_at(head, 14)?,
        compression: fourcc_at(head, 16)?,
    })
}

/// The prefix of a fixed-layout header chunk, or a refusal naming both sizes.
fn fixed_header<'a>(chunk: &Chunk<'a>, needed: usize, structure: &str) -> Result<&'a [u8]> {
    chunk.payload.get(..needed).ok_or_else(|| {
        refuse(format!(
            "{} declares {} bytes, which {SHORT_HEADER} {needed}-byte {structure}",
            located(chunk.id, chunk.start),
            chunk.size
        ))
    })
}

// --- The cross-checks ---------------------------------------------------------------

/// What the stream must be for this container to mean anything: video, MJPG, non-degenerate
/// and describing itself the same way twice.
fn check_stream_shape(headers: &HeaderList) -> Result<()> {
    let stream = &headers.stream;
    if stream.stream_type != VIDS {
        return Err(refuse(format!(
            "{NOT_VIDEO} `{}` at offset {}, not `vids`",
            fourcc_name(stream.stream_type),
            stream.at
        )));
    }
    let format = &headers.format;
    if format.compression != MJPG {
        return Err(refuse(format!(
            "{NOT_MJPG} `{}` at offset {}, not `MJPG` (D7 L0: the muxer is MJPG-only)",
            fourcc_name(format.compression),
            format.at
        )));
    }
    let main = &headers.main;
    if main.width == 0 || main.height == 0 {
        return Err(refuse(format!(
            "{ZERO_EXTENT} {}x{} frame at offset {}",
            main.width, main.height, main.at
        )));
    }
    // biHeight is signed: negative means a top-down DIB, which is a row-order statement and
    // not a different size. biWidth is not, and a negative one is a corrupt field.
    let format_width = u32::try_from(format.width).map_err(|_| {
        refuse(format!(
            "the BITMAPINFOHEADER at offset {} declares a negative biWidth of {}",
            format.at, format.width
        ))
    })?;
    let format_height = format.height.unsigned_abs();
    if format_width != main.width || format_height != main.height {
        return Err(refuse(format!(
            "avih at offset {} says {}x{} {EXTENT_MISMATCH} {format_width}x{format_height} at \
             offset {}",
            main.at, main.width, main.height, format.at
        )));
    }
    Ok(())
}

/// `avih.dwTotalFrames` and `strh.dwLength` against the chunks actually in `movi`.
///
/// Both, separately: they are two places one number lives, and a muxer that patches its
/// counters at close can miss either one.
fn check_frame_counts(headers: &HeaderList, found: usize, movi_at: usize) -> Result<()> {
    let found = u32::try_from(found).map_err(|_| {
        refuse(format!(
            "the movi list at offset {movi_at} holds {found} frames, which {NOT_ADDRESSABLE} in \
             avih.dwTotalFrames"
        ))
    })?;
    let main = &headers.main;
    if main.total_frames != found {
        return Err(refuse(format!(
            "{found} {FRAME_COUNT_MISMATCH} avih.dwTotalFrames at offset {} says {}",
            main.at, main.total_frames
        )));
    }
    let stream = &headers.stream;
    if stream.length != found {
        return Err(refuse(format!(
            "{found} {FRAME_COUNT_MISMATCH} strh.dwLength at offset {} says {}",
            stream.at, stream.length
        )));
    }
    Ok(())
}

/// The two homes of the frame interval, held to each other (D7's CFR carve-out).
fn check_rate_agreement(headers: &HeaderList) -> Result<()> {
    let main = &headers.main;
    let stream = &headers.stream;
    if stream.rate == 0 {
        return Err(refuse(format!(
            "{ZERO_RATE} at offset {}, so the stream declares no frame interval",
            stream.at
        )));
    }
    let derived = u64::from(stream.scale)
        .saturating_mul(1_000_000)
        .saturating_div(u64::from(stream.rate));
    let declared = u64::from(main.micro_sec_per_frame);
    if declared.abs_diff(derived) > RATE_TOLERANCE_US {
        return Err(refuse(format!(
            "avih at offset {} says {declared} {RATE_MISMATCH} {} over dwRate {} at offset {} \
             works out to {derived}",
            main.at, stream.scale, stream.rate, stream.at
        )));
    }
    Ok(())
}

/// Parse `idx1` into entries, refusing a payload that is not a whole number of them.
fn parse_index(payload: &[u8], at: usize) -> Result<Vec<AviIndexEntry>> {
    if !payload.len().is_multiple_of(IDX1_ENTRY_BYTES) {
        return Err(refuse(format!(
            "the idx1 chunk at offset {at} declares {} bytes, which {INDEX_RAGGED}",
            payload.len()
        )));
    }
    let mut entries = Vec::with_capacity(payload.len() / IDX1_ENTRY_BYTES);
    for entry in payload.chunks_exact(IDX1_ENTRY_BYTES) {
        entries.push(AviIndexEntry {
            chunk_id: fourcc_at(entry, 0)?,
            flags: u32_at(entry, 4)?,
            offset: u32_at(entry, 8)?,
            size: u32_at(entry, 12)?,
        });
    }
    Ok(entries)
}

/// Hold every index entry to the frame **at its own position**.
///
/// `idx1` is a seek table and it is positional: entry *i* is frame *i*, and a player that
/// seeks by it walks the entries in order. So the comparison is positional too. Resolving
/// each entry against *some* frame — a binary search over the frame offsets, which is what
/// this did first — accepts an index whose entries are reordered, and one that names the
/// same frame twice, for as long as the counts match and the two frames' lengths happen to
/// differ. Both are muxer bugs of exactly the class this reader exists to catch (an index
/// emitted out of order; an index whose offset failed to advance), and both send a player
/// to the wrong frame or never to the last one. Entry *i* against `frames[i]` refuses them
/// and is cheaper than the search it replaces.
///
/// This is also where a pad byte counted inside a chunk's own size field is caught: `idx1`'s
/// `dwChunkLength` excludes the pad by specification, so a chunk header that includes it
/// disagrees with its index entry by exactly one.
fn check_index(index: &[AviIndexEntry], frames: &[AviFrame<'_>], movi_at: usize) -> Result<()> {
    if index.len() != frames.len() {
        return Err(refuse(format!(
            "the idx1 for the movi list at offset {movi_at} holds {} {INDEX_COUNT_MISMATCH} the \
             list holds {} frames",
            index.len(),
            frames.len()
        )));
    }
    for (position, entry) in index.iter().enumerate() {
        // Unreachable given the count check above; written as a refusal rather than an
        // index because this crate denies `indexing_slicing` outside tests, and a reader
        // that panicked on a file would be the one failure mode a parser may not have.
        let frame = frames.get(position).ok_or_else(|| {
            refuse(format!(
                "idx1 entry {position} names no frame of the movi list at offset {movi_at}, \
                 which holds {} frames",
                frames.len()
            ))
        })?;
        if entry.offset != frame.movi_offset {
            return Err(refuse(format!(
                "idx1 entry {position} names movi offset {}, which {INDEX_OUT_OF_ORDER}: frame \
                 {position} of the movi list at offset {movi_at} begins at movi offset {}",
                entry.offset, frame.movi_offset
            )));
        }
        if frame.chunk_id != entry.chunk_id {
            return Err(refuse(format!(
                "idx1 entry {position} names {} at movi offset {}, {INDEX_ID_MISMATCH} `{}`",
                named(entry.chunk_id),
                entry.offset,
                fourcc_name(frame.chunk_id)
            )));
        }
        let size = usize::try_from(entry.size).map_err(|_| {
            refuse(format!(
                "idx1 entry {position} declares a length of {} bytes, which {NOT_ADDRESSABLE} on \
                 this platform",
                entry.size
            ))
        })?;
        if size != frame.bytes.len() {
            return Err(refuse(format!(
                "idx1 entry {position} at movi offset {} {INDEX_LENGTH_MISMATCH} {size} but the \
                 chunk's size field says {}",
                entry.offset,
                frame.bytes.len()
            )));
        }
    }
    Ok(())
}

// --- Recovery ------------------------------------------------------------------------

/// Where the `movi` FourCC sits, found by walking `hdrl` and whatever precedes it.
///
/// Bounded by the file rather than by the `RIFF` size field, because on a crashed file that
/// field is still the muxer's placeholder. `hdrl`'s own size *is* trusted — it was final
/// before the first frame was written, which is the whole design of D7's recovery promise.
fn find_movi(bytes: &[u8]) -> Option<usize> {
    if try_fourcc(bytes, 0)? != RIFF || try_fourcc(bytes, 8)? != AVI_FORM {
        return None;
    }
    let mut cursor = CHUNK_HEADER_BYTES.checked_add(4)?;
    loop {
        let id = try_fourcc(bytes, cursor)?;
        let size = usize::try_from(try_u32(bytes, cursor.checked_add(4)?)?).ok()?;
        let payload_at = cursor.checked_add(CHUNK_HEADER_BYTES)?;
        if id == LIST && try_fourcc(bytes, payload_at)? == MOVI {
            return Some(payload_at);
        }
        // Every step advances by at least the header, so this terminates on any input.
        cursor = payload_at.checked_add(size)?.checked_add(size % 2)?;
        if cursor >= bytes.len() {
            return None;
        }
    }
}

// --- Byte-level helpers ---------------------------------------------------------------

/// Every refusal in this module goes through here (design D13, rubric A5).
fn refuse(message: String) -> schema::error::Error {
    imaging_failure(READ_OP, message)
}

fn overflowed(here: &str, size: usize) -> schema::error::Error {
    refuse(format!(
        "{here} declares {size} bytes, which {NOT_ADDRESSABLE} on this platform"
    ))
}

/// Whether this reader has a name for a chunk id, and may therefore quote it.
fn is_named(id: [u8; 4]) -> bool {
    NAMED_CHUNKS.contains(&id)
}

/// A chunk id, quoted when this reader knows the name and described when it does not.
fn named(id: [u8; 4]) -> String {
    if is_named(id) {
        format!("`{}`", fourcc_name(id))
    } else {
        "an id this reader does not carry".to_owned()
    }
}

/// How a refusal names a chunk that a **walk** found — by name when it can, by offset
/// always.
///
/// [`fourcc_name`]'s premise is that a chunk id is header data. It holds only while the
/// walk that found the chunk is synchronised, and a size field is exactly what
/// desynchronises one: a `00dc` whose size field says 206 where the payload holds 409
/// leaves the walk 206 bytes into a JPEG, where `is_stream_chunk` will happily accept the
/// two ASCII digits it finds there and the refusal that follows would print four bytes of
/// somebody's face (rubric A12). Nothing in the container can prove a boundary trustworthy
/// either — a well-formed chunk with a lying size field is what breaks it — so the premise
/// cannot be repaired by checking harder.
///
/// What is left is the rule A12 already states: a message carries counts, offsets and
/// **format names**. An id in [`NAMED_CHUNKS`] is a format name, and echoing it echoes a
/// constant declared at the top of this file; an id outside it is bytes, and gets its
/// offset instead. The diagnosis survives, because "the chunk at offset 438" is what a
/// reader actually needs in order to go and look.
fn located(id: [u8; 4], at: usize) -> String {
    if is_named(id) {
        format!("chunk `{}` at offset {at}", fourcc_name(id))
    } else {
        format!("the chunk at offset {at}")
    }
}

/// A declared size as an index, or a refusal naming it.
///
/// Infallible on any 64-bit host; the conversion exists because rubric B10 says
/// device-derived numbers reach `usize` through `try_from` and never through `as`, and a
/// file is device-derived the moment a driver's frames went into it.
fn addressable(value: u32, what: &str) -> Result<usize> {
    usize::try_from(value).map_err(|_| {
        refuse(format!(
            "{what} is {value}, which {NOT_ADDRESSABLE} on this platform"
        ))
    })
}

fn region(bytes: &[u8], at: usize, len: usize) -> Result<&[u8]> {
    let end = at.checked_add(len).ok_or_else(|| {
        refuse(format!(
            "a {len}-byte field at offset {at} {NOT_ADDRESSABLE}"
        ))
    })?;
    bytes.get(at..end).ok_or_else(|| {
        refuse(format!(
            "a {len}-byte field at offset {at} runs past the end of a {}-byte region",
            bytes.len()
        ))
    })
}

fn fourcc_at(bytes: &[u8], at: usize) -> Result<[u8; 4]> {
    let quad = region(bytes, at, 4)?;
    <[u8; 4]>::try_from(quad)
        .map_err(|_| refuse(format!("the FourCC at offset {at} is not four bytes")))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(fourcc_at(bytes, at)?))
}

fn i32_at(bytes: &[u8], at: usize) -> Result<i32> {
    Ok(i32::from_le_bytes(fourcc_at(bytes, at)?))
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16> {
    let pair = region(bytes, at, 2)?;
    let pair = <[u8; 2]>::try_from(pair)
        .map_err(|_| refuse(format!("the WORD at offset {at} is not two bytes")))?;
    Ok(u16::from_le_bytes(pair))
}

/// A FourCC, or `None` — the recovery path's reader, which cannot refuse.
fn try_fourcc(bytes: &[u8], at: usize) -> Option<[u8; 4]> {
    let end = at.checked_add(4)?;
    <[u8; 4]>::try_from(bytes.get(at..end)?).ok()
}

/// A little-endian `DWORD`, or `None`.
fn try_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(try_fourcc(bytes, at)?))
}

/// A FourCC as text for an error message, with anything unprintable escaped.
///
/// A chunk id is header data, not payload, so quoting it costs nothing under rubric A12 —
/// and a refusal that says which FourCC it found is the difference between a diagnosis and
/// a shrug. Escaped because a corrupt id is exactly the case where this runs.
fn fourcc_name(id: [u8; 4]) -> String {
    let mut out = String::with_capacity(16);
    for byte in id {
        if byte.is_ascii_graphic() || byte == b' ' {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("\\x{byte:02x}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first frame's payload — odd, so that every walk in this module is asked the pad
    /// question. Shaped like a JPEG (SOI … EOI) because that is what the muxer will carry;
    /// nothing here decodes it.
    const FRAME_A: [u8; 5] = [0xFF, 0xD8, 0x5A, 0xFF, 0xD9];
    /// The second frame's payload — even, so the two arms of the pad path are both live.
    const FRAME_B: [u8; 4] = [0xFF, 0xD8, 0xFF, 0xD9];
    const CANON_WIDTH: u32 = 640;
    const CANON_HEIGHT: u32 = 480;
    /// 30 fps as a microsecond interval, which is what `dwScale`/`dwRate` carries too.
    const CANON_INTERVAL_US: u32 = 33_333;

    fn push_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn push_u16(bytes: &mut Vec<u8>, value: u16) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    /// A canonical two-frame AVI, assembled from the specification byte by byte.
    ///
    /// **This is a test fixture and must never become the muxer.** docs/7 P6a wants the
    /// reader derived independently of the writer; a builder that grew into the writer, or
    /// a writer that grew out of this builder, would destroy exactly the independence the
    /// split exists for. It is here so that this module has a subject before `avi::write`
    /// exists, and every refusal below is seeded by mutating these bytes at a named offset.
    struct Canonical {
        bytes: Vec<u8>,
        riff_size_at: usize,
        hdrl_size_at: usize,
        avih_micro_at: usize,
        avih_flags_at: usize,
        avih_total_frames_at: usize,
        avih_streams_at: usize,
        avih_width_at: usize,
        avih_height_at: usize,
        strl_at: usize,
        strh_size_at: usize,
        strh_type_at: usize,
        strh_scale_at: usize,
        strh_length_at: usize,
        strf_width_at: usize,
        strf_compression_at: usize,
        movi_size_at: usize,
        movi_at: usize,
        frame_at: [usize; 2],
        frame_size_at: [usize; 2],
        pad_at: usize,
        idx1_at: usize,
        idx1_size_at: usize,
        entry_at: [usize; 2],
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one linear transcription of the RIFF/AVI layout; splitting it would hide \
                  the offsets the tests name"
    )]
    fn canonical() -> Canonical {
        let mut b = Vec::new();
        b.extend_from_slice(&RIFF);
        let riff_size_at = b.len();
        push_u32(&mut b, 0);
        b.extend_from_slice(&AVI_FORM);

        // LIST hdrl
        b.extend_from_slice(&LIST);
        let hdrl_size_at = b.len();
        push_u32(&mut b, 0);
        let hdrl_payload_at = b.len();
        b.extend_from_slice(&HDRL);

        // avih
        b.extend_from_slice(&AVIH);
        push_u32(&mut b, 56);
        let avih_micro_at = b.len();
        push_u32(&mut b, CANON_INTERVAL_US);
        push_u32(&mut b, 0); // dwMaxBytesPerSec
        push_u32(&mut b, 0); // dwPaddingGranularity
        let avih_flags_at = b.len();
        push_u32(&mut b, AVIF_HASINDEX);
        let avih_total_frames_at = b.len();
        push_u32(&mut b, 2);
        push_u32(&mut b, 0); // dwInitialFrames
        let avih_streams_at = b.len();
        push_u32(&mut b, 1); // dwStreams
        push_u32(&mut b, 5); // dwSuggestedBufferSize
        let avih_width_at = b.len();
        push_u32(&mut b, CANON_WIDTH);
        let avih_height_at = b.len();
        push_u32(&mut b, CANON_HEIGHT);
        for _ in 0..4 {
            push_u32(&mut b, 0); // dwReserved
        }

        // LIST strl
        let strl_at = b.len();
        b.extend_from_slice(&LIST);
        let strl_size_at = b.len();
        push_u32(&mut b, 0);
        let strl_payload_at = b.len();
        b.extend_from_slice(&STRL);

        // strh
        b.extend_from_slice(&STRH);
        let strh_size_at = b.len();
        push_u32(&mut b, 56);
        let strh_type_at = b.len();
        b.extend_from_slice(&VIDS);
        b.extend_from_slice(&MJPG); // fccHandler
        push_u32(&mut b, 0); // dwFlags
        push_u16(&mut b, 0); // wPriority
        push_u16(&mut b, 0); // wLanguage
        push_u32(&mut b, 0); // dwInitialFrames
        let strh_scale_at = b.len();
        push_u32(&mut b, CANON_INTERVAL_US);
        push_u32(&mut b, 1_000_000); // dwRate
        push_u32(&mut b, 0); // dwStart
        let strh_length_at = b.len();
        push_u32(&mut b, 2);
        push_u32(&mut b, 5); // dwSuggestedBufferSize
        push_u32(&mut b, 0); // dwQuality
        push_u32(&mut b, 0); // dwSampleSize
        b.extend_from_slice(&0i16.to_le_bytes()); // rcFrame.left
        b.extend_from_slice(&0i16.to_le_bytes()); // rcFrame.top
        b.extend_from_slice(&640i16.to_le_bytes()); // rcFrame.right
        b.extend_from_slice(&480i16.to_le_bytes()); // rcFrame.bottom

        // strf — BITMAPINFOHEADER
        b.extend_from_slice(&STRF);
        push_u32(&mut b, 40);
        push_u32(&mut b, 40); // biSize
        let strf_width_at = b.len();
        push_u32(&mut b, CANON_WIDTH);
        push_u32(&mut b, CANON_HEIGHT);
        push_u16(&mut b, 1); // biPlanes
        push_u16(&mut b, 24); // biBitCount
        let strf_compression_at = b.len();
        b.extend_from_slice(&MJPG);
        push_u32(&mut b, CANON_WIDTH * CANON_HEIGHT * 3); // biSizeImage
        push_u32(&mut b, 0); // biXPelsPerMeter
        push_u32(&mut b, 0); // biYPelsPerMeter
        push_u32(&mut b, 0); // biClrUsed
        push_u32(&mut b, 0); // biClrImportant

        let strl_size = b.len() - strl_payload_at;
        let hdrl_size = b.len() - hdrl_payload_at;

        // LIST movi
        b.extend_from_slice(&LIST);
        let movi_size_at = b.len();
        push_u32(&mut b, 0);
        let movi_at = b.len();
        b.extend_from_slice(&MOVI);

        let frame_a_at = b.len();
        b.extend_from_slice(&CHUNK_00DC);
        let frame_a_size_at = b.len();
        push_u32(&mut b, 5);
        b.extend_from_slice(&FRAME_A);
        let pad_at = b.len();
        b.push(0); // the pad byte: outside the chunk's size, inside the list's

        let frame_b_at = b.len();
        b.extend_from_slice(&CHUNK_00DC);
        let frame_b_size_at = b.len();
        push_u32(&mut b, 4);
        b.extend_from_slice(&FRAME_B);

        let movi_size = b.len() - movi_at;

        // idx1
        let idx1_at = b.len();
        b.extend_from_slice(&IDX1);
        let idx1_size_at = b.len();
        push_u32(&mut b, 32);
        let entry_a_at = b.len();
        b.extend_from_slice(&CHUNK_00DC);
        push_u32(&mut b, 0x10); // AVIIF_KEYFRAME
        push_u32(
            &mut b,
            u32::try_from(frame_a_at - movi_at).expect("a fixture this size fits"),
        );
        push_u32(&mut b, 5);
        let entry_b_at = b.len();
        b.extend_from_slice(&CHUNK_00DC);
        push_u32(&mut b, 0x10);
        push_u32(
            &mut b,
            u32::try_from(frame_b_at - movi_at).expect("a fixture this size fits"),
        );
        push_u32(&mut b, 4);

        let mut file = Canonical {
            bytes: b,
            riff_size_at,
            hdrl_size_at,
            avih_micro_at,
            avih_flags_at,
            avih_total_frames_at,
            avih_streams_at,
            avih_width_at,
            avih_height_at,
            strl_at,
            strh_size_at,
            strh_type_at,
            strh_scale_at,
            strh_length_at,
            strf_width_at,
            strf_compression_at,
            movi_size_at,
            movi_at,
            frame_at: [frame_a_at, frame_b_at],
            frame_size_at: [frame_a_size_at, frame_b_size_at],
            pad_at,
            idx1_at,
            idx1_size_at,
            entry_at: [entry_a_at, entry_b_at],
        };
        let total = file.bytes.len();
        file.put_u32(riff_size_at, u32::try_from(total - 8).expect("fits"));
        file.put_u32(hdrl_size_at, u32::try_from(hdrl_size).expect("fits"));
        file.put_u32(strl_size_at, u32::try_from(strl_size).expect("fits"));
        file.put_u32(movi_size_at, u32::try_from(movi_size).expect("fits"));
        file
    }

    impl Canonical {
        fn put_u32(&mut self, at: usize, value: u32) {
            self.bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }

        fn put_fourcc(&mut self, at: usize, value: &[u8; 4]) {
            self.bytes[at..at + 4].copy_from_slice(value);
        }

        /// Read a `DWORD` back, for the mutations that adjust a field relative to its
        /// current value.
        fn get_u32(&self, at: usize) -> u32 {
            u32::from_le_bytes(self.bytes[at..at + 4].try_into().expect("four bytes"))
        }

        /// The message of the refusal these bytes must produce.
        fn refusal(&self) -> String {
            match read_stream(&self.bytes) {
                Ok(stream) => panic!(
                    "expected a refusal; parsed {} frames and {} index entries",
                    stream.frames.len(),
                    stream.index.len()
                ),
                Err(err) => err.to_string(),
            }
        }
    }

    fn refusal_for(bytes: &[u8]) -> String {
        match read_stream(bytes) {
            Ok(stream) => panic!("expected a refusal; parsed {} frames", stream.frames.len()),
            Err(err) => err.to_string(),
        }
    }

    #[test]
    fn the_canonical_fixture_is_the_layout_its_offsets_name() {
        // Every corruption test below mutates this file at a named offset, so a builder
        // that silently moved a field would turn those tests into assertions about
        // whatever now sits there. The absolute offsets are the specification's structure
        // sizes added up, and they are stated once, here.
        let file = canonical();
        assert_eq!(file.riff_size_at, 4);
        assert_eq!(file.avih_micro_at, 32, "avih payload starts at 32");
        assert_eq!(file.strh_type_at, 108, "strh payload starts at 108");
        assert_eq!(file.strf_width_at, 176);
        assert_eq!(file.movi_at, 220, "the movi FourCC");
        assert_eq!(file.frame_at, [224, 238]);
        assert_eq!(file.pad_at, 237);
        assert_eq!(file.idx1_at, 250);
        assert_eq!(file.bytes.len(), 290);
        // The two numbers the whole module is about: the first frame chunk sits 4 bytes
        // into movi, and the second sits past an odd payload *and* its pad byte.
        assert_eq!(file.frame_at[0] - file.movi_at, 4);
        assert_eq!(file.frame_at[1] - file.frame_at[0], 8 + 5 + 1);
    }

    #[test]
    fn the_canonical_file_parses_and_its_frames_are_byte_identical_to_what_went_in() {
        // E6: MJPEG frames are remuxed verbatim, and a reader that returned anything but
        // the exact input bytes would make that claim untestable. Also the pad path in
        // both directions — frame A is odd, frame B is even.
        let file = canonical();
        let stream = read_stream(&file.bytes).expect("the canonical file parses");

        assert_eq!(stream.frames.len(), 2);
        assert_eq!(stream.frames[0].bytes, &FRAME_A[..]);
        assert_eq!(stream.frames[1].bytes, &FRAME_B[..]);
        assert_eq!(stream.frames[0].chunk_id, CHUNK_00DC);
        // The offsets an idx1 entry carries: relative to the `movi` FourCC, so the first
        // frame is at 4, and the second is past 8 header bytes, 5 payload bytes and one
        // pad byte that the chunk's own size field does not count.
        assert_eq!(stream.frames[0].movi_offset, 4);
        assert_eq!(stream.frames[1].movi_offset, 18);

        let headers = stream.headers;
        assert_eq!(headers.micro_sec_per_frame, CANON_INTERVAL_US);
        assert_eq!(headers.stream_scale, CANON_INTERVAL_US);
        assert_eq!(headers.stream_rate, 1_000_000);
        assert_eq!(headers.total_frames, 2);
        assert_eq!(headers.stream_length, 2);
        assert_eq!((headers.width, headers.height), (CANON_WIDTH, CANON_HEIGHT));
        assert_eq!(headers.compression, MJPG);
        assert_eq!(headers.handler, MJPG);
        assert_eq!(headers.bit_count, 24);
        assert!(headers.has_index());

        assert_eq!(stream.index.len(), 2);
        assert_eq!(stream.index[0].offset, 4);
        assert_eq!(stream.index[0].size, 5);
        assert_eq!(
            stream.index[0].flags, 0x10,
            "every MJPEG frame is a keyframe"
        );
        assert_eq!(stream.index[1].offset, 18);
        assert_eq!(stream.index[1].size, 4);
    }

    #[test]
    fn a_file_that_does_not_begin_with_riff_is_refused_rather_than_parsed_as_one() {
        let mut file = canonical();
        file.put_fourcc(0, b"RIFX");
        let message = file.refusal();
        assert!(message.contains(NOT_RIFF), "{message}");
        assert!(message.contains("RIFX"), "{message}");

        // And a file too short to hold an envelope at all, which must refuse rather than
        // index into nothing.
        let message = refusal_for(b"RIFF");
        assert!(message.contains(NOT_RIFF), "{message}");

        // The boundary itself, which is a different file with a different diagnosis: 12
        // bytes *is* a whole RIFF envelope, so a file of exactly that length is a
        // truncated AVI and not a foreign one, and it must be refused for what it lacks
        // rather than for its length. The two answers send a reader to different places —
        // "this is not our format" versus "this recording lost its header list" — and the
        // length comparison is the only thing keeping them apart.
        let mut envelope = Vec::new();
        envelope.extend_from_slice(&RIFF);
        envelope.extend_from_slice(&4u32.to_le_bytes());
        envelope.extend_from_slice(&AVI_FORM);
        assert_eq!(envelope.len(), CHUNK_HEADER_BYTES + 4);
        let message = refusal_for(&envelope);
        assert!(message.contains(MISSING), "{message}");
        assert!(message.contains("LIST hdrl"), "{message}");
        assert!(!message.contains(NOT_RIFF), "{message}");
    }

    #[test]
    fn a_form_type_other_than_avi_is_refused_rather_than_read_as_a_video() {
        // A WAVE file is a RIFF file, and reading one as an AVI would walk `fmt `/`data`
        // chunks looking for frames.
        let mut file = canonical();
        file.put_fourcc(8, b"WAVE");
        let message = file.refusal();
        assert!(message.contains(NOT_AVI_FORM), "{message}");
        assert!(message.contains("WAVE"), "{message}");
    }

    #[test]
    fn a_riff_size_field_that_disagrees_with_the_bytes_present_is_refused() {
        // The crash case in disguise: a muxer that never patched its placeholder leaves a
        // file whose outermost claim is false, and `read_stream` must say so rather than
        // parse the prefix it happens to cover. `recover_frames` is the path for that
        // file, and its own test is below.
        let file_len = canonical().bytes.len();
        let mut file = canonical();
        file.put_u32(file.riff_size_at, 0);
        let message = file.refusal();
        assert!(message.contains(RIFF_SIZE_MISMATCH), "{message}");
        assert!(message.contains("declares 0 bytes"), "{message}");
        assert!(
            message.contains(&format!("{} are present", file_len - 8)),
            "{message}"
        );

        // Too large as well as too small — a size field that overruns the file must not be
        // believed just because it is bigger.
        let mut file = canonical();
        file.put_u32(file.riff_size_at, u32::MAX);
        let message = file.refusal();
        assert!(message.contains(RIFF_SIZE_MISMATCH), "{message}");
        assert!(message.contains(&u32::MAX.to_string()), "{message}");
    }

    #[test]
    fn a_chunk_that_runs_past_its_enclosing_list_is_refused_rather_than_read_past_it() {
        // `strh` claiming more bytes than `strl` holds. Without the bound, the header
        // parse would read `strf`'s bytes as the tail of the stream header — a
        // mis-parse that produces plausible numbers rather than an error.
        let mut file = canonical();
        file.put_u32(file.strh_size_at, 200);
        let message = file.refusal();
        assert!(message.contains(CHUNK_OVERRUN), "{message}");
        assert!(message.contains("`strh`"), "{message}");
        assert!(message.contains("declares 200 bytes"), "{message}");
        assert!(message.contains("LIST strl"), "{message}");
    }

    #[test]
    fn a_chunk_that_runs_past_end_of_file_is_refused_rather_than_indexed_past_it() {
        // Rubric B10 on this side of the driver boundary: a size field is device-derived
        // data, and the failure mode of believing one is an out-of-bounds read.
        let mut file = canonical();
        file.put_u32(file.frame_size_at[1], 4000);
        let message = file.refusal();
        assert!(message.contains(CHUNK_OVERRUN), "{message}");
        assert!(message.contains("declares 4000 bytes"), "{message}");
        assert!(message.contains("end of the file"), "{message}");
        assert!(
            !message.contains("plus a pad byte"),
            "an even payload has no pad byte to overrun with: {message}"
        );

        // And the odd case, which overruns by one more byte than it declares. The phrase
        // is the diagnosis: a chunk that would fit except for its pad is a different
        // finding from one that misses by four kilobytes, and the arithmetic a reader
        // checks by hand has to include the byte the message accounts for.
        let mut file = canonical();
        file.put_u32(file.frame_size_at[1], 4001);
        let message = file.refusal();
        assert!(message.contains(CHUNK_OVERRUN), "{message}");
        assert!(
            message.contains("declares 4001 bytes plus a pad byte"),
            "{message}"
        );
    }

    #[test]
    fn a_chunk_header_that_exactly_fits_is_read_and_one_byte_short_is_trailing_bytes() {
        // The boundary between "a chunk" and "a tail nobody finished writing", which is a
        // comparison against the 8-byte chunk header and is therefore off by one in two
        // directions. Both are seeded, because they fail in opposite ways: too strict
        // refuses a legal empty chunk sitting flush against the end of its list, and too
        // lax lets a 3-byte tail be read as a chunk header that is not there.
        //
        // Eight bytes of room, and a zero-length chunk to occupy them.
        let mut file = canonical();
        file.bytes.extend_from_slice(b"JUNK");
        file.bytes.extend_from_slice(&0u32.to_le_bytes());
        let riff_size = file.get_u32(file.riff_size_at);
        file.put_u32(file.riff_size_at, riff_size + 8);
        read_stream(&file.bytes).expect("a chunk header with exactly its own room is a chunk");

        // Three bytes of room, which is not a chunk header and must say so rather than
        // reaching for four bytes of FourCC that are not there.
        let mut file = canonical();
        file.bytes.extend_from_slice(&[0, 0, 0]);
        let riff_size = file.get_u32(file.riff_size_at);
        file.put_u32(file.riff_size_at, riff_size + 3);
        let message = file.refusal();
        assert!(message.contains(TRAILING_BYTES), "{message}");
        assert!(message.contains("3 trailing bytes"), "{message}");
    }

    #[test]
    fn a_movi_size_field_that_its_chunks_do_not_account_for_is_refused() {
        // docs/7 P6a's chunk-accounting criterion, from the reader's side: the walk is
        // driven by the chunks, so this comparison is not a restatement of its own bound.
        let mut file = canonical();
        let declared = file.get_u32(file.movi_size_at);
        file.put_u32(file.movi_size_at, declared + 8);
        let message = file.refusal();
        assert!(message.contains(MOVI_SIZE_MISMATCH), "{message}");
        assert!(
            message.contains(&format!("{} bytes at offset 220", declared + 8)),
            "{message}"
        );
        assert!(
            message.contains(&format!("account for {declared}")),
            "{message}"
        );
        // And where the walk stopped, which here is a chunk this reader *does* have a name
        // for — the other half of the naming rule, whose unnamed branch is asserted in
        // `a_movi_walk_that_stopped_early_…`. Printing `idx1` as "a chunk id this reader
        // does not carry" would be a message that hides the one fact that explains the
        // mismatch: the index starts here, so the size field is simply eight too big.
        assert!(
            message.contains("the walk stopped at chunk `idx1`"),
            "{message}"
        );

        // And too small, which is the more dangerous direction: a walk bounded by this
        // number would simply stop early and report one frame instead of two.
        let mut file = canonical();
        file.put_u32(file.movi_size_at, declared - 12);
        let message = file.refusal();
        assert!(message.contains(MOVI_SIZE_MISMATCH), "{message}");
        assert!(
            message.contains(&format!("account for {declared}")),
            "{message}"
        );
    }

    #[test]
    fn an_odd_length_chunk_without_a_zero_pad_byte_is_refused_rather_than_walked_out_of_phase() {
        // The single most common muxer bug, seeded two ways because it can arrive two ways.
        //
        // First: the pad byte is there but is not a pad — the muxer wrote payload where
        // padding belongs. Nothing about the sizes is wrong, so only the "a pad byte is
        // zero" rule can see it, and that rule is what makes the second case detectable at
        // all. `0x30` is `'0'`, the first byte of the following `00dc`: exactly what a walk
        // one byte out of phase would read as padding.
        let mut file = canonical();
        file.bytes[file.pad_at] = 0x30;
        let message = file.refusal();
        assert!(message.contains(BAD_PAD), "{message}");
        assert!(message.contains("`00dc`"), "{message}");
        assert!(message.contains("declares an odd 5 bytes"), "{message}");
        assert!(
            message.contains("the byte at offset 237 is not zero"),
            "{message}"
        );
        assert!(
            !message.contains("0x30"),
            "the byte's value is a frame byte whenever the size field is the thing that is \
             wrong (rubric A12): {message}"
        );

        // Second: the pad byte is missing and every size field has been corrected around
        // it, so the file is internally consistent except for the padding. It is still
        // refused, and the refusal lands on the *enclosing* list rather than on the frame:
        // every properly padded chunk contributes an even number of bytes, so a `movi` list
        // missing one pad is an odd-length list, and the same rule catches it one level up.
        // That is worth pinning — it means a muxer cannot hide a missing pad by making its
        // own accounting agree with itself.
        let mut file = canonical();
        file.bytes.remove(file.pad_at);
        let movi_size = file.get_u32(file.movi_size_at);
        file.put_u32(file.movi_size_at, movi_size - 1);
        let riff_size = file.get_u32(file.riff_size_at);
        file.put_u32(file.riff_size_at, riff_size - 1);
        // idx1 entry 1 now points one byte earlier.
        let entry_offset_at = file.entry_at[1] + 8;
        let offset = file.get_u32(entry_offset_at);
        file.put_u32(entry_offset_at, offset - 1);

        let message = file.refusal();
        assert!(message.contains(BAD_PAD), "{message}");
        assert!(message.contains("`LIST`"), "{message}");
        assert!(message.contains("declares an odd 29 bytes"), "{message}");
    }

    #[test]
    fn a_pad_byte_counted_inside_the_chunks_own_size_field_is_refused() {
        // The other half of the pad rule, and the one the walk cannot see: a 5-byte chunk
        // claiming 6 ends exactly where a correct one does. `idx1`'s dwChunkLength excludes
        // the pad by specification, so the index and the chunk header disagree by one —
        // which is the whole reason this module validates the index against the chunks
        // instead of merely parsing it.
        let mut file = canonical();
        file.put_u32(file.frame_size_at[0], 6);
        let message = file.refusal();
        assert!(message.contains(INDEX_LENGTH_MISMATCH), "{message}");
        assert!(message.contains("length of 5"), "{message}");
        assert!(message.contains("size field says 6"), "{message}");
    }

    #[test]
    fn a_total_frame_count_that_disagrees_with_the_chunks_found_is_refused() {
        // A dropped frame that never reached `movi` while the counter advanced anyway is
        // exactly the defect the notes care about: it makes a recording claim a duration it
        // did not deliver.
        let mut file = canonical();
        file.put_u32(file.avih_total_frames_at, 3);
        let message = file.refusal();
        assert!(message.contains(FRAME_COUNT_MISMATCH), "{message}");
        assert!(message.contains("2 frame chunks"), "{message}");
        assert!(message.contains("avih.dwTotalFrames"), "{message}");
        assert!(message.contains("says 3"), "{message}");
    }

    #[test]
    fn a_stream_length_that_disagrees_with_the_chunks_found_is_refused() {
        // The second home of the frame count. A muxer that patches `avih` at close and
        // forgets `strh` passes the test above and fails this one, which is the point of
        // checking both separately.
        let mut file = canonical();
        file.put_u32(file.strh_length_at, 7);
        let message = file.refusal();
        assert!(message.contains(FRAME_COUNT_MISMATCH), "{message}");
        assert!(message.contains("strh.dwLength"), "{message}");
        assert!(message.contains("says 7"), "{message}");
    }

    #[test]
    fn a_rate_rewritten_in_one_place_and_stale_in_the_other_is_refused() {
        // D7's CFR carve-out lives in two fields and both are rewritten at close. A file
        // that answers "how long did this take" two ways is answering the agent's question
        // wrongly, and this is the check that catches it.
        let mut file = canonical();
        file.put_u32(file.avih_micro_at, 40_000);
        let message = file.refusal();
        assert!(message.contains(RATE_MISMATCH), "{message}");
        assert!(message.contains("says 40000"), "{message}");
        assert!(message.contains("works out to 33333"), "{message}");

        // The other direction: `strh` rewritten, `avih` stale.
        let mut file = canonical();
        file.put_u32(file.strh_scale_at, 66_666);
        let message = file.refusal();
        assert!(message.contains(RATE_MISMATCH), "{message}");
        assert!(message.contains("works out to 66666"), "{message}");

        // And a rate of zero, which is not a slow stream but an unanswerable question.
        let mut file = canonical();
        file.put_u32(file.strh_scale_at + 4, 0);
        let message = file.refusal();
        assert!(message.contains(ZERO_RATE), "{message}");
    }

    #[test]
    fn a_one_microsecond_gap_between_the_two_rate_fields_is_tolerated() {
        // Non-vacuity for the tolerance: `dwScale`/`dwRate` is a rational and
        // `dwMicroSecPerFrame` is an integer, so a writer that rounded up where this
        // module truncates is one apart and correct. If this test ever fails the tolerance
        // has been tightened to zero, and 29.97 fps stops being expressible.
        let mut file = canonical();
        file.put_u32(file.avih_micro_at, CANON_INTERVAL_US + 1);
        assert!(read_stream(&file.bytes).is_ok());
        let mut file = canonical();
        file.put_u32(file.avih_micro_at, CANON_INTERVAL_US + 2);
        assert!(file.refusal().contains(RATE_MISMATCH));
    }

    #[test]
    fn an_index_entry_that_is_not_the_frame_at_its_own_position_is_refused() {
        // `idx1` is a seek table and it is positional: entry i is frame i. Checking each
        // entry against *some* frame — which is what a lookup by offset does — leaves three
        // muxer bugs invisible, and all three send a player to the wrong frame. Every one
        // of them is seeded here, because they arrive by different routes.
        //
        // First: an offset that is no frame at all. Offset 19 is one byte into the second
        // frame's chunk header.
        let mut file = canonical();
        file.put_u32(file.entry_at[1] + 8, 19);
        let message = file.refusal();
        assert!(message.contains(INDEX_OUT_OF_ORDER), "{message}");
        assert!(message.contains("movi offset 19"), "{message}");
        assert!(message.contains("begins at movi offset 18"), "{message}");

        // Second: the two entries swapped. Every entry still resolves to a frame, the ids
        // match, the lengths match and the counts match — nothing but the *order* is wrong,
        // and the order is the whole content of a seek table. A muxer that emitted its
        // index backwards would have shipped undetected.
        let mut file = canonical();
        let first = file.bytes[file.entry_at[0]..file.entry_at[0] + IDX1_ENTRY_BYTES].to_vec();
        let second = file.bytes[file.entry_at[1]..file.entry_at[1] + IDX1_ENTRY_BYTES].to_vec();
        file.bytes[file.entry_at[0]..file.entry_at[0] + IDX1_ENTRY_BYTES].copy_from_slice(&second);
        file.bytes[file.entry_at[1]..file.entry_at[1] + IDX1_ENTRY_BYTES].copy_from_slice(&first);
        let message = file.refusal();
        assert!(message.contains(INDEX_OUT_OF_ORDER), "{message}");
        assert!(message.contains("idx1 entry 0"), "{message}");

        // Third: entry 1 duplicating entry 0 — an index whose offset failed to advance.
        // The second frame is then indexed by nothing, and a player seeking through the
        // index gets frame 0 twice and never reaches frame 1. That the two frames here have
        // different lengths is what makes the length check *look* like it would catch this;
        // it does not, because a duplicated entry duplicates the length too.
        let mut file = canonical();
        file.put_u32(file.entry_at[1] + 8, 4);
        file.put_u32(file.entry_at[1] + 12, 5);
        let message = file.refusal();
        assert!(message.contains(INDEX_OUT_OF_ORDER), "{message}");
        assert!(
            message.contains("idx1 entry 1 names movi offset 4"),
            "{message}"
        );
    }

    #[test]
    fn an_index_length_that_disagrees_with_the_chunk_size_field_is_refused() {
        let mut file = canonical();
        file.put_u32(file.entry_at[0] + 12, 9);
        let message = file.refusal();
        assert!(message.contains(INDEX_LENGTH_MISMATCH), "{message}");
        assert!(message.contains("length of 9"), "{message}");
        assert!(message.contains("size field says 5"), "{message}");
    }

    #[test]
    fn an_index_entry_naming_a_different_chunk_than_the_one_at_its_offset_is_refused() {
        let mut file = canonical();
        file.put_fourcc(file.entry_at[0], b"01wb");
        let message = file.refusal();
        assert!(message.contains(INDEX_ID_MISMATCH), "{message}");
        // The entry's own id is *not* quoted: an index chunk located by a walk is bytes
        // like any other, and the two sides of this disagreement are told apart by the one
        // this reader has a name for (rubric A12; `located`).
        assert!(
            message.contains("an id this reader does not carry"),
            "{message}"
        );
        assert!(message.contains("00dc"), "{message}");
        assert!(!message.contains("01wb"), "{message}");
    }

    #[test]
    fn an_index_that_does_not_have_an_entry_per_frame_is_refused() {
        // Half an index is worse than none: a player seeks by it and never reaches the
        // frames it omits, and nothing about the file looks wrong.
        let mut file = canonical();
        file.put_u32(file.idx1_size_at, 16);
        file.bytes.truncate(file.entry_at[1]);
        let riff_size = file.get_u32(file.riff_size_at);
        file.put_u32(file.riff_size_at, riff_size - 16);
        let message = file.refusal();
        assert!(message.contains(INDEX_COUNT_MISMATCH), "{message}");
        assert!(message.contains("holds 1 "), "{message}");
        assert!(message.contains("2 frames"), "{message}");
    }

    #[test]
    fn an_idx1_payload_that_is_not_a_whole_number_of_entries_is_refused() {
        let mut file = canonical();
        file.put_u32(file.idx1_size_at, 31);
        let message = file.refusal();
        assert!(message.contains(INDEX_RAGGED), "{message}");
        assert!(message.contains("declares 31 bytes"), "{message}");
    }

    #[test]
    fn hasindex_set_without_an_idx1_is_refused_rather_than_believed() {
        // D7's crash promise, from the reader's side: a file that died mid-recording must
        // not claim an index it does not have, because the flag is what a player consults
        // before seeking.
        let mut file = canonical();
        file.bytes.truncate(file.idx1_at);
        let removed = u32::try_from(290 - file.idx1_at).expect("fits");
        let riff_size = file.get_u32(file.riff_size_at);
        file.put_u32(file.riff_size_at, riff_size - removed);
        let message = file.refusal();
        assert!(message.contains(HASINDEX_WITHOUT_INDEX), "{message}");
    }

    #[test]
    fn an_idx1_present_while_hasindex_is_clear_is_refused_rather_than_believed() {
        // The inverse, and the reason the flag is set only at close: a muxer that wrote the
        // index and forgot the flag produces a file every player treats as unseekable.
        let mut file = canonical();
        file.put_u32(file.avih_flags_at, 0);
        let message = file.refusal();
        assert!(message.contains(INDEX_WITHOUT_HASINDEX), "{message}");
        assert!(message.contains("2 entries"), "{message}");
    }

    #[test]
    fn a_compression_other_than_mjpg_is_refused_rather_than_passed_through() {
        // D7 L0: the muxer is MJPG-only by design, and a container claiming H.264 payloads
        // in `00dc` chunks is a file this project cannot have written.
        let mut file = canonical();
        file.put_fourcc(file.strf_compression_at, b"H264");
        let message = file.refusal();
        assert!(message.contains(NOT_MJPG), "{message}");
        assert!(message.contains("H264"), "{message}");
    }

    #[test]
    fn a_stream_that_is_not_video_is_refused() {
        let mut file = canonical();
        file.put_fourcc(file.strh_type_at, b"auds");
        let message = file.refusal();
        assert!(message.contains(NOT_VIDEO), "{message}");
        assert!(message.contains("auds"), "{message}");
    }

    #[test]
    fn headers_claiming_a_zero_dimension_are_refused_rather_than_reported() {
        // A zero-extent recording is a file every downstream consumer divides by.
        let mut file = canonical();
        file.put_u32(file.avih_width_at, 0);
        let message = file.refusal();
        assert!(message.contains(ZERO_EXTENT), "{message}");
        assert!(message.contains("0x480"), "{message}");

        let mut file = canonical();
        file.put_u32(file.avih_height_at, 0);
        let message = file.refusal();
        assert!(message.contains(ZERO_EXTENT), "{message}");
        assert!(message.contains("640x0"), "{message}");
    }

    #[test]
    fn geometry_that_disagrees_between_avih_and_the_bitmapinfoheader_is_refused() {
        // The same defect class as the rate: one number, two homes, and a muxer that
        // rewrites one of them. A player that believes `strf` and a report that believes
        // `avih` would describe two different recordings.
        let mut file = canonical();
        file.put_u32(file.strf_width_at, 320);
        let message = file.refusal();
        assert!(message.contains(EXTENT_MISMATCH), "{message}");
        assert!(message.contains("640x480"), "{message}");
        assert!(message.contains("320x480"), "{message}");
    }

    #[test]
    fn a_chunk_belonging_to_another_stream_is_refused_rather_than_silently_skipped() {
        // Design §1 makes audio a non-goal, so a second stream's chunks are not "not our
        // frames" — they are a file this reader has no model for. Skipping them would make
        // the frame counts disagree for a reason the message would never mention.
        let mut file = canonical();
        file.put_fourcc(file.frame_at[1], b"01wb");
        let message = file.refusal();
        assert!(message.contains(FOREIGN_STREAM), "{message}");
        assert!(message.contains(SECOND_STREAM), "{message}");
        assert!(message.contains("offset 238"), "{message}");
        // The id is not quoted: inside `movi` a chunk id is four bytes that a corrupt size
        // field may have taken out of a JPEG (rubric A12; `located`).
        assert!(!message.contains("01wb"), "{message}");
    }

    #[test]
    fn a_stream_zero_chunk_that_is_not_a_frame_is_reported_as_that_rather_than_as_a_second_stream()
    {
        // `00pc` is a palette change: stream 0's, legal in a video stream by the
        // specification, and not a frame. Reporting it as evidence of a second stream is
        // self-contradictory — it names stream 00, which is the stream this reader carries
        // — and sends a maintainer looking for a `LIST strl` that is not in the file. Every
        // refusal in this module is supposed to name the defect it caught.
        let mut file = canonical();
        file.put_fourcc(file.frame_at[1], b"00pc");
        let message = file.refusal();
        assert!(message.contains(NOT_A_FRAME_CHUNK), "{message}");
        assert!(message.contains("offset 238"), "{message}");
        assert!(
            !message.contains(FOREIGN_STREAM) && !message.contains(SECOND_STREAM),
            "a stream-0 chunk is not evidence of a second stream: {message}"
        );
        assert!(!message.contains("00pc"), "{message}");
    }

    #[test]
    fn a_stream_chunk_id_is_two_digits_and_not_one() {
        // The specification spells a stream chunk `##xx` — *both* of the first two
        // characters are decimal digits, because the pair is the stream number. Requiring
        // only one would make `0Xdc` stream data, which puts a chunk this reader has no
        // model for inside the frame list and reports it as a foreign *stream* rather than
        // as the end of the walk. The two answers are different files: one has a second
        // stream in it, the other has a chunk id nobody wrote.
        let mut file = canonical();
        file.put_fourcc(file.frame_at[1], b"0Xdc");
        let message = file.refusal();
        assert!(message.contains(MOVI_SIZE_MISMATCH), "{message}");
        assert!(message.contains("up to offset 238"), "{message}");
        assert!(
            message.contains("a chunk id this reader does not carry"),
            "{message}"
        );
        assert!(
            !message.contains(FOREIGN_STREAM),
            "one digit is not a stream number: {message}"
        );
    }

    #[test]
    fn a_list_inside_hdrl_that_is_not_a_strl_is_skipped_rather_than_read_as_a_stream() {
        // `LIST odml` is the OpenDML extended header, and it is legal inside `hdrl` — many
        // AVI writers emit one. Deciding "this is a stream list" from the `LIST` id alone
        // would parse it as one, find no `strh` in it and refuse a conforming file; worse,
        // it would count toward `dwStreams` and make a one-stream file look like two. The
        // list type is the half that decides, and this is the fixture that says so.
        let mut file = canonical();
        let mut odml = Vec::new();
        odml.extend_from_slice(&LIST);
        odml.extend_from_slice(&20u32.to_le_bytes()); // `odml` + the whole `dmlh` chunk
        odml.extend_from_slice(b"odml");
        odml.extend_from_slice(b"dmlh");
        odml.extend_from_slice(&8u32.to_le_bytes());
        odml.extend_from_slice(&[0; 8]);
        let grew = u32::try_from(odml.len()).expect("twenty-eight bytes");
        let at = file.strl_at;
        file.bytes.splice(at..at, odml);
        let hdrl_size = file.get_u32(file.hdrl_size_at);
        file.put_u32(file.hdrl_size_at, hdrl_size + grew);
        let riff_size = file.get_u32(file.riff_size_at);
        file.put_u32(file.riff_size_at, riff_size + grew);

        let stream = read_stream(&file.bytes).expect("an odml header list is not a stream list");
        assert_eq!(stream.frames.len(), 2);
        assert_eq!(
            stream.headers.stream_length, 2,
            "the one stream is still the one"
        );
    }

    #[test]
    fn a_movi_walk_that_stopped_early_says_where_rather_than_blaming_the_size_field() {
        // RIFF permits `JUNK` anywhere and `LIST rec ` groupings inside `movi`; this reader
        // deliberately carries neither (`is_stream_chunk`), because it is the muxer's
        // adversary and our muxer writes neither. What it must not do is diagnose the
        // refusal wrongly: the walk ends at the foreign chunk, the accounting then falls
        // short, and a message naming only the two numbers points at a `LIST movi` size
        // field that is perfectly correct — sending a reader hunting an accounting bug that
        // does not exist.
        let mut file = canonical();
        let junk = {
            let mut bytes = b"JUNK".to_vec();
            bytes.extend_from_slice(&4u32.to_le_bytes());
            bytes.extend_from_slice(&[0, 0, 0, 0]);
            bytes
        };
        let at = file.idx1_at;
        let grew = u32::try_from(junk.len()).expect("twelve bytes");
        file.bytes.splice(at..at, junk);
        let movi_size = file.get_u32(file.movi_size_at);
        file.put_u32(file.movi_size_at, movi_size + grew);
        let riff_size = file.get_u32(file.riff_size_at);
        file.put_u32(file.riff_size_at, riff_size + grew);

        let message = file.refusal();
        assert!(message.contains(MOVI_SIZE_MISMATCH), "{message}");
        assert!(message.contains("up to offset 250"), "{message}");
        assert!(
            message.contains("the walk stopped at a chunk id this reader does not carry"),
            "{message}"
        );
        // And the `JUNK` id itself is not echoed, for the same reason no other walked id
        // is: at this offset the walk may already be inside a frame.
        assert!(!message.contains("JUNK"), "{message}");
    }

    #[test]
    fn no_refusal_quotes_a_byte_of_the_frame_the_walk_landed_in() {
        // Rubric A12, at the one place this module can violate it without noticing. A
        // FourCC is header data only while the walk that found it is synchronised, and a
        // size field is exactly what desynchronises one — so both of these seed a *lying
        // size field* rather than a corrupt id, which is what puts the reader's cursor
        // inside a JPEG.
        //
        // First: the pad byte. A chunk claiming three bytes where it holds five puts
        // `payload_end` at 235, which is FRAME_A[3] — a payload byte, and one a refusal
        // used to print as `0xff`.
        let mut file = canonical();
        file.put_u32(file.frame_size_at[0], 3);
        let message = file.refusal();
        assert!(message.contains(BAD_PAD), "{message}");
        assert!(message.contains("offset 235"), "{message}");
        assert_eq!(FRAME_A[3], 0xFF, "the fixture byte this test is about");
        assert!(!message.contains("0xff"), "{message}");

        // Second: the chunk id. A zero-length first chunk leaves the walk at 232, which is
        // inside frame A's payload; the bytes planted there are two ASCII digits, so
        // `is_stream_chunk` accepts them and the refusal that follows would quote four
        // bytes of the frame.
        let mut file = canonical();
        file.put_u32(file.frame_size_at[0], 0);
        file.put_fourcc(232, b"12ab");
        file.put_u32(236, u32::MAX);
        let message = file.refusal();
        assert!(message.contains(CHUNK_OVERRUN), "{message}");
        assert!(message.contains("the chunk at offset 232"), "{message}");
        assert!(
            !message.contains("12ab"),
            "four bytes read out of a frame reached a refusal: {message}"
        );
    }

    #[test]
    fn a_second_stream_is_refused_rather_than_quietly_reduced_to_the_first() {
        // Design §1 makes audio a non-goal, so this reader has no model for a second
        // stream. Overwriting the first `strl`'s fields with the second's — which is what
        // a walk that just keeps assigning does — would produce a stream description
        // belonging to neither, and the frame counts would then disagree for a reason no
        // message would name. The seeded file is a byte-identical duplicate of the one
        // `strl`, so nothing but the count can catch it.
        let mut file = canonical();
        let strl_end = file.movi_size_at - 4;
        let copy = file.bytes[file.strl_at..strl_end].to_vec();
        let grew = u32::try_from(copy.len()).expect("fits");
        file.bytes.splice(strl_end..strl_end, copy);
        let hdrl_size = file.get_u32(file.hdrl_size_at);
        file.put_u32(file.hdrl_size_at, hdrl_size + grew);
        let riff_size = file.get_u32(file.riff_size_at);
        file.put_u32(file.riff_size_at, riff_size + grew);
        let message = file.refusal();
        assert!(message.contains(SECOND_STREAM), "{message}");
        assert!(message.contains("a second LIST strl"), "{message}");

        // And the counter that describes them: `dwStreams` is a second home for "how many
        // streams are here", so it is held to the lists actually found.
        let mut file = canonical();
        file.put_u32(file.avih_streams_at, 2);
        let message = file.refusal();
        assert!(message.contains(STREAM_COUNT_MISMATCH), "{message}");
        assert!(message.contains("says 2"), "{message}");
        assert!(message.contains("1 LIST strl"), "{message}");
    }

    #[test]
    fn a_header_chunk_shorter_than_its_specified_structure_is_refused() {
        // A short `strh` is how a reader ends up with a `dwRate` assembled from whatever
        // followed the chunk.
        let mut file = canonical();
        file.put_u32(file.strh_size_at, 40);
        let message = file.refusal();
        assert!(message.contains(SHORT_HEADER), "{message}");
        assert!(message.contains("AVIStreamHeader"), "{message}");
        assert!(message.contains("declares 40 bytes"), "{message}");
    }

    #[test]
    fn truncation_yields_the_complete_frames_and_a_refusal_from_the_strict_path() {
        // D7 L0's crash promise: "a crash leaves a recoverable `movi` stream". Three
        // truncation points, because they fail differently — mid header, mid payload, and
        // exactly on a chunk boundary — and a recovery that got the boundary case wrong
        // would return a frame whose bytes were never written.
        let file = canonical();
        let cases: [(usize, usize, &str); 4] = [
            (file.frame_at[1], 1, "exactly at a chunk boundary"),
            (file.frame_at[1] + 2, 1, "mid chunk header"),
            (file.frame_at[1] + 10, 1, "mid payload"),
            (file.idx1_at, 2, "after the last frame, before idx1"),
        ];
        for (cut, expected, what) in cases {
            let truncated = &file.bytes[..cut];
            let frames = recover_frames(truncated);
            assert_eq!(frames.len(), expected, "{what}: cut at {cut}");
            assert_eq!(frames[0].bytes, &FRAME_A[..], "{what}");
            assert_eq!(frames[0].movi_offset, 4, "{what}");
            if expected == 2 {
                assert_eq!(frames[1].bytes, &FRAME_B[..], "{what}");
            }
            // The strict path refuses every one of them: the RIFF size field describes a
            // file that is no longer there.
            let message = refusal_for(truncated);
            assert!(message.contains(RIFF_SIZE_MISMATCH), "{what}: {message}");
        }
    }

    #[test]
    fn recovery_reads_the_frames_a_crash_left_behind_without_believing_a_size_field() {
        // The realistic crash: `hdrl` is complete (it was written before the first frame),
        // the RIFF and movi size fields are still the muxer's placeholder, there is no
        // index, and the last frame is half-written. Everything `recover_frames` needs was
        // on the sink before the frames were, which is what makes the promise keepable.
        let mut file = canonical();
        file.bytes.truncate(file.frame_at[1] + 10);
        file.put_u32(file.riff_size_at, 0);
        file.put_u32(file.movi_size_at, 0);
        file.put_u32(file.avih_flags_at, 0);
        file.put_u32(file.avih_total_frames_at, 0);
        file.put_u32(file.strh_length_at, 0);

        let frames = recover_frames(&file.bytes);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].bytes, &FRAME_A[..]);

        // A placeholder of `u32::MAX` — the other obvious choice — must not change the
        // answer either, since nothing on this path reads the field.
        let mut file = canonical();
        file.bytes.truncate(file.frame_at[1] + 10);
        file.put_u32(file.riff_size_at, u32::MAX);
        file.put_u32(file.movi_size_at, u32::MAX);
        assert_eq!(recover_frames(&file.bytes).len(), 1);
    }

    #[test]
    fn recovery_over_a_complete_file_stops_at_the_index_rather_than_reading_it_as_frames() {
        let file = canonical();
        let frames = recover_frames(&file.bytes);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].bytes, &FRAME_A[..]);
        assert_eq!(frames[1].bytes, &FRAME_B[..]);
        assert_eq!(frames[1].movi_offset, 18);
    }

    #[test]
    fn recovery_walks_the_movi_list_rather_than_the_whole_file() {
        // Recovery trusts no size field, which makes it tempting to let it scan forward
        // until the bytes run out. It must not: a chunk that merely *looks* like a frame
        // and sits outside `movi` is not a frame, and a recovery that returned it would
        // hand the caller bytes no camera produced. The guard is the stream-chunk id test,
        // which ends the walk at `idx1`; this fixture is the file that notices when it
        // stops guarding — canonical, plus a `00dc` chunk parked after the index.
        let mut file = canonical();
        file.bytes.extend_from_slice(&CHUNK_00DC);
        file.bytes.extend_from_slice(&3u32.to_le_bytes());
        file.bytes.extend_from_slice(&[0xDE, 0xAD, 0xBE]);
        file.bytes.push(0);
        let riff_size = file.get_u32(file.riff_size_at);
        file.put_u32(file.riff_size_at, riff_size + 12);

        let frames = recover_frames(&file.bytes);
        assert_eq!(frames.len(), 2, "recovery walked past the end of movi");
        assert_eq!(frames[1].bytes, &FRAME_B[..]);
    }

    #[test]
    fn recovery_refuses_a_riff_file_that_is_not_an_avi_rather_than_walking_it() {
        // `recover_frames` is total and never refuses, so the only place it can say "this
        // is not my file" is by finding no `movi` — and both halves of the envelope have to
        // be checked to do that. A WAVE file is a RIFF file, and one that happens to carry
        // a `LIST movi` would otherwise have its chunks handed back as camera frames. The
        // fixture is exactly that: our own canonical AVI with its form type rewritten, so
        // nothing but the form type distinguishes it.
        let mut file = canonical();
        file.put_fourcc(8, b"WAVE");
        assert!(
            recover_frames(&file.bytes).is_empty(),
            "recovery walked a file that is not an AVI"
        );

        // And the other half of the same envelope, so that neither check is carrying both.
        let mut file = canonical();
        file.put_fourcc(0, b"RIFX");
        assert!(recover_frames(&file.bytes).is_empty());
    }

    #[test]
    fn recovery_takes_both_spellings_of_a_frame_chunk_and_no_other_streams_chunks() {
        // `recover_frames` decides what a crashed file's `movi` actually contained, and it
        // decides it from the chunk id alone — there is no index and no size field to
        // cross-check against. Two arms of that decision were unasserted, and the mutation
        // floor found them by flipping one comparison: with `00db` misread, an
        // uncompressed-DIB frame the specification calls a frame silently disappears from
        // a recovery; with the same flip, `01wb` starts coming back *as a frame*, which
        // hands the caller audio and calls it video. The second is the worse half — the
        // recovery promise is about the frames this project wrote, and a recovery that
        // invents them is worse than one that finds none.
        let mut file = canonical();
        file.put_fourcc(file.frame_at[1], &CHUNK_00DB);
        let frames = recover_frames(&file.bytes);
        assert_eq!(frames.len(), 2, "an 00db chunk is a frame by specification");
        assert_eq!(frames[1].chunk_id, CHUNK_00DB);
        assert_eq!(frames[1].bytes, &FRAME_B[..]);

        let mut file = canonical();
        file.put_fourcc(file.frame_at[1], b"01wb");
        let frames = recover_frames(&file.bytes);
        assert_eq!(
            frames.len(),
            1,
            "a second stream's chunk is not one of our frames"
        );
        assert_eq!(frames[0].bytes, &FRAME_A[..]);
    }

    #[test]
    fn recovery_over_garbage_returns_nothing_rather_than_panicking() {
        // `recover_frames` is total by contract: it is called on the file a crash left, and
        // a recovery path that panics is a daemon that dies twice.
        assert!(recover_frames(&[]).is_empty());
        assert!(recover_frames(b"not an AVI at all, not even close").is_empty());
        assert!(recover_frames(&[0xFF; 64]).is_empty());
        assert!(recover_frames(b"RIFF").is_empty());

        // A RIFF envelope over nonsense: the header walk must terminate rather than spin,
        // and a size field of `u32::MAX` must not overflow its way into a slice.
        let mut hostile = Vec::new();
        hostile.extend_from_slice(&RIFF);
        hostile.extend_from_slice(&u32::MAX.to_le_bytes());
        hostile.extend_from_slice(&AVI_FORM);
        hostile.extend_from_slice(&LIST);
        hostile.extend_from_slice(&u32::MAX.to_le_bytes());
        hostile.extend_from_slice(&HDRL);
        assert!(recover_frames(&hostile).is_empty());

        // Every prefix of the canonical file, which is every place a crash can land.
        let file = canonical();
        for cut in 0..file.bytes.len() {
            let frames = recover_frames(&file.bytes[..cut]);
            assert!(frames.len() <= 2, "cut at {cut} produced {}", frames.len());
        }

        // And every single-byte corruption of it, which is where a size field turns into
        // something enormous.
        for at in 0..file.bytes.len() {
            let mut corrupt = file.bytes.clone();
            corrupt[at] = 0xFF;
            let frames = recover_frames(&corrupt);
            assert!(
                frames.len() <= 8,
                "corrupting {at} produced {}",
                frames.len()
            );
            // The strict path must not panic on any of them either — refusing or parsing
            // are both fine answers; a panic is not.
            let _ = read_stream(&corrupt);
        }
    }

    #[test]
    fn a_debug_rendering_of_a_frame_never_contains_the_frame() {
        // Rubric A12: a frame may contain a person, and `Debug` output reaches logs. The
        // derived implementation would print every byte, which is why both types here
        // hand-write it — the same reason `schema::capture::Frame` does.
        let file = canonical();
        let stream = read_stream(&file.bytes).expect("parses");
        let rendered = format!("{stream:?}");
        assert!(rendered.contains("<2 frames>"), "{rendered}");
        assert!(!rendered.contains("255"), "{rendered}");
        let frame = format!("{:?}", stream.frames[0]);
        assert!(frame.contains("<5 bytes>"), "{frame}");
        assert!(frame.contains("00dc"), "{frame}");
        assert!(!frame.contains("216"), "{frame}");
    }

    #[test]
    fn a_corrupt_fourcc_is_reported_as_escaped_text_rather_than_as_raw_bytes() {
        // The message path itself has to survive a corrupt chunk id: this is precisely the
        // case where a FourCC is not printable, and a refusal that panicked formatting
        // itself would turn a diagnosis into a crash.
        assert_eq!(fourcc_name(*b"00dc"), "00dc");
        assert_eq!(fourcc_name(*b"AVI "), "AVI ");
        assert_eq!(fourcc_name([0x00, 0xFF, b'a', 0x1B]), "\\x00\\xffa\\x1b");
    }
}
