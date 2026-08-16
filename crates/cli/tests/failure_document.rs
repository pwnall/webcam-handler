//! What a failing `--json` run puts on standard output, from the shipped binary (owner ruling,
//! 2026-08-15; notes **N124**, **N127**).
//!
//! The ruling is short — *"Let's extend the JSON output to convey errors … Distinct exit codes
//! are nice-to-have, as a redundant mechanism"* — and everything it asks for is a property of a
//! **process**: which stream a document lands on, whether a redirected standard output loses
//! it, and what number the shell is left with. `cli_core`'s own tests assert the emitter over a
//! pair of buffers, which is the finer-grained half; this suite is the half that cannot be
//! faked, because what it runs is the program an agent runs.
//!
//! ## Why this is not folded into `agent_guide.rs`
//!
//! That suite's subject is the *manual* — it extracts the guide's own examples and runs them,
//! and it carries one assertion about a failure because the guide has a section about
//! failures. This one's subject is the **document**: its shape, its payload, the marker no
//! answer carries, and the exit codes beside it. Two subjects, and the second one has to keep
//! working if the guide is ever restructured.
//!
//! ## Everything is replayed
//!
//! `--backend fake --profile corpus/profiles/chicony-rgb.json`: a real capture of a real
//! webcam, committed, offering MJPG and YUYV and nothing else — which is what makes
//! `--pixel-format NV12` a refusal about a *device* rather than about a fixture built to
//! produce one. No camera is needed and no frame is written.

use std::process::Command;

use camino::Utf8PathBuf;
use schema::error::{Error, Failure};
use serde_json::Value;

/// The profile every run below replays. See this file's header for why it is this one.
const PROFILE: &str = "chicony-rgb";

/// A pixel format the replayed camera does not offer.
///
/// Four ASCII characters, so `PixelFormat::parse` accepts it and the refusal is the *camera's*
/// answer rather than clap's — a usage error here would be exit 2 and would prove nothing
/// about D13 at all. `the_format_refusal_names_what_the_camera_does_offer` checks that this is
/// still absent from the profile, so a re-capture that added NV12 fails loudly instead of
/// quietly turning this suite into a test of a successful photograph.
const ABSENT_FORMAT: &str = "NV12";

/// A committed profile by name, asserted present.
///
/// Takes the name since P6f: one arm needs a camera offering no compressed format at all,
/// which [`PROFILE`] is not. The `exists` check stays here rather than at the two call sites
/// because a corpus file that has moved should fail as itself, not as a backend that
/// enumerated nothing.
fn profile(name: &str) -> Utf8PathBuf {
    let path = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/profiles")
        .join(format!("{name}.json"));
    assert!(path.exists(), "the corpus is missing {path}");
    path
}

/// One `webcam-handler-cli` invocation over the replayed device.
struct Ran {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Ran {
    /// Standard output as the failure document, or a panic naming what was there instead.
    ///
    /// Deserialized into [`Failure`] rather than into a `Value`, deliberately: the type refuses
    /// a document whose marker is not `true` and one with no marker at all, so parsing *is* the
    /// assertion that this is a failure document and not merely some JSON.
    fn refusal(&self) -> Failure {
        serde_json::from_str(&self.stdout).unwrap_or_else(|error| {
            panic!(
                "standard output is not a failure document ({error}); stdout was {:?} and \
                 stderr was {:?}",
                self.stdout, self.stderr
            )
        })
    }
}

fn run(args: &[&str]) -> Ran {
    run_replaying(PROFILE, args)
}

/// The same, over a named profile.
///
/// Exists because one refusal in this file has to come from a camera that offers *no*
/// compressed format at all, and `chicony-rgb` offers MJPG — see
/// `a_container_refusal_never_says_the_camera_offers_a_format_it_has_never_had`.
fn run_replaying(name: &str, args: &[&str]) -> Ran {
    let profile = profile(name);
    let output = Command::new(env!("CARGO_BIN_EXE_webcam-handler-cli"))
        .args(["--backend", "fake", "--profile", profile.as_str()])
        .args(args)
        .output()
        .expect("webcam-handler-cli runs");
    Ran {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// A scratch directory under `target/` (note **N84**), for the one row that names an output
/// path.
///
/// A real, writable path rather than a fictional one: the refusal under test is the *camera's*
/// answer about a format, and pointing `-o` somewhere that cannot be created would risk a
/// `storage_io` refusal wearing the right test name.
fn scratch() -> tempfile::TempDir {
    engine::paths::scratch_dir().expect("a scratch directory")
}

/// The id the replayed profile enumerates as.
///
/// Asked of the binary rather than transcribed, for `json-validates.sh`'s reason: a re-capture
/// that renames the card must break as a corpus change rather than as a suite that started
/// asserting `camera_unknown` about the camera it meant to use — which is the one failure this
/// file could produce by accident and could not tell from success.
fn camera() -> String {
    let listed = run(&["--json", "list"]);
    assert_eq!(listed.code, Some(0), "{}", listed.stderr);
    let document: Value = serde_json::from_str(&listed.stdout).expect("a camera list");
    document["cameras"][0]["id"]
        .as_str()
        .expect("the replayed profile enumerates one camera")
        .to_owned()
}

#[test]
fn a_failing_verb_answers_on_standard_output_with_the_failure_and_its_discriminant() {
    // **The defect N124 recorded, from the other side.** Before this change standard output
    // was empty on every failure, `--json` or not, so an agent that redirected it lost the
    // refusal entirely and one that did not still had an English sentence to parse. All three
    // channels are asserted here, because the ruling moved one of them and took nothing from
    // the other two.
    let ran = run(&["--json", "info", "cam:nothing-answers-to-this"]);
    let document = ran.refusal();

    assert!(document.failed());
    assert_eq!(document.kind(), schema::ErrorKind::CameraUnknown);
    // The payload: what the caller asked for, so a harness can say which of its own ids was
    // wrong without re-reading its own command line.
    let Error::CameraUnknown { requested } = &document.error else {
        panic!("{document:?}");
    };
    assert_eq!(requested, "cam:nothing-answers-to-this");

    // The human's line, unchanged: the program that met it, then the same sentence the
    // document carries. Two renderings of one value and not two renderings (design §2.10).
    assert_eq!(
        ran.stderr.trim_end(),
        format!("webcam-handler-cli: {}", document.message)
    );

    // And the code, from the shipped mapping rather than from a literal here.
    assert_eq!(
        ran.code,
        Some(i32::from(cli_core::exit_code(&document.error)))
    );
}

#[test]
fn the_format_refusal_names_what_the_camera_does_offer_and_not_only_in_prose() {
    // **The case that matters most**, because it is the one an unattended caller *acts* on:
    // `format_unsupported`'s disposition in the guide is "fix the request", and the fix is to
    // ask for something in `available`. A document that carried the discriminant and dropped
    // the list would leave the agent parsing "this camera offers MJPG, YUYV" out of a
    // sentence — which is the failure AGENTS' opening section describes as guessing.
    let camera = camera();
    let dir = scratch();
    let out = dir.path().join("refused.jpg");
    let out = out.to_str().expect("a utf-8 scratch directory");
    let ran = run(&[
        "--json",
        "photo",
        &camera,
        "-o",
        out,
        "--pixel-format",
        ABSENT_FORMAT,
    ]);
    let document = ran.refusal();
    assert_eq!(document.kind(), schema::ErrorKind::FormatUnsupported);

    let Error::FormatUnsupported {
        requested,
        available,
        size,
    } = &document.error
    else {
        panic!("{document:?}");
    };
    assert_eq!(
        requested.map(|format| format.to_string()).as_deref(),
        Some(ABSENT_FORMAT)
    );
    assert!(
        !available.is_empty(),
        "the refusal an agent retries on carries no formats to retry with"
    );
    assert!(
        size.is_none(),
        "a *format* refusal carrying a size names two levers to a caller that can pull only \
         one (note **N138**): {document:?}"
    );

    // The formats are the camera's own, checked against what `info` says rather than against a
    // list written here — so a re-capture moves both together or neither.
    let described = run(&["--json", "info", &camera]);
    assert_eq!(described.code, Some(0), "{}", described.stderr);
    let detail: Value = serde_json::from_str(&described.stdout).expect("a camera detail");
    let offered: Vec<String> = detail["formats"]
        .as_array()
        .expect("a format tree")
        .iter()
        .map(|format| {
            format["pixel_format"]
                .as_str()
                .expect("a FourCC string")
                .to_owned()
        })
        .collect();
    assert!(
        !offered.iter().any(|format| format == ABSENT_FORMAT),
        "the replayed camera now offers {ABSENT_FORMAT}, so this suite is no longer asking it \
         for something it does not have: {offered:?}"
    );
    for format in available {
        assert!(
            offered.contains(&format.to_string()),
            "the refusal offers {format}, which this camera does not: {offered:?}"
        );
    }

    // Readable FourCCs on the wire, which is the owner's ruling of 2026-08-14 (note **N109**)
    // holding in the document that ruling's own example was about — asserted on the raw JSON,
    // because the typed value would render readably either way.
    let raw: Value = serde_json::from_str(&ran.stdout).expect("the document parses");
    assert_eq!(
        raw["error"]["requested"],
        Value::String(ABSENT_FORMAT.to_owned())
    );
    assert!(
        raw["error"]["available"]
            .as_array()
            .expect("available is an array")
            .iter()
            .all(Value::is_string),
        "the formats crossed as something other than their four characters: {}",
        raw["error"]["available"]
    );
}

/// The refusal a caller meets when it asks a camera for a size, and the two things the
/// refusal must not do: refuse a size the camera has, and blame the format when it does.
///
/// **Both were true of the shipped binary on 2026-08-16**, over `corpus/profiles/obsbot-tiny3.json`
/// — one of the two cameras this project is developed against. `photo --size 640x480`
/// answered `{"kind":"format_unsupported","requested":null,"available":["MJPG","YUYV"]}` and
/// exit 18, for a size that camera enumerates in YUYV, because `StreamRequest::choose`
/// ranked MJPG on its 8.3 megapixels and then asked only MJPG's list. And the sentence beside
/// it read *"format (unspecified) is unavailable; MJPG, YUYV would be accepted"*: about
/// formats, naming the caller's own format as acceptable, never mentioning size. An agent
/// obeying `docs/agent-guide.md`'s `format_unsupported` disposition — "fix the request … ask
/// for one it does" — retries `--pixel-format MJPG`, meets the identical refusal, and loops.
/// Note **N138** carries both.
///
/// So this asserts the *claims* rather than the wording, in N129's shape: it asks the camera
/// what it enumerates, asks for a size the answer says it has, then asks for one the answer
/// says it does not, and fails a refusal that attributes one to the other. It runs over the
/// OBSBOT profile rather than [`PROFILE`] because the shape that matters is two formats whose
/// size lists disagree — the chicony offers 640×480 in both, so it cannot separate "ranked
/// then vetoed" from "asked the whole device".
#[test]
fn a_size_refusal_names_the_size_and_never_the_format_that_was_answerable() {
    /// Two formats, one of which stops well below the other. See the doc comment.
    const SPLIT_SIZES: &str = "obsbot-tiny3";

    let listed = run_replaying(SPLIT_SIZES, &["--json", "list"]);
    assert_eq!(listed.code, Some(0), "{}", listed.stderr);
    let document: Value = serde_json::from_str(&listed.stdout).expect("a camera list");
    let camera = document["cameras"][0]["id"]
        .as_str()
        .expect("the replayed camera has an id")
        .to_owned();

    // What the camera says about itself, which is the only authority on it (AGENTS rule 4).
    let detail = run_replaying(SPLIT_SIZES, &["--json", "info", &camera]);
    assert_eq!(detail.code, Some(0), "{}", detail.stderr);
    let detail: Value = serde_json::from_str(&detail.stdout).expect("a camera detail");
    let mut enumerated: Vec<(u32, u32)> = Vec::new();
    for format in detail["formats"].as_array().expect("a format tree") {
        for entry in format["sizes"].as_array().expect("a size list") {
            let size = &entry["size"];
            if size["kind"] == "discrete" {
                let width = u32::try_from(size["width"].as_u64().expect("a width")).expect("u32");
                let height =
                    u32::try_from(size["height"].as_u64().expect("a height")).expect("u32");
                enumerated.push((width, height));
            }
        }
    }
    // The premise, asserted rather than assumed: a re-capture that dropped the small YUYV
    // modes would turn the first half below into a test of a refusal, silently.
    assert!(
        enumerated.contains(&(640, 480)),
        "{SPLIT_SIZES} no longer enumerates 640x480, which is the size this arm asks for: \
         {enumerated:?}"
    );
    assert!(
        !enumerated.iter().any(|(w, h)| *w <= 320 && *h <= 240),
        "{SPLIT_SIZES} now has a mode inside 320x240, so the refusal below would be about \
         something else: {enumerated:?}"
    );

    let dir = scratch();
    let out = dir.path().join("sized.jpg");
    let out = out.to_str().expect("a utf-8 scratch directory");

    // Half one: a size the camera has is answered, whatever the ranking would have preferred.
    let taken = run_replaying(
        SPLIT_SIZES,
        &["--json", "photo", &camera, "-o", out, "--size", "640x480"],
    );
    assert_eq!(
        taken.code,
        Some(0),
        "a size this camera enumerates was refused: {} / {}",
        taken.stdout,
        taken.stderr
    );
    let answer: Value = serde_json::from_str(&taken.stdout).expect("a photo report");
    assert_eq!(answer["negotiated"]["width"], 640);
    assert_eq!(answer["negotiated"]["height"], 480);

    // Half two: a size it does not have is refused, and the refusal is about the size.
    let refused = run_replaying(
        SPLIT_SIZES,
        &[
            "--json",
            "photo",
            &camera,
            "-o",
            out,
            "--size",
            "320x240",
            "--pixel-format",
            "MJPG",
        ],
    );
    let failure = refused.refusal();
    assert_eq!(failure.kind(), schema::ErrorKind::FormatUnsupported);
    let Error::FormatUnsupported {
        requested,
        size: Some(size),
        ..
    } = &failure.error
    else {
        panic!(
            "a size nothing fits was refused without saying so: {:?}",
            failure.error
        );
    };
    assert_eq!(
        *requested, None,
        "the caller's format is on this camera and is not what failed: {failure:?}"
    );
    assert_eq!((size.requested_width, size.requested_height), (320, 240));
    // Every size the refusal offers is one the camera really has, and the ones it has are
    // what a caller repairs its request with — checked against `info` rather than a list
    // written here, so a re-capture moves both together or neither.
    assert!(!size.available.is_empty(), "{failure:?}");
    for offered in &size.available {
        let schema::camera::FrameSize::Discrete { width, height } = offered else {
            continue;
        };
        assert!(
            enumerated.contains(&(*width, *height)),
            "the refusal offers {width}x{height}, which this camera does not: {enumerated:?}"
        );
    }

    // And the sentence, which is the half that sent an agent round a loop. It names the size
    // it could not deliver; it does not name the format the caller asked for, because that
    // format was answerable and changing it repairs nothing.
    let message = &failure.message;
    assert!(
        message.contains("320x240"),
        "the refusal never says which size it could not deliver: {message}"
    );
    assert!(
        !message.contains("MJPG"),
        "the refusal tells a caller its own format is the problem, so an agent following \
         the guide retries it and meets this again: {message}"
    );
}

#[test]
fn no_verb_that_answers_carries_the_marker_a_caller_branches_on() {
    // The other half of "unambiguously a failure", driven through the *binary*: the emitter's
    // walk over the committed shapes lives in `webcam-handler-xtask`
    // (`no_document_a_verb_answers_with_can_be_mistaken_for_the_failure_document`), and this
    // is the same claim about documents that were actually printed. A success document that
    // grew a `failed` field would be read as a refusal by every caller following the guide.
    let camera = camera();
    let marker = schema::error::FAILURE_MARKER;
    let answered: Vec<Vec<&str>> = vec![
        vec!["--json", "list"],
        vec!["--json", "info", &camera],
        vec!["--json", "controls", &camera],
        vec!["--json", "snapshot", &camera],
        vec!["--json", "calibrate", "list"],
    ];
    let mut checked = 0;
    for argv in &answered {
        let ran = run(argv);
        assert_eq!(ran.code, Some(0), "{argv:?}: {}", ran.stderr);
        let document: Value = serde_json::from_str(&ran.stdout)
            .unwrap_or_else(|error| panic!("{argv:?} did not answer with a document: {error}"));
        assert!(
            document.get(marker).is_none(),
            "{argv:?} answered with a document carrying `{marker}`, which is what tells a \
             caller a verb refused: {document}"
        );
        // And it does not parse as one either, which is the check a caller would actually
        // make — the marker is required, so an answer cannot satisfy the failure shape.
        assert!(
            serde_json::from_str::<Failure>(&ran.stdout).is_err(),
            "{argv:?}'s answer parses as a failure document"
        );
        checked += 1;
    }
    assert!(checked > 3, "{checked} answering verb(s) checked");
}

#[test]
fn two_different_failures_leave_two_different_exit_codes() {
    // The redundancy the ruling calls nice-to-have, from a shell's point of view. Three kinds
    // reachable from one replayed device, and the assertion is that no two of them agree — the
    // property `cli_core`'s walk proves over all eighteen, spent here on the ones a command
    // line can actually produce, so a `main` that dropped the mapping and exited 1 for
    // everything fails at the process boundary rather than only in a unit.
    let camera = camera();
    let dir = scratch();
    let out = dir.path().join("refused.jpg");
    let out = out.to_str().expect("a utf-8 scratch directory");
    let met = [
        run(&["--json", "info", "cam:nothing-answers-to-this"]),
        run(&["--json", "get", &camera, "warp_drive"]),
        run(&[
            "--json",
            "photo",
            &camera,
            "-o",
            out,
            "--pixel-format",
            ABSENT_FORMAT,
        ]),
    ];

    let mut seen: std::collections::BTreeMap<Option<i32>, schema::ErrorKind> =
        std::collections::BTreeMap::new();
    for ran in &met {
        let document = ran.refusal();
        assert_eq!(
            ran.code,
            Some(i32::from(cli_core::exit_code(&document.error))),
            "{:?} exited a code the shipped mapping does not give it",
            document.kind()
        );
        if let Some(other) = seen.insert(ran.code, document.kind()) {
            panic!(
                "{:?} and {other:?} both exit {:?}; a script cannot tell them apart",
                document.kind(),
                ran.code
            );
        }
    }
    assert_eq!(seen.len(), met.len());

    // None of them is clap's, and none is 0. "You typed it wrong" and "the camera is busy" are
    // different answers to a script deciding whether to retry, and 1 is deliberately left to
    // mean something that is not a typed refusal at all.
    for code in seen.keys() {
        assert!(
            matches!(code, Some(code) if *code != 0 && *code != 1 && *code != 2),
            "{code:?}"
        );
    }

    // The inverse, without which "these codes are distinct" would be satisfied by a binary
    // that failed at everything: a usage mistake is still clap's 2, and a verb that answers is
    // still 0.
    assert_eq!(run(&["frobnicate"]).code, Some(2));
    assert_eq!(run(&["--json", "list"]).code, Some(0));
}

#[test]
fn a_failure_without_json_leaves_standard_output_untouched() {
    // `--json` is a mode, and the ruling did not change what a person gets. This is also the
    // guard on the one place a document would be a disaster:
    // `webcam-handler-cli photo cam:x > shot.jpg` with no `-o` sends the photograph's bytes to
    // standard output, so a refusal that printed JSON there would put a document where an
    // image was supposed to be.
    let ran = run(&["info", "cam:nothing-answers-to-this"]);
    assert!(
        ran.stdout.is_empty(),
        "a failure without --json printed {} byte(s) on standard output: {}",
        ran.stdout.len(),
        ran.stdout
    );
    assert!(
        ran.stderr.starts_with("webcam-handler-cli: "),
        "{}",
        ran.stderr
    );
    // The code is the same one either way: it is a fact about the failure, not about the
    // rendering.
    assert_eq!(
        ran.code,
        Some(i32::from(cli_core::exit_code(&Error::CameraUnknown {
            requested: "cam:nothing-answers-to-this".to_owned(),
        })))
    );
}

/// The refusal a GREY-only camera meets when a caller asks it for an AVI, and the sentence
/// it must not say.
///
/// **Measured at the Chicony IR sensor on 2026-08-15** (note **N129**), which enumerates
/// `GREY` and nothing else. Asking it to record `.avi` is refused, correctly and by design
/// — D7 puts `FormatUnsupported { available }` on the container path in so many words — but
/// the refusal read *"format GREY is unavailable; this camera offers MJPG, JPEG"*, naming
/// two formats that camera has never had. `available` there is the **container's** list, not
/// the device's, and the sentence attributed it to the device.
///
/// That is not a cosmetic slip. The whole point of this variant carrying a payload is that
/// an unattended caller acts on it (AGENTS: "every variant carries what the caller needs to
/// act"), and a caller acting on that sentence retries `--pixel-format MJPG` against a
/// device with no MJPG — a refusal instructing an agent to do something impossible, which is
/// the same defect note **N123** found in `control_inactive`'s "use `--guarded`". A registry
/// that does that is worse than one that says nothing.
///
/// So this asserts the *falsifiable* half rather than the wording: every format the message
/// names is checked against what the camera actually enumerates, and the message may not
/// claim the list is the camera's while naming something absent from it. It goes red against
/// the sentence that shipped and stays green for the D6 refusal, where the list really is the
/// device's.
#[test]
fn a_container_refusal_never_says_the_camera_offers_a_format_it_has_never_had() {
    const GREY_ONLY: &str = "chicony-ir";

    let listed = run_replaying(GREY_ONLY, &["--json", "list"]);
    assert_eq!(listed.code, Some(0), "{}", listed.stderr);
    let document: Value = serde_json::from_str(&listed.stdout).expect("a camera list");
    let camera = document["cameras"][0]["id"]
        .as_str()
        .expect("the replayed camera has an id")
        .to_owned();

    let detail = run_replaying(GREY_ONLY, &["--json", "info", &camera]);
    assert_eq!(detail.code, Some(0), "{}", detail.stderr);
    let detail: Value = serde_json::from_str(&detail.stdout).expect("a camera detail");
    let offered: Vec<String> = detail["formats"]
        .as_array()
        .expect("a format tree")
        .iter()
        .map(|format| {
            format["pixel_format"]
                .as_str()
                .expect("a FourCC string")
                .to_owned()
        })
        .collect();
    // The premise, asserted rather than assumed: a re-capture that taught this sensor MJPG
    // would turn the arm below into a test of a successful recording, silently.
    assert_eq!(
        offered,
        vec!["GREY".to_owned()],
        "{GREY_ONLY} is the grey-only profile this arm needs"
    );

    let scratch = scratch();
    let out =
        Utf8PathBuf::from_path_buf(scratch.path().join("take.avi")).expect("a UTF-8 scratch path");
    let refused = run_replaying(
        GREY_ONLY,
        &[
            "--json",
            "record",
            &camera,
            "-o",
            out.as_str(),
            "--duration",
            "200ms",
        ],
    );

    let failure = refused.refusal();
    let message = &failure.message;
    // The container's list, which is the correct payload and the wrong thing to attribute.
    assert!(
        message.contains("MJPG"),
        "the refusal stopped naming what would be accepted: {message}"
    );
    // The claim itself. Any phrasing that says these belong to the device is false here, and
    // the substring is the one that shipped.
    assert!(
        !message.contains("this camera offers"),
        "the refusal tells a caller the camera offers {offered:?}, which it does not: {message}"
    );
}
