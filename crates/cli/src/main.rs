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
use clap::Parser as _;
use cli_core::{Cli, Executor, Output, Stream};
use schema::backend::{BackendKind, Camera, CameraBackend};
use schema::camera::{CameraId, CameraInfo};
use schema::error::{Error, Result};
use schema::profile::DeviceProfile;
use schema::report::{CameraDetail, CameraList, ControlReport};

fn main() -> ExitCode {
    let cli = Cli::parse();
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
            // The fake replays documents, so it needs to be given some. Refusing an
            // empty list beats enumerating nothing and letting the user conclude their
            // cameras vanished.
            if cli.profile.is_empty() {
                return Err(Error::CameraUnknown {
                    requested: "--backend fake with no --profile".to_owned(),
                });
            }
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
    /// Enumerating first means the refusal can name the candidates, which is the
    /// difference between `CameraAmbiguous` being actionable and being a shrug.
    fn resolve(&self, requested: &CameraId) -> Result<CameraInfo> {
        let cameras = self.backend.enumerate()?;
        let ids: Vec<CameraId> = cameras.iter().map(|c| c.id.clone()).collect();
        match schema::camera::resolve_prefix(&ids, requested.as_str()) {
            schema::camera::PrefixMatch::Unique(id) => cameras
                .into_iter()
                .find(|c| c.id == id)
                .ok_or_else(|| Error::CameraUnknown {
                    requested: requested.to_string(),
                }),
            schema::camera::PrefixMatch::None => Err(Error::CameraUnknown {
                requested: requested.to_string(),
            }),
            schema::camera::PrefixMatch::Ambiguous(candidates) => Err(Error::CameraAmbiguous {
                requested: requested.to_string(),
                candidates,
            }),
        }
    }

    fn open(&self, requested: &CameraId) -> Result<(CameraInfo, Box<dyn Camera>)> {
        let info = self.resolve(requested)?;
        let camera = self.backend.open(&info.id)?;
        Ok((info, camera))
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

    fn controls(&mut self, requested: &CameraId) -> Result<ControlReport> {
        let (info, camera) = self.open(requested)?;
        Ok(ControlReport {
            camera: info.id,
            controls: camera.controls()?,
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
    fn the_fake_backend_refuses_to_start_with_nothing_to_replay() {
        // Enumerating zero cameras from a backend nobody gave any documents would look
        // exactly like a machine whose cameras vanished.
        let cli = Cli::try_parse_from(["wch", "--backend", "fake", "list"]).expect("parses");
        assert!(matches!(
            backend_for(&cli),
            Err(Error::CameraUnknown { .. })
        ));
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
