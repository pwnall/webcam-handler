//! Bounded everything (rubric A14).
//!
//! Every loop over device behavior has a deadline or a cap, and every cap lives here.
//! A constant nobody reads is a defect (rubric A8), so this module grows with the code
//! that consumes it rather than ahead of it.
//!
//! Design §2.10 also makes this module the home of **path layouts**, so D9's session
//! tree is spelled once: `webcam-handler-engine::store` composes the names below into
//! paths, and the CLI and the daemon read the same constants rather than repeating the
//! strings.

/// The persisted session document version (design D9). Bumped when the on-disk shape
/// changes; a foreign version is [`crate::Error::SchemaVersionForeign`], never a
/// best-effort parse.
pub const SESSION_SCHEMA_VERSION: u32 = 1;

/// The session tree's root inside the state directory (design D9):
/// `<state dir>/sessions/<fingerprint-slug>/<task-slug>/<uuidv7>/`.
pub const SESSIONS_DIR: &str = "sessions";

/// The session document inside a session directory (design D9).
pub const SESSION_FILE: &str = "session.json";

/// The append-only event log inside a session directory (design D9).
pub const SESSION_LOG_FILE: &str = "log.ndjson";

/// The photo tree inside a session directory: `photos/<control-slug>/` (design D9).
pub const SESSION_PHOTOS_DIR: &str = "photos";

/// The one advisory lock at the state directory's root (design D9).
///
/// A plain name rather than a dotfile: D9's whole posture is that the state directory is
/// inspectable, and an operator wondering who owns it should see the file that answers.
pub const STORE_LOCK_FILE: &str = "lock";

/// The daemon's Unix socket inside the runtime directory (design D11):
/// `<XDG runtime dir>/webcam-handler/wchd.sock`.
///
/// Here rather than in the daemon because `wchc` has to resolve the same path to connect
/// (P4f), and two string literals is the drift this module exists to prevent. The
/// *directory* is the security boundary — `connect(2)` checks search permission on every
/// component and write permission on the socket inode, and the socket file itself is
/// created with `0777 & ~umask` — so the 0700 mode D11 names belongs to the directory,
/// which is where `webcam-handler-daemon::uds` asserts it.
pub const DAEMON_SOCKET_FILE: &str = "wchd.sock";

/// The longest Unix socket path the kernel will bind.
///
/// `sockaddr_un::sun_path` is 108 bytes on Linux and the path inside it is
/// NUL-terminated, so 107 bytes is the real bound. It is checked before `bind` rather
/// than inferred from `ENAMETOOLONG` afterwards, because the path is composed from
/// `$XDG_RUNTIME_DIR` and a refusal that names the limit is the difference between "the
/// daemon is broken" and "your `$TMPDIR` is too deep" — which is exactly what a test run
/// under a scratch `$TMPDIR` (`scripts/mutants.sh` exports one) would otherwise look
/// like.
pub const MAX_UNIX_SOCKET_PATH_BYTES: usize = 107;

/// The kernel's accept queue depth for the daemon's Unix socket.
///
/// Ours because the socket is ours: `tokio::net::UnixListener::bind` chose 1024 for us
/// until note N39's hardening made the daemon create, `bind` and `listen` the descriptor
/// itself, and an inherited number behind a socket this project owns is exactly what
/// AGENTS' "bounded everything" is about. 1024 was Linux's `SOMAXCONN` answer to a public
/// TCP service; this is a per-user daemon behind a 0700 directory, and the queue only has
/// to absorb the clients that arrive between two turns of the accept loop.
///
/// It is deliberately larger than [`DAEMON_MAX_CONNECTIONS`]: the backlog bounds callers
/// *waiting to be accepted* and the cap bounds callers *being served*, so equalising them
/// would make a client that connects while the daemon is at its cap meet an `ECONNREFUSED`
/// from the kernel instead of the accept loop's own bounded queue.
pub const DAEMON_LISTEN_BACKLOG: i32 = 64;

// Checked where both numbers are, in `CAMERA_IDLE_SWEEP_MS`'s shape, because the paragraph
// above is an argument about the *pair* and nothing else reads them together: tune
// `DAEMON_MAX_CONNECTIONS` up to jsonrpsee's own default of 100, or trim the backlog to
// "bound it harder", and everything still compiles while a client connecting at the cap
// meets the kernel's `ECONNREFUSED` instead of the accept loop's queue — the one outcome
// the ordering exists to prevent. The `as` is safe by construction: the left conjunct
// establishes the value is positive, so the widening cannot change it.
const _: () =
    assert!(DAEMON_LISTEN_BACKLOG > 0 && DAEMON_LISTEN_BACKLOG as u32 > DAEMON_MAX_CONNECTIONS);

/// The most connections the daemon serves at once.
///
/// jsonrpsee's own default is 100 and this is not it: a per-user daemon behind a 0700
/// directory has one `wchc` invocation, maybe a browser tab, and whatever a script is
/// doing. The cap exists so a client that leaks connections is refused rather than able
/// to exhaust the daemon's file descriptors, which on this process are also the camera's.
pub const DAEMON_MAX_CONNECTIONS: u32 = 32;

/// The largest JSON-RPC request body the daemon reads.
///
/// Requests on this surface are small: a control id, a handful of writes, a sweep
/// request, a sink path. A megabyte is three orders of magnitude of headroom and still
/// bounds what a caller can make the daemon buffer before it has decided anything.
pub const RPC_MAX_REQUEST_BYTES: u32 = 1024 * 1024;

/// The largest JSON-RPC response body the daemon writes.
///
/// jsonrpsee's default is 10 MB and a photo does not fit in it: D10 answers a capture as
/// base64 in the JSON result, a 4K MJPG frame off the OBSBOT is ~8 MB \[PF:9\], and
/// base64 is four bytes per three. Set here, from the bound the picture actually has,
/// rather than left to be discovered as a wire failure by the sub-milestone that routes
/// `wch_photo`.
pub const RPC_MAX_RESPONSE_BYTES: u32 = 64 * 1024 * 1024;

/// The most calls one JSON-RPC batch may carry.
///
/// jsonrpsee defaults to unlimited, which is an unbounded loop over caller-supplied input
/// on the one socket the daemon always serves. Nothing this project ships batches — `wch`
/// and `wchc` run one verb per invocation — so the bound is small on purpose: it keeps
/// the protocol feature available without making it a lever.
pub const RPC_MAX_BATCH: u32 = 16;

/// How long `wchc` waits for an ordinary verb's answer.
///
/// jsonrpsee's client default is sixty seconds, which is somebody else's number for
/// somebody else's deployment — and here it is not even the right *shape*, because the
/// longest ordinary verb on this surface is not a network round trip but a camera one.
/// Priced from the three bounds that compose it: a `wch_photo` that asked to wait
/// ([`CAMERA_ENQUEUE_WAIT_MS`], ten seconds) may then settle
/// ([`DEFAULT_SETTLE_DEADLINE_MS`], five) and then wait for one frame
/// ([`FRAME_DEADLINE_MS`], two). Everything else on the surface is shorter than that, and
/// the sweep — the one verb that is not — has [`CLIENT_SWEEP_REQUEST_TIMEOUT_MS`].
///
/// Two minutes leaves an order of magnitude of headroom over that seventeen seconds, which
/// is deliberate: this bound exists so a client that has lost its daemon eventually says so
/// rather than hanging forever, not so it can adjudicate whether a camera is slow. A
/// timeout that fired while the daemon was still working would be this client inventing a
/// failure the daemon had not had, which is E3's conversion wearing a stopwatch.
///
/// Read by `wchc`'s `Remote::request_timeout`, once per invocation.
pub const CLIENT_REQUEST_TIMEOUT_MS: u64 = 120_000;

// The relation the paragraph above argues, checked where all four numbers are: the ordinary
// budget has to outlast the worst ordinary command, or `wchc photo --wait` would time out on
// a daemon that was about to answer it.
const _: () = assert!(
    CLIENT_REQUEST_TIMEOUT_MS
        > CAMERA_ENQUEUE_WAIT_MS + DEFAULT_SETTLE_DEADLINE_MS + FRAME_DEADLINE_MS
);

/// How long `wchc` waits for a calibration sweep's answer.
///
/// Its own number because a sweep is the one method on this surface "whose latency is
/// unbounded by design" (`webcam-handler-api`'s `wch_calibrate_sweep`, which asks a client
/// to raise or disable its timeout rather than leave it at jsonrpsee's default). Raised
/// rather than disabled: AGENTS bounds everything, and "no timeout at all" is the shape a
/// wedged client takes when nobody is left to notice.
///
/// Priced from the sweep's own caps rather than from patience: [`MAX_SWEEP_SAMPLES`] is the
/// most samples one sweep visits and each costs at most a settle
/// ([`DEFAULT_SETTLE_DEADLINE_MS`]) plus a frame ([`FRAME_DEADLINE_MS`]), so the worst sweep
/// this build will run is a little under half an hour. An hour is that with room for the
/// writes and the per-sample scoring the arithmetic leaves out.
///
/// Read by `wchc`'s `Remote::request_timeout`, for the one verb that needs it.
pub const CLIENT_SWEEP_REQUEST_TIMEOUT_MS: u64 = 3_600_000;

// The arithmetic above, where the numbers are. A budget shorter than the sweep the planner
// will happily run would make `wchc calibrate sweep --all` fail on exactly the sweeps that
// took the longest to get wrong — and it would fail *after* the camera had been driven.
const _: () = assert!(
    CLIENT_SWEEP_REQUEST_TIMEOUT_MS
        > MAX_SWEEP_SAMPLES as u64 * (DEFAULT_SETTLE_DEADLINE_MS + FRAME_DEADLINE_MS)
);

/// How many requests `wchc` may have in flight on its one connection.
///
/// jsonrpsee sizes the channel to its background task from this and defaults it to 256,
/// which is a server-shaped number: `wchc` runs **one verb per invocation**, so what is
/// actually in flight is a call, at most one subscribe beside it (the sweep, note N57), and
/// the unsubscribe a dropped `Subscription` sends. Four is that with one to spare.
///
/// Small on purpose rather than generous: this is the one place a client could buffer work
/// the daemon has not agreed to yet, and a bound that could never be reached would not be a
/// bound. Read by `wchc`'s `Remote::connect`.
pub const CLIENT_MAX_CONCURRENT_REQUESTS: usize = 4;

/// How many undelivered events `wchc` holds for one subscription.
///
/// jsonrpsee's client **closes** a subscription whose buffer overflows, so this is not a
/// drop policy like [`WS_MESSAGE_BUFFER_CAPACITY`] one hop upstream — it is the depth at
/// which this client would end its own progress stream. Deeper than the daemon's
/// per-connection buffer for that reason: the hop that refuses has to be the one that
/// *counts* what it refused and carries on (`wch_subscribe_calibration`'s policy), never
/// this one, which cannot say anything about what it lost.
///
/// Equal to [`SUBSCRIPTION_BROADCAST_DEPTH`] because the two bound the same thing from two
/// ends — how far behind a subscriber may fall — and a client that buffered less than the
/// daemon's fan-out would make the client the first to fail on a burst the daemon was
/// built to absorb. Read by `wchc`'s `Remote::connect`.
pub const CLIENT_SUBSCRIPTION_BUFFER: usize = 256;

// The ordering the paragraph above argues, checked where both numbers are.
const _: () = assert!(CLIENT_SUBSCRIPTION_BUFFER > WS_MESSAGE_BUFFER_CAPACITY as usize);

/// How many unwritten notifications one subscription may hold before the daemon drops.
///
/// jsonrpsee's `message_buffer_capacity`, which P4b deliberately did not inherit: it turned
/// the WebSocket surface off entirely rather than ship "an unbounded, untested transport
/// for a consumer that does not exist yet" (note **N38**). P4e-i is that consumer, so the
/// number arrives with it.
///
/// It bounds the **last** hop — between the daemon's own subscription task and the socket's
/// writer — and what happens at it is the law `engine::progress::ChannelSink` already
/// states one layer in: drop, and count. Blocking there would let a subscriber that stopped
/// reading hold a task per stream; growing would let it decide how much memory the daemon
/// uses. Neither is available to a daemon whose whole story is that nothing a client does
/// can wedge it.
///
/// Sixty-four is chosen from what one subscriber can fall behind by and still be worth
/// serving, and the arithmetic is stated in samples because that is the unit with one
/// meaning: [`MAX_SWEEP_SAMPLES`] is 256 and a sweep emits two events per sample, so this
/// is a whole *thirty-two samples* of the longest sweep this build will run — an eighth of
/// its events. A client further behind than that is not rendering a progress bar, and every
/// in-flight progress variant carries `index`/`total` so its next event repaints a correct
/// one anyway. A hotplug burst is smaller still: one `uvcvideo` cycle on this desk was ten
/// nodes \[PF:21\].
///
/// Read by `daemon::uds::serve`, which sets it on every connection, and **read back** by the
/// daemon's subscription suite off a real WebSocket subscription's own sink
/// (`SubscriptionSink::max_capacity`, published as `daemon::events::StreamActivity::buffer`)
/// — because a bound this project sets and nothing reads is a bound that silently becomes
/// jsonrpsee's default again (rubric A8, note **N38**). The suite drives a subscriber past
/// it and counts what was dropped on the in-memory dispatch, where the connection buffer is
/// the only queue in the path.
pub const WS_MESSAGE_BUFFER_CAPACITY: u32 = 64;

// jsonrpsee's `set_message_buffer_capacity` **panics** on zero
// (`ServerConfigBuilder`: `assert!(c > 0)`), and a panic on the daemon's startup path is
// not an available failure mode. Checked here, in `CAMERA_IDLE_SWEEP_MS`'s tradition,
// because the number and the thing that refuses it are one screen apart nowhere else.
const _: () = assert!(WS_MESSAGE_BUFFER_CAPACITY > 0);

/// The most subscriptions one connection may hold open at once.
///
/// The second of the two numbers note **N38** says P4e owns. It is per *connection* rather
/// than per daemon, which is jsonrpsee's shape and the right one: the resource a
/// subscription costs is a task and a buffer on the connection that asked for it, and
/// [`DAEMON_MAX_CONNECTIONS`] already bounds how many connections there are — so the two
/// multiply to a bound on the whole daemon.
///
/// Eight, because this surface has **two** subscriptions and a client has no reason to open
/// either of them twice: the events are broadcast and a second receiver on one connection
/// sees exactly what the first does. The headroom is for a client that re-subscribes after
/// a lag close (`wch_subscribe_events` ends a stream that fell too far behind) without
/// having noticed its previous stream is gone. A client past the bound is refused the
/// *subscribe call* — jsonrpsee answers `-32006` before the handler runs — rather than
/// disconnected, so connect-and-abandon costs it its own slots and nobody else's.
///
/// Read by `daemon::uds::serve` and driven to its refusal, over a real WebSocket, by the
/// daemon's subscription suite.
pub const RPC_MAX_SUBSCRIPTIONS_PER_CONNECTION: u32 = 8;

/// How many events the daemon's fan-out holds for a subscriber that has fallen behind.
///
/// One event source, N subscribers (design D10's two subscriptions; P5c's browser client is
/// the N). The hop this bounds is the **first** one — between the thread or actor producing
/// events and each subscription's own task — and it exists because the producer must never
/// be able to wait on a consumer: a hotplug watch that blocked would stop reading the
/// kernel's socket, and a sweep that blocked would hold a camera at one value because a
/// progress bar went away, which is exactly what [`PROGRESS_QUEUE_DEPTH`] refuses one crate
/// down.
///
/// **One number for both streams, deliberately.** What it bounds is not a property of
/// either producer but of a *subscriber*: how far behind the slowest one may fall before it
/// starts losing events. Two constants would be two answers to one question, and the two
/// producers are already bounded by their own numbers — [`MAX_SWEEP_SAMPLES`] and a hotplug
/// burst's node count.
///
/// Deliberately larger than [`WS_MESSAGE_BUFFER_CAPACITY`]: this queue is shared by every
/// subscriber and that one is per subscription, so a fan-out bound at or below the
/// per-connection bound would make the shared queue the thing that always fails first, and
/// a slow subscriber would cost the others their events rather than only its own.
///
/// Read by `daemon::events`, once per stream.
pub const SUBSCRIPTION_BROADCAST_DEPTH: usize = 256;

// The relation the paragraph above argues, checked where both numbers are: a shared fan-out
// no deeper than one connection's private buffer would make one slow subscriber the reason
// another one lagged, which is the property the fan-out exists to provide.
const _: () = assert!(SUBSCRIPTION_BROADCAST_DEPTH > WS_MESSAGE_BUFFER_CAPACITY as usize);

/// How long the daemon's hotplug thread waits in one `HotplugWatch::next_event` call.
///
/// Not a poll interval and not a cadence: `next_event` blocks in `poll(2)` on the uevent
/// socket for the whole of it and answers the moment a burst settles, so this is how often
/// the loop gets a turn to notice it should stop rather than how often the tree is read.
/// A deadline that arrives first is `Ok(None)`, which is an answer and not an error (E3),
/// so the cost of a quiet second is one loop iteration.
///
/// One second, chosen from the only thing it trades: a longer deadline makes the loop's
/// turn rarer and a shorter one spends turns on a socket that is usually silent. It is
/// **not** a bound on how quickly an event is delivered — the socket wakes the poll — which
/// is why nothing here is sized against a human's patience.
///
/// Read by `daemon::events`, which is the only loop that owns a watch, and **driven** by the
/// daemon's subscription suite: `a_hotplug_watch_runs_only_while_somebody_is_listening`
/// drops the last subscriber and waits for the watch thread to publish that it has left,
/// which it can only do on the turn this deadline gives it. That wait is bounded by this
/// number and by nothing else, so a build that changed it changes what the suite waits for —
/// which is what makes this a bound rather than a number (rubric A8).
pub const HOTPLUG_WATCH_DEADLINE_MS: u64 = 1_000;

/// How many `accept` calls may fail in a row before the daemon stops accepting.
///
/// An accept error is usually about one client (`ECONNABORTED`: it hung up between
/// `connect` and `accept`), so retrying is right. But `EMFILE` is about *us* and does not
/// clear on its own, and a loop that retries it immediately is a spin at 100% of a core
/// that no log level makes obvious. Consecutive failures are what distinguishes the two,
/// and the daemon gives up rather than spinning.
pub const MAX_CONSECUTIVE_ACCEPT_FAILURES: u32 = 64;

/// How long a stop waits for the work the daemon had already accepted.
///
/// The bound on the *drain*, which is the one part of stopping that waits for anybody: by
/// the time it starts, the accept loop is ending and every open subscription has already
/// been told why it is over, so what is left is the requests that were in flight when the
/// signal arrived. AGENTS lists "shutdown drains" among the things that get a number here
/// precisely because the alternative — waiting for whatever is running — is unbounded, and
/// what is running may be a calibration sweep.
///
/// **A sweep will usually reach this bound rather than end inside it, and that is the case
/// the number is chosen for.** `daemon::server`'s `Inner::sessions` is held for the whole of
/// `wch_calibrate_sweep`, which is minutes of camera time on real hardware, so a stop asked
/// for mid-sweep does not get a tidy finish; it gets this long and then stops anyway, saying
/// so at `warn` (rubric rule 3 — never a silence). What makes that survivable rather than a
/// hole is D9's persisted pre-sweep snapshot: an interrupted sweep is recoverable from the
/// session document the sweep has been writing all along, which is the mechanism
/// `crates/engine/tests/crash_recovery.rs` pins and AGENTS rule 8 requires.
///
/// Twenty seconds, chosen against the one number outside this project that can overrule it:
/// systemd's default `TimeoutStopSec` is 90 s, after which the service manager sends
/// `SIGKILL`. A daemon whose own drain outlasted that would never reach its ordered release —
/// the store lock, the sockets, the descriptors — because the kernel would perform the
/// disorderly one first, and every sentence this project writes about *how* it stops would be
/// describing a path that never runs. Being under it by a factor is what makes this bound the
/// one that fires. It is deliberately far larger than a request's own budgets
/// ([`CAMERA_ENQUEUE_WAIT_MS`], [`DEFAULT_SETTLE_DEADLINE_MS`]) so that a stop arriving during
/// ordinary work waits for that work rather than cutting it in half.
///
/// Read by `daemon::shutdown`'s drain, which is the only thing that waits during a stop.
pub const DAEMON_SHUTDOWN_DRAIN_MS: u64 = 20_000;

/// How much of the service manager's watchdog interval the daemon spends before pinging it.
///
/// systemd hands a `Type=notify` service `$WATCHDOG_USEC` and then kills it if no
/// `WATCHDOG=1` arrives inside that window. Pinging *at* the interval is therefore a race the
/// daemon loses on the first scheduling hiccup, and `sd_watchdog_enabled(3)` says so in as
/// many words: send "in shorter intervals than the specified timeout, e.g. half". Two is that
/// half, and it is the divisor rather than the halved number because the interval is the
/// operator's — it arrives from the unit file at run time, so what belongs in this module is
/// the *relation* between their number and ours.
///
/// **What a ping proves is smaller than it looks, and the honest size of it is stated where
/// the number is.** `daemon::systemd`'s watchdog task is a tokio task that pings on a timer,
/// so a ping means the runtime is still scheduling — it does not mean a camera answers, that
/// the socket accepts, or that any request has ever completed. A watchdog that health-checked
/// a device would have to open one, and D12's whole posture is that `wchd` running does not
/// itself hold a camera; an availability check that took the camera would be the one failure
/// mode it exists to prevent (AGENTS rule 7 — availability is not capability).
///
/// Read by `daemon::systemd::ping_interval`, which is the only thing that divides by it.
pub const WATCHDOG_PING_DIVISOR: u32 = 2;

// A divisor of zero is a panic in `Duration::checked_div`'s caller and a divisor of one is
// the race the paragraph above is about — the daemon would ping exactly as its budget ran
// out. Checked where the number is, in `CAMERA_IDLE_SWEEP_MS`'s tradition, because nothing
// else reads it and a compile failure is the one red nothing can skip.
const _: () = assert!(WATCHDOG_PING_DIVISOR > 1);

/// How long an open camera may go unused before the next idle sweep closes it.
///
/// D12's "the daemon never opens a camera until first use and closes on idle
/// (configurable), so `wchd` running does not itself block other applications from the
/// webcam" — this is the *default* that makes "configurable" have something to configure
/// (`engine::actor::Cameras::with_idle_timeout` is the override).
///
/// Thirty seconds is chosen from the two failures it sits between. Too short and every
/// `wchc get` in a shell loop pays a fresh `open` and the driver's first-frame settle
/// \[PF:11\]; too long and a person who ran one command cannot start a video call without
/// stopping the daemon, which is precisely the complaint D12 exists to answer. A human
/// working at a terminal issues their next command inside thirty seconds, and has stopped
/// caring about the camera long before they switch applications.
///
/// **Measured from when a command *starts*, plus one sweep cadence** (note N45). The actor
/// reads no clock, so the stamp is taken on the way in and this timeout runs from there. For
/// a command shorter than the timeout that is the same thing as measuring from the end, and
/// for a command *longer* than it — `wch_calibrate_sweep` is minutes of camera time — the
/// deadline is already long past when the command returns. `engine::actor` grants exactly
/// one extra pass in that case: `engine::actor`'s `Live::used_since_sweep` makes the first
/// idle pass after a completed command decline, and the next one closes.
///
/// So the honest accounting is two sentences rather than one. A **short** command's camera
/// closes thirty to thirty-five seconds after it — the passes between the command and its
/// deadline spend the grace, so the first pass at or past the deadline closes. A **long**
/// one's closes one to two [`CAMERA_IDLE_SWEEP_MS`] cadences after it returns — five to ten
/// seconds, not thirty — because a grace of one *pass* is not a second timeout. That is
/// deliberate and it is what N45's clock-free shape buys: the failure the note was opened
/// for is "an immediate close on the very next pass", and what a long command gets instead
/// is a whole cadence in which a client that is still working can issue its next verb. A
/// client that pauses longer than that pays a fresh `open` and the driver's first-frame
/// settle \[PF:11\], exactly as one that pauses thirty seconds after a short command does;
/// buying more would mean giving the actor a clock, which reverses `engine::actor`'s central
/// decision and needs a `SteppedClock` that is deliberately not `Sync`.
/// `engine::actor`'s own suite is where both halves are pinned — a short command still
/// closing on the first pass at its deadline, a long one declining once and closing on the
/// pass after — driven from these two constants rather than from a literal.
pub const CAMERA_IDLE_CLOSE_MS: u64 = 30_000;

/// How often the daemon asks every open camera whether it has gone idle.
///
/// [`CAMERA_IDLE_CLOSE_MS`] is the deadline and this is the thing that checks it: an idle
/// close nobody asks about never happens, and the pass is what turns
/// `engine::actor::Idle`'s two numbers into a descriptor going away.
///
/// Deliberately shorter than the timeout, and by a whole factor rather than a hair. Since
/// `engine::actor::Idle::expired` reaches its deadline with `>=`, the first pass at or
/// after the deadline is the one that closes: a camera therefore closes **within one
/// cadence of its deadline**, not "some multiple of thirty". For a command shorter than
/// [`CAMERA_IDLE_CLOSE_MS`] that is thirty to thirty-five seconds after it. For one longer
/// than the timeout it is *this* cadence that measures the wait rather than the timeout —
/// see [`CAMERA_IDLE_CLOSE_MS`], where the two cases are priced; the number here is what
/// N45's grace is worth, which is why the two sentences live one screen apart.
///
/// The cost is one mutex read per *actor* every five seconds, plus one acknowledgement per
/// camera that is actually about to close. Per actor rather than per open camera, because
/// nothing is removed from the registry except a dead thread — a machine whose eight
/// cameras have each been used once and closed keeps eight entries — and per *actor* is
/// also the accounting P4d's reaping will be argued against. It is a mutex and not a queue
/// round trip because `engine::actor::CameraActor::sweep` answers from the published state
/// unless the camera is open, unused and quiescent.
pub const CAMERA_IDLE_SWEEP_MS: u64 = 5_000;

// The relation between the two, checked where both numbers are rather than asserted from a
// distance. A cadence at least as long as the timeout would leave a camera open for up to
// cadence-plus-timeout while the documentation above promises thirty seconds, and a zero
// cadence is a spin — `tokio::time::interval` refuses one by panicking, which on a daemon's
// startup path is not an available failure mode. A compile failure is the one red nothing
// can skip.
const _: () = assert!(CAMERA_IDLE_SWEEP_MS > 0 && CAMERA_IDLE_SWEEP_MS < CAMERA_IDLE_CLOSE_MS);

/// How many commands one camera's actor queues before refusing with
/// [`crate::Error::Busy`].
///
/// A bound, not a buffer. One camera is one blocking thread (D12), so everything past the
/// command being executed is already waiting on a device that can only do one thing at a
/// time, and a deeper queue buys a caller nothing but a longer wait before the same
/// answer. Eight is enough that a browser tab's poll and a `wchc` invocation arriving
/// together never collide; a caller past it is told the camera is busy — which is the
/// refusal D12 names — rather than made to wait for a queue nobody bounded.
pub const CAMERA_COMMAND_QUEUE_DEPTH: usize = 8;

/// How long a request that asked to *wait* for a place in that queue is willing to wait.
///
/// D12's sentence has two halves — "a second capture request queues **or** is refused with
/// `Busy` per its `wait` flag" — and this is the bound the waiting half needs. AGENTS
/// requires one of anything that waits, and the reason is [`CAMERA_COMMAND_QUEUE_DEPTH`]'s
/// one layer out: a wait nobody bounded is the same wedge as a queue nobody bounded, moved
/// from the daemon's memory to the caller's patience.
///
/// Ten seconds is chosen from what it sits between, and both ends are numbers in this
/// module rather than intuitions. The command in front can take a whole
/// [`DEFAULT_SETTLE_DEADLINE_MS`] plus a [`FRAME_DEADLINE_MS`] before it lets go — seven
/// seconds at its own worst case — so anything shorter would refuse a caller who was
/// waiting for exactly the one thing waiting can help with. And a *sweep* is minutes of
/// camera time (`MAX_SWEEP_SAMPLES` photos), so nothing in this range could outlast one:
/// `wait: true` behind a calibration is still `Error::Busy`, which is the honest answer
/// rather than a request that never comes back.
///
/// What it does **not** promise is a place at the back of a full queue: eight commands each
/// taking their own worst case would need far longer than this. The flag buys the caller
/// the wait for *room*, not the wait for the whole queue — and the refusal it takes when the
/// budget is spent is the same [`crate::Error::Busy`] a caller that never asked to wait
/// takes immediately, so the flag changes when the answer arrives and never what it is.
///
/// Read by `engine::actor::Enqueue::waiting`, which is the shipped deadline the daemon's
/// `wch_photo` hands to `CameraActor::submit_with`; a test that wants to talk about a
/// particular instant hands in its own.
pub const CAMERA_ENQUEUE_WAIT_MS: u64 = 10_000;

// Checked where all three numbers are, in `CAMERA_IDLE_SWEEP_MS`'s tradition, because the
// paragraph above is an argument about the *relation*: a wait budget shorter than the
// worst case of the command in front of it would spend the flag's whole purpose, and
// nothing else in this workspace reads these three together.
const _: () = assert!(CAMERA_ENQUEUE_WAIT_MS > DEFAULT_SETTLE_DEADLINE_MS + FRAME_DEADLINE_MS);

/// How many requests may be *parked* waiting for a place in a camera's queue at once.
///
/// [`CAMERA_ENQUEUE_WAIT_MS`] bounds how long one waiter waits; this bounds how many there
/// are, and without it the first bound is worth nothing. A wait for a place in the command
/// queue parks a thread by construction — the queue is a `std::sync::mpsc::SyncSender` and
/// the wake-up is a `Condvar` (note **N56**) — so in the daemon each waiter holds one of
/// tokio's blocking-pool threads, and *every other* blocking thing the daemon does
/// (enumeration, the session tree, opening a photo's destination) draws on the same pool.
/// Unbounded waiters are therefore an unbounded queue in front of every verb, which is
/// exactly what this build's story says a client cannot cause.
///
/// **It is [`DAEMON_MAX_CONNECTIONS`] because that is the number the transport used to
/// enforce and stopped.** Before the WebSocket surface, one connection answered one request
/// at a time, so the parked-thread count could not exceed the connection count; jsonrpsee's
/// WebSocket transport spawns a task per inbound message and caps nothing per connection
/// (measured against jsonrpsee-server 0.26.0, note **N56**'s amendment), so one connection
/// can hold thousands in flight. Making the permit pool exactly the old number keeps the
/// arithmetic that used to be true, and makes it true by construction rather than by an
/// accident of which transport a client chose.
///
/// A request past it is **not** made to wait for a permit — waiting for permission to wait
/// is a second unbounded queue. It takes the enqueue a caller that never asked to wait
/// takes: served if there is room right now, [`crate::Error::Busy`] if there is not. So the
/// flag degrades to its own `false` under load, and the refusal vocabulary does not grow.
///
/// Read by `daemon::server`'s permit pool, and driven past its bound — over one WebSocket
/// connection, behind a camera an in-flight sweep is provably holding — by the daemon's
/// subscription suite.
pub const CAMERA_ENQUEUE_WAITERS: u32 = 32;

// The paragraph above is an argument about a *relation* between two numbers, so it is
// checked where both of them are rather than believed.
const _: () = assert!(CAMERA_ENQUEUE_WAITERS == DAEMON_MAX_CONNECTIONS);

/// The device-profile document version (design T3).
pub const PROFILE_SCHEMA_VERSION: u32 = 1;

/// Frames to discard before a photo, by default.
///
/// Frames immediately after `STREAMON` are dark and miscolored until AE and AWB
/// converge \[PF:11\]; ten was enough on both seed cameras \[PF:9\].
pub const DEFAULT_SETTLE_SKIP_FRAMES: u32 = 10;

/// How long a settle policy may wait before giving up with
/// [`crate::Error::SettleTimeout`].
pub const DEFAULT_SETTLE_DEADLINE_MS: u64 = 5_000;

/// How long a single `DQBUF` may block before the deadline logic gets a turn.
pub const FRAME_DEADLINE_MS: u64 = 2_000;

/// The most times a settle loop may go round before giving up.
///
/// The **deadline** is the real bound; this is the backstop for the one case the deadline
/// cannot cover — a round that neither consumes a frame nor advances the clock. That
/// happens when a backend returns "no frame" without waiting *and* the clock is not
/// moving, which is a test's `SteppedClock` today and would be a wedged monotonic clock in
/// the field. Either way, spinning is the one outcome that helps nobody: the loop gives up
/// and reports the settle timeout it would eventually have reported anyway.
///
/// Generous by design. At 30 fps a `SettleFor` policy running the full
/// [`DEFAULT_SETTLE_DEADLINE_MS`] sees ~150 frames, so this leaves an order of magnitude
/// before a legitimate settle could reach it.
pub const MAX_SETTLE_ROUNDS: u32 = 4_096;

/// Buffers to request from the driver for a capture stream.
pub const DEFAULT_BUFFER_COUNT: u32 = 4;

/// The most processes a `Busy` refusal names.
///
/// The walk that finds them reads the whole process table, and a refusal listing four
/// hundred processes is less readable than one listing none. Four is enough to say "these
/// have it" and short enough to read in a terminal.
pub const MAX_HOLDERS_REPORTED: usize = 4;

/// How long `terminate_holder` waits before answering whether the node is still held.
///
/// `SIGTERM` is a **request**, and it is asynchronous: `kill(2)` returns once the signal is
/// queued, not once the target has acted on it. So a re-check taken immediately would
/// answer `still_held: true` for almost every process that is about to exit, and a field
/// that is nearly always `true` is a field nobody reads —
/// [`crate::report::TerminationReport::still_held`] exists precisely so a caller can tell
/// "signalled, device now free" from "signalled, still held" without guessing.
///
/// **This is the one place the daemon waits to learn something, and it is bounded because
/// it cannot be event-driven.** A process this one did not fork leaves no event a Unix
/// process can wait on without `pidfd_open(2)`, which this workspace does not link, so the
/// honest mechanism is a bounded poll: walk, and if the node is still held, wait
/// [`TERMINATE_RECHECK_POLL_MS`] and walk again until this budget is spent. It ends the
/// moment the fact changes, so the usual cost is one poll interval rather than this number.
///
/// A quarter of a second is chosen from what it sits between: shorter and a process that
/// exits promptly is still reported as holding the node, which is the failure this constant
/// exists to prevent; longer and a caller who asked to free a camera is left waiting on a
/// process that has plainly decided to ignore the signal, which `still_held: true` is the
/// answer for.
pub const TERMINATE_RECHECK_MS: u64 = 250;

/// How often `terminate_holder` re-reads the holder walk inside
/// [`TERMINATE_RECHECK_MS`].
///
/// Each poll is a walk of the process table, so this bounds the work as well as the wait:
/// one walk immediately and one after each of the ten waits this budget buys — **eleven
/// walks**, not a spin. The number is arithmetic over these two constants rather than a
/// third one, and `daemon::server::recheck_walks` is where it is computed and asserted, so
/// this sentence cannot drift away from the loop. Twenty-five milliseconds is well under the
/// scheduling latency a signalled process takes to die, so the common case still costs one
/// walk and no wait at all.
pub const TERMINATE_RECHECK_POLL_MS: u64 = 25;

// Checked where both numbers are, for `CAMERA_IDLE_SWEEP_MS`'s reason: a poll interval at
// least as long as the budget would make the "bounded poll" one immediate walk with the
// wait after it, and a zero interval is a spin against `/proc`.
const _: () =
    assert!(TERMINATE_RECHECK_POLL_MS > 0 && TERMINATE_RECHECK_POLL_MS < TERMINATE_RECHECK_MS);

/// The most buffers one stream may ask the driver to allocate.
///
/// `buffer_count` arrives from a caller (a CLI flag, an RPC field), and every buffer is a
/// driver-side allocation of a whole frame — 8 MB each at 4K MJPG on the OBSBOT. A request
/// for a thousand is not a caller who wants smooth capture, and the driver would be the
/// one to run out of memory.
pub const MAX_BUFFERS_PER_STREAM: u32 = 32;

/// The most samples one sweep may take.
///
/// A sweep of a 468000-wide pan range at step 3600 would otherwise plan 260 photos and
/// a great deal of motor travel.
pub const MAX_SWEEP_SAMPLES: u32 = 256;

/// The most samples a sweep that *moves motors* may take (design §5: motors wear).
pub const MAX_MOTION_SWEEP_SAMPLES: u32 = 32;

/// How many calibration progress events a channel sink queues for a consumer that has
/// stopped reading.
///
/// Bounded because the alternatives are both worse than dropping: an unbounded queue lets
/// a stalled subscriber grow the sweep's memory without limit, and a blocking send lets it
/// *stop the sweep* — a camera held at one value because a progress bar went away. The
/// events past this bound are dropped and counted, never silently lost (rubric rule 3).
pub const PROGRESS_QUEUE_DEPTH: usize = 256;

/// The largest control payload we will read back from a device.
///
/// `elem_size × elems` is device-supplied, and a lying driver is attacker-shaped input
/// (rubric B10): the product is checked against this before anything is allocated.
pub const MAX_CONTROL_PAYLOAD_BYTES: usize = 64 * 1024;

/// The most menu indices `VIDIOC_QUERYMENU` is asked about for one control.
///
/// Menus are sparse and enumeration walks `min..=max` tolerating holes \[PF:2\]; a driver
/// reporting a menu range of `0..=i64::MAX` must not become an infinite loop.
pub const MAX_MENU_INDICES: u32 = 4_096;

/// The most device nodes one camera group may contain before we stop believing the
/// grouping. Real groups hold two \[PF:7\].
pub const MAX_NODES_PER_CAMERA: usize = 16;

/// The most controls one device may enumerate.
///
/// `QUERY_EXT_CTRL` with `NEXT_CTRL` walks in strictly increasing id order and ends with
/// `EINVAL`, so a well-behaved driver terminates on its own. A driver that never says
/// `EINVAL` would otherwise walk the whole 32-bit id space; the seed cameras expose 18
/// and 24.
pub const MAX_CONTROLS_PER_DEVICE: u32 = 1_024;

/// The most pixel formats one capture node may enumerate. The seed cameras offer two.
pub const MAX_FORMATS_PER_NODE: u32 = 128;

/// The most frame sizes one pixel format may enumerate. The Chicony's MJPG offers nine.
pub const MAX_FRAME_SIZES_PER_FORMAT: u32 = 256;

/// The most frame intervals one size may enumerate. The OBSBOT's 1080p offers ten.
pub const MAX_FRAME_INTERVALS_PER_SIZE: u32 = 256;

/// The buffer one uevent netlink datagram is read into, in bytes.
///
/// The kernel builds every uevent inside its own `UEVENT_BUFFER_SIZE` (2048 bytes) and
/// broadcasts it as one datagram, so this is four times the largest packet the sender can
/// emit. It is a bound rather than a guess: a datagram longer than the buffer is reported
/// by `MSG_TRUNC` and **dropped whole** rather than parsed from a prefix, because half a
/// packet is a different thing from a packet and must not be spelled the same way (rubric
/// B10 — a packet off a kernel socket is attacker-shaped input).
///
/// Read by `webcam-handler-v4l2`'s uevent socket, which is also where the oversize
/// refusal is asserted in both directions.
pub const UEVENT_PACKET_BYTES: usize = 8 * 1024;

/// The most uevent datagrams one *drain* takes off the socket before it hands control back
/// to the loop above it — one pass, **not** one `HotplugWatch::next_event` call.
///
/// One `uvcvideo` cycle broadcast 56 packets on this host, three times running \[PF:21\],
/// so one pass reads a whole cycle. What is left over is not lost: it stays queued on the
/// socket, which is readable again immediately.
///
/// **What bounds one `next_event` is the caller's deadline, not this.** That is worth
/// stating because the opposite is the natural reading of a constant with this name.
/// `next_event` loops — drain, decide what changed, wait — until it has an event or the
/// deadline arrives, so a broadcast storm from a neighbouring subsystem is read for as long
/// as the caller was willing to wait. That is the behaviour the watch wants rather than an
/// omission: datagrams nobody reads are datagrams the kernel eventually throws away, which
/// arrives as `ENOBUFS` and costs a trigger, so draining a storm is work the watch has to
/// do. The bound that makes it safe is the deadline the trait already declares (rubric
/// A14), and the deadline is asserted in both directions where `next_event` lives.
///
/// Read by `webcam-handler-v4l2`'s hotplug watch: its flood test drives more packets than
/// this through one drain and asserts the drain stopped here, and a second arm drives the
/// same flood through `next_event` and asserts every packet was read and the deadline was
/// still honoured.
pub const MAX_UEVENTS_PER_DRAIN: u32 = 64;

/// How quiet the uevent socket must go before a burst of triggers is treated as finished,
/// in milliseconds.
///
/// **PF:19 is why this exists**: one physical camera owns two capture nodes (the Dell owns
/// four), so one plug is several uevents and re-enumerating per packet would be several
/// scans of a tree that is still moving. What this window actually has to bridge is the gap
/// between the nodes of *one camera*, and measured on this host \[PF:21\] that was at most
/// **30.2 ms**. 250 ms clears it many times over.
///
/// **It does not try to separate a driver cycle's removes from its adds, because no number
/// does.** The same captures put the largest gap *inside* the remove phase at **93.6 ms**
/// (the Dell goes, then the USB3 devices ~94 ms later) and the pause from the last remove
/// to the first add at **98.5 ms** in one cycle and 119.0 ms in another — five milliseconds
/// apart in the worse case, and all of them `modprobe` timings rather than bounds. A window
/// picked between them splits the remove phase in two; a window above them coalesces a
/// whole cycle into one reading whose diff is empty. Note N53 has the table and the
/// reasoning; what ends a burst early is the turn rule below, not this number.
///
/// It is **not** load-bearing for correctness, and that is deliberate: the watch reports the
/// difference between successive readings of the node tree, so a window that is too short
/// costs extra readings and a window that is too long coalesces more of a burst — neither
/// can invent an event or leave the watch out of step with the tree. What keeps a *driver
/// cycle* from coalescing into "nothing happened" is not this number but the turn rule in
/// `webcam-handler-v4l2`'s debounce: a trigger that reverses the burst's direction ends it
/// immediately.
pub const HOTPLUG_QUIET_MS: u64 = 250;

/// The longest a hotplug re-reading may be deferred by a trigger stream that never goes
/// quiet, in milliseconds.
///
/// [`HOTPLUG_QUIET_MS`] alone is a hang wearing a debounce costume: a failing hub or a
/// flaky cable produces an add/remove stream with no pause in it, and a quiet-only rule
/// would defer the reading forever. This is the ceiling that makes the deferral bounded
/// (rubric A14) — the reading happens this long after the *first* trigger of a burst
/// however busy the socket stays.
pub const HOTPLUG_MAX_DEFERRAL_MS: u64 = 2_000;

// Checked where both numbers are, for `CAMERA_IDLE_SWEEP_MS`'s reason: a ceiling at or
// below the quiet window would fire first every time, which would make the quiet window
// unreachable code rather than a policy.
const _: () = assert!(HOTPLUG_QUIET_MS > 0 && HOTPLUG_QUIET_MS < HOTPLUG_MAX_DEFERRAL_MS);
