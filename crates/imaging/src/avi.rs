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
//! [`RecordingSummary`] carries the same number *and* [`IntervalSource`], which is not
//! decoration. The notes are blunt about what is at stake — "for a transition they *are*
//! the measurement, because 'did this take 200 ms or 2 s' is the question being asked" —
//! so a caller must be able to tell a rate that was **measured** from one that is merely
//! what the camera was **asked** for. A mean needs two points; a one-frame recording has
//! measured nothing, and says so rather than dressing the negotiated interval up as a
//! finding. [`RecordingSummary::dropped_frames`] belongs to the same obligation: a gap in
//! the driver's `sequence` is a frame that never arrived, and reporting it is what stops a
//! dropped frame from reading as a slow transition.
//!
//! **This was the container that could do the rewrite, and for two phases it was the only
//! one.** A Y4M header is variable-width text written before the first frame, so [`crate::y4m`]
//! declared the negotiated interval and never reported [`IntervalSource::Measured`]. P6d's
//! oracle rung measured what note **N106** said would end that — a zero-padded `F` denominator
//! is read as the rate it names by `ffprobe` and by `mpv` (evidence **E17**) — so that
//! container patches its header at close too, and the decision both of them make now lives once
//! in `crate::video::declared_interval`.
//!
//! ## What this module owns, and what moved out of it at P6b
//!
//! P6a wrote [`RecordingCaps`], [`FrameOutcome`], [`CapReached`], [`IntervalSource`] and
//! the finished-recording summary here, when AVI was the only container and nothing outside
//! this crate could see them. P6b added a second container and a wire surface, so each of
//! them moved to the one home its scope now demands — the wire-bearing three to
//! [`schema::video`], the two container-shared ones to [`crate::video`] — and every one is
//! re-exported below so P6a's spellings still resolve. The arguments moved with the types:
//! **why a recording is bounded by values rather than by constants**, and **why a cap is an
//! outcome while a malformed frame is an error**, are both stated in [`crate::video`]'s
//! header, because they are now two containers' law rather than this one's. The summary
//! type is [`RecordingSummary`] rather than `AviSummary` for the same reason: every claim it
//! makes is one a Y4M take makes too.
//!
//! What is left here is what is genuinely AVI's: the muxer, the reader that distrusts it,
//! the parameters that open a recording, and [`interval_micros`].

use schema::camera::FrameInterval;

pub mod read;
pub mod write;

// The vocabulary this container shares with the other one, re-exported so that P6a's
// spellings — `imaging::avi::RecordingCaps`, `imaging::avi::CapReached` — still resolve, and
// so that this module's own documentation can go on linking the types it argues about. A
// re-export rather than a second definition: `schema::video` and `crate::video` are the
// homes, and AGENTS' "one home per law" is the reason the definitions are not here any more.
pub use schema::video::{CapReached, IntervalSource, RecordingSummary};

pub use crate::video::{FrameOutcome, RecordingCaps};

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

/// A frame interval in microseconds, when the interval names one.
///
/// Total over the vocabulary, and it **refuses to guess** (AGENTS rule 6). A
/// [`FrameInterval::Stepwise`] range describes what a device *could* do, not what it agreed
/// to do, and picking its minimum would put a number in a CFR header that no frame was
/// delivered at; a [`FrameInterval::Unknown`] carries a `v4l2_frmivaltypes` discriminant
/// this build cannot interpret, and interpreting it anyway is the defect rule 6 is about;
/// a [`FrameInterval::Unstated`] is a device that cleared `V4L2_CAP_TIMEPERFRAME`, so
/// there is nothing to interpret at all. All three are `None`, and a `None` reaching
/// [`AviParams::negotiated_interval_us`] is honestly reported as
/// [`IntervalSource::Provisional`] rather than silently rounded to 30 fps.
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
        FrameInterval::Stepwise { .. }
        | FrameInterval::Unknown { .. }
        | FrameInterval::Unstated => None,
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
