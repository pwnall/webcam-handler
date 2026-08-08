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
use cli_core::{Cli, Executor, Output, Photograph, Stream};
use schema::backend::{BackendKind, Camera, CameraBackend};
use schema::camera::{CameraId, CameraInfo};
use schema::capture::PhotoRequest;
use schema::control::{ControlDesc, ControlSlug};
use schema::error::{Error, Result};
use schema::profile::DeviceProfile;
use schema::report::{CameraDetail, CameraList, ControlReport, WriteReport};
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

/// Say what the probe touched and what it put back, on standard error.
///
/// The `--json` document carries the *pairs*; what a probe declined and how the restore
/// went are facts about the run rather than about the camera, and a caller redirecting
/// stdout should still see them.
fn report_probe(found: &engine::discover::Discovery) {
    for (control, reason) in &found.skipped {
        eprintln!("wch: did not probe {control}: {reason}");
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
        targets: &[(ControlSlug, schema::ControlValue)],
        guarded: bool,
    ) -> Result<WriteReport> {
        let (_, mut camera) = self.open(requested)?;
        let controls = camera.controls()?;
        let pairs = self.pairs_for(&controls, Vec::new());
        engine::write::set(camera.as_mut(), &pairs, targets, guarded)
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
