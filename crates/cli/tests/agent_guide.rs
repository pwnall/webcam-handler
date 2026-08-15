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

#[test]
fn a_failing_verb_prints_no_document_and_no_discriminant_which_is_what_the_guide_says() {
    // **The guide had to be honest about something this surface does not offer**, and this
    // pins it (note **N124**). `cli_core::exit_code`'s doc comment used to say a caller who
    // wants to branch on *which* failure it met "reads `--json`, where the whole typed error
    // is". It is not: on failure both roots write one `Display` line to standard error and
    // exit 1, and standard output is empty — the D13 discriminant AGENTS says is read
    // unsupervised (*"`Busy` means retry, `DeviceGone` means stop and tell the human"*) never
    // reaches a command-line caller as a value.
    //
    // So the guide says so, and this test is why that sentence can be trusted: a build that
    // starts emitting a failure document turns it red, and the guide must then be rewritten
    // rather than quietly becoming wrong in the safer direction.
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

    assert_eq!(output.status.code(), Some(1), "a typed refusal exits 1");
    assert!(
        output.stdout.is_empty(),
        "a failed --json verb printed {} byte(s) on standard output",
        output.stdout.len()
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("webcam-handler-cli: "),
        "the failure line names the program that met it: {stderr}"
    );
    // The discriminant is what an unattended caller would want and what it does not get: the
    // line carries the rendered sentence, not `camera_unknown`.
    assert!(
        !stderr.contains("camera_unknown"),
        "the failure line now carries the D13 discriminant; the guide's section on failures \
         says it does not, and one of the two has to change: {stderr}"
    );
    assert!(
        GUIDE.contains("**A failure prints no document.**"),
        "the guide no longer says what this test pins"
    );
}
