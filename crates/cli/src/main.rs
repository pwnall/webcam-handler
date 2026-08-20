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
use engine::facade::Facade;
use engine::lifecycle::{self, SessionSpec};
use engine::store::SessionStore;
use schema::backend::{BackendKind, CameraBackend};
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
    // A document verb takes files and answers a document (design §2.7, D15): it touches no
    // camera, so this root does not name a backend for it. Asked *before* `backend_for`
    // rather than left to `cli_core::run` — which asks the same question again — because
    // `--backend fake --profile …` reads and version-checks a corpus document at that call,
    // and `profile compare` refusing over a profile it was never going to replay would be
    // this root deciding something the shared surface does not.
    if let Some(answered) = cli_core::below_the_executor(cli, out) {
        return answered;
    }
    let mut executor = InProcess {
        facade: Facade::new(backend_for(cli)?),
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

/// The T4 executor over an in-process backend, written as parse-and-render around
/// [`engine::facade`] (design **D18**).
///
/// Every one-shot verb below is **one facade call** and the argument conversion around it.
/// That is the shape D18 asks for and the reason it asks: the blessed call order — resolve,
/// open, ask, assemble — used to live here, in a binary's private executor, so the first
/// embedder that was neither the owner nor the owner's agent harness had to read a CLI to
/// learn it. It now lives in `engine::facade`, and this file is what stops the two from
/// becoming siblings: the facade is the code `webcam-handler-cli` actually ships, so
/// `scripts/gates/cli-parity.sh` — which compares this root with `webcam-handler-client` byte
/// for byte on every read verb — pins the facade's answers transitively.
/// `scripts/gates/facade-is-the-composition.sh` is the other half, and it goes red on the
/// event that would undo this: an executor verb reaching an engine module the facade
/// encapsulates.
///
/// **Two verbs keep their own assembly, and it is a boundary rather than an omission.**
/// `record` holds the device from `STREAMON` to `STREAMOFF` and the calibration verbs run
/// inside the session store's fd-lock; both are *stateful lifecycles*, which D18 excludes
/// from the facade on purpose — "a facade method that half-owned a session would be a second
/// lifecycle home", which is the defect §2.10 exists to prevent. An embedder that wants those
/// wants the daemon or this binary, and this binary is entitled to them because it *is* one of
/// the two blessed compositions (design §2.11).
///
/// The lifecycles this file assembles itself are `engine::record`, `engine::store`,
/// `engine::lifecycle`, `engine::session`, `engine::calibrate` and `engine::progress`,
/// and that list is the policy `scripts/gates/facade-is-the-composition.sh` declares at the
/// top of itself — reconciled against this sentence in both directions, so neither copy can
/// carry a name the other has dropped and neither can quietly grow one. The reconciliation is
/// what makes the two copies worth having: this is where a reader learns *why* the names are
/// kept, and note **N269** records what the unreconciled pair cost.
///
/// Engine paths that are *not* lifecycles are named below too, and they are allowed rather
/// than excused: `engine::settle`'s monotonic clock, which the two lifecycles above take as an
/// argument, and the reaches that belong to the composition root rather than to a verb —
/// `engine::profile::read`, which builds the fake backend out of a corpus document, and
/// `engine::photo::WhereverTheCallerSaid`, the destination seam a facade caller supplies. How
/// many there are is not written here, because the same predicate prints every one of them on
/// every run: the allowance is visible rather than inferred from silence, and counted where a
/// count can go stale without anybody noticing.
///
/// Note that the excluded verbs still *select* their camera through the facade —
/// [`Facade::open`], [`Facade::resolve`], [`Facade::open_id`]. What D18 excludes is the
/// lifecycle, not the camera selection D14 gave one home.
struct InProcess {
    facade: Facade,
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
        self.facade.list()
    }

    fn info(&mut self, requested: &CameraSelector) -> Result<CameraDetail> {
        self.facade.detail(requested)
    }

    fn controls(
        &mut self,
        requested: &CameraSelector,
        discover_pairs: bool,
    ) -> Result<ControlReport> {
        if discover_pairs {
            // A different verb, not a flag on this one, because it *writes* to the camera
            // (note N30) — and the document it produces is assembled in the engine: probe
            // first, read the control set afterwards, merge declared with measured.
            // `webcam-handler-daemon`'s `wch_discover_pairs` answers that whole document; this
            // surface shows its `controls` and prints the other two fields on standard error.
            let found = self.facade.discover_pairs(requested, Stamp::now())?;
            report_probe(&found.skipped, &found.restored);
            return Ok(found.controls);
        }
        self.facade.controls(requested)
    }

    fn get(&mut self, requested: &CameraSelector, control: &ControlSlug) -> Result<ControlDesc> {
        self.facade.get(requested, control)
    }

    fn set(
        &mut self,
        requested: &CameraSelector,
        writes: &[schema::control::ControlWrite],
        guarded: bool,
    ) -> Result<WriteReport> {
        self.facade.set(requested, writes, guarded)
    }

    fn snapshot(&mut self, requested: &CameraSelector) -> Result<Snapshot> {
        self.facade.snapshot(requested, Stamp::now())
    }

    fn restore(
        &mut self,
        requested: &CameraSelector,
        snapshot: &Snapshot,
    ) -> Result<RestoreReport> {
        self.facade.restore(requested, snapshot)
    }

    fn photo(&mut self, requested: &CameraSelector, request: &PhotoRequest) -> Result<Photograph> {
        let taken = self.facade.photo(
            requested,
            request,
            // Where the bytes go is the one thing the facade takes as a parameter rather than
            // deciding, and its module doc says why: it is a fact about the *caller's process*
            // rather than about the camera. This one blocks on a path a person typed, which
            // for `webcam-handler-cli` is a feature rather than note N51's hazard —
            // `-o /dev/stdout` and `-o` a fifo both work, and Ctrl-C exists. The daemon passes
            // the other implementation, because its `open(2)` would run on a camera actor's
            // one thread (design §2.10 — one rule, two callers, the difference stated).
            &mut engine::photo::WhereverTheCallerSaid,
            Stamp::now(),
        )?;
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
    /// **The one verb whose engine reach is not the facade, and D18 excludes it on purpose.**
    /// A take is a lifecycle rather than a one-shot: it holds the device for its whole
    /// duration, so a facade method that owned one would own a claim on hardware across a
    /// call boundary — which is the second lifecycle home §2.10 forbids, and the reason the
    /// module doc names recording beside calibration. The camera is still *selected* through
    /// the facade; only the take itself is assembled here.
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
        let (_, mut camera) = self.facade.open(requested)?;
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
        let (info, mut camera) = self.facade.open(requested)?;
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
        let info = self.facade.resolve(requested)?;
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
                let mut camera = self.facade.open_id(&info.id)?;
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
        let (info, mut camera) = self.facade.open(requested)?;
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
        let info = self.facade.resolve(requested)?;
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
        let info = self.facade.resolve(requested)?;
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
        let (info, mut camera) = self.facade.open(requested)?;
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
        let (info, mut camera) = self.facade.open(requested)?;
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
            Some(id) => Some(self.facade.resolve(id)?.fingerprint),
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
        if !discover_pairs {
            // The T3 split and the provenance block are both the engine's, so this verb, the
            // hardware rung's comparison and the daemon's `profile_capture` produce the same
            // document — the three host facts included. The clock is still this root's,
            // because the engine reads none (design §2.10).
            return self.facade.profile(requested, capturer, Stamp::now());
        }

        // The probe writes to the camera, so what the restore achieved goes to standard
        // error the way `controls --discover-pairs` sends it there — beside the document
        // rather than inside it. A profile is a reading of a device; whether the run that
        // took it put the device back is a fact about the run, and a caller who never hears
        // it has been handed a promise (docs/8 Part C).
        let (profile, found) = self
            .facade
            .profile_probed(requested, capturer, Stamp::now())?;
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
