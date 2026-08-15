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
//! ## The frame is a value, and that is a place a second consumer joins
//!
//! `Live::absorb` takes the [`Frame`] `engine::record::turn` handed back and gives it to the
//! muxer. It runs **on the camera's actor thread**, inside the same command that dequeued the
//! frame, which is what makes the arrangement the notes' Expected usage item 10 asks for
//! possible at all: a recording and a preview collide on one camera, V4L2 allows one streamer
//! per node, and the owner's ruling is that the preview gets fed the recording's own frames.
//! That fan-out is the *next* sub-milestone's and is deliberately not built here — but the
//! frame is a named binding in a function that already runs where `crate::preview::Publisher`
//! runs, rather than a value swallowed by a closure, so adding the second consumer is adding a
//! line and not rearranging a loop.
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
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

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
    Starting,
    /// A take is running, and a driver task is feeding it.
    Running(Arc<Live>),
    /// A take has ended and nobody has collected it yet.
    Ended(Box<Finished>),
}

/// A take in progress: what it is, what it has produced, and the muxer under it.
///
/// Shared between the driver task and the actor thread's per-frame closure, which is why every
/// mutable field is an atomic or behind a `std::sync::Mutex` rather than owned by the loop.
struct Live {
    camera: CameraId,
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
            .field("camera", &self.camera)
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
    /// What `record_status` answers with — the same document a running take produces, with
    /// its ending filled in.
    status: TakeStatus,
    /// What `record_stop` answers with: the report, or the refusal that ended the take.
    ///
    /// A `Result` rather than a report and an optional error, because those are the two
    /// answers `record_stop` can give and a caller must not be able to receive both.
    outcome: std::result::Result<RecordReport, Error>,
}

/// The witness that a caller holds this camera's recording slot.
///
/// A value rather than a `bool` for `crate::preview::Starting`'s reason: the only way to get
/// one is to be the call that reserved the slot, so the obligation to release it travels in
/// the type rather than in a paragraph. It carries the camera so neither
/// [`Recordings::withdraw`] nor [`Recordings::adopt`] has to be told which slot it is about.
#[derive(Debug)]
pub struct Reserved {
    camera: CameraId,
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
    /// identical three for the identical reason.
    #[must_use]
    pub fn new(cameras: Arc<Cameras>, clock: MonotonicClock, shutdown: Shutdown) -> Recordings {
        Recordings(Arc::new(Registry {
            cameras,
            clock,
            shutdown,
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

    /// Take this camera's recording slot, or refuse.
    ///
    /// Called **before** anything device-shaped, so a second `record_start` is answered by this
    /// daemon rather than by the kernel's `EBUSY` after a file has been opened — see
    /// `Slot::Starting`.
    ///
    /// # Errors
    ///
    /// [`Error::Busy`] naming the camera's node when a take is already running on it or
    /// another `record_start` is negotiating one. The module header argues why that variant
    /// and why its `holders` list is empty.
    pub async fn reserve(&self, info: &CameraInfo) -> Result<Reserved> {
        let mut live = self.0.live.lock().await;
        match live.get(&info.id) {
            Some(Slot::Starting | Slot::Running(_)) => {
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
        live.insert(info.id.clone(), Slot::Starting);
        Ok(Reserved {
            camera: info.id.clone(),
        })
    }

    /// Give the slot back without a take in it.
    ///
    /// The path out of a `record_start` whose negotiation, container pairing or header write
    /// was refused. Written explicitly on that path rather than as a `Drop` on [`Reserved`],
    /// because releasing it takes the registry lock and a `Drop` cannot `await` —
    /// `crate::preview::Previews::attach` makes the same call at the same moment for the same
    /// reason.
    pub async fn withdraw(&self, reserved: Reserved) {
        let mut live = self.0.live.lock().await;
        // Only if the slot is still the reservation this call is about: a `record_start` that
        // was refused after a *later* one had already taken the slot must not remove the
        // later one's take. `crate::preview::remove`'s `Arc::ptr_eq` makes the same check for
        // the same reason, one variant shape along.
        if matches!(live.get(&reserved.camera), Some(Slot::Starting)) {
            live.remove(&reserved.camera);
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
    /// failing to compose itself, which is the variant `engine::actor` already uses for it.
    /// The slot is given back on that path, so a failure here leaves the camera exactly as
    /// this call found it.
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

        let live = Arc::new(Live {
            camera: reserved.camera.clone(),
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

        let status = {
            let mut map = self.0.live.lock().await;
            map.insert(reserved.camera.clone(), Slot::Running(Arc::clone(&live)));
            self.0.running.send_replace(count_running(&map));
            // Built under the lock so the answer a caller receives is the state the registry
            // is in, rather than one a driver may already have moved past.
            RecordStatus {
                camera: reserved.camera,
                take: Some(live.status(&self.0.clock)),
            }
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
        let live = self.0.live.lock().await;
        let take = match live.get(camera) {
            None | Some(Slot::Starting) => None,
            Some(Slot::Running(take)) => Some(take.status(&self.0.clock)),
            Some(Slot::Ended(finished)) => Some(finished.status.clone()),
        };
        RecordStatus {
            camera: camera.clone(),
            take,
        }
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
    /// # Errors
    ///
    /// [`Error::IllegalTransition`] when this camera holds no take at all — the module header
    /// argues why that is a refusal rather than a shrug. [`Error::Busy`] when a `record_start`
    /// for this camera is still negotiating, which means retry for the reason it does there.
    /// Otherwise whatever ended the take, unchanged.
    pub async fn collect(&self, info: &CameraInfo) -> Result<RecordReport> {
        let camera = &info.id;
        let waiting = {
            let live = self.0.live.lock().await;
            match live.get(camera) {
                None => return Err(nothing_to_stop(camera)),
                Some(Slot::Starting) => {
                    return Err(Error::Busy {
                        path: crate::preview::node_of(info),
                        holders: Vec::new(),
                    });
                }
                Some(Slot::Ended(_)) => None,
                Some(Slot::Running(take)) => {
                    take.stopping.store(true, Ordering::Release);
                    Some(take.done.subscribe())
                }
            }
        };
        if let Some(mut done) = waiting {
            // `Err` means every sender is gone, which is the driver task ending without
            // installing a result — a runtime that is shutting down. Not a device refusal and
            // not spelled like one (E3); the re-read below is what decides what to say.
            let _ = done.wait_for(|installed| *installed).await;
        }

        let mut live = self.0.live.lock().await;
        match live.remove(camera) {
            Some(Slot::Ended(finished)) => {
                self.0.running.send_replace(count_running(&live));
                finished.outcome
            }
            // The three shapes that mean the driver never installed a result. Put back
            // whatever was there — a `record_stop` that failed must not empty a slot a later
            // one could still collect — and answer with this process's own failure rather
            // than with anything about the camera.
            other => {
                if let Some(slot) = other {
                    live.insert(camera.clone(), slot);
                }
                Err(Error::DeviceIo {
                    operation: format!("collect the recording on {camera}"),
                    errno: None,
                    message: "the recording's driver ended without a result; this daemon is \
                              shutting down"
                        .to_owned(),
                })
            }
        }
    }

    /// One frame's worth of device work, on the actor's thread.
    async fn turn(&self, actor: &CameraActor, live: &Arc<Live>) -> Result<Step> {
        let sink = Arc::clone(live);
        self.ask(actor, move |device| match engine::record::turn(device)? {
            engine::record::Turn::Idle => Ok(Step::Idle),
            // **The frame, as a value.** This is the one place in the daemon where a
            // recording's bytes exist, it runs on the camera's own thread with the device
            // open, and it is where the preview fan-out joins: the same `frame` goes to
            // `crate::preview::Previews`' watch channel before it reaches the muxer, which is
            // the arrangement the owner ruled for when a recording and a preview collide.
            // Nothing here consumes it in a way that would make that a second capture.
            engine::record::Turn::Frame(frame) => sink.absorb(&frame),
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
                operation: format!("record a frame on {camera}", camera = self.camera),
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
    let camera = live.camera.clone();
    let ended = pump(&recordings, &actor, &live).await;

    // The stream, given back before anything else. A stop that fails is a warning and not a
    // second mechanism for ending a take (`crate::preview::drive`'s posture): the camera is
    // open and not streaming for anybody, and D12's idle close takes the descriptor whatever
    // this said.
    if let Err(err) = recordings.ask(&actor, engine::record::stop).await {
        tracing::debug!(%camera, error = %err, "the recording's stream could not be stopped");
    }

    let (status, outcome) = close(&recordings, &live, ended).await;

    let mut map = recordings.0.live.lock().await;
    // Only if the slot is still this take's: a `record_stop` collects by *removing*, and a
    // driver that re-inserted afterwards would resurrect a take somebody already has.
    if matches!(map.get(&camera), Some(Slot::Running(running)) if Arc::ptr_eq(running, &live)) {
        map.insert(
            camera.clone(),
            Slot::Ended(Box::new(Finished { status, outcome })),
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
            operation: format!("close the recording on {camera}", camera = live.camera),
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
    use schema::camera::{FrameInterval, PixelFormat};

    use super::*;

    /// A registry with nothing driving it, for the assertions that are about the map.
    fn registry() -> Recordings {
        Recordings::new(
            Arc::new(Cameras::new(Arc::new(
                fake::FakeBackend::new(Vec::new()).expect("a backend replaying no cameras"),
            ))),
            MonotonicClock::new(),
            Shutdown::new(),
        )
    }

    fn camera() -> CameraInfo {
        testkit::fixtures::synthetic_basic().invariant.info
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
    fn live_take(path: &Utf8Path) -> Arc<Live> {
        let negotiated = a_take().negotiated;
        let opened = Opened {
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
        };
        let recording = Recording::begin(
            &opened,
            path,
            &mut engine::record::OnDisk,
            &MonotonicClock::new(),
            Stamp::epoch(),
        )
        .expect("a writable scratch path and a container that carries MJPG");
        Arc::new(Live {
            camera: opened.camera.clone(),
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
        let live = live_take(&path);

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
        let live = live_take(&scratch.base().join("clean.avi"));
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
                status: a_take(),
                outcome: Err(Error::HolderGone { pid: 1 }),
            })),
        );

        let reserved = recordings
            .reserve(&info)
            .await
            .expect("an uncollected take is not a running one");
        assert_eq!(reserved.camera, info.id);
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
            .collect(&info)
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
                status: a_take(),
                outcome: Err(Error::HolderGone { pid: 7 }),
            })),
        );
        let collected = recordings
            .collect(&info)
            .await
            .expect_err("this take failed, and the failure is what it turned out to be");
        assert_eq!(collected.kind(), ErrorKind::HolderGone);
        assert!(
            recordings.status(&info.id).await.take.is_none(),
            "a collected take is still in the slot"
        );
        assert_eq!(
            recordings
                .collect(&info)
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
