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
/// Here rather than in the daemon because `webcam-handler-client` has to resolve the same path
/// to connect (P4f), and two string literals is the drift this module exists to prevent. The
/// *directory* is the security boundary — `connect(2)` checks search permission on every
/// component and write permission on the socket inode, and the socket file itself is created
/// with `0777 & ~umask` — so the 0700 mode D11 names belongs to the directory, which is where
/// `webcam-handler-daemon::uds` asserts it.
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
/// directory has one `webcam-handler-client` invocation, maybe a browser tab, and whatever a
/// script is doing. The cap exists so a client that leaks connections is refused rather than
/// able to exhaust the daemon's file descriptors, which on this process are also the camera's.
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
/// jsonrpsee defaults to unlimited, which is an unbounded loop over caller-supplied input on
/// the one socket the daemon always serves. Nothing this project ships batches —
/// `webcam-handler-cli` and `webcam-handler-client` run one verb per invocation — so the bound
/// is small on purpose: it keeps the protocol feature available without making it a lever.
pub const RPC_MAX_BATCH: u32 = 16;

/// How long `webcam-handler-client` waits for an ordinary verb's answer.
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
/// Read by `webcam-handler-client`'s `Remote::request_timeout`, once per invocation.
pub const CLIENT_REQUEST_TIMEOUT_MS: u64 = 120_000;

// The relation the paragraph above argues, checked where all four numbers are: the ordinary
// budget has to outlast the worst ordinary command, or `webcam-handler-client photo --wait`
// would time out on a daemon that was about to answer it.
const _: () = assert!(
    CLIENT_REQUEST_TIMEOUT_MS
        > CAMERA_ENQUEUE_WAIT_MS + DEFAULT_SETTLE_DEADLINE_MS + FRAME_DEADLINE_MS
);

/// How long `webcam-handler-client` waits for a calibration sweep's answer.
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
/// Read by `webcam-handler-client`'s `Remote::request_timeout`, for the one verb that needs
/// it.
pub const CLIENT_SWEEP_REQUEST_TIMEOUT_MS: u64 = 3_600_000;

// The arithmetic above, where the numbers are. A budget shorter than the sweep the planner
// will happily run would make `webcam-handler-client calibrate sweep --all` fail on exactly
// the sweeps that took the longest to get wrong — and it would fail *after* the camera had
// been driven.
const _: () = assert!(
    CLIENT_SWEEP_REQUEST_TIMEOUT_MS
        > MAX_SWEEP_SAMPLES as u64 * (DEFAULT_SETTLE_DEADLINE_MS + FRAME_DEADLINE_MS)
);

/// How many requests `webcam-handler-client` may have in flight on its one connection.
///
/// jsonrpsee sizes the channel to its background task from this and defaults it to 256, which
/// is a server-shaped number: `webcam-handler-client` runs **one verb per invocation**, so
/// what is actually in flight is a call, at most one subscribe beside it (the sweep, note
/// N57), and the unsubscribe a dropped `Subscription` sends. Four is that with one to spare.
///
/// Small on purpose rather than generous: this is the one place a client could buffer work
/// the daemon has not agreed to yet, and a bound that could never be reached would not be a
/// bound. Read by `webcam-handler-client`'s `Remote::connect`.
pub const CLIENT_MAX_CONCURRENT_REQUESTS: usize = 4;

/// How many undelivered events `webcam-handler-client` holds for one subscription.
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
/// built to absorb. Read by `webcam-handler-client`'s `Remote::connect`.
pub const CLIENT_SUBSCRIPTION_BUFFER: usize = 256;

// The ordering the paragraph above argues, checked where both numbers are.
const _: () = assert!(CLIENT_SUBSCRIPTION_BUFFER > WS_MESSAGE_BUFFER_CAPACITY as usize);

/// How long `webcam-handler-client` keeps reading a sweep's progress after the sweep itself
/// has answered.
///
/// The bound on the **tail**, and the tail exists because a sweep's answer and its last
/// progress event leave the daemon on two different tasks — `wch_calibrate_sweep`'s method
/// call, and the forward task `daemon::events` runs per subscription — and reach the one
/// connection's writer in whichever order that daemon's runtime scheduled them. A client
/// that stopped reading the instant its call answered would abandon a `SweepFinished` that
/// lost that race by microseconds, and that event is the one `cli_core`'s `Bar` prints a
/// sweep's closing line from and the only one it cannot repaint from a later one. Note
/// **N69** has the measurement and the failure rate it came from.
///
/// **It is a bound and not a wait**, which is the whole of why it is not the thing note
/// **N65** refused. The drain ends the instant this sweep's terminal event arrives — 34 µs
/// after the answer, measured on the runs where the answer won — so this number is only ever
/// reached when no terminal event is coming at all, which `wch_subscribe_calibration` is
/// explicitly allowed to arrange (it drops and counts, note **N57**). Then it costs this
/// once, at the end of a sweep that took camera-minutes, and the bar stops one event short
/// exactly as it did before the drain existed. What stays refused is waiting *without* a
/// bound: a dropped terminal event would hang the client for ever.
///
/// A quarter of a second is chosen from the delay it has to cover, which is one already-woken
/// task waiting for a core — no device, no disk, and nothing that can be slow for a reason of
/// its own. Measured at 34 µs; a woken thread on a host oversubscribed eightfold waits
/// milliseconds, not hundreds of them. The ceiling is [`HOTPLUG_QUIET_MS`]'s, and the two
/// numbers answer the same question for the same reader even though the mechanisms below
/// them are unrelated: how long may this tool pause for something that has almost certainly
/// already happened, before the pause is the thing a person notices? A quarter of a second
/// is this project's answer, given once.
///
/// Read by `webcam-handler-client`'s `Remote::calibrate_sweep`, through
/// `remote::SWEEP_DRAIN_BUDGET` — the one place the number becomes a `Duration` — and read *as
/// a duration that has to be able to expire* by the test named
/// `the_tail_waits_the_moment_out_and_the_budget_is_what_pays_for_it`, which delivers a
/// sweep's terminal event a hundred milliseconds behind its answer on a paused clock and
/// asserts the bar still gets it. That test exists because this number spent a day being read
/// by nothing that could go red: every tail test passed `Duration::ZERO`, so **zero passed the
/// entire workspace suite** — the fix note **N69** landed, deleted, with its code left in
/// place (note **N70**, finding F2).
pub const CLIENT_SWEEP_DRAIN_MS: u64 = 250;

// The floor, and it is mechanical rather than a matter of taste: a tail that cannot wait
// collects nothing. `Remote::calibrate_sweep`'s own doc records the measurement — this
// client's queue is provably empty when its call answers (note N65), so the event a tail
// exists for is one that has not arrived, and only waiting can collect it. Zero is therefore
// not a smaller bound but a deleted one, which is exactly the edit nothing could see.
const _: () = assert!(CLIENT_SWEEP_DRAIN_MS > 0);

// The ceiling, related to the number the paragraph above actually derives it from.
//
// What stood here was `CLIENT_SWEEP_DRAIN_MS < FRAME_DEADLINE_MS` under a comment claiming "a
// tail must cost less than the smallest piece of work the sweep it follows does".
// `FRAME_DEADLINE_MS` is two seconds and it is an *upper bound on waiting for one frame*, not
// the cost of one — a real sample at a real camera is about 500 ms [E13] — so the assert
// admitted every value up to 1999 while the sentence above it described a bound of a
// different order. An assert whose comment misdescribes it is worse than no assert, because
// it reads as a checked relation (note **N70**, finding F2).
//
// `HOTPLUG_QUIET_MS` is what the doc above derives this number from, and equality is what
// holds today: both are the longest this project is willing to pause for something that has
// already happened. `<=` rather than `==` because the derivation is a ceiling and not an
// identity — a tail shorter than the debounce is still a tail, and the day 34 µs is
// re-measured on faster hardware this number may fall on its own. Checked where both numbers
// are, in `CAMERA_ENQUEUE_WAITERS == DAEMON_MAX_CONNECTIONS`' tradition.
const _: () = assert!(CLIENT_SWEEP_DRAIN_MS <= HOTPLUG_QUIET_MS);

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

/// How long the composition root waits for the **web listener** to finish stopping.
///
/// [`DAEMON_SHUTDOWN_DRAIN_MS`]'s smaller sibling, and it exists because of a case P5b
/// measured rather than predicted: **an open MJPEG tab whose reader has stopped reading can
/// make a graceful shutdown wait forever, and no amount of cancelling the stream fixes it.**
///
/// The mechanism is worth writing down, because the obvious mitigation is the one that does
/// not work. `axum::serve`'s graceful shutdown waits for a response that is being written;
/// design §2.6 says "an open MJPEG tab must not hang shutdown", so `daemon::http::preview`'s
/// writer watches the cancellation and ends its body when it fires. That is necessary and it
/// is not sufficient: ending a body means hyper has a final chunk to **write**, and a client
/// that has stopped reading has a full socket, so the write cannot complete and the connection
/// never finishes. The daemon is then waiting for a browser to be scrolled back into view.
///
/// So the transport's stop is bounded here, and AGENTS' rule is the one being satisfied: open
/// streams are "cancelled, never awaited". On expiry the listener task is **aborted** — its
/// sockets close with it — and the daemon says so at `warn`, naming the bound, because a
/// bounded wait that expires silently is the skip-that-reads-as-pass this project refuses
/// (rule 3).
///
/// Two seconds, and the two ends it sits between are both real. Below it is the ordinary case,
/// which is not a wait at all: a page of a few kilobytes is already written, and a preview
/// whose reader is alive ends in the time it takes one task to notice a cancelled token. Above
/// it is [`DAEMON_SHUTDOWN_DRAIN_MS`], which this must stay far under — `daemon::shutdown`'s
/// header argues that two full-length waits would put the daemon's worst case within reach of
/// systemd's `TimeoutStopSec`, and this wait happens *after* that drain has already run, so it
/// is added to it rather than shared with it.
///
/// Read by `daemon::http::Serving::stopped`, which is where the composition root joins the
/// listener.
pub const WEB_LISTENER_STOP_MS: u64 = 2_000;

// The relation the paragraph above argues, checked where both numbers are. A web-listener
// bound as large as the daemon's whole drain would double the stop's worst case, since the two
// happen one after the other rather than at once.
const _: () =
    assert!(WEB_LISTENER_STOP_MS > 0 && WEB_LISTENER_STOP_MS * 4 < DAEMON_SHUTDOWN_DRAIN_MS);

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
/// a device would have to open one, and D12's whole posture is that `webcam-handler-daemon`
/// running does not itself hold a camera; an availability check that took the camera would be
/// the one failure mode it exists to prevent (AGENTS rule 7 — availability is not capability).
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
/// D12's "the daemon never opens a camera until first use and closes on idle (configurable),
/// so `webcam-handler-daemon` running does not itself block other applications from the
/// webcam" — this is the *default* that makes "configurable" have something to configure
/// (`engine::actor::Cameras::with_idle_timeout` is the override).
///
/// Thirty seconds is chosen from the two failures it sits between. Too short and every
/// `webcam-handler-client get` in a shell loop pays a fresh `open` and the driver's
/// first-frame settle \[PF:11\]; too long and a person who ran one command cannot start a
/// video call without stopping the daemon, which is precisely the complaint D12 exists to
/// answer. A human working at a terminal issues their next command inside thirty seconds, and
/// has stopped caring about the camera long before they switch applications.
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
/// command being executed is already waiting on a device that can only do one thing at a time,
/// and a deeper queue buys a caller nothing but a longer wait before the same answer. Eight is
/// enough that a browser tab's poll and a `webcam-handler-client` invocation arriving together
/// never collide; a caller past it is told the camera is busy — which is the refusal D12 names
/// — rather than made to wait for a queue nobody bounded.
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

/// The widest frame the MJPEG preview asks a camera for.
///
/// A *cap* rather than a size, and the word is load-bearing: `StreamRequest::choose` reads a
/// width and a height as an upper bound and answers with the largest mode that fits inside
/// them, so a camera whose smallest MJPEG mode is 1920×1080 still gets a preview — one that is
/// bigger than this asked for, reported as an [`crate::capture::Adjustment`], because D5's
/// "requested is not applied" is the rule here as everywhere else.
///
/// VGA because a preview is a *pane in a control panel*, not the capture. The two costs it
/// sits between are both real: a preview at the sensor's maximum is a 4K JPEG per frame
/// through a loopback socket for a picture the page draws at a few hundred pixels wide, and a
/// preview far below VGA cannot show whether a focus control did anything, which is the one
/// question the preview exists to answer while somebody drags a slider.
///
/// It is *not* what bounds the daemon's memory — a driver may hand back a frame larger than
/// anything that was asked for, and validating what arrived is
/// [`PREVIEW_MAX_FRAME_BYTES`]'s job (design §2.5: device-derived numbers are validated
/// before use). This pair is what the daemon *asks* for; that one is what it will *hold*.
///
/// Read by `daemon::preview`, which builds the one [`crate::capture::StreamRequest`] every
/// preview stream starts from.
pub const PREVIEW_MAX_WIDTH: u32 = 640;

/// The tallest frame the MJPEG preview asks a camera for.
///
/// [`PREVIEW_MAX_WIDTH`]'s other half; the two are one decision and are read together in one
/// expression. Separate constants because `StreamRequest` has two fields and a single
/// "preview size" constant would have to be unpacked by whoever read it.
pub const PREVIEW_MAX_HEIGHT: u32 = 480;

/// How long one turn of the preview loop waits for a frame off the device.
///
/// The preview holds a camera actor's thread for exactly one `next_frame` at a time (design
/// D12 gives each open camera one blocking thread, and `engine::preview` takes **one** frame
/// per command so the thread is free between frames). So this constant is two things at once,
/// and they pull in opposite directions: it is the deadline that stops a stalled sensor
/// blocking the actor forever, and it is the worst-case latency a `wch_set` pays when it
/// arrives while the preview is inside a `DQBUF`.
///
/// One second is chosen from that pair. At any frame rate a webcam offers, a frame that has
/// not arrived within a second is a device that has stopped rather than a device that is slow
/// — 30 fps is 33 ms — so nothing legitimate is cut short; and a control write that waits at
/// most one second behind a preview is a slider that feels attached to the camera rather than
/// one that appears to have hung. It is deliberately **shorter** than
/// [`FRAME_DEADLINE_MS`], which bounds the same call on the *photo* path where nothing else is
/// waiting on the thread.
///
/// Read by `engine::preview::turn`, which is the only caller that converts it into a deadline.
pub const PREVIEW_FRAME_WAIT_MS: u64 = 1_000;

// The relation the paragraph above argues, checked where both numbers are rather than
// believed — `CAMERA_IDLE_SWEEP_MS`'s tradition. A preview wait longer than the photo path's
// own `DQBUF` bound would mean the preview holds the actor longer than the operation that has
// the camera to itself, which is backwards.
const _: () = assert!(PREVIEW_FRAME_WAIT_MS > 0 && PREVIEW_FRAME_WAIT_MS <= FRAME_DEADLINE_MS);

/// How many consecutive frameless turns end a preview stream.
///
/// [`PREVIEW_FRAME_WAIT_MS`] bounds one turn; this bounds the *sequence* of turns that
/// deliver nothing, and without it a camera that answered "no frame" forever would keep an
/// actor, a descriptor and a `STREAMON` alive for as long as a browser tab stayed open. It is
/// [`MAX_SETTLE_ROUNDS`]'s argument one path along: the deadline is the real bound and this
/// covers the case the deadline cannot, which is a device that keeps answering promptly and
/// never delivers.
///
/// Eight turns is eight seconds of silence at the wait above. Long enough that a camera being
/// slow — a driver re-negotiating, a USB hub renegotiating power — is not mistaken for a
/// camera that has stopped; short enough that a preview whose device really has gone away
/// ends while the person watching is still looking at it.
///
/// A turn that could not be *submitted* counts here too, which is deliberate: a camera whose
/// command queue is full is a camera doing something else, and after this many attempts the
/// honest answer is that this preview is not the thing that camera is for right now.
///
/// Read by `daemon::preview`'s driver loop.
pub const PREVIEW_MAX_EMPTY_TURNS: u32 = 8;

/// The largest frame the preview will publish to its viewers.
///
/// **The bound on what the daemon holds, at the one place a device-derived number becomes an
/// allocation this process keeps.** A frame arrives with a length the driver chose;
/// [`PREVIEW_MAX_WIDTH`] is a request and a request is not an answer (D5), so a camera that
/// negotiated 4K, or a driver that lied about `bytesused`, must meet a number rather than a
/// hope. A frame over this cap is dropped and counted, exactly as a slow reader's frames are —
/// the preview's answer to "too much" is never to hold it.
///
/// Four mebibytes is above every real MJPEG frame this project has measured — a 3840×2160
/// JPEG off the seed hardware is one to two — and far below anything that would matter to a
/// daemon that also holds a session store. The total a running preview can pin is this times
/// [`PREVIEW_READER_QUEUE_DEPTH`] per viewer, plus one for the latest-frame channel itself,
/// times [`PREVIEW_MAX_VIEWERS_PER_CAMERA`]: the arithmetic is small and finite, which is the
/// only property that matters here.
///
/// Read by `daemon::preview`'s publisher, on every frame.
pub const PREVIEW_MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;

/// How many rendered frames one preview reader may have queued ahead of its socket.
///
/// The bound on what **one slow client** can make the daemon hold, and it is one rather than
/// zero for a reason: with a depth of one the daemon builds the next multipart part while the
/// kernel is still writing the previous one, and with a depth of zero every frame would wait
/// for a socket write to complete before the next was even framed.
///
/// It is not the mechanism that drops frames — that is the latest-frame `watch` channel, which
/// keeps exactly one value and overwrites it, so a reader that is behind skips rather than
/// backpressures (`daemon::preview` proves the property rather than restating it). This is the
/// bound on the *rendered* side: the bytes a viewer's writer has already framed and handed to
/// hyper. Two frames per viewer at the very most, both bounded by
/// [`PREVIEW_MAX_FRAME_BYTES`].
///
/// Read by `daemon::http::preview`, where the channel between a viewer's writer and its
/// response body is created.
pub const PREVIEW_READER_QUEUE_DEPTH: usize = 1;

/// How many preview readers one camera serves at once.
///
/// One camera is one stream (D12: "one actor per camera serializes device access by
/// construction"; V4L2 allows one streamer per node), and every viewer past the first is a
/// second reader of the *same* latest-frame channel rather than a second streamer — so this
/// bounds readers, not devices. Without it a page that opened an `<img>` per repaint would
/// cost one task, one channel and one held frame each, which is unbounded growth caused by a
/// client, and this daemon's whole story is that a client cannot cause that (P4e-i).
///
/// Four is a person's screen: the page's own preview, a second tab, and room for the two that
/// a reload leaves behind before the kernel notices the sockets are gone. A fifth is refused
/// with [`crate::Error::Busy`] naming the camera — the refusal D12 already has for "this
/// device is doing something" — rather than served a stream that would make the other four
/// worse.
///
/// Read by `daemon::preview`'s attach path, which counts the live receivers on the feed's
/// channel rather than a number it keeps of its own.
pub const PREVIEW_MAX_VIEWERS_PER_CAMERA: usize = 4;

/// How long one photo may hold a live preview's stream down.
///
/// V4L2 allows one streamer per node, so a photo taken while somebody is watching a preview
/// stops that stream, takes its frame and starts it again — inside the camera's own actor
/// thread, which is the only place the whole sequence is indivisible (owner ruling,
/// 2026-08-12; note **N83**). This is the bound on the middle step: the window in which the
/// viewers see no frames at all, because there are none to see.
///
/// **It bounds a budget rather than a measurement, and that is the only shape that can
/// refuse.** Nothing can interrupt a `DQBUF` that has already started — the engine has no
/// runtime and the actor's thread is inside the capture — so a limit checked *afterwards*
/// would be a number in a log rather than a bound. What is checked is the settle deadline
/// the request carries, before the stream is stopped: a photo whose own budget exceeds this
/// is refused with [`crate::Error::IllegalTransition`] and the preview never stops at all.
///
/// Ten seconds, and the pair it sits between is why. Below it, every photo this tool takes
/// by default must fit — [`DEFAULT_SETTLE_DEADLINE_MS`] plus the one blocking read that may
/// begin just before that deadline arrives ([`FRAME_DEADLINE_MS`]) — or the mechanism would
/// refuse the ordinary case it exists to serve. Above it, nothing: a preview frozen for ten
/// seconds is already a preview a person would call broken, and a client that wants a longer
/// settle than that can have it on a camera nobody is watching.
///
/// Read by `engine::preview::while_suspended`, which is the one place a preview is stopped
/// for anything other than the end of the preview.
pub const PREVIEW_SUSPEND_MAX_MS: u64 = 10_000;

// The relation the paragraph above argues, checked where all three numbers are rather than
// believed — `CAMERA_IDLE_SWEEP_MS`'s tradition. A bound that refused a *default* photo
// would be a mechanism that only ever fires on the case it was built for.
const _: () = assert!(PREVIEW_SUSPEND_MAX_MS >= DEFAULT_SETTLE_DEADLINE_MS + FRAME_DEADLINE_MS);

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

/// How long a recording runs when the caller named no duration, in milliseconds.
///
/// **The bounds here are the agent's, not a human's** — the notes' Expected usage item 10, in
/// the words the owner's statement gives it: *"A transition is seconds; the caps in
/// `schema::limits` must comfortably hold one without an agent learning to work around them,
/// while still bounding a runaway. A cap tuned for a human who notices a growing file is the
/// wrong cap for an unattended caller that will not."* Every number in this group is priced
/// against that sentence rather than against disk space.
///
/// Ten seconds, chosen from what the default is *for*: an agent validating an animation or a
/// transition, where the thing being filmed lasts between a fifth of a second and two, and
/// where the recording has to start before the transition does and end after it. Ten seconds
/// is that with an order of magnitude of slack on both sides — enough that
/// `webcam-handler-cli record` with no flag is the command an agent actually wants, which is
/// the property the sentence above is really about. A default of one or two seconds would be
/// "comfortably holds a transition" only for a caller that had already learned to time its
/// own start, and an agent that has to learn that has learned to work around the cap.
///
/// It is a *default*, not a bound: a caller that names a duration gets it, up to
/// [`MAX_RECORDING_MS`].
///
/// **No product reader yet.** The recording executor P6b's second half lands is what turns
/// this into a `RecordingCaps`, and until then the only things holding it are the relations
/// asserted below. That is recorded rather than hidden, because rubric A8 says a constant
/// nobody reads is a defect and the honest state of this one is "read by the arithmetic and
/// not yet by the product".
pub const DEFAULT_RECORDING_MS: u64 = 10_000;

/// The longest recording this build will make, in milliseconds.
///
/// Two minutes. The runaway this bounds is specific: a `record` that was asked for a
/// transition and met a camera that never stops delivering, on a host nobody is watching. Two
/// minutes is far past any transition — twelve times [`DEFAULT_RECORDING_MS`] — and far short
/// of "a recording somebody meant to make", which this tool does not offer and D7 does not
/// describe.
///
/// It is deliberately of the same order as [`MAX_RECORDING_BYTES`] rather than independent of
/// it, and the arithmetic is worth writing down because two caps that describe different
/// recordings would mean one of them is decoration. A 1080p MJPG frame off the seed hardware
/// is on the order of 200 kB \[PF:9\] and the cameras here reach 120 fps, so the fastest
/// compressed stream this project has measured is about **24 MB/s**:
///
/// - at 120 fps, this duration is ~2.9 GB — over [`MAX_RECORDING_BYTES`], so the **size** cap
///   ends the take at about 90 seconds;
/// - at 30 fps, the ordinary mode, it is ~720 MB — under the size cap, so the **duration**
///   cap ends it at two minutes.
///
/// So both caps bind, on different streams, and neither is unreachable. For Y4M the size cap
/// binds hard and early — 1920×1080 YUYV is 4.1 MB *per frame*, so 2 GiB is about seventeen
/// seconds at 30 fps — and that is the correct answer rather than a defect: D7 calls the raw
/// fallback "enormous but exact", and a caller that wants two minutes of it is asking for
/// something this tool does not do.
///
/// **No product reader yet**, for [`DEFAULT_RECORDING_MS`]'s reason.
pub const MAX_RECORDING_MS: u64 = 120_000;

/// The largest recording file this build will write, in bytes.
///
/// Two gibibytes. Two ceilings meet here and the lower one wins.
///
/// The **format's** ceiling is `imaging::avi::write::MAX_RECORDING_BYTES`, which is
/// `u32::MAX`: an `idx1` entry's `dwChunkOffset` is 32 bits, so an AVI that grows past four
/// gibibytes cannot be indexed at all and every offset past the boundary wraps. That is not a
/// policy and it is not this module's to state — it belongs to the container — so the
/// relation between the two numbers is asserted in `imaging::video`, where both are visible,
/// rather than restated here as a literal that could drift.
///
/// The **policy** ceiling is this one, and it is half of that: a cap that sat exactly on the
/// format's would make every over-budget recording a discovery about `idx1` rather than a
/// decision this project made, and it would leave Y4M — which has no 32-bit offset table and
/// would happily pass four gigabytes — bounded by an accident of the *other* container.
/// Two gibibytes is priced from [`MAX_RECORDING_MS`]'s arithmetic: it is the fastest
/// compressed stream measured here running for about ninety seconds, which is the same
/// recording the duration cap describes.
///
/// It is a bound on the **file**, index and trailer included, because that is what fills a
/// disk and what `RecordingCaps::max_bytes` is compared against.
///
/// Read by `imaging::video`'s ceiling assertion, which is what keeps it under the AVI
/// container's own limit at compile time.
pub const MAX_RECORDING_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// The most frames one recording may hold.
///
/// **A backstop rather than a bound anybody should meet**, and it is stated that way because a
/// cap whose job is to be unreachable still has to be reachable in principle. At the fastest
/// mode this project has measured — 120 fps — [`MAX_RECORDING_MS`] is 14 400 frames, so this
/// is above it with room and the duration cap is what ends an honest take. What this catches
/// is the case the other two cannot: a driver delivering faster than any mode it declares, or
/// a clock that never advances, so neither the span nor the size projection moves.
///
/// 16 384 also bounds what a recording holds in *memory*, which is the second reason it exists
/// at this size: `imaging::avi::write` keeps one eight-byte index entry per frame until close,
/// so a recording at this cap holds 128 kB of index and writes 256 kB of `idx1` — small,
/// finite, and stated here so the number is a decision rather than a round one.
///
/// **No product reader yet**, for [`DEFAULT_RECORDING_MS`]'s reason.
pub const MAX_RECORDING_FRAMES: u32 = 16_384;

// The three relations the paragraphs above argue, checked where all four numbers are —
// `CAMERA_IDLE_SWEEP_MS`' tradition, because each one is a claim about a *pair* and nothing
// else in this workspace reads them together.
//
// A default at or above the maximum is a default nobody can raise, and a zero default is a
// recording of nothing; a frame cap below what the duration cap admits at 120 fps would make
// the backstop the thing that always fires, which would silently turn every long recording
// into a frame-capped one; and a size cap that could not hold a *default* take at the fastest
// stream measured here (24 MB/s, so 24 000 bytes per millisecond) would refuse the ordinary
// case the defaults exist to serve, which is exactly the "agent learning to work around them"
// the notes' item 10 forbids.
const _: () = assert!(DEFAULT_RECORDING_MS > 0 && DEFAULT_RECORDING_MS < MAX_RECORDING_MS);
const _: () = assert!(MAX_RECORDING_FRAMES as u64 > MAX_RECORDING_MS / 1_000 * 120);
const _: () = assert!(MAX_RECORDING_BYTES > DEFAULT_RECORDING_MS * 24_000);

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
