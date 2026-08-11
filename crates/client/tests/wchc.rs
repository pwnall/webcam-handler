//! `wchc` end to end: the refusals it owns, and the verbs it answers over a real socket.
//!
//! One file, deliberately, where `crates/daemon/tests/` is eight. Its subject is a single
//! thing — the shipped `wchc` — and the two halves below need the same two helpers, so a
//! `#[path]`-included `support/` module here would be note **N49**'s hazard (an item used by
//! one includer and dead in the other) bought for nothing.
//!
//! ## The two halves
//!
//! **Without a daemon.** Everything `wchc` decides on its own: the flags it refuses because
//! it is a client and not a composition root, the refusal for a socket nothing is listening
//! on, and the three exit codes. These run the shipped binary as a subprocess, because the
//! exit code and the `wchc:` line are process facts.
//!
//! **With one.** A real `wchd` beside a real `wchc`, replaying a committed device profile —
//! the same arrangement `crates/daemon/tests/systemd.rs` and `signals.rs` use, and for the
//! same reason (`support/supervised.rs`: a v4l2 daemon "would be reporting whatever is
//! plugged into the machine running CI"). Some of these drive the binary and some drive the
//! [`client::remote::Remote`] executor directly; which one is a question of what can be
//! observed, and each says so where it matters.
//!
//! ## `wchd` is resolved, not assumed
//!
//! `env!("CARGO_BIN_EXE_wchd")` is defined only for the package that declares that binary,
//! and this is not it. So the daemon is looked up **beside** `wchc`, which cargo does
//! guarantee a path to, and its absence is a loud failure naming what to run rather than a
//! skip: a suite that quietly passed without the daemon it is about would be the "skip that
//! reads as pass" AGENTS bans.
//!
//! ## Nothing here sleeps
//!
//! Starting the daemon waits on a **line from its stderr pipe** — a read that ends when the
//! writer writes — which is `crates/daemon/tests/support/wchd.rs`'s bound for its reason: a
//! daemon that cannot serve says why and exits, closing the pipe. A sweep's progress is
//! asserted from the events themselves. There is no duration anywhere in this file.

use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::Mutex;

use camino::Utf8PathBuf;
use cli_core::{Executor as _, SessionRef, SweepWatcher};
use engine::paths::TempRuntimeDir;
use engine::store::TempStore;
use schema::camera::CameraId;
use schema::capture::{PhotoFormat, SettlePolicy, StreamRequest};
use schema::control::ControlSlug;
use schema::progress::{CalibrationProgress, ProgressEvent};
use schema::session::{SweepRequest, SweepSpec};
use serde_json::Value;

/// The `wchc` binary this suite drives, built by cargo alongside it.
fn wchc() -> Command {
    Command::new(env!("CARGO_BIN_EXE_wchc"))
}

/// The `wchd` binary beside it.
///
/// `CARGO_BIN_EXE_*` exists only for the package that declares the binary, so this is a
/// sibling lookup rather than an environment variable — and a missing sibling is a panic
/// naming the command that produces it, because "the daemon was not built" and "the daemon
/// misbehaved" must not look the same from here.
fn wchd() -> Command {
    let wchc = Utf8PathBuf::from(env!("CARGO_BIN_EXE_wchc"));
    let wchd = wchc
        .parent()
        .expect("the test binary's directory")
        .join("wchd");
    assert!(
        wchd.exists(),
        "wchd is not beside wchc at {wchd}; this suite drives the shipped daemon, so build \
         the workspace (`cargo nextest run --workspace`, which is what `just ci` runs) \
         rather than this package alone"
    );
    Command::new(wchd)
}

fn repo_root() -> Utf8PathBuf {
    Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The camera every test here replays unless it says otherwise.
///
/// A fixed camera, so what the assertions below say about "the camera" is a fact about a
/// document in this repository rather than about the machine running CI.
const REPLAYED: &str = "chicony-rgb";

/// A committed device profile, by name.
///
/// Named rather than fixed, because one test needs a *different* camera: the probe that
/// `controls --discover-pairs` runs has something to say only about a device with a motor
/// on it, and saying it is how that test tells the two wire methods apart.
fn profile(name: &str) -> Utf8PathBuf {
    let path = repo_root()
        .join("corpus/profiles")
        .join(format!("{name}.json"));
    assert!(path.exists(), "the corpus is missing {path}");
    path
}

/// A private pair of XDG directories and, when a test asks for one, a daemon in them.
struct Fixture {
    runtime: TempRuntimeDir,
    state: TempStore,
}

impl Fixture {
    fn new() -> Fixture {
        Fixture {
            runtime: TempRuntimeDir::new().expect("a runtime directory"),
            state: TempStore::new().expect("a state directory"),
        }
    }

    /// The socket a daemon started here would bind.
    ///
    /// Composed from the two homes the daemon composes it from, never written out — the
    /// same rule `crates/daemon/tests/support/wchd.rs` follows, so a fixture cannot drift
    /// away from the path it is waiting for.
    fn socket(&self) -> Utf8PathBuf {
        schema::paths::runtime_dir(&self.runtime.env())
            .expect("the fixture sets the runtime variable")
            .join(schema::limits::DAEMON_SOCKET_FILE)
    }

    /// A `wchc` bound to these directories, with the arguments given.
    fn wchc(&self, args: &[&str]) -> Command {
        let mut command = wchc();
        command
            // Environment variables rather than flags, because that is how the shipped
            // client finds the socket (note N2): a test that passed a path in would not be
            // exercising the resolution an operator gets.
            .env("XDG_RUNTIME_DIR", self.runtime.base().as_str())
            .args(args);
        command
    }

    /// Run `wchc` and collect everything the process produced.
    fn run(&self, args: &[&str]) -> Ran {
        let output = self.wchc(args).output().expect("wchc runs");
        Ran {
            code: output.status.code().unwrap_or(-1),
            stdout: output.stdout,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    /// Start a `wchd` replaying a committed profile, and wait until it is serving.
    fn daemon(&self, replaying: &str) -> Daemon {
        let socket = self.socket();
        let mut command = wchd();
        command
            .env("XDG_RUNTIME_DIR", self.runtime.base().as_str())
            .env("XDG_STATE_HOME", self.state.root().as_str())
            // Pinned rather than inherited: this suite learns that the daemon is up by
            // reading the line it logs, and a developer with `RUST_LOG=warn` exported would
            // otherwise watch it wait for a line nobody was going to write.
            .env("RUST_LOG", "info")
            // Removed in both directions: run from a `systemd-run` shell, these daemons
            // would otherwise notify somebody else's service manager and serve from
            // somebody else's listener.
            .env_remove("NOTIFY_SOCKET")
            .env_remove("LISTEN_FDS")
            .env_remove("LISTEN_PID")
            .args([
                "--backend",
                "fake",
                "--profile",
                profile(replaying).as_str(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        let mut child = command.spawn().expect("wchd is built beside this test");
        let stderr = BufReader::new(child.stderr.take().expect("stderr was piped"));
        let mut daemon = Daemon {
            child,
            stderr,
            said: Vec::new(),
        };
        // The daemon announces the socket it is serving. Waiting for *that* line is a
        // readiness signal from the process rather than a guess about how long starting
        // takes; end-of-file without it means it refused to start, and the panic carries
        // everything it said.
        daemon.wait_for(socket.as_str());
        daemon
    }
}

/// What one `wchc` invocation produced.
struct Ran {
    code: i32,
    /// Bytes, not a string: a photo with no `-o` **is** standard output.
    stdout: Vec<u8>,
    stderr: String,
}

impl Ran {
    /// The answer, asserting the verb succeeded.
    fn ok(&self) -> &[u8] {
        assert_eq!(self.code, 0, "wchc failed: {}", self.stderr);
        &self.stdout
    }

    /// The `--json` answer as a document.
    fn json(&self) -> Value {
        serde_json::from_slice(self.ok()).unwrap_or_else(|error| {
            panic!(
                "not a JSON document ({error}): {}",
                String::from_utf8_lossy(&self.stdout)
            )
        })
    }
}

/// A running `wchd`, killed with the value.
struct Daemon {
    child: Child,
    stderr: BufReader<ChildStderr>,
    said: Vec<String>,
}

impl Daemon {
    fn wait_for(&mut self, wanted: &str) {
        loop {
            let mut line = String::new();
            match self.stderr.read_line(&mut line) {
                Ok(0) | Err(_) => panic!(
                    "wchd exited without saying {wanted:?}; it said:\n{}",
                    self.said.join("\n")
                ),
                Ok(_) => {
                    self.said.push(line.trim_end().to_owned());
                    if line.contains(wanted) {
                        return;
                    }
                }
            }
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        // `SIGKILL`, deliberately: this is the teardown a *failed assertion* gets, and what
        // it has to guarantee is that no daemon is left holding a temporary directory that
        // is about to be deleted. An orderly stop is `crates/daemon/tests/signals.rs`'s
        // subject, not a thing a `Drop` here should wait for.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------- without a daemon

#[test]
fn a_socket_nothing_is_listening_on_is_refused_by_name_and_leaves_code_one() {
    // The refusal `crates/daemon/tests/support/mod.rs` calls "the refusal P4f's `wchc` has
    // to render". It **names the socket it tried**, because "no daemon is running" and
    // "your `$XDG_RUNTIME_DIR` is not what you think" are different problems with the same
    // symptom and only the path tells them apart.
    let fixture = Fixture::new();
    let ran = fixture.run(&["list"]);

    assert_eq!(ran.code, 1, "{}", ran.stderr);
    assert!(
        ran.stderr.starts_with("wchc: "),
        "the line names the program that met the error: {}",
        ran.stderr
    );
    assert!(
        ran.stderr.contains(fixture.socket().as_str()),
        "the refusal has to name the socket it tried: {}",
        ran.stderr
    );
    assert!(ran.stdout.is_empty(), "a refusal answers nothing on stdout");
}

#[test]
fn no_runtime_directory_at_all_is_a_different_refusal_from_no_daemon() {
    // The other half of "where is the socket", and it must not read as the first: a process
    // with no `$XDG_RUNTIME_DIR` is not in a login session, which is something to be told
    // rather than worked around (`schema::paths::runtime_dir` has no `/tmp` fallback).
    let mut command = wchc();
    command.env_remove("XDG_RUNTIME_DIR").arg("list");
    let output = command.output().expect("wchc runs");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("XDG_RUNTIME_DIR"), "{stderr}");
    // And not the *other* refusal: this one never got as far as a socket to name.
    assert!(
        !stderr.contains(schema::limits::DAEMON_SOCKET_FILE),
        "{stderr}"
    );
}

#[test]
fn the_flags_a_client_cannot_honour_are_refused_before_the_socket_is_touched() {
    // Ordering, asserted from outside: the fixture below has no daemon, so a build that
    // connected first would produce the *socket's* refusal and this test would see it. That
    // is what makes "before" a claim rather than a comment.
    let fixture = Fixture::new();
    for args in [
        ["--backend", "v4l2", "list"].as_slice(),
        ["--backend", "fake", "--profile", "p.json", "list"].as_slice(),
        ["--profile", "p.json", "list"].as_slice(),
    ] {
        let ran = fixture.run(args);
        assert_eq!(ran.code, 1, "{args:?}: {}", ran.stderr);
        assert!(ran.stderr.starts_with("wchc: "), "{}", ran.stderr);
        // It names the place the decision does live, which is the difference between a
        // refusal and a wall.
        assert!(ran.stderr.contains("wchd"), "{args:?}: {}", ran.stderr);
        assert!(
            !ran.stderr.contains(schema::limits::DAEMON_SOCKET_FILE),
            "{args:?}: the socket was reached before the flag was refused: {}",
            ran.stderr
        );
    }
}

#[test]
fn a_command_line_that_is_not_one_leaves_claps_code_and_names_this_binary() {
    // The third outcome `cli_core::exit_code`'s table fixes, and the one that is not ours:
    // 2 is clap's, and "you typed it wrong" must stay distinct from "the daemon is not
    // there" for a script deciding whether to retry.
    let fixture = Fixture::new();
    for args in [
        ["frobnicate"].as_slice(),
        // The cross-argument rule `Cli::check` adds, which is a usage error too.
        ["--json", "photo", "cam:x"].as_slice(),
        // A value outside a closed vocabulary.
        ["--transform", "rot45"].as_slice(),
    ] {
        let ran = fixture.run(args);
        assert_eq!(ran.code, 2, "{args:?}: {}", ran.stderr);
        // The usage block names *this* binary, so an operator with both is sent to the
        // right `--help` (`Program`'s whole reason for existing).
        assert!(ran.stderr.contains("wchc"), "{args:?}: {}", ran.stderr);
        assert!(!ran.stderr.contains("wch "), "{args:?}: {}", ran.stderr);
    }

    // …and the inverse, without which "exit 2" would prove nothing: a well-formed command
    // line that needs no daemon answers 0.
    let version = fixture.run(&["--version"]);
    assert_eq!(version.code, 0, "{}", version.stderr);
    assert!(
        String::from_utf8_lossy(version.ok()).starts_with("wchc "),
        "{}",
        String::from_utf8_lossy(&version.stdout)
    );
}

// ------------------------------------------------------------------- with a daemon

#[test]
fn the_read_verbs_answer_the_camera_the_daemon_is_replaying() {
    let fixture = Fixture::new();
    let _daemon = fixture.daemon(REPLAYED);

    let list = fixture.run(&["--json", "list"]).json();
    let cameras = list["cameras"].as_array().expect("a camera list");
    assert_eq!(cameras.len(), 1, "{list}");
    let id = cameras[0]["id"].as_str().expect("an id").to_owned();

    // `info` — the format tree, over the wire.
    let info = fixture.run(&["--json", "info", &id]).json();
    assert_eq!(info["info"]["id"], Value::String(id.clone()));
    assert!(!info["formats"].as_array().expect("formats").is_empty());

    // `get`, whose refusal is a D13 error that crossed the wire and came back typed. Both
    // directions, because a client that refused everything would pass the second half
    // alone: a control this camera has, and one it does not.
    let brightness = fixture.run(&["--json", "get", &id, "brightness"]).json();
    assert_eq!(brightness["slug"], Value::String("brightness".to_owned()));

    let missing = fixture.run(&["--json", "get", &id, "warp_drive"]);
    assert_eq!(missing.code, 1, "{}", missing.stderr);
    // Rendered by `schema::Error`'s own `Display`, from a value `api::codes::typed`
    // reconstructed — not from transport prose. That identity is the whole of the parity
    // claim, and this is the smallest place it can be seen.
    assert!(
        missing
            .stderr
            .starts_with("wchc: no control named \"warp_drive\""),
        "{}",
        missing.stderr
    );

    // `snapshot`, which is D4's document rather than a table.
    let snapshot = fixture.run(&["--json", "snapshot", &id]).json();
    assert!(!snapshot["entries"].as_array().expect("entries").is_empty());
}

#[test]
fn one_verb_with_a_flag_reaches_two_wire_methods_and_answers_the_same_document() {
    // The first of the three that are not 1:1. `controls` and `controls --discover-pairs`
    // are one T4 verb over `wch_controls` and `wch_discover_pairs` — a read and a **write**,
    // which is why the wire has two of them — and the probe's answer is a *superset*. What
    // this asserts is that the superset was unwrapped: both invocations answer a
    // `ControlReport` with the same keys, so a build that handed the raw `DiscoveryReport`
    // to the renderer would fail here rather than shipping a `--json` document with two
    // extra top-level fields that `wch` never produces.
    // A camera with a motor on it, which is what gives the probe something to *say*: it
    // refuses to move one (design §5) and reports the refusal, and that report is the whole
    // difference between the two methods on a fake backend whose pairs are otherwise
    // declared either way.
    let fixture = Fixture::new();
    let _daemon = fixture.daemon("obsbot-tiny3");
    let list = fixture.run(&["--json", "list"]).json();
    let id = list["cameras"][0]["id"].as_str().expect("an id").to_owned();

    let read = fixture.run(&["--json", "controls", &id]);
    let probed = fixture.run(&["--json", "controls", &id, "--discover-pairs"]);

    // The flag chose a different method, and this is how a caller can tell: what the probe
    // declined to touch is on the wire (`DiscoveryReport::skipped`) precisely so a socket
    // client is not running a write with its report withheld (note N30), and it reaches a
    // person through `cli_core::report_probe` — the rendering that moved out of `crates/cli`
    // at P4f so these two binaries could not fork it. The prefix is this program's, which is
    // what proves the shared renderer was told which root it was in.
    assert!(
        probed
            .stderr
            .contains("wchc: did not probe focus_automatic_continuous:"),
        "the probe's own report is missing: {}",
        probed.stderr
    );
    // The read verb says nothing, which is what makes the line above the flag's doing
    // rather than the daemon's.
    assert!(
        !read.stderr.contains("did not probe"),
        "the read verb ran a probe: {}",
        read.stderr
    );

    let read = read.json();
    let probed = probed.json();
    let keys = |value: &Value| {
        let mut keys: Vec<String> = value
            .as_object()
            .expect("a document")
            .keys()
            .cloned()
            .collect();
        keys.sort();
        keys
    };
    assert_eq!(keys(&read), keys(&probed), "{probed}");
    assert!(keys(&read).contains(&"controls".to_owned()), "{read}");
    // The two fields that are on the wire and must **not** be in the document: they are
    // facts about the run, and `cli_core::report_probe` puts them on standard error.
    for withheld in ["skipped", "restored"] {
        assert!(
            probed.get(withheld).is_none(),
            "{withheld} leaked into the answer: {probed}"
        );
    }
    // The probe wrote to the camera and put it back; a run that could not would have said
    // so on standard error, through the renderer `wch` shares. Nothing did here, which is
    // the fake replaying a camera whose controls all restore.
    assert_eq!(
        probed["camera"], read["camera"],
        "the two methods answered about different cameras"
    );
}

#[test]
fn a_photo_arrives_as_bytes_through_base64_and_as_a_file_on_the_daemons_disk() {
    // The third that is not 1:1, in both of its sinks. The `ReturnBytes` half is the one
    // that exercises D10's encoding end to end — the daemon base64s the frame, `wchc`
    // decodes it, checks the payload against the report
    // (`PhotoResponse::bytes_match_the_delivery`, whose second consumer this is — note N34)
    // and hands a `cli_core::Photograph` to the renderer both binaries share.
    let fixture = Fixture::new();
    let _daemon = fixture.daemon(REPLAYED);
    let list = fixture.run(&["--json", "list"]).json();
    let id = list["cameras"][0]["id"].as_str().expect("an id").to_owned();

    let bytes = fixture.run(&["photo", &id]);
    let image = bytes.ok();
    assert!(!image.is_empty(), "{}", bytes.stderr);
    // A JPEG's own marker, not a byte count: the payload has to be an image, and a base64
    // decode that dropped or shifted a byte would still have produced a plausible length.
    assert_eq!(&image[..2], &[0xff, 0xd8], "not a JPEG: {:?}", &image[..4]);
    assert_eq!(
        image[image.len() - 2..],
        [0xff, 0xd9],
        "the JPEG has no end marker, so the payload was truncated on the way through"
    );
    // The summary goes to standard error when the bytes are on standard output, so the two
    // never share a stream.
    assert!(bytes.stderr.contains("delivery"), "{}", bytes.stderr);

    // The `ServerPath` half. D10 puts the relative-path resolution on the *caller's* side,
    // in the shared surface, which is why an `-o` here means a path the daemon writes and
    // both binaries agree on.
    let out = fixture.state.root().join("shot.jpg");
    let written = fixture
        .run(&["--json", "photo", &id, "-o", out.as_str()])
        .json();
    assert_eq!(written["delivery"]["path"], Value::String(out.to_string()));
    let on_disk = std::fs::read(&out).expect("the daemon wrote the file");
    assert_eq!(
        u64::try_from(on_disk.len()).expect("a fixture that fits"),
        written["delivery"]["byte_count"].as_u64().expect("a count"),
        "the report's count and the file disagree"
    );
    // …and no payload rode along with it, which is the other half of the predicate: a
    // `Path` delivery carrying bytes is a document that disagrees with itself.
    assert!(written.get("bytes").is_none(), "{written}");
}

#[test]
fn a_profile_captured_over_the_wire_is_the_one_the_daemon_is_replaying() {
    // The second of the three that are not 1:1, and the whole of what it costs: T4 spells
    // this verb `capture_profile` and D10 spells the method `profile_capture`, because a
    // wire name is a compatibility contract and a trait method name is not.
    let fixture = Fixture::new();
    let _daemon = fixture.daemon(REPLAYED);
    let list = fixture.run(&["--json", "list"]).json();
    let id = list["cameras"][0]["id"].as_str().expect("an id").to_owned();

    let captured = fixture
        .run(&[
            "--json",
            "profile",
            "capture",
            &id,
            "--capturer",
            "the wchc suite",
        ])
        .json();
    assert_eq!(captured["invariant"]["info"]["id"], Value::String(id));
    // T3's provenance: a profile records who took it, and the value travelled from this
    // command line through the wire's `capturer` parameter.
    assert_eq!(
        captured["provenance"]["capturer"],
        Value::String("the wchc suite".to_owned())
    );
    // The backend that answered is the daemon's, not this client's — which is the fact
    // `--backend` is refused over.
    assert_eq!(
        captured["provenance"]["backend"],
        Value::String("fake".to_owned())
    );
}

/// Every event a sweep delivered, in order.
///
/// The observable a subprocess cannot give: the shipped watcher is an indicatif bar that
/// draws nothing when standard error is not a terminal, so "the events arrived" is only a
/// question inside the process (see `client`'s crate header).
#[derive(Debug, Default)]
struct Recording(Mutex<Vec<ProgressEvent>>);

impl SweepWatcher for Recording {
    fn event(&self, event: &ProgressEvent) {
        if let Ok(mut seen) = self.0.lock() {
            seen.push(event.clone());
        }
    }
    fn finish(&self) {}
}

#[test]
fn a_sweep_delivers_its_progress_while_the_call_it_belongs_to_is_still_in_flight() {
    // The one method here that is a state machine rather than an adapter, and the property
    // its ordering exists to provide: `wch_calibrate_sweep` answers only the final session,
    // so every event below arrived on the *separate* subscription, on the same connection,
    // while the call was outstanding. A build that subscribed after the call would lose the
    // start of the sweep — the daemon buffers nothing for a client that has not arrived
    // (note N57) — and one that never bridged the stream onto the watcher would record
    // nothing at all.
    let fixture = Fixture::new();
    let _daemon = fixture.daemon(REPLAYED);
    let socket = fixture.socket();
    let mut remote = client::remote::Remote::connect(
        &socket,
        std::time::Duration::from_millis(schema::limits::CLIENT_SWEEP_REQUEST_TIMEOUT_MS),
    )
    .expect("the daemon is listening");

    let cameras = remote.list().expect("the daemon enumerates");
    let camera: CameraId = cameras.cameras.first().expect("one camera").id.clone();
    let control = ControlSlug::parse("brightness").expect("literal slug");

    let session = remote
        .calibrate_start(&camera, "wchc sweep", "", &[])
        .expect("a session opens");
    // By id rather than by task, which is what makes the filter the **exact** one — see
    // `remote::SweepFilter`, whose two precisions are unit-tested beside it.
    let which = SessionRef::Id { id: session.id };
    remote
        .calibrate_plan(&camera, &which, std::slice::from_ref(&control), false)
        .expect("the control queues");

    let samples = 3;
    let watcher = Recording::default();
    let swept = remote
        .calibrate_sweep(
            &camera,
            &which,
            &SweepRequest {
                control: control.clone(),
                plan: SweepSpec::Log { points: samples },
                allow_motion: false,
                stream: StreamRequest {
                    pixel_format: None,
                    width: None,
                    height: None,
                    interval: None,
                    buffer_count: schema::limits::DEFAULT_BUFFER_COUNT,
                },
                settle: SettlePolicy::default(),
                photo_format: PhotoFormat::Jpeg,
            },
            &watcher,
        )
        .expect("the sweep runs");
    assert_eq!(swept.id, session.id);

    let seen = watcher
        .0
        .lock()
        .expect("the watcher was not poisoned")
        .clone();
    assert!(
        !seen.is_empty(),
        "no progress reached the watcher: the subscription and the call did not overlap"
    );
    // Every event is this session's and this control's, which is what the filter promises.
    for event in &seen {
        assert_eq!(event.session, session.id, "{event:?}");
        assert_eq!(*event.progress.control(), control, "{event:?}");
    }
    // The *first* one is the sweep's start, carrying the size of the plan — which is the
    // event a mid-sweep subscriber has no earlier events to reconstruct, and therefore the
    // one a bar's whole first frame depends on.
    //
    // What this does **not** claim is that a client subscribing *after* the call would lose
    // it. That was tried as a mutant and stayed green: the sweep opens a camera and settles
    // a sensor before its first event, so a subscribe sent a microsecond later still wins.
    // The ordering in `Remote::calibrate_sweep` is argued rather than forced, and saying so
    // here is better than an assertion that would claim credit for a race it did not run.
    let started = matches!(
        seen.first().map(|event| &event.progress),
        Some(CalibrationProgress::SweepStarted { total, .. }) if *total == samples
    );
    assert!(started, "the sweep's first event is missing: {seen:?}");
    // …and the last is its end, which is what says every event in between was consumed
    // while the call was still outstanding rather than after it.
    let finished = matches!(
        seen.last().map(|event| &event.progress),
        Some(CalibrationProgress::SweepFinished { .. })
    );
    assert!(finished, "the sweep's last event is missing: {seen:?}");
    // One `SampleTaken` per planned sample, so the bar a human sees counts what the sweep
    // actually did rather than what it announced.
    let taken = seen
        .iter()
        .filter(|event| matches!(event.progress, CalibrationProgress::SampleTaken { .. }))
        .count();
    assert_eq!(
        u32::try_from(taken).expect("a small count"),
        samples,
        "{seen:?}"
    );
}

#[test]
fn the_calibration_arc_runs_end_to_end_over_the_socket() {
    // The eight calibrate verbs a session passes through, driven the way an operator drives
    // them — through the binary, with `--json`, over the daemon's socket. It is here rather
    // than in the sweep test above because what it asserts is the *routing* of eight verbs
    // whose answers are documents, not the ordering of one that is a state machine.
    let fixture = Fixture::new();
    let _daemon = fixture.daemon(REPLAYED);
    let list = fixture.run(&["--json", "list"]).json();
    let id = list["cameras"][0]["id"].as_str().expect("an id").to_owned();

    let started = fixture
        .run(&[
            "--json",
            "calibrate",
            "start",
            &id,
            "--task",
            "legibility",
            "--goal",
            "readable",
            "--criterion",
            "sharp text",
        ])
        .json();
    assert_eq!(started["task"], Value::String("legibility".to_owned()));

    let planned = fixture
        .run(&[
            "--json",
            "calibrate",
            "plan",
            &id,
            "--task",
            "legibility",
            "brightness",
            // Two controls, so the queue still holds work after one of them is decided —
            // which is what gives the `--partial` gate below something to refuse.
            "contrast",
        ])
        .json();
    assert!(
        !planned["queue"].as_array().expect("a queue").is_empty(),
        "{planned}"
    );

    let swept = fixture.run(&[
        "--json",
        "calibrate",
        "sweep",
        &id,
        "--task",
        "legibility",
        "brightness",
        "--points",
        "3",
    ]);
    assert_eq!(swept.code, 0, "{}", swept.stderr);

    let selected = fixture
        .run(&[
            "--json",
            "calibrate",
            "select",
            &id,
            "--task",
            "legibility",
            "brightness",
            "--metric",
            "sharpness",
        ])
        .json();
    assert_eq!(selected["id"], started["id"]);

    let status = fixture
        .run(&["--json", "calibrate", "status", &id, "--task", "legibility"])
        .json();
    assert_eq!(status["session"]["id"], started["id"]);
    assert!(
        !status["log"].as_array().expect("a log").is_empty(),
        "{status}"
    );

    // `apply` needs `--partial` while the queue still holds work, which is D8's gate and
    // not this client's: the refusal crossed the wire and came back typed.
    let refused = fixture.run(&["--json", "calibrate", "apply", &id, "--task", "legibility"]);
    assert_eq!(refused.code, 1, "{}", refused.stderr);
    assert!(refused.stderr.starts_with("wchc: "), "{}", refused.stderr);
    let applied = fixture
        .run(&[
            "--json",
            "calibrate",
            "apply",
            &id,
            "--task",
            "legibility",
            "--partial",
        ])
        .json();
    assert!(
        !applied["writes"].as_array().expect("writes").is_empty(),
        "{applied}"
    );

    // AGENTS rule 8, as a verb: the camera goes back where the session found it.
    let restored = fixture
        .run(&[
            "--json",
            "calibrate",
            "restore",
            &id,
            "--task",
            "legibility",
        ])
        .json();
    assert!(
        !restored["outcomes"]
            .as_array()
            .expect("outcomes")
            .is_empty(),
        "{restored}"
    );

    let sessions = fixture.run(&["--json", "calibrate", "list"]).json();
    assert_eq!(
        sessions["sessions"]
            .as_array()
            .expect("a session list")
            .len(),
        1,
        "{sessions}"
    );
}
