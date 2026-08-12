//! The preview's device half: one frame per command, off a stream that stays running
//! (design **D12**, §2.6; docs/7 P5b).
//!
//! [`crate::capture`] is this module's sibling and its counterweight. That one takes **a**
//! frame — start, settle, take, stop, with the stop on every exit — because a photo is one
//! answer to one request. A preview is the opposite shape: the stream is started once and then
//! runs for as long as somebody is watching, which is minutes rather than milliseconds, and
//! the frames are not answers to anything. So the two share a device and share nothing else,
//! and this module is deliberately not built on top of that one.
//!
//! ## Why a preview is *many short commands* rather than one long one
//!
//! Design §2.1 gives each open camera one blocking thread and one command channel, and
//! everything the daemon asks of a camera goes through it in arrival order. That leaves
//! exactly two ways to write a preview, and the choice is the whole design of this file.
//!
//! **One long command** — `start_stream`, loop until nobody is watching, `stop_stream` — is
//! the obvious one, and it is wrong here for a reason that has nothing to do with taste: while
//! that command runs, the actor is inside it, so every `wch_set` a person makes by dragging a
//! slider queues behind a command that ends when they close the tab. A control panel beside a
//! live preview (docs/7 P5c) is exactly that arrangement, so the obvious shape breaks the
//! client this preview exists for.
//!
//! **One frame per command** is what landed. [`turn`] takes a single frame and returns, so the
//! actor is free between every pair of frames and a control write waits at most one
//! [`limits::PREVIEW_FRAME_WAIT_MS`] — a third of a frame period at 30 fps, in practice.
//! What makes it affordable is that the *stream* is not part of the command: `STREAMON`
//! happened in [`start`] and the device keeps streaming between commands, because the
//! `Box<dyn Camera>` lives in the actor's own local and nothing in between touches it. The
//! cost is one command-queue round trip per frame, which is a `try_send`, a thread wake and a
//! channel send — measured against a `DQBUF` and a JPEG, it is not the expensive part.
//!
//! ## What that costs, stated rather than discovered later
//!
//! **A camera with a preview running refuses a photo, and that is the kernel's rule rather
//! than ours.** V4L2 allows one streamer per node; `crate::capture::grab` starts a stream of
//! its own, and a device already streaming answers [`Error::Busy`] — which is a fact about the
//! machine and not a capability statement (E3), and is the same refusal a second application
//! would meet. It is honest and it is not what docs/7 P5c's photo trigger wants. The mechanism
//! that would fix it is a preview that *yields* its stream for the duration of a photo and
//! resumes afterwards, which needs a suspend protocol on the daemon's feed and a photo path
//! that knows previews exist. It is named here and deliberately not built: there is no client
//! that trips it yet, and a mechanism built before the case it serves is a bound with nothing
//! to bound (rubric A8). `crates/daemon/tests/preview.rs` pins the current behaviour instead,
//! so the day it changes, something goes red.
//!
//! ## The stream is stopped by whoever ends the preview, and by the descriptor either way
//!
//! [`crate::capture`] stops its stream from a `Drop`, because there the stop and the command
//! have the same lifetime. Here they do not: the stream outlives every individual command, so
//! there is no scope whose end is the right moment, and [`stop`] is a command like the others.
//! What covers the paths that never reach it — a daemon that was killed, a driver thread that
//! panicked \[PF:1\], a preview whose task went away — is the descriptor: the actor's
//! `Box<dyn Camera>` is dropped when the camera closes on idle or when the thread unwinds
//! (`crate::actor::Liveness`), and a closed node is a stopped stream. So the guarantee is
//! layered rather than absent, and the layer that always holds is the kernel's.
//!
//! ## No frame reaches a log, an error, or a caller that did not ask for one
//!
//! A frame may contain a person (AGENTS, design §5). [`Frame`] already refuses to print its
//! own bytes and is deliberately not serializable; this module adds the second half — the only
//! thing it does with a frame is hand it to the [`FrameSink`] the caller supplied, and the only
//! things it puts in an error are the numbers the device gave. There is no `tracing` call in
//! this file at all, which is the cheapest possible version of that promise.

use std::time::{Duration, Instant};

use schema::capture::{Frame, NegotiatedStream, StreamRequest};
use schema::error::{Error, Result};
use schema::limits;

use crate::actor::OpenCamera;

/// Somewhere for a preview to put the frame it just took.
///
/// [`crate::progress::ProgressSink`]'s shape, for [`crate::progress`]'s reason and one more of
/// its own. The reason it shares: the engine names no async runtime (design §2.8, note **N5**'s
/// wall), so this cannot be a `tokio::sync::watch::Sender` even though a `watch` is exactly
/// what design D12 asks for — "streaming fan-out … via a latest-frame `watch` channel, so a
/// slow HTTP consumer drops frames and never backpressures the device". The channel is
/// transport, so the channel lives in the transport, and what crosses the boundary is this
/// trait. Note **N41** made the same call about the actor's *reply* channel for the same
/// reason, and `daemon::preview`'s header is where the watch itself is argued.
///
/// The reason of its own is [`Demand`]. A progress sink cannot fail and cannot refuse, because
/// nothing about a subscriber is a reason to abandon a sweep holding a camera. A preview is
/// the opposite: the *only* reason to hold a camera open is that somebody is watching, so the
/// sink is asked after every frame whether anybody still is, and the answer is what ends the
/// stream. That question has to come from the sink because the sink is the only thing that can
/// see a socket.
///
/// `&self` and `Send + Sync`, so one sink can be built by a task and called from the actor's
/// own thread — which is where every call actually happens.
pub trait FrameSink: std::fmt::Debug + Send + Sync {
    /// Take one frame, and say whether the stream should keep going.
    ///
    /// Never blocks and never fails. A sink that blocked here would be backpressuring a device
    /// on behalf of a consumer, which is the one thing D12's sentence rules out; a sink that
    /// could fail would put "the browser tab is slow" on the list of things that can end a
    /// capture, and that is the wrong list.
    fn publish(&self, frame: Frame) -> Demand;
}

/// Whether anybody still wants frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Demand {
    /// Somebody is watching. Keep the stream running.
    More,
    /// Nobody is watching. The next thing the caller should do is [`stop`].
    Enough,
}

/// What one turn of the preview loop did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// A frame reached the sink, and this is what it said afterwards.
    Frame(Demand),
    /// [`limits::PREVIEW_FRAME_WAIT_MS`] passed with no frame.
    ///
    /// **Not an error**, and that is a decision rather than leniency: a device that missed one
    /// deadline is slow, and a device that has stopped delivering is a device that misses
    /// [`limits::PREVIEW_MAX_EMPTY_TURNS`] of them in a row. Which of the two this is cannot
    /// be known from one turn, so this module reports the turn and the caller counts — the
    /// same division `crate::settle` makes between a frame that did not arrive and a settle
    /// that timed out.
    Idle,
}

/// The stream a preview asks a camera for.
///
/// One expression, here rather than in the daemon, because everything in it is a fact about
/// *frames*: the format has to be one a browser can put in an `<img>` without this process
/// decoding anything, and the size is the cap [`limits::PREVIEW_MAX_WIDTH`] argues.
///
/// **MJPEG, and the camera's own bytes.** Design §2.6 names the preview
/// `multipart/x-mixed-replace`, which is a sequence of `image/jpeg` parts, and every webcam
/// this project has met offers MJPG — so the whole preview path is a copy from a `DQBUF`
/// buffer to a socket with no decode, no re-encode and no imaging crate in it at all. That is
/// the same "verbatim camera JPEG when the sink allows" rule the photo path follows (AGENTS),
/// and here it is what makes a preview cost a daemon almost nothing per frame.
///
/// The width and height are a **cap**, not a size: `StreamRequest::choose` answers with the
/// largest mode that fits inside them, and falls back to the device's own first entry when
/// nothing does. So a camera whose only MJPEG mode is 1920×1080 gets a preview at 1920×1080,
/// with the difference reported as an [`schema::capture::Adjustment`] — because D5's rule is
/// that the negotiated answer is surfaced whenever it differs from the request, and a preview
/// is not exempt from it.
#[must_use]
pub fn request() -> StreamRequest {
    StreamRequest {
        pixel_format: Some(schema::camera::PixelFormat::MJPG),
        width: Some(limits::PREVIEW_MAX_WIDTH),
        height: Some(limits::PREVIEW_MAX_HEIGHT),
        ..StreamRequest::default()
    }
}

/// Start the preview stream, and refuse a negotiation a browser cannot use.
///
/// The check after the negotiation is the point of this function existing at all — without it
/// [`start`] would be `camera.start_stream(request)` spelled twice. D5 lets a driver answer
/// with something other than what was asked for and requires that the difference be surfaced;
/// a *photo* can carry that difference to its caller and let them decide, but a preview cannot:
/// the bytes are going into an `<img>` as `image/jpeg`, and YUYV bytes labelled `image/jpeg`
/// are a broken image in a browser rather than a wrong one. So the caller is told, in the
/// vocabulary it would have been told in if the format had been refused outright.
///
/// [`schema::camera::PixelFormat::is_compressed`] rather than `== MJPG`, because `JPEG` and
/// `MJPG` are both JPEG bitstreams on the wire and a device that answered with the other one
/// has given us exactly what we can serve. That is the schema's own answer to the question,
/// which is where a rule about a format vocabulary belongs (design §2.10).
///
/// # Errors
///
/// [`Error::FormatUnsupported`] when the device negotiated something a browser cannot render,
/// carrying what was asked for and what it got — the pair that lets an operator see whether
/// their camera has an MJPEG mode at all. Otherwise whatever `start_stream` refused with,
/// unchanged: [`Error::Busy`] for a node somebody else is streaming (E3 keeps that distinct
/// from "the camera cannot"), and the device's own answer for everything else.
pub fn start(device: OpenCamera<'_>, request: &StreamRequest) -> Result<NegotiatedStream> {
    let negotiated = device.start_stream(request)?;
    if !negotiated.pixel_format.is_compressed() {
        // Stopped before refusing: this function started the stream, so the refusal must not
        // leave a camera streaming for a preview that is not going to happen. The stop's own
        // failure is discarded for `crate::capture::StreamGuard`'s reason — the caller is
        // already holding the error that matters, and a second one would replace it.
        let _ = device.stop_stream();
        return Err(Error::FormatUnsupported {
            requested: request.pixel_format,
            available: vec![negotiated.pixel_format],
        });
    }
    Ok(negotiated)
}

/// Take one frame and hand it to `sink`.
///
/// The whole of one command's device work, and it is short on purpose — see this module's
/// header for why the preview is a chain of these rather than one loop.
///
/// The deadline is [`limits::PREVIEW_FRAME_WAIT_MS`] from *now*, computed here rather than
/// taken as a parameter, and that is the one place this module reads a clock. It is not the
/// `crate::settle::Clock` seam and does not want to be: the seam exists so a *policy* can be
/// driven by a test without waiting, and there is no policy here — there is one blocking call
/// whose argument is the instant the kernel should give up at. `crate::capture::grab` reads
/// `Instant::now()` in exactly the same place and for exactly the same reason, one line under
/// a comment that says the policy owns the deadline. A test that wants to talk about a device
/// which does not deliver hands in a device that does not deliver
/// (`crate::double::ScriptedCamera`), and gets [`Delivery::Idle`] with no wall-clock wait at
/// all, because a backend that has no frame answers immediately.
///
/// # Errors
///
/// Whatever the device refused with, unchanged and unclassified —
/// [`Error::DeviceGone`] for a camera that was unplugged mid-preview, [`Error::DeviceIo`] for a
/// `DQBUF` that failed. A frame that simply did not arrive is [`Delivery::Idle`] and not an
/// error; the conversion the other way, which would let a slow camera look like a broken one,
/// is what AGENTS rule 7 forbids.
pub fn turn(device: OpenCamera<'_>, sink: &dyn FrameSink) -> Result<Delivery> {
    let deadline = Instant::now() + Duration::from_millis(limits::PREVIEW_FRAME_WAIT_MS);
    match device.next_frame(deadline) {
        Ok(frame) => Ok(Delivery::Frame(sink.publish(frame))),
        // The one refusal this path reads rather than forwards. `next_frame`'s contract makes
        // a spent deadline a `SettleTimeout`, which on the photo path is a real failure with a
        // caller waiting for a photo — and here is one frame of a stream that has hundreds
        // more coming.
        Err(Error::SettleTimeout { .. }) => Ok(Delivery::Idle),
        Err(err) => Err(err),
    }
}

/// Stop the preview stream.
///
/// A named function over one forwarded call, and it exists so that "the preview stops the
/// stream" is a thing the code *says* at the one place a reader looks for it. The caller is
/// the daemon's driver, on every path out of its loop.
///
/// # Errors
///
/// Whatever `stop_stream` refused with. `VIDIOC_STREAMOFF` on a node that is not streaming is
/// not an error, so a duplicate stop is safe — which matters because the paths that reach this
/// include one where the device already stopped on its own.
pub fn stop(device: OpenCamera<'_>) -> Result<()> {
    device.stop_stream()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use schema::ErrorKind;
    use schema::camera::PixelFormat;

    use super::*;
    use crate::double::{ScriptedCamera, frame, integer};

    /// A sink that keeps what it was given and answers what the test told it to.
    #[derive(Debug)]
    struct Watching {
        taken: Mutex<Vec<Frame>>,
        /// How many more frames this sink wants before it says [`Demand::Enough`].
        wanted: AtomicUsize,
    }

    impl Watching {
        fn wanting(frames: usize) -> Watching {
            Watching {
                taken: Mutex::new(Vec::new()),
                wanted: AtomicUsize::new(frames),
            }
        }

        fn sequences(&self) -> Vec<u32> {
            self.locked().iter().map(|frame| frame.sequence).collect()
        }

        fn locked(&self) -> std::sync::MutexGuard<'_, Vec<Frame>> {
            self.taken
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }
    }

    impl FrameSink for Watching {
        fn publish(&self, frame: Frame) -> Demand {
            self.locked().push(frame);
            match self.wanted.load(Ordering::Relaxed) {
                0 => Demand::Enough,
                left => {
                    self.wanted.store(left - 1, Ordering::Relaxed);
                    Demand::More
                }
            }
        }
    }

    /// A camera a preview can actually stream from: JPEG bitstream, `frames` frames scripted.
    ///
    /// Compressed by default rather than by request, because every test below except the
    /// negotiation one is about the *loop* and would otherwise be refused at [`start`] for a
    /// reason that has nothing to do with what it is asserting.
    fn camera(frames: u32) -> ScriptedCamera {
        ScriptedCamera::new(vec![integer("brightness", 50)])
            .compressed()
            .with_frames((0..frames).map(frame).collect())
    }

    #[test]
    fn the_stream_is_started_once_and_every_turn_takes_the_next_frame_off_it() {
        // The shape the whole module exists for: one `start`, then N commands that each take
        // one frame, then one `stop`. A build that restarted the stream per frame would answer
        // `Busy` on the second `start_stream` (the fake enforces one streamer per node
        // exactly as V4L2 does), so the sequence numbers below are what says the stream
        // *persisted* between commands rather than being re-established.
        let mut device = camera(4);
        let sink = Watching::wanting(3);
        start(&mut device, &request()).expect("the scripted camera streams");

        for _ in 0..3 {
            assert_eq!(
                turn(&mut device, &sink).expect("a frame is scripted"),
                Delivery::Frame(Demand::More)
            );
        }
        assert_eq!(
            sink.sequences(),
            vec![0, 1, 2],
            "frames arrived out of order"
        );
        assert_eq!(device.stops, 0, "a turn stopped the stream");

        stop(&mut device).expect("stopping a running stream");
        assert_eq!(device.stops, 1);
    }

    #[test]
    fn a_sink_that_has_stopped_watching_is_what_ends_the_stream() {
        // D12's "so a slow HTTP consumer drops frames" has a second half this module owns:
        // the *reason* to hold the camera is that somebody is watching. The sink is the only
        // thing that can see a socket, so it is the thing that answers — and a build that
        // ignored the answer would keep a descriptor and a `STREAMON` alive for a tab that
        // closed.
        let mut device = camera(4);
        let sink = Watching::wanting(0);
        start(&mut device, &request()).expect("the scripted camera streams");

        assert_eq!(
            turn(&mut device, &sink).expect("a frame is scripted"),
            Delivery::Frame(Demand::Enough)
        );
    }

    #[test]
    fn a_frame_that_did_not_arrive_is_an_idle_turn_and_not_a_failure() {
        // The conversion AGENTS rule 7 forbids, in the direction it would happen here: a
        // camera that missed one deadline is slow, and turning that into an error would end a
        // preview on the first frame a busy USB bus delayed. The scripted camera runs out of
        // frames, which is how it says "nothing right now" — with no wall-clock wait, because
        // a backend with no frame answers immediately (note N3).
        let mut device = camera(1);
        let sink = Watching::wanting(9);
        start(&mut device, &request()).expect("the scripted camera streams");

        assert_eq!(
            turn(&mut device, &sink).expect("one frame is scripted"),
            Delivery::Frame(Demand::More)
        );
        for _ in 0..3 {
            assert_eq!(
                turn(&mut device, &sink).expect("an empty turn is not an error"),
                Delivery::Idle
            );
        }
        assert_eq!(
            sink.sequences(),
            vec![0],
            "an idle turn published something"
        );
    }

    #[test]
    fn a_device_that_went_away_mid_preview_refuses_in_its_own_words() {
        // The other half of the arm above, and the reason it is a `match` rather than an
        // `unwrap_or`: `DeviceGone` is not "no frame right now", it is a camera that has been
        // unplugged, and a preview that counted it as an idle turn would spend
        // `PREVIEW_MAX_EMPTY_TURNS` seconds pretending otherwise.
        let mut device = camera(1).frames_refused(Error::DeviceGone {
            path: camino::Utf8PathBuf::from("/dev/video0"),
        });
        let sink = Watching::wanting(9);
        start(&mut device, &request()).expect("the scripted camera streams");

        let err = turn(&mut device, &sink).expect_err("the device is gone");
        assert_eq!(err.kind(), ErrorKind::DeviceGone);
    }

    #[test]
    fn a_negotiation_a_browser_cannot_render_is_refused_and_the_stream_is_stopped() {
        // D5 lets a driver answer with a format nobody asked for, and this is the one caller
        // that cannot carry that difference to its client: the bytes are going into an `<img>`
        // labelled `image/jpeg`. Both halves are asserted — the typed refusal, and that the
        // stream this function started does not survive it, because a camera left streaming
        // for a preview that is not happening is a camera the next `open` finds busy.
        let mut device = ScriptedCamera::new(vec![integer("brightness", 50)]);
        let refusal = start(&mut device, &request())
            .expect_err("this camera negotiates YUYV and nothing else");
        assert_eq!(refusal.kind(), ErrorKind::FormatUnsupported);
        assert_eq!(device.stops, 1, "the refused stream was left running");

        // And the other direction, so the assertion above is about the *format* rather than
        // about a function that refuses everything: the same camera, offering what a preview
        // can serve.
        let mut device = camera(4);
        let negotiated = start(&mut device, &request()).expect("MJPG is on offer");
        assert!(negotiated.pixel_format.is_compressed());
        assert_eq!(device.stops, 0);
    }

    #[test]
    fn the_request_asks_for_jpeg_bytes_at_the_size_the_limits_table_caps() {
        // The three fields that make the preview cost no decode and no re-encode, read off
        // the constants rather than written twice. A build that dropped the format would
        // negotiate whatever the device listed first, which on the seed hardware is MJPG and
        // on the next camera is not.
        let asked = request();
        assert_eq!(asked.pixel_format, Some(PixelFormat::MJPG));
        assert_eq!(asked.width, Some(limits::PREVIEW_MAX_WIDTH));
        assert_eq!(asked.height, Some(limits::PREVIEW_MAX_HEIGHT));
    }
}
