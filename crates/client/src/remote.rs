//! The T4 executor over the T5 wire.
//!
//! Everything the user sees is `webcam-handler-cli-core`'s; this file is the seam between
//! that surface and the generated RPC client, and nothing else. [`cli_core::Executor`]'s own
//! doc named it before it existed: "`wch` implements it over an in-process engine; `wchc`
//! will implement it over the generated RPC client at P4, and the parity gate then proves
//! the two produce identical `--json`."
//!
//! ## Sixteen of nineteen are assembly; three are decisions
//!
//! Most methods here are one call and one `?`. Three are not, and each is argued where it
//! is written:
//!
//! | Method | What is not 1:1 |
//! |---|---|
//! | [`Remote::controls`] | one T4 method over **two** wire methods, and a rendering that must not fork |
//! | [`Remote::capture_profile`] | T4's spelling of the wire's `profile_capture` (`webcam-handler-api`'s header records the deliberate divergence) |
//! | [`Remote::photo`] | a base64 [`api::PhotoResponse`] becomes a [`Photograph`], and the response is checked against itself first |
//!
//! …and a fourth, [`Remote::calibrate_sweep`], which is not an adapter at all but a small
//! state machine: the sweep's answer and the sweep's progress arrive on two different
//! channels of one connection, and the order in which this client asks for them is the whole
//! of whether a progress bar moves.
//!
//! ## The runtime is this file's, and it has one thread
//!
//! `jsonrpsee`'s async client is driven by a background task, so something has to own a
//! runtime. **Current-thread, not multi-thread**, and the reason is the shape of this
//! process rather than a preference: `wchc` runs *one verb per invocation*, so the only
//! concurrency it ever has is a call and the connection's background task — and a
//! current-thread runtime drives both inside the `block_on` that is already there. A
//! multi-thread runtime would start one worker per core to serve a program that issues one
//! request, and the difference was **measured** rather than assumed: `wchc list` against a
//! real daemon, reading `/proc/self/status` at the moment the client is built, is
//! `Threads: 1` as shipped and `Threads: 9` on the same 8-core host with
//! `Builder::new_multi_thread`. Eight worker threads spawned, none of which had anything to
//! do — and reaching for them at all needs `tokio/rt-multi-thread`, a feature this crate
//! deliberately does not enable, so the cheap choice is also the one the manifest makes
//! visible.
//!
//! What that choice costs is stated rather than assumed: the background task makes progress
//! **only inside `block_on`**. Between two `Executor` calls this client is not reading its
//! socket. Nothing here depends on it doing so — pings are off (so there is no keep-alive to
//! miss), the daemon sends nothing unsolicited except on a subscription, and the one
//! subscription this binary opens is drained inside the same `block_on` as the call it
//! belongs to — its tail included, which is [`Remote::calibrate_sweep`]'s fourth step and
//! the one place this client reads a socket after the call it was reading it for has
//! answered.

use std::future::Future;

use api::codes;
use api::{WchEventsClient as _, WchRpcClient as _};
use camino::{Utf8Path, Utf8PathBuf};
use cli_core::{Executor, Photograph, Selection, SessionRef, SweepWatcher};
use jsonrpsee::core::ClientError;
use jsonrpsee::core::client::{Client, ClientBuilder, Subscription};
use schema::camera::CameraId;
use schema::capture::PhotoRequest;
use schema::control::{ControlDesc, ControlSlug, ControlWrite};
use schema::error::{Error, Result};
use schema::limits;
use schema::profile::DeviceProfile;
use schema::progress::ProgressEvent;
use schema::report::{CameraDetail, CameraList, ControlReport, WriteReport};
use schema::session::{Session, SessionList, SessionStatus, SweepRequest};
use schema::snapshot::{RestoreReport, Snapshot};
use uuid::Uuid;

use crate::PROGRAM;

/// A connected `wchd`, and the runtime that drives it.
#[derive(Debug)]
pub struct Remote {
    /// The socket this client is talking to, kept so every transport failure can name it.
    ///
    /// The same reason [`crate::transport::connect`]'s refusal names it: a connection that
    /// died halfway through a verb and one that never existed are the same question to the
    /// person reading the line, and the answer is a path.
    socket: Utf8PathBuf,
    /// The runtime this process owns. One thread — see the module header.
    runtime: tokio::runtime::Runtime,
    /// The one client, over the one connection, carrying calls **and** subscriptions.
    client: Client,
}

impl Remote {
    /// Connect to the daemon's socket and build the client over it.
    ///
    /// `request_timeout` is a parameter rather than a constant because it is a property of
    /// the *verb*: `wch_calibrate_sweep` is "the one method whose latency is unbounded by
    /// design" (`webcam-handler-api`) and everything else on the surface answers in
    /// camera-time. `wchc` runs one verb per invocation, so choosing the budget at
    /// connection time costs nothing and keeps `wchc list` from hanging for an hour on a
    /// daemon that has stopped answering. `crate::request_timeout` is the one place that
    /// chooses.
    ///
    /// # Errors
    ///
    /// [`crate::transport::connect`]'s, naming the socket.
    pub fn connect(socket: &Utf8Path, request_timeout: std::time::Duration) -> Result<Remote> {
        // Current-thread, and the header argues it. `enable_io` because the transport is a
        // socket, and `enable_time` because two things on this runtime need a timer: the
        // request timeout below, which is jsonrpsee's, and the bound on a sweep's tail, which
        // is ours (`Remote::calibrate_sweep`, step 4). The manifest asks for `tokio/time` for
        // the second of those rather than inheriting it from the first.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|error| Error::StorageIo {
                path: socket.to_owned(),
                errno: error.raw_os_error(),
                message: format!("could not start the client's runtime: {error}"),
            })?;

        // Inside the runtime, twice over: `UnixStream::connect` needs a reactor, and
        // `ClientBuilder::build_with_tokio` **panics** when called outside a runtime context
        // (its own doc says so) because it spawns the background task that owns the
        // connection.
        let client = runtime.block_on(async {
            let (sender, receiver) = crate::transport::connect(socket).await?;
            Ok::<Client, Error>(
                ClientBuilder::new()
                    .request_timeout(request_timeout)
                    // Ours rather than jsonrpsee's 256 and 1024: both of its defaults are
                    // server-shaped, and both of ours are argued where they live
                    // (`schema::limits`).
                    .max_concurrent_requests(limits::CLIENT_MAX_CONCURRENT_REQUESTS)
                    .max_buffer_capacity_per_subscription(limits::CLIENT_SUBSCRIPTION_BUFFER)
                    // Pings stay off, which is the builder's default and is said out loud
                    // because the daemon's are off too (note N57's named residual): two
                    // peers neither of which pings is a decision, not an oversight, and on
                    // a `AF_UNIX` socket between two processes on one host there is no
                    // silent-network failure mode for a ping to detect.
                    .disable_ws_ping()
                    .build_with_tokio(sender, receiver),
            )
        })?;

        Ok(Remote {
            socket: socket.to_owned(),
            runtime,
            client,
        })
    }

    /// Run one call to completion on this process's runtime, and type its refusal.
    ///
    /// The `block_on` per method that the module header describes. It is where the async
    /// surface below the seam becomes the synchronous surface above it, and it is the only
    /// place that happens.
    fn on<T, F>(&self, call: F) -> Result<T>
    where
        F: Future<Output = std::result::Result<T, ClientError>>,
    {
        self.runtime
            .block_on(call)
            .map_err(|error| refusal(&self.socket, &error))
    }
}

/// A client error as one of D13's.
///
/// The typed half is `webcam-handler-api`'s and is not re-implemented here:
/// [`codes::typed`] reconstructs the [`schema::Error`] the daemon sent — checking both that
/// the payload is one of ours and that its kind maps back to the code that carried it — so
/// `wchc` renders the *same value* `wch` renders, through the same `Display`. That identity
/// is what the parity gate compares.
///
/// Everything else is a transport failure, and it is deliberately not dressed up as a camera
/// answer (E3: availability is not capability). It becomes [`Error::StorageIo`] naming the
/// socket, for [`crate::transport::connect`]'s reason: the reader's question is "is the
/// daemon there?", and the answer is a path.
fn refusal(socket: &Utf8Path, error: &ClientError) -> Error {
    if let ClientError::Call(object) = error
        && let Some(typed) = codes::typed(object)
    {
        return typed;
    }
    Error::StorageIo {
        path: socket.to_owned(),
        errno: None,
        message: format!("the daemon did not answer: {error}"),
    }
}

/// Which events on a per-client progress stream belong to *this* sweep.
///
/// `wch_subscribe_calibration` is per **client**, not per session — the session id rides on
/// every event and "D10's parenthetical is answered by a consumer-side filter" (note N57).
/// This is that filter, and it is a pure value so it can be driven without a daemon.
///
/// It answers **two questions with two precisions**, and note **N70** exists because the
/// first shape of it answered only one:
///
/// - [`SweepFilter::admits`] decides what is **drawn**, and errs toward showing.
/// - [`SweepFilter::is_mine_terminal`] decides when the daemon has said its **last word about
///   this sweep**, and errs toward waiting.
///
/// They are deliberately not the same predicate, because the two errors do not cost the same
/// thing. An event drawn that was somebody else's costs a repainted bar; a terminal event
/// *credited* to this sweep that was somebody else's costs this sweep its own last event —
/// `sweep_and_watch` skips the tail on the strength of it, and the tail is the only thing
/// standing between a `SweepFinished` that lost the race and the floor (note **N69**). So
/// drawing keeps the loose predicate and the guard takes the tight one.
///
/// ## What "this sweep" is, and how much of it this process knows
///
/// A sweep is a **session and a control**, not a session: the camera actor queues, so one
/// session may run a second sweep of another control, and every event names both. What this
/// process knows of that pair depends on how the caller named the session:
///
/// - `--session <UUID>` names it, so the session half is exact from the start.
/// - `--task <TEXT>` names a slot, and which session occupies it is a fact only the daemon
///   holds. Until the sweep answers, this process does not know the id, so the request's
///   control is the only half it has ([`schema::progress::CalibrationProgress::control`], an
///   exhaustive accessor, is what makes that half available on every variant).
/// - …and then **the answer names it** — `wch_calibrate_sweep` replies with the
///   [`Session`] — which is one step before the tail needs it. [`SweepFilter::with_answer`]
///   is where the second half arrives, and it is the whole reason a `--task` sweep can tell
///   its own last word from another camera's (note **N70**, finding F1).
///
/// **The residual is stated rather than hidden:** under `--task`, a second sweep of the
/// *same control on a different camera*, running through the same daemon at the same moment,
/// is admitted while the call is in flight, and the bar shows both. The alternative was to
/// resolve the task to an id with a `wch_calibrate_status` call before subscribing — rejected
/// because it changes which D13 refusal a bad `--task` produces (the status verb's, not the
/// sweep's), and the parity gate compares exactly that against `wch`. What is left is a bar's
/// accuracy for as long as the call is outstanding — and **only** that, which is the sentence
/// N69 falsified and this type's second predicate makes true again: the same event can no
/// longer end this sweep early.
#[derive(Debug)]
struct SweepFilter {
    /// The session, when the caller named one by id.
    session: Option<Uuid>,
    /// The control this sweep asked for.
    control: ControlSlug,
    /// The session the daemon named when it answered, for a caller who could not.
    ///
    /// `None` until the sweep answers, and `None` afterwards if it answered with a refusal —
    /// a D13 error names no session. It never contradicts `session`: the two are the same
    /// fact from two sources, and [`SweepFilter::known`] prefers the caller's because a
    /// caller who named an id was asking about *that* session whatever the daemon replies.
    answered: Option<Uuid>,
}

impl SweepFilter {
    /// The filter this request implies.
    fn new(which: &SessionRef, request: &SweepRequest) -> SweepFilter {
        SweepFilter {
            session: match which {
                SessionRef::Id { id } => Some(*id),
                // Not "no filter": the control below is what stands in for it.
                SessionRef::Task { .. } => None,
            },
            control: request.control.clone(),
            // Nothing has answered yet; step 4 is where this stops being `None`.
            answered: None,
        }
    }

    /// The same filter, knowing what the sweep's answer said about which session it was.
    ///
    /// It tightens [`SweepFilter::is_mine_terminal`] and deliberately **not**
    /// [`SweepFilter::admits`]: what is drawn must not change its rules halfway through one
    /// sweep, or a bar would start and stop showing a neighbour's events at a moment the
    /// person watching cannot see. What may change is what this process is willing to call
    /// its own ending, because that is a decision it is about to make for the first time.
    fn with_answer(self, session: Option<Uuid>) -> SweepFilter {
        SweepFilter {
            answered: session,
            ..self
        }
    }

    /// Whether `event` belongs to the sweep this process asked for — the drawing half.
    fn admits(&self, event: &ProgressEvent) -> bool {
        match self.session {
            Some(session) => event.session == session,
            None => *event.progress.control() == self.control,
        }
    }

    /// This sweep's session, as precisely as it is currently known.
    fn known(&self) -> Option<Uuid> {
        self.session.or(self.answered)
    }

    /// Whether `session` is this sweep's, as precisely as it is currently known.
    ///
    /// `true` for an unknown session is the honest answer and not a default: before the
    /// answer arrives a `--task` sweep cannot rule anything out, and the caller is the one
    /// that decides what to do with a maybe — [`sweep_and_watch`] records the id and asks
    /// again once the answer has named one.
    fn is_mine(&self, session: Uuid) -> bool {
        self.known().is_none_or(|known| known == session)
    }

    /// Whether `event` is the last word about **this** sweep — the guard half.
    ///
    /// Both halves of the pair, where [`SweepFilter::admits`] takes whichever one the caller
    /// supplied: a terminal event is this sweep's only if it is about this sweep's control
    /// *and* about a session this process cannot tell apart from its own.
    fn is_mine_terminal(&self, event: &ProgressEvent) -> bool {
        event.progress.is_terminal()
            && *event.progress.control() == self.control
            && self.is_mine(event.session)
    }
}

/// What a sweep's answer says about which session it was.
///
/// One method, for one fact, on the one value [`sweep_and_watch`] is otherwise generic over
/// — and the genericity is the point: that function's subject is an *ordering*, so a test
/// hands it a [`std::future::ready`] rather than a wire call. This trait is the smallest
/// thing that keeps that true while letting the answer close the gap `--task` leaves open
/// (note **N70**, finding F1).
///
/// `None` is a real answer and not an absence: a sweep that was refused names no session,
/// and a filter told `None` is exactly as precise as it was before it asked.
trait SweepAnswer {
    /// The session the daemon says this sweep belonged to.
    fn session(&self) -> Option<Uuid>;
}

impl SweepAnswer for std::result::Result<Session, ClientError> {
    fn session(&self) -> Option<Uuid> {
        self.as_ref().ok().map(|session| session.id)
    }
}

/// What one ask of a [`ProgressSource`] produced.
///
/// **Three answers and not two**, and the third is the one this seam shipped without: a
/// notification that arrived, was addressed to this subscription, and could not be turned
/// into a [`ProgressEvent`] by this build. Collapsing it into "the stream ended" was an
/// assumption about jsonrpsee that jsonrpsee does not hold, and note **N70** (finding F3)
/// records both the reading and what it cost.
#[derive(Debug)]
enum Arrival {
    /// One event.
    Event(ProgressEvent),
    /// A notification this build could not decode, on a stream that is **still open**.
    ///
    /// [`schema::progress::CalibrationProgress`] is an internally-tagged enum with no
    /// catch-all arm, so a `wchd` newer than the `wchc` talking to it produces exactly this:
    /// one variant this build has never heard of, one `serde_json` failure, and a queue of
    /// perfectly decodable events behind it. It is skipped and never fatal — nothing about
    /// one unreadable payload says anything about the next one, and there is no sweep-shaped
    /// question it answers (which sweep it belonged to is itself inside the payload).
    Undecodable,
    /// Nothing further is coming: the daemon ended the stream, or dropped this client for
    /// falling too far behind ([`limits::CLIENT_SUBSCRIPTION_BUFFER`]).
    Ended,
}

/// Where a sweep's progress comes from, so the tail can be driven without a daemon.
///
/// One method, and the seam exists for a reason a test can state: what [`drain_tail`] is
/// *for* is a race against a real daemon — an event that left the daemon before its answer
/// did and arrived after it — and a race cannot be arranged twice running. What a test can do
/// is hold each ordering still, which is what the scripted source beside the tests does.
///
/// It answers an [`Arrival`] rather than jsonrpsee's `Option<Result<…>>` so that the two
/// failures that type carries stay two things. **They are not the same thing, and this file
/// believed they were.** `jsonrpsee-core` 0.26's `Stream for Subscription` sets `is_closed`
/// only where its receiver yields `None`; a `serde_json::from_str` that fails is
/// `Some(Err(_))` on a subscription that stays open and keeps delivering. Reading
/// `Some(Err(_)) | None => None` therefore turned one unreadable notification into the end of
/// the sweep's progress — the bar froze mid-sweep and the tail was skipped too, which is
/// N69's symptom from a cause N69 did not consider (note **N70**, finding F3).
///
/// It must be **cancel-safe**: [`sweep_and_watch`]'s `select!` drops the future it did not
/// take, every turn, and an implementation that consumed an event on a poll it did not
/// complete would lose one per turn of the sweep.
trait ProgressSource {
    /// The next arrival on this stream.
    async fn next_event(&mut self) -> Arrival;
}

impl ProgressSource for Subscription<ProgressEvent> {
    async fn next_event(&mut self) -> Arrival {
        match self.next().await {
            Some(Ok(event)) => Arrival::Event(event),
            // The payload is deliberately not carried into the refusal or a log line: a
            // progress event names a photo path and a session, and this crate does not put
            // either in front of a reader to explain a decode failure (AGENTS "Hardware and
            // privacy"; rubric A12).
            Some(Err(_)) => Arrival::Undecodable,
            None => Arrival::Ended,
        }
    }
}

/// The tail's budget, in the one place both the shipped path and the tests that are about it
/// can name.
///
/// [`limits::CLIENT_SWEEP_DRAIN_MS`] as a [`std::time::Duration`], written once so that
/// changing the number changes what a test asserts rather than only what a binary does.
/// Before note **N70** (finding F2) every tail test passed `Duration::ZERO` and nothing at
/// all read the constant, so setting it to zero — which deletes the fix while leaving its
/// code in place — passed the entire workspace suite.
const SWEEP_DRAIN_BUDGET: std::time::Duration =
    std::time::Duration::from_millis(limits::CLIENT_SWEEP_DRAIN_MS);

/// The sweep's state machine: watch the stream while the call is in flight, then take its
/// tail.
///
/// Steps 2 to 4 of [`Remote::calibrate_sweep`], whose doc argues every one of them. It is a
/// free function, and generic over the answer's type, because the *ordering* is the subject
/// and jsonrpsee's `Result<Session, ClientError>` is not: a test hands it a
/// [`std::future::ready`] and a scripted stream, and can hold each of the two orderings still
/// — including the one that costs a bar its last line, which against a real daemon happens on
/// about one run in a hundred (note **N69**).
///
/// The one thing it does read out of that answer is [`SweepAnswer::session`], because the
/// answer is where a `--task` sweep finally learns which session it was and the guard below
/// is the first decision that needs it (note **N70**, finding F1).
async fn sweep_and_watch<S, A>(
    events: &mut S,
    call: impl Future<Output = A>,
    filter: SweepFilter,
    watch: &dyn SweepWatcher,
    budget: std::time::Duration,
) -> A
where
    S: ProgressSource,
    A: SweepAnswer,
{
    // Pinned so the loop below can poll the same future more than once without moving it; a
    // fresh call per turn would be a fresh sweep.
    let mut call = std::pin::pin!(call);
    // `false` once the stream has ended, which takes its arm out of the `select!` rather
    // than letting a closed stream answer `Ended` forever — the shape that would turn a
    // finished subscription into a spin.
    let mut watching = true;
    // The session of the last terminal event that could have been this sweep's — an **id
    // rather than a flag**, because under `--task` the question "was that mine?" has no
    // answer yet and a flag would have to guess one. That is finding F1 in one variable
    // (note **N70**): the shape this replaced was `ended |= event.progress.is_terminal()`
    // over everything `admits` let through, so another camera's sweep of the same control
    // disarmed this sweep's tail and the terminal event it was holding open for was lost.
    //
    // One id and not a set, deliberately, because the only error it can make is the safe
    // one: if this sweep's own last word is followed by a neighbour's, the last id is the
    // neighbour's, this sweep pays the bound once at the end of a sweep that took
    // camera-minutes, and it loses nothing. A set would be a queue on a per-client stream
    // whose length is a property of how many sweeps a daemon ran, which is the unbounded
    // shape AGENTS' "bounded everything" refuses.
    let mut last_terminal: Option<Uuid> = None;
    let answered = loop {
        tokio::select! {
            biased;
            arrival = events.next_event(), if watching => match arrival {
                Arrival::Event(event) => {
                    if filter.admits(&event) {
                        if filter.is_mine_terminal(&event) {
                            last_terminal = Some(event.session);
                        }
                        watch.event(&event);
                    }
                }
                // A notification this build cannot read. **The stream is still open** and
                // this is not the end of it (note **N70**, finding F3): jsonrpsee closes a
                // subscription when its receiver runs dry and not when a payload fails to
                // deserialize, so what is behind this one is still coming and is still
                // decodable. Skipped, and the loop reads on — the shape this replaced took
                // the stream's arm out of the `select!` here, and every remaining event of
                // the sweep was discarded while it sat readable in the client's own queue.
                Arrival::Undecodable => {}
                // The stream the daemon ended: a lag close, or a shutdown. Not this sweep's
                // failure and not worth abandoning it for — the sweep is running on the
                // camera and its answer is still coming. The bar simply stops moving, which
                // is the honest rendering of "nothing is being told to me any more".
                Arrival::Ended => watching = false,
            },
            answered = &mut call => break answered,
        }
    };

    // The answer is the last thing that can name this sweep's session, and it has just
    // arrived — so the guard below is decided at the one moment this process knows the most,
    // rather than event by event when it knew the least.
    let filter = filter.with_answer(answered.session());
    let ended = last_terminal.is_some_and(|session| filter.is_mine(session));

    // Step 4, entered only when there is something outstanding: the daemon has not said its
    // last word about this sweep (`ended`) and the stream it would say it on is still open
    // (`watching`). A sweep whose terminal event beat its answer — which is nearly all of
    // them — pays nothing at all here, and the bound is never reached by a client that
    // already has what it would be waiting for.
    if watching && !ended {
        // Which of the three ways the tail ended is not this call's answer: a sweep that
        // finished is a sweep that finished whether or not its last event beat the response
        // over the socket. The value exists so each of the three has a test.
        let _tail = drain_tail(events, &filter, watch, budget).await;
    }

    answered
}

/// How a sweep's tail ended — one variant per way out of [`drain_tail`], so each has a test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tail {
    /// This sweep's terminal event arrived, so everything the daemon sent was rendered.
    Terminal,
    /// The stream ended first: a lag close or a shutdown. Whatever was still in flight is
    /// not coming. A payload this build could not decode is **not** one of these — it is
    /// skipped and the tail reads on (note **N70**, finding F3).
    Ended,
    /// The bound arrived first, which means no terminal event is coming — the daemon dropped
    /// it (note **N57**) or the fan-out never ran. The bar stops one event short, which is
    /// what it did before this drain existed.
    Bounded,
}

/// Read the sweep's tail: what the daemon sent before it answered, and had not delivered yet.
///
/// The fourth step of [`Remote::calibrate_sweep`], and that method's doc argues why it is
/// here at all. This is the shape of it:
///
/// - it **stops at this sweep's terminal event**, which is the event it exists to collect and
///   the last one the daemon will send about this sweep;
/// - it stops at the end of the stream, which says the same thing with less information;
/// - and it stops at `budget`, which is the case note **N65** is right about: an event that
///   was dropped never arrives, and waiting for it would hang this process for ever.
///
/// **`budget` bounds the waiting, not the reading.** `tokio::time::timeout_at` polls the
/// event before it polls the clock, so even a zero budget delivers whatever is already in
/// hand — which is why most of the tests below can drive this function without a clock of any
/// kind. What a zero budget cannot do is the thing the bound exists for, and that has a test
/// of its own: on this client's current-thread runtime the queue is provably empty when the
/// call answers (note **N65**'s measurement, note **N69**'s reading of it), so the event this
/// drain came for is one that has not arrived yet and only *waiting* can collect it.
///
/// Another sweep's terminal event is not allowed to end this tail: the fan-out is one per
/// daemon and the stream is per *client* (note **N57**), so a second sweep — another
/// session's, or this session's on another control — puts its own last word on this socket,
/// and stopping at it would stop the drain one event before the one it came for. That is what
/// [`SweepFilter::is_mine_terminal`] asks and [`SweepFilter::admits`] does not; the second
/// sweep's events are still *drawn*, because drawing is `admits`' question and the two err in
/// opposite directions on purpose.
async fn drain_tail<S: ProgressSource>(
    events: &mut S,
    filter: &SweepFilter,
    watch: &dyn SweepWatcher,
    budget: std::time::Duration,
) -> Tail {
    let deadline = tokio::time::Instant::now() + budget;
    loop {
        match tokio::time::timeout_at(deadline, events.next_event()).await {
            Err(_elapsed) => return Tail::Bounded,
            Ok(Arrival::Ended) => return Tail::Ended,
            // One payload this build cannot read is not the end of the stream behind it
            // (note **N70**, finding F3). It does not extend the deadline either — the
            // deadline is an instant and not a countdown — so a daemon sending nothing this
            // client understands still ends this tail at the bound.
            Ok(Arrival::Undecodable) => continue,
            Ok(Arrival::Event(event)) => {
                if !filter.admits(&event) {
                    continue;
                }
                // Read before the watcher takes it, because `watch.event` borrows it and the
                // answer to "was that the last one" is a property of the event rather than of
                // the rendering.
                let terminal = filter.is_mine_terminal(&event);
                watch.event(&event);
                if terminal {
                    return Tail::Terminal;
                }
            }
        }
    }
}

impl Executor for Remote {
    fn list(&mut self) -> Result<CameraList> {
        self.on(self.client.list())
    }

    fn info(&mut self, camera: &CameraId) -> Result<CameraDetail> {
        self.on(self.client.info(camera.clone()))
    }

    /// One T4 method, two wire methods — the first of the three that are not 1:1.
    ///
    /// `wch_controls` is a read and `wch_discover_pairs` is a **write**, which is why they
    /// are two methods on the wire: "the daemon has to route, permission and count it as the
    /// write it is" (`webcam-handler-api`). T4 keeps them one verb with a flag, because a
    /// verb exists once and the answer has the same shape either way. So the flag chooses
    /// the method here, which is the whole of the adaptation.
    ///
    /// The probe's answer is a superset — it also carries what the probe *declined* to touch
    /// and what it could not put back, on the wire precisely so a socket client is not
    /// running a write with its restoration report withheld (note N30). Those two facts are
    /// printed through [`cli_core::report_probe`], which moved out of `crates/cli` at P4f for
    /// this call site: a second copy of that rendering would be the fork design §2.10
    /// forbids, in the one place the parity gate cannot see it (it is stderr, and the gate
    /// compares `--json`).
    fn controls(&mut self, camera: &CameraId, discover_pairs: bool) -> Result<ControlReport> {
        if !discover_pairs {
            return self.on(self.client.controls(camera.clone()));
        }
        let found = self.on(self.client.discover_pairs(camera.clone()))?;
        cli_core::report_probe(PROGRAM, &found.skipped, &found.restored);
        Ok(found.controls)
    }

    fn get(&mut self, camera: &CameraId, control: &ControlSlug) -> Result<ControlDesc> {
        self.on(self.client.get(camera.clone(), control.clone()))
    }

    fn set(
        &mut self,
        camera: &CameraId,
        writes: &[ControlWrite],
        guarded: bool,
    ) -> Result<WriteReport> {
        self.on(self.client.set(camera.clone(), writes.to_vec(), guarded))
    }

    fn snapshot(&mut self, camera: &CameraId) -> Result<Snapshot> {
        self.on(self.client.snapshot(camera.clone()))
    }

    fn restore(&mut self, camera: &CameraId, snapshot: &Snapshot) -> Result<RestoreReport> {
        // The snapshot travels as a **document**, not a path: `wchc restore` reads the
        // caller's filesystem (the shared surface already did, in `cli_core::run`) and sends
        // the value, so a snapshot on a machine the daemon cannot see still restores.
        self.on(self.client.restore(camera.clone(), snapshot.clone()))
    }

    /// A base64 answer becomes a [`Photograph`] — the third of the three that are not 1:1.
    ///
    /// D10 puts the bytes in the JSON-RPC result as base64 because one response is one
    /// document; `cli_core::Photograph` carries them beside the report because an in-process
    /// caller needs no encoding at all. This is the one conversion between the two, and it
    /// is where `webcam-handler-api`'s [`api::PhotoResponse::bytes_match_the_delivery`]
    /// finds the consumer it was still owed: the daemon calls it as the last statement
    /// before an answer leaves that process, and until now "a truncated payload is refused
    /// by the sender and by nobody on the receiving end" (note N34).
    ///
    /// A response that disagrees with itself is [`Error::DeviceIo`] — **ours, not the
    /// device's**, which is the same reading `daemon::server::photo_response` gives it: the
    /// camera said nothing wrong, a document did.
    fn photo(&mut self, camera: &CameraId, request: &PhotoRequest) -> Result<Photograph> {
        let response = self.on(self.client.photo(camera.clone(), request.clone()))?;
        if !response.bytes_match_the_delivery() {
            return Err(Error::DeviceIo {
                operation: "wch_photo".to_owned(),
                errno: None,
                // The counts and never the bytes: a frame may contain a person, and this
                // message reaches a terminal and a log (AGENTS; rubric A12).
                message: format!(
                    "the daemon's answer disagrees with itself: the delivery reports {} \
                     byte(s) and the payload carries {}",
                    response.report.delivery.byte_count(),
                    match &response.bytes {
                        Some(bytes) => format!("{}", bytes.len()),
                        None => "nothing at all".to_owned(),
                    }
                ),
            });
        }
        Ok(Photograph {
            report: response.report,
            returned: response.bytes.map(api::Base64Bytes::into_inner),
        })
    }

    /// T4's spelling of the wire's `profile_capture` — the second of the three.
    ///
    /// The divergence is deliberate and recorded where the wire is declared: the Rust names
    /// on the T5 trait "follow D10's spelling rather than T4's where the two differ —
    /// `profile_capture` here, `Executor::capture_profile` there. T4 is a settled surface and
    /// is not renamed for this; the two are allowed different spellings because only one of
    /// them is the wire." This line is the entire cost of that, and it is here rather than in
    /// a rename because a wire name is a compatibility contract and a trait method name is
    /// not.
    fn capture_profile(&mut self, camera: &CameraId, capturer: &str) -> Result<DeviceProfile> {
        self.on(self
            .client
            .profile_capture(camera.clone(), capturer.to_owned()))
    }

    fn calibrate_start(
        &mut self,
        camera: &CameraId,
        task: &str,
        goal: &str,
        criteria: &[String],
    ) -> Result<Session> {
        self.on(self.client.calibrate_start(
            camera.clone(),
            task.to_owned(),
            goal.to_owned(),
            criteria.to_vec(),
        ))
    }

    fn calibrate_plan(
        &mut self,
        camera: &CameraId,
        which: &SessionRef,
        controls: &[ControlSlug],
        order: bool,
    ) -> Result<Session> {
        self.on(self
            .client
            .calibrate_plan(camera.clone(), which.clone(), controls.to_vec(), order))
    }

    /// The sweep, which is a state machine rather than an adapter.
    ///
    /// `wch_calibrate_sweep` answers only the final [`Session`]; the progress a bar is drawn
    /// from arrives on the **separate** `wch_subscribe_calibration` stream, on the same
    /// connection (note N57 — calls and subscriptions are two capabilities over one socket,
    /// which is why the client is a `SubscriptionClientT`). Four steps, and the **order is
    /// the whole of it**:
    ///
    /// 1. **Subscribe first.** Events emitted between the call leaving and a later subscribe
    ///    are gone — the daemon buffers nothing for a client that has not arrived, which is
    ///    P4e-i's decision and note N57's ("a parked long-lived `Receiver` would hold a
    ///    whole sweep's events for nobody"). Subscribing afterwards would silently drop the
    ///    start of every sweep.
    /// 2. **Call, and keep both in flight.** The call and the stream are polled together,
    ///    `biased` toward the events so a burst is drained rather than left to overflow the
    ///    client's own buffer ([`limits::CLIENT_SUBSCRIPTION_BUFFER`], which jsonrpsee
    ///    closes a subscription for exceeding).
    /// 3. **Answer.** The loop ends when the *call* answers, and the client's own queue is
    ///    empty when it does: on a current-thread runtime nothing can be pushed onto the
    ///    subscription **between** the two polls of one `select!` turn, and `biased` polls
    ///    the events first, so every event this client had been given is already rendered.
    ///    That is note **N65**'s argument, it is correct, and it was re-measured here —
    ///    zero events left in this queue on every run, loaded or not.
    /// 4. **Drain the tail, under a bound.** What N65's argument does not cover is the event
    ///    that has not *arrived* yet, and that is the one a sweep loses. `wch_calibrate_sweep`'s
    ///    answer and its `SweepFinished` leave the daemon on two different tasks — the method
    ///    call, and the forward task `daemon::events` runs per subscription — and reach the
    ///    one connection's writer in whichever order that daemon's runtime scheduled them.
    ///    When the answer wins, the loop breaks on it and the terminal event lands
    ///    microseconds later on a socket nobody is reading any more: measured at **34 µs**
    ///    after the answer, and the integration suite's terminal assertion failed **2 runs
    ///    in 150** under four concurrent workspace suites before this step existed (note
    ///    **N69**). It is the one event a bar cannot recover from a later one —
    ///    `cli_core`'s `Bar` prints the sweep's closing line from it and from nothing else.
    ///
    ///    So `drain_tail` reads on until this sweep's terminal event, the end of the
    ///    stream, or [`limits::CLIENT_SWEEP_DRAIN_MS`], whichever comes first — and it is
    ///    entered **only when the terminal event is actually outstanding**, so the sweeps
    ///    whose events beat their answer, which is nearly all of them, pay nothing. **The
    ///    bound is what keeps this from being the thing N65 refused**: waiting for a
    ///    terminal event *without* one would hang forever whenever the daemon dropped it,
    ///    and dropping is a thing `wch_subscribe_calibration` is explicitly allowed to do
    ///    (it counts losses and carries on, note N57). Nothing here waits *out* a timer
    ///    either — the drain ends on the event, and the bound is only reached when there is
    ///    no event left to end on.
    ///
    ///    A drain that refused to wait at all was tried first and cannot work, which is
    ///    worth stating because it is the obvious repair: it finds this client's queue
    ///    provably empty (step 3), and spinning `tokio::task::yield_now` instead of waiting
    ///    finds nothing either — tokio's current-thread `block_on` re-polls a main future
    ///    whose waker has already fired without ever parking on the I/O driver, so the
    ///    connection's read task never touches the socket. Measured at 512 turns, zero
    ///    events. **The number that does the waiting is checked by a test that is about the
    ///    number** (`SWEEP_DRAIN_BUDGET`), because for one day it was checked by nothing at
    ///    all and zero passed the whole suite (note **N70**, finding F2).
    ///
    /// ## What the guard on step 4 is allowed to believe
    ///
    /// The tail is skipped when this sweep has already had its last word, so *which* last
    /// word counts is load-bearing in a way the first shape of this code did not treat it as
    /// (note **N70**, finding F1). The fan-out is one per daemon and camera actors are one
    /// per camera, so two sweeps genuinely run at once, and their events share this socket:
    /// a `SweepFinished` for another camera's sweep of the same control, or for another
    /// control in this same session, is admitted by `SweepFilter::admits` and drawn. It
    /// must not be allowed to *end* this one. `SweepFilter::is_mine_terminal` is the tighter
    /// question, and the session it compares against is the one the daemon names **in the
    /// answer** — which is why `--task`, whose caller cannot name a session at all, is no
    /// longer a precision this decision has to do without.
    ///
    /// ## A notification this build cannot read is not the end of the stream
    ///
    /// [`schema::progress::CalibrationProgress`] is internally tagged with no catch-all arm,
    /// so a `wchd` newer than this `wchc` produces a decode failure per unknown variant.
    /// jsonrpsee keeps such a subscription **open** — only a dry receiver closes it — so the
    /// events behind it are still coming and still decodable. They are skipped one at a time
    /// and neither the loop nor the tail ends on one (note **N70**, finding F3).
    ///
    /// **They are not counted onto anything a person sees**, and that is the same decision
    /// N69 made for a dropped terminal event rather than a new one: `wch` cannot produce this
    /// condition at all — its sink is synchronous and nothing is serialized — so a line on
    /// `wchc`'s stderr would be a divergence between the two roots in the one place the
    /// parity gate does not look, for a rendering that is already what a dropped event looks
    /// like (note N57). What a person sees is a bar missing a line, and the daemon counts its
    /// own drops for the operator who needs the number.
    ///
    /// # Errors
    ///
    /// The sweep's, typed. A sweep that fails does **not** fail the stream — the refusal
    /// arrives as a `SweepInterrupted` event *and* as this call's error, "because the sweep's
    /// own caller is the one being refused" — and it is this call's error that is returned.
    fn calibrate_sweep(
        &mut self,
        camera: &CameraId,
        which: &SessionRef,
        request: &SweepRequest,
        watch: &dyn SweepWatcher,
    ) -> Result<Session> {
        let filter = SweepFilter::new(which, request);
        let Remote {
            socket,
            runtime,
            client,
        } = &*self;

        runtime.block_on(async {
            // Step 1.
            let mut events = client
                .subscribe_calibration()
                .await
                .map_err(|error| refusal(socket, &error))?;
            // Steps 2 to 4, which are `sweep_and_watch`'s and are a free function for the
            // reason its own doc gives: everything below this line except the subscribe is
            // reachable from a unit test.
            let answered = sweep_and_watch(
                &mut events,
                client.calibrate_sweep(camera.clone(), which.clone(), request.clone()),
                filter,
                watch,
                SWEEP_DRAIN_BUDGET,
            )
            .await;

            answered.map_err(|error| refusal(socket, &error))
        })
    }

    fn calibrate_status(&mut self, camera: &CameraId, which: &SessionRef) -> Result<SessionStatus> {
        self.on(self.client.calibrate_status(camera.clone(), which.clone()))
    }

    fn calibrate_select(
        &mut self,
        camera: &CameraId,
        which: &SessionRef,
        control: &ControlSlug,
        selection: &Selection,
    ) -> Result<Session> {
        self.on(self.client.calibrate_select(
            camera.clone(),
            which.clone(),
            control.clone(),
            selection.clone(),
        ))
    }

    fn calibrate_apply(
        &mut self,
        camera: &CameraId,
        which: &SessionRef,
        partial: bool,
    ) -> Result<WriteReport> {
        self.on(self
            .client
            .calibrate_apply(camera.clone(), which.clone(), partial))
    }

    fn calibrate_restore(
        &mut self,
        camera: &CameraId,
        which: &SessionRef,
    ) -> Result<RestoreReport> {
        self.on(self.client.calibrate_restore(camera.clone(), which.clone()))
    }

    fn calibrate_list(&mut self, camera: Option<&CameraId>) -> Result<SessionList> {
        // `None` means every session on the machine — the one optional parameter on this
        // surface, and the daemon answers a missing key and an explicit `null` identically
        // (`webcam-handler-api`'s `wch_calibrate_list` measured it).
        self.on(self.client.calibrate_list(camera.cloned()))
    }
}

#[cfg(test)]
mod tests {
    use schema::progress::CalibrationProgress;
    use schema::session::SweepSpec;
    use schema::time::Stamp;

    use super::*;

    fn slug(name: &str) -> ControlSlug {
        ControlSlug::parse(name).expect("literal slug")
    }

    fn request(control: &str) -> SweepRequest {
        SweepRequest {
            control: slug(control),
            plan: SweepSpec::All,
            allow_motion: false,
            stream: schema::capture::StreamRequest {
                pixel_format: None,
                width: None,
                height: None,
                interval: None,
                buffer_count: limits::DEFAULT_BUFFER_COUNT,
            },
            settle: schema::capture::SettlePolicy::default(),
            photo_format: schema::capture::PhotoFormat::Jpeg,
        }
    }

    fn event(session: Uuid, control: &str) -> ProgressEvent {
        ProgressEvent {
            session,
            at: Stamp::epoch(),
            progress: CalibrationProgress::SweepStarted {
                control: slug(control),
                plan: SweepSpec::All,
                total: 4,
            },
        }
    }

    #[test]
    fn a_sweep_named_by_id_admits_that_sessions_events_and_no_others() {
        // The exact half. Both directions, because a filter that admitted everything and a
        // filter that admitted nothing would each pass one of them.
        let mine = Uuid::from_u128(1);
        let theirs = Uuid::from_u128(2);
        let filter = SweepFilter::new(&SessionRef::Id { id: mine }, &request("focus_absolute"));

        assert!(filter.admits(&event(mine, "focus_absolute")));
        assert!(
            !filter.admits(&event(theirs, "focus_absolute")),
            "another session's sweep of the same control would move this bar"
        );
        // …and the id wins over the control, which is what makes it the *exact* half: a
        // second sweep this session is running is still this session's.
        assert!(filter.admits(&event(mine, "brightness")));
    }

    #[test]
    fn a_sweep_named_by_task_falls_back_to_the_control_it_asked_for() {
        // The inexact half, and the residual the type's doc names: the session id is a fact
        // only the daemon holds until the call answers, so the control is what stands in.
        let filter = SweepFilter::new(
            &SessionRef::Task {
                task: "focus".to_owned(),
            },
            &request("focus_absolute"),
        );

        // Any session, so long as it is sweeping the control this process asked for — which
        // is the concession, stated as a test rather than as prose.
        assert!(filter.admits(&event(Uuid::from_u128(1), "focus_absolute")));
        assert!(filter.admits(&event(Uuid::from_u128(2), "focus_absolute")));
        // And never another control's, which is the half that keeps a `calibrate sweep`
        // bar from being repainted by a sweep of something else entirely.
        assert!(!filter.admits(&event(Uuid::from_u128(1), "brightness")));
    }

    // ------------------------------------------------------------------ the sweep's tail

    /// This sweep's last event.
    fn finished(session: Uuid, control: &str) -> ProgressEvent {
        ProgressEvent {
            session,
            at: Stamp::epoch(),
            progress: CalibrationProgress::SweepFinished {
                control: slug(control),
                samples: 4,
            },
        }
    }

    /// What a scripted progress stream does when it is asked for the next event.
    ///
    /// The fault menu of the thing it stands in for, matched exhaustively in
    /// [`Scripted::next_event`] so a seventh thing a real subscription can do has to be added
    /// here before it can be relied on anywhere.
    #[derive(Debug)]
    enum Delivery {
        /// One event — this sweep's, or another sweep's on the same per-client stream.
        Event(ProgressEvent),
        /// One event that arrives **after the answer**: the stream is pending for as long as
        /// the call is in flight, and delivers on the next ask. This is the ordering the
        /// whole tail exists for, held still — against a real daemon it is a scheduling race
        /// that lands about once in a hundred sweeps (note **N69**).
        Late(ProgressEvent),
        /// One event that arrives **after a delay on the test's own clock**, which is the
        /// only shape in this menu that makes the budget the subject: `Late` says "after the
        /// answer" and this one says "after a wait a bound either covers or does not" (note
        /// **N70**, finding F2). The sleep is on `tokio::time`'s clock, so a paused test
        /// advances it without spending a millisecond of anyone's life (AGENTS: "no `sleep`
        /// as synchronization — settle logic runs on a clock the test owns").
        Delayed {
            /// How long the daemon takes to say it.
            after: std::time::Duration,
            /// What it says.
            event: ProgressEvent,
        },
        /// A notification this build cannot decode, on a stream that stays open.
        ///
        /// The one thing a real subscription does that this menu could not say, back when
        /// [`Delivery::Ended`]'s own doc claimed to cover it — a menu that names a fault the
        /// thing it doubles does not have is a claim, and that one was never checked against
        /// jsonrpsee (note **N70**, finding F3).
        Undecodable,
        /// The stream ends here: a lag close, or a shutdown.
        Ended,
        /// Nothing, ever. The daemon has this sweep's terminal event and never sends it,
        /// which is the case the bound exists for and the one N65 is right about.
        Silence,
    }

    /// A progress stream that answers from a script, counting what it was asked.
    #[derive(Debug)]
    struct Scripted {
        script: std::collections::VecDeque<Delivery>,
        /// How many times this stream was asked for an event — the observable that says
        /// whether a tail was entered at all.
        asked: usize,
    }

    impl Scripted {
        fn of(script: impl IntoIterator<Item = Delivery>) -> Scripted {
            Scripted {
                script: script.into_iter().collect(),
                asked: 0,
            }
        }
    }

    impl ProgressSource for Scripted {
        async fn next_event(&mut self) -> Arrival {
            self.asked += 1;
            // Peeked and waited on **before** anything is taken off the script, because the
            // trait says this future must be cancel-safe and `select!` drops it every turn:
            // a delay that popped first would lose its event to the turn the call answers on,
            // which is the very turn every one of these scripts is about.
            if let Some(Delivery::Delayed { after, .. }) = self.script.front() {
                tokio::time::sleep(*after).await;
            }
            match self.script.pop_front() {
                Some(Delivery::Event(event) | Delivery::Delayed { event, .. }) => {
                    Arrival::Event(event)
                }
                // Put back at the head and answer nothing *this* turn: `select!` drops this
                // future when the call answers, and the next ask — the tail's — delivers it.
                Some(Delivery::Late(event)) => {
                    self.script.push_front(Delivery::Event(event));
                    std::future::pending().await
                }
                Some(Delivery::Undecodable) => Arrival::Undecodable,
                // A script that ran out says what an ended stream says.
                Some(Delivery::Ended) | None => Arrival::Ended,
                Some(Delivery::Silence) => std::future::pending().await,
            }
        }
    }

    /// The sweep's answer in these tests: the one fact [`sweep_and_watch`] reads out of a
    /// value it is otherwise generic over.
    ///
    /// A named session and not a string, because since note **N70** the answer is where a
    /// `--task` sweep learns which session it was — so an answer that carried nothing would
    /// be a test driving the guard with the one input it cannot have.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Answered(Option<Uuid>);

    impl SweepAnswer for Answered {
        fn session(&self) -> Option<Uuid> {
            self.0
        }
    }

    /// The answer a daemon gives a sweep it ran: this session, and no refusal.
    fn answer(session: Uuid) -> Answered {
        Answered(Some(session))
    }

    /// Every event the tail put in front of a human, in order.
    #[derive(Debug, Default)]
    struct Recording(std::sync::Mutex<Vec<ProgressEvent>>);

    impl SweepWatcher for Recording {
        fn event(&self, event: &ProgressEvent) {
            self.0
                .lock()
                .expect("the watcher was not poisoned")
                .push(event.clone());
        }
        fn finish(&self) {}
    }

    impl Recording {
        /// What was drawn, by name, which is what every assertion below is about.
        fn drawn(&self) -> Vec<&'static str> {
            self.0
                .lock()
                .expect("the watcher was not poisoned")
                .iter()
                .map(|event| event.progress.name())
                .collect()
        }

        /// Which sessions it drew for — one assertion needs this and no other.
        fn sessions(&self) -> Vec<Uuid> {
            self.0
                .lock()
                .expect("the watcher was not poisoned")
                .iter()
                .map(|event| event.session)
                .collect()
        }

        /// Which controls it drew for, which is the other half of "whose event was that":
        /// two sweeps of one session are told apart by their control and by nothing else.
        fn controls(&self) -> Vec<String> {
            self.0
                .lock()
                .expect("the watcher was not poisoned")
                .iter()
                .map(|event| event.progress.control().to_string())
                .collect()
        }
    }

    /// The filter every tail test drains under: one session, named by id.
    fn mine() -> (Uuid, SweepFilter) {
        let session = Uuid::from_u128(1);
        (
            session,
            SweepFilter::new(&SessionRef::Id { id: session }, &request("focus_absolute")),
        )
    }

    #[tokio::test]
    async fn a_tail_already_in_hand_is_delivered_with_no_budget_at_all() {
        // The case note N65 measured and the one this drain does *not* need a clock for: two
        // events the client already holds, a budget of zero, and both are drawn. It is here
        // as the first test because it is the half of `drain_tail` that is not a wait —
        // `timeout_at` polls the event before it polls the clock, and a build that polled
        // them the other way round would lose an event it was already holding.
        let (session, filter) = mine();
        let watcher = Recording::default();
        let mut events = Scripted::of([
            Delivery::Event(event(session, "focus_absolute")),
            Delivery::Event(finished(session, "focus_absolute")),
        ]);

        let tail = drain_tail(&mut events, &filter, &watcher, std::time::Duration::ZERO).await;

        assert_eq!(tail, Tail::Terminal);
        assert_eq!(watcher.drawn(), ["sweep_started", "sweep_finished"]);
    }

    #[tokio::test]
    async fn a_tail_ends_at_its_own_terminal_event_rather_than_reading_on() {
        // The sweep is over when the sweep says it is over. A drain that read on would hold
        // the process for the rest of its budget every time — and would draw an event from
        // whatever came next on a per-client stream into a bar that had already finished.
        let (session, filter) = mine();
        let watcher = Recording::default();
        let mut events = Scripted::of([
            Delivery::Event(finished(session, "focus_absolute")),
            Delivery::Event(event(session, "focus_absolute")),
        ]);

        let tail = drain_tail(&mut events, &filter, &watcher, std::time::Duration::ZERO).await;

        assert_eq!(tail, Tail::Terminal);
        assert_eq!(
            watcher.drawn(),
            ["sweep_finished"],
            "the tail read past the end of its own sweep"
        );
    }

    #[tokio::test]
    async fn another_sweeps_terminal_event_neither_draws_nor_ends_this_tail() {
        // The residual `SweepFilter`'s own doc names, at the one place it would cost an
        // event rather than an inaccuracy: `wch_subscribe_calibration` is per *client*, so a
        // second sweep's `SweepFinished` arrives on this socket, and a tail that stopped at
        // it would stop one event before the one it came for.
        let (session, filter) = mine();
        let theirs = Uuid::from_u128(2);
        let watcher = Recording::default();
        let mut events = Scripted::of([
            Delivery::Event(finished(theirs, "focus_absolute")),
            Delivery::Event(finished(session, "focus_absolute")),
        ]);

        let tail = drain_tail(&mut events, &filter, &watcher, std::time::Duration::ZERO).await;

        assert_eq!(tail, Tail::Terminal);
        assert_eq!(watcher.drawn(), ["sweep_finished"]);
        assert_eq!(watcher.sessions(), [session], "somebody else's bar moved");
    }

    #[tokio::test]
    async fn a_terminal_event_that_never_comes_ends_at_the_bound() {
        // N65's objection, which stands: a dropped terminal event never arrives, and a drain
        // that waited for one would hang this process for ever. The budget is what makes
        // that impossible — and what did arrive before the silence is still drawn, because
        // the bound ends the *waiting* and not the sweep's last visible progress.
        let (session, filter) = mine();
        let watcher = Recording::default();
        let mut events = Scripted::of([
            Delivery::Event(event(session, "focus_absolute")),
            Delivery::Silence,
        ]);

        let tail = drain_tail(&mut events, &filter, &watcher, std::time::Duration::ZERO).await;

        assert_eq!(tail, Tail::Bounded);
        assert_eq!(watcher.drawn(), ["sweep_started"]);
    }

    // ------------------------------------------------ the ordering the tail is part of

    #[tokio::test]
    async fn a_terminal_event_that_lost_the_race_to_the_answer_still_reaches_the_bar() {
        // **The defect, held still.** The daemon emits `SweepFinished` and then answers, and
        // the two travel on different tasks, so the answer can arrive first — and a client
        // that stopped reading when its call returned drew every event but the last one.
        // That is what the integration suite saw on 2 runs in 150 under load (note N69);
        // here it is the script, so it is every run.
        let (session, filter) = mine();
        let watcher = Recording::default();
        let mut events = Scripted::of([
            Delivery::Event(event(session, "focus_absolute")),
            Delivery::Late(finished(session, "focus_absolute")),
        ]);

        let answered = sweep_and_watch(
            &mut events,
            std::future::ready(answer(session)),
            filter,
            &watcher,
            std::time::Duration::ZERO,
        )
        .await;

        assert_eq!(answered, answer(session));
        assert_eq!(
            watcher.drawn(),
            ["sweep_started", "sweep_finished"],
            "the sweep's last event was left on the floor when its answer overtook it"
        );
    }

    #[tokio::test]
    async fn a_sweep_whose_events_beat_its_answer_reads_nothing_after_it() {
        // The other half of the same decision, and the one that keeps the bound off the
        // ordinary path: the terminal event arrived while the call was in flight, so there
        // is nothing outstanding and the tail is not entered at all. Asserted by counting
        // what the stream was asked — a build that always drained would ask once more, and
        // against a real daemon would wait `CLIENT_SWEEP_DRAIN_MS` for an event it already
        // had, on every sweep. (It did, for one measurement: the suite's own runtime went
        // from 0.47 s to 0.77 s, which is how it was caught.)
        let (session, filter) = mine();
        let watcher = Recording::default();
        let mut events = Scripted::of([
            Delivery::Event(event(session, "focus_absolute")),
            Delivery::Event(finished(session, "focus_absolute")),
            // The stream then says nothing, so the answer is what ends the loop.
            Delivery::Silence,
        ]);

        let answered = sweep_and_watch(
            &mut events,
            std::future::ready(answer(session)),
            filter,
            &watcher,
            std::time::Duration::ZERO,
        )
        .await;

        assert_eq!(answered, answer(session));
        assert_eq!(watcher.drawn(), ["sweep_started", "sweep_finished"]);
        assert_eq!(
            events.asked, 3,
            "the tail was entered for a sweep that had already ended"
        );
    }

    #[tokio::test]
    async fn a_stream_that_ended_mid_sweep_is_not_asked_again_for_a_tail() {
        // `watching` is the other guard, and it is not the same as `ended`: the stream is
        // gone, so there is nothing behind it to drain and asking would be asking a closed
        // subscription for the event it just told us it would never send.
        let (session, filter) = mine();
        let watcher = Recording::default();
        let mut events = Scripted::of([
            Delivery::Event(event(session, "focus_absolute")),
            Delivery::Ended,
        ]);

        let answered = sweep_and_watch(
            &mut events,
            std::future::ready(answer(session)),
            filter,
            &watcher,
            std::time::Duration::ZERO,
        )
        .await;

        assert_eq!(answered, answer(session));
        assert_eq!(watcher.drawn(), ["sweep_started"]);
        assert_eq!(
            events.asked, 2,
            "an ended stream was asked for a tail it cannot have"
        );
    }

    #[tokio::test]
    async fn another_control_in_this_session_is_drawn_but_does_not_end_this_tail() {
        // The half of the residual `admits` cannot see, because under `--session <UUID>` it
        // asks about the session and stops there: a second sweep of a **different control**
        // in this same session carries this session's id, so it is admitted — and a tail
        // that stopped at any admitted terminal event would stop at somebody else's last
        // word and abandon its own.
        let (session, filter) = mine();
        let watcher = Recording::default();
        let mut events = Scripted::of([
            Delivery::Event(finished(session, "brightness")),
            Delivery::Event(finished(session, "focus_absolute")),
        ]);

        let tail = drain_tail(&mut events, &filter, &watcher, std::time::Duration::ZERO).await;

        assert_eq!(tail, Tail::Terminal);
        // Drawn, because `admits` is what draws and the caller named the session: the
        // inaccuracy this leaves is a line on a bar, and the assertion below is the loss it
        // must not leave.
        assert_eq!(watcher.drawn(), ["sweep_finished", "sweep_finished"]);
        assert_eq!(
            watcher.controls(),
            ["brightness", "focus_absolute"],
            "the tail ended at another sweep's last word"
        );
    }

    #[tokio::test]
    async fn a_second_sweep_in_this_session_does_not_disarm_this_ones_tail() {
        // **F1, held still (note N70).** Two sweeps under one `--session S` — the camera
        // actor queues rather than refuses — so the earlier one's `SweepFinished` carries
        // *this* session's id and `admits` lets it through. A guard that took `ended` from
        // any admitted terminal event was disarmed by it, skipped the tail, and lost this
        // sweep's own last event when it lost the race to the answer.
        let (session, filter) = mine();
        let watcher = Recording::default();
        let mut events = Scripted::of([
            Delivery::Event(event(session, "focus_absolute")),
            Delivery::Event(finished(session, "brightness")),
            Delivery::Late(finished(session, "focus_absolute")),
        ]);

        let answered = sweep_and_watch(
            &mut events,
            std::future::ready(answer(session)),
            filter,
            &watcher,
            std::time::Duration::ZERO,
        )
        .await;

        assert_eq!(answered, answer(session));
        assert_eq!(
            watcher.controls(),
            ["focus_absolute", "brightness", "focus_absolute"],
            "another sweep's last word disarmed this sweep's tail"
        );
    }

    #[tokio::test]
    async fn another_cameras_sweep_of_this_control_does_not_disarm_this_ones_tail() {
        // **F1's headline, held still (note N70).** Two `wchc calibrate sweep --task framing
        // --control brightness` on two cameras: the fan-out is one per daemon, so the other
        // camera's `SweepFinished` arrives on this socket, and under `--task` the control is
        // all this process has to filter on — so it is admitted. It must not be allowed to
        // say that *this* sweep is over, and the session that decides is the one the daemon
        // names in its **answer**, which arrives exactly one step before the tail needs it.
        let mine = Uuid::from_u128(1);
        let theirs = Uuid::from_u128(2);
        let filter = SweepFilter::new(
            &SessionRef::Task {
                task: "framing".to_owned(),
            },
            &request("focus_absolute"),
        );
        let watcher = Recording::default();
        let mut events = Scripted::of([
            Delivery::Event(event(mine, "focus_absolute")),
            Delivery::Event(finished(theirs, "focus_absolute")),
            Delivery::Late(finished(mine, "focus_absolute")),
        ]);

        let answered = sweep_and_watch(
            &mut events,
            std::future::ready(answer(mine)),
            filter,
            &watcher,
            std::time::Duration::ZERO,
        )
        .await;

        assert_eq!(answered, answer(mine));
        assert_eq!(
            watcher.sessions(),
            [mine, theirs, mine],
            "another camera's sweep disarmed this sweep's tail"
        );
    }

    // ------------------------------------------- a payload this build cannot read

    #[tokio::test]
    async fn a_payload_this_build_cannot_read_is_skipped_and_the_stream_read_on() {
        // **F3, held still (note N70).** `CalibrationProgress` is an internally-tagged enum
        // with no catch-all arm, so one variant a newer `wchd` has and this `wchc` does not
        // is one `serde_json` failure — and jsonrpsee hands that back as `Some(Err(_))` on a
        // subscription that is **still open** with the rest of the sweep behind it. Reading
        // it as the end of the stream discarded every remaining event while they sat
        // decodable in the queue: a bar frozen mid-sweep, which is N69's symptom from a cause
        // N69 did not consider.
        let (session, filter) = mine();
        let watcher = Recording::default();
        let mut events = Scripted::of([
            Delivery::Event(event(session, "focus_absolute")),
            Delivery::Undecodable,
            Delivery::Event(finished(session, "focus_absolute")),
            // Silence rather than `Ended`, so nothing but the terminal event above can be
            // what keeps the tail out of this: an ended stream would skip it for its own
            // reason and this test would pass for the wrong one.
            Delivery::Silence,
        ]);

        let answered = sweep_and_watch(
            &mut events,
            std::future::ready(answer(session)),
            filter,
            &watcher,
            SWEEP_DRAIN_BUDGET,
        )
        .await;

        assert_eq!(answered, answer(session));
        assert_eq!(
            watcher.drawn(),
            ["sweep_started", "sweep_finished"],
            "one unreadable notification ended a sweep's progress"
        );
        assert_eq!(
            events.asked, 4,
            "the stream stopped being read at the payload it could not decode"
        );
    }

    #[tokio::test]
    async fn a_payload_this_build_cannot_read_does_not_disarm_the_tail() {
        // The same misreading at the other end of the same sweep, and the more expensive
        // half: an undecodable payload that set `watching = false` took the tail away too, so
        // the terminal event that lost its race to the answer had nothing left to collect it.
        // One notification this build could not read, and a sweep loses both its remaining
        // progress *and* its closing line.
        let (session, filter) = mine();
        let watcher = Recording::default();
        let mut events = Scripted::of([
            Delivery::Event(event(session, "focus_absolute")),
            Delivery::Undecodable,
            Delivery::Late(finished(session, "focus_absolute")),
        ]);

        let answered = sweep_and_watch(
            &mut events,
            std::future::ready(answer(session)),
            filter,
            &watcher,
            SWEEP_DRAIN_BUDGET,
        )
        .await;

        assert_eq!(answered, answer(session));
        assert_eq!(
            watcher.drawn(),
            ["sweep_started", "sweep_finished"],
            "an unreadable notification disarmed the tail"
        );
    }

    #[tokio::test]
    async fn a_payload_this_build_cannot_read_does_not_end_a_tail_already_running() {
        // And once inside the tail, where the same collapse would have answered `Tail::Ended`
        // — "the daemon has stopped talking" — to a stream that had this sweep's last word
        // one notification further along. Told apart from the real thing by
        // `a_stream_the_daemon_ended_ends_the_tail`, which is why both variants exist.
        let (session, filter) = mine();
        let watcher = Recording::default();
        let mut events = Scripted::of([
            Delivery::Undecodable,
            Delivery::Event(finished(session, "focus_absolute")),
        ]);

        let tail = drain_tail(&mut events, &filter, &watcher, std::time::Duration::ZERO).await;

        assert_eq!(tail, Tail::Terminal);
        assert_eq!(watcher.drawn(), ["sweep_finished"]);
    }

    // ------------------------------------------------------------- the bound as the subject

    /// A delay this bound is documented to cover.
    ///
    /// [`limits::CLIENT_SWEEP_DRAIN_MS`] is priced for "one already-woken task waiting for a
    /// core" — measured at 34 µs (note **N69**), argued to be milliseconds rather than
    /// hundreds of them on a host oversubscribed eightfold. A hundred milliseconds is three
    /// orders of magnitude past the measurement and still inside the bound, so a build that
    /// cannot deliver an event this late is a build whose budget has stopped covering the
    /// case it exists for.
    const A_MOMENT: std::time::Duration = std::time::Duration::from_millis(100);

    #[tokio::test(start_paused = true)]
    async fn the_tail_waits_the_moment_out_and_the_budget_is_what_pays_for_it() {
        // **F2, held still (note N70).** Every other test in this file hands the tail
        // `Duration::ZERO`, which drives every arm of `drain_tail` and asserts nothing at all
        // about the number the shipped path passes — so `CLIENT_SWEEP_DRAIN_MS = 0` deleted
        // N69's fix while leaving its code in place and the whole workspace suite stayed
        // green. Here the budget is the subject: the terminal event is a hundred milliseconds
        // behind the answer, on a clock this test owns, and the assertion is that the real
        // constant is enough to collect it.
        let (session, filter) = mine();
        let watcher = Recording::default();
        let mut events = Scripted::of([
            Delivery::Event(event(session, "focus_absolute")),
            Delivery::Delayed {
                after: A_MOMENT,
                event: finished(session, "focus_absolute"),
            },
        ]);

        let started = tokio::time::Instant::now();
        let answered = sweep_and_watch(
            &mut events,
            std::future::ready(answer(session)),
            filter,
            &watcher,
            SWEEP_DRAIN_BUDGET,
        )
        .await;

        assert_eq!(answered, answer(session));
        assert_eq!(
            watcher.drawn(),
            ["sweep_started", "sweep_finished"],
            "the tail refused to wait {A_MOMENT:?} for the event it exists to collect, under a \
             budget of {SWEEP_DRAIN_BUDGET:?}"
        );
        // …and it ended on the event rather than on the timer, which is the sentence
        // `CLIENT_SWEEP_DRAIN_MS`'s doc opens with ("it is a bound and not a wait") and which
        // nothing asserted: a drain that waited its bound out would cost every sweep a
        // quarter second, and that regression has already happened once (note N69, 0.47 s →
        // 0.77 s).
        assert!(
            started.elapsed() < SWEEP_DRAIN_BUDGET,
            "the tail waited out its bound instead of ending on the event: {:?}",
            started.elapsed()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_tail_with_no_budget_is_the_fix_deleted_and_that_is_visible_here() {
        // The inverse arm, and the one that makes the test above a statement about the
        // *number*: the same script under a budget of zero loses the same event. AGENTS rule
        // 2 in its own words — a test that cannot fail in the other direction is not a test —
        // and the direction that matters here is the one a mutation to zero would take.
        //
        // It is also the shape N65 argued for and N69 measured failing: a drain that refuses
        // to wait finds this client's queue provably empty, because the event it came for has
        // not arrived yet.
        let (session, filter) = mine();
        let watcher = Recording::default();
        let mut events = Scripted::of([
            Delivery::Event(event(session, "focus_absolute")),
            Delivery::Delayed {
                after: A_MOMENT,
                event: finished(session, "focus_absolute"),
            },
        ]);

        let answered = sweep_and_watch(
            &mut events,
            std::future::ready(answer(session)),
            filter,
            &watcher,
            std::time::Duration::ZERO,
        )
        .await;

        assert_eq!(answered, answer(session));
        assert_eq!(
            watcher.drawn(),
            ["sweep_started"],
            "a budget of zero collected an event that had not arrived, which is not something \
             a bound can do"
        );
    }

    #[tokio::test]
    async fn a_stream_the_daemon_ended_ends_the_tail() {
        // The third way out, and it is not the second one wearing a different name: a stream
        // that ended says "nothing further is coming" with certainty, where the bound says it
        // by running out of patience. Telling them apart is what makes both testable.
        let (_session, filter) = mine();
        let watcher = Recording::default();
        let mut events = Scripted::of([Delivery::Ended]);

        let tail = drain_tail(&mut events, &filter, &watcher, std::time::Duration::ZERO).await;

        assert_eq!(tail, Tail::Ended);
        assert!(watcher.drawn().is_empty());
    }
}
