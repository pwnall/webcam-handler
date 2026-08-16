//! The recordings this daemon is driving: one take per camera, and the loop that feeds it
//! (design **D7**, **D10**, **D12**; docs/7 P6c).
//!
//! [`crate::preview`] is this module's sibling and the two are built the same way on purpose —
//! a registry under a lock, a driver task per camera, one `actor.submit` per frame — because
//! they solve the same shape: a stream that runs for seconds or minutes, on a device one OS
//! thread owns, on behalf of a client that is not holding it. Reading that module first is the
//! cheapest way to read this one. Where they differ is stated below rather than left to be
//! inferred.
//!
//! ## Why a recording is many short commands and not one long one
//!
//! `engine::actor` gives each open camera one blocking thread and one command channel, and
//! everything the daemon asks of a camera goes through it **in arrival order**. A take written
//! as a single actor command would therefore make `wch_record_stop` undeliverable: the stop
//! would queue behind the recording it is trying to stop, and the only thing that could end the
//! take would be the take's own duration. That is not a latency problem — it is a wire method
//! that does not work, and it is the whole reason `engine::record` is turn-based (its header
//! argues it; note **N111** records it).
//!
//! So `drive` issues one command per frame. The actor is free between every pair of them, so
//! a `wch_record_stop`, a `wch_get` or a preview's own turn waits at most one
//! `limits::FRAME_DEADLINE_MS` behind a recording rather than for the length of it.
//!
//! ## The frame is a value, and that is where the second consumer joins
//!
//! `Live::absorb` takes the [`Frame`] `engine::record::turn` handed back and gives it to the
//! muxer. It runs **on the camera's actor thread**, inside the same command that dequeued the
//! frame, which is what makes the notes' Expected usage item 10 answerable at all: a recording
//! and a preview collide on one camera, V4L2 allows one streamer per node, and the owner ruled
//! on 2026-08-14 (note **N117**) that **the preview is fed the recording's own frames**.
//!
//! So `Recordings::turn`'s closure hands each frame to the muxer and then to
//! `crate::preview::Watchers::show`, in that order and on that thread. Three properties follow
//! and none of them is an optimisation:
//!
//! - **The muxer reads it first**, because the recording is the product and the fan-out is what
//!   the ruling adds to it. `absorb` takes a `&Frame`; `show` takes it by value. So the frame is
//!   **moved** into the viewers' `Shot` after the container has copied what it needs, and the
//!   second consumer costs no copy of the bytes at all.
//! - **A frame the container refused is not shown.** `absorb`'s `?` is what decides it, and the
//!   case is a take that is ending inside this same turn — a cap, or a disk. The viewers' next
//!   event is the hand-back rather than a frame from a take that is over.
//! - **There is no second fan-out and no second sink.** The `Publisher` is the preview's own, so
//!   the byte cap and the paintability guard have one home (design §2.10). What this module owns
//!   is *when* to publish; what may be published is `crate::preview`'s.
//!
//! ## Who holds the camera's stream, and the two calls that move it
//!
//! One streamer per node is the kernel's rule, so a take and a live preview cannot both pump the
//! device — and two loops dequeuing from one stream would hand the recording every other frame,
//! which item 10 forbids in as many words. [`Recordings::reserve`] therefore ends in
//! `Previews::hand_over`, which claims the camera's feed and **waits** for any preview driver to
//! leave; the resulting `Watchers` live **in the slot**, so the obligation to give them back is
//! the registry's rather than a caller's, and it travels out of the slot through
//! [`Recordings::withdraw`] on every refusal path and into `Live` on the one that succeeds.
//! `drive` hands it back after the stream is stopped and before the container is closed.
//!
//! Both ends are `crate::preview`'s decisions, argued there: this module says *when* a recording
//! owns a camera's frames, and that module says what owning them means.
//!
//! ## The obligation is the registry's, because a caller can be cancelled
//!
//! [`Reserved`] is a **witness and not a holding**: the slot and the camera's frames are in
//! `Registry::live` from the instant they are claimed, and what the value a caller holds
//! carries is an `Arc` whose `Weak` twin the slot keeps. That is the answer to the defect note
//! **N171** records — a handler future that is *dropped* rather than refused runs neither
//! `withdraw` nor `adopt`, so a rule that says "every refusal path calls `withdraw`" is a rule a
//! cancellation can skip. Nothing here relies on a code path running:
//!
//! - the reservation's liveness is a question the registry **asks** (`Reservation::wanted`),
//!   and every entry point asks it before it decides anything, so a slot whose `record_start`
//!   went away is not a slot;
//! - `Previews::hand_over` runs on a task of its own, so a caller cancelled *inside* the wait
//!   cannot leave the camera's frames half-claimed — the task finishes, finds no reservation
//!   wanting them, and hands them straight back;
//! - and [`Reserved`]'s own `Drop` spawns that reap when a runtime is there, which is
//!   promptness rather than correctness: the owner's tab gets its picture back at once instead
//!   of at the next verb.
//!
//! Note **N169** is the rule this is one instance of, and `crate::server`'s `photo` is the
//! other.
//!
//! ## One take per camera, and what each of the three verbs does about it
//!
//! [`Recordings`] holds at most one slot per camera, and the three wire methods are its
//! three transitions. The answers are decisions rather than mechanics, so they are argued here
//! and asserted in `crates/daemon/tests/mutating_verbs.rs`:
//!
//! - **A second `record_start` on a camera already recording is [`Error::Busy`].** That word
//!   means *retry* to an unattended reader (AGENTS: "`Busy` means retry"), and retrying is
//!   exactly right — the take that is running is bounded by its own duration, so waiting and
//!   asking again is the action that succeeds. It names the camera's node and carries **no**
//!   holders, for `engine::actor`'s reason: the pid holding it is this daemon, and naming it
//!   would invite a client to kill the daemon it is talking to.
//! - **A `record_start` on a camera whose take has ended and was never collected discards
//!   that take's report, counted and logged.** The alternative — refusing — would let one
//!   abandoned poll loop wedge a camera until somebody called `record_stop` by hand, which is
//!   a state AGENTS' unattended primary consumer cannot get itself out of. Nothing is lost
//!   that the caller was promised: the *file* is where it was asked for, whole and parseable;
//!   what is discarded is this daemon's copy of the accounting. It moves
//!   [`Recordings::watch_discarded`], because rubric rule 3 wants a number rather than a
//!   silence.
//! - **`record_status` answers about the take that is running, or the one waiting to be
//!   collected, and answers "no take" for a camera that has never recorded *and* for one whose
//!   take has been collected.** Those two are one answer because what a caller can do about
//!   either is identical, and a daemon that distinguished them would be remembering something
//!   about a camera after the caller asked for it to be handed over.
//! - **`record_stop` ends the take *and collects it*, emptying the slot.** A take that has
//!   already reached its own bound — the ordinary case, since most takes end on their duration
//!   while the caller is still polling — is simply handed over. That makes
//!   `start` → poll → `stop` total for every ending a take can have, which is what a verb
//!   spanning three calls owes a consumer with no hands.
//! - **`record_stop` on a camera holding no take at all is [`Error::IllegalTransition`]**, not
//!   a bland "nothing happened". An unattended caller has to be able to tell "my recording is
//!   over" from "my `record_start` never took", and those are the same call's two answers; a
//!   success for both would make the second indistinguishable from the first. Note **N46**
//!   widened the variant to mean "the request names something this build will not do", and a
//!   recording that does not exist is exactly that.
//! - **A take that failed is collected as the *device's own refusal*, never as a report.** The
//!   container is closed first, so docs/7 P6b's "every fault leaves a parseable file" holds
//!   across the wire as it does in process; what the caller gets is the `DeviceGone` or the
//!   `StorageIo` that ended it, because a refusal arriving as a successful recording is the
//!   conversion AGENTS rule 7 forbids.
//!
//! ## What shutdown does to a running take
//!
//! AGENTS: open MJPEG and WebSocket streams are "cancelled, never awaited, on shutdown". A
//! recording is the same class and is treated identically — `pump` watches
//! `crate::shutdown::Shutdown` between turns *and* races it against each turn, so a
//! cancellation ends the take within the frame that is being taken when it arrives. What a
//! recording adds is the obligation P6b built: the container is **closed** on the way out, so
//! the file a client was promised is one both of `imaging::avi::read`'s readers can open. If
//! the runtime dies before that close runs, what is left is D7's recoverable `movi` prefix —
//! which is the property that exists for exactly this case, and is why the close being
//! best-effort is a bounded loss rather than a corrupt file.
//!
//! The take's ending is [`RecordingEnd::Stopped`], the same one `record_stop` produces, and
//! deliberately so: from the recording's side "the caller ended it" and "the daemon ended it"
//! are one fact — nothing about the device or the bounds was involved — and inventing a sixth
//! ending to say which of the two would be a vocabulary that grew to describe the *stopper*
//! rather than the recording.
//!
//! ## A frame may contain a person
//!
//! AGENTS, rubric A12. Nothing here logs a frame or anything derived from one: every
//! `tracing` call names a camera, a path the caller chose, a count or a typed refusal, and the
//! only place bytes exist at all is `Live::absorb`'s argument, which goes to the muxer and
//! nowhere else. `Live`'s `Debug` prints counts, as `engine::record::Recording`'s does.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Weak};

use camino::{Utf8Path, Utf8PathBuf};
use engine::actor::{CameraActor, Cameras, OpenCamera};
use engine::record::{FrameOutcome, Opened, Recording};
use engine::settle::{Clock, Millis, MonotonicClock};
use schema::camera::{CameraId, CameraInfo};
use schema::capture::{Frame, NegotiatedStream};
use schema::time::Stamp;
use schema::video::{RecordReport, RecordStatus, RecordingEnd, TakeStatus, VideoFormat};
use schema::{Error, ErrorKind, Result, limits};
use tokio::sync::{Mutex, oneshot, watch};

use crate::preview::Watchers;
use crate::shutdown::Shutdown;

/// Every camera this daemon is recording, and every take waiting to be collected.
///
/// One per process, held by `crate::server::Wchd` beside [`crate::preview::Previews`] and
/// built over the **same** `engine::actor::Cameras` — a second registry over one backend would
/// be a second thread on one node, which is the arrangement `Cameras` exists to make
/// unrepresentable.
///
/// Cheap to clone (an `Arc` bump) and cloned deliberately: the driver task for each take holds
/// one, because it outlives the request that started it.
#[derive(Debug, Clone)]
pub struct Recordings(Arc<Registry>);

/// The shared state behind [`Recordings`]. One of each, never two.
#[derive(Debug)]
struct Registry {
    /// The registry the actors come from — the same one every other verb is answered out of.
    cameras: Arc<Cameras>,
    /// A **clone** of the daemon's clock, not a second one. `engine::settle::Millis` are
    /// milliseconds since a clock's own origin and comparing two clocks' readings is a bug
    /// (that module says so), so a take that stamped its commands from a clock of its own
    /// would feed the actor's idle deadline numbers from another timeline — and would measure
    /// its own duration against one clock while the report measured it against another.
    clock: MonotonicClock,
    /// The daemon's one stop token, watched by every driver.
    shutdown: Shutdown,
    /// The **same** fan-out `crate::server::Wchd` hands the web listener, not a second one.
    ///
    /// Held here because a take owns its camera's frames for the length of it (note **N117**),
    /// and "which cameras have a fan-out" has to be one map: a recording that published into a
    /// registry of its own would leave `Previews::attach` starting a second stream on a node
    /// V4L2 allows one streamer on. The dependency runs one way — this module knows about
    /// previews and that module knows nothing about recordings — which is what keeps the ruling
    /// from becoming two answers to "who owns this camera's stream".
    previews: crate::preview::Previews,
    /// One slot per camera, and the lock that makes "one" true.
    ///
    /// A `tokio::sync::Mutex` for `crate::preview`'s reason: the decision "does this camera
    /// already have a take?" has to be atomic with the insert that answers it. **Nothing
    /// device-shaped happens under it** — the negotiation and the header write are the
    /// handler's, after [`Recordings::reserve`] has returned — so a camera inside a
    /// minutes-long sweep delays the `record_start` that asked for *it* and no other camera's.
    live: Mutex<BTreeMap<CameraId, Slot>>,
    /// How many takes are running, as something to **await**.
    ///
    /// `crate::preview`'s feed count and its pair and for its reason: the map is what *enforces*
    /// one take per camera, and this is that count in the one shape a test can wait on. A
    /// test that polled the map for "the driver started" would be a test with a sleep in it
    /// under another name (AGENTS).
    running: watch::Sender<usize>,
    /// How many takes have reached an ending, ever.
    ///
    /// The driver's only observable from outside, and the one that separates "the take ended"
    /// from "somebody collected it": `record_stop` empties the slot, so a suite that watched
    /// only the map could not tell a driver that finished from one that never ran.
    finished: watch::Sender<u64>,
    /// How many finished takes a later `record_start` discarded before collecting.
    ///
    /// The module header argues why discarding is the right answer; this is the half that
    /// keeps it from being silent (rubric rule 3). It can only move when a caller abandoned a
    /// poll loop, so a number that is not zero on a healthy daemon is a finding.
    discarded: watch::Sender<u64>,
}

/// What one camera's recording slot holds.
#[derive(Debug)]
enum Slot {
    /// A `record_start` is negotiating a stream and writing a header.
    ///
    /// Its own state rather than an absence, because the reservation is taken **before** the
    /// device work and released after it: without this, two `record_start` calls arriving
    /// together would both find an empty map and both reach `VIDIOC_STREAMON`, and the second
    /// would be refused by the *kernel* — a refusal about the machine rather than about this
    /// daemon, arriving after a file had been opened.
    Starting(Box<Reservation>),
    /// A take is running, and a driver task is feeding it.
    Running(Arc<Live>),
    /// A take has ended and nobody has collected it yet.
    Ended(Box<Finished>),
}

/// What a `Slot::Starting` is holding while its `record_start` negotiates.
///
/// The two claims a `record_start` takes both live **here** rather than in the [`Reserved`] the
/// handler holds, and that is the whole of note **N171**'s repair: a future that is dropped
/// mid-flight destroys the value it was holding, and a value that was holding an obligation
/// takes the obligation with it. What the handler holds instead is a witness whose absence this
/// registry can *see*.
#[derive(Debug)]
struct Reservation {
    /// The camera this reservation is about, resolved by the `record_start` that took it.
    ///
    /// Kept because every refusal about this slot has to name a node ([`Error::Busy`]'s `path`)
    /// and because a take on a camera that has since been unplugged is still collectable
    /// through it — `crate::server`'s `record_stop` resolves the *registry* when the machine no
    /// longer resolves (note **N173**).
    info: CameraInfo,
    /// Whether anybody is still going to use this reservation.
    ///
    /// The `Arc` is in the caller's [`Reserved`] and this is its `Weak`, so "the `record_start`
    /// went away" is a fact with no code path behind it: dropping a future drops the `Arc`, and
    /// `Reservation::wanted` answers `false` from then on. A `bool` a refusal path set would
    /// be the thing that is not set when a caller is cancelled.
    holder: Weak<Holding>,
    /// The camera's frames, from the moment `Previews::hand_over` answers.
    ///
    /// `None` for the length of that call and no longer. It is not a state a decision reads —
    /// a reservation is a reservation whether or not the hand-over has finished — but it is
    /// what makes the give-back total: whoever removes this slot hands back whatever is here.
    watchers: Option<Watchers>,
}

/// The `Arc` half of a reservation's liveness. One per `record_start`, held by its [`Reserved`].
///
/// A unit type because the only thing about it that carries information is whether it still
/// exists.
#[derive(Debug)]
struct Holding;

impl Reservation {
    /// Whether the `record_start` that took this slot is still there to use it.
    fn wanted(&self) -> bool {
        self.holder.strong_count() > 0
    }

    /// Whether `holder` is *this* reservation's witness.
    ///
    /// Pointer identity rather than a camera id, for `crate::preview::remove`'s reason one
    /// variant shape along: a slot that was given back and re-taken by a later `record_start`
    /// has the same key and is a different claim, and an earlier caller must not be able to
    /// discharge a later one's.
    fn is(&self, holder: &Weak<Holding>) -> bool {
        Weak::ptr_eq(&self.holder, holder)
    }
}

/// A take in progress: what it is, what it has produced, and the muxer under it.
///
/// Shared between the driver task and the actor thread's per-frame closure, which is why every
/// mutable field is an atomic or behind a `std::sync::Mutex` rather than owned by the loop.
struct Live {
    /// The camera this take is on, as it resolved when the take began.
    ///
    /// The whole [`CameraInfo`] and not just its id, for [`Reservation::info`]'s second reason:
    /// a take outlives the enumeration that started it, so an unplug must not make the take
    /// uncollectable (note **N173**).
    info: CameraInfo,
    /// Whoever is watching this camera, and this take's claim on their fan-out.
    ///
    /// Held for the length of the take rather than looked up per frame, and that is the field
    /// that makes the whole arrangement affordable: the lookup would be
    /// `crate::preview`'s `tokio::sync::Mutex`, and the thread that has the frame in its hand is
    /// the camera's **actor** thread, which has no runtime to await on. So the claim is taken
    /// once, before the device work, and what runs per frame is a `watch::Sender` write.
    watchers: Watchers,
    path: Utf8PathBuf,
    format: VideoFormat,
    negotiated: NegotiatedStream,
    started_at: Stamp,
    /// The daemon's clock reading when the header was written — the origin
    /// [`TakeStatus::elapsed_ms`] is measured from.
    started_ms: Millis,
    /// How long the take may run on that clock, from `RecordRequest::budget_ms`.
    budget_ms: u64,
    /// How many frames the container has accepted.
    ///
    /// Counted here rather than asked of the muxer because `imaging::video::Recorder` hands
    /// back no count until `finish`, and a status that could not answer until the take was
    /// over would be a progress mechanism with no progress in it. It ends equal to
    /// `RecordingSummary::frames_written`, which
    /// `a_polled_status_counts_the_frames_the_finished_report_counts` asserts rather than
    /// assumes.
    frames: AtomicU32,
    /// The muxer, reachable from the driver **and** from the actor thread's closure.
    ///
    /// `std::sync::Mutex` and not tokio's: it is taken on the actor's blocking thread, where
    /// there is no runtime to await on, and it is never held across an `await` — the whole of
    /// what happens under it is one `Recorder::write_frame`. Contention is nil by
    /// construction, because the driver issues one command at a time and does not touch the
    /// muxer until the loop has ended.
    ///
    /// `Option` because [`drive`] takes it out to `finish` it, which consumes it.
    recording: std::sync::Mutex<Option<Recording>>,
    /// Raised by [`Recordings::collect`] and read by [`pump`] between turns.
    stopping: AtomicBool,
    /// Set once the driver has installed this take's [`Slot::Ended`].
    ///
    /// What `record_stop` **awaits** rather than polls: "the driver finished" is an event, and
    /// a stop that spun on the map would be a sleep with a different name (AGENTS).
    done: watch::Sender<bool>,
}

impl std::fmt::Debug for Live {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // A frame may contain a person, and the muxer holds the sink those frames go to.
        // Counts, a path the caller named and this module's own vocabulary — never bytes.
        f.debug_struct("Live")
            .field("camera", &self.info.id)
            .field("path", &self.path)
            .field("format", &self.format)
            .field("frames", &self.frames.load(Ordering::Relaxed))
            .field("budget_ms", &self.budget_ms)
            .finish_non_exhaustive()
    }
}

/// A take that has ended, waiting to be collected.
#[derive(Debug)]
struct Finished {
    /// The camera it was taken on, carried for [`Reservation::info`]'s second reason: the
    /// ending most worth collecting is `DeviceFailed`, and a camera that failed is very often
    /// a camera that is no longer there to enumerate (note **N173**).
    info: CameraInfo,
    /// What `record_status` answers with — the same document a running take produces, with
    /// its ending filled in.
    status: TakeStatus,
    /// What `record_stop` answers with: the report, or the refusal that ended the take.
    ///
    /// A `Result` rather than a report and an optional error, because those are the two
    /// answers `record_stop` can give and a caller must not be able to receive both.
    outcome: std::result::Result<RecordReport, Error>,
}

/// The witness that a caller holds this camera's recording slot **and its frames**.
///
/// A value rather than a `bool` for `crate::preview::Starting`'s reason: the only way to get
/// one is to be the call that reserved the slot. What it is *not*, since note **N171**, is the
/// place the claims live — those are in `Slot::Starting`, and this is the `Arc` whose
/// existence says somebody still means to use them. The difference is what a cancelled handler
/// costs: a value that held the claims took them with it when its future was dropped, and a
/// value that holds a witness gives them back by being dropped.
///
/// It carries the camera so neither [`Recordings::withdraw`] nor [`Recordings::adopt`] has to
/// be told which slot it is about.
#[derive(Debug)]
pub struct Reserved {
    info: CameraInfo,
    /// The registry to give this slot back to, for [`Reserved`]'s `Drop`.
    ///
    /// A clone of the one `Arc`, not a second registry — and no cycle, because the slot holds
    /// only a [`Weak`] of the value below.
    recordings: Recordings,
    held: Arc<Holding>,
}

impl Drop for Reserved {
    /// Give an undischarged reservation back **now** rather than at the next verb.
    ///
    /// Correctness does not depend on this — every entry point in this module reaps a slot
    /// whose holder has gone before it decides anything, which is what makes the invariant
    /// independent of a runtime being here. What this buys is promptness, and the thing it is
    /// prompt about is somebody's picture: a `record_start` cancelled after it claimed the
    /// camera's frames leaves an open tab watching a feed with no publisher until the next
    /// `record_*` verb on that camera, and there may not be one.
    ///
    /// It is a `spawn` and not the work itself for the reason the explicit path existed in the
    /// first place: giving a slot back takes two `tokio::sync::Mutex`es and a `Drop` cannot
    /// `await`. `try_current` rather than `Handle::current` because a task dropped while the
    /// runtime itself is going away has no handle to spawn on — and no need of one, since the
    /// registry it would repair is about to be dropped too.
    ///
    /// Discharged reservations reach here as well ([`Recordings::withdraw`] and
    /// [`Recordings::adopt`] both consume the value), and the reap is a no-op for them: it acts
    /// only on a `Slot::Starting` this very witness belongs to, which by then is gone.
    fn drop(&mut self) {
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let recordings = self.recordings.clone();
            let camera = self.info.id.clone();
            runtime.spawn(async move { recordings.reap(&camera).await });
        }
    }
}

/// What one turn of a take's loop produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// A frame arrived and the container took it.
    Wrote,
    /// `limits::FRAME_DEADLINE_MS` passed with no frame. **Not an error** — a device that
    /// missed one deadline is slow, and one that has stopped delivering misses
    /// `limits::RECORDING_MAX_EMPTY_TURNS` of them in a row (`engine::record::Turn::Idle`
    /// argues it; AGENTS rule 7 forbids converting the first into the second).
    Idle,
    /// A frame arrived and a cap refused it, which ends the take.
    Capped,
}

impl Recordings {
    /// A registry over `cameras`, stamping its commands from `clock` and stopping with
    /// `shutdown`.
    ///
    /// Every argument is a value the composition root already made, and each is the daemon's
    /// **one** of that thing rather than a second — `crate::preview::Previews::new` takes the
    /// identical first three for the identical reason, and the fourth is *its* value rather
    /// than one built here.
    #[must_use]
    pub fn new(
        cameras: Arc<Cameras>,
        previews: crate::preview::Previews,
        clock: MonotonicClock,
        shutdown: Shutdown,
    ) -> Recordings {
        Recordings(Arc::new(Registry {
            cameras,
            clock,
            shutdown,
            previews,
            live: Mutex::new(BTreeMap::new()),
            running: watch::Sender::new(0),
            finished: watch::Sender::new(0),
            discarded: watch::Sender::new(0),
        }))
    }

    /// How many takes are running right now.
    #[must_use]
    pub fn running(&self) -> usize {
        *self.0.running.borrow()
    }

    /// The same count, as something to **await**.
    ///
    /// "The driver started" and "the driver put the take down" are events, and the only honest
    /// way to wait for an event is to be told about it (AGENTS: nothing sleeps to synchronize).
    #[must_use]
    pub fn watch_running(&self) -> watch::Receiver<usize> {
        self.0.running.subscribe()
    }

    /// How many takes have reached an ending, as something to **await**.
    ///
    /// The observable a suite waits on to say "this recording is over" without asking the
    /// camera and without a clock — and the one number a build whose driver never ran would
    /// leave at zero while every other assertion here still passed.
    #[must_use]
    pub fn watch_finished(&self) -> watch::Receiver<u64> {
        self.0.finished.subscribe()
    }

    /// How many finished takes were discarded uncollected, as something to **await**.
    ///
    /// The module header's second bullet, as a number: it moves only when a `record_start`
    /// found a take nobody had collected, which is a caller that abandoned its poll loop.
    #[must_use]
    pub fn watch_discarded(&self) -> watch::Receiver<u64> {
        self.0.discarded.subscribe()
    }

    /// Take this camera's recording slot **and its frames**, or refuse.
    ///
    /// Called **before** anything device-shaped, so a second `record_start` is answered by this
    /// daemon rather than by the kernel's `EBUSY` after a file has been opened — see
    /// `Slot::Starting` — and so the preview driver has left the device before
    /// `engine::record::start` asks it for a stream.
    ///
    /// The two claims are taken in that order and it matters: the slot is what makes this the
    /// only `record_start` in flight for this camera, and the hand-over is what waits. Taking
    /// the frames first would leave a second `record_start` waiting on a preview driver it was
    /// about to be refused over.
    ///
    /// **The hand-over happens with the registry lock released**, and the shape below is the
    /// whole reason: `Previews::hand_over` waits for a `STREAMOFF`, and a wait under this lock
    /// would be one camera's preview delaying every other camera's `record_status`. The slot is
    /// `Slot::Starting` throughout, which is what makes that safe.
    ///
    /// **And the hand-over runs on a task of its own**, which is the half note **N171** added.
    /// That wait is the widest part of a `record_start`, so it is where a client that hangs up
    /// is most likely to cancel this future — and a cancellation *inside* `hand_over` would
    /// leave the camera's frames claimed by a value that no longer exists. Spawning makes the
    /// claim uncancellable: the task finishes whatever happens here, and hands the frames
    /// straight back if by then there is no reservation wanting them.
    ///
    /// # Errors
    ///
    /// [`Error::Busy`] naming the camera's node when a take is already running on it or
    /// another `record_start` is negotiating one. The module header argues why that variant
    /// and why its `holders` list is empty. [`Error::DeviceIo`] when the hand-over task itself
    /// could not be joined — this process failing to compose itself, and the one path on which
    /// a take must not start, because the camera may still have a preview driver on it.
    pub async fn reserve(&self, info: &CameraInfo) -> Result<Reserved> {
        let reserved = self.claim(info).await?;
        let handing = {
            let recordings = self.clone();
            let info = info.clone();
            let holder = Arc::downgrade(&reserved.held);
            tokio::spawn(async move {
                let watchers = recordings.0.previews.hand_over(&info).await;
                recordings.stow(&info.id, &holder, watchers).await;
            })
        };
        if let Err(err) = handing.await {
            self.withdraw(reserved).await;
            return Err(Error::DeviceIo {
                operation: format!("claim the frames of {camera}", camera = info.id),
                errno: None,
                message: format!("the hand-over from this camera's preview did not finish: {err}"),
            });
        }
        Ok(reserved)
    }

    /// Put `Slot::Starting` in this camera's slot, or refuse.
    async fn claim(&self, info: &CameraInfo) -> Result<Reserved> {
        // Before anything is decided, because a slot whose `record_start` went away is not a
        // slot and must not refuse this one (note **N171**). It is a separate lock acquisition
        // and that is deliberate: the give-back it performs is `Previews`', and this module
        // does not hold its own lock across another module's.
        self.reap(&info.id).await;
        let held = Arc::new(Holding);
        let mut live = self.0.live.lock().await;
        match live.get(&info.id) {
            Some(Slot::Starting(_) | Slot::Running(_)) => {
                return Err(Error::Busy {
                    path: crate::preview::node_of(info),
                    holders: Vec::new(),
                });
            }
            Some(Slot::Ended(finished)) => {
                // The module header's second bullet. Counted before it is logged, so the
                // number a test awaits has moved by the time the line is written.
                self.0
                    .discarded
                    .send_modify(|count| *count = count.saturating_add(1));
                tracing::warn!(
                    camera = %info.id,
                    path = %finished.status.path,
                    "a new recording replaced one that ended and was never collected; the \
                     file it wrote is still where it was asked for"
                );
            }
            None => {}
        }
        live.insert(
            info.id.clone(),
            Slot::Starting(Box::new(Reservation {
                info: info.clone(),
                holder: Arc::downgrade(&held),
                watchers: None,
            })),
        );
        Ok(Reserved {
            info: info.clone(),
            recordings: self.clone(),
            held,
        })
    }

    /// Put the camera's frames in the slot that was waiting for them, or give them straight
    /// back.
    ///
    /// The tail of [`Recordings::reserve`]'s hand-over task, and the reason that task can
    /// finish alone: whatever happened to the caller in the meantime, this leaves the frames
    /// with exactly one owner. They go to the reservation when it is still this one and still
    /// wanted, and back to `crate::preview` otherwise — including the case where a `Reserved`
    /// was dropped mid-hand-over, where the dead slot goes with them.
    async fn stow(&self, camera: &CameraId, holder: &Weak<Holding>, watchers: Watchers) {
        let unwanted = {
            let mut live = self.0.live.lock().await;
            match live.get_mut(camera) {
                Some(Slot::Starting(reservation)) if reservation.is(holder) => {
                    if reservation.wanted() {
                        reservation.watchers = Some(watchers);
                        None
                    } else {
                        live.remove(camera);
                        Some(watchers)
                    }
                }
                // Somebody else's slot, or none: this reservation was withdrawn while its
                // frames were being handed over, and the frames are all that is left to
                // return.
                _ => Some(watchers),
            }
        };
        if let Some(watchers) = unwanted {
            watchers.hand_back().await;
        }
    }

    /// Give back a reservation whose `record_start` is no longer there to use it.
    ///
    /// **The invariant, rather than the tidying**: it is asked at the top of every entry point
    /// that reads this registry, so a cancelled `record_start` cannot make a camera answer
    /// `Busy` for the life of the process (note **N171**) — and asking is what makes that true
    /// without a runtime, a task or a `Drop` having to have run.
    ///
    /// A no-op on every other shape, including a reservation that is still wanted: a
    /// `record_start` in the middle of writing its header is exactly what `Slot::Starting`
    /// means.
    ///
    /// **And a no-op while the hand-over is still in flight**, which is the one subtlety worth
    /// the sentence. A reservation whose caller has gone but whose `Previews::hand_over` has
    /// not yet answered is holding the camera's frames in a call rather than in a field, so
    /// removing the slot here would let a second `record_start` claim those same frames — and
    /// the first call's hand-back would then take the *second* take's fan-out out of the
    /// registry, which is a second stream on one node the moment a tab arrives. So the slot
    /// stays until [`Recordings::stow`] answers, which is the wait the `record_start` that took
    /// it would have imposed anyway.
    async fn reap(&self, camera: &CameraId) {
        let abandoned = {
            let mut live = self.0.live.lock().await;
            match live.get(camera) {
                Some(Slot::Starting(reservation))
                    if !reservation.wanted() && reservation.watchers.is_some() =>
                {
                    match live.remove(camera) {
                        Some(Slot::Starting(reservation)) => reservation.watchers,
                        // Removed under a lock this call holds, so the entry is the one the
                        // arm above matched; written rather than assumed away because a
                        // `None` here would otherwise be a frame claim dropped in silence.
                        _ => None,
                    }
                }
                _ => None,
            }
        };
        if let Some(watchers) = abandoned {
            tracing::debug!(
                %camera,
                "a record_start went away before its take began; the slot and the camera's \
                 frames are free again"
            );
            watchers.hand_back().await;
        }
    }

    /// Give the slot back without a take in it, and the camera's frames with it.
    ///
    /// The path out of a `record_start` whose negotiation, container pairing or header write
    /// was refused. It is the *explicit* discharge and it stays, because a refusal that names
    /// its reason should not have to wait for a `Drop` to be scheduled — but since note
    /// **N171** it is no longer the only thing standing between a cancelled handler and a
    /// wedged camera: `Recordings::reap` answers for the paths that never reach here.
    ///
    /// **The hand-back is on this path too, and it is the half that is easy to forget**: a
    /// `record_start` that stopped somebody's preview and was then refused its container would
    /// otherwise leave a tab watching a feed nothing publishes into, for a take that never
    /// existed.
    pub async fn withdraw(&self, reserved: Reserved) {
        let watchers = self.release(&reserved).await;
        if let Some(watchers) = watchers {
            watchers.hand_back().await;
        }
    }

    /// Take this reservation's slot out of the registry, and answer what it was holding.
    ///
    /// The one home for "this reservation is over", shared by the two ways it can be:
    /// [`Recordings::withdraw`] hands what comes back to `crate::preview`, and
    /// [`Recordings::adopt`] hands it to the take.
    async fn release(&self, reserved: &Reserved) -> Option<Watchers> {
        let mut live = self.0.live.lock().await;
        Recordings::take_out(&mut live, reserved)
    }

    /// [`Recordings::release`], out of a map the caller has **already locked**.
    ///
    /// Synchronous, and that is the whole of note **N176**: [`Recordings::adopt`] has to take
    /// the reservation out and put the take in as **one** mutation, so the two cannot be an
    /// `await` apart. A slot that is briefly absent while `VIDIOC_STREAMON` has already
    /// succeeded is a camera this daemon reports as free while it is streaming — which is
    /// N118's photo defect, `record_stop`'s `IllegalTransition`-for-`Busy`, and two
    /// `record_start`s holding one node, all at once. Handing the guard through rather than
    /// taking the lock twice is what makes the transition atomic by construction.
    ///
    /// **Only if the slot is still this reservation**, by pointer identity: a `record_start`
    /// refused after a *later* one had already taken the slot must not remove the later one's
    /// claim. A `matches!(.., Slot::Starting)` would not see the difference, because both
    /// reservations spell it the same way.
    fn take_out(live: &mut BTreeMap<CameraId, Slot>, reserved: &Reserved) -> Option<Watchers> {
        let holder = Arc::downgrade(&reserved.held);
        match live.get(&reserved.info.id) {
            Some(Slot::Starting(reservation)) if reservation.is(&holder) => {
                match live.remove(&reserved.info.id) {
                    Some(Slot::Starting(reservation)) => reservation.watchers,
                    // See `reap`: removed under the lock this call is holding.
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Put a negotiated, header-written take into the slot and start driving it.
    ///
    /// The second half of `record_start`, after the device work the *handler* did through
    /// `crate::server::Wchd`'s actor entry point. The split is not decoration: that entry
    /// point owns D12's `wait` flag and the bounded pool of threads a waiting request parks
    /// (`crate::server::Waiters`), and a second submitter here would be a second answer to
    /// "how many requests may wait for a camera".
    ///
    /// # Errors
    ///
    /// [`Error::DeviceIo`] when the camera's actor thread cannot be started — this process
    /// failing to compose itself, which is the variant `engine::actor` already uses for it —
    /// and when the reservation this call was handed is no longer in the registry, which is
    /// the same class of failure and cannot happen while [`Recordings::reserve`] is the only
    /// way to get one. The slot is given back on both paths, so a failure here leaves the
    /// camera exactly as this call found it.
    pub async fn adopt(
        &self,
        reserved: Reserved,
        info: &CameraInfo,
        opened: &Opened,
        recording: Recording,
        path: &Utf8Path,
        now: Stamp,
    ) -> Result<RecordStatus> {
        let at = self.0.clock.now_ms();
        let actor = match self.0.cameras.actor(info, at) {
            Ok(actor) => actor,
            Err(err) => {
                // Nothing is ever going to drive this take, and leaving the reservation
                // behind would make the camera refuse every later `record_start` with `Busy`
                // for the life of the process.
                self.withdraw(reserved).await;
                return Err(err);
            }
        };

        // **`Slot::Starting` becomes `Slot::Running` in one mutation of one map, under one
        // guard** — note **N176**, and the reason this whole block is written inside the
        // critical section rather than around it. `VIDIOC_STREAMON` has already succeeded by
        // the time this call is reached, so a slot that is *absent* for even an instant is a
        // camera this daemon reports as free while the device is streaming: a photo dequeued in
        // that instant suspends the take's stream (N118, N170), a `record_stop` is told
        // `IllegalTransition` — terminal — where `Busy` is due, and a second `record_start` can
        // claim the same node. Taking the frames out with `take_out` and putting the take in
        // with the same guard is what makes the transition atomic by construction rather than
        // by a window being small.
        //
        // The frames come out of the *slot*, which is where they have been since the hand-over
        // finished (see the module header). `None` is a reservation that is no longer in the
        // registry — nothing can produce it while `reserve` is the only constructor of a
        // `Reserved`, and it is answered rather than assumed away, because the alternative is a
        // take that publishes into a fan-out somebody else owns.
        let (status, live) = {
            let mut map = self.0.live.lock().await;
            let Some(watchers) = Recordings::take_out(&mut map, &reserved) else {
                return Err(Error::DeviceIo {
                    operation: format!("adopt the recording on {camera}", camera = info.id),
                    errno: None,
                    message: "this camera's recording slot was given away while its take was \
                              being negotiated"
                        .to_owned(),
                });
            };
            let live = Arc::new(Live {
                info: info.clone(),
                // Moved out of the slot and into the take: from here the obligation to give the
                // camera's frames back belongs to `drive`, which is the only thing that knows
                // when this take's stream has stopped.
                watchers,
                path: path.to_owned(),
                format: opened.format,
                negotiated: opened.negotiated.clone(),
                started_at: now,
                started_ms: at,
                budget_ms: opened.budget_ms,
                frames: AtomicU32::new(0),
                recording: std::sync::Mutex::new(Some(recording)),
                stopping: AtomicBool::new(false),
                done: watch::Sender::new(false),
            });
            // The key was emptied by `take_out` two lines up, under this same guard, so this
            // insert can overwrite nothing: an unconditional insert here is what discarded a
            // *later* `record_start`'s reservation — and its frame claim with it — while the
            // two acquisitions were apart.
            map.insert(info.id.clone(), Slot::Running(Arc::clone(&live)));
            self.0.running.send_replace(count_running(&map));
            // Built under the lock so the answer a caller receives is the state the registry
            // is in, rather than one a driver may already have moved past.
            (
                RecordStatus {
                    camera: info.id.clone(),
                    take: Some(live.status(&self.0.clock)),
                },
                live,
            )
        };

        tokio::spawn(drive(self.clone(), actor, live));
        Ok(status)
    }

    /// What this camera's recording is doing.
    ///
    /// A read: it opens no camera, takes no device lock and changes nothing a later
    /// `record_stop` will answer.
    ///
    /// A camera whose `record_start` is still negotiating answers **no take**, which is honest
    /// rather than convenient: there is nothing to describe yet — no negotiated stream, no
    /// container, no file — and a status carrying a request's hopes instead of a device's
    /// answer would be exactly the "requested is not applied" collapse AGENTS rule 5 forbids.
    /// Nobody is polling in that window either, because it closes before `record_start`
    /// answers.
    pub async fn status(&self, camera: &CameraId) -> RecordStatus {
        self.reap(camera).await;
        let live = self.0.live.lock().await;
        let take = match live.get(camera) {
            None | Some(Slot::Starting(_)) => None,
            Some(Slot::Running(take)) => Some(take.status(&self.0.clock)),
            Some(Slot::Ended(finished)) => Some(finished.status.clone()),
        };
        RecordStatus {
            camera: camera.clone(),
            take,
        }
    }

    /// Whether this camera has a take on it, and the refusal if it does.
    ///
    /// The one question another verb asks this registry, and it is asked by `wch_photo` — whose
    /// suspend/resume would otherwise take the *recording's* stream down and put a gap in the
    /// one measurement a recording exists to carry (note **N118**; `crate::server`'s `photo` is
    /// where the argument is written, beside the call).
    ///
    /// A camera whose `record_start` is still negotiating (`Slot::Starting`) is refused too,
    /// and that is the same answer for the same reason: the stream is about to exist, and a
    /// photo that slipped between the reservation and the `VIDIOC_STREAMON` would be refused by
    /// the *device* a moment later anyway. A camera holding an uncollected take is **not**
    /// refused — nothing is streaming, and the file is finished.
    ///
    /// The node in the refusal comes out of the **slot**, which is the camera the take is
    /// actually on: this is asked with an id, because its second caller has no `CameraInfo` and
    /// a registry that made one up would be naming a device rather than reading one.
    ///
    /// ## Why this answers `Ok` where [`Recordings::claim`] answers `Busy`, on the same slot
    ///
    /// A reservation **nobody is waiting on** is a `Busy` to a second `record_start` and an
    /// `Ok` to a photograph, and the two are not a disagreement about the camera (note
    /// **N178**). They are different questions asked of one fact. `claim` asks *may I take this
    /// slot* — and it may not, because the frames that reservation asked for are still inside a
    /// `Previews::hand_over` call, so `reap` deliberately leaves it there until that call
    /// answers. This asks *is a stream running that a suspend/resume would break* — and there
    /// is not one and never will be: the `record_start` that would have reached
    /// `VIDIOC_STREAMON` has gone. Refusing the photograph too would cost a caller a `Busy` on
    /// a camera nothing is going to record, for as long as somebody else's cancelled call takes
    /// to unwind.
    fn refusal(live: &BTreeMap<CameraId, Slot>, camera: &CameraId) -> Result<()> {
        match live.get(camera) {
            // A reservation nobody is waiting on is not a recording — see `reap`, which is what
            // takes it out of the map. Answered here as well because this decision is also read
            // from a thread that cannot reap (notes **N170** and **N171**).
            Some(Slot::Starting(reservation)) if !reservation.wanted() => Ok(()),
            Some(Slot::Starting(reservation)) => Err(Error::Busy {
                path: crate::preview::node_of(&reservation.info),
                holders: Vec::new(),
            }),
            Some(Slot::Running(take)) => Err(Error::Busy {
                path: crate::preview::node_of(&take.info),
                holders: Vec::new(),
            }),
            None | Some(Slot::Ended(_)) => Ok(()),
        }
    }

    /// `Recordings::refusal`, for a caller with a runtime under it.
    ///
    /// # Errors
    ///
    /// [`Error::Busy`] naming the camera's node, with no holders — `Recordings::reserve`'s
    /// refusal, arriving at the other verb that meets the same fact.
    pub async fn not_recording(&self, camera: &CameraId) -> Result<()> {
        // The same lock every other answer here is read under, awaited rather than tried: a
        // `try_lock` whose failure meant "no take" would be a refusal that depended on this
        // daemon's scheduling, which is a test that cannot be made to go red both ways. Nothing
        // holds this lock across device work — `reserve` releases it before the hand-over and
        // `collect` awaits the driver outside it — so the wait is a `BTreeMap` lookup long.
        let live = self.0.live.lock().await;
        Recordings::refusal(&live, camera)
    }

    /// `Recordings::refusal`, **on a camera's actor thread**, where the answer is exact.
    ///
    /// This is the half of note **N118**'s interlock that note **N170** had to add, and the
    /// reason it exists at all is the actor's queue. `not_recording` above is asked by
    /// `wch_photo` before it opens a destination, which is a *check* — the photo then goes to
    /// the blocking pool and only afterwards enqueues its command, and a `record_start` that
    /// claimed the camera inside that window puts its `VIDIOC_STREAMON` in front of the photo.
    /// The photo then suspends the take's stream, which is precisely what N118 exists to
    /// prevent.
    ///
    /// Asked **here**, from inside the photo's own actor command, there is no window left:
    /// `engine::actor` runs one command at a time in arrival order, so a take whose stream
    /// exists is in this map already, and a take whose stream does not exist yet cannot start
    /// it until this command has returned. The check-then-act becomes a check.
    ///
    /// # Panics
    ///
    /// `blocking_lock` panics inside a runtime, and this is the one call in the daemon that is
    /// **not** inside one: `engine::actor` gives each open camera an OS thread (D12) precisely
    /// because V4L2 ioctls block, and the engine holds no runtime (note N41). A caller that ran
    /// this on a runtime worker would be a caller that had moved the photo's device work off
    /// the actor, which is a larger change than this line.
    ///
    /// The wait is a `BTreeMap` lookup long, because nothing in this module holds this lock
    /// across an `await`.
    ///
    /// # Errors
    ///
    /// [`Error::Busy`], exactly as [`Recordings::not_recording`].
    pub fn not_recording_on_this_thread(&self, camera: &CameraId) -> Result<()> {
        let live = self.0.live.blocking_lock();
        Recordings::refusal(&live, camera)
    }

    /// Whether this registry is holding a **take** for `camera`.
    ///
    /// The question `crate::server`'s `record_stop` and `record_status` ask when the *machine*
    /// no longer resolves the id they were given (note **N173**): a take on a camera that has
    /// been unplugged is still a take, and D13 keeps `CameraUnknown` — "a name that never
    /// resolved" — for the case where this answers `false`. It is deliberately not a resolver:
    /// D1's prefixes are resolved against an enumeration, and an id that no longer enumerates
    /// is matched here exactly, which is the id `record_start` answered with.
    ///
    /// **A reservation is not a take, and that is what this answers `false` to** (note
    /// **N178**). `Slot::Starting` is a `record_start` that has not negotiated a stream yet —
    /// [`Recordings::status`] already says "no take" about one, in as many words and for the
    /// reason argued there. So a build that counted it here answered `record_status` on an
    /// **unplugged** camera with a successful `RecordStatus { take: None }`: a document saying
    /// nothing is recording, about a camera that is no longer on this machine, to a consumer
    /// whose whole vocabulary for that fact is `DeviceGone` and `CameraUnknown`. The
    /// reservation's own `record_start` is the call that will be told what happened to the
    /// device, because it is the one holding it.
    pub async fn holds(&self, camera: &CameraId) -> bool {
        self.reap(camera).await;
        matches!(
            self.0.live.lock().await.get(camera),
            Some(Slot::Running(_) | Slot::Ended(_))
        )
    }

    /// End this camera's take if one is running, and hand over what it turned out to be.
    ///
    /// **Stop and collect are one verb**, and the module header argues it. The wait for a
    /// running take is an `await` on the driver's own signal rather than a poll of the map,
    /// and it is bounded by what the driver is doing: `pump` reads the stop flag between
    /// turns, so the longest it can be is one `limits::FRAME_DEADLINE_MS` plus the container's
    /// close. A device that never returns from `DQBUF` wedges the camera's actor thread and
    /// therefore this wait too — that residual is `crate::server::open_destination`'s, stated
    /// at its real size in note **N59**, and nothing in D12 provides the cancellable device
    /// thread that would bound it.
    ///
    /// ## What the second reading can find, and why it is four answers rather than one
    ///
    /// The wait above releases the lock, so the slot this call comes back to is not always the
    /// one it left — and note **N172** is the defect that taught it: a single catch-all told
    /// every one of those callers that *this daemon is shutting down*, which on a healthy
    /// daemon is a sentence about the wrong machine. Two of the shapes are ordinary
    /// interleavings between two clients, and each already has a decided answer one `match`
    /// higher up:
    ///
    /// - **the slot is empty** — a second `record_stop` got there first and is holding the
    ///   report. This camera now holds nothing, which is `nothing_to_stop`'s exact sentence;
    /// - **a reservation is in it** — a `record_start` arrived while this call was waiting and
    ///   discarded the finished take it found (the module header's second bullet, counted).
    ///   `Busy` means retry, and the take that took the slot is bounded by its own duration;
    /// - **a *different* take is running** — the same interleaving, one step further on.
    ///   `Busy` for the same reason;
    /// - **this call's own take is still running**, by pointer identity, which is the one shape
    ///   that really does mean the driver ended without installing a result. That happens when
    ///   the runtime is going away underneath it, and it is the only shape allowed to say so.
    ///
    /// # Errors
    ///
    /// [`Error::IllegalTransition`] when this camera holds no take at all — the module header
    /// argues why that is a refusal rather than a shrug. [`Error::Busy`] when a `record_start`
    /// for this camera is still negotiating, which means retry for the reason it does there.
    /// [`Error::DeviceIo`] for a driver that ended without a result. Otherwise whatever ended
    /// the take, unchanged.
    pub async fn collect(&self, camera: &CameraId) -> Result<RecordReport> {
        self.reap(camera).await;
        let waiting = {
            let live = self.0.live.lock().await;
            match live.get(camera) {
                None => return Err(nothing_to_stop(camera)),
                Some(Slot::Starting(reservation)) => {
                    return Err(Error::Busy {
                        path: crate::preview::node_of(&reservation.info),
                        holders: Vec::new(),
                    });
                }
                Some(Slot::Ended(_)) => None,
                Some(Slot::Running(take)) => {
                    take.stopping.store(true, Ordering::Release);
                    Some((Arc::clone(take), take.done.subscribe()))
                }
            }
        };
        let asked = match waiting {
            Some((take, mut done)) => {
                // `Err` means every sender is gone, which is the driver task ending without
                // installing a result — a runtime that is shutting down. Not a device refusal
                // and not spelled like one (E3); the re-read below is what decides what to say.
                let _ = done.wait_for(|installed| *installed).await;
                Some(take)
            }
            None => None,
        };

        let mut live = self.0.live.lock().await;
        match live.remove(camera) {
            Some(Slot::Ended(finished)) => {
                self.0.running.send_replace(count_running(&live));
                finished.outcome
            }
            None => Err(nothing_to_stop(camera)),
            // Put back whatever was there — a `record_stop` that failed must not empty a slot
            // a later one could still collect — and then say which of the two it was.
            Some(slot) => {
                let ours = matches!(
                    (&slot, &asked),
                    (Slot::Running(running), Some(take)) if Arc::ptr_eq(running, take)
                );
                let refusal = if ours {
                    Error::DeviceIo {
                        operation: format!("collect the recording on {camera}"),
                        errno: None,
                        message: "the recording's driver ended without a result; this daemon is \
                                  shutting down"
                            .to_owned(),
                    }
                } else {
                    Error::Busy {
                        path: node_of_slot(&slot),
                        holders: Vec::new(),
                    }
                };
                live.insert(camera.clone(), slot);
                Err(refusal)
            }
        }
    }

    /// One frame's worth of device work, on the actor's thread.
    async fn turn(&self, actor: &CameraActor, live: &Arc<Live>) -> Result<Step> {
        let sink = Arc::clone(live);
        self.ask(actor, move |device| match engine::record::turn(device)? {
            engine::record::Turn::Idle => Ok(Step::Idle),
            // **The frame, as a value, and the two consumers of it.** This is the one place in
            // the daemon where a recording's bytes exist, it runs on the camera's own thread
            // with the device open, and it is where the preview fan-out joins — which is the
            // owner's 2026-08-14 ruling in three lines (note **N117**).
            //
            // The muxer first, by reference; the viewers second, by **move**. So the ordering is
            // the ownership: the container copies what it needs and the frame then *becomes* the
            // viewers' `Shot` with no second copy of the bytes anywhere in this process. A frame
            // `absorb` refused never reaches `show`, because the `?` is what a cap and a disk
            // both come out of and the take is ending inside this turn either way.
            engine::record::Turn::Frame(frame) => {
                let step = sink.absorb(&frame)?;
                sink.watchers.show(frame);
                Ok(step)
            }
        })
        .await
    }

    /// Run `work` on `actor`'s thread and wait for its answer.
    ///
    /// The N41 shape, spelled here because N41's answer *is* that every caller spells it: the
    /// actor names no reply channel, so a caller brings one whose `send` is synchronous and
    /// non-blocking. `crate::preview::Previews::ask` writes the same six lines, and neither is
    /// a copy of a rule — the rule is that the engine holds no runtime, and this is what it
    /// looks like from a caller that has one.
    async fn ask<T, F>(&self, actor: &CameraActor, work: F) -> Result<T>
    where
        F: FnOnce(OpenCamera<'_>) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let (answered, answer) = oneshot::channel::<Result<T>>();
        actor.submit(self.0.clock.now_ms(), move |device| {
            let outcome = device.and_then(work);
            engine::actor::answering(move || {
                // Nobody left to tell: the driver ended while this frame was being taken,
                // which is what a cancelled daemon looks like from the actor's side.
                let _ = answered.send(outcome);
            })
        })?;
        match answer.await {
            Ok(answer) => answer,
            Err(_) => Err(actor.device_gone()),
        }
    }
}

/// The node a refusal about `slot` names.
///
/// One function because every slot knows which camera it is about — a refusal that named the
/// camera the *caller* asked about would be naming a device this daemon may no longer be able
/// to enumerate (note **N173**).
fn node_of_slot(slot: &Slot) -> Utf8PathBuf {
    match slot {
        Slot::Starting(reservation) => crate::preview::node_of(&reservation.info),
        Slot::Running(take) => crate::preview::node_of(&take.info),
        Slot::Ended(finished) => crate::preview::node_of(&finished.info),
    }
}

/// How many slots hold a running take.
///
/// Derived from the map rather than kept beside it, so the count and the thing it counts
/// cannot disagree — `Slot::Starting` and `Slot::Ended` are both in the map and neither is a
/// recording in progress.
fn count_running(live: &BTreeMap<CameraId, Slot>) -> usize {
    live.values()
        .filter(|slot| matches!(slot, Slot::Running(_)))
        .count()
}

/// The refusal for a `record_stop` on a camera holding nothing.
///
/// Its own function because the sentence has to carry the remedy: AGENTS' primary consumer has
/// no hands, so a refusal that says only "no" costs a retry of the same call.
fn nothing_to_stop(camera: &CameraId) -> Error {
    Error::IllegalTransition {
        from: format!("no recording on {camera}"),
        op: "stop a recording; this camera has none running and none waiting to be collected \
             — start one with record_start, and poll record_status until it is over"
            .to_owned(),
    }
}

impl Live {
    /// How long this take has run, on the daemon's clock.
    fn elapsed(&self, clock: &dyn Clock) -> u64 {
        clock.now_ms().saturating_sub(self.started_ms)
    }

    /// What `record_status` says about this take while it is running.
    fn status(&self, clock: &dyn Clock) -> TakeStatus {
        TakeStatus {
            path: self.path.clone(),
            format: self.format,
            negotiated: self.negotiated.clone(),
            started_at: self.started_at,
            budget_ms: self.budget_ms,
            elapsed_ms: self.elapsed(clock),
            frames_written: self.frames.load(Ordering::Relaxed),
            ended: None,
            failed: None,
        }
    }

    /// Append one frame to this take's container.
    ///
    /// **On the camera's actor thread**, inside the command that dequeued the frame — see the
    /// module header for why that placement is the point rather than an optimisation.
    ///
    /// # Errors
    ///
    /// Whatever `engine::record::Recording::write` refuses with: [`Error::StorageIo`] naming
    /// the file for a sink that refused, and the muxer's own [`Error::DeviceIo`] for a frame
    /// this open container cannot carry. The two are different findings — a disk and a driver
    /// — and AGENTS rule 7 is the line between them.
    fn absorb(&self, frame: &Frame) -> Result<Step> {
        let mut held = lock(&self.recording);
        let Some(recording) = held.as_mut() else {
            // Unreachable while [`drive`] is the only thing that takes the muxer, and written
            // rather than assumed away for AGENTS rule 6's reason: a build that changed who
            // may take it must fail here rather than drop a frame silently.
            return Err(Error::DeviceIo {
                operation: format!("record a frame on {camera}", camera = self.info.id),
                errno: None,
                message: "the recording's container was closed while its driver was still \
                          feeding it"
                    .to_owned(),
            });
        };
        match recording.write(frame)? {
            FrameOutcome::Written => {
                // Saturating rather than wrapping. `limits::MAX_RECORDING_FRAMES` is 16 384
                // and the container refuses every frame past it, so the top of a `u32` is out
                // of reach — but a count that wrapped to zero would tell an agent polling
                // `record_status` that its recording had just started, and "unreachable" is
                // not a reason to write the arithmetic that would say so.
                let _ = self
                    .frames
                    .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |seen| {
                        Some(seen.saturating_add(1))
                    });
                Ok(Step::Wrote)
            }
            FrameOutcome::Refused(_) => Ok(Step::Capped),
        }
    }
}

/// Take a poisoned lock's value anyway.
///
/// A poisoned mutex here means a panic while a frame was being written, and what is behind it
/// is a muxer that still has to be closed so the file stays parseable — `engine::record`'s own
/// `lock` helper makes the identical choice for the identical reason.
fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// One camera's recording, from its first turn to its closed container.
///
/// A task rather than a loop inside the request handler, because the take outlives the call
/// that started it: `record_start` answers as soon as the header is on disk, and everything
/// after that happens here.
///
/// The order on the way out is deliberate and each step is argued where it is written: stop the
/// stream, close the container, install the result. Stopping first gives the camera back before
/// a file is closed; closing before installing means the take a caller collects is one whose
/// file is finished.
async fn drive(recordings: Recordings, actor: Arc<CameraActor>, live: Arc<Live>) {
    let camera = live.info.id.clone();
    let ended = pump(&recordings, &actor, &live).await;

    // The stream, given back before anything else. A stop that fails is a warning and not a
    // second mechanism for ending a take (`crate::preview::drive`'s posture): the camera is
    // open and not streaming for anybody, and D12's idle close takes the descriptor whatever
    // this said.
    if let Err(err) = recordings.ask(&actor, engine::record::stop).await {
        tracing::debug!(%camera, error = %err, "the recording's stream could not be stopped");
    }

    // The camera's frames, given back the moment the device is free and **before** the container
    // is closed: a close seeks, writes an index and flushes, and the owner's tab should not wait
    // for a disk to get its picture back. `crate::preview::Previews::release` is what decides
    // whether that means a fresh preview driver or a feed nobody wants any more — including on
    // the path this take failed on, where the resume meets the same dead device and ends the
    // readers' streams through the mechanism that already exists for it.
    live.watchers.hand_back().await;

    let (status, outcome) = close(&recordings, &live, ended).await;

    let mut map = recordings.0.live.lock().await;
    // Only if the slot is still this take's: a `record_stop` collects by *removing*, and a
    // driver that re-inserted afterwards would resurrect a take somebody already has.
    if matches!(map.get(&camera), Some(Slot::Running(running)) if Arc::ptr_eq(running, &live)) {
        map.insert(
            camera.clone(),
            Slot::Ended(Box::new(Finished {
                info: live.info.clone(),
                status,
                outcome,
            })),
        );
    }
    recordings.0.running.send_replace(count_running(&map));
    recordings
        .0
        .finished
        .send_modify(|count| *count = count.saturating_add(1));
    drop(map);
    // Set **after** the slot is installed, so a `record_stop` woken by it finds the result
    // rather than an empty map. The two orderings are not interchangeable and this is the one
    // that makes the wait exact.
    live.done.send_replace(true);
}

/// Close the container and work out what to tell whoever collects this take.
///
/// Split from [`drive`] so that "what a take turned out to be" is a value with a name rather
/// than four branches inside a function that also stops streams and edits a registry.
///
/// The close runs on a **blocking pool thread**: it seeks, writes an index and flushes, and
/// this daemon parks no runtime worker on a file (`crate::server::Wchd::offload`'s doctrine).
/// A pool that has already gone — a runtime shutting down — leaves the container unclosed, and
/// what is on disk is then D7's recoverable `movi` prefix, which is the property that exists
/// for exactly this case.
async fn close(
    recordings: &Recordings,
    live: &Arc<Live>,
    ended: Result<RecordingEnd>,
) -> (TakeStatus, std::result::Result<RecordReport, Error>) {
    let clock = recordings.0.clock.clone();
    let elapsed = live.elapsed(&clock);
    let frames = live.frames.load(Ordering::Relaxed);
    // `DeviceFailed` for a take the device or the disk ended, which is what P6b's ordering is
    // for: the container is closed either way, and the caller is then handed the refusal
    // rather than a report (AGENTS rule 7).
    let (ending, refusal) = match ended {
        Ok(ending) => (ending, None),
        Err(err) => (RecordingEnd::DeviceFailed, Some(err)),
    };

    let taken = lock(&live.recording).take();
    let finished = match taken {
        None => Err(Error::DeviceIo {
            operation: format!("close the recording on {camera}", camera = live.info.id),
            errno: None,
            message: "the recording's container had already been taken".to_owned(),
        }),
        Some(recording) => {
            match tokio::task::spawn_blocking(move || recording.finish(ending, &clock)).await {
                Ok(answer) => answer,
                Err(err) => Err(Error::StorageIo {
                    path: live.path.clone(),
                    errno: None,
                    message: format!(
                        "the container could not be closed because this daemon is stopping: \
                         {err}"
                    ),
                }),
            }
        }
    };

    // The status a later `record_status` reads, built from what was measured rather than from
    // the report — which may not exist, because a take can fail and a close can fail after it.
    let status = TakeStatus {
        path: live.path.clone(),
        format: live.format,
        negotiated: live.negotiated.clone(),
        started_at: live.started_at,
        budget_ms: live.budget_ms,
        elapsed_ms: elapsed,
        frames_written: frames,
        ended: Some(ending),
        // The take's own refusal first, and the close's only if there was none: a take that
        // failed and *then* could not be flushed has one interesting cause and this is it.
        // The **ending** stays whatever the loop decided either way — a take that ran its
        // duration ran its duration, and rewriting that to `DeviceFailed` because a flush
        // refused would lose the one fact the vocabulary carries (AGENTS rule 7).
        failed: refusal
            .as_ref()
            .map(Error::kind)
            .or_else(|| finished.as_ref().err().map(Error::kind)),
    };

    // The device's refusal outranks the close's: the take failed first, and the close is what
    // this daemon did about it. A caller told `StorageIo` for a camera that was unplugged
    // would go and look at the disk.
    let outcome = match (refusal, finished) {
        (Some(refused), _) => Err(refused),
        (None, answer) => answer,
    };
    (status, outcome)
}

/// The loop: one frame per turn, until something says stop.
///
/// Four ways out, and each is argued where `engine::record::drive` argues its three — this is
/// that function's shape with the actor between it and the device, and with a fourth exit the
/// in-process one cannot have.
///
/// - the caller's **duration** is spent, checked *before* each turn on the daemon's clock, so
///   a budget of zero records a header and no frames rather than one frame;
/// - a **cap** refused a frame, at which point the loop stops asking the device — a take that
///   went on streaming after its size cap would be a bug the summary could not show;
/// - `limits::RECORDING_MAX_EMPTY_TURNS` turns in a row brought **nothing**;
/// - and the fourth: somebody said **stop** — `record_stop`, or the daemon's own shutdown.
///
/// A camera whose command queue is full is counted **separately** from one that delivered
/// nothing, for `crate::preview::pump`'s reason: E3 keeps "busy" and "cannot" apart, and a
/// camera alternating between the two must not end a take on a mixture of the two reasons.
/// When that budget is spent the take ends on the device's own [`Error::Busy`], which means
/// *retry* — the honest advice for a client whose camera is being driven by somebody else.
///
/// # Errors
///
/// The device's, unchanged, and the file's from [`Live::absorb`]. A caller holding one of those
/// still holds a container that has to be closed; [`close`] is what does that on every path.
async fn pump(
    recordings: &Recordings,
    actor: &CameraActor,
    live: &Arc<Live>,
) -> Result<RecordingEnd> {
    let mut idle: u32 = 0;
    let mut deferred: u32 = 0;
    loop {
        if recordings.0.shutdown.is_cancelled() || live.stopping.load(Ordering::Acquire) {
            return Ok(RecordingEnd::Stopped);
        }
        if live.elapsed(&recordings.0.clock) >= live.budget_ms {
            return Ok(RecordingEnd::Duration);
        }

        let step = tokio::select! {
            // Biased so a cancellation that arrives while a frame is available still ends the
            // take: an unbiased `select!` would pick at random, and "open streams are
            // cancelled, never awaited" (AGENTS) is not a claim to leave to a coin.
            biased;
            () = recordings.0.shutdown.cancelled() => return Ok(RecordingEnd::Stopped),
            step = recordings.turn(actor, live) => step,
        };

        match step {
            Ok(Step::Wrote) => {
                idle = 0;
                deferred = 0;
            }
            Ok(Step::Capped) => return Ok(RecordingEnd::Cap),
            Ok(Step::Idle) => {
                idle = idle.saturating_add(1);
                if idle >= limits::RECORDING_MAX_EMPTY_TURNS {
                    return Ok(RecordingEnd::DeviceQuiet);
                }
            }
            Err(err) if err.kind() == ErrorKind::Busy => {
                deferred = deferred.saturating_add(1);
                if deferred >= limits::RECORDING_MAX_EMPTY_TURNS {
                    return Err(err);
                }
            }
            Err(err) => return Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::task::Poll;

    use schema::backend::CameraBackend;
    use schema::camera::{FrameInterval, PixelFormat};

    use super::*;

    /// A registry with nothing driving it, for the assertions that are about the map.
    ///
    /// Over the **same** `Cameras` as its fan-out, which is the composition
    /// `crate::server::Wchd::with_idle_timeout` builds: a reservation reaches
    /// `Previews::hand_over`, so a fixture that handed the two registries different camera
    /// registries would be testing an arrangement this daemon does not have.
    fn registry() -> Recordings {
        let cameras = Arc::new(Cameras::new(Arc::new(
            fake::FakeBackend::new(Vec::new()).expect("a backend replaying no cameras"),
        )));
        let clock = MonotonicClock::new();
        let shutdown = Shutdown::new();
        Recordings::new(
            Arc::clone(&cameras),
            crate::preview::Previews::new(cameras, clock.clone(), shutdown.clone()),
            clock,
            shutdown,
        )
    }

    fn camera() -> CameraInfo {
        testkit::fixtures::synthetic_basic().invariant.info
    }

    /// A registry whose backend actually **has** the camera, and that camera.
    ///
    /// [`registry`] replays nothing, which is all the assertions about the map need and is not
    /// enough for [`Recordings::adopt`]: that call opens the camera's actor before it touches
    /// the registry, so over an empty backend it would take its refusal path and never reach
    /// the transition under test.
    fn recording_registry() -> (Recordings, CameraInfo) {
        let backend = Arc::new(
            fake::FakeBackend::from_profile(testkit::fixtures::synthetic_basic())
                .expect("the synthetic profile is this build's version"),
        );
        let info = backend
            .enumerate()
            .expect("the fake enumerates what it replays")
            .first()
            .cloned()
            .expect("one profile is one camera");
        let cameras = Arc::new(Cameras::new(
            backend as Arc<dyn schema::backend::CameraBackend>,
        ));
        let clock = MonotonicClock::new();
        let shutdown = Shutdown::new();
        let recordings = Recordings::new(
            Arc::clone(&cameras),
            crate::preview::Previews::new(cameras, clock.clone(), shutdown.clone()),
            clock,
            shutdown,
        );
        (recordings, info)
    }

    /// What `engine::record::start` would have negotiated, without a device having been asked.
    ///
    /// Built by hand for [`live_take`]'s reason: the subjects below are the registry's own
    /// transitions, and driving a real negotiation would put a device in front of a decision
    /// that is three lines of `match`. `engine::record::Opened`'s fields are public for
    /// exactly this.
    fn an_opened() -> Opened {
        let negotiated = a_take().negotiated;
        Opened {
            camera: camera().id,
            negotiated: negotiated.clone(),
            format: VideoFormat::Avi,
            params: imaging::video::RecordingParams {
                width: negotiated.width,
                height: negotiated.height,
                pixel_format: negotiated.pixel_format,
                negotiated_interval_us: Some(33_333),
                caps: engine::record::caps(),
            },
            budget_ms: limits::DEFAULT_RECORDING_MS,
        }
    }

    fn a_take() -> TakeStatus {
        TakeStatus {
            path: "/tmp/take.avi".into(),
            format: VideoFormat::Avi,
            negotiated: NegotiatedStream {
                pixel_format: PixelFormat::MJPG,
                width: 64,
                height: 48,
                bytes_per_line: 0,
                size_image: 4096,
                interval: FrameInterval::Discrete {
                    numerator: 1,
                    denominator: 30,
                },
                adjustments: Vec::new(),
            },
            started_at: Stamp::epoch(),
            budget_ms: limits::DEFAULT_RECORDING_MS,
            elapsed_ms: 12,
            frames_written: 3,
            ended: Some(RecordingEnd::Duration),
            failed: None,
        }
    }

    /// A take with a real container under it, into `path`.
    ///
    /// Built by hand rather than driven, because what the two tests below are about is
    /// [`close`] — the function that decides *what a caller collects* — and driving a device
    /// to a mid-take refusal through an actor would be a fixture for a decision that is three
    /// lines of `match`. `engine::record::Opened`'s fields are public for exactly this reason
    /// (its own doc says so), and `engine::record::OnDisk` is what a real take writes through.
    async fn live_take(recordings: &Recordings, path: &Utf8Path) -> Arc<Live> {
        let opened = an_opened();
        let recording = Recording::begin(
            &opened,
            path,
            &mut engine::record::OnDisk,
            &MonotonicClock::new(),
            Stamp::epoch(),
        )
        .expect("a writable scratch path and a container that carries MJPG");
        Arc::new(Live {
            info: camera(),
            // A real claim on a real fan-out, taken the way `reserve` takes one. Nobody is
            // watching it, which is what makes it cheap: `Watchers::show` is never reached from
            // these tests, and `close` — the subject below — does not touch it.
            watchers: recordings.0.previews.hand_over(&camera()).await,
            path: path.to_owned(),
            format: opened.format,
            negotiated: opened.negotiated.clone(),
            started_at: Stamp::epoch(),
            started_ms: 0,
            budget_ms: opened.budget_ms,
            frames: AtomicU32::new(0),
            recording: std::sync::Mutex::new(Some(recording)),
            stopping: AtomicBool::new(false),
            done: watch::Sender::new(false),
        })
    }

    #[tokio::test]
    async fn a_take_the_device_refused_is_collected_as_that_refusal_and_never_as_a_report() {
        // **AGENTS rule 7, at the one place in this daemon that could break it.** A recording
        // whose camera vanished mid-take still gets its container closed — docs/7 P6b's "every
        // fault leaves a parseable file", which holds across the wire exactly as it does in
        // process — and the close *succeeds*. So there are two answers in hand at that moment,
        // a device refusal and a finished report, and handing over the report would be this
        // daemon deciding a `DeviceGone` was a successful recording.
        //
        // A unit test rather than an integration one because the subject is `close`'s three
        // lines of `match`: a hand-applied mutant that preferred the close's answer survived
        // the whole workspace suite, which is what this test exists to stop (note **N115**,
        // mutant M9).
        let recordings = registry();
        let scratch = engine::paths::TempRuntimeDir::new().expect("a throw-away directory");
        let path = scratch.base().join("vanished.avi");
        let live = live_take(&recordings, &path).await;

        let (status, outcome) = close(
            &recordings,
            &live,
            Err(Error::DeviceGone {
                path: "/dev/video0".into(),
            }),
        )
        .await;

        let refused = outcome.expect_err("a device that vanished is not a recording");
        assert_eq!(refused.kind(), ErrorKind::DeviceGone);
        assert_eq!(status.ended, Some(RecordingEnd::DeviceFailed));
        assert_eq!(status.failed, Some(ErrorKind::DeviceGone));
        // And the file is still one the strict reader accepts, which is the half that makes
        // the refusal affordable: the caller is told the take failed *and* handed a file it
        // can open.
        let bytes = std::fs::read(&path).expect("the container was closed");
        imaging::avi::read::read_stream(&bytes)
            .expect("a take that died on the device still gets its index written");

        // The other direction, so the arm above refuses a *device failure* rather than
        // refusing takes: a loop that ended cleanly is collected as a report, and the status
        // names the ending it reached with no refusal beside it.
        let live = live_take(&recordings, &scratch.base().join("clean.avi")).await;
        let (status, outcome) = close(&recordings, &live, Ok(RecordingEnd::Duration)).await;
        let report = outcome.expect("a take that ended on its own duration is a recording");
        assert_eq!(report.ended, RecordingEnd::Duration);
        assert_eq!(status.ended, Some(RecordingEnd::Duration));
        assert_eq!(status.failed, None);
    }

    #[tokio::test]
    async fn a_second_start_on_a_camera_that_is_already_recording_is_told_to_retry() {
        // The refusal an unattended agent has to be able to act on: `Busy` means retry, and
        // retrying is what succeeds — the take that is running is bounded by its own duration.
        // Both directions, because a reservation that never refused would let two drivers
        // feed one muxer.
        let recordings = registry();
        let info = camera();
        let first = recordings.reserve(&info).await.expect("a free camera");

        let refused = recordings
            .reserve(&info)
            .await
            .expect_err("the camera is taken");
        assert_eq!(refused.kind(), ErrorKind::Busy);
        assert!(
            !matches!(&refused, Error::Busy { holders, .. } if !holders.is_empty()),
            "a Busy naming this daemon's pid invites a client to kill it: {refused:?}"
        );

        // And the slot comes back, so the refusal is a state rather than a latch.
        recordings.withdraw(first).await;
        recordings
            .reserve(&info)
            .await
            .expect("the camera was given back");
    }

    #[tokio::test]
    async fn a_record_start_that_went_away_before_its_take_began_leaves_the_camera_free() {
        // Note **N171**, at its plainest: the handler future that took this slot is *dropped*
        // rather than refused, so neither `withdraw` nor `adopt` runs and a rule that lives on
        // those two paths has been skipped. Both claims have to come back — the slot, or every
        // later `record_start` on this camera answers `Busy` for the life of the process, and
        // the camera's frames, or an open tab watches a feed nothing publishes into.
        //
        // Nothing here waits for the `Drop`'s task: the assertions go through the entry points
        // a client would use, which is where the reap that makes this true actually lives.
        let recordings = registry();
        let info = camera();

        let reserved = recordings.reserve(&info).await.expect("a free camera");
        assert_eq!(
            recordings.0.previews.feeds(),
            1,
            "the reservation did not claim this camera's frames, so the half below proves nothing"
        );

        drop(reserved);

        assert!(
            recordings.status(&info.id).await.take.is_none(),
            "an abandoned reservation is still being reported as a take"
        );
        assert_eq!(
            recordings.0.previews.feeds(),
            0,
            "the abandoned reservation kept this camera's frames"
        );
        let again = recordings
            .reserve(&info)
            .await
            .expect("an abandoned reservation is not a recording");
        // And the second reservation is a real one rather than a leftover: it holds the frames
        // and it can be given back the ordinary way.
        assert_eq!(recordings.0.previews.feeds(), 1);
        recordings.withdraw(again).await;
        assert_eq!(recordings.0.previews.feeds(), 0);
    }

    #[tokio::test]
    async fn a_record_start_dropped_inside_the_hand_over_still_gives_the_camera_back() {
        // The window docs/11 §4.10 tried twice to hit against a live daemon and could not,
        // because over the fake the reserve→running interval is a few milliseconds. So it is
        // **constructed** rather than raced: the reservation's future is polled exactly once —
        // which is enough to claim the slot and spawn the hand-over, and not enough to finish
        // it — and then dropped, which is what a client hanging up looks like from here.
        //
        // This is the widest part of a `record_start` on real hardware (a `STREAMOFF` behind a
        // preview driver), and it is the one place where the claim exists in a *call* rather
        // than in a field. Note **N171**'s answer is that the call is not the caller's: it runs
        // on a task of its own, finds nobody wanting what it fetched, and puts both halves
        // back.
        let recordings = registry();
        let info = camera();

        let mut starting = Box::pin(recordings.reserve(&info));
        let first = std::future::poll_fn(|cx| Poll::Ready(starting.as_mut().poll(cx))).await;
        assert!(
            first.is_pending(),
            "the reservation finished in one poll, so this test constructed no window"
        );
        assert!(
            matches!(
                recordings.0.live.lock().await.get(&info.id),
                Some(Slot::Starting(_))
            ),
            "the slot was not claimed before the hand-over, which is the order reserve promises"
        );

        drop(starting);

        // The hand-over task is not this test's to await, so what is waited on is the state it
        // leaves behind. Nothing sleeps and nothing is timed — and the bound is a **poll count**
        // rather than a duration, which is exact here for the reason note **N178** gives: the
        // thing being waited for is a task on this runtime, which `#[tokio::test]` runs on this
        // thread, so "it has not happened after this many turns of the scheduler" is a fact
        // about the build and not about how loaded the host is. A build whose task never ran
        // fails by name instead of burning a core to nextest's deadline.
        let mut turns = 0;
        while recordings.0.live.lock().await.contains_key(&info.id) {
            turns += 1;
            assert!(
                turns < 1_024,
                "the cancelled record_start's slot never came back"
            );
            tokio::task::yield_now().await;
        }
        assert_eq!(
            recordings.0.previews.feeds(),
            0,
            "the cancelled hand-over left this camera's frames claimed by nobody"
        );
        recordings
            .reserve(&info)
            .await
            .expect("a cancelled record_start is not a recording");
    }

    #[tokio::test]
    async fn a_camera_whose_take_is_being_adopted_is_never_reported_free_to_the_next_caller() {
        // **Note N176.** By the time `adopt` runs, `engine::record::start` has already reached
        // `VIDIOC_STREAMON` — the device is streaming. So the registry must go from
        // `Slot::Starting` to `Slot::Running` without ever being *empty*, because every other
        // verb reads exactly that map to decide whether this camera is held: an empty slot
        // admits a photo whose suspend/resume stops the take's stream (N118, N170), answers a
        // `record_stop` with a terminal `IllegalTransition` where the retryable `Busy` is due,
        // and lets a second `record_start` claim a node V4L2 allows one streamer on.
        //
        // The interleaving is **constructed** and not raced, which is docs/11 §4.10's whole
        // point: this test holds the registry lock, parks `adopt` on it, queues a second caller
        // *behind* `adopt` — `tokio::sync::Mutex` hands the lock on in the order it was asked
        // for — and then lets go. Whatever `adopt` leaves in the map when it next releases the
        // lock is exactly what that second caller sees, whether `adopt` has finished or not.
        let (recordings, info) = recording_registry();
        let scratch = engine::paths::TempRuntimeDir::new().expect("a throw-away directory");
        let path = scratch.base().join("adopting.avi");
        let opened = an_opened();
        let recording = Recording::begin(
            &opened,
            &path,
            &mut engine::record::OnDisk,
            &MonotonicClock::new(),
            Stamp::epoch(),
        )
        .expect("a writable scratch path and a container that carries MJPG");
        let reserved = recordings.reserve(&info).await.expect("a free camera");

        let held = recordings.0.live.lock().await;
        let mut adopting =
            Box::pin(recordings.adopt(reserved, &info, &opened, recording, &path, Stamp::epoch()));
        assert!(
            std::future::poll_fn(|cx| Poll::Ready(adopting.as_mut().poll(cx)))
                .await
                .is_pending(),
            "adopt did not stop at the registry lock this test is holding, so nothing below is \
             ordered against it"
        );

        // The next caller in the queue, and it is the one N118 is about: `wch_photo` asks this
        // exact question from the camera's own actor thread before it suspends anything.
        let asking = tokio::spawn({
            let recordings = recordings.clone();
            let camera = info.id.clone();
            async move { recordings.not_recording(&camera).await }
        });
        // One turn of the scheduler is what puts that task *in* the queue rather than merely
        // spawned; it is behind `adopt`, which asked first.
        tokio::task::yield_now().await;
        drop(held);

        // One poll of `adopt`: enough to take the reservation out, and — on a build whose
        // transition is one mutation — to put the take in and answer. On a build that releases
        // and re-acquires, this is where the map is empty and the lock is somebody else's.
        let advanced = std::future::poll_fn(|cx| Poll::Ready(adopting.as_mut().poll(cx))).await;

        let seen = asking.await.expect("the scripted second caller");
        let refused = seen.expect_err(
            "this camera was reported free while its take's stream was already running",
        );
        assert_eq!(refused.kind(), ErrorKind::Busy, "{refused:?}");

        let status = match advanced {
            Poll::Ready(answer) => answer,
            Poll::Pending => adopting.await,
        };
        let started = status.expect("a reservation this test took and nobody else touched");
        assert!(started.take.is_some(), "{started:?}");
        // And the take is collectable afterwards, so the arm above ordered a transition rather
        // than wedging one. The container is closed on the way out whatever the loop met.
        let _ = recordings.collect(&info.id).await;
    }

    #[tokio::test]
    async fn a_stop_whose_take_somebody_else_took_says_which_and_never_blames_a_shutdown() {
        // Note **N172**. `collect` releases the registry lock to wait for the driver, so the
        // slot it comes back to is not always the one it left — and a single catch-all told
        // every one of those callers that this daemon was shutting down, which on a healthy
        // daemon is a sentence about the wrong machine. Three shapes, three answers, and the
        // third is the one the sentence was always true of.
        let recordings = registry();
        let info = camera();
        let scratch = engine::paths::TempRuntimeDir::new().expect("a throw-away directory");

        // 1. A second `record_stop` got there first: this camera now holds nothing, which is
        //    the answer `record_stop` already has for it.
        let take = live_take(&recordings, &scratch.base().join("collected.avi")).await;
        let refused = stopped_while(&recordings, &info, &take, |live| {
            live.remove(&info.id);
        })
        .await;
        assert_eq!(refused.kind(), ErrorKind::IllegalTransition);
        let rendered = refused.to_string();
        assert!(rendered.contains("record_start"), "{rendered}");

        // 2. A `record_start` arrived while this call was waiting and took the camera —
        //    discarding the finished take it found, counted, which is the module header's
        //    second bullet. `Busy` means retry, and the take that took the slot is bounded by
        //    its own duration.
        let take = live_take(&recordings, &scratch.base().join("replaced.avi")).await;
        let claimant = Arc::new(Holding);
        let refused = stopped_while(&recordings, &info, &take, |live| {
            live.insert(
                info.id.clone(),
                Slot::Starting(Box::new(Reservation {
                    info: camera(),
                    holder: Arc::downgrade(&claimant),
                    watchers: None,
                })),
            );
        })
        .await;
        assert_eq!(
            refused.kind(),
            ErrorKind::Busy,
            "a slot somebody else filled was reported as this daemon shutting down: {refused}"
        );

        // 3. And the shape the sentence is true of, so the two above are a narrowing rather
        //    than a deletion: this call's *own* take is still in the slot, which means its
        //    driver ended without installing a result.
        let take = live_take(&recordings, &scratch.base().join("driverless.avi")).await;
        let refused = stopped_while(&recordings, &info, &take, |_| {}).await;
        assert_eq!(refused.kind(), ErrorKind::DeviceIo);
        assert!(refused.to_string().contains("shutting down"), "{refused}");
        assert!(
            matches!(
                recordings.0.live.lock().await.get(&info.id),
                Some(Slot::Running(_))
            ),
            "a stop that could not collect emptied a slot a later one could still collect"
        );
    }

    /// Run a `record_stop` over `take`, and change the registry underneath it while it waits.
    ///
    /// The interleaving is **constructed** rather than raced: `collect` subscribes to the
    /// take's `done` before it waits, so a receiver on that channel is the exact signal that
    /// this call has passed its first `match` and released the lock. `edit` then runs while it
    /// is parked, and the driver's own signal releases it.
    async fn stopped_while(
        recordings: &Recordings,
        info: &CameraInfo,
        take: &Arc<Live>,
        edit: impl FnOnce(&mut BTreeMap<CameraId, Slot>),
    ) -> Error {
        recordings
            .0
            .live
            .lock()
            .await
            .insert(info.id.clone(), Slot::Running(Arc::clone(take)));
        let stopping = tokio::spawn({
            let recordings = recordings.clone();
            let camera = info.id.clone();
            async move { recordings.collect(&camera).await }
        });
        // A poll bound and not a duration, and the difference from note **N178**'s other loop is
        // what makes it exact: the thing being waited for is a **task on this runtime**, which
        // `#[tokio::test]` runs on this thread, so "it has not happened after this many turns of
        // the scheduler" is a fact about the build rather than about how loaded the host is. It
        // takes three; the bound is generous and its failure is named.
        let mut turns = 0;
        while take.done.receiver_count() == 0 {
            turns += 1;
            assert!(
                turns < 1_024,
                "the scripted record_stop never reached its wait, so nothing below is ordered \
                 against it"
            );
            tokio::task::yield_now().await;
        }
        edit(&mut *recordings.0.live.lock().await);
        take.done.send_replace(true);
        stopping
            .await
            .expect("the scripted record_stop")
            .expect_err("none of these three shapes is a report")
    }

    #[tokio::test]
    async fn a_start_over_an_uncollected_take_discards_it_and_says_how_many_it_has_discarded() {
        // The module header's second bullet, in both halves: the take is replaced rather than
        // refused — an abandoned poll loop must not wedge a camera — and the loss is a number
        // rather than a silence (rubric rule 3). A build that refused instead leaves the
        // count at zero and fails the reservation.
        let recordings = registry();
        let info = camera();
        let mut discarded = recordings.watch_discarded();
        assert_eq!(*discarded.borrow_and_update(), 0);

        recordings.0.live.lock().await.insert(
            info.id.clone(),
            Slot::Ended(Box::new(Finished {
                info: camera(),
                status: a_take(),
                outcome: Err(Error::HolderGone { pid: 1 }),
            })),
        );

        let reserved = recordings
            .reserve(&info)
            .await
            .expect("an uncollected take is not a running one");
        assert_eq!(reserved.info.id, info.id);
        assert_eq!(*discarded.borrow_and_update(), 1);
        assert!(
            recordings.status(&info.id).await.take.is_none(),
            "the discarded take is still being reported"
        );
    }

    #[tokio::test]
    async fn a_stop_on_a_camera_with_no_take_names_the_verb_that_would_have_made_one() {
        // Not a bland success: an unattended caller has to be able to tell "my recording is
        // over" from "my record_start never took", and those are this call's two answers. The
        // remedy is in the sentence because AGENTS' primary consumer has no hands.
        let recordings = registry();
        let info = camera();
        let refused = recordings
            .collect(&info.id)
            .await
            .expect_err("this camera holds nothing");
        assert_eq!(refused.kind(), ErrorKind::IllegalTransition);
        let rendered = refused.to_string();
        assert!(rendered.contains("record_start"), "{rendered}");
        assert!(rendered.contains("record_status"), "{rendered}");

        // And the other direction, so the arm above refuses an absence rather than refusing
        // stops: a camera holding an ended take hands it over and empties the slot.
        recordings.0.live.lock().await.insert(
            info.id.clone(),
            Slot::Ended(Box::new(Finished {
                info: camera(),
                status: a_take(),
                outcome: Err(Error::HolderGone { pid: 7 }),
            })),
        );
        let collected = recordings
            .collect(&info.id)
            .await
            .expect_err("this take failed, and the failure is what it turned out to be");
        assert_eq!(collected.kind(), ErrorKind::HolderGone);
        assert!(
            recordings.status(&info.id).await.take.is_none(),
            "a collected take is still in the slot"
        );
        assert_eq!(
            recordings
                .collect(&info.id)
                .await
                .expect_err("collected once")
                .kind(),
            ErrorKind::IllegalTransition
        );
    }

    #[tokio::test]
    async fn a_camera_that_never_recorded_and_one_whose_take_was_collected_answer_alike() {
        // Two states, one answer, and it is a decision rather than an accident: what a caller
        // can do about either is start a recording. The assertion is that the *camera id* is
        // still the resolved one, because a status that lost it would make a poller unable to
        // tell whose answer it is holding.
        let recordings = registry();
        let info = camera();
        let status = recordings.status(&info.id).await;
        assert_eq!(status.camera, info.id);
        assert!(status.take.is_none());
        assert!(!status.is_running());
    }
}
