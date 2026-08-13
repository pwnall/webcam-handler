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
//! counts_what_it_did_not_run`] drives all three arms with the preconditions pointed at things
//! that are not there, which is the "prove it can go red" rule applied to the *reporting*
//! rather than to the assertions.
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
//! Design §2.8 puts it where ffprobe and mpv already are: **test-time external tooling, outside
//! the shipped licence inventory** — "node is a test-host convenience, never a build
//! dependency". The distinction that matters is the product's: `crates/web/assets/` vendors or
//! hand-writes everything it loads, and `scripts/gates/no-external-fetch-in-web.sh` is what
//! keeps that true. Nothing in `tests/browser/` is served to a browser by `wchd`; it is the
//! thing that drives one.

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
            FakeBackend::new(vec![small_mjpeg_camera()])
                .expect("the synthetic profile is this build's version"),
        );
        let camera = backend
            .enumerate()
            .expect("the fake enumerates what it replays")
            .first()
            .expect("one camera")
            .id
            .clone();

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
            shutdown.clone(),
        )
        .await
        .expect("loopback on an ephemeral port");

        Serving {
            serving,
            camera,
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

// ------------------------------------------------------------------------------- the rung

#[tokio::test(flavor = "multi_thread")]
async fn the_shipped_client_renders_and_refuses_in_a_real_chromium() {
    let claims = claims();
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
    let workdir = suite.clone();
    let output_arg = output.clone();
    let run = tokio::task::spawn_blocking(move || {
        Command::new(ready.node.as_std_path())
            .arg(ready.cli.as_std_path())
            .arg("test")
            .current_dir(workdir.as_std_path())
            .env("WCH_E2E_URL", url)
            .env("WCH_E2E_CAMERA", camera)
            .env("WCH_E2E_MODULE_COUNT", modules.to_string())
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
/// what happened rather than against a promise. Both directions go red — a claim added to a
/// spec and not to `claims.json` is an unexpected title, and a claim that quietly stopped
/// asserting half of what it did is a count that no longer matches.
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

#[test]
fn a_declined_rung_names_what_was_missing_and_counts_what_it_did_not_run() {
    // Rubric rule 2, applied to the *reporting*: a skip is only worth anything if it can be
    // shown to happen and to say something. Each arm below removes exactly one precondition and
    // asserts the decline names that one — because a rung that answered "install node" to a host
    // whose node is fine is a rung whose skips stop being read, which is how a skip becomes a
    // pass in a costume.
    let claims = claims();
    assert!(
        !claims.is_empty(),
        "a rung with no claims cannot decline any"
    );

    let suite = suite_dir();
    let build = pinned_chromium_build(&suite);
    let nowhere = Utf8PathBuf::from("/nonexistent/webcam-handler/r1-web");

    let no_node = preconditions(&nowhere.join("node"), &suite, &nowhere, &build)
        .expect_err("there is no node at a path that does not exist");
    assert!(no_node.missing.contains("no usable node"), "{no_node:?}");

    // A real node, and nothing else. `WCH_E2E_NODE` is the same seam the rung itself uses, so
    // this arm is exercised with whatever node this host has — or, on a host with none, it is
    // the arm above that fires and this one that is unreachable, which is the honest shape.
    let node =
        Utf8PathBuf::from(std::env::var("WCH_E2E_NODE").unwrap_or_else(|_| "node".to_owned()));
    if Command::new(node.as_std_path())
        .arg("--version")
        .output()
        .is_ok_and(|version| version.status.success())
    {
        let no_playwright = preconditions(&node, &nowhere, &nowhere, &build)
            .expect_err("no Playwright is installed under a path that does not exist");
        assert!(
            no_playwright
                .missing
                .contains("pinned Playwright is not installed"),
            "{no_playwright:?}"
        );
        assert!(no_playwright.remedy.contains("just rung-web-install"));

        let no_browser = preconditions(&node, &suite, &nowhere, &build);
        match no_browser {
            // The ordinary answer on a host that got this far: Playwright is installed and the
            // browser directory is the thing this arm took away.
            Err(declined) => assert!(
                declined
                    .missing
                    .contains(&format!("Chromium build {build}")),
                "{declined:?}"
            ),
            // A host with node but no `node_modules` never reaches the browser question, and
            // saying so is better than asserting a decline this arm cannot cause.
            Ok(_) => panic!("a browser was found under a path that does not exist"),
        }
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
        assertions > 0,
        "a claim that asserts nothing is not a claim"
    );
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
    // until somebody adds a `build.rs` that shells out to a bundler. **This workspace runs no
    // build script at all** — the V4L2 bindings are committed bindgen output rather than
    // generated at build time — so the strongest available statement is also the simplest one:
    // there is nothing for `cargo build` to run, in any member, so there is nothing that could
    // reach for node.
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
