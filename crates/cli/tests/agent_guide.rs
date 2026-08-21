//! Every command the agent guide shows, run against the built binary (docs/7 P6e, G6).
//!
//! `scripts/gates/agent-guide-current.sh` proves the committed guide is what the generator
//! emits. That is one of the two claims docs/9's agent-guide row makes, and on its own it is
//! satisfied by a guide whose *written* half teaches a flag nobody has: the generator would
//! emit the same wrong sentence every time, and the diff would be clean. This suite is the
//! other claim — **the examples work** — and the two together are what "the agent-facing doc
//! cannot drift from the command surface it teaches" means.
//!
//! ## The examples are extracted from the guide, not listed here
//!
//! A list of commands written in this file would be a second document, and a second document
//! is exactly the drift under test: somebody would fix an example here and leave the guide
//! saying the old thing. So the input is `docs/agent-guide.md` itself, read at compile time,
//! and the extraction contract is the generator's (`xtask/src/guide.rs`): a fenced block
//! tagged `console`, one command per line, each line beginning with `$ `. Everything else in
//! the document — the `text` blocks holding derived synopses, inline code spans — is not an
//! example and is not run.
//!
//! Two implementations of that contract exist, one here and one in the generator's own tests,
//! because the generator's is not reachable from another crate. `every_console_block_in_the_
//! guide_yields_a_command` is what keeps this one honest against the *document* rather than
//! against the other implementation: it counts the fences and the blocks it actually read.
//!
//! ## What is substituted, and why the program name is one of the substitutions
//!
//! The guide writes its examples with `webcam-handler-client`, because AGENTS' primary
//! consumer runs a daemon and the README's product opinion is the same. This suite runs
//! `webcam-handler-cli`, and that is not a cheat — it is the substitution **the guide's own
//! text instructs the reader to make**: *"Substitute `webcam-handler-cli` if you are running
//! without a daemon; the words after the program name are the same."* Three legs hold that
//! claim up, and only the first is here: the words are fed to `webcam-handler-client`'s own
//! parser in-process (below), they are run end to end through `webcam-handler-cli` (below),
//! and `scripts/gates/cli-parity.sh` compares the two roots' `--json` byte for byte on every
//! read verb. A daemon per example would buy the fourth leg at the price of a socket, a
//! runtime and a second camera owner in a test that is about *prose*.
//!
//! The placeholders — `<CAMERA>`, `<CONTROL>` and the rest — are substituted from a committed
//! device profile the fake backend replays, so this needs no camera and asserts nothing about
//! pixels. `chicony-rgb` is the profile: it is a real capture of a real webcam, it exposes
//! writable integer controls (the guide's `set` and sweep examples need one), and it offers
//! MJPG at 1280x720, which is the size the photo recipe names.
//!
//! ## In order, in one scratch directory
//!
//! The examples are run in document order against one state directory, because the
//! calibration walkthrough **is** a sequence: `calibrate status` cannot answer before
//! `calibrate start` has run. That makes the guide's narrative order part of what this suite
//! checks — a walkthrough reordered into something a reader cannot follow fails here.
//!
//! Every file these commands write lands in a scratch directory under `target/` (note N84). A
//! frame may contain a person (rubric A12); these are synthetic, and the habit is the point.

use std::process::Command;

use camino::Utf8PathBuf;

/// The committed guide, read at compile time.
///
/// `include_str!` rather than a run-time read so the test's input is the file in the tree it
/// was built from, and so cargo rebuilds this suite when the guide moves.
const GUIDE: &str = include_str!("../../../docs/agent-guide.md");

/// The profile replayed for every example. See this file's header for why it is this one.
const PROFILE: &str = "chicony-rgb";

/// The camera id that profile enumerates as.
const CAMERA: &str = "cam:integrated-camera-integrated-c";

/// The task name the calibration walkthrough's session is opened under.
const TASK: &str = "agent-guide";

/// Every command the guide shows, in document order.
fn examples() -> Vec<String> {
    let mut found = Vec::new();
    let mut inside = false;
    for line in GUIDE.lines() {
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

/// A command line split the way a shell would split the guide's examples.
///
/// Double quotes only: a goal and a criterion are English phrases with spaces in them, and
/// that is the whole of the quoting the guide uses. Anything more would be a shell parser
/// nobody asked for, in a test whose subject is a manual.
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
    assert!(!quoted, "unbalanced quotes in {line:?}");
    if started {
        words.push(current);
    }
    words
}

/// The scratch directory and the replayed device an example runs against.
struct Bench {
    dir: tempfile::TempDir,
    profile: Utf8PathBuf,
    control: String,
    value: i64,
}

impl Bench {
    fn new() -> Bench {
        let profile = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/profiles")
            .join(format!("{PROFILE}.json"));
        assert!(profile.exists(), "the corpus is missing {profile}");
        let (control, value) = writable_control(&profile);
        Bench {
            dir: engine::paths::scratch_dir().expect("a scratch directory"),
            profile,
            control,
            value,
        }
    }

    fn path(&self, name: &str) -> String {
        self.dir
            .path()
            .join(name)
            .to_str()
            .expect("a utf-8 temp dir")
            .to_owned()
    }

    /// One placeholder, replaced by a value the replayed device actually has.
    fn substitute(&self, word: &str) -> String {
        match word {
            "<CAMERA>" => CAMERA.to_owned(),
            "<CONTROL>" => self.control.clone(),
            "<VALUE>" => self.value.to_string(),
            "<CONTROL>=<VALUE>" => format!("{}={}", self.control, self.value),
            "<TASK>" => TASK.to_owned(),
            "<PHOTO>" => self.path("photo.jpg"),
            "<RECORDING>" => self.path("take.avi"),
            "<SNAPSHOT>" => self.path("snapshot.json"),
            "<PROFILE>" => self.path("profile.json"),
            other => {
                assert!(
                    !(other.starts_with('<') && other.ends_with('>')),
                    "the guide shows the placeholder {other}, and this suite does not know \
                     what to put there"
                );
                other.to_owned()
            }
        }
    }
}

/// A writable integer control the replayed profile has, and a value inside its range.
///
/// Read out of the profile rather than transcribed, for the reason `json-validates.sh` reads
/// its own: a re-capture that renames a control must fail as a corpus change rather than as a
/// guide that stopped working. The default is clamped rather than trusted — a control's
/// declared default is free to sit outside its declared range \[PF:5\], and feeding a device
/// an out-of-range value would test the clamp instead of the manual.
fn writable_control(profile: &Utf8PathBuf) -> (String, i64) {
    let bytes = std::fs::read(profile).expect("the profile reads");
    let document: serde_json::Value = serde_json::from_slice(&bytes).expect("the profile parses");
    let controls = document["invariant"]["controls"]
        .as_array()
        .expect("a profile carries controls");
    for control in controls {
        if control["type"]["kind"] != "integer" {
            continue;
        }
        let flags = control["flags"]["raw"].as_u64().unwrap_or(0);
        // 0x0001 is DISABLED and 0x0004 is READ_ONLY; either makes a control unwritable.
        if flags & 0x0005 != 0 {
            continue;
        }
        let (Some(min), Some(max)) = (
            control["range"]["min"].as_i64(),
            control["range"]["max"].as_i64(),
        ) else {
            continue;
        };
        if max <= min {
            continue;
        }
        let slug = control["slug"].as_str().expect("a control has a slug");
        let value = control["default"].as_i64().unwrap_or(min).clamp(min, max);
        return (slug.to_owned(), value);
    }
    panic!("{profile} exposes no writable integer control for the guide's examples to name");
}

#[test]
fn every_console_block_in_the_guide_yields_a_command() {
    // This suite reads the guide with its own implementation of the generator's extraction
    // contract, so the failure worth guarding against is a *silent* one: an extractor that
    // matched fewer blocks than the document has would run fewer examples and still report
    // green. Counting the fences is a question about the document rather than about the
    // other implementation, which is the only comparison available across a crate boundary.
    let fences = GUIDE.matches("```console").count();
    let commands = examples();
    assert!(fences > 5, "the guide shows {fences} example block(s)");
    assert!(
        commands.len() >= fences,
        "{fences} console block(s) in the guide and only {} command(s) extracted; the \
         extraction contract and the generator's have diverged",
        commands.len()
    );
}

#[test]
fn every_example_the_guide_shows_is_a_command_line_the_client_accepts() {
    // The words after the program name, fed to the parser the guide's own examples name.
    // In-process, so what is judged is `webcam-handler-client`'s tree rather than a
    // subprocess's exit code — a flag that exists under one root and not the other is
    // precisely what this catches, and it is the leg the run below cannot supply.
    let bench = Bench::new();
    let mut checked = 0;
    for example in examples() {
        let argv: Vec<String> = shell_words(&example)
            .iter()
            .map(|word| bench.substitute(word))
            .collect();
        assert_eq!(
            argv.first().map(String::as_str),
            Some(cli_core::Program::Client.as_str()),
            "the guide's examples are written with the client: {example}"
        );
        cli_core::Cli::try_parse_checked_from(cli_core::Program::Client, &argv).unwrap_or_else(
            |error| {
                panic!("the guide shows `{example}`, which webcam-handler-client refuses:\n{error}")
            },
        );
        checked += 1;
    }
    assert!(checked > 10, "{checked} example(s) checked");
}

#[test]
fn every_example_the_guide_shows_runs_and_answers() {
    // The claim docs/9's row makes in the words it makes it in: *the examples smoke-checked
    // against the built binaries*. Every command in the guide, in document order, against a
    // replayed device — so an example naming a flag that was renamed, a verb that moved, or a
    // step of the walkthrough that cannot follow the one before it fails here rather than in
    // front of the reader it was written for.
    let bench = Bench::new();
    let state = bench.path("state");

    let mut ran = 0;
    for example in examples() {
        let words = shell_words(&example);
        let mut argv: Vec<String> = words.iter().map(|word| bench.substitute(word)).collect();
        // The substitution the guide's own text tells a reader without a daemon to make; see
        // this file's header. Everything after the program name is untouched.
        assert_eq!(
            argv.first().map(String::as_str),
            Some(cli_core::Program::Client.as_str())
        );
        argv.remove(0);

        let output = Command::new(env!("CARGO_BIN_EXE_webcam-handler-cli"))
            .args(["--backend", "fake", "--profile", bench.profile.as_str()])
            .args(&argv)
            // The session store goes to scratch: a suite that wrote into the operator's real
            // state directory would leave a session called `agent-guide` in front of them
            // every time the tests ran, and the sample photos a sweep writes are frames.
            .env("XDG_STATE_HOME", &state)
            .output()
            .expect("webcam-handler-cli runs");

        assert!(
            output.status.success(),
            "the guide shows `{example}`, which exited {:?}:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );

        // An example that asked for `--json` is an example whose answer a reader will parse.
        // A verb that exited 0 having printed something else is the shape of failure this
        // guide's whole `--json` section would be lying about.
        if argv.iter().any(|word| word == "--json") {
            let stdout = String::from_utf8_lossy(&output.stdout);
            serde_json::from_str::<serde_json::Value>(&stdout).unwrap_or_else(|error| {
                panic!("the guide shows `{example}` with --json, and the answer is not one JSON document: {error}")
            });
        }
        ran += 1;
    }
    assert!(ran > 10, "{ran} example(s) run");
}

/// Every `--flag` the failure table names, with the row it is named in.
///
/// Read out of the committed guide rather than listed here, for [`examples`]'s reason: a list
/// written in this file would be a second document, and the drift under test is exactly
/// between two documents. The table's rows are `| \`kind\` | \`code\` | **do** | prose |`, and
/// the flags are the code spans in the prose that begin with two dashes.
fn flags_the_failure_table_names() -> Vec<(String, String)> {
    let mut found = Vec::new();
    for line in GUIDE.lines() {
        let cells: Vec<&str> = line.trim().trim_matches('|').split('|').collect();
        if cells.len() != 4 {
            continue;
        }
        let kind = cells[0].trim().trim_matches('`');
        let code = cells[1].trim().trim_matches('`');
        if !code.chars().all(|c| c.is_ascii_digit()) || code.is_empty() {
            continue;
        }
        let mut rest = cells[3];
        while let Some(open) = rest.find("`--") {
            let after = &rest[open + 1..];
            let Some(close) = after.find('`') else { break };
            found.push((kind.to_owned(), after[..close].to_owned()));
            rest = &after[close + 1..];
        }
    }
    found
}

#[test]
fn every_flag_the_failure_table_offers_as_a_lever_really_produces_that_failure() {
    // **The other half, and the one a name check cannot reach** (docs/11 **M18**, §9.3's
    // "drive the flag, require the kind"). A flag can exist and still be the wrong advice:
    // the `format_unsupported` row tells a reader that `--size` and `--pixel-format` are the
    // two halves of that refusal, and until a `--size` no mode fits *was* refused rather than
    // quietly substituted, half of that sentence named a flag that could not produce the
    // failure it was filed under.
    //
    // Every flag the table names is either driven here or **named with a reason**, and the
    // join is checked, because a flag that is neither is how this claim quietly stops being
    // true (`scripts/gates/cli-parity.sh`'s bucket rule, one document along).
    let bench = Bench::new();
    let photo = bench.path("lever.jpg");
    let drivers: &[(&str, &[&str], schema::ErrorKind)] = &[
        (
            "--pixel-format",
            &["photo", CAMERA, "-o", "", "--pixel-format", "NV12"],
            schema::ErrorKind::FormatUnsupported,
        ),
        (
            "--size",
            &["photo", CAMERA, "-o", "", "--size", "1x1"],
            schema::ErrorKind::FormatUnsupported,
        ),
    ];
    // The flags the table names as something to *change* rather than as the lever that
    // produced the refusal, each with where its claim is asserted instead.
    let named_elsewhere: &[(&str, &str)] = &[
        (
            "--wait",
            "the busy row's claim about it is a negative one — it waits for room in the \
             camera's command queue and not for a stream to end — and it is asserted against \
             a running take in `crates/daemon/tests/mutating_verbs.rs` \
             (`a_photograph_during_a_take_is_told_who_has_the_camera_and_waiting_does_not_\
             change_it`), which needs a daemon this suite deliberately does not start",
        ),
        (
            "--no-guard",
            "the control_inactive row names it as the thing that is *not* the remedy, and \
             that is a fact about the planner rather than about the row's wording: \
             `engine::pairing::plan_unguarded` — the planner this flag selects — documents \
             in its own `# Errors` that it never produces `ControlInactive`, because \
             refusing for an inactive partner is precisely what it exists not to do, and \
             `an_inactive_control_with_no_discoverable_partner_is_refused_by_name` drives \
             the guarded sibling that does. So driving this flag here could only ever \
             answer, whatever a profile's INACTIVE flags said (note **N220**)",
        ),
        (
            "--settle-deadline",
            "the settle_timeout row names it as the remedy, not the cause: producing that \
             refusal takes a device that delivers nothing, which is the fake's scripted \
             fault menu in `crates/engine`'s capture suite",
        ),
        (
            "--skip-frames",
            "the same row and the same reason as `--settle-deadline`",
        ),
    ];

    for (kind, flag) in flags_the_failure_table_names() {
        let driven = drivers.iter().find(|(named, _, _)| *named == flag);
        let excused = named_elsewhere.iter().any(|(named, _)| *named == flag);
        assert!(
            driven.is_some() || excused,
            "the failure table offers `{flag}` for `{kind}`, and this suite neither drives it \
             nor says why not"
        );
        let Some((_, args, expected)) = driven else {
            continue;
        };
        let argv: Vec<&str> = args
            .iter()
            .map(|word| {
                if word.is_empty() {
                    photo.as_str()
                } else {
                    *word
                }
            })
            .collect();
        let output = Command::new(env!("CARGO_BIN_EXE_webcam-handler-cli"))
            .args(["--backend", "fake", "--profile", bench.profile.as_str()])
            .arg("--json")
            .args(&argv)
            .output()
            .expect("webcam-handler-cli runs");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let failure: schema::error::Failure = serde_json::from_str(&stdout).unwrap_or_else(|_| {
            panic!(
                "the failure table offers `{flag}` for `{kind}`, and driving it answered \
                 instead of refusing:\n{stdout}"
            )
        });
        assert_eq!(
            failure.kind(),
            *expected,
            "the failure table files `{flag}` under `{kind}`, and driving it produced \
             something else: {stdout}"
        );
        assert_eq!(
            format!("{:?}", failure.kind()),
            format!("{expected:?}"),
            "{stdout}"
        );
    }
}

/// One failure row's `Do` prose, by the kind the row is keyed on.
///
/// The same four-cell reading [`flags_the_failure_table_names`] does, asked for one row rather
/// than for a token class — because the claim below is about a *sentence* and not about the
/// spans inside it. Read out of the committed guide, so a remedy repaired in the generator and
/// not regenerated is a row this cannot find its subject in.
fn failure_table_remedy(kind: &str) -> String {
    for line in GUIDE.lines() {
        let cells: Vec<&str> = line.trim().trim_matches('|').split('|').collect();
        if cells.len() != 4 {
            continue;
        }
        if cells[0].trim().trim_matches('`') != kind {
            continue;
        }
        let code = cells[1].trim().trim_matches('`');
        if code.is_empty() || !code.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        return cells[3].trim().to_owned();
    }
    panic!("the failure table has no `{kind}` row for this claim to be about")
}

/// The refusal document one run of the shipped binary printed, and its exit code.
///
/// The `--json` half of `a_failing_verb_prints_the_document_the_guide_shows_and_exits_the_code_
/// it_lists`, factored out because the two selector claims below each need one and neither is
/// about the document's shape.
fn refusal_from(profiles: &[&str], selector: &str) -> (serde_json::Value, Option<i32>) {
    let corpus = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/profiles");
    let mut command = Command::new(env!("CARGO_BIN_EXE_webcam-handler-cli"));
    command.args(["--backend", "fake"]);
    for profile in profiles {
        let path = corpus.join(format!("{profile}.json"));
        assert!(path.exists(), "the corpus is missing {path}");
        command.args(["--profile", path.as_str()]);
    }
    command.args(["--json", "info", selector]);
    let output = command.output().expect("webcam-handler-cli runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let document = serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!("a failed --json verb prints a document: {error}\n{stdout}")
    });
    (document, output.status.code())
}

#[test]
fn the_two_selector_rows_of_the_failure_table_send_a_reader_where_this_build_really_sends_them() {
    // **The `Do` column is payload, and payload is tested by driving the claim** (rubric A15).
    // Both of these rows were written for D1, where a camera had one grammar — an id or a
    // prefix of one — and both survived D14 unchanged, which is note **N303**'s class one
    // reader further along than the batch that found it looked (note **N308**):
    //
    //   * `camera_unknown` told a reader that "ids come from what the device says about
    //     itself, not from `/dev/video0`" — two hundred lines below a generated table listing
    //     `/dev/videoN` as one of five spellings this build takes, and above an `info
    //     /dev/video0` that answers with the camera. One document, two grammars, and the wrong
    //     one is the one an agent reaches when it is already failing.
    //   * `camera_ambiguous` said "use a longer prefix or a whole id" for a refusal D12/D14
    //     calls the normal case somewhere: `usb:<vid>:<pid>` names a *model*, two cameras of
    //     one model match it \[PF:8, PF:13\], and there is no prefix in a VID:PID to lengthen.
    //     An instruction an unattended agent cannot carry out.
    //
    // Driven rather than spelled, and the assertions are about the *run's own document* rather
    // than about today's wording: the ambiguous row must name a field the refusal really
    // carries, and neither row may deny a spelling the parser really takes. A remedy reworded
    // in any way that stays true of the build passes; a remedy that goes back to teaching D1's
    // grammar does not.
    let node_path = failure_table_remedy("camera_unknown");
    let ambiguous = failure_table_remedy("camera_ambiguous");

    // The refusal a node path really produces. `/dev/video7` parses — the scheme is one this
    // build knows — so the answer is `camera_unknown` about a live listing and not a refusal to
    // read the spelling, which is exactly what the row must not tell a reader.
    let (unknown, code) = refusal_from(&["chicony-rgb"], "/dev/video7");
    assert_eq!(unknown["error"]["kind"], "camera_unknown", "{unknown}");
    assert_eq!(unknown["error"]["requested"], "/dev/video7", "{unknown}");
    assert_eq!(
        code,
        Some(i32::from(cli_core::exit_code(
            &schema::error::Error::CameraUnknown {
                requested: "/dev/video7".to_owned(),
            }
        )))
    );
    assert!(
        !node_path.contains("not from `/dev/video0`"),
        "the `camera_unknown` remedy still tells a reader a node path is not how a camera is \
         named, and this build just resolved one: {node_path}"
    );
    assert!(
        node_path.contains("How to name a camera"),
        "the `camera_unknown` remedy teaches its own grammar instead of sending a reader to \
         the one section generated from `SelectorScheme::ALL`: {node_path}"
    );

    // And the same spelling, resolved. The negative assertion above is only worth having
    // beside this: a build that refused `/dev/videoN` outright would make the old row true
    // again, and then it is this line that goes red rather than that one.
    let corpus = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/profiles");
    let resolved = Command::new(env!("CARGO_BIN_EXE_webcam-handler-cli"))
        .args([
            "--backend",
            "fake",
            "--profile",
            corpus.join("chicony-rgb.json").as_str(),
            "--json",
            "info",
            "/dev/video0",
        ])
        .output()
        .expect("webcam-handler-cli runs");
    assert!(
        resolved.status.success(),
        "this build no longer answers a node path, so the remedy's grammar is the question \
         again: {}",
        String::from_utf8_lossy(&resolved.stdout)
    );

    // The ambiguous half, over the one committed pair D14 pins it on: two halves of one
    // physical Chicony, one USB id, one serial.
    let (many, _) = refusal_from(&["chicony-rgb", "chicony-ir"], "usb:04f2:b83c");
    assert_eq!(many["error"]["kind"], "camera_ambiguous", "{many}");

    // **The reconciliation: the remedy names a field of the document the reader is holding.**
    // The population is this refusal's own payload, less the two fields every D13 refusal
    // carries — `kind`, which a reader branches on before reading any remedy, and `requested`,
    // which is what it already sent. What is left is what this failure gives a caller to act
    // on, and the row has to name one of them. Derived rather than written, so the day
    // `candidates` is spelled otherwise the row is red with it, and a row that went back to
    // naming only a lever this refusal has none of is red today.
    let payload = many["error"]
        .as_object()
        .unwrap_or_else(|| panic!("the refusal is an object: {many}"));
    let actionable: Vec<&String> = payload
        .keys()
        .filter(|key| key.as_str() != "kind" && key.as_str() != "requested")
        .collect();
    assert!(
        !actionable.is_empty(),
        "this refusal carries nothing beyond `kind` and `requested`, so there is no field for \
         the remedy to name and this claim has no subject: {many}"
    );
    assert!(
        actionable
            .iter()
            .any(|field| ambiguous.contains(&format!("`{field}`"))),
        "the `camera_ambiguous` remedy names none of {actionable:?} — the fields the document \
         this refusal printed actually carries — so a reader is told to act on something it is \
         not holding: {ambiguous}"
    );

    // A regression pin on the one sentence this row shipped with, and it is a pin on a
    // spelling rather than on the class: the assertion above is the class, and it would pass a
    // reworded row that still names `candidates`. Kept because the old sentence is what a
    // reader reaching for the familiar wording would write back.
    assert!(
        !ambiguous.starts_with("The prefix matched"),
        "the `camera_ambiguous` remedy is back to calling every ambiguous spelling a prefix, \
         and the one just driven has no prefix to lengthen: {ambiguous}"
    );
}

/// Every code span the failure table's prose carries, with the row it is written in.
///
/// [`flags_the_failure_table_names`] one token class along, and read out of the committed
/// guide for its reason: the drift under test is between two documents, so a list written
/// here would be a third one. A row's prose is the fourth cell; a code span is anything
/// between backticks in it.
fn code_spans_the_failure_table_uses() -> Vec<(String, String)> {
    let mut found = Vec::new();
    for line in GUIDE.lines() {
        let cells: Vec<&str> = line.trim().trim_matches('|').split('|').collect();
        if cells.len() != 4 {
            continue;
        }
        let kind = cells[0].trim().trim_matches('`');
        let code = cells[1].trim().trim_matches('`');
        if code.is_empty() || !code.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let mut rest = cells[3];
        while let Some(open) = rest.find('`') {
            let after = &rest[open + 1..];
            let Some(close) = after.find('`') else { break };
            found.push((kind.to_owned(), after[..close].to_owned()));
            rest = &after[close + 1..];
        }
    }
    found
}

/// Every verb this guide documents, spelled the way a reader types it.
///
/// The verbs section keys each one as ``### `calibrate sweep` ``, so the headings *are* the
/// surface as this document teaches it — which is the surface the failure table's advice has
/// to be true of. Taken from the same file rather than from the clap tree because a row that
/// sent a reader to a verb the generator emits but the guide never shows would be just as
/// unrunnable for the reader.
fn verbs_the_guide_documents() -> std::collections::BTreeSet<String> {
    GUIDE
        .lines()
        .filter_map(|line| line.strip_prefix("### "))
        .map(|heading| heading.trim().trim_matches('`').to_owned())
        .collect()
}

/// Every name the committed JSON Schema bundle publishes — field names and vocabulary values.
///
/// The failure table's prose names two kinds of thing, and only one of them is something to
/// run: `holders`, `this_process` and `size.available` are payload fields an agent *reads*,
/// and `recording` is a value one of them takes. They are recognised from the artifact the
/// same agent parses (`schemas/webcam-handler-schema.json`), so a field renamed in the schema
/// stops being an excuse here the moment it stops being a field.
fn names_the_bundle_publishes() -> std::collections::BTreeSet<String> {
    fn walk(node: &serde_json::Value, into: &mut std::collections::BTreeSet<String>) {
        match node {
            serde_json::Value::Object(map) => {
                for (key, child) in map {
                    match (key.as_str(), child) {
                        ("properties", serde_json::Value::Object(fields)) => {
                            into.extend(fields.keys().cloned());
                        }
                        ("const", serde_json::Value::String(value)) => {
                            into.insert(value.clone());
                        }
                        ("enum", serde_json::Value::Array(values)) => into.extend(
                            values
                                .iter()
                                .filter_map(|value| value.as_str().map(str::to_owned)),
                        ),
                        _ => {}
                    }
                    walk(child, into);
                }
            }
            serde_json::Value::Array(items) => items.iter().for_each(|item| walk(item, into)),
            _ => {}
        }
    }
    const BUNDLE: &str = include_str!("../../../schemas/webcam-handler-schema.json");
    let mut names = std::collections::BTreeSet::new();
    walk(
        &serde_json::from_str(BUNDLE).expect("the committed bundle is JSON"),
        &mut names,
    );
    names
}

#[test]
fn every_name_the_failure_table_carries_is_one_this_surface_really_has() {
    // **The token class `every_flag_the_failure_table_offers_as_a_lever_really_produces_that_
    // failure` cannot see** (docs/11 §9.3; notes **N123**, **N129**, **N220**). That arm walks
    // the code spans beginning with two dashes, so it is structurally blind to a *verb*: the
    // `busy` row shipped telling an unattended reader to "poll `record_status` and ask again",
    // and `record_status` is on no surface this guide documents — `webcam-handler-client`
    // offers no such verb, `record` does its own start-poll-stop, and the JSON-RPC method for
    // it is spelled `wch_record_status`. One `grep` of the guide found the name exactly once,
    // in the remedy itself.
    //
    // So this is the same population walk one class along: **every** code span in the table,
    // sorted into the two things such a span can honestly be — something to run, which must be
    // a verb this guide documents, or something to read, which must be a name the committed
    // bundle publishes — and anything that is neither is named here with a reason. A span that
    // is neither driven, resolvable nor named is how a remedy quietly stops being runnable.
    let verbs = verbs_the_guide_documents();
    let published = names_the_bundle_publishes();
    // The spans that are neither a verb nor a payload name, each with why it is prose rather
    // than an instruction. Kept to things the row *shows*, never something it tells a reader
    // to run: an entry here for a verb would be the excuse this test exists to refuse.
    let prose: &[(&str, &str)] = &[
        (
            "usb:",
            "a scheme prefix the `camera_ambiguous` row names to say why one spelling can \
             match two devices; the spellings themselves live in *How to name a camera*, \
             generated from `SelectorScheme::ALL`, and are not repeated here",
        ),
        (
            "privacy",
            "a control slug, shown as the example of a control a camera reports read-only; \
             `controls <CAMERA>` is the verb that lists them and is checked as one",
        ),
        (
            "video",
            "the Unix group the `permission_denied` remedy names — a group, not a verb",
        ),
        (
            "webcam-handler-client",
            "a program rather than a verb, and the one this guide's own \"Which program to \
             run\" section tells the reader to prefer",
        ),
    ];

    let mut resolved = 0;
    let mut ran = 0;
    for (kind, span) in code_spans_the_failure_table_uses() {
        // The flags are the other arm's population, and it drives or names every one.
        if span.starts_with("--") {
            continue;
        }
        // `controls <CAMERA>` is the verb plus the placeholder the guide writes for its
        // argument, and the verb is the half that has to exist.
        let named = span.split(" <").next().unwrap_or(&span).to_owned();
        if verbs.contains(&named) {
            ran += 1;
            continue;
        }
        // A dotted path is a payload field reached through another — `size.available` — and
        // every segment of it has to be a name the bundle really publishes.
        if named.split('.').all(|segment| published.contains(segment)) {
            resolved += 1;
            continue;
        }
        assert!(
            prose.iter().any(|(excused, _)| *excused == named),
            "the `{kind}` row tells a reader about `{span}`, which is neither a verb \
             `docs/agent-guide.md` documents nor a name `schemas/webcam-handler-schema.json` \
             publishes — an unattended reader following that row has nothing to run and \
             nothing to read"
        );
    }
    // Not vacuous in either direction: the table really does name verbs, and it really does
    // name payload fields, so a walk that had stopped finding either would fail here rather
    // than pass by finding nothing to check.
    assert!(ran > 0, "the failure table names no verb at all");
    assert!(
        resolved > 0,
        "the failure table names no payload field at all"
    );
}

#[test]
fn a_failing_verb_prints_the_document_the_guide_shows_and_exits_the_code_it_lists() {
    // **What N124 found and the owner's ruling of 2026-08-15 repaired** (note **N127**),
    // asserted against the *manual* rather than only against the binary — which is this
    // suite's whole subject. Until this change the guide said "A failure prints no document"
    // and a test here pinned that sentence; the sentence and the pin moved together, because
    // a manual that is wrong in the safer direction is still wrong for a reader with no hands.
    //
    // Three claims, and the guide makes all three: the document is on standard output, the
    // human line is still on standard error, and the exit code is the one the failure table
    // gives that kind.
    let bench = Bench::new();
    let output = Command::new(env!("CARGO_BIN_EXE_webcam-handler-cli"))
        .args([
            "--backend",
            "fake",
            "--profile",
            bench.profile.as_str(),
            "--json",
            "info",
            "cam:nothing-answers-to-this",
        ])
        .output()
        .expect("webcam-handler-cli runs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let document: schema::error::Failure = serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!("a failed --json verb must print the guide's failure document: {error}\n{stdout}")
    });
    assert!(document.failed());
    assert_eq!(document.kind(), schema::ErrorKind::CameraUnknown);

    // The code, from the table the guide prints — and the table is generated from
    // `cli_core::exit_code`, so this compares the shipped binary against the shipped mapping
    // rather than against a number written here.
    assert_eq!(
        output.status.code(),
        Some(i32::from(cli_core::exit_code(&document.error))),
        "the exit code is not the one the guide's failure table lists"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("webcam-handler-cli: "),
        "the failure line names the program that met it: {stderr}"
    );
    assert!(stderr.contains(&document.message), "{stderr}");

    // And the guide teaches exactly this. The marker first, because it is what a reader
    // branches on before parsing anything else; then the discriminant it just met, spelled
    // the way the failure table spells it.
    assert!(
        GUIDE.contains(&format!("| `{}` |", schema::error::FAILURE_MARKER)),
        "the guide no longer tells a reader which field says a verb refused"
    );
    assert!(
        GUIDE.contains("| `camera_unknown` |"),
        "the guide no longer lists the failure this run met"
    );
    assert!(
        !GUIDE.contains("A failure prints no document."),
        "the guide still says a failure prints no document, and this run just printed one"
    );
}
