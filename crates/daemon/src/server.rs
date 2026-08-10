//! The server half of the T5 wire surface (design D10, docs/7 P4b).
//!
//! `webcam-handler-api` declares the trait; this module implements it. There is no second
//! wire surface here and no second registration path: the daemon builds its `RpcModule`
//! with the generated `into_rpc()`, which is what `crates/api` calls "the only
//! authoritative statement of what the T5 surface registers".
//!
//! ## What is routed, and what the rest answer
//!
//! docs/7 gives P4b six read verbs — `list`, `info`, `controls`, `get`,
//! `calibrate status` and `calibrate list` — and gives P4c "the mutating half over RPC".
//! But `into_rpc()` registers all nineteen the day this lands, so the other thirteen are
//! reachable from the socket and have to answer *something*.
//!
//! They answer [`schema::Error::Unimplemented`], which is the variant D13 keeps for
//! exactly this shape of gap — "a method whose phase has not arrived needs an answer that
//! is neither a panic, nor the kernel's fault, nor a capability lie; it names the
//! operation and the phase, and says *this build* rather than *this device*". Note **N43**
//! records the decision, the alternatives, and why a producer added at P4b is not the
//! escape hatch note N6 guards against: [`unrouted`] is a pinned surface in N6's own
//! shape, its rows are **derived** from `api::METHODS` minus [`ROUTED`], and P4c cannot
//! land without emptying it.
//!
//! ## Where the answers come from
//!
//! The engine, and only the engine. Every routed verb reaches the same functions
//! `crates/cli`'s in-process executor reaches — `engine::resolve::{camera, list}`,
//! `engine::pairing::{in_effect, describe}`, `engine::lifecycle::{status, list}` — because
//! a verb implemented twice is the defect T4 and T5 exist to prevent, and P4f's parity
//! gate will compare the two surfaces byte for byte. Where the two used to differ was in
//! *assembly*, not in policy, so the assemblies with a rule inside them moved into the
//! engine as this landed rather than being copied here. All six of them: `list` was the
//! one that was copied, D1 comment and all, and the P4b review caught it.
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
//! ## What this module does not hold
//!
//! The state directory's lock. D9 gives the daemon one for its lifetime and
//! [`crate::state::OwnedState`] is where it lives; the six verbs routed here take no lock
//! at all — reading is not a state write, and a `calibrate status` that refused while a
//! daemon held the lock would be a status verb nobody can use on the machine the sessions
//! are on. P4c's mutating verbs need the token and will take it as a field then; carrying
//! one now would be a typed declaration nothing reads (rubric A8).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use api::{WchRpcServer, WireError};
use engine::actor::{CameraActivity, Cameras, OpenCamera};
use engine::settle::{Clock, Millis, MonotonicClock};
use engine::store::SessionStore;
use jsonrpsee::core::async_trait;
use schema::backend::CameraBackend;
use schema::camera::{CameraId, CameraInfo};
use schema::capture::PhotoRequest;
use schema::control::{ControlDesc, ControlSlug, ControlWrite};
use schema::profile::DeviceProfile;
use schema::report::{
    CameraDetail, CameraList, ControlReport, DiscoveryReport, TerminationReport, WriteReport,
};
use schema::session::{Selection, Session, SessionList, SessionRef, SessionStatus, SweepRequest};
use schema::snapshot::{RestoreReport, Snapshot};
use schema::{Error, limits};
use tokio::sync::oneshot;

/// The phase that lands the thirteen methods this build does not route.
///
/// docs/7 P4c: "**Lands:** the mutating half over RPC". One constant because it is one
/// fact, and because the sub-milestone that empties [`unrouted`] should have exactly one
/// string to delete.
const ROUTING_PHASE: &str = "P4c";

/// The wire names P4b routes.
///
/// A **pin**, in the tradition of `crates/api/fixtures/d13-rpc-codes.tsv` and of the
/// nineteen spellings `crates/api` asserts: which verbs a build answers is a fact a client
/// depends on, so widening it has to be a diff somebody wrote on purpose. Everything else
/// about the split is derived from it — [`unrouted`] is `api::METHODS` minus this list, so
/// a twentieth method cannot appear in neither.
///
/// docs/7 P4b names them: "read-verb routing (`list`, `info`, `controls`, `get`,
/// `calibrate status/list`)". `discover_pairs` is *not* among them and is not an oversight
/// — it writes to the camera, which is why it is its own method (note N30).
pub const ROUTED: &[&str] = &[
    "wch_calibrate_list",
    "wch_calibrate_status",
    "wch_controls",
    "wch_get",
    "wch_info",
    "wch_list",
];

/// The T5 methods this build does not route yet, and the phase that lands each.
///
/// Note N6's mechanism, instantiated a second time and deliberately: it is what keeps
/// [`schema::Error::Unimplemented`] from being an escape hatch rather than a schedule.
/// `webcam-handler-v4l2::unimplemented_surface` is the first instance — "the one list of
/// methods that answer it, and a test pins the list's size and contents" — and this is the
/// same list for the same variant on a different surface.
///
/// **Derived, not transcribed.** The population is `api::METHODS`, which the wire surface
/// generates from its own declaration, minus [`ROUTED`]. So a method added to T5 joins
/// this list by existing, and a method P4c routes leaves it by being named in [`ROUTED`] —
/// there is nowhere for the two lists to disagree, and when the last row goes the map is
/// empty and this module's producer of `Unimplemented` is unreachable.
#[must_use]
pub fn unrouted() -> BTreeMap<&'static str, &'static str> {
    let routed: BTreeSet<&str> = ROUTED.iter().copied().collect();
    api::METHODS
        .iter()
        .map(|method| method.name)
        .filter(|name| !routed.contains(name))
        .map(|name| (name, ROUTING_PHASE))
        .collect()
}

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

/// The refusal a method this build does not route answers with.
///
/// Named rather than written out at each of the thirteen call sites, so the sentence D13
/// renders comes from one place and the phase comes from [`ROUTING_PHASE`].
fn unimplemented(method: &str) -> WireError {
    WireError(Error::Unimplemented {
        operation: method.to_owned(),
        arrives_in: ROUTING_PHASE.to_owned(),
    })
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
    /// Where `now` enters. The actor reads no clock — every command carries the caller's
    /// reading (note N41) — so this is the caller.
    clock: MonotonicClock,
}

impl Wchd {
    /// A daemon over `backend`, closing idle cameras after
    /// [`limits::CAMERA_IDLE_CLOSE_MS`].
    #[must_use]
    pub fn new(backend: Arc<dyn CameraBackend>, store: SessionStore) -> Wchd {
        Wchd::with_idle_timeout(backend, store, limits::CAMERA_IDLE_CLOSE_MS)
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
        idle_after_ms: Millis,
    ) -> Wchd {
        Wchd(Arc::new(Inner {
            cameras: Cameras::with_idle_timeout(backend, idle_after_ms),
            store,
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
    /// enumerations for one request.
    async fn on_camera<T, F>(&self, requested: CameraId, work: F) -> schema::Result<(CameraInfo, T)>
    where
        F: FnOnce(OpenCamera<'_>) -> schema::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let info = self.resolve(requested).await?;
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

    // ------------------------------------------------------ P4c's, one refusal each
    //
    // Thirteen methods, each answering the row `unrouted()` derives for it. The bodies are
    // identical on purpose: what a caller acts on is the operation and the phase, and both
    // come from one place.

    async fn discover_pairs(&self, _camera: CameraId) -> Result<DiscoveryReport, WireError> {
        Err(unimplemented("wch_discover_pairs"))
    }

    async fn set(
        &self,
        _camera: CameraId,
        _writes: Vec<ControlWrite>,
        _guarded: bool,
    ) -> Result<WriteReport, WireError> {
        Err(unimplemented("wch_set"))
    }

    async fn snapshot(&self, _camera: CameraId) -> Result<Snapshot, WireError> {
        Err(unimplemented("wch_snapshot"))
    }

    async fn restore(
        &self,
        _camera: CameraId,
        _snapshot: Snapshot,
    ) -> Result<RestoreReport, WireError> {
        Err(unimplemented("wch_restore"))
    }

    async fn photo(
        &self,
        _camera: CameraId,
        _request: PhotoRequest,
    ) -> Result<api::PhotoResponse, WireError> {
        Err(unimplemented("wch_photo"))
    }

    async fn profile_capture(
        &self,
        _camera: CameraId,
        _capturer: String,
    ) -> Result<DeviceProfile, WireError> {
        Err(unimplemented("wch_profile_capture"))
    }

    async fn terminate_holder(
        &self,
        _camera: CameraId,
        _pid: i32,
    ) -> Result<TerminationReport, WireError> {
        Err(unimplemented("wch_terminate_holder"))
    }

    async fn calibrate_start(
        &self,
        _camera: CameraId,
        _task: String,
        _goal: String,
        _criteria: Vec<String>,
    ) -> Result<Session, WireError> {
        Err(unimplemented("wch_calibrate_start"))
    }

    async fn calibrate_plan(
        &self,
        _camera: CameraId,
        _session: SessionRef,
        _controls: Vec<ControlSlug>,
        _order: bool,
    ) -> Result<Session, WireError> {
        Err(unimplemented("wch_calibrate_plan"))
    }

    async fn calibrate_sweep(
        &self,
        _camera: CameraId,
        _session: SessionRef,
        _request: SweepRequest,
    ) -> Result<Session, WireError> {
        Err(unimplemented("wch_calibrate_sweep"))
    }

    async fn calibrate_select(
        &self,
        _camera: CameraId,
        _session: SessionRef,
        _control: ControlSlug,
        _selection: Selection,
    ) -> Result<Session, WireError> {
        Err(unimplemented("wch_calibrate_select"))
    }

    async fn calibrate_apply(
        &self,
        _camera: CameraId,
        _session: SessionRef,
        _partial: bool,
    ) -> Result<WriteReport, WireError> {
        Err(unimplemented("wch_calibrate_apply"))
    }

    async fn calibrate_restore(
        &self,
        _camera: CameraId,
        _session: SessionRef,
    ) -> Result<RestoreReport, WireError> {
        Err(unimplemented("wch_calibrate_restore"))
    }
}

#[cfg(test)]
mod tests {
    use engine::store::TempStore;
    use fake::FakeBackend;
    use schema::ErrorKind;
    use schema::error::Error;

    use super::*;

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
    fn the_routed_and_unrouted_halves_together_are_the_whole_wire_surface() {
        // The claim the split rests on: no method is in both, and none is in neither.
        // Derived from `api::METHODS`, so a twentieth method joins one side or fails this
        // — which is what stops P4c inheriting a population nobody designed.
        let routed: BTreeSet<&str> = ROUTED.iter().copied().collect();
        assert_eq!(routed.len(), ROUTED.len(), "a wire name is pinned twice");
        assert_eq!(routed.len(), 6, "{routed:?}");

        let unrouted = unrouted();
        assert_eq!(unrouted.len(), 13, "{unrouted:?}");

        let mut both: BTreeSet<&str> = routed.clone();
        both.extend(unrouted.keys().copied());
        let surface: BTreeSet<&str> = api::METHODS.iter().map(|method| method.name).collect();
        assert_eq!(both, surface);
        assert!(
            routed.is_disjoint(&unrouted.keys().copied().collect()),
            "a method is both routed and not"
        );

        // Every pinned name is a real one. Without this, a typo in `ROUTED` would move a
        // method silently into `unrouted` and the counts above would still add up.
        for name in ROUTED {
            assert!(surface.contains(name), "{name} is not a T5 method");
        }
        // And the one that reads like a read verb but is not: measuring pairs writes to
        // the camera, so it waits for the mutating half (note N30).
        assert!(unrouted.contains_key("wch_discover_pairs"), "{unrouted:?}");
    }

    #[test]
    fn an_unrouted_method_names_itself_and_the_phase_that_lands_it() {
        // N6's rendering claim, on this surface: the refusal says *this build* and *which
        // phase*, and it is not a capability answer about the device (E3). The population
        // is the derived map, so a method whose body named the wrong operation would have
        // to be caught by the walk below — which is what the trait-level test does.
        let unrouted = unrouted();
        assert!(!unrouted.is_empty());
        for (operation, phase) in &unrouted {
            let refusal = unimplemented(operation);
            let error = refusal.clone().into_inner();
            assert_eq!(error.kind(), ErrorKind::Unimplemented);
            assert_ne!(
                error.kind(),
                ErrorKind::DeviceIo,
                "a phase that has not arrived is not the kernel's fault"
            );
            let rendered = refusal.to_string();
            assert!(rendered.contains(operation), "{rendered}");
            assert!(rendered.contains(phase), "{rendered}");
        }
    }

    #[tokio::test]
    async fn every_unrouted_method_refuses_with_its_own_name() {
        // Thirteen calls, because a trait does not reify its methods (note N28) and the
        // thing that can go wrong here is a body naming the wrong operation — which no
        // derived population can see. The *set* is checked above; this checks the bodies.
        //
        // Each call is made with arguments that would be valid, so nothing here is refused
        // for its parameters: the refusal has to come from the body.
        let (backend, _temp, wchd) = daemon(limits::CAMERA_IDLE_CLOSE_MS);
        let camera = only_camera(&backend);
        let session = SessionRef::Task {
            task: "exposure".to_owned(),
        };

        let mut refused: BTreeMap<String, String> = BTreeMap::new();
        let mut record = |answer: std::result::Result<(), WireError>| {
            let Err(WireError(Error::Unimplemented {
                operation,
                arrives_in,
            })) = answer
            else {
                panic!("a method this build does not route answered something else");
            };
            assert!(
                refused.insert(operation.clone(), arrives_in).is_none(),
                "{operation} was named by two different methods"
            );
        };

        record(wchd.discover_pairs(camera.clone()).await.map(|_| ()));
        record(wchd.set(camera.clone(), Vec::new(), true).await.map(|_| ()));
        record(wchd.snapshot(camera.clone()).await.map(|_| ()));
        record(
            wchd.restore(
                camera.clone(),
                Snapshot {
                    taken_at: schema::time::Stamp::now(),
                    camera: schema::camera::CameraFingerprint {
                        bus_path: String::new(),
                        usb_id: None,
                        card: String::new(),
                        driver: String::new(),
                        serial: None,
                    },
                    entries: Vec::new(),
                },
            )
            .await
            .map(|_| ()),
        );
        record(
            wchd.photo(
                camera.clone(),
                PhotoRequest {
                    stream: schema::capture::StreamRequest::default(),
                    settle: schema::capture::SettlePolicy::default(),
                    transform: schema::capture::Transform::default(),
                    sink: schema::capture::Sink::ReturnBytes {
                        format: schema::capture::PhotoFormat::Jpeg,
                    },
                },
            )
            .await
            .map(|_| ()),
        );
        record(
            wchd.profile_capture(camera.clone(), "a test".to_owned())
                .await
                .map(|_| ()),
        );
        record(wchd.terminate_holder(camera.clone(), 1).await.map(|_| ()));
        record(
            wchd.calibrate_start(
                camera.clone(),
                "exposure".to_owned(),
                "a goal".to_owned(),
                Vec::new(),
            )
            .await
            .map(|_| ()),
        );
        record(
            wchd.calibrate_plan(camera.clone(), session.clone(), Vec::new(), false)
                .await
                .map(|_| ()),
        );
        record(
            wchd.calibrate_sweep(
                camera.clone(),
                session.clone(),
                SweepRequest::new(
                    ControlSlug::parse("brightness").expect("a literal slug"),
                    schema::session::SweepSpec::All,
                ),
            )
            .await
            .map(|_| ()),
        );
        record(
            wchd.calibrate_select(
                camera.clone(),
                session.clone(),
                ControlSlug::parse("brightness").expect("a literal slug"),
                Selection::ByMetric {
                    metric: schema::metrics::MetricName::Sharpness,
                },
            )
            .await
            .map(|_| ()),
        );
        record(
            wchd.calibrate_apply(camera.clone(), session.clone(), false)
                .await
                .map(|_| ()),
        );
        record(wchd.calibrate_restore(camera, session).await.map(|_| ()));

        // The bodies and the derived inventory describe the same thirteen methods, each
        // waiting for the same phase.
        let named: BTreeMap<&str, &str> = refused
            .iter()
            .map(|(operation, phase)| (operation.as_str(), phase.as_str()))
            .collect();
        assert_eq!(named, unrouted());

        // And nothing opened a camera on the way: a refusal is not a device operation.
        assert_eq!(backend.opens(), 0);
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
        let wchd = Wchd::new(Arc::new(Diagnosing), SessionStore::new(temp.root()));

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
