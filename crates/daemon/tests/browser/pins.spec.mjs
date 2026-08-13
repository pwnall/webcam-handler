// The pin, asserted rather than declared.
//
// docs/7 P5d asks for "versions pinned", and design §3.1 for "browser and package versions
// are pinned". A `package.json` saying `"1.62.1"` and a `package-lock.json` beside it are how
// that is *arranged*; this file is how it is *checked*, and the two are different claims. The
// failure it exists for is the one nobody notices: a rung whose browser drifted keeps passing
// until the day it fails for a reason that has nothing to do with this repository, and by then
// the question "what did it drive last week" has no answer.
//
// So three things are compared, and they are three because a weaker pin can satisfy two of
// them. The *package* that ran must be the exact version named — exact, with no range operator
// in it at all, because `^1.62.1` is a pin that silently becomes 1.63 on the next install. The
// *build* that package declares must be the one written down. And the binary that actually
// launched must be that build, and must report the Chrome version written down — which is the
// only one of the three that a lockfile cannot get wrong on your behalf, because it is a
// property of the file on disk rather than of a manifest describing it.

import { expect, test, chromium } from "@playwright/test";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";

import { pins } from "./harness.mjs";

const require = createRequire(import.meta.url);

/**
 * What `playwright-core` says its browsers are.
 *
 * Read as a file rather than `require`d: the package's `exports` map does not list
 * `browsers.json`, so Node refuses the subpath. Reading the file is also the more honest
 * question — it asks what is installed under this suite's own `node_modules`, which is the
 * tree `package-lock.json` pins, rather than whatever a resolver would find.
 */
const browsers = JSON.parse(
  readFileSync(new URL("./node_modules/playwright-core/browsers.json", import.meta.url), "utf8"),
).browsers;

test("the browser this rung drove is the pinned one", async ({ browser, browserName }) => {
  // One project, and it is Chromium: the Chrome-only owner ruling (design §2.7) and the
  // harness agree by construction rather than by convention, and this is where that stops
  // being a comment in the config.
  expect(browserName).toBe("chromium");

  const pinned = pins.devDependencies["@playwright/test"];
  // A pin with a range operator is not a pin. This is the assertion that would have caught a
  // well-meaning `npm install --save @playwright/test`, which writes `^1.62.1`.
  expect(pinned).toMatch(/^\d+\.\d+\.\d+$/);
  expect(require("@playwright/test/package.json").version).toBe(pinned);

  // What the package says the browser is…
  const chromiumBuilds = browsers
    .filter((entry) => entry.name === "chromium" || entry.name === "chromium-headless-shell")
    .map((entry) => entry.revision);
  expect(chromiumBuilds).toEqual([pins.wch.chromiumBuild, pins.wch.chromiumBuild]);

  // …and what actually launched. `executablePath()` is the file on disk, so a cache holding
  // some other build under the expected name cannot satisfy both this and the version below.
  expect(chromium.executablePath()).toContain(`-${pins.wch.chromiumBuild}`);
  expect(browser.version()).toBe(pins.wch.chromiumVersion);
});
