//! Getting one frame off a camera (design D5).
//!
//! Start a stream, discard frames until the sensor has settled \[PF:11\], take the next
//! one, stop the stream. Four steps, and the interesting part is that the third one has a
//! deadline and the fourth one has to happen anyway.
//!
//! ## The policy is pure and this is its shell
//!
//! [`crate::settle`] is a fold over `(a frame arrived, what time is it)` on a steppable
//! clock, so every off-by-one in "skip ten frames" is testable without a camera and
//! without a `sleep`. This module is the loop that feeds it: it asks the policy how long
//! it may block, blocks that long, and hands the answer back. Nothing here decides *when*
//! a stream has settled — that would be the second copy of a rule whose whole value is
//! being one copy.
//!
//! ## The stream is stopped on every exit
//!
//! Including the timeout, including a device that vanished, including a panic in a test.
//! A camera left streaming is a camera the next `open` finds busy, and "the last run
//! crashed" is not something a user should have to know to explain an `EBUSY`. That is a
//! `Drop`, not a line at the end of the happy path — the happy path is the one place the
//! stop was never going to be missed.

use std::time::{Duration, Instant};

use schema::backend::Camera;
use schema::capture::{Frame, NegotiatedStream, SettlePolicy, StreamRequest};
use schema::error::Result;
use schema::limits;

use crate::settle::{Clock, SettleDecision, SettlePolicyState};

/// One frame, and the stream it came off.
///
/// Not a DTO. [`Frame`] is deliberately not serializable — a frame may contain a person
/// (rubric A12) — so this type is the engine's internal answer and the *photo* is what
/// crosses a boundary.
#[derive(Debug)]
pub struct Capture {
    /// What the device agreed to, with every difference from the request (D5).
    pub negotiated: NegotiatedStream,
    /// The frame the settle policy chose.
    pub frame: Frame,
    /// How many frames were discarded first, so a caller can tell a camera that settled
    /// immediately from one that needed the whole warm-up.
    pub frames_settled: u32,
}

/// Start, settle, take one frame, stop.
///
/// `clock` is a seam so tests drive the deadline by hand ([`crate::settle::SteppedClock`]);
/// production passes [`crate::settle::MonotonicClock`]. Nothing in this module sleeps — a
/// `sleep` in a test is a flake with a timer on it (note N3).
///
/// # Errors
///
/// [`schema::Error::SettleTimeout`] when the policy's deadline passes first, carrying how
/// long it waited and how many frames it saw — the pair that separates a slow camera from
/// a dead one (E3). Otherwise whatever the device said, unchanged.
pub fn grab(
    camera: &mut dyn Camera,
    request: &StreamRequest,
    policy: SettlePolicy,
    clock: &dyn Clock,
) -> Result<Capture> {
    let negotiated = camera.start_stream(request)?;
    let guard = StreamGuard { camera };
    let mut state = SettlePolicyState::new(policy, clock.now_ms());

    // The deadline is what ends this loop, and [`limits::MAX_SETTLE_ROUNDS`] is what ends
    // it when the deadline cannot: a round that neither consumes a frame nor advances the
    // clock makes no progress, and a backend answering "no frame" without waiting on a
    // clock that is not moving produces exactly that. Rubric A14 — bounded everything, and
    // the bound that matters is the one covering the case the obvious bound misses.
    for _ in 0..limits::MAX_SETTLE_ROUNDS {
        // The policy owns the deadline; this converts its answer into the one the
        // blocking read understands, and it is the only place in the engine that does.
        // `next_wait_ms` is bounded by `limits::FRAME_DEADLINE_MS` as well as by what is
        // left, so a camera delivering nothing still gets its `on_idle` turn.
        let wait = Duration::from_millis(state.next_wait_ms(clock.now_ms()));
        let decision = match guard.camera.next_frame(Instant::now() + wait) {
            Ok(frame) => match state.on_frame(clock.now_ms()) {
                SettleDecision::Take => {
                    let frames_settled = state.frames_seen().saturating_sub(1);
                    return Ok(Capture {
                        negotiated,
                        frame,
                        frames_settled,
                    });
                }
                other => other,
            },
            // A frame that did not arrive is not on its own a failure: the device may
            // simply be slower than one wait. Whether it is a *timeout* is the policy's
            // call, and `on_idle` is where that question lives.
            Err(schema::Error::SettleTimeout { .. }) => state
                .on_idle(clock.now_ms())
                .unwrap_or(SettleDecision::Skip),
            // Anything else — the device vanished, the node went busy — is the device's
            // answer and travels unchanged. The guard still stops the stream.
            Err(error) => return Err(error),
        };

        if decision == SettleDecision::Timeout {
            return Err(state.timeout_error(clock.now_ms()));
        }
    }

    // The backstop fired. The honest answer is the one the deadline would have given: the
    // sensor never settled, and here is how long we waited and how many frames we saw.
    Err(state.timeout_error(clock.now_ms()))
}

/// Stops the stream when it goes out of scope, however it goes out of scope.
struct StreamGuard<'a> {
    camera: &'a mut dyn Camera,
}

impl Drop for StreamGuard<'_> {
    fn drop(&mut self) {
        // A stop that fails has nothing left to tell anyone: the caller is already
        // holding either a frame or the error that matters, and a second error would
        // replace the useful one. Dropped deliberately, like `Fd`'s close.
        let _ = self.camera.stop_stream();
    }
}

#[cfg(test)]
mod tests {
    use schema::ErrorKind;
    use schema::camera::PixelFormat;
    use schema::capture::SettleSpec;

    use super::*;
    use crate::double::{ScriptedCamera, frame, integer};
    use crate::settle::SteppedClock;

    fn camera(frames: u32) -> ScriptedCamera {
        ScriptedCamera::new(vec![integer("brightness", 50)])
            .with_frames((0..frames).map(frame).collect())
    }

    fn skip(frames: u32) -> SettlePolicy {
        SettlePolicy {
            spec: SettleSpec::SkipFrames { frames },
            deadline_ms: 5_000,
        }
    }

    #[test]
    fn the_frame_taken_is_the_one_after_the_last_skipped_and_the_count_says_so() {
        let mut device = camera(10);
        let clock = SteppedClock::new(0);
        let capture = grab(&mut device, &StreamRequest::default(), skip(3), &clock)
            .expect("four frames are available");

        assert_eq!(
            capture.frame.sequence, 3,
            "skip(3) discards frames 0..2 and takes the fourth"
        );
        assert_eq!(capture.frames_settled, 3);
        assert_eq!(capture.negotiated.pixel_format, PixelFormat::YUYV);
        assert_eq!(device.stops, 1, "the stream was stopped");
    }

    #[test]
    fn skipping_nothing_takes_the_first_frame() {
        // The off-by-one from the other side: `skip_frames(0)` means "no warm-up", not
        // "skip one". The policy owns that rule; this asserts the loop honours it.
        let mut device = camera(4);
        let clock = SteppedClock::new(0);
        let capture = grab(&mut device, &StreamRequest::default(), skip(0), &clock)
            .expect("one frame is enough");
        assert_eq!(capture.frame.sequence, 0);
        assert_eq!(capture.frames_settled, 0);
    }

    #[test]
    fn the_default_policy_settles_the_way_the_limits_table_says() {
        let mut device = camera(limits::DEFAULT_SETTLE_SKIP_FRAMES + 4);
        let clock = SteppedClock::new(0);
        let capture = grab(
            &mut device,
            &StreamRequest::default(),
            SettlePolicy::default(),
            &clock,
        )
        .expect("captures");
        assert_eq!(capture.frames_settled, limits::DEFAULT_SETTLE_SKIP_FRAMES);
    }

    #[test]
    fn a_camera_that_stops_delivering_times_out_and_the_stream_is_stopped() {
        // The failure PF:11 is really about: a stream that starts and then produces
        // nothing. The camera runs out of scripted frames, which reaches the loop as a
        // frame that did not arrive, and the *policy* — not the device — decides that the
        // deadline has passed.
        let mut device = camera(2);
        let clock = SteppedClock::new(0);
        // A deadline in the past means the first `on_idle` is already expired, so the
        // loop terminates without a sleep and without a wall clock (N3).
        let policy = SettlePolicy {
            spec: SettleSpec::SkipFrames { frames: 10 },
            deadline_ms: 0,
        };
        let error = grab(&mut device, &StreamRequest::default(), policy, &clock)
            .expect_err("ten frames were asked for and two exist");

        assert_eq!(error.kind(), ErrorKind::SettleTimeout);
        let schema::Error::SettleTimeout { frames_seen, .. } = error else {
            panic!("expected a settle timeout");
        };
        assert!(
            frames_seen <= 2,
            "it cannot have seen frames that do not exist"
        );
        assert_eq!(device.stops, 1, "a timeout still stops the stream");
    }

    #[test]
    fn a_settle_that_expires_mid_stream_reports_a_timeout_rather_than_a_late_frame() {
        // A photo taken after the caller's deadline is a photo the caller did not ask
        // for. The clock is stepped past the deadline while frames are still arriving, so
        // the *only* thing that can produce the timeout is the policy's ordering rule.
        let mut device = camera(100);
        let clock = SteppedClock::new(0);
        let policy = SettlePolicy {
            spec: SettleSpec::SettleFor { millis: 10_000 },
            deadline_ms: 50,
        };
        clock.advance(60);
        let error = grab(&mut device, &StreamRequest::default(), policy, &clock)
            .expect_err("the deadline is already past");
        assert_eq!(error.kind(), ErrorKind::SettleTimeout);
        assert_eq!(device.stops, 1);
    }

    #[test]
    fn a_stream_that_cannot_start_refuses_and_needs_no_stopping() {
        // E3: a camera that offers no format has stated a *capability* limit, and the
        // refusal reaches the caller unchanged rather than arriving as a settle timeout
        // once the loop gave up waiting for a frame that was never coming.
        let mut device = ScriptedCamera::new(vec![integer("brightness", 50)]).without_formats();
        let clock = SteppedClock::new(0);
        let error = grab(&mut device, &StreamRequest::default(), skip(0), &clock)
            .expect_err("a camera with no format cannot be streamed");
        assert_eq!(error.kind(), ErrorKind::FormatUnsupported);
        assert_eq!(
            device.stops, 0,
            "a stream that never started needs no stopping"
        );
    }

    #[test]
    fn a_backend_that_never_delivers_on_a_clock_that_never_moves_gives_up_rather_than_spinning() {
        // The one case the deadline cannot end, and therefore the one that needs a
        // separate bound: every round the backend answers "no frame" without waiting, and
        // the clock does not move, so `on_idle` never expires. Written with a stepped
        // clock because that is the shape in which it is reachable — and reachable is what
        // makes `MAX_SETTLE_ROUNDS` a bound rather than a comment.
        //
        // It is a *test that would hang* if the bound were removed, which is the strongest
        // form the inverse can take here.
        let mut device = camera(0);
        let clock = SteppedClock::new(0);
        let error = grab(
            &mut device,
            &StreamRequest::default(),
            SettlePolicy {
                spec: SettleSpec::SkipFrames { frames: 1 },
                deadline_ms: u64::MAX,
            },
            &clock,
        )
        .expect_err("the backstop reports the timeout the deadline never would");
        assert_eq!(error.kind(), ErrorKind::SettleTimeout);
        assert_eq!(device.stops, 1, "the stream is stopped on that path too");
    }

    #[test]
    fn the_negotiated_stream_is_reported_even_when_it_differs_from_the_request() {
        // D5's whole point, at the layer above the backend: the caller asked for
        // something the device does not have, and the answer says so rather than quietly
        // being a different photo.
        let mut device = camera(2);
        let clock = SteppedClock::new(0);
        let capture = grab(
            &mut device,
            &StreamRequest {
                width: Some(1920),
                height: Some(1080),
                ..StreamRequest::default()
            },
            skip(0),
            &clock,
        )
        .expect("the device adjusts rather than refusing");
        assert!(!capture.negotiated.is_exact());
        assert_eq!(
            (capture.negotiated.width, capture.negotiated.height),
            (32, 16)
        );
    }
}
