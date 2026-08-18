//! What a stream's own frames say about how it was delivered (design D16; FR-W4).
//!
//! A pure core over two numbers every frame already carries — the driver's `sequence` and
//! its `timestamp_us` — answering [`schema::video::StreamStats`]: frames delivered and
//! dropped, how many *runs* of drops there were, and what the intervals between frames
//! looked like. Integer arithmetic end to end, no clock, no allocation past a stated bound,
//! and no judgement: "healthy" is the consumer's word, not this module's.
//!
//! ## One home, extended rather than founded
//!
//! Sequence-gap counting existed here before D16 did — twice, letter for letter, once in
//! each muxer's `count_sequence` — and the mean frame interval had its one home next door in
//! `video::declared_interval` since P6d. This module is where the first of those
//! becomes one copy and the second gains the rest of its distribution; both muxers push
//! their frames through an [`Accumulator`] and read `frames_dropped` back out of it, so
//! `RecordingSummary::dropped_frames` and `RecordReport::stats` are two readings of one
//! count rather than two counts that agree today (design §2.10).
//!
//! ## The bound is an existing bound
//!
//! Intervals are retained exactly up to [`schema::limits::MAX_RECORDING_FRAMES`] — the cap
//! that already governs every take, 16 384 of them, about 128 KiB. Past it the accumulator
//! keeps the streaming moments (count, sum, min, max) over *every* interval and the order
//! statistics over the retained prefix, and the answer carries both counts so a consumer can
//! see which of its numbers cover what. The truncation is on the answer, never silent
//! (AGENTS rule 3's spirit, one layer down).
//!
//! ## What it deliberately is not
//!
//! Not a live-stream verb, not a trending store, not a threshold. And not a wall clock: the
//! `wall_clock_skew_us` field this module always leaves `None` is filled by the engine,
//! because a pure core that read a clock would be the one part of `webcam-handler-imaging`
//! that could not be tested by handing it a vector.

use schema::limits::MAX_RECORDING_FRAMES;
use schema::video::{IntervalStats, StreamStats};

/// Accumulates one stream's delivery record, frame by frame.
///
/// Push every frame as it arrives; read [`Accumulator::stats`] when the stream ends. Frames
/// must be pushed in arrival order — the accumulator is deliberately incapable of sorting,
/// since the order frames arrived in is the fact being measured.
#[derive(Debug, Clone)]
pub struct Accumulator {
    /// The previous frame's `(sequence, timestamp_us)`, once there has been one.
    previous: Option<(u32, i64)>,
    frames_delivered: u64,
    frames_dropped: u64,
    gap_events: u32,
    sequence_resets: u32,
    clock_reversals: u32,
    /// Every interval observed, as moments: the retained prefix is `retained` below.
    observed: u64,
    sum_us: u128,
    min_us: Option<u64>,
    max_us: Option<u64>,
    /// The retained prefix, for the order statistics.
    retained: Vec<u64>,
}

impl Default for Accumulator {
    fn default() -> Self {
        Self::new()
    }
}

impl Accumulator {
    /// How many intervals are retained exactly, after which the order statistics describe a
    /// prefix and the answer says so.
    ///
    /// The recording cap rather than a number of this module's own: a stream longer than the
    /// cap is one this tool has already declined to record whole, so a second bound here
    /// would be a second opinion about the same length.
    pub const RETAINED_INTERVALS: u32 = MAX_RECORDING_FRAMES;

    /// An accumulator that has seen nothing.
    #[must_use]
    pub fn new() -> Self {
        Accumulator {
            previous: None,
            frames_delivered: 0,
            frames_dropped: 0,
            gap_events: 0,
            sequence_resets: 0,
            clock_reversals: 0,
            observed: 0,
            sum_us: 0,
            min_us: None,
            max_us: None,
            retained: Vec::new(),
        }
    }

    /// Record one delivered frame.
    ///
    /// Three facts come out of the pair, and each is kept apart from the others because they
    /// are three different things a link can do: the sequence advanced by more than one
    /// (frames were dropped), the sequence did not advance (a driver restarted its counter,
    /// or — after four and a half years at 30 fps — a `u32` wrapped), and the timestamp did
    /// not advance (the host's clock went backwards, which is a finding rather than an
    /// interval).
    pub fn push(&mut self, sequence: u32, timestamp_us: i64) {
        self.frames_delivered = self.frames_delivered.saturating_add(1);

        let Some((previous_sequence, previous_timestamp)) = self.previous else {
            self.previous = Some((sequence, timestamp_us));
            return;
        };

        // Only a forward gap is a drop. `checked_sub` rather than `wrapping_sub` for the
        // reason both muxers stated before this module existed: a wrapping subtraction turns
        // a restarted counter into four billion dropped frames.
        match sequence.checked_sub(previous_sequence) {
            Some(0) | None => self.sequence_resets = self.sequence_resets.saturating_add(1),
            Some(1) => {}
            Some(gap) => {
                self.frames_dropped = self
                    .frames_dropped
                    .saturating_add(u64::from(gap.saturating_sub(1)));
                self.gap_events = self.gap_events.saturating_add(1);
            }
        }

        match timestamp_us
            .checked_sub(previous_timestamp)
            .and_then(|delta| u64::try_from(delta).ok())
            .filter(|delta| *delta > 0)
        {
            Some(interval) => self.observe(interval),
            None => self.clock_reversals = self.clock_reversals.saturating_add(1),
        }

        self.previous = Some((sequence, timestamp_us));
    }

    /// Fold one interval into the moments, and into the retained prefix while there is room.
    fn observe(&mut self, interval_us: u64) {
        self.observed = self.observed.saturating_add(1);
        self.sum_us = self.sum_us.saturating_add(u128::from(interval_us));
        self.min_us = Some(self.min_us.map_or(interval_us, |min| min.min(interval_us)));
        self.max_us = Some(self.max_us.map_or(interval_us, |max| max.max(interval_us)));
        if u32::try_from(self.retained.len()).is_ok_and(|kept| kept < Self::RETAINED_INTERVALS) {
            self.retained.push(interval_us);
        }
    }

    /// What this stream delivered.
    ///
    /// `wall_clock_skew_us` is always `None` here — see the module doc. A caller that has a
    /// wall clock fills it on the value.
    #[must_use]
    pub fn stats(&self) -> StreamStats {
        StreamStats {
            frames_delivered: self.frames_delivered,
            frames_dropped: self.frames_dropped,
            gap_events: self.gap_events,
            sequence_resets: self.sequence_resets,
            clock_reversals: self.clock_reversals,
            intervals: self.interval_stats(),
            wall_clock_skew_us: None,
        }
    }

    /// How many frames the driver's sequence numbers say never arrived.
    ///
    /// The number both containers report as `RecordingSummary::dropped_frames`, read off the
    /// one accumulator that counted it.
    #[must_use]
    pub const fn frames_dropped(&self) -> u64 {
        self.frames_dropped
    }

    /// The interval distribution, or `None` when the stream spanned no interval at all.
    fn interval_stats(&self) -> Option<IntervalStats> {
        let (Some(min_us), Some(max_us)) = (self.min_us, self.max_us) else {
            return None;
        };
        if self.observed == 0 {
            return None;
        }

        // The mean is over every interval observed, so it is the sum — which is a `u128`
        // precisely so a long take cannot overflow it — divided by the count. `u64::try_from`
        // rather than `as`: the quotient of two observed quantities is bounded by `max_us`,
        // and stating that with a conversion that can fail is cheaper than arguing it.
        let mean_us = u64::try_from(self.sum_us / u128::from(self.observed)).unwrap_or(max_us);

        let mut sorted = self.retained.clone();
        sorted.sort_unstable();
        let retained = u32::try_from(sorted.len()).unwrap_or(u32::MAX);

        Some(IntervalStats {
            observed: self.observed,
            retained,
            mean_us,
            min_us,
            max_us,
            p50_us: percentile(&sorted, 50).unwrap_or(mean_us),
            p99_us: percentile(&sorted, 99).unwrap_or(mean_us),
            jitter_us: mean_absolute_deviation(&sorted, mean_us),
        })
    }
}

/// The `p`th percentile of a sorted slice, by the nearest-rank method.
///
/// Nearest-rank rather than an interpolating definition, and that is a decision: every value
/// this answers with is an interval the stream really produced, so a `p99` of 41 ms is a
/// frame that took 41 ms rather than a number between two frames that took 33 and 50. An
/// unattended consumer comparing a percentile against a threshold is comparing observations.
///
/// `None` for an empty slice — a percentile of nothing is not zero.
fn percentile(sorted: &[u64], p: u32) -> Option<u64> {
    if sorted.is_empty() {
        return None;
    }
    let len = u64::try_from(sorted.len()).unwrap_or(u64::MAX);
    // Ceiling of `p/100 * len`, in integers, then one-based rank to zero-based index. The
    // `max(1)` is the p0 corner: rank zero is not a rank.
    let rank = u64::from(p).saturating_mul(len).div_ceil(100).max(1);
    let index = usize::try_from(rank - 1).unwrap_or(sorted.len() - 1);
    sorted.get(index.min(sorted.len() - 1)).copied()
}

/// Mean absolute deviation from `mean_us`, over the retained intervals.
///
/// Zero for an empty slice, which is the one place in this module a zero is not a claim: no
/// intervals means no deviation to report, and the caller already knows the count.
fn mean_absolute_deviation(sorted: &[u64], mean_us: u64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let total: u128 = sorted
        .iter()
        .map(|interval| u128::from(interval.abs_diff(mean_us)))
        .sum();
    let len = u128::try_from(sorted.len()).unwrap_or(u128::MAX);
    u64::try_from(total / len).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Push a run of frames at a constant interval, starting at sequence 0.
    fn steady(frames: u32, interval_us: i64) -> Accumulator {
        let mut accumulator = Accumulator::new();
        for index in 0..frames {
            accumulator.push(index, i64::from(index) * interval_us);
        }
        accumulator
    }

    #[test]
    fn an_unbroken_run_drops_nothing_and_says_so() {
        // The inverse arm, and it is the one that matters: a gap counter that answered "one"
        // for a healthy stream would make every take look like a failing link.
        let stats = steady(30, 33_333).stats();
        assert_eq!(stats.frames_delivered, 30);
        assert_eq!(stats.frames_dropped, 0);
        assert_eq!(stats.gap_events, 0);
        assert_eq!(stats.sequence_resets, 0);
        assert_eq!(stats.clock_reversals, 0);

        let intervals = stats.intervals.expect("thirty frames span intervals");
        assert_eq!(intervals.observed, 29);
        assert_eq!(intervals.mean_us, 33_333);
        assert_eq!(intervals.min_us, 33_333);
        assert_eq!(intervals.max_us, 33_333);
        assert_eq!(intervals.p50_us, 33_333);
        assert_eq!(intervals.p99_us, 33_333);
        assert_eq!(intervals.jitter_us, 0);
        assert!(!intervals.is_truncated());
    }

    #[test]
    fn a_gap_in_the_sequence_is_counted_as_the_frames_it_hides() {
        // Sequences 0, 1, 5 — three frames delivered, three missing between the second and
        // the third, and one run of drops rather than three.
        let mut accumulator = Accumulator::new();
        accumulator.push(0, 0);
        accumulator.push(1, 33_333);
        accumulator.push(5, 166_665);

        let stats = accumulator.stats();
        assert_eq!(stats.frames_delivered, 3);
        assert_eq!(stats.frames_dropped, 3);
        assert_eq!(stats.gap_events, 1);
    }

    #[test]
    fn one_long_gap_and_many_short_ones_are_told_apart() {
        // The reason `gap_events` sits beside `frames_dropped`: a stall and a lossy link
        // produce the same drop count and want different repairs.
        let mut stall = Accumulator::new();
        stall.push(0, 0);
        stall.push(7, 233_331);

        let mut lossy = Accumulator::new();
        for index in 0..7_u32 {
            lossy.push(index * 2, i64::from(index) * 66_666);
        }

        assert_eq!(stall.stats().frames_dropped, 6);
        assert_eq!(stall.stats().gap_events, 1);
        assert_eq!(lossy.stats().frames_dropped, 6);
        assert_eq!(lossy.stats().gap_events, 6);
    }

    #[test]
    fn a_repeated_or_backwards_sequence_is_a_reset_and_not_four_billion_drops() {
        // The trap a wrapping subtraction sets, kept shut. Both muxers have said this in
        // their own words since P6b; this is where the sentence now lives.
        let mut accumulator = Accumulator::new();
        accumulator.push(9, 0);
        accumulator.push(9, 33_333);
        accumulator.push(3, 66_666);

        let stats = accumulator.stats();
        assert_eq!(stats.frames_dropped, 0);
        assert_eq!(stats.gap_events, 0);
        assert_eq!(stats.sequence_resets, 2);
    }

    #[test]
    fn a_clock_that_ran_backwards_produces_no_interval_and_is_counted() {
        let mut accumulator = Accumulator::new();
        accumulator.push(0, 1_000);
        accumulator.push(1, 500);
        accumulator.push(2, 33_500);

        let stats = accumulator.stats();
        assert_eq!(stats.clock_reversals, 1);
        let intervals = stats.intervals.expect("one interval was a duration");
        assert_eq!(intervals.observed, 1);
        assert_eq!(intervals.mean_us, 33_000);
    }

    #[test]
    fn fewer_than_two_frames_span_no_intervals_at_all() {
        assert!(Accumulator::new().stats().intervals.is_none());
        let mut one = Accumulator::new();
        one.push(0, 0);
        let stats = one.stats();
        assert_eq!(stats.frames_delivered, 1);
        assert!(
            stats.intervals.is_none(),
            "one frame spans nothing, and zeros would claim a measurement nobody made"
        );
    }

    #[test]
    fn the_percentiles_are_the_intervals_the_stream_really_produced() {
        // A hundred intervals of 1..=100 µs: the median is the 50th value and the 99th
        // percentile is the 99th, by nearest rank. Written against a distribution whose
        // answers can be read off by eye, because a percentile implementation that is one
        // index out still looks plausible on real data.
        let mut accumulator = Accumulator::new();
        let mut timestamp = 0_i64;
        accumulator.push(0, timestamp);
        for index in 1..=100_u32 {
            timestamp += i64::from(index);
            accumulator.push(index, timestamp);
        }

        let intervals = accumulator
            .stats()
            .intervals
            .expect("a hundred intervals were observed");
        assert_eq!(intervals.observed, 100);
        assert_eq!(intervals.min_us, 1);
        assert_eq!(intervals.max_us, 100);
        assert_eq!(intervals.mean_us, 50, "5050 / 100 truncates to 50");
        assert_eq!(intervals.p50_us, 50);
        assert_eq!(intervals.p99_us, 99);
        // Mean absolute deviation of 1..=100 about 50: 1300 + 1225 over 100 = 25.
        assert_eq!(intervals.jitter_us, 25);
    }

    #[test]
    fn a_percentile_of_one_interval_is_that_interval() {
        let mut accumulator = Accumulator::new();
        accumulator.push(0, 0);
        accumulator.push(1, 40_000);
        let intervals = accumulator.stats().intervals.expect("one interval");
        assert_eq!(intervals.p50_us, 40_000);
        assert_eq!(intervals.p99_us, 40_000);
        assert_eq!(intervals.jitter_us, 0);
    }

    #[test]
    fn past_the_retained_bound_the_moments_still_cover_everything_and_the_answer_says_so() {
        // The bound, driven. Every interval is 1 µs except the last, which is far outside the
        // retained prefix — so a `max` that came from the prefix would miss it, and a
        // `retained` that claimed the whole stream would hide that the percentiles did.
        let mut accumulator = Accumulator::new();
        let frames = Accumulator::RETAINED_INTERVALS + 10;
        let mut timestamp = 0_i64;
        accumulator.push(0, timestamp);
        for index in 1..=frames {
            timestamp += if index == frames { 900_000 } else { 1 };
            accumulator.push(index, timestamp);
        }

        let intervals = accumulator.stats().intervals.expect("many intervals");
        assert_eq!(intervals.observed, u64::from(frames));
        assert_eq!(intervals.retained, Accumulator::RETAINED_INTERVALS);
        assert!(
            intervals.is_truncated(),
            "the answer must state the truncation"
        );
        assert_eq!(
            intervals.max_us, 900_000,
            "the moments cover every interval, retained or not"
        );
        assert_eq!(
            intervals.p99_us, 1,
            "the order statistics cover the retained prefix, and say so through `retained`"
        );
    }

    #[test]
    fn the_accumulator_and_the_header_agree_about_the_mean_over_one_take() {
        // One home, two readers, reconciled (design D16). `video::declared_interval` decides
        // what a *file's header* says and has since P6d; this module decides what a *stream's
        // report* says. Over a take whose clock never went backwards they are the same
        // arithmetic on the same frames, and a test that did not say so would leave two
        // numbers free to drift apart in a document that shows both.
        for interval_us in [33_333_i64, 40_000, 16_667] {
            for frames in [2_u32, 3, 30, 601] {
                let accumulator = steady(frames, interval_us);
                let mean = accumulator
                    .stats()
                    .intervals
                    .expect("two or more frames span an interval")
                    .mean_us;

                let (declared, source, span) = crate::video::declared_interval(
                    Some(0),
                    i64::from(frames - 1) * interval_us,
                    frames,
                    None,
                );
                assert_eq!(source, schema::video::IntervalSource::Measured);
                assert_eq!(span, Some((frames - 1) as u64 * interval_us as u64));
                assert_eq!(
                    u64::from(declared),
                    mean,
                    "{frames} frames at {interval_us} µs: the header and the report disagree"
                );
            }
        }
    }

    #[test]
    fn nothing_here_allocates_past_the_bound_it_states() {
        let mut accumulator = Accumulator::new();
        for index in 0..(Accumulator::RETAINED_INTERVALS + 1_000) {
            accumulator.push(index, i64::from(index));
        }
        assert_eq!(
            accumulator.retained.len(),
            Accumulator::RETAINED_INTERVALS as usize,
            "a bounded core that grew without bound would be the defect the bound names"
        );
    }
}
