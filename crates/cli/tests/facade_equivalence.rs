//! The one-time criterion D18 asks for: `engine::facade` answers what the executor it
//! replaced answered, byte for byte, on every read verb over the fake (docs/13 P7d).
//!
//! **Why this exists once and then stops being interesting.** P7d rebuilt
//! `webcam-handler-cli`'s `InProcess` executor as parse-and-render around facade calls. That
//! move is only safe if the promoted assembly is the *same* assembly — five modules called in
//! one order — and the honest way to establish it is to compare the answers, not to read the
//! two implementations and agree with oneself. docs/13 says exactly that, and says what owns
//! the claim afterwards: "facade answers byte-identical to the pre-move executor on every read
//! verb over the fake (a one-time equivalence criterion, then the parity gate owns it
//! transitively)". `scripts/gates/cli-parity.sh` already compares `webcam-handler-cli` and
//! `webcam-handler-client` byte for byte on every read verb, and this root now *is* the
//! facade's consumer, so from here on any drift in the facade moves bytes the parity gate is
//! watching. What that gate cannot do is compare against code that no longer exists, which is
//! this suite's whole job and the reason it is written down once.
//!
//! **Where the expected bytes came from.** `crates/cli/tests/fixtures/pre-facade/` holds the
//! standard output of the **pre-move** binary, captured on 2026-08-18 from a build of commit
//! `a907975` — the last revision whose executor assembled these verbs itself. That executor is
//! byte-identical to the one committed at `7dd0c3e^`, the tree before `engine::facade` existed:
//! `git diff 7dd0c3e^ a907975 -- crates/cli/src/main.rs` is nine added lines in `run`, the
//! early return that answers a document verb without naming a backend, and it touches no verb
//! compared here. The fixtures are captured artifacts in this workspace's sense — immutable,
//! replaced wholesale or not at all. **A fixture edited to make this suite green is the one
//! thing that would make it worthless**, because the file is standing in for a program nobody
//! can run any more.
//!
//! Every row was produced by exactly the command line the table below rebuilds, over one
//! committed profile. One profile rather than six is a deliberate limit and it is the right
//! one: what is under test is an *assembly* — which engine calls, in which order, with which
//! arguments — and a difference in that assembly changes every device's answer, not one
//! device's. Which device replays is `corpus-floor.sh`'s subject and the battery's.
//!
//! Two of the seven verbs stamp their answer with a clock this process reads per run, so byte
//! equality is impossible for *any* two runs of *any* build. Those two are compared with the
//! one minted line masked on both sides, and the mask is asserted to have found its line — a
//! mask that matched nothing would compare two unmasked documents and report a stamp
//! difference as a regression, which is a test that fails for the wrong reason.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The committed profile these answers replay, and the camera it enumerates as.
///
/// The id is written out rather than read back from `list`, because `list`'s own answer is one
/// of the things under test: deriving the subject from the output would let a run where every
/// verb answered about the wrong camera agree with itself.
const PROFILE: &str = "chicony-rgb";
const CAMERA: &str = "cam:integrated-camera-integrated-c";

/// The verbs whose whole answer is a document about the device, with no clock in it.
///
/// Each row is the fixture's name and the argv after `--json`. These are the five verbs the
/// facade now assembles that answer the same bytes twice in a row — which is what makes a
/// byte comparison the right instrument for them.
const STAMP_FREE: &[(&str, &[&str])] = &[
    ("list", &["list"]),
    ("info", &["info", CAMERA]),
    ("controls", &["controls", CAMERA]),
    (
        "controls-discover-pairs",
        &["controls", CAMERA, "--discover-pairs"],
    ),
    ("get", &["get", CAMERA, "brightness"]),
];

/// The two verbs whose answer carries a stamp this process mints, and the JSON key that holds
/// it.
///
/// `snapshot` records when the camera was read and a captured profile records when it was
/// captured; both come from `Stamp::now()` at this composition root, because the engine reads
/// no clock (design §2.10). Everything else in both documents is the device's.
const STAMPED: &[(&str, &[&str], &str)] = &[
    ("snapshot", &["snapshot", CAMERA], "taken_at"),
    (
        "profile-capture",
        &["profile", "capture", CAMERA],
        "captured_at",
    ),
];

fn corpus_profile() -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/profiles")
        .join(format!("{PROFILE}.json"));
    assert!(path.exists(), "the corpus is missing {}", path.display());
    path
}

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pre-facade")
        .join(format!("{name}.json"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "{} is the pre-move executor's answer for `{name}` and it could not be read \
             ({error}); a criterion with no expectation to compare against passes by \
             comparing nothing",
            path.display()
        )
    })
}

/// Run the shipped binary over the fake and give back what it wrote to standard output.
fn answer(argv: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_webcam-handler-cli"))
        .arg("--backend")
        .arg("fake")
        .arg("--profile")
        .arg(corpus_profile())
        .arg("--json")
        .args(argv)
        .output()
        .expect("the binary runs");
    assert!(
        output.status.success(),
        "`{}` refused over the fake ({}): {}",
        argv.join(" "),
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("the answer is utf-8")
}

/// Replace the value of the one pretty-printed line whose key is `key`, and say how many lines
/// that was.
///
/// A count rather than a bool because both directions are interesting: zero means the field
/// this test knows about is gone, and more than one means the document grew a second field of
/// that name and this mask is hiding more than it claims to.
fn mask(document: &str, key: &str) -> (String, usize) {
    let needle = format!("\"{key}\":");
    let mut masked = Vec::new();
    let mut hits = 0;
    for line in document.lines() {
        if line.trim_start().starts_with(&needle) {
            hits += 1;
            let indent = &line[..line.len() - line.trim_start().len()];
            masked.push(format!("{indent}\"{key}\": <minted by the run>"));
        } else {
            masked.push(line.to_owned());
        }
    }
    (masked.join("\n"), hits)
}

#[test]
fn every_stamp_free_read_verb_answers_the_bytes_the_pre_facade_executor_answered() {
    assert_eq!(
        STAMP_FREE.len(),
        5,
        "the table lost a verb: a criterion whose population shrank silently is one that \
         stopped covering what it says it covers"
    );
    for (name, argv) in STAMP_FREE {
        assert_eq!(
            answer(argv),
            fixture(name),
            "`webcam-handler-cli --json {}` answers different bytes than the executor that \
             assembled it before `engine::facade` existed. The facade is supposed to be the \
             same five engine calls in the same order (docs/12 D18: \"it is not a new \
             layer\"), so a difference here is a change to what this tool answers, whatever \
             else it is — reconcile `crates/engine/src/facade.rs` with \
             `crates/cli/tests/fixtures/pre-facade/{name}.json`, and never the other way \
             round",
            argv.join(" ")
        );
    }
}

#[test]
fn the_two_stamped_read_verbs_differ_only_in_the_stamp_this_run_minted() {
    assert_eq!(
        STAMPED.len(),
        2,
        "the table lost a verb: a criterion whose population shrank silently is one that \
         stopped covering what it says it covers"
    );
    for (name, argv, key) in STAMPED {
        let (now, now_hits) = mask(&answer(argv), key);
        let (before, before_hits) = mask(&fixture(name), key);
        assert_eq!(
            (now_hits, before_hits),
            (1, 1),
            "`{}`'s answer and the pre-move fixture must each carry exactly one `{key}` line \
             for this comparison to mean what it says; a mask that found none or several is \
             hiding a difference rather than a clock",
            argv.join(" ")
        );
        assert_eq!(
            now,
            before,
            "`webcam-handler-cli --json {}` answers different bytes than the executor that \
             assembled it before `engine::facade` existed, in a field that is not the `{key}` \
             stamp this run minted — reconcile `crates/engine/src/facade.rs` with \
             `crates/cli/tests/fixtures/pre-facade/{name}.json`",
            argv.join(" ")
        );
    }
}
