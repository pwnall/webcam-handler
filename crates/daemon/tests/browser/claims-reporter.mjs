// What this run actually claimed, counted by the runner rather than by anybody's memory.
//
// `crates/daemon/tests/web_browser.rs` prints, when it declines, "N browser claim(s) and M
// assertion(s) were not run". Those two numbers live in `claims.json`, and a number in a file
// is worth exactly as much as the thing that checks it — so this reporter emits the same two
// numbers **as observed**, one entry per test with its count of `expect` calls, and the Rust
// harness compares the two on every green run. A claim added to a spec and not to the manifest
// is red; a claim that quietly stopped asserting half of what it did is red.
//
// It exists because Playwright's built-in `json` reporter omits `steps`, which is where the
// assertion count is: a reporter is the supported place to ask the runner what it did, and
// thirty lines here is cheaper than parsing a `blob` archive or — much worse — counting
// `expect(` in the source, which would be inferring the suite instead of counting it.
//
// **Only top-level `expect` steps are counted.** `expect.poll` and `expect.toPass` retry, and
// each retry is a child step under the one expectation the author wrote; counting those would
// make the number depend on how fast the machine was.

import { writeFileSync } from "node:fs";

export default class ClaimReporter {
  constructor(options = {}) {
    this.outputFile = options.outputFile;
    /** Test id → { title, status, assertions }. */
    this.claims = new Map();
  }

  onTestBegin(test) {
    this.claims.set(test.id, { title: test.title, status: "unknown", assertions: 0 });
  }

  onStepEnd(test, _result, step) {
    if (step.category !== "expect") {
      return;
    }
    for (let above = step.parent; above !== undefined; above = above.parent) {
      if (above.category === "expect") {
        return;
      }
    }
    const claim = this.claims.get(test.id);
    if (claim !== undefined) {
      claim.assertions += 1;
    }
  }

  onTestEnd(test, result) {
    const claim = this.claims.get(test.id);
    if (claim !== undefined) {
      claim.status = result.status;
    }
  }

  onEnd() {
    if (this.outputFile === undefined) {
      throw new Error("the claim reporter was given no outputFile; playwright.config.mjs sets it");
    }
    writeFileSync(
      this.outputFile,
      `${JSON.stringify({ claims: [...this.claims.values()] }, null, 2)}\n`,
    );
  }

  // Reported by the `list` reporter beside this one; printing them twice would make a run's
  // output argue with itself.
  printsToStdio() {
    return false;
  }
}
