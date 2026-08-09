//! The one webcam-handler wire surface (T5).
//!
//! One jsonrpsee `#[rpc(server, client)]` trait over `webcam-handler-schema` DTOs, plus
//! the single exhaustive match mapping the D13 error registry onto JSON-RPC codes.
//!
//! | Module | Home of |
//! |---|---|
//! | [`codes`] | the D13 → JSON-RPC code registry, both directions (D10, D13) |
//! | [`photo`] | D10's base64-in-JSON photo answer (D6, D10) |
//! | [`wire`] | the surface as data — the inventory, declared with the trait it describes |
//! | this file | the T5 trait itself — one method per operation the daemon routes |
//!
//! ## Why one trait, and why both halves in one crate
//!
//! D10 makes the whole daemon API one `#[rpc(server, client)]` trait: the daemon
//! implements the server half, `wchc` consumes the generated client, and the direct CLI
//! calls the same operations on the engine through T4's executor. A verb therefore exists
//! exactly once. Splitting the trait so the client and the server could live in different
//! crates would produce two wire surfaces, which is the thing D10 exists to prevent — and
//! it is the alternative note N5 weighed and rejected when the tokio question came up.
//!
//! ## What is not here yet
//!
//! `record_start`, `record_stop` and `record_status` join at P6 **with their tests**: D10
//! completes there and G6 says so, and a method declared before anything can exercise it
//! is a wire promise nothing keeps. `subscribe_events` (hotplug) and
//! `subscribe_calibration` (per-session progress) join at P4e with their delivery
//! semantics, for the same reason — a subscription the daemon cannot yet deliver would
//! have to be answered with `Error::Unimplemented`, which is the variant P4d deletes.
//!
//! ## Errors on the wire
//!
//! Every method answers with a [`codes::WireError`], which is the D13 registry and nothing
//! else: `code` from the closed range [`codes::D13_CODES`], `message` from the error's own
//! `Display`, `data` the serialized error. No `anyhow` string crosses this seam, and no
//! camera frame — or anything derived from one — reaches a message or a `data` field.
//! [`codes`] explains why the mapping is exhaustive over `ErrorKind` rather than over
//! `Error`; it is not a style choice.
//!
//! ## Paths on the wire
//!
//! D10: a relative `-o out.jpg` resolves against the **caller's** cwd, *before* the
//! request is sent, so it means the same file under `wch` and `wchc`. That resolution
//! lives once, in the shared command surface (`cli_core::Command::photo_request`), which
//! is why the methods here take an assembled [`schema::capture::PhotoRequest`] rather than
//! the flags one is built from: a server handed the raw flags would have to build the
//! sink, and the only cwd it has is its own — under systemd, `/`.
//!
//! Paths travelling the *other* way are the server's and are not caller-relative:
//! `SessionListing::path` is absolute in the daemon's state directory, and a sample's
//! `photo` is relative to its session directory, which lives there too (D9). For a
//! per-user UDS daemon that is the same filesystem; a browser client (P5c) can open
//! neither, and reads the documents rather than the files.
//!
//! ## The document that describes this surface
//!
//! `xtask generate` writes `schemas/webcam-handler-openrpc.json` from [`METHODS`] and from
//! [`codes::rpc_code`], and `scripts/gates/schema-artifacts-current.sh` re-runs the
//! emitter and diffs it every CI run. Like the JSON Schema bundle beside it, that file is
//! a **generated artifact — documentation, not a second source of truth**: nothing in this
//! workspace reads it back, and the types and the trait here are what the daemon actually
//! speaks. [`wire`] explains why the trait is declared through a macro, which is what
//! keeps the document's method list from being a second list of methods.
//!
//! ## N5's wall
//!
//! This crate links tokio, because `jsonrpsee-core` activates it from both its client and
//! its server features — measured across four feature sets in note N5. The half of the
//! wall that was actually protecting something is intact and gate-asserted: no axum, no
//! hyper, no tower-http. The behavioural half — that this crate never *starts* a runtime
//! or spawns a task — is review-held, because linkage cannot see it. Nothing here runs;
//! it declares.
#![forbid(unsafe_code)]
// The wire surface is request-driven end to end: every value that reaches it was written
// by somebody else and arrived over a socket.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod codes;
pub mod photo;
pub mod wire;

use schema::camera::CameraId;
use schema::capture::PhotoRequest;
use schema::control::{ControlDesc, ControlSlug, ControlWrite};
use schema::profile::DeviceProfile;
use schema::report::{
    CameraDetail, CameraList, ControlReport, DiscoveryReport, TerminationReport, WriteReport,
};
use schema::session::{Selection, Session, SessionList, SessionRef, SessionStatus, SweepRequest};
use schema::snapshot::{RestoreReport, Snapshot};

use wire::wire_surface;

pub use codes::{WireError, rpc_code};
pub use photo::{Base64Bytes, PhotoResponse};

wire_surface! {
    namespace = "wch";

    /// Everything the daemon answers (design D10, T5).
    ///
    /// Declared through [`wire`]'s `wire_surface!`, which supplies the two things that are
    /// laws rather than per-method choices — `#[rpc(server, client)]` and
    /// `param_kind = map` — and emits [`METHODS`] from the same tokens, so the document
    /// xtask writes cannot describe a surface this trait does not have.
    ///
    /// The namespace is `wch` and jsonrpsee's default separator is `_`, so the wire names are
    /// `wch_list`, `wch_calibrate_start`, and so on. Every method takes **named** parameters,
    /// and the emitted document says so (`"paramStructure": "by-name"`): a request object
    /// costs nothing on the server, and it buys a legible OpenRPC document and a hand-written
    /// web client (P5c) that reads its own requests. The generated server also accepts a
    /// positional array — but not for every method (see [`wire::Param`]), which is why the
    /// document commits to the one shape that always works rather than to "either".
    ///
    /// Every method is `async`. The daemon's implementation hands the request to the camera's
    /// actor thread (D12) and awaits a reply; `#[method(blocking)]` would put a minutes-long
    /// sweep on a tokio blocking thread *and* still queue it behind the actor, which is two
    /// queues for one device.
    ///
    /// The Rust names are the wire names, and they follow D10's spelling rather than T4's
    /// where the two differ — `profile_capture` here, `Executor::capture_profile` there. T4 is
    /// a settled surface and is not renamed for this; the two are allowed different spellings
    /// because only one of them is the wire.
    pub trait WchRpc {
        /// Every camera, plus anything worth saying about what is missing (D1).
        ///
        /// # Errors
        ///
        /// Whatever the backend says. An empty list is not an error — D1's "an empty
        /// enumeration is diagnosed, not shrugged at" is answered by the hints in the list,
        /// not by a refusal.
        #[method(name = "list")]
        async fn list(&self) -> Result<CameraList, WireError>;

        /// One camera's identity and format tree.
        ///
        /// # Errors
        ///
        /// [`schema::Error::CameraUnknown`] or [`schema::Error::CameraAmbiguous`] for an id
        /// that does not resolve; otherwise whatever the backend says.
        #[method(name = "info")]
        async fn info(&self, camera: CameraId) -> Result<CameraDetail, WireError>;

        /// One camera's control set, and the auto/manual pairs in effect for it.
        ///
        /// Read-only: the pairs it reports are the declared table filtered to this device
        /// (D3's first layer). Measuring pairs on the device is `wch_discover_pairs`,
        /// which is a different method because it writes.
        ///
        /// # Errors
        ///
        /// As `wch_info`.
        #[method(name = "controls")]
        async fn controls(&self, camera: CameraId) -> Result<ControlReport, WireError>;

        /// Measure this camera's auto/manual pairs by asking it (D3's second layer).
        ///
        /// **This writes to the camera.** The probe toggles automation-shaped controls, reads
        /// the control set back, and restores what it touched. It is its own method rather
        /// than a flag on `wch_controls` because the daemon has to route, permission and
        /// count it as the write it is — the plan routes the read verbs at P4b and this one
        /// with the mutating half at P4c, which is only expressible if they are two methods.
        /// The shared command surface keeps its `controls --discover-pairs` flag; a verb with
        /// a flag and an operation with a name are different units, and the engine still has
        /// exactly one probe.
        ///
        /// The answer is more than a [`ControlReport`] for the same reason: what the probe
        /// declined to touch, and what it could not put back, are facts about the run that
        /// `wch` prints on standard error. A caller that could not see them would be running a
        /// write with its restoration report withheld.
        ///
        /// # Errors
        ///
        /// As `wch_info`, plus whatever the camera said when the probe could not begin —
        /// a probe that cannot record where the camera started must not start.
        #[method(name = "discover_pairs")]
        async fn discover_pairs(&self, camera: CameraId) -> Result<DiscoveryReport, WireError>;

        /// One control's descriptor and current value.
        ///
        /// The whole descriptor rather than the bare value: a value with no range, no flags
        /// and no menu is not renderable, and an agent reading this needs the same context a
        /// human reading the table does.
        ///
        /// # Errors
        ///
        /// As `wch_info`, plus [`schema::Error::ControlUnknown`] naming the closest
        /// slugs this camera does have.
        #[method(name = "get")]
        async fn get(
            &self,
            camera: CameraId,
            control: ControlSlug,
        ) -> Result<ControlDesc, WireError>;

        /// Write controls, switching automation off first unless `guarded` is false (D3).
        ///
        /// The answer carries every write the plan made — the automation switch-offs
        /// included — each with its own `{requested, applied}` pair, because requested is not
        /// applied (E4) and a clamp is a warning on the result rather than an error code
        /// (D13: a warning with an error code is a success nobody can distinguish from a
        /// failure).
        ///
        /// # Errors
        ///
        /// As `wch_info`, plus the planner's refusals —
        /// [`schema::Error::ControlReadOnly`], [`schema::Error::ControlInactive`] naming the
        /// automation to disable — and the device's.
        #[method(name = "set")]
        async fn set(
            &self,
            camera: CameraId,
            writes: Vec<ControlWrite>,
            guarded: bool,
        ) -> Result<WriteReport, WireError>;

        /// Every writable control's current value (D4).
        ///
        /// # Errors
        ///
        /// As `wch_info`.
        #[method(name = "snapshot")]
        async fn snapshot(&self, camera: CameraId) -> Result<Snapshot, WireError>;

        /// Put a snapshot back, automation before manual (D4).
        ///
        /// Takes the snapshot document itself, not a path: a client reads its own file and
        /// sends the value, so `wchc restore` reads the caller's filesystem rather than the
        /// daemon's.
        ///
        /// # Errors
        ///
        /// As `wch_info`, plus [`schema::Error::FingerprintMismatch`] when the snapshot
        /// came from a different camera. A control that could not be put back is in the
        /// *report*, not an error — including the one that is a success, a control whose
        /// automation partner owns it now as it did then (note N9).
        #[method(name = "restore")]
        async fn restore(
            &self,
            camera: CameraId,
            snapshot: Snapshot,
        ) -> Result<RestoreReport, WireError>;

        /// Take one photo (D5, D6, D10).
        ///
        /// Takes an assembled [`PhotoRequest`], never the flags one is built from: the sink
        /// carries either "hand the bytes back" or an **absolute** server path, and D10 puts
        /// the relative-path resolution on the caller's side, before the request is sent. A
        /// relative `Sink::ServerPath` arriving here is a request the server must refuse
        /// rather than resolve against its own working directory, which under systemd is `/`.
        /// That rule is not restated here as prose to be remembered: it is
        /// [`schema::capture::Sink::is_addressable`], beside the variants it constrains, and
        /// the routing that lands at P4c calls it.
        ///
        /// A `ServerPath` sink is a write primitive: any client that can call this can write a
        /// file anywhere the daemon's uid can. That is deliberate and it is exactly what D11's
        /// authentication model covers — filesystem permissions on a 0700 socket directory —
        /// and it is why the opt-in TCP listener is token-gated with no flag that removes the
        /// token off loopback.
        ///
        /// # Errors
        ///
        /// As `wch_info`, plus [`schema::Error::SettleTimeout`] \[PF:11\],
        /// [`schema::Error::FormatUnsupported`] when the camera offers nothing that was asked
        /// for, and [`schema::Error::StorageIo`] from the sink.
        #[method(name = "photo")]
        async fn photo(
            &self,
            camera: CameraId,
            request: PhotoRequest,
        ) -> Result<PhotoResponse, WireError>;

        /// One camera's full device profile (T3).
        ///
        /// `capturer` is provenance: a profile records who took it, because a transcription
        /// and a probe are different claims about a device (E1, E2).
        ///
        /// # Errors
        ///
        /// As `wch_info`.
        #[method(name = "profile_capture")]
        async fn profile_capture(
            &self,
            camera: CameraId,
            capturer: String,
        ) -> Result<DeviceProfile, WireError>;

        /// Signal the process holding a camera's node (design §5).
        ///
        /// Names both the camera and the pid, and is never a fallback behaviour of anything
        /// else: nothing in this surface kills a process to get a device free. The answer says
        /// what was sent and whether the node was still held afterwards, because signalling is
        /// a request and a process may ignore it — E4's doctrine does not stop at the device.
        ///
        /// # Errors
        ///
        /// As `wch_info`, plus [`schema::Error::HolderGone`] when the pid no longer
        /// holds the device, which is a refusal rather than a no-op: signalling a pid that has
        /// been recycled would kill somebody else's process.
        #[method(name = "terminate_holder")]
        async fn terminate_holder(
            &self,
            camera: CameraId,
            pid: i32,
        ) -> Result<TerminationReport, WireError>;

        /// Open a calibration session for a camera and a task (D8).
        ///
        /// # Errors
        ///
        /// As `wch_info`, plus [`schema::Error::SessionConflict`] when the slot already
        /// holds an open session (note N14), and whatever the store refuses with —
        /// [`schema::Error::StoreLocked`] when another process owns the state directory (D9).
        #[method(name = "calibrate_start")]
        async fn calibrate_start(
            &self,
            camera: CameraId,
            task: String,
            goal: String,
            criteria: Vec<String>,
        ) -> Result<Session, WireError>;

        /// Queue controls for calibration, or reorder the queue.
        ///
        /// `controls` empty means every control the camera has. `order` treats the named
        /// controls as the queue's new order rather than as additions.
        ///
        /// # Errors
        ///
        /// As `wch_calibrate_start`, plus [`schema::Error::ControlUnknown`] for a slug
        /// this camera does not have and [`schema::Error::IllegalTransition`] when `order` is
        /// not a permutation of the queue.
        #[method(name = "calibrate_plan")]
        async fn calibrate_plan(
            &self,
            camera: CameraId,
            session: SessionRef,
            controls: Vec<ControlSlug>,
            order: bool,
        ) -> Result<Session, WireError>;

        /// Sweep one control and record a sample per value (D8).
        ///
        /// Request and response, with no progress parameter: the live events are
        /// `schema::progress::ProgressEvent`s and P4e puts them on their own subscription
        /// rather than threading a watcher through a call. Until then a client's progress bar
        /// simply does not move, which is honest; a boolean or a subscription id in this
        /// request would be a wire field with no producer and a second progress vocabulary
        /// forever.
        ///
        /// This is the one method whose latency is unbounded by design — a sweep is minutes of
        /// camera time — so a client's request timeout has to be raised or disabled at
        /// connection setup rather than left at jsonrpsee's 60-second default.
        ///
        /// # Errors
        ///
        /// As `wch_calibrate_start`, plus the planner's refusals (a control with no
        /// ordered range; a motorized control without `allow_motion`, because §5 says a plan
        /// that would move motors says so first) and whatever the camera said at the sample
        /// that stopped it.
        #[method(name = "calibrate_sweep")]
        async fn calibrate_sweep(
            &self,
            camera: CameraId,
            session: SessionRef,
            request: SweepRequest,
        ) -> Result<Session, WireError>;

        /// A session's document and its history.
        ///
        /// # Errors
        ///
        /// As `wch_info`, plus [`schema::Error::SchemaVersionForeign`] for a session
        /// another build wrote (D9) — a foreign document is a typed refusal, never a
        /// best-effort parse.
        #[method(name = "calibrate_status")]
        async fn calibrate_status(
            &self,
            camera: CameraId,
            session: SessionRef,
        ) -> Result<SessionStatus, WireError>;

        /// Record a control's chosen value and who chose it (D8).
        ///
        /// # Errors
        ///
        /// As `wch_calibrate_start`, plus the D8 machine's refusals: a control that
        /// never swept, a value no sample holds, a metric that cannot rank.
        #[method(name = "calibrate_select")]
        async fn calibrate_select(
            &self,
            camera: CameraId,
            session: SessionRef,
            control: ControlSlug,
            selection: Selection,
        ) -> Result<Session, WireError>;

        /// Write a session's calibrated values back to a camera, automation first (D4's
        /// ordering, D8's gate).
        ///
        /// # Errors
        ///
        /// As `wch_calibrate_start`, plus [`schema::Error::FingerprintMismatch`] naming
        /// the fields that differ when the camera is not the one the session was recorded
        /// against, and [`schema::Error::IllegalTransition`] when the session still has
        /// uncalibrated work and `partial` is false.
        #[method(name = "calibrate_apply")]
        async fn calibrate_apply(
            &self,
            camera: CameraId,
            session: SessionRef,
            partial: bool,
        ) -> Result<WriteReport, WireError>;

        /// Put the camera back where this session found it, from its pre-sweep snapshot
        /// (note N23).
        ///
        /// The eighth calibrate verb, and the one that makes D4's "leave the camera as you
        /// found it" true for a session rather than only for a single write. Running it twice
        /// is not an error: the second time there is nothing left to put back, and an empty
        /// report says so.
        ///
        /// # Errors
        ///
        /// As `wch_calibrate_start`, plus [`schema::Error::FingerprintMismatch`] when
        /// the camera is not the session's.
        #[method(name = "calibrate_restore")]
        async fn calibrate_restore(
            &self,
            camera: CameraId,
            session: SessionRef,
        ) -> Result<RestoreReport, WireError>;

        /// Every session on this machine, or one camera's, newest first.
        ///
        /// `camera` is the only optional parameter on this surface: `null`, or the key left
        /// out altogether, both mean every session. Measured against a real `RpcModule` —
        /// `{"params":{}}` and `{"params":{"camera":null}}` are answered identically —
        /// because serde resolves a missing `Option` field through `missing_field`, which
        /// visits `None`. The emitted document says the same thing by marking this parameter
        /// `required: false`, which is derived from its schema rather than written twice.
        ///
        /// # Errors
        ///
        /// As `wch_info` when a camera is named; otherwise whatever the store says. A
        /// session whose document this build cannot read still *lists* — listing parses
        /// nothing (D9).
        #[method(name = "calibrate_list")]
        async fn calibrate_list(&self, camera: Option<CameraId>) -> Result<SessionList, WireError>;
    }
}

#[cfg(test)]
mod tests {
    use jsonrpsee::core::async_trait;
    use schema::error::Error;

    use super::*;

    /// A stand-in that answers nothing, so the macro's own registration can be read.
    ///
    /// **Not an implementation.** P4b writes the daemon's; this exists because a Rust
    /// trait does not reify its methods, so the only authoritative statement of what the
    /// T5 surface registers is a real [`jsonrpsee::core::server::RpcModule`] built by the
    /// generated `into_rpc()` — the same mechanism docs/9 names for P4c's method-count
    /// walk, one sub-milestone early and without a daemon. Nothing here is ever called, so
    /// the refusal each method returns is arbitrary; what is not arbitrary is that the
    /// bodies typecheck, which is what proves [`WireError`] really satisfies jsonrpsee's
    /// server error bound rather than merely converting when we ask it to.
    struct AnswersNothing;

    fn nothing(operation: &str) -> WireError {
        WireError(Error::CameraUnknown {
            requested: operation.to_owned(),
        })
    }

    #[async_trait]
    impl WchRpcServer for AnswersNothing {
        async fn list(&self) -> Result<CameraList, WireError> {
            Err(nothing("list"))
        }
        async fn info(&self, _camera: CameraId) -> Result<CameraDetail, WireError> {
            Err(nothing("info"))
        }
        async fn controls(&self, _camera: CameraId) -> Result<ControlReport, WireError> {
            Err(nothing("controls"))
        }
        async fn discover_pairs(&self, _camera: CameraId) -> Result<DiscoveryReport, WireError> {
            Err(nothing("discover_pairs"))
        }
        async fn get(
            &self,
            _camera: CameraId,
            _control: ControlSlug,
        ) -> Result<ControlDesc, WireError> {
            Err(nothing("get"))
        }
        async fn set(
            &self,
            _camera: CameraId,
            _writes: Vec<ControlWrite>,
            _guarded: bool,
        ) -> Result<WriteReport, WireError> {
            Err(nothing("set"))
        }
        async fn snapshot(&self, _camera: CameraId) -> Result<Snapshot, WireError> {
            Err(nothing("snapshot"))
        }
        async fn restore(
            &self,
            _camera: CameraId,
            _snapshot: Snapshot,
        ) -> Result<RestoreReport, WireError> {
            Err(nothing("restore"))
        }
        async fn photo(
            &self,
            _camera: CameraId,
            _request: PhotoRequest,
        ) -> Result<PhotoResponse, WireError> {
            Err(nothing("photo"))
        }
        async fn profile_capture(
            &self,
            _camera: CameraId,
            _capturer: String,
        ) -> Result<DeviceProfile, WireError> {
            Err(nothing("profile_capture"))
        }
        async fn terminate_holder(
            &self,
            _camera: CameraId,
            _pid: i32,
        ) -> Result<TerminationReport, WireError> {
            Err(nothing("terminate_holder"))
        }
        async fn calibrate_start(
            &self,
            _camera: CameraId,
            _task: String,
            _goal: String,
            _criteria: Vec<String>,
        ) -> Result<Session, WireError> {
            Err(nothing("calibrate_start"))
        }
        async fn calibrate_plan(
            &self,
            _camera: CameraId,
            _session: SessionRef,
            _controls: Vec<ControlSlug>,
            _order: bool,
        ) -> Result<Session, WireError> {
            Err(nothing("calibrate_plan"))
        }
        async fn calibrate_sweep(
            &self,
            _camera: CameraId,
            _session: SessionRef,
            _request: SweepRequest,
        ) -> Result<Session, WireError> {
            Err(nothing("calibrate_sweep"))
        }
        async fn calibrate_status(
            &self,
            _camera: CameraId,
            _session: SessionRef,
        ) -> Result<SessionStatus, WireError> {
            Err(nothing("calibrate_status"))
        }
        async fn calibrate_select(
            &self,
            _camera: CameraId,
            _session: SessionRef,
            _control: ControlSlug,
            _selection: Selection,
        ) -> Result<Session, WireError> {
            Err(nothing("calibrate_select"))
        }
        async fn calibrate_apply(
            &self,
            _camera: CameraId,
            _session: SessionRef,
            _partial: bool,
        ) -> Result<WriteReport, WireError> {
            Err(nothing("calibrate_apply"))
        }
        async fn calibrate_restore(
            &self,
            _camera: CameraId,
            _session: SessionRef,
        ) -> Result<RestoreReport, WireError> {
            Err(nothing("calibrate_restore"))
        }
        async fn calibrate_list(
            &self,
            _camera: Option<CameraId>,
        ) -> Result<SessionList, WireError> {
            Err(nothing("calibrate_list"))
        }
    }

    /// Drive one of jsonrpsee's futures to its answer without a runtime.
    ///
    /// `RpcModule::raw_json_request` is `async` and this crate starts no runtime and spawns
    /// no task — N5's review-held half, and it stays true in tests too. It does not have to:
    /// a direct call over [`AnswersNothing`] awaits only the method body, which returns
    /// immediately, so a single poll with the no-op waker is the whole of it. A future that
    /// ever *did* pend here would be a future waiting on a reactor, which is exactly the
    /// thing that must not appear in this crate — so the panic below is an assertion, not a
    /// shortcut.
    fn answered(
        module: &jsonrpsee::core::server::RpcModule<AnswersNothing>,
        request: &str,
    ) -> String {
        use std::task::{Context, Poll, Waker};

        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut call = std::pin::pin!(module.raw_json_request(request, 1));
        match call.as_mut().poll(&mut context) {
            Poll::Ready(answer) => answer
                .expect("the request is well-formed JSON-RPC")
                .0
                .to_string(),
            Poll::Pending => panic!("{request} pended: something in this crate wants a runtime"),
        }
    }

    #[test]
    fn an_optional_parameter_may_be_left_out_and_a_required_one_may_not() {
        // The claim the emitted document makes — `"required": !ty.admits_absence()` — put
        // to the only authority there is: the generated server. The population is
        // `METHODS`, so a method added to the surface joins this walk by existing.
        //
        // One request shape does all of it. A by-name request with no parameters at all is
        // served exactly when every parameter of the method may be left out, so the
        // expectation is computed rather than tabulated, and both directions are present
        // in one run: `wch_list` (no parameters) and `wch_calibrate_list` (one optional)
        // are answered by the method body, and the other seventeen are refused before it.
        let module = AnswersNothing.into_rpc();
        let mut omissible = 0;
        for method in METHODS {
            let request = format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"{}","params":{{}}}}"#,
                method.name
            );
            let answer = answered(&module, &request);
            let code = serde_json::from_str::<serde_json::Value>(&answer)
                .expect("a JSON-RPC response")["error"]["code"]
                .as_i64()
                .expect("this double refuses everything, so every answer is an error");

            if method.params.iter().all(|param| param.ty.admits_absence()) {
                omissible += 1;
                assert_eq!(
                    code,
                    i64::from(rpc_code(schema::error::ErrorKind::CameraUnknown)),
                    "{} declares every parameter omissible and the server disagreed: {answer}",
                    method.name
                );
            } else {
                assert_eq!(
                    code,
                    i64::from(jsonrpsee::types::error::INVALID_PARAMS_CODE),
                    "{} has a parameter the document calls required and the server did not: \
                     {answer}",
                    method.name
                );
            }
        }
        // Not vacuous in either direction: without this, a surface whose parameters were
        // all required — or all optional — would pass while proving only one half.
        assert!(omissible > 0 && omissible < METHODS.len(), "{omissible}");
    }

    #[test]
    fn the_trait_registers_the_nineteen_wch_methods_and_nothing_else() {
        // Read off a real `RpcModule`, not off a list in this file: the macro's own
        // registration is the authority on what the wire carries, and a hand list here
        // would agree with itself forever (rubric rule 6).
        let module = AnswersNothing.into_rpc();
        let mut names: Vec<&str> = module.method_names().collect();
        names.sort_unstable();

        // Every name is namespaced. D10 says `namespace = "wch"`, and jsonrpsee's default
        // separator is `_`, so an unprefixed name here would mean a method escaped onto a
        // socket the daemon shares with nothing.
        for name in &names {
            assert!(
                name.starts_with("wch_"),
                "{name} is not in the wch namespace"
            );
        }

        // Nineteen: D10's list minus `record_*` (P6, with their tests) and minus the two
        // subscriptions (P4e, with their delivery semantics). Recorded as a number so the
        // next sub-milestone that adds one has to come here and say so — P4c's gate row
        // walks the *registered daemon module*, which is a different claim and a different
        // population.
        assert_eq!(names.len(), 19, "{names:?}");
        for absent in [
            "wch_record_start",
            "wch_record_stop",
            "wch_record_status",
            "wch_subscribe_events",
            "wch_subscribe_calibration",
        ] {
            assert!(!names.contains(&absent), "{absent} landed early");
        }

        // The spellings themselves — a *pin*, not a population, in the same tradition as
        // `fixtures/d13-rpc-codes.tsv`: a wire name is a compatibility contract in the way
        // a code is, so changing one has to be a diff somebody wrote on purpose.
        // `wch_profile_capture` is D10's spelling, not T4's `capture_profile`, and
        // `wch_discover_pairs` is its own method rather than a flag on `wch_controls`
        // because the daemon routes it with the writes.
        assert_eq!(
            names,
            vec![
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
            ]
        );
    }

    #[test]
    fn the_inventory_and_the_registration_describe_the_same_surface() {
        // Two expansions of one declaration, compared. `METHODS` is `wire_surface!`'s
        // half and `method_names()` is jsonrpsee's, and the macro deliberately does not
        // own the whole of a wire name: the namespace separator is the proc macro's
        // (`rpc_macro.rs` defaults it to `_`). So this is where `"wch"` + `"_"` + `"list"`
        // is *checked* against what the module registers rather than assumed — the one
        // fact about this surface that really is derived twice.
        let module = AnswersNothing.into_rpc();
        let mut registered: Vec<&str> = module.method_names().collect();
        registered.sort_unstable();

        let mut inventoried: Vec<&str> = METHODS.iter().map(|method| method.name).collect();
        inventoried.sort_unstable();

        assert_eq!(
            inventoried, registered,
            "the OpenRPC document would describe a different surface than the daemon serves"
        );
    }

    #[test]
    fn every_method_carries_the_prose_and_the_parameter_names_a_document_needs() {
        // xtask writes the OpenRPC document straight out of these, so a method with no
        // summary is a hole a consumer reads — and the emitter cannot invent one, because
        // the sentence it would invent is the sentence the author already wrote.
        for method in METHODS {
            let name = method.name;
            assert!(!method.summary().is_empty(), "{name} has no summary");
            // A summary is a sentence. House style opens every doc comment with one, and a
            // fragment here means the first line was a heading or a continuation — which
            // would put the wrong text in the document without failing anything else.
            assert!(
                method.summary().ends_with('.'),
                "{name}'s summary is not a sentence: {}",
                method.summary()
            );
            // Rustdoc's `# Errors` section is what tells a caller which D13 variants this
            // operation refuses with, and the document's error registry is only useful
            // beside it. A method that does not say is a method whose refusals are a
            // surprise.
            assert!(
                method.description().contains("# Errors"),
                "{name} does not document what it refuses with"
            );

            // Parameter *names* need no assertion here and deliberately have none: they
            // are `stringify!` of a Rust identifier, so an empty one is unrepresentable,
            // and a repeated one does not link — jsonrpsee's generated `ParamsObject`
            // declares a field per parameter, so two of a name is `E0124: field is
            // already declared` on top of rustc's own `E0415: identifier is bound more
            // than once in this parameter list` (measured by renaming `writes` to
            // `camera` in the `wire_surface!` invocation). An assertion whose false branch
            // cannot be reached is the smell rubric Part C names, and it would have
            // claimed credit for what the compiler does.
        }
    }
}
