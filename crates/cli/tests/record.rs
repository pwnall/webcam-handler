//! `webcam-handler-cli record` end to end, against the fake backend (docs/7 P6c, G6).
//!
//! `photo.rs` is this file's sibling and the split is the same one the engine makes: the
//! engine's own suite calls `engine::record::run` and is the finer-grained one; what a
//! subprocess suite adds is the half nothing else covers — that the **binary** turns a command
//! line into the request the engine gets, resolves `-o` the way D10 says, and puts the bytes
//! where it said it did.
//!
//! ## What the file is read by, and why it is not this file
//!
//! `imaging::avi::read::read_stream` — the strict reader P6a wrote from the RIFF/AVI
//! specification *before* the muxer existed, which "shares **no** code with it, not a
//! constant, not a FourCC, not a helper" and which `scripts/gates/avi-reparse-is-independent.sh`
//! keeps that way. A chunk walk written here would be a third parser, and one that agrees with
//! whatever it was written against. What this suite adds on top of the engine's use of that
//! same reader is that the bytes came out of the **shipped binary** and off a **committed
//! profile**, through a command line somebody could type.
//!
//! ## Both containers, because a camera decides which one
//!
//! D7's pairing is a law with one home (`schema::video::VideoFormat::carries`) and two arms,
//! and `corpus/profiles/chicony-ir.json` is a captured profile of a real **GREY-only** device.
//! So the Y4M arm here is an answer about hardware rather than about a fixture built to make
//! the point, and so is the refusal beside it: `.avi` over that camera is
//! `FormatUnsupported` carrying a `ContainerRefusal`, naming the `.y4m` that *would* have
//! taken those frames — a file rather than a format, because the format was the camera's
//! answer and not the caller's request (note **N211**).
//!
//! ## Every take is short, and that is the fixture rather than a shortcut
//!
//! `--duration` is bounded well under a second on every row below. The fake synthesises a
//! frame per turn, so a default ten-second take would spend ten seconds of a test run proving
//! something a 300 ms take proves: that the duration reached the engine, that the loop ended on
//! it, and that the container closed. What a *long* take would add is nothing this suite can
//! assert and everything docs/7 P6d's real capture will.
//!
//! Every recording lands in a scratch directory under `target/`. A frame may contain a person
//! (rubric A12) — these are synthetic, and the habit is the point.

use std::process::Command;

use camino::{Utf8Path, Utf8PathBuf};
use serde_json::Value;

/// The `webcam-handler-cli` binary this test drives, built by cargo alongside it.
fn wch() -> Command {
    Command::new(env!("CARGO_BIN_EXE_webcam-handler-cli"))
}

/// A scratch directory under the one scratch root (note N84), and the paths under it.
struct Scratch {
    dir: tempfile::TempDir,
}

impl Scratch {
    fn new() -> Scratch {
        Scratch {
            dir: engine::paths::scratch_dir().expect("a scratch directory"),
        }
    }

    fn path(&self, name: &str) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(self.dir.path().join(name)).expect("a utf-8 temp dir")
    }

    fn base(&self) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(self.dir.path().to_path_buf()).expect("a utf-8 temp dir")
    }
}

/// The committed profile a camera is replayed from, and the id it enumerates as.
struct Replayed {
    profile: Utf8PathBuf,
    camera: String,
}

fn replayed(name: &str, camera: &str) -> Replayed {
    let profile = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/profiles")
        .join(format!("{name}.json"));
    assert!(profile.exists(), "the corpus is missing {profile}");
    Replayed {
        profile,
        camera: camera.to_owned(),
    }
}

/// Run `webcam-handler-cli record` against a replayed camera, from `cwd`.
///
/// Answers the exit status, standard output and standard error, all three, because two of the
/// claims below are about a *refusal* and a helper that asserted success would make them
/// unaskable — which is the shape `photo.rs`'s own helper had to grow a sibling for.
fn record_from(device: &Replayed, cwd: &Utf8Path, extra: &[&str]) -> (bool, String, String) {
    let output = wch()
        .current_dir(cwd.as_std_path())
        .args([
            "--backend",
            "fake",
            "--profile",
            device.profile.as_str(),
            "record",
            &device.camera,
        ])
        .args(extra)
        .output()
        .expect("webcam-handler-cli runs");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// The same, insisting that it answered.
fn record(device: &Replayed, cwd: &Utf8Path, extra: &[&str]) -> (String, String) {
    let (ok, out, err) = record_from(device, cwd, extra);
    assert!(ok, "webcam-handler-cli record {extra:?} failed: {err}");
    (out, err)
}

#[test]
fn wch_record_writes_an_avi_the_independent_reader_accepts_and_a_document_that_describes_it() {
    // The whole verb, through the binary: a command line becomes a request, the request
    // becomes a file, and the `--json` document describes *that* file rather than the
    // request. The frame count is asserted on **both sides** — the reader's and the
    // report's — because either alone is satisfiable by a build that lost the other: a
    // summary counted off a request would match nothing on disk, and a file nobody read
    // would be a claim about bytes.
    let scratch = Scratch::new();
    let path = scratch.path("take.avi");
    let device = replayed("chicony-rgb", "cam:integrated-camera-integrated-c");
    let (stdout, _) = record(
        &device,
        &scratch.base(),
        &["--json", "-o", path.as_str(), "--duration", "300ms"],
    );

    let report: Value = serde_json::from_str(&stdout).expect("--json emits one document");
    assert_eq!(report["format"], "avi", "an MJPG camera records AVI");
    assert_eq!(report["path"], path.as_str());
    assert_eq!(report["ended"], "duration", "{report}");
    let declared = report["summary"]["frames_written"]
        .as_u64()
        .expect("a frame count");

    let bytes = std::fs::read(&path).expect("the file exists");
    let stream = imaging::avi::read::read_stream(&bytes)
        .expect("a finished recording is one the strict reader accepts");
    assert_eq!(
        stream.frames.len() as u64,
        declared,
        "the report counted frames the file does not hold"
    );
    assert!(declared > 0, "a 300 ms take on this fake wrote no frame");
    // The index is what makes it the *strict* reader's file rather than a recoverable
    // prefix, and it is the half a take that was interrupted would not have.
    assert_eq!(stream.index.len(), stream.frames.len(), "{stream:?}");
}

#[test]
fn wch_record_over_a_grey_camera_writes_y4m_and_refuses_the_container_that_cannot_carry_it() {
    // D7's pairing over a **captured** device rather than a fixture built to make the point:
    // `chicony-ir` is a real GREY-only camera, so all three arms here are answers about
    // hardware. `crates/engine`'s suite makes the same three claims one layer down; what this
    // adds is that a person typing `-o take.avi` at that camera is told what AVI would have
    // taken, in a line they can read.
    let scratch = Scratch::new();
    let device = replayed("chicony-ir", "cam:integrated-camera-integrated-i");

    let y4m = scratch.path("take.y4m");
    let (stdout, _) = record(
        &device,
        &scratch.base(),
        &["--json", "-o", y4m.as_str(), "--duration", "300ms"],
    );
    let report: Value = serde_json::from_str(&stdout).expect("one document");
    assert_eq!(report["format"], "y4m", "{report}");
    let bytes = std::fs::read(&y4m).expect("the file exists");
    assert!(
        bytes.starts_with(b"YUV4MPEG2 "),
        "a .y4m that is not a YUV4MPEG2 stream is not a recording"
    );

    // The refusal, and it names the **file** that would have taken these frames rather than
    // only saying no. It named AVI's two formats until 2026-08-17 (note **N211**), which is a
    // list this camera has never had a member of: a person reading that line, or an agent
    // reading the document beside it, is told to go and ask a GREY-only sensor for MJPG.
    let avi = scratch.path("take.avi");
    let (ok, _, stderr) = record_from(
        &device,
        &scratch.base(),
        &["-o", avi.as_str(), "--duration", "300ms"],
    );
    assert!(!ok, "AVI carries no GREY");
    assert!(stderr.contains(".y4m"), "{stderr}");
    assert!(!stderr.contains("MJPG"), "{stderr}");
    assert!(!stderr.contains("JPEG"), "{stderr}");
    assert!(
        !avi.exists(),
        "a recording refused by the pairing left a file behind"
    );

    // And a path with **no** extension records whatever the camera gives, which is the arm
    // AGENTS' handless primary consumer depends on: an agent that has not enumerated this
    // camera cannot know to type `.y4m`, and a verb where it must is a verb needing a call
    // sequence.
    let unnamed = scratch.path("take");
    let (stdout, _) = record(
        &device,
        &scratch.base(),
        &["--json", "-o", unnamed.as_str(), "--duration", "300ms"],
    );
    let report: Value = serde_json::from_str(&stdout).expect("one document");
    assert_eq!(report["format"], "y4m", "{report}");
}

#[test]
fn a_relative_output_path_lands_in_the_directory_the_command_was_typed_in() {
    // D10's rule, from the side only a subprocess can see: the resolution happens in the
    // shared command surface against the **caller's** cwd, so the file appears beside the
    // person rather than beside the daemon. `webcam-handler-client` sends the resolved path
    // for exactly this reason, and `crates/client/tests/wchc.rs` makes the same claim through
    // a socket — one rule, two roots, and this is the half where the cwd is a real process's.
    let scratch = Scratch::new();
    let device = replayed("chicony-rgb", "cam:integrated-camera-integrated-c");
    let (stdout, _) = record(
        &device,
        &scratch.base(),
        &["--json", "-o", "relative.avi", "--duration", "300ms"],
    );

    let landed = scratch.path("relative.avi");
    assert!(
        landed.exists(),
        "a relative -o did not land in the directory the command was run from"
    );
    let report: Value = serde_json::from_str(&stdout).expect("one document");
    assert_eq!(
        report["path"],
        landed.as_str(),
        "the document names a path the caller cannot open"
    );
    // Not vacuous: the same name relative to *this* test process's directory is not where
    // the file went, which is what the assertion above would otherwise be silent about.
    assert!(
        !Utf8Path::new("relative.avi").exists(),
        "the recording landed in the test runner's own directory"
    );
}

#[test]
fn a_duration_past_the_cap_and_a_container_this_build_cannot_write_are_refused_before_a_stream() {
    // The two request predicates, through the binary, and the second half of each: **no file
    // is left behind**. A build that created the destination before it refused would have
    // truncated an operator's earlier recording on the way to saying no — note **N51**'s
    // lesson one verb along, and the ordering `engine::record::start` is built around.
    let scratch = Scratch::new();
    let device = replayed("chicony-rgb", "cam:integrated-camera-integrated-c");

    let unwritable = scratch.path("take.mkv");
    let (ok, _, stderr) = record_from(&device, &scratch.base(), &["-o", unwritable.as_str()]);
    assert!(!ok, "this build writes no Matroska");
    assert!(
        stderr.contains(".avi") && stderr.contains(".y4m"),
        "{stderr}"
    );
    assert!(!unwritable.exists(), "a refused recording wrote a file");

    let too_long = scratch.path("long.avi");
    let (ok, _, stderr) = record_from(
        &device,
        &scratch.base(),
        &[
            "-o",
            too_long.as_str(),
            "--duration",
            &format!("{}ms", schema::limits::MAX_RECORDING_MS + 1),
        ],
    );
    assert!(!ok, "a millisecond past the cap is past it");
    assert!(
        stderr.contains(&schema::limits::MAX_RECORDING_MS.to_string()),
        "an agent that is not told the cap cannot ask again inside it: {stderr}"
    );
    assert!(!too_long.exists());

    // And a duration clap cannot read is a **usage** error rather than a device one — exit 2
    // rather than 1, so a script that retries on "the camera is busy" does not retry on a
    // typo (note **N113**).
    let output = wch()
        .current_dir(scratch.base().as_std_path())
        .args([
            "--backend",
            "fake",
            "--profile",
            device.profile.as_str(),
            "record",
            &device.camera,
            "-o",
            scratch.path("typo.avi").as_str(),
            "--duration",
            "10",
        ])
        .output()
        .expect("webcam-handler-cli runs");
    assert_eq!(output.status.code(), Some(2), "a usage error is exit 2");
}

#[test]
fn the_human_rendering_and_the_json_document_describe_the_same_take() {
    // `cli_core::render`'s rule, at the verb that landed last: the two renderings are two
    // views of one value, so a fact one of them shows and the other omits is a bug in the
    // renderer rather than a feature of the mode. Asserted on the facts a person actually
    // acts on — which file, which container, and how it ended — because those are the three
    // a table that had quietly stopped reading the report would get wrong.
    let scratch = Scratch::new();
    let device = replayed("chicony-rgb", "cam:integrated-camera-integrated-c");
    let path = scratch.path("both.avi");
    let (human, _) = record(
        &device,
        &scratch.base(),
        &["-o", path.as_str(), "--duration", "300ms"],
    );
    let (json, _) = record(
        &device,
        &scratch.base(),
        &["--json", "-o", path.as_str(), "--duration", "300ms"],
    );
    let report: Value = serde_json::from_str(&json).expect("one document");

    assert!(human.contains(path.as_str()), "{human}");
    assert!(human.contains("avi"), "{human}");
    assert!(
        human.contains("the duration you asked for was spent"),
        "the table does not say why the recording stopped: {human}"
    );
    assert_eq!(report["ended"], "duration");
    // The frame count is in both, and it is the number a reader compares against the file.
    let frames = report["summary"]["frames_written"]
        .as_u64()
        .expect("a frame count");
    assert!(human.contains(&frames.to_string()), "{human}");
}
