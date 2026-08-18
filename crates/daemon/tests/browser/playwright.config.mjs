// R1-web: the pinned Playwright + Chromium rung (design 6 §3.1, docs/7 P5d).
//
// Everything about this file that looks like a preference is a decision the rung's verdict
// depends on. Four of them:
//
// **Chromium, and never a channel.** `devices["Desktop Chrome"]` — the spelling most
// Playwright configs open with — sets `channel: "chrome"`, which launches whatever Google
// Chrome the *host* happens to have installed. That would make this rung's verdict a
// property of somebody else's apt upgrade rather than of this repository, which is exactly
// the failure the pin exists to prevent. `browserName: "chromium"` launches the build
// `package.json` pins, and `pins.spec.mjs` asserts at run time that the build which launched
// is the one named there. The Chrome-only owner ruling (design §2.7) and this file agree by
// construction: there is one project and it is Chromium.
//
// **Nothing this run writes may land in the repository.** A frame may contain a person
// (AGENTS, "Hardware and privacy"), and although this rung drives the *fake* backend — so
// every pixel it can capture is synthetic — a trace, a screenshot and a report are still
// files somebody has to not commit. `WCH_E2E_OUTPUT` is required rather than defaulted: a
// default would be a path inside this directory, and a rung that can quietly write into
// `crates/` is one bad day from a screenshot in a commit. The Rust harness points it at a
// gitignored scratch directory and nothing here may invent one.
//
// **Traces on failure, not on success.** A green run leaves nothing; a red one leaves the
// whole trace — DOM snapshots, network, console — which is the difference between "the
// browser half broke" and "the browser half broke and here is why", on a rung whose subject
// is a thing no Rust test can see.
//
// **One worker, no parallelism, no retries.** The suite shares one daemon and one camera:
// V4L2 allows one streamer per node and this daemon models that faithfully even over the
// fake, so two tests previewing at once is a real arrangement with a real answer rather than
// a hermetic one. Retries would turn a flake into a pass, and a rung that hides its flakes
// reports a verdict it did not reach.

import { defineConfig } from "@playwright/test";

/** Where every artifact of this run goes. Required — see the header. */
const output = process.env.WCH_E2E_OUTPUT;
if (output === undefined || output === "") {
  throw new Error(
    "WCH_E2E_OUTPUT is unset: this suite writes traces, screenshots and its report, and the " +
      "harness that launches it is the one that decides where — never this file, and never " +
      "anywhere inside the repository.",
  );
}

/** The daemon this run is about, as the URL `webcam-handler-daemon` printed, token and all. */
const url = process.env.WCH_E2E_URL;
if (url === undefined || url === "") {
  throw new Error("WCH_E2E_URL is unset: there is no daemon for this suite to be about.");
}

export default defineConfig({
  testDir: ".",
  outputDir: `${output}/artifacts`,
  fullyParallel: false,
  workers: 1,
  retries: 0,
  // A `test.only` left behind is a suite that silently stopped making most of its claims,
  // which is the "skip that reads as pass" defect one layer in.
  forbidOnly: true,
  // Bounded, and it is not synchronisation: nothing here learns anything by waiting. Every
  // wait in the suite is Playwright's auto-waiting on a condition the daemon or the browser
  // causes, and this number exists so that a condition which never happens is a named
  // failure rather than a rung that never finishes.
  timeout: 30_000,
  expect: { timeout: 10_000 },
  // `list` for a person, and the claim reporter for the harness — which needs the one thing
  // the built-in `json` reporter does not carry, an assertion count per test. See
  // `claims-reporter.mjs`.
  reporter: [["list"], ["./claims-reporter.mjs", { outputFile: `${output}/run-claims.json` }]],
  use: {
    baseURL: new URL(url).origin,
    browserName: "chromium",
    headless: true,
    // **Pinned, not defaulted** (design D20). The workbench's layout claim is a statement
    // about a viewport — the preview and the control being adjusted visible together at every
    // scroll position — and a claim measured against whatever Playwright's default happens to
    // be this release is a claim that moves when the tool does. This is Playwright's 1.62
    // default written down, so nothing about the existing claims changes and the layout one
    // has a number to be true of.
    viewport: { width: 1280, height: 720 },
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    video: "off",
  },
});
