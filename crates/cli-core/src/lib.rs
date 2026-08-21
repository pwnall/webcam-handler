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
use schema::camera::PixelFormat;
use schema::capture::{
    PhotoFormat, PhotoRequest, SettlePolicy, SettleSpec, Sink, StreamRequest, Transform,
};
use schema::control::{ControlDesc, ControlSlug, ControlWrite};
use schema::error::{Error, ErrorKind, Result};
use schema::metrics::MetricName;
use schema::profile::DeviceProfile;
use schema::report::{CameraDetail, CameraList, ControlReport, WriteReport};
use schema::selector::{CameraSelector, SelectorScheme};
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

pub use render::{Bar, Output, Quiet};

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
    ///
    /// **It also undoes rustdoc's bracket escape**, which is the other thing standing between
    /// a doc comment and a person. This repository cites a probe finding as `\[PF:3\]` in
    /// prose, escaped because `-D warnings` reads a bare `[PF:3]` as an intra-doc link to an
    /// item called `PF:3` and refuses it. Rustdoc renders the escape away; clap does not, so
    /// four flags shipped `\[PF:11\]` to a terminal — an instruction to a documentation tool
    /// that is not running, which is note **N123**'s defect wearing a backslash. The two
    /// generated artifacts already undo it (`xtask::unescape_doc_brackets`), and this is the
    /// third surface a doc comment reaches, so it undoes it here rather than asking four
    /// strings — and every one written after them — to spell a citation two ways.
    #[must_use]
    pub fn command(self) -> clap::Command {
        use clap::CommandFactory as _;

        Program::unescape(Cli::command().name(self.as_str()))
    }

    /// Whether this root composes a backend of its own.
    ///
    /// The one difference between the two binaries that a *command line* can be wrong about,
    /// and therefore the one `Cli::check` needs: `webcam-handler-cli` builds a backend per
    /// invocation and `--backend fake` there is a request this process must be able to
    /// satisfy, so it needs `--profile`. `webcam-handler-client` links no backend at all
    /// (AGENTS: "`webcam-handler-client` links no backend and no engine") and refuses both
    /// flags outright, so requiring one beside the other would be a refusal telling a caller
    /// to add a flag that is itself refused (note **N214**).
    #[must_use]
    pub const fn builds_a_backend(self) -> bool {
        match self {
            Program::Cli => true,
            Program::Client => false,
        }
    }

    /// Undo rustdoc's bracket escape everywhere this tree prints prose.
    ///
    /// Walks the same four strings per argument that
    /// `contracts::no_text_this_surface_prints_carries_a_rustdoc_link` bans links in — about,
    /// long about, help, long help — because clap prints the first paragraph for `-h` and the
    /// whole comment for `--help`, and a rule that reached only one would leave the other half
    /// carrying backslashes. Recursive over subcommands for the same reason the ban is.
    fn unescape(command: clap::Command) -> clap::Command {
        fn undo(text: &clap::builder::StyledStr) -> String {
            text.to_string().replace("\\[", "[").replace("\\]", "]")
        }

        let mut command = command;
        if let Some(about) = command.get_about().map(undo) {
            command = command.about(about);
        }
        if let Some(long) = command.get_long_about().map(undo) {
            command = command.long_about(long);
        }
        command = command.mut_args(|arg| {
            let help = arg.get_help().map(undo);
            let long_help = arg.get_long_help().map(undo);
            let mut arg = arg;
            if let Some(help) = help {
                arg = arg.help(help);
            }
            if let Some(long_help) = long_help {
                arg = arg.long_help(long_help);
            }
            arg
        });
        command.mut_subcommands(Program::unescape)
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
    /// Required with `--backend fake` on the root that builds a backend, and enforced while
    /// the command line is being parsed rather than at run time: a backend with nothing to
    /// replay enumerates nothing, and "no cameras" is exactly what a user whose cameras had
    /// vanished would see. A usage mistake must not be spelled like a device answer.
    #[arg(long, global = true, value_name = "PATH")]
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
    /// There are two, and both are here rather than as attributes because **the tree is
    /// shared and an attribute is not** (T4). A `#[arg]` rule is a property of the one
    /// [`Cli`] both binaries parse with, so clap applies it to both — and clap applies it
    /// *before* either root's own code runs.
    ///
    /// - `--json photo` with no `--out`: `--json` is a **global** argument, and clap's
    ///   `required_if_eq` resolves the arg it names within the command that declares it, so a
    ///   subcommand cannot name a flag defined on the root. Written out so the rule is
    ///   visible and so the refusal is still a usage error rather than a device one.
    /// - `--backend fake` with no `--profile`: this was `required_if_eq("backend", "fake")`
    ///   on the field until 2026-08-17, which made it true of `webcam-handler-client` as
    ///   well — a root that builds no backend at all. Measured: `webcam-handler-client
    ///   --backend fake list` answered *"the following required arguments were not provided:
    ///   `--profile <PATH>`"* and exit 2, naming a flag whose addition cannot help, because
    ///   `client::refuse_composition_flags` refuses `--profile` too. That is note **N123**'s
    ///   defect — a message naming a flag that does nothing for the reader — and clap's
    ///   ordering is what put it there, so the rule moved to where the *root* is known
    ///   (docs/11 **M20**, note **N214**).
    ///
    /// It builds the tree through `program` for the same reason the parse does: a refusal
    /// whose usage block named the other binary would send an operator to the wrong
    /// `--help`.
    fn check(&self, program: Program) -> std::result::Result<(), clap::Error> {
        // `diff: None` is the taking form, and the rule is about it alone: `photo diff --json`
        // writes no image anywhere, so its document has standard output to itself and a rule
        // demanding `--out` would refuse the one shape of this verb that has nothing to write.
        if self.json
            && let Command::Photo {
                out: None,
                diff: None,
                ..
            } = &self.command
        {
            return Err(program.command().error(
                clap::error::ErrorKind::MissingRequiredArgument,
                "photo --json needs --out <PATH>: with no path the photo's bytes are \
                 standard output, and the JSON document cannot share it",
            ));
        }
        if program.builds_a_backend()
            && self.backend.0 == BackendKind::Fake
            && self.profile.is_empty()
        {
            return Err(program.command().error(
                clap::error::ErrorKind::MissingRequiredArgument,
                "--backend fake needs --profile <PATH>: a backend with nothing to replay \
                 enumerates nothing, and \"no cameras\" is exactly what a user whose cameras \
                 had vanished would see",
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

/// A camera, in any spelling the caller holds (D1, D14).
#[derive(Debug, Clone, Args)]
pub struct CameraArg {
    /// Which camera, in any of the spellings this build understands.
    ///
    /// **Both help forms are set explicitly, and that is what keeps this comment out of the
    /// terminal.** clap derives an argument's short help from a doc comment's first line and its
    /// long help from the rest, so a comment written for a Rust reader — rustdoc links included,
    /// which notes **N123** and **N249** keep off a terminal — is what `--help` would print. The
    /// vocabulary has one home, so both forms come from [`camera_arg_help`] and this paragraph
    /// stays where it is addressed.
    #[arg(
        value_name = "CAMERA",
        help = camera_arg_help(),
        long_help = camera_arg_help()
    )]
    pub camera: String,
}

/// The `<CAMERA>` help, rendered from the scheme vocabulary rather than transcribed from it.
///
/// **A doc comment cannot do this and that is the whole reason this function exists.** clap
/// takes an argument's help from its doc comment, which is a literal — so the five spellings
/// were written out here, a second time in `schema::selector`'s `Deserialize`, a third time in
/// `xtask`'s placeholder glossary and a fourth in the daemon's prose, and a sixth scheme joined
/// `SelectorScheme::ALL` and none of them (note **N303**). Built from `ALL` here, it joins by
/// existing.
///
/// The spellings are backticked one by one rather than taken from [`schema::selector::vocabulary`]
/// whole, because this string is *two* readers' — clap prints it to a terminal and
/// `docs/agent-guide.md` prints it as a Markdown table cell, where a bare `<id>` is an HTML tag.
/// The vocabulary is still `ALL`'s; only the punctuation is this function's, which is the same
/// split `xtask`'s selector table already runs on.
#[must_use]
pub fn camera_arg_help() -> String {
    let spellings = SelectorScheme::ALL
        .iter()
        .map(|scheme| format!("`{}`", scheme.example()))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "Which camera: {spellings}. An id may be written bare, or as any unambiguous prefix of one."
    )
}

impl CameraArg {
    /// The selector to resolve.
    ///
    /// # Errors
    ///
    /// [`Error::CameraUnknown`] for an argument no camera could ever match — the empty string,
    /// a scheme this build does not know, a scheme with an empty body, a malformed `usb:` pair
    /// — with the refusal naming the whole vocabulary. Anything else is resolved against the
    /// live enumeration, which is where a name nothing answers to becomes an error that can
    /// list what *does* exist.
    pub fn selector(&self) -> Result<CameraSelector> {
        schema::selector::parse(&self.camera)
    }
}

/// `calibrate list`'s optional camera, taught the same vocabulary as every other verb's.
///
/// One sentence of its own — the argument is optional and every other one is required — and then
/// the shared vocabulary verbatim, so this verb cannot be the one that ends up a spelling short.
#[must_use]
fn calibrate_list_camera_help() -> String {
    format!(
        "One camera's sessions; every camera's when omitted. {}",
        camera_arg_help()
    )
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

    /// Take a photo, or compare two that were already taken.
    ///
    /// `photo <CAMERA>` opens a camera; `photo diff <A> <B>` opens neither, and reads two
    /// files instead.
    // `subcommand_negates_reqs` is what lets one verb be both: `<CAMERA>` stays required for
    // the form that takes a camera and is not demanded of the form that names two files. The
    // camera is `Option<CameraArg>` for the same reason and only that reason — clap still
    // refuses `photo` with no camera at all, in its own words and with its own exit code,
    // because the flatten leaves the inner argument required (measured: the `Option` changes
    // what is *extracted*, not what is *required*).
    //
    // `args_conflicts_with_subcommands` is deliberately **not** set, and the measurement is
    // why. With it, clap stops recognising the subcommand once any argument has been parsed —
    // including a global one — so `photo --json diff a b`, which is a caller doing something
    // entirely reasonable, answered *"error: the subcommand '/path/a.png' cannot be used with
    // --json"* and exit 2 (measured, 2026-08-18). Without it that invocation works, and the
    // price is that a taking flag typed beside the comparison is ignored rather than refused:
    // `photo -o shot.jpg diff a b` compares the two files and writes no photo. That price is
    // paid where it is smallest — `photo diff --help` offers `<A>`, `<B>` and the global flags
    // and nothing else, so a caller reading the manual for the form they are invoking is never
    // shown `--out` at all.
    #[command(subcommand_negates_reqs = true)]
    Photo {
        /// Which camera.
        #[command(flatten)]
        camera: Option<CameraArg>,

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

        /// Wait for room in the camera's command queue rather than being refused (D12).
        ///
        /// **What it waits for is the queue, and not the camera.** A camera that is busy
        /// because something is *streaming* it — a recording, another program — is refused
        /// with `busy` whether or not this flag is given, and this flag does not wait for
        /// that stream to end. What it waits for is a turn among the commands queued for
        /// this camera's one thread, bounded by
        /// `schema::limits::CAMERA_ENQUEUE_WAIT_MS`, after which the answer is the same
        /// refusal it would have been at once.
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

        // The document half of this verb (D17). No doc comment, because clap prints an
        // `about` for the subcommand itself and this field's own comment would reach nobody;
        // the sentence a reader meets is `PhotoCommand::Diff`'s.
        #[command(subcommand)]
        diff: Option<PhotoCommand>,
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

        /// How long to record, as a duration such as `10s`, `1500ms` or `1m30s`, and at least
        /// `1ms`.
        ///
        /// `schema::limits::DEFAULT_RECORDING_MS` when omitted, and refused rather than
        /// clamped past `schema::limits::MAX_RECORDING_MS` — an agent that asked for five
        /// minutes and silently received two cannot tell that from a camera that stopped.
        ///
        /// The floor is the same argument at the other end and it is refused here rather than
        /// answered: `500us` is a number the caller never wrote, so it is a usage error naming
        /// the text that was typed, and a take shorter than one frame used to answer *success*
        /// over a container header with nothing in it (note **N213**). One millisecond is not
        /// a policy — it is the smallest number the wire field can hold — but it is a bound a
        /// reader of this line meets, so this line says it.
        #[arg(long, value_name = "DURATION", value_parser = duration)]
        duration: Option<u64>,

        /// What to ask the device's format negotiation for.
        #[command(flatten)]
        stream: StreamArgs,

        /// Wait for room in the camera's command queue rather than being refused (D12).
        ///
        /// The same flag `photo --wait` is, waiting for the same thing: a turn among the
        /// commands queued for this camera's thread, never for another take to finish. A
        /// camera that is already recording answers `busy` to this verb with or without it.
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

    /// Capture device profiles, and compare two of them (D15).
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
///
/// **And the small end, which that argument always covered and this function did not** (docs/11
/// **L30**, note **N213**). `humantime` reads `--duration 500us` happily and `as_millis`
/// truncates it to `0`, which reached the recorder as "run no turns": measured through the
/// shipped binary, `--duration 500us` wrote a 224-byte AVI with `frames_written: 0` and exited
/// `0`. Truncation is this conversion's own doing, so it is refused here, where the message can
/// name the flag and the text that was typed — exactly as an unreadable duration is. A duration
/// that is *written* as zero (`--duration 0s`, or `{"duration_ms": 0}` off a socket) parses
/// fine and is refused one layer down by `RecordRequest::budget_ms`, which is where the bounds
/// on a recording live; this arm is only about a number the caller did not write.
fn duration(text: &str) -> std::result::Result<u64, String> {
    let parsed = humantime::parse_duration(text).map_err(|error| {
        format!(
            "{text:?} is not a duration ({error}); write one as 10s, 1500ms or 1m30s, up to \
             the {}s this build records",
            schema::limits::MAX_RECORDING_MS / 1_000
        )
    })?;
    let millis = u64::try_from(parsed.as_millis())
        .map_err(|_| format!("{text:?} is longer than this build can express in milliseconds"))?;
    if millis == 0 && !parsed.is_zero() {
        return Err(format!(
            "{text:?} is under a millisecond, and a recording is measured in whole \
             milliseconds; write 1ms or more"
        ));
    }
    Ok(millis)
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

    /// How long the whole settle may take, in milliseconds. At most 10000, because one
    /// camera is one thread and a longer settle is time nobody else gets; a bigger number is
    /// refused rather than quietly shortened.
    #[arg(long, value_name = "MS")]
    pub settle_deadline: Option<u64>,
}

// The number the help above states, checked against the constant it states. A flag that
// advertised a bound the tool does not have would send an unattended caller into a retry
// loop against a refusal that never moves (note **N147**).
const _: () = assert!(schema::limits::MAX_SETTLE_DEADLINE_MS == 10_000);

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
        /// One camera's sessions. Every camera's when omitted.
        ///
        /// The one camera positional that is not a [`CameraArg`] — it is optional, and clap
        /// flattens a struct rather than an `Option` of one — so its help comes from the same
        /// [`camera_arg_help`] every other verb's does. A second sentence here would be a second
        /// grammar for one verb, which is the defect D14 exists to end.
        #[arg(
            value_name = "CAMERA",
            help = calibrate_list_camera_help(),
            long_help = calibrate_list_camera_help()
        )]
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
        // `diff: None` and not `..`: `photo diff` describes no photo request at all, and a
        // builder that answered `Some` for it would hand the executor a capture nobody asked
        // for the day a caller reached this method with the other form of the verb.
        let Command::Photo {
            out,
            format,
            transform,
            stream,
            settle,
            wait,
            diff: None,
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

/// `webcam-handler-cli photo diff …`
///
/// One variant, and a subtree rather than a flag for the reason design D17 gives the verb its
/// name: `photo` is where a caller already goes for a photograph, and a comparison of two of
/// them is a second thing that verb does rather than a modifier on the first. A `--diff A B`
/// on the taking form would have made `--out`, `--size` and `--settle-for` legal beside it and
/// meaningless.
#[derive(Debug, Subcommand)]
pub enum PhotoCommand {
    /// Compare two photographs: what each one measures, and what moved between them.
    ///
    /// Reads both files and answers one comparison: every metric this build computes, on each
    /// side and as a delta, plus a structural-similarity score.
    ///
    /// Nothing is resized and nothing is decided. Two images of different sizes still measure,
    /// and the similarity score is the one quantity that needs matching shapes — it comes back
    /// saying so, with both sizes on it, rather than refusing the whole comparison.
    ///
    /// The files are photographs this tool wrote: JPEG, PNG, or binary Netpbm. Which one is
    /// read from the bytes rather than from the name, so a renamed file still reads.
    ///
    /// Touches no camera and needs no daemon: both programs read the two files in their own
    /// process.
    Diff {
        /// A photograph, as `photo --out` wrote it.
        #[arg(value_name = "A")]
        a: Utf8PathBuf,

        /// The photograph to compare it against.
        #[arg(value_name = "B")]
        b: Utf8PathBuf,
    },
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

        /// Also probe which controls this device pairs, and record what it demonstrated.
        ///
        /// **This writes to the camera**, which a capture otherwise never does. It
        /// snapshots first and restores after, and the restore's outcome is reported on
        /// standard error — but the document it produces is a document of a device that was
        /// perturbed to make it, and the flag is what puts that in the caller's hands.
        ///
        /// Without it the profile's measured pairs are empty, which means "this capture
        /// measured none" and never "this device has none".
        #[arg(long)]
        discover_pairs: bool,
    },

    /// Compare two captured profiles: the same device, and what moved.
    ///
    /// Reads both files and answers with two halves of one comparison. The device half
    /// names the sections that describe the camera differently: its format tree, the
    /// control slugs whose descriptor differs or is present on one side only, and the
    /// automation pairs a probe measured. The identity half names the fields that say
    /// where the camera is, in the spelling the answer uses: its id, its bus path, its
    /// serial, its device nodes.
    ///
    /// The same camera reached over a forwarded bus, on another port, after a reboot or on
    /// another machine differs in identity and must not differ in the device half. A
    /// format tree that differs on its own is the one device difference a camera is
    /// allowed to produce when it is plugged in again; decide what that means for your rig.
    ///
    /// Touches no camera and needs no daemon: both programs read the two files in their own
    /// process.
    Compare {
        /// A device profile written by `profile capture`.
        #[arg(value_name = "A")]
        a: Utf8PathBuf,

        /// The profile to compare it against.
        #[arg(value_name = "B")]
        b: Utf8PathBuf,
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
    /// [`Error::CameraUnknown`] or [`Error::CameraAmbiguous`] for a selector that does not
    /// resolve; otherwise whatever the backend says.
    fn info(&mut self, camera: &CameraSelector) -> Result<CameraDetail>;

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
    fn controls(&mut self, camera: &CameraSelector, discover_pairs: bool) -> Result<ControlReport>;

    /// One control's descriptor and current value.
    ///
    /// The whole descriptor rather than the bare value: a value with no range, no flags
    /// and no menu is not renderable, and an agent reading `--json` needs the same
    /// context a human reading the table does.
    ///
    /// # Errors
    ///
    /// As [`Executor::info`], plus [`Error::ControlUnknown`] naming the closest slugs.
    fn get(&mut self, camera: &CameraSelector, control: &ControlSlug) -> Result<ControlDesc>;

    /// Write controls, switching automation off first unless `guarded` is false (D3).
    ///
    /// # Errors
    ///
    /// As [`Executor::info`], plus the planner's refusals and the device's.
    fn set(
        &mut self,
        camera: &CameraSelector,
        writes: &[ControlWrite],
        guarded: bool,
    ) -> Result<WriteReport>;

    /// Every writable control's current value (D4).
    ///
    /// # Errors
    ///
    /// As [`Executor::info`].
    fn snapshot(&mut self, camera: &CameraSelector) -> Result<Snapshot>;

    /// Put a snapshot back (D4).
    ///
    /// # Errors
    ///
    /// As [`Executor::info`], plus [`Error::FingerprintMismatch`] when the snapshot came
    /// from a different camera. A control that could not be put back is in the *report*,
    /// not an error.
    fn restore(&mut self, camera: &CameraSelector, snapshot: &Snapshot) -> Result<RestoreReport>;

    /// Take one photo (D5, D6).
    ///
    /// The bytes ride beside the report rather than in it, for a `ReturnBytes` sink: a
    /// `Vec<u8>` inside a `--json` document needs an encoding only the wire surface needs
    /// (D10, P4), and here the caller can simply be handed them.
    ///
    /// # Errors
    ///
    /// As [`Executor::info`], plus [`Error::SettleTimeout`] and whatever the sink says.
    fn photo(&mut self, camera: &CameraSelector, request: &PhotoRequest) -> Result<Photograph>;

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
    fn record(&mut self, camera: &CameraSelector, request: &RecordRequest) -> Result<RecordReport>;

    /// One camera's full device profile (T3).
    ///
    /// `discover_pairs` runs D3's empirical probe first, which **writes to the camera** and
    /// restores it afterwards, and folds what it measured into the document's
    /// `invariant.measured_pairs`. It is a parameter rather than a second method for
    /// [`Executor::controls`]' reason — the answer has the same shape either way, and the
    /// difference is data in it — with one thing extra riding on this one: the document is a
    /// corpus entry, so whether the device was perturbed to produce it has to be the
    /// caller's decision and not a default (note **N239**).
    ///
    /// # Errors
    ///
    /// As [`Executor::info`], plus the probe's own refusals when it was asked for — chiefly
    /// a snapshot that could not be taken, which stops the probe before it writes anything.
    fn capture_profile(
        &mut self,
        camera: &CameraSelector,
        capturer: &str,
        discover_pairs: bool,
    ) -> Result<DeviceProfile>;

    /// Open a calibration session for a camera and a task (D8).
    ///
    /// # Errors
    ///
    /// As [`Executor::info`], plus [`Error::SessionConflict`] when the slot already holds
    /// an open session (N14), and whatever the store refuses with —
    /// [`Error::StoreLocked`] when a daemon owns the state directory (D9).
    fn calibrate_start(
        &mut self,
        camera: &CameraSelector,
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
        camera: &CameraSelector,
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
        camera: &CameraSelector,
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
    fn calibrate_status(
        &mut self,
        camera: &CameraSelector,
        which: &SessionRef,
    ) -> Result<SessionStatus>;

    /// Record a control's chosen value and who chose it (D8).
    ///
    /// # Errors
    ///
    /// As [`Executor::calibrate_start`], plus the D8 machine's refusals: a control that
    /// never swept, a value no sample holds, a metric that cannot rank.
    fn calibrate_select(
        &mut self,
        camera: &CameraSelector,
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
        camera: &CameraSelector,
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
    fn calibrate_restore(
        &mut self,
        camera: &CameraSelector,
        which: &SessionRef,
    ) -> Result<RestoreReport>;

    /// Every session on this machine, or one camera's, newest first.
    ///
    /// # Errors
    ///
    /// As [`Executor::info`] when a camera is named; otherwise whatever the store says.
    /// A session whose document this build cannot read still *lists* — listing parses
    /// nothing (D9).
    fn calibrate_list(&mut self, camera: Option<&CameraSelector>) -> Result<SessionList>;
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
    // The document verbs first, and they never reach `executor` at all — design §2.7's clause,
    // argued on [`below_the_executor`]. Both roots call that function before they build the
    // thing they would hand this one, so the early return here is the *third* call site rather
    // than the rule: a root that skipped it would open a socket it has no use for, and this
    // arm would still answer.
    if let Some(answered) = below_the_executor(cli, out) {
        return answered;
    }
    match &cli.command {
        Command::List => {
            let list = executor.list()?;
            render::list(&list, cli.json, out)
        }
        Command::Info(arg) => {
            let detail = executor.info(&arg.selector()?)?;
            render::info(&detail, cli.json, out)
        }
        Command::Controls {
            camera,
            discover_pairs,
        } => {
            let report = executor.controls(&camera.selector()?, *discover_pairs)?;
            render::controls(&report, cli.json, out)
        }
        Command::Get { camera, control } => {
            let slug = ControlSlug::parse(control).ok_or_else(|| Error::ControlUnknown {
                requested: control.clone(),
                did_you_mean: Vec::new(),
            })?;
            let desc = executor.get(&camera.selector()?, &slug)?;
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
            let report = executor.set(&camera.selector()?, &writes, !*no_guard)?;
            render::writes(&report, cli.json, out)
        }
        Command::Snapshot {
            camera,
            out: destination,
        } => {
            let snapshot = executor.snapshot(&camera.selector()?)?;
            render::snapshot(&snapshot, destination.as_deref(), out)
        }
        Command::Restore { camera, snapshot } => {
            let document = read_snapshot(snapshot)?;
            let report = executor.restore(&camera.selector()?, &document)?;
            render::restore(&report, cli.json, out)
        }
        Command::Photo {
            camera: Some(camera),
            diff: None,
            ..
        } => {
            let cwd = current_directory()?;
            let request = cli
                .command
                .photo_request(&cwd)?
                .ok_or_else(unreachable_photo)?;
            let taken = executor.photo(&camera.selector()?, &request)?;
            render::photo(&taken.report, taken.returned.as_deref(), cli.json, out)
        }
        // `photo diff` is answered above, before the executor existed; `photo` with no camera
        // at all is clap's refusal and never reaches here. Both arms exist so this dispatch
        // carries no `unwrap` on a request-driven path — [`unreachable_photo`]'s reason.
        Command::Photo { diff: Some(_), .. } => Err(unreachable_document()),
        Command::Photo { camera: None, .. } => Err(unreachable_photo()),
        Command::Record { camera, .. } => {
            let cwd = current_directory()?;
            let request = cli
                .command
                .record_request(&cwd)?
                .ok_or_else(unreachable_record)?;
            let report = executor.record(&camera.selector()?, &request)?;
            render::record(&report, cli.json, out)
        }
        Command::Calibrate(command) => calibrate(command, cli.json, executor, out),
        Command::Profile(ProfileCommand::Capture {
            camera,
            out: destination,
            capturer,
            discover_pairs,
        }) => {
            let profile =
                executor.capture_profile(&camera.selector()?, capturer, *discover_pairs)?;
            render::profile(&profile, destination.as_deref(), out)
        }
        // Answered above, before the executor existed. The arm keeps this match exhaustive
        // without an `unwrap` on a request-driven path, exactly as [`unreachable_photo`] does
        // for the arm the request builder has already ruled out.
        Command::Profile(ProfileCommand::Compare { .. }) => Err(unreachable_document()),
    }
}

/// Run the verb here and now if it is a **document verb**, and answer `None` if it is not
/// (design §2.7's T4 clause; D15).
///
/// A document verb takes files, answers a document, and touches no camera, no store and no
/// socket — so it executes inside this crate, on both roots, and never reaches the
/// [`Executor`] seam. There are two. `profile compare` reads two profiles somebody already
/// captured and compares the devices they describe (D15); `photo diff` reads two photographs
/// somebody already took and measures them against each other (D17).
///
/// **The boundary is sharp, and it is about what a verb needs rather than what it answers.** A
/// verb that needs a backend, a store or a daemon is an executor verb whatever document it
/// comes back with — `profile capture` answers a profile off a live camera and is an executor
/// verb for exactly that reason, and so would a comparison that captured either side itself.
/// The fall-through below is written to that boundary: a verb added to [`Command`] and not
/// named here is an executor verb, which is the direction that fails safe. The opposite
/// default would let a verb reach a caller with the camera silently unopened.
///
/// **Public because it is what lets `webcam-handler-client` decline to open a socket it has no
/// use for.** That root calls this before it connects; `webcam-handler-cli` calls it before it
/// builds a backend. A client that connected first would make `profile compare` need a running
/// daemon on one root and not on the other — one verb with two preconditions, which is the
/// fork T4 exists to prevent and the thing `scripts/gates/cli-parity.sh`'s `document` bucket
/// measures by driving this verb with no daemon to reach.
///
/// # Errors
///
/// Inside the `Some`: whatever the verb says. A path that is not a readable device profile is
/// [`Error::StorageIo`] naming it, and a profile from another schema version is
/// [`Error::SchemaVersionForeign`] naming both — see `read_profile`. A path that is not a
/// photograph this build writes is [`Error::DeviceIo`] saying what was found — see
/// `compare_photographs`.
pub fn below_the_executor(cli: &Cli, out: &mut Output) -> Option<Result<()>> {
    match &cli.command {
        Command::Profile(ProfileCommand::Compare { a, b }) => {
            Some(compare_profiles(a, b, cli.json, out))
        }
        Command::Photo {
            diff: Some(PhotoCommand::Diff { a, b }),
            ..
        } => Some(compare_photographs(a, b, cli.json, out)),
        _ => None,
    }
}

/// `profile compare` (design D15): two documents in, one document out.
///
/// The comparison itself is [`DeviceProfile::compare`] and lives in the schema crate, which is
/// where the identity/device partition is closed by destructuring — this function is the two
/// reads and the rendering around it, and deliberately decides nothing about which fields are
/// which.
///
/// `a` is read before `b` so a run naming two unreadable paths refuses the first one a reader
/// would go and look at.
///
/// # Errors
///
/// As [`read_profile`], for either path.
fn compare_profiles(a: &Utf8Path, b: &Utf8Path, as_json: bool, out: &mut Output) -> Result<()> {
    let mine = read_profile(a)?;
    let theirs = read_profile(b)?;
    render::comparison(&mine.compare(&theirs), as_json, out)
}

/// `photo diff` (design D17): two photographs in, one comparison out.
///
/// [`compare_profiles`]'s shape one document along, and the split is the same: the comparison
/// is `imaging::compare::photos`, which is **total** — it answers for every pair of images,
/// and the one quantity that needs matching shapes says so on itself rather than refusing. So
/// this function decides nothing about what "differs" means, and D17 adds no error kind. The
/// only failures here are about the two *files*.
///
/// `a` is read **and decoded** before `b` is opened, which is [`compare_profiles`]'s ordering
/// and its reason: a run naming two files that are not photographs refuses the first one a
/// reader would go and look at.
///
/// # Errors
///
/// [`Error::StorageIo`] naming the path, for a file that cannot be read or that is past
/// [`schema::limits::MAX_PHOTO_DECODE_BYTES`]. [`Error::DeviceIo`]
/// from `imaging::compare::read` for bytes in no format this build writes and for bytes their
/// own decoder refused, carried verbatim from the crate that owns the format vocabulary: the
/// message names every format that *would* have read and the first bytes that were found
/// instead. It does **not** name the path, which is the one thing this refusal leaves to the
/// caller's own command line — the ordering above is what makes "which of the two" answerable
/// at all, and a message rebuilt here to add the path would be a second opinion about what
/// `imaging` refuses.
fn compare_photographs(a: &Utf8Path, b: &Utf8Path, as_json: bool, out: &mut Output) -> Result<()> {
    let mine = imaging::compare::read(&read_photograph(a)?)?;
    let theirs = imaging::compare::read(&read_photograph(b)?)?;
    render::photo_comparison(&imaging::compare::photos(&mine, &theirs), as_json, out)
}

/// The bytes of a file named on a command line, or a refusal naming it.
///
/// Its own function rather than an inline `std::fs::read`, because *this* is the refusal that
/// carries the path and it has two callers a line apart — the same argument
/// [`read_profile`]'s first four lines make, and the reason a caller meets one spelling of
/// "that file is not there" whichever side of the comparison it was on.
///
/// Bounded at [`schema::limits::MAX_PHOTO_DECODE_BYTES`], which is the same number the
/// decoder's own door reads and the reason that door was not the first one: every format this
/// build writes spends at most one file byte per raster byte (note **N322**).
fn read_photograph(path: &Utf8Path) -> Result<Vec<u8>> {
    read_named_file(path, schema::limits::MAX_PHOTO_DECODE_BYTES, "photograph")
}

/// The bytes of a file a caller named, read through the one bounded door.
///
/// **A thin wrapper on [`schema::file::read_under_budget`], and the wrapper is the point.** The
/// law — what a caller-named path costs an allocator, and how a file past the budget is refused
/// — has one home, in the crate the engine can reach as well as this one; what lives here is the
/// choice of *which* budget each verb reads and what it calls its subject in the refusal. Two
/// implementations of the reading would be the second home design §2.10 forbids, and the door
/// this crate cannot see is `engine::profile::read`, which is the daemon's `--profile` (note
/// **N329**).
fn read_named_file(path: &Utf8Path, budget: u64, subject: &str) -> Result<Vec<u8>> {
    schema::file::read_under_budget(path, budget, subject)
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
            let session = executor.calibrate_start(&camera.selector()?, task, goal, criteria)?;
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
                executor.calibrate_plan(&camera.selector()?, &which.which()?, &slugs, *order)?;
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
            let session = executor.calibrate_sweep(
                &camera.selector()?,
                &which.which()?,
                &request,
                &*watcher,
            )?;
            watcher.finish();
            render::session(&session, as_json, out)?;
            // The camera is still borrowed, and saying so is the difference between a
            // restore an operator forgot and one they declined. Design D4 restores by
            // default; this tool's restore is a verb rather than a default because the
            // snapshot is session-scoped (N23), so the default is replaced by a reminder
            // that names the exact command. Standard error, so the `--json` answer on
            // standard output is still one document.
            if session.pre_snapshot.is_some() {
                out.note(&format!(
                    "note: this sweep borrowed the camera and it still holds what the \
                         sweep left; `calibrate restore {} --session {}` puts it back",
                    camera.camera, session.id
                ));
            }
            Ok(())
        }
        CalibrateCommand::Status { camera, which } => {
            let status = executor.calibrate_status(&camera.selector()?, &which.which()?)?;
            render::status(&status, as_json, out)
        }
        CalibrateCommand::Select {
            camera,
            which,
            control,
            by,
        } => {
            let session = executor.calibrate_select(
                &camera.selector()?,
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
            let report =
                executor.calibrate_apply(&camera.selector()?, &which.which()?, *partial)?;
            render::writes(&report, as_json, out)
        }
        CalibrateCommand::Restore { camera, which } => {
            let report = executor.calibrate_restore(&camera.selector()?, &which.which()?)?;
            // An empty report is not "restored nothing", it is "there was nothing to put
            // back" — the ordinary answer to running this twice, and the two would be
            // indistinguishable from a table with no rows in it.
            if report.outcomes.is_empty() {
                out.note(
                    "note: this session carries no unconsumed pre-sweep snapshot; the camera \
                     was not written to",
                );
            }
            render::restore(&report, as_json, out)
        }
        CalibrateCommand::List { camera } => {
            // Through the one parser, exactly as `CameraArg::selector` is: this positional is
            // optional rather than flattened, which is the only reason it is not a `CameraArg`,
            // and a second grammar for it would be a spelling that works on every other verb
            // and not on this one (D14).
            let selector = camera.as_deref().map(schema::selector::parse).transpose()?;
            let sessions = executor.calibrate_list(selector.as_ref())?;
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
    let bytes = read_named_file(
        path,
        schema::limits::MAX_SNAPSHOT_FILE_BYTES,
        "control snapshot",
    )?;
    serde_json::from_slice(&bytes).map_err(|error| Error::StorageIo {
        path: path.to_owned(),
        errno: None,
        message: format!("not a snapshot document: {error}"),
    })
}

/// Read a device profile named on a command line, refusing a foreign one by *version* before
/// anything tries to represent it.
///
/// **Here rather than through `engine::profile::read`, and that is the thin-client wall rather
/// than a preference** (T6): `webcam-handler-client` links no engine, and a document verb has
/// to run identically on both roots — reaching for the engine's reader would put this verb in
/// exactly one of the two binaries. What is *not* copied is the law: the version this build
/// speaks is [`schema::limits::PROFILE_SCHEMA_VERSION`] and the refusal is the D13 registry's
/// [`Error::SchemaVersionForeign`], both read from the crate that owns them.
///
/// The version comes off a probe that deserializes only `schema_version`, for the reason the
/// engine's reader and the session store both give: a document this build cannot represent is
/// refused *for its version* rather than for whichever field this build's shape happens to be
/// missing, which is the difference between an agent knowing to re-capture and an agent
/// reading a serde path.
///
/// # Errors
///
/// [`Error::StorageIo`] naming the path when it cannot be read, is larger than
/// [`schema::limits::MAX_PROFILE_FILE_BYTES`], is not JSON, carries no `schema_version`, or
/// does not deserialize into a profile; [`Error::SchemaVersionForeign`] naming both versions
/// for a profile this build does not read.
fn read_profile(path: &Utf8Path) -> Result<DeviceProfile> {
    let bytes = read_named_file(
        path,
        schema::limits::MAX_PROFILE_FILE_BYTES,
        "device profile",
    )?;
    let unreadable = |message: String| Error::StorageIo {
        path: path.to_owned(),
        errno: None,
        message,
    };

    let probe: ProfileVersionProbe = serde_json::from_slice(&bytes)
        .map_err(|error| unreadable(format!("is not a JSON document: {error}")))?;
    match probe.schema_version {
        None => {
            return Err(unreadable(
                "carries no schema_version; every device profile this tool writes has one, \
                 so this file was not written by it"
                    .to_owned(),
            ));
        }
        Some(found) if found != schema::limits::PROFILE_SCHEMA_VERSION => {
            return Err(Error::SchemaVersionForeign {
                found,
                supported: schema::limits::PROFILE_SCHEMA_VERSION,
            });
        }
        Some(_) => {}
    }

    serde_json::from_slice(&bytes)
        .map_err(|error| unreadable(format!("is not a device profile: {error}")))
}

/// Only the field that decides whether the rest of a profile may be read.
#[derive(Debug, serde::Deserialize)]
struct ProfileVersionProbe {
    schema_version: Option<u32>,
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

/// [`unreachable_photo`]'s counterpart for a document verb reaching the executor dispatch,
/// which [`below_the_executor`] has already answered before [`run`]'s match is entered.
fn unreachable_document() -> Error {
    Error::IllegalTransition {
        from: "a_document_verb".to_owned(),
        op: "reach the executor seam".to_owned(),
    }
}

/// Report a failure on the process's two streams, and answer with the code to exit.
///
/// **One home for the whole failure edge** (owner ruling, 2026-08-15; note **N127**). Both
/// composition roots call exactly this — `webcam-handler-cli` with an error its engine
/// produced, `webcam-handler-client` with one `api::codes::typed` rebuilt from the wire — so
/// "the same failure produces the same document from both roots" is true by construction and
/// `scripts/gates/cli-parity.sh` is left proving it end to end rather than hoping. A `format!`
/// in each `main` is the second copy design §2.10 is about, and this is the seam where two
/// roots would otherwise drift the furthest: nobody reads a refusal path twice.
///
/// Two channels, two readers, and neither is asked to parse the other's:
///
/// - **Standard output carries [`schema::error::Failure`] when `as_json`** — the ruling's
///   mechanism. Reading that document alone tells a caller that this is a failure, which
///   failure it is in the registry's own spelling, and the payload it needs to act:
///   `FormatUnsupported`'s `available`, `Busy`'s `holders`, `StorageIo`'s `path`. Note
///   **N124** measured what came before — nothing at all on standard output, `--json` or not
///   — so a caller that redirected stdout lost the failure completely.
/// - **Standard error carries [`Program::error_line`], unchanged**, because a person watching
///   a terminal is the other reader and this ruling takes nothing from them.
///
/// The two are one value rendered twice: `Failure::new` derives its `message` from the same
/// `Display` the line uses, so there is no second rendering to drift (design §2.10).
///
/// Writing the document is best-effort in exactly the way the line already was. A refusal
/// whose *cause* was a closed standard output cannot be printed on it, and turning that into a
/// panic — or into a different exit code — would replace the failure the caller asked about
/// with one about the pipe. Since 2026-08-17 the line needs no `let _` to say so:
/// [`Output::note`] is the only door to standard error and it answers nothing, which is that
/// same argument made once, for every commentary line the surface writes (note **N216**).
#[must_use]
pub fn report_failure(program: Program, error: &Error, as_json: bool, out: &mut Output) -> u8 {
    if as_json {
        let _ = render::failure(&schema::error::Failure::new(error.clone()), out);
    }
    out.note(&program.error_line(error));
    exit_code(error)
}

/// The closed range every D13 exit code comes from.
///
/// Eighteen codes, contiguous, `10 ..= 27` — `api::codes::D13_CODES`'s shape one channel
/// along, and for its reason: "eighteen codes, no holes" is a property a walk can check, and a
/// block with a gap in it is a block nobody can describe.
///
/// **It starts at 10 because the three small codes are the process's own and stay that way.**
/// `0` is an answer, `2` is clap's usage refusal, and `1` is deliberately left **unassigned**:
/// a caller that meets 1 has met something other than a typed D13 refusal — a wrapper, a
/// harness, a shell's own generic failure — and the gap is what lets them tell. Above the
/// block, the numbers a caller does not own: `<sysexits.h>` gives 64–78 standard meanings,
/// a POSIX shell answers 126 for "found and not executable" and 127 for "not found", and
/// 128 + N is a process killed by signal N. `the_declared_range_collides_with_nothing_the_
/// shell_or_the_process_already_owns` asserts every one of those rather than trusting this
/// paragraph, exactly as `codes.rs` asserts D13's wire block against jsonrpsee's reserved
/// constants.
pub const D13_EXIT_CODES: std::ops::RangeInclusive<u8> = 10..=27;

/// The exit code a failure leaves behind — **redundancy, not the mechanism**.
///
/// [`report_failure`]'s document is what a caller branches on: it is self-contained, it
/// carries the payload, and it survives being written down. This is the second, coarser
/// channel beside it, for the caller that has a shell and not a JSON parser. **Nobody should
/// ever "simplify" the document away on the ground that the code carries the same
/// information — it does not.** A number cannot name the formats a camera does offer, and an
/// exit status is one byte a pipeline is free to lose.
///
/// ## What this used to say, and what replaced it (notes **N124**, **N127**)
///
/// It returned `1` for all eighteen kinds and argued for that: shell exit codes are a
/// one-bit channel, and "a caller who wants to branch on *which* thing went wrong reads
/// `--json`, where the whole typed error is". The second half was false — N124 measured that
/// `--json` carried no failure document at all — and the owner's ruling of 2026-08-15 settled
/// both halves at once: *"Let's extend the JSON output to convey errors. Exit codes are
/// numerical, so they're not a self-contained way to communicate errors. Distinct exit codes
/// are nice-to-have, as a redundant mechanism."* So the document exists and these codes are
/// distinct, and the old argument is gone rather than kept beside its replacement.
///
/// The process now has three shapes of outcome:
///
/// | Code | Meaning |
/// |---|---|
/// | 0 | the verb answered |
/// | 2 | clap's own: the command line was not a command line |
/// | [`D13_EXIT_CODES`] | a typed [`Error`] — which one is the code, and the whole of it is the `--json` document |
///
/// 2 stays clap's and 1 stays unclaimed; [`D13_EXIT_CODES`]'s doc argues both.
///
/// ## Every arm is a literal, and the match is over the kind
///
/// Both decisions are `api::codes::rpc_code`'s, taken again here because the same two traps
/// are here. The match is over [`ErrorKind`] rather than over [`Error`], which is
/// `#[non_exhaustive]` and would therefore need a wildcard arm in this crate — and a wildcard
/// is exactly what would let a nineteenth variant reach a caller wearing somebody else's
/// code. And every arm is an explicit number rather than `BASE + kind as u8`, because an
/// ordinal derivation renumbers every code below an inserted variant while every test here
/// stays green: the walk still finds eighteen distinct codes in range, and a script that
/// retried on `Busy` starts retrying on something else.
///
/// **The committed pin is `docs/agent-guide.md`.** The generated failure table prints this
/// code per kind, so changing one moves a committed file and
/// `scripts/gates/agent-guide-current.sh` goes red until somebody regenerates it — which is
/// the same arrangement `crates/api/fixtures/d13-rpc-codes.tsv` gives the wire codes, reusing
/// a file that has to exist anyway.
#[must_use]
pub fn exit_code(error: &Error) -> u8 {
    match error.kind() {
        ErrorKind::DeviceGone => 10,
        ErrorKind::Busy => 11,
        ErrorKind::PermissionDenied => 12,
        ErrorKind::CameraUnknown => 13,
        ErrorKind::CameraAmbiguous => 14,
        ErrorKind::ControlUnknown => 15,
        ErrorKind::ControlReadOnly => 16,
        ErrorKind::ControlInactive => 17,
        ErrorKind::FormatUnsupported => 18,
        ErrorKind::SettleTimeout => 19,
        ErrorKind::FingerprintMismatch => 20,
        ErrorKind::SessionConflict => 21,
        ErrorKind::IllegalTransition => 22,
        ErrorKind::SchemaVersionForeign => 23,
        ErrorKind::StoreLocked => 24,
        ErrorKind::HolderGone => 25,
        ErrorKind::DeviceIo => 26,
        // The highest code in the block, which is `D13_EXIT_CODES.end()`.
        ErrorKind::StorageIo => 27,
    }
}

#[cfg(test)]
mod tests {
    use schema::control::ControlValue;
    use schema::selector::SelectorScheme;

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
        let error = Error::busy("/dev/video0".into(), Vec::new());
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
        assert_eq!(
            arg.selector().expect("a selector").to_string(),
            "cam:obsbot"
        );

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
        assert_eq!(
            camera.selector().expect("a selector").to_string(),
            "cam:obsbot"
        );
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
        let Command::Profile(ProfileCommand::Capture {
            out,
            capturer,
            discover_pairs,
            ..
        }) = &cli.command
        else {
            panic!("expected profile capture");
        };
        assert_eq!(out.as_deref(), Some(camino::Utf8Path::new("p.json")));
        assert_eq!(capturer, "unattributed");
        // The same opt-in the `controls` probe has, and here it decides something more than
        // whether a camera is written to: a profile is a corpus entry, and a capture that
        // probed by default would make every committed document one taken from a perturbed
        // device without the verb ever having said so (note **N239**).
        assert!(
            !discover_pairs,
            "a capture reads; the probe that writes is asked for"
        );
    }

    #[test]
    fn the_fake_backend_cannot_be_selected_without_something_to_replay() {
        // The inverse of the test below: a fake backend with no documents enumerates
        // nothing, which reads exactly like a machine whose cameras disappeared.
        //
        // Through [`Cli::parse_checked`]'s own entry point rather than clap's derived
        // `try_parse_from`, because that is the parse **both binaries perform** — the rule
        // moved off the field and into `Cli::check` at 2026-08-17 so it could be the *root's*
        // rather than the shared tree's (note **N214**), and a test driving a parse neither
        // root uses would keep passing while the surface stopped refusing.
        let error = Cli::try_parse_checked_from(
            Program::Cli,
            ["webcam-handler-cli", "--backend", "fake", "list"],
        )
        .expect_err("--backend fake without --profile must not parse");
        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::MissingRequiredArgument,
            "{error}"
        );
        assert!(error.to_string().contains("--profile"), "{error}");

        // …and the default backend needs no profile at all.
        assert!(Cli::try_parse_checked_from(Program::Cli, ["webcam-handler-cli", "list"]).is_ok());
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
    fn a_fake_backend_needs_its_profiles_on_the_root_that_builds_one_and_nowhere_else() {
        // Both directions of the rule that moved off the shared tree (docs/11 **M20**, note
        // **N214**). On `webcam-handler-cli` the profiles are what a fake backend *is*, so
        // their absence is a usage error while the command line is being read — the same
        // answer clap gave when this was `required_if_eq`, and asserted here so moving the
        // rule did not quietly drop it.
        let refused = Cli::try_parse_checked_from(
            Program::Cli,
            ["webcam-handler-cli", "--backend", "fake", "list"],
        )
        .expect_err("a fake backend with nothing to replay enumerates nothing");
        assert_eq!(
            refused.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
        assert!(
            refused.to_string().contains("--profile"),
            "the refusal has to name the flag that repairs it: {refused}"
        );

        // And on `webcam-handler-client` it parses, because this root builds no backend: the
        // refusal that belongs to `--backend fake` there is `client::refuse_composition_flags`'
        // typed one, which names the daemon. clap answering first turned that into "add
        // `--profile`", which is note **N123**'s class — an instruction that cannot be
        // followed, since `--profile` is refused on this root too.
        let parsed = Cli::try_parse_checked_from(
            Program::Client,
            ["webcam-handler-client", "--backend", "fake", "list"],
        )
        .expect("the client's refusal for this line is its own, not clap's");
        assert!(parsed.backend_was_chosen());
        assert!(parsed.profile.is_empty());

        // The rule is about `fake` and not about naming a backend at all.
        Cli::try_parse_checked_from(
            Program::Cli,
            ["webcam-handler-cli", "--backend", "v4l2", "list"],
        )
        .expect("the v4l2 backend replays nothing and needs no profile");
    }

    #[test]
    fn an_empty_camera_argument_is_refused_rather_than_resolved() {
        let arg = CameraArg {
            camera: String::new(),
        };
        assert!(matches!(arg.selector(), Err(Error::CameraUnknown { .. })));
    }

    #[test]
    fn every_camera_taking_verb_teaches_the_whole_vocabulary_in_its_help() {
        // The other half of the door below: the parser takes every spelling `SelectorScheme`
        // declares, and a reader who has never seen this tool learns which ones from `--help`
        // and from the guide generated out of the same string. That sentence was itself a
        // transcription until note **N303** — five spellings written out in a doc comment a
        // hundred lines from `SelectorScheme::ALL`, with nothing able to notice a sixth — and
        // the count went on being one here until note **N319** took it out.
        //
        // Walked over the *rendered* help of every argument named `CAMERA` on every verb, in
        // both roots, because what is being asserted is what a reader is shown rather than what
        // `camera_arg_help` returns — a `#[arg(help = …)]` that stopped being applied would
        // leave the function correct and the terminal wrong.
        let mut seen = 0;
        for &program in Program::ALL {
            let command = program.command();
            for verb in command.get_subcommands() {
                for sub in std::iter::once(verb).chain(verb.get_subcommands()) {
                    for arg in sub.get_arguments() {
                        if arg
                            .get_value_names()
                            .is_none_or(|names| !names.iter().any(|name| name.as_str() == "CAMERA"))
                        {
                            continue;
                        }
                        seen += 1;
                        // **Both forms, because the comment on `CameraArg` claims both.** clap
                        // prints the short one under `-h` and the long one under `--help`, and
                        // the doc comment there says that setting both explicitly is what keeps
                        // a Rust-facing paragraph and its rustdoc links off a terminal (notes
                        // **N123**, **N249**). An arm reading only `get_help()` held half of
                        // that sentence: dropping `long_help` left the function correct, this
                        // arm green, and `--help` printing the comment. It is caught by
                        // `no_text_this_surface_prints_carries_a_rustdoc_link` either way — but
                        // by a different predicate than the one this arm is about, and a check
                        // red for the wrong reason reads as green about the right one
                        // (notes **N240**–**N243**).
                        for form in ["-h", "--help"] {
                            let help = match form {
                                "-h" => arg.get_help().map(ToString::to_string),
                                _ => arg.get_long_help().map(ToString::to_string),
                            }
                            .unwrap_or_else(|| {
                                panic!(
                                    "{} {}'s <CAMERA> has no {form} text",
                                    program,
                                    sub.get_name()
                                )
                            });
                            for scheme in SelectorScheme::ALL {
                                assert!(
                                    help.contains(scheme.example()),
                                    "{} {}'s <CAMERA> {form} text does not teach {scheme:?}: \
                                     {help}",
                                    program,
                                    sub.get_name()
                                );
                            }
                        }
                    }
                }
            }
        }
        // A walk that found no `CAMERA` argument would pass every assertion above without
        // making one, which is the skip that reads as a pass (note **N231**).
        assert!(
            seen >= 2 * SelectorScheme::ALL.len(),
            "only {seen} <CAMERA> argument(s) were found across both roots"
        );
    }

    #[test]
    fn a_camera_positional_reaches_the_executor_in_every_spelling_the_parser_knows() {
        // D14 at *this* door. A `CameraArg` that had kept D1's parser would compile, would
        // pass every test above, and would refuse `photo /dev/video0` on the one surface the
        // primary consumer types into — so the walk is over the vocabulary rather than over a
        // list written here, and a sixth scheme fails it by having no sample.
        let spellings = [
            (SelectorScheme::Id, "cam:obsbot-tiny-3"),
            (SelectorScheme::NodePath, "/dev/video0"),
            (SelectorScheme::BusPath, "bus:3-4:1.2"),
            (SelectorScheme::UsbId, "usb:04f2:b83c"),
            (SelectorScheme::Serial, "serial:0001"),
        ];
        assert_eq!(spellings.len(), SelectorScheme::ALL.len());
        for (scheme, spelling) in spellings {
            let cli = Cli::try_parse_from(["webcam-handler-cli", "info", spelling])
                .unwrap_or_else(|error| panic!("{spelling} did not parse: {error}"));
            let Command::Info(arg) = &cli.command else {
                panic!("expected info");
            };
            let selector = arg
                .selector()
                .unwrap_or_else(|error| panic!("{spelling} is not a selector: {error}"));
            assert_eq!(selector.scheme(), scheme, "{spelling}");
            assert_eq!(selector.to_string(), spelling, "{spelling}");
        }
        // And the refusal, at the same door: a scheme this build does not know is a request no
        // camera can ever match, which is what makes it `CameraUnknown` rather than a new kind.
        let cli =
            Cli::try_parse_from(["webcam-handler-cli", "info", "bus_path:3-4"]).expect("parses");
        let Command::Info(arg) = &cli.command else {
            panic!("expected info");
        };
        assert!(matches!(
            arg.selector(),
            Err(Error::CameraUnknown { requested }) if requested == "bus_path:3-4"
        ));
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
        // It is about the *taking* form of `photo` alone, too (D17). `photo diff` writes no
        // image anywhere, so the stream its document would have shared is empty and demanding
        // a path would refuse the one shape of this verb that has nothing to write. Both
        // spellings of the flag's position, because `--json` is global and a caller reaches
        // for it on either side of the verb.
        for argv in [
            [
                "webcam-handler-cli",
                "--json",
                "photo",
                "diff",
                "a.png",
                "b.png",
            ],
            [
                "webcam-handler-cli",
                "photo",
                "--json",
                "diff",
                "a.png",
                "b.png",
            ],
        ] {
            assert!(
                Cli::try_parse_checked_from(Program::Cli, argv).is_ok(),
                "{argv:?} must parse"
            );
        }
    }

    #[test]
    fn photo_is_a_verb_and_a_subtree_and_neither_form_takes_the_others_arguments() {
        // The shape D17 asks of one verb, and the three things that have to stay true of it.
        //
        // A camera is still required for the taking form, in clap's own words and with clap's
        // own exit code: the camera became `Option<CameraArg>` so that the comparison form
        // could exist beside it, and if that had also made it optional to clap, `photo` with
        // nothing after it would have stopped being a usage error and become a device one.
        let error = Cli::try_parse_checked_from(Program::Cli, ["webcam-handler-cli", "photo"])
            .expect_err("photo with no camera and no subcommand is a usage error");
        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
        assert!(error.to_string().contains("CAMERA"), "{error}");

        // The comparison form does not demand one, and parses into the arm that answers below
        // the executor.
        let cli = Cli::try_parse_checked_from(
            Program::Cli,
            ["webcam-handler-cli", "photo", "diff", "a.png", "b.png"],
        )
        .expect("photo diff parses");
        let Command::Photo {
            camera: None,
            diff: Some(PhotoCommand::Diff { a, b }),
            ..
        } = &cli.command
        else {
            panic!("photo diff parsed as {:?}", cli.command);
        };
        assert_eq!((a.as_str(), b.as_str()), ("a.png", "b.png"));

        // And the comparison form describes no photo request at all. A builder answering
        // `Some` here would hand an executor a capture nobody asked for.
        let cwd = Utf8PathBuf::from("/tmp");
        assert!(
            cli.command
                .photo_request(&cwd)
                .expect("no refusal")
                .is_none(),
            "photo diff described a photo request"
        );
        let taking = Cli::try_parse_checked_from(
            Program::Cli,
            ["webcam-handler-cli", "photo", "cam:x", "-o", "shot.jpg"],
        )
        .expect("photo parses");
        assert!(
            taking
                .command
                .photo_request(&cwd)
                .expect("no refusal")
                .is_some(),
            "the taking form still describes one"
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
    fn every_kind_has_a_distinct_exit_code_inside_the_declared_range() {
        // The redundancy the owner's ruling of 2026-08-15 asks for (note **N127**), walked
        // over `ErrorKind::ALL` — generated by the vocabulary macro, so this cannot shrink
        // when a variant is added, while the exhaustive match in `exit_code` is what stops a
        // variant being added without a code at all.
        let mut seen: std::collections::BTreeMap<u8, ErrorKind> = std::collections::BTreeMap::new();
        for &kind in ErrorKind::ALL {
            let code = exit_code(&Error::sample(kind));
            assert!(
                D13_EXIT_CODES.contains(&code),
                "{kind:?} exits {code}, outside {D13_EXIT_CODES:?}"
            );
            // Distinctness is also the assertion that would notice a wildcard arm: the
            // compiler cannot tell `_ => 27` from eighteen literals, but two kinds sharing a
            // code is exactly what a catch-all produces — and two kinds sharing a code is the
            // collapse AGENTS' opening section names, since `busy` and `device_gone` want
            // opposite responses.
            if let Some(other) = seen.insert(code, kind) {
                panic!("{kind:?} and {other:?} both exit {code}");
            }
        }
        assert_eq!(seen.len(), ErrorKind::ALL.len());

        // The range is exactly as wide as the registry: no holes, nothing reserved that
        // nobody uses. Written in `u16` because the arithmetic would otherwise be a `u8`
        // subtraction one variant away from wrapping.
        let span = u16::from(*D13_EXIT_CODES.end()) - u16::from(*D13_EXIT_CODES.start()) + 1;
        assert_eq!(
            usize::from(span),
            ErrorKind::ALL.len(),
            "{D13_EXIT_CODES:?} has holes in it"
        );
    }

    #[test]
    fn the_declared_range_collides_with_nothing_the_shell_or_the_process_already_owns() {
        // The half `codes.rs` spends on jsonrpsee's reserved constants, one channel along. A
        // D13 code colliding with one of these would make an infrastructure failure
        // indistinguishable from a camera answer — E3's "availability is not capability" lost
        // at the process boundary — so each band is asserted rather than assumed.
        //
        // 0 is an answer and 2 is clap's; 1 is left unassigned on purpose, so a caller that
        // meets it knows it met something other than a typed refusal. 64–78 is
        // `<sysexits.h>`'s standard block, which many tools do use; 126 is a POSIX shell's
        // "found and not executable" and 127 its "not found"; 128 + N is a process killed by
        // signal N, which is every code from 129 up and the reason nothing here goes near the
        // top of the byte.
        for reserved in [0u8, 1, 2] {
            assert!(
                !D13_EXIT_CODES.contains(&reserved),
                "{reserved} is the process's own and D13 claims it too: {D13_EXIT_CODES:?}"
            );
        }
        for sysexit in 64u8..=78 {
            assert!(!D13_EXIT_CODES.contains(&sysexit), "{sysexit} is sysexits'");
        }
        for shell in 126u8..=127 {
            assert!(!D13_EXIT_CODES.contains(&shell), "{shell} is the shell's");
        }
        assert!(
            *D13_EXIT_CODES.end() < 128,
            "{D13_EXIT_CODES:?} reaches the band a signal death reports in"
        );

        // Not vacuous: the block is inhabited, and by every kind.
        assert_eq!(
            ErrorKind::ALL
                .iter()
                .filter(|&&kind| D13_EXIT_CODES.contains(&exit_code(&Error::sample(kind))))
                .count(),
            ErrorKind::ALL.len()
        );
    }

    #[test]
    fn a_failing_verb_answers_with_the_failure_document_on_standard_output_and_the_line_on_error() {
        // The shape of the ruling, at the seam both roots share. Asserted here as well as in
        // the two subprocess suites because this is the function they call: a root that
        // stopped calling it would fail there, and a change to what it emits fails here.
        let error = Error::format_unsupported(
            Some(PixelFormat::NV12),
            vec![PixelFormat::MJPG, PixelFormat::YUYV],
        );
        for &program in Program::ALL {
            let stdout = render::tests::Buffer::default();
            let stderr = render::tests::Buffer::default();
            let mut out = Output::to_buffers(Box::new(stdout.clone()), Box::new(stderr.clone()));

            let code = report_failure(program, &error, true, &mut out);
            assert_eq!(code, exit_code(&error));

            let document: schema::error::Failure =
                serde_json::from_str(&stdout.text()).expect("standard output carries a document");
            assert!(document.failed());
            assert_eq!(document.kind(), schema::ErrorKind::FormatUnsupported);
            assert_eq!(document.error, error);
            // The payload an agent retries on, reachable as a value rather than as prose.
            let Error::FormatUnsupported { available, .. } = &document.error else {
                panic!("the document changed shape");
            };
            assert_eq!(available, &[PixelFormat::MJPG, PixelFormat::YUYV]);

            // The human's half, unchanged and still naming which binary refused.
            assert_eq!(stderr.text(), format!("{}\n", program.error_line(&error)));
            // The message names them too, so the person reading stderr and the program
            // reading stdout learn the same thing. Containment rather than a suffix since
            // note **N129**: that sentence stopped ending in the list when it stopped
            // claiming the list was the camera's, and an assertion pinned to the last
            // words of a message is pinned to its phrasing rather than to its content.
            for named in ["MJPG", "YUYV"] {
                assert!(document.message.contains(named), "{document:?}");
            }
            assert!(stderr.text().contains(&document.message));
        }
    }

    #[test]
    fn without_json_a_failure_still_prints_nothing_on_standard_output() {
        // The other direction, and the one that keeps `--json` a *mode*: a person running the
        // verb without it gets the sentence they always got, and a shell doing
        // `webcam-handler-cli photo cam:x > shot.jpg` does not find a JSON document where the
        // image should have been.
        let error = Error::sample(schema::ErrorKind::Busy);
        let stdout = render::tests::Buffer::default();
        let stderr = render::tests::Buffer::default();
        let mut out = Output::to_buffers(Box::new(stdout.clone()), Box::new(stderr.clone()));

        assert_eq!(
            report_failure(Program::Cli, &error, false, &mut out),
            exit_code(&error)
        );
        assert!(stdout.text().is_empty(), "{}", stdout.text());
        assert!(stderr.text().contains("busy"), "{}", stderr.text());
    }

    // ------------------------------------------- P7c: `profile compare`, the document verb
    //
    // Design §2.7's T4 clause and D15. The arms below are about the *verb* — that it never
    // reaches the executor, that it refuses a file that is not a profile by name, that its
    // `--json` answer is the schema document and its table names the same sections. The claim
    // that the two shipped binaries print identical bytes for one pair of files is a property
    // of two processes and is asserted where two processes can be run
    // (`crates/client/tests/wchc.rs`).

    /// An [`Executor`] that is a defect if anything calls it.
    ///
    /// The document verbs' whole claim is that they run *below* this seam, and a `run` that
    /// quietly acquired one would still answer correctly — the executor would simply be built,
    /// or connected, for nothing. On `webcam-handler-client` that is not a nuance: building it
    /// is opening a socket, and a verb that needed a daemon on one root and not on the other
    /// would be the fork T4 exists to prevent. So the seam is made to *say so*, which is what
    /// `the_double_is_armed` proves it does.
    struct NeverAsked;

    impl NeverAsked {
        fn refuse(method: &str) -> ! {
            panic!("a document verb reached the executor seam at {method}");
        }
    }

    impl Executor for NeverAsked {
        fn list(&mut self) -> Result<CameraList> {
            Self::refuse("list")
        }
        fn info(&mut self, _camera: &CameraSelector) -> Result<CameraDetail> {
            Self::refuse("info")
        }
        fn controls(
            &mut self,
            _camera: &CameraSelector,
            _discover_pairs: bool,
        ) -> Result<ControlReport> {
            Self::refuse("controls")
        }
        fn get(&mut self, _camera: &CameraSelector, _control: &ControlSlug) -> Result<ControlDesc> {
            Self::refuse("get")
        }
        fn set(
            &mut self,
            _camera: &CameraSelector,
            _writes: &[ControlWrite],
            _guarded: bool,
        ) -> Result<WriteReport> {
            Self::refuse("set")
        }
        fn snapshot(&mut self, _camera: &CameraSelector) -> Result<Snapshot> {
            Self::refuse("snapshot")
        }
        fn restore(
            &mut self,
            _camera: &CameraSelector,
            _snapshot: &Snapshot,
        ) -> Result<RestoreReport> {
            Self::refuse("restore")
        }
        fn photo(
            &mut self,
            _camera: &CameraSelector,
            _request: &PhotoRequest,
        ) -> Result<Photograph> {
            Self::refuse("photo")
        }
        fn record(
            &mut self,
            _camera: &CameraSelector,
            _request: &RecordRequest,
        ) -> Result<RecordReport> {
            Self::refuse("record")
        }
        fn capture_profile(
            &mut self,
            _camera: &CameraSelector,
            _capturer: &str,
            _discover_pairs: bool,
        ) -> Result<DeviceProfile> {
            Self::refuse("capture_profile")
        }
        fn calibrate_start(
            &mut self,
            _camera: &CameraSelector,
            _task: &str,
            _goal: &str,
            _criteria: &[String],
        ) -> Result<Session> {
            Self::refuse("calibrate_start")
        }
        fn calibrate_plan(
            &mut self,
            _camera: &CameraSelector,
            _which: &SessionRef,
            _controls: &[ControlSlug],
            _order: bool,
        ) -> Result<Session> {
            Self::refuse("calibrate_plan")
        }
        fn calibrate_sweep(
            &mut self,
            _camera: &CameraSelector,
            _which: &SessionRef,
            _request: &SweepRequest,
            _watch: &dyn SweepWatcher,
        ) -> Result<Session> {
            Self::refuse("calibrate_sweep")
        }
        fn calibrate_status(
            &mut self,
            _camera: &CameraSelector,
            _which: &SessionRef,
        ) -> Result<SessionStatus> {
            Self::refuse("calibrate_status")
        }
        fn calibrate_select(
            &mut self,
            _camera: &CameraSelector,
            _which: &SessionRef,
            _control: &ControlSlug,
            _selection: &Selection,
        ) -> Result<Session> {
            Self::refuse("calibrate_select")
        }
        fn calibrate_apply(
            &mut self,
            _camera: &CameraSelector,
            _which: &SessionRef,
            _partial: bool,
        ) -> Result<WriteReport> {
            Self::refuse("calibrate_apply")
        }
        fn calibrate_restore(
            &mut self,
            _camera: &CameraSelector,
            _which: &SessionRef,
        ) -> Result<RestoreReport> {
            Self::refuse("calibrate_restore")
        }
        fn calibrate_list(&mut self, _camera: Option<&CameraSelector>) -> Result<SessionList> {
            Self::refuse("calibrate_list")
        }
    }

    /// A committed profile, by name.
    ///
    /// Fixtures enter these arms as corpus rather than as a builder: two captures of two real
    /// webcams differ in every section a comparison has an opinion about, which a constructed
    /// pair only differs in where somebody remembered to make it.
    fn corpus_path(name: &str) -> Utf8PathBuf {
        let path = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/profiles")
            .join(format!("{name}.json"));
        assert!(path.exists(), "the corpus is missing {path}");
        path
    }

    fn corpus(name: &str) -> DeviceProfile {
        let bytes = std::fs::read(corpus_path(name)).expect("a committed profile");
        serde_json::from_slice(&bytes).expect("a committed profile parses")
    }

    /// One `profile compare` run through the shared surface, with the executor armed.
    fn compared(argv: &[&str]) -> (Result<()>, String, String) {
        let cli = Cli::try_parse_from(argv).expect("profile compare parses");
        let stdout = render::tests::Buffer::default();
        let stderr = render::tests::Buffer::default();
        let mut out = Output::to_buffers(Box::new(stdout.clone()), Box::new(stderr.clone()));
        let answered = run(&cli, &mut NeverAsked, &mut out);
        (answered, stdout.text(), stderr.text())
    }

    #[test]
    #[should_panic(expected = "a document verb reached the executor seam at list")]
    fn the_double_is_armed() {
        // Non-vacuity for every arm below: `NeverAsked` proves nothing about `profile compare`
        // unless it would have gone off for a verb that *is* an executor verb.
        let _ = compared(&["webcam-handler-cli", "--json", "list"]);
    }

    #[test]
    fn a_document_verb_answers_without_ever_reaching_the_executor() {
        // Design §2.7's clause as a property: `profile compare` takes two files, answers a
        // document, and touches no camera — so it must come back with the answer from a run in
        // which the executor seam would have panicked on contact.
        let a = corpus_path("chicony-rgb");
        let b = corpus_path("chicony-ir");
        let (answered, stdout, stderr) = compared(&[
            "webcam-handler-cli",
            "--json",
            "profile",
            "compare",
            a.as_str(),
            b.as_str(),
        ]);
        answered.expect("the comparison answers");

        // The document, and it is the schema type verbatim — no envelope, which is what makes
        // the `--json` contract row above mean something.
        let document: schema::profile::ProfileComparison =
            serde_json::from_str(&stdout).expect("standard output carries a ProfileComparison");
        assert_eq!(
            document,
            corpus("chicony-rgb").compare(&corpus("chicony-ir"))
        );
        assert!(stderr.is_empty(), "{stderr}");

        // Two different webcams, so both halves have something to say. Asserted against the
        // corpus rather than against the comparison: these two captures carry different cards,
        // which is a fact about the committed documents.
        assert!(!document.device_matches(), "{document}");
        assert!(
            document.identity.iter().any(|field| field == "card"),
            "{document}"
        );
    }

    #[test]
    fn the_json_answer_round_trips_and_the_table_names_the_sections_that_differ() {
        // The two renderings of one value, and the rule this crate opens with: neither may
        // show a fact the other omits. The table's verdict column has to name the sections and
        // the control slugs, because a reader told only that "the device differs" is a reader
        // with nowhere to look.
        let a = corpus_path("chicony-rgb");
        let b = corpus_path("chicony-ir");
        let argv = ["profile", "compare", a.as_str(), b.as_str()];

        let (answered, json_text, _) =
            compared(&[&["webcam-handler-cli", "--json"], &argv[..]].concat());
        answered.expect("the comparison answers");
        let document: schema::profile::ProfileComparison =
            serde_json::from_str(&json_text).expect("a ProfileComparison");
        assert_eq!(
            serde_json::to_string_pretty(&document).expect("re-serializes") + "\n",
            json_text,
            "the printed bytes are the document's own serialization and nothing else"
        );

        let (answered, table, _) = compared(&[&["webcam-handler-cli"], &argv[..]].concat());
        answered.expect("the comparison answers");
        for named in document.device.sections() {
            assert!(table.contains(named), "{named} is missing from:\n{table}");
        }
        // …and the slugs underneath the `controls` section, which is the whole reason that
        // section is a list of names rather than a count.
        let slug = document
            .device
            .controls
            .first()
            .expect("two different webcams describe some control differently");
        assert!(table.contains(slug.as_str()), "{table}");
        for half in ["device", "identity"] {
            assert!(table.contains(half), "{table}");
        }
    }

    #[test]
    fn a_camera_at_another_address_is_the_same_device_and_the_table_says_both() {
        // FR-W2's question through the verb: the same capture with its bus path rewritten is
        // the camera reached over a forwarded bus, and the two halves must disagree — same
        // device, different address. Written to a scratch file rather than committed, because
        // a corpus entry is a document a tool captured and this one is a document a test made.
        // `TempDir::new_in` over the one scratch root, not `tempfile::tempdir()`: the owner's
        // 2026-08-12 ruling (note **N84**) puts every test's scratch under `target/`, and this
        // crate reaches it through `schema::paths` because it links no engine (T6).
        let root = schema::paths::scratch_root().expect("a scratch root");
        let dir = tempfile::TempDir::new_in(&root).expect("a scratch directory");
        let a = corpus_path("chicony-rgb");
        let mut forwarded = corpus("chicony-rgb");
        forwarded.invariant.info.fingerprint.bus_path = "9-9:1.0".to_owned();
        let b = Utf8PathBuf::from_path_buf(dir.path().join("forwarded.json"))
            .expect("a UTF-8 scratch path");
        std::fs::write(&b, serde_json::to_vec(&forwarded).expect("serializes"))
            .expect("writes the rewritten capture");

        let (answered, table, notes) = compared(&[
            "webcam-handler-cli",
            "profile",
            "compare",
            a.as_str(),
            b.as_str(),
        ]);
        answered.expect("the comparison answers");
        assert!(table.contains("same device"), "{table}");
        assert!(table.contains("fingerprint.bus_path"), "{table}");
        // The format-tree note belongs to a formats-only difference and this is not one, so a
        // rendering that printed it unconditionally is caught here rather than by a reader.
        assert!(notes.is_empty(), "{notes}");
    }

    #[test]
    fn a_formats_only_difference_prints_the_owners_ruling_and_says_so_in_the_document_too() {
        // The one line a human consumer acts on, and the field the `--json` consumer branches
        // on, driven through the shipped verb over the same pair. Both halves were unasserted
        // until 2026-08-20: the note's string literal had exactly one occurrence in the tree —
        // itself — so deleting the whole `if` block in `render::comparison` left `just ci`
        // green, and the verdict was a Rust method the document did not carry at all (notes
        // **N89**, **N286**, **N287**). The negative direction is the `notes.is_empty()`
        // assertion in the arm above, over a pair that is not formats-only.
        //
        // `TempDir::new_in` over the one scratch root, not `tempfile::tempdir()`: the owner's
        // 2026-08-12 ruling (note **N84**) puts every test's scratch under `target/`, and a
        // doctored capture is a document a test made rather than one a tool captured, so it
        // never goes near `corpus/`.
        let root = schema::paths::scratch_root().expect("a scratch root");
        let dir = tempfile::TempDir::new_in(&root).expect("a scratch directory");
        let a = corpus_path("chicony-rgb");
        let mut fewer_modes = corpus("chicony-rgb");
        let dropped = fewer_modes
            .invariant
            .formats
            .pop()
            .expect("a committed capture advertises at least one pixel format");
        assert!(
            !fewer_modes.invariant.formats.is_empty(),
            "dropping the last pixel format would make this a capture of a camera that \
             advertises nothing, which is a different claim from a replug"
        );
        let b = Utf8PathBuf::from_path_buf(dir.path().join("fewer-modes.json"))
            .expect("a UTF-8 scratch path");
        std::fs::write(&b, serde_json::to_vec(&fewer_modes).expect("serializes"))
            .expect("writes the doctored capture");

        let argv = ["profile", "compare", a.as_str(), b.as_str()];
        let (answered, table, notes) = compared(&[&["webcam-handler-cli"], &argv[..]].concat());
        answered.expect("the comparison answers");
        assert!(
            table.contains("formats"),
            "the section that moved is missing from:\n{table}"
        );
        assert!(
            notes.contains(
                "the format tree is the only device section that differs, and a camera may \
                 advertise a different one each time it is plugged in"
            ),
            "a capture that stopped advertising {} is the owner's 2026-08-13 ruling and the \
             rendering said nothing about it: {notes:?}",
            dropped.pixel_format
        );

        // …and the same conclusion out of the document, from the field rather than from a
        // conjunction over the three device flags — which is the spelling note **N89** rules
        // out and the reason the verdict is on the document at all.
        let (answered, json_text, _) =
            compared(&[&["webcam-handler-cli", "--json"], &argv[..]].concat());
        answered.expect("the comparison answers");
        let document: schema::profile::ProfileComparison =
            serde_json::from_str(&json_text).expect("a ProfileComparison");
        assert_eq!(
            document.verdict(),
            schema::profile::DeviceVerdict::OnlyTheFormatTree,
            "{json_text}"
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&json_text)
                .expect("an object")
                .get("verdict")
                .and_then(serde_json::Value::as_str),
            Some("only_the_format_tree"),
            "the table printed the ruling and the document a subprocess reads did not carry \
             it: {json_text}"
        );
    }

    #[test]
    fn a_file_that_is_not_a_profile_is_refused_by_name_and_never_compared() {
        // Three refusals, all from the D13 registry v3 adds nothing to, and each one naming
        // what a caller has to fix. An agent reading these unsupervised needs the path in the
        // first two and both version numbers in the third.
        // `TempDir::new_in` over the one scratch root, not `tempfile::tempdir()`: the owner's
        // 2026-08-12 ruling (note **N84**) puts every test's scratch under `target/`, and this
        // crate reaches it through `schema::paths` because it links no engine (T6).
        let root = schema::paths::scratch_root().expect("a scratch root");
        let dir = tempfile::TempDir::new_in(&root).expect("a scratch directory");
        let good = corpus_path("chicony-rgb");
        let write = |name: &str, bytes: &[u8]| -> Utf8PathBuf {
            let path = Utf8PathBuf::from_path_buf(dir.path().join(name)).expect("a UTF-8 path");
            std::fs::write(&path, bytes).expect("writes the fixture");
            path
        };

        let missing = dir.path().join("was-never-written.json");
        let missing = Utf8PathBuf::from_path_buf(missing).expect("a UTF-8 path");
        let (answered, stdout, _) = compared(&[
            "webcam-handler-cli",
            "profile",
            "compare",
            missing.as_str(),
            good.as_str(),
        ]);
        let error = answered.expect_err("a path with no file behind it is not a comparison");
        assert_eq!(error.kind(), ErrorKind::StorageIo, "{error}");
        assert!(error.to_string().contains(missing.as_str()), "{error}");
        assert!(stdout.is_empty(), "{stdout}");

        // A JSON document that is not a profile, and one that is not JSON at all: the message
        // has to tell those apart, because "your file is corrupt" and "you named the wrong
        // file" send a caller to different places.
        //
        // Carrying *this build's* profile version deliberately, so the refusal under test is
        // about the document's shape: a fixture at some other number would be refused by the
        // version probe one line earlier and this arm would be measuring the wrong sentence.
        // Named for what it stands for — a caller who pointed `profile compare` at a
        // calibration session's own document — without spelling that document's filename:
        // `atomic-write-home.sh` reads any file naming D9's filenames beside a raw write
        // primitive as a bypass of `write_json_atomic`, and it is right to, because it cannot
        // tell a fixture from a store write by grepping. The reader loses nothing; what this
        // fixture is about is its *contents*.
        let not_a_profile = write(
            "a-session-document.json",
            format!(
                r#"{{"schema_version":{},"id":"nope"}}"#,
                schema::limits::PROFILE_SCHEMA_VERSION
            )
            .as_bytes(),
        );
        let (answered, ..) = compared(&[
            "webcam-handler-cli",
            "profile",
            "compare",
            good.as_str(),
            not_a_profile.as_str(),
        ]);
        let error = answered.expect_err("a session document is not a device profile");
        assert_eq!(error.kind(), ErrorKind::StorageIo, "{error}");
        assert!(
            error.to_string().contains("is not a device profile"),
            "{error}"
        );

        let not_json = write("photo.jpg", b"\xff\xd8\xff\xe0 not json");
        let (answered, ..) = compared(&[
            "webcam-handler-cli",
            "profile",
            "compare",
            good.as_str(),
            not_json.as_str(),
        ]);
        let error = answered.expect_err("a JPEG is not a device profile");
        assert!(
            error.to_string().contains("is not a JSON document"),
            "{error}"
        );
    }

    #[test]
    fn a_profile_from_another_schema_version_is_refused_for_its_version() {
        // The refusal an agent can act on: re-capture, or use the build that wrote it. Refused
        // *for the version* rather than for whichever field this build's shape is missing,
        // which is why the read probes `schema_version` before it parses anything else — and
        // the fixture is a real committed profile with one number changed, so nothing else
        // about it could be what this arm is measuring.
        // `TempDir::new_in` over the one scratch root, not `tempfile::tempdir()`: the owner's
        // 2026-08-12 ruling (note **N84**) puts every test's scratch under `target/`, and this
        // crate reaches it through `schema::paths` because it links no engine (T6).
        let root = schema::paths::scratch_root().expect("a scratch root");
        let dir = tempfile::TempDir::new_in(&root).expect("a scratch directory");
        let mut document: serde_json::Value = serde_json::from_slice(
            &std::fs::read(corpus_path("chicony-rgb")).expect("a committed profile"),
        )
        .expect("a JSON document");
        let ours = schema::limits::PROFILE_SCHEMA_VERSION;
        document["schema_version"] = serde_json::json!(ours + 1);
        let foreign =
            Utf8PathBuf::from_path_buf(dir.path().join("next-version.json")).expect("a UTF-8 path");
        std::fs::write(&foreign, document.to_string()).expect("writes the fixture");

        let (answered, ..) = compared(&[
            "webcam-handler-cli",
            "profile",
            "compare",
            corpus_path("chicony-rgb").as_str(),
            foreign.as_str(),
        ]);
        let error = answered.expect_err("a profile from another version is not read");
        assert_eq!(
            error,
            Error::SchemaVersionForeign {
                found: ours + 1,
                supported: ours,
            },
            "{error}"
        );

        // The other direction, and it is what makes the arm above about the *version*: the
        // same bytes at this build's version are read and compared.
        document["schema_version"] = serde_json::json!(ours);
        let ours_path =
            Utf8PathBuf::from_path_buf(dir.path().join("this-version.json")).expect("a UTF-8 path");
        std::fs::write(&ours_path, document.to_string()).expect("writes the fixture");
        let (answered, table, _) = compared(&[
            "webcam-handler-cli",
            "profile",
            "compare",
            corpus_path("chicony-rgb").as_str(),
            ours_path.as_str(),
        ]);
        answered.expect("a profile at this build's version compares");
        assert!(table.contains("same device"), "{table}");
    }

    // ---------------------------------------------- P8b: `photo diff`, the document verb
    //
    // Design §2.7's T4 clause again, and D17. The arms are shaped like the `profile compare`
    // ones above because the claim is the same one about a different document: the verb runs
    // below the executor, refuses a file it cannot read by name, and prints two renderings of
    // one value. What is new is the third of those — a comparison that cannot compute one
    // answer states the reason and answers the rest (AGENTS rule 6), and a human rendering
    // that turned that into a refusal would throw away the half that *was* computed.
    //
    // The claim that the two shipped binaries print identical bytes for one pair of files is a
    // property of two processes and is asserted where two can be run
    // (`crates/client/tests/wchc.rs`).

    /// A scratch directory for the arms below to write photographs into.
    ///
    /// `TempDir::new_in` over the one scratch root for the reason the profile arms give: the
    /// owner's 2026-08-12 ruling (note **N84**) puts every test's scratch under `target/`, and
    /// this crate reaches it through `schema::paths` because it links no engine (T6). The
    /// pictures in it are synthetic, so nothing here is a frame that could contain a person.
    fn photo_scratch() -> tempfile::TempDir {
        let root = schema::paths::scratch_root().expect("a scratch root");
        tempfile::TempDir::new_in(&root).expect("a scratch directory")
    }

    /// One file in it, and the path to name on a command line.
    fn write_file(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> Utf8PathBuf {
        let path = Utf8PathBuf::from_path_buf(dir.path().join(name)).expect("a UTF-8 path");
        std::fs::write(&path, bytes).expect("writes the fixture");
        path
    }

    /// One synthetic picture, as the PNG bytes this build writes.
    ///
    /// Through `imaging::encode`, which is the writer `imaging::compare::read` is defined
    /// against — a hand-built PNG here would be testing somebody else's encoder. It takes a
    /// `Decoded` rather than a pixel buffer so this crate names no type from `image` itself:
    /// the edge is to `webcam-handler-imaging`, and the pixel type is that crate's business.
    fn png(image: imaging::Decoded) -> Vec<u8> {
        imaging::encode::png(&image).expect("a fixture encodes")
    }

    /// A file of `size` bytes that occupies none of them, and the path to name on a command
    /// line.
    ///
    /// `set_len` rather than bytes, because what is under test is the number the *file system*
    /// reports and the whole point of the bound is that nothing reads past it: a fixture
    /// written a byte at a time would cost half a gigabyte of disk to prove that half a
    /// gigabyte is never allocated.
    fn sparse_file(dir: &tempfile::TempDir, name: &str, size: u64) -> Utf8PathBuf {
        let path = Utf8PathBuf::from_path_buf(dir.path().join(name)).expect("a UTF-8 path");
        std::fs::File::create(&path)
            .expect("creates the fixture")
            .set_len(size)
            .expect("a sparse file of the size this arm names");
        path
    }

    #[test]
    fn a_file_past_this_builds_budget_is_refused_naming_it_rather_than_read_whole() {
        // **The door the bound names was not the first door** (note **N322**). Both document
        // verbs take two paths a caller named and both used to hand them straight to
        // `std::fs::read`, which sizes its buffer from the file's own length — so the number
        // reaching the allocator came off the command line exactly as the extents in a
        // photograph's header do, one call before the budget note **N268** landed was ever
        // consulted. Measured through the shipped binary at the time: a 3 GiB file answered
        // `photo diff` correctly after a resident set of 3.1 GB, and under a memory ceiling
        // below its size the same command was killed with exit 137, no `Failure` document and
        // an empty standard output — the one failure shape the `--json` ruling forbids.
        //
        // Every verb of this crate that takes a path off a command line, because a bound on
        // some of them is a ban on some spellings of the defect (note **N249**). Two of the
        // three were bounded and `restore` was not, and it answered exactly the same way when
        // it was measured a day later: killed at exit 137 with nothing on either stream (note
        // **N329**). The fourth door is `engine::profile::read`, which this crate cannot reach
        // and `crates/engine/src/profile.rs`'s own arm holds.
        let dir = photo_scratch();
        for (label, budget, path) in [
            (
                "photo diff",
                schema::limits::MAX_PHOTO_DECODE_BYTES,
                sparse_file(&dir, "huge.png", schema::limits::MAX_PHOTO_DECODE_BYTES + 1),
            ),
            (
                "profile compare",
                schema::limits::MAX_PROFILE_FILE_BYTES,
                sparse_file(
                    &dir,
                    "huge.json",
                    schema::limits::MAX_PROFILE_FILE_BYTES + 1,
                ),
            ),
            (
                "restore",
                schema::limits::MAX_SNAPSHOT_FILE_BYTES,
                sparse_file(
                    &dir,
                    "huge-snapshot.json",
                    schema::limits::MAX_SNAPSHOT_FILE_BYTES + 1,
                ),
            ),
        ] {
            // `restore` reads the document before it resolves the camera, which is why
            // `NeverAsked` can hold this arm: the refusal happens before any executor verb is
            // reached, and `the_double_is_armed` is what makes that non-vacuous.
            let argv: Vec<&str> = match label {
                "restore" => vec![
                    "webcam-handler-cli",
                    "restore",
                    "cam:integrated",
                    path.as_str(),
                ],
                _ => {
                    let mut split = label.split(' ');
                    vec![
                        "webcam-handler-cli",
                        split.next().unwrap_or_default(),
                        split.next().unwrap_or_default(),
                        path.as_str(),
                        path.as_str(),
                    ]
                }
            };
            let (answered, _, _) = compared(&argv);
            let refusal = answered.expect_err(
                "a file past this build's budget was read rather than refused, so what happens \
                 next is whatever the allocator does",
            );
            let Error::StorageIo {
                path: named,
                errno,
                message,
            } = &refusal
            else {
                panic!("{label} refused a file for its size as {refusal:?}");
            };
            assert_eq!(named, &path, "the refusal must name the file it refused");
            assert_eq!(
                *errno, None,
                "no system call failed here — this build declined to make one"
            );
            assert!(
                message.contains(&(budget + 1).to_string())
                    && message.contains(&budget.to_string()),
                "the refusal must name what the file is and what this build will spend, so an \
                 unattended reader can tell a file that is too big from a machine that is out \
                 of memory: {message}"
            );
        }
    }

    #[test]
    fn a_file_inside_the_budget_is_not_refused_for_its_size_on_either_document_verb() {
        // The bound from the other side (N255). A file this build could have written reaches
        // the reader that knows what it is: the photograph is compared, the profile is parsed,
        // and neither refusal path above is taken — which is what stops the budget from being
        // tightened onto the product it exists above. The sizes are read off the fixtures
        // rather than assumed, because an arm that asserted "inside the budget" about a file
        // that was not would be green for the wrong reason.
        let dir = photo_scratch();
        let picture = imaging::fixtures::checkerboard(32, 32, 4);
        let photograph = write_file(&dir, "a.png", &png(imaging::Decoded::Gray(picture)));
        let profile = corpus_path("chicony-rgb");
        for (verb, sub, budget, path) in [
            (
                "photo",
                "diff",
                schema::limits::MAX_PHOTO_DECODE_BYTES,
                photograph,
            ),
            (
                "profile",
                "compare",
                schema::limits::MAX_PROFILE_FILE_BYTES,
                profile,
            ),
        ] {
            let size = std::fs::metadata(&path)
                .expect("the fixture is there")
                .len();
            assert!(
                size < budget,
                "{path} is {size} bytes and this arm needs a file inside the {budget}-byte \
                 budget"
            );
            let (answered, _, _) = compared(&[
                "webcam-handler-cli",
                verb,
                sub,
                path.as_str(),
                path.as_str(),
            ]);
            answered.unwrap_or_else(|refusal| {
                panic!("a {size}-byte file this build could have written was refused: {refusal}")
            });
        }
    }

    #[test]
    fn a_snapshot_inside_the_budget_reaches_the_reader_that_knows_what_it_is() {
        // `restore`'s side of the bound, from the other direction (note **N255**). The two
        // document verbs above can assert the file was *answered*; this one cannot, because a
        // snapshot this build could have written would go on to reach the executor and
        // `NeverAsked` is what makes this whole module's arms mean anything. So what it asserts
        // is which refusal came back: a small file that is not a snapshot is refused for not
        // being one, which is only reachable once the size door has let it through. A bound
        // tightened onto the product would answer the other sentence here.
        let dir = photo_scratch();
        let path = write_file(&dir, "not-a-snapshot.json", b"{\"hello\": 1}");
        let (answered, _, _) = compared(&[
            "webcam-handler-cli",
            "restore",
            "cam:integrated",
            path.as_str(),
        ]);
        let refusal = answered.expect_err("this fixture is not a snapshot document");
        let Error::StorageIo { message, .. } = &refusal else {
            panic!("a small non-snapshot was refused as {refusal:?}");
        };
        assert!(
            message.contains("not a snapshot document"),
            "a file inside the budget has to reach the parser, so the refusal is about its \
             shape and not about its size: {message}"
        );
    }

    #[test]
    fn a_photograph_comparison_answers_without_ever_reaching_the_executor() {
        // The §2.7 clause as a property, for the second document verb: two files in, one
        // document out, from a run in which the executor seam would have panicked on contact.
        // `the_double_is_armed` above is what makes that non-vacuous.
        let dir = photo_scratch();
        let base = imaging::fixtures::checkerboard(32, 32, 4);
        let blurred = imaging::fixtures::blurred(&base, 2.0).expect("a 32x32 image blurs");
        let a = write_file(&dir, "a.png", &png(imaging::Decoded::Gray(base.clone())));
        let b = write_file(&dir, "b.png", &png(imaging::Decoded::Gray(blurred.clone())));

        let (answered, stdout, stderr) = compared(&[
            "webcam-handler-cli",
            "--json",
            "photo",
            "diff",
            a.as_str(),
            b.as_str(),
        ]);
        answered.expect("the comparison answers");

        // The schema type verbatim, and the *bytes* the core's own serialization produces —
        // which is what makes the `--json` contract row for `photo-diff` mean something. The
        // expectation is the core's answer to the same two images, not a transcription of
        // this run.
        //
        // Compared as bytes rather than by parsing and comparing values, and that is a
        // measurement rather than a preference: `serde_json` without its `float_roundtrip`
        // feature parses `0.49816176470588236` back as `0.4981617647058824`, one ULP away
        // (measured here, 2026-08-18). It is harmless for this document — D17's consumer
        // applies its own tolerance, and nothing in the comparison is a discriminant — but a
        // value equality after a parse would be a test that passes on the numbers it happened
        // to be given.
        let expected = imaging::compare::photos(&base, &blurred);
        assert_eq!(
            stdout,
            serde_json::to_string_pretty(&expected).expect("the core's answer serializes") + "\n"
        );
        assert!(stderr.is_empty(), "{stderr}");
        // And it is one schema type and nothing else: a consumer parses this and is done.
        let _: schema::metrics::PhotoComparison =
            serde_json::from_str(&stdout).expect("standard output carries a PhotoComparison");

        // It found something, too: a blurred copy of a picture is less sharp than the picture
        // and scores below 1.0 against it. Two claims about the *pair*, so a run that had
        // compared one file with itself could not have produced them.
        let delta = expected
            .delta
            .get(&MetricName::Sharpness)
            .copied()
            .expect("every metric has a delta");
        assert!(delta < 0.0, "blurring must lower the sharpness: {delta}");
        let score = expected.ssim.score().expect("equal sizes score");
        assert!((0.0..1.0).contains(&score), "{score}");
    }

    #[test]
    fn the_json_answer_parses_as_one_document_and_the_table_carries_every_metric_on_both_sides() {
        // The two renderings of one value, and this crate's opening rule: neither may show a
        // fact the other omits. The table's population is `MetricName::ALL` for the same
        // reason the document's is — a sixth metric joins both by existing — so the assertion
        // walks the vocabulary rather than a list written here.
        let dir = photo_scratch();
        let base = imaging::fixtures::gradient(32, 32);
        let brighter = imaging::fixtures::overexposed(&base);
        let a = write_file(&dir, "a.png", &png(imaging::Decoded::Gray(base)));
        let b = write_file(&dir, "b.png", &png(imaging::Decoded::Gray(brighter)));
        let argv = ["photo", "diff", a.as_str(), b.as_str()];

        let (answered, json_text, _) =
            compared(&[&["webcam-handler-cli", "--json"], &argv[..]].concat());
        answered.expect("the comparison answers");
        // One schema type and no envelope: what a consumer parses is the document, and every
        // property it carries is one the committed bundle declares — which
        // `scripts/gates/json-validates.sh` is what checks, against the bundle itself.
        let document: schema::metrics::PhotoComparison =
            serde_json::from_str(&json_text).expect("a PhotoComparison");
        assert_eq!(document.delta.len(), MetricName::ALL.len(), "{json_text}");

        let (answered, table, _) = compared(&[&["webcam-handler-cli"], &argv[..]].concat());
        answered.expect("the comparison answers");
        for name in MetricName::ALL {
            assert!(
                table.contains(name.as_str()),
                "{name} is missing from:\n{table}"
            );
        }
        // Both shapes, so a reader can see that the two pictures are the same size — which is
        // also what decides whether the line below it can carry a score at all.
        assert!(table.contains("32x32"), "{table}");
        assert!(table.contains("ssim: "), "{table}");
    }

    #[test]
    fn a_pair_that_cannot_be_scored_says_why_and_still_reports_every_metric() {
        // **D17's shape, at the surface** (AGENTS rule 6). Two photographs of different sizes
        // are a comparison this tool answers, not one it refuses: every per-metric scalar is
        // well defined on unequal images and is printed, and the one quantity that needed
        // matching shapes says so — with both shapes in the sentence, because a reader told
        // only that there is no score has nothing to act on.
        let dir = photo_scratch();
        let small = imaging::fixtures::checkerboard(16, 16, 4);
        let large = imaging::fixtures::checkerboard(32, 32, 4);
        let a = write_file(
            &dir,
            "small.png",
            &png(imaging::Decoded::Gray(small.clone())),
        );
        let b = write_file(&dir, "large.png", &png(imaging::Decoded::Gray(large)));
        let argv = ["photo", "diff", a.as_str(), b.as_str()];

        let (answered, table, stderr) = compared(&[&["webcam-handler-cli"], &argv[..]].concat());
        answered.expect("a mismatched pair is answered, not refused");
        assert!(table.contains("not computed"), "{table}");
        assert!(
            table.contains("16x16") && table.contains("32x32"),
            "{table}"
        );
        assert!(table.contains("nothing here resizes"), "{table}");
        for name in MetricName::ALL {
            assert!(table.contains(name.as_str()), "{name}:\n{table}");
        }
        // The reason is the answer, so it goes to standard output with the rest of it; a
        // sentence pushed to standard error would be one a caller redirecting stdout loses.
        assert!(stderr.is_empty(), "{stderr}");

        // The same fact reached from `--json`, which is what "two renderings of one value"
        // means here: the reason is data, and both consumers branch on the same field.
        let (answered, json_text, _) =
            compared(&[&["webcam-handler-cli", "--json"], &argv[..]].concat());
        answered.expect("a mismatched pair is answered, not refused");
        let document: schema::metrics::PhotoComparison =
            serde_json::from_str(&json_text).expect("a PhotoComparison");
        assert_eq!(
            document.ssim,
            schema::metrics::Ssim::Unavailable {
                reason: schema::metrics::SsimUnavailable::DimensionsDiffer {
                    a: [16, 16],
                    b: [32, 32],
                },
            },
            "{json_text}"
        );

        // The inverse, and the arm is about the *reason* without it: a pair that can be scored
        // prints a number and never the sentence above.
        let same = write_file(&dir, "same.png", &png(imaging::Decoded::Gray(small)));
        let (answered, scored, _) = compared(&[
            "webcam-handler-cli",
            "photo",
            "diff",
            a.as_str(),
            same.as_str(),
        ]);
        answered.expect("an equal-sized pair scores");
        assert!(scored.contains("ssim: 1.0000"), "{scored}");
        assert!(!scored.contains("not computed"), "{scored}");
    }

    #[test]
    fn a_file_that_is_not_a_photograph_is_refused_by_name_and_never_compared() {
        // The refusals this verb owns, all from the D13 registry v3 adds nothing to (D17: a
        // dimension mismatch is represented, so the only failures here are about the files).
        let dir = photo_scratch();
        let good = write_file(
            &dir,
            "good.png",
            &png(imaging::Decoded::Gray(imaging::fixtures::checkerboard(
                16, 16, 4,
            ))),
        );

        // Bytes in no format this build writes. The refusal names the formats that *would*
        // have read and what was found instead, which is what an unattended caller needs:
        // "convert it" and "you named the wrong file" are different repairs.
        let not_a_photograph = write_file(&dir, "notes.txt", b"GIF89a and then some");
        let (answered, stdout, _) = compared(&[
            "webcam-handler-cli",
            "photo",
            "diff",
            good.as_str(),
            not_a_photograph.as_str(),
        ]);
        let error = answered.expect_err("a GIF is not a photograph this build writes");
        assert_eq!(error.kind(), ErrorKind::DeviceIo, "{error}");
        let message = error.to_string();
        for expected in ["not a photograph this build writes", "the first bytes are"] {
            assert!(message.contains(expected), "{message}");
        }
        for format in PhotoFormat::ALL {
            assert!(message.contains(format.as_str()), "{message}");
        }
        assert!(stdout.is_empty(), "{stdout}");

        // A file that is there and is not decodable is told apart from one that is not there:
        // the second names the path, because that is the refusal a caller acts on by looking.
        let missing = good.with_file_name("was-never-written.png");
        let (answered, ..) = compared(&[
            "webcam-handler-cli",
            "photo",
            "diff",
            missing.as_str(),
            good.as_str(),
        ]);
        let error = answered.expect_err("a path with no file behind it is not a comparison");
        assert_eq!(error.kind(), ErrorKind::StorageIo, "{error}");
        assert!(error.to_string().contains(missing.as_str()), "{error}");

        // And the ordering this verb documents, which is the only thing that makes "which of
        // the two" answerable: `a` is read and decoded before `b` is opened, so a run naming
        // two unreadable files refuses the first. Both directions, because a refusal that
        // always named the same side would satisfy one of them.
        let other_missing = missing.with_file_name("also-never-written.png");
        for (first, second, named) in [
            (&missing, &other_missing, &missing),
            (&other_missing, &missing, &other_missing),
        ] {
            let (answered, ..) = compared(&[
                "webcam-handler-cli",
                "photo",
                "diff",
                first.as_str(),
                second.as_str(),
            ]);
            let error = answered.expect_err("neither file is a photograph");
            assert!(
                error.to_string().contains(named.as_str()),
                "the refusal is about {named} and says: {error}"
            );
        }
    }
}
