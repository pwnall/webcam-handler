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
/// It has two precisions because the caller has two ways of naming a session:
///
/// - `--session <UUID>` names one, so the filter is **exact**: an event carries the id.
/// - `--task <TEXT>` names a slot, and which session occupies it is a fact only the daemon
///   holds. Until the sweep answers — which is *after* every event has been rendered — this
///   process does not know the id, so the filter falls back to the one thing the request
///   does pin down: the control being swept, which every event names
///   ([`schema::progress::CalibrationProgress::control`], an exhaustive accessor).
///
/// **The residual is stated rather than hidden:** under `--task`, a second sweep of the
/// *same control on a different camera*, running through the same daemon at the same moment,
/// would also be admitted, and the bar would show both. The alternative was to resolve the
/// task to an id with a `wch_calibrate_status` call before subscribing — rejected because it
/// changes which D13 refusal a bad `--task` produces (the status verb's, not the sweep's),
/// and the parity gate compares exactly that against `wch`. Nothing is lost but a bar's
/// accuracy, and only on a daemon running two sweeps at once.
#[derive(Debug)]
struct SweepFilter {
    /// The session, when the caller named one by id.
    session: Option<Uuid>,
    /// The control this sweep asked for.
    control: ControlSlug,
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
        }
    }

    /// Whether `event` belongs to the sweep this process asked for.
    fn admits(&self, event: &ProgressEvent) -> bool {
        match self.session {
            Some(session) => event.session == session,
            None => *event.progress.control() == self.control,
        }
    }
}

/// Where a sweep's progress comes from, so the tail can be driven without a daemon.
///
/// One method, and the seam exists for a reason a test can state: what [`drain_tail`] is
/// *for* is a race against a real daemon — an event that left the daemon before its answer
/// did and arrived after it — and a race cannot be arranged twice running. What a test can do
/// is hold each ordering still, which is what the scripted source beside the tests does.
///
/// It answers an [`Option`] rather than jsonrpsee's `Option<Result<…>>` because
/// [`Remote::calibrate_sweep`]'s loop already treats the two ends alike: a stream the daemon
/// closed and a payload this build cannot decode are both "nothing further is coming", and
/// neither is this sweep's failure.
/// It must be **cancel-safe**: [`sweep_and_watch`]'s `select!` drops the future it did not
/// take, every turn, and an implementation that consumed an event on a poll it did not
/// complete would lose one per turn of the sweep.
trait ProgressSource {
    /// The next event, or `None` when nothing further will arrive.
    async fn next_event(&mut self) -> Option<ProgressEvent>;
}

impl ProgressSource for Subscription<ProgressEvent> {
    async fn next_event(&mut self) -> Option<ProgressEvent> {
        match self.next().await {
            Some(Ok(event)) => Some(event),
            Some(Err(_)) | None => None,
        }
    }
}

/// The sweep's state machine: watch the stream while the call is in flight, then take its
/// tail.
///
/// Steps 2 to 4 of [`Remote::calibrate_sweep`], whose doc argues every one of them. It is a
/// free function, and generic over the answer's type, because the *ordering* is the subject
/// and jsonrpsee's `Result<Session, ClientError>` is not: a test hands it a
/// [`std::future::ready`] and a scripted stream, and can hold each of the two orderings still
/// — including the one that costs a bar its last line, which against a real daemon happens on
/// about one run in a hundred (note **N69**).
async fn sweep_and_watch<S, A>(
    events: &mut S,
    call: impl Future<Output = A>,
    filter: &SweepFilter,
    watch: &dyn SweepWatcher,
    budget: std::time::Duration,
) -> A
where
    S: ProgressSource,
{
    // Pinned so the loop below can poll the same future more than once without moving it; a
    // fresh call per turn would be a fresh sweep.
    let mut call = std::pin::pin!(call);
    // `false` once the stream has ended, which takes its arm out of the `select!` rather
    // than letting a closed stream answer `None` forever — the shape that would turn a
    // finished subscription into a spin.
    let mut watching = true;
    // `true` once the daemon has said its last word about this sweep, which is the whole of
    // whether the tail below has anything to wait for.
    let mut ended = false;
    let answered = loop {
        tokio::select! {
            biased;
            event = events.next_event(), if watching => match event {
                Some(event) => {
                    if filter.admits(&event) {
                        ended |= event.progress.is_terminal();
                        watch.event(&event);
                    }
                }
                // A payload this build cannot decode, or a stream the daemon ended (a lag
                // close, or a shutdown). Neither is this sweep's failure and neither is
                // worth abandoning it for: the sweep is running on the camera and its
                // answer is still coming. The bar simply stops moving, which is the honest
                // rendering of "nothing is being told to me any more".
                None => watching = false,
            },
            answered = &mut call => break answered,
        }
    };

    // Step 4, entered only when there is something outstanding: the daemon has not said its
    // last word about this sweep (`ended`) and the stream it would say it on is still open
    // (`watching`). A sweep whose terminal event beat its answer — which is nearly all of
    // them — pays nothing at all here, and the bound is never reached by a client that
    // already has what it would be waiting for.
    if watching && !ended {
        // Which of the three ways the tail ended is not this call's answer: a sweep that
        // finished is a sweep that finished whether or not its last event beat the response
        // over the socket. The value exists so each of the three has a test.
        let _tail = drain_tail(events, filter, watch, budget).await;
    }

    answered
}

/// How a sweep's tail ended — one variant per way out of [`drain_tail`], so each has a test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tail {
    /// This sweep's terminal event arrived, so everything the daemon sent was rendered.
    Terminal,
    /// The stream ended first: a lag close, a shutdown, or a payload this build could not
    /// decode. Whatever was still in flight is not coming.
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
/// hand — which is why the tests below can drive every arm of this function without a clock
/// of any kind, and why an exhausted budget can never lose an event this client was already
/// holding.
///
/// Another sweep's events are neither drawn nor allowed to end this tail: the stream is per
/// *client* (note **N57**), so a second sweep on the same daemon puts its own terminal event
/// on this socket, and treating it as ours would stop the drain one event before the one it
/// came for.
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
            Ok(None) => return Tail::Ended,
            Ok(Some(event)) => {
                if !filter.admits(&event) {
                    continue;
                }
                // Read before the watcher takes it, because `watch.event` borrows it and the
                // answer to "was that the last one" is a property of the event rather than of
                // the rendering.
                let terminal = event.progress.is_terminal();
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
    ///    events.
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
                &filter,
                watch,
                std::time::Duration::from_millis(limits::CLIENT_SWEEP_DRAIN_MS),
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
    /// [`Scripted::next_event`] so a fifth thing a real subscription can do has to be added
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
        /// The stream ends here: a lag close, a shutdown, or an undecodable payload, which
        /// [`ProgressSource`] deliberately collapses into one answer.
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
        async fn next_event(&mut self) -> Option<ProgressEvent> {
            self.asked += 1;
            match self.script.pop_front() {
                Some(Delivery::Event(event)) => Some(event),
                // Put back at the head and answer nothing *this* turn: `select!` drops this
                // future when the call answers, and the next ask — the tail's — delivers it.
                Some(Delivery::Late(event)) => {
                    self.script.push_front(Delivery::Event(event));
                    std::future::pending().await
                }
                // A script that ran out says what an ended stream says.
                Some(Delivery::Ended) | None => None,
                Some(Delivery::Silence) => std::future::pending().await,
            }
        }
    }

    /// The sweep's answer in these tests, which only has to be a value the ordering carries.
    const ANSWERED: &str = "the session";

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
            std::future::ready(ANSWERED),
            &filter,
            &watcher,
            std::time::Duration::ZERO,
        )
        .await;

        assert_eq!(answered, ANSWERED);
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
            std::future::ready(ANSWERED),
            &filter,
            &watcher,
            std::time::Duration::ZERO,
        )
        .await;

        assert_eq!(answered, ANSWERED);
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
            std::future::ready(ANSWERED),
            &filter,
            &watcher,
            std::time::Duration::ZERO,
        )
        .await;

        assert_eq!(answered, ANSWERED);
        assert_eq!(watcher.drawn(), ["sweep_started"]);
        assert_eq!(
            events.asked, 2,
            "an ended stream was asked for a tail it cannot have"
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
