//! `webcam-handler-cli calibrate` end to end, against the fake backend (docs/7 P3d, gate G3).
//!
//! Everything here runs as a *subprocess*, over a session tree the test builds and throws
//! away. The engine's own suites drive `lifecycle` and `calibrate` directly and are the
//! finer-grained ones; what these add is the half nothing else covers — that the binary
//! parses the flags into the request the engine gets, puts the session where `$XDG_STATE_HOME`
//! says, records the selector a caller claimed, and refuses the things it must refuse.
//!
//! ## Every refusal has its twin
//!
//! A suite that only asserts the sad path cannot tell "it refuses the wrong camera" from
//! "it refuses". So the fingerprint refusal is followed by the same apply against the right
//! camera, `--partial` is asserted in both directions with the *applied set* checked, and
//! the three selector spellings are each written and read back.
//!
//! ## The state directory's lock lives here too
//!
//! D9's daemonless protocol is `webcam-handler-cli`'s half of a two-party rule, and the party
//! it is a rule about is `webcam-handler-daemon`. The daemon's end — that a running
//! `webcam-handler-daemon` really holds the state directory for its whole life, and what it is
//! told when a `webcam-handler-cli` got there first — is asserted against a real daemon
//! process in `crates/daemon/tests/lock.rs`. What is asserted here is the other end: that the
//! refusal reaches a *person*, in the words design D9 writes, through the binary they typed.
//!
//! ## The schema half
//!
//! `session.json` and `log.ndjson` are documents an agent reads without this tool, so they
//! are validated against the **committed bundle** — the same artifact
//! `schema-artifacts-current.sh` proves matches the Rust types, and the same one
//! `json-validates.sh` checks the `--json` answers against. [`Bundle`] is not a complete
//! JSON Schema validator (the offline toolchain has none, and docs/9 records that limit
//! for the gate); it enforces the checkable core — required properties, declared
//! properties, `$ref` recursion, `const` discriminants, types and enum branches — and
//! [`the_schema_check_fails_documents_that_do_not_match`] proves it can go red on each.

use std::process::Command;

use camino::{Utf8Path, Utf8PathBuf};
use engine::store::{LockProtocol, SessionStore, StoreLock};
use schema::paths::MapEnv;
use schema::selector::SelectorScheme;
use serde_json::{Map, Value};

/// The `webcam-handler-cli` binary this test drives, built by cargo alongside it.
fn wch() -> Command {
    Command::new(env!("CARGO_BIN_EXE_webcam-handler-cli"))
}

fn repo_root() -> Utf8PathBuf {
    Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A state directory under the one scratch root (note N84), thrown away with the test.
struct Scratch {
    dir: tempfile::TempDir,
}

impl Scratch {
    fn new() -> Scratch {
        Scratch {
            dir: engine::paths::scratch_dir().expect("a scratch directory"),
        }
    }

    fn state(&self) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(self.dir.path().join("state")).expect("a utf-8 temp dir")
    }

    /// Somebody else holds this state directory's lock, following `protocol`.
    ///
    /// A real `flock` over a real second open file description, which is all a
    /// `webcam-handler-cli` subprocess can tell about whoever holds it — so the daemon this
    /// stands in for needs no process, and nothing here has to synchronize with one.
    ///
    /// Resolved through [`SessionStore::from_env`] rather than by joining a path, because
    /// `engine::paths::state_dir` appends `webcam-handler` to `$XDG_STATE_HOME`: a
    /// contender built any other way would be locking a file the binary never opens, and
    /// every refusal asserted below would be a refusal that never happened.
    fn held_by_somebody_else(&self, protocol: LockProtocol) -> StoreLock {
        let env = MapEnv::from_pairs(&[("XDG_STATE_HOME", self.state().as_str())]);
        SessionStore::from_env(&env)
            .expect("the fixture sets the variable")
            .lock(protocol)
            .expect("nothing holds a fresh state directory")
    }
}

/// A `webcam-handler-cli` bound to one state directory and a set of replayed cameras.
struct Wch {
    state: Utf8PathBuf,
    profiles: Vec<Utf8PathBuf>,
}

/// What one invocation produced.
struct Ran {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Wch {
    /// `webcam-handler-cli` over the named committed profiles, writing sessions under
    /// `scratch`.
    fn new(scratch: &Scratch, profiles: &[&str]) -> Wch {
        let profiles = profiles
            .iter()
            .map(|name| {
                let path = repo_root()
                    .join("corpus/profiles")
                    .join(format!("{name}.json"));
                assert!(path.exists(), "the corpus is missing {path}");
                path
            })
            .collect();
        Wch {
            state: scratch.state(),
            profiles,
        }
    }

    fn run(&self, args: &[&str]) -> Ran {
        let mut command = wch();
        // The state directory arrives as an environment variable rather than a flag,
        // because that is how the shipped tool finds it (note N2) — a test that passed a
        // path would not be exercising the resolution the operator gets.
        command.env("XDG_STATE_HOME", self.state.as_str());
        command.args(["--backend", "fake"]);
        for profile in &self.profiles {
            command.args(["--profile", profile.as_str()]);
        }
        let output = command
            .args(args)
            .output()
            .expect("webcam-handler-cli runs");
        Ran {
            code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    /// Run, asserting it answered.
    fn ok(&self, args: &[&str]) -> Ran {
        let ran = self.run(args);
        assert_eq!(
            ran.code, 0,
            "webcam-handler-cli {args:?} exited {}: {}",
            ran.code, ran.stderr
        );
        ran
    }

    /// Run with `--json`, asserting it answered, and parse the document.
    fn json(&self, args: &[&str]) -> Value {
        let mut with_json = args.to_vec();
        with_json.push("--json");
        let ran = self.ok(&with_json);
        serde_json::from_str(&ran.stdout).unwrap_or_else(|error| {
            panic!(
                "webcam-handler-cli {args:?} did not emit JSON: {error}\n{}",
                ran.stdout
            )
        })
    }

    /// Run, asserting it refused, and hand back what it said.
    fn refuses(&self, args: &[&str]) -> Ran {
        let ran = self.run(args);
        assert_ne!(
            ran.code, 0,
            "webcam-handler-cli {args:?} was expected to refuse: {}",
            ran.stdout
        );
        ran
    }

    /// Refuse, and be the refusal this call meant.
    ///
    /// Since the owner's ruling of 2026-08-15 (note **N127**) each D13 kind leaves an exit code
    /// of its own, so "it refused" and "it refused for *this* reason" are two different claims
    /// and the second one is now cheap. The expected code is read out of the shipped mapping
    /// rather than written as a number here: `cli_core::exit_code` is the one home, and a
    /// literal in a test would be a second table nobody regenerates.
    fn refuses_with(&self, args: &[&str], kind: schema::ErrorKind) -> Ran {
        let ran = self.refuses(args);
        assert_eq!(
            ran.code,
            i32::from(cli_core::exit_code(&schema::Error::sample(kind))),
            "webcam-handler-cli {args:?} was expected to refuse with {kind:?}: {}",
            ran.stderr
        );
        ran
    }

    /// Run with a standard error that **cannot be written**, and hand back the answer.
    ///
    /// The child's file descriptor 2 is one end of a `AF_UNIX` pair whose other end is
    /// closed **before the child exists**, so every write to it fails with `EPIPE` — the
    /// everyday way a terminal-less run loses its standard error, a reader that went away.
    /// Deterministic and unraceable: the peer is dropped here, in this process, before
    /// `spawn`, so there is no window in which a write could succeed. No signal in it
    /// either — Rust sets `SIGPIPE` to `SIG_IGN` at startup, so the child meets an ordinary
    /// `io::Error`, which is exactly the input note **N216** is about.
    ///
    /// A socket pair rather than a file on a device that refuses writes, for two reasons: a
    /// `/dev/full` opened for writing is a raw write primitive in a file that names the
    /// state directory, which `scripts/gates/atomic-write-home.sh` refuses on sight and is
    /// right to; and a read-only descriptor is not reliably unwritable — measured on this
    /// host, a write to `/dev/null` opened `O_RDONLY` *succeeds*.
    fn run_with_unwritable_stderr(&self, args: &[&str]) -> Ran {
        let mut command = wch();
        command.env("XDG_STATE_HOME", self.state.as_str());
        command.args(["--backend", "fake"]);
        for profile in &self.profiles {
            command.args(["--profile", profile.as_str()]);
        }
        let (reader, blocked) =
            std::os::unix::net::UnixStream::pair().expect("a socket pair for the child's stderr");
        drop(reader);
        let child = command
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::from(std::os::fd::OwnedFd::from(
                blocked,
            )))
            .spawn()
            .expect("webcam-handler-cli runs");
        let output = child.wait_with_output().expect("it finishes");
        Ran {
            code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::new(),
        }
    }

    /// The directory of the one session in the tree.
    fn only_session_dir(&self) -> Utf8PathBuf {
        let list = self.json(&["calibrate", "list"]);
        let sessions = list["sessions"].as_array().expect("a sessions array");
        assert_eq!(sessions.len(), 1, "expected one session: {list}");
        Utf8PathBuf::from(sessions[0]["path"].as_str().expect("a path"))
    }
}

// ------------------------------------------------------------------ the schema check

/// The committed JSON Schema bundle, and the checkable core of it.
struct Bundle {
    defs: Map<String, Value>,
}

impl Bundle {
    /// The bundle `just generate` writes and `schema-artifacts-current.sh` keeps current.
    fn committed() -> Bundle {
        let path = repo_root().join("schemas/webcam-handler-schema.json");
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("{path} is unreadable ({error}); `just generate`"));
        let bundle: Value = serde_json::from_slice(&bytes).expect("the bundle is JSON");
        let defs = bundle["$defs"]
            .as_object()
            .expect("the bundle keeps its types under $defs")
            .clone();
        Bundle { defs }
    }

    /// Whether `value` matches `#/$defs/<name>`.
    fn check(&self, name: &str, value: &Value) -> Result<(), String> {
        let schema = self
            .defs
            .get(name)
            .unwrap_or_else(|| panic!("the bundle has no #/$defs/{name}"));
        self.matches(schema, value, name, true)
    }

    /// `value` against one schema. `strict` enforces "no property the schema does not
    /// declare"; it is dropped inside `oneOf`/`anyOf` branches, where the other branches'
    /// properties are legitimately present.
    fn matches(&self, schema: &Value, value: &Value, at: &str, strict: bool) -> Result<(), String> {
        let Some(object) = schema.as_object() else {
            // `true`/`false` schemas: `true` accepts anything, and schemars emits no
            // `false` in this bundle.
            return Ok(());
        };

        if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
            let name = reference.trim_start_matches("#/$defs/");
            let target = self
                .defs
                .get(name)
                .ok_or_else(|| format!("{at}: dangling $ref {reference}"))?;
            return self.matches(target, value, at, strict);
        }
        if let Some(expected) = object.get("const")
            && expected != value
        {
            return Err(format!("{at}: expected {expected}, found {value}"));
        }
        if let Some(kind) = object.get("type") {
            self.type_matches(kind, value, at)?;
        }
        for key in ["oneOf", "anyOf"] {
            if let Some(branches) = object.get(key).and_then(Value::as_array) {
                let mut failures = Vec::new();
                let mut matched = false;
                for (nth, branch) in branches.iter().enumerate() {
                    match self.matches(branch, value, &format!("{at}.{key}[{nth}]"), false) {
                        Ok(()) => {
                            matched = true;
                            break;
                        }
                        Err(why) => failures.push(why),
                    }
                }
                if !matched {
                    return Err(format!("{at}: no {key} branch matched: {failures:?}"));
                }
            }
        }
        if let Some(branches) = object.get("allOf").and_then(Value::as_array) {
            for (nth, branch) in branches.iter().enumerate() {
                self.matches(branch, value, &format!("{at}.allOf[{nth}]"), strict)?;
            }
        }
        if let Some(items) = object.get("items")
            && let Some(elements) = value.as_array()
        {
            for (nth, element) in elements.iter().enumerate() {
                self.matches(items, element, &format!("{at}[{nth}]"), true)?;
            }
        }

        let Some(fields) = value.as_object() else {
            return Ok(());
        };
        for required in object
            .get("required")
            .and_then(Value::as_array)
            .unwrap_or(&Vec::new())
        {
            let name = required.as_str().unwrap_or_default();
            if !fields.contains_key(name) {
                return Err(format!("{at}: required property {name} is missing"));
            }
        }
        if let Some(properties) = object.get("properties").and_then(Value::as_object) {
            for (name, sub) in properties {
                if let Some(found) = fields.get(name) {
                    self.matches(sub, found, &format!("{at}.{name}"), true)?;
                }
            }
            // Only where the answer is unambiguous: a schema that also carries a
            // combinator or a map's `additionalProperties` declares its properties
            // somewhere else too, and refusing an "undeclared" key there would refuse
            // valid documents.
            let combined = ["oneOf", "anyOf", "allOf", "additionalProperties"]
                .iter()
                .any(|key| object.contains_key(*key));
            if strict && !combined {
                for name in fields.keys() {
                    if !properties.contains_key(name) {
                        return Err(format!("{at}: property {name} is not in the schema"));
                    }
                }
            }
        }
        if let Some(additional) = object.get("additionalProperties")
            && additional.is_object()
        {
            for (name, sub) in fields {
                self.matches(additional, sub, &format!("{at}.{name}"), true)?;
            }
        }
        Ok(())
    }

    fn type_matches(&self, kind: &Value, value: &Value, at: &str) -> Result<(), String> {
        let names: Vec<&str> = match kind {
            Value::String(one) => vec![one.as_str()],
            Value::Array(many) => many.iter().filter_map(Value::as_str).collect(),
            _ => return Ok(()),
        };
        let ok = names.iter().any(|name| match *name {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "integer" => value.is_i64() || value.is_u64(),
            "number" => value.is_number(),
            "boolean" => value.is_boolean(),
            "null" => value.is_null(),
            _ => true,
        });
        if ok {
            Ok(())
        } else {
            Err(format!("{at}: {value} is not {names:?}"))
        }
    }
}

/// The document on disk, as bytes — which is how a second process meets it.
fn document(session_dir: &Utf8Path) -> Value {
    let path = session_dir.join(schema::limits::SESSION_FILE);
    let bytes =
        std::fs::read(&path).unwrap_or_else(|error| panic!("{path} is unreadable: {error}"));
    serde_json::from_slice(&bytes).expect("session.json is JSON")
}

/// Every line of the session's log, as documents.
fn history(session_dir: &Utf8Path) -> Vec<Value> {
    let path = session_dir.join(schema::limits::SESSION_LOG_FILE);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{path} is unreadable: {error}"));
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("every log line is JSON"))
        .collect()
}

// ------------------------------------------------------------------ the session flow

/// Take a session as far as one calibrated control, and hand back the tool.
fn calibrated_session(scratch: &Scratch) -> Wch {
    let wch = Wch::new(scratch, &["chicony-rgb"]);
    wch.ok(&[
        "calibrate",
        "start",
        "cam:integrated",
        "--task",
        "read text from the DUT display",
        "--goal",
        "the serial number is legible",
        "--criterion",
        "text clarity",
    ]);
    wch.ok(&[
        "calibrate",
        "plan",
        "cam:integrated",
        "--task",
        "read text from the DUT display",
        "brightness",
    ]);
    wch.ok(&[
        "calibrate",
        "sweep",
        "cam:integrated",
        "--task",
        "read text from the DUT display",
        "brightness",
        "--values",
        "0,128,255",
        "--skip-frames",
        "0",
    ]);
    wch
}

#[test]
fn a_session_the_binary_wrote_validates_against_the_committed_schema_bundle() {
    let scratch = Scratch::new();
    let wch = calibrated_session(&scratch);
    let bundle = Bundle::committed();
    let dir = wch.only_session_dir();

    let session = document(&dir);
    bundle
        .check("Session", &session)
        .unwrap_or_else(|why| panic!("the session this build wrote is not a Session: {why}"));
    // The `--json` answer and the file are the same document, so validating one is not a
    // claim about the other unless they are compared.
    assert_eq!(
        wch.json(&[
            "calibrate",
            "status",
            "cam:integrated",
            "--task",
            "read text from the DUT display",
        ])["session"],
        session,
        "`calibrate status` answered with something other than the document on disk"
    );

    let log = history(&dir);
    assert!(log.len() >= 5, "a session with no history: {log:?}");
    for line in &log {
        bundle
            .check("LogEntry", line)
            .unwrap_or_else(|why| panic!("{line} is not a LogEntry: {why}"));
    }

    // And the sample rows carry what D8 says they carry, through the schema rather than by
    // eye: three samples, each with a photo that exists under the session directory.
    let samples = session["controls"]["brightness"]["samples"]
        .as_array()
        .expect("the swept control has samples");
    assert_eq!(samples.len(), 3);
    for sample in samples {
        let photo = dir.join(sample["photo"].as_str().expect("a relative photo path"));
        assert!(photo.exists(), "{photo} is in the document and not on disk");
    }
}

#[test]
fn the_schema_check_fails_documents_that_do_not_match() {
    // Rule 2 for the check itself: a validator that cannot go red proves nothing about the
    // documents it passes. Each arm below is one of the four things it enforces.
    let scratch = Scratch::new();
    let wch = calibrated_session(&scratch);
    let bundle = Bundle::committed();
    let good = document(&wch.only_session_dir());
    assert!(bundle.check("Session", &good).is_ok());

    let mut extra = good.clone();
    extra["tool_opinion"] = Value::String("undeclared".to_owned());
    assert!(
        bundle.check("Session", &extra).is_err(),
        "a property the schema does not declare passed"
    );

    let mut missing = good.clone();
    missing
        .as_object_mut()
        .expect("an object")
        .remove("schema_version");
    assert!(
        bundle.check("Session", &missing).is_err(),
        "a document missing a required property passed"
    );

    let mut wrong_type = good.clone();
    wrong_type["schema_version"] = Value::String("one".to_owned());
    assert!(
        bundle.check("Session", &wrong_type).is_err(),
        "a property of the wrong type passed"
    );

    let mut wrong_variant = good;
    wrong_variant["controls"]["brightness"]["status"]["status"] =
        Value::String("perfected".to_owned());
    assert!(
        bundle.check("Session", &wrong_variant).is_err(),
        "a status outside the closed vocabulary passed"
    );
}

// ------------------------------------------------------------------ selector identity

#[test]
fn every_selector_a_caller_can_claim_survives_the_write_and_the_read() {
    // D8: the record says *who* chose, because nothing may pretend a Laplacian knows what
    // "text legible on the DUT" means. All three forms, each in its own session, each read
    // back from the document a second process would see.
    let scratch = Scratch::new();
    let wch = Wch::new(&scratch, &["chicony-rgb"]);
    let bundle = Bundle::committed();

    for (task, select, expected) in [
        (
            "by metric",
            vec!["--metric", "sharpness"],
            serde_json::json!({"kind": "metric", "name": "sharpness"}),
        ),
        (
            "by agent",
            vec!["--value", "128", "--by", "agent"],
            serde_json::json!({"kind": "agent"}),
        ),
        (
            "by human",
            vec!["--value", "128", "--by", "human"],
            serde_json::json!({"kind": "human"}),
        ),
    ] {
        wch.ok(&["calibrate", "start", "cam:integrated", "--task", task]);
        wch.ok(&[
            "calibrate",
            "plan",
            "cam:integrated",
            "--task",
            task,
            "brightness",
        ]);
        wch.ok(&[
            "calibrate",
            "sweep",
            "cam:integrated",
            "--task",
            task,
            "brightness",
            "--values",
            "0,128",
            "--skip-frames",
            "0",
        ]);

        let mut args = vec![
            "calibrate",
            "select",
            "cam:integrated",
            "--task",
            task,
            "brightness",
        ];
        args.extend(select);
        let answered = wch.json(&args);
        let status = &answered["controls"]["brightness"]["status"];
        assert_eq!(status["status"], "calibrated", "{task}: {status}");
        assert_eq!(
            status["selector"], expected,
            "{task}: the selector recorded is not the one claimed"
        );
        bundle
            .check("Session", &answered)
            .unwrap_or_else(|why| panic!("{task}: {why}"));

        // And the same fact off the disk, which is what a later `status` reads.
        let stored = wch.json(&["calibrate", "status", "cam:integrated", "--task", task]);
        assert_eq!(
            stored["session"]["controls"]["brightness"]["status"]["selector"], expected,
            "{task}: the selector did not survive the round trip"
        );
        // The log says it too, so the *history* names who chose and not only the state.
        let chose: Vec<&Value> = stored["log"]
            .as_array()
            .expect("a log")
            .iter()
            .filter(|entry| entry["event"] == "selected")
            .collect();
        assert_eq!(chose.len(), 1, "{task}: {:?}", stored["log"]);
        assert_eq!(chose[0]["selector"], expected, "{task}");
    }

    // The metric selection also carries the score it earned; the two claimed ones do not,
    // because nobody measured anything.
    let by_metric = wch.json(&[
        "calibrate",
        "status",
        "cam:integrated",
        "--task",
        "by metric",
    ]);
    assert!(
        by_metric["session"]["controls"]["brightness"]["status"]["score"].is_number(),
        "a metric ranked and recorded no score"
    );
    let by_agent = wch.json(&[
        "calibrate",
        "status",
        "cam:integrated",
        "--task",
        "by agent",
    ]);
    assert!(
        by_agent["session"]["controls"]["brightness"]["status"]["score"].is_null(),
        "an agent's choice was given a score nobody computed"
    );
}

#[test]
fn a_value_chosen_without_saying_who_chose_it_is_a_usage_error() {
    // The rule that keeps the record honest, enforced where a usage mistake belongs: exit
    // 2, clap's, rather than exit 1 — "you typed it wrong" and "the camera is busy" are
    // different failures.
    let scratch = Scratch::new();
    let wch = calibrated_session(&scratch);
    let task = "read text from the DUT display";

    let refused = wch.refuses(&[
        "calibrate",
        "select",
        "cam:integrated",
        "--task",
        task,
        "brightness",
        "--value",
        "128",
    ]);
    assert_eq!(refused.code, 2, "{}", refused.stderr);
    assert!(refused.stderr.contains("--by"), "{}", refused.stderr);

    // Nor may a caller claim to be a metric: `--metric` is the tool saying it computed a
    // ranking, and `--by metric` would be a caller claiming one.
    let refused = wch.refuses(&[
        "calibrate",
        "select",
        "cam:integrated",
        "--task",
        task,
        "brightness",
        "--value",
        "128",
        "--by",
        "metric:sharpness",
    ]);
    assert_eq!(refused.code, 2, "{}", refused.stderr);

    // And the twin: with `--by`, it lands.
    wch.ok(&[
        "calibrate",
        "select",
        "cam:integrated",
        "--task",
        task,
        "brightness",
        "--value",
        "128",
        "--by",
        "agent",
    ]);
}

// ------------------------------------------------------------------ apply

#[test]
fn apply_refuses_a_camera_that_is_not_the_sessions_naming_the_fields_that_differ() {
    // D13's `FingerprintMismatch { fields }`: "wrong camera" is not actionable and "the bus
    // path and the card differ" is. The two replayed cameras share a driver and differ in
    // three fields, so this asserts the refusal names those three and *not* the one they
    // agree on.
    let scratch = Scratch::new();
    let wch = Wch::new(&scratch, &["chicony-rgb", "obsbot-tiny3"]);
    let task = "read text from the DUT display";
    wch.ok(&["calibrate", "start", "cam:integrated", "--task", task]);
    wch.ok(&[
        "calibrate",
        "plan",
        "cam:integrated",
        "--task",
        task,
        "brightness",
    ]);
    wch.ok(&[
        "calibrate",
        "sweep",
        "cam:integrated",
        "--task",
        task,
        "brightness",
        "--values",
        "0,128",
        "--skip-frames",
        "0",
    ]);
    wch.ok(&[
        "calibrate",
        "select",
        "cam:integrated",
        "--task",
        task,
        "brightness",
        "--metric",
        "sharpness",
    ]);

    // The session is reachable from the other camera only by its id, which is exactly why
    // `--session` exists: a lookup keyed on the camera could never produce a mismatch.
    let id = wch.json(&["calibrate", "list"])["sessions"][0]["id"]
        .as_str()
        .expect("a session id")
        .to_owned();
    // A device refusal and not a usage error, named: the exit code says which of the
    // eighteen it is (note **N127**), so this asserts the reason rather than only that
    // something went wrong.
    let refused = wch.refuses_with(
        &["calibrate", "apply", "cam:obsbot", "--session", &id],
        schema::ErrorKind::FingerprintMismatch,
    );

    // The named fields are the ones that actually differ between the two cameras, read
    // from the enumeration rather than transcribed.
    let cameras = wch.json(&["list"]);
    let session_camera = &cameras["cameras"][0]["fingerprint"];
    let other_camera = &cameras["cameras"][1]["fingerprint"];
    let mut differing: Vec<&str> = Vec::new();
    let mut agreeing: Vec<&str> = Vec::new();
    for field in ["bus_path", "usb_id", "card", "driver", "serial"] {
        if session_camera[field].is_null() || other_camera[field].is_null() {
            continue;
        }
        if session_camera[field] == other_camera[field] {
            agreeing.push(field);
        } else {
            differing.push(field);
        }
    }
    assert!(
        !differing.is_empty() && !agreeing.is_empty(),
        "the two replayed cameras must differ in some fields and agree in others: \
         {differing:?} / {agreeing:?}"
    );
    for field in &differing {
        assert!(
            refused.stderr.contains(field),
            "{field} differs and the refusal does not name it: {}",
            refused.stderr
        );
    }
    for field in &agreeing {
        assert!(
            !refused.stderr.contains(field),
            "{field} agrees and the refusal named it anyway: {}",
            refused.stderr
        );
    }

    // The twin: the camera the session was recorded against takes it, and the write lands.
    let report = wch.json(&[
        "calibrate",
        "apply",
        "cam:integrated",
        "--session",
        &id,
        "--partial",
    ]);
    let written: Vec<&str> = report["writes"]
        .as_array()
        .expect("writes")
        .iter()
        .map(|write| write["slug"].as_str().expect("a slug"))
        .collect();
    assert!(written.contains(&"brightness"), "{report}");
}

#[test]
fn apply_needs_partial_while_work_is_queued_and_then_writes_exactly_what_was_calibrated() {
    let scratch = Scratch::new();
    let wch = Wch::new(&scratch, &["chicony-rgb"]);
    let task = "read text from the DUT display";
    wch.ok(&["calibrate", "start", "cam:integrated", "--task", task]);
    // Two controls queued, one of them calibrated: the shape `--partial` exists for.
    wch.ok(&[
        "calibrate",
        "plan",
        "cam:integrated",
        "--task",
        task,
        "brightness",
        "contrast",
    ]);
    wch.ok(&[
        "calibrate",
        "sweep",
        "cam:integrated",
        "--task",
        task,
        "brightness",
        "--values",
        "0,128",
        "--skip-frames",
        "0",
    ]);
    wch.ok(&[
        "calibrate",
        "select",
        "cam:integrated",
        "--task",
        task,
        "brightness",
        "--metric",
        "sharpness",
    ]);

    let refused = wch.refuses_with(
        &["calibrate", "apply", "cam:integrated", "--task", task],
        schema::ErrorKind::IllegalTransition,
    );
    assert!(
        refused.stderr.contains("unsettled(1 pending)"),
        "the refusal must say what is pending: {}",
        refused.stderr
    );

    let report = wch.json(&[
        "calibrate",
        "apply",
        "cam:integrated",
        "--task",
        task,
        "--partial",
    ]);
    // Exactly the calibrated controls, and the automation switch-offs D4's ordering needed
    // — nothing else, and in particular not the queued control nobody chose a value for.
    let session =
        wch.json(&["calibrate", "status", "cam:integrated", "--task", task])["session"].clone();
    let calibrated: Vec<&str> = session["controls"]
        .as_object()
        .expect("controls")
        .iter()
        .filter(|(_, entry)| entry["status"]["status"] == "calibrated")
        .map(|(slug, _)| slug.as_str())
        .collect();
    assert_eq!(calibrated, vec!["brightness"], "{session}");

    let disabled: Vec<&str> = report["disabled_automation"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|slug| slug.as_str().expect("a slug"))
                .collect()
        })
        .unwrap_or_default();
    let written: Vec<&str> = report["writes"]
        .as_array()
        .expect("writes")
        .iter()
        .map(|write| write["slug"].as_str().expect("a slug"))
        .collect();
    for slug in &written {
        assert!(
            calibrated.contains(slug) || disabled.contains(slug),
            "{slug} was written and is neither calibrated nor an automation switch-off: \
             {report}"
        );
    }
    assert!(
        !written.contains(&"contrast"),
        "the uncalibrated control was written anyway: {report}"
    );

    // And the history records the apply, so a later reader knows the camera was changed.
    let applied = history(&wch.only_session_dir())
        .into_iter()
        .filter(|entry| entry["event"] == "applied")
        .count();
    assert_eq!(applied, 1, "an apply that left no trace");
}

// ------------------------------------------------------------------ giving the camera back

#[test]
fn a_sweep_borrows_the_camera_and_calibrate_restore_spends_the_record_that_gives_it_back() {
    // AGENTS rule 8, D4 and design §5 all say a sweep leaves the camera as it found it, and
    // `lifecycle::sweep_write` persists the pre-sweep snapshot before the first write for
    // exactly that reason. Until P3's review nothing a user could type ever spent it:
    // `lifecycle::recover`'s only callers in the workspace were tests, so a real
    // `webcam-handler-cli calibrate sweep` exited 0 with the camera holding its last swept
    // value and the record sitting in `session.json` forever — and the *next* session on that
    // camera recorded the previous sweep's endpoint as "the camera as the operator found it".
    //
    // What this arm can and cannot claim: the fake replays a profile into a fresh device per
    // process, so a subprocess suite cannot watch a value survive between two
    // `webcam-handler-cli` runs. The claim here is the half that is durable and is the half
    // that was missing — the verb exists, it reports on every control the snapshot held, and
    // it *consumes* the record. That the camera physically goes back is the engine suite's
    // (`lifecycle::recover`'s own tests, and `sweep.rs`'s interrupted-sweep arm) and the R3
    // hardware rung's.
    let scratch = Scratch::new();
    let wch = calibrated_session(&scratch);
    let task = "read text from the DUT display";
    let dir = wch.only_session_dir();

    // The borrow, on the record: the sweep armed a snapshot and nothing has spent it.
    let borrowed = document(&dir);
    let held = borrowed["pre_snapshot"]["entries"]
        .as_array()
        .expect("the sweep armed a pre-sweep snapshot")
        .len();
    assert!(held > 0, "the snapshot holds no controls: {borrowed}");

    // …and the sweep said so, naming the command that undoes it. A default nobody is told
    // about is a default nobody runs.
    let swept_again = wch.ok(&[
        "calibrate",
        "sweep",
        "cam:integrated",
        "--task",
        task,
        "contrast",
        "--values",
        "16,32",
        "--skip-frames",
        "0",
    ]);
    assert!(
        swept_again.stderr.contains("calibrate restore"),
        "a sweep left the camera borrowed and did not say how to give it back: {}",
        swept_again.stderr
    );

    let report = wch.json(&["calibrate", "restore", "cam:integrated", "--task", task]);
    let outcomes = report["outcomes"].as_array().expect("outcomes");
    assert_eq!(
        outcomes.len(),
        held,
        "the restore did not account for every control the snapshot held: {report}"
    );
    for outcome in outcomes {
        assert!(
            outcome["outcome"] != "unrestorable",
            "a control did not come back: {outcome}"
        );
    }
    // Consumed, atomically, in the document a second process reads (D9): the record is the
    // only route back, so a restore that reported success and kept it would be a session
    // that thinks it put the camera back twice.
    assert!(
        document(&dir)["pre_snapshot"].is_null(),
        "a complete restore left the record unspent: {}",
        document(&dir)
    );
    let log = history(&dir);
    assert_eq!(
        log.iter()
            .filter(|entry| entry["event"] == "restored")
            .count(),
        1,
        "the restore left no trace in the session's history: {log:?}"
    );

    // The other direction, and it is not an error: running it again finds nothing to put
    // back and says so, rather than inventing a restore or refusing.
    let again = wch.run(&["calibrate", "restore", "cam:integrated", "--task", task]);
    assert_eq!(again.code, 0, "{}", again.stderr);
    assert!(
        again.stderr.contains("no unconsumed pre-sweep snapshot"),
        "an empty restore read as a restore that did nothing: {}",
        again.stderr
    );
    let empty = wch.json(&["calibrate", "restore", "cam:integrated", "--task", task]);
    assert_eq!(empty["outcomes"].as_array().map(Vec::len), Some(0));
}

#[test]
fn a_sweep_whose_note_cannot_be_printed_still_answers_with_one_document_and_a_zero() {
    // **A note is commentary, and commentary cannot fail a verb that succeeded** (docs/11
    // **M28**, note **N216**). Every line this surface wrote to standard error was
    // propagated with `?`, so a standard error that refuses replaced an outcome that had
    // already happened: the sweep is on disk, the samples are recorded, the camera has been
    // moved and its pre-sweep snapshot persisted — and the process answered `failed: true`
    // and exited 27, about the *stream*.
    //
    // `calibrate sweep --json` is the worst of them and the reason this arm is here rather
    // than in a unit test: its note is written **after** `render::session` has printed the
    // answer, so the failure document went onto standard output beside it and the run
    // printed *two* schema documents — which design §2.7 says can never happen ("a `--json`
    // invocation prints exactly one `webcam-handler-schema` type, and which type it is says
    // whether the verb answered"). A caller parsing that stream reads the first document,
    // believes the sweep answered, and is right — while the exit code says it failed.
    let scratch = Scratch::new();
    let wch = Wch::new(&scratch, &["chicony-rgb"]);
    let task = "read text from the DUT display";
    wch.ok(&[
        "calibrate",
        "start",
        "cam:integrated",
        "--task",
        task,
        "--goal",
        "legible",
        "--criterion",
        "text clarity",
    ]);
    wch.ok(&[
        "calibrate",
        "plan",
        "cam:integrated",
        "--task",
        task,
        "brightness",
    ]);

    let ran = wch.run_with_unwritable_stderr(&[
        "calibrate",
        "sweep",
        "cam:integrated",
        "--task",
        task,
        "brightness",
        "--values",
        "0,128",
        "--skip-frames",
        "0",
        "--json",
    ]);

    assert_eq!(
        ran.code, 0,
        "a sweep that ran, sampled and persisted reported a failure because it could not \
         print the sentence about it: {}",
        ran.stdout
    );
    // Exactly one document, and it is the answer. Parsed as the session it claims to be —
    // and `Failure`'s marker is checked in the same breath, because two documents
    // concatenated do not parse as one and a `Failure` alone would not be a session.
    let answer: Value = serde_json::from_str(&ran.stdout).unwrap_or_else(|error| {
        panic!(
            "standard output is not one document ({error}):\n{}",
            ran.stdout
        );
    });
    assert!(
        answer.get("id").is_some() && answer.get("controls").is_some(),
        "the one document on standard output is not the session the verb answered with: \
         {answer}"
    );
    assert!(
        !ran.stdout.contains(schema::error::FAILURE_MARKER),
        "the answer stream carries a failure marker, so a caller cannot tell an answer from \
         a refusal: {}",
        ran.stdout
    );

    // And the outcome the note was *about* really happened, so this is a test of a verb that
    // succeeded rather than of one that failed early and printed nothing.
    let dir = wch.only_session_dir();
    assert!(
        !document(&dir)["pre_snapshot"].is_null(),
        "the sweep did not borrow the camera, so the note under test would not have been \
         written at all: {}",
        document(&dir)
    );
}

#[test]
fn a_mutating_calibrate_verb_refuses_a_session_that_belongs_to_another_camera() {
    // D8: a session belongs to a (camera fingerprint, task) pair, and `lifecycle::find`
    // walks the whole tree by design so `--session <uuid>` can reach one recorded against
    // another camera — which is the only way the disagreement arises, and therefore the
    // only reason the check has anything to refuse.
    //
    // Before P3's review the check lived in `apply` alone. `plan` and `sweep` reached a
    // camera without it: a sweep through a foreign `--session` drove camera B and recorded
    // the samples in a document whose fingerprint is camera A's, after which `apply` wrote
    // B's measurements to A with its own check green. Worse for rule 8 — `arm_pre_snapshot`
    // short-circuits on the snapshot already persisted for A, so B was moved with no record
    // of it anywhere on disk and `recover` would refuse to put it back.
    let scratch = Scratch::new();
    let wch = Wch::new(&scratch, &["chicony-rgb", "obsbot-tiny3"]);
    let task = "read text from the DUT display";
    wch.ok(&["calibrate", "start", "cam:integrated", "--task", task]);
    wch.ok(&[
        "calibrate",
        "plan",
        "cam:integrated",
        "--task",
        task,
        "brightness",
    ]);
    let id = wch.json(&["calibrate", "list"])["sessions"][0]["id"]
        .as_str()
        .expect("a session id")
        .to_owned();

    // The fields that actually differ, read from the enumeration rather than transcribed.
    let cameras = wch.json(&["list"]);
    let mine = &cameras["cameras"][0]["fingerprint"];
    let theirs = &cameras["cameras"][1]["fingerprint"];
    let differing: Vec<&str> = ["bus_path", "usb_id", "card", "driver", "serial"]
        .into_iter()
        .filter(|field| !mine[field].is_null() && !theirs[field].is_null())
        .filter(|field| mine[field] != theirs[field])
        .collect();
    assert!(
        !differing.is_empty(),
        "the two replayed cameras are indistinguishable, so this arm proves nothing"
    );

    // Every mutating verb, each with the twin that must still work.
    let foreign: Vec<Vec<&str>> = vec![
        vec![
            "calibrate",
            "plan",
            "cam:obsbot",
            "--session",
            &id,
            "brightness",
        ],
        vec![
            "calibrate",
            "sweep",
            "cam:obsbot",
            "--session",
            &id,
            "brightness",
            "--values",
            "0,32",
            "--skip-frames",
            "0",
        ],
        vec![
            "calibrate",
            "select",
            "cam:obsbot",
            "--session",
            &id,
            "brightness",
            "--metric",
            "sharpness",
        ],
        vec!["calibrate", "restore", "cam:obsbot", "--session", &id],
        vec!["calibrate", "apply", "cam:obsbot", "--session", &id],
    ];
    for args in &foreign {
        let refused = wch.refuses_with(args, schema::ErrorKind::FingerprintMismatch);
        for field in &differing {
            assert!(
                refused.stderr.contains(field),
                "{args:?}: {field} differs and the refusal does not name it: {}",
                refused.stderr
            );
        }
    }

    // Nothing the foreign runs asked for landed: the OBSBOT was never swept into the
    // Chicony's document, and the Chicony's own snapshot was never armed for it.
    let dir = wch.only_session_dir();
    let session = document(&dir);
    assert!(
        session["controls"]["brightness"]["samples"]
            .as_array()
            .is_none_or(Vec::is_empty),
        "a sweep on the wrong camera recorded samples anyway: {session}"
    );
    assert!(
        session["pre_snapshot"].is_null(),
        "a refused sweep armed a snapshot: {session}"
    );

    // And the twin: the camera the session was recorded against takes the same sweep.
    wch.ok(&[
        "calibrate",
        "sweep",
        "cam:integrated",
        "--session",
        &id,
        "brightness",
        "--values",
        "0,32",
        "--skip-frames",
        "0",
    ]);
    assert_eq!(
        document(&dir)["controls"]["brightness"]["samples"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
}

// ------------------------------------------------------------------ the verbs' edges

#[test]
fn a_calibrate_verb_needs_exactly_one_session_named() {
    let scratch = Scratch::new();
    let wch = calibrated_session(&scratch);
    let task = "read text from the DUT display";

    // Neither: a usage error, because the tool cannot guess which session is meant.
    let neither = wch.refuses(&["calibrate", "status", "cam:integrated"]);
    assert_eq!(neither.code, 2, "{}", neither.stderr);
    // Both: the same, from the other side.
    let id = wch.json(&["calibrate", "list"])["sessions"][0]["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let both = wch.refuses(&[
        "calibrate",
        "status",
        "cam:integrated",
        "--task",
        task,
        "--session",
        &id,
    ]);
    assert_eq!(both.code, 2, "{}", both.stderr);

    // Either one alone answers, and with the same session.
    let by_task = wch.json(&["calibrate", "status", "cam:integrated", "--task", task]);
    let by_id = wch.json(&["calibrate", "status", "cam:integrated", "--session", &id]);
    assert_eq!(by_task, by_id);
}

#[test]
fn a_task_nobody_started_is_refused_rather_than_invented() {
    let scratch = Scratch::new();
    let wch = calibrated_session(&scratch);
    let refused = wch.refuses(&[
        "calibrate",
        "status",
        "cam:integrated",
        "--task",
        "a task nobody started",
    ]);
    assert_eq!(
        refused.code,
        i32::from(cli_core::exit_code(&schema::Error::sample(
            schema::ErrorKind::IllegalTransition
        )))
    );
    assert!(
        refused.stderr.contains("no_session"),
        "the refusal must say what was not found: {}",
        refused.stderr
    );
}

#[test]
fn a_sweep_that_does_not_say_how_big_it_is_is_refused_before_the_camera_moves() {
    // A sweep is minutes of camera time and, on a PTZ head, motor travel (design §5). The
    // plan is a required group, so the expensive choice cannot be the silent one.
    let scratch = Scratch::new();
    let wch = calibrated_session(&scratch);
    let task = "read text from the DUT display";
    let refused = wch.refuses(&[
        "calibrate",
        "sweep",
        "cam:integrated",
        "--task",
        task,
        "contrast",
    ]);
    assert_eq!(refused.code, 2, "{}", refused.stderr);

    // The twin, and the cheapest plan that says something: one explicit value.
    wch.ok(&[
        "calibrate",
        "sweep",
        "cam:integrated",
        "--task",
        task,
        "contrast",
        "--values",
        "32",
        "--skip-frames",
        "0",
    ]);
}

#[test]
fn a_plan_covers_every_control_and_says_why_the_ones_it_cannot_calibrate_are_out() {
    // The skill's step 6 — "cover all the setting names" — and the reason a blocked control
    // is recorded rather than omitted: a draft that dropped them would read as a device
    // that does not have them.
    let scratch = Scratch::new();
    let wch = Wch::new(&scratch, &["chicony-rgb"]);
    let task = "everything";
    wch.ok(&["calibrate", "start", "cam:integrated", "--task", task]);
    let session = wch.json(&["calibrate", "plan", "cam:integrated", "--task", task]);

    let controls = wch.json(&["controls", "cam:integrated"]);
    let every: Vec<&str> = controls["controls"]
        .as_array()
        .expect("controls")
        .iter()
        .map(|desc| desc["slug"].as_str().expect("a slug"))
        .collect();
    assert!(
        every.len() > 5,
        "the fixture is too small to prove anything"
    );

    let queued: Vec<&str> = session["queue"]
        .as_array()
        .expect("a queue")
        .iter()
        .map(|slug| slug.as_str().expect("a slug"))
        .collect();
    let recorded = session["controls"].as_object().expect("controls");
    for slug in &every {
        assert!(
            queued.contains(slug) || recorded.contains_key(*slug),
            "{slug} is on the camera and in neither the queue nor the record: {session}"
        );
    }

    // The device's own reasons, not a shrug: this profile carries a read-only control
    // [PF:12] and a compound one [PF:1], and both are blocked for what they are.
    let blocked: Vec<(&str, &str)> = recorded
        .iter()
        .filter(|(_, entry)| entry["status"]["status"] == "blocked")
        .map(|(slug, entry)| {
            (
                slug.as_str(),
                entry["status"]["reason"]["kind"]
                    .as_str()
                    .expect("a reason kind"),
            )
        })
        .collect();
    assert!(
        blocked.iter().any(|(_, kind)| *kind == "read_only"),
        "no control was blocked read-only: {blocked:?}"
    );
    assert!(
        blocked.iter().any(|(_, kind)| *kind == "not_sweepable"),
        "no control was blocked as unsweepable: {blocked:?}"
    );
    // And a blocked control is *terminal*, so a session of nothing but blocked controls
    // settles rather than blocking `apply` forever.
    assert!(
        !queued
            .iter()
            .any(|slug| blocked.iter().any(|(b, _)| b == slug)),
        "a blocked control was queued for sweeping too"
    );
}

#[test]
fn a_plan_queues_the_motorised_controls_because_motion_is_a_sweep_flag_and_not_a_refusal() {
    // `lifecycle::uncalibratable` asks the sweep planner with `allow_motion = true`, and N19
    // states the reason as law: that a control moves motors is a reason a *sweep* needs a
    // flag (design §5), not a reason the control cannot be calibrated. Nothing pinned the
    // `true` — the P3 review flipped it to `false` and the whole workspace stayed green,
    // because the only test that drafts a whole device drafts the motor-less Chicony. This
    // is that arm, on the PTZ camera the rule exists for.
    //
    // Both directions of the same rule, which is the point: the control is *draftable*
    // (queued, not blocked) and it is not *implicitly sweepable* (the sweep is refused
    // without `--allow-motion` and accepted with it). A build that blocked PTZ controls at
    // draft time would fail the first half; one that swept motors without being told would
    // fail the second.
    let scratch = Scratch::new();
    let wch = Wch::new(&scratch, &["obsbot-tiny3"]);
    let task = "framing";
    wch.ok(&["calibrate", "start", "cam:obsbot", "--task", task]);
    let session = wch.json(&["calibrate", "plan", "cam:obsbot", "--task", task]);

    let queued: Vec<&str> = session["queue"]
        .as_array()
        .expect("a queue")
        .iter()
        .map(|slug| slug.as_str().expect("a slug"))
        .collect();
    let recorded = session["controls"].as_object().expect("controls");
    // Read from the device's own enumeration rather than transcribed, so a re-capture that
    // renames a motor control does not quietly turn this into a test of nothing.
    let controls = wch.json(&["controls", "cam:obsbot"]);
    let motorised: Vec<&str> = controls["controls"]
        .as_array()
        .expect("controls")
        .iter()
        .filter_map(|desc| desc["slug"].as_str())
        .filter(|slug| slug.starts_with("pan") || slug.starts_with("tilt"))
        .collect();
    assert!(
        !motorised.is_empty(),
        "the replayed camera has no motorised control, so this arm proves nothing: \
         {controls}"
    );
    for slug in &motorised {
        assert!(
            queued.contains(slug),
            "{slug} moves motors and the draft left it out of the queue: {session}"
        );
        assert!(
            recorded
                .get(*slug)
                .is_none_or(|entry| entry["status"]["status"] != "blocked"),
            "{slug} was blocked at draft time; a PTZ camera's whole purpose is uncalibratable: \
             {session}"
        );
    }

    // The other half: a plan that would move motors still says so first (§5's product
    // default). One control, two steps, and only because `--allow-motion` was typed.
    let control = motorised.first().expect("a motorised control");
    let refused = wch.refuses(&[
        "calibrate",
        "sweep",
        "cam:obsbot",
        "--task",
        task,
        control,
        "--values",
        "0,3600",
        "--skip-frames",
        "0",
    ]);
    assert!(
        refused.stderr.contains("motion"),
        "the refusal did not name the reason: {}",
        refused.stderr
    );
    let swept = wch.json(&[
        "calibrate",
        "sweep",
        "cam:obsbot",
        "--task",
        task,
        control,
        "--values",
        "0,3600",
        "--allow-motion",
        "--skip-frames",
        "0",
    ]);
    assert_eq!(
        swept["controls"][control]["samples"]
            .as_array()
            .map(Vec::len),
        Some(2),
        "the sweep the flag allowed took no samples: {swept}"
    );
}

#[test]
fn the_queue_can_be_reordered_between_sweeps_and_only_into_a_permutation_of_itself() {
    // The skill's step 10.5, and D8's "the caller may reorder between sweeps": on this
    // camera exposure moves focus, so an operator who queued them the wrong way round has
    // to be able to say so — and a reorder that quietly dropped a control would erase
    // queued work.
    let scratch = Scratch::new();
    let wch = Wch::new(&scratch, &["chicony-rgb"]);
    let task = "ordering";
    wch.ok(&["calibrate", "start", "cam:integrated", "--task", task]);
    let queued = wch.json(&[
        "calibrate",
        "plan",
        "cam:integrated",
        "--task",
        task,
        "brightness",
        "contrast",
    ]);
    assert_eq!(
        queued["queue"],
        serde_json::json!(["brightness", "contrast"])
    );

    let reordered = wch.json(&[
        "calibrate",
        "plan",
        "cam:integrated",
        "--task",
        task,
        "--order",
        "contrast",
        "brightness",
    ]);
    assert_eq!(
        reordered["queue"],
        serde_json::json!(["contrast", "brightness"]),
        "the queue kept its old order"
    );

    // Not a permutation: dropping one, and adding one that was never queued.
    for order in [vec!["contrast"], vec!["contrast", "brightness", "gamma"]] {
        let mut args = vec![
            "calibrate",
            "plan",
            "cam:integrated",
            "--task",
            task,
            "--order",
        ];
        args.extend(order);
        let refused = wch.refuses(&args);
        assert!(
            refused.stderr.contains("not_a_permutation"),
            "{}",
            refused.stderr
        );
    }
    // …and nothing moved.
    assert_eq!(
        wch.json(&["calibrate", "status", "cam:integrated", "--task", task])["session"]["queue"],
        serde_json::json!(["contrast", "brightness"])
    );
}

#[test]
fn a_second_session_for_one_camera_and_task_is_refused_naming_the_one_that_is_open() {
    // N14, through the verb: `calibrate start` twice is a resume, not a second directory.
    let scratch = Scratch::new();
    let wch = calibrated_session(&scratch);
    let task = "read text from the DUT display";
    let refused = wch.refuses_with(
        &["calibrate", "start", "cam:integrated", "--task", task],
        schema::ErrorKind::SessionConflict,
    );
    let id = wch.json(&["calibrate", "list"])["sessions"][0]["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    assert!(
        refused.stderr.contains(&id),
        "a conflict nobody can act on: {}",
        refused.stderr
    );
    // …and a different task on the same camera is a different session, not a conflict.
    wch.ok(&[
        "calibrate",
        "start",
        "cam:integrated",
        "--task",
        "another task",
    ]);
    assert_eq!(
        wch.json(&["calibrate", "list"])["sessions"]
            .as_array()
            .expect("sessions")
            .len(),
        2
    );
}

#[test]
fn list_shows_every_session_and_one_cameras_when_a_camera_is_named() {
    // The listing is a directory walk with no parse behind it (D9), which is what lets it
    // show a session this build could not load — and what makes the camera filter a real
    // filter rather than a `session.fingerprint` comparison.
    let scratch = Scratch::new();
    let wch = Wch::new(&scratch, &["chicony-rgb", "obsbot-tiny3"]);
    wch.ok(&["calibrate", "start", "cam:integrated", "--task", "here"]);
    wch.ok(&["calibrate", "start", "cam:obsbot", "--task", "there"]);

    let cameras: Vec<String> = wch.json(&["calibrate", "list"])["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .map(|found| found["camera"].as_str().expect("a camera").to_owned())
        .collect();
    assert_eq!(
        cameras.len(),
        2,
        "both sessions belong in an unfiltered list"
    );

    let one = wch.json(&["calibrate", "list", "cam:obsbot"]);
    let listed = one["sessions"].as_array().expect("sessions");
    assert_eq!(listed.len(), 1, "a named camera lists only its own: {one}");
    assert_eq!(listed[0]["task_slug"], "there");
    // The camera slug is the directory's, so the row points at where the session is.
    let camera = listed[0]["camera"].as_str().expect("a camera");
    assert!(
        listed[0]["path"].as_str().expect("a path").contains(camera),
        "{one}"
    );
    assert!(
        cameras.contains(&camera.to_owned()),
        "the filtered listing named a camera the full one did not: {one}"
    );
}

#[test]
fn the_session_listing_takes_every_spelling_the_other_verbs_take() {
    // **The one camera positional that is not a `CameraArg`.** Every other camera-taking verb
    // flattens that struct, whose `selector()` is `schema::selector::parse`, so they cannot
    // fork; `calibrate list`'s camera is optional, clap flattens a struct rather than an
    // `Option` of one, and it therefore parses on a line of its own (`cli_core`'s
    // `CalibrateCommand::List` arm). Until note **N303** the only test on that positional
    // passed `cam:obsbot` — an `Id` spelling `CameraId::parse` accepts identically — so
    // swapping that line for an id-only parser left the whole workspace green while `calibrate
    // list bus:3-4:1.0` started refusing. This is the arm that goes red on a second grammar for
    // this one verb.
    //
    // The spellings are read out of the committed profile the fake replays, not out of this
    // build's own answer about the camera: a test that asked `info` for the bus path and then
    // asked `calibrate list` about it would be asking one resolver to agree with itself (note
    // **N252**). The walk is over `SelectorScheme::ALL` so a sixth scheme has no sample and
    // fails here rather than quietly going untested.
    let scratch = Scratch::new();
    let wch = Wch::new(&scratch, &["chicony-rgb", "obsbot-tiny3"]);
    wch.ok(&["calibrate", "start", "cam:integrated", "--task", "here"]);
    wch.ok(&["calibrate", "start", "cam:obsbot", "--task", "there"]);

    let profile: Value = serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("corpus/profiles/chicony-rgb.json"))
            .expect("the committed profile is readable"),
    )
    .expect("the committed profile is a document");
    let info = &profile["invariant"]["info"];
    let fingerprint = &info["fingerprint"];
    let node = info["nodes"][0]["path"].as_str().expect("a node path");
    let usb = &fingerprint["usb_id"];
    let spellings: Vec<(SelectorScheme, String)> = vec![
        (SelectorScheme::Id, "cam:integrated".to_owned()),
        (SelectorScheme::NodePath, node.to_owned()),
        (
            SelectorScheme::BusPath,
            format!(
                "bus:{}",
                fingerprint["bus_path"].as_str().expect("a bus path")
            ),
        ),
        (
            SelectorScheme::UsbId,
            format!(
                "usb:{:04x}:{:04x}",
                usb["vendor"].as_u64().expect("a vendor id"),
                usb["product"].as_u64().expect("a product id")
            ),
        ),
        (
            SelectorScheme::Serial,
            format!(
                "serial:{}",
                fingerprint["serial"].as_str().expect("a serial")
            ),
        ),
    ];
    assert_eq!(
        spellings.len(),
        SelectorScheme::ALL.len(),
        "a scheme joined the vocabulary and has no sample here"
    );

    for (scheme, spelling) in spellings {
        let one = wch.json(&["calibrate", "list", &spelling]);
        let listed = one["sessions"].as_array().expect("sessions");
        assert_eq!(
            listed.len(),
            1,
            "{spelling} ({scheme:?}) listed {} session(s): {one}",
            listed.len()
        );
        assert_eq!(
            listed[0]["task_slug"], "here",
            "{spelling} ({scheme:?}) listed the other camera's session: {one}"
        );
    }

    // The refusal at the same door, and it is what stops the loop above reading as a pass for a
    // positional that ignores its argument: a spelling no camera can ever match is refused here
    // exactly as it is everywhere else, naming the whole vocabulary rather than one grammar.
    let refused = wch.refuses(&["calibrate", "list", "nope:1", "--json"]);
    let document: Value =
        serde_json::from_str(&refused.stdout).expect("a failing --json run prints a document");
    assert_eq!(
        document["error"]["kind"], "camera_unknown",
        "{}",
        refused.stdout
    );
    let message = document["message"].as_str().expect("a message");
    for scheme in SelectorScheme::ALL {
        assert!(
            message.contains(scheme.example()),
            "the refusal does not teach {scheme:?}: {message}"
        );
    }
}

#[test]
fn a_refusal_from_outside_the_schema_crate_ends_with_its_instruction_too() {
    // **D13's `IllegalTransition` rendering, held over a producer that is not
    // `webcam-handler-schema`'s.** `an_illegal_transitions_instruction_is_the_last_thing_it_says`
    // drives all seven of that crate's producers, and its comment used to say that
    // `a_refusal_ends_with_the_instruction_its_payload_carries` drives one of the others through
    // the shipped binary. It does not: that arm runs `photo … -o x.tiff`, whose refusal is
    // `schema::capture::Sink::writable_format` — the *first* element of the same array. So no
    // producer outside the schema crate had its `op` driven at all, and the design's sentence
    // about "every producer" rested on a population of one crate (note **N303**).
    //
    // This one is the engine's, reached through the whole stack: `calibrate select` before any
    // sweep, refused by `engine::session` through `engine::refusal::illegal`, rendered by
    // `cli_core::report`. The assertions take both halves out of the document the same run
    // printed rather than pinning today's wording, which is note **N211**'s shape.
    let scratch = Scratch::new();
    let wch = Wch::new(&scratch, &["chicony-rgb"]);
    wch.ok(&["calibrate", "start", "cam:integrated", "--task", "t"]);

    let refused = wch.refuses(&[
        "--json",
        "calibrate",
        "select",
        "cam:integrated",
        "brightness",
        "--task",
        "t",
        "--value",
        "10",
        "--by",
        "human",
    ]);
    let document: Value =
        serde_json::from_str(&refused.stdout).expect("a failing --json run prints a document");
    assert_eq!(
        document["error"]["kind"], "illegal_transition",
        "{}",
        refused.stdout
    );
    let from = document["error"]["from"].as_str().expect("a from state");
    let op = document["error"]["op"].as_str().expect("an instruction");
    let message = document["message"].as_str().expect("a message");
    assert!(
        message.ends_with(op),
        "the instruction is not the last thing this refusal says, so an agent reading the \
         sentence gets the condition glued to the end of it: {message}"
    );
    assert!(
        message.starts_with(from),
        "the refusal no longer says what state it refused from: {message}"
    );
    // Not vacuous: an `op` that had become empty would satisfy `ends_with` for nothing.
    assert!(!op.is_empty(), "the refusal carries no instruction at all");
}

#[test]
fn nothing_a_calibration_writes_lands_where_the_repository_can_see_it() {
    // Rubric A12 as a habit: sample photos are frames, and a frame may contain a person.
    // These are synthetic; the discipline is the point, and this asserts the state
    // directory really is somewhere the repository cannot mistake for its own.
    //
    // **This asserted "outside the tree" until the 2026-08-12 ruling made that false on
    // purpose** (note N84): test scratch lives under `target/` now. The replacement is in
    // two halves and is stronger than what it replaces — `schema::paths`'s own tests carry
    // the general law (the root is inside the build directory, and the `.gitignore` beside
    // it names `/target/`), and this asserts the local one: a whole session tree, sample
    // photos and all, is under that root.
    let scratch = Scratch::new();
    let wch = calibrated_session(&scratch);
    let root = schema::paths::scratch_root()
        .expect("a scratch root")
        .canonicalize_utf8()
        .expect("the scratch root resolves");
    let dir = wch
        .only_session_dir()
        .canonicalize_utf8()
        .expect("the session directory resolves");
    assert!(
        dir.starts_with(&root),
        "{dir} is not under {root}; a session tree belongs in the one scratch root and nowhere else"
    );
    let photos = dir.join(schema::limits::SESSION_PHOTOS_DIR);
    assert!(photos.is_dir(), "{photos} should hold the sample photos");
}

// ------------------------------------------------------------------ the state-dir lock

/// The one holder every refusal here has to name.
fn this_process() -> String {
    format!("pid {}", std::process::id())
}

#[test]
fn a_wch_meeting_a_daemons_lock_is_sent_to_wchc_and_its_read_verbs_still_answer() {
    // Design D9, verbatim: "`webcam-handler-cli` finding it held reports *daemon owns the
    // state (and likely the camera) — use webcam-handler-client* rather than corrupting or
    // blocking (D13)". The sentence is asserted here because this is where a person reads it —
    // `webcam-handler-cli` renders the typed error onto stderr and exits — and the value it is
    // rendered from is asserted in `webcam-handler-schema` and in
    // `crates/daemon/tests/lock.rs`.
    let scratch = Scratch::new();
    let wch = Wch::new(&scratch, &["chicony-rgb"]);
    let task = "read text from the DUT display";
    let daemon = scratch.held_by_somebody_else(LockProtocol::HeldForLifetime);

    let refused = wch.refuses(&["calibrate", "start", "cam:integrated", "--task", task]);
    assert!(
        refused
            .stderr
            .contains("daemon owns the state (and likely the camera) — use webcam-handler-client"),
        "{}",
        refused.stderr
    );
    // Advice without a holder is advice nobody can check, so the line names who as well.
    assert!(
        refused.stderr.contains(&this_process()),
        "{}",
        refused.stderr
    );
    // The exit code says which of the eighteen this was (note **N127**): `store_locked` has
    // one of its own, so a script can tell "a daemon has the state directory" from "the camera
    // is busy" without reading the sentence. Read out of `cli_core::exit_code`, the one home.
    assert_eq!(
        refused.code,
        i32::from(cli_core::exit_code(&schema::Error::sample(
            schema::ErrorKind::StoreLocked
        ))),
        "{}",
        refused.stderr
    );

    // Reading is not a state write. This is the half that makes a daemon's lifetime hold
    // liveable for whoever is sitting at the machine, and it is stated as law on
    // `InProcess::calibrate_status`: a status verb that refused while a daemon held the
    // lock would be a status verb nobody can use where the sessions are.
    let listed = wch.json(&["calibrate", "list"]);
    assert_eq!(
        listed["sessions"]
            .as_array()
            .expect("a sessions array")
            .len(),
        0,
        "{listed}"
    );

    // And released, the very same command goes through — so what was refused was the lock
    // and not the command.
    drop(daemon);
    wch.ok(&["calibrate", "start", "cam:integrated", "--task", task]);
    assert_eq!(
        wch.json(&["calibrate", "list"])["sessions"]
            .as_array()
            .expect("a sessions array")
            .len(),
        1
    );
}

#[test]
fn another_wchs_momentary_lock_is_refused_without_sending_anybody_to_a_daemon() {
    // The other direction of D9's advice, and what makes the first test an assertion about a
    // *decision* rather than about a constant: another `webcam-handler-cli` a few milliseconds
    // into a mutating verb will be gone shortly, and telling its user to go and start
    // `webcam-handler-client` would send them after a daemon that does not exist.
    let scratch = Scratch::new();
    let wch = Wch::new(&scratch, &["chicony-rgb"]);
    let task = "read text from the DUT display";
    let peer = scratch.held_by_somebody_else(LockProtocol::PerOperation);

    let refused = wch.refuses(&["calibrate", "start", "cam:integrated", "--task", task]);
    assert!(
        refused.stderr.contains(&this_process()),
        "{}",
        refused.stderr
    );
    assert!(
        refused.stderr.contains("free shortly"),
        "{}",
        refused.stderr
    );
    assert!(
        !refused.stderr.contains("webcam-handler-client"),
        "{}",
        refused.stderr
    );

    drop(peer);
    wch.ok(&["calibrate", "start", "cam:integrated", "--task", task]);
}
