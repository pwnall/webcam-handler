// What every claim in this rung needs to know, in one place.
//
// The daemon is not started here and is not startable from here: `crates/daemon/tests/
// web_browser.rs` owns it, because the thing being asserted is *the shipped composition* —
// `daemon::http::open` over the fake backend, with the token it minted and printed — and a
// suite that could bring its own daemon up would be a suite that could quietly assert
// against a different one. So everything below is read out of the environment the harness
// set, and a missing variable is a loud failure rather than a default: a default base URL is
// how a browser rung ends up green against nothing.

import { createRequire } from "node:module";

import { expect } from "@playwright/test";

const require = createRequire(import.meta.url);

/** The pins, read from the one file that holds them. */
export const pins = require("./package.json");

/** One required environment variable, or a failure naming it. */
function required(name) {
  const value = process.env[name];
  if (value === undefined || value === "") {
    throw new Error(`${name} is unset; crates/daemon/tests/web_browser.rs is what sets it`);
  }
  return value;
}

/** The URL `webcam-handler-daemon` printed — the one an operator opens, credential and all. */
export const readyToOpenUrl = required("WCH_E2E_URL");

/** The camera the fake replays. */
export const cameraId = required("WCH_E2E_CAMERA");

/**
 * An absolute path a claim may record into.
 *
 * `wch_record_start` takes a server path and refuses a relative one (note **N110**), and the
 * only process that knows a writable throw-away directory is the Rust harness — so the path
 * arrives the same way the URL does rather than being guessed at here.
 */
export const takePath = required("WCH_E2E_TAKE");

/**
 * How many ES modules this build embeds.
 *
 * Passed in from `web::paths()` rather than counted here, so that a module added to the
 * client and never imported by it is red in the anonymous-load claim instead of being a file
 * the browser never fetched and nobody missed.
 */
export const moduleCount = Number(required("WCH_E2E_MODULE_COUNT"));

/**
 * The bearer token, taken out of the URL the daemon printed rather than passed beside it.
 *
 * `crates/daemon/tests/http.rs` states the reason and it survives the trip into JavaScript: a
 * build that published one secret and gated on another would satisfy every assertion in this
 * suite if the token arrived by any other route than the line an operator copies.
 */
export const token = new URL(readyToOpenUrl).searchParams.get("token");

/** This daemon's origin, with no credential in it. */
export const origin = new URL(readyToOpenUrl).origin;

/**
 * One control's card in the panel.
 *
 * Matched on the **slug**, which is this project's own identifier (`schema::control::
 * ControlSlug`) and not the device's `name` — `app.js` makes the same distinction for camera
 * buttons and says why: a device string may contain anything at all, so a locator that
 * matched rendered device text would be a test keyed on data the device chooses. The `· `
 * suffix anchors it, so `focus_absolute` cannot match `focus_automatic_continuous`.
 */
export function control(page, slug) {
  return page
    .locator(".control")
    .filter({ has: page.locator(".slug", { hasText: new RegExp(`^${slug} · `) }) });
}

/**
 * Open the page the way an operator opens it, and wait until the panel has been painted from
 * a live `wch_controls`.
 *
 * Waiting on the *status lines* rather than on a control element is deliberate: `app.js`
 * writes them after each call returns, so a page that rendered a half-built panel cannot
 * satisfy them, and a page whose socket never opened writes a failure into the first one
 * instead of leaving this hanging on an element that will never appear.
 */
export async function openClient(page) {
  await page.goto(readyToOpenUrl);
  await expect(page.locator("#connection")).toHaveText("connected");
  await expect(page.locator("#controls-status")).toHaveText(/^\d+ controls, \d+ automation pair/);
  return page;
}
