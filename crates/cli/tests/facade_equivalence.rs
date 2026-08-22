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
//! minted lines masked on both sides, and each mask is asserted to have found its line — a
//! mask that matched nothing would compare two unmasked documents and report a stamp
//! difference as a regression, which is a test that fails for the wrong reason.
//!
//! **And one of them stamps a fact about the *machine*, which is the other half of the same
//! sentence and was missing from it until note N350.** A captured profile's provenance carries
//! `kernel` — `/proc/sys/kernel/osrelease`, read at this composition root on every backend
//! (`engine::profile::kernel_release`) — so the compared bytes held a value this laptop
//! supplied. That is green wherever both sides were produced on one host and red everywhere
//! else, and it is how this suite failed on GitHub Actions the first time a runner ever got far
//! enough to execute it: `7.0.0-1011-azure` against this tree's `7.0.0-30-generic`. It was a
//! true finding about the test, not about the facade — `schema::profile` already says
//! provenance "rides outside both" halves and is "Never compared", and
//! `DeviceProfile::compare` already honours that by destructuring `ProfileInvariant` alone.
//! This suite was the one place in the tree comparing it, so the field is masked here for the
//! same reason the clock is, and for a reason worth spelling differently: a clock makes two
//! runs differ, a host fact makes two *machines* differ.
//!
//! Masking one field would have closed one spelling of that class rather than the class
//! (AGENTS/N249), so two arms hold it shut. [`a_stamped_answer_produced_on_another_host_still_compares_equal`]
//! rewrites every value in [`HOST_FACTS`] to a foreign spelling and re-runs the whole
//! comparison, which goes red the moment a machine's answer reaches the compared bytes
//! unmasked — it reddens today against this file with `"kernel"` removed from [`STAMPED`],
//! which is the inverse the selftest drives. And
//! [`every_provenance_field_this_answer_carries_is_classified_by_where_it_comes_from`]
//! destructures `ProfileProvenance` exhaustively, so a sixth field cannot be added without
//! somebody saying whether the run, the host or the tree supplies it — the mechanism
//! `profile-partition-is-closed.sh` gives the invariant half, applied to the block D15 left
//! out of its partition.

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

/// The two verbs whose answer carries something this *run* or this *host* supplied, and the
/// JSON keys that hold it.
///
/// `snapshot` records when the camera was read and a captured profile records when it was
/// captured; both come from `Stamp::now()` at this composition root, because the engine reads
/// no clock (design §2.10). A captured profile also records the kernel it was captured
/// under — `engine::profile::kernel_release()`, the one home for that fact (note **N350**).
/// Everything else in both documents is the device's or the tree's.
///
/// A list per verb rather than one key for both, because the two verbs do not carry the same
/// fields: `snapshot` emits no `kernel` line at all, so a shared key list would ask
/// [`mask`] to find a field that is not there and the hit assertion below would go red on a
/// tree with nothing wrong with it.
const STAMPED: &[(&str, &[&str], &[&str])] = &[
    ("snapshot", &["snapshot", CAMERA], &["taken_at"]),
    (
        "profile-capture",
        &["profile", "capture", CAMERA],
        &["captured_at", "kernel"],
    ),
];

/// One row of [`HOST_FACTS`]: the field's name as it appears in the answer, the live reader the
/// *product* uses to fill it, and the spelling another host answers.
///
/// Named rather than written inline because the tuple is the subject of two tests and clippy is
/// right that a bare three-tuple of a function pointer says nothing about which member is which.
type HostFact = (&'static str, fn() -> String, &'static str);

/// One row of the walk [`every_compared_verb`] builds: the fixture's name, the argv after
/// `--json`, and the keys masked on both sides of that verb's comparison.
type ComparedVerb = (
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
);

/// The values in a compared answer that this *machine* supplies, each with the live reader
/// that produces it here and the spelling another host would have written.
///
/// The foreign spelling is the one GitHub Actions actually reported (`ubuntu-26.04`,
/// 2026-08-22) rather than an invented string, so the arm that uses it reproduces a failure
/// that happened rather than one imagined.
///
/// Keyed by the *reader*, not by the JSON key: [`a_stamped_answer_produced_on_another_host_still_compares_equal`]
/// substitutes on the value this host currently answers, so it cannot degenerate into a second
/// copy of [`STAMPED`]'s key list and agree with it by construction.
const HOST_FACTS: &[HostFact] = &[(
    "kernel",
    engine::profile::kernel_release,
    "7.0.0-1011-azure",
)];

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

/// Apply [`mask`] once per key, and give back the hit count of each in the key's own order.
///
/// Sequential rather than a single pass, so each key's count is that key's — a combined total
/// would let a document that lost one field and grew a duplicate of another add up to the
/// number this test expects and pass.
fn mask_all(document: &str, keys: &[&str]) -> (String, Vec<usize>) {
    let mut masked = document.to_owned();
    let mut hits = Vec::with_capacity(keys.len());
    for key in keys {
        let (next, found) = mask(&masked, key);
        masked = next;
        hits.push(found);
    }
    (masked, hits)
}

/// Every verb this suite compares, with the keys masked on it — the five stamp-free rows
/// carrying an empty list.
///
/// One walk over all seven rather than two walks over the two tables, because the arm that
/// uses it asks a question about *every* answer this suite compares: a host fact that landed
/// in `info`'s or `controls`'s bytes would be exactly as invisible as the one that landed in
/// a captured profile's, and there is no reason the class should be watched on two rows and
/// not on seven.
fn every_compared_verb() -> Vec<ComparedVerb> {
    let mut rows: Vec<_> = STAMP_FREE
        .iter()
        .map(|(name, argv)| (*name, *argv, &[] as &[&str]))
        .collect();
    rows.extend(
        STAMPED
            .iter()
            .map(|(name, argv, keys)| (*name, *argv, *keys)),
    );
    rows
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
    for (name, argv, keys) in STAMPED {
        let (now, now_hits) = mask_all(&answer(argv), keys);
        let (before, before_hits) = mask_all(&fixture(name), keys);
        let expected = vec![1_usize; keys.len()];
        assert_eq!(
            (&now_hits, &before_hits),
            (&expected, &expected),
            "`{}`'s answer and the pre-move fixture must each carry exactly one line per \
             masked key {keys:?} for this comparison to mean what it says; a mask that found \
             none or several is hiding a difference rather than a stamp. Counts here: live \
             {now_hits:?}, fixture {before_hits:?}",
            argv.join(" ")
        );
        assert_eq!(
            now,
            before,
            "`webcam-handler-cli --json {}` answers different bytes than the executor that \
             assembled it before `engine::facade` existed, in a field that is none of the \
             {keys:?} this run or this host supplied — reconcile \
             `crates/engine/src/facade.rs` with \
             `crates/cli/tests/fixtures/pre-facade/{name}.json`",
            argv.join(" ")
        );
    }
}

/// The arm that goes red on a tree where the two sides were produced on different hosts.
///
/// **What it is for.** The suite above compares committed bytes against bytes this machine
/// produces, so anything the *machine* contributes is an expectation about one laptop wearing
/// the clothes of an expectation about the code. That is precisely how this file failed on
/// GitHub Actions and passed here for four days (note **N350**), and masking `kernel` fixes
/// the one field that did it without saying anything about the next one. This arm says the
/// general thing: rewrite every value in [`HOST_FACTS`] to the spelling a different host
/// answers, and the comparison must still hold.
///
/// **Why it can fail.** Remove `"kernel"` from [`STAMPED`]'s `profile-capture` row and this
/// goes red, with the sentence CI printed; that is the inverse `scripts/gates/selftest.sh`
/// seeds, and it is driven by the thing under test rather than by a stub. It is red *today*,
/// on this laptop, against the tree as it stood before N350 — no runner required to see it.
///
/// **Why the substitution is keyed on the value.** Replacing occurrences of what
/// `engine::profile::kernel_release()` answers *right now* means the arm reads the same home
/// the product reads. Keying on the JSON key name instead would make this a second copy of
/// [`STAMPED`], and two copies of one list agree with each other whatever the answer does.
#[test]
fn a_stamped_answer_produced_on_another_host_still_compares_equal() {
    assert_eq!(
        HOST_FACTS.len(),
        1,
        "the table lost a row: a criterion whose population shrank silently is one that \
         stopped covering what it says it covers"
    );
    for (field, read_here, foreign) in HOST_FACTS {
        let here = read_here();
        assert!(
            !here.is_empty() && here != *foreign,
            "`{field}` reads `{here}` on this host and the foreign spelling this arm \
             substitutes is `{foreign}`; a rewrite from a value to itself, or from an empty \
             string, would substitute nothing and this arm would pass by doing nothing"
        );

        let mut rewritten = 0_usize;
        for (name, argv, keys) in every_compared_verb() {
            let live = answer(argv);
            rewritten += live.matches(here.as_str()).count();
            let elsewhere = live.replace(here.as_str(), foreign);

            let (now, now_hits) = mask_all(&elsewhere, keys);
            let (before, before_hits) = mask_all(&fixture(name), keys);
            assert_eq!(
                (&now_hits, &before_hits),
                (&vec![1_usize; keys.len()], &vec![1_usize; keys.len()]),
                "masking {keys:?} over `{}` found {now_hits:?} live and {before_hits:?} in \
                 the fixture; this arm cannot say anything about host facts while the mask \
                 it shares with the suite above is itself finding the wrong lines",
                argv.join(" ")
            );
            assert_eq!(
                now,
                before,
                "`webcam-handler-cli --json {}` answers bytes that carry this host's own \
                 `{field}`. Rewriting it from `{here}` to `{foreign}` — the spelling a GitHub \
                 `ubuntu-26.04` runner reports — made the answer differ from \
                 `crates/cli/tests/fixtures/pre-facade/{name}.json`, so a value this machine \
                 supplied is inside the compared bytes and this criterion can only hold where \
                 both sides were produced on one host. Add `{field}` to this verb's key list \
                 in `STAMPED`, or take the field out of the answer",
                argv.join(" ")
            );
        }

        assert!(
            rewritten >= 1,
            "`{field}` reads `{here}` here and that string appears in none of the {} answers \
             this suite compares, so this arm substituted nothing and proved nothing. Either \
             the field left the answer — in which case its `HOST_FACTS` row and its `STAMPED` \
             key should go with it — or the reader in that row is no longer the one the \
             product uses",
            every_compared_verb().len()
        );
    }
}

/// Every field of a captured profile's provenance, sorted into what supplies it.
///
/// **The mechanism is the destructuring, not the assertions.** `ProfileProvenance` has five
/// fields and nothing closed it: a sixth host-derived field added tomorrow would land in this
/// suite's compared bytes with nothing red, exactly as `kernel` did. Destructured
/// exhaustively, a sixth field stops this file compiling until somebody writes down which of
/// the three supplies it — the same shape `profile-partition-is-closed.sh` gives the invariant
/// half, applied to the block D15 deliberately left outside its partition.
///
/// The three assertions on the *tree* fields are the other half of the argument: they are the
/// reason those three are safe to compare unmasked. `tool_version` is this workspace's version
/// at this commit, `capturer` is a clap literal, and `backend` proves the answer came from the
/// fake — all three identical on any host at one revision, and all three worth keeping inside
/// the comparison rather than masking the block wholesale.
#[test]
fn every_provenance_field_this_answer_carries_is_classified_by_where_it_comes_from() {
    let document: schema::profile::DeviceProfile =
        serde_json::from_str(&answer(&["profile", "capture", CAMERA]))
            .expect("`profile capture --json` answers a device profile");

    // Exhaustive by construction: adding a field to `ProfileProvenance` is a compile error
    // here, and that is this test's whole subject.
    let schema::profile::ProfileProvenance {
        // clock — this run mints it, so it is masked and `STAMPED` carries its key.
        captured_at,
        // host — this machine supplies it, so it is masked *and* `HOST_FACTS` carries it.
        kernel,
        // tree — the same on every host at this commit, so it stays inside the comparison.
        tool_version,
        // tree — a clap default, never read from the environment.
        capturer,
        // tree — the routing this suite depends on being the fake.
        backend,
    } = document.provenance;

    let (_, _, profile_capture_keys) = STAMPED
        .iter()
        .find(|(name, _, _)| *name == "profile-capture")
        .expect("`profile-capture` is one of the stamped verbs");

    for (kind, key) in [("clock", "captured_at"), ("host", "kernel")] {
        assert!(
            profile_capture_keys.contains(&key),
            "`{key}` is a {kind} fact — this run or this machine supplies it, not the device \
             and not the tree — so it cannot be inside the bytes compared against a fixture \
             captured on 2026-08-18 on another run of another host. Add it to \
             `STAMPED`'s `profile-capture` key list"
        );
    }
    assert!(
        HOST_FACTS.iter().any(|(field, _, _)| *field == "kernel"),
        "`kernel` is the one provenance field a *machine* answers for itself \
         (`engine::profile::kernel_release`), so masking it is not enough: \
         `a_stamped_answer_produced_on_another_host_still_compares_equal` needs its row in \
         `HOST_FACTS` or nothing proves the mask is doing the job it was added for"
    );
    assert!(
        !captured_at.to_string().is_empty(),
        "the clock field parsed to an empty stamp, which is not a clock"
    );
    assert_eq!(
        kernel,
        engine::profile::kernel_release(),
        "the answer's `kernel` is supposed to be this host's own, read through the one home \
         `engine::profile::kernel_release` (note N350's subject). A value from anywhere else \
         means the classification above is wrong and the mask is hiding the wrong field"
    );

    let fixture_provenance: schema::profile::DeviceProfile =
        serde_json::from_str(&fixture("profile-capture")).expect("the fixture is a profile");
    assert_eq!(
        (tool_version.as_str(), capturer.as_str(), backend,),
        (
            fixture_provenance.provenance.tool_version.as_str(),
            fixture_provenance.provenance.capturer.as_str(),
            fixture_provenance.provenance.backend,
        ),
        "`tool_version`, `capturer` and `backend` are classified above as facts about the \
         *tree*, which is what makes it right to leave them inside the byte comparison rather \
         than masking the provenance block wholesale. One of them disagreeing with the \
         pre-move fixture means it is not a tree fact after all — and the three carry real \
         weight: `backend` is what proves this answer came from the fake rather than from a \
         device, and `capturer` is the clap default the pre-move executor also used"
    );
}
