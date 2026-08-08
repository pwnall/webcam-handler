//! The one command surface (T4).
//!
//! The clap tree, the argument types, the rendering and the `--json` contract live here
//! once; `wch` and `wchc` differ only in which [`Executor`] they hand it. A verb that
//! exists twice is a defect (design §2.10), and the P4 parity gate makes that mechanical
//! by comparing the two binaries' `--json` output byte for byte.
//!
//! ## Why the executor is a trait and not a backend
//!
//! `wchc` links no backend and no engine — that is the thin-client wall (T6). So the
//! thing this crate calls cannot *be* a backend; it is a seam with two implementations
//! living in the two binaries: an in-process engine over `Box<dyn CameraBackend>` for
//! `wch`, and a generated RPC client for `wchc` at P4. Everything above the seam —
//! argument parsing, table layout, JSON emission, exit codes — happens once, here.
//!
//! ## The `--json` contract
//!
//! `--json` emits a `webcam-handler-schema` type verbatim, and nothing else: no envelope,
//! no timestamp, no tool version. An agent parses one document whose shape is in the
//! committed bundle, and `just gate-g1` validates real output against that bundle. Human
//! output and JSON output are two renderings of the *same* value, so a fact one of them
//! shows and the other omits is a bug in the renderer rather than a feature of the mode.
#![forbid(unsafe_code)]
// The command surface is request-driven end to end: every path here is reachable from a
// command line somebody else wrote.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

mod render;

use camino::Utf8PathBuf;
use clap::{Args, Parser, Subcommand};
use schema::backend::BackendKind;
use schema::camera::CameraId;
use schema::error::{Error, Result};
use schema::profile::DeviceProfile;
use schema::report::{CameraDetail, CameraList, ControlReport};

pub use render::{Output, Stream};

/// Drive V4L2 webcams: enumerate, inspect, and capture device profiles.
#[derive(Debug, Parser)]
#[command(name = "wch", version, about, long_about = None)]
pub struct Cli {
    /// Emit the schema document for this verb instead of a table.
    #[arg(long, global = true)]
    pub json: bool,

    /// Which backend to drive.
    #[arg(long, global = true, value_name = "KIND", default_value = "v4l2")]
    pub backend: BackendKindArg,

    /// Device profiles for the fake backend to replay. Repeatable.
    #[arg(long, global = true, value_name = "PATH")]
    pub profile: Vec<Utf8PathBuf>,

    /// What to do.
    #[command(subcommand)]
    pub command: Command,
}

/// `--backend` as clap sees it. A newtype rather than a `ValueEnum` derive on
/// [`BackendKind`], because the vocabulary's spelling belongs to the schema
/// (`BackendKind::as_str`) and a derive would put a second copy of it in the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendKindArg(pub BackendKind);

impl std::str::FromStr for BackendKindArg {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        BackendKind::parse(s).map(BackendKindArg).ok_or_else(|| {
            let known: Vec<&str> = BackendKind::ALL.iter().map(|k| k.as_str()).collect();
            format!(
                "unknown backend {s:?}; known backends: {}",
                known.join(", ")
            )
        })
    }
}

/// A camera id or unambiguous prefix, as typed (D1).
#[derive(Debug, Clone, Args)]
pub struct CameraArg {
    /// The camera: `cam:obsbot-tiny-3`, or any unambiguous prefix such as `cam:obsbot`.
    #[arg(value_name = "CAMERA")]
    pub camera: String,
}

impl CameraArg {
    /// The id to resolve.
    ///
    /// # Errors
    ///
    /// [`Error::CameraUnknown`] for an argument that is not an id at all — the empty
    /// string. Anything else is resolved against the live enumeration, which is where a
    /// name nothing answers to becomes an error that can list what *does* exist.
    pub fn id(&self) -> Result<CameraId> {
        CameraId::parse(&self.camera).ok_or_else(|| Error::CameraUnknown {
            requested: self.camera.clone(),
        })
    }
}

/// The verbs. P1 lands the read half; the rest arrive with their phases (docs/2).
#[derive(Debug, Subcommand)]
pub enum Command {
    /// List the cameras attached to this machine.
    List,

    /// Show one camera's identity, nodes, and format tree.
    Info(CameraArg),

    /// Show one camera's full control set.
    Controls(CameraArg),

    /// Capture device profiles.
    #[command(subcommand)]
    Profile(ProfileCommand),
}

/// `wch profile …`
#[derive(Debug, Subcommand)]
pub enum ProfileCommand {
    /// Capture a camera's profile: everything enumerable about it, as one document.
    Capture {
        /// Which camera.
        #[command(flatten)]
        camera: CameraArg,

        /// Where to write it. Standard output when omitted.
        #[arg(long, short, value_name = "PATH")]
        out: Option<Utf8PathBuf>,

        /// Who to record as having taken the capture (T3 provenance).
        #[arg(long, value_name = "WHO", default_value = "unattributed")]
        capturer: String,
    },
}

/// What a binary must be able to do for the command surface to work.
///
/// Deliberately narrow: every method answers with a schema value, and none of them
/// renders, prints, or decides an exit code. `wch` implements it over an in-process
/// engine; `wchc` will implement it over the generated RPC client at P4, and the parity
/// gate then proves the two produce identical `--json`.
pub trait Executor {
    /// Every camera, plus anything worth saying about what is missing (D1).
    ///
    /// # Errors
    ///
    /// Whatever the backend says. An empty list is not an error.
    fn list(&mut self) -> Result<CameraList>;

    /// One camera's identity and format tree.
    ///
    /// # Errors
    ///
    /// [`Error::CameraUnknown`] or [`Error::CameraAmbiguous`] for an id that does not
    /// resolve; otherwise whatever the backend says.
    fn info(&mut self, camera: &CameraId) -> Result<CameraDetail>;

    /// One camera's control set.
    ///
    /// # Errors
    ///
    /// As [`Executor::info`].
    fn controls(&mut self, camera: &CameraId) -> Result<ControlReport>;

    /// One camera's full device profile (T3).
    ///
    /// # Errors
    ///
    /// As [`Executor::info`].
    fn capture_profile(&mut self, camera: &CameraId, capturer: &str) -> Result<DeviceProfile>;
}

/// Run a parsed command against `executor`, writing to `out`.
///
/// # Errors
///
/// The executor's error, unrendered — the caller decides how a failure reaches the user
/// and what it exits with, because those are process concerns and this is a library.
pub fn run<E: Executor>(cli: &Cli, executor: &mut E, out: &mut Output) -> Result<()> {
    match &cli.command {
        Command::List => {
            let list = executor.list()?;
            render::list(&list, cli.json, out)
        }
        Command::Info(arg) => {
            let detail = executor.info(&arg.id()?)?;
            render::info(&detail, cli.json, out)
        }
        Command::Controls(arg) => {
            let report = executor.controls(&arg.id()?)?;
            render::controls(&report, cli.json, out)
        }
        Command::Profile(ProfileCommand::Capture {
            camera,
            out: destination,
            capturer,
        }) => {
            let profile = executor.capture_profile(&camera.id()?, capturer)?;
            render::profile(&profile, destination.as_deref(), out)
        }
    }
}

/// The exit code a failure leaves behind.
///
/// Two codes, not eighteen: a caller who wants to branch on *which* thing went wrong
/// reads `--json`, where the whole typed error is. Shell exit codes are a one-bit channel
/// and pretending otherwise invites a script to treat `2` as meaningful when the registry
/// grows.
#[must_use]
pub fn exit_code(_error: &Error) -> u8 {
    1
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory as _;

    use super::*;

    #[test]
    fn the_command_tree_is_well_formed() {
        // clap's own consistency check: duplicate arguments, conflicting shorts, and
        // malformed help all fail here rather than at somebody's first invocation.
        Cli::command().debug_assert();
    }

    #[test]
    fn the_backend_argument_speaks_the_schemas_vocabulary_and_no_other() {
        for &kind in BackendKind::ALL {
            assert_eq!(
                kind.as_str().parse::<BackendKindArg>().expect("known"),
                BackendKindArg(kind)
            );
        }
        let error = "v4l3"
            .parse::<BackendKindArg>()
            .expect_err("an unknown backend must not parse");
        // The refusal lists what does exist, derived from the vocabulary rather than
        // transcribed — so a third backend needs no edit here.
        for &kind in BackendKind::ALL {
            assert!(error.contains(kind.as_str()), "{error}");
        }
    }

    #[test]
    fn the_read_verbs_parse_the_way_the_agent_guide_will_teach_them() {
        let cli = Cli::try_parse_from(["wch", "list"]).expect("parses");
        assert!(matches!(cli.command, Command::List));
        assert!(!cli.json);
        assert_eq!(cli.backend, BackendKindArg(BackendKind::V4l2));

        let cli = Cli::try_parse_from(["wch", "--json", "info", "cam:obsbot"]).expect("parses");
        assert!(cli.json);
        let Command::Info(arg) = &cli.command else {
            panic!("expected info");
        };
        assert_eq!(arg.id().expect("an id").as_str(), "cam:obsbot");

        // The prefix D1 promises, and the `cam:` prefix being optional on input.
        let cli = Cli::try_parse_from(["wch", "controls", "obsbot"]).expect("parses");
        let Command::Controls(arg) = &cli.command else {
            panic!("expected controls");
        };
        assert_eq!(arg.id().expect("an id").as_str(), "cam:obsbot");

        let cli = Cli::try_parse_from(["wch", "profile", "capture", "cam:x", "-o", "p.json"])
            .expect("parses");
        let Command::Profile(ProfileCommand::Capture { out, capturer, .. }) = &cli.command else {
            panic!("expected profile capture");
        };
        assert_eq!(out.as_deref(), Some(camino::Utf8Path::new("p.json")));
        assert_eq!(capturer, "unattributed");
    }

    #[test]
    fn the_fake_backend_is_selectable_with_the_profiles_it_replays() {
        let cli = Cli::try_parse_from([
            "wch",
            "--backend",
            "fake",
            "--profile",
            "a.json",
            "--profile",
            "b.json",
            "list",
        ])
        .expect("parses");
        assert_eq!(cli.backend, BackendKindArg(BackendKind::Fake));
        assert_eq!(cli.profile.len(), 2);
    }

    #[test]
    fn an_empty_camera_argument_is_refused_rather_than_resolved() {
        let arg = CameraArg {
            camera: String::new(),
        };
        assert!(matches!(arg.id(), Err(Error::CameraUnknown { .. })));
    }

    #[test]
    fn every_error_kind_leaves_a_nonzero_exit_code() {
        for &kind in schema::error::ErrorKind::ALL {
            assert_ne!(exit_code(&Error::sample(kind)), 0, "{kind:?}");
        }
    }
}
