//! `webcam-handler-cli` — the direct CLI. Drives a backend in-process.
//!
//! This is one of the two composition roots (design §2.11): the only places that name a
//! concrete backend. The `match` over [`BackendKind`] below is exhaustive on purpose —
//! adding a third backend stops this build until it is wired here, which is the whole
//! reason the vocabulary is closed.
//!
//! Everything the user sees comes from `webcam-handler-cli-core`. This file contributes
//! the executor and the process's edges: argument parsing, and handing a typed failure to the
//! shared surface's `cli_core::report_failure`, which owns the `--json` failure document, the
//! line on standard error and the exit code (note **N127**).
#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

use std::process::ExitCode;

use cli_core::{Cli, Executor, Output, Photograph, Program, Selection, SessionRef, SweepWatcher};
use engine::calibrate::{SweepContext, SweepRequest};
use engine::lifecycle::{self, SessionSpec};
use engine::store::SessionStore;
use schema::backend::{BackendKind, Camera, CameraBackend};
use schema::camera::CameraInfo;
use schema::capture::PhotoRequest;
use schema::control::{ControlDesc, ControlSlug};
use schema::error::Result;
use schema::pairing::ProbeSkip;
use schema::profile::DeviceProfile;
use schema::progress::ProgressEvent;
use schema::report::{CameraDetail, CameraList, ControlReport, WriteReport};
use schema::selector::CameraSelector;
use schema::session::{Session, SessionList, SessionStatus};
use schema::snapshot::{RestoreReport, Snapshot};
use schema::time::Stamp;
use schema::video::{RecordReport, RecordRequest};

/// Which root this is.
///
/// The command surface is shared with `webcam-handler-client` (T4), so the name is a parameter
/// rather than a property of the tree — see [`Program`]. One value, read by both edges of this
/// process that have to say it: the parser that renders `--help` and `--version`, and the
/// lines a failing run writes to standard error.
const PROGRAM: Program = Program::Cli;

fn main() -> ExitCode {
    let cli = Cli::parse_checked(PROGRAM);
    let mut out = Output::process();

    match run(&cli, &mut out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // The whole failure edge in one call, and the call is the shared surface's
            // (`cli_core::report_failure`, note **N127**): the typed refusal reaches standard
            // output as a `schema::error::Failure` under `--json` and standard error as the
            // one line a person reads, and the code beside it is D13's own. Written here
            // rather than in `run` because these are process concerns; written *there* rather
            // than here because `webcam-handler-client` has to produce the identical bytes,
            // which is what `scripts/gates/cli-parity.sh` now compares.
            ExitCode::from(cli_core::report_failure(
                PROGRAM, &error, cli.json, &mut out,
            ))
        }
    }
}

fn run(cli: &Cli, out: &mut Output) -> Result<()> {
    let mut executor = InProcess {
        backend: backend_for(cli)?,
    };
    cli_core::run(cli, &mut executor, out)
}

/// The one place `webcam-handler-cli` names a backend.
fn backend_for(cli: &Cli) -> Result<Box<dyn CameraBackend>> {
    match cli.backend.0 {
        BackendKind::V4l2 => Ok(Box::new(v4l2::V4l2Backend::new())),
        BackendKind::Fake => {
            // `--profile` is `required_if_eq("backend", "fake")`, so an empty list here
            // cannot come from a command line — clap refuses it as the usage error it is,
            // rather than letting it arrive as a camera error later.
            let mut profiles = Vec::with_capacity(cli.profile.len());
            for path in &cli.profile {
                // Read through the engine, which is where T3's round trip lives:
                // `webcam-handler-daemon` has the same flag at its own composition root, and a
                // version check written at each root is two answers to one question.
                profiles.push(engine::profile::read(path)?);
            }
            Ok(Box::new(fake::FakeBackend::new(profiles)?))
        }
    }
}

/// The T4 executor over an in-process backend.
///
/// Every method here is assembly: resolve an id, open the camera, ask it the questions,
/// put the answers in the schema type. No policy, because policy belongs to the engine
/// and rendering belongs to `cli-core`.
struct InProcess {
    backend: Box<dyn CameraBackend>,
}

impl InProcess {
    /// Resolve a caller-supplied selector (D1's ids and prefixes, D14's four other
    /// spellings) against a live enumeration.
    ///
    /// Enumerating first is what lets the refusal name the candidates, which is the difference
    /// between `CameraAmbiguous` being actionable and being a shrug. The rule itself lives in
    /// `engine::resolve`, so `webcam-handler-cli` and the P4 daemon cannot disagree about what
    /// a prefix means.
    fn resolve(&self, requested: &CameraSelector) -> Result<CameraInfo> {
        let cameras = self.backend.enumerate()?;
        engine::resolve::camera(&cameras, requested).cloned()
    }

    fn open(&self, requested: &CameraSelector) -> Result<(CameraInfo, Box<dyn Camera>)> {
        let info = self.resolve(requested)?;
        let camera = self.backend.open(&info.id)?;
        Ok((info, camera))
    }
}

/// The calibration half of the executor (design D8, D9).
///
/// Every mutating verb runs inside [`SessionStore::with_lock`] — take, run, release — which is
/// D9's *daemonless* protocol, and the whole of it: a `webcam-handler-cli` that met a lock a
/// daemon holds is refused with [`schema::Error::StoreLocked`] naming the holder rather than
/// blocking, and that refusal comes out of the store rather than out of a check written here.
///
/// Which session a verb means, and what a verb about to change one has to hold to read it,
/// are [`lifecycle::session_for`]'s and [`lifecycle::session_to_update`]'s — both moved into
/// the engine when the daemon acquired the same verbs over the wire, because two copies of
/// "which session did you mean" is how the check P3 found implemented once out of three
/// times gets re-implemented twice.
impl InProcess {
    /// The session store under this process's XDG state directory (note N2).
    fn store(&self) -> Result<SessionStore> {
        SessionStore::from_env(&schema::paths::SystemEnv)
    }
}

/// The engine's progress seam, fed from the command surface's.
///
/// Two traits with one method because of the thin-client wall (T6): `cli-core` is shared with
/// `webcam-handler-client`, which links no engine and cannot name
/// `engine::progress::ProgressSink`. This is the whole of the bridge — the events on both
/// sides are the same schema DTOs, so nothing is translated. P4e-i built the daemon's half
/// onto the same seam — a `ProgressSink` that fans the events out to subscribers
/// (`daemon::events`) rather than rendering them — which is why the seam is a sink and not a
/// `&dyn Fn`.
#[derive(Debug)]
struct Watched<'a>(&'a dyn SweepWatcher);

impl engine::progress::ProgressSink for Watched<'_> {
    fn emit(&self, event: &ProgressEvent) {
        self.0.event(event);
    }
}

/// Say what the probe touched and what it put back, on standard error.
///
/// One line of assembly over [`cli_core::report_probe`], which is where the rendering moved at
/// P4f: `webcam-handler-client` runs the same probe over `wch_discover_pairs` and prints the
/// same two notes, and a second copy here would be the fork design §2.10 forbids. What stays
/// local is only the program's own name, which is this root's one fact about itself.
fn report_probe(skipped: &[ProbeSkip], restored: &RestoreReport) {
    cli_core::report_probe(PROGRAM, skipped, restored);
}

impl Executor for InProcess {
    fn list(&mut self) -> Result<CameraList> {
        engine::resolve::list(self.backend.as_ref())
    }

    fn info(&mut self, requested: &CameraSelector) -> Result<CameraDetail> {
        let (info, camera) = self.open(requested)?;
        Ok(CameraDetail {
            formats: camera.formats()?,
            info,
        })
    }

    fn controls(
        &mut self,
        requested: &CameraSelector,
        discover_pairs: bool,
    ) -> Result<ControlReport> {
        let (info, mut camera) = self.open(requested)?;
        if discover_pairs {
            // The probe writes, and the document it produces is assembled in the engine —
            // probe first, read the control set afterwards, merge declared with measured.
            // `webcam-handler-daemon`'s `wch_discover_pairs` answers that whole document; this
            // surface shows its `controls` and prints the other two fields on standard error.
            // Two authors of one assembly is what note N34 booked the move against.
            let found = engine::discover::report(camera.as_mut(), Stamp::now())?;
            report_probe(&found.skipped, &found.restored);
            return Ok(found.controls);
        }
        let controls = camera.controls()?;
        Ok(ControlReport {
            // The declared table (D3) narrowed to the relationships this device can
            // exhibit. Nothing measured: measuring writes to the camera, and that is the
            // flag above (note N30).
            pairs: engine::pairing::in_effect(&controls, Vec::new()),
            camera: info.id,
            controls,
        })
    }

    fn get(&mut self, requested: &CameraSelector, control: &ControlSlug) -> Result<ControlDesc> {
        let (_, camera) = self.open(requested)?;
        // The suggestion list on a miss comes from the planner's, so `get brightnes` and
        // `set brightnes=1` name the same candidates — which is why the lookup lives in
        // the engine beside the planner rather than at each surface that offers a `get`.
        engine::pairing::describe(&camera.controls()?, control)
    }

    fn set(
        &mut self,
        requested: &CameraSelector,
        writes: &[schema::control::ControlWrite],
        guarded: bool,
    ) -> Result<WriteReport> {
        let (_, mut camera) = self.open(requested)?;
        // The composition — which pair set this write plans against, and where the wire's
        // `ControlWrite` stops (note N35) — is the engine's, because `webcam-handler-daemon`'s
        // `wch_set` reaches the same rule and a second author for it is two opinions about
        // what a camera's automation looks like.
        engine::write::set_requested(camera.as_mut(), writes, guarded)
    }

    fn snapshot(&mut self, requested: &CameraSelector) -> Result<Snapshot> {
        let (_, mut camera) = self.open(requested)?;
        engine::snapshot::take_in_effect(camera.as_mut(), Stamp::now())
    }

    fn restore(
        &mut self,
        requested: &CameraSelector,
        snapshot: &Snapshot,
    ) -> Result<RestoreReport> {
        let (_, mut camera) = self.open(requested)?;
        engine::snapshot::restore_in_effect(camera.as_mut(), snapshot)
    }

    fn photo(&mut self, requested: &CameraSelector, request: &PhotoRequest) -> Result<Photograph> {
        let (_, mut camera) = self.open(requested)?;
        let taken = engine::photo::take(
            camera.as_mut(),
            request,
            // The blocking open, which for `webcam-handler-cli` is a feature rather than note
            // N51's hazard: a person typed this path, `-o /dev/stdout` and `-o` a fifo both
            // work, and Ctrl-C exists. The daemon's destination is the other one (design §2.10
            // — one rule, two callers, and the difference is stated rather than assumed).
            &mut engine::photo::WhereverTheCallerSaid,
            &engine::settle::MonotonicClock::new(),
            Stamp::now(),
        )
        // `webcam-handler-cli` opens a camera per invocation, takes one photo and closes it,
        // so nothing in this process can be previewing the device and the gap beside the
        // answer is always `None`. It is dropped here rather than asserted: what a *different*
        // caller does with it is `daemon::server`'s business, and the suspend/resume this
        // discards the report of is the same mechanism either way (note **N83**).
        .outcome?;
        // Two structurally identical types, and they stay separate on purpose:
        // `webcam-handler-client` links no engine (T6), so the shared command surface cannot
        // name the engine's.
        Ok(Photograph {
            report: taken.report,
            returned: taken.returned,
        })
    }

    /// Record one video, holding the camera for the whole take.
    ///
    /// `engine::record::run` and nothing else — the whole verb is one engine call, exactly as
    /// `photo` is, because a second assembly in a composition root is the defect T4 and T5
    /// exist to prevent. It is the *only* caller of that function in this workspace, and its
    /// own doc says why: it holds the device from `STREAMON` to `STREAMOFF`, which
    /// `webcam-handler-daemon` cannot do — a take written as one long command would make
    /// `record_stop` undeliverable behind the recording it was trying to stop (note **N111**).
    /// This process has no actor and one verb, so holding the camera is correct here and only
    /// here.
    ///
    /// `engine::record::OnDisk` is the file seam's real implementation and the one a person
    /// gets: `-o` names a path somebody typed, `File::create` truncates it, and Ctrl-C exists.
    /// The daemon's is the other one (design §2.10 — one rule, two callers, and the difference
    /// is stated rather than assumed), because its `open(2)` would run on a camera actor's one
    /// thread (note **N51**).
    fn record(
        &mut self,
        requested: &CameraSelector,
        request: &RecordRequest,
    ) -> Result<RecordReport> {
        let (_, mut camera) = self.open(requested)?;
        engine::record::run(
            camera.as_mut(),
            request,
            &mut engine::record::OnDisk,
            // Two clocks, because they measure different things: the monotonic one bounds the
            // take's duration and the wall one stamps the report. Conflating them is how an
            // NTP step becomes a duration, which is `engine::photo::take`'s argument at the
            // same seam.
            &engine::settle::MonotonicClock::new(),
            Stamp::now(),
        )
    }

    fn calibrate_start(
        &mut self,
        requested: &CameraSelector,
        task: &str,
        goal: &str,
        criteria: &[String],
    ) -> Result<Session> {
        let (info, mut camera) = self.open(requested)?;
        let store = self.store()?;
        store.with_lock(|lock| {
            let spec = SessionSpec {
                // The clock enters here, at the composition root: the engine reads none,
                // and a UUIDv7 *is* a timestamp, which is what makes a session directory
                // sort chronologically without anything parsing a document (D9).
                id: uuid::Uuid::now_v7(),
                fingerprint: info.fingerprint.clone(),
                task: task.to_owned(),
                goal: goal.to_owned(),
                criteria: criteria.to_vec(),
                // The schema crate's reading of one fact, not this binary's:
                // `webcam-handler-daemon` records provenance into the same documents, and two
                // readings of "which tool version wrote this" could disagree.
                tool_version: schema::TOOL_VERSION.to_owned(),
            };
            let mut session = lifecycle::create(&store, lock, &spec, Stamp::now())?;
            // D3's empirical probe, at session start and nowhere else (N16). It *writes*
            // to the camera — toggling each automation-shaped control and putting it back
            // — so it happens while the camera is still where the operator left it, and
            // what it declined to touch is said out loud.
            let found = lifecycle::discover_pairs(
                &store,
                lock,
                &mut session,
                camera.as_mut(),
                Stamp::now(),
            )?;
            report_probe(&found.skipped, &found.restored);
            Ok(session)
        })
    }

    fn calibrate_plan(
        &mut self,
        requested: &CameraSelector,
        which: &SessionRef,
        controls: &[ControlSlug],
        order: bool,
    ) -> Result<Session> {
        let store = self.store()?;
        let info = self.resolve(requested)?;
        store.with_lock(|lock| {
            let mut session = lifecycle::session_to_update(&store, lock, &info.fingerprint, which)?;
            if order {
                // The camera is deliberately not opened: reordering a queue is an edit to a
                // document, and a caller who wanted to put exposure before focus should not
                // be refused because something else currently holds the device.
                lifecycle::commit_state(&store, lock, &mut session, Stamp::now(), |draft, now| {
                    engine::session::reorder_queue(draft, controls, now)
                })?;
            } else {
                // Drafting asks the *device* what it has and what it will not let this tool
                // calibrate, so this is where the camera has to open.
                let mut camera = self.backend.open(&info.id)?;
                lifecycle::draft(
                    &store,
                    lock,
                    &mut session,
                    camera.as_mut(),
                    controls,
                    Stamp::now(),
                )?;
            }
            Ok(session)
        })
    }

    fn calibrate_sweep(
        &mut self,
        requested: &CameraSelector,
        which: &SessionRef,
        request: &SweepRequest,
        watch: &dyn SweepWatcher,
    ) -> Result<Session> {
        let store = self.store()?;
        // The camera is opened before the lock deliberately: a camera nothing answers to is
        // the caller's own mistake and reporting it costs nobody the state directory. The
        // *document* is read inside, because that read is half of a read-modify-write.
        let (info, mut camera) = self.open(requested)?;
        let progress = Watched(watch);
        let clock = engine::settle::MonotonicClock::new();
        store.with_lock(|lock| {
            let mut session = lifecycle::session_to_update(&store, lock, &info.fingerprint, which)?;
            let context = SweepContext {
                store: &store,
                lock,
                clock: &clock,
                progress: &progress,
                started_at: Stamp::now(),
            };
            engine::calibrate::run(&context, &mut session, camera.as_mut(), request)?;
            Ok(session)
        })
    }

    fn calibrate_status(
        &mut self,
        requested: &CameraSelector,
        which: &SessionRef,
    ) -> Result<SessionStatus> {
        // No lock: reading is not a state write, and a `webcam-handler-cli calibrate status`
        // that refused while a daemon held the lock would be a status verb nobody can use on
        // the machine the sessions are on.
        let store = self.store()?;
        let info = self.resolve(requested)?;
        lifecycle::status(&store, &info.fingerprint, which)
    }

    fn calibrate_select(
        &mut self,
        requested: &CameraSelector,
        which: &SessionRef,
        control: &ControlSlug,
        selection: &Selection,
    ) -> Result<Session> {
        let store = self.store()?;
        let info = self.resolve(requested)?;
        store.with_lock(|lock| {
            let mut session = lifecycle::session_to_update(&store, lock, &info.fingerprint, which)?;
            // The `Selection` match is the engine's, beside the two transitions it chooses
            // between: `webcam-handler-daemon`'s `wch_calibrate_select` crosses the same
            // boundary, and a second copy is a second chance to record a selector no metric
            // earned.
            lifecycle::select(&store, lock, &mut session, control, selection, Stamp::now())?;
            Ok(session)
        })
    }

    fn calibrate_apply(
        &mut self,
        requested: &CameraSelector,
        which: &SessionRef,
        partial: bool,
    ) -> Result<WriteReport> {
        let store = self.store()?;
        let (info, mut camera) = self.open(requested)?;
        store.with_lock(|lock| {
            let mut session = lifecycle::session_to_update(&store, lock, &info.fingerprint, which)?;
            lifecycle::apply(
                &store,
                lock,
                &mut session,
                camera.as_mut(),
                partial,
                Stamp::now(),
            )
        })
    }

    fn calibrate_restore(
        &mut self,
        requested: &CameraSelector,
        which: &SessionRef,
    ) -> Result<RestoreReport> {
        let store = self.store()?;
        let (info, mut camera) = self.open(requested)?;
        store.with_lock(|lock| {
            let mut session = lifecycle::session_to_update(&store, lock, &info.fingerprint, which)?;
            // `lifecycle::restore` rather than `recover` plus two decisions written here:
            // which pair set a restore plans against (the session's, N16) and what "no
            // snapshot" means (not a failure) are rules, and `webcam-handler-daemon`'s
            // `wch_calibrate_restore` answers the same verb.
            lifecycle::restore(&store, lock, &mut session, camera.as_mut(), Stamp::now())
        })
    }

    fn calibrate_list(&mut self, requested: Option<&CameraSelector>) -> Result<SessionList> {
        let store = self.store()?;
        // Resolving first, and only when a camera was named: `None` means every session on
        // this machine, and a listing that enumerated cameras to answer it would refuse on
        // a host whose cameras have all been unplugged.
        let fingerprint = match requested {
            Some(id) => Some(self.resolve(id)?.fingerprint),
            None => None,
        };
        lifecycle::list(&store, fingerprint.as_ref())
    }

    fn capture_profile(
        &mut self,
        requested: &CameraSelector,
        capturer: &str,
        discover_pairs: bool,
    ) -> Result<DeviceProfile> {
        let (_, mut camera) = self.open(requested)?;
        // The T3 split lives in the engine, so this verb, the hardware rung's comparison,
        // and P4's `profile_capture` method all produce the same document.
        let context = engine::profile::CaptureContext {
            captured_at: schema::time::Stamp::now(),
            // Both host facts read where they have one home: the kernel release moved
            // beside the field it fills when `webcam-handler-daemon` acquired this verb,
            // and the tool version is the schema crate's, so a profile captured over a
            // socket and one captured on a command line carry the same provenance.
            kernel: engine::profile::kernel_release(),
            tool_version: schema::TOOL_VERSION.to_owned(),
            capturer: capturer.to_owned(),
            backend: self.backend.kind(),
        };
        if !discover_pairs {
            return engine::profile::capture(camera.as_mut(), &context);
        }

        // The probe wrote to the camera, so what the restore achieved goes to standard
        // error the way `controls --discover-pairs` sends it there — beside the document
        // rather than inside it. A profile is a reading of a device; whether the run that
        // took it put the device back is a fact about the run, and a caller who never hears
        // it has been handed a promise (docs/8 Part C).
        let (profile, found) = engine::profile::capture_probed(camera.as_mut(), &context)?;
        eprintln!(
            "{}: probe measured {} pair(s), declined {}, left the camera alone: {}",
            // The selector's canonical spelling, which is what the caller typed — the
            // *resolved* id is not in hand here, and naming a camera back to a caller in a
            // spelling they did not use is a note about a different string than the one they
            // are looking at.
            requested,
            profile.invariant.measured_pairs.len(),
            found.skipped.len(),
            found.left_the_camera_alone()
        );
        for skip in &found.skipped {
            eprintln!("  declined {}: {}", skip.control.as_str(), skip.reason);
        }
        Ok(profile)
    }
}
