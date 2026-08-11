//! The settle policy, as a pure state machine over frame arrivals (design D5, PF:11).
//!
//! The frames immediately after `STREAMON` are dark and miscolored: auto-exposure and
//! auto-white-balance have not converged yet, and on both seed cameras ten frames was
//! enough \[PF:9, PF:11\]. So a photo waits — and waiting is where flaky tests come from.
//!
//! Nothing here reads a clock or sleeps. The policy is a fold over `(frame arrived, what
//! time is it)` and the caller supplies both, which turns "the deadline expired between
//! these two frames" from a race into an argument. [`SteppedClock`] is the double the
//! tests drive, [`FrozenClock`] is the one for a test that is not about the deadline at
//! all, and [`MonotonicClock`] is what the capture loop uses.
//!
//! Two rules are load-bearing and easy to get backwards:
//!
//! - The deadline is checked **before** convergence, so a policy that would have been
//!   satisfied by the very frame that arrived one millisecond too late reports
//!   [`SettleDecision::Timeout`] rather than [`SettleDecision::Take`]. A photo taken
//!   after the caller's deadline is a photo the caller did not ask for.
//! - A stream that delivers *nothing* must still time out. Frames are the only thing
//!   that drives [`SettlePolicyState::on_frame`], so the capture loop calls
//!   [`SettlePolicyState::on_idle`] whenever its own frame wait expires — the deadline
//!   belongs to the policy, not to `DQBUF`.

use std::cell::Cell;
#[cfg(test)]
use std::time::Duration;
use std::time::Instant;

use schema::capture::{SettlePolicy, SettleSpec};
use schema::{Error, limits};

/// Milliseconds on a clock's own timeline.
///
/// Only differences are meaningful: [`MonotonicClock`] counts from the moment it was
/// created, and comparing two clocks' readings is a bug.
pub type Millis = u64;

/// A source of monotonic time.
///
/// The one seam in this module, and it exists so the state machine has none: everything
/// below takes a [`Millis`] parameter, and this trait is how the *shell* gets one.
pub trait Clock {
    /// Milliseconds since this clock's origin.
    fn now_ms(&self) -> Millis;
}

/// The real clock.
///
/// [`Instant`], not the wall clock: an NTP step or a suspend/resume mid-settle must not
/// be able to make a deadline expire early, or never.
#[derive(Debug, Clone)]
pub struct MonotonicClock {
    origin: Instant,
}

impl MonotonicClock {
    /// Start a clock at the current instant.
    #[must_use]
    pub fn new() -> Self {
        MonotonicClock {
            origin: Instant::now(),
        }
    }
}

impl Default for MonotonicClock {
    fn default() -> Self {
        MonotonicClock::new()
    }
}

impl Clock for MonotonicClock {
    fn now_ms(&self) -> Millis {
        // 2^64 milliseconds is half a billion years, so the saturation below is
        // unreachable in practice. It is written this way rather than with `as` because
        // the *direction* matters if it ever happens: saturating high reads as "past
        // every deadline", which fails a settle safely instead of restarting it.
        u64::try_from(self.origin.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

/// A clock the caller moves by hand.
///
/// The scriptable double for the [`Clock`] seam. Not `Sync` — a stepped clock shared
/// across threads is a race dressed as a fixture, and the settle tests are
/// single-threaded by construction.
#[derive(Debug, Default)]
pub struct SteppedClock {
    now: Cell<Millis>,
}

impl SteppedClock {
    /// A clock reading `start`.
    #[must_use]
    pub fn new(start: Millis) -> Self {
        SteppedClock {
            now: Cell::new(start),
        }
    }

    /// Move the clock forward. Saturating, so a test cannot silently wrap time.
    pub fn advance(&self, by: Millis) {
        self.now.set(self.now.get().saturating_add(by));
    }
}

impl Clock for SteppedClock {
    fn now_ms(&self) -> Millis {
        self.now.get()
    }
}

/// A clock that cannot move, for a test whose subject is not the deadline.
///
/// [`SteppedClock`] is the double for a test *about* the settle deadline: the test moves it,
/// and the assertion is about what the policy decided on either side of the move. This is
/// the double for the other kind of test — one that scripts a device and asserts what the
/// **device** answered. Its reading is a constant, so the deadline is never reached, so
/// [`SettleDecision::Timeout`] is not an outcome such a test can be handed: the scripted
/// answer is the only reachable one, however loaded the machine running it.
///
/// That is the whole of what this type buys, and it is worth stating as a claim rather than
/// as a convenience: **a deadline that cannot expire makes "the device answered" the only
/// outcome the test can observe, so the assertion is about the device and not about the
/// machine.** Note N60 is the run that priced the alternative — a sweep test scripting a
/// vanished device, a real clock, eleven frames that arrived under contention *after* the
/// five-second deadline had passed, and a perfectly correct [`Error::SettleTimeout`] handed
/// to an assertion expecting `DeviceGone`.
///
/// **Not [`SteppedClock`] with the [`Cell`] swapped for an atomic.** That would be a clock
/// two threads can *move*, which is exactly what note N45's argument forbids — "a stepped
/// clock shared across threads is a race dressed as a fixture" — wearing a different type.
/// This holds no state at all: it is `Sync` by construction rather than by synchronisation,
/// because there is nothing to share and therefore nothing to race. A frozen clock crosses a
/// thread boundary without touching that argument.
///
/// **Pair it with a frame-counted settle.** [`SettleSpec::SkipFrames`] converges by counting
/// frames and does not consult the clock to do it. [`SettleSpec::SettleFor`] converges by
/// *elapsed* time, and no time elapses here — such a settle would end at
/// [`limits::MAX_SETTLE_ROUNDS`] rather than at its spec. A test about a duration wants
/// [`SteppedClock`], which is what that type is for.
#[derive(Debug, Default, Clone, Copy)]
pub struct FrozenClock;

impl FrozenClock {
    /// What it reads, and goes on reading.
    ///
    /// *Which* number this is means nothing — only differences are meaningful (see
    /// [`Millis`]) and every difference this clock can produce is zero. That it is **not
    /// zero** is a decision, and E7's is the reason: `MonotonicClock::now_ms` replaced by
    /// `0` "left the whole workspace green", so zero is precisely the reading of a clock
    /// that has been mutated away. This file is inside the mutation floor's scope
    /// (`.cargo/mutants.toml`), and a fixture whose reading no test could tell apart from
    /// that defect would arrive as a survivor with no argument behind it. One is a number
    /// the assertion below can name.
    pub const READING: Millis = 1;
}

impl Clock for FrozenClock {
    fn now_ms(&self) -> Millis {
        FrozenClock::READING
    }
}

/// What the policy says about the frame that just arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettleDecision {
    /// Discard it and keep streaming.
    Skip,
    /// This is the frame. Terminal.
    Take,
    /// The deadline passed. Terminal, and a
    /// [`schema::Error::SettleTimeout`] for the caller to raise — never a
    /// "good enough" frame, because a caller who set a deadline meant it.
    Timeout,
}

impl SettleDecision {
    /// Whether no further frames can change the answer.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, SettleDecision::Take | SettleDecision::Timeout)
    }
}

/// A settle policy in progress.
///
/// Construct one when the stream starts, feed it every frame, and stop at the first
/// terminal decision. Terminal decisions are sticky: a caller that loops one extra time
/// gets the same answer rather than a second [`SettleDecision::Take`].
#[derive(Debug, Clone)]
pub struct SettlePolicyState {
    policy: SettlePolicy,
    started_ms: Millis,
    frames_seen: u32,
    outcome: Option<SettleDecision>,
}

impl SettlePolicyState {
    /// Begin a settle at `started_ms` on the caller's clock.
    #[must_use]
    pub fn new(policy: SettlePolicy, started_ms: Millis) -> Self {
        SettlePolicyState {
            policy,
            started_ms,
            frames_seen: 0,
            outcome: None,
        }
    }

    /// Account for a frame that arrived at `now`, and say what to do with it.
    pub fn on_frame(&mut self, now: Millis) -> SettleDecision {
        if let Some(outcome) = self.outcome {
            return outcome;
        }
        self.frames_seen = self.frames_seen.saturating_add(1);

        if self.expired(now) {
            return self.finish(SettleDecision::Timeout);
        }

        let converged = match self.policy.spec {
            // `frames_seen` already counts this frame, so the `frames`-th frame is still
            // skipped and the one after it is the photo.
            SettleSpec::SkipFrames { frames } => self.frames_seen > frames,
            SettleSpec::SettleFor { millis } => self.waited_ms(now) >= millis,
        };
        if converged {
            self.finish(SettleDecision::Take)
        } else {
            SettleDecision::Skip
        }
    }

    /// Check the deadline without a frame.
    ///
    /// `None` means "still waiting". The capture loop calls this whenever its own frame
    /// wait expires, because a camera that delivers nothing at all never calls
    /// [`Self::on_frame`] and must still time out — E3's distinction between "the camera
    /// cannot" and "the camera did not" only survives if the second one is reachable.
    pub fn on_idle(&mut self, now: Millis) -> Option<SettleDecision> {
        if let Some(outcome) = self.outcome {
            return Some(outcome);
        }
        if self.expired(now) {
            Some(self.finish(SettleDecision::Timeout))
        } else {
            None
        }
    }

    /// How many frames have been offered so far.
    #[must_use]
    pub fn frames_seen(&self) -> u32 {
        self.frames_seen
    }

    /// How long this settle has been running at `now`.
    #[must_use]
    pub fn waited_ms(&self, now: Millis) -> u64 {
        now.saturating_sub(self.started_ms)
    }

    /// How long the caller may block waiting for the next frame.
    ///
    /// The smaller of what is left of the deadline and
    /// [`limits::FRAME_DEADLINE_MS`]: the second bound is what guarantees
    /// [`Self::on_idle`] gets a turn even when the deadline is generous, so a wedged
    /// driver cannot hold the actor thread past its own deadline.
    #[must_use]
    pub fn next_wait_ms(&self, now: Millis) -> u64 {
        let remaining = self.policy.deadline_ms.saturating_sub(self.waited_ms(now));
        remaining.min(limits::FRAME_DEADLINE_MS)
    }

    /// The typed timeout for this settle, populated with what actually happened.
    ///
    /// `frames_seen == 0` and `frames_seen == 9` are different diagnoses — a dead stream
    /// versus a policy that wanted one more frame — and the variant carries both so the
    /// caller does not have to guess.
    #[must_use]
    pub fn timeout_error(&self, now: Millis) -> Error {
        Error::SettleTimeout {
            waited_ms: self.waited_ms(now),
            frames_seen: self.frames_seen,
        }
    }

    fn expired(&self, now: Millis) -> bool {
        self.waited_ms(now) >= self.policy.deadline_ms
    }

    fn finish(&mut self, outcome: SettleDecision) -> SettleDecision {
        self.outcome = Some(outcome);
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schema::ErrorKind;

    fn skip_frames(frames: u32, deadline_ms: u64) -> SettlePolicy {
        SettlePolicy {
            spec: SettleSpec::SkipFrames { frames },
            deadline_ms,
        }
    }

    fn settle_for(millis: u64, deadline_ms: u64) -> SettlePolicy {
        SettlePolicy {
            spec: SettleSpec::SettleFor { millis },
            deadline_ms,
        }
    }

    #[test]
    fn skip_frames_converges_on_the_frame_after_the_last_skipped_one() {
        let mut state = SettlePolicyState::new(skip_frames(3, 5_000), 0);
        let clock = SteppedClock::new(0);
        for expected_frame in 1..=3 {
            clock.advance(33);
            assert_eq!(
                state.on_frame(clock.now_ms()),
                SettleDecision::Skip,
                "frame {expected_frame} must be skipped"
            );
        }
        clock.advance(33);
        assert_eq!(state.on_frame(clock.now_ms()), SettleDecision::Take);
        assert_eq!(state.frames_seen(), 4);
    }

    #[test]
    fn skipping_zero_frames_takes_the_first_one() {
        // The off-by-one this pins: `skip_frames(0)` means "no warm-up", not "skip one".
        let mut state = SettlePolicyState::new(skip_frames(0, 5_000), 0);
        assert_eq!(state.on_frame(1), SettleDecision::Take);
    }

    #[test]
    fn settle_for_converges_on_the_first_frame_at_or_after_its_duration() {
        let mut state = SettlePolicyState::new(settle_for(100, 5_000), 1_000);
        assert_eq!(state.on_frame(1_033), SettleDecision::Skip);
        assert_eq!(state.on_frame(1_099), SettleDecision::Skip);
        assert_eq!(state.on_frame(1_100), SettleDecision::Take);
    }

    #[test]
    fn a_stream_that_never_delivers_times_out_at_the_deadline_and_not_before() {
        // PF:11's failure mode with the frames removed: no `on_frame` call ever happens,
        // so the deadline has to live somewhere the frame loop cannot starve.
        let mut state = SettlePolicyState::new(skip_frames(10, 5_000), 0);
        assert_eq!(state.on_idle(4_999), None, "not expired one ms early");
        assert_eq!(state.on_idle(5_000), Some(SettleDecision::Timeout));
        assert_eq!(state.frames_seen(), 0);

        let err = state.timeout_error(5_000);
        assert_eq!(err.kind(), ErrorKind::SettleTimeout);
        assert_eq!(
            err,
            Error::SettleTimeout {
                waited_ms: 5_000,
                frames_seen: 0,
            }
        );
    }

    #[test]
    fn a_deadline_that_expires_mid_settle_produces_timeout_not_take() {
        // The bug: checking convergence before the deadline, so the frame that arrives
        // one millisecond late is silently accepted as the photo. Here the eleventh
        // frame is exactly the one `skip_frames(10)` would take.
        let mut state = SettlePolicyState::new(skip_frames(10, 100), 0);
        for _ in 0..10 {
            assert_eq!(state.on_frame(50), SettleDecision::Skip);
        }
        assert_eq!(state.on_frame(100), SettleDecision::Timeout);

        // The inverse: the same frame one millisecond earlier is the photo.
        let mut in_time = SettlePolicyState::new(skip_frames(10, 100), 0);
        for _ in 0..10 {
            assert_eq!(in_time.on_frame(50), SettleDecision::Skip);
        }
        assert_eq!(in_time.on_frame(99), SettleDecision::Take);
    }

    #[test]
    fn a_settle_for_longer_than_its_deadline_times_out_rather_than_waiting_forever() {
        // A configuration hazard rather than a device fault: both bounds are honored,
        // and the tighter one wins.
        let mut state = SettlePolicyState::new(settle_for(1_000, 200), 0);
        assert_eq!(state.on_frame(100), SettleDecision::Skip);
        assert_eq!(state.on_frame(200), SettleDecision::Timeout);
    }

    #[test]
    fn terminal_decisions_are_sticky() {
        // A caller that loops once more must not get a second Take (two photos) or a
        // Take after a Timeout (a photo the deadline refused).
        let mut taken = SettlePolicyState::new(skip_frames(0, 5_000), 0);
        assert_eq!(taken.on_frame(1), SettleDecision::Take);
        assert_eq!(taken.on_frame(2), SettleDecision::Take);
        assert_eq!(taken.on_idle(6_000), Some(SettleDecision::Take));

        let mut timed_out = SettlePolicyState::new(skip_frames(0, 10), 0);
        assert_eq!(timed_out.on_frame(10), SettleDecision::Timeout);
        assert_eq!(timed_out.on_frame(11), SettleDecision::Timeout);
        assert!(SettleDecision::Timeout.is_terminal());
        assert!(!SettleDecision::Skip.is_terminal());
    }

    #[test]
    fn the_frame_wait_is_bounded_by_the_frame_deadline_and_by_what_is_left() {
        let state = SettlePolicyState::new(skip_frames(10, 60_000), 0);
        assert_eq!(state.next_wait_ms(0), limits::FRAME_DEADLINE_MS);
        // Near the end of a settle the remaining time is the tighter bound, so the loop
        // wakes up in time to declare its own timeout.
        let short = SettlePolicyState::new(skip_frames(10, 100), 0);
        assert_eq!(short.next_wait_ms(60), 40);
        assert_eq!(short.next_wait_ms(100), 0);
        assert_eq!(
            short.next_wait_ms(10_000),
            0,
            "never negative, never wrapped"
        );
    }

    #[test]
    fn the_default_policy_settles_the_way_the_limits_table_says() {
        let mut state = SettlePolicyState::new(SettlePolicy::default(), 0);
        for _ in 0..limits::DEFAULT_SETTLE_SKIP_FRAMES {
            assert_eq!(state.on_frame(1), SettleDecision::Skip);
        }
        assert_eq!(state.on_frame(1), SettleDecision::Take);
    }

    #[test]
    fn the_stepped_clock_only_moves_when_told_to() {
        let clock = SteppedClock::new(500);
        assert_eq!(clock.now_ms(), 500);
        assert_eq!(clock.now_ms(), 500);
        clock.advance(250);
        assert_eq!(clock.now_ms(), 750);
        clock.advance(u64::MAX);
        assert_eq!(clock.now_ms(), u64::MAX, "saturating, never wrapping");
    }

    #[test]
    fn the_frozen_clock_reads_the_same_number_however_much_real_time_passes() {
        // The only property the type has, measured against an independent clock so that a
        // reading which had quietly become a real one is caught here rather than by a
        // contended integration test six crates away. Spun on rather than slept on, for
        // the reason `the_monotonic_clock_advances_with_real_time` states (note N3).
        const OBSERVED: u64 = 5;
        let clock = FrozenClock;
        let first = clock.now_ms();
        let independently = Instant::now();
        while independently.elapsed() < Duration::from_millis(OBSERVED) {
            std::hint::spin_loop();
        }
        assert_eq!(
            clock.now_ms(),
            first,
            "{OBSERVED} ms of real time passed and a clock that cannot move moved"
        );
        assert_eq!(
            first,
            FrozenClock::READING,
            "the reading that constant documents — and the assertion that keeps a frozen \
             clock distinguishable from E7's stuck one"
        );
    }

    #[test]
    fn a_settle_on_the_frozen_clock_reaches_its_frame_count_past_any_deadline() {
        // What a test using it buys, as an execution rather than as prose: a one-
        // millisecond deadline — shorter than any machine could keep — and the settle
        // still converges on its frames, because the deadline it is checked against never
        // arrives. On any moving clock this is a `Timeout` on frame one.
        let clock = FrozenClock;
        let mut state = SettlePolicyState::new(skip_frames(10, 1), clock.now_ms());
        for skipped in 1..=10 {
            assert_eq!(
                state.on_frame(clock.now_ms()),
                SettleDecision::Skip,
                "frame {skipped}"
            );
        }
        assert_eq!(state.on_frame(clock.now_ms()), SettleDecision::Take);
    }

    #[test]
    fn the_frozen_clock_crosses_a_thread_boundary_where_the_stepped_one_cannot() {
        // Note N45's argument is about a clock two threads can *move*. This one has nothing
        // to move, so it is `Sync` by construction — and the claim is made by a second
        // thread that actually reads it, because a `Sync` a comment asserts is a `Sync`
        // nothing checks. The same body over a `SteppedClock` does not compile, which is
        // the difference the two types exist to keep.
        let clock = FrozenClock;
        let shared = &clock;
        let elsewhere = std::thread::scope(|scope| {
            scope
                .spawn(move || shared.now_ms())
                .join()
                .expect("a reading cannot panic")
        });
        assert_eq!(elsewhere, clock.now_ms(), "two threads, two answers");
    }

    #[test]
    fn the_monotonic_clock_does_not_run_backwards() {
        let clock = MonotonicClock::new();
        let first = clock.now_ms();
        let second = clock.now_ms();
        assert!(second >= first, "{second} < {first}");
    }

    #[test]
    fn the_monotonic_clock_advances_with_real_time() {
        // The test above is satisfied by a clock stuck at any constant, and P3f's mutation
        // run proved it: replacing `now_ms` with `0` left the whole workspace green. A
        // stuck clock is not a cosmetic defect here — `expired` is `waited_ms >= deadline`,
        // so a settle against it never times out, and PF:11's wedged driver holds the actor
        // thread forever instead of raising `SettleTimeout`.
        //
        // The reading is therefore compared against an *independent* clock. Spun on rather
        // than slept on: `std::thread::sleep` is banned workspace-wide (note N3), and this
        // is a measurement of elapsed time rather than a synchronisation between two
        // things — the loop cannot finish early and cannot hang, because its condition is
        // the passage of the time it is waiting for.
        const OBSERVED: u64 = 5;
        let clock = MonotonicClock::new();
        let independently = Instant::now();
        while independently.elapsed() < Duration::from_millis(OBSERVED) {
            std::hint::spin_loop();
        }
        let reading = clock.now_ms();
        assert!(
            reading >= OBSERVED,
            "{OBSERVED} ms of real time passed and the clock reports {reading} ms"
        );
    }
}
