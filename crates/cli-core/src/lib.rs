//! The one command surface (T4).
//!
//! The clap tree, the argument types, the rendering and the `--json` contract live here once;
//! `webcam-handler-cli` and `webcam-handler-client` differ only in which [`Executor`] they
//! hand it. A verb that exists twice is a defect (design §2.10), and the P4 parity gate makes
//! that mechanical by comparing the two binaries' `--json` output byte for byte.
//!
//! ## Why the executor is a trait and not a backend
//!
//! `webcam-handler-client` links no backend and no engine — that is the thin-client wall (T6).
//! So the thing this crate calls cannot *be* a backend; it is a seam with two implementations
//! living in the two binaries: an in-process engine over `Box<dyn CameraBackend>` for
//! `webcam-handler-cli`, and a generated RPC client for `webcam-handler-client` at P4.
//! Everything above the seam — argument parsing, table layout, JSON emission, exit codes —
//! happens once, here.
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

pub mod contracts;
mod render;

use camino::{Utf8Path, Utf8PathBuf};
use clap::{Args, Parser, Subcommand};
use schema::backend::BackendKind;
use schema::camera::{CameraId, PixelFormat};
use schema::capture::{
    PhotoFormat, PhotoRequest, SettlePolicy, SettleSpec, Sink, StreamRequest, Transform,
};
use schema::control::{ControlDesc, ControlSlug, ControlWrite};
use schema::error::{Error, Result};
use schema::metrics::MetricName;
use schema::profile::DeviceProfile;
use schema::report::{CameraDetail, CameraList, ControlReport, WriteReport};
use schema::session::{Session, SessionList, SessionStatus, SweepRequest, SweepSpec};
use schema::snapshot::{RestoreReport, Snapshot};
use schema::video::{RecordReport, RecordRequest};
use schema::vocabulary::closed_vocabulary;

// The wire and the command line name the same session and the same selection, so they are one
// type in the schema rather than two that drift (design §2.10). Re-exported rather than
// aliased so `cli_core::SessionRef` — the spelling every `Executor` signature and both
// binaries already use — keeps meaning something, and so `webcam-handler-client` can hand what
// it parsed straight to the T5 client at P4f.
pub use schema::session::{ChosenBy, Selection, SessionRef};

pub use photograph::Photograph;
pub use render::{SweepWatcher, report_probe};

/// The photo answer, and its bytes when the caller asked for them.
///
/// Defined here rather than imported from the engine: `webcam-handler-client` links no engine
/// (T6), and the command surface both binaries share cannot name a type only one of them can
/// see.
mod photograph {
    use schema::capture::PhotoReport;

    /// A photo, and — for a `ReturnBytes` sink — its bytes.
    ///
    /// `Debug` is hand-written for the reason `schema::capture::Frame`'s is: **a frame may
    /// contain a person** (AGENTS.md; rubric A12). A derived one would print the whole
    /// JPEG into the first `tracing::debug!(?photograph)` or `format!("{photograph:?}")`
    /// anybody adds, and nothing could go red on it.
    pub struct Photograph {
        /// What was taken, where it went, and what was done to it.
        pub report: PhotoReport,
        /// The bytes, when the sink asked for them rather than for a file.
        pub returned: Option<Vec<u8>>,
    }

    impl std::fmt::Debug for Photograph {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            /// The byte count, wearing the only `Debug` a payload may have.
            ///
            /// Going through `Option`'s own `Debug` keeps `Some(…)` and `None` apart,
            /// which is the difference between "a file was written" and "an empty payload
            /// came back".
            struct ByteCount(usize);
            impl std::fmt::Debug for ByteCount {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "<{} bytes>", self.0)
                }
            }

            // A frame may contain a person. The count, and never the bytes.
            f.debug_struct("Photograph")
                .field("report", &self.report)
                .field(
                    "returned",
                    &self.returned.as_ref().map(|bytes| ByteCount(bytes.len())),
                )
                .finish()
        }
    }
}

pub use render::{Bar, Output, Quiet, Stream};

closed_vocabulary! {
    /// Which root is running the one command surface.
    ///
    /// The surface is shared and the *name* is not: `webcam-handler-cli --help`,
    /// `webcam-handler-cli --version` and the line a failed `webcam-handler-cli` prints all
    /// say `webcam-handler-cli`, and the identical run of `webcam-handler-client` has to say
    /// `webcam-handler-client`. The naive way to get that is a second `#[command(name = …)]`
    /// on a second root type, which is the one thing T4 forbids — a verb would then exist
    /// twice, and the P4f parity gate scrapes `--help` for its verb population, so a forked
    /// tree would be a gate that compares a surface with itself.
    ///
    /// So the name is a **parameter of the parse** rather than a property of the tree.
    /// [`Cli::try_parse_checked_from`] takes one of these and renames the built
    /// [`clap::Command`]; nothing below this line knows which binary it is in. The
    /// vocabulary is closed and generated (rubric rule 6, [`schema::vocabulary`]), so
    /// `ALL` cannot drift from the type and a third root would have to be named here to
    /// exist at all.
    ///
    /// It carries the error prefix too ([`Program::error_line`]), because that is the same
    /// question wearing a different hat: `webcam-handler-cli: {error}` and
    /// `webcam-handler-client: {error}` are one format with one variable in it, and two roots
    /// each holding their own `format!` is the second copy design §2.10 is about.
    ///
    /// The variants are spelled short — `Cli`, `Client` — because a Rust identifier cannot
    /// carry the hyphens the names have; [`Program::as_str`] below is the one place the
    /// spellings a user sees are written down (note **N90**).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Program {
        /// `webcam-handler-cli` — the direct CLI, driving a backend in-process.
        Cli,
        /// `webcam-handler-client` — the daemon client, driving `webcam-handler-daemon` over
        /// the T5 wire.
        Client,
    }
}

impl Program {
    /// The name this program answers to.
    ///
    /// One string per root, and this is the only place either of them is spelled: clap's
    /// usage line takes it from `argv[0]` at run time, but `--version` and any error
    /// rendered off a freshly built command tree take it from the tree's own name, which
    /// is what [`Program::command`] sets.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Program::Cli => "webcam-handler-cli",
            Program::Client => "webcam-handler-client",
        }
    }

    /// The shared command tree, wearing this program's name.
    ///
    /// The tree is [`Cli`]'s — one surface, built once — and the rename is the whole of
    /// the difference between the two binaries' help output. `Cli` therefore carries no
    /// `#[command(name = …)]` of its own: a default name on the derive would be a second
    /// answer that a caller reaching for [`clap::Parser::parse`] could get by accident,
    /// and it would be `webcam-handler-cli`'s name in `webcam-handler-client`'s mouth.
    #[must_use]
    pub fn command(self) -> clap::Command {
        use clap::CommandFactory as _;

        Cli::command().name(self.as_str())
    }

    /// The one line a root writes to standard error when a verb fails.
    ///
    /// Here rather than in each `main` so the two roots cannot disagree about the shape of
    /// a failure. The typed error renders itself (D13); this adds only the name of the
    /// program that met it, which is what tells an operator with both binaries in a script
    /// which half of the pair refused.
    #[must_use]
    pub fn error_line(self, error: &Error) -> String {
        format!("{self}: {error}")
    }
}

impl std::fmt::Display for Program {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The id clap knows `--backend` by.
///
/// The derive takes an argument's id from its **field name**, and `ArgMatches` is a map on
/// that id — so this string and the field below are one name spelled twice, which is exactly
/// the drift a test has to catch: a lookup on an id no argument has answers `None`, which
/// reads as "the flag was not typed" and would make `webcam-handler-client`'s refusal quietly
/// stop refusing. `the_backend_flag_is_reachable_by_the_id_the_matches_are_keyed_on` is that
/// test.
const BACKEND_ARG: &str = "backend";

/// Drive V4L2 webcams: enumerate, inspect, and capture device profiles.
#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
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

    /// Whether `--backend` was *typed*, as opposed to defaulted — see
    /// [`Cli::backend_was_chosen`].
    ///
    /// `#[arg(skip)]`, so clap neither parses nor renders it; it is filled in by
    /// [`Cli::try_parse_checked_from`] from the matches, which is the only place that still
    /// has them.
    #[arg(skip)]
    backend_chosen: bool,
}

impl Cli {
    /// Parse the process's arguments, applying the rules clap's attributes cannot express.
    ///
    /// Exits the way clap exits, which is the point: a usage mistake leaves code 2 and a
    /// device refusal leaves code 1, and a script deciding whether to retry needs them
    /// apart.
    ///
    /// `program` is the root doing the parsing, and it is an argument rather than a
    /// constant because the tree is shared — see [`Program`].
    #[must_use]
    pub fn parse_checked(program: Program) -> Cli {
        match Cli::try_parse_checked_from(program, std::env::args_os()) {
            Ok(cli) => cli,
            Err(error) => error.exit(),
        }
    }

    /// [`Cli::parse_checked`] over an explicit argument list, for tests.
    ///
    /// Built from [`Program::command`] rather than from clap's derived
    /// [`clap::Parser::try_parse_from`], which is what makes the program name reach the
    /// help, the version line and the usage line of every error raised below: those come
    /// from the *tree*, and `try_parse_from` would build an unnamed one.
    ///
    /// # Errors
    ///
    /// clap's own error, for a parse failure or for one of the cross-argument rules.
    pub fn try_parse_checked_from<I, T>(
        program: Program,
        args: I,
    ) -> std::result::Result<Cli, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        use clap::FromArgMatches as _;

        let mut command = program.command();
        let mut matches = command.try_get_matches_from_mut(args)?;
        // Read **before** the struct is built, because building it removes the values it
        // takes (`ArgMatches::remove_one`) and a source cannot be read off a match that is
        // no longer there. See [`Cli::backend_was_chosen`] for why the source is a fact
        // anybody needs.
        let backend_chosen =
            matches.value_source(BACKEND_ARG) == Some(clap::parser::ValueSource::CommandLine);
        // What clap's own `try_parse_from` does with the error, and the reason this is not
        // a bare `?`: an unformatted `FromArgMatches` error renders without the usage
        // block, so the program name would go missing on exactly the path that names it.
        let mut cli =
            Cli::from_arg_matches_mut(&mut matches).map_err(|error| error.format(&mut command))?;
        cli.backend_chosen = backend_chosen;
        cli.check(program)?;
        Ok(cli)
    }

    /// Whether the command line **named** a backend, rather than taking the default.
    ///
    /// `--backend` carries `default_value = "v4l2"`, so [`Cli::backend`] always holds one and
    /// the value alone cannot answer "did somebody ask for this?". `webcam-handler-client`
    /// needs the question answered, because it refuses the flag: the daemon chose its backend
    /// at its own composition root and a client cannot change it, so `webcam-handler-client
    /// --backend v4l2` has to be refused exactly as `webcam-handler-client --backend fake` is.
    /// Refusing on the *value* would let the spelling that happens to match the default
    /// through, and a client that silently accepted `--backend v4l2` while the daemon replayed
    /// a profile would be lying about which machine's cameras it was showing.
    ///
    /// `false` for a [`Cli`] that did not come through [`Cli::try_parse_checked_from`] —
    /// clap's own `Parser::try_parse_from` builds one without ever seeing this — which is
    /// the honest reading either way: nothing was typed that this crate saw.
    #[must_use]
    pub fn backend_was_chosen(&self) -> bool {
        self.backend_chosen
    }

    /// The cross-argument rules, in clap's error type so they still exit 2.
    ///
    /// There is one, and it is here rather than as an attribute because `--json` is a
    /// **global** argument: clap's `required_if_eq` resolves the arg it names within the
    /// command that declares it, and a subcommand cannot name a flag defined on the root.
    /// Written out rather than worked around, so the rule is visible and so the refusal is
    /// still a usage error rather than a device one.
    ///
    /// It builds the tree through `program` for the same reason the parse does: a refusal
    /// whose usage block named the other binary would send an operator to the wrong
    /// `--help`.
    fn check(&self, program: Program) -> std::result::Result<(), clap::Error> {
        if self.json
            && let Command::Photo { out: None, .. } = &self.command
        {
            return Err(program.command().error(
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
vocabulary_arg!(MetricArg, MetricName, "metric");

/// `CONTROL=VALUE`, as typed on a `set` command line.
///
/// Parsed by clap rather than at run time, so a malformed assignment is a usage error
/// (exit 2) rather than a device error (exit 1) — "you typed it wrong" and "the camera is
/// busy" are different kinds of failure and a script deciding whether to retry needs to
/// tell them apart.
///
/// A newtype over the schema's [`ControlWrite`] for the reason [`BackendKindArg`] is one over
/// `BackendKind`: "which control, and what value" is one shape, and the wire carries it
/// (`wch_set` takes `Vec<ControlWrite>`, D10). A second struct with the same two fields here
/// would be a copy of a rule (design §2.10) and would put a conversion at exactly the seam
/// `webcam-handler-cli` and `webcam-handler-client` are compared across (P4f's parity gate).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment(pub ControlWrite);

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
        Ok(Assignment(ControlWrite {
            control,
            value: schema::control::ControlValue::Int(value),
        }))
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
/// the daemon and recording arrive with their phases (docs/7).
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

        // The cross-reference a Rust reader wants is `Cli::try_parse_checked_from`, and it
        // is a line comment rather than an intra-doc link because **clap prints the doc
        // comment below to a user** — every paragraph of it, under `--help`. Rustdoc renders
        // a bracketed path as a link; clap renders it as brackets around an identifier the
        // reader cannot open, which is what `webcam-handler-cli photo --help` did until
        // P6e's guide put the line in front of somebody (note **N123**).
        /// Where to write it. The extension chooses the encoding; standard output when
        /// omitted, which `--json` therefore does not allow: a photo's bytes and a JSON
        /// document cannot share one stream, so that combination is refused while the
        /// command line is being parsed, before any camera is opened.
        #[arg(long, short, value_name = "PATH")]
        out: Option<Utf8PathBuf>,

        /// The encoding, when there is no path to take it from.
        #[arg(long, value_name = "FORMAT", default_value = "jpeg")]
        format: PhotoFormatArg,

        /// Rotate or mirror. On a pass-through JPEG this is an EXIF tag, not a re-encode
        /// (E6).
        #[arg(long, value_name = "TRANSFORM", default_value = "none")]
        transform: TransformArg,

        /// What to ask the device's format negotiation for.
        #[command(flatten)]
        stream: StreamArgs,

        /// How long to let the sensor settle first.
        #[command(flatten)]
        settle: SettleArgs,

        /// Wait for the camera rather than being refused while it is busy (D12).
        ///
        /// Inert under `webcam-handler-cli`, which opens its own camera per invocation and
        /// runs one verb: the queue it would be waiting for is its own and always empty. It
        /// is meaningful under `webcam-handler-client`, where the daemon serves every client
        /// from one thread per camera.
        // `Command::photo_request` is where the flag became reachable, and that citation is a
        // line comment for the reason the one on `photo --out` is: clap prints the paragraph
        // above to a user (note **N123**).
        #[arg(long)]
        wait: bool,
    },

    /// Record a video.
    ///
    /// **One verb, three wire calls.** `webcam-handler-cli` records in this process and
    /// returns; `webcam-handler-client` starts the take, polls `record_status` and collects it
    /// with `record_stop` — a state machine, exactly as `calibrate sweep` is one, and for the
    /// identical reason. AGENTS' primary consumer has no hands, and *"a verb needing a call
    /// sequence … is a defect for the consumer that matters most"*, so this verb performs
    /// the sequence rather than asking a caller to write it.
    // The seam it lives behind is `Executor`, and the citation is a line comment because clap
    // prints the paragraph above to a user (note **N123**).
    Record {
        /// Which camera.
        #[command(flatten)]
        camera: CameraArg,

        /// Where to write it. The extension chooses the container: `.avi` or `.y4m`, and a
        /// path with no extension lets the camera's negotiated format decide.
        ///
        /// **Required**, unlike `photo --out`, and the difference is note **N110**'s: a
        /// recording's bytes go to a path and never back in the answer, because a take at its
        /// own cap is thirty-two times the largest JSON-RPC body this daemon writes. There is
        /// therefore no "standard output" spelling for a recording to compete with a `--json`
        /// document, which is why `record` needs no counterpart to the rule that refuses
        /// `photo --json` without a path.
        // That rule is `Cli::try_parse_checked_from`'s, cited in a line comment because clap
        // prints the paragraphs above to a user (note **N123**).
        #[arg(long, short, value_name = "PATH", required = true)]
        out: Utf8PathBuf,

        /// How long to record, as a duration such as `10s`, `1500ms` or `1m30s`.
        ///
        /// `schema::limits::DEFAULT_RECORDING_MS` when omitted, and refused rather than
        /// clamped past `schema::limits::MAX_RECORDING_MS` — an agent that asked for five
        /// minutes and silently received two cannot tell that from a camera that stopped.
        #[arg(long, value_name = "DURATION", value_parser = duration)]
        duration: Option<u64>,

        /// What to ask the device's format negotiation for.
        #[command(flatten)]
        stream: StreamArgs,

        /// Wait for the camera rather than being refused while it is busy (D12).
        ///
        /// Inert under `webcam-handler-cli`, which opens its own camera per invocation, and
        /// meaningful under `webcam-handler-client` — the same split `photo --wait` has. It
        /// bounds only the **start**: once a take is running it owns the camera, and a
        /// recording that had to re-queue per frame would be bounded by other clients rather
        /// than by its own duration.
        #[arg(long)]
        wait: bool,
    },

    /// Run a calibration session: sweep controls, score the samples, apply the result (D8).
    #[command(subcommand)]
    Calibrate(CalibrateCommand),

    /// Capture device profiles.
    #[command(subcommand)]
    Profile(ProfileCommand),
}

/// The format-negotiation flags, shared by every verb that opens a stream.
///
/// One declaration, flattened into `photo` and `calibrate sweep`, because the two must ask
/// the device for the same thing in the same words: a sweep whose `--size` meant something
/// slightly different from a photo's would produce samples nobody could reproduce with the
/// photo verb afterwards.
#[derive(Debug, Clone, Args)]
pub struct StreamArgs {
    /// The frame size to ask the device for, as `WxH`.
    #[arg(long, value_name = "WxH")]
    pub size: Option<SizeArg>,

    /// The pixel format to ask the device for, as a fourcc such as `MJPG`.
    #[arg(long, value_name = "FOURCC", value_parser = fourcc)]
    pub pixel_format: Option<PixelFormat>,
}

/// `--pixel-format`, through the schema's own decoder (note **N109**).
///
/// The flag is parsed **here**, at the command line, rather than where the request is built,
/// and that is a repair rather than a tidy-up. It used to be an `Option<String>` that
/// [`StreamArgs::request`] ran through `and_then(PixelFormat::parse)`, so
/// `--pixel-format MJP` — a typo, or a five-character name — produced `None`, which is
/// the same value as *not passing the flag at all*: the camera then chose its own format
/// under D5's ranking and the caller got a photo in a format it had not asked for, with
/// nothing anywhere saying so. For AGENTS' primary consumer that is the worst shape a
/// failure has, because the answer looks like a success. Now clap refuses it by name.
///
/// [`PixelFormat::parse`] is the decoder the wire uses, so a format the daemon would accept
/// and one this flag accepts are the same set — which is the property that makes
/// `webcam-handler-cli` and `webcam-handler-client` interchangeable on this flag rather than
/// merely similar.
/// `--duration`, as milliseconds, through `humantime` (note **N113**).
///
/// **This project's first human-scale duration flag, and the rule it establishes is in the
/// note.** Design §2.7 named "humantime durations" as a T4 argument type at design time and
/// nothing had claimed it until `record`; the settle flags beside it (`--settle-for MS`,
/// `--settle-deadline MS`) stay integer milliseconds, and the line between them is the scale a
/// caller reasons at: a recording is seconds to minutes, where `10s` and `1m30s` are what a
/// person and an agent both write, and a settle is a sub-second tolerance whose sibling is a
/// *frame count* (`--skip-frames 3`) — spelling one of that pair as a duration string and the
/// other as an integer would put two vocabularies on one pair of alternatives. The two never
/// meet on one command line, because a recording carries no settle policy at all (note
/// **N111**).
///
/// Parsed **here**, by clap, for `fourcc`'s reason and note **N109**'s: a value this cannot
/// read is a usage error with an exit code of 2 and a message naming the flag, rather than
/// `None` three layers down that reads exactly like not passing the flag at all.
///
/// The answer is milliseconds because that is what `schema::video::RecordRequest::duration_ms`
/// carries and what `schema::limits` prices the cap in — converting once, at the edge, is what
/// keeps `--duration 10s` and `{"duration_ms": 10000}` the same request. A duration too large
/// for a `u64` of milliseconds is refused rather than saturated, because a saturating parse
/// would turn a typo into the longest recording this build will refuse.
fn duration(text: &str) -> std::result::Result<u64, String> {
    let parsed = humantime::parse_duration(text).map_err(|error| {
        format!(
            "{text:?} is not a duration ({error}); write one as 10s, 1500ms or 1m30s, up to \
             the {}s this build records",
            schema::limits::MAX_RECORDING_MS / 1_000
        )
    })?;
    u64::try_from(parsed.as_millis())
        .map_err(|_| format!("{text:?} is longer than this build can express in milliseconds"))
}

fn fourcc(text: &str) -> std::result::Result<PixelFormat, String> {
    PixelFormat::parse(text).ok_or_else(|| {
        format!(
            "{text:?} is not a fourcc; a fourcc is four characters, such as MJPG, with \\xNN \
             for a byte that is not an ASCII graphic"
        )
    })
}

impl StreamArgs {
    /// The request these flags describe.
    #[must_use]
    pub fn request(&self) -> StreamRequest {
        StreamRequest {
            pixel_format: self.pixel_format,
            width: self.size.map(|s| s.width),
            height: self.size.map(|s| s.height),
            interval: None,
            buffer_count: schema::limits::DEFAULT_BUFFER_COUNT,
            // Not a command-line flag and deliberately not one: D5's 2026-08-13 amendment
            // derives it from where the photo is going, and `PhotoRequest::stream_for_sink` is
            // the one place that happens — for `webcam-handler-client` as much as for
            // `webcam-handler-cli`, since the derivation runs where the photo is taken rather
            // than where it is typed.
            sink_fidelity: schema::camera::SinkFidelity::default(),
        }
    }
}

/// The settle flags, shared by every verb that takes a frame \[PF:11\].
#[derive(Debug, Clone, Copy, Args)]
pub struct SettleArgs {
    /// Discard this many frames before taking one \[PF:11\].
    #[arg(long, value_name = "N", conflicts_with = "settle_for")]
    pub skip_frames: Option<u32>,

    /// Discard frames for this long before taking one, in milliseconds.
    #[arg(long, value_name = "MS")]
    pub settle_for: Option<u64>,

    /// How long the whole settle may take, in milliseconds.
    #[arg(long, value_name = "MS")]
    pub settle_deadline: Option<u64>,
}

impl SettleArgs {
    /// The policy these flags describe; the `limits` table decides what they leave unsaid.
    #[must_use]
    pub fn policy(&self) -> SettlePolicy {
        let spec = match (self.skip_frames, self.settle_for) {
            (Some(frames), _) => SettleSpec::SkipFrames { frames },
            (None, Some(millis)) => SettleSpec::SettleFor { millis },
            (None, None) => SettleSpec::default(),
        };
        SettlePolicy {
            spec,
            deadline_ms: self
                .settle_deadline
                .unwrap_or(schema::limits::DEFAULT_SETTLE_DEADLINE_MS),
        }
    }
}

/// Which session a calibrate verb is about.
///
/// A (camera, task) pair names the session an operator is working on: D8 says a session
/// belongs to that pair, and at most one of a task's sessions is open (N14). `--session`
/// names one by its UUID instead, which is what `calibrate list` prints — the only way to
/// reach a session recorded against a *different* camera, and therefore the only way the
/// fingerprint check `apply` performs can ever have something to refuse.
#[derive(Debug, Clone, Args)]
#[group(required = true, multiple = false)]
pub struct SessionArg {
    /// The task, in the words `calibrate start` recorded.
    #[arg(long, value_name = "TEXT")]
    pub task: Option<String>,

    /// The session's UUID, as `calibrate list` prints it.
    #[arg(long, value_name = "UUID")]
    pub session: Option<uuid::Uuid>,
}

/// `webcam-handler-cli calibrate …` — the calibration session verbs (design D8).
#[derive(Debug, Subcommand)]
pub enum CalibrateCommand {
    /// Open a session for a camera and a task.
    Start {
        /// Which camera.
        #[command(flatten)]
        camera: CameraArg,

        /// The task, in your own words: "read text from the DUT display".
        #[arg(long, value_name = "TEXT")]
        task: String,

        /// What a good photo looks like for this task.
        #[arg(long, value_name = "TEXT", default_value = "")]
        goal: String,

        /// One quality criterion, in priority order. Repeatable.
        ///
        /// Recorded because the *selector* needs them (D8) — whether that selector is a
        /// human, an agent, or a metric, it is judging against something.
        #[arg(long = "criterion", value_name = "TEXT")]
        criteria: Vec<String>,
    },

    /// Draft the control queue: what will be calibrated, in what order.
    ///
    /// With no controls named, every control the camera has is classified — the sweepable
    /// ones queued, the rest recorded `blocked` with the device's reason. That is the
    /// skill's "cover all the setting names" step, and a control the device will not let
    /// this tool calibrate is a fact worth writing down rather than an omission.
    Plan {
        /// Which camera.
        #[command(flatten)]
        camera: CameraArg,

        /// Which session.
        #[command(flatten)]
        which: SessionArg,

        /// The controls, by slug. Every control on the camera when none are named.
        #[arg(value_name = "CONTROL")]
        controls: Vec<String>,

        /// Treat the controls named as the queue's new order (a permutation of it).
        #[arg(long, requires = "controls")]
        order: bool,
    },

    /// Sweep one control: a photo per value, scored.
    Sweep {
        /// Which camera.
        #[command(flatten)]
        camera: CameraArg,

        /// Which session.
        #[command(flatten)]
        which: SessionArg,

        /// The control to sweep, by slug.
        #[arg(value_name = "CONTROL")]
        control: String,

        /// How to derive the values.
        #[command(flatten)]
        plan: PlanArgs,

        /// Allow a sweep that moves motors (design §5: never implicit).
        #[arg(long)]
        allow_motion: bool,

        /// The encoding the sample photos are written in.
        #[arg(long, value_name = "FORMAT", default_value = "jpeg")]
        photo_format: PhotoFormatArg,

        /// What to ask the device's format negotiation for.
        #[command(flatten)]
        stream: StreamArgs,

        /// How long to let the sensor settle before each shot.
        #[command(flatten)]
        settle: SettleArgs,
    },

    /// Where a session stands, and what happened to get it there.
    Status {
        /// Which camera.
        #[command(flatten)]
        camera: CameraArg,

        /// Which session.
        #[command(flatten)]
        which: SessionArg,
    },

    /// Record the value chosen for a control, and who chose it (D8).
    Select {
        /// Which camera.
        #[command(flatten)]
        camera: CameraArg,

        /// Which session.
        #[command(flatten)]
        which: SessionArg,

        /// The control, by slug.
        #[arg(value_name = "CONTROL")]
        control: String,

        /// Who chose, and how.
        #[command(flatten)]
        by: SelectorArgs,
    },

    /// Write a session's calibrated values back to the camera (D4 ordering).
    Apply {
        /// Which camera.
        #[command(flatten)]
        camera: CameraArg,

        /// Which session.
        #[command(flatten)]
        which: SessionArg,

        /// Apply what is decided so far, leaving uncalibrated controls alone.
        ///
        /// The only way past an unfinished session. Without it a session with nothing
        /// chosen, or with queued controls still pending, is refused — a verb that wrote
        /// nothing and reported success is how a calibration silently does not apply.
        #[arg(long)]
        partial: bool,
    },

    /// Put the camera back where the session found it, and spend the record (D4, §6).
    ///
    /// A sweep *borrows* the camera: it drives a control to take a photograph and has no
    /// interest in where it left it, and `lifecycle::sweep_write` persists the camera's
    /// state before the first write of the session reaches it precisely so there is a way
    /// back — one that survives a crash, because it is on disk rather than in a process.
    /// This is the verb that spends it. Until P3's review there was none: the record was
    /// written by the tool and read only by tests, so `calibrate sweep` left every camera
    /// holding its last swept value and AGENTS rule 8 had no shipped implementation.
    ///
    /// Session-scoped rather than sweep-scoped, and note N23 argues it: the snapshot is
    /// taken once per session by design, `apply` deliberately does not consume it (N20),
    /// and putting a PTZ head back between every pair of sweeps would be travel §5's
    /// "motors wear" spends for nothing.
    ///
    /// Running it twice is not an error — the second time there is nothing left to put
    /// back, and it says so.
    Restore {
        /// Which camera.
        #[command(flatten)]
        camera: CameraArg,

        /// Which session.
        #[command(flatten)]
        which: SessionArg,
    },

    /// Every session on this machine, newest first.
    List {
        /// One camera's sessions, by id or unambiguous prefix. All cameras when omitted.
        #[arg(value_name = "CAMERA")]
        camera: Option<String>,
    },
}

/// How a sweep derives the values it visits (design D8's sweep plans).
///
/// Exactly one, and **required**: a sweep is minutes of camera time and, on a PTZ head,
/// motor travel. A default would make the expensive choice the silent one.
#[derive(Debug, Clone, Args)]
#[group(required = true, multiple = false)]
pub struct PlanArgs {
    /// Every step from the control's minimum to its maximum.
    #[arg(long)]
    pub all: bool,

    /// Every `N`-th value, aligned to the control's own step.
    #[arg(long, value_name = "N")]
    pub step: Option<i64>,

    /// This many logarithmically spaced values.
    #[arg(long, value_name = "N")]
    pub points: Option<u32>,

    /// Exactly these values, comma-separated.
    ///
    /// `allow_hyphen_values` because **a control value is signed and PTZ ranges are centred
    /// on zero**: `pan_absolute` is `-468000..=468000` and `hue` is `-180..=180`, so the
    /// first value of an explicit PTZ sweep normally begins with a minus. Without it clap
    /// reads `--values -108000,0` as the flag `-1` and refuses with "unexpected argument
    /// '-1'", which is a parser talking about itself. Found on real hardware during the P3e
    /// R3 run, where it is the ordinary case rather than an edge one.
    #[arg(
        long,
        value_name = "V,V,…",
        value_delimiter = ',',
        allow_hyphen_values = true
    )]
    pub values: Option<Vec<i64>>,
}

impl PlanArgs {
    /// The spec these flags describe.
    ///
    /// # Errors
    ///
    /// [`Error::IllegalTransition`] if none of them was given, which clap's required group
    /// has already refused — the arm exists so the conversion has no `unwrap` in it.
    pub fn spec(&self) -> Result<SweepSpec> {
        if self.all {
            return Ok(SweepSpec::All);
        }
        if let Some(step) = self.step {
            return Ok(SweepSpec::Uniform { step });
        }
        if let Some(points) = self.points {
            return Ok(SweepSpec::Log { points });
        }
        if let Some(values) = &self.values {
            return Ok(SweepSpec::Explicit {
                values: values.clone(),
            });
        }
        Err(Error::IllegalTransition {
            from: "no_sweep_plan".to_owned(),
            op: "sweep: pass one of --all, --step, --points or --values".to_owned(),
        })
    }
}

/// Who chose a value, and how (design D8's selector identity).
///
/// `--metric` and `--value` are alternatives because they are different claims. A metric
/// *ranks*: naming one records `metric:<name>` and the score it earned. A value is
/// *chosen*, and `--by` is required with it because the record has to say whether an agent
/// looked at the photos or a person did — nothing here may pretend a Laplacian knows what
/// "text legible on the DUT" means, and a default `--by` is exactly that pretence.
#[derive(Debug, Clone, Args)]
#[group(required = true, multiple = true)]
pub struct SelectorArgs {
    /// Rank the samples by this metric and take the best.
    #[arg(long, value_name = "METRIC", conflicts_with_all = ["value", "by"])]
    pub metric: Option<MetricArg>,

    /// The value chosen, as a sample's *applied* value.
    ///
    /// Hyphen-tolerant for the same reason `--values` is: the value being selected is a
    /// value the camera held, and on a pan or tilt control half of them are negative.
    #[arg(long, value_name = "N", requires = "by", allow_hyphen_values = true)]
    pub value: Option<i64>,

    /// Who chose it: `agent` or `human`.
    #[arg(long, value_name = "WHO", requires = "value")]
    pub by: Option<ChosenByArg>,
}

impl SelectorArgs {
    /// The selection these flags describe.
    ///
    /// # Errors
    ///
    /// [`Error::IllegalTransition`] for a combination clap's group has already refused; the
    /// arm exists so this conversion has no `unwrap` in it.
    pub fn selection(&self) -> Result<Selection> {
        match (self.metric, self.value, self.by) {
            (Some(metric), None, None) => Ok(Selection::ByMetric { metric: metric.0 }),
            (None, Some(value), Some(by)) => Ok(Selection::ByValue {
                value,
                chosen_by: by.0,
            }),
            _ => Err(Error::IllegalTransition {
                from: "no_selector".to_owned(),
                op: "select: pass --metric <NAME>, or --value <N> --by <agent|human>".to_owned(),
            }),
        }
    }
}

/// `--by` as clap sees it.
///
/// A newtype over the schema's [`ChosenBy`] for the reason [`BackendKindArg`] is one: the
/// vocabulary belongs to the schema, because the same two spellings are what the wire
/// carries (D10) and what a session file records, and a second enum here would be a copy
/// of a rule (design §2.10). `FromStr` resolves `--by <WHO>` by walking `ChosenBy::ALL`
/// and builds the refusal's `known:` list from the same walk, so a variant the compiler
/// accepted and the parser could not reach is impossible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChosenByArg(pub ChosenBy);

impl std::str::FromStr for ChosenByArg {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        // Matched against the schema's own spelling of each selector, so `--by agent` and
        // the `agent` a session file records are one string rather than two that can drift.
        ChosenBy::ALL
            .iter()
            .copied()
            .find(|chooser| chooser.selector().label() == s)
            .map(ChosenByArg)
            .ok_or_else(|| {
                let known: Vec<String> = ChosenBy::ALL
                    .iter()
                    .map(|chooser| chooser.selector().label())
                    .collect();
                format!("unknown chooser {s:?}; known: {}", known.join(", "))
            })
    }
}

impl Command {
    /// The photo request a `photo` invocation describes, and where its bytes go.
    ///
    /// `cwd` is the caller's directory, passed in rather than read: D10 says a relative `-o`
    /// resolves against the *caller's* cwd, and at P4 the caller is on the other end of a
    /// socket. Resolving it here — in the shared command surface — is what makes
    /// `webcam-handler-cli photo -o out.jpg` and `webcam-handler-client photo -o out.jpg` mean
    /// the same file.
    ///
    /// `--wait` is D12's flag and it **landed here at P4f**, with the surface that can mean
    /// it. The absence this doc used to argue was note N42's and note N56 restated it: "the
    /// consumer where it is meaningful is `webcam-handler-client`, whose transport is P4f's…
    /// It stays a wire field until the surface that can mean it exists." That surface exists
    /// now, so the flag is a flag: `webcam-handler-client photo --wait` asks the daemon to
    /// *queue* behind whatever is holding the camera's one thread
    /// (`limits::CAMERA_ENQUEUE_WAIT_MS` bounds the wait), where without it a busy camera is
    /// `Error::Busy`. It is still inert under `webcam-handler-cli` for exactly the reason it
    /// was inert before — one process, one camera, one verb, an empty queue — and its `--help`
    /// line says so rather than leaving a user to discover it, which is the objection that
    /// kept a flag with no reachable consumer out of P4b.
    ///
    /// `PhotoRequest::wait` is `#[serde(default)]`, so nothing about the committed schema
    /// artifacts moves with this — asserted by `scripts/gates/schema-artifacts-current.sh`
    /// rather than claimed.
    ///
    /// # Errors
    ///
    /// [`Error::IllegalTransition`] from [`Sink::writable_format`] when the output path's
    /// extension names an encoding this build does not write, naming both the extension and
    /// the three it does. Raised *here* — while a command line is being parsed, before
    /// anything opens a camera — but decided on the type, because `webcam-handler-daemon`
    /// links no `cli-core` and the same refusal has to hold for a sink a socket built (note
    /// N46, debt D-1). Deliberately not `FormatUnsupported`: that variant is the camera saying
    /// what it cannot offer, and `.webp` is not the camera's fault (E3).
    pub fn photo_request(&self, cwd: &camino::Utf8Path) -> Result<Option<PhotoRequest>> {
        let Command::Photo {
            out,
            format,
            transform,
            stream,
            settle,
            wait,
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
                let sink = Sink::ServerPath { path: absolute };
                // Asked, not repeated. The refusal for an extension this build cannot write is
                // `Sink`'s, beside the variants, so `webcam-handler-cli photo -o a.webp` and a
                // socket sending `{"kind":"server_path","path":"/tmp/a.webp"}` are refused by
                // one rule with one message. What is local to this surface is only *when*: at
                // parse time, before a camera is opened, which is why the answer it produces
                // is discarded and the error is not.
                sink.writable_format()?;
                sink
            }
        };

        Ok(Some(PhotoRequest {
            stream: stream.request(),
            settle: settle.policy(),
            transform: transform.0,
            sink,
            // D12's flag, carried verbatim rather than decided here. The surface says what was
            // asked for; whether asking means anything is the *executor's* answer, and the two
            // executors differ — see this method's doc. Nothing in this crate branches on it,
            // which is what makes `webcam-handler-cli photo --wait` and `webcam-handler-cli
            // photo` produce byte-identical `--json` while `webcam-handler-client`'s two
            // differ.
            wait: *wait,
        }))
    }

    /// The recording request a `record` invocation describes, and where its file goes.
    ///
    /// [`Command::photo_request`]'s counterpart, and `cwd` is here for the identical reason:
    /// D10 says a relative `-o` resolves against the **caller's** cwd, and for
    /// `webcam-handler-client` the caller is on the other end of a socket. Resolving it here —
    /// in the shared command surface, before the request is sent — is what makes
    /// `webcam-handler-cli record -o take.avi` and `webcam-handler-client record -o take.avi`
    /// name the same file. A daemon handed the relative path would resolve it against its own
    /// working directory, which under systemd is `/`.
    ///
    /// The container check is made here too, and only its *timing* is local to this surface:
    /// `-o take.mkv` is refused while a command line is being parsed, before anything opens a
    /// camera, but the rule is [`schema::video::RecordRequest::container`]'s, beside the
    /// variants it constrains, because `webcam-handler-daemon` links no `cli-core` and a
    /// socket can build the same request (debt D-1, note **N46**). The answer it produces is
    /// discarded and the error is not — exactly what [`Command::photo_request`] does with
    /// `Sink::writable_format`.
    ///
    /// # Errors
    ///
    /// [`Error::IllegalTransition`] naming the extension that was typed and the ones this
    /// build writes. Deliberately not [`Error::FormatUnsupported`]: that variant is the
    /// *camera* saying what it cannot offer (E3), and `.mkv` is not the camera's fault.
    pub fn record_request(&self, cwd: &camino::Utf8Path) -> Result<Option<RecordRequest>> {
        let Command::Record {
            out,
            duration,
            stream,
            wait,
            ..
        } = self
        else {
            return Ok(None);
        };

        let absolute = if out.is_absolute() {
            out.clone()
        } else {
            cwd.join(out)
        };
        let request = RecordRequest {
            stream: stream.request(),
            duration_ms: *duration,
            sink: Sink::ServerPath { path: absolute },
            // D12's flag, carried verbatim rather than decided here: the surface says what was
            // asked for, and whether asking means anything is the *executor's* answer (see
            // `Command::photo_request`, which makes the same split).
            wait: *wait,
        };
        // Asked, not repeated — and asked *before* the duration, because a caller who typed
        // both a bad extension and a bad duration is better served by the one that is about
        // the file they will be looking for.
        request.container()?;
        request.budget_ms()?;
        Ok(Some(request))
    }
}

/// `webcam-handler-cli profile …`
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
/// Deliberately narrow: every method answers with a schema value, and none of them renders,
/// prints, or decides an exit code. `webcam-handler-cli` implements it over an in-process
/// engine; `webcam-handler-client` will implement it over the generated RPC client at P4, and
/// the parity gate then proves the two produce identical `--json`.
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
        writes: &[ControlWrite],
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

    /// Record one video, start to finish (D7, D10).
    ///
    /// **One method, however many wire calls it costs.** `webcam-handler-cli` holds the camera
    /// and records in this process; `webcam-handler-client` starts a take, polls
    /// `wch_record_status` and collects it with `wch_record_stop`, because D10 puts three
    /// methods on the wire and says progress comes from polling. That asymmetry is the whole
    /// reason this is a seam rather than a wire type: AGENTS' primary consumer has no hands,
    /// and a verb whose *user* has to write the sequence is the defect that section names.
    /// [`Executor::calibrate_sweep`] is the precedent, exactly — one T4 method over a
    /// several-call state machine `webcam-handler-client` owns.
    ///
    /// It **blocks for the length of the take**, which is bounded by
    /// `schema::limits::MAX_RECORDING_MS` and refused past it, so a caller that wants control
    /// back sooner asks for a shorter recording rather than for a handle.
    ///
    /// # Errors
    ///
    /// As [`Executor::info`], plus [`Error::IllegalTransition`] for a request this build will
    /// not honour, [`Error::Busy`] when the camera is already recording or is held by
    /// something else, [`Error::FormatUnsupported`] when the container the path names cannot
    /// carry what the camera negotiated, [`Error::StorageIo`] from the file, and the device's
    /// own answer for anything it refused.
    fn record(&mut self, camera: &CameraId, request: &RecordRequest) -> Result<RecordReport>;

    /// One camera's full device profile (T3).
    ///
    /// # Errors
    ///
    /// As [`Executor::info`].
    fn capture_profile(&mut self, camera: &CameraId, capturer: &str) -> Result<DeviceProfile>;

    /// Open a calibration session for a camera and a task (D8).
    ///
    /// # Errors
    ///
    /// As [`Executor::info`], plus [`Error::SessionConflict`] when the slot already holds
    /// an open session (N14), and whatever the store refuses with —
    /// [`Error::StoreLocked`] when a daemon owns the state directory (D9).
    fn calibrate_start(
        &mut self,
        camera: &CameraId,
        task: &str,
        goal: &str,
        criteria: &[String],
    ) -> Result<Session>;

    /// Queue controls for calibration, or reorder the queue.
    ///
    /// `controls` empty means every control the camera has. `order` treats the named
    /// controls as the queue's new order rather than as additions.
    ///
    /// # Errors
    ///
    /// As [`Executor::calibrate_start`], plus [`Error::ControlUnknown`] for a slug this
    /// camera does not have and [`Error::IllegalTransition`] when `order` is not a
    /// permutation of the queue.
    fn calibrate_plan(
        &mut self,
        camera: &CameraId,
        which: &SessionRef,
        controls: &[ControlSlug],
        order: bool,
    ) -> Result<Session>;

    /// Sweep one control, reporting progress as it goes (D8).
    ///
    /// `watch` is where the progress events go while the sweep runs. It is this crate's seam
    /// rather than the engine's, because `webcam-handler-client` links no engine (T6): the
    /// binaries bridge whichever stream they have onto it, and the rendering happens once,
    /// here.
    ///
    /// # Errors
    ///
    /// As [`Executor::calibrate_start`], plus the planner's refusals (a control with no
    /// ordered range, a motion control without `--allow-motion`) and whatever the camera
    /// said at the sample that stopped it.
    fn calibrate_sweep(
        &mut self,
        camera: &CameraId,
        which: &SessionRef,
        request: &SweepRequest,
        watch: &dyn SweepWatcher,
    ) -> Result<Session>;

    /// A session's document and its history.
    ///
    /// # Errors
    ///
    /// As [`Executor::info`], plus [`Error::SchemaVersionForeign`] for a session another
    /// build wrote (D9).
    fn calibrate_status(&mut self, camera: &CameraId, which: &SessionRef) -> Result<SessionStatus>;

    /// Record a control's chosen value and who chose it (D8).
    ///
    /// # Errors
    ///
    /// As [`Executor::calibrate_start`], plus the D8 machine's refusals: a control that
    /// never swept, a value no sample holds, a metric that cannot rank.
    fn calibrate_select(
        &mut self,
        camera: &CameraId,
        which: &SessionRef,
        control: &ControlSlug,
        selection: &Selection,
    ) -> Result<Session>;

    /// Write a session's calibrated values back to a camera (D4 ordering).
    ///
    /// # Errors
    ///
    /// As [`Executor::calibrate_start`], plus [`Error::FingerprintMismatch`] naming the
    /// fields that differ when the camera is not the one the session was recorded against,
    /// and [`Error::IllegalTransition`] when the session has uncalibrated work and
    /// `partial` is false.
    fn calibrate_apply(
        &mut self,
        camera: &CameraId,
        which: &SessionRef,
        partial: bool,
    ) -> Result<WriteReport>;

    /// Put the camera back where the session found it, and spend the record (D4, §6).
    ///
    /// The eighth verb, and the one that makes "leave the camera as you found it" a thing
    /// an operator can type. Answers with the same [`RestoreReport`] `restore` does — a
    /// control that could not be put back is in the *report*, not an error — and with an
    /// **empty** report when the session carries no unconsumed snapshot, which is what
    /// running it twice looks like.
    ///
    /// # Errors
    ///
    /// As [`Executor::calibrate_start`], plus [`Error::FingerprintMismatch`] naming the
    /// fields that differ when the camera is not the one the session was recorded against.
    fn calibrate_restore(&mut self, camera: &CameraId, which: &SessionRef)
    -> Result<RestoreReport>;

    /// Every session on this machine, or one camera's, newest first.
    ///
    /// # Errors
    ///
    /// As [`Executor::info`] when a camera is named; otherwise whatever the store says.
    /// A session whose document this build cannot read still *lists* — listing parses
    /// nothing (D9).
    fn calibrate_list(&mut self, camera: Option<&CameraId>) -> Result<SessionList>;
}

impl SessionArg {
    /// The session these flags name.
    ///
    /// # Errors
    ///
    /// [`Error::IllegalTransition`] when neither was given, which clap's required group has
    /// already refused; the arm exists so this conversion has no `unwrap` in it.
    pub fn which(&self) -> Result<SessionRef> {
        match (&self.task, self.session) {
            (Some(task), None) => Ok(SessionRef::Task { task: task.clone() }),
            (None, Some(id)) => Ok(SessionRef::Id { id }),
            _ => Err(Error::IllegalTransition {
                from: "no_session_named".to_owned(),
                op: "name a session with --task <TEXT> or --session <UUID>".to_owned(),
            }),
        }
    }
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
            let writes: Vec<ControlWrite> = assignments
                .iter()
                .map(|assignment| assignment.0.clone())
                .collect();
            let report = executor.set(&camera.id()?, &writes, !*no_guard)?;
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
        Command::Record { camera, .. } => {
            let cwd = current_directory()?;
            let request = cli
                .command
                .record_request(&cwd)?
                .ok_or_else(unreachable_record)?;
            let report = executor.record(&camera.id()?, &request)?;
            render::record(&report, cli.json, out)
        }
        Command::Calibrate(command) => calibrate(command, cli.json, executor, out),
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

/// `webcam-handler-cli calibrate …`, dispatched.
///
/// Its own function rather than seven more arms in [`run`]: the calibration verbs share a
/// shape — resolve a camera, name a session, hand the answer to a renderer — and the shape
/// is easier to check when it is not interleaved with the ones that do not.
fn calibrate<E: Executor>(
    command: &CalibrateCommand,
    as_json: bool,
    executor: &mut E,
    out: &mut Output,
) -> Result<()> {
    match command {
        CalibrateCommand::Start {
            camera,
            task,
            goal,
            criteria,
        } => {
            let session = executor.calibrate_start(&camera.id()?, task, goal, criteria)?;
            render::session(&session, as_json, out)
        }
        CalibrateCommand::Plan {
            camera,
            which,
            controls,
            order,
        } => {
            let slugs = control_slugs(controls)?;
            let session =
                executor.calibrate_plan(&camera.id()?, &which.which()?, &slugs, *order)?;
            render::session(&session, as_json, out)
        }
        CalibrateCommand::Sweep {
            camera,
            which,
            control,
            plan,
            allow_motion,
            photo_format,
            stream,
            settle,
        } => {
            let request = SweepRequest {
                control: control_slug(control)?,
                plan: plan.spec()?,
                allow_motion: *allow_motion,
                stream: stream.request(),
                settle: settle.policy(),
                photo_format: photo_format.0,
            };
            // The progress bar is suspended for the duration of the rendering below, and
            // it goes to standard error: a sweep's `--json` document shares standard output
            // with nothing, and a bar drawn into it would make the answer unparsable.
            let watcher = render::watcher(as_json);
            let session =
                executor.calibrate_sweep(&camera.id()?, &which.which()?, &request, &*watcher)?;
            watcher.finish();
            render::session(&session, as_json, out)?;
            // The camera is still borrowed, and saying so is the difference between a
            // restore an operator forgot and one they declined. Design D4 restores by
            // default; this tool's restore is a verb rather than a default because the
            // snapshot is session-scoped (N23), so the default is replaced by a reminder
            // that names the exact command. Standard error, so the `--json` answer on
            // standard output is still one document.
            if session.pre_snapshot.is_some() {
                out.line(
                    Stream::Stderr,
                    &format!(
                        "note: this sweep borrowed the camera and it still holds what the \
                         sweep left; `calibrate restore {} --session {}` puts it back",
                        camera.camera, session.id
                    ),
                )?;
            }
            Ok(())
        }
        CalibrateCommand::Status { camera, which } => {
            let status = executor.calibrate_status(&camera.id()?, &which.which()?)?;
            render::status(&status, as_json, out)
        }
        CalibrateCommand::Select {
            camera,
            which,
            control,
            by,
        } => {
            let session = executor.calibrate_select(
                &camera.id()?,
                &which.which()?,
                &control_slug(control)?,
                &by.selection()?,
            )?;
            render::session(&session, as_json, out)
        }
        CalibrateCommand::Apply {
            camera,
            which,
            partial,
        } => {
            let report = executor.calibrate_apply(&camera.id()?, &which.which()?, *partial)?;
            render::writes(&report, as_json, out)
        }
        CalibrateCommand::Restore { camera, which } => {
            let report = executor.calibrate_restore(&camera.id()?, &which.which()?)?;
            // An empty report is not "restored nothing", it is "there was nothing to put
            // back" — the ordinary answer to running this twice, and the two would be
            // indistinguishable from a table with no rows in it.
            if report.outcomes.is_empty() {
                out.line(
                    Stream::Stderr,
                    "note: this session carries no unconsumed pre-sweep snapshot; the camera \
                     was not written to",
                )?;
            }
            render::restore(&report, as_json, out)
        }
        CalibrateCommand::List { camera } => {
            let id = camera
                .as_deref()
                .map(|name| {
                    CameraId::parse(name).ok_or_else(|| Error::CameraUnknown {
                        requested: name.to_owned(),
                    })
                })
                .transpose()?;
            let sessions = executor.calibrate_list(id.as_ref())?;
            render::sessions(&sessions, as_json, out)
        }
    }
}

/// One control slug, refused by name rather than resolved to the wrong control.
fn control_slug(name: &str) -> Result<ControlSlug> {
    ControlSlug::parse(name).ok_or_else(|| Error::ControlUnknown {
        requested: name.to_owned(),
        did_you_mean: Vec::new(),
    })
}

/// A list of control slugs, all or nothing.
fn control_slugs(names: &[String]) -> Result<Vec<ControlSlug>> {
    names.iter().map(|name| control_slug(name)).collect()
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

/// [`unreachable_photo`]'s counterpart for the `record` arm, and it exists for the same
/// reason: the match above has already established which command this is, and the arm keeps
/// the dispatch free of an `unwrap` on a device-driven path.
fn unreachable_record() -> Error {
    Error::IllegalTransition {
        from: "not_a_record_command".to_owned(),
        op: "build a recording request".to_owned(),
    }
}

/// The exit code a failure leaves behind.
///
/// **One code for every D13 error**, not eighteen. Shell exit codes are a one-bit channel,
/// and mapping a growing registry onto small integers invites a script to treat `2` as
/// meaningful and then break when a nineteenth variant lands.
///
/// ## What this used to say, and why the correction matters (note **N124**)
///
/// It said a caller wanting to branch on *which* thing went wrong "reads `--json`, where the
/// whole typed error is". **That is not true of either root and never was.** A failing verb
/// prints no document: [`run`] returns the typed error unrendered, both `main`s write
/// `Program::error_line` — one `Display` sentence — to standard error, and standard output
/// stays empty. The discriminant AGENTS says is read unsupervised (*"`Busy` means retry,
/// `DeviceGone` means stop and tell the human, `PermissionDenied` means a setup problem —
/// collapsing them makes the agent guess"*) is therefore not something a command-line caller
/// receives as a value at all; it crosses the **wire** (D13's `data` object, design D10) and
/// stops at the two programs that render it.
///
/// The sentence was found by writing P6e's agent guide against this surface, which is what a
/// manual is for. Nothing here changes: what the two roots do is a decision D10 and D13 make
/// together, and inventing a failure document in a doc comment's footnote is not how this
/// project would take it. `a_failing_verb_prints_no_document_and_no_discriminant_which_is_
/// what_the_guide_says` pins the behaviour the guide now describes, so a build that starts
/// emitting one goes red here rather than leaving the manual quietly wrong.
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
    use schema::control::ControlValue;

    use super::*;

    #[test]
    fn the_command_tree_is_well_formed() {
        // clap's own consistency check: duplicate arguments, conflicting shorts, and
        // malformed help all fail here rather than at somebody's first invocation. Run
        // per root, over the generated `ALL` rather than a hand list, because the tree a
        // binary actually parses with is the renamed one.
        for &program in Program::ALL {
            program.command().debug_assert();
        }
    }

    #[test]
    fn each_root_announces_its_own_name_over_the_one_shared_tree() {
        // The property T4 needs and the parity gate rests on: one surface, two names. If the
        // name were a property of the tree instead of the parse, `webcam-handler-client
        // --help` would announce `webcam-handler-cli` and the P4f gate would be scraping a
        // verb population from a binary that had told it the wrong story about which binary it
        // was.
        //
        // Every assertion is anchored rather than a `contains`, because `webcam-handler-cli`
        // is a prefix of `webcam-handler-client` — the same trap `wch`/`wchc` set before note
        // **N90**'s rename, and it survived it intact. An unanchored membership test would
        // pass for `Program::Cli` over `webcam-handler-client`'s output, and the arm that
        // matters would never go red.
        let mut verbs: Option<Vec<String>> = None;
        for &program in Program::ALL {
            let command = program.command();
            assert_eq!(command.get_name(), program.as_str());

            let version = command.clone().render_version();
            assert!(
                version.starts_with(&format!("{} ", program.as_str())),
                "{program}: {version}"
            );

            // The usage block of a refusal raised off a freshly built tree — the shape
            // `Cli::check` produces — carries the name too, so an operator sent to
            // `--help` is sent to the right one.
            let usage = command.clone().render_usage().to_string();
            assert!(
                usage.starts_with(&format!("Usage: {} ", program.as_str())),
                "{program}: {usage}"
            );

            // And it is the *same* surface underneath: the P4f parity gate derives its
            // verb population by scraping `--help`, so a tree that had been forked to get
            // the name would show up here before it showed up there.
            let offered: Vec<String> = command
                .get_subcommands()
                .map(|sub| sub.get_name().to_owned())
                .collect();
            assert!(offered.len() > 1, "{program}: {offered:?}");
            match &verbs {
                None => verbs = Some(offered),
                Some(first) => assert_eq!(first, &offered, "{program} offers other verbs"),
            }
        }
    }

    #[test]
    fn the_error_line_names_the_program_that_met_the_error() {
        // One format, one variable. The two roots print failures through this rather than each
        // holding a `format!`, so `webcam-handler-client`'s prefix cannot drift from
        // `webcam-handler-cli`'s shape.
        let error = Error::Busy {
            path: "/dev/video0".into(),
            holders: Vec::new(),
        };
        for &program in Program::ALL {
            let line = program.error_line(&error);
            assert!(
                line.starts_with(&format!("{}: ", program.as_str())),
                "{line}"
            );
            assert!(line.contains(&error.to_string()), "{line}");
        }
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
        let cli = Cli::try_parse_from(["webcam-handler-cli", "list"]).expect("parses");
        assert!(matches!(cli.command, Command::List));
        assert!(!cli.json);
        assert_eq!(cli.backend, BackendKindArg(BackendKind::V4l2));

        let cli = Cli::try_parse_from(["webcam-handler-cli", "--json", "info", "cam:obsbot"])
            .expect("parses");
        assert!(cli.json);
        let Command::Info(arg) = &cli.command else {
            panic!("expected info");
        };
        assert_eq!(arg.id().expect("an id").as_str(), "cam:obsbot");

        // The prefix D1 promises, and the `cam:` prefix being optional on input.
        let cli =
            Cli::try_parse_from(["webcam-handler-cli", "controls", "obsbot"]).expect("parses");
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

        let cli = Cli::try_parse_from([
            "webcam-handler-cli",
            "profile",
            "capture",
            "cam:x",
            "-o",
            "p.json",
        ])
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
        let error = Cli::try_parse_from(["webcam-handler-cli", "--backend", "fake", "list"])
            .expect_err("--backend fake without --profile must not parse");
        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::MissingRequiredArgument,
            "{error}"
        );
        assert!(error.to_string().contains("--profile"), "{error}");

        // …and the default backend needs no profile at all.
        assert!(Cli::try_parse_from(["webcam-handler-cli", "list"]).is_ok());
    }

    #[test]
    fn the_fake_backend_is_selectable_with_the_profiles_it_replays() {
        let cli = Cli::try_parse_from([
            "webcam-handler-cli",
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
    fn the_backend_flag_is_reachable_by_the_id_the_matches_are_keyed_on() {
        // `BACKEND_ARG` and the field it names are one name spelled twice, and the failure
        // mode of a mismatch is silent: `ArgMatches::value_source` on an id no argument has
        // answers `None`, which reads exactly like "the flag was not typed" — so
        // `webcam-handler-client` would stop refusing `--backend` and nothing else would
        // notice.
        for &program in Program::ALL {
            assert!(
                program
                    .command()
                    .get_arguments()
                    .any(|arg| arg.get_id() == BACKEND_ARG),
                "{program} has no argument with the id the matches are read by"
            );
        }
    }

    #[test]
    fn a_backend_that_was_typed_is_distinguishable_from_the_one_that_was_defaulted() {
        // The fact `webcam-handler-client` refuses on, and the reason it is a fact rather than
        // a comparison: `--backend` carries `default_value = "v4l2"`, so the *value* is the
        // same in both rows below and only the provenance differs.
        let defaulted =
            Cli::try_parse_checked_from(Program::Client, ["webcam-handler-client", "list"])
                .expect("parses");
        assert_eq!(defaulted.backend, BackendKindArg(BackendKind::V4l2));
        assert!(!defaulted.backend_was_chosen());

        let typed = Cli::try_parse_checked_from(
            Program::Client,
            ["webcam-handler-client", "--backend", "v4l2", "list"],
        )
        .expect("parses");
        assert_eq!(typed.backend, defaulted.backend, "the values are the same");
        assert!(
            typed.backend_was_chosen(),
            "the same value, typed, has to be distinguishable from the default"
        );

        // The other spelling, so the fact is about the flag rather than about one value.
        let other = Cli::try_parse_checked_from(
            Program::Client,
            [
                "webcam-handler-client",
                "--backend",
                "fake",
                "--profile",
                "p.json",
                "list",
            ],
        )
        .expect("parses");
        assert!(other.backend_was_chosen());
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
        let cli = Cli::try_parse_from(["webcam-handler-cli", "get", "cam:x", "brightness"])
            .expect("parses");
        let Command::Get { control, .. } = &cli.command else {
            panic!("expected get");
        };
        assert_eq!(control, "brightness");

        let cli = Cli::try_parse_from([
            "webcam-handler-cli",
            "set",
            "cam:x",
            "brightness=200",
            "contrast=10",
        ])
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
                .map(|assignment| (assignment.0.control.as_str(), assignment.0.value.clone()))
                .collect::<Vec<_>>(),
            vec![
                ("brightness", ControlValue::Int(200)),
                ("contrast", ControlValue::Int(10)),
            ]
        );
        assert!(!no_guard, "the guard is the default (D3)");

        let cli = Cli::try_parse_from(["webcam-handler-cli", "snapshot", "cam:x", "-o", "s.json"])
            .expect("parses");
        assert!(matches!(cli.command, Command::Snapshot { .. }));
        let cli = Cli::try_parse_from(["webcam-handler-cli", "restore", "cam:x", "s.json"])
            .expect("parses");
        assert!(matches!(cli.command, Command::Restore { .. }));
    }

    #[test]
    fn a_malformed_assignment_is_refused_at_parse_time_naming_what_was_typed() {
        // A usage error rather than a device error: exit 2 rather than 1, so a script
        // that retries on "the camera is busy" does not retry on a typo.
        for bad in ["brightness", "brightness=high", "=5"] {
            let error = Cli::try_parse_from(["webcam-handler-cli", "set", "cam:x", bad])
                .expect_err("a malformed assignment must not parse");
            assert_eq!(
                error.kind(),
                clap::error::ErrorKind::ValueValidation,
                "{bad}"
            );
        }
        // …and a well-formed one, negative values included, does parse.
        let cli =
            Cli::try_parse_from(["webcam-handler-cli", "set", "cam:x", "pan_absolute=-468000"])
                .expect("negative control values are ordinary");
        let Command::Set { assignments, .. } = &cli.command else {
            panic!("expected set");
        };
        assert_eq!(assignments[0].0.value, ControlValue::Int(-468_000));
    }

    #[test]
    fn the_photo_vocabularies_are_the_schemas_and_an_unknown_name_lists_the_known_ones() {
        for &transform in Transform::ALL {
            let cli = Cli::try_parse_from([
                "webcam-handler-cli",
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
            "webcam-handler-cli",
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
    fn record_needs_a_path_and_that_is_why_it_needs_no_json_rule_of_its_own() {
        // Two halves of one decision. **`-o` is required**, because a recording's bytes go to
        // a path and never back in the answer (note **N110**) — so there is no "standard
        // output" spelling for a recording. **Therefore `record --json` needs no counterpart
        // to `photo`'s rule**: nothing is competing for standard output, so the document is
        // the whole of it. The second half is asserted rather than assumed, because "the rule
        // is unnecessary" is exactly the kind of claim that stops being true quietly.
        let missing =
            Cli::try_parse_checked_from(Program::Cli, ["webcam-handler-cli", "record", "cam:x"])
                .expect_err("record without -o must not parse");
        assert_eq!(
            missing.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );

        // And with a path, `--json` parses — the arm `Cli::check` deliberately does not have.
        let cli = Cli::try_parse_checked_from(
            Program::Cli,
            [
                "webcam-handler-cli",
                "--json",
                "record",
                "cam:x",
                "-o",
                "take.avi",
            ],
        )
        .expect("record --json needs no path rule because -o is already required");
        assert!(cli.json);
        assert!(matches!(cli.command, Command::Record { .. }));
    }

    #[test]
    fn a_duration_is_a_humantime_string_and_one_this_build_cannot_read_is_a_usage_error() {
        // Note **N113**: `--duration` is this project's first human-scale duration flag, and
        // the answer it produces is milliseconds because that is what the request carries.
        // Parsed by clap so a typo is exit 2 with a message naming the flag, rather than
        // `None` three layers down that reads exactly like not passing the flag (note N109's
        // finding, at a different flag).
        for (typed, expected) in [
            ("10s", 10_000_u64),
            ("1500ms", 1_500),
            ("1m30s", 90_000),
            ("0s", 0),
        ] {
            let cli = Cli::try_parse_from([
                "webcam-handler-cli",
                "record",
                "cam:x",
                "-o",
                "take.avi",
                "--duration",
                typed,
            ])
            .unwrap_or_else(|error| panic!("{typed} should parse: {error}"));
            let Command::Record { duration, .. } = &cli.command else {
                panic!("expected record");
            };
            assert_eq!(*duration, Some(expected), "{typed}");
        }

        // The inverse arm, without which this test cannot discriminate: a bare integer is
        // **not** a duration, which is the whole of what choosing humantime commits to — and
        // the message says what to write instead, because AGENTS' primary consumer has no
        // hands.
        for bad in ["10", "soon", "10 fortnights"] {
            let error = Cli::try_parse_from([
                "webcam-handler-cli",
                "record",
                "cam:x",
                "-o",
                "take.avi",
                "--duration",
                bad,
            ])
            .expect_err("a value humantime cannot read must not parse");
            assert_eq!(
                error.kind(),
                clap::error::ErrorKind::ValueValidation,
                "{bad}"
            );
            assert!(error.to_string().contains("10s"), "{error}");
        }

        // And no duration at all is `None`, which `RecordRequest::budget_ms` turns into
        // `limits::DEFAULT_RECORDING_MS` — the one home for that default.
        let cli = Cli::try_parse_from(["webcam-handler-cli", "record", "cam:x", "-o", "take.avi"])
            .expect("parses");
        let Command::Record { duration, .. } = &cli.command else {
            panic!("expected record");
        };
        assert_eq!(*duration, None);
    }

    #[test]
    fn a_relative_recording_path_is_resolved_against_the_callers_directory_before_it_is_sent() {
        // D10's rule at the verb that landed last, and the assertion is the same one
        // `photo`'s carries: the resolution happens **here**, in the shared surface, so
        // `webcam-handler-cli record -o take.avi` and `webcam-handler-client record -o
        // take.avi` name the same file. A daemon handed the relative path would resolve it
        // against its own working directory, which under systemd is `/`.
        let cli =
            Cli::try_parse_from(["webcam-handler-cli", "record", "cam:x", "-o", "takes/a.avi"])
                .expect("parses");
        let request = cli
            .command
            .record_request(camino::Utf8Path::new("/home/someone"))
            .expect("an absolute path this build writes")
            .expect("a record command produces a request");
        assert_eq!(
            request.sink,
            Sink::ServerPath {
                path: "/home/someone/takes/a.avi".into()
            }
        );

        // An absolute path is left alone, so the join above is a *resolution* rather than a
        // prefix applied to everything.
        let cli =
            Cli::try_parse_from(["webcam-handler-cli", "record", "cam:x", "-o", "/tmp/a.avi"])
                .expect("parses");
        let request = cli
            .command
            .record_request(camino::Utf8Path::new("/home/someone"))
            .expect("absolute")
            .expect("a record command produces a request");
        assert_eq!(
            request.sink,
            Sink::ServerPath {
                path: "/tmp/a.avi".into()
            }
        );

        // And a command that is not a `record` produces nothing rather than a wrong request —
        // the arm `run`'s dispatch relies on, and the reason it has no `unwrap` on it.
        let cli = Cli::try_parse_from(["webcam-handler-cli", "list"]).expect("parses");
        assert!(
            cli.command
                .record_request(camino::Utf8Path::new("/tmp"))
                .expect("no request to refuse")
                .is_none()
        );
    }

    #[test]
    fn a_container_this_build_cannot_write_is_refused_while_the_command_line_is_being_parsed() {
        // The `.webp` defect one container along (debt D-1, note **N46**): `/tmp/take.mkv`
        // filled with a Y4M is a file whose name lies about its contents. Refused **here**,
        // before anything opens a camera — but by `RecordRequest::container`, which lives
        // beside the variants it constrains, because `webcam-handler-daemon` links no
        // `cli-core` and a socket can build the same request.
        let cli =
            Cli::try_parse_from(["webcam-handler-cli", "record", "cam:x", "-o", "/tmp/a.mkv"])
                .expect("clap has no opinion about extensions");
        let refused = cli
            .command
            .record_request(camino::Utf8Path::new("/tmp"))
            .expect_err("this build writes no Matroska");
        assert_eq!(refused.kind(), schema::ErrorKind::IllegalTransition);
        let rendered = refused.to_string();
        for &container in schema::video::VideoFormat::ALL {
            assert!(rendered.contains(container.extension()), "{rendered}");
        }

        // Both writable extensions parse, and so does a path with none — the arm AGENTS'
        // handless consumer depends on, since an agent that has not enumerated a camera
        // cannot know whether to type `.avi` or `.y4m`.
        for &container in schema::video::VideoFormat::ALL {
            let path = format!("/tmp/a.{}", container.extension());
            let cli = Cli::try_parse_from(["webcam-handler-cli", "record", "cam:x", "-o", &path])
                .expect("parses");
            cli.command
                .record_request(camino::Utf8Path::new("/tmp"))
                .unwrap_or_else(|error| panic!("{path}: {error}"));
        }
        let cli = Cli::try_parse_from(["webcam-handler-cli", "record", "cam:x", "-o", "/tmp/take"])
            .expect("parses");
        cli.command
            .record_request(camino::Utf8Path::new("/tmp"))
            .expect("a path with no extension lets the negotiated stream decide");

        // And a duration past the cap is refused by the same call, which is what keeps
        // `record --duration 500m` from costing a camera a stream (`budget_ms`, note N110's
        // sibling refusal).
        let cli = Cli::try_parse_from([
            "webcam-handler-cli",
            "record",
            "cam:x",
            "-o",
            "/tmp/a.avi",
            "--duration",
            &format!("{}ms", schema::limits::MAX_RECORDING_MS + 1),
        ])
        .expect("clap has no opinion about the cap");
        assert_eq!(
            cli.command
                .record_request(camino::Utf8Path::new("/tmp"))
                .expect_err("a millisecond past the cap is past it")
                .kind(),
            schema::ErrorKind::IllegalTransition
        );
    }

    #[test]
    fn json_photo_needs_a_path_because_the_bytes_and_the_document_share_one_stream() {
        // Without `-o`, the photo's bytes *are* standard output. Emitting a JSON document
        // there too would produce a file that is neither. clap refuses it, so the answer
        // is a usage error rather than a corrupt image.
        let error = Cli::try_parse_checked_from(
            Program::Cli,
            ["webcam-handler-cli", "--json", "photo", "cam:x"],
        )
        .expect_err("--json without -o must not parse");
        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
        assert!(error.to_string().contains("--out"), "{error}");

        // Both halves of the inverse: `-o` with `--json`, and no `--json` without `-o`.
        assert!(
            Cli::try_parse_checked_from(
                Program::Cli,
                [
                    "webcam-handler-cli",
                    "--json",
                    "photo",
                    "cam:x",
                    "-o",
                    "a.jpg"
                ]
            )
            .is_ok()
        );
        assert!(
            Cli::try_parse_checked_from(Program::Cli, ["webcam-handler-cli", "photo", "cam:x"])
                .is_ok()
        );
        // And the rule is about `photo` alone: every other verb answers in JSON with no
        // path at all, which is the whole point of `--json`.
        assert!(
            Cli::try_parse_checked_from(Program::Cli, ["webcam-handler-cli", "--json", "list"])
                .is_ok()
        );
    }

    #[test]
    fn a_relative_output_path_is_resolved_against_the_callers_directory() {
        // D10: `-o out.jpg` means the caller's directory in `webcam-handler-cli` and in
        // `webcam-handler-client` alike, and the resolution happens here — in the shared
        // surface — so the two cannot differ.
        let cli =
            Cli::try_parse_from(["webcam-handler-cli", "photo", "cam:x", "-o", "shots/a.jpg"])
                .expect("parses");
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
        let cli = Cli::try_parse_from(["webcam-handler-cli", "photo", "cam:x", "-o", "/tmp/a.jpg"])
            .expect("parses");
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
        let cli =
            Cli::try_parse_from(["webcam-handler-cli", "photo", "cam:x", "-o", "/tmp/a.webp"])
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
            "webcam-handler-cli",
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
                "webcam-handler-cli",
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
        let plain = Cli::try_parse_from(["webcam-handler-cli", "photo", "cam:x", "-o", "a.jpg"])
            .expect("parses");
        let request = plain
            .command
            .photo_request(camino::Utf8Path::new("/tmp"))
            .expect("builds")
            .expect("a request");
        assert_eq!(request.settle, SettlePolicy::default());
    }

    #[test]
    fn a_pixel_format_flag_that_names_no_format_is_refused_rather_than_ignored() {
        // Note **N109**. `--pixel-format` used to be an `Option<String>` filtered through
        // `and_then(PixelFormat::parse)`, so every spelling below produced `None` — which is
        // indistinguishable from omitting the flag, and D5's ranking then chose a format the
        // caller had not asked for while the answer reported success. An unattended agent
        // has no way to notice that; a non-zero exit and a usage line is the whole repair.
        for typo in ["MJP", "MJPGX", "\\x4d", "mjpg "] {
            let refused = Cli::try_parse_from([
                "webcam-handler-cli",
                "photo",
                "cam:x",
                "-o",
                "a.jpg",
                "--pixel-format",
                typo,
            ]);
            assert!(
                refused.is_err(),
                "--pixel-format {typo:?} was accepted and would have been silently dropped"
            );
        }
        // The flag and the wire agree about what a format is called, which is the reason
        // the decoder has one home: an unprintable fourcc is typable, and a padded kernel
        // name copied out of `v4l2-ctl` parses as itself.
        for (typed, expected) in [
            ("MJPG", PixelFormat::MJPG),
            ("\\x00\\x01AB", PixelFormat([0x00, 0x01, b'A', b'B'])),
            ("Y16 ", PixelFormat(*b"Y16 ")),
        ] {
            let cli = Cli::try_parse_from([
                "webcam-handler-cli",
                "photo",
                "cam:x",
                "-o",
                "a.jpg",
                "--pixel-format",
                typed,
            ])
            .expect("parses");
            let request = cli
                .command
                .photo_request(camino::Utf8Path::new("/tmp"))
                .expect("builds")
                .expect("a request");
            assert_eq!(request.stream.pixel_format, Some(expected), "{typed:?}");
        }
    }

    #[test]
    fn the_wait_flag_reaches_the_request_and_is_absent_from_it_by_default() {
        // D12's flag, which landed at P4f with the surface that can mean it (notes N42, N56).
        // Both directions, because a request that always waited and one that never did would
        // each pass one of them — and the daemon branches on this field, so a build that
        // dropped it would turn `webcam-handler-client photo --wait` into a `Busy` refusal on
        // a camera that was about to be free.
        let waiting = Cli::try_parse_checked_from(
            Program::Cli,
            [
                "webcam-handler-cli",
                "photo",
                "cam:x",
                "-o",
                "a.jpg",
                "--wait",
            ],
        )
        .expect("parses");
        let request = waiting
            .command
            .photo_request(camino::Utf8Path::new("/tmp"))
            .expect("builds")
            .expect("a request");
        assert!(request.wait);

        let plain = Cli::try_parse_checked_from(
            Program::Cli,
            ["webcam-handler-cli", "photo", "cam:x", "-o", "a.jpg"],
        )
        .expect("parses");
        let request = plain
            .command
            .photo_request(camino::Utf8Path::new("/tmp"))
            .expect("builds")
            .expect("a request");
        assert!(!request.wait, "waiting must never be the default");

        // It is a flag on `photo` and on nothing else: the queue it waits for is a camera's
        // one thread, and no other verb takes a capture through it. A `--wait` clap accepted
        // anywhere would be a flag with no reader.
        assert!(
            Cli::try_parse_checked_from(Program::Cli, ["webcam-handler-cli", "list", "--wait"])
                .is_err()
        );

        // And the `--help` line says it is inert under `webcam-handler-cli`, which is the
        // alternative to leaving a user to discover it. Read off the built tree rather than
        // from the source (rubric rule 6), and asserted for both roots, because the surface is
        // shared.
        for &program in Program::ALL {
            let help = program
                .command()
                .find_subcommand_mut("photo")
                .expect("the photo verb")
                .render_long_help()
                .to_string();
            assert!(help.contains("--wait"), "{program}: {help}");
            assert!(
                help.contains("Inert under `webcam-handler-cli`"),
                "{program}: {help}"
            );
        }
    }

    #[test]
    fn a_malformed_size_is_refused_naming_the_shape_it_wanted() {
        for bad in ["1920", "1920*1080", "wide x tall"] {
            assert!(
                Cli::try_parse_from([
                    "webcam-handler-cli",
                    "photo",
                    "cam:x",
                    "-o",
                    "a.jpg",
                    "--size",
                    bad
                ])
                .is_err(),
                "{bad} should not parse as a size"
            );
        }
        assert!(
            Cli::try_parse_from([
                "webcam-handler-cli",
                "photo",
                "cam:x",
                "-o",
                "a.jpg",
                "--size",
                "640x480"
            ])
            .is_ok()
        );
    }

    #[test]
    fn the_calibrate_verbs_parse_the_way_the_agent_guide_will_teach_them() {
        let cli = Cli::try_parse_checked_from(
            Program::Cli,
            [
                "webcam-handler-cli",
                "calibrate",
                "start",
                "cam:obsbot",
                "--task",
                "read text from the DUT display",
                "--goal",
                "legible text",
                "--criterion",
                "text clarity",
                "--criterion",
                "colour accuracy",
            ],
        )
        .expect("parses");
        let Command::Calibrate(CalibrateCommand::Start {
            task,
            goal,
            criteria,
            ..
        }) = &cli.command
        else {
            panic!("expected calibrate start");
        };
        assert_eq!(task, "read text from the DUT display");
        assert_eq!(goal, "legible text");
        // Ordered, because D8 says the criteria are ranked and the *selector* reads them.
        assert_eq!(criteria, &["text clarity", "colour accuracy"]);

        let cli = Cli::try_parse_checked_from(
            Program::Cli,
            [
                "webcam-handler-cli",
                "calibrate",
                "apply",
                "cam:obsbot",
                "--task",
                "focus",
                "--partial",
            ],
        )
        .expect("parses");
        let Command::Calibrate(CalibrateCommand::Apply { partial, which, .. }) = &cli.command
        else {
            panic!("expected calibrate apply");
        };
        assert!(partial);
        assert_eq!(
            which.which().expect("a session"),
            SessionRef::Task {
                task: "focus".to_owned()
            }
        );

        // `--partial` is opt-in: without it the D8 gate is the one that answers.
        let cli = Cli::try_parse_checked_from(
            Program::Cli,
            [
                "webcam-handler-cli",
                "calibrate",
                "apply",
                "cam:obsbot",
                "--task",
                "f",
            ],
        )
        .expect("parses");
        let Command::Calibrate(CalibrateCommand::Apply { partial, .. }) = &cli.command else {
            panic!("expected calibrate apply");
        };
        assert!(!partial, "--partial must never be the default");

        // `restore` — the eighth verb — names a session the two ordinary ways and takes no
        // other argument: it puts the camera back where *that session* found it, and there
        // is nothing to choose (note N23).
        let cli = Cli::try_parse_checked_from(
            Program::Cli,
            [
                "webcam-handler-cli",
                "calibrate",
                "restore",
                "cam:obsbot",
                "--task",
                "focus",
            ],
        )
        .expect("parses");
        let Command::Calibrate(CalibrateCommand::Restore { which, .. }) = &cli.command else {
            panic!("expected calibrate restore");
        };
        assert_eq!(
            which.which().expect("a session"),
            SessionRef::Task {
                task: "focus".to_owned()
            }
        );
        // …and it needs one, like every other session verb.
        assert!(
            Cli::try_parse_checked_from(
                Program::Cli,
                ["webcam-handler-cli", "calibrate", "restore", "cam:obsbot"]
            )
            .is_err()
        );

        // `list` takes an optional camera: every session on the machine, or one camera's.
        let cli =
            Cli::try_parse_checked_from(Program::Cli, ["webcam-handler-cli", "calibrate", "list"])
                .expect("parses");
        assert!(matches!(
            cli.command,
            Command::Calibrate(CalibrateCommand::List { camera: None })
        ));
    }

    #[test]
    fn a_negative_control_value_survives_the_command_line_in_both_flag_forms() {
        // Found by the P3e R3 run: `--values -108000,0,108000` was refused with "unexpected
        // argument '-1' found", because every PTZ range is centred on zero and clap reads a
        // leading minus as a flag. A tool whose reason for existing includes a pan/tilt head
        // cannot refuse the half of that head's range that is negative.
        //
        // Both forms, because the two are separate clap paths and only the separated one was
        // broken — a test that used `=` would have stayed green through the defect.
        for args in [
            ["--values=-108000,0,108000"].as_slice(),
            ["--values", "-108000,0,108000"].as_slice(),
        ] {
            let cli = Cli::try_parse_checked_from(
                Program::Cli,
                [
                    "webcam-handler-cli",
                    "calibrate",
                    "sweep",
                    "cam:obsbot",
                    "pan_absolute",
                    "--task",
                    "framing",
                    "--allow-motion",
                ]
                .iter()
                .copied()
                .chain(args.iter().copied()),
            )
            .unwrap_or_else(|error| panic!("{args:?} must parse: {error}"));
            let Command::Calibrate(CalibrateCommand::Sweep { plan, .. }) = &cli.command else {
                panic!("expected calibrate sweep");
            };
            assert_eq!(
                plan.spec().expect("a plan"),
                SweepSpec::Explicit {
                    values: vec![-108_000, 0, 108_000]
                },
                "{args:?}"
            );
        }

        // The same for the value a selector names: it is a value the camera held, and on a
        // pan control half of those are negative.
        for args in [
            ["--value=-3600", "--by", "agent"].as_slice(),
            ["--value", "-3600", "--by", "agent"].as_slice(),
        ] {
            let cli = Cli::try_parse_checked_from(
                Program::Cli,
                [
                    "webcam-handler-cli",
                    "calibrate",
                    "select",
                    "cam:obsbot",
                    "pan_absolute",
                ]
                .iter()
                .copied()
                .chain(["--task", "framing"])
                .chain(args.iter().copied()),
            )
            .unwrap_or_else(|error| panic!("{args:?} must parse: {error}"));
            let Command::Calibrate(CalibrateCommand::Select { by, .. }) = &cli.command else {
                panic!("expected calibrate select");
            };
            assert_eq!(
                by.selection().expect("a selection"),
                Selection::ByValue {
                    value: -3600,
                    chosen_by: ChosenBy::Agent
                },
                "{args:?}"
            );
        }
    }

    #[test]
    fn a_session_is_named_by_task_or_by_id_and_never_by_neither_or_both() {
        let id = "019fd0f0-0000-7000-8000-000000000001";
        let cli = Cli::try_parse_checked_from(
            Program::Cli,
            [
                "webcam-handler-cli",
                "calibrate",
                "status",
                "cam:x",
                "--session",
                id,
            ],
        )
        .expect("parses");
        let Command::Calibrate(CalibrateCommand::Status { which, .. }) = &cli.command else {
            panic!("expected calibrate status");
        };
        assert_eq!(
            which.which().expect("a session"),
            SessionRef::Id {
                id: id.parse().expect("a uuid")
            }
        );

        // Neither, and both: usage errors, because the tool cannot guess which session is
        // meant and must not pick one.
        assert!(
            Cli::try_parse_checked_from(
                Program::Cli,
                ["webcam-handler-cli", "calibrate", "status", "cam:x"]
            )
            .is_err()
        );
        assert!(
            Cli::try_parse_checked_from(
                Program::Cli,
                [
                    "webcam-handler-cli",
                    "calibrate",
                    "status",
                    "cam:x",
                    "--task",
                    "t",
                    "--session",
                    id,
                ]
            )
            .is_err()
        );
        // And a UUID that is not one is refused at parse time rather than looked up.
        assert!(
            Cli::try_parse_checked_from(
                Program::Cli,
                [
                    "webcam-handler-cli",
                    "calibrate",
                    "status",
                    "cam:x",
                    "--session",
                    "the-one-from-yesterday",
                ]
            )
            .is_err()
        );
    }

    #[test]
    fn every_sweep_plan_flag_maps_to_the_spec_it_names_and_the_four_are_alternatives() {
        let spec = |args: &[&str]| -> SweepSpec {
            let mut argv = vec![
                "webcam-handler-cli",
                "calibrate",
                "sweep",
                "cam:x",
                "--task",
                "t",
                "focus_absolute",
            ];
            argv.extend_from_slice(args);
            let cli = Cli::try_parse_checked_from(Program::Cli, argv).expect("parses");
            let Command::Calibrate(CalibrateCommand::Sweep { plan, .. }) = &cli.command else {
                panic!("expected calibrate sweep");
            };
            plan.spec().expect("a spec")
        };
        assert_eq!(spec(&["--all"]), SweepSpec::All);
        assert_eq!(spec(&["--step", "16"]), SweepSpec::Uniform { step: 16 });
        assert_eq!(spec(&["--points", "8"]), SweepSpec::Log { points: 8 });
        assert_eq!(
            spec(&["--values", "0,64,128"]),
            SweepSpec::Explicit {
                values: vec![0, 64, 128]
            }
        );

        // A sweep is minutes of camera time and, on a PTZ head, motor travel: it says how
        // big it is or it does not run. Both failure directions — none of the four, and
        // two of them.
        assert!(
            Cli::try_parse_checked_from(
                Program::Cli,
                [
                    "webcam-handler-cli",
                    "calibrate",
                    "sweep",
                    "cam:x",
                    "--task",
                    "t",
                    "focus_absolute",
                ]
            )
            .is_err()
        );
        assert!(
            Cli::try_parse_checked_from(
                Program::Cli,
                [
                    "webcam-handler-cli",
                    "calibrate",
                    "sweep",
                    "cam:x",
                    "--task",
                    "t",
                    "focus_absolute",
                    "--all",
                    "--step",
                    "4",
                ]
            )
            .is_err()
        );

        // Motion is never implicit (design §5), and the flag is what makes it explicit.
        let cli = Cli::try_parse_checked_from(
            Program::Cli,
            [
                "webcam-handler-cli",
                "calibrate",
                "sweep",
                "cam:x",
                "--task",
                "t",
                "pan_absolute",
                "--step",
                "3600",
                "--allow-motion",
            ],
        )
        .expect("parses");
        let Command::Calibrate(CalibrateCommand::Sweep {
            allow_motion,
            settle,
            ..
        }) = &cli.command
        else {
            panic!("expected calibrate sweep");
        };
        assert!(allow_motion);
        // The settle flags are the photo verb's, flattened: one declaration, so a sweep and
        // a photo ask the device for the same thing in the same words.
        assert_eq!(settle.policy(), SettlePolicy::default());
    }

    #[test]
    fn the_selector_flags_record_who_chose_and_refuse_the_combinations_that_would_lie() {
        let selection = |args: &[&str]| -> Result<Selection> {
            let mut argv = vec![
                "webcam-handler-cli",
                "calibrate",
                "select",
                "cam:x",
                "--task",
                "t",
                "focus_absolute",
            ];
            argv.extend_from_slice(args);
            let cli = Cli::try_parse_checked_from(Program::Cli, argv).expect("parses");
            let Command::Calibrate(CalibrateCommand::Select { by, .. }) = &cli.command else {
                panic!("expected calibrate select");
            };
            by.selection()
        };
        assert_eq!(
            selection(&["--metric", "sharpness"]).expect("a metric ranks"),
            Selection::ByMetric {
                metric: MetricName::Sharpness
            }
        );
        assert_eq!(
            selection(&["--value", "512", "--by", "agent"]).expect("an agent chooses"),
            Selection::ByValue {
                value: 512,
                chosen_by: ChosenBy::Agent
            }
        );
        assert_eq!(
            selection(&["--value", "512", "--by", "human"]).expect("a human chooses"),
            Selection::ByValue {
                value: 512,
                chosen_by: ChosenBy::Human
            }
        );

        // A value with nobody claiming it would record a calibration whose selector was
        // invented, which is the one thing D8's selector field exists to prevent.
        assert!(
            Cli::try_parse_checked_from(
                Program::Cli,
                [
                    "webcam-handler-cli",
                    "calibrate",
                    "select",
                    "cam:x",
                    "--task",
                    "t",
                    "focus_absolute",
                    "--value",
                    "512",
                ]
            )
            .is_err()
        );
        // Nor may a caller *claim* to be a metric: that is something the tool computes.
        assert!("metric:sharpness".parse::<ChosenByArg>().is_err());
        assert!("metric".parse::<ChosenByArg>().is_err());
        // …and the two it does accept are the schema's own spellings. `ALL` is generated
        // from the enum by `closed_vocabulary!`, so this walk is over a population the
        // compiler owns rather than over a hand list that can silently omit a variant
        // (rubric rule 6) — and each member is driven through **clap**, not through
        // `FromStr` alone, so a chooser the type declares and the command line cannot
        // reach fails here rather than at somebody's terminal.
        for chooser in ChosenBy::ALL {
            let label = chooser.selector().label();
            assert_eq!(
                label.parse::<ChosenByArg>().expect("known"),
                ChosenByArg(*chooser),
                "{label} does not round-trip through --by's parser"
            );
            assert_eq!(
                selection(&["--value", "512", "--by", &label])
                    .unwrap_or_else(|error| panic!("--by {label} was refused: {error}")),
                Selection::ByValue {
                    value: 512,
                    chosen_by: *chooser,
                },
                "--by {label} did not reach the selector the type maps it to"
            );
            // And the refusal names it, so a caller who mistyped is told the whole set.
            let refusal = "not-a-chooser"
                .parse::<ChosenByArg>()
                .expect_err("no such chooser");
            assert!(
                refusal.contains(&label),
                "the unknown-chooser refusal does not name {label}: {refusal}"
            );
        }
        // A metric and a value together is two answers to one question.
        assert!(
            Cli::try_parse_checked_from(
                Program::Cli,
                [
                    "webcam-handler-cli",
                    "calibrate",
                    "select",
                    "cam:x",
                    "--task",
                    "t",
                    "focus_absolute",
                    "--metric",
                    "sharpness",
                    "--value",
                    "512",
                    "--by",
                    "human",
                ]
            )
            .is_err()
        );
        // And a metric this build does not compute is refused naming the ones it does.
        let error = Cli::try_parse_checked_from(
            Program::Cli,
            [
                "webcam-handler-cli",
                "calibrate",
                "select",
                "cam:x",
                "--task",
                "t",
                "focus_absolute",
                "--metric",
                "vibes",
            ],
        )
        .expect_err("vibes is not a metric");
        for &metric in MetricName::ALL {
            assert!(error.to_string().contains(metric.as_str()), "{error}");
        }
    }

    #[test]
    fn photo_bytes_never_reach_a_debug_line() {
        // Rubric A12 as a test, and the reason `Photograph` hand-writes `Debug`: a frame
        // may contain a person, so formatting a document that holds one has to be
        // incapable of printing it. The renderer both binaries share takes one of these,
        // so a `?photograph` in either of them would print a whole JPEG.
        use schema::camera::{CameraId, FrameInterval, PixelFormat};
        use schema::capture::{
            NegotiatedStream, PhotoDelivery, PhotoFormat, PhotoRendering, PhotoReport,
            TransformApplication,
        };
        use schema::time::Stamp;

        let bytes = vec![0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10];
        let report = PhotoReport {
            camera: CameraId::parse("cam:test").expect("literal id"),
            taken_at: Stamp::epoch(),
            negotiated: NegotiatedStream {
                pixel_format: PixelFormat::MJPG,
                width: 640,
                height: 480,
                bytes_per_line: 0,
                size_image: 65_536,
                interval: FrameInterval::Discrete {
                    numerator: 1,
                    denominator: 30,
                },
                adjustments: Vec::new(),
            },
            rendering: PhotoRendering::Verbatim {
                source: PixelFormat::MJPG,
            },
            transform: TransformApplication::Identity,
            width: 640,
            height: 480,
            frames_settled: 1,
            delivery: PhotoDelivery::Bytes {
                format: PhotoFormat::Jpeg,
                byte_count: 6,
            },
        };

        let returned = Photograph {
            report: report.clone(),
            returned: Some(bytes.clone()),
        };
        let rendered = format!("{returned:?}");
        assert!(rendered.contains("<6 bytes>"), "{rendered}");
        // `255, 216, 255` is what a derived `Debug` prints for a JPEG's first three bytes.
        assert!(
            !rendered.contains("255, 216"),
            "frame bytes leaked: {rendered}"
        );

        // The other variant, so the redaction does not erase the distinction it protects:
        // "written to a file" and "an empty payload" must still read differently.
        let to_a_file = Photograph {
            report,
            returned: None,
        };
        assert!(format!("{to_a_file:?}").contains("None"));
        assert_ne!(format!("{to_a_file:?}"), rendered);
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
