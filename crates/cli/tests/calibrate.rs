//! `wch calibrate` end to end, against the fake backend (docs/7 P3d, gate G3).
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
use serde_json::{Map, Value};

/// The `wch` binary this test drives, built by cargo alongside it.
fn wch() -> Command {
    Command::new(env!("CARGO_BIN_EXE_wch"))
}

fn repo_root() -> Utf8PathBuf {
    Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A state directory outside the repository, thrown away with the test.
struct Scratch {
    dir: tempfile::TempDir,
}

impl Scratch {
    fn new() -> Scratch {
        Scratch {
            dir: tempfile::tempdir().expect("a scratch directory"),
        }
    }

    fn state(&self) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(self.dir.path().join("state")).expect("a utf-8 temp dir")
    }
}

/// A `wch` bound to one state directory and a set of replayed cameras.
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
    /// `wch` over the named committed profiles, writing sessions under `scratch`.
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
        let output = command.args(args).output().expect("wch runs");
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
            "wch {args:?} exited {}: {}",
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
            panic!("wch {args:?} did not emit JSON: {error}\n{}", ran.stdout)
        })
    }

    /// Run, asserting it refused, and hand back what it said.
    fn refuses(&self, args: &[&str]) -> Ran {
        let ran = self.run(args);
        assert_ne!(
            ran.code, 0,
            "wch {args:?} was expected to refuse: {}",
            ran.stdout
        );
        ran
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
    let refused = wch.refuses(&["calibrate", "apply", "cam:obsbot", "--session", &id]);
    assert_eq!(refused.code, 1, "a device refusal, not a usage error");

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

    let refused = wch.refuses(&["calibrate", "apply", "cam:integrated", "--task", task]);
    assert_eq!(refused.code, 1);
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
    assert_eq!(refused.code, 1);
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
    let refused = wch.refuses(&["calibrate", "start", "cam:integrated", "--task", task]);
    assert_eq!(refused.code, 1);
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
fn nothing_a_calibration_writes_lands_in_the_repository() {
    // Rubric A12 as a habit: sample photos are frames, and a frame may contain a person.
    // These are synthetic; the discipline is the point, and this asserts the state
    // directory really is outside the tree.
    let scratch = Scratch::new();
    let wch = calibrated_session(&scratch);
    let repo = repo_root()
        .canonicalize_utf8()
        .expect("the repository root resolves");
    let dir = wch.only_session_dir();
    assert!(
        !dir.starts_with(&repo),
        "{dir} is inside {repo}; sessions must not be written into the tree"
    );
    let photos = dir.join(schema::limits::SESSION_PHOTOS_DIR);
    assert!(photos.is_dir(), "{photos} should hold the sample photos");
}
