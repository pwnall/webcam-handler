//! **R1-web** — the pinned Playwright + Chromium rung, launched from here (design §3.1
//! R1-web, docs/7 P5d, docs/9 Part 2's "The R1-web Playwright rung" row).
//!
//! `web_client.rs` is this file's neighbour and it says out loud where it stops: "There is no
//! headless DOM here, no JSDOM, and no assertion of the form *and therefore the `<select>` has
//! two options*". This is the sub-milestone that makes those claims true, in a real headless
//! Chromium — or reports honestly that it could not. Rubric B7 is the reason either half is
//! worth the machinery: **a browser behavior verified only through the JSON the page consumes
//! is not verified.**
//!
//! ## The rung is here and the claims are in JavaScript, on purpose
//!
//! What this file owns is the *composition*: `daemon::http::open` over the fake backend, the
//! token it minted, the camera it serves, and the child process it hands all of that to. What
//! `tests/browser/` owns is what a browser does with it. The split is the one design §3.1
//! names — "launched as a subprocess from a `webcam-handler-daemon` integration-gate test" —
//! and it is what keeps node out of `cargo build` entirely: nothing below runs at build time,
//! this workspace has no build script at all (asserted, in
//! [`node_is_never_a_build_dependency`]), and a machine with no node compiles every crate.
//!
//! ## Three preconditions, three different named skips, and none of them is a pass
//!
//! AGENTS rule 3: every auto-skipping rung reports a **named, counted skip** — never silence.
//! A rung that declines has to say *what was missing* and *what it therefore did not claim*,
//! because a skip nobody can distinguish from a pass is the defect the whole rung ladder is
//! built against. So the preconditions are asked one at a time, each has its own sentence and
//! its own remedy, and the decline names every claim it cost by title and counts the
//! assertions inside them (`tests/browser/claims.json`).
//!
//! The skip path is not taken on trust either: [`a_declined_rung_names_what_was_missing_and_
//! counts_what_it_did_not_run`] drives every arm against a **host it fabricates**, which is the
//! "prove it can go red" rule applied to the *reporting* rather than to the assertions. That
//! test's own header says why the host is a value rather than the machine the suite is running
//! on, and what it cost to learn.
//!
//! `.config/nextest.toml` gives this binary `success-output = "final"`, and that line is part
//! of the mechanism rather than a convenience: nextest hides a passing test's output, so
//! without it a decline would be printed into a void and `just ci` would carry a silent skip —
//! which is the exact shape rule 3 forbids.
//!
//! ## Privacy, and why this rung may take a screenshot at all
//!
//! A frame may contain a person (AGENTS, "Hardware and privacy"). This rung drives the **fake**
//! backend — the camera it previews is `synthetic_basic`, whose frames are generated — so there
//! is no camera in the room anywhere near it. Everything the browser writes still goes to the
//! one scratch root under `target/` (note **N84**): traces on failure, screenshots on failure,
//! and the JSON report. Nothing it produces can be committed by accident, and — which is the
//! half `scratch/` could not offer — nothing it produces is even visible to the walk that
//! sniffs this tree for frames.
//!
//! ## Playwright's place in the dependency inventory
//!
//! Design §2.8 puts it where ffprobe and mpv already are: **test-time external tooling,
//! outside the shipped licence inventory** — "node is a test-host convenience, never a build
//! dependency". The distinction that matters is the product's: `crates/web/assets/` vendors or
//! hand-writes everything it loads, and `scripts/gates/no-external-fetch-in-web.sh` is what
//! keeps that true. Nothing in `tests/browser/` is served to a browser by
//! `webcam-handler-daemon`; it is the thing that drives one.

use std::collections::BTreeSet;
use std::process::Command;
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use daemon::http;
use daemon::server::Wchd;
use daemon::shutdown::Shutdown;
use engine::paths::TempRuntimeDir;
use engine::store::{LockProtocol, SessionStore, StoreLock, TempStore};
use fake::FakeBackend;
use schema::backend::CameraBackend;
use schema::camera::{CameraId, FrameInterval, FrameSize, FrameSizeInfo, PixelFormat};
use serde_json::Value;

// ------------------------------------------------------------------- the claims manifest

/// The claims this rung makes, as the suite's own list of them.
///
/// Compiled in rather than read at run time, so the decline path needs no filesystem: a
/// machine missing every precondition can still say what it did not do.
const CLAIMS: &str = include_str!("browser/claims.json");

/// One browser claim: the Playwright test title, what it establishes, and how big it is.
#[derive(Debug, PartialEq, Eq)]
struct Claim {
    title: String,
    what: String,
    assertions: u64,
}

/// The manifest, parsed.
fn claims() -> Vec<Claim> {
    let document: Value = serde_json::from_str(CLAIMS).expect("claims.json is JSON");
    document["claims"]
        .as_array()
        .expect("claims.json carries a claims array")
        .iter()
        .map(|claim| Claim {
            title: claim["title"]
                .as_str()
                .expect("every claim has a title")
                .to_owned(),
            what: claim["what"]
                .as_str()
                .expect("every claim says what it establishes")
                .to_owned(),
            assertions: claim["assertions"]
                .as_u64()
                .expect("every claim counts its assertions"),
        })
        .collect()
}

// ------------------------------------------------------------------------- the claims floor

/// A floor's two numbers: how many claims this rung makes, and how much they assert.
#[derive(Debug, Clone, Copy)]
struct Floor {
    claims: usize,
    assertions: u64,
}

/// What the manifest must carry, as counts a reviewer has to lower on purpose.
///
/// [`verify_report`] compares the manifest against a run for **consistency**, and consistency
/// is not a floor. A claim deleted from `browser/client.spec.mjs` *and* from
/// `browser/claims.json` agrees with itself perfectly: the title sets still match, every count
/// still matches, the exit status is zero, and this rung goes on reporting that everything it
/// claims to check was checked — while the decline line on the next host quietly names one
/// fewer thing it did not do. The number that would have noticed is the one nobody wrote down.
///
/// So it is written down here, in the shape `scripts/mutants-accepted.txt` uses and for its
/// reason: a register nobody can shrink by accident. Deleting a claim is then two edits and a
/// diff a reviewer reads, which is what "deliberately" means in a repository.
///
/// **It is a floor and not an equality, and that is the one place this differs from that
/// register.** An acceptance list must match its survivors in both directions because a stale
/// acceptance is a lie; a claim *added* to a spec is the rung growing, and a constant that
/// turned growth red is a constant people route around. So a surplus is reported as a counted
/// line naming how far ahead the manifest has got — never silence, AGENTS rule 3 — and raising
/// this pair is what closes a boundary.
///
/// Measured, never estimated: **23 claims and 198 assertions** are what `claims.json` carries on
/// the run that landed this (the same two numbers the decline line above prints).
///
/// Raised at P6c's second half, which is what "raising this pair closes a boundary" looks like
/// in practice — and it was not optional. The manifest grew by the recording claim (note
/// **N117**), and the *self-test* below drives a manifest one claim short of the floor: one
/// short of sixteen is fifteen, which cleared a floor of fifteen and complained about nothing,
/// so the arm that has to go red went green. A floor left behind its manifest is a floor that
/// lets that many claims be deleted in silence.
///
/// It was written as 11 and 84 hours earlier, and moved in the same session — which is worth a
/// sentence, because *how* it moved is the mechanism working rather than a number being patched.
/// Two repairs from the P5 review ran concurrently in isolated worktrees: one raised the manifest
/// to 15/117 by adding four claims (note **N96**), the other introduced this floor against the
/// 11/84 it could see. Neither touched the other's files, so nothing collided textually — and the
/// tree was still wrong, because a floor four claims below its manifest lets four claims be
/// deleted in silence, which is the exact hole this constant exists to close. The arm that caught
/// it is the one that drives a manifest one claim short: one short of 15 is 14, which clears a
/// floor of 11 and complains about nothing. **File isolation prevents collisions between edits and
/// not between meanings** (note **N98**).
/// Raised again at the G6 repair pass, by the five claims docs/11 §2.2 asked this rung for. The
/// review's fourth HIGH was a preview stream that never ends, and what let it survive a review
/// which read the same module was that **every claim this rung made was about something
/// starting** — a menu painting, a frame arriving, a refusal rendering. The five added then were
/// about things stopping (notes **N152**, **N154**–**N157**), and this pair moves with them in
/// the same diff, because a floor left behind its manifest is a floor that lets that many claims
/// be deleted in silence.
///
/// Raised twice more by the repair pass over that batch. Both additions are the same lesson from
/// the other side: the five claims about *stopping* left the **healthy** path of what they drove
/// unasserted. The heartbeat's every claim severed the link first, so no run had ever seen a
/// heartbeat answered and the whole mechanism rested on an unread sentence about jsonrpsee (note
/// **N157**'s correction); and the calibration view's reorder was argued closed in prose rather than driven at
/// all (note **N154**'s correction). A rung that only asks whether a thing stops cannot see a
/// thing that stops when it should not.
///
/// Raised once more by the B7 repair pass, for the claim that closes the last widget in this
/// panel that could put a number in front of an operator the device never sent. The menu with
/// no current had a claim; the *slider* with no current did not, and a range input cannot hold
/// "nothing" — it held the declared default, live, one relative gesture from a write (note
/// **N199**).
///
/// Raised at P9a for D20's two layout claims, and they are the first pair here that assert about
/// *arrangement* rather than about content: the preview and the control being adjusted visible
/// together at every scroll position of a 77-control column, and the same claim one column wide
/// where a sticky pane inside the wrong scroll container sticks to nothing. Both are invisible to
/// every other kind of test this project has — the JSON is identical either way — which is rubric
/// B7's sentence arriving at a layout (note **N262**).
///
/// Raised again at P9c for the flow itself: a session started, planned, swept and *chosen from*
/// through the page's own buttons, with `selector: human` read back on a second socket — because
/// the page's belief about a session is exactly what must not be the evidence — and the arm that
/// holds an out-of-order click to being the daemon's refusal rendered rather than a button the
/// page greyed out on a guess.
const FLOOR: Floor = Floor {
    claims: 28,
    assertions: 259,
};

/// Where `claims` falls short of `floor`, one sentence per shortfall. Empty is green.
///
/// A pure function over values, so the arms in
/// [`a_declined_rung_names_what_was_missing_and_counts_what_it_did_not_run`] can drive it in
/// both directions without a filesystem: AGENTS rule 2 asks the red-on-inverse to be written,
/// and a floor whose failing direction has never been seen is a floor nobody has checked.
fn below_the_floor(claims: &[Claim], floor: &Floor) -> Vec<String> {
    let mut short = Vec::new();
    for claim in claims {
        if claim.assertions == 0 {
            short.push(format!(
                "`{title}` counts zero assertions; a claim that asserts nothing is a title in a \
                 report, and the skip line would go on offering it as something this host did \
                 not run",
                title = claim.title,
            ));
        }
    }
    if claims.len() < floor.claims {
        short.push(format!(
            "the manifest carries {count} claim(s) and the floor is {floor}; a claim deleted \
             from the spec and from claims.json together is green everywhere else in this file, \
             which is what the floor is for — lower it deliberately, in the same diff",
            count = claims.len(),
            floor = floor.claims,
        ));
    }
    let total: u64 = claims.iter().map(|claim| claim.assertions).sum();
    if total < floor.assertions {
        short.push(format!(
            "the manifest counts {total} assertion(s) and the floor is {floor}; a claim that \
             quietly halved what it asserts keeps its title and its place in the skip line",
            floor = floor.assertions,
        ));
    }
    short
}

/// How far the manifest has got ahead of the floor, when it has. Named and counted, never
/// silence: a floor that is behind is not a finding, and it is not nothing either.
fn above_the_floor(claims: &[Claim], floor: &Floor) -> Option<String> {
    let total: u64 = claims.iter().map(|claim| claim.assertions).sum();
    let extra_claims = claims.len().saturating_sub(floor.claims);
    let extra_assertions = total.saturating_sub(floor.assertions);
    if extra_claims == 0 && extra_assertions == 0 {
        return None;
    }
    Some(format!(
        "r1-web: the claims floor is {extra_claims} claim(s) and {extra_assertions} \
         assertion(s) behind the manifest ({} and {total} today); raise it when this boundary \
         closes",
        claims.len(),
    ))
}

// ----------------------------------------------------------------------- the preconditions

/// Why the rung declined, in the two halves a reader needs: what was missing, and what to do.
#[derive(Debug)]
struct Declined {
    missing: String,
    remedy: String,
}

/// Everything the rung needs to run, once it is known to be there.
#[derive(Debug)]
struct Ready {
    node: Utf8PathBuf,
    cli: Utf8PathBuf,
}

/// Ask, in order, whether this host can run the rung at all.
///
/// The order is the order of the answers' usefulness: no node at all is a different sentence
/// from a node with no Playwright beside it, which is different again from a Playwright whose
/// pinned browser was never fetched — and telling an operator "install node" when node is
/// installed is how a skip stops being read.
///
/// `node` is a path rather than a lookup so the decline arms can be driven; `WCH_E2E_NODE` is
/// the same seam for a host whose node is not on `PATH`.
fn preconditions(
    node: &Utf8Path,
    suite: &Utf8Path,
    browsers: &Utf8Path,
    build: &str,
) -> Result<Ready, Declined> {
    // Asked by *running* it, which is the only question worth asking: a `node` on `PATH` that
    // cannot execute is the same problem as no node, and both are this rung's answer to give.
    let version = Command::new(node.as_std_path()).arg("--version").output();
    let Ok(version) = version else {
        return Err(Declined {
            missing: format!("no usable node at `{node}`"),
            remedy: "install Node.js, or point WCH_E2E_NODE at one — node is a test-host \
                     convenience here and never a build dependency, so every crate still builds \
                     without it"
                .to_owned(),
        });
    };
    if !version.status.success() {
        return Err(Declined {
            missing: format!("`{node} --version` failed"),
            remedy: "repair the node installation, or point WCH_E2E_NODE at a working one"
                .to_owned(),
        });
    }

    let cli = suite.join("node_modules/@playwright/test/cli.js");
    if !cli.is_file() {
        return Err(Declined {
            missing: format!("the pinned Playwright is not installed under `{suite}`"),
            remedy: "run `just rung-web-install`, which is `npm ci` against the committed \
                     package-lock.json plus the one pinned browser"
                .to_owned(),
        });
    }

    // The browser is pinned by build number, and this is where "pinned" stops being a manifest
    // claim: a host that has Playwright but never fetched build `build` would otherwise fail
    // deep inside the child process with a download error, which is a red rung rather than an
    // absent one. The two directory spellings are Playwright's own — a full Chromium and the
    // headless shell it prefers when `headless` is set — and either one is enough to launch.
    let installed = [
        browsers.join(format!("chromium-{build}")),
        browsers.join(format!("chromium_headless_shell-{build}")),
    ];
    if !installed.iter().any(|path| path.is_dir()) {
        return Err(Declined {
            missing: format!("the pinned Chromium build {build} is not in `{browsers}`"),
            remedy: "run `just rung-web-install`; the rung refuses to drive whatever browser \
                     happens to be on the host, because a verdict that depends on somebody \
                     else's upgrade is not a verdict"
                .to_owned(),
        });
    }

    Ok(Ready {
        node: node.to_owned(),
        cli,
    })
}

/// The decline, as the lines a run prints.
///
/// One line begins with `SKIP` and exactly one does, so `scripts/rung-web.sh` can count
/// declines without counting the claims they name. Everything after it is the naming half:
/// AGENTS rule 3 asks for counted *and* named, and a bare number would leave a reader knowing
/// that something was not checked without knowing what.
fn decline_report(declined: &Declined, claims: &[Claim]) -> String {
    let assertions: u64 = claims.iter().map(|claim| claim.assertions).sum();
    let mut report = format!(
        "SKIP r1-web: {missing} — {remedy}; {count} browser claim(s) and {assertions} \
         assertion(s) were not run\n",
        missing = declined.missing,
        remedy = declined.remedy,
        count = claims.len(),
    );
    for claim in claims {
        report.push_str(&format!(
            "r1-web:   - {title} ({assertions} assertions) — {what}\n",
            title = claim.title,
            assertions = claim.assertions,
            what = claim.what,
        ));
    }
    report
}

/// Where Playwright keeps its browsers on this host.
///
/// Its own resolution order, reproduced because the rung has to answer "is the pinned build
/// here" *before* handing the question to a process that would answer it by downloading one.
fn browsers_root() -> Utf8PathBuf {
    if let Ok(explicit) = std::env::var("PLAYWRIGHT_BROWSERS_PATH") {
        return Utf8PathBuf::from(explicit);
    }
    if let Ok(cache) = std::env::var("XDG_CACHE_HOME") {
        return Utf8PathBuf::from(cache).join("ms-playwright");
    }
    Utf8PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache/ms-playwright")
}

/// The suite's directory.
fn suite_dir() -> Utf8PathBuf {
    Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser")
}

/// The workspace root, which is what the no-build-script claim below walks.
fn workspace_root() -> Utf8PathBuf {
    Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Utf8Path::parent)
        .expect("crates/daemon sits two directories under the workspace root")
        .to_owned()
}

/// The pinned Chromium build, out of the one file that holds every pin.
fn pinned_chromium_build(suite: &Utf8Path) -> String {
    let manifest = std::fs::read_to_string(suite.join("package.json"))
        .expect("the suite's package.json is committed beside it");
    let manifest: Value = serde_json::from_str(&manifest).expect("package.json is JSON");
    manifest["wch"]["chromiumBuild"]
        .as_str()
        .expect("package.json pins a Chromium build")
        .to_owned()
}

// ---------------------------------------------------------------------------- the fixture

/// One daemon on loopback, over the fake backend, with the token it printed.
struct Serving {
    serving: http::Serving,
    camera: CameraId,
    /// The camera whose control set does not fit on a screen (design D20's layout fixture).
    wide_camera: CameraId,
    /// The second camera, which exists so a claim can *switch* between two of them.
    ///
    /// Two claims need a page to stop looking at one camera and start looking at another, and
    /// neither can be made against a machine with one camera on it: whether the preview a page
    /// walked away from is a preview the daemon stopped (docs/11 **H4**, note **N152**) and
    /// whether the panel on screen belongs to the camera on screen (**M32**, note **N154**)
    /// are both questions about the *switch*. It replays the same profile with its identity
    /// and one control value changed, so the two panels are told apart by what the device
    /// reports rather than by their order on the page.
    second_camera: CameraId,
    /// Where a claim that starts a recording may write one.
    ///
    /// Handed to the browser rather than chosen there, because `wch_record_start` takes an
    /// **absolute server path** (note **N110**) and the one process that knows a writable
    /// throw-away directory is this one — a spec that picked `/tmp/take.avi` would be a suite
    /// writing outside the scratch root AGENTS gives it (note **N84**).
    take: Utf8PathBuf,
    shutdown: Shutdown,
    _lock: Arc<StoreLock>,
    _state: TempStore,
    _runtime: TempRuntimeDir,
}

impl Serving {
    /// Start one. Must be called from inside a tokio runtime.
    ///
    /// `web_client.rs`'s composition, and for its reasons: the listener `main` opens, over the
    /// daemon's own fan-out and stop token, so the browser is driving the shipped arrangement
    /// rather than one this file assembled.
    async fn start() -> Serving {
        let backend = Arc::new(
            FakeBackend::new(vec![
                small_mjpeg_camera(),
                second_mjpeg_camera(),
                wide_camera(),
            ])
            .expect("the synthetic profile is this build's version"),
        );
        let cameras = backend
            .enumerate()
            .expect("the fake enumerates what it replays");
        let camera = cameras.first().expect("the first camera").id.clone();
        let second_camera = cameras.get(1).expect("the second camera").id.clone();
        let wide_camera = cameras.get(2).expect("the wide camera").id.clone();
        assert_ne!(
            camera, second_camera,
            "the two profiles collapsed into one identity, so no claim below can switch \
             between them"
        );
        assert_ne!(
            camera, wide_camera,
            "the layout fixture collapsed into the ordinary camera's identity, so the \
             workbench claim would be made against eighteen controls rather than the \
             seventy-seven it is sized for"
        );

        let state = TempStore::new().expect("a state directory");
        let store = SessionStore::new(state.root());
        let lock = Arc::new(
            store
                .lock(LockProtocol::HeldForLifetime)
                .expect("nothing else holds a throw-away state directory"),
        );
        let runtime = TempRuntimeDir::new().expect("a runtime directory");

        let shutdown = Shutdown::new();
        let wchd = Wchd::new(
            Arc::clone(&backend) as Arc<dyn CameraBackend>,
            SessionStore::new(state.root()),
            Arc::clone(&lock),
            shutdown.clone(),
        );
        let methods = daemon::server::mount(wchd.clone()).expect("the T5 surface mounts once");
        let serving = http::open(
            "127.0.0.1:0".parse().expect("a literal address"),
            false,
            methods,
            wchd.previews(),
            Arc::new(SessionStore::new(state.root())),
            shutdown.clone(),
        )
        .await
        .expect("loopback on an ephemeral port");

        Serving {
            serving,
            camera,
            second_camera,
            wide_camera,
            take: runtime.base().join("browser-take.avi"),
            shutdown,
            _lock: lock,
            _state: state,
            _runtime: runtime,
        }
    }

    /// Stop it the way `main` stops it, and wait.
    async fn stop(self) {
        self.shutdown.cancel();
        self.serving
            .stopped()
            .await
            .expect("the web listener ended");
    }
}

/// The committed profile with its MJPEG modes rewritten small.
///
/// `web_client.rs`'s fixture and its argument: 160×120 at 30 fps is an ordinary webcam mode,
/// and it makes every preview frame a few kilobytes rather than a quarter-megapixel render —
/// which matters more here than there, because a browser decodes each one. **The controls are
/// untouched**, and they are the subject: all four edges design §1.2 recorded are in them.
fn small_mjpeg_camera() -> schema::profile::DeviceProfile {
    let mut profile = testkit::fixtures::synthetic_basic();
    for format in &mut profile.invariant.formats {
        if format.pixel_format == PixelFormat::MJPG {
            format.sizes = vec![FrameSizeInfo {
                size: FrameSize::Discrete {
                    width: 160,
                    height: 120,
                },
                intervals: vec![FrameInterval::Discrete {
                    numerator: 1,
                    denominator: 30,
                }],
            }];
        }
    }
    profile
}

/// The widest control set this project has captured, with its modes rewritten small.
///
/// The **layout** fixture (design D20), and the reason it is a third camera rather than a
/// replacement for the first: the workbench's claim is about a control column that does not
/// fit on a screen, and the synthetic profile's eighteen controls very nearly do. `vivid`'s
/// seventy-seven do not fit on any screen, which is the case the two-pane shell is sized
/// against — *"the 77-control vivid case is the sizing fixture, not the 18-control common
/// case"*, in D20's own words.
///
/// It is the committed corpus document rather than a construction, so the widget mix a
/// browser has to render — the class headers, the bitmasks, the RECT payload, the menus with
/// holes in them — is a real driver's rather than one this file thought of. Its formats are
/// cut down to one small mode for `small_mjpeg_camera`'s reason: a browser decodes every
/// preview frame, and 4K would make this rung slow for nothing.
fn wide_camera() -> schema::profile::DeviceProfile {
    let mut profile = testkit::corpus::load("vivid").expect("the vivid profile is committed");
    let mut kept = false;
    profile.invariant.formats.retain_mut(|format| {
        if kept {
            return false;
        }
        kept = true;
        format.sizes = vec![FrameSizeInfo {
            size: FrameSize::Discrete {
                width: 160,
                height: 120,
            },
            intervals: vec![FrameInterval::Discrete {
                numerator: 1,
                denominator: 30,
            }],
        }];
        true
    });
    assert!(kept, "the vivid profile enumerated no formats at all");
    profile
}

/// The brightness the second camera reports, so that two panels can be told apart.
///
/// The fixture's own brightness is 128 and this is not it. That difference is the whole
/// mechanism behind [`SECOND_CAMERA_CARD`]'s paragraph: a claim that switches cameras and then
/// reads one number off the panel is asking *which device this panel was painted from*, which
/// is the question M32 is about. Matching numbers would have made the wrong answer look like
/// the right one.
const SECOND_CAMERA_BRIGHTNESS: i64 = 64;

/// What the second camera calls itself, which is what gives it an id of its own.
///
/// `fake::assign_ids` derives every camera's [`CameraId`] from its card, so this string is the
/// second camera's identity rather than its label — two profiles sharing a card would collapse
/// into one id and the switch claims would be switching to the camera they were already on.
/// `Serving::start` asserts they came out different rather than trusting that.
const SECOND_CAMERA_CARD: &str = "Synthetic Second Camera";

/// A second camera on this daemon: the same device, under another identity, at another
/// brightness.
///
/// **It is a second identity rather than a second device**, and that is the honest description
/// of what it buys. Nothing here claims the fake now replays two kinds of hardware — every
/// format, every control and every measured pair is `synthetic_basic`'s, so the four device
/// edges design §1.2 recorded are on both cameras and no claim about them cares which one it
/// ran against. What this profile adds is the one thing a one-camera daemon cannot offer: a
/// camera to switch *to*. Two of this rung's claims are about the switch itself — the preview
/// the page walked away from (docs/11 **H4**) and the panel that still belongs to the camera
/// the page walked away from (**M32**) — and neither is askable on a machine with one camera.
///
/// Its bus path, its bus info and its node paths differ too, because a fixture that is a
/// second camera only in its card would be a fixture that D1's grouping is entitled to fold
/// back into one \[PF:13\]: `bus_info` is per-USB-device, and two logical cameras sharing one
/// are one camera with two nodes.
fn second_mjpeg_camera() -> schema::profile::DeviceProfile {
    let mut profile = small_mjpeg_camera();
    let info = &mut profile.invariant.info;
    info.id = CameraId::parse("cam:synthetic-second").expect("a literal, non-empty id");
    info.card = SECOND_CAMERA_CARD.to_owned();
    info.fingerprint.card = SECOND_CAMERA_CARD.to_owned();
    info.fingerprint.bus_path = "3-5:1.0".to_owned();
    info.bus_info = "usb-0000:00:14.0-5".to_owned();
    for (index, node) in info.nodes.iter_mut().enumerate() {
        node.path = format!("/dev/video{}", index + 10).into();
    }
    let brightness = schema::control::ControlSlug::parse("brightness").expect("a literal slug");
    profile.state.values.insert(
        brightness,
        schema::control::ControlValue::Int(SECOND_CAMERA_BRIGHTNESS),
    );
    profile
}

// ------------------------------------------------------------------------------- the rung

#[tokio::test(flavor = "multi_thread")]
async fn the_shipped_client_renders_and_refuses_in_a_real_chromium() {
    let claims = claims();
    // Before anything is decided about this host: the manifest is what both outcomes below are
    // counted in, so a manifest that has fallen through its own floor makes the decline *and*
    // the green line report a smaller rung as if it were the whole one.
    let shortfall = below_the_floor(&claims, &FLOOR);
    assert!(
        shortfall.is_empty(),
        "tests/browser/claims.json is below the floor this rung carries: {shortfall:?}"
    );
    let suite = suite_dir();
    let build = pinned_chromium_build(&suite);
    let node =
        Utf8PathBuf::from(std::env::var("WCH_E2E_NODE").unwrap_or_else(|_| "node".to_owned()));

    let ready = match preconditions(&node, &suite, &browsers_root(), &build) {
        Ok(ready) => ready,
        Err(declined) => {
            print!("{}", decline_report(&declined, &claims));
            return;
        }
    };

    // Everything the browser writes lands here: a trace holds DOM snapshots and a screenshot
    // holds pixels, and neither belongs in a repository whose whole privacy rule is that
    // frames do not enter it. P5d put this at `scratch/r1-web/`, an in-tree directory
    // gitignored by a `/scratch/` line; the 2026-08-12 ruling supersedes both (note **N84**),
    // and the new home is better on the point that mattered — `target/` is gitignored *and*
    // pruned by `gate_find`, so these artifacts are invisible to the privacy walk instead of
    // merely uncommittable, which is the collision E15 recorded. It is named rather than
    // `mktemp`'d and it is not swept: a trace exists to be opened after a failing run.
    let output = schema::paths::scratch_root()
        .expect("a scratch root")
        .join("r1-web");
    std::fs::create_dir_all(&output).expect("a scratch directory for this rung's artifacts");
    let report_path = output.join("run-claims.json");
    let _ = std::fs::remove_file(&report_path);

    let daemon = Serving::start().await;
    let url = daemon.serving.ready_to_open_url();
    // Every `.js` this build embeds, counted from the crate's own table rather than from a
    // number written down twice — the suite asserts the browser fetched exactly this many, so a
    // module added to the client and imported by nothing is red there.
    let modules = web::paths().filter(|name| name.ends_with(".js")).count();

    let camera = daemon.camera.as_str().to_owned();
    let second_camera = daemon.second_camera.as_str().to_owned();
    let wide_camera = daemon.wide_camera.as_str().to_owned();
    let brightness = SECOND_CAMERA_BRIGHTNESS.to_string();
    // **The aperture through which a browser reads the daemon's own viewer count.** Nothing on
    // this listener reports how many readers a camera's feed has — `daemon::http` serves two
    // camera-bearing routes and neither of them is a status page, and adding one to answer a
    // test would be a route `CAMERA_BEARING_PATHS` has to grow for the suite's convenience. So
    // the count is read where the daemon already refuses on it: the viewer past
    // `limits::PREVIEW_MAX_VIEWERS_PER_CAMERA` meets `Error::Busy` and a `503`, so a probe that
    // opens exactly this many readers and is served all of them has established that the camera
    // had none left over (docs/11 **H4**, note **N152**). Passed in rather than written down in
    // JavaScript for `WCH_E2E_MODULE_COUNT`'s reason: a number typed twice is a claim that stops
    // being about the constant the daemon reads.
    let viewer_cap = schema::limits::PREVIEW_MAX_VIEWERS_PER_CAMERA.to_string();
    let take = daemon.take.clone();
    let workdir = suite.clone();
    let output_arg = output.clone();
    let run = tokio::task::spawn_blocking(move || {
        Command::new(ready.node.as_std_path())
            .arg(ready.cli.as_std_path())
            .arg("test")
            .current_dir(workdir.as_std_path())
            .env("WCH_E2E_URL", url)
            .env("WCH_E2E_CAMERA", camera)
            .env("WCH_E2E_SECOND_CAMERA", second_camera)
            .env("WCH_E2E_WIDE_CAMERA", wide_camera)
            .env("WCH_E2E_SECOND_BRIGHTNESS", brightness)
            .env("WCH_E2E_PREVIEW_VIEWER_CAP", viewer_cap)
            .env("WCH_E2E_MODULE_COUNT", modules.to_string())
            .env("WCH_E2E_TAKE", take.as_str())
            .env("WCH_E2E_OUTPUT", output_arg.as_str())
            .status()
    })
    .await
    .expect("the Playwright child was waited for");
    let status = run.expect("the Playwright child ran");

    daemon.stop().await;

    assert!(
        status.success(),
        "the browser rung failed ({status}); its trace is under {output} — open it with \
         `npx playwright show-trace`"
    );
    verify_report(&report_path, &claims);
    println!(
        "r1-web: {count} browser claim(s) and {assertions} assertion(s) ran in Chromium {build}",
        count = claims.len(),
        assertions = claims.iter().map(|claim| claim.assertions).sum::<u64>(),
    );
}

/// Compare what the runner says it did against what this repository says the rung is.
///
/// The count in the decline above is only worth printing if something proves it, and this is
/// the something: `tests/browser/claims-reporter.mjs` counts one entry per test and one
/// assertion per top-level `expect`, from inside the runner, so the manifest is checked against
/// what happened rather than against a promise. Both directions of *drift* go red — a claim
/// added to a spec and not to `claims.json` is an unexpected title, and a claim that quietly
/// stopped asserting half of what it did is a count that no longer matches.
///
/// **What it cannot see is the two of them agreeing.** This is a consistency check, and a claim
/// deleted from the spec and from the manifest together is perfectly consistent — the title sets
/// match, every count matches, and the rung shrinks with nothing to notice. That is [`FLOOR`]'s
/// half, checked before either outcome is reported rather than here, because it is a claim about
/// the repository and this function is a claim about a run.
fn verify_report(path: &Utf8Path, claims: &[Claim]) {
    let report = std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("the browser rung wrote no report at {path}: {err}"));
    let report: Value = serde_json::from_str(&report).expect("the claim reporter writes JSON");
    let ran = report["claims"]
        .as_array()
        .expect("the claim reporter writes a claims array");

    let expected: BTreeSet<&str> = claims.iter().map(|claim| claim.title.as_str()).collect();
    let actual: BTreeSet<&str> = ran
        .iter()
        .filter_map(|claim| claim["title"].as_str())
        .collect();
    assert_eq!(
        actual, expected,
        "the suite and tests/browser/claims.json disagree about which claims exist; the \
         manifest is what the skip line counts, so a claim missing from it is a claim a \
         declining host would never know it had not made"
    );

    for claim in claims {
        let seen = ran
            .iter()
            .find(|candidate| candidate["title"].as_str() == Some(claim.title.as_str()))
            .expect("every expected title was just matched");
        assert_eq!(
            seen["status"],
            Value::from("passed"),
            "the runner reported `{title}` as {status}, and the exit status did not say so",
            title = claim.title,
            status = seen["status"],
        );
        assert_eq!(
            seen["assertions"].as_u64(),
            Some(claim.assertions),
            "`{title}` made {seen} assertion(s) and tests/browser/claims.json says {said}; update \
             the manifest deliberately, because it is the number the skip line reports",
            title = claim.title,
            seen = seen["assertions"],
            said = claim.assertions,
        );
    }
}

// ------------------------------------------------------------- the skip path, driven for real

/// What a host's node is, in the three answers this rung tells apart.
#[derive(Debug, Clone, Copy)]
enum NodeState {
    /// Nothing at that path at all.
    Absent,
    /// Something that runs and refuses — a broken installation, which is a different sentence
    /// from an absent one and gets a different remedy.
    Refusing,
    /// Something that answers `--version` with a zero exit.
    Working,
}

/// The three things [`preconditions`] asks about, as values a test can set.
#[derive(Debug, Clone, Copy)]
struct Host {
    node: NodeState,
    /// Whether `node_modules/@playwright/test/cli.js` is under the suite directory.
    playwright: bool,
    /// Which of Playwright's two browser-directory spellings is present, if either.
    browser: Option<&'static str>,
}

/// Where a fabricated host is built. One directory per arm, wiped before it is used.
///
/// Under the one scratch root (note **N84**): `target/` is gitignored, declared regenerable by
/// `CACHEDIR.TAG`, and pruned by every gate that walks the tree — the same three reasons the
/// rung's traces go there. Named per arm rather than `mktemp`'d so a failing run leaves
/// something a person can look at, and wiped on the way in so a leftover from an earlier run
/// cannot answer a question this run meant to ask.
fn fabricate(host: Host, arm: &str, build: &str) -> (Utf8PathBuf, Utf8PathBuf, Utf8PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    let root = schema::paths::scratch_root()
        .expect("a scratch root")
        .join("r1-web/preconditions")
        .join(arm);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("a scratch directory for this arm's fabricated host");

    let node = root.join("node");
    match host.node {
        NodeState::Absent => {}
        NodeState::Refusing | NodeState::Working => {
            // A shell script, because what the precondition asks is not "is this node" but "does
            // it run and does it answer" — it spawns the path and reads the status, so a two-line
            // program answers the question exactly as a 100 MB one would.
            let exit = match host.node {
                NodeState::Refusing => 1,
                _ => 0,
            };
            std::fs::write(&node, format!("#!/bin/sh\nexit {exit}\n")).expect("a stub node");
            std::fs::set_permissions(node.as_std_path(), std::fs::Permissions::from_mode(0o755))
                .expect("a stub node that can be run");
        }
    }

    let suite = root.join("suite");
    if host.playwright {
        let cli = suite.join("node_modules/@playwright/test/cli.js");
        std::fs::create_dir_all(cli.parent().expect("a file has a directory"))
            .expect("a stub Playwright tree");
        std::fs::write(&cli, "// fabricated by the r1-web precondition arms\n")
            .expect("a stub Playwright CLI");
    } else {
        std::fs::create_dir_all(&suite).expect("a suite directory with no Playwright in it");
    }

    let browsers = root.join("ms-playwright");
    std::fs::create_dir_all(&browsers).expect("a browsers root");
    if let Some(spelling) = host.browser {
        std::fs::create_dir_all(browsers.join(format!("{spelling}-{build}")))
            .expect("a stub browser directory");
    }

    (node, suite, browsers)
}

#[test]
fn a_declined_rung_names_what_was_missing_and_counts_what_it_did_not_run() {
    // Rubric rule 2, applied to the *reporting*: a skip is only worth anything if it can be
    // shown to happen and to say something. Each arm below arranges exactly one missing
    // precondition and asserts the decline names that one — because a rung that answered
    // "install node" to a host whose node is fine is a rung whose skips stop being read, which
    // is how a skip becomes a pass in a costume.
    //
    // ## The host is a value, and it cost a red `just ci` to learn that
    //
    // These arms used to take the host they were running on: this machine's `node`, the suite
    // directory beside this file, and a browsers root that does not exist. Two of the three sat
    // inside `if node --version succeeds`, and the arrangement could drive none of its questions
    // reliably, because what it was really asking was what this machine happens to have
    // installed.
    //
    //   - On a host with node and **no `crates/daemon/tests/browser/node_modules/`** — the
    //     ordinary state of a fresh clone, since that directory is gitignored and `just
    //     rung-web-install` is what puts it back — the browser arm's precondition returned at the
    //     *Playwright* question, and the assertion demanding the words "Chromium build 1234"
    //     panicked. So `just ci` and `just gate-g5` were red on precisely the host state the
    //     decline exists for, and `scripts/rung-web.sh` read the non-zero exit as FAIL rather
    //     than as the SKIPPED it was.
    //   - On a host with no node at all, two of the three arms did not run and nothing counted or
    //     named them — a silent skip inside the test whose subject is skips not being silent.
    //
    // So the host became a value: each arm builds a directory holding exactly what its question
    // is about, every question is drivable on every machine, and the `if` is gone. **What a
    // fabricated host cannot say** is stated rather than left implied — that a real `npm ci`
    // tree really puts the CLI at that path, and that Playwright really names its browser
    // directories `chromium-<build>` and `chromium_headless_shell-<build>`. Those are facts
    // about somebody else's tool, and what establishes them is the rung above on a host that has
    // them, where these same preconditions answer `Ok` and a browser starts.
    let claims = claims();
    assert!(
        !claims.is_empty(),
        "a rung with no claims cannot decline any"
    );

    let suite = suite_dir();
    let build = pinned_chromium_build(&suite);

    // The four declines, each one thing short of the arm below it, and the two accepts that stop
    // the four from being satisfiable by a `preconditions` that has quietly stopped ever saying
    // yes — which would skip this rung on every host in the world with every test still green.
    let declines = [
        (
            "no-node",
            Host {
                node: NodeState::Absent,
                playwright: false,
                browser: None,
            },
            "no usable node".to_owned(),
        ),
        (
            "node-refuses",
            Host {
                node: NodeState::Refusing,
                playwright: false,
                browser: None,
            },
            "--version` failed".to_owned(),
        ),
        (
            "no-playwright",
            Host {
                node: NodeState::Working,
                playwright: false,
                browser: None,
            },
            "pinned Playwright is not installed".to_owned(),
        ),
        (
            "no-browser",
            Host {
                node: NodeState::Working,
                playwright: true,
                browser: None,
            },
            format!("Chromium build {build}"),
        ),
    ];

    let mut arms = 0;
    for (arm, host, must_name) in &declines {
        let (node, suite, browsers) = fabricate(*host, arm, &build);
        let declined = preconditions(&node, &suite, &browsers, &build)
            .expect_err("this host is one precondition short");
        assert!(
            declined.missing.contains(must_name.as_str()),
            "the `{arm}` arm declined without naming what it took away ({must_name}): \
             {declined:?}"
        );
        assert!(
            !declined.remedy.is_empty(),
            "the `{arm}` arm's decline names no remedy, and a skip an operator cannot act on is \
             a skip that gets read as a pass: {declined:?}"
        );
        arms += 1;
    }

    // Both of Playwright's spellings, because either one is enough to launch and the rung says
    // so in `preconditions`. A build that lost the headless-shell spelling would decline on
    // every host that fetched only that one, which is the ordinary result of `install chromium`
    // with `headless` set — an absent rung, dressed as a named skip.
    for spelling in ["chromium", "chromium_headless_shell"] {
        let host = Host {
            node: NodeState::Working,
            playwright: true,
            browser: Some(spelling),
        };
        let (node, suite, browsers) = fabricate(host, spelling, &build);
        let ready = preconditions(&node, &suite, &browsers, &build).unwrap_or_else(|declined| {
            panic!("a host with all three preconditions was declined ({spelling}): {declined:?}")
        });
        assert_eq!(ready.node, node, "the rung would run some other node");
        assert_eq!(
            ready.cli,
            suite.join("node_modules/@playwright/test/cli.js"),
            "the rung would hand some other file to node"
        );
        arms += 1;
    }

    // The counted half of AGENTS rule 3, about this test rather than about the rung: every arm
    // ran on every host, and the line says how many and which.
    println!(
        "r1-web: {arms} precondition arm(s) driven against a fabricated host — {declines} \
         decline(s) named ({named}) and 2 accept(s), one per browser-directory spelling",
        declines = declines.len(),
        named = declines
            .iter()
            .map(|(arm, _, _)| *arm)
            .collect::<Vec<_>>()
            .join(", "),
    );

    // ---------------------------------------------------------------- the floor, in both
    // directions
    //
    // `verify_report` is a consistency check and says so; this is the half that is a floor. Both
    // directions are driven here rather than asserted about, because a floor whose failing
    // direction has never been seen is a number nobody has checked (AGENTS rule 2).
    assert!(
        below_the_floor(&claims, &FLOOR).is_empty(),
        "the committed manifest is below its own floor: {:?}",
        below_the_floor(&claims, &FLOOR)
    );
    let one_short = &claims[..claims.len() - 1];
    assert!(
        !below_the_floor(one_short, &FLOOR).is_empty(),
        "a manifest one claim short of the floor was accepted; deleting a claim from the spec \
         and from claims.json together is green in every other check in this file"
    );
    let silent = vec![Claim {
        title: "seeded by this test".to_owned(),
        what: "a claim that asserts nothing".to_owned(),
        assertions: 0,
    }];
    assert!(
        below_the_floor(&silent, &FLOOR)
            .iter()
            .any(|line| line.contains("zero assertions")),
        "a claim asserting nothing was accepted"
    );
    if let Some(surplus) = above_the_floor(&claims, &FLOOR) {
        println!("{surplus}");
    }

    // And the counted half: the decline says how much was not done, and names each piece of it.
    let report = decline_report(
        &Declined {
            missing: "nothing, in this test".to_owned(),
            remedy: "nothing, in this test".to_owned(),
        },
        &claims,
    );
    let assertions: u64 = claims.iter().map(|claim| claim.assertions).sum();
    assert!(
        report.contains(&format!(
            "{count} browser claim(s) and {assertions} assertion(s) were not run",
            count = claims.len()
        )),
        "{report}"
    );
    for claim in &claims {
        assert!(report.contains(&claim.title), "{report}");
        assert!(report.contains(&claim.what), "{report}");
    }
    // Exactly one line begins with `SKIP`, so counting declines and counting the claims they
    // name are two different numbers rather than one confused one.
    assert_eq!(
        report
            .lines()
            .filter(|line| line.starts_with("SKIP"))
            .count(),
        1,
        "{report}"
    );
}

// ------------------------------------------------------- node is never a build dependency

#[test]
fn node_is_never_a_build_dependency() {
    // Design §3.1 and §2.8 both say it, and it is the sort of claim that stays true right up
    // until somebody adds a `build.rs` that shells out to a bundler. What this test can say is
    // therefore about *members*: no crate in this workspace declares a build script or a
    // `[build-dependencies]` table, so `cargo build` runs no code of **ours** before this claim
    // could be checked, and there is nothing of ours that could reach for node.
    //
    // Its honest limit is one level down the graph, and the limit is not hypothetical: 47
    // packages in the resolved graph do run build scripts, `v4l2-sys-mit`'s among them, which
    // is the `bindgen` run over the kernel UAPI headers that `## Building` installs libclang
    // for \[PF:10\]. So "this workspace runs no build script at all" would be false, and a
    // pinned allowlist of the 47 would churn on every dependency bump while proving nothing
    // about node in particular. So a *dependency* whose build script reached for node is a gap
    // this test does not cover, and nothing else covers it either: `.github/workflows/ci.yml`
    // installs no node, but GitHub's Ubuntu runner image ships one anyway for its own actions,
    // so a green CI run is not the witness it looks like. The gap is named here rather than
    // papered over, because the alternative — widening the walk to the whole graph — buys a
    // list that goes red for the wrong reason every time `serde` bumps.
    //
    // The population is derived rather than listed: every `Cargo.toml` under the workspace,
    // found by walking it, with the count asserted non-zero so a walk that found nothing cannot
    // pass by checking nothing.
    let root = workspace_root();
    let mut manifests = Vec::new();
    walk_manifests(&root, &mut manifests);
    assert!(
        manifests.len() > 5,
        "only {count} manifest(s) found under {root}; the walk is not seeing the workspace",
        count = manifests.len()
    );

    for manifest in &manifests {
        let text = std::fs::read_to_string(manifest).expect("a manifest this walk just found");
        assert!(
            !text.contains("[build-dependencies]"),
            "{manifest} declares build-dependencies; `cargo build` would run code before this \
             claim could be checked"
        );
        assert!(
            !text
                .lines()
                .any(|line| line.trim_start().starts_with("build =")),
            "{manifest} names a build script"
        );
        let beside = manifest
            .parent()
            .expect("a manifest has a directory")
            .join("build.rs");
        assert!(
            !beside.exists(),
            "{beside} exists; a build script is the one place node could become a build \
             dependency without anybody deciding to make it one"
        );
        for tool in ["node_modules", "npm ", "playwright"] {
            assert!(
                !text.contains(tool),
                "{manifest} names `{tool}`; the browser rung is a test-time tool and a manifest \
                 is the wrong place for it"
            );
        }
    }
}

/// Every workspace `Cargo.toml`, skipping the trees that are not ours to police.
fn walk_manifests(dir: &Utf8Path, into: &mut Vec<Utf8PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir.as_std_path()) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(path) = Utf8PathBuf::from_path_buf(entry.path()) else {
            continue;
        };
        let name = path.file_name().unwrap_or_default();
        if path.is_dir() {
            // `target/` is build output, `vendor/` is the manual workflow this tool replaces,
            // `node_modules/` is the rung's own tooling, and `.git` is neither.
            if matches!(name, "target" | "vendor" | "node_modules" | ".git") {
                continue;
            }
            walk_manifests(&path, into);
        } else if name == "Cargo.toml" {
            into.push(path);
        }
    }
}
