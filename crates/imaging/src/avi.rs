//! The D7 L0 video container: MJPEG frames remuxed into AVI, verbatim.
//!
//! AVI is the canonical MJPEG container, the format is frozen, and no maintained AVI
//! writer exists on crates.io — so design D7 owns ~300 lines here rather than linking a
//! codec stack whose licence we could not ship. Nothing in this module decodes, re-encodes
//! or scales a frame: E6 says byte fidelity is the product, and the notes say why for
//! video specifically — "a re-encode inserts motion artefacts exactly where the agent is
//! looking for them", which would fabricate evidence about the one property being judged.
//!
//! ## Two implementations of one layout, on purpose
//!
//! [`read`] is a reader for the RIFF/AVI specification; [`mod@write`] is the muxer. The
//! two share **no** code — not a constant, not a FourCC, not a helper — because docs/7 P6a
//! asks for "an independent re-parse path that is **not** the writer's code", and a re-parse
//! assembled from the muxer's own layout helpers agrees with the muxer by construction. It
//! can catch a typo; it cannot catch the bugs a muxer actually ships, which are two halves
//! of one wrong idea agreeing with each other. [`read`] was written from the specification
//! **before** the muxer existed, and every literal in it is derived there. That is also why
//! its FourCCs, its fixed structure sizes and its flag bits are private: a shared constant
//! is a shared assumption, and the whole value of this pair is that they were assumed
//! separately.
//!
//! The reader is therefore the muxer's adversary, not its helper. What it refuses is listed
//! in [`read`]'s own documentation, and the muxer's tests are expected to go through it.
//!
//! ## One rate, two homes, and a summary that says which one is true
//!
//! D7's CFR carve-out: "AVI is a constant-frame-rate container and cameras are not: the
//! header's rate is the negotiated frame interval, rewritten at close to the measured mean
//! interval". The interval therefore lives in two header fields —
//! `avih.dwMicroSecPerFrame` and `strh.dwScale`/`dwRate` — and [`write::AviWriter::finish`]
//! rewrites both, because a reader that finds them disagreeing has caught the defect and a
//! reader that finds only one patched has been lied to consistently.
//!
//! [`AviSummary`] carries the same number *and* [`IntervalSource`], which is not
//! decoration. The notes are blunt about what is at stake — "for a transition they *are*
//! the measurement, because 'did this take 200 ms or 2 s' is the question being asked" —
//! so a caller must be able to tell a rate that was **measured** from one that is merely
//! what the camera was **asked** for. A mean needs two points; a one-frame recording has
//! measured nothing, and says so rather than dressing the negotiated interval up as a
//! finding. [`AviSummary::dropped_frames`] belongs to the same obligation: a gap in the
//! driver's `sequence` is a frame that never arrived, and reporting it is what stops a
//! dropped frame from reading as a slow transition.
//!
//! ## A recording is bounded by values, not by constants
//!
//! [`RecordingCaps`] has **no `Default`**, deliberately. docs/7 puts "duration/size caps
//! from `limits`" in P6b and `webcam-handler-schema::limits`' own header says a constant
//! nobody reads is a defect (rubric A8), so this crate takes the bounds as values and the
//! caller owns which constants they came from. The consequence is the one worth having: a
//! caller cannot forget to bound a recording, because there is no bound to forget — the
//! type will not construct without all three.
//!
//! ## A cap is an outcome; a malformed frame is an error
//!
//! [`write::AviWriter::write_frame`] answers a cap with `Ok(`[`FrameOutcome::Refused`]`)`
//! and a bad frame with `Err`. The split is AGENTS rule 7 in another costume —
//! "availability is not capability" — because a recording that stopped at its size cap did
//! exactly what it was told to, while a frame whose geometry disagrees with the stream's is
//! a defect somewhere upstream. Collapsing the two would make an agent unable to tell "your
//! recording is as long as you allowed" from "your camera is misbehaving", and the primary
//! consumer has no hands to investigate with.

use std::time::Duration;

use schema::camera::FrameInterval;

use crate::vocabulary::closed_vocabulary;

pub mod read;
pub mod write;

/// What a recording is allowed to cost.
///
/// Three bounds rather than one because they fail differently: `max_bytes` protects the
/// disk, `max_frames` protects the index and the memory it lives in, and `max_span`
/// protects the caller from a camera that is still delivering long after the thing being
/// filmed stopped happening. Whichever binds first ends the recording, and
/// [`AviSummary::cap_reached`] names it.
///
/// No `Default` (see the module doc): the values belong to the caller, and P6b is where
/// they meet `webcam-handler-schema::limits`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordingCaps {
    /// The largest the finished file may be, **index included**.
    ///
    /// Counted against the whole file rather than the frame payloads because the `idx1`
    /// written at close is 16 bytes per frame, and a cap that stopped at the last frame
    /// would be walked past by the close itself.
    pub max_bytes: u64,
    /// How many frames may be written.
    pub max_frames: u32,
    /// How far the last frame's timestamp may be from the first frame's.
    ///
    /// Measured on the driver's clock, from the first frame **written** — this crate reads
    /// no clock of its own (design §2.10), so wall-clock time is the caller's question.
    pub max_span: Duration,
}

/// Everything the muxer needs before the first frame.
///
/// `negotiated_interval_us` is an `Option` because the negotiation may not have produced a
/// number at all: [`interval_micros`] refuses to invent one for a stepwise range or for a
/// `v4l2_frmivaltypes` value this build cannot read (AGENTS rule 6). A recording still
/// happens — see [`write::PROVISIONAL_INTERVAL_US`] — and the summary says the interval was
/// provisional.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AviParams {
    /// Frame width, as negotiated with the device.
    pub width: u32,
    /// Frame height, as negotiated with the device.
    pub height: u32,
    /// The negotiated frame interval in microseconds, when the negotiation named one.
    pub negotiated_interval_us: Option<u32>,
    /// What the recording is allowed to cost.
    pub caps: RecordingCaps,
}

/// What happened to a frame handed to the muxer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameOutcome {
    /// It is on the sink.
    Written,
    /// A cap was reached, so it was **not** written and neither will anything after it.
    Refused(CapReached),
}

closed_vocabulary! {
    /// Which bound ended the recording.
    ///
    /// `ALL` is generated from this definition, and the tests walk it: a cap that no
    /// recording can reach is a bound that is not a bound, and a hand-written list of the
    /// three would let one quietly stop being enforced (rubric rule 6).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum CapReached {
        /// [`RecordingCaps::max_bytes`].
        Size,
        /// [`RecordingCaps::max_frames`].
        Frames,
        /// [`RecordingCaps::max_span`].
        Span,
    }
}

closed_vocabulary! {
    /// Where the frame interval in the finished header came from.
    ///
    /// The distinction D7's CFR carve-out exists to preserve, carried out of the muxer so
    /// the caller never has to guess whether the number it is reading was observed.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum IntervalSource {
        /// The mean of the delivered frame timestamps — the close-time rewrite happened.
        Measured,
        /// What the camera was asked for. Fewer than two frames arrived, or their
        /// timestamps described no usable span, so nothing was measured.
        Negotiated,
        /// Neither: the negotiation named no interval and none was measured, so the header
        /// carries [`write::PROVISIONAL_INTERVAL_US`].
        Provisional,
    }
}

/// What a finished recording turned out to be.
///
/// Every field is a *measurement* of the file that was written, not a restatement of what
/// was asked for — which is the point of returning it at all. `declared_interval_us` is the
/// number now in both of the header's rate fields, and `interval_source` says whether it is
/// a finding or a placeholder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AviSummary {
    /// How many frame chunks are in `movi`.
    pub frames_written: u32,
    /// The size of the finished file, `idx1` included.
    pub bytes_written: u64,
    /// The frame interval now in `avih.dwMicroSecPerFrame` and `strh.dwScale`/`dwRate`.
    pub declared_interval_us: u32,
    /// Whether that interval was measured, negotiated or provisional.
    pub interval_source: IntervalSource,
    /// Frames the driver's `sequence` numbers say never arrived.
    ///
    /// A `u64` because the gaps accumulate: each one may be as large as a `u32`, and a
    /// recording may hold as many gaps as it holds frames.
    pub dropped_frames: u64,
    /// The last written frame's timestamp minus the first's, when that is a duration.
    ///
    /// `None` when fewer than two frames were written, and when the driver's clock ran
    /// backwards across the take — a negative span is not a duration, and reporting it as
    /// zero would claim a measurement nobody made.
    pub span_us: Option<u64>,
    /// The bound that ended the recording, if one did.
    pub cap_reached: Option<CapReached>,
}

/// A frame interval in microseconds, when the interval names one.
///
/// Total over the vocabulary, and it **refuses to guess** (AGENTS rule 6). A
/// [`FrameInterval::Stepwise`] range describes what a device *could* do, not what it agreed
/// to do, and picking its minimum would put a number in a CFR header that no frame was
/// delivered at; a [`FrameInterval::Unknown`] carries a `v4l2_frmivaltypes` discriminant
/// this build cannot interpret, and interpreting it anyway is the defect rule 6 is about.
/// Both are `None`, and a `None` reaching [`AviParams::negotiated_interval_us`] is honestly
/// reported as [`IntervalSource::Provisional`] rather than silently rounded to 30 fps.
///
/// A degenerate discrete interval — a zero numerator or denominator, or one that works out
/// to zero microseconds or to more than a `u32` holds — is `None` for the same reason.
#[must_use]
pub fn interval_micros(interval: &FrameInterval) -> Option<u32> {
    match *interval {
        FrameInterval::Discrete {
            numerator,
            denominator,
        } => {
            if numerator == 0 || denominator == 0 {
                return None;
            }
            let micros = u64::from(numerator)
                .checked_mul(1_000_000)?
                .checked_div(u64::from(denominator))?;
            u32::try_from(micros).ok().filter(|value| *value > 0)
        }
        // Named rather than `_`, so a new arm in the kernel's vocabulary stops the build
        // here instead of quietly becoming "no interval".
        FrameInterval::Stepwise { .. } | FrameInterval::Unknown { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_discrete_interval_becomes_microseconds_and_a_range_becomes_nothing() {
        // AGENTS rule 6: the unknown is represented, never guessed. A stepwise range that
        // answered with its own minimum would put a rate in the CFR header that no frame
        // was ever delivered at, which is precisely the lie D7's rewrite exists to stop.
        assert_eq!(
            interval_micros(&FrameInterval::Discrete {
                numerator: 1,
                denominator: 30,
            }),
            Some(33_333)
        );
        assert_eq!(
            interval_micros(&FrameInterval::Discrete {
                numerator: 1001,
                denominator: 30_000,
            }),
            Some(33_366),
            "29.97 fps truncates rather than rounding, and stays a legal interval"
        );
        assert_eq!(
            interval_micros(&FrameInterval::Stepwise {
                min_numerator: 1,
                min_denominator: 30,
                max_numerator: 1,
                max_denominator: 5,
            }),
            None
        );
        assert_eq!(interval_micros(&FrameInterval::Unknown { raw: 9 }), None);
    }

    #[test]
    fn a_degenerate_interval_is_refused_rather_than_turned_into_a_zero_rate() {
        // `dwRate` of zero is a file `read_stream` refuses, and a zero-microsecond frame
        // interval is meaningless to every player. Both halves of the fraction, plus the
        // interval so short it rounds to nothing and the one too long for the field.
        assert_eq!(
            interval_micros(&FrameInterval::Discrete {
                numerator: 0,
                denominator: 30,
            }),
            None
        );
        assert_eq!(
            interval_micros(&FrameInterval::Discrete {
                numerator: 1,
                denominator: 0,
            }),
            None
        );
        assert_eq!(
            interval_micros(&FrameInterval::Discrete {
                numerator: 1,
                denominator: 2_000_000,
            }),
            None,
            "half a microsecond truncates to zero, which is not an interval"
        );
        assert_eq!(
            interval_micros(&FrameInterval::Discrete {
                numerator: 5000,
                denominator: 1,
            }),
            None,
            "5000 seconds per frame does not fit dwMicroSecPerFrame"
        );
    }
}
