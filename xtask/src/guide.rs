//! The agent usage guide (docs/7 P6e): emitted from the command surface, never typed.
//!
//! AGENTS' "Who runs this, and why" names the primary consumer: *an AI agent harness drives
//! the client to photograph the device under test, to check its own work* — continuous,
//! unattended, and with **no hands**. This document is that consumer's manual, and it is the
//! successor to `vendor/v4l2-webcam-skill/`, which teaches the same operations as sequences
//! of `v4l2-ctl` and `ffmpeg` commands (design §1.1's map is the correspondence).
//!
//! ## The register here is not this repository's register, and that is deliberate
//!
//! Every other doc comment in this workspace argues: it cites design items and note ids, it
//! names the defect a check exists to catch, and it says why a trade-off went the way it did.
//! **The guide this module emits does none of that.** Its reader is a program deciding what
//! to call next, so it is imperative, exact, and free of the argument — what to run, what
//! comes back, and what to do about a refusal. A sentence explaining *why* `--wait` is inert
//! under `webcam-handler-cli` belongs in the flag's own doc comment (where it is); the guide
//! says that it is inert.
//!
//! This paragraph exists so that the next person to read the emitted Markdown does not
//! "improve" it into the house style. The house style is *here*, in the generator; the
//! product is over there.
//!
//! ## Derived, and what that costs
//!
//! The verb vocabulary, every flag, every one-line description, the `--json` contract of each
//! verb and the whole D13 error table are **read out of the code that implements them** — the
//! clap tree `webcam-handler-cli-core` declares, `cli_core::contracts`' table, and
//! `ErrorKind::ALL` walked for `api::rpc_code` and `Error::sample`. So a renamed flag or a
//! nineteenth error variant moves this file, and `scripts/gates/agent-guide-current.sh` goes
//! red until it is regenerated and committed. That is the whole of "so it cannot drift".
//!
//! What is **not** derivable is narrative: which verb to reach for first, what a calibration
//! session is for, and what an agent should *do* about `Busy` as opposed to `DeviceGone`.
//! That prose lives in this file — in one place per claim, marked in the emitted document as
//! prose, and every command it shows is executed against the built binary by
//! `crates/cli/tests/agent_guide.rs`. A reader who cannot tell a derived sentence from a
//! written one will trust the wrong sentence, which is why the provenance is printed rather
//! than implied.
//!
//! ### The one thing the derivation imports
//!
//! clap's help strings are doc comments, and this project's doc comments carry citations —
//! `(D3)`, `\[PF:11\]`, `(note N110)`. They arrive in the guide with the sentences they
//! qualify. Stripping them would mean a regex over prose nobody validated, so they are kept
//! and explained once, in the guide's own "How to read this document" section. Rustdoc's
//! bracket escaping is undone by [`crate::unescape_doc_brackets`] — the same rewrite the
//! JSON artifacts get, for the same reason.

use std::fmt::Write as _;

use anyhow::{Result, bail};
use schema::error::{Error, ErrorKind, Failure};

use crate::{error_component_name, unescape_doc_brackets};

/// Where the guide is committed, relative to the repository root.
///
/// Read out of this constant by `scripts/gates/agent-guide-current.sh` rather than repeated
/// there, exactly as `ARTIFACT_DIR` is read by `schema-artifacts-current.sh`: a second copy
/// of "where the generated file lives" is the drift the gate exists to catch, wearing the
/// gate's own approval.
pub(crate) const GUIDE_PATH: &str = "docs/agent-guide.md";

/// The token an example uses where the reader substitutes a real value.
///
/// Upper case, which is not decoration: the synopses in this document are *derived* and clap
/// writes a value name that way, so a hand-written example using `<camera>` beside a
/// generated synopsis saying `<CAMERA>` would look like two different things to a reader who
/// has only this file. One convention, and the derived half chose it.
///
/// `crates/cli/tests/agent_guide.rs` substitutes these against a replayed corpus profile,
/// which is what makes every example in this guide a command that has actually run.
struct Placeholder {
    /// How it is written in an example.
    token: &'static str,
    /// What the reader puts there.
    meaning: &'static str,
}

/// Every placeholder the guide's examples use.
///
/// A hand list, and it is checked: `every_placeholder_the_examples_use_is_documented` walks
/// the emitted examples for `<…>` tokens and fails on one this table does not explain, which
/// is how a reader is never shown a token with no instruction beside it.
const PLACEHOLDERS: &[Placeholder] = &[
    Placeholder {
        token: "<CAMERA>",
        meaning: "a camera id from `list`, or any unambiguous prefix of one \
                  (`cam:obsbot-tiny-3`, `cam:obsbot`)",
    },
    Placeholder {
        token: "<CONTROL>",
        meaning: "a control slug from `controls` (`brightness`, `pan_absolute`)",
    },
    Placeholder {
        token: "<VALUE>",
        meaning: "an integer inside that control's declared range",
    },
    Placeholder {
        token: "<TASK>",
        meaning: "your own name for what a calibration session is for (`read the DUT display`)",
    },
    Placeholder {
        token: "<PHOTO>",
        meaning: "a path to write a photo to; the extension chooses the encoding \
                  (`.jpg`, `.png`)",
    },
    Placeholder {
        token: "<RECORDING>",
        meaning: "a path to write a recording to; the extension chooses the container \
                  (`.avi`, `.y4m`)",
    },
    Placeholder {
        token: "<SNAPSHOT>",
        meaning: "a path to a snapshot document written by `snapshot`",
    },
    Placeholder {
        token: "<PROFILE>",
        meaning: "a path to write a device profile to (`.json`)",
    },
];

/// Where a section's content came from, printed under its heading.
///
/// A reader who cannot tell a sentence generated from the parser from one somebody wrote will
/// trust the wrong one — the derived half cannot be stale (the freshness gate re-emits and
/// diffs it) and the written half can be wrong in a way no diff notices, which is exactly the
/// distinction worth spending a line on.
#[derive(Debug, Clone, Copy)]
enum Provenance {
    /// Generated from the named source at build time.
    Derived(&'static str),
    /// Written by hand in this generator; every command in it is executed by the guide's
    /// example test.
    Prose,
}

impl Provenance {
    /// The line printed under the section heading.
    fn line(self) -> String {
        match self {
            Provenance::Derived(source) => {
                format!("*Generated from {source}. Do not edit; regenerate.*")
            }
            Provenance::Prose => "*Written prose. Any command in it is run against the built \
                                  binary by the guide's example test.*"
                .to_owned(),
        }
    }

    /// The word the table of contents uses.
    fn word(self) -> &'static str {
        match self {
            Provenance::Derived(_) => "generated",
            Provenance::Prose => "written",
        }
    }
}

/// One section of the guide.
struct Section {
    /// Its heading, without the `##`.
    title: String,
    /// Where its content came from.
    provenance: Provenance,
    /// The Markdown under the heading.
    body: String,
}

/// The guide, as the bytes it is committed as.
///
/// # Errors
///
/// Anything a section's assembly refuses with — a verb the clap tree offers and the contract
/// table does not name, a D13 kind that does not serialize as a name. Both are conditions the
/// emitter must not paper over: a guide missing a verb is a manual that hides an operation,
/// and this is the last place that can notice.
pub(crate) fn guide() -> Result<String> {
    let root = cli_core::contracts::command_tree(cli_core::Program::Cli);
    let sections = vec![
        Section {
            title: "How to read this document".to_owned(),
            provenance: Provenance::Prose,
            body: how_to_read(),
        },
        Section {
            title: "Which program to run".to_owned(),
            provenance: Provenance::Prose,
            body: which_program(),
        },
        Section {
            title: "Options that work on every verb".to_owned(),
            provenance: Provenance::Derived("the shared clap tree in `webcam-handler-cli-core`"),
            body: global_options(&root),
        },
        Section {
            title: "The verbs".to_owned(),
            provenance: Provenance::Derived(
                "the shared clap tree in `webcam-handler-cli-core`, and \
                 `crates/cli-core/json-contracts.tsv` for the answers",
            ),
            body: verb_reference(&root)?,
        },
        Section {
            title: "The words a flag takes".to_owned(),
            provenance: Provenance::Derived("the closed vocabularies in `webcam-handler-schema`"),
            body: vocabularies(),
        },
        Section {
            title: "What `--json` answers with".to_owned(),
            provenance: Provenance::Derived("`crates/cli-core/json-contracts.tsv`"),
            body: json_contracts()?,
        },
        Section {
            title: "Failures, and what to do about each one".to_owned(),
            provenance: Provenance::Derived(
                "the D13 error registry in `webcam-handler-schema` — every failure, its exit \
                 code, its JSON-RPC code and its message; the `Do` column is written prose",
            ),
            body: errors()?,
        },
        Section {
            title: "Take a photograph of the device under test".to_owned(),
            provenance: Provenance::Prose,
            body: photograph_recipe(),
        },
        Section {
            title: "Record a video of the device under test".to_owned(),
            provenance: Provenance::Prose,
            body: record_recipe(),
        },
        Section {
            title: "Leave the camera as you found it".to_owned(),
            provenance: Provenance::Prose,
            body: snapshot_recipe(),
        },
        Section {
            title: "Calibrate a camera for a task".to_owned(),
            provenance: Provenance::Prose,
            body: calibration_walkthrough(),
        },
        Section {
            title: "If you know the manual v4l2-ctl and ffmpeg workflow".to_owned(),
            provenance: Provenance::Prose,
            body: operations_map(&root)?,
        },
    ];

    let mut out = String::new();
    out.push_str(&preamble());
    out.push_str(&contents(&sections));
    for section in &sections {
        writeln!(out, "## {}\n", section.title)?;
        writeln!(out, "{}\n", section.provenance.line())?;
        out.push_str(section.body.trim_end());
        out.push_str("\n\n");
    }
    // One trailing newline and no more, which is what every generated artifact here ends on.
    while out.ends_with("\n\n") {
        out.pop();
    }
    Ok(out)
}

/// The banner and the first paragraph a reader meets.
fn preamble() -> String {
    format!(
        "<!-- Generated by `webcam-handler-xtask generate` from the command surface. \
         Do not edit this file; edit `xtask/src/guide.rs` or the code it reads, then run \
         `just generate`. -->\n\n\
         # Driving webcam-handler\n\n\
         webcam-handler drives V4L2 webcams from a command line: list them, read and write \
         their controls, take photos, record video, and calibrate a camera for a task by \
         sweeping a control and scoring the photographs.\n\n\
         This is the reference for a program driving it — an agent harness photographing a \
         device under test to check its own work. It is generated from the command surface \
         itself, so a verb, a flag or an error listed here is one this build has. Version \
         {}.\n\n",
        schema::TOOL_VERSION
    )
}

/// The table of contents, with each section's provenance beside it.
fn contents(sections: &[Section]) -> String {
    let mut out = String::from("| Section | Content |\n|---|---|\n");
    for section in sections {
        let anchor = anchor(&section.title);
        let _ = writeln!(
            out,
            "| [{}](#{anchor}) | {} |",
            section.title,
            section.provenance.word()
        );
    }
    out.push('\n');
    out
}

/// A GitHub-style heading anchor.
fn anchor(title: &str) -> String {
    title
        .chars()
        .filter_map(|c| {
            if c.is_ascii_alphanumeric() {
                Some(c.to_ascii_lowercase())
            } else if c == ' ' || c == '-' {
                Some('-')
            } else {
                None
            }
        })
        .collect()
}

fn how_to_read() -> String {
    let mut out = String::from(
        "Every section says under its heading whether it was generated from the code or \
         written by hand. A generated section cannot describe a verb or a flag this build \
         does not have: `scripts/gates/agent-guide-current.sh` re-runs the generator and \
         fails if this file differs from what it emits.\n\n\
         Examples are written with placeholders. Substitute a real value for each one before \
         running the command:\n\n\
         | In an example | Substitute |\n|---|---|\n",
    );
    for placeholder in PLACEHOLDERS {
        let _ = writeln!(out, "| `{}` | {} |", placeholder.token, placeholder.meaning);
    }
    out.push_str(
        "\nA synopsis is written the way a manual page writes one: `[…]` is optional, \
         `(a | b)` are alternatives you must choose between, and `…` marks something you may \
         repeat.\n\n\
         Descriptions carry citations — `(D3)`, `[PF:11]`, `(note N110)`. They point into \
         this project's design documents and implementation notes, they are provenance for \
         the sentence they sit in, and there is nothing for you to do about them.\n",
    );
    out
}

fn which_program() -> String {
    format!(
        "Two programs offer the same verbs, the same flags and the same `--json` documents. \
         They differ in what is behind them.\n\n\
         | Program | Use it when |\n|---|---|\n\
         | `{client}` | **the normal case.** It talks to `webcam-handler-daemon` over a Unix \
         socket. The daemon owns the cameras, so a browser tab, a person at a terminal and \
         this program can use one camera at the same time. |\n\
         | `{cli}` | there is no daemon and nothing else wants the camera. It opens the \
         device itself, one verb per run. |\n\n\
         Start the daemon with `webcam-handler-daemon`; it serves a socket under \
         `$XDG_RUNTIME_DIR` and needs no arguments. `{client}` finds it there. Add `--http` \
         to serve the browser client too — it prints a URL carrying a token, and it listens \
         on loopback only.\n\n\
         Every example below is written with `{client}`. Substitute `{cli}` if you are \
         running without a daemon; the words after the program name are the same, except \
         that `--backend` and `--profile` are `{cli}`'s alone.\n\n\
         Only one process may stream from a camera at a time. That is the kernel's rule, not \
         this tool's, and it is the whole reason to prefer the daemon: without one, two \
         programs that both want the camera take turns and one of them is refused `busy`.\n",
        cli = cli_core::Program::Cli.as_str(),
        client = cli_core::Program::Client.as_str(),
    )
}

/// The flags the root declares, which clap propagates to every verb.
fn global_options(root: &clap::Command) -> String {
    let mut out = String::from(
        "These are declared once and accepted by every verb, before or after it.\n\n\
         | Option | Value | Default | What it does |\n|---|---|---|---|\n",
    );
    for arg in root.get_arguments() {
        if arg.is_positional() {
            continue;
        }
        let _ = writeln!(out, "{}", option_row(arg));
    }
    let _ = write!(
        out,
        "\nExit codes: **0** the verb answered, **{}–{}** a typed failure — one code per \
         failure, listed in the section on failures — and **2** the command line was not a \
         command line. Code **1** is not used. Read the `--json` document rather than the \
         code alone: the code says which failure, the document says what to do about it.\n",
        cli_core::D13_EXIT_CODES.start(),
        cli_core::D13_EXIT_CODES.end(),
    );
    out
}

/// One row of an options table.
fn option_row(arg: &clap::Arg) -> String {
    let long = arg
        .get_long()
        .map(|long| format!("`--{long}`"))
        .unwrap_or_else(|| format!("`{}`", arg.get_id()));
    let short = arg
        .get_short()
        .map(|short| format!(", `-{short}`"))
        .unwrap_or_default();
    let value = value_names(arg)
        .map(|names| format!("`{names}`"))
        .unwrap_or_else(|| "—".to_owned());
    let default = defaults(arg)
        .map(|value| format!("`{value}`"))
        .unwrap_or_else(|| "—".to_owned());
    format!("| {long}{short} | {value} | {default} | {} |", help(arg))
}

/// The value names an argument takes, as one string, or `None` for a flag.
fn value_names(arg: &clap::Arg) -> Option<String> {
    if matches!(
        arg.get_action(),
        clap::ArgAction::SetTrue | clap::ArgAction::SetFalse | clap::ArgAction::Help
    ) {
        return None;
    }
    let names = arg.get_value_names()?;
    if names.is_empty() {
        return None;
    }
    Some(
        names
            .iter()
            .map(|name| format!("<{name}>"))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// The default an argument takes when it is left out, if it has one.
///
/// An **empty** default is rendered as `""` rather than as nothing: `calibrate start --goal`
/// defaults to the empty string, and a cell that showed it as blank would be indistinguishable
/// from a flag with no default at all — one of which you may leave out and one of which you
/// may not.
fn defaults(arg: &clap::Arg) -> Option<String> {
    let values = arg.get_default_values();
    if values.is_empty() {
        return None;
    }
    let rendered = values
        .iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    if rendered.is_empty() {
        return Some("\"\"".to_owned());
    }
    Some(rendered)
}

/// An argument's one-line description, as a table cell.
///
/// The doc comment's first line, through the same bracket unescape the JSON artifacts get,
/// with newlines flattened because a Markdown table row is one line. An argument with no
/// description is not silently blank: the em dash is visible, and the reader can see that the
/// surface has nothing to say about it.
fn help(arg: &clap::Arg) -> String {
    arg.get_help()
        .map(|help| cell(&help.to_string()))
        .unwrap_or_else(|| "—".to_owned())
}

/// Prose, flattened into one Markdown table cell.
fn cell(text: &str) -> String {
    let flattened = text.split_whitespace().collect::<Vec<_>>().join(" ");
    unescape_doc_brackets(&flattened).replace('|', "\\|")
}

/// Every verb, with what it takes and what it answers with.
fn verb_reference(root: &clap::Command) -> Result<String> {
    let mut out = String::new();
    for verb in cli_core::contracts::verbs() {
        let Some(command) = cli_core::contracts::find_verb(root, &verb) else {
            bail!("the command surface offers {verb} and the walk cannot resolve it");
        };
        let Some(document) = cli_core::contracts::json_contract(&verb) else {
            bail!(
                "the command surface offers {verb} and crates/cli-core/json-contracts.tsv \
                 names no document for it"
            );
        };

        let words = verb.replace('-', " ");
        let _ = writeln!(out, "### `{words}`\n");
        if let Some(about) = command.get_about() {
            let _ = writeln!(out, "{}\n", cell(&about.to_string()));
        }
        let _ = writeln!(out, "```text\n{}\n```\n", synopsis(&words, command));
        for rule in group_rules(command) {
            let _ = writeln!(out, "{rule}\n");
        }
        let _ = writeln!(out, "`--json` answers with one `{document}` document.\n");

        let positionals: Vec<&clap::Arg> = command
            .get_arguments()
            .filter(|a| a.is_positional())
            .collect();
        let options: Vec<&clap::Arg> = command
            .get_arguments()
            .filter(|a| !a.is_positional() && !a.is_global_set())
            .collect();
        if !positionals.is_empty() {
            out.push_str("| Argument | What it is |\n|---|---|\n");
            for arg in positionals {
                let _ = writeln!(
                    out,
                    "| `{}` | {} |",
                    value_names(arg).unwrap_or_else(|| arg.get_id().to_string()),
                    help(arg)
                );
            }
            out.push('\n');
        }
        if !options.is_empty() {
            out.push_str("| Option | Value | Default | What it does |\n|---|---|---|---|\n");
            for arg in options {
                let _ = writeln!(out, "{}", option_row(arg));
            }
            out.push('\n');
        }
    }
    Ok(out)
}

/// The usage line for one verb.
///
/// Built here rather than taken from `clap::Command::render_usage`, because that renders the
/// *leaf's* usage — `Usage: sweep [OPTIONS] <CONTROL>` — and a caller types the whole path
/// through the tree. Required arguments are bare, optional ones are bracketed, and a
/// repeatable one ends in `…`, which is the convention every manual page uses.
///
/// **Required groups are rendered as alternations**, and that is not cosmetic: `calibrate
/// sweep` requires exactly one of `--all`, `--step`, `--points` and `--values`, and clap says
/// so through an `ArgGroup` rather than through any member's own `required`. A synopsis that
/// asked each argument in turn would print all four as optional and teach a caller a command
/// line that exits 2 — which is the same defect class as an example naming a flag that does
/// not exist, arriving through the *derived* half of this document.
fn synopsis(words: &str, command: &clap::Command) -> String {
    let mut line = format!("{} {words}", cli_core::Program::Client.as_str());
    let mut spent: Vec<String> = Vec::new();
    for arg in command.get_arguments() {
        if arg.is_global_set() {
            continue;
        }
        let id = arg.get_id().to_string();
        if spent.contains(&id) {
            continue;
        }
        match group_of(command, &id) {
            Some(group) => {
                let members: Vec<String> = group
                    .get_args()
                    .map(std::string::ToString::to_string)
                    .collect();
                let rendered: Vec<String> = members
                    .iter()
                    .filter_map(|member| {
                        command
                            .get_arguments()
                            .find(|arg| arg.get_id().as_str() == member)
                            .map(piece)
                    })
                    .collect();
                spent.extend(members);
                let _ = write!(line, " ({})", rendered.join(" | "));
            }
            None => {
                let piece = piece(arg);
                if arg.is_required_set() {
                    let _ = write!(line, " {piece}");
                } else {
                    let _ = write!(line, " [{piece}]");
                }
            }
        }
    }
    line
}

/// The required group an argument belongs to, if it is in one.
///
/// Required only: clap's derive puts every `#[command(flatten)]` struct's arguments into a
/// group of its own whether or not the struct asked for one, so an unfiltered lookup would
/// render `<CAMERA>` as a one-member alternation.
fn group_of<'a>(command: &'a clap::Command, id: &str) -> Option<&'a clap::ArgGroup> {
    command
        .get_groups()
        .filter(|group| group.is_required_set())
        .find(|group| group.get_args().any(|arg| arg.as_str() == id))
}

/// One argument as it appears in a synopsis, without its brackets.
///
/// A value name that already ends in an ellipsis keeps the one it has: `--values <V,V,…>` is
/// a list *in one occurrence* and is also repeatable, and `<V,V,…>…` spends a second symbol
/// on a distinction a reader of a usage line cannot act on.
fn piece(arg: &clap::Arg) -> String {
    let repeatable = matches!(arg.get_action(), clap::ArgAction::Append);
    let already = value_names(arg).is_some_and(|names| names.contains('…'));
    let repeat = if repeatable && !already { "…" } else { "" };
    match (arg.get_long(), value_names(arg)) {
        (Some(long), Some(value)) => format!("--{long} {value}{repeat}"),
        (Some(long), None) => format!("--{long}"),
        (None, Some(value)) => format!("{value}{repeat}"),
        (None, None) => arg.get_id().to_string(),
    }
}

/// The sentence a required group needs under the synopsis, if the verb has any.
///
/// The alternation in the usage line says *which* arguments are alternatives; this says
/// whether one of them is enough and whether more than one is allowed — two facts clap holds
/// as `required` and `multiple` on the group, and neither is visible in a `(a | b)`.
fn group_rules(command: &clap::Command) -> Vec<String> {
    let mut rules = Vec::new();
    for group in command.get_groups().filter(|g| g.is_required_set()) {
        let members: Vec<String> = group
            .get_args()
            .filter_map(|id| {
                command
                    .get_arguments()
                    .find(|arg| arg.get_id() == id)
                    .map(|arg| match arg.get_long() {
                        Some(long) => format!("`--{long}`"),
                        None => format!("`{}`", arg.get_id()),
                    })
            })
            .collect();
        if members.len() < 2 {
            continue;
        }
        // `ArgGroup::is_multiple` takes `&mut self` — a quirk of clap's builder, not a fact
        // about the group — and `get_groups` yields shared references, so the question is
        // asked of a copy.
        let how = if group.clone().is_multiple() {
            "At least one"
        } else {
            "Exactly one"
        };
        rules.push(format!("{how} of {} is required.", members.join(", ")));
    }
    rules
}

/// The closed vocabularies a flag's value comes from.
///
/// **A manual that names a flag and not its words is a manual a caller cannot act on.** clap's
/// help says `--metric <METRIC>` and stops there, which is fine for a person who will try one
/// and read the refusal — the refusal does list the set — and useless for the consumer AGENTS
/// puts first, who has one shot and no hands. Every one of these is a `closed_vocabulary!`
/// with a generated `ALL`, so the sets are walked rather than typed and a variant added to any
/// of them appears here in the same commit (rubric rule 6).
///
/// `--pixel-format` is deliberately absent: a fourcc is four bytes the *device* names, not a
/// vocabulary this build closes, so the honest answer is the one `info` gives for the camera
/// in front of you.
fn vocabularies() -> String {
    use schema::backend::BackendKind;
    use schema::capture::{PhotoFormat, Transform};
    use schema::metrics::MetricName;
    use schema::session::ChosenBy;

    let mut out = String::from(
        "These flags take a word from a closed set. A word outside the set is refused while \
         the command line is being parsed — by name, with the set listed — so anything here is \
         a word this build accepts, and there is nothing else to try.\n\n\
         | Flag | Words |\n|---|---|\n",
    );
    let _ = writeln!(
        out,
        "| `--backend` | {} |",
        words(BackendKind::ALL.iter().map(|kind| kind.as_str()))
    );
    let _ = writeln!(
        out,
        "| `--transform` | {} |",
        words(Transform::ALL.iter().map(|transform| transform.as_str()))
    );
    let _ = writeln!(
        out,
        "| `--format`, `--photo-format` | {} |",
        words(PhotoFormat::ALL.iter().map(|format| format.as_str()))
    );
    let _ = writeln!(
        out,
        "| `--metric` | {} |",
        words(MetricName::ALL.iter().map(|metric| metric.as_str()))
    );
    let _ = writeln!(
        out,
        "| `--by` | {} |",
        words(
            ChosenBy::ALL
                .iter()
                .map(|chooser| chooser.selector().label())
        )
    );
    out
}

/// A vocabulary as one table cell.
fn words<I, S>(items: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    items
        .into_iter()
        .map(|word| format!("`{}`", word.as_ref()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The verb-to-document table, and the sentence that says where the shapes are.
fn json_contracts() -> Result<String> {
    let mut out = String::from(
        "`--json` prints exactly one document and nothing else: no envelope, no timestamp, \
         no tool version. The shape of each is in `schemas/webcam-handler-schema.json`, under \
         `$defs`, which is committed and validated in CI.\n\n\
         | Verb | Document |\n|---|---|\n",
    );
    let mut rows = 0;
    for verb in cli_core::contracts::verbs() {
        let Some(document) = cli_core::contracts::json_contract(&verb) else {
            bail!("{verb} has no `--json` contract");
        };
        let _ = writeln!(
            out,
            "| `{}` | [`{document}`](../schemas/webcam-handler-schema.json) |",
            verb.replace('-', " ")
        );
        rows += 1;
    }
    if rows == 0 {
        bail!("the command surface offers no verbs; the guide would document nothing");
    }
    out.push_str(
        "\nTwo verbs write a document whether or not you pass `--json`: `snapshot` and \
         `profile capture` exist to produce one. Both take `-o <PATH>`, and print to standard \
         output when you leave it out.\n\n\
         `photo --json` requires `-o <PHOTO>`: with no path the photo's bytes are standard \
         output, and the document cannot share it.\n\n",
    );
    out.push_str(&failure_document());
    Ok(out)
}

/// What a failing `--json` run prints, and how to branch on it.
///
/// **The example is serialized rather than typed**, from `Error::sample` — the registry's own
/// walkable population, the same value the OpenRPC document carries as this variant's `data`.
/// A hand-written block here would be the shape a reader trusts and the one nothing checks; a
/// generated one moves when the document moves, and `agent-guide-current.sh` refuses the
/// committed file until it does.
///
/// `FormatUnsupported` is the variant shown because it is the one an unattended caller acts on
/// most directly: `available` is the retry, and it is exactly what the old behaviour — one
/// English sentence on standard error, note **N124** — made the reader parse prose to find.
fn failure_document() -> String {
    let sample = Failure::new(Error::sample(ErrorKind::FormatUnsupported));
    let rendered = serde_json::to_string_pretty(&sample).unwrap_or_else(|_| "{}".to_owned());
    format!(
        "**A failure answers too.** When a verb refuses, `--json` prints one document on \
         standard output — this one, and never a verb's own answer:\n\n\
         ```json\n{rendered}\n```\n\n\
         Branch on it:\n\n\
         | Read | To find out |\n|---|---|\n\
         | `{marker}` | that the verb refused. It is `true` in every failure document and no \
         answer carries the field. |\n\
         | `error.kind` | which refusal. The words are the first column of the next section. |\n\
         | the rest of `error` | what to do about it: `available` here, `holders` for `busy`, \
         `path` for `storage_io`. |\n\
         | `message` | the same sentence a person would have read. |\n\n\
         The same line also goes to standard error, prefixed with the program's name, and the \
         process exits with the code the next section gives that failure. **The document is \
         what to act on**; the exit code is a second, coarser copy of `error.kind` for a \
         caller with no JSON parser. Without `--json` there is no document — standard error \
         and the exit code are the whole answer.\n\n\
         Through the daemon over JSON-RPC the same failure arrives as an error object whose \
         `data` is the `error` field above, byte for byte.\n",
        marker = schema::error::FAILURE_MARKER,
    )
}

/// What an agent should do about each D13 variant.
///
/// An exhaustive `match`, which is the point: the registry is closed at eighteen and
/// `ErrorKind::ALL` is generated from the vocabulary macro, so a nineteenth variant does not
/// compile until somebody has decided what an unattended caller is supposed to do about it.
/// AGENTS states the requirement in one sentence — *"`Busy` means retry, `DeviceGone` means
/// stop and tell the human, `PermissionDenied` means a setup problem — collapsing them makes
/// the agent guess"* — and this function is where that sentence is spent.
fn disposition(kind: ErrorKind) -> (&'static str, String) {
    let (verb, meaning) = disposition_text(kind);
    match kind {
        // **The one row that names a number, and it is derived** (note **N147**). The advice
        // below tells a reader to retry with a longer deadline, and until the bound was
        // written beside it that instruction had no ceiling — an unattended reader following
        // it past the cap meets `illegal_transition` instead, which its own row answers with
        // "fix the request" and no idea which part. The figure comes out of
        // `schema::limits::MAX_SETTLE_DEADLINE_MS` rather than being typed here, so a guide
        // that advertised a ceiling this build does not have cannot be committed.
        ErrorKind::SettleTimeout => (
            verb,
            format!(
                "{meaning} The deadline has a ceiling of its own: {} ms is the most one \
                 capture may hold a camera, and a larger `--settle-deadline` is refused as \
                 `illegal_transition` rather than quietly shortened.",
                schema::limits::MAX_SETTLE_DEADLINE_MS
            ),
        ),
        _ => (verb, meaning.to_owned()),
    }
}

/// [`disposition`]'s table: literal prose, one row per D13 variant.
fn disposition_text(kind: ErrorKind) -> (&'static str, &'static str) {
    match kind {
        ErrorKind::DeviceGone => (
            "stop",
            "The camera is gone — unplugged, or its driver unbound. Nothing you can do will \
             bring it back. Stop and tell the human.",
        ),
        ErrorKind::Busy => (
            "retry",
            "Another process is streaming from the camera. Retry; under \
             `webcam-handler-client`, `--wait` asks the daemon to queue you instead of \
             refusing. The holders it could see are in the message.",
        ),
        ErrorKind::PermissionDenied => (
            "fix the setup",
            "The device node is there and this user may not open it. The message carries the \
             remedy — usually joining the `video` group and logging back in. Retrying changes \
             nothing.",
        ),
        ErrorKind::CameraUnknown => (
            "fix the request",
            "No camera answers to that id. Run `list` and use an id it printed; ids come from \
             what the device says about itself, not from `/dev/video0`.",
        ),
        ErrorKind::CameraAmbiguous => (
            "fix the request",
            "The prefix matched several cameras. The message names them; use a longer prefix \
             or a whole id.",
        ),
        ErrorKind::ControlUnknown => (
            "fix the request",
            "This camera has no such control. The message carries the closest slugs it does \
             have, and `controls <CAMERA>` lists all of them.",
        ),
        ErrorKind::ControlReadOnly => (
            "do not retry",
            "The device says this control cannot be written — `privacy` on a camera with a \
             hardware shutter, for instance. This is the camera's answer about itself, not a \
             temporary state.",
        ),
        ErrorKind::ControlInactive => (
            "change the plan",
            "An automation control currently owns this one, and the message names the \
             automation. Write without `--no-guard` and the write turns it off first; or turn \
             it off yourself and write again.",
        ),
        ErrorKind::FormatUnsupported => (
            "fix the request",
            "The camera cannot deliver what was asked for, and the payload says which half. \
             When `size` is present the frame size is the problem: `size.requested_width` \
             and `size.requested_height` are what you asked for and `size.available` lists \
             every size this camera can deliver — pick one, or leave `--size` out and let \
             the camera choose. Otherwise the format is the problem: `requested` is what you \
             asked for and `available` lists what would be taken — either a \
             `--pixel-format` this device does not offer, or a recording container that \
             cannot carry what this camera produces, and a `.avi` needs MJPEG frames so a \
             camera that delivers raw ones records to `.y4m`. The two never both appear: one \
             refusal names one lever.",
        ),
        ErrorKind::SettleTimeout => (
            "retry once, then stop",
            "The camera did not deliver enough frames inside the settle deadline. Retry with a \
             longer `--settle-deadline` or fewer `--skip-frames`. If it repeats, the device is \
             not delivering and that is worth telling the human.",
        ),
        ErrorKind::FingerprintMismatch => (
            "stop",
            "The snapshot or session you named was recorded against a different camera, and \
             the message names the fields that differ. Do not apply it here — the values would \
             mean something else on this device.",
        ),
        ErrorKind::SessionConflict => (
            "change the plan",
            "This camera and task already have an open calibration session. Use it — \
             `calibrate status` says where it got to — or start one under a different task \
             name.",
        ),
        ErrorKind::IllegalTransition => (
            "fix the request",
            "The verb does not apply in the state the session or the request is in — \
             selecting a value for a control that never swept, an output extension this build \
             does not write. `calibrate status` says what state a session is in; the message \
             says what was refused.",
        ),
        ErrorKind::SchemaVersionForeign => (
            "stop",
            "A different build of this tool wrote the document. Do not edit it into shape; \
             run the build that wrote it, or start a new session.",
        ),
        ErrorKind::StoreLocked => (
            "read the message, then retry or switch",
            "Something else holds the state directory. The message says which protocol: a \
             lock held for a process's lifetime is a running daemon, so use \
             `webcam-handler-client` rather than waiting; a lock held for one operation will \
             be free shortly, so retry.",
        ),
        ErrorKind::HolderGone => (
            "retry",
            "The process that was holding the camera has exited since it was named. Ask \
             again.",
        ),
        ErrorKind::DeviceIo => (
            "retry once, then stop",
            "The driver refused an operation, and the message names the operation and the \
             `errno`. One retry is reasonable; a repeat is a fact about this device and \
             belongs in front of the human.",
        ),
        ErrorKind::StorageIo => (
            "fix the setup",
            "The filesystem refused — no such directory, no room, no permission. The message \
             names the path. This is not the camera's fault and retrying the capture will not \
             help.",
        ),
    }
}

/// The D13 table: every variant, its wire code, an example message and what to do.
fn errors() -> Result<String> {
    let mut out = String::from(
        "A failed verb writes one line to standard error, prints the failure document under \
         `--json` (previous section), and exits with the code in the `Exit` column. Every \
         failure this tool can produce is in this table — there are eighteen, they are a \
         closed set, and they are kept apart on purpose: `busy` and `device_gone` want \
         opposite responses from you.\n\n",
    );
    out.push_str("| Failure | Exit | Do | What it means |\n|---|---|---|---|\n");
    let mut rows = 0;
    for &kind in ErrorKind::ALL {
        let name = error_component_name(kind)?;
        let (verb, meaning) = disposition(kind);
        let _ = writeln!(
            out,
            "| `{name}` | `{}` | **{verb}** | {} |",
            cli_core::exit_code(&Error::sample(kind)),
            cell(&meaning)
        );
        rows += 1;
    }
    if rows != ErrorKind::ALL.len() {
        bail!(
            "the failure table has {rows} rows over {} kinds",
            ErrorKind::ALL.len()
        );
    }
    out.push_str(
        "\nThrough the daemon the same failures arrive as JSON-RPC errors, with a code per \
         kind and the typed error as `data`. The whole registry is in \
         `schemas/webcam-handler-openrpc.json` under `components/errors`; the codes and one \
         example message each are here:\n\n",
    );
    out.push_str("| Failure | JSON-RPC code | Example message |\n|---|---|---|\n");
    for &kind in ErrorKind::ALL {
        let name = error_component_name(kind)?;
        let _ = writeln!(
            out,
            "| `{name}` | `{}` | {} |",
            api::rpc_code(kind),
            cell(&Error::sample(kind).to_string())
        );
    }
    Ok(out)
}

fn photograph_recipe() -> String {
    format!(
        "One call takes a photograph, settles the sensor first, and tells you what the camera \
         actually delivered.\n\n\
         ```console\n\
         $ {client} photo <CAMERA> -o <PHOTO> --json\n\
         ```\n\n\
         The document says where the file went, what the camera negotiated, and what was done \
         to the frame. Read `negotiated` rather than assuming: a camera that cannot deliver \
         what you asked for delivers something else and says so.\n\n\
         Two things decide whether two photographs an hour apart are comparable, which is what \
         this tool is for.\n\n\
         **Settle before the shot.** A webcam's auto-exposure and auto-focus need frames to \
         converge, and the first frame after `STREAMON` is not the picture. The default \
         discards ten frames; `--skip-frames` and `--settle-for` set it yourself, and \
         `--settle-deadline` bounds the whole thing so a camera that never settles fails \
         instead of hanging. That deadline may be at most {settle_cap} ms — one camera is one \
         thread, so a settle is time nothing else on that camera gets — and a larger one is \
         refused as `illegal_transition` rather than quietly shortened.\n\n\
         ```console\n\
         $ {client} photo <CAMERA> -o <PHOTO> --skip-frames 20 --settle-deadline 8000\n\
         ```\n\n\
         **Ask for a size and a format, or accept the camera's choice — and an explicit \
         request is answered or refused, never quietly replaced.** A `--pixel-format` this \
         camera does not enumerate is `format_unsupported` naming what it does have. A \
         `--size` no mode can deliver is `format_unsupported` too, and its `size` field \
         names the size you asked for and every size this camera can deliver. A size the \
         camera can *fit inside* is answered with the largest mode that does, and the \
         difference is in `negotiated` and in `adjustments` — so read `negotiated` rather \
         than assuming you got the number you typed.\n\n\
         ```console\n\
         $ {client} photo <CAMERA> -o <PHOTO> --size 1280x720 --pixel-format MJPG --json\n\
         ```\n\n\
         With no `-o`, the photo's bytes are standard output — which is why `--json` requires \
         a path.\n",
        client = cli_core::Program::Client.as_str(),
        // Derived rather than typed, for the reason `disposition`'s `settle_timeout` row is
        // (note **N147**): this sentence tells an unattended reader what to fit inside, and a
        // number that drifted from `schema::limits` would be an instruction to keep asking
        // for something the tool refuses.
        settle_cap = schema::limits::MAX_SETTLE_DEADLINE_MS,
    )
}

fn record_recipe() -> String {
    format!(
        "Recording answers the question a photograph cannot: whether an animation or a \
         transition on the device under test did what it was supposed to.\n\n\
         ```console\n\
         $ {client} record <CAMERA> -o <RECORDING> --duration 2s --json\n\
         ```\n\n\
         `-o` is required and its extension chooses the container: `.avi` carries the \
         camera's own MJPEG frames, `.y4m` carries raw ones, and a path with no extension \
         lets the camera's negotiated format decide. A camera that cannot deliver MJPEG is \
         refused an `.avi` by name, listing what it can deliver — record it to `.y4m` \
         instead. There is no standard-output spelling: a recording goes to a path and \
         never comes back in the answer.\n\n\
         **The path is written by whatever holds the camera.** Under \
         `webcam-handler-client` that is the daemon, so `-o` must name somewhere the daemon \
         can write; a relative path is resolved against your working directory before the \
         request is sent, so the two programs put the file in the same place. The same is \
         true of `photo -o`.\n\n\
         `--duration` takes `10s`, `1500ms` or `1m30s`; leave it out for {default_ms} ms. \
         Longer than {max_ms} ms is refused rather than quietly shortened — a caller that \
         asked for five minutes and silently got two cannot tell that from a camera that \
         stopped.\n\n\
         The report counts the frames the file holds and the mean interval between them. \
         **Frame timing is a payload, not a footnote**: a webcam delivers frames when it \
         delivers them, so the interval the file declares is the one this take measured, and \
         it is in the answer for you to read.\n",
        client = cli_core::Program::Client.as_str(),
        default_ms = schema::limits::DEFAULT_RECORDING_MS,
        max_ms = schema::limits::MAX_RECORDING_MS,
    )
}

fn snapshot_recipe() -> String {
    format!(
        "Any write to a camera outlives your process: the next photograph anybody takes gets \
         the settings you left. Record them before you write, put them back when you are \
         done.\n\n\
         ```console\n\
         $ {client} snapshot <CAMERA> -o <SNAPSHOT>\n\
         $ {client} set <CAMERA> <CONTROL>=<VALUE>\n\
         $ {client} restore <CAMERA> <SNAPSHOT>\n\
         ```\n\n\
         `snapshot` records every writable control's current value. `restore` puts them back \
         automation first, and answers with a report saying what each control ended up at — a \
         control the device would not take back is in the report, not an error, so read it \
         rather than trusting the exit code alone.\n\n\
         Writes are guarded by default: setting a control whose automation owns it turns the \
         automation off first, because a manual value written under a live automation is \
         overwritten on the next frame. `--no-guard` writes anyway, and the read-back shows \
         you what actually stuck.\n\n\
         ```console\n\
         $ {client} controls <CAMERA> --json\n\
         $ {client} get <CAMERA> <CONTROL>\n\
         ```\n\n\
         `controls` is the whole model of a camera: every control, its range or its menu, its \
         flags, and which automation owns which manual control. Every write answers with \
         `requested` and `applied` side by side, because drivers clamp silently — what you \
         asked for and what the camera took are two facts and this tool never conflates \
         them.\n\n\
         When you need everything a camera says about itself in one document — every format, \
         every size, every frame rate, every control and every menu item — capture a \
         profile:\n\n\
         ```console\n\
         $ {client} profile capture <CAMERA> -o <PROFILE>\n\
         ```\n\n\
         That is the document to attach when reporting something a camera did, and the one \
         this project's own tests replay in place of hardware.\n",
        client = cli_core::Program::Client.as_str()
    )
}

fn calibration_walkthrough() -> String {
    format!(
        "Calibration answers *what settings make this camera see this thing well?* by taking \
         photographs and scoring them, not by guessing. A session is a durable record on \
         disk: it survives a crash, and you can come back to it.\n\n\
         Every step below names the session by the task you gave it. Run them in order.\n\n\
         **1. Open a session.** The task is how you find it again; the goal and the criteria \
         are for whoever chooses a value later — which may be you, reading the photographs.\n\n\
         ```console\n\
         $ {client} calibrate start <CAMERA> --task <TASK> --goal \"the display of the device \
         under test is legible\" --criterion \"text edges are sharp\"\n\
         ```\n\n\
         Opening a session probes the camera's automation pairs, which **writes to the device \
         and puts it back**. It also records the camera's state, so there is a way back from \
         everything the session does afterwards.\n\n\
         **2. Draft the queue.** Name the controls to calibrate, in the order to do them. \
         Name none and every control is classified: the sweepable ones queued, the rest \
         recorded as blocked with the device's own reason.\n\n\
         ```console\n\
         $ {client} calibrate plan <CAMERA> --task <TASK> <CONTROL>\n\
         ```\n\n\
         **3. Sweep.** A photograph per value, each one scored. `--points 3` samples three \
         values across the control's range; `--all`, `--step` and `--values` say exactly which \
         instead. One of the four is required — a sweep is minutes of camera time and, on a \
         pan/tilt/zoom head, motor travel, so the expensive choice is never the silent one.\n\n\
         ```console\n\
         $ {client} calibrate sweep <CAMERA> --task <TASK> <CONTROL> --points 3\n\
         ```\n\n\
         A sweep that would move motors refuses unless you pass `--allow-motion`. Motors \
         wear out.\n\n\
         **4. Read what happened.** Every sample, its value, its score, and how the sweep \
         ended.\n\n\
         ```console\n\
         $ {client} calibrate status <CAMERA> --task <TASK> --json\n\
         ```\n\n\
         **5. Choose.** Either let a metric rank the samples, or name the value yourself and \
         say who chose it. Both are recorded — a metric cannot know what \"the text is \
         legible\" means, and the record says whether one was asked to.\n\n\
         ```console\n\
         $ {client} calibrate select <CAMERA> --task <TASK> <CONTROL> --metric sharpness\n\
         ```\n\n\
         **6. Apply.** The chosen values are written to the camera, automation first.\n\n\
         ```console\n\
         $ {client} calibrate apply <CAMERA> --task <TASK>\n\
         ```\n\n\
         **7. Give the camera back.** The sweep left the camera holding its last swept value. \
         This puts back what the session found, and spends the record — running it twice is \
         not an error, it says there was nothing left to put back.\n\n\
         ```console\n\
         $ {client} calibrate restore <CAMERA> --task <TASK>\n\
         ```\n\n\
         `calibrate list` shows every session on this machine, newest first, if you have \
         forgotten what a task was called.\n\n\
         ```console\n\
         $ {client} calibrate list --json\n\
         ```\n",
        client = cli_core::Program::Client.as_str()
    )
}

/// Design §1.1's map, as the guide's reader meets it.
///
/// The left column is what the vendored skill teaches by hand; the right column is the verb
/// that replaces the sequence. The verb names are checked —
/// `every_verb_the_operations_map_names_is_a_verb_the_surface_offers` walks this table
/// against `cli_core::contracts::verbs` — so this prose cannot promise an operation the
/// surface does not have.
const OPERATIONS_MAP: &[(&str, &str, &[&str])] = &[
    (
        "`v4l2-ctl --list-devices`, then `--info` per node to find the capture node",
        "`list`",
        &["list"],
    ),
    ("`v4l2-ctl --list-formats-ext`", "`info`", &["info"]),
    ("`v4l2-ctl --list-ctrls-menus`", "`controls`", &["controls"]),
    (
        "`v4l2-ctl --get-ctrl` / `--set-ctrl`",
        "`get` / `set`, with the read-back beside the request",
        &["get", "set"],
    ),
    (
        "find the auto/manual pairs, switch the automation off, set the manual control, put \
         it all back",
        "`set` (guarded by default), and `snapshot` / `restore`",
        &["set", "snapshot", "restore"],
    ),
    (
        "`ffmpeg` one-frame capture with a `-ss` settle",
        "`photo`, with a settle policy that is a flag rather than a guess",
        &["photo"],
    ),
    (
        "`ffmpeg -vf hflip/vflip/transpose`",
        "`photo --transform`",
        &["photo"],
    ),
    ("`ffmpeg -t` video capture", "`record`", &["record"]),
    (
        "`draft-calibration.sh`, `uniform-sampling.sh`, and tracking which control is done",
        "`calibrate start` / `plan` / `sweep` / `status` / `select` / `apply` / `list`",
        &[
            "calibrate-start",
            "calibrate-plan",
            "calibrate-sweep",
            "calibrate-status",
            "calibrate-select",
            "calibrate-apply",
            "calibrate-list",
        ],
    ),
    (
        "`fuser` to find who holds the camera, `fuser --kill` to take it",
        "the `busy` failure names the holders. There is no verb here that kills one — the \
         daemon exposes that on its JSON-RPC surface, because killing somebody's process is \
         a command you give on purpose and never a fallback",
        &[],
    ),
    (
        "`lsusb` to work out why a camera is missing",
        "`list` answers with the hints it could diagnose, including the driverless-USB-camera \
         case",
        &["list"],
    ),
];

fn operations_map(root: &clap::Command) -> Result<String> {
    let mut out = String::from(
        "`vendor/v4l2-webcam-skill/` teaches these operations as sequences of `v4l2-ctl` and \
         `ffmpeg` commands. Each row is one of them and the call that replaces it. Nothing \
         here shells out: this tool talks to the kernel directly, so there is no `ffmpeg` to \
         install and no output to parse.\n\n\
         | By hand | Here |\n|---|---|\n",
    );
    for (manual, tool, verbs) in OPERATIONS_MAP {
        for verb in *verbs {
            if cli_core::contracts::find_verb(root, verb).is_none() {
                bail!("the operations map names {verb}, which the command surface does not offer");
            }
        }
        let _ = writeln!(out, "| {manual} | {tool} |");
    }
    out.push_str(
        "\nThe skill remains readable for the V4L2 background it explains. It is not the \
         way to drive this hardware any more.\n",
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The emitted guide, for a test that asks it a question.
    fn emitted() -> String {
        guide().expect("the guide is emitted")
    }

    /// Every command an example shows, in document order.
    ///
    /// The extraction contract, and it is the one `crates/cli/tests/agent_guide.rs` uses
    /// against the *committed* file: a fenced block tagged `console`, one command per line,
    /// each line beginning with `$ `. Anything else in the document — a `text` block holding
    /// a synopsis, an inline `code span` — is not an example and is not run.
    fn examples(document: &str) -> Vec<String> {
        let mut found = Vec::new();
        let mut inside = false;
        for line in document.lines() {
            if line.trim_start().starts_with("```") {
                inside = line.trim() == "```console";
                continue;
            }
            if inside && let Some(command) = line.trim_start().strip_prefix("$ ") {
                found.push(command.trim().to_owned());
            }
        }
        found
    }

    #[test]
    fn every_verb_the_command_surface_offers_is_documented_with_the_document_it_answers_with() {
        // The claim the whole generator exists for: a verb reachable from a command line and
        // absent from the manual is an operation an unattended caller cannot find. Both
        // halves are asserted, because a heading with no contract beside it teaches the verb
        // and hides the answer.
        let document = emitted();
        for verb in cli_core::contracts::verbs() {
            let words = verb.replace('-', " ");
            assert!(
                document.contains(&format!("### `{words}`")),
                "{verb} has no section in the guide"
            );
            let contract =
                cli_core::contracts::json_contract(&verb).expect("every verb has a contract");
            assert!(
                document.contains(&format!("| `{words}` | [`{contract}`]")),
                "{verb} is not in the `--json` contract table as {contract}"
            );
        }
    }

    #[test]
    fn every_d13_variant_reaches_the_failure_table_with_both_codes_the_registry_gives_it() {
        // `ErrorKind::ALL` is generated by the vocabulary macro and `disposition` is an
        // exhaustive match over it, so a nineteenth variant stops this build before it can
        // reach a reader undocumented. This is the other half: that every variant the
        // registry has actually reaches the *rendered* tables, with both of the numbers a
        // caller may meet — a renderer that dropped a row would compile perfectly.
        //
        // **The exit column is why this document is where the exit codes are pinned** (note
        // **N127**). A wire code has `crates/api/fixtures/d13-rpc-codes.tsv`; an exit code has
        // this table, because the guide is regenerated and diffed by
        // `scripts/gates/agent-guide-current.sh` on every run — so changing one is a committed
        // diff a human reads rather than a constant that moved under a script.
        let document = emitted();
        assert_eq!(ErrorKind::ALL.len(), 18, "the D13 registry changed size");
        for &kind in ErrorKind::ALL {
            let name = error_component_name(kind).expect("a kind names itself");
            let (verb, _) = disposition(kind);
            let exit = cli_core::exit_code(&Error::sample(kind));
            assert!(
                document.contains(&format!("| `{name}` | `{exit}` | **{verb}** |")),
                "{name} has no row in the failure table carrying its exit code and disposition"
            );
            assert!(
                document.contains(&format!("| `{name}` | `{}` |", api::rpc_code(kind))),
                "{name} does not carry the JSON-RPC code the registry gives it"
            );
        }

        // The band the options table announces is the band the rows use. Two statements of
        // one fact in one document is how a reader ends up trusting the wrong one.
        assert!(
            document.contains(&format!(
                "**{}–{}** a typed failure",
                cli_core::D13_EXIT_CODES.start(),
                cli_core::D13_EXIT_CODES.end()
            )),
            "the options table no longer announces the exit-code band the failure table uses"
        );
    }

    #[test]
    fn the_guide_shows_the_failure_document_the_binaries_actually_emit() {
        // **The manual and the product, checked against each other** (note **N127**). The
        // shape is serialized here from `Error::sample`, so this asserts that what was
        // serialized reached the page and that the page teaches the marker a caller branches
        // on — the sentence a reader acts on before parsing anything else.
        //
        // The other end of the same claim is `crates/cli/tests/failure_document.rs`, which
        // runs the shipped binary and compares its standard output against this block. Two
        // ends, because a generator that emitted a plausible document nothing prints would
        // satisfy this one alone.
        let document = emitted();
        let sample = Failure::new(Error::sample(ErrorKind::FormatUnsupported));
        let rendered = serde_json::to_string_pretty(&sample).expect("the sample serializes");
        assert!(
            document.contains(&rendered),
            "the guide no longer shows the failure document it is generated from:\n{rendered}"
        );
        assert!(
            document.contains(&format!("| `{}` |", schema::error::FAILURE_MARKER)),
            "the guide does not tell a reader which field says a verb refused"
        );
        // And it no longer says the opposite. This sentence was true until this change and
        // was pinned by a test of its own; a guide carrying both would be a manual arguing
        // with itself.
        assert!(
            !document.contains("A failure prints no document."),
            "the guide still claims a failure prints no document"
        );
    }

    #[test]
    fn every_verb_the_operations_map_names_is_a_verb_the_surface_offers() {
        // The map is prose — the left column is what the vendored skill teaches, and no
        // walk of this workspace can derive it — so the half that *can* be checked is
        // checked: a verb renamed under a row that still names the old spelling would send
        // a reader to a command that exits 2.
        let root = cli_core::contracts::command_tree(cli_core::Program::Cli);
        let verbs = cli_core::contracts::verbs();
        let mut named = 0;
        for (_, _, row) in OPERATIONS_MAP {
            for verb in *row {
                assert!(
                    verbs.iter().any(|offered| offered == verb),
                    "the operations map names {verb}, which the surface does not offer"
                );
                assert!(cli_core::contracts::find_verb(&root, verb).is_some());
                named += 1;
            }
        }
        assert!(named > 5, "the operations map names {named} verb(s)");
    }

    #[test]
    fn every_example_the_guide_shows_names_a_program_this_workspace_builds() {
        // The smoke test in `crates/cli` runs these against the built binary; this is the
        // cheap half that runs everywhere and catches the shape of the mistake first — an
        // example whose first word is a program nobody ships.
        let document = emitted();
        let examples = examples(&document);
        assert!(
            examples.len() > 10,
            "the guide shows {} example(s)",
            examples.len()
        );
        for example in &examples {
            let program = example.split_whitespace().next().unwrap_or_default();
            assert!(
                cli_core::Program::ALL
                    .iter()
                    .any(|known| known.as_str() == program),
                "the example {example:?} runs {program:?}, which is not one of this \
                 workspace's command-line programs"
            );
        }
    }

    #[test]
    fn every_example_the_guide_shows_parses_as_a_command_line() {
        // The examples are written prose, and this is what stops them being written *wrong*:
        // each one is fed to the real parser, so a flag that was renamed, a value the
        // vocabulary does not have, or a required argument left out fails here rather than
        // in front of the reader. The tokens are placeholders, so they are substituted
        // first — with values whose *shape* is right, which is all the parser judges.
        for example in examples(&emitted()) {
            let mut argv: Vec<String> = Vec::new();
            for word in shell_words(&example) {
                argv.push(substitute(&word));
            }
            let program =
                if argv.first().map(String::as_str) == Some(cli_core::Program::Cli.as_str()) {
                    cli_core::Program::Cli
                } else {
                    cli_core::Program::Client
                };
            cli_core::Cli::try_parse_checked_from(program, &argv).unwrap_or_else(|error| {
                panic!(
                    "the guide shows `{example}`, which is not a \
                                                command line this build accepts:\n{error}"
                )
            });
        }
    }

    #[test]
    fn every_placeholder_the_examples_use_is_documented() {
        // A token with no row in the placeholder table is a reader shown `<thing>` and left
        // to guess what to put there.
        let document = emitted();
        for example in examples(&document) {
            let mut rest = example.as_str();
            while let Some(open) = rest.find('<') {
                let Some(close) = rest[open..].find('>') else {
                    break;
                };
                let token = &rest[open..=open + close];
                assert!(
                    PLACEHOLDERS
                        .iter()
                        .any(|placeholder| placeholder.token == token),
                    "the example {example:?} uses {token}, which the placeholder table does \
                     not explain"
                );
                rest = &rest[open + close + 1..];
            }
        }
    }

    #[test]
    fn every_placeholder_the_table_documents_is_used_by_an_example() {
        // The other direction of the check above, and it is not symmetry for its own sake: a
        // row explaining a token no example shows is a reader told to substitute something
        // they will never meet, which is the first sign that a recipe was deleted and its
        // vocabulary left behind.
        let examples = examples(&emitted());
        for placeholder in PLACEHOLDERS {
            assert!(
                examples
                    .iter()
                    .any(|example| example.contains(placeholder.token)),
                "the placeholder table explains {}, and no example uses it",
                placeholder.token
            );
        }
    }

    #[test]
    fn every_word_a_closed_vocabulary_holds_reaches_the_guide() {
        // The section exists because clap's help says `--metric <METRIC>` and stops. It is
        // worth only as much as its completeness: a set printed with one variant missing is
        // worse than no set at all, because a caller would believe it.
        let rendered = vocabularies();
        let mut counted = 0;
        for word in schema::backend::BackendKind::ALL
            .iter()
            .map(|kind| kind.as_str().to_owned())
            .chain(
                schema::capture::Transform::ALL
                    .iter()
                    .map(|transform| transform.as_str().to_owned()),
            )
            .chain(
                schema::capture::PhotoFormat::ALL
                    .iter()
                    .map(|format| format.as_str().to_owned()),
            )
            .chain(
                schema::metrics::MetricName::ALL
                    .iter()
                    .map(|metric| metric.as_str().to_owned()),
            )
            .chain(
                schema::session::ChosenBy::ALL
                    .iter()
                    .map(|chooser| chooser.selector().label()),
            )
        {
            assert!(
                rendered.contains(&format!("`{word}`")),
                "the vocabulary table does not carry {word}"
            );
            counted += 1;
        }
        assert!(counted > 8, "{counted} word(s) walked");
        assert!(emitted().contains(rendered.trim_end()));
    }

    #[test]
    fn the_guide_is_byte_identical_across_two_runs() {
        // The freshness gate regenerates and diffs, so a generator that shuffled a map
        // between runs would fail CI at random and teach everybody to re-run it.
        assert_eq!(emitted(), emitted());
    }

    #[test]
    fn a_section_says_whether_it_was_generated_or_written() {
        // The distinction the guide's own header promises. A section that printed neither
        // marker would leave a reader unable to tell a sentence the parser guarantees from
        // one somebody typed — and the written ones are exactly the ones that can be wrong.
        let document = emitted();
        let headings = document.matches("\n## ").count();
        let markers = document.matches("*Generated from ").count()
            + document.matches("*Written prose.").count();
        assert!(headings > 5, "the guide has {headings} section(s)");
        assert_eq!(
            headings, markers,
            "{headings} section(s), {markers} marker(s)"
        );
    }

    /// A command line split the way a shell would split this guide's examples.
    ///
    /// Double quotes only, because that is the whole of what the examples use: a goal and a
    /// criterion are English phrases with spaces in them. Anything more would be a shell
    /// parser nobody asked for.
    fn shell_words(line: &str) -> Vec<String> {
        let mut words = Vec::new();
        let mut current = String::new();
        let mut quoted = false;
        let mut started = false;
        for c in line.chars() {
            match c {
                '"' => {
                    quoted = !quoted;
                    started = true;
                }
                c if c.is_whitespace() && !quoted => {
                    if started {
                        words.push(std::mem::take(&mut current));
                        started = false;
                    }
                }
                c => {
                    current.push(c);
                    started = true;
                }
            }
        }
        if started {
            words.push(current);
        }
        words
    }

    /// A placeholder replaced by something of the right shape.
    ///
    /// Shape and not meaning: this test asks the parser whether the words are a command line,
    /// and the parser judges `cam:x` and `cam:obsbot-tiny-3` identically. The test that asks
    /// a *camera* is the one in `crates/cli`, which substitutes a replayed device's real id.
    fn substitute(word: &str) -> String {
        match word {
            "<CAMERA>" => "cam:x".to_owned(),
            "<CONTROL>" => "brightness".to_owned(),
            "<VALUE>" => "1".to_owned(),
            "<CONTROL>=<VALUE>" => "brightness=1".to_owned(),
            "<TASK>" => "a-task".to_owned(),
            "<PHOTO>" => "/tmp/photo.jpg".to_owned(),
            "<RECORDING>" => "/tmp/take.avi".to_owned(),
            "<SNAPSHOT>" => "/tmp/snapshot.json".to_owned(),
            "<PROFILE>" => "/tmp/profile.json".to_owned(),
            other => other.to_owned(),
        }
    }
}
