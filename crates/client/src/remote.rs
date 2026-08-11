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
//! belongs to.

use std::future::Future;

use api::codes;
use api::{WchEventsClient as _, WchRpcClient as _};
use camino::{Utf8Path, Utf8PathBuf};
use cli_core::{Executor, Photograph, Selection, SessionRef, SweepWatcher};
use jsonrpsee::core::ClientError;
use jsonrpsee::core::client::{Client, ClientBuilder};
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
        // socket; no timer is enabled here on purpose — `jsonrpsee-core/async-client`
        // enables `tokio/time` for itself, and this line stays a statement about what *this*
        // binary needs.
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
    /// 3. **Answer, with nothing left to drain.** There is deliberately no drain after the
    ///    loop, and the absence is a *consequence of the runtime choice* rather than an
    ///    omission: on a current-thread runtime the connection's background task can only
    ///    run while this future is awaiting, so nothing can be pushed onto the subscription
    ///    **between** the two polls of one `select!` turn. Biased ordering polls the events
    ///    first, so any event the client already holds is delivered before the answer is
    ///    taken, and a poll after the break could only ever find the queue empty. Measured
    ///    as well as argued: a counting drain, kept temporarily, read zero events on every
    ///    one of five runs.
    ///
    ///    An event the *daemon* writes after its response — its fan-out and its answer are
    ///    two tasks over there — is not waited for either, and must not be: waiting for a
    ///    terminal event would hang forever whenever one was dropped, and dropping is a
    ///    thing `wch_subscribe_calibration` is explicitly allowed to do (it counts losses
    ///    and carries on, note N57). In practice the answer loses that race by construction,
    ///    having several hops of its own left to make after the last event is emitted.
    ///    Nothing anywhere here waits on a clock.
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
            // Step 2. Pinned so the loop below can poll the same future more than once
            // without moving it; a fresh call per turn would be a fresh sweep.
            let call = client.calibrate_sweep(camera.clone(), which.clone(), request.clone());
            let mut call = std::pin::pin!(call);

            // `false` once the stream has ended, which takes its arm out of the `select!`
            // rather than letting a closed stream answer `None` forever — the shape that
            // would turn a finished subscription into a spin.
            let mut watching = true;
            let answered = loop {
                tokio::select! {
                    biased;
                    event = events.next(), if watching => match event {
                        Some(Ok(event)) => {
                            if filter.admits(&event) {
                                watch.event(&event);
                            }
                        }
                        // A payload this build cannot decode, or a stream the daemon ended
                        // (a lag close, or a shutdown). Neither is this sweep's failure and
                        // neither is worth abandoning it for: the sweep is running on the
                        // camera and its answer is still coming. The bar simply stops
                        // moving, which is the honest rendering of "nothing is being told
                        // to me any more".
                        Some(Err(_)) | None => watching = false,
                    },
                    answered = &mut call => break answered,
                }
            };

            // No drain after the loop, and that absence is **proved rather than assumed**
            // — see this method's doc, step 3.
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
}
