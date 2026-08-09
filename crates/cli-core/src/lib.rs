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

use camino::{Utf8Path, Utf8PathBuf};
use clap::{Args, Parser, Subcommand};
use schema::backend::BackendKind;
use schema::camera::{CameraId, PixelFormat};
use schema::capture::{
    PhotoFormat, PhotoRequest, SettlePolicy, SettleSpec, Sink, StreamRequest, Transform,
};
use schema::control::{ControlDesc, ControlSlug, ControlValue};
use schema::error::{Error, Result};
use schema::profile::DeviceProfile;
use schema::report::{CameraDetail, CameraList, ControlReport, WriteReport};
use schema::snapshot::{RestoreReport, Snapshot};

pub use photograph::Photograph;

/// The photo answer, and its bytes when the caller asked for them.
///
/// Defined here rather than imported from the engine: `wchc` links no engine (T6), and the
/// command surface both binaries share cannot name a type only one of them can see.
mod photograph {
    use schema::capture::PhotoReport;

    /// A photo, and — for a `ReturnBytes` sink — its bytes.
    #[derive(Debug)]
    pub struct Photograph {
        /// What was taken, where it went, and what was done to it.
        pub report: PhotoReport,
        /// The bytes, when the sink asked for them rather than for a file.
        pub returned: Option<Vec<u8>>,
    }
}

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
    ///
    /// Required with `--backend fake`, and enforced by clap rather than at run time: a
    /// backend with nothing to replay enumerates nothing, and "no cameras" is exactly
    /// what a user whose cameras had vanished would see. A usage mistake must not be
    /// spelled like a device answer.
    #[arg(
        long,
        global = true,
        value_name = "PATH",
        required_if_eq("backend", "fake")
    )]
    pub profile: Vec<Utf8PathBuf>,

    /// What to do.
    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    /// Parse the process's arguments, applying the rules clap's attributes cannot express.
    ///
    /// Exits the way clap exits, which is the point: a usage mistake leaves code 2 and a
    /// device refusal leaves code 1, and a script deciding whether to retry needs them
    /// apart.
    #[must_use]
    pub fn parse_checked() -> Cli {
        match Cli::try_parse_checked_from(std::env::args_os()) {
            Ok(cli) => cli,
            Err(error) => error.exit(),
        }
    }

    /// [`Cli::parse_checked`] over an explicit argument list, for tests.
    ///
    /// # Errors
    ///
    /// clap's own error, for a parse failure or for one of the cross-argument rules.
    pub fn try_parse_checked_from<I, T>(args: I) -> std::result::Result<Cli, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        let cli = Cli::try_parse_from(args)?;
        cli.check()?;
        Ok(cli)
    }

    /// The cross-argument rules, in clap's error type so they still exit 2.
    ///
    /// There is one, and it is here rather than as an attribute because `--json` is a
    /// **global** argument: clap's `required_if_eq` resolves the arg it names within the
    /// command that declares it, and a subcommand cannot name a flag defined on the root.
    /// Written out rather than worked around, so the rule is visible and so the refusal is
    /// still a usage error rather than a device one.
    fn check(&self) -> std::result::Result<(), clap::Error> {
        use clap::CommandFactory as _;

        if self.json
            && let Command::Photo { out: None, .. } = &self.command
        {
            return Err(Cli::command().error(
                clap::error::ErrorKind::MissingRequiredArgument,
                "photo --json needs --out <PATH>: with no path the photo's bytes are \
                 standard output, and the JSON document cannot share it",
            ));
        }
        Ok(())
    }
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

/// `--transform` and `--format`, as clap sees them.
///
/// Newtypes over the schema's own spellings for the reason [`BackendKindArg`] is one: a
/// `ValueEnum` derive would put a second copy of the vocabulary in the CLI, and the two
/// would drift the first time a variant was renamed. The schema's `as_str` is asserted
/// equal to its serde rendering, so `--transform hflip` and `"transform":"hflip"` are the
/// same string by construction.
macro_rules! vocabulary_arg {
    ($name:ident, $inner:ty, $what:literal) => {
        #[doc = concat!("`--", $what, "` as clap sees it.")]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name(pub $inner);

        impl std::str::FromStr for $name {
            type Err = String;

            fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
                <$inner>::parse(s).map($name).ok_or_else(|| {
                    let known: Vec<&str> = <$inner>::ALL.iter().map(|v| v.as_str()).collect();
                    format!("unknown {} {s:?}; known: {}", $what, known.join(", "))
                })
            }
        }
    };
}

vocabulary_arg!(TransformArg, Transform, "transform");
vocabulary_arg!(PhotoFormatArg, PhotoFormat, "format");

/// `CONTROL=VALUE`, as typed on a `set` command line.
///
/// Parsed by clap rather than at run time, so a malformed assignment is a usage error
/// (exit 2) rather than a device error (exit 1) — "you typed it wrong" and "the camera is
/// busy" are different kinds of failure and a script deciding whether to retry needs to
/// tell them apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    /// Which control.
    pub control: schema::control::ControlSlug,
    /// The value to write.
    pub value: schema::control::ControlValue,
}

impl std::str::FromStr for Assignment {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let (name, value) = s
            .split_once('=')
            .ok_or_else(|| format!("{s:?} is not CONTROL=VALUE"))?;
        let control = schema::control::ControlSlug::parse(name)
            .ok_or_else(|| format!("{s:?} names no control"))?;
        // Integers only, and deliberately: every scalar control on every device the
        // corpus knows takes one, a menu takes an index, and the compound controls that
        // do not take an integer take opaque bytes nobody types on a command line. A
        // value this cannot parse is refused here, where the message can say so, rather
        // than becoming a device refusal three layers down.
        let value = value.parse::<i64>().map_err(|_| {
            format!("{value:?} is not an integer; control values are written as integers")
        })?;
        Ok(Assignment {
            control,
            value: schema::control::ControlValue::Int(value),
        })
    }
}

/// `WxH`, as typed on a `--size` flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizeArg {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl std::str::FromStr for SizeArg {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let (width, height) = s
            .split_once(['x', 'X'])
            .ok_or_else(|| format!("{s:?} is not WxH, such as 1920x1080"))?;
        Ok(SizeArg {
            width: width
                .parse()
                .map_err(|_| format!("{width:?} is not a width"))?,
            height: height
                .parse()
                .map_err(|_| format!("{height:?} is not a height"))?,
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

/// The verbs. P1 landed the read half, P2 the write and capture halves; calibration,
/// the daemon and recording arrive with their phases (docs/2).
#[derive(Debug, Subcommand)]
pub enum Command {
    /// List the cameras attached to this machine.
    List,

    /// Show one camera's identity, nodes, and format tree.
    Info(CameraArg),

    /// Show one camera's full control set.
    Controls {
        /// Which camera.
        #[command(flatten)]
        camera: CameraArg,

        /// Toggle each automation-shaped control and record what it freezes \[PF:3\].
        ///
        /// **This writes to the camera.** It snapshots first and restores after, and the
        /// report says what the restore achieved — but it is a probe, not a read, and the
        /// flag is what makes that visible on the command line.
        #[arg(long)]
        discover_pairs: bool,
    },

    /// Read one control's full descriptor and current value.
    Get {
        /// Which camera.
        #[command(flatten)]
        camera: CameraArg,

        /// Which control, by slug.
        #[arg(value_name = "CONTROL")]
        control: String,
    },

    /// Write one or more controls.
    Set {
        /// Which camera.
        #[command(flatten)]
        camera: CameraArg,

        /// The writes, as `CONTROL=VALUE`. Applied in the order given.
        #[arg(value_name = "CONTROL=VALUE", required = true)]
        assignments: Vec<Assignment>,

        /// Write without switching automation partners off first (D3).
        ///
        /// The write still goes out — real hardware accepts it and lets the automation
        /// overwrite it on the next frame \[PF:3\] — and the read-back reports what
        /// actually stuck, so the futility is visible rather than hidden.
        #[arg(long)]
        no_guard: bool,
    },

    /// Record every writable control's current value (D4).
    Snapshot {
        /// Which camera.
        #[command(flatten)]
        camera: CameraArg,

        /// Where to write it. Standard output when omitted.
        #[arg(long, short, value_name = "PATH")]
        out: Option<Utf8PathBuf>,
    },

    /// Put a snapshot back, automation first (D4).
    Restore {
        /// Which camera.
        #[command(flatten)]
        camera: CameraArg,

        /// The snapshot document.
        #[arg(value_name = "PATH")]
        snapshot: Utf8PathBuf,
    },

    /// Take a photo.
    Photo {
        /// Which camera.
        #[command(flatten)]
        camera: CameraArg,

        /// Where to write it. The extension chooses the encoding; standard output when
        /// omitted, which `--json` therefore does not allow — see
        /// [`Cli::try_parse_checked_from`], which enforces it.
        #[arg(long, short, value_name = "PATH")]
        out: Option<Utf8PathBuf>,

        /// The encoding, when there is no path to take it from.
        #[arg(long, value_name = "FORMAT", default_value = "jpeg")]
        format: PhotoFormatArg,

        /// Rotate or mirror. On a pass-through JPEG this is an EXIF tag, not a re-encode
        /// (E6).
        #[arg(long, value_name = "TRANSFORM", default_value = "none")]
        transform: TransformArg,

        /// The frame size to ask the device for, as `WxH`.
        #[arg(long, value_name = "WxH")]
        size: Option<SizeArg>,

        /// The pixel format to ask the device for, as a fourcc such as `MJPG`.
        #[arg(long, value_name = "FOURCC")]
        pixel_format: Option<String>,

        /// Discard this many frames before taking one \[PF:11\].
        #[arg(long, value_name = "N", conflicts_with = "settle_for")]
        skip_frames: Option<u32>,

        /// Discard frames for this long before taking one, in milliseconds.
        #[arg(long, value_name = "MS")]
        settle_for: Option<u64>,

        /// How long the whole settle may take, in milliseconds.
        #[arg(long, value_name = "MS")]
        settle_deadline: Option<u64>,
    },

    /// Capture device profiles.
    #[command(subcommand)]
    Profile(ProfileCommand),
}

impl Command {
    /// The photo request a `photo` invocation describes, and where its bytes go.
    ///
    /// `cwd` is the caller's directory, passed in rather than read: D10 says a relative
    /// `-o` resolves against the *caller's* cwd, and at P4 the caller is on the other end
    /// of a socket. Resolving it here — in the shared command surface — is what makes
    /// `wch photo -o out.jpg` and `wchc photo -o out.jpg` mean the same file.
    ///
    /// # Errors
    ///
    /// [`Error::IllegalTransition`] when the output path's extension names an encoding
    /// this build does not write, naming both the extension and the three it does.
    /// Refused here rather than downstream, and deliberately *not* as
    /// `FormatUnsupported`: that variant is the camera saying what it cannot offer, and
    /// `.webp` is not the camera's fault (E3).
    pub fn photo_request(&self, cwd: &camino::Utf8Path) -> Result<Option<PhotoRequest>> {
        let Command::Photo {
            out,
            format,
            transform,
            size,
            pixel_format,
            skip_frames,
            settle_for,
            settle_deadline,
            ..
        } = self
        else {
            return Ok(None);
        };

        let sink = match out {
            None => Sink::ReturnBytes { format: format.0 },
            Some(path) => {
                let absolute = if path.is_absolute() {
                    path.clone()
                } else {
                    cwd.join(path)
                };
                if let Some(extension) = absolute.extension()
                    && PhotoFormat::from_extension(extension).is_none()
                {
                    // Not `FormatUnsupported`: that variant is the *camera* saying what it
                    // does not offer, and blaming a webcam for `.webp` is exactly the
                    // availability-versus-capability confusion E3 exists to prevent. This
                    // is a usage refusal, and it names both halves — the extension that
                    // was typed and the three this build writes — because an error that
                    // says neither leaves the caller to guess.
                    return Err(Error::IllegalTransition {
                        from: format!("unwritable_extension({extension})"),
                        op: format!(
                            "write a photo to {absolute}; this build writes {}",
                            PhotoFormat::ALL
                                .iter()
                                .map(|f| format!(".{}", f.extension()))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    });
                }
                Sink::ServerPath { path: absolute }
            }
        };

        let spec = match (skip_frames, settle_for) {
            (Some(frames), _) => SettleSpec::SkipFrames { frames: *frames },
            (None, Some(millis)) => SettleSpec::SettleFor { millis: *millis },
            (None, None) => SettleSpec::default(),
        };

        Ok(Some(PhotoRequest {
            stream: StreamRequest {
                pixel_format: pixel_format.as_deref().and_then(PixelFormat::parse),
                width: size.map(|s| s.width),
                height: size.map(|s| s.height),
                interval: None,
                buffer_count: schema::limits::DEFAULT_BUFFER_COUNT,
            },
            settle: SettlePolicy {
                spec,
                deadline_ms: settle_deadline.unwrap_or(schema::limits::DEFAULT_SETTLE_DEADLINE_MS),
            },
            transform: transform.0,
            sink,
        }))
    }
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

    /// One camera's control set, and the auto/manual pairs in effect for it.
    ///
    /// `discover_pairs` runs D3's empirical probe, which **writes to the camera** and
    /// restores it afterwards. It is a parameter rather than a second method because the
    /// answer has the same shape either way — the difference is the provenance on the
    /// pairs, which is data the caller can read.
    ///
    /// # Errors
    ///
    /// As [`Executor::info`].
    fn controls(&mut self, camera: &CameraId, discover_pairs: bool) -> Result<ControlReport>;

    /// One control's descriptor and current value.
    ///
    /// The whole descriptor rather than the bare value: a value with no range, no flags
    /// and no menu is not renderable, and an agent reading `--json` needs the same
    /// context a human reading the table does.
    ///
    /// # Errors
    ///
    /// As [`Executor::info`], plus [`Error::ControlUnknown`] naming the closest slugs.
    fn get(&mut self, camera: &CameraId, control: &ControlSlug) -> Result<ControlDesc>;

    /// Write controls, switching automation off first unless `guarded` is false (D3).
    ///
    /// # Errors
    ///
    /// As [`Executor::info`], plus the planner's refusals and the device's.
    fn set(
        &mut self,
        camera: &CameraId,
        targets: &[(ControlSlug, ControlValue)],
        guarded: bool,
    ) -> Result<WriteReport>;

    /// Every writable control's current value (D4).
    ///
    /// # Errors
    ///
    /// As [`Executor::info`].
    fn snapshot(&mut self, camera: &CameraId) -> Result<Snapshot>;

    /// Put a snapshot back (D4).
    ///
    /// # Errors
    ///
    /// As [`Executor::info`], plus [`Error::FingerprintMismatch`] when the snapshot came
    /// from a different camera. A control that could not be put back is in the *report*,
    /// not an error.
    fn restore(&mut self, camera: &CameraId, snapshot: &Snapshot) -> Result<RestoreReport>;

    /// Take one photo (D5, D6).
    ///
    /// The bytes ride beside the report rather than in it, for a `ReturnBytes` sink: a
    /// `Vec<u8>` inside a `--json` document needs an encoding only the wire surface needs
    /// (D10, P4), and here the caller can simply be handed them.
    ///
    /// # Errors
    ///
    /// As [`Executor::info`], plus [`Error::SettleTimeout`] and whatever the sink says.
    fn photo(&mut self, camera: &CameraId, request: &PhotoRequest) -> Result<Photograph>;

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
        Command::Controls {
            camera,
            discover_pairs,
        } => {
            let report = executor.controls(&camera.id()?, *discover_pairs)?;
            render::controls(&report, cli.json, out)
        }
        Command::Get { camera, control } => {
            let slug = ControlSlug::parse(control).ok_or_else(|| Error::ControlUnknown {
                requested: control.clone(),
                did_you_mean: Vec::new(),
            })?;
            let desc = executor.get(&camera.id()?, &slug)?;
            render::control(&desc, cli.json, out)
        }
        Command::Set {
            camera,
            assignments,
            no_guard,
        } => {
            let targets: Vec<(ControlSlug, ControlValue)> = assignments
                .iter()
                .map(|a| (a.control.clone(), a.value.clone()))
                .collect();
            let report = executor.set(&camera.id()?, &targets, !*no_guard)?;
            render::writes(&report, cli.json, out)
        }
        Command::Snapshot {
            camera,
            out: destination,
        } => {
            let snapshot = executor.snapshot(&camera.id()?)?;
            render::snapshot(&snapshot, destination.as_deref(), out)
        }
        Command::Restore { camera, snapshot } => {
            let document = read_snapshot(snapshot)?;
            let report = executor.restore(&camera.id()?, &document)?;
            render::restore(&report, cli.json, out)
        }
        Command::Photo { camera, .. } => {
            let cwd = current_directory()?;
            let request = cli
                .command
                .photo_request(&cwd)?
                .ok_or_else(unreachable_photo)?;
            let taken = executor.photo(&camera.id()?, &request)?;
            render::photo(&taken.report, taken.returned.as_deref(), cli.json, out)
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

/// The caller's directory, for D10's relative-path rule.
///
/// # Errors
///
/// [`Error::StorageIo`] when the process has no readable cwd — a directory deleted out
/// from under a running shell, which is rare and is not something to paper over with a
/// guess at `/`.
fn current_directory() -> Result<Utf8PathBuf> {
    let cwd = std::env::current_dir().map_err(|error| Error::StorageIo {
        path: "<cwd>".into(),
        errno: error.raw_os_error(),
        message: error.to_string(),
    })?;
    Utf8PathBuf::from_path_buf(cwd).map_err(|path| Error::StorageIo {
        path: "<cwd>".into(),
        errno: None,
        message: format!("the current directory {path:?} is not UTF-8"),
    })
}

/// Read a snapshot document, refusing a foreign one by *shape* rather than by field.
///
/// # Errors
///
/// [`Error::StorageIo`] naming the path, for a file that is missing or is not a snapshot.
fn read_snapshot(path: &Utf8Path) -> Result<Snapshot> {
    let bytes = std::fs::read(path).map_err(|error| Error::StorageIo {
        path: path.to_owned(),
        errno: error.raw_os_error(),
        message: error.to_string(),
    })?;
    serde_json::from_slice(&bytes).map_err(|error| Error::StorageIo {
        path: path.to_owned(),
        errno: None,
        message: format!("not a snapshot document: {error}"),
    })
}

/// The refusal for a `photo` arm that is not a photo — which the match above has already
/// ruled out, and which exists so the dispatch has no `unwrap` on it.
fn unreachable_photo() -> Error {
    Error::IllegalTransition {
        from: "not_a_photo_command".to_owned(),
        op: "build a photo request".to_owned(),
    }
}

/// The exit code a failure leaves behind.
///
/// **One code for every D13 error**, not eighteen. A caller who wants to branch on *which*
/// thing went wrong reads `--json`, where the whole typed error is; shell exit codes are a
/// one-bit channel, and mapping a growing registry onto small integers invites a script to
/// treat `2` as meaningful and then break when a nineteenth variant lands.
///
/// The process therefore has three outcomes, and only the first two come from here:
///
/// | Code | Meaning |
/// |---|---|
/// | 0 | the verb answered |
/// | 1 | a typed [`Error`] — the camera, the device, or the filesystem said no |
/// | 2 | clap's own: the command line was not a command line |
///
/// 2 is clap's convention and is left to it deliberately. "You typed it wrong" and "the
/// camera is busy" are different kinds of failure, and a script that retries the second
/// should not retry the first.
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
        let Command::Controls {
            camera,
            discover_pairs,
        } = &cli.command
        else {
            panic!("expected controls");
        };
        assert_eq!(camera.id().expect("an id").as_str(), "cam:obsbot");
        assert!(
            !discover_pairs,
            "the probe is opt-in: it writes to the camera"
        );

        let cli = Cli::try_parse_from(["wch", "profile", "capture", "cam:x", "-o", "p.json"])
            .expect("parses");
        let Command::Profile(ProfileCommand::Capture { out, capturer, .. }) = &cli.command else {
            panic!("expected profile capture");
        };
        assert_eq!(out.as_deref(), Some(camino::Utf8Path::new("p.json")));
        assert_eq!(capturer, "unattributed");
    }

    #[test]
    fn the_fake_backend_cannot_be_selected_without_something_to_replay() {
        // The inverse of the test below, and the reason `required_if_eq` is on the
        // argument: a fake backend with no documents enumerates nothing, which reads
        // exactly like a machine whose cameras disappeared.
        let error = Cli::try_parse_from(["wch", "--backend", "fake", "list"])
            .expect_err("--backend fake without --profile must not parse");
        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::MissingRequiredArgument,
            "{error}"
        );
        assert!(error.to_string().contains("--profile"), "{error}");

        // …and the default backend needs no profile at all.
        assert!(Cli::try_parse_from(["wch", "list"]).is_ok());
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
    fn the_write_verbs_parse_the_way_the_agent_guide_will_teach_them() {
        let cli = Cli::try_parse_from(["wch", "get", "cam:x", "brightness"]).expect("parses");
        let Command::Get { control, .. } = &cli.command else {
            panic!("expected get");
        };
        assert_eq!(control, "brightness");

        let cli = Cli::try_parse_from(["wch", "set", "cam:x", "brightness=200", "contrast=10"])
            .expect("parses");
        let Command::Set {
            assignments,
            no_guard,
            ..
        } = &cli.command
        else {
            panic!("expected set");
        };
        assert_eq!(
            assignments
                .iter()
                .map(|a| (a.control.as_str(), a.value.clone()))
                .collect::<Vec<_>>(),
            vec![
                ("brightness", ControlValue::Int(200)),
                ("contrast", ControlValue::Int(10)),
            ]
        );
        assert!(!no_guard, "the guard is the default (D3)");

        let cli =
            Cli::try_parse_from(["wch", "snapshot", "cam:x", "-o", "s.json"]).expect("parses");
        assert!(matches!(cli.command, Command::Snapshot { .. }));
        let cli = Cli::try_parse_from(["wch", "restore", "cam:x", "s.json"]).expect("parses");
        assert!(matches!(cli.command, Command::Restore { .. }));
    }

    #[test]
    fn a_malformed_assignment_is_refused_at_parse_time_naming_what_was_typed() {
        // A usage error rather than a device error: exit 2 rather than 1, so a script
        // that retries on "the camera is busy" does not retry on a typo.
        for bad in ["brightness", "brightness=high", "=5"] {
            let error = Cli::try_parse_from(["wch", "set", "cam:x", bad])
                .expect_err("a malformed assignment must not parse");
            assert_eq!(
                error.kind(),
                clap::error::ErrorKind::ValueValidation,
                "{bad}"
            );
        }
        // …and a well-formed one, negative values included, does parse.
        let cli = Cli::try_parse_from(["wch", "set", "cam:x", "pan_absolute=-468000"])
            .expect("negative control values are ordinary");
        let Command::Set { assignments, .. } = &cli.command else {
            panic!("expected set");
        };
        assert_eq!(assignments[0].value, ControlValue::Int(-468_000));
    }

    #[test]
    fn the_photo_vocabularies_are_the_schemas_and_an_unknown_name_lists_the_known_ones() {
        for &transform in Transform::ALL {
            let cli = Cli::try_parse_from([
                "wch",
                "photo",
                "cam:x",
                "--transform",
                transform.as_str(),
                "-o",
                "a.jpg",
            ])
            .unwrap_or_else(|error| panic!("{} should parse: {error}", transform.as_str()));
            let Command::Photo { transform: got, .. } = &cli.command else {
                panic!("expected photo");
            };
            assert_eq!(got.0, transform);
        }

        let error = Cli::try_parse_from([
            "wch",
            "photo",
            "cam:x",
            "--transform",
            "rot45",
            "-o",
            "a.jpg",
        ])
        .expect_err("rot45 is not a transform");
        let text = error.to_string();
        for &transform in Transform::ALL {
            assert!(text.contains(transform.as_str()), "{text}");
        }
    }

    #[test]
    fn json_photo_needs_a_path_because_the_bytes_and_the_document_share_one_stream() {
        // Without `-o`, the photo's bytes *are* standard output. Emitting a JSON document
        // there too would produce a file that is neither. clap refuses it, so the answer
        // is a usage error rather than a corrupt image.
        let error = Cli::try_parse_checked_from(["wch", "--json", "photo", "cam:x"])
            .expect_err("--json without -o must not parse");
        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
        assert!(error.to_string().contains("--out"), "{error}");

        // Both halves of the inverse: `-o` with `--json`, and no `--json` without `-o`.
        assert!(
            Cli::try_parse_checked_from(["wch", "--json", "photo", "cam:x", "-o", "a.jpg"]).is_ok()
        );
        assert!(Cli::try_parse_checked_from(["wch", "photo", "cam:x"]).is_ok());
        // And the rule is about `photo` alone: every other verb answers in JSON with no
        // path at all, which is the whole point of `--json`.
        assert!(Cli::try_parse_checked_from(["wch", "--json", "list"]).is_ok());
    }

    #[test]
    fn a_relative_output_path_is_resolved_against_the_callers_directory() {
        // D10: `-o out.jpg` means the caller's directory in `wch` and in `wchc` alike, and
        // the resolution happens here — in the shared surface — so the two cannot differ.
        let cli =
            Cli::try_parse_from(["wch", "photo", "cam:x", "-o", "shots/a.jpg"]).expect("parses");
        let request = cli
            .command
            .photo_request(camino::Utf8Path::new("/home/someone"))
            .expect("builds")
            .expect("a photo command produces a request");
        assert_eq!(
            request.sink,
            Sink::ServerPath {
                path: "/home/someone/shots/a.jpg".into()
            }
        );

        // An absolute path is left alone.
        let cli =
            Cli::try_parse_from(["wch", "photo", "cam:x", "-o", "/tmp/a.jpg"]).expect("parses");
        let request = cli
            .command
            .photo_request(camino::Utf8Path::new("/home/someone"))
            .expect("builds")
            .expect("a request");
        assert_eq!(
            request.sink,
            Sink::ServerPath {
                path: "/tmp/a.jpg".into()
            }
        );
    }

    #[test]
    fn an_output_extension_this_build_cannot_write_is_refused_before_the_camera_is_opened() {
        let cli = Cli::try_parse_from(["wch", "photo", "cam:x", "-o", "/tmp/a.webp"])
            .expect("clap does not know about encodings");
        let error = cli
            .command
            .photo_request(camino::Utf8Path::new("/tmp"))
            .expect_err("webp is not one of the three");

        // Not `FormatUnsupported` — that is the camera saying what it does not offer, and
        // blaming a webcam for `.webp` is the availability-versus-capability confusion E3
        // exists to prevent.
        assert_eq!(error.kind(), schema::ErrorKind::IllegalTransition);
        let rendered = error.to_string();
        assert!(rendered.contains("webp"), "the extension typed: {rendered}");
        for format in schema::PhotoFormat::ALL {
            assert!(
                rendered.contains(format.extension()),
                "the formats it does write: {rendered}"
            );
        }
    }

    #[test]
    fn the_settle_flags_build_the_policy_and_conflict_where_they_should() {
        let cli = Cli::try_parse_from([
            "wch",
            "photo",
            "cam:x",
            "-o",
            "a.jpg",
            "--skip-frames",
            "3",
            "--settle-deadline",
            "250",
            "--size",
            "1920x1080",
            "--pixel-format",
            "MJPG",
        ])
        .expect("parses");
        let request = cli
            .command
            .photo_request(camino::Utf8Path::new("/tmp"))
            .expect("builds")
            .expect("a request");
        assert_eq!(request.settle.spec, SettleSpec::SkipFrames { frames: 3 });
        assert_eq!(request.settle.deadline_ms, 250);
        assert_eq!(
            (request.stream.width, request.stream.height),
            (Some(1920), Some(1080))
        );
        assert_eq!(request.stream.pixel_format, PixelFormat::parse("MJPG"));

        // The two settle specs are alternatives, not a pair: honouring both would need a
        // rule about which wins, and clap refusing is a better answer than inventing one.
        assert!(
            Cli::try_parse_from([
                "wch",
                "photo",
                "cam:x",
                "-o",
                "a.jpg",
                "--skip-frames",
                "3",
                "--settle-for",
                "100",
            ])
            .is_err()
        );

        // With neither, the limits table decides.
        let plain = Cli::try_parse_from(["wch", "photo", "cam:x", "-o", "a.jpg"]).expect("parses");
        let request = plain
            .command
            .photo_request(camino::Utf8Path::new("/tmp"))
            .expect("builds")
            .expect("a request");
        assert_eq!(request.settle, SettlePolicy::default());
    }

    #[test]
    fn a_malformed_size_is_refused_naming_the_shape_it_wanted() {
        for bad in ["1920", "1920*1080", "wide x tall"] {
            assert!(
                Cli::try_parse_from(["wch", "photo", "cam:x", "-o", "a.jpg", "--size", bad])
                    .is_err(),
                "{bad} should not parse as a size"
            );
        }
        assert!(
            Cli::try_parse_from(["wch", "photo", "cam:x", "-o", "a.jpg", "--size", "640x480"])
                .is_ok()
        );
    }

    #[test]
    fn every_error_kind_leaves_the_same_nonzero_exit_code_distinct_from_claps() {
        for &kind in schema::error::ErrorKind::ALL {
            let code = exit_code(&Error::sample(kind));
            assert_ne!(code, 0, "{kind:?} would look like success");
            // Distinct from clap's usage code: "you typed it wrong" and "the camera is
            // busy" must not be the same answer to a script deciding whether to retry.
            assert_ne!(code, 2, "{kind:?} collides with clap's usage exit code");
            assert_eq!(code, 1, "{kind:?} differs from the other kinds");
        }
    }
}
