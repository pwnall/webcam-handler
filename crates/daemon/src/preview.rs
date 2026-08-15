//! The latest-frame fan-out: one camera, one stream, N readers, and no queue anywhere
//! (design **D12**, §2.6; docs/7 P5b).
//!
//! `crate::events` is this module's sibling and the comparison is worth making, because the
//! two solve the same shape and reach opposite answers. That one fans a *sequence* out to
//! subscribers and must not lose a value silently, so it is a `broadcast` with a depth and a
//! counted lag. This one fans out **frames**, where the newest value makes every older one
//! worthless, so it is a `watch`: one slot, overwritten in place, and a reader that was busy
//! finds the current picture rather than a backlog of stale ones.
//!
//! ## Where the channel lives, and why it is not in the engine
//!
//! Design D12 says the fan-out "happens inside the actor via a latest-frame `watch` channel".
//! The first half of that sentence is honoured literally and the second cannot be, and the
//! difference is worth stating rather than quietly resolving.
//!
//! **Inside the actor** is true of the *publishing*: every frame this module hands out was
//! taken by `engine::preview::turn` running on the camera's own OS thread, with the device
//! open and nothing else touching it, and it is put into the channel from that thread before
//! the command returns. There is no second thread reading frames, no copy of the device
//! handle, and no path by which two things dequeue from one camera — which is the property
//! D12's paragraph is about.
//!
//! **The `watch` itself cannot live in the engine**, because `tokio::sync::watch` is tokio and
//! design §2.8 gives the engine no runtime — a wall `scripts/gates/dependency-walls.sh`
//! enforces as a linkage fact rather than a paragraph. This is the same fork note **N41** took
//! for the actor's *reply* channel and for the same reason ("a `tokio::sync::oneshot` in the
//! signature would force a runtime on `webcam-handler-cli`"), so the resolution is the same
//! one: the engine names a trait (`engine::preview::FrameSink`), the caller brings the
//! channel, and the channel is here because here is where the transport is. A
//! `std::sync::mpsc` in the engine would satisfy the letter and lose the point — it is a
//! queue, and a queue is precisely what D12's sentence rules out.
//!
//! ## The property, proved rather than restated
//!
//! D12's claim is that "a slow HTTP consumer drops frames and never backpressures the
//! device". `tokio::sync::watch` gives that by construction, and the argument has three steps
//! and no timing in it:
//!
//! 1. **`Sender::send_replace` overwrites and returns.** It takes the lock, replaces the one
//!    value, notifies, and returns — with no receiver, with one, or with four, and whether or
//!    not any of them has read the value it is replacing. There is no capacity to be full and
//!    therefore no state in which a sender waits. `Publisher::publish` is called from the
//!    camera's actor thread and cannot be made to block by anything a reader does.
//! 2. **A receiver that reads late reads the newest value.** `Receiver::borrow_and_update`
//!    answers with what is in the slot *now*, not with what was there when it was last
//!    notified, so the frames a reader was too slow for are not queued anywhere: they were
//!    overwritten, which is to say they never existed as far as memory is concerned.
//! 3. **What it missed is therefore countable but not recoverable**, which is the honest
//!    shape of a drop. Every value carries a monotonic [`Shot::index`], so a reader that goes
//!    from index 7 to index 300 knows it skipped 292 — and [`Viewer::next`] adds exactly that
//!    difference to [`Previews::watch_skipped`], because rubric rule 3 wants a number rather
//!    than a silence.
//!
//! Step 1 is the one a mutant reaches: an `mpsc` with any depth passes every test that reads
//! frames and fails `crates/daemon/tests/preview.rs`'s stalled-reader test, because a bounded
//! channel makes the *device* wait for a socket and an unbounded one makes the daemon hold
//! every frame a stalled tab did not read.
//!
//! ## Who starts the capture, and who stops it
//!
//! D12 again: the daemon "never opens a camera until first use and closes on idle". A preview
//! tab is a use, so:
//!
//! - **The first viewer starts it.** [`Previews::attach`] finds no feed for that camera,
//!   creates one, and spawns the driver that issues `engine::preview::start`.
//! - **The second viewer starts nothing.** It finds the feed and subscribes to the same
//!   channel. That is what makes "two tabs on one camera are not two streamers" structural
//!   rather than lucky: the registry is consulted under a lock, and the only code that starts
//!   a stream is the code that inserted an entry into it. V4L2 would have refused the second
//!   `STREAMON` with `EBUSY` anyway — but it would have refused it *to the second tab*, which
//!   would then have failed for a reason that is about the daemon rather than about the
//!   machine.
//! - **The last viewer stops it.** The driver checks the channel's receiver count between
//!   frames; when it reaches zero it takes the registry lock, re-checks under it (so a viewer
//!   arriving in that instant wins rather than races), removes the feed, stops the stream and
//!   ends. The camera is then *open and unused*, and closes on the ordinary idle sweep
//!   `limits::CAMERA_IDLE_CLOSE_MS` describes — this module closes no camera, which is what
//!   keeps "when does a camera close" one rule (D12) rather than two.
//! - **The daemon stops it too.** Every driver watches `crate::shutdown::Shutdown`, and so
//!   does `Publisher::publish` on the actor thread, so a cancellation ends both the loop and
//!   the frame that is being taken when it arrives. AGENTS: open streams are "cancelled, never
//!   awaited".
//!
//! ## What a preview costs the camera while it runs, stated because it is a real cost
//!
//! One streamer per node is the kernel's rule, so a verb that needs a stream of its own has to
//! get the preview's out of the way. Since the owner's ruling of 2026-08-12 (note **N83**)
//! `wch_photo` does exactly that and this module does **nothing** to arrange it:
//! `engine::photo::take` stops the stream, takes its frame and starts it again, inside the one
//! actor command that already owns the device, so there is no protocol here, no flag on a
//! `Feed` and no window in which this registry's opinion could disagree with the device's.
//! What this module does is the half only it can do — *count* the interruption
//! ([`Previews::interrupted`]) and say what a reader should make of the frames either side of
//! it.
//!
//! A **calibration sweep** is still refused with [`schema::Error::Busy`], and that is a scope
//! decision rather than an oversight: a sweep is minutes of photos, so suspending a preview
//! for one would be a preview that is off rather than paused, and the bound that makes the
//! photo's pause defensible (`limits::PREVIEW_SUSPEND_MAX_MS`) is precisely what a sweep
//! cannot fit inside. The refusal stays E3-correct — availability is not capability, and it
//! names the node.
//!
//! Control reads and writes are unaffected — `VIDIOC_S_EXT_CTRLS` on a streaming node is
//! ordinary — and they wait at most one `limits::PREVIEW_FRAME_WAIT_MS` behind the preview,
//! because the preview holds the actor for one frame at a time (`engine::preview`'s header
//! argues that shape).
//!
//! ## What a viewer sees across a photo, and what it may conclude from it
//!
//! The pause drops nothing, which is the surprising part: a frame that was never captured was
//! never published, so this daemon's publication count does not move and no reader falls
//! behind. The three things that *are* visible, in the order a client meets them:
//!
//! 1. **Frames stop arriving and then start again.** The gap is the length of one capture,
//!    bounded by `limits::PREVIEW_SUSPEND_MAX_MS` and in practice a settle.
//! 2. **The pause puts no hole in [`Shot::index`]** — nothing was published, so there is
//!    nothing to skip. A reader may still see a jump there for its *own* reason (it was slow,
//!    and [`Viewer::next`] counted it), which is exactly why the two numbers are kept apart:
//!    the index measures this reader against this daemon and says nothing about the device.
//! 3. **[`Shot::sequence`] starts over at zero.** `STREAMON` resets the driver's own counter,
//!    so the frame after a photo is sequence 0 again. **This is the one thing a client can
//!    read the pause off**, and it is left as the device reports it rather than rewritten into
//!    a continuation, because that field's whole contract is that it is the *device's* number
//!    (D5: the device is the authority on itself). A client that treats a sequence going
//!    backwards as "the stream restarted" is reading it correctly; one that treats it as a
//!    kernel drop is not, and the two are distinguishable precisely because a kernel drop
//!    moves the sequence *forward* past frames that were lost.
//!
//! What no client can conclude is *why* the stream restarted, and there is nothing dishonest
//! in that: from a viewer's side "a photo was taken" and "the device restarted its stream" are
//! the same event, and the daemon's own count is where the difference is recorded.
//!
//! ## No frame reaches a log
//!
//! A frame may contain a person (AGENTS, design §5). [`Shot`] holds bytes and prints their
//! *count*, exactly as `schema::capture::Frame` does; every `tracing` call in this file names
//! a camera, a count or a reason and never a frame; and nothing here writes a byte to a path.
//! The bytes' only destinations are the one `watch` slot and a socket the operator opened.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use engine::actor::{CameraActor, Cameras, OpenCamera};
use engine::preview::{Delivery, Demand, FrameSink};
use engine::settle::{Clock, MonotonicClock};
use schema::camera::{CameraId, CameraInfo};
use schema::capture::Frame;
use schema::{Error, ErrorKind, Result, limits};
use tokio::sync::{Mutex, oneshot, watch};

use crate::shutdown::Shutdown;

/// Every camera this daemon is currently previewing, and the fan-out for each.
///
/// One per process, held by `crate::server::Wchd` and handed to the web listener as a value —
/// which is the same arrangement `Methods` has, and for the same reason: the listener composes
/// transports and decides nothing about cameras.
///
/// Cheap to clone (an `Arc` bump) and cloned deliberately: the driver task for each feed holds
/// one, because it outlives the request that started it.
#[derive(Debug, Clone)]
pub struct Previews(Arc<Fanout>);

/// The shared state behind [`Previews`]. One of each, never two.
#[derive(Debug)]
struct Fanout {
    /// The registry the actors come from — **the same one** `crate::server::Wchd` answers
    /// every other verb out of, behind an `Arc` so that both readers reach one map of actors.
    /// Two registries over one backend would be two threads on one node, which is the thing
    /// `engine::actor::Cameras` exists to make unrepresentable.
    cameras: Arc<Cameras>,
    /// A **clone** of the daemon's clock, not a second one. `engine::settle::Millis` are
    /// milliseconds since a clock's own origin and comparing two clocks' readings is a bug
    /// (that module says so), so a preview that stamped its commands from a clock of its own
    /// would be feeding the actor's idle deadline numbers from another timeline.
    clock: MonotonicClock,
    /// The daemon's one stop token — watched by every driver and by every publish.
    shutdown: Shutdown,
    /// One feed per camera being previewed, and the lock that makes "one" true.
    ///
    /// A `tokio::sync::Mutex` because [`Previews::attach`] holds it across an `await` — the
    /// map operations themselves are trivial, but the decision "does this camera already have
    /// a feed?" has to be atomic with the insert that answers it, and the retire path takes
    /// the same lock to re-check under it.
    ///
    /// **Nothing device-shaped happens under this lock.** The stream is started by the driver
    /// task after the lock is released, and the answer travels back over a `oneshot`, so a
    /// camera that is inside a minutes-long sweep delays the preview that asked for *it* and
    /// no other camera's.
    live: Mutex<BTreeMap<CameraId, Arc<Feed>>>,
    /// How many cameras are being previewed, as something to **await**.
    ///
    /// The same pair `crate::server::Waiters` keeps and for the same reason: the map is what
    /// *enforces* one feed per camera, and this is that count in the one shape a test can wait
    /// on. "The last tab closed and the capture stopped" is an event, and a test that polled
    /// the map for it would be a test with a sleep in it (AGENTS). It is written under the
    /// same lock the map is edited under, so the two cannot disagree about a change; it is
    /// *read* without the lock, which is what makes it observable from a shutdown path that
    /// must not block.
    feeds: watch::Sender<usize>,
    /// How many frames this daemon has published, ever.
    ///
    /// A `watch` rather than a counter for `crate::events::Events::lost`'s reason, restated
    /// because it is what makes the stalled-reader test possible without a clock: the count
    /// can be read at any moment, but only a *change* can be waited for, and a test that
    /// polled a counter would be a test with a sleep in it under another name (AGENTS).
    published: watch::Sender<u64>,
    /// How many published frames a reader never saw, summed over every reader.
    ///
    /// `Fanout::published`'s sibling and the number that says "drop" rather than
    /// "backpressure": with a queue in place of the `watch` this can only ever be zero, since
    /// a queue delivers everything it accepted. Rubric rule 3 — a loss is counted, never
    /// silent.
    skipped: watch::Sender<u64>,
    /// How many times a photo has stopped a live preview's stream and started it again.
    ///
    /// **The pause's own number, and it has to be its own** — a suspension is not a
    /// [`Fanout::skipped`]: nothing was published that a reader missed, so folding it into
    /// that count would tell an operator their socket was slow when their camera was busy
    /// being a camera. The two are different facts about the same second (E3's habit, applied
    /// to a count rather than to an error).
    ///
    /// A `watch` for [`Fanout::published`]'s reason, one step further: the interruption is an
    /// *event*, it is the only observable the pause has, and a test that polled for it would
    /// be a test with a sleep in it (AGENTS).
    interrupted: watch::Sender<u64>,
}

/// One camera's stream, as everything reading it sees it.
#[derive(Debug)]
struct Feed {
    /// Which camera, for the log lines and for the registry key the driver removes.
    camera: CameraId,
    /// **The latest-frame channel.** One slot, overwritten per frame; the receiver count is
    /// the viewer count, which is why nothing here keeps a second tally of viewers.
    frames: watch::Sender<Option<Arc<Shot>>>,
    /// `Fanout::published`, cloned so the actor thread can bump it without reaching back
    /// through a structure that holds this one. A `watch::Sender` clone is another handle on
    /// the same value, not a second value.
    published: watch::Sender<u64>,
    /// How many frames this feed refused for being larger than
    /// [`limits::PREVIEW_MAX_FRAME_BYTES`].
    ///
    /// Counted rather than logged per frame: a driver that hands back oversized buffers hands
    /// back one per frame, and a log line per frame is how a daemon fills a journal with a
    /// camera's frame rate.
    oversized: AtomicU64,
}

/// One published frame: the camera's own bytes, and where it sits in the sequence.
///
/// Behind an `Arc` in the channel, so handing it to four viewers is four pointer bumps and one
/// copy of the bytes for the whole daemon.
pub struct Shot {
    /// This daemon's publication count at the moment it was published, starting at one.
    ///
    /// The number a reader's skip is computed from, and the one the multipart part carries so
    /// that a *client* can see the same gap the daemon counted. Per daemon rather than per
    /// feed because there is one of it and a reader compares it only with itself.
    index: u64,
    /// The driver's own sequence number for this frame — gaps here mean the *kernel* dropped
    /// frames, which is a different fact from the one [`Shot::index`] measures and is worth
    /// keeping distinct (D5: the device is the authority on itself).
    sequence: u32,
    /// The frame, exactly as the camera delivered it: a JPEG bitstream, never decoded, never
    /// re-encoded (AGENTS' "verbatim camera JPEG when the sink allows").
    bytes: Vec<u8>,
}

impl std::fmt::Debug for Shot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // A frame may contain a person. Never the bytes — `schema::capture::Frame` states the
        // same rule one crate down, and this type exists on the other side of the boundary
        // that copy was made across.
        f.debug_struct("Shot")
            .field("index", &self.index)
            .field("sequence", &self.sequence)
            .field("bytes", &format_args!("<{} bytes>", self.bytes.len()))
            .finish()
    }
}

impl Shot {
    /// This daemon's publication count at the moment this frame was published.
    #[must_use]
    pub fn index(&self) -> u64 {
        self.index
    }

    /// The driver's sequence number for this frame.
    #[must_use]
    pub fn sequence(&self) -> u32 {
        self.sequence
    }

    /// The frame's bytes — a JPEG bitstream, as the camera produced it.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// One published frame, assembled by hand.
///
/// The one constructor outside `Publisher::publish`, and it is `#[cfg(test)]` because the
/// only thing that should ever make a [`Shot`] is a camera. `crate::http::preview`'s framing
/// is a pure function over this type, and a test of that framing needs a value to frame.
#[cfg(test)]
pub(crate) fn shot(index: u64, sequence: u32, bytes: Vec<u8>) -> Shot {
    Shot {
        index,
        sequence,
        bytes,
    }
}

/// One reader of one camera's feed.
///
/// Held by the task that writes a `multipart/x-mixed-replace` response
/// (`crate::http::preview`). Dropping it is how a client that went away stops the capture:
/// the receiver goes with it, the feed's receiver count falls, and the driver notices between
/// frames.
#[derive(Debug)]
pub struct Viewer {
    frames: watch::Receiver<Option<Arc<Shot>>>,
    /// The last index this reader was handed, or `None` before its first frame.
    ///
    /// The skip count is computed from this rather than from a counter the publisher keeps,
    /// because "how far behind is *this* reader" is a fact about one reader — two viewers of
    /// one feed fall behind by different amounts, and a per-feed number could not say so.
    seen: Option<u64>,
    /// `Fanout::skipped`, for the drops this reader's own slowness caused.
    skipped: watch::Sender<u64>,
}

impl Previews {
    /// A fan-out over `cameras`, stamping its commands from `clock` and stopping with
    /// `shutdown`.
    ///
    /// Every argument is a value the composition root already made. In particular the clock
    /// is the daemon's own (see `Fanout::clock`) and the token is the daemon's one
    /// [`Shutdown`], not a second one — `crate::shutdown::Shutdown`'s header is where that
    /// distinction is argued.
    #[must_use]
    pub fn new(cameras: Arc<Cameras>, clock: MonotonicClock, shutdown: Shutdown) -> Previews {
        Previews(Arc::new(Fanout {
            cameras,
            clock,
            shutdown,
            live: Mutex::new(BTreeMap::new()),
            feeds: watch::Sender::new(0),
            published: watch::Sender::new(0),
            skipped: watch::Sender::new(0),
            interrupted: watch::Sender::new(0),
        }))
    }

    /// Record that a photo stopped `camera`'s preview stream and started it again.
    ///
    /// Called by `crate::server`'s `wch_photo` on **both** of its paths — a photo that
    /// succeeded and a photo that failed — because the interruption happened either way and a
    /// gap counted only when the picture came out would under-report exactly the case an
    /// operator would be investigating (`engine::photo::Taken` is shaped to make that
    /// possible). It is called after the actor's command has returned, from the request's own
    /// task; nothing here touches a device.
    ///
    /// **A resume that failed is a warning and not a second mechanism.** The feed is left
    /// exactly where it is: its driver's next turn asks a camera that is no longer streaming
    /// for a frame, gets the device's own refusal, and takes the path this module already has
    /// for a device that refused — `Ended::Refused`, the feed withdrawn, its readers' streams
    /// ended, one `tracing::warn!`. That happens within one `limits::PREVIEW_FRAME_WAIT_MS`,
    /// which is why withdrawing the feed *here* would be a second home for "how a preview
    /// ends" (§2.10) rather than a faster answer. The line below exists because the driver's
    /// warning names the symptom and this one names the cause.
    pub fn interrupted(&self, camera: &CameraId, gap: &engine::preview::Gap) {
        self.0
            .interrupted
            .send_modify(|count| *count = count.saturating_add(1));
        if let Err(err) = &gap.resumed {
            tracing::warn!(
                %camera,
                error = %err,
                "a photo stopped the preview stream and the camera refused to start it again"
            );
        }
    }

    /// How many times a photo has interrupted a preview, as something to **await**.
    ///
    /// [`Previews::watch_published`]'s reason on the third of this module's counts. It is the
    /// pause's only observable: no frame is lost to it, so a build that stopped resuming — or
    /// one that stopped suspending and answered `Busy` again — is invisible to every other
    /// number here.
    #[must_use]
    pub fn watch_interrupted(&self) -> watch::Receiver<u64> {
        self.0.interrupted.subscribe()
    }

    /// How many frames this daemon has published, as something to **await**.
    ///
    /// The *capture* side of docs/7 P5b's stalled-reader criterion: this advances while a
    /// reader's own count does not, which is the difference between dropping and
    /// backpressuring. A `watch` for `Fanout::published`'s reason.
    #[must_use]
    pub fn watch_published(&self) -> watch::Receiver<u64> {
        self.0.published.subscribe()
    }

    /// How many published frames a reader never saw, as something to **await**.
    ///
    /// The number that can only move if frames are being *dropped*: a queue in place of the
    /// latest-frame channel delivers everything it took, so it would leave this at zero
    /// forever while every other assertion in the preview suite still passed.
    #[must_use]
    pub fn watch_skipped(&self) -> watch::Receiver<u64> {
        self.0.skipped.subscribe()
    }

    /// The daemon's one stop token, for the response body that carries these frames.
    ///
    /// `crate::http::preview`'s writer has to watch it *itself* rather than rely on the feed
    /// ending: a client that stopped reading leaves that task parked on a socket write, and a
    /// task parked on a socket write is exactly what "an open MJPEG tab must not hang
    /// shutdown" (design §2.6) is about. Reached here rather than cloned into every
    /// [`Viewer`], because the token has one home and this is a second reader of it.
    pub(crate) fn shutdown(&self) -> &Shutdown {
        &self.0.shutdown
    }

    /// How many cameras are being previewed right now.
    ///
    /// The observable for the lifecycle claim above — the first viewer creates a feed and the
    /// last one to leave takes it away — and it is a count of *feeds*, so a build in which two
    /// tabs became two streams would answer two here as well as opening two devices.
    #[must_use]
    pub fn feeds(&self) -> usize {
        *self.0.feeds.borrow()
    }

    /// The same count, as something to **await**.
    ///
    /// [`Previews::watch_published`]'s reason: a feed being retired is an event, and the only
    /// honest way to wait for an event is to be told about it.
    #[must_use]
    pub fn watch_feeds(&self) -> watch::Receiver<usize> {
        self.0.feeds.subscribe()
    }

    /// Start reading `requested`'s preview, starting the capture if this is the first reader.
    ///
    /// The resolution is `engine::resolve::camera`'s — the same function `webcam-handler-cli`,
    /// `webcam-handler-client` and every other daemon verb reach, so a prefix means the same
    /// thing on this route as it does on the wire (D1) — and it runs on a blocking pool thread
    /// because enumeration is an `open` and a walk of ioctls per node (E2: live every time,
    /// never cached).
    ///
    /// **The registry lock is not held across anything device-shaped.** The feed is created
    /// and inserted under it; the `STREAMON` is the driver's, after the lock is released, and
    /// its answer comes back over a `oneshot` this function awaits. So a second tab on the
    /// same camera is answered from the map immediately, and a first tab on a camera that is
    /// inside a sweep waits for that camera and blocks no other.
    ///
    /// # Errors
    ///
    /// [`Error::CameraUnknown`] or [`Error::CameraAmbiguous`] from the resolution (D1's rule,
    /// not this module's). [`Error::Busy`] when the camera already has
    /// [`limits::PREVIEW_MAX_VIEWERS_PER_CAMERA`] readers, or when its actor's command queue
    /// is full. [`Error::FormatUnsupported`] when the device negotiates something a browser
    /// cannot render (`engine::preview::start`). [`Error::DeviceIo`] when an actor thread
    /// cannot be started, and whatever the device refused `STREAMON` with otherwise.
    pub async fn attach(&self, requested: &CameraId) -> Result<Viewer> {
        let info = self.resolve(requested).await?;
        let (feed, viewer, owes_a_start) = self.reserve(info.clone()).await?;
        if owes_a_start.is_none() {
            // A feed that was already there: this reader is the second tab, and there is
            // nothing to wait for. It joins the stream in progress and sees the next frame.
            return Ok(viewer);
        }

        let actor = match self.0.cameras.actor(&info, self.0.clock.now_ms()) {
            Ok(actor) => actor,
            Err(err) => {
                // A feed was inserted and nothing is ever going to drive it — an actor thread
                // that could not be spawned is this process failing to compose itself, and
                // leaving the entry behind would make every later viewer of that camera
                // subscribe to a channel with no publisher.
                self.withdraw(&feed).await;
                return Err(err);
            }
        };
        let (answered, answer) = oneshot::channel();
        tokio::spawn(drive(self.clone(), actor, feed, answered));
        match answer.await {
            Ok(Ok(())) => Ok(viewer),
            Ok(Err(err)) => Err(err),
            // The driver task went away without answering, which is a runtime that is shutting
            // down. Not a device refusal, and not spelled like one (E3).
            Err(_) => Err(Error::DeviceIo {
                operation: format!("start the preview stream for {camera}", camera = info.id),
                errno: None,
                message: "the preview driver ended before the stream started".to_owned(),
            }),
        }
    }

    /// Resolve a caller-supplied id or prefix against a live enumeration (D1, E2).
    async fn resolve(&self, requested: &CameraId) -> Result<CameraInfo> {
        let cameras = Arc::clone(&self.0.cameras);
        let requested = requested.clone();
        match tokio::task::spawn_blocking(move || {
            let seen = cameras.enumerate()?;
            engine::resolve::camera(&seen, &requested).cloned()
        })
        .await
        {
            Ok(answer) => answer,
            Err(err) => Err(Error::DeviceIo {
                operation: "enumerate cameras for a preview".to_owned(),
                errno: None,
                message: err.to_string(),
            }),
        }
    }

    /// Take a place on `info`'s feed, creating the feed if there is not one.
    ///
    /// Answers the feed, the viewer, and — **only when this call is the one that created it** —
    /// a witness that the caller owes the stream a start. A `bool` would carry the same
    /// information and would let a caller ignore it; this is the shape that says which call
    /// is responsible.
    async fn reserve(&self, info: CameraInfo) -> Result<(Arc<Feed>, Viewer, Option<Starting>)> {
        let mut live = self.0.live.lock().await;
        if let Some(feed) = live.get(&info.id) {
            // The second tab. The bound is read off the channel's own receiver count rather
            // than a tally kept beside it, because a tally is a second answer to "how many
            // readers" and the channel already has the only one that cannot drift.
            if feed.frames.receiver_count() >= limits::PREVIEW_MAX_VIEWERS_PER_CAMERA {
                return Err(Error::Busy {
                    path: node_of(&info),
                    holders: Vec::new(),
                });
            }
            let viewer = self.viewer(feed);
            return Ok((Arc::clone(feed), viewer, None));
        }

        let feed = Arc::new(Feed {
            camera: info.id.clone(),
            frames: watch::Sender::new(None),
            published: self.0.published.clone(),
            oversized: AtomicU64::new(0),
        });
        // Subscribed **before** the lock is released, so the driver this call is about to
        // spawn can never see a feed with no readers and retire it before its first frame.
        let viewer = self.viewer(&feed);
        live.insert(info.id.clone(), Arc::clone(&feed));
        self.0.feeds.send_replace(live.len());
        Ok((feed, viewer, Some(Starting)))
    }

    /// One reader of `feed`.
    fn viewer(&self, feed: &Arc<Feed>) -> Viewer {
        Viewer {
            frames: feed.frames.subscribe(),
            seen: None,
            skipped: self.0.skipped.clone(),
        }
    }

    /// Remove `feed` from the registry when nobody is reading it any more.
    ///
    /// Answers whether it did. The re-check under the lock is the whole function: a viewer
    /// that arrives between the driver's count and this call finds the feed still in the map
    /// and subscribes to it, and this call then sees its receiver and declines — so the race
    /// is resolved in favour of the reader that wants a camera, which is the direction a
    /// preview should err in.
    async fn retire(&self, feed: &Arc<Feed>) -> bool {
        let mut live = self.0.live.lock().await;
        if feed.frames.receiver_count() > 0 {
            return false;
        }
        remove(&mut live, feed);
        self.0.feeds.send_replace(live.len());
        true
    }

    /// Remove `feed` from the registry whatever its readers are doing.
    ///
    /// The path out of the driver loop that is not `Previews::retire` — a cancelled daemon,
    /// a camera that stopped delivering, a device that refused. Readers that are still
    /// attached get the end of their stream when the feed's channel is dropped, which is the
    /// only honest thing left to give them.
    async fn withdraw(&self, feed: &Arc<Feed>) {
        let mut live = self.0.live.lock().await;
        remove(&mut live, feed);
        self.0.feeds.send_replace(live.len());
    }

    /// One frame's worth of device work, on the actor's thread.
    async fn turn(&self, actor: &CameraActor, feed: &Arc<Feed>) -> Result<Delivery> {
        let sink = Publisher {
            feed: Arc::clone(feed),
            shutdown: self.0.shutdown.clone(),
        };
        self.ask(actor, move |device| engine::preview::turn(device, &sink))
            .await
    }

    /// Run `work` on `actor`'s thread and wait for its answer.
    ///
    /// The N41 shape, spelled here because N41's answer *is* that every caller spells it: the
    /// actor names no reply channel, so a caller brings one whose `send` is synchronous and
    /// non-blocking. `crate::server` writes the same six lines around its own `oneshot` for
    /// the same reason, and neither is a copy of a rule — the rule is that the engine holds no
    /// runtime, and this is what it looks like from a caller that has one.
    ///
    /// The command is stamped from the daemon's clock on the way in, which is what keeps a
    /// camera with a live preview from ever looking idle to the sweep.
    async fn ask<T, F>(&self, actor: &CameraActor, work: F) -> Result<T>
    where
        F: FnOnce(OpenCamera<'_>) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let (answered, answer) = oneshot::channel::<Result<T>>();
        actor.submit(self.0.clock.now_ms(), move |device| {
            let outcome = device.and_then(work);
            engine::actor::answering(move || {
                // Nobody left to tell: the driver task ended while this frame was being
                // taken, which is what a cancelled daemon looks like from the actor's side.
                let _ = answered.send(outcome);
            })
        })?;
        match answer.await {
            Ok(answer) => answer,
            Err(_) => Err(actor.device_gone()),
        }
    }
}

/// Remove `feed` from `live`, and only if the entry under its key is still this one.
///
/// `Arc::ptr_eq` rather than a key comparison, because a feed that was withdrawn and then
/// re-created by a new viewer has the same key and is a different stream — and a driver that
/// removed the *new* feed on its way out would leave a camera streaming with a registry that
/// says nobody is previewing it.
fn remove(live: &mut BTreeMap<CameraId, Arc<Feed>>, feed: &Arc<Feed>) {
    if live
        .get(&feed.camera)
        .is_some_and(|entry| Arc::ptr_eq(entry, feed))
    {
        live.remove(&feed.camera);
    }
}

/// The node a refusal about `info` names.
///
/// A camera with no capture node cannot be previewed at all, so this path is reached only by a
/// refusal that has to name *something*; naming the camera beats inventing a device node.
/// `engine::actor::CameraActor::spawn` makes the same choice for the same reason.
///
/// `pub(crate)` since P6c, for `crate::record`'s two `Busy` refusals: "which path does a
/// refusal about this camera name" is one question, and a second copy of the fallback would
/// let a `record_start` and a `preview` name different things about one device (design
/// §2.10).
pub(crate) fn node_of(info: &CameraInfo) -> camino::Utf8PathBuf {
    info.capture_node().map_or_else(
        || camino::Utf8PathBuf::from(info.id.as_str()),
        |node| node.path.clone(),
    )
}

/// The witness that a caller owes a feed its `STREAMON`.
///
/// A unit type rather than a `bool` so that the responsibility travels in the type: the only
/// way to get one is to be the call that inserted the feed.
#[derive(Debug)]
struct Starting;

/// Why a driver loop ended.
#[derive(Debug)]
enum Ended {
    /// The last reader went away.
    Unwatched,
    /// The daemon is stopping.
    ShuttingDown,
    /// The camera delivered nothing for [`limits::PREVIEW_MAX_EMPTY_TURNS`] turns.
    Silent,
    /// The camera's command queue was full for that many turns — it is busy with something
    /// else, which is a different fact from a camera that has stopped delivering (E3).
    Deferred,
    /// The device refused.
    Refused(Error),
}

impl Ended {
    /// The word a log line carries. A fixed vocabulary rather than a rendered `Debug`, so an
    /// operator grepping a journal has something stable to grep for.
    fn name(&self) -> &'static str {
        match self {
            Ended::Unwatched => "the last viewer left",
            Ended::ShuttingDown => "the daemon is shutting down",
            Ended::Silent => "the camera stopped delivering frames",
            Ended::Deferred => "the camera was busy with other work",
            Ended::Refused(_) => "the device refused",
        }
    }
}

/// One camera's preview, from `STREAMON` to `STREAMOFF`.
///
/// A task rather than a loop inside the request handler, because the stream outlives any one
/// reader: the second tab must find a running feed, and the first tab closing must not end a
/// stream the second is watching. It ends by removing its feed from the registry **before**
/// stopping the stream, so no viewer can attach to a feed whose device is on its way to
/// `STREAMOFF`.
async fn drive(
    previews: Previews,
    actor: Arc<CameraActor>,
    feed: Arc<Feed>,
    started: oneshot::Sender<Result<()>>,
) {
    let camera = feed.camera.clone();
    if let Err(err) = previews
        .ask(&actor, move |device| {
            engine::preview::start(device, &engine::preview::request()).map(|_| ())
        })
        .await
    {
        // The feed never carried a frame, so the reader that asked for it gets the device's
        // own words rather than an empty `200` — see `crate::http::preview` for what that
        // becomes on the wire.
        previews.withdraw(&feed).await;
        let _ = started.send(Err(err));
        return;
    }
    let _ = started.send(Ok(()));

    let ended = pump(&previews, &actor, &feed).await;
    previews.withdraw(&feed).await;
    if let Err(err) = previews.ask(&actor, engine::preview::stop).await {
        // The camera is open and not streaming for anybody, and the descriptor closes on the
        // ordinary idle sweep — which stops the stream whatever this said. Worth a line
        // because it names a device that is not behaving, and not worth an escalation because
        // there is no client waiting on it.
        tracing::debug!(%camera, error = %err, "the preview stream could not be stopped");
    }

    match &ended {
        Ended::Refused(err) => {
            tracing::warn!(%camera, error = %err, reason = ended.name(), "the preview stream ended");
        }
        _ => {
            tracing::debug!(
                %camera,
                reason = ended.name(),
                oversized = feed.oversized.load(Ordering::Relaxed),
                "the preview stream ended"
            );
        }
    }
}

/// The loop: one frame per turn, until something says stop.
///
/// Split from [`drive`] so that "why did this preview end" is a value with a name rather than
/// a `break` in the middle of a function that also starts and stops streams.
async fn pump(previews: &Previews, actor: &CameraActor, feed: &Arc<Feed>) -> Ended {
    let mut silent = 0_u32;
    let mut deferred = 0_u32;
    loop {
        if previews.0.shutdown.is_cancelled() {
            return Ended::ShuttingDown;
        }
        // Between frames, so the check costs nothing while frames are flowing and the stream
        // ends within one `PREVIEW_FRAME_WAIT_MS` of the last tab closing.
        if feed.frames.receiver_count() == 0 && previews.retire(feed).await {
            return Ended::Unwatched;
        }

        let turn = tokio::select! {
            // Biased so that a cancellation that arrives while a frame is available still
            // ends the loop: an unbiased `select!` would pick at random, and "an open MJPEG
            // tab must not hang shutdown" (design §2.6) is not a claim to leave to a coin.
            biased;
            () = previews.0.shutdown.cancelled() => return Ended::ShuttingDown,
            turn = previews.turn(actor, feed) => turn,
        };

        match turn {
            Ok(Delivery::Frame(_)) => {
                // Both budgets reset on a frame, and `Demand::Enough` is not read here: the
                // top of the loop asks the channel itself how many readers there are, which
                // is the same question with one answer instead of two.
                silent = 0;
                deferred = 0;
            }
            Ok(Delivery::Idle) => {
                silent += 1;
                if silent >= limits::PREVIEW_MAX_EMPTY_TURNS {
                    return Ended::Silent;
                }
            }
            // A full command queue is the camera doing something else, and it is deliberately
            // *not* folded into the arm above: E3 keeps "busy" and "cannot" apart, and the two
            // budgets are separate so that a camera alternating between the two does not end a
            // preview on a mixture of the two reasons.
            Err(err) if err.kind() == ErrorKind::Busy => {
                deferred += 1;
                if deferred >= limits::PREVIEW_MAX_EMPTY_TURNS {
                    return Ended::Deferred;
                }
            }
            Err(err) => return Ended::Refused(err),
        }
    }
}

/// The sink `engine::preview` hands each frame to — the daemon's half of D12's watch channel.
///
/// Built per turn and dropped with it, because it holds only two handles and building it is
/// two `Arc` bumps. It is called **on the camera's actor thread**, which is why nothing in
/// `Publisher::publish` awaits, blocks or needs a runtime context: `watch::Sender` is usable
/// from any thread, exactly as `crate::events`'s `broadcast::Sender` is and for the same
/// reason.
#[derive(Debug)]
struct Publisher {
    feed: Arc<Feed>,
    shutdown: Shutdown,
}

impl FrameSink for Publisher {
    fn publish(&self, frame: Frame) -> Demand {
        // Device-derived and validated before it becomes an allocation this process keeps
        // (design §2.5). A frame over the cap is dropped and counted — the same answer a slow
        // reader's frames get, because the alternative is a driver deciding how much memory
        // this daemon uses.
        if frame.bytes.len() > limits::PREVIEW_MAX_FRAME_BYTES {
            self.feed.oversized.fetch_add(1, Ordering::Relaxed);
            return self.demand();
        }

        // The index is taken from the same `send_modify` that publishes the count, so two
        // actor threads publishing at once cannot be handed one number twice.
        let mut index = 0;
        self.feed.published.send_modify(|count| {
            *count = count.saturating_add(1);
            index = *count;
        });
        // `send_replace` and not `send`: `send` answers `Err` when there are no receivers, and
        // this is the one place where "nobody is listening" must not look like a failure —
        // the answer to it is [`Demand::Enough`], one line down, which ends the stream
        // deliberately rather than as a side effect of an error path.
        self.feed.frames.send_replace(Some(Arc::new(Shot {
            index,
            sequence: frame.sequence,
            bytes: frame.bytes,
        })));
        self.demand()
    }
}

impl Publisher {
    /// Whether anybody still wants frames, asked of the two things that can say no.
    ///
    /// The shutdown token is read **here**, on the actor thread, as well as in the driver's
    /// loop: the loop cannot notice a cancellation that arrives while the thread is inside a
    /// `DQBUF`, and this is the first instruction after it comes out.
    fn demand(&self) -> Demand {
        if self.shutdown.is_cancelled() || self.feed.frames.receiver_count() == 0 {
            Demand::Enough
        } else {
            Demand::More
        }
    }
}

impl Viewer {
    /// The next frame this reader should send, or `None` when the feed has ended.
    ///
    /// **This is where a slow reader's drop becomes a number.** Every call answers with
    /// whatever is in the channel *now*, so a reader that spent a second writing to a stalled
    /// socket comes back to the current frame and not to the one after the one it sent; the
    /// difference between the two indices is what it missed, and it is added to the daemon's
    /// skipped count on the way past.
    ///
    /// A reader that has just attached waits for the *next* frame rather than being handed the
    /// one already in the slot. At any frame rate a camera offers that is milliseconds, and
    /// the alternative is a new tab opening with a picture that may be a minute old.
    pub async fn next(&mut self) -> Option<Arc<Shot>> {
        loop {
            // `Err` means every sender is gone, which is the feed being withdrawn — the end
            // of the stream, and the one non-`None` way out of this loop.
            self.frames.changed().await.ok()?;
            let Some(shot) = self.frames.borrow_and_update().clone() else {
                // The channel's initial value, seen by a reader that attached and was notified
                // before the first frame. There is nothing to send yet.
                continue;
            };
            if let Some(seen) = self.seen {
                let missed = shot.index.saturating_sub(seen).saturating_sub(1);
                if missed > 0 {
                    self.skipped
                        .send_modify(|count| *count = count.saturating_add(missed));
                }
            }
            self.seen = Some(shot.index);
            return Some(shot);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use schema::camera::PixelFormat;

    use super::*;

    /// A frame of `bytes` bytes with a sequence number, as a device would deliver it.
    ///
    /// The content is a constant: this module never looks inside a frame, and a test that
    /// needed pixels would be using the fake through the daemon's integration suite.
    fn frame(sequence: u32, bytes: usize) -> Frame {
        Frame {
            bytes: vec![0xff; bytes],
            pixel_format: PixelFormat::MJPG,
            width: 64,
            height: 48,
            bytes_per_line: 0,
            sequence,
            timestamp_us: i64::from(sequence) * 33_333,
        }
    }

    /// A feed with nobody driving it, for the publish-side assertions.
    fn feed() -> (Previews, Arc<Feed>) {
        let previews = Previews::new(
            Arc::new(Cameras::new(Arc::new(
                fake::FakeBackend::new(Vec::new()).expect("a backend replaying no cameras"),
            ))),
            MonotonicClock::new(),
            Shutdown::new(),
        );
        let feed = Arc::new(Feed {
            camera: CameraId::parse("cam:test").expect("a literal id"),
            frames: watch::Sender::new(None),
            published: previews.0.published.clone(),
            oversized: AtomicU64::new(0),
        });
        (previews, feed)
    }

    fn publisher(previews: &Previews, feed: &Arc<Feed>) -> Publisher {
        Publisher {
            feed: Arc::clone(feed),
            shutdown: previews.0.shutdown.clone(),
        }
    }

    #[tokio::test]
    async fn a_reader_that_missed_frames_is_handed_the_newest_one_and_the_gap_is_counted() {
        // D12's property, at the altitude where it is a property of the channel rather than of
        // a socket: the publisher never waits, and a reader that comes back late gets the
        // current frame plus a count of what it skipped. A queue in place of the `watch` fails
        // both halves — it would hand back frame 2 and count nothing.
        let (previews, feed) = feed();
        let sink = publisher(&previews, &feed);
        let mut viewer = previews.viewer(&feed);
        let mut skipped = previews.watch_skipped();

        assert_eq!(sink.publish(frame(0, 16)), Demand::More);
        let first = viewer.next().await.expect("the first frame");
        assert_eq!(first.index(), 1);
        assert_eq!(first.sequence(), 0);

        // Four more while this reader is not reading. Every one of them is published without
        // waiting for anybody, which is the half that says the device is never backpressured.
        for sequence in 1..5 {
            assert_eq!(sink.publish(frame(sequence, 16)), Demand::More);
        }
        let next = viewer.next().await.expect("the newest frame");
        assert_eq!(next.sequence(), 4, "the reader was handed a stale frame");
        assert_eq!(next.index(), 5);

        // The count is a number rather than a silence (rubric rule 3), and it is *this*
        // reader's gap: three frames existed between the two it was handed.
        skipped.changed().await.expect("the count is live");
        assert_eq!(*skipped.borrow_and_update(), 3);
    }

    #[tokio::test]
    async fn a_frame_over_the_cap_is_dropped_and_counted_rather_than_held() {
        // `limits::PREVIEW_MAX_FRAME_BYTES` doing its job, which is the one bound in this
        // module a device rather than a client can reach: `bytesused` is the driver's number,
        // and a daemon that allocated whatever it was told to would be letting a device decide
        // how much memory it uses (design §2.5).
        let (previews, feed) = feed();
        let sink = publisher(&previews, &feed);
        let mut viewer = previews.viewer(&feed);

        assert_eq!(
            sink.publish(frame(0, limits::PREVIEW_MAX_FRAME_BYTES + 1)),
            Demand::More,
            "an oversized frame ended the stream instead of being dropped"
        );
        assert_eq!(feed.oversized.load(Ordering::Relaxed), 1);
        assert_eq!(
            *previews.watch_published().borrow(),
            0,
            "a frame nobody can hold was counted as published"
        );

        // And the frame after it arrives, so the cap is a filter rather than a fault: the
        // reader's first frame is the one that fitted.
        sink.publish(frame(1, 16));
        assert_eq!(
            viewer
                .next()
                .await
                .expect("the frame that fitted")
                .sequence(),
            1
        );
    }

    #[tokio::test]
    async fn a_feed_with_no_readers_and_a_cancelled_daemon_are_the_two_ways_to_say_enough() {
        // The two answers that end a capture, asserted separately because they are two
        // mechanisms: the receiver count is what a closed tab moves, and the token is what a
        // stopping daemon moves. A build that read only one of them holds a camera open for
        // the case it does not read.
        let (previews, feed) = feed();
        let sink = publisher(&previews, &feed);

        let viewer = previews.viewer(&feed);
        assert_eq!(sink.publish(frame(0, 16)), Demand::More);
        drop(viewer);
        assert_eq!(sink.publish(frame(1, 16)), Demand::Enough);

        let _watching = previews.viewer(&feed);
        assert_eq!(sink.publish(frame(2, 16)), Demand::More);
        previews.0.shutdown.cancel();
        assert_eq!(sink.publish(frame(3, 16)), Demand::Enough);
    }

    #[tokio::test]
    async fn a_withdrawn_feed_ends_its_readers_stream() {
        // What a viewer sees when the daemon stops or the device fails: the sender is dropped
        // with the feed, and `next` answers `None` rather than waiting forever. Without it an
        // open tab would hold a response body open past the shutdown bound, which is exactly
        // what design §2.6 says must not happen.
        let (previews, feed) = feed();
        let mut viewer = previews.viewer(&feed);
        previews.withdraw(&feed).await;
        drop(feed);

        assert!(viewer.next().await.is_none());
    }

    #[tokio::test]
    async fn the_fifth_reader_of_one_camera_is_refused_rather_than_served() {
        // `limits::PREVIEW_MAX_VIEWERS_PER_CAMERA`, read off the channel's receiver count.
        // The refusal is `Busy` naming the camera — D12's own word for "this device is doing
        // something" — and it is asserted as a *kind* rather than a message so a rewording
        // does not fail it.
        let (previews, feed) = feed();
        let info = testkit::fixtures::synthetic_basic().invariant.info;
        previews
            .0
            .live
            .lock()
            .await
            .insert(info.id.clone(), Arc::clone(&feed));

        let mut held = Vec::new();
        for _ in 0..limits::PREVIEW_MAX_VIEWERS_PER_CAMERA {
            let (_feed, viewer, starting) = previews.reserve(info.clone()).await.expect("a place");
            assert!(starting.is_none(), "an existing feed was started again");
            held.push(viewer);
        }
        let refused = previews
            .reserve(info.clone())
            .await
            .expect_err("the fifth reader");
        assert_eq!(refused.kind(), ErrorKind::Busy);

        // One leaves, and the next one in is served — so the bound is a bound rather than a
        // latch.
        held.pop();
        previews.reserve(info).await.expect("a place came free");
    }
}
