//! `wch` — the direct CLI. Drives a backend in-process.
//!
//! This is one of the two composition roots (design §2.11): the only places that name a
//! concrete backend. The `match` over [`BackendKind`] below is exhaustive on purpose —
//! adding a third backend stops this build until it is wired here, which is the whole
//! reason the vocabulary is closed.
//!
//! Everything the user sees comes from `webcam-handler-cli-core`. This file contributes
//! the executor and the process's edges: argument parsing, exit code, and turning a typed
//! error into a line on standard error.
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

use camino::Utf8Path;
use cli_core::{Cli, Executor, Output, Photograph, Selection, SessionRef, Stream, SweepWatcher};
use engine::calibrate::{SweepContext, SweepRequest};
use engine::lifecycle::{self, SessionSpec};
use engine::store::{SessionStore, StoreLock};
use schema::backend::{BackendKind, Camera, CameraBackend};
use schema::camera::{CameraFingerprint, CameraId, CameraInfo};
use schema::capture::PhotoRequest;
use schema::control::{ControlDesc, ControlSlug};
use schema::error::{Error, Result};
use schema::profile::DeviceProfile;
use schema::progress::ProgressEvent;
use schema::report::{CameraDetail, CameraList, ControlReport, WriteReport};
use schema::session::{Session, SessionList, SessionListing, SessionStatus};
use schema::snapshot::{RestoreReport, Snapshot};
use schema::time::Stamp;

fn main() -> ExitCode {
    let cli = Cli::parse_checked();
    let mut out = Output::process();

    match run(&cli, &mut out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // The typed error, rendered once. `--json` consumers get the same information
            // as the document they asked for, so a failure there is still a parse failure
            // for them — which is correct: there is no answer.
            let _ = out.line(Stream::Stderr, &format!("wch: {error}"));
            ExitCode::from(cli_core::exit_code(&error))
        }
    }
}

fn run(cli: &Cli, out: &mut Output) -> Result<()> {
    let mut executor = InProcess {
        backend: backend_for(cli)?,
    };
    cli_core::run(cli, &mut executor, out)
}

/// The one place `wch` names a backend.
fn backend_for(cli: &Cli) -> Result<Box<dyn CameraBackend>> {
    match cli.backend.0 {
        BackendKind::V4l2 => Ok(Box::new(v4l2::V4l2Backend::new())),
        BackendKind::Fake => {
            // `--profile` is `required_if_eq("backend", "fake")`, so an empty list here
            // cannot come from a command line — clap refuses it as the usage error it is,
            // rather than letting it arrive as a camera error later.
            let mut profiles = Vec::with_capacity(cli.profile.len());
            for path in &cli.profile {
                profiles.push(read_profile(path)?);
            }
            Ok(Box::new(fake::FakeBackend::new(profiles)?))
        }
    }
}

fn read_profile(path: &Utf8Path) -> Result<DeviceProfile> {
    let bytes = std::fs::read(path).map_err(|error| Error::StorageIo {
        path: path.to_owned(),
        errno: error.raw_os_error(),
        message: error.to_string(),
    })?;
    let profile: DeviceProfile =
        serde_json::from_slice(&bytes).map_err(|error| Error::StorageIo {
            path: path.to_owned(),
            errno: None,
            message: format!("not a device profile: {error}"),
        })?;
    if !profile.version_is_supported() {
        return Err(Error::SchemaVersionForeign {
            found: profile.schema_version,
            supported: schema::limits::PROFILE_SCHEMA_VERSION,
        });
    }
    Ok(profile)
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
    /// Resolve a caller-supplied id or prefix (D1) against a live enumeration.
    ///
    /// Enumerating first is what lets the refusal name the candidates, which is the
    /// difference between `CameraAmbiguous` being actionable and being a shrug. The rule
    /// itself lives in `engine::resolve`, so `wch` and the P4 daemon cannot disagree
    /// about what a prefix means.
    fn resolve(&self, requested: &CameraId) -> Result<CameraInfo> {
        let cameras = self.backend.enumerate()?;
        engine::resolve::camera(&cameras, requested).cloned()
    }

    fn open(&self, requested: &CameraId) -> Result<(CameraInfo, Box<dyn Camera>)> {
        let info = self.resolve(requested)?;
        let camera = self.backend.open(&info.id)?;
        Ok((info, camera))
    }

    /// The pair set every verb plans against: the declared table (D3) merged with whatever
    /// was measured on this device (E1).
    ///
    /// One place, because a `set` that guarded against a different pair set than the
    /// `snapshot` that recorded roles would order its restore against a device the two
    /// halves disagreed about.
    fn pairs_for(
        &self,
        controls: &[ControlDesc],
        measured: Vec<schema::pairing::AutomationPair>,
    ) -> Vec<schema::pairing::AutomationPair> {
        let merged = engine::pairing::merge(schema::pairing::declared_pairs(), measured);
        engine::pairing::applicable(controls, &merged)
    }
}

/// The calibration half of the executor (design D8, D9).
///
/// Every mutating verb runs inside [`SessionStore::with_lock`] — take, run, release — which
/// is D9's *daemonless* protocol, and the whole of it: a `wch` that met a lock a daemon
/// holds is refused with [`Error::StoreLocked`] naming the holder rather than blocking, and
/// that refusal comes out of the store rather than out of a check written here.
impl InProcess {
    /// The session store under this process's XDG state directory (note N2).
    fn store(&self) -> Result<SessionStore> {
        SessionStore::from_env(&engine::paths::SystemEnv)
    }

    /// The session a verb names, loaded, and checked against the camera it names.
    ///
    /// The two forms are two lookups because they answer two questions (D8, N14): a task
    /// names the newest session in *this camera's* slot, and a UUID names one session
    /// wherever it lives — including under another camera, which is what gives the
    /// fingerprint check something to refuse.
    ///
    /// The check is `engine::lifecycle::belongs_to`, and it is here rather than in each
    /// verb because it is one law: before the P3 review only `apply` performed it, so
    /// `plan` and `sweep` could drive camera B through a `--session` naming camera A's
    /// session — recording samples measured on B in A's document, and then applying them
    /// to A with `apply`'s own check green.
    ///
    /// # Errors
    ///
    /// [`Error::IllegalTransition`] naming what was asked for when no session answers to
    /// it: `calibrate start` is the verb that makes one, and a lookup that invented an
    /// empty session would let `sweep` write to a camera with nothing recording it.
    /// [`Error::FingerprintMismatch`] naming the fields that differ when the session
    /// belongs to another camera.
    fn session_for(
        store: &SessionStore,
        fingerprint: &CameraFingerprint,
        which: &SessionRef,
    ) -> Result<Session> {
        let (found, named) = match which {
            SessionRef::Task { task } => (
                lifecycle::latest(store, fingerprint, task)?,
                format!("task={task:?}"),
            ),
            SessionRef::Id { id } => (lifecycle::find(store, *id)?, format!("session={id}")),
        };
        let session = found.ok_or_else(|| Error::IllegalTransition {
            from: format!("no_session({named})"),
            op: "read this session; `wch calibrate start` opens one".to_owned(),
        })?;
        lifecycle::belongs_to(&session, fingerprint)?;
        Ok(session)
    }

    /// The same, for a verb that is about to change something.
    ///
    /// The [`StoreLock`] is **proof, not a parameter**. D9's daemonless protocol is a
    /// read-modify-write under one advisory lock, and `SessionStore` expresses "under the
    /// lock" as a `&StoreLock` argument on every mutating method exactly so the discipline
    /// cannot be forgotten — but that argument can only guard the *write*. Each mutating
    /// calibrate verb used to load the session document immediately before `with_lock` and
    /// then commit a draft cloned from that pre-lock read, so only the write half of the
    /// cycle was protected: two concurrent `wch` processes whose windows overlapped could both
    /// exit 0 with one of them silently republishing the other's document without its
    /// samples. Requiring the token here means a caller that has not taken the lock cannot
    /// perform the read at all, which is the one kind of hold that does not decay.
    ///
    /// # Errors
    ///
    /// As [`InProcess::session_for`].
    fn session_to_update(
        store: &SessionStore,
        _lock: &StoreLock,
        fingerprint: &CameraFingerprint,
        which: &SessionRef,
    ) -> Result<Session> {
        InProcess::session_for(store, fingerprint, which)
    }
}

/// The engine's progress seam, fed from the command surface's.
///
/// Two traits with one method because of the thin-client wall (T6): `cli-core` is shared
/// with `wchc`, which links no engine and cannot name `engine::progress::ProgressSink`.
/// This is the whole of the bridge — the events on both sides are the same schema DTOs, so
/// nothing is translated, and P4e will bridge a subscription onto the same watcher.
#[derive(Debug)]
struct Watched<'a>(&'a dyn SweepWatcher);

impl engine::progress::ProgressSink for Watched<'_> {
    fn emit(&self, event: &ProgressEvent) {
        self.0.event(event);
    }
}

/// Say what the probe touched and what it put back, on standard error.
///
/// The `--json` document carries the *pairs*; what a probe declined and how the restore
/// went are facts about the run rather than about the camera, and a caller redirecting
/// stdout should still see them.
fn report_probe(found: &engine::discover::Discovery) {
    for skip in &found.skipped {
        eprintln!("wch: did not probe {}: {}", skip.control, skip.reason);
    }
    if !found.left_the_camera_alone() {
        let stuck: Vec<String> = found
            .restored
            .unrestored()
            .iter()
            .map(ToString::to_string)
            .collect();
        eprintln!(
            "wch: the probe could not put {} control(s) back: {}",
            stuck.len(),
            stuck.join(", ")
        );
    }
}

impl Executor for InProcess {
    fn list(&mut self) -> Result<CameraList> {
        Ok(CameraList {
            cameras: self.backend.enumerate()?,
            // D1: an empty enumeration is diagnosed, not shrugged at. Asked of the
            // backend, which is the only thing that knows what its own absence looks
            // like.
            hints: self.backend.diagnose(),
        })
    }

    fn info(&mut self, requested: &CameraId) -> Result<CameraDetail> {
        let (info, camera) = self.open(requested)?;
        Ok(CameraDetail {
            formats: camera.formats()?,
            info,
        })
    }

    fn controls(&mut self, requested: &CameraId, discover_pairs: bool) -> Result<ControlReport> {
        let (info, mut camera) = self.open(requested)?;
        // The probe first, because it writes: the control set reported afterwards is the
        // one the camera is actually in, and reading before the probe would describe a
        // device the caller no longer has.
        let measured = if discover_pairs {
            let found = engine::discover::pairs(camera.as_mut(), Stamp::now())?;
            report_probe(&found);
            found.pairs
        } else {
            Vec::new()
        };
        let controls = camera.controls()?;
        Ok(ControlReport {
            // The declared table (D3) narrowed to the relationships this device can
            // exhibit, merged with anything the probe measured. Every pair carries its own
            // provenance, and measured beats declared (E1) — so a caller reading `--json`
            // can tell a nomination from an observation without asking how it was made.
            pairs: engine::pairing::applicable(&controls, &self.pairs_for(&controls, measured)),
            camera: info.id,
            controls,
        })
    }

    fn get(&mut self, requested: &CameraId, control: &ControlSlug) -> Result<ControlDesc> {
        let (_, camera) = self.open(requested)?;
        let controls = camera.controls()?;
        controls
            .iter()
            .find(|desc| &desc.slug == control)
            .cloned()
            // The suggestion list comes from the planner's, so `get brightnes` and
            // `set brightnes=1` name the same candidates.
            .ok_or_else(|| {
                engine::pairing::plan_unguarded(
                    &controls,
                    &[(control.clone(), schema::ControlValue::Int(0))],
                )
                .err()
                .unwrap_or(Error::ControlUnknown {
                    requested: control.to_string(),
                    did_you_mean: Vec::new(),
                })
            })
    }

    fn set(
        &mut self,
        requested: &CameraId,
        writes: &[schema::control::ControlWrite],
        guarded: bool,
    ) -> Result<WriteReport> {
        let (_, mut camera) = self.open(requested)?;
        let controls = camera.controls()?;
        let pairs = self.pairs_for(&controls, Vec::new());
        // The engine's planner takes pairs, and this is the one place the two spellings
        // meet — see `ControlWrite`'s own note on why the wire spelling stops at the T4
        // boundary rather than continuing into `engine::pairing` (note N35).
        let targets: Vec<(ControlSlug, schema::ControlValue)> = writes
            .iter()
            .map(|write| (write.control.clone(), write.value.clone()))
            .collect();
        engine::write::set(camera.as_mut(), &pairs, &targets, guarded)
    }

    fn snapshot(&mut self, requested: &CameraId) -> Result<Snapshot> {
        let (_, mut camera) = self.open(requested)?;
        let controls = camera.controls()?;
        let pairs = self.pairs_for(&controls, Vec::new());
        engine::snapshot::take(camera.as_mut(), &pairs, Stamp::now())
    }

    fn restore(&mut self, requested: &CameraId, snapshot: &Snapshot) -> Result<RestoreReport> {
        let (_, mut camera) = self.open(requested)?;
        let controls = camera.controls()?;
        let pairs = self.pairs_for(&controls, Vec::new());
        engine::snapshot::restore(camera.as_mut(), &pairs, snapshot)
    }

    fn photo(&mut self, requested: &CameraId, request: &PhotoRequest) -> Result<Photograph> {
        let (_, mut camera) = self.open(requested)?;
        let taken = engine::photo::take(
            camera.as_mut(),
            request,
            &engine::settle::MonotonicClock::new(),
            Stamp::now(),
        )?;
        // Two structurally identical types, and they stay separate on purpose: `wchc`
        // links no engine (T6), so the shared command surface cannot name the engine's.
        Ok(Photograph {
            report: taken.report,
            returned: taken.returned,
        })
    }

    fn calibrate_start(
        &mut self,
        requested: &CameraId,
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
                tool_version: env!("CARGO_PKG_VERSION").to_owned(),
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
            report_probe(&found);
            Ok(session)
        })
    }

    fn calibrate_plan(
        &mut self,
        requested: &CameraId,
        which: &SessionRef,
        controls: &[ControlSlug],
        order: bool,
    ) -> Result<Session> {
        let store = self.store()?;
        let info = self.resolve(requested)?;
        store.with_lock(|lock| {
            let mut session = InProcess::session_to_update(&store, lock, &info.fingerprint, which)?;
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
        requested: &CameraId,
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
            let mut session = InProcess::session_to_update(&store, lock, &info.fingerprint, which)?;
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
        requested: &CameraId,
        which: &SessionRef,
    ) -> Result<SessionStatus> {
        // No lock: reading is not a state write, and a `wch calibrate status` that refused
        // while a daemon held the lock would be a status verb nobody can use on the machine
        // the sessions are on.
        let store = self.store()?;
        let info = self.resolve(requested)?;
        let session = InProcess::session_for(&store, &info.fingerprint, which)?;
        let log = lifecycle::history(&store, &session)?;
        Ok(SessionStatus { session, log })
    }

    fn calibrate_select(
        &mut self,
        requested: &CameraId,
        which: &SessionRef,
        control: &ControlSlug,
        selection: &Selection,
    ) -> Result<Session> {
        let store = self.store()?;
        let info = self.resolve(requested)?;
        store.with_lock(|lock| {
            let mut session = InProcess::session_to_update(&store, lock, &info.fingerprint, which)?;
            lifecycle::commit(&store, lock, &mut session, Stamp::now(), |draft, now| {
                // Whichever branch runs, the *selector* is recorded (D8): a metric names
                // itself and the score it earned, and a value names whoever claimed it.
                match selection {
                    Selection::ByMetric { metric } => {
                        engine::session::select_by_metric(draft, control, *metric, now)
                    }
                    Selection::ByValue { value, chosen_by } => engine::session::select_value(
                        draft,
                        control,
                        *value,
                        chosen_by.selector(),
                        now,
                    ),
                }
            })?;
            Ok(session)
        })
    }

    fn calibrate_apply(
        &mut self,
        requested: &CameraId,
        which: &SessionRef,
        partial: bool,
    ) -> Result<WriteReport> {
        let store = self.store()?;
        let (info, mut camera) = self.open(requested)?;
        store.with_lock(|lock| {
            let mut session = InProcess::session_to_update(&store, lock, &info.fingerprint, which)?;
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
        requested: &CameraId,
        which: &SessionRef,
    ) -> Result<RestoreReport> {
        let store = self.store()?;
        let (info, mut camera) = self.open(requested)?;
        store.with_lock(|lock| {
            let mut session = InProcess::session_to_update(&store, lock, &info.fingerprint, which)?;
            // The pair set comes off the *session*, where the probe wrote it at `calibrate
            // start` (N16): D4's restore ordering is automation-first, and a restore that
            // planned against a pair set the sweep did not guard with would put the camera
            // back in an order the writes it is undoing never used.
            let pairs = session.pairs.clone();
            let recovery = lifecycle::recover(
                &store,
                lock,
                &mut session,
                camera.as_mut(),
                &pairs,
                Stamp::now(),
            )?;
            // No snapshot is not a failure: it is what running this twice looks like, and
            // an empty report is the honest shape for "nothing was written".
            Ok(recovery.report().cloned().unwrap_or(RestoreReport {
                outcomes: Vec::new(),
            }))
        })
    }

    fn calibrate_list(&mut self, requested: Option<&CameraId>) -> Result<SessionList> {
        let store = self.store()?;
        let found = match requested {
            Some(id) => store.sessions_for(&self.resolve(id)?.fingerprint)?,
            None => store.all_sessions()?,
        };
        Ok(SessionList {
            sessions: found
                .into_iter()
                .map(|session| SessionListing {
                    id: session.id,
                    camera: session.camera,
                    task_slug: session.task_slug,
                    path: session.dir,
                })
                .collect(),
        })
    }

    fn capture_profile(&mut self, requested: &CameraId, capturer: &str) -> Result<DeviceProfile> {
        let (_, mut camera) = self.open(requested)?;
        // The T3 split lives in the engine, so this verb, the hardware rung's comparison,
        // and P4's `profile_capture` method all produce the same document.
        engine::profile::capture(
            camera.as_mut(),
            &engine::profile::CaptureContext {
                captured_at: schema::time::Stamp::now(),
                kernel: kernel_release(),
                tool_version: env!("CARGO_PKG_VERSION").to_owned(),
                capturer: capturer.to_owned(),
                backend: self.backend.kind(),
            },
        )
    }
}

/// `uname -r`, for the profile's provenance.
///
/// Read from `/proc/sys/kernel/osrelease` rather than by running `uname`: design §1 bans
/// runtime external binaries, and this is one line of a pseudo-file. A host without
/// `/proc` records the absence rather than a guess.
fn kernel_release() -> String {
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|text| text.trim().to_owned())
        .unwrap_or_else(|_| "(unknown)".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kernel_release_is_read_without_running_a_program() {
        // Design §1: no runtime external binaries. On this host the file is there; on a
        // host without /proc the absence is recorded rather than guessed at.
        let release = kernel_release();
        assert!(!release.is_empty());
        if std::path::Path::new("/proc/sys/kernel/osrelease").exists() {
            assert_ne!(release, "(unknown)");
            assert!(!release.contains('\n'), "{release:?} was not trimmed");
        }
    }

    #[test]
    fn a_profile_from_a_future_version_is_refused_by_version_before_anything_else() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().join("future.json"))
            .expect("utf-8 temp dir");
        std::fs::write(&path, br#"{"schema_version":99}"#).expect("write");
        // Refused for its version, not for the fields the rest of the document lacks.
        assert!(matches!(
            read_profile(&path),
            Err(Error::StorageIo { .. } | Error::SchemaVersionForeign { .. })
        ));

        let missing = path.with_file_name("nope.json");
        assert!(matches!(
            read_profile(&missing),
            Err(Error::StorageIo { .. })
        ));
    }
}
