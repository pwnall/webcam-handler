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
//! ## What that used to cost, and what it costs now (owner ruling, 2026-08-12)
//!
//! **A camera with a preview running used to refuse a photo**, because V4L2 allows one
//! streamer per node: `crate::capture::grab` starts a stream of its own, and a device already
//! streaming answers [`Error::Busy`]. That was honest — a fact about the machine rather than a
//! capability statement (E3) — and it was not what docs/7 P5c's photo trigger wants. This
//! header named the fix, declined to build it on rubric A8 ("a mechanism built before the case
//! it serves is a bound with nothing to bound") and pinned the refusal with a test.
//!
//! The owner has ruled that the fix gets built, and [`while_suspended`] is it: the photo
//! stops the preview's stream, takes its frame and starts the stream again, **all inside the
//! one command on the camera's own actor thread**. The old argument is superseded rather than
//! wrong — its condition was "there is no client that trips it yet", and P5c is that client —
//! but two of its sentences were wrong on their own terms and are worth correcting rather than
//! deleting, because both were guesses about where the mechanism would have to live:
//!
//! - "needs a suspend protocol on the daemon's feed" — it needs nothing on the feed. The
//!   daemon's registry is not consulted, because the *device* is the authority on whether it
//!   is streaming (`schema::backend::Camera::streaming`, AGENTS rule 4), and asking the feed
//!   would have put the question one layer above the answer and one race away from it.
//! - "a photo path that knows previews exist" — the photo path knows only that *something* was
//!   streaming when its command began, and inside one actor there is exactly one thing that
//!   can be: a preview, because every other stream in this engine starts and stops inside a
//!   single command (`crate::capture::grab`'s `StreamGuard`). So the invariant does the
//!   knowing, and no caller passes a flag.
//!
//! Note **N83** records the ruling, what the retired test pinned, and what replaced it.
//!
//! ## The second bullet's invariant is false since P6c, and this is where the answer went
//!
//! `crate::record` streams across commands too — a take holds its stream for its whole duration
//! (note **N111**) — so "inside one actor there is exactly one thing that can be streaming" no
//! longer holds, and [`while_suspended`] cannot tell the two apart: `Camera::streaming` answers
//! about the **device**, and a device does not know what a stream is for. That is not a defect
//! in this function; it is the limit of what the device can be asked, and inventing an answer
//! here would put the question back above the fact — which is the mistake the bullet above
//! records somebody already making once.
//!
//! Suspending a *recording* would be a loss rather than a pause: the frames inside the window
//! are frames the take never gets, and the gap in the driver's timestamps is what D7's
//! close-time rewrite turns into a slower mean interval for the whole file — which the notes'
//! Expected usage item 10 forbids in as many words ("it must not make a dropped frame look like
//! a slow transition"). So the caller that must not arrive here is kept away one layer up, where
//! the answer exists: `daemon::server`'s `wch_photo` refuses a camera holding a running take
//! with [`Error::Busy`] — *retry*, which is the right advice, because a take is bounded by its
//! own duration (note **N118**). `webcam-handler-cli` cannot reach the case at all: it opens a
//! camera per invocation, takes one photo and closes it, so nothing in that process is ever
//! recording while a photo is taken.
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
/// largest mode that fits inside them, and the difference is reported as an
/// [`schema::capture::Adjustment`] — because D5's rule is that the negotiated answer is
/// surfaced whenever it differs from the request, and a preview is not exempt from it.
///
/// A camera *every* one of whose MJPEG modes is over the cap is the case this module's own
/// `negotiate` exists for — it is private, and it is worth reading before changing either:
/// the cap cannot be met, the
/// preview happens anyway, and what it happens at is the format's largest mode — 3840×2160
/// on the OBSBOT, for a cap of 640×480. That is what this path has always done and the
/// repair of 2026-08-16 deliberately did not change it (note **N134**); it is a bandwidth
/// decision nobody has taken, not a bug the refusal introduced.
#[must_use]
pub fn request() -> StreamRequest {
    StreamRequest {
        pixel_format: Some(schema::camera::PixelFormat::MJPG),
        width: Some(limits::PREVIEW_MAX_WIDTH),
        height: Some(limits::PREVIEW_MAX_HEIGHT),
        ..StreamRequest::default()
    }
}

/// Start the stream for a request whose size is a **cap**, not a demand.
///
/// D5's "an explicit request still wins … or a typed refusal" is about a *caller* who named
/// a size, and since the owner's ruling of 2026-08-16 a named size nothing fits is refused
/// rather than answered with the format's largest mode (note **N134**). The preview did not
/// name its size — [`limits::PREVIEW_MAX_WIDTH`] is a bandwidth bound this module imposed —
/// and the OBSBOT is the camera that makes the difference load-bearing: its MJPG list starts
/// at 1280×720, so every frame it can send is over a 640×480 cap and refusing would mean the
/// owner cannot look at the camera at all.
///
/// So the cap is asked for first and dropped if nothing fits under it, and the answer is then
/// compared against the request that *did* name a size — the caller is told what it got and
/// what it asked for, which is exactly D5's reporting half, untouched. The alternative —
/// teaching the shared resolver that some sizes are caps — would put a second meaning on two
/// fields the wire already carries one meaning for.
///
/// **The second request is the one this module had before the refusal existed**, so the
/// stream a preview gets on such a camera is the same one it has always got: the format's
/// largest mode. Answering with the *smallest* offered mode would be the better preview and
/// is not this repair's to decide — it is a bandwidth choice for whoever owns the preview's
/// budget, and it is written down here rather than made quietly.
fn negotiate(device: OpenCamera<'_>, request: &StreamRequest) -> Result<NegotiatedStream> {
    match device.start_stream(request) {
        // A refusal carrying a [`schema::error::SizeRefusal`] is the one a cap can outgrow,
        // and it says so in the payload rather than being inferred from an empty
        // `requested` slot beside a request that happened to name a size — which is what
        // this match did for the two days between the halves of the H1b repair, and is what
        // note **N134**'s *Retires when* clause said would replace it (note **N138**). A
        // format this preview named and the device does not have comes back as a format
        // refusal, and no second request would help.
        Err(Error::FormatUnsupported { size: Some(_), .. }) => {
            let uncapped = StreamRequest {
                width: None,
                height: None,
                ..request.clone()
            };
            let mut negotiated = device.start_stream(&uncapped)?;
            negotiated.adjustments = NegotiatedStream::diff(
                request,
                negotiated.pixel_format,
                negotiated.width,
                negotiated.height,
                negotiated.interval,
            );
            Ok(negotiated)
        }
        other => other,
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
    let negotiated = negotiate(&mut *device, request)?;
    if !negotiated.pixel_format.is_compressed() {
        // Stopped before refusing: this function started the stream, so the refusal must not
        // leave a camera streaming for a preview that is not going to happen. The stop's own
        // failure is discarded for `crate::capture::StreamGuard`'s reason — the caller is
        // already holding the error that matters, and a second one would replace it.
        let _ = device.stop_stream();
        return Err(Error::format_unsupported(
            request.pixel_format,
            vec![negotiated.pixel_format],
        ));
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

/// What one interruption did to a preview that was running.
///
/// The *second* fact [`while_suspended`] produces, and it is a value rather than a log line
/// because the pause is invisible everywhere else: no frame is dropped, no reader falls
/// behind, and this daemon's publication count simply stops for a moment and then continues.
/// Rubric rule 3 wants a number rather than a silence, and this is what the number is counted
/// from.
#[derive(Debug, Clone)]
pub struct Gap {
    /// The stream the viewers were watching, stopped and started again around the capture.
    ///
    /// Carried rather than discarded because it is what makes the resume *exact*: the request
    /// that puts the stream back names the format, the size and the interval the device had
    /// already agreed to, so a preview does not change shape underneath a page that has
    /// already sized an `<img>` around it.
    pub stream: NegotiatedStream,
    /// Whether the stream came back, and the device's own words when it did not.
    ///
    /// Separate from the photo's own answer on purpose — see [`while_suspended`] for which of
    /// the two wins and why neither is allowed to hide the other.
    pub resumed: Result<()>,
}

/// Run `work` with any running preview stream stopped, and start it again afterwards.
///
/// **The whole of the owner's 2026-08-12 ruling, as one function that cannot be half-used.**
/// There is no `suspend` and no `resume` in this module's public surface, because a suspend
/// any caller can invoke separately is a suspend somebody forgets to resume — and the thing
/// they would forget is a camera left dark for a tab that is still open. The stop, the work
/// and the start are one expression, and the only way to reach the first is to supply the
/// second.
///
/// **It is indivisible because of where it runs, not because of what it locks.** The caller
/// is `crate::photo::take`, which runs inside one `engine::actor` command on the thread that
/// owns the `Box<dyn Camera>`; that thread takes one command at a time in arrival order, so
/// no other work — a second photo, a preview turn, a control write — can be interleaved
/// between the stop and the start. Nothing here takes a lock, and there is nothing to take one
/// on: the exclusivity D12 asks for is still enforced by the actor's shape rather than by this
/// function agreeing to behave.
///
/// ## What happens when there is no preview
///
/// `work` runs, and that is the whole of it: no stop, no start, no bound consulted, no [`Gap`].
/// The early return below is `work(device)` with nothing before or after it, which is what
/// makes "a photo with no preview running is unchanged" a property of the *code path* rather
/// than of a test that happened to check the observable parts.
///
/// ## What happens when the capture fails
///
/// The stream comes back. A photo that errored and left the preview dead would be AGENTS
/// rule 8 broken by the code that exists to honour it, so the start is on every path out of
/// the work — the error path, and the unwinding path a panicking backend produces \[PF:1\],
/// which is why the resume is owned by a drop guard rather than written twice.
///
/// **The work's answer wins, always.** A resume that fails does not replace the photo's own
/// outcome — that is `crate::capture::StreamGuard`'s doctrine ("the caller is already holding
/// either a frame or the error that matters") — but unlike that one it is not discarded
/// either: it comes back in [`Gap::resumed`], where the daemon counts it, logs it and lets the
/// preview's own driver end the feed on its next turn rather than leaving a channel nobody
/// will ever publish to again. The one thing it must never be is a panic, on a path that is
/// holding a photo somebody asked for.
///
/// ## The bound
///
/// `budget_ms` is how long the caller's own work may take — for a photo, the settle deadline
/// it carries — and a budget over [`limits::PREVIEW_SUSPEND_MAX_MS`] is refused **before the
/// stream is stopped**, so a request too expensive to serve costs the viewers nothing at all.
/// Checking the budget rather than the elapsed time is the only shape that can refuse: nothing
/// can interrupt a `DQBUF` the actor's thread is already inside, so a limit consulted
/// afterwards would be a measurement wearing a bound's name.
///
/// # Errors
///
/// The work's, unchanged. [`Error::IllegalTransition`] when the caller's budget exceeds the
/// bound above. Whatever `stop_stream` refused with, in which case the work does not run at
/// all — a device that will not stop is not one this can take a frame from, and running the
/// capture anyway would be starting a second stream on a node that still holds the first.
pub fn while_suspended<T>(
    device: OpenCamera<'_>,
    budget_ms: u64,
    work: impl FnOnce(OpenCamera<'_>) -> Result<T>,
) -> (Result<T>, Option<Gap>) {
    let Some(stream) = device.streaming() else {
        return (work(device), None);
    };
    if budget_ms > limits::PREVIEW_SUSPEND_MAX_MS {
        return (
            Err(Error::IllegalTransition {
                from: "a camera whose preview stream is running".to_owned(),
                op: format!(
                    "hold that preview down for up to {budget_ms} ms, which is more than the \
                     {bound} ms a photo may",
                    bound = limits::PREVIEW_SUSPEND_MAX_MS
                ),
            }),
            None,
        );
    }
    if let Err(refused) = device.stop_stream() {
        return (Err(refused), None);
    }

    let mut resuming = Resuming {
        request: Some(resume_request(&stream)),
        device,
    };
    let outcome = work(&mut *resuming.device);
    let resumed = resuming.resume();
    (outcome, Some(Gap { stream, resumed }))
}

/// The request that puts a suspended stream back exactly as it was.
///
/// Every field the device already answered with, asked for by name rather than re-derived from
/// [`request`]. The difference matters on a camera whose preview was *adjusted* — D5 lets a
/// driver answer 1920×1080 to a 640×480 request, and re-asking the original would make the
/// second half of a preview a different size from the first, mid-`<img>`. `buffer_count` is
/// the default because that is what the preview's own request carried; it is not part of the
/// negotiated answer, so there is nothing to restore it from.
fn resume_request(stream: &NegotiatedStream) -> StreamRequest {
    StreamRequest {
        pixel_format: Some(stream.pixel_format),
        width: Some(stream.width),
        height: Some(stream.height),
        interval: Some(stream.interval),
        ..StreamRequest::default()
    }
}

/// Starts the stream again when it goes out of scope, however it goes out of scope.
///
/// `crate::capture::StreamGuard`'s mirror image and its counterpart: that one stops a stream
/// its scope started, this one starts a stream its scope stopped. The unwinding path is the
/// reason both are drop guards rather than lines at the end of a function — the most popular
/// V4L2 crate panics on a control type this kernel emits \[PF:1\], so "the work panicked" is a
/// measured failure mode, and a preview that ended because somebody pressed a photo button
/// would be indistinguishable from a camera that failed.
///
/// The difference from its counterpart is [`Resuming::resume`]: a stop that fails has nothing
/// left to tell anyone, and a *start* that fails leaves a feed that will never produce another
/// frame — so the ordinary path takes the request out of this guard, keeps the answer, and
/// leaves the guard with nothing to do.
struct Resuming<'device> {
    device: OpenCamera<'device>,
    /// The request to start with, taken by whoever resumes first.
    ///
    /// An `Option` is what makes the two mechanisms one: [`Resuming::resume`] empties it, so
    /// the drop that follows a successful resume starts nothing, and the drop that follows a
    /// panic finds it still full.
    request: Option<StreamRequest>,
}

impl Resuming<'_> {
    /// Start the stream again, once.
    fn resume(&mut self) -> Result<()> {
        match self.request.take() {
            Some(request) => self.device.start_stream(&request).map(|_negotiated| ()),
            // Already resumed. Reached only from the drop that follows the ordinary path,
            // where saying `Ok` is not a claim about the device — it is this guard reporting
            // that it had nothing left to do.
            None => Ok(()),
        }
    }
}

impl Drop for Resuming<'_> {
    fn drop(&mut self) {
        // The unwinding path, where there is no caller left to hand an error to: a panic is
        // already travelling and replacing it would lose the diagnosis. The stream is what
        // this can still put back, so it puts it back and says nothing.
        let _ = self.resume();
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
    use schema::backend::Camera as _;
    use schema::camera::PixelFormat;
    use schema::capture::{SettlePolicy, SettleSpec};

    use super::*;
    use crate::double::{ScriptedCamera, frame, integer};
    use crate::settle::SteppedClock;

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
        //
        // The device *offers* MJPG and answers YUYV, which is the only shape that still
        // reaches this branch: a camera that does not offer MJPG at all is refused by the
        // shared resolver before a stream is started (note **N134**), and that refusal is
        // the arm below rather than this one.
        let mut device = camera(4).answers_with(PixelFormat::YUYV);
        let refusal =
            start(&mut device, &request()).expect_err("YUYV bytes cannot be served as JPEG");
        assert_eq!(refusal.kind(), ErrorKind::FormatUnsupported);
        assert_eq!(device.stops, 1, "the refused stream was left running");

        // The neighbouring refusal, so the two are not confused: a camera with no MJPG at
        // all never starts a stream to stop.
        let mut device = ScriptedCamera::new(vec![integer("brightness", 50)]);
        let refusal = start(&mut device, &request())
            .expect_err("this camera enumerates YUYV and nothing else");
        assert_eq!(refusal.kind(), ErrorKind::FormatUnsupported);
        assert_eq!(
            device.stops, 0,
            "a request refused before the device was touched stopped a stream"
        );

        // And the other direction, so the assertion above is about the *format* rather than
        // about a function that refuses everything: the same camera, offering what a preview
        // can serve.
        let mut device = camera(4);
        let negotiated = start(&mut device, &request()).expect("MJPG is on offer");
        assert!(negotiated.pixel_format.is_compressed());
        assert_eq!(device.stops, 0);
    }

    #[test]
    fn a_camera_whose_smallest_mode_is_bigger_than_the_cap_still_previews_and_says_so() {
        // The OBSBOT, whose MJPG list starts at 1280×720: every mode it has is over the
        // 640×480 cap, so since 2026-08-16 the request `request()` builds is one the shared
        // resolver refuses (note **N134**). A cap that could refuse the only camera in the
        // room is not a cap, so `negotiate` asks again without it — and the answer is still
        // reported against the size that *was* asked for, because D5's reporting half is
        // what tells the owner why the picture is bigger than the preview asked for. The
        // double offers one mode, so "the largest that fits" and "the only one there is"
        // are the same answer here; which of them the second request should ask for on a
        // camera with several is the open decision `negotiate` writes down.
        let mut device = camera(4).sized(1_280, 720);
        let negotiated = start(&mut device, &request()).expect("a preview must still happen");

        assert_eq!((negotiated.width, negotiated.height), (1_280, 720));
        assert_eq!(
            negotiated.adjustments,
            vec![schema::capture::Adjustment::Size {
                requested_width: limits::PREVIEW_MAX_WIDTH,
                requested_height: limits::PREVIEW_MAX_HEIGHT,
                negotiated_width: 1_280,
                negotiated_height: 720,
            }],
            "the retry must not lose the difference the first request named"
        );
        assert_eq!(device.stops, 0);

        // The inverse, so the arm above is not measuring a function that ignores the cap: a
        // camera with a mode under it is answered at that mode, with nothing to report.
        let mut device = camera(4).sized(320, 240);
        let negotiated = start(&mut device, &request()).expect("320x240 fits under the cap");
        assert_eq!((negotiated.width, negotiated.height), (320, 240));
        assert!(
            negotiated
                .adjustments
                .iter()
                .any(|a| matches!(a, schema::capture::Adjustment::Size { .. })),
            "320x240 is not 640x480 and D5 reports the difference: {:?}",
            negotiated.adjustments
        );

        // And a format the camera does not have is *not* a cap and is not retried: the
        // refusal names MJPG, and asking a second time without a size cannot help.
        let mut device = ScriptedCamera::new(vec![integer("brightness", 50)]).sized(1_280, 720);
        let refusal = start(&mut device, &request()).expect_err("this camera has no MJPG");
        assert_eq!(refusal.kind(), ErrorKind::FormatUnsupported);
        assert_eq!(
            device.started.len(),
            0,
            "a format refusal was asked twice: {:?}",
            device.started
        );
    }

    #[test]
    fn a_cap_that_could_not_be_met_is_dropped_whole_rather_than_half_of_it() {
        // The retry above asks the device for **no size at all**, and both halves of the cap
        // have to go for that to be true. [`StreamRequest::choose`] reads a size only when
        // width *and* height are `Some` — "a half-specified size names nothing" — so a retry
        // that kept one of them resolves to the format's largest mode identically, and
        // `negotiate` then overwrites `adjustments` from the request that *did* name a size.
        // Nothing downstream can tell the two apart.
        //
        // Which is exactly why the line needed an assertion of its own rather than an
        // acceptance. Measured one dropped field at a time, at workspace scope: with either
        // `None` deleted the whole suite still passed **1494 of 1494** (note **N209**),
        // because every arm here read what came *back* and none read what was asked. The two
        // programs stop being the same the day the shared resolver honours half a size, which
        // is a rule written in `capture.rs` and not a law of arithmetic — and on that day a
        // preview would be asking a device for the width it had just been refused, silently.
        //
        // The double records the requests it accepted, which is the seam this reads; a
        // request `choose` refused is not among them, so the single entry here is the retry.
        let mut device = camera(4).sized(1_280, 720);
        start(&mut device, &request()).expect("a preview must still happen");

        assert_eq!(
            device.started.len(),
            1,
            "the cap was refused and the retry is the request that got through: {:?}",
            device.started
        );
        let retry = device.started.first().expect("the uncapped request");
        assert_eq!(
            (retry.width, retry.height),
            (None, None),
            "the retry kept half of the cap the device had just refused: {retry:?}"
        );
        // Nothing else about the request moved. The retry is the same question with the size
        // taken out of it, so a preview that named MJPG still names MJPG — without this the
        // assertion above would be satisfied by a retry that had thrown the request away.
        assert_eq!(retry.pixel_format, request().pixel_format);
        assert_eq!(retry.interval, request().interval);
        assert_eq!(retry.sink_fidelity, request().sink_fidelity);

        // The direction that keeps this arm about the *retry*: a camera with a mode under
        // the cap is answered on the first request, and that one names the cap in full.
        let mut device = camera(4).sized(320, 240);
        start(&mut device, &request()).expect("320x240 fits under the cap");
        let asked = device.started.first().expect("the capped request");
        assert_eq!(
            (asked.width, asked.height),
            (
                Some(limits::PREVIEW_MAX_WIDTH),
                Some(limits::PREVIEW_MAX_HEIGHT)
            ),
            "a cap that was met was not the cap: {asked:?}"
        );
    }

    /// A camera with a preview stream already running, and what it negotiated.
    ///
    /// Every suspension test starts here, because "something was streaming when this command
    /// began" is the whole precondition [`while_suspended`] branches on.
    fn previewing(frames: u32) -> (ScriptedCamera, NegotiatedStream) {
        let mut device = camera(frames);
        let negotiated = start(&mut device, &request()).expect("the scripted camera streams");
        (device, negotiated)
    }

    #[test]
    fn a_capture_during_a_preview_stops_the_stream_takes_its_frame_and_starts_it_again() {
        // The owner's ruling, as the sequence it is: STREAMOFF, the work's own stream,
        // STREAMON. The work here is `crate::capture::grab` rather than a stand-in, because
        // what a photo actually does is *start a stream of its own* — and this double
        // refuses a second `STREAMON` exactly as V4L2 does, so a build that skipped the
        // suspend would fail here with `Busy` rather than passing a test that only counted
        // stops.
        let (mut device, negotiated) = previewing(20);
        let clock = SteppedClock::new(0);
        let (captured, gap) = while_suspended(&mut device, 1_000, |device| {
            assert!(
                device.streaming().is_none(),
                "the capture ran while the preview still held the stream"
            );
            crate::capture::grab(
                device,
                &StreamRequest::default(),
                SettlePolicy {
                    spec: SettleSpec::SkipFrames { frames: 0 },
                    deadline_ms: 5_000,
                },
                &clock,
            )
        });

        captured.expect("the camera has frames");
        let gap = gap.expect("a preview was running, so there is a gap to report");
        assert_eq!(gap.stream, negotiated, "the gap names a stream nobody had");
        gap.resumed.expect("the stream came back");
        assert!(device.streaming().is_some(), "the preview was left stopped");
        // Three starts: the preview's, the capture's own, and the resume. Two stops: the
        // suspend and the capture guard's. A build that resumed twice, or not at all, has a
        // different pair of numbers here.
        assert_eq!(device.started.len(), 3);
        assert_eq!(device.stops, 2);
    }

    #[test]
    fn the_resume_asks_for_the_stream_that_was_suspended_rather_than_for_a_fresh_negotiation() {
        // D5 lets a driver answer with something other than what was asked for, so "start
        // the preview again" and "ask for a preview again" are different requests — and the
        // difference is a picture that changes size underneath a page that has already sized
        // an `<img>` around it. The resume names the negotiated answer, field for field.
        let (mut device, negotiated) = previewing(4);
        let (outcome, gap) = while_suspended(&mut device, 1_000, |_device| Ok(()));
        outcome.expect("the work does nothing and cannot fail");
        gap.expect("a preview was running")
            .resumed
            .expect("the stream came back");

        let resumed = device.started.last().expect("the resume started a stream");
        assert_eq!(resumed.pixel_format, Some(negotiated.pixel_format));
        assert_eq!(resumed.width, Some(negotiated.width));
        assert_eq!(resumed.height, Some(negotiated.height));
        assert_eq!(resumed.interval, Some(negotiated.interval));
        // ... and it is not the preview's own request, which asks for the *cap* rather than
        // for a size (`limits::PREVIEW_MAX_WIDTH`) and would be a different ask on any camera
        // whose preview was adjusted.
        assert_ne!(resumed.width, request().width);
    }

    #[test]
    fn a_capture_that_failed_still_leaves_the_preview_streaming() {
        // AGENTS rule 8 at the one place the code that exists to honour it could break it: a
        // photo that errors must not leave the camera dark for a tab that is still open. The
        // work's refusal travels unchanged, and the stream is back before the caller sees it.
        let (mut device, _negotiated) = previewing(4);
        let refusal = Error::DeviceIo {
            operation: "VIDIOC_DQBUF".to_owned(),
            errno: Some(5),
            message: "the capture failed".to_owned(),
        };
        let (outcome, gap) =
            while_suspended(&mut device, 1_000, |_device| Err::<(), _>(refusal.clone()));

        assert_eq!(
            outcome.expect_err("the work failed").kind(),
            ErrorKind::DeviceIo,
            "the work's own refusal was replaced"
        );
        gap.expect("a preview was running")
            .resumed
            .expect("the stream came back on the error path too");
        assert!(device.streaming().is_some());
        assert_eq!(device.started.len(), 2, "the resume did not happen");
    }

    #[test]
    fn a_capture_that_panicked_still_leaves_the_preview_streaming() {
        // The path a drop guard exists for and a line at the end of a function cannot reach.
        // It is not hypothetical: the most popular V4L2 crate panics on a control type this
        // kernel emits \[PF:1\], so "the work panicked" is a measured failure mode, and a
        // preview that ended because somebody pressed a photo button would be
        // indistinguishable from a camera that failed.
        let (mut device, _negotiated) = previewing(4);
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = while_suspended(&mut device, 1_000, |_device| -> Result<()> {
                panic!("a backend that panics on a control type it does not know");
            });
        }));

        assert!(panicked.is_err(), "the panic was swallowed");
        assert!(
            device.streaming().is_some(),
            "a panicking capture left the preview stopped"
        );
        assert_eq!(device.started.len(), 2);
    }

    #[test]
    fn a_resume_the_device_refuses_is_reported_and_never_replaces_the_photo() {
        // The failure with no good answer, given the least bad one: the photo succeeded and
        // the stream did not come back. Returning the resume's error would throw away a
        // picture the caller asked for and cannot retake (the frame is gone); returning `Ok`
        // and saying nothing would leave a channel nobody will ever publish to again. So the
        // work's answer is the answer, and the refusal rides beside it in the gap — where
        // `daemon::preview::interrupted` counts it and logs it, and the feed's own driver
        // ends the stream on its next turn.
        let mut device = camera(4).starts_refused_after(
            1,
            Error::DeviceIo {
                operation: "VIDIOC_STREAMON".to_owned(),
                errno: Some(12),
                message: "cannot allocate memory".to_owned(),
            },
        );
        start(&mut device, &request()).expect("the first start is allowed");

        let (outcome, gap) = while_suspended(&mut device, 1_000, |_device| Ok(7_u32));
        assert_eq!(outcome.expect("the work's own answer"), 7);
        let refused = gap
            .expect("a preview was running")
            .resumed
            .expect_err("the second start is refused");
        assert_eq!(refused.kind(), ErrorKind::DeviceIo);
        assert!(
            device.streaming().is_none(),
            "the double claimed a stream its own refusal did not start"
        );
    }

    #[test]
    fn a_budget_longer_than_the_bound_is_refused_before_the_stream_is_stopped() {
        // `limits::PREVIEW_SUSPEND_MAX_MS`, read by the only thing that can enforce it.
        // Nothing can interrupt a `DQBUF` the actor's thread is already inside, so the bound
        // is checked against the caller's declared budget — and the refusal costs the viewers
        // nothing at all, which is what the `stops` assertion says.
        let (mut device, _negotiated) = previewing(4);
        let ran = std::cell::Cell::new(false);
        let (outcome, gap) =
            while_suspended(&mut device, limits::PREVIEW_SUSPEND_MAX_MS + 1, |_| {
                ran.set(true);
                Ok::<(), Error>(())
            });

        assert_eq!(
            outcome.expect_err("the budget is over the bound").kind(),
            ErrorKind::IllegalTransition
        );
        assert!(!ran.get(), "the work ran after its budget was refused");
        assert!(gap.is_none(), "a refusal reported a gap it did not cause");
        assert_eq!(device.stops, 0, "a refused photo stopped the preview");
        assert!(device.streaming().is_some());

        // The other direction, so the assertion above is about the *bound* rather than about
        // a function that refuses everything: a budget exactly at it is served.
        let (outcome, gap) = while_suspended(&mut device, limits::PREVIEW_SUSPEND_MAX_MS, |_| {
            ran.set(true);
            Ok::<(), Error>(())
        });
        outcome.expect("a budget at the bound is within it");
        assert!(ran.get());
        assert!(gap.is_some());
    }

    #[test]
    fn a_capture_with_no_preview_running_touches_no_stream_at_all() {
        // "A photo with no preview running is unchanged", proven where the claim lives: the
        // early return is `work(device)` with nothing before or after it, so there is no stop
        // to skip and no start to add. The numbers are what a build that took the suspend
        // path anyway would move — `stops` on a node that is not streaming is harmless, and
        // the *start* that followed it would leave this camera streaming for nobody.
        let mut device = camera(4);
        let (outcome, gap) = while_suspended(&mut device, 1_000, |device| {
            device.start_stream(&StreamRequest::default())?;
            device.stop_stream()
        });

        outcome.expect("the work runs exactly as it always did");
        assert!(gap.is_none(), "a gap was reported with nobody watching");
        assert_eq!(
            device.started.len(),
            1,
            "the work's own stream, and no other"
        );
        assert_eq!(device.stops, 1, "the work's own stop, and no other");
        assert!(
            device.streaming().is_none(),
            "a photo with no preview left the camera streaming"
        );
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
