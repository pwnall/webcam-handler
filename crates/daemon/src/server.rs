//! The server half of the T5 wire surface (design D10, docs/7 P4b and P4c).
//!
//! `webcam-handler-api` declares the trait; this module implements it. There is no second
//! wire surface here and no second registration path: the daemon builds its `RpcModule`
//! with the generated `into_rpc()`, which is what `crates/api` calls "the only
//! authoritative statement of what the T5 surface registers".
//!
//! ## What is routed
//!
//! **All of it.** docs/7 gave P4b six read verbs — `list`, `info`, `controls`, `get`,
//! `calibrate status` and `calibrate list` — and P4c "the mutating half over RPC", which
//! landed in steps: the five control-shaped verbs (`set`, `snapshot`, `restore`,
//! `discover_pairs`, `profile_capture`), then `photo`, and finally `terminate_holder` and
//! the six `calibrate_*` verbs that write. So [`ROUTED`] is the whole nineteen-method
//! surface, and the map of methods answering [`schema::Error::Unimplemented`] is gone with
//! the producer that filled it (note **N43**: "P4c cannot land without emptying it").
//!
//! [`ROUTED`] stays anyway, and what it is worth is smaller than it used to be, so it is
//! stated rather than implied: it is a **second deliberate transcription** of a
//! compatibility contract, in `fixtures/d13-rpc-codes.tsv`'s tradition, whose value is that
//! a rename in `crates/api` costs a second diff somebody has to write on purpose. What stops
//! a twentieth method landing *unrouted* is not this list — `wire_surface!` declares the
//! trait and `api::METHODS` from one source, so a twentieth method fails to compile
//! `impl WchRpcServer for Wchd` until it is implemented. The compiler owns that law; this
//! constant owns the spelling. After P4c the variant's only producer left in the workspace
//! is `webcam-handler-v4l2::unimplemented_surface()`, which P4d deletes.
//!
//! ## Where the answers come from
//!
//! The engine, and only the engine. Every routed verb reaches the same functions
//! `crates/cli`'s in-process executor reaches — `engine::resolve::{camera, list}`,
//! `engine::pairing::{in_effect, describe}`, `engine::write::set`,
//! `engine::snapshot::{take, restore}`, `engine::discover::report`,
//! `engine::profile::capture`, `engine::photo::take`, `engine::calibrate::run`, and
//! `engine::lifecycle::{status, list, session_to_update, create, discover_pairs, draft,
//! select, apply, restore}` — because a verb implemented twice is the defect T4 and T5
//! exist to prevent, and P4f's parity gate will compare the two surfaces byte for byte.
//! Where the two used to differ was in *assembly*, not in policy, so the assemblies with a
//! rule inside them moved into the engine as this landed rather than being copied here. At
//! P4b that was `list`, which was copied, D1 comment and all, and the P4b review caught it.
//! At P4c it was [`engine::discover::report`] — the `DiscoveryReport` assembly note **N34**
//! booked against this sub-milestone, because probe-then-read-then-merge has three rules in
//! it and a second author is free to get any of them wrong — plus the two host facts
//! `profile_capture` records, `engine::profile::kernel_release` and `schema::TOOL_VERSION`;
//! the extension-to-encoding rule `photo` needs, which moved onto
//! [`schema::capture::Sink::writable_format`] so that a `.webp` off a socket is refused by
//! the same sentence `wch photo -o a.webp` is refused by (note N46, debt D-1); the three
//! control-shaped assemblies — `engine::write::set_requested`,
//! `engine::snapshot::take_in_effect` and `engine::snapshot::restore_in_effect` — whose rule
//! is *which pair set this device is in now*, read off the device rather than supplied,
//! because that decides whether an automation control is switched off first and D4's restore
//! order (the P4c review caught these three as copies, which is the same finding the P4b
//! review made about `list`); and finally the four the calibrate verbs needed —
//! `session_to_update` (the token is *proof*, and the read is half of the read-modify-write),
//! `select` (the `Selection` match records a selector whichever branch runs), `restore`
//! (which pair set, and "no snapshot is not a failure"), and the branch `calibrate_plan`
//! keeps.
//!
//! What this module adds on top of an engine call is small and each piece is about a
//! request a socket can build and a command line cannot: `addressable`, which answers for a
//! photo's sink before a camera opens; `photo_response`, which checks the answer against
//! itself before it is sent (both are note **N34**'s named consumers); and
//! `terminate_holder`'s three — `holder_node`, `not_this_daemon` and
//! `Wchd::node_still_held` — which are a verb with no engine home at all, because the thing
//! it acts on is a *process* rather than a camera (note **N48**).
//!
//! ## What blocks, and where
//!
//! Two kinds of work, and they go to two different places:
//!
//! - **Anything that needs the open device** goes to that camera's actor thread (D12) —
//!   `Wchd::on_camera` hands it a closure holding a `tokio::sync::oneshot::Sender` and
//!   awaits the receiver, so a request queued behind a minutes-long sweep occupies no
//!   thread anywhere. That is the shape `crates/api`'s trait doc asks for, and the reason
//!   `engine::actor` names no reply channel of its own (note N41).
//! - **Enumeration and the session store** need no open camera and hold nothing between
//!   calls, so they go to `tokio::task::spawn_blocking` (`Wchd::offload`): a blocking
//!   pool thread rather than a runtime worker, because `enumerate` is a walk of `/sys` and
//!   an ioctl per node.
//!
//! Neither happens on a runtime worker, which is the whole of the rule: V4L2 ioctls and
//! `DQBUF` block (design §2.1), so nothing that can block runs where a runtime worker
//! would be parked on it.
//!
//! ## The state directory's lock, and the thing `flock` cannot do
//!
//! D9 gives the daemon one advisory lock for its lifetime, [`crate::state::OwnedState`]
//! takes it, and this module holds the *same* token — never a second one. Taking a second
//! is not a deadlock but something worse: `flock` denies a second open file description in
//! this process exactly as it denies another process's, so
//! [`engine::store::SessionStore::with_lock`] here would answer a client
//! [`schema::Error::StoreLocked`] naming this daemon's own pid and advising it to use
//! `wchc` against the daemon it is already talking to. `crate::state`'s header is where
//! that trap is written down and `tests/lock.rs` is the red test.
//!
//! The read verbs take no lock at all — reading is not a state write, and a
//! `calibrate status` that refused while a daemon held the lock would be a status verb
//! nobody can use on the machine the sessions are on. Neither do the camera-shaped mutating
//! verbs: `set`, `snapshot`, `restore`, `discover_pairs`, `profile_capture` and `photo`
//! write to a *camera*, or to a path the caller named, and none of them touches the session
//! tree.
//!
//! **A held lock does not serialize this daemon against itself, and that is a defect
//! waiting to be written rather than a subtlety** (note **N47**). `wch` is safe because D9's
//! daemonless protocol takes the flock per operation, so two `wch` processes cannot
//! interleave a read-modify-write; a daemon holding one flock for its lifetime gets no
//! mutual exclusion between its own concurrent request tasks, and `calibrate_plan --order`
//! and `calibrate_select` open no camera at all, so not even the per-camera actor separates
//! them. So the token lives behind `Inner::sessions`, a `tokio::sync::Mutex`, and the only
//! way to reach it is to hold that mutex — structural rather than remembered, in the same
//! spirit as `engine::store` expressing "under the lock" as a parameter.

use std::sync::Arc;

use api::{WchRpcServer, WireError};
use camino::{Utf8Path, Utf8PathBuf};
use engine::actor::{CameraActivity, Cameras, OpenCamera};
use engine::settle::{Clock, Millis, MonotonicClock};
use engine::store::{SessionStore, StoreLock};
use jsonrpsee::core::async_trait;
use schema::backend::CameraBackend;
use schema::camera::{CameraId, CameraInfo};
use schema::capture::{PhotoRequest, Sink};
use schema::control::{ControlDesc, ControlSlug, ControlWrite};
use schema::profile::DeviceProfile;
use schema::report::{
    CameraDetail, CameraList, ControlReport, DiscoveryReport, TerminationReport, TerminationSignal,
    WriteReport,
};
use schema::session::{Selection, Session, SessionList, SessionRef, SessionStatus, SweepRequest};
use schema::snapshot::{RestoreReport, Snapshot};
use schema::{Error, limits};
use tokio::sync::oneshot;

/// The wire names this build routes.
///
/// A **pin**, in the tradition of `crates/api/fixtures/d13-rpc-codes.tsv` and of the
/// nineteen spellings `crates/api` asserts: which verbs a build answers is a fact a client
/// depends on, so widening it has to be a diff somebody wrote on purpose.
///
/// docs/7 P4b named the first six: "read-verb routing (`list`, `info`, `controls`, `get`,
/// `calibrate status/list`)". P4c added the mutating half in steps — the five
/// control-shaped ones (`set`, `snapshot`, `restore`, `discover_pairs` and
/// `profile_capture`; `discover_pairs` is among *these* rather than among P4b's precisely
/// because it writes to the camera, which is why it is its own method, note N30), then
/// `photo`, then `terminate_holder` and the six `calibrate_*` verbs that write.
///
/// **It is a pin even now that it names everything, and it is only a pin.** While a method
/// could be unrouted, `api::METHODS` minus this list was the derived half that proved
/// nothing fell into neither, and the partition was doing work. What is left is the equality
/// — this list *is* `api::METHODS`, and it *is* what `into_rpc()` registers — which catches a
/// **rename**, not an omission: adding a method to `wire_surface!` stops the build at the
/// trait impl long before it reaches either assertion. Kept, because a client depends on the
/// spellings and a second diff is the cost of changing one; not kept because it is the thing
/// that stops an unrouted method, which it is not.
pub const ROUTED: &[&str] = &[
    "wch_calibrate_apply",
    "wch_calibrate_list",
    "wch_calibrate_plan",
    "wch_calibrate_restore",
    "wch_calibrate_select",
    "wch_calibrate_start",
    "wch_calibrate_status",
    "wch_calibrate_sweep",
    "wch_controls",
    "wch_discover_pairs",
    "wch_get",
    "wch_info",
    "wch_list",
    "wch_photo",
    "wch_profile_capture",
    "wch_restore",
    "wch_set",
    "wch_snapshot",
    "wch_terminate_holder",
];

/// The timer the idle-sweep driver runs on.
///
/// A named function with a test on it, because the *decision* it carries is invisible in
/// the call that makes it: `tokio::time::interval` defaults to
/// [`tokio::time::MissedTickBehavior::Burst`], under which every tick that came due while
/// a pass was running is still owed — so a pass that overran the cadence is followed by
/// back-to-back passes until the timer has caught up. On this daemon that would mean a
/// long device command (a P4c calibration sweep occupies a camera for minutes) is followed
/// by a dozen housekeeping round trips through every actor's command queue, competing with
/// the request the camera's owner just sent.
///
/// [`tokio::time::MissedTickBehavior::Delay`] is the behaviour
/// [`Wchd::spawn_idle_sweeps_every`]'s doc describes and the one this daemon wants: a
/// missed tick is a sweep that happens later, not a sweep that is owed.
fn idle_sweep_cadence(period_ms: Millis) -> tokio::time::Interval {
    let mut cadence = tokio::time::interval(std::time::Duration::from_millis(period_ms));
    cadence.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    cadence
}

/// Everything the daemon answers from.
///
/// Behind an `Arc` because the generated `into_rpc()` **moves** `self` into jsonrpsee's own
/// `Arc` (`RpcModule::new`), and the composition root still needs the registry afterwards
/// to sweep idle cameras. Cloning this is an atomic bump; there is exactly one shared state in
/// a process, which is what makes "one actor per camera" true of the process rather than of
/// a handle (`engine::actor::Cameras` states the other half).
#[derive(Debug, Clone)]
pub struct Wchd(Arc<Inner>);

/// The daemon's shared state. One of each, never two.
///
/// **There is no backend handle here, and that is the point.** `engine::actor`'s header
/// rests the concurrency model on "nothing hands the `Box<dyn Camera>` out … two callers
/// write at once is unrepresentable rather than prevented", and a
/// `Arc<dyn CameraBackend>` beside the registry would make `inner.backend.open(&id)?`
/// compile in every handler in this module — a second descriptor on a node an actor
/// already owns, one line away from the P4c author who copies `crates/cli`'s in-process
/// executor (where the same call is correct, because a `wch` process has no actor). The
/// two things the daemon needs of a backend, [`Cameras::enumerate`] and [`Cameras::list`],
/// are on the registry instead, and the registry has no `open`.
#[derive(Debug)]
struct Inner {
    /// One actor per open camera, and the daemon's only handle on the backend the
    /// composition root chose (design §2.11).
    cameras: Cameras,
    /// D9's session tree, read without a lock (see this module's header).
    store: SessionStore,
    /// D9's token, behind the one thing that serializes this process against itself.
    ///
    /// **The mutex is not protecting the `Arc`; it is protecting the read-modify-write the
    /// `Arc` licenses.** Every mutating store function takes a `&StoreLock` by signature so
    /// the discipline cannot be forgotten, and `engine::lifecycle::session_to_update`
    /// extends that to the *read* — but neither can help a process that holds one flock and
    /// runs two request tasks, because `flock` does not exclude its holder from itself
    /// (note **N47**). Putting the token behind this mutex makes "one session edit at a
    /// time" the only way to spell a session edit at all, rather than a rule six handlers
    /// have to remember.
    ///
    /// The cost, which is real and is stated rather than discovered: `wch_calibrate_sweep`
    /// holds this for the whole sweep, which is minutes of camera time, so a
    /// `wch_calibrate_select` against an unrelated session waits for it. `wch` pays the
    /// same cost in a different currency — a concurrent `wch` is *refused* `StoreLocked`
    /// rather than made to wait — and waiting is the better answer for a daemon, because
    /// the refusal it would otherwise give names this daemon's own pid. The bound is the
    /// sweep's own (`limits::MAX_SWEEP_SAMPLES`) plus the client's request timeout, which
    /// `crates/api`'s `calibrate_sweep` doc already tells a client to raise.
    sessions: tokio::sync::Mutex<Arc<StoreLock>>,
    /// Where `now` enters. The actor reads no clock — every command carries the caller's
    /// reading (note N41) — so this is the caller. Two readers now: the idle deadline, and
    /// the settle policy a `photo` runs on, which is *monotonic* time and not the wall
    /// time that goes in a photo's EXIF. Conflating those two is how an NTP step becomes a
    /// settle failure, which is why `engine::photo::take` takes them separately.
    clock: MonotonicClock,
}

impl Wchd {
    /// A daemon over `backend`, closing idle cameras after
    /// [`limits::CAMERA_IDLE_CLOSE_MS`].
    ///
    /// `lock` is the token [`crate::state::OwnedState`] took at startup, not a second one:
    /// see this module's header for what a second would do.
    #[must_use]
    pub fn new(backend: Arc<dyn CameraBackend>, store: SessionStore, lock: Arc<StoreLock>) -> Wchd {
        Wchd::with_idle_timeout(backend, store, lock, limits::CAMERA_IDLE_CLOSE_MS)
    }

    /// The same, with D12's "configurable" idle timeout supplied.
    ///
    /// Mirrors `engine::actor::Cameras`'s own pair rather than exposing the registry,
    /// because a caller that could hand in a registry could hand in one built over a
    /// *different* backend than the one this value enumerates from — two opinions about
    /// what cameras exist, in a type whose whole job is that there is one.
    #[must_use]
    pub fn with_idle_timeout(
        backend: Arc<dyn CameraBackend>,
        store: SessionStore,
        lock: Arc<StoreLock>,
        idle_after_ms: Millis,
    ) -> Wchd {
        Wchd(Arc::new(Inner {
            cameras: Cameras::with_idle_timeout(backend, idle_after_ms),
            store,
            sessions: tokio::sync::Mutex::new(lock),
            clock: MonotonicClock::new(),
        }))
    }

    /// What every camera this daemon has an actor for is doing — docs/7 P4b's status
    /// surface, reached through the value an integration test holds (note N42).
    #[must_use]
    pub fn activity(&self) -> Vec<CameraActivity> {
        self.0.cameras.activity()
    }

    /// Close every camera that has gone idle; answer which ones closed.
    ///
    /// One pass, at this instant. **Blocking** — it waits for each actor to acknowledge,
    /// which is what makes the answer an observation rather than a request — so
    /// [`Wchd::spawn_idle_sweeps`] runs it on a blocking thread and a test calls it
    /// directly.
    pub fn sweep_idle_cameras(&self) -> Vec<CameraId> {
        self.0.cameras.sweep(self.0.clock.now_ms())
    }

    /// Run [`Wchd::sweep_idle_cameras`] on this build's cadence, for as long as the
    /// runtime lives.
    ///
    /// The shipped caller, and the one reader of [`limits::CAMERA_IDLE_SWEEP_MS`]:
    /// everything about the driver is [`Wchd::spawn_idle_sweeps_every`]'s, so a test can
    /// talk about a cadence without the constant, and the constant has exactly one home.
    ///
    /// Must be called from inside a tokio runtime.
    pub fn spawn_idle_sweeps(&self) -> tokio::task::JoinHandle<()> {
        self.spawn_idle_sweeps_every(limits::CAMERA_IDLE_SWEEP_MS)
    }

    /// Run [`Wchd::sweep_idle_cameras`] every `cadence_ms`, for as long as the runtime
    /// lives.
    ///
    /// D12 says the daemon "closes on idle", and a deadline nobody checks is not a
    /// deadline: `engine::actor` computes idleness and **this is the thing that asks**.
    /// How long after a deadline a camera closes, and what a pass costs, are
    /// [`limits::CAMERA_IDLE_SWEEP_MS`]'s to state — restating them here would be two
    /// homes for one piece of arithmetic, which is how they come to disagree.
    /// Without it a shipped `wchd` would open a camera on the first `wch_info` and hold
    /// the descriptor until the process exited, which is precisely the complaint D12
    /// exists to answer — so the driver is a parameterised function with a test on it
    /// rather than a body only `main` reaches.
    ///
    /// A timer, not a sleep-as-synchronization: nothing waits on this to learn anything,
    /// and the tests that drive it run on tokio's paused clock, where advancing time is an
    /// argument rather than a wait.
    ///
    /// Each pass is awaited before the next tick is honoured, and the cadence is
    /// [`tokio::time::MissedTickBehavior::Delay`] so the ticks that came due *while* a pass
    /// was running are not all still owed: a sweep that queued behind a long device command
    /// delays the next sweep, it does not earn a backlog of passes to run back to back
    /// against the camera whose owner is using it. `idle_sweep_cadence` is where that decision
    /// is made and asserted.
    ///
    /// Nothing stops this task: it ends when the runtime it was spawned on is dropped,
    /// which is when the process is stopping. Ending it in an order is P4e's shutdown
    /// discipline; the returned handle is what a test uses to abort one it started.
    ///
    /// Must be called from inside a tokio runtime.
    pub fn spawn_idle_sweeps_every(&self, cadence_ms: Millis) -> tokio::task::JoinHandle<()> {
        let wchd = self.clone();
        tokio::spawn(async move {
            let mut cadence = idle_sweep_cadence(cadence_ms);
            loop {
                cadence.tick().await;
                let pass = wchd.clone();
                let closed = tokio::task::spawn_blocking(move || pass.sweep_idle_cameras()).await;
                match closed {
                    Ok(closed) => {
                        for camera in closed {
                            // A camera the operator can now use from another application,
                            // which is what D12's idle close is for — worth a line, and a
                            // camera id is not a frame (see `crate::logging`).
                            tracing::info!(%camera, "closed an idle camera");
                        }
                    }
                    // The blocking pool is gone, which happens when the runtime is
                    // shutting down. There is nothing left to sweep and nobody to tell.
                    Err(_) => break,
                }
            }
        })
    }

    /// Run `work` on a blocking pool thread, with the daemon's state.
    ///
    /// For the work that needs no open camera: enumeration, and reads of the session tree.
    /// Both are blocking (an ioctl per node; a directory walk and a parse per document) and
    /// both are safe to run concurrently — V4L2 permits many opens, and D9's read verbs take
    /// no lock — so they do not need an actor and must not have one: queueing a `calibrate
    /// list` behind a camera's sweep would be a second queue for a device that is not
    /// involved.
    async fn offload<T, F>(&self, work: F) -> schema::Result<T>
    where
        F: FnOnce(&Inner) -> schema::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let inner = Arc::clone(&self.0);
        match tokio::task::spawn_blocking(move || work(&inner)).await {
            Ok(answer) => answer,
            // The pool thread panicked (a backend that panics on device vocabulary is
            // measured, not hypothetical \[PF:1\]) or the runtime is shutting down. Same
            // variant `engine::actor` uses when it cannot start a thread, for the same
            // reason: it is this process failing to perform an operation, and it must not
            // be spelled like a device declining to.
            Err(err) => Err(Error::DeviceIo {
                operation: "run a blocking daemon task".to_owned(),
                errno: None,
                message: err.to_string(),
            }),
        }
    }

    /// Resolve a caller-supplied id or prefix (D1) against a live enumeration.
    ///
    /// The rule is `engine::resolve::camera`'s, which is what stops `wch` and `wchd`
    /// disagreeing about what a prefix means. Enumeration is live every time (E2): a daemon
    /// that cached its camera list would answer about a camera that had been unplugged, and
    /// noticing that is P4d's hotplug watch rather than an assumption this can make.
    async fn resolve(&self, requested: CameraId) -> schema::Result<CameraInfo> {
        self.offload(move |inner| {
            let cameras = inner.cameras.enumerate()?;
            engine::resolve::camera(&cameras, &requested).cloned()
        })
        .await
    }

    /// Resolve `requested`, then run `work` against its open device (D12).
    ///
    /// The camera opens here and nowhere else, on the first command that needs it: the
    /// registry hands back an actor without opening anything, and the actor opens on first
    /// use. That is why `wch_list` costs a daemon no descriptor at all.
    ///
    /// The reply channel is this function's, not the actor's (note N41): `work` runs on the
    /// actor's thread and sends through a `oneshot::Sender`, whose `send` is synchronous and
    /// non-blocking, and the receiver is awaited here. Nothing is parked on a thread.
    ///
    /// The resolved [`CameraInfo`] comes back with the answer because every verb that opens
    /// a camera also reports which one it opened, and resolving it twice would be two
    /// enumerations for one request. A verb that needs the resolution *before* the closure —
    /// the five calibrate ones, which check a session's fingerprint against the camera —
    /// takes [`Wchd::resolve`] and then [`Wchd::on_resolved_camera_with_state`] rather than
    /// paying for a second enumeration and getting a second opinion with it.
    async fn on_camera<T, F>(&self, requested: CameraId, work: F) -> schema::Result<(CameraInfo, T)>
    where
        F: FnOnce(OpenCamera<'_>) -> schema::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let info = self.resolve(requested).await?;
        self.on_resolved_camera(info, work).await
    }

    /// [`Wchd::on_camera`], for a caller that has already resolved.
    ///
    /// The half of [`Wchd::on_camera`] after the enumeration, split out so that "which
    /// camera is this request about" is answered **once per request** even when the handler
    /// needed the answer before it could build its closure. Two enumerations are two live
    /// reads (E2), so they are not merely twice the cost: a replug between them would leave
    /// a session's fingerprint check made against one device and the writes made to another,
    /// which is the check rubric B5 exists to make impossible.
    ///
    /// It answers with the same [`CameraInfo`] it was given, so both entry points hand a
    /// caller back the camera its work ran against and neither has to remember which.
    async fn on_resolved_camera<T, F>(
        &self,
        info: CameraInfo,
        work: F,
    ) -> schema::Result<(CameraInfo, T)>
    where
        F: FnOnce(OpenCamera<'_>) -> schema::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let at = self.0.clock.now_ms();
        let actor = self.0.cameras.actor(&info, at)?;

        let (answered, answer) = oneshot::channel::<schema::Result<T>>();
        actor.submit(at, move |device| {
            let outcome = device.and_then(work);
            // Handed back rather than sent from inside the work, so the actor has already
            // recorded that it is between commands by the time this request's answer
            // reaches the client — see `engine::actor::Answering`.
            engine::actor::answering(move || {
                // A failed send means this request was abandoned — the client hung up and
                // the receiver went with it. There is nobody left to tell, and the device
                // work is done either way.
                let _ = answered.send(outcome);
            })
        })?;

        match answer.await {
            Ok(answer) => answer.map(|value| (info, value)),
            // The actor's thread left holding the reply. `engine::actor` owns that
            // refusal, including which node it names.
            Err(_) => Err(actor.device_gone()),
        }
    }

    /// [`Wchd::on_camera`], for work that needs the daemon's state as well as the device.
    ///
    /// Six of the routed verbs are a session read-modify-write *around* a device operation
    /// — `calibrate_start`'s probe, `calibrate_plan`'s draft, the sweep, `apply`,
    /// `restore` — and [`Wchd::on_camera`]'s closure is handed only the device. Capturing an
    /// `Arc<Inner>` at each of those call sites would be six chances to capture the wrong
    /// thing; this is the one answer to "which store", in the same spirit as
    /// `Inner::sessions` being the one answer to "which lock".
    ///
    /// The store work runs on the *actor's* thread, which is where it belongs: a sweep
    /// commits a sample per value from inside the same closure that took the photo, so
    /// splitting the two would put half a read-modify-write on a pool thread and the other
    /// half on an actor. Both halves block, and the actor's thread is a blocking thread.
    async fn on_camera_with_state<T, F>(
        &self,
        requested: CameraId,
        work: F,
    ) -> schema::Result<(CameraInfo, T)>
    where
        F: FnOnce(&Inner, OpenCamera<'_>) -> schema::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let inner = Arc::clone(&self.0);
        self.on_camera(requested, move |device| work(&inner, device))
            .await
    }

    /// [`Wchd::on_camera_with_state`], for a caller that has already resolved.
    ///
    /// What the five calibrate verbs that open a camera use, for
    /// [`Wchd::on_resolved_camera`]'s reason: each needs `info.fingerprint` to build the
    /// closure that checks the session, so each already holds the answer, and handing the
    /// [`CameraId`] back to be enumerated a second time would put the check and the writes
    /// on two different readings of the machine. It is also the enumeration that would run
    /// *inside* `editing_sessions()`, charging a `/sys` walk per node to every other client
    /// waiting on the session mutex.
    async fn on_resolved_camera_with_state<T, F>(
        &self,
        info: CameraInfo,
        work: F,
    ) -> schema::Result<(CameraInfo, T)>
    where
        F: FnOnce(&Inner, OpenCamera<'_>) -> schema::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let inner = Arc::clone(&self.0);
        self.on_resolved_camera(info, move |device| work(&inner, device))
            .await
    }

    /// Take the right to change a session document, and with it D9's token.
    ///
    /// The whole of note **N47**'s fix, and the reason it is a function rather than a field
    /// access: holding the guard *is* the exclusion, so a handler that wants the lock has
    /// already taken it by the time it can name it. There is no other path to a
    /// `&StoreLock` in this crate — [`crate::state::OwnedState`] hands its copy to
    /// [`Wchd::new`] and nothing else — which is what makes "one session edit at a time" a
    /// property of the type rather than of six handlers' discipline.
    ///
    /// Awaiting this parks no thread: it is `tokio::sync::Mutex`, so a request waiting
    /// behind a minutes-long sweep is a suspended future exactly like one waiting behind a
    /// camera's actor. The two are taken in one order throughout — this first, the actor's
    /// queue second — so there is no inversion to reason about.
    async fn editing_sessions(&self) -> tokio::sync::MutexGuard<'_, Arc<StoreLock>> {
        self.0.sessions.lock().await
    }

    /// Whether anything still has `node` open, after the bounded wait
    /// [`schema::report::TerminationReport::still_held`] promises.
    ///
    /// **This is the one place this daemon waits in order to learn something, and it says so
    /// rather than dressing the wait as a timer.** `SIGTERM` is a request and `kill(2)`
    /// returns when the signal is *queued*, so a walk taken immediately after would report
    /// almost every process that is about to exit as still holding the camera — and a field
    /// that is nearly always `true` is a field nobody reads, which defeats the reason
    /// `still_held` exists at all (E4: requested is not applied, and the caller has to be
    /// able to tell "signalled, device now free" from "signalled, still held").
    ///
    /// It cannot be event-driven. A process this one did not fork leaves nothing a Unix
    /// process can wait on without `pidfd_open(2)`, which this workspace does not link, so
    /// the honest mechanism is a poll with two bounds: [`limits::TERMINATE_RECHECK_MS`] of
    /// wall time and the walks that fit inside it. It ends the moment the fact changes, so
    /// the usual cost is one interval rather than the budget.
    ///
    /// The answer is about the **node**, not about the pid, because that is what the field
    /// says and what a caller wants to know. One consequence worth expecting on real
    /// hardware: a camera this daemon has open is one this daemon holds, so `still_held`
    /// answers `true` until D12's idle close lets go of it.
    ///
    /// **`false` means "nothing this uid can see holds it", which is as far as `/proc` goes.**
    /// `webcam-handler-v4l2::holders` says so in its own header — another user's process is
    /// invisible without privilege — so an empty walk cannot tell "free" from "held by
    /// somebody we may not look at". Reporting the conservative answer instead is not
    /// available here: a walk of a node nobody holds is *also* empty, so `true` on an empty
    /// walk would make the field constant, and a field that is always `true` is the one
    /// [`limits::TERMINATE_RECHECK_MS`] exists to prevent. The refusal that does take the
    /// conservative direction is the one with a syscall behind it — a pid the walk cannot
    /// confirm is never signalled (note **N48**) — because there the answer decides an
    /// action rather than describing one.
    async fn node_still_held(&self, node: &Utf8Path) -> schema::Result<bool> {
        let walks = recheck_walks();
        for walk in 1..=walks {
            let probe = node.to_owned();
            let holders = self.offload(move |_| Ok(v4l2::holders::of(&probe))).await?;
            if holders.is_empty() {
                return Ok(false);
            }
            if walk == walks {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(
                limits::TERMINATE_RECHECK_POLL_MS,
            ))
            .await;
        }
        Ok(true)
    }
}

/// How many `/proc` walks one [`Wchd::node_still_held`] answer costs at worst.
///
/// Derived from the two constants rather than written down, so "how long the caller waits"
/// and "how much work that is" cannot disagree — and named rather than inlined because
/// [`limits::TERMINATE_RECHECK_POLL_MS`]'s doc states the number, and a stated number wants
/// something that can go red when the arithmetic moves under it (rubric B11).
///
/// One walk immediately, then one after each poll interval: the budget buys
/// `TERMINATE_RECHECK_MS / TERMINATE_RECHECK_POLL_MS` *waits*, and there is a walk on either
/// side of every one of them. The loop ends the moment the fact changes, so the usual cost
/// is one walk rather than this number.
const fn recheck_walks() -> u64 {
    limits::TERMINATE_RECHECK_MS / limits::TERMINATE_RECHECK_POLL_MS + 1
}

/// Everything about a photo's sink that can be answered before a camera is opened.
///
/// Three rules, every one of which only a *socket* can break, and all three refused here
/// rather than downstream so that a request this build was never going to honour costs
/// nobody a descriptor. That is assertable — `FakeBackend::opens()` is still zero after any
/// of the refusals — which is what keeps this an early check rather than a duplicated one.
///
/// - [`Sink::is_addressable`] is note **N34**'s orphan predicate and this is the consumer it
///   named. `cli_core::Command::photo_request` resolves a relative `-o` against the
///   *caller's* cwd before sending (D10), so neither `wch` nor `wchc` can produce a relative
///   `Sink::ServerPath`; a hand-written client can, and this daemon's working directory
///   under systemd is `/`, so `{"kind":"server_path","path":"out.jpg"}` would write
///   `/out.jpg` as the daemon's uid. The predicate lives beside the variants it constrains
///   because a paragraph is a thing an implementer has to have read.
/// - [`Sink::writable_format`] is the same shape for the other question, and it is called
///   again inside `engine::photo` where the answer is used. That is one rule asked twice
///   rather than two rules: the engine's call is the backstop every caller gets, and this
///   one is why `/tmp/x.webp` never reaches a camera.
/// - **The destination is a regular file, or does not exist yet.** The third rule, and the
///   only one here that reads the filesystem — note **N51**. `engine::photo` writes with
///   `std::fs::write`, whose `open` has no `O_NONBLOCK`: on a fifo it blocks until a reader
///   appears, and that open runs *inside the camera's actor closure*, which nothing bounds.
///   So `mkfifo /tmp/x.jpg` followed by a `wch_photo` naming it would park that camera's one
///   thread forever — the request never answers, every later request for that camera is
///   `Busy` because the actor's queue fills, and D12's idle close never fires because
///   `CameraActor::sweep` sees `busy` first, so the operator's webcam is unusable by any
///   application until `wchd` is restarted. The second shape is quieter and worse:
///   `/dev/stdout` is a regular path a client may name, and under systemd it is the journal
///   — a camera frame in the logs, which AGENTS forbids absolutely (rubric A12). Neither is
///   reachable from `wch`, which resolves `-o` on a command line somebody typed.
///
/// All three refusals are [`Error::IllegalTransition`], which note **N46** records for the
/// first two: not
/// `FormatUnsupported`, which is the camera saying what it cannot offer (E3); not
/// `StorageIo`, which would claim a filesystem was consulted; not `DeviceIo`, which would
/// blame the kernel for a request nobody could honour. The third takes the same variant for
/// the same reason — it is the request naming a destination this build will not write, not
/// the disk declining to be written — even though it is the one that does stat a path.
///
/// **Blocking**, therefore, and the reason the caller runs it on the blocking pool: a `stat`
/// is fast until the path is on a hung mount, and this daemon runs nothing that can block on
/// a runtime worker.
fn addressable(sink: &Sink) -> schema::Result<()> {
    if !sink.is_addressable() {
        // The predicate is the authority on *whether*; this only says *what*. The one way
        // a sink fails it today is a relative `ServerPath` — `ReturnBytes` carries no
        // destination, which its own suite asserts in both directions — but the second arm
        // is written rather than assumed away, because a `match` that answered a request
        // with a panic would be the one thing a device-driven path may not do.
        let destination = match sink {
            Sink::ServerPath { path } => path.to_string(),
            Sink::ReturnBytes { format } => format!("a {format} payload"),
        };
        return Err(Error::IllegalTransition {
            from: format!("unaddressable_sink({destination})"),
            op: format!(
                "write a photo to {destination}; a server path is resolved on the host \
                 running this daemon, whose working directory is / under systemd, so it \
                 has to be absolute"
            ),
        });
    }
    sink.writable_format()?;
    if let Sink::ServerPath { path } = sink {
        // `metadata` and not `symlink_metadata`, deliberately: a symlink to a regular file
        // is an ordinary destination, and it is the *target* that decides whether the write
        // blocks or lands in a log. `/dev/stdout` is refused by this reading and not by the
        // other one. A path that does not exist has no metadata and is fine — `std::fs::write`
        // creates a regular file, which is the case this rule is protecting.
        if let Ok(existing) = std::fs::metadata(path) {
            let kind = existing.file_type();
            if !kind.is_file() {
                return Err(Error::IllegalTransition {
                    from: format!("not_a_regular_file({path})"),
                    op: format!(
                        "write a photo to {path}; that path names a {} rather than a file, \
                         and this daemon writes photos only to regular files — opening a \
                         fifo would park the camera's thread until somebody read it, and a \
                         character device or /dev/stdout would put a frame somewhere frames \
                         may not go",
                        describe_file_type(&kind)
                    ),
                });
            }
        }
    }
    Ok(())
}

/// What a destination is, for a refusal that has to say why it is not a file.
///
/// A `match` with a payload-carrying fallback (AGENTS rule 6): `FileType` grows variants
/// as platforms do, and a refusal that panicked on an unfamiliar one would be the one thing
/// a request-driven path may not do.
fn describe_file_type(kind: &std::fs::FileType) -> &'static str {
    use std::os::unix::fs::FileTypeExt;
    if kind.is_dir() {
        "directory"
    } else if kind.is_fifo() {
        "fifo"
    } else if kind.is_socket() {
        "socket"
    } else if kind.is_char_device() {
        "character device"
    } else if kind.is_block_device() {
        "block device"
    } else if kind.is_symlink() {
        "symbolic link"
    } else {
        "thing that is not a regular file"
    }
}

/// The photo answer, checked against itself before it is sent.
///
/// [`api::PhotoResponse::bytes_match_the_delivery`] is note **N34**'s other orphan predicate
/// and this is the consumer it named: "the daemon, which refuses a `PhotoResponse` it is
/// about to send that disagrees with itself (P4c)". `engine::photo::from_capture` produces
/// the two halves together — `returned` is `Some` exactly when the delivery is `Bytes` —
/// so a self-consistent answer is the ordinary case and this is the assertion that it
/// stays one. A truncated payload with an intact `byte_count` is what a client cannot
/// distinguish from a whole photo, and after P4f `wchc` makes the same check from the other
/// end, which is what makes the pair worth two call sites.
///
/// The refusal is [`Error::DeviceIo`] because the failure is **ours**: this process assembled
/// an answer that disagrees with itself, and spelling that like a camera refusal would be
/// the availability-versus-capability conversion E3 forbids. It is the same variant
/// [`Wchd::offload`] answers for a panicked pool thread, for the same reason. The message
/// carries the two counts and nothing else — never the bytes (rubric A12).
fn photo_response(taken: engine::photo::Photograph) -> schema::Result<api::PhotoResponse> {
    let response = api::PhotoResponse {
        bytes: taken.returned.map(api::Base64Bytes::new),
        report: taken.report,
    };
    if !response.bytes_match_the_delivery() {
        return Err(Error::DeviceIo {
            operation: "assemble a photo answer".to_owned(),
            errno: None,
            message: format!(
                "the delivery reports {} bytes and the payload carries {}",
                response.report.delivery.byte_count(),
                response
                    .bytes
                    .as_ref()
                    .map_or_else(|| "none".to_owned(), |bytes| bytes.len().to_string()),
            ),
        });
    }
    Ok(response)
}

/// The node a camera's holders would be holding.
///
/// `terminate_holder`'s first question, and the only one whose answer can be "there is
/// nothing here": a camera enumeration can describe a device with no
/// [`schema::camera::NodeKind::VideoCapture`] node at all, and nothing can hold a node that
/// does not exist. That is [`Error::HolderGone`] rather than an invented path — the
/// alternative, `engine::actor::CameraActor::spawn`'s fallback of naming the camera id, is
/// right for a *refusal* that has to name something and wrong here, because this path is
/// about to compare the answer against `/proc/<pid>/fd/*` symlinks and a camera id matches
/// none of them.
fn holder_node(info: &CameraInfo, pid: i32) -> schema::Result<Utf8PathBuf> {
    info.capture_node()
        .map(|node| node.path.clone())
        .ok_or(Error::HolderGone { pid })
}

/// Refuse a request that names this daemon.
///
/// A camera this daemon has open is a camera this daemon *holds*, so its own pid is in the
/// walk — and on a real device that is the ordinary case, because the actor keeps the
/// descriptor between commands (D12). A client that named it would be asking the daemon to
/// terminate itself through the socket it is talking to, which is a denial of service with
/// an authentication story rather than a kill somebody meant.
///
/// The other end of this sentence is already in the engine: `engine::actor` leaves the
/// `holders` list of its own [`Error::Busy`] refusals **empty**, because "naming this
/// process's pid would invite a client to kill the daemon it is talking to". That keeps the
/// pid out of the answer a client would read it from; this keeps it out of the request.
///
/// [`Error::IllegalTransition`] rather than [`Error::HolderGone`], which would be a lie —
/// this process does hold the node — and rather than [`Error::PermissionDenied`], which is
/// the kernel's answer about a uid and would send an operator looking for a privilege
/// problem. It is the same family note **N46** widened the variant to carry: the request
/// names something this build will not do.
fn not_this_daemon(pid: i32) -> schema::Result<()> {
    // Linux caps `pid_max` at 2^22, so the conversion cannot fail on a host this daemon
    // runs on; `0` is the fallback because it is a pid the layers below already refuse
    // (`kill(2)` reads it as a process group), so a host that somehow broke the assumption
    // would refuse rather than compare against a number it guessed.
    let me = i32::try_from(std::process::id()).unwrap_or(0);
    if pid == me {
        return Err(Error::IllegalTransition {
            from: format!("this_daemon({pid})"),
            op: "signal a camera's holder; that pid is this daemon, which holds the node \
                 because a request asked it to — stop `wchd` itself, or wait for the idle \
                 close, rather than asking it to terminate itself"
                .to_owned(),
        });
    }
    Ok(())
}

#[async_trait]
impl WchRpcServer for Wchd {
    async fn list(&self) -> Result<CameraList, WireError> {
        // The assembly is `engine::resolve::list`'s, reached through the registry — the
        // same function `wch list` reaches through T4's executor, because D1's "an empty
        // enumeration is diagnosed" is a rule and a rule copied to a second composition
        // root is where the two surfaces P4f's parity gate compares start to differ.
        Ok(self.offload(|inner| inner.cameras.list()).await?)
    }

    async fn info(&self, camera: CameraId) -> Result<CameraDetail, WireError> {
        let (info, formats) = self.on_camera(camera, |device| device.formats()).await?;
        Ok(CameraDetail { formats, info })
    }

    async fn controls(&self, camera: CameraId) -> Result<ControlReport, WireError> {
        let (info, controls) = self.on_camera(camera, |device| device.controls()).await?;
        Ok(ControlReport {
            // Nothing measured: T5's `controls` is read-only, and the probe that would fill
            // this is `wch_discover_pairs`, which writes to the camera and lands with the
            // mutating half (note N30).
            pairs: engine::pairing::in_effect(&controls, Vec::new()),
            camera: info.id,
            controls,
        })
    }

    async fn get(&self, camera: CameraId, control: ControlSlug) -> Result<ControlDesc, WireError> {
        let (_, controls) = self.on_camera(camera, |device| device.controls()).await?;
        Ok(engine::pairing::describe(&controls, &control)?)
    }

    async fn calibrate_status(
        &self,
        camera: CameraId,
        session: SessionRef,
    ) -> Result<SessionStatus, WireError> {
        // No lock, under either of D9's protocols. A `with_lock` here would not hang —
        // `flock` denies a second open file description in the same process and
        // `StoreLock::take` never blocks — it would answer this client `StoreLocked`,
        // naming this daemon's own pid and advising it to use `wchc` against the daemon it
        // is already talking to. `crate::state` is where that trap is written down.
        let info = self.resolve(camera).await?;
        Ok(self
            .offload(move |inner| {
                engine::lifecycle::status(&inner.store, &info.fingerprint, &session)
            })
            .await?)
    }

    async fn calibrate_list(&self, camera: Option<CameraId>) -> Result<SessionList, WireError> {
        // `None` means every session on this machine, so there is nothing to resolve and no
        // enumeration to do — a `calibrate list` still answers on a host whose cameras have
        // all been unplugged, which is the point of a listing that parses nothing (D9).
        let fingerprint = match camera {
            Some(requested) => Some(self.resolve(requested).await?.fingerprint),
            None => None,
        };
        Ok(self
            .offload(move |inner| engine::lifecycle::list(&inner.store, fingerprint.as_ref()))
            .await?)
    }

    // ------------------------------------------------- P4c's control-shaped mutating half
    //
    // Five verbs that write to a camera and nothing else. Each is one engine call — the
    // one `crates/cli`'s executor makes — on that camera's actor thread (D12), because the
    // device is what they need and the device is what the actor owns. None of them touches
    // D9's session tree, which is why this step needs no lock token (see this module's
    // header).

    async fn discover_pairs(&self, camera: CameraId) -> Result<DiscoveryReport, WireError> {
        // The whole document, assembled in the engine: probe (which writes), then read the
        // control set the camera is *now* in, then merge declared with measured so measured
        // wins (E1). Note N34 booked that move against this sub-milestone, and the reason
        // it is not spelled out here is that spelling it out here is the defect.
        let now = schema::time::Stamp::now();
        Ok(self
            .on_camera(camera, move |device| engine::discover::report(device, now))
            .await?
            .1)
    }

    async fn set(
        &self,
        camera: CameraId,
        writes: Vec<ControlWrite>,
        guarded: bool,
    ) -> Result<WriteReport, WireError> {
        // The whole verb is one engine call, and the composition inside it — which pair set
        // this write plans against, and where the wire's `ControlWrite` stops — is the
        // engine's too. Written out here it would be a second author for a rule that decides
        // whether an automation control is switched off first (note **N35**, AGENTS "One
        // home per law"). A driver that clamped is a warning on the report and never an
        // error (E4, \[PF:6\]); the report crosses the wire carrying every
        // `{requested, applied}` pair the plan produced.
        Ok(self
            .on_camera(camera, move |device| {
                engine::write::set_requested(device, &writes, guarded)
            })
            .await?
            .1)
    }

    async fn snapshot(&self, camera: CameraId) -> Result<Snapshot, WireError> {
        // `now` enters at the composition root because the engine reads no clock; the pair
        // set does not, for `engine::write::set_requested`'s reason.
        let now = schema::time::Stamp::now();
        Ok(self
            .on_camera(camera, move |device| {
                engine::snapshot::take_in_effect(device, now)
            })
            .await?
            .1)
    }

    async fn restore(
        &self,
        camera: CameraId,
        snapshot: Snapshot,
    ) -> Result<RestoreReport, WireError> {
        // The pair set is this device's, read now — the snapshot arrived over a socket and a
        // caller's document does not get to say what this camera's automation looks like.
        // That sentence is `engine::snapshot::restore_in_effect`'s, where both roots read it,
        // rather than a paragraph each of them keeps its own copy of. A snapshot from another
        // camera is refused by fingerprint before any of it matters, and a control that could
        // not be put back is an outcome in the report rather than a refusal — including
        // `OwnedByAutomation`, which is the ordinary success (note N9).
        Ok(self
            .on_camera(camera, move |device| {
                engine::snapshot::restore_in_effect(device, &snapshot)
            })
            .await?
            .1)
    }

    async fn profile_capture(
        &self,
        camera: CameraId,
        capturer: String,
    ) -> Result<DeviceProfile, WireError> {
        // The provenance is assembled off the runtime, because reading it blocks:
        // `kernel_release` reads a pseudo-file, and work that blocks and needs no camera
        // goes to the blocking pool. Both host facts have exactly one home —
        // `engine::profile::kernel_release` moved beside the field it fills when this verb
        // acquired a second author, and `schema::TOOL_VERSION` is the one reading of "which
        // build wrote this" — because `wch profile capture` writes the same document into
        // the same corpus.
        let context = self
            .offload(move |inner| {
                Ok(engine::profile::CaptureContext {
                    captured_at: schema::time::Stamp::now(),
                    kernel: engine::profile::kernel_release(),
                    tool_version: schema::TOOL_VERSION.to_owned(),
                    capturer,
                    // Asked, not assumed: "a profile captured from the fake backend would
                    // be circular corpus" is the whole point of the field.
                    backend: inner.cameras.backend_kind(),
                })
            })
            .await?;
        Ok(self
            .on_camera(camera, move |device| {
                engine::profile::capture(device, &context)
            })
            .await?
            .1)
    }

    async fn photo(
        &self,
        camera: CameraId,
        request: PhotoRequest,
    ) -> Result<api::PhotoResponse, WireError> {
        // The sink is answered for **before** anything is resolved or opened: every one of
        // the things that can be wrong with one is wrong about the request rather than about
        // the device, and a camera opened for a request that was always going to be refused
        // is a descriptor taken from whoever is using it (see `addressable`). On the
        // blocking pool because the last of the three rules stats a path.
        {
            let sink = request.sink.clone();
            self.offload(move |_| addressable(&sink)).await?;
        }
        let now = schema::time::Stamp::now();
        let (_, taken) = self
            .on_camera_with_state(camera, move |inner, device| {
                // Two clocks, because they measure different things and conflating them is
                // how an NTP step becomes a settle failure: `now` is the wall time that
                // goes in the EXIF, and `inner.clock` is the monotonic one the settle
                // policy runs on — this daemon's one reading of "what time is it", the
                // field `Inner::clock` exists to be.
                engine::photo::take(device, &request, &inner.clock, now)
            })
            .await?;
        // Nothing here logs. The only facts this verb has that the answer does not already
        // carry are a path and a byte count, and a `tracing` call on the one code path in
        // this process that holds a frame is exactly the line `crate::logging` exists to
        // keep unwritten — "no frame, no photo payload, and nothing derived from one is
        // ever a field on an event".
        Ok(photo_response(taken)?)
    }

    // ------------------------------------------------------------ the most dangerous verb
    //
    // Everything about `terminate_holder` is written where it happens rather than gathered
    // here, because a reader who wants to know why a signal is sent should not have to hold
    // a paragraph from the top of the file in their head.

    async fn terminate_holder(
        &self,
        camera: CameraId,
        pid: i32,
    ) -> Result<TerminationReport, WireError> {
        // Nothing here touches the camera, and that is a decision rather than an omission:
        // the process being signalled is not this daemon, the camera may not be open here
        // at all, and queueing a kill behind a minutes-long sweep *on the very camera the
        // caller is trying to free* would be the exact inverse of what the verb is for. So
        // it resolves the camera to learn which node to ask about, and everything after
        // that is `/proc` — blocking, camera-free, and therefore `offload`'s.
        let info = self.resolve(camera).await?;
        let node = holder_node(&info, pid)?;

        // 1. Diagnose, about the pid the caller named. This is the *same* `/proc` module a
        //    `Busy` refusal names its holders with — a second one would be a second answer
        //    to "who has this node" — reached through the backend crate that owns it (note
        //    N48). It asks `holders::holder` rather than looking the pid up in
        //    `holders::of`'s answer, and which of the two gates the signal is the difference
        //    between a usable verb and one that refuses a browser's fifth process: `of` is
        //    bounded by `limits::MAX_HOLDERS_REPORTED` so that a refusal stays readable, and
        //    a pid past that bound in `/proc`'s arbitrary order really does hold the node.
        //
        //    **An absence is still a refusal**, and that direction is deliberate: the walk
        //    sees only processes this uid may look at, so a holder can be invisible. The
        //    verb declines to signal a pid it could not confirm, which occasionally refuses
        //    a kill somebody was entitled to — the correct direction, and what note N48
        //    records. The `Holder` it produces is what the report carries, because the
        //    answer names the target the request named.
        let holder = {
            let node = node.clone();
            self.offload(move |_| {
                v4l2::holders::holder(pid, &node).ok_or(Error::HolderGone { pid })
            })
            .await?
        };
        // 2. And never this daemon, whatever the walk said.
        not_this_daemon(pid)?;

        // 3. Re-verify, then signal, with nothing between them. Both statements are inside
        //    one blocking closure on purpose: the window this narrows is the pid-reuse race
        //    — the holder exits, the kernel reaps it, and the number is handed to somebody
        //    else's program — and every `await` between the check and the syscall is that
        //    window made wider. It cannot be *closed* from user space without
        //    `pidfd_open(2)`; what bounds it is a few instructions instead of a whole
        //    request, `SIGTERM` rather than `SIGKILL`, and the fact that the caller named
        //    the number. Note **N48** says all four out loud rather than claiming the race
        //    away.
        {
            let node = node.clone();
            self.offload(move |_| {
                if !v4l2::holders::holds(pid, &node) {
                    return Err(Error::HolderGone { pid });
                }
                v4l2::holders::terminate(pid)
            })
            .await?;
        }

        // A process this daemon signalled on somebody's behalf is exactly the class of
        // event an operator has to be able to find afterwards, and every field here is a
        // path, a number or a command name — never a frame (see `crate::logging`).
        tracing::info!(
            camera = %info.id,
            node = %node,
            pid,
            comm = holder.comm.as_deref().unwrap_or("(unknown)"),
            "signalled the process holding a camera, as asked"
        );

        Ok(TerminationReport {
            camera: info.id,
            holder,
            signal: TerminationSignal::Term,
            still_held: self.node_still_held(&node).await?,
        })
    }

    // ------------------------------------------------------------- P4c's calibrate half
    //
    // Six verbs that change a session document, and every one of them takes
    // `editing_sessions()` first: that guard is D9's token *and* the serialization `flock`
    // cannot give a single process (note N47). The read of the document happens under it
    // too — `lifecycle::session_to_update` demands the token for exactly that reason — so
    // the whole read-modify-write is inside one hold rather than only the write half.
    //
    // Two of them open no camera, and which two is load-bearing rather than an
    // optimisation; `crates/cli`'s executor says why at the same branch.

    async fn calibrate_start(
        &self,
        camera: CameraId,
        task: String,
        goal: String,
        criteria: Vec<String>,
    ) -> Result<Session, WireError> {
        // One enumeration, and the value it produced is what opens the camera below: the
        // fingerprint this session is recorded against and the device it is probed on are
        // then the same reading of the machine rather than two (see
        // `Wchd::on_resolved_camera`).
        let info = self.resolve(camera).await?;
        let editing = self.editing_sessions().await;
        let lock = Arc::clone(&editing);
        // The clock and the id enter at the composition root, because the engine reads
        // neither — and a UUIDv7 *is* a timestamp, which is what makes a session directory
        // sort chronologically without anything parsing a document (D9).
        let spec = engine::lifecycle::SessionSpec {
            id: uuid::Uuid::now_v7(),
            fingerprint: info.fingerprint.clone(),
            task,
            goal,
            criteria,
            // The schema crate's reading of one fact, not this binary's: `wch calibrate
            // start` records provenance into the same documents.
            tool_version: schema::TOOL_VERSION.to_owned(),
        };
        Ok(self
            .on_resolved_camera_with_state(info, move |inner, device| {
                let now = schema::time::Stamp::now();
                let mut session = engine::lifecycle::create(&inner.store, &lock, &spec, now)?;
                // D3's empirical probe, at session start and nowhere else (N16). It
                // *writes* to the camera and puts it back, which is why this verb needs the
                // device at all — and why the `Discovery` it answers is dropped here: T5's
                // `calibrate_start` answers a `Session`, and `wch` prints the probe's other
                // two facts on stderr, which a socket client cannot see. `wch_discover_pairs`
                // is the verb that hands them over (note N30).
                engine::lifecycle::discover_pairs(
                    &inner.store,
                    &lock,
                    &mut session,
                    device,
                    schema::time::Stamp::now(),
                )?;
                Ok(session)
            })
            .await?
            .1)
    }

    async fn calibrate_plan(
        &self,
        camera: CameraId,
        session: SessionRef,
        controls: Vec<ControlSlug>,
        order: bool,
    ) -> Result<Session, WireError> {
        // Resolved once, for `Wchd::on_resolved_camera`'s reason — and the `--order` branch
        // below needs the fingerprint without opening anything at all, so this is also the
        // only reading either branch takes.
        let info = self.resolve(camera).await?;
        let editing = self.editing_sessions().await;
        let lock = Arc::clone(&editing);
        let now = schema::time::Stamp::now();

        let fingerprint = info.fingerprint.clone();
        if order {
            // The camera is deliberately not opened: reordering a queue is an edit to a
            // document, and a caller who wanted to put exposure before focus should not be
            // refused because something else currently holds the device. Routing this
            // through the actor would compile, would pass every test that did not hold a
            // camera, and would break parity with `wch` the day somebody reordered a queue
            // during a sweep.
            return Ok(self
                .offload(move |inner| {
                    let mut session = engine::lifecycle::session_to_update(
                        &inner.store,
                        &lock,
                        &fingerprint,
                        &session,
                    )?;
                    engine::lifecycle::commit_state(
                        &inner.store,
                        &lock,
                        &mut session,
                        now,
                        |draft, now| engine::session::reorder_queue(draft, &controls, now),
                    )?;
                    Ok(session)
                })
                .await?);
        }

        // Drafting asks the *device* what it has and what it will not let this tool
        // calibrate, so this is where the camera has to open.
        Ok(self
            .on_resolved_camera_with_state(info, move |inner, device| {
                let mut session = engine::lifecycle::session_to_update(
                    &inner.store,
                    &lock,
                    &fingerprint,
                    &session,
                )?;
                engine::lifecycle::draft(
                    &inner.store,
                    &lock,
                    &mut session,
                    device,
                    &controls,
                    now,
                )?;
                Ok(session)
            })
            .await?
            .1)
    }

    async fn calibrate_sweep(
        &self,
        camera: CameraId,
        session: SessionRef,
        request: SweepRequest,
    ) -> Result<Session, WireError> {
        let info = self.resolve(camera).await?;
        let fingerprint = info.fingerprint.clone();
        let editing = self.editing_sessions().await;
        let lock = Arc::clone(&editing);
        Ok(self
            .on_resolved_camera_with_state(info, move |inner, device| {
                let mut session = engine::lifecycle::session_to_update(
                    &inner.store,
                    &lock,
                    &fingerprint,
                    &session,
                )?;
                let context = engine::calibrate::SweepContext {
                    store: &inner.store,
                    lock: &lock,
                    // Constructed here rather than borrowed from `Inner`: the settle policy
                    // runs on monotonic time, `started_at` is the wall reading each sample's
                    // `captured_at` is offset from, and conflating them is how an NTP step
                    // in the middle of a twenty-minute pan sweep makes sample 40 older than
                    // sample 39.
                    clock: &MonotonicClock::new(),
                    // **Nothing is listening, and that is the documented answer.** The live
                    // events are `schema::progress::ProgressEvent`s and P4e puts them on
                    // their own subscription rather than threading a watcher through a
                    // call; the T5 method's own doc says a client's progress bar simply does
                    // not move until then. `Silent` is a real sink rather than an `Option`,
                    // so the sweep emits into it and the events are dropped where a
                    // subscription will later be attached — the seam is already the shape
                    // P4e needs, which is what docs/7's risk register asked P3c to
                    // guarantee. The honest consequence, which the parity gate does not
                    // cover: `wch calibrate sweep` renders indicatif from these same events
                    // and `wchc calibrate sweep` shows nothing at all until P4e.
                    progress: &engine::progress::Silent,
                    started_at: schema::time::Stamp::now(),
                };
                engine::calibrate::run(&context, &mut session, device, &request)?;
                Ok(session)
            })
            .await?
            .1)
    }

    async fn calibrate_select(
        &self,
        camera: CameraId,
        session: SessionRef,
        control: ControlSlug,
        selection: Selection,
    ) -> Result<Session, WireError> {
        let info = self.resolve(camera).await?;
        let editing = self.editing_sessions().await;
        let lock = Arc::clone(&editing);
        let now = schema::time::Stamp::now();
        // No camera: recording which value somebody chose is an edit to a document, and the
        // values it chooses between were photographed during the sweep.
        Ok(self
            .offload(move |inner| {
                let mut session = engine::lifecycle::session_to_update(
                    &inner.store,
                    &lock,
                    &info.fingerprint,
                    &session,
                )?;
                engine::lifecycle::select(
                    &inner.store,
                    &lock,
                    &mut session,
                    &control,
                    &selection,
                    now,
                )?;
                Ok(session)
            })
            .await?)
    }

    async fn calibrate_apply(
        &self,
        camera: CameraId,
        session: SessionRef,
        partial: bool,
    ) -> Result<WriteReport, WireError> {
        let info = self.resolve(camera).await?;
        let fingerprint = info.fingerprint.clone();
        let editing = self.editing_sessions().await;
        let lock = Arc::clone(&editing);
        let now = schema::time::Stamp::now();
        Ok(self
            .on_resolved_camera_with_state(info, move |inner, device| {
                let mut session = engine::lifecycle::session_to_update(
                    &inner.store,
                    &lock,
                    &fingerprint,
                    &session,
                )?;
                // Deliberately no restore afterwards, and deliberately without consuming
                // the pre-sweep snapshot (N20): applying a calibration is the point of
                // having made one, and reading AGENTS rule 8 as "every write restores"
                // would make the calibration unusable by the tool that produced it. Putting
                // the camera back is `wch_calibrate_restore`, which is its own verb (N23).
                engine::lifecycle::apply(&inner.store, &lock, &mut session, device, partial, now)
            })
            .await?
            .1)
    }

    async fn calibrate_restore(
        &self,
        camera: CameraId,
        session: SessionRef,
    ) -> Result<RestoreReport, WireError> {
        let info = self.resolve(camera).await?;
        let fingerprint = info.fingerprint.clone();
        let editing = self.editing_sessions().await;
        let lock = Arc::clone(&editing);
        let now = schema::time::Stamp::now();
        Ok(self
            .on_resolved_camera_with_state(info, move |inner, device| {
                let mut session = engine::lifecycle::session_to_update(
                    &inner.store,
                    &lock,
                    &fingerprint,
                    &session,
                )?;
                // Which pair set a restore plans against, and what "no snapshot" means, are
                // both `engine::lifecycle::restore`'s — running this twice is not an error,
                // and the second run's empty report is the honest shape for "nothing was
                // left to put back".
                engine::lifecycle::restore(&inner.store, &lock, &mut session, device, now)
            })
            .await?
            .1)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use engine::store::{LockProtocol, TempStore};
    use fake::FakeBackend;
    use schema::ErrorKind;
    use schema::error::Error;

    use super::*;

    /// D9's lifetime lock over a throw-away state directory.
    ///
    /// The arrangement `crate::state::OwnedState` makes in the shipped daemon, without the
    /// environment: one token, taken once, shared with the `Wchd` that will pass it to every
    /// mutating store call. Taking a *second* here would fail the same way it fails in
    /// production, which is the point of there being only one.
    fn lifetime_lock(temp: &TempStore) -> Arc<StoreLock> {
        Arc::new(
            SessionStore::new(temp.root())
                .lock(LockProtocol::HeldForLifetime)
                .expect("nothing else holds a throw-away state directory"),
        )
    }

    /// A daemon over one replayed camera and a throw-away state directory.
    fn daemon(idle_after_ms: Millis) -> (Arc<FakeBackend>, TempStore, Wchd) {
        let backend = Arc::new(
            FakeBackend::from_profile(testkit::fixtures::synthetic_basic())
                .expect("the synthetic profile is this build's version"),
        );
        let temp = TempStore::new().expect("a state directory");
        let wchd = Wchd::with_idle_timeout(
            Arc::clone(&backend) as Arc<dyn CameraBackend>,
            SessionStore::new(temp.root()),
            lifetime_lock(&temp),
            idle_after_ms,
        );
        (backend, temp, wchd)
    }

    /// A backend that enumerates nothing and has something to say about it.
    ///
    /// Not the fake, and not a gap in the fake: `schema::report::HintKind`'s two variants
    /// are V4L2 findings — a USB device with no driver bound \[PF:14\], a node that could
    /// not be read — and a backend replaying a document knows neither. A `FakeBackend` that
    /// produced one would be claiming a capability no replayed profile has, which AGENTS
    /// calls a bug in the fake.
    ///
    /// What this double exists for is the *assembly*: D1 says an empty enumeration is
    /// diagnosed rather than shrugged at, and a `wch_list` that dropped the hints would
    /// answer "no cameras" to an operator whose camera is plugged in with nothing driving
    /// it. Fixed values, in `engine::profile`'s `StubCamera` tradition — the subject is what
    /// the daemon does with an answer, not how a backend arrives at one.
    #[derive(Debug)]
    struct Diagnosing;

    /// The one finding this double reports.
    fn the_hint() -> schema::report::ListHint {
        schema::report::ListHint {
            kind: schema::report::HintKind::DriverlessUsbVideoDevice,
            subject: "1-2".to_owned(),
        }
    }

    impl CameraBackend for Diagnosing {
        fn kind(&self) -> schema::backend::BackendKind {
            // Honest about being an instrument: nothing here drives hardware.
            schema::backend::BackendKind::Fake
        }

        fn enumerate(&self) -> schema::Result<Vec<CameraInfo>> {
            Ok(Vec::new())
        }

        fn open(&self, id: &CameraId) -> schema::Result<Box<dyn schema::backend::Camera>> {
            Err(Error::CameraUnknown {
                requested: id.to_string(),
            })
        }

        fn watch(&self) -> schema::Result<Box<dyn schema::backend::HotplugWatch>> {
            Err(Error::DeviceIo {
                operation: "watch a backend with no devices".to_owned(),
                errno: None,
                message: "this double enumerates nothing, so nothing can be added or removed"
                    .to_owned(),
            })
        }

        fn diagnose(&self) -> Vec<schema::report::ListHint> {
            vec![the_hint()]
        }
    }

    /// A backend that says, out loud and once, when a camera's descriptor goes away.
    ///
    /// `FakeBackend::closes()` is a counter, and a counter is something a test can *read*
    /// but not *wait for*. The claim here needs waiting: the thing under test is a task
    /// nobody joins, running a pass on a blocking-pool thread, so "the driver closed the
    /// camera" arrives at an instant this test does not choose. A channel makes that
    /// instant an event — the test blocks on a `recv` that ends when the descriptor is
    /// dropped, which is a signal from the subject rather than a guess about how long it
    /// takes, and is why nothing here sleeps or polls.
    ///
    /// A decorator rather than a counter on the fake, because it is this test's
    /// synchronisation and not a capability any device has: a `FakeBackend` that
    /// announced its own closes would be claiming something no replayed profile does,
    /// which AGENTS calls a bug in the fake.
    #[derive(Debug)]
    struct Announcing {
        inner: Arc<FakeBackend>,
        closed: std::sync::Mutex<std::sync::mpsc::Sender<CameraId>>,
    }

    /// One open camera, forwarding everything and announcing its own end.
    #[derive(Debug)]
    struct Watched {
        /// An `Option` so that [`Drop`] can release the real handle *before* it
        /// announces: a decorator's fields drop after its `Drop` body, so announcing
        /// first would tell a test that the descriptor had gone away while the thing that
        /// counts descriptors had not yet seen it go.
        camera: Option<Box<dyn schema::backend::Camera>>,
        id: CameraId,
        closed: std::sync::mpsc::Sender<CameraId>,
    }

    impl Watched {
        fn inner(&self) -> &dyn schema::backend::Camera {
            self.camera.as_deref().expect("only Drop takes the handle")
        }

        fn inner_mut(&mut self) -> &mut (dyn schema::backend::Camera + 'static) {
            self.camera
                .as_deref_mut()
                .expect("only Drop takes the handle")
        }
    }

    impl Drop for Watched {
        fn drop(&mut self) {
            drop(self.camera.take());
            // The failed send is the test having stopped listening, which is not this
            // double's business.
            let _ = self.closed.send(self.id.clone());
        }
    }

    impl schema::backend::Camera for Watched {
        fn info(&self) -> &CameraInfo {
            self.inner().info()
        }

        fn formats(&self) -> schema::Result<Vec<schema::camera::FormatInfo>> {
            self.inner().formats()
        }

        fn controls(&self) -> schema::Result<Vec<ControlDesc>> {
            self.inner().controls()
        }

        fn get(
            &mut self,
            id: schema::control::ControlId,
        ) -> schema::Result<schema::control::ControlValue> {
            self.inner_mut().get(id)
        }

        fn set(
            &mut self,
            id: schema::control::ControlId,
            value: schema::control::ControlValue,
        ) -> schema::Result<schema::control::Applied> {
            self.inner_mut().set(id, value)
        }

        fn start_stream(
            &mut self,
            request: &schema::capture::StreamRequest,
        ) -> schema::Result<schema::capture::NegotiatedStream> {
            self.inner_mut().start_stream(request)
        }

        fn next_frame(
            &mut self,
            deadline: std::time::Instant,
        ) -> schema::Result<schema::capture::Frame> {
            self.inner_mut().next_frame(deadline)
        }

        fn stop_stream(&mut self) -> schema::Result<()> {
            self.inner_mut().stop_stream()
        }
    }

    impl CameraBackend for Announcing {
        fn kind(&self) -> schema::backend::BackendKind {
            self.inner.kind()
        }

        fn enumerate(&self) -> schema::Result<Vec<CameraInfo>> {
            self.inner.enumerate()
        }

        fn open(&self, id: &CameraId) -> schema::Result<Box<dyn schema::backend::Camera>> {
            let camera = self.inner.open(id)?;
            Ok(Box::new(Watched {
                camera: Some(camera),
                id: id.clone(),
                closed: lock(&self.closed).clone(),
            }))
        }

        fn watch(&self) -> schema::Result<Box<dyn schema::backend::HotplugWatch>> {
            self.inner.watch()
        }

        fn diagnose(&self) -> Vec<schema::report::ListHint> {
            self.inner.diagnose()
        }
    }

    /// A poisoned lock here means a test thread panicked holding a channel sender, which
    /// is not a reason to replace a useful failure with a confusing one.
    fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        mutex
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A daemon whose closes arrive on a channel.
    fn announcing_daemon(
        idle_after_ms: Millis,
    ) -> (
        Arc<FakeBackend>,
        std::sync::mpsc::Receiver<CameraId>,
        TempStore,
        Wchd,
    ) {
        let inner = Arc::new(
            FakeBackend::from_profile(testkit::fixtures::synthetic_basic())
                .expect("the synthetic profile is this build's version"),
        );
        let (closed, closes) = std::sync::mpsc::channel();
        let backend = Arc::new(Announcing {
            inner: Arc::clone(&inner),
            closed: std::sync::Mutex::new(closed),
        });
        let temp = TempStore::new().expect("a state directory");
        let wchd = Wchd::with_idle_timeout(
            backend as Arc<dyn CameraBackend>,
            SessionStore::new(temp.root()),
            lifetime_lock(&temp),
            idle_after_ms,
        );
        (inner, closes, temp, wchd)
    }

    /// The camera the fixture replays.
    fn only_camera(backend: &FakeBackend) -> CameraId {
        backend
            .enumerate()
            .expect("the fake enumerates what it replays")
            .first()
            .map(|info| info.id.clone())
            .expect("one profile is one camera")
    }

    #[test]
    fn the_server_value_is_what_jsonrpsee_requires_of_it() {
        // `WchRpcServer: Sized + Send + Sync + 'static`, and `into_rpc` puts the value in
        // an `Arc` a connection task reads from. A field that stopped being `Sync` would
        // otherwise fail to compile inside the generated registration, where the error
        // names jsonrpsee's macro rather than the field that caused it.
        fn requires<T: Send + Sync + 'static>() {}
        requires::<Wchd>();
    }

    #[test]
    fn the_pinned_routing_is_the_whole_wire_surface_and_nothing_answers_unimplemented() {
        // Note **N43**'s retirement, as an assertion rather than as a deletion. While a
        // method could be unrouted, the claim was a *partition*: `ROUTED` plus
        // `api::METHODS` minus `ROUTED` covered the surface and overlapped nowhere, so a
        // twentieth method could not fall into neither. P4c empties the second half, so the
        // claim that is left is the equality — and it is the one a client actually depends
        // on, because it says every method on this surface answers.
        let routed: BTreeSet<&str> = ROUTED.iter().copied().collect();
        assert_eq!(routed.len(), ROUTED.len(), "a wire name is pinned twice");

        let surface: BTreeSet<&str> = api::METHODS.iter().map(|method| method.name).collect();
        assert_eq!(
            routed, surface,
            "the pin and the wire surface disagree: a method was added to T5 and not routed, \
             or a name here is not a T5 method"
        );
        // Not vacuous, and pinned at the number `crates/api` pins the trait at: two empty
        // sets compare equal and would say nothing (note N29 is why nineteen is the number
        // and why the two subscriptions are not in it).
        assert_eq!(routed.len(), 19, "{routed:?}");

        // And the one that reads like a read verb but is not: measuring pairs writes to the
        // camera, so it landed with the mutating half rather than with P4b's six (N30).
        assert!(routed.contains("wch_discover_pairs"), "{routed:?}");

        // This assertion is half of a pair, and the other half is in
        // `tests/method_surface.rs`. docs/9's method-count walk proves every registered
        // method is *driven and answers*; it cannot tell an answer from a refusal that
        // arrived promptly, so a build where all nineteen answered `Unimplemented` would
        // pass it. What this test proves is that there is no such half of the surface at
        // all. Neither is worth much without the other, which is why each says so.
    }

    #[test]
    fn the_registration_the_method_count_walk_reads_is_the_one_this_module_pins() {
        // The join between the two halves above, in process: `tests/method_surface.rs`
        // compares its recording against `method_names()` off a real `Wchd`'s `into_rpc()`,
        // and this is the assertion that the value it reads is the surface `ROUTED` pins
        // rather than some other module. Without it, a second registration path — the thing
        // D10 exists to prevent — would leave both suites green about different populations.
        let (_backend, _temp, wchd) = daemon(limits::CAMERA_IDLE_CLOSE_MS);
        let registered: BTreeSet<&str> = wchd.into_rpc().method_names().collect();
        let routed: BTreeSet<&str> = ROUTED.iter().copied().collect();
        assert_eq!(
            registered, routed,
            "the registration is not the pinned surface"
        );
    }

    #[test]
    fn a_camera_with_no_capture_node_has_no_holder_to_signal() {
        // The one answer `terminate_holder` can give before `/proc` is read at all. A
        // camera can enumerate with no capture node (PF:19's topology is the reason
        // `capture_node` is an `Option` at all), and nothing can hold a node that does not
        // exist — so this is `HolderGone` rather than a path invented to compare against
        // `/proc/<pid>/fd/*` symlinks that would match none of them.
        let mut info = testkit::fixtures::synthetic_basic().invariant.info;
        let node = holder_node(&info, 7).expect("the fixture has a capture node");
        assert!(node.as_str().starts_with("/dev/"), "{node}");

        info.nodes.clear();
        let refused = holder_node(&info, 7).expect_err("nothing to hold");
        assert_eq!(refused.kind(), ErrorKind::HolderGone);
        assert!(refused.to_string().contains('7'), "{refused}");
    }

    #[test]
    fn the_walks_one_still_held_answer_costs_are_the_number_the_constant_states() {
        // Rubric B11: a doc that states a number has to be checkable against the tree, both
        // ways. `TERMINATE_RECHECK_POLL_MS`'s doc prices `terminate_holder`'s cost against a
        // large process table in *walks*, and the loop performs one more than it performs
        // waits — which the sentence used to get wrong by one, in the one place a reader
        // would go for the figure.
        assert_eq!(recheck_walks(), 11, "the arithmetic moved under the doc");
        assert_eq!(
            recheck_walks(),
            limits::TERMINATE_RECHECK_MS / limits::TERMINATE_RECHECK_POLL_MS + 1,
            "a walk on either side of every wait"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_still_held_answer_is_immediate_when_nothing_holds_the_node_and_bounded_when_something_does()
     {
        // Both directions of the one place this daemon waits in order to learn something,
        // and both of them **deterministic**: time is tokio's paused clock, so the elapsed
        // reading below is exact arithmetic rather than a measurement, and nothing sleeps.
        //
        // The `false` direction is here rather than in the integration suite for a reason
        // note **N48** now records: over a socket, `still_held: false` requires a forked
        // child to have received `SIGTERM` and left `/proc/<pid>/fd` inside
        // `TERMINATE_RECHECK_MS` of *wall* clock — the one duration-dependent assertion the
        // workspace would have, in a repo whose rule is that nothing synchronises on a
        // sleep. Here the fact is arranged rather than raced.
        let (_backend, _temp, wchd) = daemon(limits::CAMERA_IDLE_CLOSE_MS);
        let scratch = engine::paths::TempRuntimeDir::new().expect("a throw-away directory");
        let node = scratch.base().join("video-under-test");
        std::fs::write(&node, b"").expect("a writable scratch directory");

        // Nobody holds it: the first walk answers, and the budget is not spent. That is what
        // makes `still_held` worth reading — a field that always cost a quarter of a second
        // and always said `true` would be neither.
        let began = tokio::time::Instant::now();
        assert!(!wchd.node_still_held(&node).await.expect("the walk runs"));
        assert_eq!(
            began.elapsed(),
            std::time::Duration::ZERO,
            "a node nobody holds cost a wait"
        );

        // This process holds it, and holds it throughout — so every walk finds it and what
        // ends the loop is the bound rather than the fact changing. `SIGTERM` is a request
        // and a process may ignore it (E4); this is that answer, arranged.
        let held = std::fs::File::open(&node).expect("the node exists");
        let began = tokio::time::Instant::now();
        assert!(wchd.node_still_held(&node).await.expect("the walk runs"));
        assert_eq!(
            began.elapsed(),
            std::time::Duration::from_millis(limits::TERMINATE_RECHECK_MS),
            "the bounded poll is not bounded by the budget it states"
        );

        // And letting go is visible to the same call, which is what stops the arm above from
        // being "this always answers true after a wait".
        drop(held);
        assert!(!wchd.node_still_held(&node).await.expect("the walk runs"));
    }

    #[test]
    fn a_request_naming_this_daemon_is_refused_and_every_other_pid_is_not() {
        // The daemon holds its cameras open between commands (D12), so on real hardware its
        // own pid *is* in the holder walk — and a client that named it would be asking the
        // daemon to terminate itself through the socket it is talking to. `engine::actor`
        // keeps the pid out of the answer a client would read it from (its `Busy` refusals
        // carry an empty holder list, on purpose); this keeps it out of the request.
        let me = i32::try_from(std::process::id()).expect("a pid fits");
        let refused = not_this_daemon(me).expect_err("that is this process");
        assert_eq!(refused.kind(), ErrorKind::IllegalTransition);
        assert!(refused.to_string().contains(&me.to_string()), "{refused}");
        // Not a lie about the node — this daemon does hold it — and not the kernel's answer
        // about a uid, which would send an operator looking for a privilege problem.
        assert_ne!(refused.kind(), ErrorKind::HolderGone);
        assert_ne!(refused.kind(), ErrorKind::PermissionDenied);

        // And the check is about *this* pid rather than about pids in general: a verb that
        // refused everything would pass the arm above and be useless.
        not_this_daemon(me.saturating_add(1)).expect("somebody else's process");
        not_this_daemon(1).expect("init is not this daemon");
    }

    #[test]
    fn a_sink_this_daemon_cannot_honour_is_refused_and_a_sink_it_can_is_not() {
        // Both directions of the check that runs before a camera is opened. The integration
        // suite asserts the *consequence* — `FakeBackend::opens()` is zero after either
        // refusal — and this asserts the two refusals themselves, because a check that only
        // ever sees requests it accepts is a check that cannot discriminate.
        let relative = Sink::ServerPath {
            path: "out.jpg".into(),
        };
        let error = addressable(&relative).expect_err("the daemon's cwd is /");
        assert_eq!(error.kind(), ErrorKind::IllegalTransition);
        assert!(error.to_string().contains("out.jpg"), "{error}");
        assert!(error.to_string().contains("absolute"), "{error}");
        // Not the camera's fault, and not the filesystem's: nothing was asked of either.
        assert_ne!(error.kind(), ErrorKind::FormatUnsupported);
        assert_ne!(error.kind(), ErrorKind::StorageIo);

        let unwritable = Sink::ServerPath {
            path: "/tmp/x.webp".into(),
        };
        let error = addressable(&unwritable).expect_err("this build writes three encodings");
        assert_eq!(error.kind(), ErrorKind::IllegalTransition);
        assert!(error.to_string().contains("webp"), "{error}");
        assert_ne!(
            error.kind(),
            ErrorKind::FormatUnsupported,
            "an extension is not something a camera declined to offer"
        );

        // And the two a real client sends.
        addressable(&Sink::ServerPath {
            path: "/tmp/out.jpg".into(),
        })
        .expect("an absolute path this build writes");
        addressable(&Sink::ReturnBytes {
            format: schema::capture::PhotoFormat::Jpeg,
        })
        .expect("a payload has nowhere to be wrong");
    }

    /// A photo answer with `count` claimed and `bytes` carried.
    ///
    /// Built by hand because the thing under test is what happens when the two disagree,
    /// which nothing in the engine produces — `engine::photo::from_capture` derives
    /// `returned` from the delivery it just built. `Photograph`'s fields are `pub`, so the
    /// disagreement is constructible here and nowhere a client can reach.
    fn claimed(count: u64, bytes: Option<Vec<u8>>) -> engine::photo::Photograph {
        engine::photo::Photograph {
            report: schema::capture::PhotoReport {
                camera: CameraId::parse("cam:test").expect("a literal id"),
                taken_at: schema::time::Stamp::epoch(),
                negotiated: schema::capture::NegotiatedStream {
                    pixel_format: schema::camera::PixelFormat::MJPG,
                    width: 640,
                    height: 480,
                    bytes_per_line: 0,
                    size_image: 1 << 20,
                    interval: schema::camera::FrameInterval::Discrete {
                        numerator: 1,
                        denominator: 30,
                    },
                    adjustments: Vec::new(),
                },
                rendering: schema::capture::PhotoRendering::Verbatim {
                    source: schema::camera::PixelFormat::MJPG,
                },
                transform: schema::capture::TransformApplication::Identity,
                width: 640,
                height: 480,
                frames_settled: 0,
                delivery: schema::capture::PhotoDelivery::Bytes {
                    format: schema::capture::PhotoFormat::Jpeg,
                    byte_count: count,
                },
            },
            returned: bytes,
        }
    }

    #[test]
    fn a_photo_answer_that_disagrees_with_itself_is_refused_rather_than_sent() {
        // Note N34's second orphan predicate, and the reason it is a predicate rather than
        // a constructor invariant: a truncated payload with an intact `byte_count` is what
        // a client cannot tell from a whole photo. A self-consistent answer is the ordinary
        // case, so what this pins is that the check happens at all.
        let whole = photo_response(claimed(3, Some(vec![1, 2, 3])))
            .expect("a payload that matches its count");
        assert!(whole.bytes_match_the_delivery());
        assert_eq!(whole.bytes.as_ref().map(api::Base64Bytes::len), Some(3));

        let truncated =
            photo_response(claimed(3, Some(vec![1, 2]))).expect_err("one byte short of itself");
        // Ours, not the device's: this process assembled an answer that disagrees with
        // itself, and spelling that like a camera refusal would be E3's conversion at the
        // transport layer. Same variant `Wchd::offload` answers for a panicked pool thread.
        assert_eq!(truncated.kind(), ErrorKind::DeviceIo);
        assert_ne!(truncated.kind(), ErrorKind::FormatUnsupported);
        let rendered = truncated.to_string();
        assert!(
            rendered.contains('3') && rendered.contains('2'),
            "{rendered}"
        );
        // The counts and never the payload: a frame may contain a person, and an error
        // message is a log line waiting to happen (rubric A12).
        assert!(!rendered.contains("[1, 2]"), "{rendered}");

        // The other mismatched shape, because a check that catches one of them is half a
        // check: a `Bytes` delivery with nothing attached.
        let absent = photo_response(claimed(3, None)).expect_err("bytes were promised");
        assert_eq!(absent.kind(), ErrorKind::DeviceIo);
        assert!(absent.to_string().contains("none"), "{absent}");
    }

    #[tokio::test]
    async fn a_camera_opens_on_the_first_verb_that_needs_it_and_never_before() {
        // D12 through the wire surface, which is where the claim actually matters: a
        // running `wchd` must not hold a webcam somebody else wants. `list` is the verb
        // that proves the negative half — it answers from enumeration, so a daemon that
        // opened cameras to list them would fail here.
        let (backend, _temp, wchd) = daemon(limits::CAMERA_IDLE_CLOSE_MS);
        assert_eq!(backend.opens(), 0, "constructing a daemon opened a camera");
        assert!(wchd.activity().is_empty());

        let listed = wchd.list().await.expect("the fake enumerates");
        assert_eq!(listed.cameras.len(), 1);
        assert_eq!(backend.opens(), 0, "`list` opened a camera to enumerate");
        assert!(
            wchd.activity().is_empty(),
            "`list` started an actor for a camera nobody asked about"
        );

        let camera = only_camera(&backend);
        wchd.info(camera.clone()).await.expect("the fake opens");
        assert_eq!(backend.opens(), 1);
        assert_eq!(
            wchd.activity()
                .into_iter()
                .map(|activity| (activity.camera, activity.open))
                .collect::<Vec<_>>(),
            vec![(camera.clone(), true)]
        );

        // A second verb reuses the handle: one actor, one descriptor (note N41).
        wchd.controls(camera).await.expect("still open");
        assert_eq!(backend.opens(), 1, "the second verb re-opened the device");
        assert_eq!(backend.closes(), 0);
    }

    #[tokio::test]
    async fn an_empty_enumeration_is_diagnosed_rather_than_answered_as_no_cameras() {
        // D1, over the wire. The interesting half is that the list is *empty* and the
        // answer still says something: "no cameras" and "your camera has no driver bound"
        // are different facts, and a `list` that carried only the first would send an
        // operator looking for a webcam that is plugged in.
        let temp = TempStore::new().expect("a state directory");
        let wchd = Wchd::new(
            Arc::new(Diagnosing),
            SessionStore::new(temp.root()),
            lifetime_lock(&temp),
        );

        let listed = wchd
            .list()
            .await
            .expect("an empty enumeration is not an error");
        assert!(listed.cameras.is_empty());
        assert_eq!(listed.hints, vec![the_hint()]);
        // And the sentence a human reads is the schema's, not one this daemon invented.
        assert!(
            !the_hint().message().is_empty(),
            "the hint renders to nothing"
        );
    }

    #[tokio::test]
    async fn an_idle_camera_is_closed_by_a_pass_at_the_millisecond_it_is_given() {
        // The pass itself, at a millisecond this test chooses — the driver that runs it on
        // a cadence is asserted separately, below, and the two are different claims. The
        // reason the timeout is a *parameter*: with zero, the deadline has passed by the
        // time the sweep asks, so this asserts the pass rather than waiting for a clock. `closes()` is the assertion that matters —
        // "the actor decided to close" is bookkeeping, and a descriptor going away is the
        // fact (note N42).
        let (backend, _temp, wchd) = daemon(0);
        let camera = only_camera(&backend);
        wchd.info(camera.clone()).await.expect("the fake opens");
        assert_eq!((backend.opens(), backend.closes()), (1, 0));

        // The first pass after a command declines and takes the grace with it (note N45).
        // At this timeout the deadline has certainly passed, so this pass is measuring the
        // grace and nothing else — which is why it is asserted here rather than skipped
        // past: a change that removed it would make the next line pass for a new reason.
        assert_eq!(wchd.sweep_idle_cameras(), Vec::new());
        assert_eq!(backend.closes(), 0);

        assert_eq!(wchd.sweep_idle_cameras(), vec![camera.clone()]);
        assert_eq!(backend.closes(), 1, "the descriptor is still open");
        assert!(wchd.activity().iter().all(|activity| !activity.open));

        // A closed camera is not a broken one, and a second pass closes nothing twice.
        assert_eq!(wchd.sweep_idle_cameras(), Vec::new());
        wchd.info(camera).await.expect("it opens again");
        assert_eq!((backend.opens(), backend.closes()), (2, 1));
    }

    #[tokio::test]
    async fn the_sweep_cadence_delays_a_missed_tick_rather_than_owing_it() {
        // The decision `tokio::time::interval` makes by default is the opposite of the one
        // this daemon wants, and it is invisible at the call site — so it is asserted
        // where it is made. Under `Burst`, a pass that overran the cadence is followed by
        // one immediate pass per tick that came due while it ran: a dozen round trips
        // through every actor's command queue, against the camera whose owner is using it.
        let cadence = idle_sweep_cadence(limits::CAMERA_IDLE_SWEEP_MS);
        assert_eq!(
            cadence.missed_tick_behavior(),
            tokio::time::MissedTickBehavior::Delay
        );
        // And the shipped driver runs on the constant rather than on a number of its own.
        assert_eq!(
            cadence.period(),
            std::time::Duration::from_millis(limits::CAMERA_IDLE_SWEEP_MS)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_driver_the_daemon_spawns_closes_an_idle_camera_with_nobody_asking() {
        // The half of D12 that makes a running `wchd` not hold somebody's webcam, asserted
        // through the thing that actually ships it. Every other idle-close assertion in
        // this workspace calls a sweep itself; this one calls none — it starts the driver,
        // moves the clock, and waits for the *descriptor* to go away.
        //
        // Nothing sleeps. Time is tokio's paused clock, so advancing it is an argument;
        // the wait afterwards is a blocking `recv` on the pool, which ends when the
        // camera's handle is dropped and which leaves the runtime free to poll the driver
        // meanwhile.
        const CADENCE_MS: Millis = 5_000;

        let (backend, closes, _temp, wchd) = announcing_daemon(0);
        let camera = only_camera(&backend);
        wchd.info(camera.clone()).await.expect("the fake opens");
        assert_eq!((backend.opens(), backend.closes()), (1, 0));

        // Note N45's grace, spent by a pass this test makes itself. It is spent *here*
        // rather than by letting the driver take two ticks because the subject below is the
        // driver — one tick, one close — and a test that needed two ticks would be asserting
        // the actor's grace and the cadence at once, on a paused clock whose next tick is
        // only scheduled once the previous pass has finished. The grace itself is asserted
        // where it lives, in `engine::actor`'s own suite and in the pass-level test above.
        assert_eq!(wchd.sweep_idle_cameras(), Vec::new());
        assert_eq!(backend.closes(), 0);

        let driver = wchd.spawn_idle_sweeps_every(CADENCE_MS);
        tokio::time::advance(std::time::Duration::from_millis(CADENCE_MS)).await;

        let announced = tokio::task::spawn_blocking(move || closes.recv())
            .await
            .expect("the blocking pool is alive")
            .expect("the driver closed a camera and said which");
        assert_eq!(announced, camera);
        assert_eq!(
            backend.closes(),
            1,
            "the actor decided to close and the descriptor stayed open"
        );
        // What is deliberately *not* asserted here: `activity()`. The announcement above
        // is the descriptor going away, and the actor publishes the bookkeeping a moment
        // afterwards — deliberately in that order, because a registry that said "closed"
        // while the handle was still open would be the one lie D12 cannot afford. The
        // bookkeeping is asserted where the pass is awaited instead.
        driver.abort();
    }
}
