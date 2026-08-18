//! `webcam-handler-client` end to end: the refusals it owns, and the verbs it answers over a
//! real socket.
//!
//! One file for the fake backend, where `crates/daemon/tests/` is eight. Its subject is a
//! single thing — the shipped `webcam-handler-client` against a daemon replaying a committed
//! document — and everything it needs *around* that is now `support/fixture.rs`, shared with
//! the R3 arms in `hardware.rs`. Until P4g it was all here, on the argument that a
//! `#[path]`-included module would be note **N49**'s hazard bought for nothing; the second
//! includer is what buys it, and that module's header records the trade the second time round.
//!
//! ## The two halves
//!
//! **Without a daemon.** Everything `webcam-handler-client` decides on its own: the flags it
//! refuses because it is a client and not a composition root, the refusal for a socket nothing
//! is listening on, and the three exit codes. These run the shipped binary as a subprocess,
//! because the exit code and the `webcam-handler-client:` line are process facts.
//!
//! **With one.** A real `webcam-handler-daemon` beside a real `webcam-handler-client`,
//! replaying a committed device profile — the same arrangement
//! `crates/daemon/tests/systemd.rs` and `signals.rs` use, and for the same reason
//! (`support/supervised.rs`: a v4l2 daemon "would be reporting whatever is plugged into the
//! machine running CI"). Some of these drive the binary and some drive the
//! [`client::remote::Remote`] executor directly; which one is a question of what can be
//! observed, and each says so where it matters.
//!
//! ## `webcam-handler-daemon` is resolved, not assumed
//!
//! `env!("CARGO_BIN_EXE_webcam-handler-daemon")` is defined only for the package that declares
//! that binary, and this is not it. So the daemon is looked up **beside**
//! `webcam-handler-client`, which cargo does guarantee a path to, and its absence is a loud
//! failure naming what to run rather than a skip: a suite that quietly passed without the
//! daemon it is about would be the "skip that reads as pass" AGENTS bans.
//!
//! ## Nothing here sleeps
//!
//! Starting the daemon waits on a **line from its stderr pipe** — a read that ends when the
//! writer writes — which is `crates/daemon/tests/support/wchd.rs`'s bound for its reason: a
//! daemon that cannot serve says why and exits, closing the pipe. A sweep's progress is
//! asserted from the events themselves. There is no duration anywhere in this file.

use std::process::Command;
use std::sync::Mutex;

use camino::Utf8PathBuf;
use cli_core::{Executor as _, SessionRef, SweepWatcher};
use schema::capture::{PhotoFormat, SettlePolicy, StreamRequest};
use schema::control::ControlSlug;
use schema::progress::{CalibrationProgress, ProgressEvent};
use schema::selector::CameraSelector;
use schema::session::{SweepRequest, SweepSpec};
use serde_json::Value;

#[path = "support/fixture.rs"]
mod fixture;
#[path = "support/photographs.rs"]
mod photographs;

use fixture::{Daemon, Fixture, wchc};

fn repo_root() -> Utf8PathBuf {
    Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The exit code a D13 refusal of this kind leaves behind.
///
/// From `cli_core::exit_code` — the one home of the mapping (note **N127**) — rather than from
/// a literal, so a code that moves moves here too and a test cannot come to assert a number the
/// shipped binaries stopped using.
fn refusal_code(kind: schema::ErrorKind) -> i32 {
    i32::from(cli_core::exit_code(&schema::Error::sample(kind)))
}

/// The `webcam-handler-cli` binary beside `webcam-handler-client`.
///
/// A sibling lookup for `fixture::wchd`'s reason — `CARGO_BIN_EXE_*` exists only for the
/// package that declares the binary — and it lives here rather than in `support/fixture.rs`
/// because only this suite has a use for it: the hardware suite includes that module too, and
/// an item one includer never calls is a `dead_code` failure in its binary (note **N49**).
///
/// One test drives it, and its subject is the claim P4f's parity gate makes about answers,
/// extended to refusals: the *same failure* has to produce the *same document* from both roots.
fn wch() -> Command {
    let beside = Utf8PathBuf::from(env!("CARGO_BIN_EXE_webcam-handler-client"));
    let wch = beside
        .parent()
        .expect("the test binary's directory")
        .join("webcam-handler-cli");
    assert!(
        wch.exists(),
        "webcam-handler-cli is not beside webcam-handler-client at {wch}; this test compares \
         the two roots' refusals, so build the workspace (`cargo nextest run --workspace`, \
         which is what `just ci` runs) rather than this package alone"
    );
    Command::new(wch)
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

/// Start a `webcam-handler-daemon` replaying a committed profile, and wait until it is
/// serving.
///
/// The backend arguments are this file's and the spawning is `support/fixture.rs`'s, which is
/// the whole of the split between the two suites that share that module: a document is what
/// makes these assertions repeatable on a machine with no camera, and it is the one thing the
/// hardware suite cannot pass.
fn replaying(fixture: &Fixture, profile_name: &str) -> Daemon {
    fixture.spawn(&[
        "--backend",
        "fake",
        "--profile",
        profile(profile_name).as_str(),
    ])
}

// ---------------------------------------------------------------- without a daemon

#[test]
fn a_socket_nothing_is_listening_on_is_refused_by_name_and_leaves_code_one() {
    // The refusal `crates/daemon/tests/support/mod.rs` calls "the refusal P4f's
    // `webcam-handler-client` has to render". It **names the socket it tried**, because "no
    // daemon is running" and "your `$XDG_RUNTIME_DIR` is not what you think" are different
    // problems with the same symptom and only the path tells them apart.
    let fixture = Fixture::new();
    let ran = fixture.run(&["list"]);

    // `storage_io`'s own exit code, from the shipped mapping. Since the owner's ruling of
    // 2026-08-15 (note **N127**) each D13 kind leaves a code of its own, so "no daemon is
    // there" is distinguishable from "the camera is busy" by a script with no JSON parser —
    // and the number is read out of `cli_core::exit_code` rather than written here, because a
    // literal in a test is a second table nobody regenerates.
    assert_eq!(
        ran.code,
        refusal_code(schema::ErrorKind::StorageIo),
        "{}",
        ran.stderr
    );
    assert!(
        ran.stderr.starts_with("webcam-handler-client: "),
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
    let output = command.output().expect("webcam-handler-client runs");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    assert_eq!(
        output.status.code(),
        Some(refusal_code(schema::ErrorKind::StorageIo)),
        "{stderr}"
    );
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
    //
    // **The fourth vector is the one an agent types, and it was the one missing** (docs/11
    // **M20**, §9.2's "the fixture is one parameter away from the case"; note **N214**). The
    // three above dodge, one each, the rule that used to sit on the shared tree as
    // `required_if_eq("backend", "fake")`: the first names a backend that is not `fake`, the
    // second supplies the `--profile` the rule asked for, and the third names no backend at
    // all. `--backend fake list` — a caller reaching for the replayed backend, which is what
    // an agent does first — met clap instead, exit 2, told to add `--profile <PATH>`: a flag
    // this root refuses too, so the instruction cannot be followed.
    let fixture = Fixture::new();
    for args in [
        ["--backend", "v4l2", "list"].as_slice(),
        ["--backend", "fake", "list"].as_slice(),
        ["--backend", "fake", "--profile", "p.json", "list"].as_slice(),
        ["--profile", "p.json", "list"].as_slice(),
    ] {
        let ran = fixture.run(args);
        // `illegal_transition`, which is the variant this refusal has always been
        // (`client::refused`) and is now visible as a code as well as a sentence.
        assert_eq!(
            ran.code,
            refusal_code(schema::ErrorKind::IllegalTransition),
            "{args:?}: {}",
            ran.stderr
        );
        assert!(
            ran.stderr.starts_with("webcam-handler-client: "),
            "{}",
            ran.stderr
        );
        // It names the place the decision does live, which is the difference between a
        // refusal and a wall.
        assert!(
            ran.stderr.contains("webcam-handler-daemon"),
            "{args:?}: {}",
            ran.stderr
        );
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
        assert!(
            ran.stderr.contains("webcam-handler-client"),
            "{args:?}: {}",
            ran.stderr
        );
        // The trailing space is load-bearing and survived note N90's rename intact:
        // `webcam-handler-cli` is a prefix of `webcam-handler-client` exactly as `wch` was
        // of `wchc`, so an unanchored membership test could never go red here. `…-cli `
        // with a separator after it is the other binary and nothing else.
        assert!(
            !ran.stderr.contains("webcam-handler-cli "),
            "{args:?}: {}",
            ran.stderr
        );
    }

    // …and the inverse, without which "exit 2" would prove nothing: a well-formed command
    // line that needs no daemon answers 0.
    let version = fixture.run(&["--version"]);
    assert_eq!(version.code, 0, "{}", version.stderr);
    assert!(
        String::from_utf8_lossy(version.ok()).starts_with("webcam-handler-client "),
        "{}",
        String::from_utf8_lossy(&version.stdout)
    );
}

#[test]
fn a_document_verb_answers_from_both_roots_with_no_daemon_and_the_same_bytes() {
    // **The one-implementation claim, asserted rather than assumed** (design §2.7's T4 clause;
    // D15). `profile compare` takes two files and answers a document: it touches no camera, no
    // store and no socket, so it runs inside `webcam-handler-cli-core` on both roots. Two
    // consequences, and this is the only place either can be observed, because both are
    // properties of a *process*:
    //
    // - `webcam-handler-client` answers it with **no daemon running at all** — this fixture
    //   starts none, and every other test in this half of the file is here precisely because
    //   that produces a refusal naming the socket. A build that connected first would meet
    //   that refusal here.
    // - the two shipped binaries print the **same bytes**, which is the claim
    //   `scripts/gates/cli-parity.sh` exempts the `document` bucket from making: the exemption
    //   is that there is one implementation, and this is where that is checked against the
    //   only thing that can disprove it, which is two programs.
    //
    // Two *different* committed profiles, so both halves of the answer carry something. A file
    // compared with itself would answer the same shape whether or not the comparison worked.
    let fixture = Fixture::new();
    let a = profile(REPLAYED);
    let b = profile("chicony-ir");
    let argv = ["--json", "profile", "compare", a.as_str(), b.as_str()];

    let theirs = fixture.run(&argv);
    assert_eq!(
        theirs.code, 0,
        "webcam-handler-client could not answer a document verb with no daemon: {}",
        theirs.stderr
    );
    // Not merely bytes: the document is `ProfileComparison` and nothing else, and it found
    // something — two captures of two different webcams are two different devices.
    let document: schema::profile::ProfileComparison = serde_json::from_slice(&theirs.stdout)
        .expect("standard output carries a ProfileComparison");
    assert!(!document.device_matches(), "{document}");
    assert!(!document.identity.is_empty(), "{document}");

    let mine = wch().args(argv).output().expect("webcam-handler-cli runs");
    assert_eq!(
        mine.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&mine.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&mine.stdout),
        String::from_utf8_lossy(&theirs.stdout),
        "one verb, one implementation, and these two roots printed different bytes"
    );

    // And the same on the refusal path, which is where the two roots have the most room to
    // diverge: `webcam-handler-cli` builds a `schema::Error` and `webcam-handler-client` — for
    // every *other* verb — rebuilds one off the wire. A document verb never crosses a socket,
    // so both refusals are the same value rendered by the same emitter, and a build that
    // routed one of them elsewhere is what this half would catch.
    let missing = repo_root().join("corpus/profiles/no-such-capture.json");
    assert!(
        !missing.exists(),
        "the fixture names a file that must not exist"
    );
    let refused = ["--json", "profile", "compare", missing.as_str(), b.as_str()];

    let theirs = fixture.run(&refused);
    let mine = wch()
        .args(refused)
        .output()
        .expect("webcam-handler-cli runs");
    assert_eq!(
        theirs.code,
        refusal_code(schema::ErrorKind::StorageIo),
        "{}",
        theirs.stderr
    );
    assert_eq!(mine.status.code(), Some(theirs.code));
    assert_eq!(
        String::from_utf8_lossy(&mine.stdout),
        String::from_utf8_lossy(&theirs.stdout),
        "the two roots refuse a document verb with different documents"
    );
    // The refusal names the file a caller has to go and look at, on both streams and from
    // both programs — a `storage_io` that named neither would be a refusal an agent cannot act
    // on.
    assert!(
        theirs.stderr.contains(missing.as_str()),
        "{}",
        theirs.stderr
    );
    assert!(
        String::from_utf8_lossy(&theirs.stdout).contains(missing.as_str()),
        "{}",
        String::from_utf8_lossy(&theirs.stdout)
    );
}

#[test]
fn the_other_document_verb_answers_from_both_roots_with_no_daemon_and_the_same_bytes() {
    // The arm above, for `photo diff` (D17). A second document verb is not a repetition: the
    // exemption `scripts/gates/cli-parity.sh` grants that bucket is *per verb*, so the claim
    // that one implementation serves both roots has to be made about each member of it — and
    // this one reaches for a different crate on the way (`imaging`, for the decoders and the
    // comparison core), which is the edge that would put the verb in one binary if it were
    // ever taken through the engine.
    //
    // Its files are **synthetic pictures rather than a capture**, and deliberately so: a
    // fixture taken off the fake backend would put a backend in an arm whose whole subject is
    // a verb that needs none. They are encoded through `imaging::encode`, which is the writer
    // whose output the reader under test accepts.
    let fixture = Fixture::new();
    let base = imaging::fixtures::checkerboard(48, 48, 6);
    let blurred = imaging::fixtures::blurred(&base, 2.0).expect("a 48x48 image blurs");
    let a = photographs::write_photograph(fixture.state.root(), "sharp.png", base);
    let b = photographs::write_photograph(fixture.state.root(), "blurred.png", blurred);
    let argv = ["--json", "photo", "diff", a.as_str(), b.as_str()];

    let theirs = fixture.run(&argv);
    assert_eq!(
        theirs.code, 0,
        "webcam-handler-client could not answer a document verb with no daemon: {}",
        theirs.stderr
    );
    // One `PhotoComparison` and nothing else, and it found something: a blurred copy of a
    // picture is less sharp than the picture and scores below 1.0 against it. Two claims about
    // the *pair*, so a run that had compared one file with itself could not have made them.
    let document: schema::metrics::PhotoComparison =
        serde_json::from_slice(&theirs.stdout).expect("standard output carries a PhotoComparison");
    let delta = document
        .delta
        .get(&schema::metrics::MetricName::Sharpness)
        .copied()
        .expect("every metric has a delta");
    assert!(delta < 0.0, "blurring must lower the sharpness: {delta}");
    let score = document.ssim.score().expect("two 48x48 images score");
    assert!((0.0..1.0).contains(&score), "{score}");

    let mine = wch().args(argv).output().expect("webcam-handler-cli runs");
    assert_eq!(
        mine.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&mine.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&mine.stdout),
        String::from_utf8_lossy(&theirs.stdout),
        "one verb, one implementation, and these two roots printed different bytes"
    );

    // And the refusal path, which is where the two roots have the most room to diverge — the
    // same argument the arm above makes. A file that is *there* and is not a photograph is the
    // shape unique to this verb: `profile compare` refuses an unreadable file by path, and this
    // one refuses readable bytes by naming the formats that would have read.
    let refused_path = write_not_a_photograph(&fixture);
    let refused = ["--json", "photo", "diff", a.as_str(), refused_path.as_str()];
    let theirs = fixture.run(&refused);
    let mine = wch()
        .args(refused)
        .output()
        .expect("webcam-handler-cli runs");
    assert_eq!(
        theirs.code,
        refusal_code(schema::ErrorKind::DeviceIo),
        "{}",
        theirs.stderr
    );
    assert_eq!(mine.status.code(), Some(theirs.code));
    assert_eq!(
        String::from_utf8_lossy(&mine.stdout),
        String::from_utf8_lossy(&theirs.stdout),
        "the two roots refuse a document verb with different documents"
    );
    assert!(
        theirs.stderr.contains("not a photograph this build writes"),
        "{}",
        theirs.stderr
    );
}

/// A file that exists, is readable, and is in no format this build writes.
///
/// Named rather than inlined so the arm above reads as the claim it is making: what is under
/// test is the refusal for *content*, and a fixture spelled out in the middle of the
/// assertions would look like part of the comparison.
fn write_not_a_photograph(fixture: &Fixture) -> Utf8PathBuf {
    photographs::write_not_a_photograph(fixture.state.root(), "not-a-photograph.png")
}

// ------------------------------------------------------------------- with a daemon

#[test]
fn the_read_verbs_answer_the_camera_the_daemon_is_replaying() {
    let fixture = Fixture::new();
    let _daemon = replaying(&fixture, REPLAYED);

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
    assert_eq!(
        missing.code,
        refusal_code(schema::ErrorKind::ControlUnknown),
        "{}",
        missing.stderr
    );
    // Rendered by `schema::Error`'s own `Display`, from a value `api::codes::typed`
    // reconstructed — not from transport prose. That identity is the whole of the parity
    // claim, and this is the smallest place it can be seen.
    assert!(
        missing
            .stderr
            .starts_with("webcam-handler-client: no control named \"warp_drive\""),
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
    // extra top-level fields that `webcam-handler-cli` never produces.
    // A camera with a motor on it, which is what gives the probe something to *say*: it
    // refuses to move one (design §5) and reports the refusal, and that report is the whole
    // difference between the two methods on a fake backend whose pairs are otherwise
    // declared either way.
    let fixture = Fixture::new();
    let _daemon = replaying(&fixture, "obsbot-tiny3");
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
            .contains("webcam-handler-client: did not probe focus_automatic_continuous:"),
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
    // The probe wrote to the camera and put it back; a run that could not would have said so
    // on standard error, through the renderer `webcam-handler-cli` shares. Nothing did here,
    // which is the fake replaying a camera whose controls all restore.
    assert_eq!(
        probed["camera"], read["camera"],
        "the two methods answered about different cameras"
    );
}

#[test]
fn a_photo_arrives_as_bytes_through_base64_and_as_a_file_on_the_daemons_disk() {
    // The third that is not 1:1, in both of its sinks. The `ReturnBytes` half is the one that
    // exercises D10's encoding end to end — the daemon base64s the frame,
    // `webcam-handler-client` decodes it, checks the payload against the report
    // (`PhotoResponse::bytes_match_the_delivery`, whose second consumer this is — note N34)
    // and hands a `cli_core::Photograph` to the renderer both binaries share.
    let fixture = Fixture::new();
    let _daemon = replaying(&fixture, REPLAYED);
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
fn one_verb_over_three_wire_methods_records_a_file_where_the_caller_asked_for_it() {
    // **The fourth thing that is not 1:1**, and the largest: T4 has one `record` verb and
    // D10 puts three methods on the wire, so `webcam-handler-client` runs a state machine —
    // `record_start`, a poll of `record_status`, then `record_stop`. AGENTS is why that is
    // not three verbs: *"The primary consumer has no hands — a verb needing a call sequence …
    // is a defect for the consumer that matters most."*
    //
    // Two claims only this suite can make, because both are about a **process** rather than a
    // value:
    //
    // 1. **A relative `-o` is resolved on the caller's side.** The client is run from a
    //    directory of its own and asks for `take.avi`; D10 says that means the caller's
    //    directory, so the file has to appear *there* — and the daemon, whose working
    //    directory is this test runner's, must not have resolved it against its own. A build
    //    that sent the relative path would either write it beside the daemon or be refused
    //    `IllegalTransition` by `RecordRequest::server_path`; both are visible here and
    //    neither is a passing run.
    // 2. **The poll loop really polled.** The report describes a *finished* take — a
    //    container the daemon closed, with the frames the file holds — which a client that
    //    stopped after `record_start` could not have.
    let fixture = Fixture::new();
    let _daemon = replaying(&fixture, REPLAYED);
    let list = fixture.run(&["--json", "list"]).json();
    let id = list["cameras"][0]["id"].as_str().expect("an id").to_owned();

    // The caller's directory, and deliberately not the daemon's state directory: the two have
    // to be different for the claim to mean anything.
    let calling_from = fixture.runtime.base().join("caller");
    std::fs::create_dir_all(calling_from.as_std_path()).expect("a writable scratch directory");

    let output = wchc()
        .env("XDG_RUNTIME_DIR", fixture.runtime.base().as_str())
        .current_dir(calling_from.as_std_path())
        .args([
            "--json",
            "record",
            &id,
            "-o",
            "take.avi",
            "--duration",
            "400ms",
        ])
        .output()
        .expect("webcam-handler-client runs");
    assert!(
        output.status.success(),
        "webcam-handler-client record failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("--json emits one document");

    let landed = calling_from.join("take.avi");
    assert_eq!(
        report["path"],
        Value::String(landed.to_string()),
        "the document names a path the caller cannot open: {report}"
    );
    assert!(
        landed.exists(),
        "a relative -o did not land in the directory the client was run from"
    );
    // Not vacuous: `take.avi` relative to *this* test process is not where it went, which is
    // the only way to tell "resolved on the caller's side" from "resolved somewhere".
    assert!(
        !Utf8PathBuf::from("take.avi").exists(),
        "the recording landed in the test runner's own directory"
    );

    // The take really finished: a container the daemon closed, and a frame count the file
    // backs. A client that returned after `record_start` would have a report of a take that
    // was still being written — which `record_stop` would happily have handed over, so the
    // discriminating half is the *bytes*.
    assert_eq!(report["format"], "avi", "{report}");
    assert_eq!(report["ended"], "duration", "{report}");
    let frames = report["summary"]["frames_written"]
        .as_u64()
        .expect("a frame count");
    assert!(
        frames > 0,
        "the recording never reached the device: {report}"
    );
    let bytes = std::fs::read(&landed).expect("the file the client named");
    assert_eq!(
        u64::try_from(bytes.len()).expect("a length this host can represent"),
        report["summary"]["bytes_written"]
            .as_u64()
            .expect("a byte count"),
        "the report's size and the file disagree, so the container was not closed"
    );

    // And the camera is free again — `record_stop` collects, so the slot is empty and a
    // second `record` answers rather than meeting `Busy`.
    let second = fixture.run(&[
        "--json",
        "record",
        &id,
        "-o",
        fixture.state.root().join("second.avi").as_str(),
        "--duration",
        "100ms",
    ]);
    assert_eq!(second.code, 0, "{}", second.stderr);
}

#[test]
fn a_profile_captured_over_the_wire_is_the_one_the_daemon_is_replaying() {
    // The second of the three that are not 1:1, and the whole of what it costs: T4 spells
    // this verb `capture_profile` and D10 spells the method `profile_capture`, because a
    // wire name is a compatibility contract and a trait method name is not.
    let fixture = Fixture::new();
    let _daemon = replaying(&fixture, REPLAYED);
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
    let _daemon = replaying(&fixture, REPLAYED);
    let socket = fixture.socket();
    let mut remote = client::remote::Remote::connect(
        &socket,
        std::time::Duration::from_millis(schema::limits::CLIENT_SWEEP_REQUEST_TIMEOUT_MS),
    )
    .expect("the daemon is listening");

    let cameras = remote.list().expect("the daemon enumerates");
    // The id an answer carries, asked back as a *request*: D14's selector is what a verb
    // takes, and `CameraSelector::Id` is the spelling this enumeration just handed over.
    let camera = CameraSelector::Id(cameras.cameras.first().expect("one camera").id.clone());
    let control = ControlSlug::parse("brightness").expect("literal slug");

    let session = remote
        .calibrate_start(&camera, "webcam-handler-client sweep", "", &[])
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
                    ..StreamRequest::default()
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
    // …and the last is its end.
    //
    // **This assertion is sound because the sweep drains its tail, and it was racing before
    // that** (note N69). `wch_calibrate_sweep`'s answer and its `SweepFinished` leave the
    // daemon on two different tasks, so the answer can arrive first, and a client that
    // stopped reading when its call returned abandoned exactly this event — measured failing
    // 2 runs in 150 under four concurrent workspace suites, with the other seven events
    // present and this one missing. `Remote::calibrate_sweep`'s fourth step is what makes
    // the assertion a statement about the client rather than about the scheduler.
    let finished = matches!(
        seen.last().map(|event| &event.progress),
        Some(CalibrationProgress::SweepFinished { .. })
    );
    assert!(finished, "the sweep's last event is missing: {seen:?}");
    // One `SampleTaken` per planned sample, so the bar a human sees counts what the sweep
    // actually did rather than what it announced.
    //
    // This count was racing on the same ordering and was never observed losing: the last
    // sample's event is one hop further from the answer than the terminal one — the daemon
    // commits the session durably in between — so it is the same defect with a wider window
    // rather than a property that was safe. The drain covers both, because it reads until
    // the terminal event and everything this sweep emitted comes before it.
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
    let _daemon = replaying(&fixture, REPLAYED);
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
    assert_eq!(
        refused.code,
        refusal_code(schema::ErrorKind::IllegalTransition),
        "{}",
        refused.stderr
    );
    assert!(
        refused.stderr.starts_with("webcam-handler-client: "),
        "{}",
        refused.stderr
    );
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

#[test]
fn a_refusal_that_crossed_the_wire_is_the_document_webcam_handler_cli_prints_locally() {
    // **The strictest form of the parity claim, and the one P4f's gate could not make.** It
    // compares the two roots' `--json` *answers* byte for byte; this compares their
    // *refusals*, which is the harder half — `webcam-handler-cli` builds a `schema::Error` in
    // this process, while `webcam-handler-client` receives an `ErrorObject` and rebuilds one
    // with `api::codes::typed`, so a document that matched would be a value that survived a
    // serialization, a JSON-RPC code, a transport and a reconstruction unchanged.
    //
    // Both are pointed at the same committed profile — the daemon replays it, and
    // `webcam-handler-cli` replays it in its own process — so the two refusals are about one
    // device. Since the owner's ruling of 2026-08-15 (note **N127**) both roots produce the
    // document through one function (`cli_core::report_failure`), which is what makes this an
    // assertion about a *wall* rather than about two renderers that happen to agree today:
    // what could still differ is the value, and the value is what crossed the socket.
    //
    // `scripts/gates/cli-parity.sh` makes the same comparison over the shipped three from
    // outside the workspace. This one is here because it runs on every push without a gate
    // run, and because it can say which field disagreed.
    let fixture = Fixture::new();
    let _daemon = replaying(&fixture, REPLAYED);
    let profile = profile(REPLAYED);

    let id = {
        let listed = fixture.run(&["--json", "list"]).json();
        listed["cameras"][0]["id"]
            .as_str()
            .expect("the replayed profile enumerates a camera")
            .to_owned()
    };

    // Three refusals with three shapes of payload: none at all beyond what was asked for, a
    // suggestion list the engine computed, and the format list an agent retries with. A
    // comparison over one of them would pass for a client that dropped every payload it did
    // not understand.
    let refusals: Vec<Vec<String>> = vec![
        vec![
            "--json".to_owned(),
            "info".to_owned(),
            "cam:nothing-answers-to-this".to_owned(),
        ],
        vec![
            "--json".to_owned(),
            "get".to_owned(),
            id.clone(),
            "warp_drive".to_owned(),
        ],
        vec![
            "--json".to_owned(),
            "photo".to_owned(),
            id.clone(),
            "-o".to_owned(),
            fixture.state.root().join("refused.jpg").as_str().to_owned(),
            "--pixel-format".to_owned(),
            "NV12".to_owned(),
        ],
    ];

    let mut compared = 0;
    for argv in &refusals {
        let borrowed: Vec<&str> = argv.iter().map(String::as_str).collect();
        let theirs = fixture.run(&borrowed);

        let locally = wch()
            .args(["--backend", "fake", "--profile", profile.as_str()])
            .args(&borrowed)
            .output()
            .expect("webcam-handler-cli runs");
        let mine = String::from_utf8_lossy(&locally.stdout).into_owned();

        // Both must *refuse*. Without this the comparison has a hole big enough to drive a
        // fixture through: two programs that both answered would agree on a document and be
        // reported as parity for a claim about failures.
        assert_ne!(theirs.code, 0, "{argv:?}: {}", theirs.stderr);
        assert_ne!(locally.status.code(), Some(0), "{argv:?}: {mine}");

        let received: schema::error::Failure = serde_json::from_slice(&theirs.stdout)
            .unwrap_or_else(|error| {
                panic!(
                    "{argv:?}: webcam-handler-client printed no failure document ({error}): {}",
                    String::from_utf8_lossy(&theirs.stdout)
                )
            });
        assert!(received.failed());

        assert_eq!(
            String::from_utf8_lossy(&theirs.stdout),
            mine,
            "{argv:?}: the two roots do not agree about a refusal; T4 says a verb exists once \
             and D13 says the registry does too"
        );
        // The redundant channel agrees as well, which a byte comparison of standard output
        // cannot see: a client that printed the right document and exited 1 for everything
        // would pass the line above.
        assert_eq!(
            theirs.code,
            locally.status.code().expect("webcam-handler-cli exited"),
            "{argv:?}: the two roots exit differently on one refusal"
        );
        assert_eq!(theirs.code, refusal_code(received.kind()), "{argv:?}");
        compared += 1;
    }
    assert_eq!(compared, refusals.len());

    // Not vacuous in the other direction either: the three refusals are three *different*
    // ones, so the comparison above is over three documents rather than one repeated.
    assert!(refusals.len() > 2, "{} refusal(s) compared", refusals.len());
}

#[test]
fn a_client_that_is_finished_says_goodbye_before_its_runtime_goes_away() {
    // **The claim the transport made and never kept** (docs/11 **L31**, note **N219**).
    // `Sender::close` writes a real WebSocket close frame, and its doc said "this is the
    // ordinary end of every connection this binary opens" — but jsonrpsee reaches it only
    // from the `send_task` it spawned, and `Remote` dropped its current-thread runtime before
    // its client, so that task was discarded unpolled. The frame was written for no
    // connection this binary ever opened, and the daemon ended every one of them on a read
    // error instead.
    //
    // Over a real daemon on a real socket, because the thing under test is what a *task*
    // does after a value is dropped: a double of the transport would answer whatever it was
    // written to answer.
    let fixture = Fixture::new();
    let _daemon = replaying(&fixture, REPLAYED);
    let socket = fixture.socket();
    let mut remote = client::remote::Remote::connect(
        &socket,
        std::time::Duration::from_millis(schema::limits::CLIENT_REQUEST_TIMEOUT_MS),
    )
    .expect("the daemon is listening");

    // A verb first, so the connection under test is one that carried traffic — a socket that
    // was opened and never used is not the shape a client exits in.
    remote.list().expect("the daemon enumerates");

    assert!(
        remote.close(),
        "the connection was dropped without its close frame, so the daemon sees a read error \
         where a goodbye belongs"
    );
    // There is no second call to make: `close` takes the value (note **N223**), so "said
    // once" is the borrow checker's claim rather than an assertion that could stop being
    // checked. `Drop` runs `say_goodbye` again on the way out of `close` and finds the token
    // gone, which is the same fact from the inside.
}

#[test]
fn a_goodbye_the_socket_refused_is_not_reported_as_said() {
    // **The other direction of the same `bool`, and the one it could not answer** (docs/11
    // L31's repair, note **N223**). `Sender::close` fired its signal whatever the write did,
    // so `Goodbye::wait` answered "`close()` was reached" while the field it waits on said
    // *"fired when the close frame has been written, and never otherwise"*. The assertion
    // above could then only fail if the send task were never polled at all — never on the
    // write, which is the half its message names.
    //
    // The fixture is a peer that is *gone*: the daemon is `SIGKILL`ed under a live
    // connection, so `soketto`'s write meets `EPIPE` on a socket whose reader has been
    // reaped. Measured against the unrepaired transport, this answered `true`.
    let fixture = Fixture::new();
    let mut daemon = replaying(&fixture, REPLAYED);
    let socket = fixture.socket();
    let mut remote = client::remote::Remote::connect(
        &socket,
        std::time::Duration::from_millis(schema::limits::CLIENT_REQUEST_TIMEOUT_MS),
    )
    .expect("the daemon is listening");
    // The same traffic as the arm above, so the two differ in one thing: whether the peer is
    // there when the goodbye is written.
    remote.list().expect("the daemon enumerates");

    daemon.child.kill().expect("the daemon was running");
    daemon.child.wait().expect("the daemon is reaped");

    assert!(
        !remote.close(),
        "a close frame that could not be written was reported as a goodbye that was said"
    );
}
