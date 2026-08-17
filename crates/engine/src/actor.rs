//! The camera actors — one OS thread per open camera (design §2.1, D12).
//!
//! Design §2.1 states the model and the reason for it:
//!
//! > The engine owns each open camera through a dedicated OS thread (the *camera actor*):
//! > V4L2 ioctls and DQBUF block, so each camera gets one blocking thread with a command
//! > channel; the daemon's async tasks and the direct CLI both talk to the same actor API.
//! > One actor per camera serializes device access by construction — there is no "two
//! > writers negotiate" state.
//!
//! ## What makes the serialization structural rather than conventional
//!
//! The `Box<dyn Camera>` lives in one place: the local variable inside [`CameraActor`]'s
//! thread. Nothing hands it out — the only way to touch the device is to send a closure
//! that the thread runs, one at a time, in arrival order — and [`Cameras`] keys its
//! registry on [`CameraId`], so a second request for the same camera reaches the same
//! thread rather than a second handle onto the same node. There is no lock to forget and
//! no rule to break: "two callers write at once" is unrepresentable rather than prevented.
//!
//! The one thing this cannot promise is exclusivity against a *different* process, and it
//! must not pretend to: V4L2 allows many opens and one streamer per node, so another
//! program holding the sensor is still [`Error::Busy`] from the backend, which is a fact
//! about the machine and not a capability statement (E3).
//!
//! ## Why the reply channel belongs to the caller
//!
//! Two of the design's constraints meet here and together they pick the shape. §2.1 says
//! "the daemon's async tasks and the direct CLI both talk to the same actor API", so the
//! actor is the engine's; §2.8's crate inventory gives the engine `schema + imaging +
//! tempfile + fd-lock + tracing` and **no runtime**, so the actor cannot name a tokio
//! channel. A blocking `std` reply channel in its place would be worse than a missing
//! feature — it would make the daemon block a runtime worker on every request, which is
//! the thing `crates/api`'s trait doc rejects `#[method(blocking)]` for.
//!
//! So the actor names no reply channel at all. [`CameraActor::submit`] takes a closure that is
//! *given* the open device and closes over whatever the caller wants to answer through: the
//! daemon's handler closes over a `tokio::sync::oneshot::Sender`, whose `send` is synchronous
//! and non-blocking — exactly what a blocking thread needs — and awaits the receiver;
//! [`CameraActor::ask`] closes over a `std` channel for callers that have a thread to spare,
//! which is this module's own tests today and `webcam-handler-cli` when it stops opening a
//! camera per invocation. One actor API, two transports, no runtime in the engine. Note
//! **N41** records the measurement and the two readings that were rejected.
//!
//! ## Why every command carries the time
//!
//! Idle close is a deadline, and a deadline measured by reading a clock inside the actor
//! is a deadline no test can reach without waiting — which this project bans, in tests as
//! much as anywhere else. So the actor reads no clock: the caller stamps each command
//! from [`crate::settle::Clock`], the same seam and the same doctrine the settle policy
//! states ("the caller supplies both, which turns *the deadline expired between these two
//! frames* from a race into an argument"). The production driver reads
//! [`crate::settle::MonotonicClock`]; a test hands in whatever millisecond it wants to
//! talk about.
//!
//! [`crate::settle::SteppedClock`] is deliberately not `Sync` — "a stepped clock shared
//! across threads is a race dressed as a fixture" — so this was never a matter of sharing
//! one, and the tick-carries-the-time shape is what makes that decision cost nothing here.
//!
//! ## What is observable, and what is not yet
//!
//! [`Cameras::activity`] is the status surface docs/7 P4b asks for: which cameras have an
//! actor, whether each one's device is open right now, and when it was last used. It is a
//! library accessor rather than a wire method on purpose — T5 is pinned at nineteen
//! methods and a `wch_status` would be a twentieth (note N42).
//!
//! ## D12's `wait` flag, and the one place a caller's own thread waits
//!
//! D12's other half — "a second capture request queues or is refused with `Busy` per its
//! `wait` flag" — is [`Enqueue`]. The command queue *is* D12's queue: a caller that arrives
//! past [`limits::CAMERA_COMMAND_QUEUE_DEPTH`] is refused with [`Error::Busy`]
//! ([`Enqueue::Refuse`], which is what [`CameraActor::submit`] has always done), or waits
//! for room until an instant it names ([`Enqueue::WaitUntil`]) and takes the *same* refusal
//! when that instant arrives. The flag changes when the answer arrives, never what it is.
//!
//! **The waiting happens on the caller's thread, and the deadline is the caller's.** That is
//! the same doctrine as the paragraph above rather than an exception to it: the actor still
//! reads no clock, because the only clock read is `Instant::now()` on the thread that chose
//! to wait, computing how much of its *own* budget is left. A caller with no thread to spare
//! — the daemon, on a runtime worker — hands the wait to a blocking pool thread, which is
//! where a blocking wait is already legal. Note **N42** deferred this mechanism from P4b and
//! P4c, and note **N51** named the same missing bound from the other side.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, mpsc};
use std::time::{Duration, Instant};

use camino::Utf8PathBuf;
use schema::backend::{Camera, CameraBackend};
use schema::camera::{CameraId, CameraInfo};
use schema::error::Occupation;
use schema::{Error, Result, limits};

use crate::settle::Millis;

/// The open device, as one unit of work sees it.
///
/// The `+ 'static` is written out rather than left to elision, and it is load-bearing:
/// inside a reference the default trait-object lifetime is the reference's own, `&mut T`
/// is invariant in `T`, and the actor holds a `Box<dyn Camera>` (which is
/// `dyn Camera + 'static`). Eliding here would name a type nothing can coerce into, and
/// every closure a caller wrote would fail to compile for a reason that reads like a
/// mistake in the caller.
pub type OpenCamera<'device> = &'device mut (dyn Camera + 'static);

/// How a unit of work tells its caller what happened, once the actor is done with it.
///
/// Returned by the work rather than run inside it, and that ordering is the whole reason
/// this type exists: the actor publishes "between commands" *before* it hands the answer
/// over, so a caller holding a reply is holding a status that already accounts for the
/// command that produced it. Without the split, a caller could receive its answer, ask
/// [`CameraActor::activity`], and be told the actor was still inside the command it had
/// just answered — which is the difference between a status surface and a rumour, and is
/// what [`CameraActor::sweep`] reads to decide whether asking would mean waiting.
pub type Answering = Box<dyn FnOnce() + Send>;

/// Package `answer` as the thing a unit of work hands back.
///
/// A named constructor because the coercion to a boxed `FnOnce` is the sort of thing that
/// reads as ceremony at a call site; this says what it is for.
pub fn answering(answer: impl FnOnce() + Send + 'static) -> Answering {
    Box::new(answer)
}

/// Work that needs the device, and the caller's own way of answering.
type Work = Box<dyn for<'device> FnOnce(Result<OpenCamera<'device>>) -> Answering + Send>;

/// How an idle sweep reports what it did.
type SweepAnswer = Box<dyn FnOnce(bool) + Send>;

/// How a caller wants to meet a full command queue — D12's `wait` flag, as a value.
///
/// A value rather than a `bool` because only one of the two arms means anything on its own:
/// "wait" without a bound is the wedge AGENTS forbids of anything that waits, so the waiting
/// arm carries the deadline that makes it bounded and the refusing arm carries nothing
/// because it needs nothing.
///
/// A [`std::time::Instant`] rather than a duration or a [`Millis`], and the difference is
/// the same one [`crate::settle`] makes about its own deadline: an *instant* is a decision
/// the caller has already made, so a wait that is resumed, retried or split still ends when
/// the caller said it would, and a deadline that has already passed is a refusal rather than
/// a wait of zero length. [`Millis`] is not available for this: it is the caller's stamp on
/// a *command*, taken from [`crate::settle::Clock`], and a `SteppedClock` is deliberately
/// not `Sync` — a wait is the one thing here that spans two threads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enqueue {
    /// Refuse with [`Error::Busy`] when there is no room right now.
    ///
    /// What [`CameraActor::submit`] has always done, and what every caller that has not
    /// asked for anything else still gets.
    Refuse,
    /// Wait for room, and give up with the same [`Error::Busy`] at this instant.
    WaitUntil(Instant),
}

impl Enqueue {
    /// Wait up to [`limits::CAMERA_ENQUEUE_WAIT_MS`] from now.
    ///
    /// The shipped spelling of `wait: true`, and the one reader of that constant: a caller
    /// that wants a different budget names its own instant, which is what this module's own
    /// tests do. The clock is read *here*, on the caller's thread, at the moment the caller
    /// decides to wait — the actor's thread is never involved and still reads nothing.
    #[must_use]
    pub fn waiting() -> Enqueue {
        Enqueue::WaitUntil(Instant::now() + Duration::from_millis(limits::CAMERA_ENQUEUE_WAIT_MS))
    }
}

/// The signal a waiting caller waits on: a place in the command queue has come free.
///
/// A counter and a condition variable rather than a permit pool, because the queue is
/// already the bound — [`limits::CAMERA_COMMAND_QUEUE_DEPTH`] is enforced by the
/// `sync_channel` itself — and a second count of the same seats would be a second answer to
/// "is there room" that could drift from the first. What is missing from `std::sync::mpsc`
/// is only the *wake-up*: `SyncSender` offers a send that blocks forever and a send that
/// never blocks, and `send_timeout` is unstable on the pinned toolchain, so the bounded
/// middle is a `try_send` retried when this says something changed.
///
/// [`Seats::served`] is what the waiter's predicate compares against, and a counter rather
/// than a flag for the ordinary condition-variable reason: a flag can be raised and lowered
/// between two waiters' turns, and a number that only goes up cannot be missed.
#[derive(Debug)]
struct Room {
    /// The two numbers, behind one lock because a waiter changes both.
    seats: Mutex<Seats>,
    /// Raised whenever [`Seats`] changes — a place came free, a caller started or stopped
    /// waiting for one, or the thread left.
    changed: Condvar,
}

/// What is true about the queue's occupancy right now.
#[derive(Debug, Default)]
struct Seats {
    /// How many commands the actor thread has taken off the queue.
    served: u64,
    /// How many callers are parked waiting for a place in it.
    ///
    /// Written by [`CameraActor::send_waiting`] and read by this module's own suite, which
    /// is the point rather than an accident: without it, "the caller waited rather than
    /// being refused" is a claim about the scheduler, and a test that released the held
    /// thread before the waiter had reached the queue would pass against an actor that never
    /// learned to wait at all. With it, the release is taken *after* the subject says it is
    /// waiting — a signal, which is the only kind of waiting this project allows.
    waiting: usize,
}

impl Room {
    /// A place has come free, or the thread has gone; either way, everyone waiting should
    /// look again.
    ///
    /// `notify_all` and not `notify_one`: a freed place can be taken by a caller that never
    /// waited for it ([`Enqueue::Refuse`] does not touch this lock at all), so waking one
    /// waiter could spend a wake-up on a caller that finds the queue full again while
    /// another waits out its whole budget beside it.
    fn freed(&self) {
        lock(&self.seats).served += 1;
        self.changed.notify_all();
    }
}

/// One question for one camera's actor thread.
enum Command {
    /// Work that needs the device: open it if it is closed, and count as a use.
    Use {
        /// The caller's clock reading when it issued this.
        at: Millis,
        /// What to do with the device — or with the refusal that stood in for it.
        work: Work,
    },
    /// Close the device if it has gone idle, and say whether it did.
    ///
    /// Not a use: an idle sweep that refreshed the deadline would be a camera that never
    /// closes, which is the bug this command exists to prevent.
    Sweep {
        /// The caller's clock reading.
        at: Millis,
        /// Whether the device was closed.
        answer: SweepAnswer,
    },
}

/// Whether an open camera has gone unused long enough to close (D12).
///
/// A fold over values: no clock, no thread, no device. "Idle" is a *measurable quantity*
/// only if something can compute it from two numbers, and this is that something — which
/// is also what lets both directions of the deadline be asserted without a test waiting
/// for anything.
///
/// The stamp is taken when a command **starts**, so `last_used_ms` means "when the last
/// command was issued" and not "when the device was last touched" — the actor reads no
/// clock, so "when did the work finish" is not a number it has. For a command longer than
/// the timeout, which is what `wch_calibrate_sweep` is, that would mean the camera is idle
/// by its own deadline the instant the command returns and the descriptor goes away on the
/// next pass. What closes that is not here and is not arithmetic: `Live::used_since_sweep`
/// makes the *first* pass after a completed command decline, which costs one sweep cadence
/// and needs no clock. Note **N45** carries the two shapes that were priced and why this is
/// the one that landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Idle {
    /// How long unused is long enough.
    after_ms: Millis,
    /// The clock reading of the last command that needed the device.
    last_used_ms: Millis,
}

impl Idle {
    /// A camera last used at `now_ms`, to be closed after `after_ms` of quiet.
    #[must_use]
    pub const fn new(after_ms: Millis, now_ms: Millis) -> Idle {
        Idle {
            after_ms,
            last_used_ms: now_ms,
        }
    }

    /// Record a use.
    ///
    /// The *later* of the two readings, not simply the new one. A monotonic clock never
    /// goes backwards, but two callers reading it concurrently can reach the actor in the
    /// other order, and the harm is one-directional: an older stamp winning would move the
    /// deadline *closer*, which closes a camera somebody is using. Nothing is harmed by
    /// the reverse.
    pub const fn used(&mut self, now_ms: Millis) {
        if now_ms > self.last_used_ms {
            self.last_used_ms = now_ms;
        }
    }

    /// The clock reading of the last recorded use.
    #[must_use]
    pub const fn last_used_ms(&self) -> Millis {
        self.last_used_ms
    }

    /// Whether the deadline has arrived.
    ///
    /// `>=`, so a camera unused for exactly the timeout has reached it — the same
    /// direction the settle policy takes with its own deadline, and the one that makes a
    /// zero timeout mean what it says: close at the first sweep after every use.
    ///
    /// Saturating, so a caller that hands in a reading older than the last use gets
    /// "not idle" rather than an underflow that reads as "idle for four billion seconds".
    #[must_use]
    pub const fn expired(&self, now_ms: Millis) -> bool {
        now_ms.saturating_sub(self.last_used_ms) >= self.after_ms
    }
}

/// What one camera's actor is doing, as of the last command it processed.
///
/// The status surface docs/7 P4b names ("open/idle observable via the status surface"),
/// and deliberately not a wire DTO — see this module's header and note N42.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraActivity {
    /// Which camera.
    pub camera: CameraId,
    /// Whether its device is open right now.
    pub open: bool,
    /// The clock reading of the last command that needed the device.
    ///
    /// Zero when nothing has needed it yet, which is the reading the actor was spawned
    /// with rather than a claim about time.
    pub last_used_ms: Millis,
}

/// What the actor publishes about itself, behind one lock.
#[derive(Debug)]
struct Live {
    /// Whether the device is open.
    open: bool,
    /// Whether the thread is inside a command right now.
    ///
    /// Published so that a *reader* can tell "this camera is between commands" from "this
    /// camera's thread is in the middle of one", without asking the thread — which is the
    /// whole point, because asking a thread that is in the middle of a command means
    /// waiting for the command. [`CameraActor::sweep`] is that reader and
    /// [`Cameras::sweep`] is why it matters: a housekeeping pass that blocked on one
    /// camera would leave every other camera open for as long as that one was busy.
    busy: bool,
    /// Whether a command has needed the device since the last idle pass looked.
    ///
    /// Note **N45**'s fix, and the whole of it. [`Idle`] measures from when a command
    /// *started*, because that is the only reading the actor is given (see this module's
    /// "why every command carries the time"), so a command that ran longer than the timeout
    /// is expired the moment it returns and would be closed by the very next pass — the
    /// client's next verb then pays a fresh `open` and the driver's first-frame settle
    /// \[PF:11\], every time, for every long operation.
    ///
    /// A boolean is enough because a *pass* is the evidence the deadline is missing: this
    /// is raised by [`Thread::publish`] under the same lock as `busy`, and
    /// [`CameraActor::sweep`] takes it and declines once. A pass that arrives while the
    /// command is still running sees `busy` and never reaches it, so the flag survives to
    /// the first pass after the work is done — which is exactly the pass that must not
    /// close a camera whose last command has only just let go of it.
    ///
    /// The cost, which [`limits::CAMERA_IDLE_CLOSE_MS`] states beside the timeout and which
    /// is **one pass, not one timeout**: a completed command buys a single declined sweep,
    /// so a command longer than the timeout closes one to two
    /// [`limits::CAMERA_IDLE_SWEEP_MS`] cadences after it returns rather than on the very
    /// next pass. A short command pays nothing at all, because the passes between it and its
    /// deadline spend the grace before the deadline arrives. Buying a whole timeout back
    /// instead would need the actor to know when the work finished, which is a clock — the
    /// alternative shape N45 priced, and it reverses this module's central decision and
    /// needs a `SteppedClock` that is deliberately not `Sync` ("a race dressed as a
    /// fixture"). Re-stamping [`Idle`] from the declining pass's own `at` is the third shape
    /// and it is worse than either: it would postpone a *short* command's close by a whole
    /// timeout as well, for a case that never needed it.
    used_since_sweep: bool,
    /// The idle deadline, which is also the home of "when was it last used".
    idle: Idle,
}

/// A handle onto one camera's actor thread.
///
/// Cheap to clone by `Arc` (which is how [`Cameras`] hands it out) and cheap to hold: an
/// actor that exists is not an open camera, because the device opens on the first command
/// that needs it and closes again when [`CameraActor::sweep`] finds it idle.
#[derive(Debug)]
pub struct CameraActor {
    /// The camera this actor owns, as enumeration described it.
    info: CameraInfo,
    /// The node named by a [`Error::Busy`] refusal, resolved once at spawn.
    node: Utf8PathBuf,
    /// The one way in.
    commands: SyncSender<Command>,
    /// How a caller that asked to wait learns that a place has come free.
    room: Arc<Room>,
    /// What the thread publishes; read by [`CameraActor::activity`].
    live: Arc<Mutex<Live>>,
    /// `false` once the thread has left its loop, however it left.
    alive: Arc<AtomicBool>,
}

impl CameraActor {
    /// Start an actor for `info`, opening nothing.
    ///
    /// # Errors
    ///
    /// [`Error::DeviceIo`] when the thread cannot be spawned. `std::thread::spawn` panics
    /// on that, which is not available on a request-driven path: a daemon out of threads
    /// has to refuse one request, not die holding somebody's camera.
    fn spawn(
        backend: Arc<dyn CameraBackend>,
        info: CameraInfo,
        after_ms: Millis,
        now_ms: Millis,
    ) -> Result<CameraActor> {
        let node = info.capture_node().map_or_else(
            // A camera with no capture node cannot be opened at all, so this path is only
            // reached by a refusal that has to name *something*; naming the camera beats
            // inventing a device node that does not exist.
            || Utf8PathBuf::from(info.id.as_str()),
            |node| node.path.clone(),
        );
        let live = Arc::new(Mutex::new(Live {
            open: false,
            busy: false,
            // Nothing has used the device yet, so there is no grace to grant: an actor that
            // was spawned and never asked for anything closes nothing, and the first pass
            // that finds it open will have a command behind it.
            used_since_sweep: false,
            idle: Idle::new(after_ms, now_ms),
        }));
        let alive = Arc::new(AtomicBool::new(true));
        let room = Arc::new(Room {
            seats: Mutex::new(Seats::default()),
            changed: Condvar::new(),
        });

        let (commands, inbox) = mpsc::sync_channel(limits::CAMERA_COMMAND_QUEUE_DEPTH);
        let thread = Thread {
            backend,
            id: info.id.clone(),
            live: Arc::clone(&live),
            room: Arc::clone(&room),
            _liveness: Liveness {
                alive: Arc::clone(&alive),
                live: Arc::clone(&live),
                room: Arc::clone(&room),
            },
        };
        std::thread::Builder::new()
            // Named, because an operator reading `top -H` or a core file on a daemon with
            // four cameras open should be able to tell the four threads apart.
            .name(format!("camera {}", info.id))
            .spawn(move || thread.run(&inbox))
            .map_err(|err| Error::DeviceIo {
                operation: format!("spawn the actor thread for {}", info.id),
                errno: err.raw_os_error(),
                message: err.to_string(),
            })?;

        Ok(CameraActor {
            info,
            node,
            commands,
            room,
            live,
            alive,
        })
    }

    /// The camera this actor owns, as enumeration described it.
    #[must_use]
    pub fn info(&self) -> &CameraInfo {
        &self.info
    }

    /// Whether the thread is still running.
    ///
    /// Advisory, and racy by nature — the thread may leave between this answer and the
    /// next command. It exists so [`Cameras::actor`] can replace a dead actor rather than
    /// hand out a handle to a camera that will refuse forever; a caller that acts on it
    /// still has to handle [`Error::DeviceGone`].
    #[must_use]
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }

    /// What this actor is doing, as of the last command it processed.
    #[must_use]
    pub fn activity(&self) -> CameraActivity {
        let live = lock(&self.live);
        CameraActivity {
            camera: self.info.id.clone(),
            open: live.open,
            last_used_ms: live.idle.last_used_ms(),
        }
    }

    /// Hand `work` to the actor and return without waiting for it.
    ///
    /// `work` runs on the actor's thread with the device open — or, when opening it
    /// failed, with that refusal, so the caller answers its own client either way and no
    /// request is left without a reply. `at` is the caller's clock reading and is what
    /// keeps the idle deadline honest; see this module's header for why the actor does not
    /// read a clock of its own.
    ///
    /// This is the API the daemon's async handlers use: the closure holds a
    /// `tokio::sync::oneshot::Sender` and the handler awaits the receiver, so a request
    /// waiting on a minutes-long sweep occupies no thread anywhere.
    ///
    /// # Errors
    ///
    /// [`Error::Busy`] when the actor already has
    /// [`limits::CAMERA_COMMAND_QUEUE_DEPTH`] commands waiting — D12's refusal, and the
    /// only thing an unbounded queue would have bought is a longer wait for the same
    /// answer. [`Error::DeviceGone`] when the thread has left.
    pub fn submit<F>(&self, at: Millis, work: F) -> Result<()>
    where
        F: for<'device> FnOnce(Result<OpenCamera<'device>>) -> Answering + Send + 'static,
    {
        self.submit_with(at, Enqueue::Refuse, work)
    }

    /// [`CameraActor::submit`], choosing what a full command queue means — D12's `wait`
    /// flag.
    ///
    /// [`Enqueue::Refuse`] is [`CameraActor::submit`] exactly: no lock is taken, nothing
    /// waits, and a full queue is [`Error::Busy`] now. [`Enqueue::WaitUntil`] **blocks the
    /// calling thread** until a place comes free or that instant arrives, and it is named
    /// so a caller cannot reach it by accident: the daemon runs it on a blocking pool
    /// thread, never on a runtime worker, for the reason this module's header gives.
    ///
    /// The wait ends the moment the actor takes a command off the queue, which is a signal
    /// from the thread and not a poll — nothing here sleeps, and the deadline is a bound
    /// rather than a schedule.
    ///
    /// # Errors
    ///
    /// [`Error::Busy`] when the queue is full and either the caller did not ask to wait or
    /// its instant arrived first — the *same* refusal either way, with the same empty
    /// holder list, because the flag chooses when the answer arrives and not what it is.
    /// [`Error::DeviceGone`] when the thread has left, including when it leaves while a
    /// caller is waiting: a thread that is gone is not a device that is held (E3).
    pub fn submit_with<F>(&self, at: Millis, how: Enqueue, work: F) -> Result<()>
    where
        F: for<'device> FnOnce(Result<OpenCamera<'device>>) -> Answering + Send + 'static,
    {
        let command = Command::Use {
            at,
            work: Box::new(work),
        };
        match how {
            Enqueue::Refuse => self.send(command),
            Enqueue::WaitUntil(deadline) => self.send_waiting(command, deadline),
        }
    }

    /// Run `work` against the device and wait for its answer.
    ///
    /// **Blocking**, and named so a caller cannot use it by accident from an async
    /// context: it parks the calling thread until the actor gets to the work. That is
    /// right for a CLI and for this module's tests, and wrong for the daemon, which owns
    /// its reply channel through [`CameraActor::submit`] instead.
    ///
    /// # Errors
    ///
    /// Whatever `work` returns, or [`CameraActor::submit`]'s refusals, or
    /// [`Error::DeviceGone`] when the thread leaves before answering — a dropped reply is
    /// a thread that is gone, never a device that was busy (E3 keeps those two apart).
    pub fn ask<T, F>(&self, at: Millis, work: F) -> Result<T>
    where
        F: FnOnce(OpenCamera<'_>) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let (answered, answer) = mpsc::sync_channel::<Result<T>>(1);
        self.submit(at, move |device| {
            let outcome = device.and_then(work);
            answering(move || {
                // A failed send means the caller stopped waiting; there is nobody left to
                // tell, and the device work is already done either way.
                let _ = answered.send(outcome);
            })
        })?;
        match answer.recv() {
            Ok(answer) => answer,
            Err(_) => Err(self.device_gone()),
        }
    }

    /// Close the device if it has been idle since `at`, and say whether it did.
    ///
    /// Blocking, for the reason [`CameraActor::ask`] is: the answer *is* the observation,
    /// and a sweep whose result arrived later would be a test that had to wait for it.
    ///
    /// **The actor is only asked when its published state says the answer could be
    /// `true`.** A camera that is closed, or whose deadline has not arrived, or whose
    /// thread is inside a command right now, answers `false` from the published state — a mutex read
    /// — without putting anything on the command queue. That is not an optimisation: a
    /// device command may take minutes by design (a P4c calibration sweep) or may never
    /// return at all (a `DQBUF` on a wedged driver), and [`Cameras::sweep`] walks the
    /// actors one after another, so a pass that waited behind one camera's command would
    /// hold every *other* camera open for as long as it lasted. Reading `busy` is what
    /// makes the housekeeping pass's cost a property of the registry rather than of the
    /// slowest device in it.
    ///
    /// The published state is a filter and not the decision: the actor re-reads the same
    /// deadline before closing anything, because only the thread knows whether the handle
    /// is still there. The one thing this cannot exclude is a command that arrives between
    /// the read and the send, in which case this waits for that command — a race with a
    /// window of a few instructions, against a caller who by definition wants the camera.
    ///
    /// **A pass that finds a command has completed since the last one declines, once**, and
    /// takes `Live::used_since_sweep` with it. That is note **N45**'s fix and it is here
    /// rather than in the thread on purpose: the flag is published under this same lock,
    /// before the work runs, so reading it costs the mutex this pass already takes and a
    /// long command does not earn a queue round trip per pass — which is the property the
    /// paragraph above exists to preserve. A pass that runs *during* the command sees
    /// `busy` first and never reaches the flag, so a sweep of a long command cannot spend
    /// the grace the command has not finished needing.
    ///
    /// # Errors
    ///
    /// [`Error::DeviceGone`] when the thread has left *while its published state still
    /// said the device was open* — a window that closes as the thread unwinds, because
    /// the drop guard clears the flag on the way out. A thread that has finished leaving
    /// answers `false`, which is true: it closed nothing on this pass.
    pub fn sweep(&self, at: Millis) -> Result<bool> {
        {
            let mut live = lock(&self.live);
            if !live.open || live.busy {
                return Ok(false);
            }
            // Before the deadline is consulted, and that ordering is what keeps a *short*
            // command closing on the first pass past its deadline: the passes that happen
            // between a short command and its deadline spend the grace, so by the time the
            // deadline arrives there is none left. A long command gets no such passes —
            // every one of them saw `busy` — so its grace is still here, which is the case
            // N45 is about.
            if std::mem::take(&mut live.used_since_sweep) {
                return Ok(false);
            }
            if !live.idle.expired(at) {
                return Ok(false);
            }
        }

        let (closed, answer) = mpsc::sync_channel::<bool>(1);
        let command = Command::Sweep {
            at,
            answer: Box::new(move |it| {
                let _ = closed.send(it);
            }),
        };
        match self.commands.try_send(command) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => return Ok(false),
            Err(TrySendError::Disconnected(_)) => return Err(self.device_gone()),
        }
        answer.recv().map_err(|_| self.device_gone())
    }

    fn send(&self, command: Command) -> Result<()> {
        match self.commands.try_send(command) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(self.busy()),
            Err(TrySendError::Disconnected(_)) => Err(self.device_gone()),
        }
    }

    /// [`CameraActor::send`], waiting for room until `deadline`.
    ///
    /// The lock is held *across* the `try_send`, and that is the whole of the correctness
    /// argument: [`Room::freed`] cannot bump between a send that found the queue full and
    /// the wait that follows it, so there is no wake-up to lose. Everything else is the
    /// ordinary condition-variable loop — re-read the counter, try, wait for it to move —
    /// with the caller's own budget recomputed each turn so that a stream of wake-ups
    /// cannot extend the wait past the instant the caller named.
    ///
    /// A spent deadline and a deadline that expires mid-wait take the *same* line out of
    /// this function, which is what makes one test able to go red for both.
    fn send_waiting(&self, mut command: Command, deadline: Instant) -> Result<()> {
        let mut seats = lock(&self.room.seats);
        // Raised before the first attempt and lowered after the last one, so that "somebody
        // is waiting for this camera" spans the whole of the wait rather than the instant
        // inside `wait_timeout_while` — see [`Seats::waiting`] for what reads it and why.
        seats.waiting += 1;
        self.room.changed.notify_all();

        let outcome = loop {
            match self.commands.try_send(command) {
                Ok(()) => break Ok(()),
                Err(TrySendError::Full(returned)) => command = returned,
                Err(TrySendError::Disconnected(_)) => break Err(self.device_gone()),
            }
            // A thread that left while this caller was waiting is `DeviceGone`, not `Busy`
            // — and it is asked here rather than left to the next `try_send` because the
            // drop guard lowers this flag *before* the inbox it shares a thread with is
            // dropped, so for one moment a disconnected actor still answers `Full`.
            if !self.is_alive() {
                break Err(self.device_gone());
            }
            let Some(budget) = deadline.checked_duration_since(Instant::now()) else {
                break Err(self.busy());
            };
            let seen = seats.served;
            let (guard, _) = self
                .room
                .changed
                .wait_timeout_while(seats, budget, |seats| seats.served == seen)
                .unwrap_or_else(PoisonError::into_inner);
            seats = guard;
        };

        seats.waiting -= 1;
        drop(seats);
        self.room.changed.notify_all();
        outcome
    }

    /// Block until `count` callers are parked in [`CameraActor::send_waiting`].
    ///
    /// The rendezvous this module's suite needs and nothing else has: it ends when the
    /// subject says so, never when a duration passes, which is what lets a test release the
    /// actor's held thread *after* a waiter has provably reached the full queue. Without it
    /// a test could only guess, and an actor that had quietly gone back to refusing would
    /// pass whenever the guess was generous.
    #[cfg(test)]
    fn awaited_by(&self, count: usize) {
        let seats = lock(&self.room.seats);
        let _parked = self
            .room
            .changed
            .wait_while(seats, |seats| seats.waiting < count)
            .unwrap_or_else(PoisonError::into_inner);
    }

    /// The refusal for a command queue that is full — D12's `Busy`.
    ///
    /// Its holder list is empty, and that is the honest answer rather than a missing one.
    /// The list is filled by the `/proc/*/fd` walk that answers "which *other* processes
    /// have this node open"; here the work in the way is our own, and it feeds
    /// `terminate_holder` — naming this process's pid would invite a client to kill the
    /// daemon it is talking to (note N42).
    ///
    /// **Which is exactly why it goes through [`Error::busy_here`]** (note **N221**). An empty
    /// list renders as *"held by an unidentified process"*, written for the walk that found
    /// nobody it could see — so for one repair this function said the sentence above and
    /// shipped the opposite of it, about the very daemon the caller was talking to.
    /// [`schema::error::Occupation::RunningCommands`] is what withholds the pid and still
    /// says what has the camera, and it is the one alternative D12's `wait` helps with: it
    /// waits for room in *this* queue.
    fn busy(&self) -> Error {
        Error::busy_here(self.node.clone(), Occupation::RunningCommands)
    }

    /// The refusal for an actor whose thread has left.
    ///
    /// [`Error::DeviceGone`] rather than [`Error::Busy`]: the thread that owned the
    /// descriptor is gone, so the device is not held by anything and retrying this handle
    /// will never work. Availability is not capability, and neither is either of these
    /// (E3) — which is why the two are separate arms where a command is sent.
    ///
    /// Public because a caller that brought its own reply channel has to be able to say
    /// the same thing. [`CameraActor::ask`] answers this when its `std` receiver ends
    /// without a value; the daemon awaits a `tokio` receiver and reaches the same
    /// condition, and a second spelling of "the thread that owned this device is gone"
    /// would be a second refusal for one fact — including a second guess at which node to
    /// name, which this type resolved once at spawn.
    #[must_use]
    pub fn device_gone(&self) -> Error {
        Error::DeviceGone {
            path: self.node.clone(),
        }
    }
}

/// Everything the actor's thread owns.
struct Thread {
    backend: Arc<dyn CameraBackend>,
    id: CameraId,
    live: Arc<Mutex<Live>>,
    room: Arc<Room>,
    _liveness: Liveness,
}

impl Thread {
    /// The actor loop: one command at a time, forever, in arrival order.
    ///
    /// Ends when the last [`CameraActor`] handle is dropped, which drops the device with
    /// it — the descriptor closes because the `Box<dyn Camera>` does, not because anything
    /// here remembered to close it.
    fn run(self, inbox: &Receiver<Command>) {
        let mut open: Option<Box<dyn Camera>> = None;
        while let Ok(command) = inbox.recv() {
            // A place has come free — announced *before* the command runs, because the
            // queue's bound is about how many commands are waiting and this one has stopped
            // waiting. A caller that asked to wait gets in now rather than after work that
            // may take minutes (D12's `wait` flag; [`CameraActor::send_waiting`]).
            self.room.freed();
            match command {
                Command::Use { at, work } => {
                    // Three steps in an order that is load-bearing: the work runs with the
                    // device, the actor records that it is between commands, and only then
                    // does the caller's answer leave. A panic anywhere in the first step
                    // skips the other two and unwinds instead — [`Liveness`] is what
                    // lowers the flags then, because the case it exists for is the loop
                    // not reaching its end.
                    let answer = work(self.device(&mut open, at));
                    self.finished();
                    answer();
                }
                Command::Sweep { at, answer } => answer(self.close_if_idle(&mut open, at)),
            }
        }
    }

    /// The open device, opening it on first use.
    ///
    /// The publish happens *before* the answer leaves, in both directions, so a caller
    /// holding its reply is holding a status that already accounts for the command that
    /// produced it — including [`Thread::finished`], which is why a unit of work hands its
    /// answer back rather than sending it ([`Answering`]). That is what makes
    /// [`Cameras::activity`] assertable without a test waiting for anything.
    fn device<'slot>(
        &self,
        slot: &'slot mut Option<Box<dyn Camera>>,
        at: Millis,
    ) -> Result<OpenCamera<'slot>> {
        let camera = match slot.take() {
            Some(camera) => camera,
            None => match self.backend.open(&self.id) {
                Ok(camera) => camera,
                Err(err) => {
                    // Still closed, and still a use: a camera whose every open fails must
                    // not look idle, or the sweeper spends the rest of the run closing
                    // nothing on its behalf.
                    self.publish(false, at);
                    return Err(err);
                }
            },
        };
        self.publish(true, at);
        Ok(slot.insert(camera).as_mut())
    }

    /// Close the device if it has gone idle. Answers whether it did.
    fn close_if_idle(&self, slot: &mut Option<Box<dyn Camera>>, at: Millis) -> bool {
        let idle = lock(&self.live).idle.expired(at);
        if !idle {
            return false;
        }
        // `is_some` first, so "closed" means a descriptor went away rather than "the
        // deadline passed on a camera that was never open".
        let closed = slot.take().is_some();
        if closed {
            self.publish_closed();
        }
        closed
    }

    /// Record a command that needed the device, and that the thread is inside it.
    fn publish(&self, open: bool, at: Millis) {
        let mut live = lock(&self.live);
        live.open = open;
        live.busy = true;
        // Raised here, with `busy`, rather than in `finished`: a pass that arrives while
        // the work is running must not be able to spend the grace the command has not
        // finished earning, and the cheapest way to guarantee that is for the flag to be up
        // for the whole of the command rather than for the instant after it (note N45).
        live.used_since_sweep = true;
        live.idle.used(at);
    }

    /// Record that the thread is between commands again.
    fn finished(&self) {
        lock(&self.live).busy = false;
    }

    /// Record the close. The idle deadline is left where it was: it says when the camera
    /// was last *used*, and closing it is not a use.
    fn publish_closed(&self) {
        lock(&self.live).open = false;
    }
}

/// Marks an actor dead, and its device closed, when its thread leaves — however it leaves.
///
/// A drop guard rather than a line after the loop, because the case it exists for is the
/// loop *not* reaching its end: the most popular V4L2 crate panics on a control type this
/// kernel emits \[PF:1\], so "a backend panicked" is a measured failure mode and not a
/// hypothetical. Unwinding runs destructors, so every flag falls on that path too, and the
/// next request for that camera gets a fresh actor instead of a handle nobody answers.
///
/// **`open` falls here for the same reason `alive` does, and it is not bookkeeping
/// tidiness.** The unwind drops the `Box<dyn Camera>` — the descriptor is gone the moment
/// the thread starts leaving — and nothing else will ever run on that thread to say so:
/// [`Thread::publish_closed`] is only reached from a `Sweep` the dead thread cannot
/// process, and [`Cameras::activity`] lists every actor whether its thread is alive or
/// not. A status surface that went on claiming a camera this process no longer holds is
/// the exact opposite of the fact docs/7 P4b asks it to report, and it would persist until
/// some later request happened to replace the actor. The local `Box` is dropped before
/// this guard is (locals unwind before the parameter that owns the guard), so the flag
/// falls *after* the descriptor it describes, never before.
struct Liveness {
    alive: Arc<AtomicBool>,
    live: Arc<Mutex<Live>>,
    room: Arc<Room>,
}

impl Drop for Liveness {
    fn drop(&mut self) {
        {
            let mut live = lock(&self.live);
            live.open = false;
            live.busy = false;
            // For `open`'s reason, one step further: a dead actor claiming that a command
            // had just finished would postpone one housekeeping pass on a camera this
            // process no longer holds — a grace granted to work that is not coming back.
            live.used_since_sweep = false;
        }
        self.alive.store(false, Ordering::Release);
        // Last, and the order is load-bearing: a caller waiting for a place in the queue of
        // a thread that is leaving must find `alive` already `false` when this wakes it, or
        // it would meet `Error::Busy` where the fact is `Error::DeviceGone` (E3). Without
        // this, such a caller would wait out its whole budget for room that is never coming.
        self.room.freed();
    }
}

/// Every camera this process has an actor for (D12).
///
/// The registry is what makes "one actor per camera" a fact rather than a habit: an actor
/// is reached by [`CameraId`] and created only when there is not one already, so two
/// requests for one camera cannot become two threads holding two descriptors onto one
/// node. One registry per process is the other half of that sentence — a second `Cameras`
/// over the same backend would be a second opinion, which is why the daemon holds exactly
/// one and hands out `&self`.
///
/// Nothing is ever removed from it except a thread that died, so an actor for a camera
/// that has been unplugged outlives its camera: it holds a thread, it holds whatever
/// descriptor it had, and it answers each request with whatever the backend says about a
/// node that is gone (which is [`Error::DeviceGone`]'s whole purpose, so nothing here has
/// to invent an answer). Reaping the entry belongs with the hotplug watch that would
/// notice one — P4d — and is written down here rather than left as a leak somebody finds.
#[derive(Debug)]
pub struct Cameras {
    backend: Arc<dyn CameraBackend>,
    idle_after_ms: Millis,
    live: Mutex<BTreeMap<CameraId, Arc<CameraActor>>>,
}

impl Cameras {
    /// A registry over `backend`, closing idle cameras after
    /// [`limits::CAMERA_IDLE_CLOSE_MS`].
    #[must_use]
    pub fn new(backend: Arc<dyn CameraBackend>) -> Cameras {
        Cameras::with_idle_timeout(backend, limits::CAMERA_IDLE_CLOSE_MS)
    }

    /// A registry that closes idle cameras after `after_ms` — D12's "configurable".
    ///
    /// Zero is meaningful and is not a disabled timeout: it closes the device at the first
    /// sweep after every use, which is what somebody who wants the daemon to touch their
    /// webcam as briefly as possible is asking for.
    #[must_use]
    pub fn with_idle_timeout(backend: Arc<dyn CameraBackend>, after_ms: Millis) -> Cameras {
        Cameras {
            backend,
            idle_after_ms: after_ms,
            live: Mutex::new(BTreeMap::new()),
        }
    }

    /// How long an open camera may go unused before a sweep closes it.
    #[must_use]
    pub fn idle_timeout_ms(&self) -> Millis {
        self.idle_after_ms
    }

    /// Every camera the backend can see, right now (T1).
    ///
    /// A forward, and it is here rather than in the caller so that a process which owns a
    /// registry needs **no other handle on the backend**. That is what makes this module's
    /// first claim — "nothing hands the `Box<dyn Camera>` out … two callers write at once
    /// is unrepresentable rather than prevented" — true of the daemon as well as of the
    /// engine: `CameraBackend::open` is not reachable from anything a request handler
    /// holds, so a handler cannot produce a second descriptor on a node an actor already
    /// owns. A handler that wants the device asks [`Cameras::actor`] for it.
    ///
    /// Live every time, never cached (E2): a daemon that remembered its camera list would
    /// answer about a camera that had been unplugged.
    ///
    /// # Errors
    ///
    /// Whatever the backend refuses enumeration with.
    pub fn enumerate(&self) -> Result<Vec<CameraInfo>> {
        self.backend.enumerate()
    }

    /// A source of hotplug events from the backend this registry drives (T1).
    ///
    /// The fourth forward, for [`Cameras::enumerate`]'s reason, and the one P4e-i's
    /// `subscribe_events` needed: a daemon whose only handle on the backend is this
    /// registry could not otherwise watch at all, and giving it a second
    /// `Arc<dyn CameraBackend>` to watch with would put `backend.open(&id)` one line away
    /// from every request handler — the exact thing this type's header says must stay
    /// unrepresentable.
    ///
    /// **Watching is not opening** (note N53): the V4L2 watch diffs `/sys/class/video4linux`
    /// and opens no node, so a subscriber costs no descriptor and D12's "the daemon never
    /// opens a camera until first use" is untouched — subscribing to events is not a use.
    ///
    /// The answer is exclusively owned and `&mut`-driven (`HotplugWatch: Send`, not `Sync`),
    /// so a caller runs it on one thread of its own. This registry keeps nothing.
    ///
    /// # Errors
    ///
    /// Whatever the backend refuses a watch with — for V4L2, [`Error::DeviceIo`] when the
    /// uevent socket cannot be opened.
    pub fn watch(&self) -> Result<Box<dyn schema::backend::HotplugWatch>> {
        self.backend.watch()
    }

    /// Which backend this registry drives (T1).
    ///
    /// A forward, for [`Cameras::enumerate`]'s reason: a process holding a
    /// registry needs no other handle on the backend, and this is the one fact about it
    /// that is not a question about a camera. `profile_capture` writes it into a document's
    /// provenance, where "a profile captured from the fake backend would be circular
    /// corpus" is the whole point of the field — so a daemon that guessed instead of asking
    /// would be signing a corpus entry with a name it made up.
    #[must_use]
    pub fn backend_kind(&self) -> schema::backend::BackendKind {
        self.backend.kind()
    }

    /// What `list` answers, assembled where D1's rule lives.
    ///
    /// The other half of the forward above, for the same reason, and it forwards to
    /// [`crate::resolve::list`] rather than assembling anything here — the rule that an empty
    /// enumeration is diagnosed has one home, and it is shared with `webcam-handler-cli`.
    ///
    /// # Errors
    ///
    /// Whatever the backend refuses enumeration with.
    pub fn list(&self) -> Result<schema::report::CameraList> {
        crate::resolve::list(self.backend.as_ref())
    }

    /// The actor for `info`, started if this is the first time anyone asked.
    ///
    /// Starting an actor opens nothing: the device opens on the first command that needs
    /// it, which is what "the daemon never opens a camera until first use" means and what
    /// makes `wch_list` — which needs no device at all — cost a daemon nothing.
    ///
    /// A dead actor is replaced rather than handed out again. See [`CameraActor::is_alive`]
    /// for what that does and does not promise.
    ///
    /// # Errors
    ///
    /// [`Error::DeviceIo`] when a thread cannot be spawned.
    pub fn actor(&self, info: &CameraInfo, at: Millis) -> Result<Arc<CameraActor>> {
        let mut live = lock(&self.live);
        if let Some(actor) = live.get(&info.id).filter(|actor| actor.is_alive()) {
            return Ok(Arc::clone(actor));
        }
        let fresh = Arc::new(CameraActor::spawn(
            Arc::clone(&self.backend),
            info.clone(),
            self.idle_after_ms,
            at,
        )?);
        live.insert(info.id.clone(), Arc::clone(&fresh));
        Ok(fresh)
    }

    /// Close every camera that has been idle since `at`; answer which ones closed.
    ///
    /// The housekeeping pass D12's "closes on idle" needs somebody to run: the daemon runs
    /// it on a cadence, and a test runs it at the millisecond it wants to talk about.
    ///
    /// The pass walks the actors one after another, and what keeps that from being a
    /// liability is [`CameraActor::sweep`]'s published-state filter: an actor that is
    /// closed, unexpired or mid-command answers from a mutex rather than from its command
    /// queue, so the pass costs one lock per actor plus one acknowledgement per camera
    /// that is actually about to close. Nothing is removed from the registry except a dead
    /// thread (see this type's own paragraph), so that per-actor cost is paid for every
    /// camera this process has *ever* opened; reaping the entries belongs with the hotplug
    /// watch that would notice one, which is P4d's.
    pub fn sweep(&self, at: Millis) -> Vec<CameraId> {
        // Copied out from under the registry lock before anything blocks on an actor: a
        // sweep that held this lock while an actor finished a minutes-long command would
        // stop every *other* camera's requests from being routed.
        let actors: Vec<Arc<CameraActor>> = lock(&self.live).values().map(Arc::clone).collect();
        actors
            .iter()
            // A dead actor closed nothing, which is what `false` says. It is not this
            // pass's business to report it either: the next request for that camera is
            // where the death matters, and `Cameras::actor` answers it there by replacing
            // the actor rather than by having housekeeping decide.
            .filter(|actor| actor.sweep(at).unwrap_or(false))
            .map(|actor| actor.info().id.clone())
            .collect()
    }

    /// What every actor is doing — the status surface (docs/7 P4b).
    ///
    /// In [`CameraId`] order, because it comes out of a `BTreeMap` and a status listing
    /// whose order depended on which camera was asked about first would be a status
    /// listing nobody could diff.
    #[must_use]
    pub fn activity(&self) -> Vec<CameraActivity> {
        lock(&self.live)
            .values()
            .map(|actor| actor.activity())
            .collect()
    }
}

/// Take a lock, recovering from poisoning.
///
/// The same helper, for the same reason, as the fake backend's: a poisoned lock here means
/// a thread panicked while holding it, and a second panic would replace a useful failure
/// with a confusing one. What is behind these locks is two booleans and two integers —
/// nothing that a panic mid-update can leave meaning something else.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::TryRecvError;

    use fake::FakeBackend;
    use schema::ErrorKind;
    use schema::control::ControlDesc;

    use super::*;

    /// A registry over one replayed camera, with the idle timeout the test wants to talk
    /// about.
    fn one_camera(after_ms: Millis) -> (Arc<FakeBackend>, Cameras, CameraInfo) {
        let backend = Arc::new(
            FakeBackend::from_profile(testkit::fixtures::synthetic_basic())
                .expect("the synthetic profile is this build's version"),
        );
        let info = backend
            .enumerate()
            .expect("the fake enumerates what it replays")
            .first()
            .cloned()
            .expect("one profile is one camera");
        let cameras =
            Cameras::with_idle_timeout(Arc::clone(&backend) as Arc<dyn CameraBackend>, after_ms);
        (backend, cameras, info)
    }

    /// The device work every test here uses: a control read, which is the cheapest thing
    /// that genuinely needs an open camera.
    fn controls(camera: OpenCamera<'_>) -> Result<Vec<ControlDesc>> {
        camera.controls()
    }

    #[test]
    fn an_idle_deadline_is_two_numbers_and_both_directions_are_reachable() {
        let mut idle = Idle::new(100, 1_000);
        assert_eq!(idle.last_used_ms(), 1_000);
        assert!(!idle.expired(1_099), "99 ms of quiet is not 100");
        assert!(idle.expired(1_100), "the deadline is reached, not passed");
        assert!(idle.expired(9_999));

        idle.used(1_500);
        assert!(!idle.expired(1_599));
        assert!(idle.expired(1_600));

        // A stamp older than the last use loses, so two callers racing to the actor cannot
        // between them close a camera one of them is using.
        idle.used(1_200);
        assert_eq!(idle.last_used_ms(), 1_500);
        assert!(!idle.expired(1_599));

        // And a reading from before the last use is "not idle", never an underflow.
        assert!(!idle.expired(0));
    }

    #[test]
    fn a_zero_timeout_closes_at_the_first_sweep_after_a_use() {
        // D12 says the idle close is configurable, so zero has to mean something rather
        // than accidentally meaning "never".
        let idle = Idle::new(0, 7);
        assert!(idle.expired(7));
        assert!(idle.expired(8));
    }

    #[test]
    fn nothing_opens_until_something_needs_the_device() {
        let (backend, cameras, info) = one_camera(1_000);
        assert_eq!(
            backend.opens(),
            0,
            "the registry opened a camera by existing"
        );

        let actor = cameras.actor(&info, 0).expect("a thread can be spawned");
        assert_eq!(backend.opens(), 0, "starting an actor opened the device");
        assert_eq!(
            actor.activity(),
            CameraActivity {
                camera: info.id.clone(),
                open: false,
                last_used_ms: 0,
            }
        );

        let first = actor.ask(10, controls).expect("the fake opens");
        assert!(!first.is_empty(), "the profile has controls");
        assert_eq!(backend.opens(), 1);
        assert_eq!(
            actor.activity(),
            CameraActivity {
                camera: info.id.clone(),
                open: true,
                last_used_ms: 10,
            }
        );

        // The second command reuses the handle: that is the whole point of holding it.
        actor.ask(20, controls).expect("still open");
        assert_eq!(
            backend.opens(),
            1,
            "the second command re-opened the device"
        );
        assert_eq!(actor.activity().last_used_ms, 20);
        assert_eq!(backend.closes(), 0);
    }

    #[test]
    fn an_idle_camera_closes_and_the_next_use_opens_it_again() {
        // Driven entirely by the millisecond the sweep carries: no clock is read anywhere
        // in this test and nothing waits for one.
        //
        // This is also N45's *first* direction: a command shorter than the timeout still
        // closes on the first pass at or past its deadline. The grace note N45 buys is
        // spent by the pass below at 1_499 — which is what a short command's cadence of
        // passes does with it, and why the fix costs a long operation one cadence and a
        // short one nothing.
        let (backend, cameras, info) = one_camera(1_000);
        let actor = cameras.actor(&info, 0).expect("a thread can be spawned");
        actor.ask(500, controls).expect("the fake opens");
        assert_eq!((backend.opens(), backend.closes()), (1, 0));

        assert_eq!(
            cameras.sweep(1_499),
            Vec::new(),
            "999 ms of quiet is not the 1000 ms timeout"
        );
        assert!(actor.activity().open, "the sweep closed a camera in use");
        assert_eq!(backend.closes(), 0);

        assert_eq!(
            cameras.sweep(1_500),
            vec![info.id.clone()],
            "the deadline arrived and the camera stayed open"
        );
        assert_eq!(
            backend.closes(),
            1,
            "the actor forgot the handle without dropping it — the descriptor is still open"
        );
        assert_eq!(
            actor.activity(),
            CameraActivity {
                camera: info.id.clone(),
                open: false,
                // Closing is not a use, so the deadline still says when it was last used.
                last_used_ms: 500,
            }
        );

        // A closed camera is not a broken one.
        actor.ask(2_000, controls).expect("it opens again");
        assert_eq!((backend.opens(), backend.closes()), (2, 1));
        assert!(actor.activity().open);
    }

    #[test]
    fn a_command_longer_than_the_timeout_is_not_closed_by_the_pass_that_follows_it() {
        // Note **N45**'s second direction, and the one that fails against the unfixed
        // actor: the idle deadline is stamped when a command *starts*, so a command that
        // ran longer than the timeout is already expired when it returns and the very next
        // housekeeping pass would take the descriptor away — after which the client's next
        // verb pays a fresh `open` and the driver's first-frame settle \[PF:11\], every
        // time, for every long operation. `wch_calibrate_sweep` is minutes of camera time,
        // so this is the ordinary case for it rather than a corner.
        //
        // "Longer than the timeout" is expressed the way everything else here is — by the
        // milliseconds the caller hands in — because the actor reads no clock: the command
        // is stamped at 0 and the first pass afterwards asks about 9_000, which is nine
        // times the timeout. Nothing sleeps and nothing waits.
        let (backend, cameras, info) = one_camera(1_000);
        let actor = cameras.actor(&info, 0).expect("a thread can be spawned");
        actor.ask(0, controls).expect("the fake opens");
        assert_eq!((backend.opens(), backend.closes()), (1, 0));

        assert_eq!(
            cameras.sweep(9_000),
            Vec::new(),
            "the pass immediately after a long command closed the camera it had just \
             finished using"
        );
        assert!(actor.activity().open, "the descriptor went away");
        assert_eq!(backend.closes(), 0);

        // One cadence, not a second timeout: the grace is a single pass, so the *next* one
        // closes. A fix that made the camera immortal until it was used again would pass
        // the assertion above and fail this one.
        assert_eq!(cameras.sweep(9_001), vec![info.id.clone()]);
        assert_eq!(backend.closes(), 1);

        // And the grace is per command rather than per actor: using the camera again arms
        // it again, so a second long command gets the same one pass and no more.
        actor.ask(9_002, controls).expect("it opens again");
        assert_eq!(
            cameras.sweep(20_000),
            Vec::new(),
            "the second command's pass"
        );
        assert_eq!(cameras.sweep(20_001), vec![info.id.clone()]);
        assert_eq!((backend.opens(), backend.closes()), (2, 2));
    }

    #[test]
    fn a_long_commands_grace_is_one_sweep_cadence_and_not_a_second_timeout() {
        // The arithmetic [`limits::CAMERA_IDLE_CLOSE_MS`]'s doc states, asserted against the
        // shipped numbers rather than against a literal — because the sentence it makes is
        // about *those* numbers ("five to ten seconds, not thirty") and a test with its own
        // timeout could not notice when they moved.
        //
        // The test above proves the shape with a millisecond of slack; this one prices it.
        // It is worth its own arm because the earlier reading of the fix said the grace was
        // worth a whole [`limits::CAMERA_IDLE_CLOSE_MS`] and nothing could go red on the
        // difference.
        let (backend, cameras, info) = one_camera(limits::CAMERA_IDLE_CLOSE_MS);
        let actor = cameras.actor(&info, 0).expect("a thread can be spawned");
        actor.ask(0, controls).expect("the fake opens");

        // A `wch_calibrate_sweep`, in the only terms the actor has: stamped when it started
        // and returning ten timeouts later. Every housekeeping pass in between saw `busy`
        // (the arm below this one is where that is proven), so the grace is intact.
        let ended = limits::CAMERA_IDLE_CLOSE_MS * 10;
        assert_eq!(
            cameras.sweep(ended + limits::CAMERA_IDLE_SWEEP_MS),
            Vec::new(),
            "the first pass after a long command took the descriptor away"
        );
        assert_eq!(
            cameras.sweep(ended + 2 * limits::CAMERA_IDLE_SWEEP_MS),
            vec![info.id.clone()],
            "the grace outlasted one pass, so it is not one pass"
        );
        assert_eq!(backend.closes(), 1);

        // And the reading the docs used to make: had the grace been worth a second timeout,
        // the camera would still be open here. It is not, and the two constants are what
        // make that a difference somebody can see — checked where they are, because two
        // numbers that had stopped being distinguishable would make the assertions above
        // pass for a reason nobody meant.
        const {
            assert!(2 * limits::CAMERA_IDLE_SWEEP_MS < limits::CAMERA_IDLE_CLOSE_MS);
        }
    }

    #[test]
    fn a_pass_that_runs_during_a_long_command_does_not_spend_its_grace() {
        // The interaction that makes the fix work rather than merely exist. A sweep is
        // minutes of camera time and the daemon's housekeeping runs every five seconds, so
        // *dozens* of passes happen while the command is running — if any of them consumed
        // the grace, the pass after the command would close the camera and N45 would be
        // undischarged on the only verb it is about.
        //
        // The actor's one thread is held by the first command for the whole of this test's
        // middle, which is what makes "during" an observation rather than a hope.
        let (backend, cameras, info) = one_camera(1_000);
        let actor = cameras.actor(&info, 0).expect("a thread can be spawned");
        actor.ask(0, controls).expect("the fake opens");

        let (started, holding) = mpsc::sync_channel::<()>(1);
        let (release, held) = mpsc::sync_channel::<()>(1);
        actor
            .submit(1, move |_device| {
                let _ = started.send(());
                // Ends when this test says so, never when a duration passes.
                let _ = held.recv();
                answering(|| {})
            })
            .expect("an empty queue");
        holding.recv().expect("the actor is inside the command");

        // Every one of these sees `busy` and answers from the published state — which is
        // also why they cost nothing and why they cannot reach the flag.
        for at in [2_000, 3_000, 4_000, 5_000] {
            assert_eq!(cameras.sweep(at), Vec::new(), "closed a camera in use");
        }
        assert_eq!(backend.closes(), 0);

        drop(release);
        // The command has to be *finished*, not merely released, before the pass below
        // means anything: `ask` returns after the actor has published that it is between
        // commands, so this is the synchronisation and not a guess.
        actor.ask(6_000, controls).expect("the queue drained");

        assert_eq!(
            cameras.sweep(60_000),
            Vec::new(),
            "the passes taken while the command was running spent its grace"
        );
        assert_eq!(cameras.sweep(60_001), vec![info.id.clone()]);
        assert_eq!(backend.closes(), 1);
    }

    #[test]
    fn a_sweep_closes_nothing_that_was_never_open() {
        let (backend, cameras, info) = one_camera(0);
        cameras.actor(&info, 0).expect("a thread can be spawned");

        // Zero timeout, so the deadline has certainly passed — and there is still nothing
        // to close, which is the difference between "the deadline expired" and "a
        // descriptor went away".
        assert_eq!(cameras.sweep(1), Vec::new());
        assert_eq!((backend.opens(), backend.closes()), (0, 0));
    }

    #[test]
    fn one_camera_runs_one_command_at_a_time_and_says_so_when_the_queue_is_full() {
        // D12's serialization, asserted rather than asserted about: the first command
        // holds the actor's only thread until this test lets go of it, so everything
        // observed while it is held is observed about a device that is provably occupied.
        let (_backend, cameras, info) = one_camera(10_000);
        let actor = cameras.actor(&info, 0).expect("a thread can be spawned");

        let (started, holding) = mpsc::sync_channel::<()>(1);
        let (release, held) = mpsc::sync_channel::<()>(1);
        let (finished, first_answer) = mpsc::sync_channel::<bool>(1);
        actor
            .submit(1, move |device| {
                let ok = device.is_ok();
                let _ = started.send(());
                // Blocks the actor's one thread until this test releases it. Not a sleep:
                // it ends when another thread says so, not when a duration passes.
                let _ = held.recv();
                answering(move || {
                    let _ = finished.send(ok);
                })
            })
            .expect("an empty queue");
        holding.recv().expect("the actor started the first command");

        // The queue takes exactly its bound and then refuses. Deterministic *because* the
        // thread is held: nothing can drain while this loop runs.
        let (answered, answers) = mpsc::sync_channel::<()>(limits::CAMERA_COMMAND_QUEUE_DEPTH);
        for queued in 0..limits::CAMERA_COMMAND_QUEUE_DEPTH {
            let answered = answered.clone();
            actor
                .submit(2, move |_device| {
                    answering(move || {
                        let _ = answered.send(());
                    })
                })
                .unwrap_or_else(|err| panic!("command {queued} of the bound was refused: {err}"));
        }
        let refused = actor
            .submit(2, |_device| answering(|| {}))
            .expect_err("the queue took more than it is bounded to");
        assert_eq!(refused.kind(), ErrorKind::Busy);

        // Nothing behind the held command has run, because there is one thread.
        assert_eq!(answers.try_recv(), Err(TryRecvError::Empty));
        // ... and the housekeeping pass answers from the published state rather than
        // joining the queue. This call returning at all is the assertion: the actor's one
        // thread is provably held by the first command, so a sweep that enqueued anything
        // would not come back until this test released it.
        assert!(!actor.sweep(u64::MAX).expect("the actor is alive"));

        drop(release);
        assert_eq!(
            first_answer.recv(),
            Ok(true),
            "the first command had a device"
        );
        for drained in 0..limits::CAMERA_COMMAND_QUEUE_DEPTH {
            answers
                .recv()
                .unwrap_or_else(|err| panic!("queued command {drained} never ran: {err}"));
        }
    }

    /// One camera whose actor's single thread is provably held, with its command queue
    /// filled to exactly [`limits::CAMERA_COMMAND_QUEUE_DEPTH`].
    ///
    /// Every arm of D12's `wait` flag starts here, and the determinism is the same sentence
    /// `one_camera_runs_one_command_at_a_time_and_says_so_when_the_queue_is_full` makes:
    /// **nothing can drain while the thread is held**, so "the queue is full" is an
    /// observation rather than a hope. Dropping the returned sender releases it.
    fn a_full_queue(
        cameras: &Cameras,
        info: &CameraInfo,
    ) -> (Arc<CameraActor>, mpsc::SyncSender<()>) {
        let actor = cameras.actor(info, 0).expect("a thread can be spawned");
        let (started, holding) = mpsc::sync_channel::<()>(1);
        let (release, held) = mpsc::sync_channel::<()>(1);
        actor
            .submit(1, move |_device| {
                let _ = started.send(());
                // Ends when this test says so, never when a duration passes.
                let _ = held.recv();
                answering(|| {})
            })
            .expect("an empty queue");
        holding.recv().expect("the actor started the first command");

        for queued in 0..limits::CAMERA_COMMAND_QUEUE_DEPTH {
            actor
                .submit(2, |_device| answering(|| {}))
                .unwrap_or_else(|err| panic!("command {queued} of the bound was refused: {err}"));
        }
        (actor, release)
    }

    #[test]
    fn a_full_queue_refuses_a_caller_that_will_not_wait_and_one_whose_deadline_has_passed() {
        // D12's flag, refusing half. Two callers meet the same full queue and take the
        // *same* refusal for two different reasons, which is the property that makes the
        // flag safe to add: it changes when the answer arrives and never what it is (note
        // N42's empty holder list is the other half of that sentence, asserted below).
        let (_backend, cameras, info) = one_camera(10_000);
        let (actor, release) = a_full_queue(&cameras, &info);

        let refused = actor
            .submit(2, |_device| answering(|| {}))
            .expect_err("the queue took more than it is bounded to");
        assert_eq!(refused.kind(), ErrorKind::Busy);

        // A deadline that has already arrived is a refusal and *not* a wait of zero length.
        // This is the arm that makes the bound assertable without anything waiting for a
        // clock: the same line of `send_waiting` answers here and when a budget runs out
        // mid-wait, so a build that had stopped honouring the deadline goes red here.
        let spent = actor
            .submit_with(2, Enqueue::WaitUntil(Instant::now()), |_device| {
                answering(|| {})
            })
            .expect_err("a deadline that has already arrived is not a wait");
        assert_eq!(spent.kind(), ErrorKind::Busy);
        assert_eq!(
            spent.to_string(),
            refused.to_string(),
            "the flag changed what the refusal says and not only when it arrives"
        );
        // Note **N42**: the holder list stays empty, because the work in the way is this
        // process's own and the field feeds `terminate_holder`.
        assert!(
            matches!(&spent, Error::Busy { holders, .. } if holders.is_empty()),
            "{spent:?}"
        );
        // **And the empty list is said rather than left to be read as ignorance** (docs/11
        // **M19** and its repair's own defect, note **N221**). The work in the way is this
        // process's, which is precisely what `Occupation` carries — and until this arm was
        // written the refusal rendered as *"held by an unidentified process"*, sending a
        // caller to hunt for a holder that is the program it is talking to.
        assert!(
            matches!(
                &spent,
                Error::Busy {
                    this_process: Some(schema::error::Occupation::RunningCommands),
                    ..
                }
            ),
            "a full queue is this process's own work and has to say so: {spent:?}"
        );
        let rendered = spent.to_string();
        assert!(
            !rendered.contains("unidentified"),
            "the refusal sends the caller looking for a process to blame: {rendered}"
        );

        drop(release);
    }

    #[test]
    fn a_caller_that_waits_takes_the_place_the_running_command_frees() {
        // D12's other half — "a second capture request **queues** … per its `wait` flag" —
        // and the mechanism notes N42 and N51 both named as missing: an enqueue that waits,
        // with a bound.
        //
        // Nothing here sleeps and nothing guesses. The queue is full because the actor's one
        // thread is held; the waiter is *provably parked* before this test lets go, because
        // `awaited_by` blocks until the waiter says so; and the sixty-second deadline is a
        // bound nothing reaches rather than a schedule — the same one-sided argument
        // `crate::settle`'s own suite makes about its deadline.
        let (_backend, cameras, info) = one_camera(10_000);
        let (actor, release) = a_full_queue(&cameras, &info);

        let (answered, outcome) = mpsc::sync_channel::<Result<()>>(1);
        let (ran, arrived) = mpsc::sync_channel::<()>(1);
        let waiting = Arc::clone(&actor);
        let waiter = std::thread::spawn(move || {
            let _ = answered.send(waiting.submit_with(
                2,
                Enqueue::WaitUntil(Instant::now() + Duration::from_secs(60)),
                move |_device| {
                    answering(move || {
                        let _ = ran.send(());
                    })
                },
            ));
        });

        // The subject's own signal that it is waiting, and the reason the release below
        // cannot come too early: an actor that had gone back to refusing would answer the
        // waiter immediately, and this test would then be comparing `Err(Busy)` against
        // `Ok(())` rather than racing the scheduler.
        //
        // The red-on-inverse, watched before this was called done: with `submit_with`'s
        // waiting arm rewired to `send`, nothing ever parks, this line never returns, and
        // the run becomes a named `TIMEOUT` (`.config/nextest.toml`, 60 s x 3) rather than a
        // hang — which is a failure with this test's name on it.
        actor.awaited_by(1);
        assert_eq!(
            outcome.try_recv(),
            Err(TryRecvError::Empty),
            "a caller that asked to wait was answered while the queue was provably full"
        );

        drop(release);
        outcome
            .recv()
            .expect("the waiting thread answered")
            .expect("a caller that asked to wait was refused when room came free");
        arrived
            .recv()
            .expect("the command the waiting caller enqueued never ran");
        waiter.join().expect("the waiting thread");
    }

    #[test]
    fn a_caller_waiting_on_a_thread_that_dies_is_told_the_device_is_gone_and_not_that_it_is_busy() {
        // E3, at the one seam where the two facts are easiest to confuse: a caller parked on
        // a full queue whose actor thread then unwinds (a backend that panics on device
        // vocabulary is measured, not hypothetical \[PF:1\]). Retrying this handle will
        // never work, so `Busy` — which invites a retry, and whose `holders` list invites a
        // `terminate_holder` — would be the wrong sentence, and waiting out the whole budget
        // for room that is never coming would be the wrong *timing*.
        let (_backend, cameras, info) = one_camera(10_000);
        let actor = cameras.actor(&info, 0).expect("a thread can be spawned");

        // The held command panics when this test lets go, so the thread dies without ever
        // serving what is queued behind it — which is what keeps the queue full across the
        // death rather than draining into it.
        let (started, holding) = mpsc::sync_channel::<()>(1);
        let (release, held) = mpsc::sync_channel::<()>(1);
        actor
            .submit(1, move |_device| -> Answering {
                let _ = started.send(());
                let _ = held.recv();
                panic!("a backend panicked on this camera's control vocabulary");
            })
            .expect("an empty queue");
        holding.recv().expect("the actor started the first command");
        for queued in 0..limits::CAMERA_COMMAND_QUEUE_DEPTH {
            actor
                .submit(2, |_device| answering(|| {}))
                .unwrap_or_else(|err| panic!("command {queued} of the bound was refused: {err}"));
        }

        let (answered, outcome) = mpsc::sync_channel::<Result<()>>(1);
        let waiting = Arc::clone(&actor);
        let waiter = std::thread::spawn(move || {
            let _ = answered.send(waiting.submit_with(
                2,
                Enqueue::WaitUntil(Instant::now() + Duration::from_secs(60)),
                |_device| answering(|| {}),
            ));
        });
        actor.awaited_by(1);

        drop(release);
        let gone = outcome
            .recv()
            .expect("the waiting thread answered")
            .expect_err("the actor's thread is gone");
        assert_eq!(
            gone.kind(),
            ErrorKind::DeviceGone,
            "a thread that died is not a device that is held (E3)"
        );
        waiter.join().expect("the waiting thread");
        assert!(!actor.is_alive());
    }

    #[test]
    fn the_shipped_wait_budget_is_the_one_constant_and_nothing_repeats_it() {
        // Rubric A8: `limits::CAMERA_ENQUEUE_WAIT_MS` has exactly one reader and this is the
        // assertion that it is the one the daemon gets. Both bounds, because a `waiting()`
        // that ignored the constant in either direction would still land between two
        // readings of the clock taken far enough apart.
        let before = Instant::now();
        let Enqueue::WaitUntil(deadline) = Enqueue::waiting() else {
            panic!("the shipped enqueue does not wait");
        };
        let after = Instant::now();
        let budget = Duration::from_millis(limits::CAMERA_ENQUEUE_WAIT_MS);
        assert!(deadline >= before + budget, "the budget was shortened");
        assert!(deadline <= after + budget, "the budget was lengthened");

        // And the other arm is a value with no clock in it at all, which is what makes
        // "this caller will not wait" free.
        assert_eq!(Enqueue::Refuse, Enqueue::Refuse);
        assert_ne!(Enqueue::Refuse, Enqueue::waiting());
    }

    #[test]
    fn two_lookups_of_one_camera_are_one_actor_and_one_descriptor() {
        // The registry's whole job, and the sentence design §2.1 rests the concurrency
        // model on: "one actor per camera serializes device access by construction". A
        // registry that started a thread per lookup would keep every test above green —
        // each of those holds one handle — and would quietly put two descriptors on one
        // node, which is the "two writers negotiate" state D12 exists to make
        // unrepresentable.
        let (backend, cameras, info) = one_camera(10_000);
        let first = cameras.actor(&info, 0).expect("a thread can be spawned");
        let second = cameras.actor(&info, 0).expect("a thread can be spawned");
        assert!(
            Arc::ptr_eq(&first, &second),
            "asking twice for one camera produced two actors"
        );

        first.ask(1, controls).expect("the fake opens");
        second
            .ask(2, controls)
            .expect("the same device, already open");
        assert_eq!(
            backend.opens(),
            1,
            "two lookups of one camera opened two descriptors"
        );

        // And the serialization is a property of the camera, not of the handle: work sent
        // through the second lookup waits for work sent through the first.
        let (started, holding) = mpsc::sync_channel::<()>(1);
        let (release, held) = mpsc::sync_channel::<()>(1);
        let (ran, arrived) = mpsc::sync_channel::<()>(1);
        first
            .submit(3, move |_device| {
                let _ = started.send(());
                let _ = held.recv();
                answering(|| {})
            })
            .expect("an empty queue");
        holding.recv().expect("the first handle holds the thread");
        second
            .submit(3, move |_device| {
                answering(move || {
                    let _ = ran.send(());
                })
            })
            .expect("room behind it");
        assert_eq!(
            arrived.try_recv(),
            Err(TryRecvError::Empty),
            "the second handle reached the device while the first one had it"
        );

        drop(release);
        arrived
            .recv()
            .expect("the queue drained once the device was free");
    }

    #[test]
    fn two_cameras_do_not_queue_behind_each_other() {
        // The other half of "one actor per camera": the serialization is per device, and a
        // second camera is a second thread.
        let backend = Arc::new(
            FakeBackend::new(vec![
                testkit::fixtures::synthetic_basic(),
                testkit::fixtures::synthetic_basic(),
            ])
            .expect("the synthetic profile is this build's version"),
        );
        let cameras = Cameras::new(Arc::clone(&backend) as Arc<dyn CameraBackend>);
        let enumerated = backend.enumerate().expect("two profiles are two cameras");
        let (first, second) = match enumerated.as_slice() {
            [first, second] => (first.clone(), second.clone()),
            other => panic!("two profiles enumerated {} cameras", other.len()),
        };
        assert_ne!(first.id, second.id, "D1 assigns distinct ids");

        let held = cameras.actor(&first, 0).expect("a thread can be spawned");
        let free = cameras.actor(&second, 0).expect("a thread can be spawned");

        let (started, holding) = mpsc::sync_channel::<()>(1);
        let (release, blocked) = mpsc::sync_channel::<()>(1);
        held.submit(1, move |_device| {
            let _ = started.send(());
            let _ = blocked.recv();
            answering(|| {})
        })
        .expect("an empty queue");
        holding.recv().expect("the first camera's actor is held");

        // Answered while the other camera's actor is provably stuck.
        free.ask(1, controls)
            .expect("a second camera is a second thread");
        drop(release);
    }

    #[test]
    fn one_wedged_camera_does_not_hold_the_housekeeping_pass_for_every_other_one() {
        // The property that makes the idle-close pass survivable on a real machine. A
        // device command can take minutes by design (P4c's calibration sweep) and can
        // fail to return at all (a `DQBUF` against a driver that has stopped delivering),
        // and `Cameras::sweep` walks the actors in one thread — so if a busy actor were
        // *asked* whether it was idle, every other camera would stay open for as long as
        // that one command lasted, with no log line and no bound.
        //
        // Two cameras: A is held mid-command by this test, B is open and past its
        // deadline. The pass has to close B while A is stuck, and it has to come back.
        let backend = Arc::new(
            FakeBackend::new(vec![
                testkit::fixtures::synthetic_basic(),
                testkit::fixtures::synthetic_basic(),
            ])
            .expect("the synthetic profile is this build's version"),
        );
        let cameras =
            Cameras::with_idle_timeout(Arc::clone(&backend) as Arc<dyn CameraBackend>, 1_000);
        let enumerated = backend.enumerate().expect("two profiles are two cameras");
        let (wedged, healthy) = match enumerated.as_slice() {
            [first, second] => (first.clone(), second.clone()),
            other => panic!("two profiles enumerated {} cameras", other.len()),
        };

        let stuck = cameras.actor(&wedged, 0).expect("a thread can be spawned");
        let idle = cameras.actor(&healthy, 0).expect("a thread can be spawned");
        idle.ask(0, controls).expect("the fake opens");
        assert_eq!((backend.opens(), backend.closes()), (1, 0));

        let (started, holding) = mpsc::sync_channel::<()>(1);
        let (release, held) = mpsc::sync_channel::<()>(1);
        stuck
            .submit(0, move |_device| {
                let _ = started.send(());
                // Never returns until this test says so. Not a sleep: it ends when
                // another thread speaks, which is the only kind of waiting this project
                // allows.
                let _ = held.recv();
                answering(|| {})
            })
            .expect("an empty queue");
        holding.recv().expect("the wedged camera holds its thread");

        // B's own grace, spent (note N45): the pass that follows a completed command
        // declines once. It also asks A, which is the point — this call returning is the
        // assertion, and it would already have hung here if a busy actor were consulted
        // through its queue.
        assert_eq!(cameras.sweep(1_999), Vec::new());

        // The whole assertion is that this call returns, and returns with B in it. A pass
        // that asked A would still be inside `answer.recv()` when this test timed out.
        assert_eq!(cameras.sweep(2_000), vec![healthy.id.clone()]);
        assert_eq!(
            backend.closes(),
            1,
            "the healthy camera's descriptor is still open"
        );
        assert!(
            cameras
                .activity()
                .iter()
                .all(|activity| activity.camera != healthy.id || !activity.open)
        );

        // And the wedged camera was never asked, so it is exactly where it was: the pass
        // did not queue anything behind the command that is still running.
        drop(release);
        stuck
            .ask(3_000, controls)
            .expect("the command this test was holding has finished");
    }

    #[test]
    fn an_actor_whose_thread_dies_stops_claiming_the_device_it_no_longer_holds() {
        // The status surface after the failure mode PF:1 makes measured. Unwinding drops
        // the `Box<dyn Camera>`, so the descriptor is gone — and the flag that says so has
        // to fall with it, because nothing will ever run on that thread again to lower it
        // and `Cameras::activity` lists dead actors along with live ones. A daemon that
        // went on reporting a camera it had already released is the exact inverse of what
        // docs/7 P4b asks the surface for.
        let (backend, cameras, info) = one_camera(10_000);
        let doomed = cameras.actor(&info, 0).expect("a thread can be spawned");
        doomed.ask(1, controls).expect("the fake opens");
        assert!(doomed.activity().open);
        assert_eq!((backend.opens(), backend.closes()), (1, 0));

        // Three commands, queued while the thread is provably held, so that the thread's
        // *end* is something this test can wait for rather than guess at: the panic ends
        // the loop, and the command queued behind it is therefore never run — it is
        // dropped with the inbox, which the thread drops after the guard that lowers the
        // flags. So a closed sentinel channel means "the guard has already run", and
        // nothing here polls or sleeps for it.
        let (started, holding) = mpsc::sync_channel::<()>(1);
        let (release, held) = mpsc::sync_channel::<()>(1);
        doomed
            .submit(2, move |_device| {
                let _ = started.send(());
                let _ = held.recv();
                answering(|| {})
            })
            .expect("an empty queue");
        holding.recv().expect("the actor holds the thread");
        doomed
            .submit(3, |_device| -> Answering {
                panic!("a backend panicked on this camera's control vocabulary");
            })
            .expect("room behind it");
        let (sentinel, never_ran) = mpsc::sync_channel::<()>(1);
        doomed
            .submit(4, move |_device| {
                answering(move || {
                    let _ = sentinel.send(());
                })
            })
            .expect("room behind it");

        drop(release);
        assert_eq!(
            never_ran.recv(),
            Err(std::sync::mpsc::RecvError),
            "the command behind the panic ran, so the thread did not die"
        );

        // The descriptor and the claim about it, checked against each other (note N42).
        assert_eq!(backend.closes(), 1, "the unwind did not drop the handle");
        assert!(!doomed.is_alive());
        assert_eq!(
            cameras.activity(),
            vec![CameraActivity {
                camera: info.id.clone(),
                open: false,
                // The panicking command, which is the last one that reached the device.
                last_used_ms: 3,
            }],
            "the registry claims a camera this process has already released"
        );

        // And a caller still gets the refusal that says which fact this is (E3).
        assert_eq!(
            doomed
                .ask(5, controls)
                .expect_err("the thread is gone")
                .kind(),
            ErrorKind::DeviceGone
        );

        // And housekeeping has nothing to do about it: a dead actor closed nothing, and
        // the death matters at the next request, which replaces it.
        assert_eq!(cameras.sweep(u64::MAX), Vec::new());
        assert!(cameras.activity().iter().all(|activity| !activity.open));
    }

    #[test]
    fn an_actor_whose_thread_dies_refuses_and_is_replaced() {
        // A backend that panics on device vocabulary is measured, not hypothetical
        // \[PF:1\], so the daemon has to survive one. The panic below stands in for it.
        let (backend, cameras, info) = one_camera(10_000);
        let doomed = cameras.actor(&info, 0).expect("a thread can be spawned");
        assert!(doomed.is_alive());

        let waiting = doomed.ask(1, |_device| -> Result<()> {
            panic!("a backend panicked on this camera's control vocabulary");
        });
        let err = waiting.expect_err("the thread died holding the answer");
        assert_eq!(
            err.kind(),
            ErrorKind::DeviceGone,
            "a thread that died is not a device that is busy (E3)"
        );

        // The handle is now useless, and says so rather than hanging.
        assert_eq!(
            doomed
                .ask(2, controls)
                .expect_err("the thread is gone")
                .kind(),
            ErrorKind::DeviceGone
        );

        // ... but the camera is not. The registry hands out a fresh actor, which is the
        // difference between "one camera stopped working" and "the daemon fell over".
        let replacement = cameras.actor(&info, 3).expect("a thread can be spawned");
        assert!(replacement.is_alive());
        replacement
            .ask(3, controls)
            .expect("a replaced actor opens the device");
        assert_eq!(backend.opens(), 2, "the dead actor's open was not reused");
    }

    #[test]
    fn a_camera_that_cannot_be_opened_refuses_every_time_and_never_looks_open() {
        // Availability is not capability (E3): a busy device is a refusal the caller sees,
        // and the status surface must not claim a descriptor the actor does not have.
        let (backend, cameras, info) = one_camera(1_000);
        backend.hold_fault(fake::Fault::Busy);
        let actor = cameras.actor(&info, 0).expect("a thread can be spawned");

        for at in [10, 20] {
            let err = actor
                .ask(at, controls)
                .expect_err("the device is held elsewhere");
            assert_eq!(err.kind(), ErrorKind::Busy);
            assert_eq!(
                actor.activity(),
                CameraActivity {
                    camera: info.id.clone(),
                    open: false,
                    // Still a use: an actor whose opens keep failing must not read as idle
                    // for the whole run.
                    last_used_ms: at,
                }
            );
        }
        assert_eq!((backend.opens(), backend.closes()), (0, 0));

        backend.release_fault(fake::Fault::Busy);
        actor.ask(30, controls).expect("the holder let go");
        assert!(actor.activity().open);
        assert_eq!(backend.opens(), 1);
    }

    #[test]
    fn the_status_surface_lists_every_actor_in_camera_order() {
        let backend = Arc::new(
            FakeBackend::new(vec![
                testkit::fixtures::synthetic_basic(),
                testkit::fixtures::synthetic_basic(),
            ])
            .expect("the synthetic profile is this build's version"),
        );
        let cameras =
            Cameras::with_idle_timeout(Arc::clone(&backend) as Arc<dyn CameraBackend>, 50);
        assert_eq!(cameras.activity(), Vec::new(), "nothing has been asked for");

        let enumerated = backend.enumerate().expect("two profiles are two cameras");
        for info in enumerated.iter().rev() {
            cameras.actor(info, 0).expect("a thread can be spawned");
        }
        let listed: Vec<CameraId> = cameras
            .activity()
            .into_iter()
            .map(|activity| activity.camera)
            .collect();
        let mut expected: Vec<CameraId> = enumerated.iter().map(|info| info.id.clone()).collect();
        expected.sort();
        assert_eq!(
            listed, expected,
            "the listing follows the order it was asked in"
        );

        // Only the camera that was used is open, and only it closes.
        let used = enumerated.first().expect("two cameras").clone();
        cameras
            .actor(&used, 100)
            .expect("already spawned")
            .ask(100, controls)
            .expect("the fake opens");
        assert_eq!(
            cameras
                .activity()
                .into_iter()
                .filter(|activity| activity.open)
                .map(|activity| activity.camera)
                .collect::<Vec<_>>(),
            vec![used.id.clone()]
        );
        // Two passes, because the first after a command declines (note N45). The camera
        // that was never used has no grace to spend and closes nothing either way.
        assert_eq!(cameras.sweep(150), Vec::new());
        assert_eq!(cameras.sweep(151), vec![used.id]);
        assert!(cameras.activity().iter().all(|activity| !activity.open));
    }

    #[test]
    fn the_default_registry_takes_its_idle_timeout_from_the_one_constant() {
        // The constant has a reader, and the reader is the shipped default rather than a
        // number repeated in the daemon (rubric A8).
        let (backend, _, _) = one_camera(0);
        let cameras = Cameras::new(backend as Arc<dyn CameraBackend>);
        assert_eq!(cameras.idle_timeout_ms(), limits::CAMERA_IDLE_CLOSE_MS);
    }
}
