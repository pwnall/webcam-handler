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
//! surface, and the map of methods answering `Error::Unimplemented` is gone with the
//! producer that filled it (note **N43**: "P4c cannot land without emptying it").
//!
//! [`ROUTED`] stays anyway, and what it is worth is smaller than it used to be, so it is
//! stated rather than implied: it is a **second deliberate transcription** of a
//! compatibility contract, in `fixtures/d13-rpc-codes.tsv`'s tradition, whose value is that
//! a rename in `crates/api` costs a second diff somebody has to write on purpose. What stops
//! a twentieth method landing *unrouted* is not this list — `wire_surface!` declares the
//! trait and `api::METHODS` from one source, so a twentieth method fails to compile
//! `impl WchRpcServer for Wchd` until it is implemented. The compiler owns that law; this
//! constant owns the spelling. P4c left one producer of the variant in the workspace,
//! P4d's hotplug watch took it, and P4d then deleted the variant itself — so "no method on
//! this surface answers `Unimplemented`" is no longer a property of this module's routing
//! but a shape the D13 registry cannot express (note **N6**, retired).
//!
//! ## Where the answers come from
//!
//! The engine, and only the engine. Every routed verb reaches the same functions
//! `crates/cli`'s in-process executor reaches — `engine::resolve::{camera, list}`,
//! `engine::pairing::{in_effect, describe}`, `engine::write::set`, `engine::snapshot::{take,
//! restore}`, `engine::discover::report`, `engine::profile::capture`, `engine::photo::take`,
//! `engine::calibrate::run`, and `engine::lifecycle::{status, list, session_to_update, create,
//! discover_pairs, draft, select, apply, restore}` — because a verb implemented twice is the
//! defect T4 and T5 exist to prevent, and P4f's parity gate will compare the two surfaces byte
//! for byte. Where the two used to differ was in *assembly*, not in policy, so the assemblies
//! with a rule inside them moved into the engine as this landed rather than being copied here.
//! At P4b that was `list`, which was copied, D1 comment and all, and the P4b review caught it.
//! At P4c it was [`engine::discover::report`] — the `DiscoveryReport` assembly note **N34**
//! booked against this sub-milestone, because probe-then-read-then-merge has three rules in it
//! and a second author is free to get any of them wrong — plus the two host facts
//! `profile_capture` records, `engine::profile::kernel_release` and `schema::TOOL_VERSION`;
//! the extension-to-encoding rule `photo` needs, which moved onto
//! [`schema::capture::Sink::writable_format`] so that a `.webp` off a socket is refused by the
//! same sentence `webcam-handler-cli photo -o a.webp` is refused by (note N46, debt D-1); the
//! three control-shaped assemblies — `engine::write::set_requested`,
//! `engine::snapshot::take_in_effect` and `engine::snapshot::restore_in_effect` — whose rule
//! is *which pair set this device is in now*, read off the device rather than supplied,
//! because that decides whether an automation control is switched off first and D4's restore
//! order (the P4c review caught these three as copies, which is the same finding the P4b
//! review made about `list`); and finally the four the calibrate verbs needed —
//! `session_to_update` (the token is *proof*, and the read is half of the read-modify-write),
//! `select` (the `Selection` match records a selector whichever branch runs), `restore` (which
//! pair set, and "no snapshot is not a failure"), and the branch `calibrate_plan` keeps.
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
//! D9 gives the daemon one advisory lock for its lifetime, [`crate::state::OwnedState`] takes
//! it, and this module holds the *same* token — never a second one. Taking a second is not a
//! deadlock but something worse: `flock` denies a second open file description in this process
//! exactly as it denies another process's, so [`engine::store::SessionStore::with_lock`] here
//! would answer a client [`schema::Error::StoreLocked`] naming this daemon's own pid and
//! advising it to use `webcam-handler-client` against the daemon it is already talking to.
//! `crate::state`'s header is where that trap is written down and `tests/lock.rs` is the red
//! test.
//!
//! The read verbs take no lock at all — reading is not a state write, and a
//! `calibrate status` that refused while a daemon held the lock would be a status verb
//! nobody can use on the machine the sessions are on. Neither do the camera-shaped mutating
//! verbs: `set`, `snapshot`, `restore`, `discover_pairs`, `profile_capture` and `photo`
//! write to a *camera*, or to a path the caller named, and none of them touches the session
//! tree.
//!
//! **A held lock does not serialize this daemon against itself, and that is a defect waiting
//! to be written rather than a subtlety** (note **N47**). `webcam-handler-cli` is safe because
//! D9's daemonless protocol takes the flock per operation, so two `webcam-handler-cli`
//! processes cannot interleave a read-modify-write; a daemon holding one flock for its
//! lifetime gets no mutual exclusion between its own concurrent request tasks, and
//! `calibrate_plan --order` and `calibrate_select` open no camera at all, so not even the
//! per-camera actor separates them. So the token lives behind `Inner::sessions`, a
//! `tokio::sync::Mutex`, and the only way to reach it is to hold that mutex — structural
//! rather than remembered, in the same spirit as `engine::store` expressing "under the lock"
//! as a parameter.

use std::sync::Arc;

use api::{WchEventsServer, WchRpcServer, WireError};
use camino::{Utf8Path, Utf8PathBuf};
use engine::actor::{CameraActivity, Cameras, Enqueue, OpenCamera};
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
use schema::video::{RecordReport, RecordRequest, RecordStatus};
use schema::{Error, ErrorKind, limits};
use tokio::sync::oneshot;

use crate::events::{Events, ProgressBroadcast};

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
    "wch_record_start",
    "wch_record_status",
    "wch_record_stop",
    "wch_restore",
    "wch_set",
    "wch_snapshot",
    "wch_terminate_holder",
];

/// The subscription spellings this build routes, both names of each.
///
/// [`ROUTED`]'s sibling and a pin for its reason. It is derived from `api::SUBSCRIPTIONS`
/// rather than written out, and that is not the same kind of list: what [`ROUTED`] pins is
/// the *spelling* of a call, and what this pins is that both halves of a subscription — the
/// subscribe **and** the `unsubscribe` jsonrpsee derives from it — reach the registration
/// this daemon serves. A build whose merge dropped the unsubscribe half would leave clients
/// unable to close a stream, and nothing else would notice.
///
/// Two populations rather than one widened [`ROUTED`], because they have two producers: a
/// method's name comes from `#[method(name = …)]` and a subscription's second name comes
/// from `build_unsubscribe_method` inside jsonrpsee.
#[must_use]
pub fn routed_subscriptions() -> Vec<&'static str> {
    api::SUBSCRIPTIONS
        .iter()
        .flat_map(api::wire::Subscription::names)
        .collect()
}

/// The whole T5 surface as the one registration the daemon serves.
///
/// D10's "one wire surface" survives the two generated traits by being merged **here**, at
/// the single composition point both the shipped binary and every integration fixture go
/// through — inventing a second registration path is the thing D10 exists to prevent, and
/// two `into_rpc()` calls that never met would be exactly that.
///
/// # Errors
///
/// [`Error::DeviceIo`] when the two halves collide on a name. `Methods::merge` verifies
/// every incoming spelling against the ones already registered, which is the *only* place
/// a cross-trait collision can be caught — the proc macro's own `check_name` looks inside
/// one trait. It is this process failing to compose itself rather than a device declining
/// anything, which is the variant `Wchd::offload` already uses for that.
pub fn mount(wchd: Wchd) -> schema::Result<jsonrpsee_server::Methods> {
    let mut methods: jsonrpsee_server::Methods = WchRpcServer::into_rpc(wchd.clone()).into();
    methods
        .merge(WchEventsServer::into_rpc(wchd))
        .map_err(|err| Error::DeviceIo {
            operation: "mount the T5 wire surface".to_owned(),
            errno: None,
            message: err.to_string(),
        })?;
    Ok(methods)
}

/// The bounds every transport that serves [`mount`]'s surface runs under.
///
/// **One home for the numbers, because since P5b there are two transports.** `crate::uds`
/// serves this surface over `AF_UNIX` and `crate::http::rpc` serves it over the opt-in TCP
/// listener's WebSocket route, and both build a jsonrpsee service from a
/// [`jsonrpsee_server::ServerConfig`].
/// Two copies of that expression would be two answers to "how big is a batch on the wire
/// surface" — the second copy being the one that stops matching the day a constant moves —
/// which is design §2.10's rule applied to a configuration instead of to a law. The
/// *values* live in `schema::limits` and always did; what this function stops being copied
/// is which of them the wire reads.
///
/// jsonrpsee's own defaults (10 MB bodies, 100 connections, *unlimited* batches, and 1024
/// for both WebSocket numbers) are somebody else's numbers for somebody else's deployment,
/// and AGENTS's "bounded everything" does not let this daemon inherit them silently. Six
/// fields are set here:
///
/// - [`limits::DAEMON_MAX_CONNECTIONS`] — jsonrpsee's cap of that name, which is **not** the
///   bound `crate::uds::serve` enforces at accept; that function's doc has the measurement
///   and the distinction. On the WebSocket route it is nearer the name: jsonrpsee holds the
///   permit for an upgraded connection's whole life, and a permit is per *builder*, so the
///   two transports get a bound each rather than sharing one.
/// - [`limits::RPC_MAX_REQUEST_BYTES`] and [`limits::RPC_MAX_RESPONSE_BYTES`] — a request
///   this daemon will read and an answer it will write, the second sized for a photo.
/// - [`limits::RPC_MAX_BATCH`] — an unbounded batch on a transport that carries a camera is
///   the one default here that is a denial of service rather than a preference.
/// - [`limits::RPC_MAX_SUBSCRIPTIONS_PER_CONNECTION`] and
///   [`limits::WS_MESSAGE_BUFFER_CAPACITY`] — the two the WebSocket surface costs, which
///   P4b declined to inherit by turning that surface off and P4e-i took on when something
///   subscribed (note **N38**, note **N57**). `set_message_buffer_capacity` panics on zero,
///   which is why `schema::limits` carries a `const` assertion beside the constant rather
///   than a hope: a panic on the daemon's startup path is not an available failure mode.
///
/// **What is not claimed:** `ServerConfig` has nine more fields and this build inherits them,
/// `ping_config` (`None`) being the one with teeth — a peer that opens a WebSocket, subscribes
/// and never reads again is never reaped by an inactivity timer. `crate::uds::serve`'s doc is
/// where that residual is argued and note **N57** is where it is recorded; it is stated in one
/// place rather than two, which is the same reason this function exists.
#[must_use]
pub fn wire_bounds() -> jsonrpsee_server::ServerConfig {
    jsonrpsee_server::ServerConfig::builder()
        .max_connections(limits::DAEMON_MAX_CONNECTIONS)
        .max_request_body_size(limits::RPC_MAX_REQUEST_BYTES)
        .max_response_body_size(limits::RPC_MAX_RESPONSE_BYTES)
        .set_batch_request_config(jsonrpsee_server::BatchRequestConfig::Limit(
            limits::RPC_MAX_BATCH,
        ))
        .max_subscriptions_per_connection(limits::RPC_MAX_SUBSCRIPTIONS_PER_CONNECTION)
        .set_message_buffer_capacity(limits::WS_MESSAGE_BUFFER_CAPACITY)
        .build()
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
/// **There is no backend handle here, and that is the point.** `engine::actor`'s header rests
/// the concurrency model on "nothing hands the `Box<dyn Camera>` out … two callers write at
/// once is unrepresentable rather than prevented", and a `Arc<dyn CameraBackend>` beside the
/// registry would make `inner.backend.open(&id)?` compile in every handler in this module — a
/// second descriptor on a node an actor already owns, one line away from the P4c author who
/// copies `crates/cli`'s in-process executor (where the same call is correct, because a
/// `webcam-handler-cli` process has no actor). The two things the daemon needs of a backend,
/// [`Cameras::enumerate`] and [`Cameras::list`], are on the registry instead, and the registry
/// has no `open`.
#[derive(Debug)]
struct Inner {
    /// One actor per open camera, and the daemon's only handle on the backend the
    /// composition root chose (design §2.11).
    ///
    /// Behind an `Arc` since P5b, because [`Inner::previews`] holds the **same** registry: a
    /// preview reaches a camera through the actor every other verb reaches it through, and a
    /// second `Cameras` over one backend would be a second thread on one node — which is the
    /// arrangement `engine::actor::Cameras`'s header says must stay unrepresentable.
    cameras: Arc<Cameras>,
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
    /// `wch_calibrate_select` against an unrelated session waits for it. `webcam-handler-cli`
    /// pays the same cost in a different currency — a concurrent `webcam-handler-cli` is
    /// *refused* `StoreLocked` rather than made to wait — and waiting is the better answer for
    /// a daemon, because the refusal it would otherwise give names this daemon's own pid. The
    /// bound is the sweep's own (`limits::MAX_SWEEP_SAMPLES`) plus the client's request
    /// timeout, which `crates/api`'s `calibrate_sweep` doc already tells a client to raise.
    sessions: tokio::sync::Mutex<Arc<StoreLock>>,
    /// Where `now` enters. The actor reads no clock — every command carries the caller's
    /// reading (note N41) — so this is the caller. Two readers now: the idle deadline, and
    /// the settle policy a `photo` runs on, which is *monotonic* time and not the wall
    /// time that goes in a photo's EXIF. Conflating those two is how an NTP step becomes a
    /// settle failure, which is why `engine::photo::take` takes them separately.
    clock: MonotonicClock,
    /// The two things this daemon says without being asked (D10, P4e-i).
    ///
    /// An `Arc` of its own inside the one `Arc<Inner>`, because a hotplug watch **thread**
    /// holds it and that thread outlives the request that started it. `crate::events` is
    /// where every bound on it lives.
    events: Arc<Events>,
    /// The requests that chose to *wait* for a place in a camera's command queue, bounded.
    ///
    /// D12's `wait` flag parks a thread by construction, and this is the answer to "how
    /// many" — see [`Waiters`] and [`limits::CAMERA_ENQUEUE_WAITERS`].
    waiters: Waiters,
    /// The MJPEG preview's latest-frame fan-out (D12, docs/7 P5b).
    ///
    /// Here rather than in the web listener because it is about **cameras**, and the listener
    /// decides nothing about cameras: it is handed this value exactly as it is handed the
    /// `Methods` value, and `crate::preview` is where every question about a feed is answered.
    /// It shares this value's clock and this value's stop token, so a preview stamps the idle
    /// deadline from the same timeline every other command does and ends on the same
    /// cancellation every subscription does.
    previews: crate::preview::Previews,
    /// The takes this daemon is driving, one per camera (D7, D10; docs/7 P6c).
    ///
    /// Beside [`Inner::previews`] rather than inside it, and over the **same** three values
    /// for the same three reasons: the registry, so a recording reaches a camera through the
    /// actor every other verb reaches it through; the clock, so a take measures its own
    /// duration against the timeline every command is stamped from; and the stop token, so a
    /// running take ends on the cancellation every open stream ends on.
    ///
    /// Two registries and not one, because they answer different questions about one camera —
    /// "who is watching" and "what is being written" — and the phase that fans one frame to
    /// both (docs/7 P6c's next half) joins them at the *frame*, inside the actor command that
    /// dequeued it, rather than by merging two maps whose lifecycles are unrelated.
    recordings: crate::record::Recordings,
}

/// The permits a request that chose to wait for a place in a camera's queue has to hold.
///
/// **The bound [`limits::CAMERA_ENQUEUE_WAIT_MS`] is worth nothing without.** That constant
/// bounds one waiter; a client that sends a thousand `wch_photo {"wait": true}` on one
/// WebSocket connection sends a thousand waiters, because jsonrpsee's WebSocket transport
/// spawns a task per inbound message and bounds nothing per connection — measured, and the
/// correction note **N56** carries. Each of those parks a **blocking-pool** thread, and the
/// pool is the same one `Wchd::offload` uses for every enumeration, every session read and
/// every photo destination, so unbounded waiters are an unbounded queue in front of every
/// verb the daemon serves.
///
/// Two numbers, one place, because they are two views of one fact and separating them is how
/// they come to disagree: the semaphore is what *enforces* the bound, and the `watch` is the
/// same count in the one shape a test can **await** — `crate::events::Fanout::live` makes the
/// identical pair for the identical reason. A caller that polled a counter for "the waiters
/// are parked" would be a caller with a sleep in it under another name (AGENTS).
#[derive(Debug)]
struct Waiters {
    /// One permit per parked thread. `try_acquire` and never `acquire`: waiting for
    /// permission to wait is a second unbounded queue.
    permits: Arc<tokio::sync::Semaphore>,
    /// How many permits are held right now, as something to be told about.
    live: tokio::sync::watch::Sender<usize>,
}

impl Inner {
    /// The camera listing, assembled once for both callers that want one.
    ///
    /// `engine::resolve::list`'s assembly, reached through the registry — the same function
    /// `webcam-handler-cli list` reaches through T4's executor, because D1's "an empty
    /// enumeration is diagnosed" is a rule and a rule copied to a second composition root is
    /// where the two surfaces P4f's parity gate compares start to differ. The two callers here
    /// are the `wch_list` handler and [`Wchd::list_cameras`], which is what the daemon's
    /// startup status is built from.
    fn list_cameras(&self) -> schema::Result<schema::report::CameraList> {
        self.cameras.list()
    }
}

impl Waiters {
    fn new() -> Waiters {
        Waiters {
            permits: Arc::new(tokio::sync::Semaphore::new(
                usize::try_from(limits::CAMERA_ENQUEUE_WAITERS).unwrap_or(usize::MAX),
            )),
            live: tokio::sync::watch::Sender::new(0),
        }
    }

    /// A permit, or `None` when [`limits::CAMERA_ENQUEUE_WAITERS`] are already parked.
    fn take(&self) -> Option<Waiting> {
        let permit = Arc::clone(&self.permits).try_acquire_owned().ok()?;
        self.live.send_modify(|live| *live += 1);
        Some(Waiting {
            _permit: permit,
            live: self.live.clone(),
        })
    }
}

/// One parked waiter, as a value whose `Drop` is the whole of the release.
///
/// A guard rather than a line at the end of the waiting arm, for `crate::events::Counted`'s
/// reason: the request that owns it can end by answering, by refusing, or by its client
/// hanging up, and a decrement written on one of those paths is a decrement the other two
/// skip. It is moved **into the blocking closure** rather than held beside the `await`,
/// because the resource being counted is the pool thread — a future dropped mid-`offload`
/// does not unpark the thread it started, so releasing the permit there would let the bound
/// be exceeded by exactly the requests that were abandoned.
///
/// **Which of the two moves first, stated rather than relied on.** Taking is permit-then-
/// count, so a waiter woken by the count reaching the bound is looking at that many permits
/// really held — which is what makes the suite's rendezvous exact. Releasing is the other
/// way round (`Drop::drop` runs before a struct's fields), so for an instant the count is
/// lower than the permits outstanding. Nothing reads it in that direction; a caller that
/// wanted "the count fell, so a permit is free" would need the field ordering
/// `crate::events::Attached` uses, and would have to say so.
#[derive(Debug)]
struct Waiting {
    /// Held for its `Drop` and never read: the permit *is* the bound, and reading it would
    /// be the thing that made holding it incidental.
    _permit: tokio::sync::OwnedSemaphorePermit,
    live: tokio::sync::watch::Sender<usize>,
}

impl Drop for Waiting {
    fn drop(&mut self) {
        self.live.send_modify(|live| *live = live.saturating_sub(1));
    }
}

impl Wchd {
    /// A daemon over `backend`, closing idle cameras after
    /// [`limits::CAMERA_IDLE_CLOSE_MS`].
    ///
    /// `lock` is the token [`crate::state::OwnedState`] took at startup, not a second one:
    /// see this module's header for what a second would do. `shutdown` is the daemon's one
    /// stop token, and it is a *parameter* for the same reason: the composition root makes it
    /// before anything is served, hands the same token to this value and to
    /// `crate::shutdown::serve_until_stopped`, and a daemon that made its own would be
    /// watching a fact nobody sets.
    #[must_use]
    pub fn new(
        backend: Arc<dyn CameraBackend>,
        store: SessionStore,
        lock: Arc<StoreLock>,
        shutdown: crate::shutdown::Shutdown,
    ) -> Wchd {
        Wchd::with_idle_timeout(backend, store, lock, shutdown, limits::CAMERA_IDLE_CLOSE_MS)
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
        shutdown: crate::shutdown::Shutdown,
        idle_after_ms: Millis,
    ) -> Wchd {
        let cameras = Arc::new(Cameras::with_idle_timeout(backend, idle_after_ms));
        let clock = MonotonicClock::new();
        // Named rather than built inline, because the recordings registry takes a **clone** of
        // it: a take owns its camera's frames for the length of it (note **N117**), so the two
        // registries have to be looking at one map of feeds. A second `Previews::new` here would
        // compile, and would leave `Previews::attach` starting a second stream on a camera a
        // recording already holds.
        let previews =
            crate::preview::Previews::new(Arc::clone(&cameras), clock.clone(), shutdown.clone());
        Wchd(Arc::new(Inner {
            recordings: crate::record::Recordings::new(
                Arc::clone(&cameras),
                previews.clone(),
                clock.clone(),
                shutdown.clone(),
            ),
            previews,
            cameras,
            store,
            sessions: tokio::sync::Mutex::new(lock),
            clock,
            events: Arc::new(Events::new(shutdown)),
            waiters: Waiters::new(),
        }))
    }

    /// The MJPEG preview's fan-out, for the composition root to hand the web listener.
    ///
    /// A clone of the daemon's one [`crate::preview::Previews`] rather than a second — an
    /// `Arc` bump, exactly like the `Methods` value beside it — because "one camera, one
    /// stream" is a property of this process and a listener with a fan-out of its own would
    /// be a second answer to which cameras are being previewed.
    #[must_use]
    pub fn previews(&self) -> crate::preview::Previews {
        self.0.previews.clone()
    }

    /// How many preview frames this daemon has published, as something to **await**.
    ///
    /// [`Wchd::watch_subscribers`]' reason, on the other stream: publication is an event, and
    /// a test that polled for it would be a test with a sleep in it (AGENTS). It is the
    /// *capture* half of docs/7 P5b's stalled-reader criterion — this advances while a reader
    /// that has stopped reading stays where it was.
    #[must_use]
    pub fn watch_preview_frames(&self) -> tokio::sync::watch::Receiver<u64> {
        self.0.previews.watch_published()
    }

    /// How many preview frames a reader never saw, as something to **await**.
    ///
    /// The number that says *dropped* rather than *queued*: the latest-frame `watch` channel
    /// overwrites, so a reader that falls behind skips — and a build that put a queue there
    /// would leave this at zero while delivering every frame late.
    #[must_use]
    pub fn watch_preview_drops(&self) -> tokio::sync::watch::Receiver<u64> {
        self.0.previews.watch_skipped()
    }

    /// How many times a photo has suspended a live preview, as something to **await**.
    ///
    /// The third of the preview's counts and the only observable its *pause* has: a
    /// suspension publishes no frame and loses none, so neither
    /// [`Wchd::watch_preview_frames`] nor [`Wchd::watch_preview_drops`] moves for it. A build
    /// that answered `Busy` to a photo during a preview — which is what this daemon did before
    /// the owner's ruling of 2026-08-12 — leaves this at zero forever.
    #[must_use]
    pub fn watch_preview_interruptions(&self) -> tokio::sync::watch::Receiver<u64> {
        self.0.previews.watch_interrupted()
    }

    /// How many frames a preview refused for being a picture a browser cannot paint, as
    /// something to **await**.
    ///
    /// The fourth of the preview's counts, and the only one only a *recording* can move (note
    /// **N117**): a preview negotiates a JPEG stream or is refused one, while a take records
    /// what its caller asked for — and D7's raw fallback is a real answer. Those frames are
    /// dropped rather than served as `image/jpeg`, and this is where "the picture stopped
    /// moving" becomes a number instead of a silence.
    #[must_use]
    pub fn watch_unpaintable_preview_frames(&self) -> tokio::sync::watch::Receiver<u64> {
        self.0.previews.watch_unpaintable()
    }

    /// How many cameras this daemon is previewing right now.
    ///
    /// The lifecycle observable: one feed per camera being watched, whatever the number of
    /// tabs watching it, and none once the last of them leaves.
    #[must_use]
    pub fn previewed_cameras(&self) -> usize {
        self.0.previews.feeds()
    }

    /// The same count, as something to **await**.
    ///
    /// [`Wchd::watch_subscribers`]' reason on the preview's stream: "the last tab closed and
    /// the capture stopped" is an event, and the only honest way to wait for an event is to be
    /// told. It is what lets the preview suite assert the *lifecycle* — first viewer starts a
    /// feed, last viewer takes it away — without guessing how long a driver takes to notice.
    #[must_use]
    pub fn watch_previewed_cameras(&self) -> tokio::sync::watch::Receiver<usize> {
        self.0.previews.watch_feeds()
    }

    /// How many recordings this daemon is driving right now.
    ///
    /// [`Wchd::previewed_cameras`]' sibling on the other registry, and the lifecycle
    /// observable for D10's three `record_*` verbs: one take per camera, however many clients
    /// asked, and none once `wch_record_stop` has collected it.
    #[must_use]
    pub fn running_recordings(&self) -> usize {
        self.0.recordings.running()
    }

    /// The same count, as something to **await**.
    ///
    /// [`Wchd::watch_subscribers`]' reason on the recording registry: "the driver started" and
    /// "the driver put the take down" are events, and a test that polled for them would be a
    /// test with a sleep in it (AGENTS).
    #[must_use]
    pub fn watch_running_recordings(&self) -> tokio::sync::watch::Receiver<usize> {
        self.0.recordings.watch_running()
    }

    /// How many takes have reached an ending, as something to **await**.
    ///
    /// The one observable that separates "the take ended" from "somebody collected it":
    /// `wch_record_stop` empties the slot, so a suite watching only
    /// [`Wchd::watch_running_recordings`] could not tell a driver that finished from one that
    /// never ran at all.
    #[must_use]
    pub fn watch_finished_recordings(&self) -> tokio::sync::watch::Receiver<u64> {
        self.0.recordings.watch_finished()
    }

    /// How many finished takes a later `wch_record_start` discarded uncollected, as something
    /// to **await**.
    ///
    /// `crate::record`'s header argues why discarding is the right answer to a caller that
    /// abandoned its poll loop; this is the half that keeps it from being silent (rubric
    /// rule 3), and it is the number a healthy daemon leaves at zero.
    #[must_use]
    pub fn watch_discarded_recordings(&self) -> tokio::sync::watch::Receiver<u64> {
        self.0.recordings.watch_discarded()
    }

    /// The daemon's stop token — [`crate::shutdown::Shutdown`].
    ///
    /// One home, reached rather than copied: it is stored in `crate::events`'s `Events`,
    /// because a subscription's forwarding loop is the only thing in this process that waits
    /// on it, and everything else that needs it (the idle-sweep driver below, a test that
    /// wants to cancel) asks here instead of holding a second field. A clone is the same
    /// token, so "the daemon is stopping" cannot come to mean two things.
    #[must_use]
    pub fn shutdown(&self) -> &crate::shutdown::Shutdown {
        self.0.events.shutdown()
    }

    /// What the daemon's subscriptions are doing right now (D10, P4e-i).
    ///
    /// The status surface for the event half, in [`Wchd::activity`]'s tradition and for its
    /// reason: the counts an operator wants and the ones an integration test asserts are the
    /// same counts, so there is one accessor rather than a test hook beside a metric.
    ///
    /// Note **N17** pre-authorised exactly this shape — "if P4e's subscription needs to
    /// distinguish 'this client is slow' from 'this client is gone' … that is a *query* on
    /// the sink, not a failure of `emit`".
    #[must_use]
    pub fn subscriptions(&self) -> crate::events::SubscriptionActivity {
        self.0.events.activity()
    }

    /// The live subscriber count, as something to **await** rather than poll.
    ///
    /// "The subscription was reaped" is an event, and the only honest way to wait for an
    /// event is to be told: a test that polled [`Wchd::subscriptions`] for it would be a
    /// test with a sleep in it under another name (AGENTS).
    #[must_use]
    pub fn watch_subscribers(&self) -> tokio::sync::watch::Receiver<usize> {
        self.0.events.watch_live()
    }

    /// Every event this daemon's subscriptions failed to deliver, as something to **await**.
    ///
    /// [`Wchd::watch_subscribers`]' sibling and for its reason: a loss is an event, and the
    /// itemised counters [`Wchd::subscriptions`] reports can be read at any moment but only
    /// a change can be waited for. It is what lets "the bound refused an event" be a
    /// rendezvous rather than a guess about how far a subscription's task has got — the
    /// difference between an exact assertion and a measurement of the scheduler.
    #[must_use]
    pub fn watch_losses(&self) -> tokio::sync::watch::Receiver<u64> {
        self.0.events.watch_lost()
    }

    /// How many requests are parked waiting for a place in a camera's command queue — D12's
    /// `wait` flag, as the number [`limits::CAMERA_ENQUEUE_WAITERS`] bounds.
    ///
    /// An operator's number first: it is how oversubscribed the cameras are, and it is the
    /// one thing a daemon that *feels* slow can be asked that the camera counters do not
    /// answer. It is a `watch` rather than a counter for [`Wchd::watch_subscribers`]'
    /// reason — "the waiters are parked" is an event, and a test that polled for it would be
    /// a test with a sleep in it — and that is what lets the suite release a held device
    /// *after* the waiters have provably arrived rather than guessing that they have.
    ///
    /// Rubric A8: without it, [`limits::CAMERA_ENQUEUE_WAITERS`] would be a number nothing
    /// reads and nothing can go red about.
    #[must_use]
    pub fn watch_waiting_captures(&self) -> tokio::sync::watch::Receiver<usize> {
        self.0.waiters.live.subscribe()
    }

    /// Whether a hotplug watch thread is running, as something to **await**.
    ///
    /// `crate::events::Hotplug`'s header states that the watch exists exactly while somebody
    /// is subscribed — started by the first subscriber, put down after the last one leaves —
    /// and this is what makes that a property something outside can check rather than a
    /// paragraph. It is also the only thing that can observe
    /// [`limits::HOTPLUG_WATCH_DEADLINE_MS`] doing its job: the thread notices its last
    /// reader has gone on the turn that deadline gives it, so a build that changed the
    /// number changes how long this takes to go false and nothing else does.
    #[must_use]
    pub fn watch_hotplug(&self) -> tokio::sync::watch::Receiver<bool> {
        self.0.events.watch_hotplug_running()
    }

    /// What every camera this daemon has an actor for is doing — docs/7 P4b's status
    /// surface, reached through the value an integration test holds (note N42).
    #[must_use]
    pub fn activity(&self) -> Vec<CameraActivity> {
        self.0.cameras.activity()
    }

    /// Enumerate, exactly as `wch_list` does.
    ///
    /// **Blocking** — an `open` and a walk of ioctls per node — which is why the RPC handler
    /// reaches it through `Wchd::offload` and why `crate::systemd::publish_camera_count`
    /// reaches it through a `spawn_blocking` of its own. It exists as a public method for
    /// that second caller and for no other reason: the daemon's startup status names how many
    /// cameras this machine had, and a composition root that enumerated through some other
    /// path would be a second answer to "what does this daemon see" (design §2.10). Both go
    /// through `Inner::list_cameras`, so there is one.
    ///
    /// # Errors
    ///
    /// Whatever the backend refuses enumeration with. A host with no cameras is `Ok` with an
    /// empty list and a hint, never an error — that distinction is `engine::resolve::list`'s
    /// and E3's, and nothing here may flatten it.
    pub fn list_cameras(&self) -> schema::Result<schema::report::CameraList> {
        self.0.list_cameras()
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

    /// Run [`Wchd::sweep_idle_cameras`] on this build's cadence, until the daemon is asked to
    /// stop.
    ///
    /// The shipped caller, and the one reader of [`limits::CAMERA_IDLE_SWEEP_MS`]:
    /// everything about the driver is [`Wchd::spawn_idle_sweeps_every`]'s, so a test can
    /// talk about a cadence without the constant, and the constant has exactly one home.
    ///
    /// Must be called from inside a tokio runtime.
    pub fn spawn_idle_sweeps(&self) -> tokio::task::JoinHandle<()> {
        self.spawn_idle_sweeps_every(limits::CAMERA_IDLE_SWEEP_MS)
    }

    /// Run [`Wchd::sweep_idle_cameras`] every `cadence_ms`, until the daemon is asked to
    /// stop.
    ///
    /// D12 says the daemon "closes on idle", and a deadline nobody checks is not a deadline:
    /// `engine::actor` computes idleness and **this is the thing that asks**. How long after a
    /// deadline a camera closes, and what a pass costs, are [`limits::CAMERA_IDLE_SWEEP_MS`]'s
    /// to state — restating them here would be two homes for one piece of arithmetic, which is
    /// how they come to disagree. Without it a shipped `webcam-handler-daemon` would open a
    /// camera on the first `wch_info` and hold the descriptor until the process exited, which
    /// is precisely the complaint D12 exists to answer — so the driver is a parameterised
    /// function with a test on it rather than a body only `main` reaches.
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
    /// **This task ends itself**, on the daemon's [`crate::shutdown::Shutdown`] token: the
    /// cadence is raced against it, so a stop asked for between two passes ends the driver on
    /// the spot rather than at the next tick, and a stop asked for *during* a pass ends it
    /// when that pass returns. The returned handle is what
    /// [`crate::shutdown::serve_until_stopped`] **awaits**, which is what makes "the
    /// idle-sweep driver ended" a fact something waited for rather than a detached task's
    /// maybe; a test that starts one of its own can still abort it.
    ///
    /// What it is deliberately *not* is a drain: a pass already in flight is allowed to
    /// finish, because it is a round trip through an actor's command queue and abandoning it
    /// would leave a camera the registry thinks is closing. That wait is the actor's own and
    /// is bounded by the command in front of it — **which is not a bound this daemon owns**,
    /// because that command may be a calibration sweep. So the *join* is bounded instead, by
    /// the one deadline [`crate::shutdown`] gives the whole teardown
    /// ([`limits::DAEMON_SHUTDOWN_DRAIN_MS`], shared with steps 3 and 5), and a pass still
    /// running when it expires is abandoned rather than waited for. The sentence this replaces
    /// claimed the wait "sits inside" that number *because the join happens after the drain*,
    /// which is precisely backwards: after the drain is where nothing was measuring it (note
    /// **N174**).
    ///
    /// A panicked pass does **not** end this driver — see the `JoinError` arm below.
    ///
    /// Must be called from inside a tokio runtime.
    pub fn spawn_idle_sweeps_every(&self, cadence_ms: Millis) -> tokio::task::JoinHandle<()> {
        let wchd = self.clone();
        self.spawn_sweeps_every(cadence_ms, move || wchd.sweep_idle_cameras())
    }

    /// [`Wchd::spawn_idle_sweeps_every`], over a pass supplied by the caller.
    ///
    /// The shipped caller passes [`Wchd::sweep_idle_cameras`] and there is no other, so this
    /// is the same function with its one collaborator named — the split
    /// [`Wchd::spawn_idle_sweeps`] already makes for the *cadence*, made once more for the
    /// *pass*. What it buys is the claim below the loop that nothing else can make: a pass
    /// that **panics** must not end this driver, and a `sweep_idle_cameras` that panics on
    /// demand is not something a replayed profile can be asked for (AGENTS: a fake capability
    /// no real device exhibits is a bug in the fake). The panic that is real — a backend that
    /// panics on device vocabulary \[PF:1\] — happens on a camera's *actor* thread, which the
    /// pool thread running a pass never sees as a panic of its own.
    fn spawn_sweeps_every<P>(&self, cadence_ms: Millis, pass: P) -> tokio::task::JoinHandle<()>
    where
        P: Fn() -> Vec<CameraId> + Clone + Send + 'static,
    {
        let stopping = self.shutdown().clone();
        tokio::spawn(async move {
            let mut cadence = idle_sweep_cadence(cadence_ms);
            loop {
                // Both arms are cancel-safe: `Interval::tick` keeps its schedule across a
                // cancelled poll, and a cancelled token stays cancelled, so racing them costs
                // neither a tick nor an edge.
                tokio::select! {
                    () = stopping.cancelled() => break,
                    _ = cadence.tick() => {}
                }
                let pass = pass.clone();
                let closed = tokio::task::spawn_blocking(pass).await;
                match closed {
                    Ok(closed) => {
                        for camera in closed {
                            // A camera the operator can now use from another application,
                            // which is what D12's idle close is for — worth a line, and a
                            // camera id is not a frame (see `crate::logging`).
                            tracing::info!(%camera, "closed an idle camera");
                        }
                    }
                    // **Two endings, and only one of them is this driver's** (note **N174**).
                    // A cancelled join is the blocking pool going away, which happens when
                    // the runtime is shutting down: there is nothing left to sweep and
                    // nobody to tell. A *panicked* pass is a defect — a backend that panics
                    // on device vocabulary is measured rather than hypothetical \[PF:1\] —
                    // and treating the two alike cost D12's idle close for the life of the
                    // process, silently, which is the one outcome rubric rule 3 forbids
                    // outright: the daemon would hold every camera it ever opened and say
                    // nothing about why.
                    //
                    // So a panic is said out loud and the driver **carries on**. The next
                    // pass is a fresh `spawn_blocking` and the camera whose vocabulary
                    // caused it is still there, so a deterministic panic repeats this line
                    // once per `limits::CAMERA_IDLE_SWEEP_MS` — which is the honest
                    // behaviour for a daemon whose housekeeping is failing, and cheaper
                    // than a descriptor nobody can get back.
                    Err(err) if err.is_panic() => {
                        tracing::error!(
                            error = %err,
                            "an idle-camera sweep panicked; this daemon's cameras stay open \
                             until a later pass closes them"
                        );
                    }
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
    /// The rule is `engine::resolve::camera`'s, which is what stops `webcam-handler-cli` and
    /// `webcam-handler-daemon` disagreeing about what a prefix means. Enumeration is live
    /// every time (E2): a daemon that cached its camera list would answer about a camera that
    /// had been unplugged, and noticing that is P4d's hotplug watch rather than an assumption
    /// this can make.
    async fn resolve(&self, requested: CameraId) -> schema::Result<CameraInfo> {
        self.offload(move |inner| {
            let cameras = inner.cameras.enumerate()?;
            engine::resolve::camera(&cameras, &requested).cloned()
        })
        .await
    }

    /// Resolve `requested` for the two verbs that are about a **take** rather than a camera.
    ///
    /// [`Wchd::resolve`] against the machine first, exactly as every other verb does, because
    /// D1's prefixes are a property of an enumeration and E2 says the enumeration is live. What
    /// this adds is the one case where the machine has stopped being the authority on the
    /// question being asked: **a camera that was unplugged during a take**.
    ///
    /// That take is a real thing this daemon is holding — a file on disk, a driver that has
    /// ended or is ending, and a `RecordingEnd::DeviceFailed` waiting to be collected — and
    /// before note **N173** it was unreachable: `record_stop` and `record_status` answered
    /// `CameraUnknown`, which D13 defines as *"a name that never resolved — distinct from
    /// `DeviceGone`, a camera that was there"*. AGENTS rule 7 is the line being crossed there
    /// (availability converted into "this name means nothing"), and the consequence for the
    /// unattended consumer is worse than the wording: `CameraUnknown` reads as *your id is
    /// wrong*, so an agent whose camera was unplugged mid-take retries with a different name
    /// instead of reading the refusal that is sitting in the registry with its name on it.
    ///
    /// So a `CameraUnknown` is turned back into the take's own id **only when this daemon is
    /// holding something for exactly that id** — `Recordings::holds`, an exact match rather
    /// than a second resolver, and the id is the one `record_start` answered with. Every other
    /// refusal from the enumeration travels unchanged: a backend that cannot enumerate at all
    /// is not a camera that vanished.
    async fn recording_camera(&self, requested: CameraId) -> schema::Result<CameraId> {
        match self.resolve(requested.clone()).await {
            Ok(info) => Ok(info.id),
            Err(err)
                if err.kind() == ErrorKind::CameraUnknown
                    && self.0.recordings.holds(&requested).await =>
            {
                Ok(requested)
            }
            Err(err) => Err(err),
        }
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
        self.on_resolved_camera_queueing(info, Enqueue::Refuse, work)
            .await
    }

    /// [`Wchd::on_resolved_camera`], choosing what a full command queue means — D12's
    /// `wait` flag.
    ///
    /// The one caller that passes anything but [`Enqueue::Refuse`] is `wch_photo`, because
    /// `PhotoRequest::wait` is the only place on this wire where a client says which it
    /// wants. Every other verb takes the refusal, which is what all of them have always
    /// taken.
    ///
    /// **The waiting arm goes to the blocking pool, and that is not an implementation
    /// detail.** `CameraActor::submit_with(_, WaitUntil(_), _)` parks the calling thread by
    /// construction, and this daemon's one rule about blocking is that nothing which can
    /// park a thread runs on a runtime worker (see this module's header). So a request that
    /// *chose* to wait pays a pool thread for as long as it waits; a request that did not
    /// pays nothing at all, because `try_send` does not block and never leaves this task.
    ///
    /// **And the number of those threads is bounded here** — [`Waiters`], sized by
    /// [`limits::CAMERA_ENQUEUE_WAITERS`]. It is not bounded by the transport: one WebSocket
    /// connection can hold arbitrarily many calls in flight, which is the correction note
    /// **N56** carries and the reason this permit pool exists rather than an arithmetic
    /// argument in a doc comment. A request that finds the pool exhausted takes the enqueue a
    /// request that never asked to wait takes — served if there is room right now,
    /// [`Error::Busy`] if there is not — so the flag degrades to its own `false` under load
    /// and the refusal vocabulary does not grow by a variant meaning "too many waiters"
    /// (D13 is closed at eighteen).
    async fn on_resolved_camera_queueing<T, F>(
        &self,
        info: CameraInfo,
        how: Enqueue,
        work: F,
    ) -> schema::Result<(CameraInfo, T)>
    where
        F: FnOnce(OpenCamera<'_>) -> schema::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let at = self.0.clock.now_ms();
        let actor = self.0.cameras.actor(&info, at)?;

        let (answered, answer) = oneshot::channel::<schema::Result<T>>();
        let submitted = move |device: schema::Result<OpenCamera<'_>>| {
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
        };
        // The permit is asked for only on the arm that would park a thread, so an ordinary
        // request never touches the pool and never moves the count a waiting one is measured
        // by.
        let parking = match how {
            Enqueue::Refuse => None,
            Enqueue::WaitUntil(_) => self.0.waiters.take(),
        };
        match parking {
            // No lock, no pool thread, no `await`: the fast path is exactly what every verb
            // did before the flag existed. It is also where a `wait: true` request past the
            // permit pool lands, which is why the two are one arm rather than two — a caller
            // past the bound is refused with the words the flag already promised.
            None => actor.submit(at, submitted)?,
            Some(permit) => {
                let waiting = Arc::clone(&actor);
                self.offload(move |_| {
                    let outcome = waiting.submit_with(at, how, submitted);
                    // Released on the thread it bounds rather than on the task that started
                    // it — see [`Waiting`].
                    drop(permit);
                    outcome
                })
                .await?;
            }
        }

        match answer.await {
            Ok(answer) => answer.map(|value| (info, value)),
            // The actor's thread left holding the reply. `engine::actor` owns that
            // refusal, including which node it names.
            Err(_) => Err(actor.device_gone()),
        }
    }

    /// [`Wchd::on_resolved_camera`], for work that needs the daemon's state as well as the
    /// device.
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
    ///
    /// **For a caller that has already resolved**, which every caller of it is: the five
    /// calibrate verbs need `info.fingerprint` to build the closure that checks the session,
    /// and `wch_photo` needs the resolution to happen *before* it opens the destination a
    /// client named, so that a request naming a camera this host does not have leaves no
    /// file behind. Handing the [`CameraId`] back to be enumerated a second time would put
    /// the check and the writes on two different readings of the machine (E2), and would run
    /// that enumeration *inside* `editing_sessions()`, charging a `/sys` walk per node to
    /// every other client waiting on the session mutex.
    async fn on_resolved_camera_with_state<T, F>(
        &self,
        info: CameraInfo,
        work: F,
    ) -> schema::Result<(CameraInfo, T)>
    where
        F: FnOnce(&Inner, OpenCamera<'_>) -> schema::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        self.on_resolved_camera_with_state_queueing(info, Enqueue::Refuse, work)
            .await
    }

    /// [`Wchd::on_resolved_camera_with_state`], choosing what a full command queue means.
    ///
    /// [`Wchd::on_resolved_camera_queueing`]'s reason at the entry point `wch_photo` uses:
    /// the flag has to reach the enqueue from the request, and the request's verb is one
    /// that needs the daemon's state as well as the device.
    async fn on_resolved_camera_with_state_queueing<T, F>(
        &self,
        info: CameraInfo,
        how: Enqueue,
        work: F,
    ) -> schema::Result<(CameraInfo, T)>
    where
        F: FnOnce(&Inner, OpenCamera<'_>) -> schema::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let inner = Arc::clone(&self.0);
        self.on_resolved_camera_queueing(info, how, move |device| work(&inner, device))
            .await
    }

    /// `record_start`'s steps three to five, on the task that owns the camera's stream.
    ///
    /// Split from the handler for one reason and it is the whole of note **N177**: from
    /// `Recordings::reserve` to `Recordings::adopt` this call is holding claims that no `Drop`
    /// can give back — the recording slot, the camera's frames and, after
    /// `engine::record::start`, the device's own `VIDIOC_STREAMON` — and a future a client can
    /// cancel must not be the thing holding them.
    ///
    /// # Errors
    ///
    /// Whatever refused: the request's own predicates one layer up, [`Error::Busy`] from the
    /// reservation, the destination's `StorageIo`, the device's refusal of the negotiation or
    /// the header write. The slot and the camera's frames go back on every one of those paths.
    async fn begin_recording(
        &self,
        info: CameraInfo,
        request: RecordRequest,
        path: Utf8PathBuf,
    ) -> schema::Result<RecordStatus> {
        // 3. The camera's recording slot, taken **before** any device work — so a second
        //    `record_start` is refused by this daemon with `Busy` (retry) rather than by the
        //    kernel's `EBUSY` after a file has been opened, and so two starts arriving together
        //    cannot both reach `VIDIOC_STREAMON`.
        let reserved = self.0.recordings.reserve(&info).await?;
        // 4. The destination, resolved from a name to a **descriptor** exactly once — note
        //    **N51**'s discharge, and the same call `wch_photo` makes: no open on this path can
        //    wait, and nothing a client does to the name afterwards can redirect these bytes.
        //    On the blocking pool, because `open(2)` is fast until the path is on a hung mount.
        let destination = {
            let opening = path.clone();
            match self
                .offload(move |_| open_destination(&opening, "recording"))
                .await
            {
                Ok(file) => file,
                Err(err) => {
                    // The slot goes back on every path out of here, or this camera would
                    // refuse every later `record_start` with `Busy` for the life of the
                    // process.
                    self.0.recordings.withdraw(reserved).await;
                    return Err(err);
                }
            }
        };

        // Now the device work: negotiate, pair the container against what was negotiated, and
        // write the header — all inside **one** actor command, because a header written in a
        // second command could arrive after something else had taken the stream. D12's `wait`
        // flag chooses what a full command queue means, through the one entry point that owns
        // the bounded pool of threads a waiting request parks (`Waiters`).
        let now = schema::time::Stamp::now();
        let clock = self.0.clock.clone();
        let opening_path = path.clone();
        let how = enqueueing(request.wait);
        let begun = self
            .on_resolved_camera_queueing(info.clone(), how, move |device| {
                let opened = engine::record::start(&mut *device, &request)?;
                let mut files = OpenedAheadFile(Some(destination));
                match engine::record::Recording::begin(
                    &opened,
                    &opening_path,
                    &mut files,
                    &clock,
                    now,
                ) {
                    Ok(recording) => Ok((opened, recording)),
                    Err(refused) => {
                        // Stopped before the refusal travels, for `engine::record::start`'s
                        // reason: a camera left streaming for a recording that is not going to
                        // happen is a camera the next `open` finds busy. The stop's own failure
                        // is discarded because the caller is already holding the error that
                        // matters.
                        let _ = engine::record::stop(device);
                        Err(refused)
                    }
                }
            })
            .await;
        let (info, (opened, recording)) = match begun {
            Ok(started) => started,
            Err(err) => {
                self.0.recordings.withdraw(reserved).await;
                return Err(err);
            }
        };

        // The take is real from here: the container's header is on disk, so a caller that
        // never polls still has a file both of `imaging::avi::read`'s readers can open.
        self.0
            .recordings
            .adopt(reserved, &info, &opened, recording, &path, now)
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

/// Everything about a photo's sink that can be answered without touching the filesystem.
///
/// Two rules, both of which only a *socket* can break, and both refused here rather than
/// downstream so that a request this build was never going to honour costs nobody a
/// descriptor. That is assertable — `FakeBackend::opens()` is still zero after either of the
/// refusals — which is what keeps this an early check rather than a duplicated one.
///
/// - [`Sink::is_addressable`] is note **N34**'s orphan predicate and this is the consumer it
///   named. `cli_core::Command::photo_request` resolves a relative `-o` against the *caller's*
///   cwd before sending (D10), so neither `webcam-handler-cli` nor `webcam-handler-client` can
///   produce a relative `Sink::ServerPath`; a hand-written client can, and this daemon's
///   working directory under systemd is `/`, so `{"kind":"server_path","path":"out.jpg"}`
///   would write `/out.jpg` as the daemon's uid. The predicate lives beside the variants it
///   constrains because a paragraph is a thing an implementer has to have read.
/// - [`Sink::writable_format`] is the same shape for the other question, and it is called
///   again inside `engine::photo` where the answer is used. That is one rule asked twice
///   rather than two rules: the engine's call is the backstop every caller gets, and this
///   one is why `/tmp/x.webp` never reaches a camera.
///
/// The third rule — *the destination is a regular file* — is [`open_destination`]'s, because
/// it is a question about an **inode** rather than about a request, and note **N51** is the
/// entry that says why the difference is not pedantry. Both refusals are
/// [`Error::IllegalTransition`], which note **N46** records: not `FormatUnsupported`, which
/// is the camera saying what it cannot offer (E3); not `StorageIo`, which would claim a
/// filesystem was consulted; not `DeviceIo`, which would blame the kernel for a request
/// nobody could honour.
///
/// **This one no longer touches the filesystem at all**, which is the P4e-i change: it used
/// to `stat` the path and refuse a non-regular file, and a `stat` followed later by an
/// `open` is a window a client can step through. Nothing here blocks now — the caller still
/// runs it on the blocking pool, beside the open that does.
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
    Ok(())
}

/// Open a photo's destination the way a *daemon* must: without ever waiting in `open(2)`,
/// and before a camera is touched.
///
/// Note **N51**'s discharge, and the whole of what it adds over the `stat` it replaces:
/// **every question is asked of the descriptor**, so there is no window between the check
/// and the write for a client to step through. The `stat`-then-`open` shape P4c landed
/// refused a fifo it *saw*, and a client that replaced the path between the two calls still
/// won the race and still parked the camera's one thread in `open(2)` forever. Here the
/// verdict and the destination are the same object: whatever the name points at afterwards,
/// the bytes go to the inode this function approved.
///
/// Two flags do the work, and they do different halves:
///
/// - `O_NONBLOCK` is what makes the open itself a *refusal* rather than a wait. A fifo with no
///   reader answers `ENXIO` instead of blocking until somebody reads it, which is the wedge
///   N51 measured — one `mkfifo` and one `wch_photo` used to make the operator's webcam
///   unusable by any application until `webcam-handler-daemon` was restarted. `rustix` carries
///   the per-architecture value, so the flag costs neither a guess nor an `unsafe` block (this
///   crate is `#![forbid(unsafe_code)]`, and N51's 2026-08-10 amendment is the ruling).
/// - The `fstat` is what refuses the rest: a directory, a socket, a character device, and
///   `/dev/stdout` — which under systemd is the journal, and a camera frame in the journal
///   is a bound this project does not trade (rubric A12). A fifo *with* a reader opens
///   perfectly well under `O_NONBLOCK` and is still not a file this daemon writes a frame
///   to, so the two checks are not two spellings of one.
///
/// **`O_TRUNC` is deliberately absent.** Truncating before the capture has happened would
/// empty an operator's existing photo on the way to reporting that the camera failed; the
/// length is set on the descriptor after the bytes exist, by
/// [`engine::photo::write_to_open_file`].
///
/// **Blocking**, and the reason the caller runs it on the blocking pool: `open(2)` on a
/// regular file is fast until the path is on a hung mount, and this daemon runs nothing that
/// can block on a runtime worker.
///
/// **The residual, stated at its real size** (note **N59** corrected this paragraph). A
/// regular file whose *write* blocks on a dead mount still costs **the camera**, not one
/// command: `engine::actor::Thread::run` calls the work inline, so a `write(2)` that never
/// returns means the actor never records that it finished, `Live::busy` stays raised, D12's
/// idle close can never fire, and no later command on that camera ever runs. What the
/// bounded enqueue buys is only how quickly *other* callers are told no — a `wait: true`
/// caller gives up at [`limits::CAMERA_ENQUEUE_WAIT_MS`] with [`Error::Busy`] instead of
/// occupying a pool thread indefinitely, and a `wait: false` caller was already refused.
/// Ending such a command outright would need a cancellable device thread, which nothing in
/// D12 provides.
///
/// # Errors
///
/// [`Error::IllegalTransition`] when the path names something this build will not write a
/// photo to, for [`addressable`]'s reason and in its words. [`Error::StorageIo`] for every
/// other reason an open fails — a missing parent directory, a permission, a full disk —
/// because those are the filesystem declining rather than the request being unhonourable.
///
/// `writing` is the noun the refusal uses — `"photo"` or `"recording"`, its two callers.
/// A parameter rather than two functions because the *rule* is one rule: `wch_record_start`
/// has the identical obligation for the identical reason (a `mkfifo` and one call used to make
/// the operator's webcam unusable until the daemon was restarted), and a second copy of an
/// `O_NONBLOCK` open with an `fstat` behind it is exactly the second home design §2.10 forbids
/// — the copy being the one that stops matching the day a flag moves.
fn open_destination(path: &Utf8Path, writing: &str) -> schema::Result<std::fs::File> {
    use rustix::fs::{FileType, Mode, OFlags};

    let refuse = |what: &str| Error::IllegalTransition {
        from: format!("not_a_regular_file({path})"),
        op: format!(
            "write a {writing} to {path}; that path names a {what} rather than a file, and \
             this daemon writes {writing}s only to regular files — opening a fifo would park \
             the camera's thread until somebody read it, and a character device or \
             /dev/stdout would put a frame somewhere frames may not go"
        ),
    };

    let opened = rustix::fs::open(
        path.as_std_path(),
        OFlags::WRONLY | OFlags::CREATE | OFlags::NONBLOCK | OFlags::CLOEXEC,
        // 0666 before the umask, which is what `File::create` asks for and therefore what a
        // photo written by `webcam-handler-cli` already gets. A daemon that tightened it here
        // would make the same photo readable or not depending on which surface took it.
        Mode::from_bits_truncate(0o666),
    );
    let opened = match opened {
        Ok(opened) => opened,
        // The two errnos that mean "this path is not a file", rather than "the filesystem
        // said no". `EISDIR` is a directory opened for writing; `ENXIO` is the fifo with no
        // reader — the wedge itself, arriving as a refusal because of `O_NONBLOCK`.
        Err(rustix::io::Errno::ISDIR) => return Err(refuse("directory")),
        Err(rustix::io::Errno::NXIO) => return Err(refuse(&describe_unopenable(path))),
        Err(errno) => {
            return Err(Error::StorageIo {
                path: path.to_owned(),
                errno: Some(errno.raw_os_error()),
                message: format!("{}: {errno}", std::io::Error::from(errno)),
            });
        }
    };

    let found = rustix::fs::fstat(&opened).map_err(|errno| Error::StorageIo {
        path: path.to_owned(),
        errno: Some(errno.raw_os_error()),
        message: format!("{}: {errno}", std::io::Error::from(errno)),
    })?;
    let kind = FileType::from_raw_mode(found.st_mode);
    if kind != FileType::RegularFile {
        return Err(refuse(describe_file_type(kind)));
    }
    Ok(std::fs::File::from(opened))
}

/// What a path is, for the one refusal that could not ask its descriptor.
///
/// `ENXIO` means the open declined to wait, and the *verdict* is already made — this only
/// supplies the sentence. So the `stat` here is not the `stat` note N51 is about: nothing
/// this function returns can change what the daemon does, only what it says, and a client
/// that swaps the path between the failed open and this call gets a less accurate noun
/// rather than a parked camera.
///
/// The fallback names the condition rather than guessing at a type, because `ENXIO` also
/// answers for a device node with no driver behind it.
fn describe_unopenable(path: &Utf8Path) -> String {
    use rustix::fs::FileType;
    match rustix::fs::stat(path.as_std_path()) {
        Ok(found) => describe_file_type(FileType::from_raw_mode(found.st_mode)).to_owned(),
        Err(_) => "destination that cannot be opened without waiting for a reader".to_owned(),
    }
}

/// What a destination is, for a refusal that has to say why it is not a file.
///
/// An exhaustive `match` including `rustix`'s own catch-all `Unknown` arm, which is AGENTS
/// rule 6's shape with the fallback supplied by the vocabulary rather than by us: an
/// `st_mode` this kernel invents lands in `Unknown` and gets a sentence, and a variant
/// `rustix` adds is a compile error rather than a panic on a request-driven path.
///
/// `RegularFile` shares the fallback arm because the only caller has already established it
/// is not one — naming it would be writing a refusal for a destination that is accepted.
fn describe_file_type(kind: rustix::fs::FileType) -> &'static str {
    use rustix::fs::FileType;
    match kind {
        FileType::Directory => "directory",
        FileType::Fifo => "fifo",
        FileType::Socket => "socket",
        FileType::CharacterDevice => "character device",
        FileType::BlockDevice => "block device",
        FileType::Symlink => "symbolic link",
        FileType::RegularFile | FileType::Unknown => "thing that is not a regular file",
    }
}

/// D12's `wait` flag, as the enqueue it chooses.
///
/// A named function rather than an `if` at the call site, because it is the whole of what
/// this daemon does with the field and it has exactly two answers, both of which have to be
/// able to go red. `wait: true` waits for room in the camera's command queue, bounded by
/// [`limits::CAMERA_ENQUEUE_WAIT_MS`] — the constant `Enqueue::waiting` reads, so the budget
/// is not repeated here. `wait: false` — the default, and every request written before the
/// field existed — takes [`Error::Busy`] the moment the queue is full. Both end in the same
/// refusal, so the flag buys when the answer arrives and never what it says (note **N42**).
fn enqueueing(wait: bool) -> Enqueue {
    if wait {
        Enqueue::waiting()
    } else {
        Enqueue::Refuse
    }
}

/// The one file a photo request may write, opened before this daemon touched a camera.
///
/// The daemon's [`engine::photo::Destination`], and the reason that trait exists:
/// `webcam-handler-cli` opens a path when the bytes are ready because a person typed it, and
/// this daemon cannot, because its open would run on a camera actor's one thread (note
/// **N51**, and [`open_destination`] for the two flags).
///
/// It holds a descriptor rather than a name, so the `path` it is handed at write time is
/// used for the error message and nothing else — **that is the fix**, stated as a type: the
/// name was resolved once, before the camera opened, and nothing a client does afterwards
/// can redirect these bytes.
#[derive(Debug)]
struct OpenedAhead(Option<std::fs::File>);

impl engine::photo::Destination for OpenedAhead {
    fn write(&mut self, path: &Utf8Path, bytes: &[u8]) -> schema::Result<()> {
        // `None` means this request never had a destination — a `ReturnBytes` sink, which
        // `engine::photo::deliver` does not write and this arm therefore never sees. It is
        // written rather than assumed away for the reason every payload-carrying fallback in
        // this workspace is (AGENTS rule 6): a build that changed which sinks are written
        // must fail here rather than open a path this daemon never checked.
        let Some(file) = self.0.as_mut() else {
            return Err(Error::StorageIo {
                path: path.to_owned(),
                errno: None,
                message: "this request opened no destination before the camera was used".to_owned(),
            });
        };
        engine::photo::write_to_open_file(file, path, bytes)
    }
}

/// The one file a recording may write, opened before this daemon touched a camera.
///
/// [`OpenedAhead`]'s counterpart at the other seam, and it exists for the same reason: the
/// engine's `engine::record::Files` lets `webcam-handler-cli` open a path when the recording
/// starts, because a person typed it, and this daemon cannot — its `open(2)` would run on a
/// camera actor's one thread (note **N51**, and [`open_destination`] for the two flags that
/// make the open a refusal rather than a wait).
///
/// **The truncation is here, and it is not an implementation detail.** `engine::record::OnDisk`
/// truncates because `std::fs::File::create` does, and this seam has to mean the same thing on
/// both roots or a recording written over a longer file would leave the previous take's tail
/// past the end of the new one — bytes no reader of either container is looking for, in a file
/// whose name says it is a recording. It cannot happen at `open_destination`, which carries no
/// `O_TRUNC` on purpose: the length must not move until nothing can still refuse the request,
/// and `engine::record::Recording::begin` calls this at exactly that moment — after the
/// request's own predicates, after the negotiation and after D7's container pairing.
#[derive(Debug)]
struct OpenedAheadFile(Option<std::fs::File>);

impl engine::record::Files for OpenedAheadFile {
    fn create(&mut self, path: &Utf8Path) -> schema::Result<Box<dyn engine::record::WriteSeek>> {
        // `None` means this destination has already been handed over, which one recording
        // cannot do twice — `Recording::begin` calls this once. Written rather than assumed
        // away for AGENTS rule 6's reason: a build that called it twice must fail here rather
        // than record into a descriptor nobody opened.
        let Some(file) = self.0.take() else {
            return Err(Error::StorageIo {
                path: path.to_owned(),
                errno: None,
                message: "this recording's destination was already opened".to_owned(),
            });
        };
        file.set_len(0).map_err(|err| Error::StorageIo {
            path: path.to_owned(),
            errno: err.raw_os_error(),
            message: format!("the destination could not be truncated: {err}"),
        })?;
        Ok(Box::new(file))
    }
}

/// The photo answer, checked against itself before it is sent.
///
/// [`api::PhotoResponse::bytes_match_the_delivery`] is note **N34**'s other orphan predicate
/// and this is the consumer it named: "the daemon, which refuses a `PhotoResponse` it is about
/// to send that disagrees with itself (P4c)". `engine::photo::from_capture` produces the two
/// halves together — `returned` is `Some` exactly when the delivery is `Bytes` — so a
/// self-consistent answer is the ordinary case and this is the assertion that it stays one. A
/// truncated payload with an intact `byte_count` is what a client cannot distinguish from a
/// whole photo, and after P4f `webcam-handler-client` makes the same check from the other end,
/// which is what makes the pair worth two call sites.
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
                 because a request asked it to — stop `webcam-handler-daemon` itself, or \
                 wait for the idle \
                 close, rather than asking it to terminate itself"
                .to_owned(),
        });
    }
    Ok(())
}

#[async_trait]
impl WchRpcServer for Wchd {
    async fn list(&self) -> Result<CameraList, WireError> {
        // [`Inner::list_cameras`] is where the assembly and its argument live; this is the
        // wire's way in, and `Wchd::list_cameras` is the composition root's.
        Ok(self.offload(Inner::list_cameras).await?)
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
        // No lock, under either of D9's protocols. A `with_lock` here would not hang — `flock`
        // denies a second open file description in the same process and `StoreLock::take`
        // never blocks — it would answer this client `StoreLocked`, naming this daemon's own
        // pid and advising it to use `webcam-handler-client` against the daemon it is already
        // talking to. `crate::state` is where that trap is written down.
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
        // `kernel_release` reads a pseudo-file, and work that blocks and needs no camera goes
        // to the blocking pool. Both host facts have exactly one home —
        // `engine::profile::kernel_release` moved beside the field it fills when this verb
        // acquired a second author, and `schema::TOOL_VERSION` is the one reading of "which
        // build wrote this" — because `webcam-handler-cli profile capture` writes the same
        // document into the same corpus.
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
        // Three steps before the camera is touched, in an order where each one is the
        // cheapest thing that can still refuse.
        //
        // 1. The sink and the settle budget, which are questions about the *request* and
        //    need neither a filesystem nor an enumeration — so a `.webp`, a relative path or
        //    a deadline no capture may hold a camera for costs nothing and creates nothing
        //    (see `addressable`; the budget's rule is
        //    `schema::capture::SettlePolicy::within_bound`, beside the field it constrains).
        //    `engine::capture::grab` asks the budget question again at the door and that is
        //    the invariant; asking it here is what stops the refusal costing a caller-named
        //    zero-length file and a seat in this camera's actor queue (note **N147**).
        {
            let sink = request.sink.clone();
            let settle = request.settle;
            self.offload(move |_| {
                addressable(&sink)?;
                settle.within_bound()
            })
            .await?;
        }
        // 2. Which camera. Before the destination is opened rather than after, so a request
        //    naming a camera this host does not have leaves no file where none was.
        let info = self.resolve(camera).await?;
        // 2a. **A camera that is recording is `Busy`, and this is the one refusal in this verb
        //     that is about another client rather than about this request** (note **N118**).
        //
        //     Note N83's suspend/resume is what lets a photo interleave with a preview, and the
        //     premise it rests on is `engine::preview`'s: inside one actor exactly one thing can
        //     be streaming across commands, and that thing is a preview. P6c made that false —
        //     a take streams for its whole duration — and `engine::photo::take` cannot tell the
        //     two apart, because `Camera::streaming` answers about the *device* and the device
        //     does not know what the stream is for. So a photo taken during a take would stop
        //     the recording's stream, take its frame and start it again: frames the take never
        //     gets, and a gap in the frame timestamps that D7's close-time rewrite turns into a
        //     slower mean interval for the **whole** file. The notes' Expected usage item 10
        //     forbids exactly that ("it must not make a dropped frame look like a slow
        //     transition"), and a recording's whole product is the measurement it would spoil.
        //
        //     `Busy` because it means *retry* to an unattended reader and retrying is what
        //     succeeds — a take is bounded by its own duration — and with no holders for
        //     `crate::record`'s reason: the process holding that camera is this daemon.
        //     Refused *here* rather than in the engine because this is the layer that knows
        //     what a stream is for: `webcam-handler-cli` opens a camera per invocation and can
        //     never reach the case at all.
        //
        //     **This is the cheap half of the interlock and not the interlock** (note
        //     **N170**). Asked here, it is a check-then-act: the lock is released, the
        //     destination is opened on the blocking pool, and only then is the command
        //     enqueued — so a `record_start` that claims the camera inside that window still
        //     gets its `VIDIOC_STREAMON` in front of this photo. What this refusal buys is the
        //     ordinary case answered before anything is created: no descriptor, no
        //     zero-length file where none was, no seat in the camera's queue. The refusal
        //     that cannot be raced is the same question asked again from inside the actor
        //     command below, where the queue has already decided the order.
        self.0.recordings.not_recording(&info.id).await?;
        // 3. The destination, resolved from a name to a **descriptor** exactly once — which
        //    is note **N51**'s discharge: nothing a client does to the path afterwards can
        //    redirect these bytes, and no open on this path can wait. On the blocking pool,
        //    because `open(2)` is fast until the path is on a hung mount and this daemon
        //    parks no runtime worker.
        //
        //    A request that gets this far and then fails leaves a zero-length file where
        //    there was none — the same thing a failed capture leaves, and the price of
        //    resolving the name before the camera. What it never does is empty a file that
        //    was already there: `open_destination` carries no `O_TRUNC`, and the length is
        //    set once the bytes exist.
        let destination = {
            let sink = request.sink.clone();
            self.offload(move |_| match &sink {
                Sink::ServerPath { path } => open_destination(path, "photo").map(Some),
                // Nothing is opened for a request that asked for its bytes back, which is
                // why the destination is an `Option` rather than a second sink type.
                Sink::ReturnBytes { .. } => Ok(None),
            })
            .await?
        };
        let how = enqueueing(request.wait);
        let now = schema::time::Stamp::now();
        let camera = info.id.clone();
        let (info, taken) = self
            .on_resolved_camera_with_state_queueing(info, how, move |inner, device| {
                // 4. **And the same question again, where it cannot be raced** (note
                //    **N170**). `engine::actor` runs one command per camera at a time in
                //    arrival order, so by the time this line runs the queue has already
                //    decided whether this photo or a take's `VIDIOC_STREAMON` goes first: a
                //    take whose stream exists is in the registry, and a take whose stream does
                //    not exist yet cannot start it until this command returns. Step 2a is the
                //    same rule asked one layer up, where it costs nothing and creates nothing;
                //    this is the one that holds.
                //
                //    Before the device work rather than after, so a refused photo neither
                //    suspends the take's stream nor writes a byte to the destination this
                //    request already opened.
                inner.recordings.not_recording_on_this_thread(&camera)?;
                // Two clocks, because they measure different things and conflating them is
                // how an NTP step becomes a settle failure: `now` is the wall time that
                // goes in the EXIF, and `inner.clock` is the monotonic one the settle
                // policy runs on — this daemon's one reading of "what time is it", the
                // field `Inner::clock` exists to be.
                //
                // `Ok` of the whole attempt rather than the photo's own `Result`, and that
                // is what carries the interruption out on the path it matters most: a photo
                // that *failed* still stopped somebody's preview and still started it
                // again, and a closure returning `Err` here would drop that fact on the
                // floor (`engine::photo::Taken`'s doc argues the shape). The refusal is
                // unwrapped four lines down, unchanged and by the same `?`.
                Ok(engine::photo::take(
                    device,
                    &request,
                    &mut OpenedAhead(destination),
                    &inner.clock,
                    now,
                ))
            })
            .await?;
        // The gap, counted where the preview's other numbers live — before the photo's own
        // answer is looked at, so the count does not depend on whether the picture came out.
        if let Some(gap) = &taken.gap {
            self.0.previews.interrupted(&info.id, gap);
        }
        // Nothing here logs. The only facts this verb has that the answer does not already
        // carry are a path and a byte count, and a `tracing` call on the one code path in
        // this process that holds a frame is exactly the line `crate::logging` exists to
        // keep unwritten — "no frame, no photo payload, and nothing derived from one is
        // ever a field on an event". The one line the *interruption* is worth is
        // `Previews::interrupted`'s, which names a camera and a device refusal and never a
        // frame.
        Ok(photo_response(taken.outcome?)?)
    }

    // ------------------------------------------------------------------ P6c's three
    //
    // One verb on the command surface and three on the wire, and the arithmetic is
    // `engine::record`'s: a take is a chain of turns because the camera's actor takes one
    // command at a time, so the loop belongs to a driver and not to a method (note **N111**).
    // Everything about *which* answers each of the three gives — and every refusal below — is
    // argued in `crate::record`'s header, where the state lives; what is here is the routing.

    async fn record_start(
        &self,
        camera: CameraId,
        request: RecordRequest,
    ) -> Result<RecordStatus, WireError> {
        // Four steps before the camera is touched, in `wch_photo`'s order and for its reason:
        // each one is the cheapest thing that can still refuse.
        //
        // 1. The request's own three predicates, which need neither a filesystem nor an
        //    enumeration — a `ReturnBytes` sink (note **N110**), a relative path, an extension
        //    naming a container this build does not write, a duration past the cap. They live
        //    on `schema::video::RecordRequest` rather than here, beside the variants they
        //    constrain, so a request a socket built meets them as surely as one a command line
        //    built (note **N46**, debt D-1) — and `engine::record::start` asks them again, one
        //    layer down, which is one rule asked twice rather than two rules.
        //
        //    Inline rather than through `Wchd::offload`, and the difference from `addressable`
        //    one verb up is real: these are pure functions over a request that has already been
        //    deserialized, with no syscall behind them, so a pool thread would buy nothing and
        //    would put a hop between a malformed request and its refusal.
        let path = request.server_path()?.to_owned();
        let _container = request.container()?;
        let _budget = request.budget_ms()?;
        // 2. Which camera. Before the destination is opened rather than after, so a request
        //    naming a camera this host does not have leaves no file where none was.
        let info = self.resolve(camera).await?;

        // **And everything device-shaped from here runs on a task nothing outside can cancel**
        // (note **N177**; note **N169**'s rule, of which this is the third instance). A
        // jsonrpsee handler future is *dropped* when its client hangs up, and between
        // `engine::record::start` reaching `VIDIOC_STREAMON` and `Recordings::adopt` installing
        // a driver there is a stretch in which a drop would leave the camera **streaming with
        // nobody driving it**: the actor runs the command it was given whatever became of the
        // caller, and `Opened` is a value with no `Drop` and no owner left. The daemon would
        // then report that camera free — `Recordings::reap` gives its slot back, correctly, for
        // a `record_start` that has gone — while the *kernel* refused the next `record_start`
        // on it, which is verbatim the outcome `Recordings::claim` exists to prevent. It heals
        // only at `limits::CAMERA_IDLE_CLOSE_MS`.
        //
        // So the two halves of the claim — taking the stream and disposing of it — are in one
        // future, exactly as note N171 put `Previews::hand_over` on a task of its own. The
        // answer travels back over a `oneshot`, and **a failed `send` is the race-free fact
        // that nobody is listening**: at that point the take is real, running and nobody's, so
        // it is stopped and collected here rather than left for the idle sweep.
        let (answered, answer) = oneshot::channel::<schema::Result<RecordStatus>>();
        let starting = self.clone();
        tokio::spawn(async move {
            let outcome = starting.begin_recording(info, request, path).await;
            match answered.send(outcome) {
                Ok(()) => {}
                Err(Ok(started)) => {
                    // The client hung up while its take was being negotiated. The file is
                    // whole and where it was asked for — `Recordings::collect` closes the
                    // container — and what is discarded is this daemon's copy of the
                    // accounting, which is the same trade the module header's second bullet
                    // makes for an uncollected take.
                    tracing::warn!(
                        camera = %started.camera,
                        "a record_start's client went away after its take began; the take was \
                         stopped and its report discarded, and the file it wrote is still where \
                         it was asked for"
                    );
                    let _ = starting.0.recordings.collect(&started.camera).await;
                }
                Err(Err(refused)) => {
                    // Nothing was taken, so there is nothing to give back — but a refusal that
                    // reached nobody is still a fact about this daemon (AGENTS rule 3).
                    tracing::debug!(
                        error = %refused,
                        "a record_start was refused after its client had gone"
                    );
                }
            }
        });
        match answer.await {
            Ok(outcome) => Ok(outcome?),
            // The task above panicked, which is this process failing to compose itself — the
            // variant `Recordings::reserve` already uses for its own spawned half.
            Err(_) => Err(Error::DeviceIo {
                operation: "start a recording".to_owned(),
                errno: None,
                message: "the task driving this record_start did not finish".to_owned(),
            }
            .into()),
        }
    }

    async fn record_status(&self, camera: CameraId) -> Result<RecordStatus, WireError> {
        // A read, and the cheapest one on this surface: the resolution is the only work it
        // does. It opens no camera and takes no lock the driver wants, so a poll loop running
        // at `limits::CLIENT_RECORD_POLL_MS` costs the take nothing — which is the property
        // that makes D10's "progress by polling" affordable at all.
        //
        // The enumeration is not skipped for a camera this daemon is already recording, and
        // that is E2 rather than an oversight: a camera id is resolved against the machine
        // every time, so a status asked about an id that has stopped resolving is answered
        // about the machine as it is now rather than out of a map that remembers it.
        let camera = self.recording_camera(camera).await?;
        Ok(self.0.recordings.status(&camera).await)
    }

    async fn record_stop(&self, camera: CameraId) -> Result<RecordReport, WireError> {
        // Stop *and* collect, which `crate::record`'s header argues is one verb because it is
        // one question. Nothing here touches the camera: the driver owns the take and the
        // stop is a flag it reads between turns, so a stop does not queue behind the recording
        // it is ending — which is the whole reason a recording is a chain of turns (note
        // **N111**).
        let camera = self.recording_camera(camera).await?;
        Ok(self.0.recordings.collect(&camera).await?)
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
            // The schema crate's reading of one fact, not this binary's: `webcam-handler-cli
            // calibrate start` records provenance into the same documents.
            tool_version: schema::TOOL_VERSION.to_owned(),
        };
        Ok(self
            .on_resolved_camera_with_state(info, move |inner, device| {
                let now = schema::time::Stamp::now();
                let mut session = engine::lifecycle::create(&inner.store, &lock, &spec, now)?;
                // D3's empirical probe, at session start and nowhere else (N16). It *writes*
                // to the camera and puts it back, which is why this verb needs the device at
                // all — and why the `Discovery` it answers is dropped here: T5's
                // `calibrate_start` answers a `Session`, and `webcam-handler-cli` prints the
                // probe's other two facts on stderr, which a socket client cannot see.
                // `wch_discover_pairs` is the verb that hands them over (note N30).
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
            // refused because something else currently holds the device. Routing this through
            // the actor would compile, would pass every test that did not hold a camera, and
            // would break parity with `webcam-handler-cli` the day somebody reordered a queue
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
        // Before the camera is resolved, for `wch_photo`'s reason and one more: a sweep that
        // reached the actor would arm a snapshot and move a motor before its settle budget
        // was looked at (note **N147**). `engine::calibrate::run` asks this too, and that is
        // the invariant; this is what makes the refusal cost nothing.
        request.settle.within_bound()?;
        let info = self.resolve(camera).await?;
        let fingerprint = info.fingerprint.clone();
        let editing = self.editing_sessions().await;
        let lock = Arc::clone(&editing);
        // Built here rather than inside the closure, because the closure runs on the
        // actor's thread and the sink is a handle on state this daemon already owns —
        // constructing it there would be constructing it per sweep.
        let progress = ProgressBroadcast::new(Arc::clone(&self.0.events));
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
                    // **P4e-i's whole change on the emit side**, and it is one line
                    // because P3c built the seam for it: this used to be
                    // `engine::progress::Silent`, "a real sink rather than an `Option`, so
                    // the sweep emits into it and the events are dropped where a
                    // subscription will later be attached". This is that subscription.
                    //
                    // It emits from the **actor's own thread**, which is where a sweep
                    // runs, so the sink must not be able to wait or to fail —
                    // `crate::events::Fanout::emit` is both, and `engine::progress`'s
                    // header is where the reason is written down. What a client that is
                    // not there, or is behind, costs is `crate::events`'.
                    progress: &progress,
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

// ------------------------------------------------------------ the two things it says first
//
// `crate::events` is where every bound, every counter and every drop decision lives; these
// two bodies are the routing, and they are this short on purpose. What is *here* rather
// than there is the one thing that is a daemon fact rather than an event fact: the hotplug
// watch is started on the blocking pool, because binding a netlink socket and reading
// `/sys` is work that blocks and this daemon runs none of that on a runtime worker.

#[async_trait]
impl WchEventsServer for Wchd {
    async fn subscribe_events(
        &self,
        pending: jsonrpsee::PendingSubscriptionSink,
    ) -> jsonrpsee::core::SubscriptionResult {
        // Attached and started in one blocking closure, in that order, so that a watch
        // thread deciding to give up cannot race a subscriber into a stream nothing feeds —
        // `crate::events::Hotplug::attach` has the argument.
        //
        // A failure here is refused *before* the accept, which is the one moment a
        // subscription can still carry a JSON-RPC code: after it, jsonrpsee has only a
        // `subscription_error` notification, which carries no code at all. So a backend
        // that cannot watch is a D13 refusal like every other refusal on this surface,
        // rather than a stream that opens and immediately says something untyped.
        let events = Arc::clone(&self.0.events);
        let attached = match self
            .offload(move |inner| crate::events::attach_hotplug(&inner.events, &inner.cameras))
            .await
        {
            Ok(attached) => attached,
            Err(error) => {
                pending.reject(WireError(error)).await;
                return Ok(());
            }
        };
        crate::events::subscribe_hotplug(events, pending, attached).await
    }

    async fn subscribe_calibration(
        &self,
        pending: jsonrpsee::PendingSubscriptionSink,
    ) -> jsonrpsee::core::SubscriptionResult {
        // Nothing to start and nothing to refuse: the producer is this daemon's own sweeps,
        // which exist whether or not anybody is watching, and the events they emit while
        // nobody is are dropped and counted (`crate::events`' header).
        crate::events::subscribe_calibration(Arc::clone(&self.0.events), pending).await
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
            crate::shutdown::Shutdown::new(),
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

        fn streaming(&self) -> Option<schema::capture::NegotiatedStream> {
            self.inner().streaming()
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
            crate::shutdown::Shutdown::new(),
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
        // Not vacuous, and pinned at the number `crates/api` pins the *call* surface at:
        // two empty sets compare equal and would say nothing. Twenty-two is the trait's own
        // count since P6c closed D10's list with `record_start`, `record_status` and
        // `record_stop`; the two subscriptions are a second population with a second pin
        // (`routed_subscriptions`), because a subscription's second wire name comes from
        // jsonrpsee rather than from `#[method(name = …)]` (note N29's accounting).
        assert_eq!(routed.len(), 22, "{routed:?}");

        let subscribed: BTreeSet<&str> = routed_subscriptions().into_iter().collect();
        assert_eq!(
            subscribed.len(),
            2 * api::SUBSCRIPTIONS.len(),
            "{subscribed:?}"
        );
        assert!(
            subscribed.is_disjoint(&routed),
            "a subscription and a method share a wire name: {subscribed:?}"
        );

        // And the one that reads like a read verb but is not: measuring pairs writes to the
        // camera, so it landed with the mutating half rather than with P4b's six (N30).
        assert!(routed.contains("wch_discover_pairs"), "{routed:?}");

        // This assertion is half of a pair, and the other half is in
        // `tests/method_surface.rs`. docs/9's method-count walk proves every registered
        // method is *driven and answers*; it cannot tell an answer from a refusal that
        // arrived promptly, so a build where all nineteen answered a "not yet" refusal
        // would pass it. What this test proves is that there is no such half of the
        // surface at all. Neither is worth much without the other, which is why each says
        // so.
        //
        // The second half of this test's name has outlived the variant it names, and that
        // is deliberate rather than stale: P4d deleted `Error::Unimplemented` (note N6),
        // so "nothing answers it" went from a claim this partition carried to a shape the
        // registry cannot express. The name is kept because the claim it makes is still
        // true and is now stronger; what can still go red here is the equality above.
    }

    #[test]
    fn the_registration_the_method_count_walk_reads_is_the_one_this_module_pins() {
        // The join between the two halves above, in process: `tests/method_surface.rs`
        // compares its recording against `method_names()` off a real `Wchd`'s `into_rpc()`,
        // and this is the assertion that the value it reads is the surface `ROUTED` pins
        // rather than some other module. Without it, a second registration path — the thing
        // D10 exists to prevent — would leave both suites green about different populations.
        let (_backend, _temp, wchd) = daemon(limits::CAMERA_IDLE_CLOSE_MS);
        let methods = mount(wchd).expect("the two halves of one declaration cannot collide");
        let registered: BTreeSet<&str> = methods.method_names().collect();

        // Both pins, unioned: `mount` is where the two `into_rpc()` calls meet, so this is
        // also the assertion that the merge kept *all* of both halves — a merge that
        // dropped the unsubscribe callbacks would leave clients unable to close a stream
        // and nothing else in this crate would notice.
        let mut pinned: BTreeSet<&str> = ROUTED.iter().copied().collect();
        pinned.extend(routed_subscriptions());
        assert_eq!(
            registered, pinned,
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

        // And this check no longer reads the filesystem at all, which is the half of note
        // **N51**'s discharge that is an *absence*: a path that names a directory passes
        // here and is refused by `open_destination`, on the descriptor, where no client can
        // step between the question and the answer.
        addressable(&Sink::ServerPath {
            path: "/tmp/out.jpg".into(),
        })
        .expect("a path that does not exist is an ordinary destination");
    }

    #[test]
    fn a_destination_is_opened_from_its_descriptor_and_a_fifo_never_waits_for_a_reader() {
        // Note **N51**'s discharge, at the layer that does it. The integration suite drives
        // the same three shapes over the wire; this is where each refusal's *identity* is
        // asserted, because a check that answered one variant for everything would still
        // pass a suite that only asked whether the photo failed.
        let scratch = engine::paths::TempRuntimeDir::new().expect("a throw-away directory");

        // A directory. `EISDIR`, before anything is written.
        let directory = scratch.base().join("a-directory.jpg");
        std::fs::create_dir(&directory).expect("a writable scratch directory");
        let refused = open_destination(&directory, "photo").expect_err("a directory is not a file");
        assert_eq!(refused.kind(), ErrorKind::IllegalTransition);
        assert!(refused.to_string().contains("directory"), "{refused}");

        // A fifo with no reader — the wedge N51 measured. **This call returning at all is
        // the assertion**: without `O_NONBLOCK` it does not return, and this test becomes a
        // named nextest `TIMEOUT` rather than a wrong answer.
        let fifo = scratch.base().join("wedge.jpg");
        let made = std::process::Command::new("mkfifo")
            .arg(fifo.as_str())
            .status()
            .expect("mkfifo(1) runs");
        assert!(made.success(), "mkfifo {fifo}");
        let refused = open_destination(&fifo, "photo").expect_err("a fifo is not a file");
        assert_eq!(refused.kind(), ErrorKind::IllegalTransition);
        assert!(refused.to_string().contains("fifo"), "{refused}");

        // A missing parent directory is the *filesystem* declining, not the request being
        // unhonourable — a distinction E3 makes and a single refusal variant would lose.
        let nowhere = scratch.base().join("no-such-directory/shot.jpg");
        let refused = open_destination(&nowhere, "photo").expect_err("there is no such directory");
        assert_eq!(refused.kind(), ErrorKind::StorageIo);
        assert!(refused.to_string().contains("shot.jpg"), "{refused}");

        // And the ordinary destination, in both of its shapes: a path that does not exist
        // yet, and one that does. Neither is truncated by the open — the length is set when
        // the bytes exist — so a capture that fails afterwards leaves an operator's photo
        // where it was rather than emptying it on the way to reporting the failure.
        let fresh = scratch.base().join("fresh.jpg");
        drop(open_destination(&fresh, "photo").expect("a new regular file"));
        assert!(fresh.exists(), "{fresh}");
        let existing = scratch.base().join("existing.jpg");
        std::fs::write(&existing, b"an operator's photo").expect("a writable scratch directory");
        drop(open_destination(&existing, "photo").expect("an existing regular file"));
        assert_eq!(
            std::fs::read(&existing).expect("still there"),
            b"an operator's photo",
            "opening the destination emptied a file before anything was captured"
        );
    }

    #[test]
    fn d12s_wait_flag_chooses_between_the_two_enqueues_and_nothing_else() {
        // The flag both ways, at the one place this daemon reads it. What each answer
        // *does* is `engine::actor`'s and is asserted there against a queue that is provably
        // full; what is asserted here is that the field reaches the choice, in both
        // directions — a handler that had wired the flag backwards would take a photo either
        // way and no integration test could tell.
        assert_eq!(enqueueing(false), Enqueue::Refuse);

        let before = std::time::Instant::now();
        let Enqueue::WaitUntil(deadline) = enqueueing(true) else {
            panic!("`wait: true` chose the refusing enqueue");
        };
        // The budget is `engine::actor::Enqueue::waiting`'s, which reads
        // `limits::CAMERA_ENQUEUE_WAIT_MS` — asserted as a bound rather than transcribed, so
        // the constant keeps its single reader (rubric A8).
        assert!(deadline > before, "`wait: true` chose a spent deadline");
        assert!(
            deadline
                <= std::time::Instant::now()
                    + std::time::Duration::from_millis(limits::CAMERA_ENQUEUE_WAIT_MS),
            "the daemon widened the bound the constant states"
        );
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
        // D12 through the wire surface, which is where the claim actually matters: a running
        // `webcam-handler-daemon` must not hold a webcam somebody else wants. `list` is the
        // verb that proves the negative half — it answers from enumeration, so a daemon that
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
            crate::shutdown::Shutdown::new(),
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
        // The half of D12 that makes a running `webcam-handler-daemon` not hold somebody's
        // webcam, asserted through the thing that actually ships it. Every other idle-close
        // assertion in this workspace calls a sweep itself; this one calls none — it starts
        // the driver, moves the clock, and waits for the *descriptor* to go away.
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

    #[tokio::test(start_paused = true)]
    async fn the_idle_sweep_driver_ends_itself_when_the_daemon_is_asked_to_stop() {
        // P4e-ii's step 6, at the end this module owns. Until now this task ended only with
        // the runtime, and `main` dropped its handle — so "housekeeping stopped" was a
        // consequence of the process exiting rather than something the daemon did, and
        // nothing could wait for it.
        //
        // Two claims, and the second is the one that needed the token: the driver **ends**,
        // and the handle **resolves** — a detached task that merely stopped ticking would
        // satisfy an assertion about the first and hang on the second, which is the difference
        // between `crate::shutdown` joining it and hoping.
        //
        // Nothing sleeps. The clock is paused, so a driver that ignored the token would sit on
        // its cadence for ever and become a named nextest TIMEOUT rather than a green test —
        // `advance` below is what proves it was still running, and the join is what proves it
        // is not.
        let (_backend, _temp, wchd) = daemon(0);
        let driver = wchd.spawn_idle_sweeps_every(limits::CAMERA_IDLE_SWEEP_MS);

        // One tick first, so what is asserted below is a *running* driver being ended rather
        // than one that never started.
        tokio::time::advance(std::time::Duration::from_millis(
            limits::CAMERA_IDLE_SWEEP_MS,
        ))
        .await;

        wchd.shutdown().cancel();
        driver
            .await
            .expect("the idle-sweep driver ended without panicking");
    }

    #[tokio::test]
    async fn a_sweep_that_panicked_is_said_out_loud_and_the_next_one_still_closes_a_camera() {
        // Note **N174**, second half. The driver folded every `JoinError` into `break`, so a
        // pass that *panicked* ended D12's idle close for the life of the process — and said
        // nothing, which is the outcome rubric rule 3 exists to forbid: every camera this
        // daemon ever opened stays open, and the operator's other application is refused a
        // device with no line anywhere saying why.
        //
        // Both halves are asserted and neither implies the other: it is reported, and the
        // driver **carries on** — the second pass closes the idle camera, which is the
        // observable that could not exist in a build that broke out of the loop.
        //
        // **Nothing here is timed and nothing is polled.** What the test waits on is a
        // message the pass itself sends once it has swept, so a build that broke out of the
        // loop drops that sender and the wait ends in a named failure rather than in a hang.
        // The cadence is the subject's own timer rather than this test's synchronisation,
        // which is why it runs on the real clock: a paused one cannot advance itself while a
        // pass is on the blocking pool, and advancing it by hand would be this test choosing
        // the instant the thing under test is supposed to choose.
        const CADENCE_MS: Millis = 1;
        let (backend, closes, _temp, wchd) = announcing_daemon(0);
        let camera = only_camera(&backend);
        wchd.info(camera.clone()).await.expect("the fake opens");
        // Note N45's grace, spent here rather than by a pass, so that the pass after the
        // panicking one is the pass that closes.
        assert_eq!(wchd.sweep_idle_cameras(), Vec::new());

        let (ran, mut passes) = tokio::sync::mpsc::unbounded_channel();
        let (announced, logged) = crate::logging::captured(async {
            let driver = wchd.spawn_sweeps_every(CADENCE_MS, {
                let sweeper = wchd.clone();
                let first = Arc::new(std::sync::atomic::AtomicBool::new(true));
                move || {
                    if first.swap(false, std::sync::atomic::Ordering::SeqCst) {
                        panic!("a pass that met something it could not handle");
                    }
                    let closed = sweeper.sweep_idle_cameras();
                    // The failing send is a test that has stopped listening, which is not
                    // this pass's business.
                    let _ = ran.send(());
                    closed
                }
            });
            // The sender lives in the driver's own closure, so a build that broke out of the
            // loop ends this wait with `None` instead of leaving it to nextest's deadline.
            // Nothing waits for the close on that path, because there will not be one.
            let survived = passes.recv().await;
            let closed = match survived {
                Some(()) => tokio::task::spawn_blocking(move || closes.recv())
                    .await
                    .expect("the blocking pool is alive")
                    .ok(),
                None => None,
            };
            driver.abort();
            (survived, closed)
        })
        .await;

        let (survived, closed) = announced;
        assert!(
            survived.is_some(),
            "the driver ran no pass after one panicked: {logged:?}"
        );
        assert_eq!(
            closed.expect("a later pass closed the idle camera"),
            camera,
            "the driver stopped closing cameras after a pass panicked"
        );
        assert!(
            logged.contains("ERROR") && logged.contains("panicked"),
            "a panicking sweep disabled D12's idle close in silence: {logged:?}"
        );
    }

    // ------------------------------------------------- the photo/recording interlock (N170)

    /// A photo that settles immediately, into `path`.
    ///
    /// `SkipFrames { frames: 0 }` because nothing below is about settling: the subject is
    /// *when* the refusal is decided, and a policy that waited for frames would put the
    /// device work this test is ordering against behind a second one.
    fn a_photo(path: &camino::Utf8Path) -> PhotoRequest {
        PhotoRequest {
            stream: schema::capture::StreamRequest::default(),
            settle: schema::capture::SettlePolicy {
                spec: schema::capture::SettleSpec::SkipFrames { frames: 0 },
                deadline_ms: 5_000,
            },
            transform: schema::capture::Transform::None,
            sink: Sink::ServerPath {
                path: path.to_owned(),
            },
            wait: false,
        }
    }

    /// A take of `duration_ms` into `path`.
    fn a_recording(path: &camino::Utf8Path, duration_ms: u64) -> RecordRequest {
        RecordRequest {
            stream: schema::capture::StreamRequest::default(),
            duration_ms: Some(duration_ms),
            sink: Sink::ServerPath {
                path: path.to_owned(),
            },
            wait: false,
        }
    }

    /// Advance `future` by one poll, and answer what it said.
    ///
    /// The whole of how the test below constructs an interleaving instead of racing for one: a
    /// future that is not polled cannot advance, so "this request is parked between these two
    /// of its steps" becomes a statement the test makes rather than one it hopes for. Nothing
    /// here is timed.
    async fn one_poll<F: std::future::Future>(
        future: &mut std::pin::Pin<Box<F>>,
    ) -> std::task::Poll<F::Output> {
        std::future::poll_fn(|cx| std::task::Poll::Ready(future.as_mut().poll(cx))).await
    }

    /// Wait until **everything already handed to the blocking pool has finished**.
    ///
    /// The fence the test below is built on, and it works because that test's runtime is built
    /// with `max_blocking_threads(1)`: one thread taking queued closures in turn means a
    /// closure submitted now runs after every closure submitted before it, so awaiting an empty
    /// one is a barrier for the pool. It is not a sleep and not a duration — it ends when the
    /// pool reaches it.
    async fn pool_caught_up() {
        tokio::task::spawn_blocking(|| ())
            .await
            .expect("the blocking pool is alive");
    }

    #[test]
    fn a_photo_admitted_before_a_take_began_is_refused_when_its_command_reaches_the_camera() {
        // **Note N118's interlock, at the window note N170 found in it.** `wch_photo` asks
        // `Recordings::not_recording`, releases the registry lock, opens its destination on
        // the blocking pool and *then* enqueues its actor command — so a `record_start` that
        // claims the camera inside that window gets its `VIDIOC_STREAMON` in front of the
        // photo, and the photo's suspend/resume then stops **the take's** stream. The frames
        // in that gap are frames the take never gets, and D7's close-time rewrite spreads the
        // gap over the whole file as a slower mean interval: the one path in this build that
        // could corrupt a recording's measurement with nothing going red.
        //
        // The window is **constructed** rather than raced (docs/11 §4.10 raced for it twice and
        // missed), and **the construction is the runtime** (note **N178**). One poll of
        // `wch_photo` can cross both ends of the window — there is no `await` between the
        // blocking open resolving and `CameraActor::submit` — so a test that asked "does the
        // destination exist?" and then polled could be preempted between the two and poll a
        // future that was ready to run straight past it. That is not hypothetical: it failed
        // twice in ten workspace runs on a loaded host, saying the interlock had let a photo
        // through when what had happened was that the test never built the window.
        //
        // So the pool is given **one thread**, and every poll is followed by a fence. After
        // each fence, everything this photo handed the pool has finished and nothing is in
        // flight; the destination's existence is then read with no race under it, and the poll
        // that would submit the actor command is the one this test declines to make.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(1)
            .build()
            .expect("this host can start a tokio runtime");
        runtime.block_on(the_photo_interlock());
    }

    async fn the_photo_interlock() {
        let (backend, _temp, wchd) = daemon(limits::CAMERA_IDLE_CLOSE_MS);
        let camera = only_camera(&backend);
        let scratch = engine::paths::TempRuntimeDir::new().expect("a throw-away directory");
        let photo = scratch.base().join("during-a-take.jpg");
        let take = scratch.base().join("take.avi");

        let mut photographing = Box::pin(wchd.photo(camera.clone(), a_photo(&photo)));
        // Bounded by a count of *its own polls*, which is exact here for the reason note N178
        // gives: with the fence below, one poll is one step of this request and the number of
        // steps is a property of `wch_photo` rather than of the host. It takes three.
        let mut steps = 0;
        while !photo.exists() {
            assert!(
                one_poll(&mut photographing).await.is_pending(),
                "the photo answered before a take could start, so this test constructed \
                 no window"
            );
            pool_caught_up().await;
            steps += 1;
            assert!(steps < 64, "the photo never opened its destination");
        }
        // The window, and it is now a fact rather than a hope: the destination exists, so the
        // open has run; the photo has not been polled since, so it has submitted nothing; and
        // the pool is idle, so nothing is about to change either.
        assert_eq!(
            backend.streams_started(),
            0,
            "the photo's command reached the camera before this test could start a take, so the \
             window it constructs was missed"
        );

        // Inside the window: the check has passed and nothing has been enqueued.
        let started = wchd
            .record_start(camera.clone(), a_recording(&take, 60_000))
            .await
            .expect("a camera with no take on it");
        assert!(started.is_running(), "{started:?}");

        let refused = photographing
            .await
            .expect_err("a photo whose command lands during a take");
        assert_eq!(
            refused.0.kind(),
            ErrorKind::Busy,
            "the photo went ahead on a camera that was recording: {refused:?}"
        );
        // And the take is untouched, which is the half that says *why* the refusal matters:
        // nothing suspended its stream and nothing started a second one.
        assert_eq!(
            *wchd.watch_preview_interruptions().borrow(),
            0,
            "the refused photo suspended the take's stream anyway"
        );
        assert_eq!(
            backend.streams_started(),
            1,
            "the refused photo started a stream of its own"
        );

        // The other direction, so the refusal above is about a running take rather than about
        // photos: with the take collected, the same request answers a photograph.
        wchd.record_stop(camera.clone())
            .await
            .expect("the take this test started");
        wchd.photo(camera, a_photo(&photo))
            .await
            .expect("a camera whose take has been collected takes photographs");
    }

    // -------------------------------------------------- a client that hung up mid-start (N177)

    /// What one gated `VIDIOC_STREAMON` is waiting on, and what says it has begun.
    ///
    /// **Not a sleep and not a duration** — `tests/support/gated.rs` states the argument for the
    /// identical shape one directory along: the announcement on the way *in* is what turns "the
    /// device is inside `start_stream`" from a guess about scheduling into an observation, and
    /// the hold ends when another thread hands over a token.
    #[derive(Debug)]
    struct StreamGate {
        /// Only the **first** stream is held. A decorator that gated every one would hold the
        /// take this test starts *afterwards*, which is the half that proves the camera came
        /// back.
        armed: std::sync::atomic::AtomicBool,
        entered: std::sync::mpsc::SyncSender<()>,
        /// One token lets the held stream through. Buffered, so the test's send does not
        /// itself wait for the device.
        release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
        /// Every `STREAMOFF`, announced. This is the *device's* answer to "was this camera left
        /// streaming", which is the question — a daemon that has forgotten the take would
        /// answer it about itself.
        stopped: std::sync::Mutex<std::sync::mpsc::Sender<()>>,
    }

    /// The fake, held inside its first `VIDIOC_STREAMON`.
    #[derive(Debug)]
    struct Halting {
        inner: Arc<FakeBackend>,
        gate: Arc<StreamGate>,
    }

    /// One open camera, forwarding everything and asking the gate before it streams.
    #[derive(Debug)]
    struct Held {
        camera: Box<dyn schema::backend::Camera>,
        gate: Arc<StreamGate>,
    }

    impl CameraBackend for Halting {
        fn kind(&self) -> schema::backend::BackendKind {
            self.inner.kind()
        }

        fn enumerate(&self) -> schema::Result<Vec<CameraInfo>> {
            self.inner.enumerate()
        }

        fn open(&self, id: &CameraId) -> schema::Result<Box<dyn schema::backend::Camera>> {
            Ok(Box::new(Held {
                camera: self.inner.open(id)?,
                gate: Arc::clone(&self.gate),
            }))
        }

        fn watch(&self) -> schema::Result<Box<dyn schema::backend::HotplugWatch>> {
            self.inner.watch()
        }

        fn diagnose(&self) -> Vec<schema::report::ListHint> {
            self.inner.diagnose()
        }
    }

    impl schema::backend::Camera for Held {
        fn info(&self) -> &CameraInfo {
            self.camera.info()
        }

        fn formats(&self) -> schema::Result<Vec<schema::camera::FormatInfo>> {
            self.camera.formats()
        }

        fn controls(&self) -> schema::Result<Vec<ControlDesc>> {
            self.camera.controls()
        }

        fn get(
            &mut self,
            id: schema::control::ControlId,
        ) -> schema::Result<schema::control::ControlValue> {
            self.camera.get(id)
        }

        fn set(
            &mut self,
            id: schema::control::ControlId,
            value: schema::control::ControlValue,
        ) -> schema::Result<schema::control::Applied> {
            self.camera.set(id, value)
        }

        fn start_stream(
            &mut self,
            request: &schema::capture::StreamRequest,
        ) -> schema::Result<schema::capture::NegotiatedStream> {
            if self
                .gate
                .armed
                .swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                // Announce first, then wait: the test's next line depends on this camera being
                // *inside* the call it is about to reason about.
                let _ = self.gate.entered.send(());
                let _ = lock(&self.gate.release).recv();
            }
            self.camera.start_stream(request)
        }

        fn streaming(&self) -> Option<schema::capture::NegotiatedStream> {
            self.camera.streaming()
        }

        fn next_frame(
            &mut self,
            deadline: std::time::Instant,
        ) -> schema::Result<schema::capture::Frame> {
            self.camera.next_frame(deadline)
        }

        fn stop_stream(&mut self) -> schema::Result<()> {
            let outcome = self.camera.stop_stream();
            // After the device, so a test told "the stream stopped" is told it about the
            // fake's own state and not about this decorator's intent.
            let _ = lock(&self.gate.stopped).send(());
            outcome
        }
    }

    /// The two announcements and the one token, handed to a test as values.
    struct Gated {
        entered: std::sync::mpsc::Receiver<()>,
        release: std::sync::mpsc::SyncSender<()>,
        stopped: std::sync::mpsc::Receiver<()>,
    }

    /// A daemon whose first `VIDIOC_STREAMON` waits for this test.
    fn halting_daemon() -> (Arc<FakeBackend>, Gated, TempStore, Wchd) {
        let inner = Arc::new(
            FakeBackend::from_profile(testkit::fixtures::synthetic_basic())
                .expect("the synthetic profile is this build's version"),
        );
        // Buffered by one: an actor is one thread and can only be inside one `start_stream`.
        let (entered, entries) = std::sync::mpsc::sync_channel(1);
        let (release, releases) = std::sync::mpsc::sync_channel(1);
        let (stopped, stops) = std::sync::mpsc::channel();
        let backend = Arc::new(Halting {
            inner: Arc::clone(&inner),
            gate: Arc::new(StreamGate {
                armed: std::sync::atomic::AtomicBool::new(true),
                entered,
                release: std::sync::Mutex::new(releases),
                stopped: std::sync::Mutex::new(stopped),
            }),
        });
        let temp = TempStore::new().expect("a state directory");
        let wchd = Wchd::with_idle_timeout(
            backend as Arc<dyn CameraBackend>,
            SessionStore::new(temp.root()),
            lifetime_lock(&temp),
            crate::shutdown::Shutdown::new(),
            limits::CAMERA_IDLE_CLOSE_MS,
        );
        (
            inner,
            Gated {
                entered: entries,
                release,
                stopped: stops,
            },
            temp,
            wchd,
        )
    }

    /// Wait for one announcement from a gated backend, off the runtime's own threads.
    ///
    /// A blocking `recv` on the blocking pool rather than a loop that yields: what is being
    /// waited for is an OS thread's progress, and a task that spun for it would burn a core
    /// for as long as the wait lasted — which on a build with the repair missing is until
    /// nextest's deadline.
    async fn announced(waiting: std::sync::mpsc::Receiver<()>) {
        tokio::task::spawn_blocking(move || {
            waiting
                .recv()
                .expect("the gated backend outlives every announcement this test waits for");
        })
        .await
        .expect("the blocking pool is alive");
    }

    #[tokio::test]
    async fn a_record_start_whose_client_hung_up_leaves_no_camera_streaming() {
        // **Note N177.** A jsonrpsee handler future is *dropped* when its client hangs up, and
        // `record_start` holds three claims across its device work: the recording slot, the
        // camera's frames, and — from `engine::record::start` onwards — the device's own
        // `VIDIOC_STREAMON`. Note N171 gave the first two back with a witness the registry can
        // test. The third has no witness and no `Drop`: the actor runs the command it was given
        // whatever became of the caller, so a build that let the handler own that stretch left
        // the camera **streaming with nobody driving it** — while the daemon, correctly reaping
        // a reservation whose `record_start` had gone, reported that camera free. The next
        // `record_start` was then refused by the *kernel*, which is verbatim what
        // `Recordings::claim` exists to prevent.
        //
        // The window is **held open** rather than raced for: the backend announces from inside
        // `start_stream` and waits there, so "the client hung up after the device streamed on"
        // is a state this test puts the daemon in. What it then waits for is the device's own
        // `STREAMOFF` — a build with no repair never issues one, and nothing here spins while
        // that wait lasts.
        let (backend, gated, _temp, wchd) = halting_daemon();
        let camera = only_camera(&backend);
        let scratch = engine::paths::TempRuntimeDir::new().expect("a throw-away directory");
        let abandoned = scratch.base().join("hung-up.avi");
        let after = scratch.base().join("after.avi");
        let mut finished = wchd.0.recordings.watch_finished();

        let starting = tokio::spawn({
            let wchd = wchd.clone();
            let camera = camera.clone();
            let request = a_recording(&abandoned, 60_000);
            async move { wchd.record_start(camera, request).await }
        });
        announced(gated.entered).await;

        // The client hangs up. An abort is a *request* and the join is the fact (note N174's
        // words), so both are here.
        starting.abort();
        assert!(
            starting.await.is_err(),
            "the record_start future answered instead of being dropped, so this test cancelled \
             nothing"
        );
        gated
            .release
            .send(())
            .expect("the gated backend is still there");

        // **The property**, asked of the device rather than of the daemon.
        announced(gated.stopped).await;

        // The `STREAMOFF` is the first half of the driver's way out; the take is put down after
        // it, and that is what the slot's own count says. Awaited rather than polled, so the
        // assertion below is about the camera and not about this test's timing.
        finished
            .wait_for(|ended| *ended == 1)
            .await
            .expect("the registry outlives its own counter");

        // And the consumer-visible half: the camera takes a new recording, answered by this
        // daemon rather than refused by the kernel with an `EBUSY` about a stream nobody owns.
        let again = wchd
            .record_start(camera.clone(), a_recording(&after, 60_000))
            .await
            .expect("a camera whose abandoned take was stopped");
        assert!(again.is_running(), "{again:?}");
        assert_eq!(
            backend.streams_started(),
            2,
            "the second take did not reach the device, so its success says nothing"
        );
        wchd.record_stop(camera)
            .await
            .expect("the take this test started");
    }

    /// The fake, with the one thing a replayed profile cannot do: go away.
    ///
    /// A decorator rather than a fault, for `Announcing`'s reason: what it models is the
    /// *host*, not the device. An enumeration that stops listing a camera is what an unplug
    /// looks like to `engine::resolve`, and an already-open descriptor outliving it is what
    /// an unplug looks like to a take that is running — the two halves that make a take
    /// uncollectable through a verb that resolves first (note **N173**).
    #[derive(Debug)]
    struct Vanishing {
        inner: Arc<FakeBackend>,
        gone: Arc<std::sync::atomic::AtomicBool>,
    }

    impl CameraBackend for Vanishing {
        fn kind(&self) -> schema::backend::BackendKind {
            self.inner.kind()
        }

        fn enumerate(&self) -> schema::Result<Vec<CameraInfo>> {
            if self.gone.load(std::sync::atomic::Ordering::Acquire) {
                return Ok(Vec::new());
            }
            self.inner.enumerate()
        }

        fn open(&self, id: &CameraId) -> schema::Result<Box<dyn schema::backend::Camera>> {
            self.inner.open(id)
        }

        fn watch(&self) -> schema::Result<Box<dyn schema::backend::HotplugWatch>> {
            self.inner.watch()
        }

        fn diagnose(&self) -> Vec<schema::report::ListHint> {
            self.inner.diagnose()
        }
    }

    #[tokio::test]
    async fn a_take_whose_camera_was_unplugged_is_still_collected_by_the_name_it_started_with() {
        // Note **N173**. `record_stop` and `record_status` resolve against a live enumeration
        // first, so a camera unplugged mid-take made both of them answer `CameraUnknown` —
        // which D13 defines as *"a name that never resolved — distinct from `DeviceGone`, a
        // camera that was there"*. The take is still here: a file on disk, a driver ending,
        // and an ending waiting to be handed over. An unattended agent told `CameraUnknown`
        // reads it as *your id is wrong* and retries with another name; what it needs is the
        // refusal sitting in the registry with its own name on it (AGENTS rule 7).
        let inner = Arc::new(
            FakeBackend::from_profile(testkit::fixtures::synthetic_basic())
                .expect("the synthetic profile is this build's version"),
        );
        let gone = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let temp = TempStore::new().expect("a state directory");
        let wchd = Wchd::new(
            Arc::new(Vanishing {
                inner: Arc::clone(&inner),
                gone: Arc::clone(&gone),
            }) as Arc<dyn CameraBackend>,
            SessionStore::new(temp.root()),
            lifetime_lock(&temp),
            crate::shutdown::Shutdown::new(),
        );
        let camera = only_camera(&inner);
        let scratch = engine::paths::TempRuntimeDir::new().expect("a throw-away directory");
        let path = scratch.base().join("unplugged.avi");

        wchd.record_start(camera.clone(), a_recording(&path, 60_000))
            .await
            .expect("a camera with no take on it");

        gone.store(true, std::sync::atomic::Ordering::Release);

        let status = wchd
            .record_status(camera.clone())
            .await
            .expect("a take on a camera that has been unplugged is still a take");
        assert!(
            status.take.is_some(),
            "the take stopped being reported the moment its camera stopped enumerating"
        );
        let report = wchd
            .record_stop(camera.clone())
            .await
            .expect("the take this test started");
        assert_eq!(report.path, path);

        // And the other direction, which is what keeps this from being "record_stop stopped
        // resolving": with the take collected, this daemon holds nothing for that name, and a
        // name that names nothing is `CameraUnknown` exactly as D13 says.
        let refused = wchd
            .record_status(camera.clone())
            .await
            .expect_err("this daemon holds nothing for that name any more");
        assert_eq!(refused.0.kind(), ErrorKind::CameraUnknown);
        let never = CameraId::parse("cam:nothing-here").expect("a literal id");
        assert_eq!(
            wchd.record_stop(never)
                .await
                .expect_err("a name this host never had")
                .0
                .kind(),
            ErrorKind::CameraUnknown
        );
    }
}
