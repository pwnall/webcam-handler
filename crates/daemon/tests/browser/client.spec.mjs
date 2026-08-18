// The browser half of the web client, asserted in a browser.
//
// P5c landed `crates/web/assets/` and deliberately claimed nothing about whether any of it
// renders: "a browser behavior verified only through the JSON the page consumes is not
// verified" (rubric B7, docs/7 P5c). `crates/daemon/tests/web_client.rs` is the suite that
// stops exactly where this one begins — it proves the DTO arrives with the holes, the
// INACTIVE flag, the out-of-range value and the unnameable type in it, over the real socket,
// from a real daemon. Everything below is the other half: that those facts reach a screen.
//
// ## What each claim is chosen for
//
// The panel's rendering rules are the code in this repository that is *least* checkable
// without a DOM, because every one of them is a device fact turning into an element:
//
// - a sparse menu becomes a `<select>` carrying **the device's own indices** \[PF:2\] — the
//   single most valuable assertion here, because `controls.js` sorts numerically over keys
//   that crossed JSON as strings and writes `Number(key)` rather than an option's position,
//   and a `<select>` is the only place either of those can be seen to be true;
// - an INACTIVE control stays **usable** with its badge \[PF:3\], because "INACTIVE" and
//   "unsupported" render identically to anyone who only reads the JSON, and the difference is
//   the whole of AGENTS rule 7;
// - an out-of-range current renders **without `min`/`max`** \[PF:4\], because the absence of
//   two attributes is not a thing any protocol test can see;
// - an unnameable control type shows its **raw discriminant** \[PF:1\], which is AGENTS rule
//   6 arriving at a person;
// - a clamp **snaps a slider back** \[PF:6\] and says both numbers, which is D3's "requested
//   is not applied" as an operator experiences it.
//
// Beside them, the three things only a browser does at all: an `<img>` *painting* successive
// parts of a `multipart/x-mixed-replace` response, a page that keeps painting across a photo
// (note **N83**), and a page that loads with no credential while the camera behind it does not
// (note **N82**, D11's amendment).
//
// P5e added two families, both of them about what this client says when it does *not* know
// something. **Two absences the panel used to fill in for the device** — a control whose
// `current` never arrived, and a bitmask field handed something that is not a number — which
// are asserted by handing `controls.js` a document rather than a daemon, for the reasons the
// block above them gives. And **three claims about a socket that has closed**, because every
// affordance on this page outlives its connection by design: the buttons are markup, the
// preview is a separate HTTP request, and the subscriptions are registrations in a map. Each of
// those had to be told, and the ones that were not told went on looking healthy.
//
// ## Nothing here sleeps, and nothing asserts a pixel
//
// Every wait is Playwright auto-waiting on a condition the daemon or the browser causes — an
// element's text, an attribute, an image whose mean luma has moved. The suite's only timeout
// is the config's, and it exists so a condition that never happens is a named failure instead
// of a rung that never finishes. Where a frame is examined at all it is examined for an
// *ordering* the fake declares (`fake::frames`: brightness is a monotonic gain), never for a
// value, which is the same rule the hardware suites run under.

import { expect, test } from "@playwright/test";

import {
  cameraId,
  control,
  fixtureCameras,
  wideCameraId,
  moduleCount,
  openClient,
  origin,
  previewViewerCap,
  readyToOpenUrl,
  secondBrightness,
  secondCameraId,
  takePath,
  token,
} from "./harness.mjs";

test("a sparse menu becomes a select carrying the device's own indices", async ({ page }) => {
  await openClient(page);

  // The Chicony's `Auto Exposure` has items at 1 and 3 and no item 2 \[PF:2\]. Three ways a
  // naive panel gets this wrong, and all three are ruled out below: a `<select>` built by
  // walking `0..n` would carry a `2`; one using an option's *position* as its value would
  // carry `0` and `1`; and one sorting the DTO's keys as the strings they arrive as would
  // still be right here by luck, which is why the *labels* are checked in order too — the
  // index is in every label precisely so the hole is visible to a reader.
  const select = control(page, "auto_exposure").locator("select");
  const options = select.locator("option");
  await expect(options).toHaveText(["1 · Manual Mode", "3 · Aperture Priority Mode"]);
  expect(await options.evaluateAll((nodes) => nodes.map((node) => node.value))).toEqual(["1", "3"]);

  // …and the device's current index is the one selected, rather than the first option a
  // `<select>` shows when nothing matches.
  await expect(select).toHaveValue("3");
});

test("an INACTIVE control is still usable and names what owns it", async ({ page }) => {
  await openClient(page);

  // \[PF:3\]: `exposure_time_absolute` is INACTIVE because `auto_exposure` owns it right now.
  // The widget must stay **enabled** — D3's guarded write is what releases it, and that is the
  // checkbox above the panel — because rendering it as disabled would be the
  // availability-as-capability collapse AGENTS rule 7 refuses, and would look identical to a
  // control the camera does not support.
  //
  // The provenance is `declared`, and that is right rather than a shortfall: the fixture
  // records this pair as *measured*, but `engine::pairing::in_effect` is handed no measured
  // pairs by a read verb — measuring pairs writes to the camera and is its own operation (note
  // N30) — so what the page shows is the UVC table's claim, honestly labelled as one. A page
  // that said `measured` here would be claiming a probe nobody ran.
  const card = control(page, "exposure_time_absolute");
  await expect(card.locator(".badge.owned")).toHaveText("inactive");
  await expect(card.locator(".note.owned")).toHaveText(
    "auto_exposure owns this right now; a guarded write switches it off by selecting the menu " +
      "item named like manual (D3, declared)",
  );
  await expect(card.locator("input[type=number]")).toBeEnabled();
  await expect(card.locator("input[type=range]")).toBeEnabled();

  // …and the other direction, without which "enabled" is a claim about a panel that never
  // disables anything. `privacy` is READ_ONLY on the seed hardware \[PF:12\] and AGENTS is
  // explicit that the hardware privacy control is honored and never worked around — so its
  // widget *is* disabled, and says which of the two reasons it is. The pair is one assertion
  // in two halves: availability is not capability, and this panel renders the difference.
  const readOnly = control(page, "privacy");
  await expect(readOnly.locator("input[type=checkbox]")).toBeDisabled();
  await expect(readOnly.locator(".note.surprising")).toHaveText(
    "the device reports this control read-only",
  );
});

test("a current outside its declared range renders with no min and no max", async ({ page }) => {
  await openClient(page);

  // \[PF:4\]: the OBSBOT reports `Zoom, Continuous` as 245 in a range of -100..=100. The number
  // field is the authority and carries **no bounds** — a field that refused 245 would be this
  // page enforcing a limit the driver does not — while the slider is parked at the closest
  // position an `<input type=range>` can hold, with the note saying both numbers. The absence
  // of the two attributes is the assertion no protocol test can make.
  const card = control(page, "zoom_continuous");
  const number = card.locator("input[type=number]");
  await expect(number).toHaveValue("245");
  expect(
    await number.evaluate((node) => node.hasAttribute("min") || node.hasAttribute("max")),
  ).toBe(false);
  await expect(card.locator("input[type=range]")).toHaveValue("100");
  await expect(card.locator(".note.surprising")).toHaveText(
    "the device reports 245, outside the range it declares (-100…100) — carried as the device " +
      "reported it [PF:4]",
  );
});

test("a control type this build cannot name shows its raw discriminant", async ({ page }) => {
  await openClient(page);

  // \[PF:1\] and AGENTS rule 6: the kernel emitted a control type `schema` does not name, and
  // the panel renders the discriminant an operator can look up plus the payload byte count —
  // rather than dropping the row, which would be indistinguishable from a camera that does not
  // have the control.
  const card = control(page, "region_of_interest_rectangle");
  await expect(card.locator(".slug")).toHaveText(
    "region_of_interest_rectangle · 0x00981ae1 · unknown 0x00000fff",
  );
  await expect(card.locator(".widget .mono")).toHaveText("16 bytes · 00 00 00 00 00 00 00 00 …");
  await expect(card.locator(".widget .note")).toHaveText(
    "a control type this build does not name (raw 0x00000fff) [PF:1] · 1 × 16 bytes",
  );
});

test("a clamp snaps the slider back and says both numbers", async ({ page }) => {
  await openClient(page);

  // \[PF:6\], D3, E4: the driver takes a write past the maximum, silently applies the maximum
  // and reports success. What an operator sees is a slider that moves back — and what makes
  // that honest rather than a widget twitching is the note carrying *both* numbers. The write
  // goes out with the guard as the page ships it (checked), so this is the shipped default
  // path and not a configuration invented for a test.
  const card = control(page, "brightness");
  const number = card.locator("input[type=number]");
  await expect(number).toHaveValue("128");

  await number.fill("5000");
  await number.blur();

  await expect(card.locator(".note.surprising")).toHaveText(
    "asked for 5000, the device holds 255 · clamped into 0…255 [PF:6]",
  );
  // The repaint is from the *device*, not from the answer (controls.js: a write can move
  // another control's properties outright), so these two are what the camera now holds.
  await expect(card.locator("input[type=range]")).toHaveValue("255");
  await expect(number).toHaveValue("255");
});

// ----------------------------------------------------------- two shapes the fixture has not
//
// **Where these documents come from, and why they are the test's rather than the daemon's.**
// Everything above drives the shipped composition end to end, and everything above is about a
// document `synthetic_basic` can produce. Two rendering rules are not: a control whose
// `current` the device never reported, and a `bitmask` control. The fixture has neither, no
// backend can be asked to invent one from here — `web_browser.rs` owns the daemon — and both
// are ordinary rather than exotic. `ControlDesc::current` is an `Option` because a `WRITE_ONLY`
// control's value is the device's to keep and an enumeration that did not fetch values has
// none, and **every profile in `corpus/` carries controls that arrive with no current**
// (`user_controls`, `camera_controls`); `region_of_interest_auto_ctrls` below is the Chicony's
// own `BITMASK` control, `corpus/profiles/chicony-rgb.json`, id and range and flags as
// captured.
//
// So these two claims hand the *shipped module* a document this project has measured the parts
// of, in a real Chromium, and assert what it renders. That is the "malformed fixture that must
// trip the validator" AGENTS asks tests to write, one language over: the module is the subject,
// the document is the input, and nothing about the daemon is being claimed by either.

/** The Chicony's `Auto Exposure` \[PF:2\], with the one field an enumeration may not fill. */
const MENU_WITH_NO_CURRENT = {
  id: 0x009a_0901,
  name: "Auto Exposure",
  slug: "auto_exposure",
  type: { kind: "menu" },
  range: { min: 0, max: 3, step: 1 },
  default: 3,
  flags: { raw: 4096, known: ["has_which_min_max"], unknown_bits: 0 },
  menu: { 1: { kind: "name", name: "Manual Mode" }, 3: { kind: "name", name: "Aperture Priority Mode" } },
  elems: 1,
  elem_size: 4,
};

/** `Power Line Frequency` as the fixture carries it \[PF:5\] — the same menu, read. */
const MENU_WITH_A_CURRENT = {
  id: 0x0098_0918,
  name: "Power Line Frequency",
  slug: "power_line_frequency",
  type: { kind: "menu" },
  range: { min: 0, max: 2, step: 1 },
  default: 3,
  flags: { raw: 0, known: [], unknown_bits: 0 },
  menu: { 0: { kind: "name", name: "Disabled" }, 1: { kind: "name", name: "50 Hz" }, 2: { kind: "name", name: "60 Hz" } },
  elems: 1,
  elem_size: 4,
  current: { kind: "int", value: 2 },
};

/** The Chicony's `Brightness`, an ordinary integer, with the one field an enumeration may not fill. */
const SCALAR_WITH_NO_CURRENT = {
  id: 0x0098_0900,
  name: "Brightness",
  slug: "brightness",
  type: { kind: "integer" },
  range: { min: -64, max: 64, step: 1 },
  default: 0,
  flags: { raw: 0, known: [], unknown_bits: 0 },
  menu: {},
  elems: 1,
  elem_size: 4,
};

/** The Chicony's `Region of Interest Auto Ctrls`, a real `BITMASK`, with no current. */
const BITMASK_WITH_NO_CURRENT = {
  id: 0x0098_1ae2,
  name: "Region of Interest Auto Ctrls",
  slug: "region_of_interest_auto_ctrls",
  type: { kind: "bitmask" },
  range: { min: 0, max: 1, step: 0 },
  default: 1,
  flags: { raw: 4096, known: ["has_which_min_max"], unknown_bits: 0 },
  menu: {},
  elems: 1,
  elem_size: 4,
};

/**
 * Paint one report with `controls.js` itself, beside the panel the daemon filled.
 *
 * The page is opened first and the module is `import`ed out of the same origin it already
 * loaded, so what runs is the file this daemon serves rather than a copy. `write` records what
 * it was asked for and answers **nothing until released**, which is what makes an in-flight
 * write observable at all; `refresh` does nothing, because the repaint-from-the-device it
 * stands for is asserted over the real socket by the clamp claim above and doing it twice here
 * would be this suite asserting its own stub.
 */
async function probePanel(page, controls) {
  await openClient(page);
  await page.evaluate(
    async (report) => {
      const module = await import("./controls.js");
      const panel = document.createElement("div");
      panel.id = "probe";
      panel.className = "panel";
      document.body.append(panel);
      window.wchWrites = [];
      module.paint(panel, report, {
        write: (control, value) => {
          window.wchWrites.push({ control, value });
          return new Promise((resolve) => {
            window.wchAnswer = () =>
              resolve({ writes: [{ slug: control, requested: value, applied: value }] });
          });
        },
        refresh: async () => {},
      });
    },
    { controls },
  );
}

/** One card in the probe panel, matched on its slug exactly as `harness.mjs` matches. */
function probed(page, slug) {
  return page
    .locator("#probe .control")
    .filter({ has: page.locator(".slug", { hasText: new RegExp(`^${slug} · `) }) });
}

test("a menu whose value the device did not report selects nothing", async ({ page }) => {
  // `controls.js`'s own header forbids this in as many words — "a `<select>` with no matching
  // option silently shows its first item, which would make this page report a value the camera
  // does not hold" — and the guard it wrote for it was `current !== null && !items.some(…)`, so
  // the case where there is no current at all fell straight through it and the browser selected
  // the lowest index on the device's behalf. `toggle`, five lines away, checks `=== null`
  // explicitly and says why: "a box drawn from a value nobody has is a UI asserting a device
  // state it was never told". A menu is the same claim with more numbers in it.
  await probePanel(page, [MENU_WITH_NO_CURRENT, MENU_WITH_A_CURRENT]);

  const unread = probed(page, "auto_exposure");
  const options = unread.locator("option");
  await expect(options).toHaveText(["— not read —", "1 · Manual Mode", "3 · Aperture Priority Mode"]);
  // Selected, so the browser's own "first item wins" never runs, and disabled, so the absence
  // cannot be written back to the device as though it were a value.
  await expect(unread.locator("select")).toHaveValue("");
  expect(await options.first().evaluate((node) => node.disabled)).toBe(true);
  // …and the absence is in words as well as in the widget, which is the note that was already
  // right: the two together are what stop "not read" from looking like "off".
  await expect(unread.locator(".note.surprising")).toHaveText(
    "the device reported no current value for this control",
  );

  // The positive control, and it is the load-bearing half: a panel that had simply stopped
  // selecting anything would satisfy every assertion above.
  const read = probed(page, "power_line_frequency");
  await expect(read.locator("option")).toHaveText(["0 · Disabled", "1 · 50 Hz", "2 · 60 Hz"]);
  await expect(read.locator("select")).toHaveValue("2");
});

test("a scalar whose value the device did not report draws no slider", async ({ page }) => {
  // The same rule as the menu claim above, on the widget where breaking it costs the most.
  // A `<select>` with no match shows its first option; a range input has no such state —
  // it has to hold a number, and the only one available is the declared default. So the
  // panel drew the slider at the default and left it live, and the gesture a slider invites
  // is *relative*: one nudge writes `default + step` to a control the device never said the
  // position of (note **N199**).
  await probePanel(page, [SCALAR_WITH_NO_CURRENT, MENU_WITH_A_CURRENT]);

  const card = probed(page, "brightness");
  await expect(card.locator('input[type="range"]')).toHaveCount(0);
  await expect(card.locator('input[type="number"]')).toHaveValue("");
  await expect(card).toContainText("the device reported no current value for this control");
  await expect(card).toContainText("type an absolute value to write one");

  // Nothing has been written, and nothing can be written by accident: the panel offers no
  // control that produces a value on its own.
  expect(await page.evaluate(() => window.wchWrites)).toEqual([]);

  // The inverse, beside it, or the assertion above would pass for a panel that draws no
  // sliders at all.
  await expect(probed(page, "power_line_frequency").locator("select")).toHaveValue("2");
});

test("the bitmask field refuses what it cannot write, and says when a write is in flight", async ({
  page,
}) => {
  // Three findings in one card, because they are three ways the same field lied.
  //
  // **`Number("") === 0`.** An operator who cleared the field and tabbed out wrote a zero to
  // the device — every bit off, silently, from a gesture that means "I have typed nothing yet".
  // **A non-numeric entry produced no feedback at all**: `Number.isFinite` was false, the
  // handler returned, and the panel sat there looking exactly as it does after a write that
  // worked. **And `writing…` was a status nobody could see** — it was recorded in `outcomes`,
  // which is read by the *next* paint, and the next paint happens after the write resolves.
  //
  // The last one is why the stub write above never answers on its own: an in-flight state is
  // only observable if something can hold the write open, and it is worth observing precisely
  // because the write it describes may never come back (a socket that dies mid-write leaves
  // this card as the only thing on screen that knows).
  await probePanel(page, [BITMASK_WITH_NO_CURRENT]);

  const card = probed(page, "region_of_interest_auto_ctrls");
  const field = card.locator("input[type=text]");
  // No current, so no number: the field showed `0x0` for a value the device never gave it,
  // which is the same fault as the menu above wearing hexadecimal.
  await expect(field).toHaveValue("");

  await field.fill("zz");
  await field.blur();
  await expect(card.locator(".note.live")).toHaveText(
    '"zz" is not a whole number this field can write, so nothing was written',
  );
  expect(await page.evaluate(() => window.wchWrites.length)).toBe(0);

  // What a real entry does, so that "refused" above is a fact about the entry rather than about
  // a field that cannot write at all.
  await field.fill("0x3");
  await field.blur();
  await expect(card.locator(".note.live")).toHaveText("writing…");
  expect(await page.evaluate(() => window.wchWrites)).toEqual([
    { control: "region_of_interest_auto_ctrls", value: { kind: "int", value: 3 } },
  ]);

  await page.evaluate(() => window.wchAnswer());
  await expect(card.locator(".note.live")).toHaveText("wrote 3");

  // And the emptied field, which is the destructive one: `0` is a legal bitmask and the device
  // would have taken it.
  await field.fill("");
  await field.blur();
  await expect(card.locator(".note.live")).toHaveText(
    "an empty field is not a zero, so nothing was written",
  );
  expect(await page.evaluate(() => window.wchWrites.length)).toBe(1);
});

test("the preview paints successive frames and keeps painting across a photo", async ({ page }) => {
  await openClient(page);

  // The status line is written by the `<img>`'s own `load` handler, so reaching it is already
  // one paint rather than one response.
  await expect(page.locator("#preview-status")).toHaveText(`streaming ${cameraId}`);
  // The element itself, remembered, so that every claim below is about *this* `<img>` and the
  // one request it opened.
  await page.evaluate(() => {
    window.wchFrame = document.getElementById("preview-frame");
  });
  // …and what it painted is the camera's own frame at the size the daemon negotiated: a decoded
  // image has a natural size and a broken one does not.
  expect(await page.evaluate(() => window.wchFrame.naturalWidth)).toBe(160);

  // **Successive frames, proven by the picture changing while the request does not.** Counting
  // `load` events does not work and finding that out is part of what this rung is for: Chrome
  // fires exactly one for a `multipart/x-mixed-replace` `<img>` and then replaces the frame
  // silently. What is observable is the pixels — so brightness is written through the panel and
  // the mean luma is watched, which asserts an *ordering* the fake declares (`fake::frames`:
  // brightness is a monotonic gain) rather than a pixel value, exactly as the hardware suites
  // do. A page that had fetched one frame and stopped cannot pass this, and neither can one
  // that re-opened its request, because the element identity is checked at the end.
  const brightness = control(page, "brightness").locator("input[type=number]");
  const write = async (value) => {
    await brightness.fill(String(value));
    await brightness.blur();
    await expect(brightness).toHaveValue(String(value));
  };

  await write(0);
  const dark = await settledLuma(page);
  await write(255);
  const bright = await settledLuma(page);
  expect(bright).toBeGreaterThan(dark);

  // Note **N83**: a photo suspends and resumes the preview inside the actor that owns the
  // device, and `photo.js` therefore holds no reference to the preview element at all. From a
  // viewer's side that is one claim — the picture pauses and comes back on the same request —
  // and this is the only place in the workspace it is made from a viewer's side.
  await page.locator("#take-photo").click();
  await expect(page.locator("#photo-frame")).toHaveAttribute("src", /^data:image\/jpeg;base64,/);
  await expect
    .poll(() => page.evaluate(() => document.getElementById("photo-frame").naturalWidth))
    .toBeGreaterThan(0);
  await expect(page.locator("#photo-report")).toContainText(/\d+ bytes, delivered as /);
  // The 2026-08-14 ruling, from the only side that can see it (note **N109**): both places
  // this report names a pixel format name it as four characters, because that is what
  // `wch_photo` now puts on the wire. Red if `photo.js` still calls `Array.prototype.map` on
  // what is now a string — `describe` would throw and the report would be empty — and red
  // again if any client renders `89,85,89,86` at an operator.
  //
  // **YUYV 640×480, not the 160×120 MJPG the preview is streaming**, and that is D5's
  // amendment rather than an accident: this fixture shrinks only the MJPG branch, so the
  // largest mode left on the camera is the uncompressed one and note **N85**'s ranking
  // picks it. The rendering sentence says `converted`, which is the same ruling's other
  // half showing through — a photo off an uncompressed mode is not verbatim bytes.
  await expect(page.locator("#photo-report")).toContainText(
    "converted from YUYV and encoded as jpeg",
  );
  await expect(page.locator("#photo-report")).toContainText("negotiated YUYV 640×480");
  // Anchored on the word before it, because the byte count in the same report is a number
  // this page does not control and `89` on its own would be a flake waiting for an 89-byte
  // frame. `negotiated 89` is what a page rendering the array shape would say.
  await expect(page.locator("#photo-report")).not.toContainText("negotiated 89");

  await write(0);
  expect(await settledLuma(page)).toBeLessThan(bright);
  // One element and one request throughout. `preview.js` replaces the node whenever it stops or
  // repoints a feed, so a page that had torn its preview down and opened another would fail
  // here having satisfied everything above.
  expect(await page.evaluate(() => window.wchFrame === document.getElementById("preview-frame")))
    .toBe(true);
});

/**
 * The preview's mean luma, once two consecutive reads agree.
 *
 * The fake ramps exposure for the first frames after `STREAMON` \[PF:11\] and then holds
 * steady, so "two samples the same" is the stream having settled — a condition the daemon
 * causes rather than a duration this suite guessed at. The image is same-origin, so the canvas
 * is untainted and nothing here needs a header.
 */
async function settledLuma(page) {
  const sample = () =>
    page.evaluate(() => {
      const img = document.getElementById("preview-frame");
      const canvas = document.createElement("canvas");
      canvas.width = img.naturalWidth;
      canvas.height = img.naturalHeight;
      const context = canvas.getContext("2d");
      context.drawImage(img, 0, 0);
      const pixels = context.getImageData(0, 0, canvas.width, canvas.height).data;
      let sum = 0;
      for (let at = 0; at < pixels.length; at += 4) {
        sum += pixels[at];
      }
      return Math.round(sum / (pixels.length / 4));
    });

  let last = null;
  await expect
    .poll(async () => {
      const now = await sample();
      const steady = now === last;
      last = now;
      return steady;
    })
    .toBe(true);
  return last;
}

test("the preview paints a recording's own frames and the page says who owns the camera", async ({
  page,
}) => {
  // **The owner's ruling of 2026-08-14, from the only side that can see it** (note **N117**).
  // The notes' Expected usage item 10 said a recording and a preview collide in a way a
  // photograph does not — a take holds the stream for its whole duration — and named two honest
  // answers. The ruling took the first: the preview is fed the recording's own frames. From a
  // viewer's side that is one claim, and it is not a claim any protocol test can make: **the
  // picture goes on moving, on the request the page already had, while another client records.**
  //
  // Four things are asserted and each would survive the other three being broken:
  //
  //   1. the page **says** a recording owns the camera, with the daemon's own numbers in it —
  //      item 10's *second* option, which the ruling made cheap rather than unnecessary, and the
  //      only explanation a viewer gets when D7 records a format an `<img>` cannot paint;
  //   2. the picture **keeps moving** while the take runs, proven by a control write changing
  //      the mean luma — an ordering the fake declares, never a pixel value;
  //   3. the `<img>` is the **same element with the same request** throughout, so the frames
  //      arrived on the response the page opened before the take began rather than on one it
  //      re-opened;
  //   4. when the take ends the page says so, and the picture still moves — a preview that had
  //      been left attached to nothing would satisfy 1 to 3 and fail here.
  //
  // The recording is driven on a **second** WebSocket, as the sweep claim below drives its
  // sweep: a page that had started the take itself would prove nothing about two consumers on
  // one camera, which is the arrangement the notes call the ordinary Tuesday of this deployment.
  await openClient(page);
  await expect(page.locator("#preview-status")).toHaveText(`streaming ${cameraId}`);
  await page.evaluate(() => {
    window.wchRecordedFrame = document.getElementById("preview-frame");
  });

  const brightness = control(page, "brightness").locator("input[type=number]");
  const write = async (value) => {
    await brightness.fill(String(value));
    await brightness.blur();
    await expect(brightness).toHaveValue(String(value));
  };
  await write(255);
  const before = await settledLuma(page);

  // MJPG by name rather than by default, and that is this fixture's shape showing through: the
  // camera's largest mode is uncompressed, so a take that asked for nothing would record YUYV —
  // which is a real answer (D7's raw fallback) and is the one case the ruling cannot serve,
  // because those bytes cannot go into an `<img>` labelled `image/jpeg`. That case is asserted
  // in `crates/daemon/tests/preview.rs`, where a socket can watch a count; this claim is about
  // the case a browser can see.
  const socket = await page.evaluateHandle(
    async ({ camera, wire, path }) => {
      const connection = new WebSocket(wire);
      await new Promise((opened, refused) => {
        connection.addEventListener("open", opened, { once: true });
        connection.addEventListener("error", () => refused(new Error("refused")), { once: true });
      });
      let next = 0;
      connection.wchCall = (method, params) =>
        new Promise((resolve, reject) => {
          const id = (next += 1);
          const onMessage = (event) => {
            const frame = JSON.parse(event.data);
            if (frame.id !== id) {
              return;
            }
            connection.removeEventListener("message", onMessage);
            if (frame.error === undefined) {
              resolve(frame.result);
            } else {
              reject(new Error(JSON.stringify(frame.error)));
            }
          };
          connection.addEventListener("message", onMessage);
          connection.send(JSON.stringify({ jsonrpc: "2.0", id, method, params }));
        });
      await connection.wchCall("wch_record_start", {
        camera,
        request: {
          stream: { pixel_format: "MJPG" },
          // Long enough that the luma dance below is comfortably inside the take, and short
          // enough that its *own* ending is what this claim waits for rather than a
          // `record_stop` — the ending is the state item 10's second option is about, and it is
          // the one a poller meets: a take is over for as long as it takes its caller to notice.
          duration_ms: 5000,
          sink: { kind: "server_path", path },
        },
      });
      return connection;
    },
    {
      camera: cameraId,
      wire: `${origin.replace(/^http/, "ws")}/rpc?token=${encodeURIComponent(token)}`,
      path: takePath,
    },
  );

  // Claim 1. Matched on the sentence's stable half — the numbers in it are a clock's and a
  // regex over them would be a claim about how fast this machine is.
  await expect(page.locator("#recording-status")).toContainText("a recording owns this camera");
  await expect(page.locator("#recording-status")).toContainText("the recording's own frames");

  // Claim 2. The same instrument the photo claim above uses, for the same reason: Chrome fires
  // one `load` for a `multipart/x-mixed-replace` `<img>` and then replaces the frame silently,
  // so what is observable is the pixels. A build in which the take kept its frames to itself
  // paints the last pre-take frame for ever and this never moves.
  await write(0);
  expect(await settledLuma(page)).toBeLessThan(before);

  // Claim 3, taken while the take is still running: one element, one request. `preview.js`
  // replaces the node whenever it stops or repoints a feed, so a page that had torn its preview
  // down and opened another — which is what a client would have to do if the daemon had simply
  // ended the feed — fails here having satisfied everything above.
  expect(
    await page.evaluate(() => window.wchRecordedFrame === document.getElementById("preview-frame")),
  ).toBe(true);

  // Claim 4, in the two states a take really has. First it **ends on its own duration** and
  // nobody has collected it — the state an agent's poll loop meets, and the one the page can
  // still describe — and the sentence says the preview is this camera's own stream again, which
  // is `Previews::release`'s second answer arriving at a person: a tab that is still open gets a
  // driver back rather than the end of its response. Then it is **collected**, the daemon stops
  // remembering it (note **N114**), and this line has nothing left to say and says nothing.
  await expect(page.locator("#recording-status")).toContainText("finished");
  await expect(page.locator("#recording-status")).toContainText("this camera's own stream again");
  await page.evaluate(
    ({ connection, camera }) => connection.wchCall("wch_record_stop", { camera }),
    { connection: socket, camera: cameraId },
  );
  await expect(page.locator("#recording-status")).toHaveText("");

  // …and the picture is still this camera's, on the same request, after all of it.
  await write(255);
  expect(await settledLuma(page)).toBeGreaterThan(0);
  expect(
    await page.evaluate(() => window.wchRecordedFrame === document.getElementById("preview-frame")),
  ).toBe(true);
});

test("the calibration view tracks a sweep it did not start", async ({ page }) => {
  // **The preview is kept off this page, and the reason is a finding rather than a
  // convenience.** A sweep and a live preview on the same camera collide: `engine::preview::
  // while_suspended` is what lets a *photo* interleave with a preview (note N83) and
  // `engine::photo::take` is its only caller, so `wch_calibrate_sweep` against a camera this
  // page is previewing is refused `Busy` — measured here, at 2026-08-13, by this rung failing
  // that way. That is the deployment's own arrangement (the owner watches the client *while*
  // calibrating from `webcam-handler-client`), so it is recorded in this sub-milestone's
  // report rather than worked around silently; what is worked around is only this claim's need
  // for a free camera.
  //
  // Matched on the *path*, not on a substring: `/preview.js` is one of the client's modules,
  // and a `/\/preview/` regex aborts it — which is a page that never runs at all, and is how
  // the first version of this line failed.
  await page.route((url) => url.pathname === "/preview", (route) => route.abort());

  await openClient(page);
  await expect(page.locator("#preview-status")).toContainText("could not load the preview");
  // Its own status line since P5d — see index.html. While the session list and the sweep
  // subscription shared one, this sentence was overwritten within milliseconds of being
  // written, so a *refused* subscription looked exactly like a healthy page.
  await expect(page.locator("#sweep-status")).toHaveText("watching every sweep this daemon runs");

  // The sweep is driven on a **second** WebSocket, which makes this the arrangement
  // `wch_subscribe_calibration`'s parameterless shape exists for: the subscription is per
  // client, every event carries its session id, and the page is watching a session it did not
  // open. A page that had started the sweep itself would prove nothing about that — and
  // `calibration.js` starts none on purpose, because a sweep is minutes of motor wear behind a
  // click with no plan attached.
  const session = await page.evaluate(
    async ({ camera, wire }) => {
      const socket = new WebSocket(wire);
      await new Promise((opened, refused) => {
        socket.addEventListener("open", opened, { once: true });
        socket.addEventListener("error", () => refused(new Error("refused")), { once: true });
      });
      let next = 0;
      const call = (method, params) =>
        new Promise((resolve, reject) => {
          const id = (next += 1);
          const onMessage = (event) => {
            const frame = JSON.parse(event.data);
            if (frame.id !== id) {
              return;
            }
            socket.removeEventListener("message", onMessage);
            if (frame.error === undefined) {
              resolve(frame.result);
            } else {
              reject(new Error(JSON.stringify(frame.error)));
            }
          };
          socket.addEventListener("message", onMessage);
          socket.send(JSON.stringify({ jsonrpc: "2.0", id, method, params }));
        });

      const opened = await call("wch_calibrate_start", {
        camera,
        task: "p5d sweep watched from a real browser",
        goal: "a legible frame",
        criteria: ["sharp"],
      });
      const which = { kind: "id", id: opened.id };
      await call("wch_calibrate_plan", {
        camera,
        session: which,
        controls: ["brightness"],
        order: false,
      });
      await call("wch_calibrate_sweep", {
        camera,
        session: which,
        request: { control: "brightness", plan: { kind: "explicit", values: [0, 255] } },
      });
      socket.close();
      return opened.id;
    },
    {
      camera: cameraId,
      wire: `${origin.replace(/^http/, "ws")}/rpc?token=${encodeURIComponent(token)}`,
    },
  );

  // The view prefixes every line with the session it belongs to, rather than dropping events
  // for sessions it is not looking at — which is what makes one page able to watch a daemon
  // running more than one.
  const short = session.slice(0, 8);
  const log = page.locator("#sweep-log li");
  // The **stride** rides the start line since note **N145**, and this is the surface AGENTS
  // names for it: the owner calibrates from this page and is the person who types a precision.
  // Two values 255 apart plan a stride of 255, and a line that said only "2 samples" would
  // leave every comparison drawn from those photographs drawn at a resolution nobody chose.
  await expect(log.first()).toHaveText(
    `${short} sweep of brightness started: 2 samples (2 named values) — every 255`,
  );
  await expect(log.last()).toHaveText(`${short} sweep of brightness finished after 2 samples`);

  const lines = await log.allTextContents();
  expect(lines.every((line) => line.startsWith(short))).toBe(true);
  // `index`/`total` are read off each event rather than counted, which is the property that
  // lets a subscriber joining mid-sweep paint a truthful bar — and `requested → applied` rides
  // every sample, because a sample labelled with a value the camera never held would poison
  // every comparison built on it \[PF:6\].
  expect(lines).toContain(`${short} 1/2 brightness := 0`);
  expect(lines.some((line) => line.startsWith(`${short} 2/2 brightness sampled at 255`))).toBe(true);
});

test("the client loads with no credential and the camera is still refused", async ({ page }) => {
  // Note **N82**, and the half only a browser can show. Before the owner's 2026-08-12 ruling
  // every route was gated, and a module graph is one request per module with no query string
  // carried over — so the page fetched its own files anonymously and the gate refused them,
  // correctly. The ruling opened the assets and kept the camera closed, and *both* halves of
  // that are assertions here: a rung that still claimed "anonymous requests are refused" would
  // assert something false of `/`.
  const served = new Map();
  page.on("response", (response) => {
    const url = new URL(response.url());
    if (url.origin === origin) {
      served.set(url.pathname, response.status());
    }
  });

  await page.goto(`${origin}/`);

  // The modules did not merely arrive — they **ran**, and they got as far as being refused a
  // socket. `index.html` ships `connecting…` in that element and only a module ever replaces
  // it, so this is the client executing without a credential rather than ten `200`s that did
  // nothing.
  //
  // **The sentence is the one this page can know, and the assertion is pinned to that rather
  // than to whatever it happens to say.** A browser does not hand a page the status of a
  // failed WebSocket handshake, so two candidates are named instead of one guessed at — but
  // *which* two depends on what this page presented, and on this page it presented nothing.
  // Until P5e the `catch` wrote "either the token this page was opened with is not this run's,
  // or nothing is listening on this port any more" unconditionally, and this claim pinned that
  // sentence and called it honest: a first disjunct that is false by construction, on the one
  // failure `app.js`'s own header calls the most likely — an operator who read the port off a
  // log and opened `/` by hand — being read by the operator least able to argue with it. A
  // claim that pins a wrong sentence is a claim that cannot go red on it, which is why the
  // repair is here as well as in the client.
  await expect(page.locator("#connection")).toHaveClass(/failed/);
  await expect(page.locator("#connection")).toHaveText(
    "this page was opened without the ?token= the daemon prints. If webcam-handler-daemon was started " +
      "with --http-insecure-loopback there is no token and the camera routes are open; " +
      "otherwise the socket and the preview will be refused, and the URL webcam-handler-daemon printed is " +
      "the one to open. The socket was then refused (webcam-handler-daemon did not accept a WebSocket) " +
      "— a page that presented no token cannot be carrying the wrong one, so what is left is the " +
      "gate above or nothing listening on this port any more.",
  );

  expect(served.get("/")).toBe(200);
  expect(served.get("/app.css")).toBe(200);
  const modules = [...served].filter(([path]) => path.endsWith(".js"));
  expect(modules.map(([, status]) => status)).toEqual(Array(moduleCount).fill(200));

  // …and the two routes that carry the camera turn the same anonymous browser away. Both are
  // attempted the way the page itself would attempt them, because that is the only way to find
  // out what a browser does with the answer: a `401` on a WebSocket handshake is not observable
  // from script at all, and a `401` on an `<img>` is an `error` event with nothing in it.
  const upgrade = await page.evaluate(
    () =>
      new Promise((resolve) => {
        const socket = new WebSocket(`ws://${location.host}/rpc`);
        socket.addEventListener("open", () => resolve("opened"));
        socket.addEventListener("close", () => resolve("closed"));
        socket.addEventListener("error", () => resolve("errored"));
      }),
  );
  expect(upgrade).not.toBe("opened");

  const paint = (camera, credential) =>
    page.evaluate(
      ({ camera: id, credential: secret }) =>
        new Promise((resolve) => {
          const image = new Image();
          image.addEventListener("load", () => resolve("painted"));
          image.addEventListener("error", () => resolve("refused"));
          const query = new URLSearchParams({ camera: id });
          if (secret !== null) {
            query.set("token", secret);
          }
          image.src = `/preview?${query}`;
        }),
      { camera, credential },
    );
  expect(await paint(cameraId, null)).toBe("refused");
  // The positive control, without which the line above is a test that a broken URL fails: the
  // same request from the same anonymous page, with the token the daemon printed, paints.
  expect(await paint(cameraId, token)).toBe("painted");
});

test("a page in another origin cannot reach the camera, token and all", async ({ page }) => {
  // **The owner's ruling of 2026-08-13, asserted with headers a browser wrote rather than
  // headers a test typed.** `crates/daemon/tests/http.rs` drives every shape of this rule over a
  // hand-written socket, which is the right place for a matrix; what it cannot establish is that
  // a *real* Chromium tags a *real* foreign request the way `daemon::http::provenance` expects.
  // That is this claim, and it is the only place in the workspace where both halves — the tag
  // and the refusal — are produced by the things that produce them in the field.
  //
  // **The foreign origin is a sandboxed iframe**, which is how a page gets an opaque origin
  // without a second host, a DNS name or Playwright's request interception. Note **N93**
  // deliberately refused to draw a conclusion from an intercepted `http://evil.example` — Chrome
  // decides a document's IP address space from the connection that delivered it, so a synthesised
  // page may not be classified the way a real one would. A `srcdoc` iframe under
  // `sandbox="allow-scripts"` needs no such synthesis: the document is delivered by this daemon,
  // and the sandbox alone makes its origin opaque, which is the property under test.
  //
  // **It carries this run's real token**, which is what makes the claim about the cross-origin
  // rule rather than about the gate. Before 2026-08-13 this request was served: the token is in
  // the URL, an `<img>` may carry no header, and D11's gate had nothing else to ask.
  const previewed = [];
  page.on("response", (response) => {
    const url = new URL(response.url());
    if (url.origin === origin && url.pathname === "/preview") {
      previewed.push(
        response
          .request()
          .allHeaders()
          .then((headers) => ({ status: response.status(), headers })),
      );
    }
  });

  await page.goto(`${origin}/`);
  const image = `${origin}/preview?${new URLSearchParams({ camera: cameraId, token })}`;
  const socket = `${origin.replace(/^http/, "ws")}/rpc?${new URLSearchParams({ token })}`;

  // The positive control, and it is the load-bearing one: this is the `<img>` shape N93 measured
  // to carry **no `Origin` at all**, so a rule built on `Origin` alone would have had no opinion
  // about it in either direction. It must still paint.
  const ours = await page.evaluate(
    (src) =>
      new Promise((resolve) => {
        const element = new Image();
        element.addEventListener("load", () => resolve("painted"));
        element.addEventListener("error", () => resolve("refused"));
        element.src = src;
      }),
    image,
  );
  expect(ours).toBe("painted");

  // The same two requests, from an origin the browser considers foreign. The iframe reports back
  // by `postMessage`, which is all a sandboxed document may do to its parent.
  const foreign = await page.evaluate(
    async ({ image: src, socket: url }) => {
      const inAnOpaqueOrigin = (script) =>
        new Promise((resolve) => {
          const frame = document.createElement("iframe");
          frame.setAttribute("sandbox", "allow-scripts");
          const answered = (event) => {
            if (event.source === frame.contentWindow) {
              window.removeEventListener("message", answered);
              frame.remove();
              resolve(event.data);
            }
          };
          window.addEventListener("message", answered);
          frame.srcdoc = `<script>${script}</script>`;
          document.body.append(frame);
        });

      return {
        painted: await inAnOpaqueOrigin(`
          const element = new Image();
          element.addEventListener("load", () => parent.postMessage("painted", "*"));
          element.addEventListener("error", () => parent.postMessage("refused", "*"));
          element.src = ${JSON.stringify(src)};
        `),
        upgrade: await inAnOpaqueOrigin(`
          const connection = new WebSocket(${JSON.stringify(url)});
          connection.addEventListener("open", () => parent.postMessage("opened", "*"));
          connection.addEventListener("close", () => parent.postMessage("closed", "*"));
          connection.addEventListener("error", () => parent.postMessage("errored", "*"));
        `),
      };
    },
    { image, socket },
  );
  expect(foreign.painted).toBe("refused");
  // A refused handshake is not observable from script as a status — a browser hands a page an
  // `error` or a `close` and never a code — so the honest assertion is the negative one, exactly
  // as in the anonymous claim above.
  expect(foreign.upgrade).not.toBe("opened");

  // **What the daemon actually answered**, which is what turns "refused" from a fact about
  // Chromium into a fact about `webcam-handler-daemon`. A browser that had blocked the request
  // on its own — Private Network Access, a mixed-content rule, an extension — would produce the
  // same `error` event and no response at all, and this suite would have claimed something it did
  // not check. Two responses in order: ours, then theirs.
  const seen = await Promise.all(previewed);
  expect(seen.map((response) => response.status)).toEqual([200, 403]);

  // …and the header the daemon read it from, so this claim re-measures N93's table for the shape
  // no operator can produce by hand. The day a browser stops tagging an opaque-origin subresource
  // this way, this line is where it is discovered — which is the reason N93 records the browser
  // build beside the table.
  expect(seen[1].headers["sec-fetch-site"]).toBe("cross-site");
});

/**
 * Proxy this page's socket, open the client through it, and hand back the cut.
 *
 * **How the socket is cut, and why it is cut this way.** What the three claims below assert is
 * `rpc.js`'s `close` handler and `app.js`'s answer to it — the path a stopped daemon or a
 * dropped link takes — and a test that closed the socket politely from inside the page would
 * reach neither. `context.setOffline(true)` was tried first and does not close an established
 * WebSocket in this Chromium, which is worth recording because it looks like it should. So the
 * connection is proxied and then dropped from the middle: the page's socket really ends while
 * the daemon is still up and still holding its side, which is the arrangement a lost link
 * produces.
 */
async function proxiedSocket(page) {
  let dropped = null;
  await page.routeWebSocket((url) => url.pathname === "/rpc", (socket) => {
    socket.connectToServer();
    dropped = socket;
  });
  await openClient(page);
  return () => dropped.close();
}

test("the page reports a lost socket and works again on the next one", async ({ page }) => {
  const drop = await proxiedSocket(page);
  await expect(page.locator("#preview-status")).toHaveText(`streaming ${cameraId}`);

  drop();
  await expect(page.locator("#connection")).toHaveText(
    "the connection to webcam-handler-daemon closed; reload the URL webcam-handler-daemon printed",
  );
  await expect(page.locator("#take-photo")).toBeDisabled();
  // The preview is a *separate* HTTP request and would otherwise keep painting frames from a
  // daemon this page can no longer ask anything about, so it is ended too — and ending it is
  // how a browser tells the daemon its last reader has gone.
  expect(
    await page.evaluate(() => document.getElementById("preview-frame").hasAttribute("src")),
  ).toBe(false);

  // And the reconnect, which is the client's own advice ("reload the URL webcam-handler-daemon
  // printed") taken literally: the same token opens a second socket on the same daemon, and a
  // full round-trip over it repaints the panel from the device.
  await page.unrouteAll();
  await openClient(page);
  await expect(page.locator("#preview-status")).toHaveText(`streaming ${cameraId}`);
  await expect(control(page, "auto_exposure").locator("select")).toHaveValue("3");
});

test("a socket that closed refuses the next call instead of parking it", async ({ page }) => {
  // **The defect is one instant wide, and `rpc.js`'s own `close` handler is the thing that
  // argues against it.** That handler rejects every call pending *at* the close because "a
  // promise nobody will ever settle is a spinner that spins forever" — and then nothing marked
  // the handle dead, so the very next call parked exactly such a promise. WHATWG is why it is
  // silent rather than loud: `WebSocket.send()` throws only while the socket is CONNECTING, and
  // on a CLOSING or CLOSED one it **discards the frame and returns**. No exception, no answer,
  // no id ever seen again.
  //
  // What that looks like from a chair is what this claim drives, and it is why the claim is
  // here rather than in a protocol test: the page keeps every affordance it had. A click on the
  // camera it is already showing re-enabled the photo button and reopened the MJPEG preview — a
  // separate HTTP request whose token is as good as it ever was, so it paints — while
  // `refreshControls()` hung at its `await` and left the previous panel on screen. Live video
  // and a full control panel under a banner saying the connection is gone.
  const drop = await proxiedSocket(page);
  await expect(page.locator("#preview-status")).toHaveText(`streaming ${cameraId}`);
  // The positive control, without which every assertion below is satisfied by a page that
  // never worked: the button an operator uses is usable while the socket is up.
  await expect(page.locator("#take-photo")).toBeEnabled();

  drop();
  await expect(page.locator("#connection")).toHaveText(
    "the connection to webcam-handler-daemon closed; reload the URL webcam-handler-daemon printed",
  );

  // The click, on the one camera this fixture has. Nothing about the button changed when the
  // socket died — it is markup this page painted from a listing it already had — so this is the
  // ordinary thing an operator does next, and it is the path that used to undo every decision
  // `socketClosed` had just made.
  await page.locator("button[data-camera]").first().click();

  // Three consumers, three refusals, and none of them a spinner. `wch_controls` and
  // `wch_calibrate_list` are answered at once by the handle rather than by the daemon, and each
  // renders the refusal it already knew how to render.
  await expect(page.locator("#controls-status")).toHaveText(
    "the connection to webcam-handler-daemon closed",
  );
  await expect(page.locator("#control-panel .control")).toHaveCount(0);
  await expect(page.locator("#calibration-status")).toHaveText(
    "the session list was refused: the connection to webcam-handler-daemon closed",
  );
  // …and the two things the click used to switch back on. `#take-photo` has one owner since
  // P5e — a click that re-enabled it was handing an operator a button whose call could never be
  // answered, and `#photo-status` would have read "taking a photo …" for as long as the tab
  // lived — and the preview is left as `socketClosed` left it.
  await expect(page.locator("#take-photo")).toBeDisabled();
  expect(
    await page.evaluate(() => document.getElementById("preview-frame").hasAttribute("src")),
  ).toBe(false);
});

test("a subscription that died with the socket stops saying it is watching", async ({ page }) => {
  // **E16 §1's defect class through a different door.** `#sweep-status` exists because the
  // calibration view's own line was being overwritten by the session list's, and index.html
  // says what that cost: "a calibration view that had silently stopped being live looked
  // exactly like a healthy one". `rpc.js`'s `close` handler then dropped every registration
  // with `streams.clear()` **without calling one `ended`** — so the element got its own home
  // and went on reading `watching every sweep this daemon runs` about a subscription that no
  // longer existed, which is the same lie with the same shape.
  //
  // The hotplug watch died the same way and more quietly: neither its one retry nor its "the
  // device-change stream ended twice" sentence could fire, because nothing told it anything.
  const drop = await proxiedSocket(page);
  await expect(page.locator("#sweep-status")).toHaveText("watching every sweep this daemon runs");

  drop();
  await expect(page.locator("#sweep-status")).toHaveText(
    "the sweep stream ended because the connection to webcam-handler-daemon closed",
  );

  // **And the sentence the fix must not produce.** Telling the hotplug watch its stream ended
  // makes it re-subscribe, that call is refused by a dead handle, and its refusal is written
  // into this element as "connected; this daemon cannot watch for device changes …" — a
  // sentence that opens by contradicting the one above it. `watchDevices` therefore declines to
  // write at all once the socket is what ended, which is the ownership rule app.js's header
  // states: no writer of this element may make a statement it cannot know.
  await expect(page.locator("#connection")).toHaveText(
    "the connection to webcam-handler-daemon closed; reload the URL webcam-handler-daemon printed",
  );
});

// ------------------------------------------------- what the page stops doing, asked in Chromium
//
// The five claims below are the G6 review's own finding about this rung turned into arms of it
// (docs/11 §2.2). Every claim above asserts that something *happens* — a menu paints, a frame
// arrives, a refusal is rendered — and the review's fourth HIGH was a stream that never *ends*:
// `preview.stop()` detached an `<img>` and left its `multipart/x-mixed-replace` response running
// for the life of the tab. That defect is invisible in the source, invisible to every protocol
// test, and was read past by a review that confirmed fourteen other defects in the same module,
// because the question it turns on — "is a detached element's request aborted?" — is a question
// about Chromium. **A rung that only ever asks whether a thing starts cannot see a thing that
// does not stop**, so these arms ask the other half: what the page stops streaming, stops
// showing, stops believing and stops saying.

/**
 * Proxy this page's socket, and hand back the levers a claim needs on the frames crossing it.
 *
 * `proxiedSocket` above cuts a connection; this cuts *into* one. Three claims need something a
 * healthy daemon will never do for them — an answer that is refused, an answer that arrives
 * late, and a link that stops carrying anything without ever closing — and all three are
 * properties of the wire rather than of the daemon, so the wire is where they are arranged.
 * Everything not named by a lever is forwarded verbatim in both directions, which is what keeps
 * the page talking to the real `webcam-handler-daemon` for every other frame of the run.
 *
 * The levers, and what each one is for:
 *
 * - **`refuse(predicate, error)`** answers a matching *request* from the page itself and never
 *   forwards it, which is the only way to see what this client does with a `wch_list` the fake
 *   backend will always answer (**M33**).
 * - **`hold(predicate)`** keeps matching *answers* off the page until `release()`, which is how
 *   an answer about a camera the page has walked away from is made to arrive after the walk
 *   rather than during it (**M32**).
 * - **`sever()`** stops forwarding in both directions while leaving both sockets open — a link
 *   that died without a FIN, which is the one failure `rpc.js` had no answer for (**L38**).
 * - **`answered(method)`** counts the answers that reached the page for a request of that name,
 *   which is how a claim asks whether the daemon replied to something the *page* decided to
 *   send. The heartbeat is the only such call — nothing in the client's own code path asks for
 *   it — so it is the only one whose arrival is otherwise unobservable from the DOM.
 */
async function interposed(page) {
  const wire = {
    severed: false,
    refusals: [],
    holding: () => false,
    held: [],
    toPage: null,
    answered: [],
  };
  await page.routeWebSocket(
    (url) => url.pathname === "/rpc",
    (socket) => {
      const server = socket.connectToServer();
      wire.toPage = socket;
      // **Per socket, because JSON-RPC ids are.** Every connection numbers its calls from one,
      // so a claim that opens a second socket beside the client's own — the only way to watch an
      // *idle* connection, since `recording.js` polls the client's once a second — would have two
      // requests under id 1 in one map, and `hold` would then be deciding about whichever was
      // written last. The levers below reach the most recent socket, which is the one a claim
      // that opens its own is asking about.
      const asked = new Map();
      socket.onMessage((message) => {
        if (wire.severed) {
          return;
        }
        const frame = JSON.parse(message);
        // Remembered before anything is decided, because an answer carries an id and nothing
        // else: "the answer to the `wch_controls` this page sent about that camera" is only a
        // sentence the proxy can say if the proxy wrote the request down first.
        asked.set(frame.id, frame);
        const refusal = wire.refusals.find((candidate) => candidate.matches(frame));
        if (refusal !== undefined) {
          socket.send(
            JSON.stringify({ jsonrpc: "2.0", id: frame.id, error: refusal.error }),
          );
          return;
        }
        server.send(message);
      });
      server.onMessage((message) => {
        if (wire.severed) {
          return;
        }
        const frame = JSON.parse(message);
        const request = asked.get(frame.id);
        if (wire.holding(request ?? {}, frame)) {
          wire.held.push(message);
          return;
        }
        if (request !== undefined) {
          wire.answered.push(request.method);
        }
        socket.send(message);
      });
    },
  );
  return {
    refuse: (matches, error) => wire.refusals.push({ matches, error }),
    /**
     * Stop refusing anything, leaving the proxy in place.
     *
     * A claim's positive control runs through the same wire rather than around it — `unrouteAll`
     * would take the proxy out and leave "the page works" a statement about a page this suite
     * had stopped interfering with, which is a weaker thing to have proved.
     */
    allow: () => {
      wire.refusals.length = 0;
    },
    hold: (predicate) => {
      wire.holding = predicate;
    },
    /**
     * Let one held answer through — **newest first** — and answer whether there was one.
     *
     * The order is the whole point rather than a convenience: what is being reproduced is a
     * daemon that spawns a task per inbound message and therefore answers two questions in the
     * order its scheduler felt like, so releasing the answers a page is waiting for in reverse
     * is the arrival this rung could not otherwise arrange.
     */
    release: () => {
      const message = wire.held.pop();
      if (message === undefined) {
        return false;
      }
      wire.toPage.send(message);
      return true;
    },
    held: () => wire.held.length,
    answered: (method) => wire.answered.filter((name) => name === method).length,
    sever: () => {
      wire.severed = true;
    },
    /** End the page's socket from the wire, which is ordered *after* everything released. */
    close: () => wire.toPage.close(),
  };
}

/**
 * Open `count` readers of one camera's preview and answer what the daemon said to each.
 *
 * **This is the daemon's own viewer count, read through the one aperture a browser has.**
 * Nothing on this listener reports how many readers a feed has — `CAMERA_BEARING_PATHS` is two
 * routes and neither is a status page — but `Previews::reserve` refuses the reader past
 * `limits::PREVIEW_MAX_VIEWERS_PER_CAMERA` with `Error::Busy`, which reaches an HTTP client as
 * `503`. So a camera with *no* readers left over serves every one of `previewViewerCap`, and a
 * camera holding one serves one fewer and refuses the last. The two answers are different
 * arrays, which is what makes the count observable without a route that exists for a test.
 *
 * Opened one at a time and held open until the last has answered, because concurrency is the
 * whole point: `fetch` resolves on the response headers, and the reservation is already made by
 * the time they are written. They are aborted on the way out, so a claim that calls this twice
 * is not counting its own probe the second time.
 *
 * **And it is one connection under a limit that is not this project's.** Chromium allows six
 * concurrent HTTP/1.1 connections per host, and this probe holds `previewViewerCap` of them
 * (four) beside the page's own preview: five. A viewer cap raised to five would make the sixth
 * `fetch` queue in the browser *behind* connections that never end, so it would never reach the
 * daemon and this probe would hang rather than report a `503` — a deadlock against Chromium
 * wearing a test's timeout. Raising `limits::PREVIEW_MAX_VIEWERS_PER_CAMERA` past four therefore
 * means reading the cap here through a second browser context (its own connection pool) rather
 * than adjusting a number: the constant this probe reads is the daemon's, and the one it runs
 * into is the browser's.
 */
function previewReaders(page, camera, count) {
  return page.evaluate(
    async ({ camera, count, token }) => {
      const query = new URLSearchParams();
      query.set("camera", camera);
      if (token !== null) {
        query.set("token", token);
      }
      const url = `/preview?${query}`;
      const controllers = [];
      const statuses = [];
      try {
        for (let reader = 0; reader < count; reader += 1) {
          const controller = new AbortController();
          controllers.push(controller);
          statuses.push((await fetch(url, { signal: controller.signal })).status);
        }
      } finally {
        for (const controller of controllers) {
          controller.abort();
        }
      }
      return statuses;
    },
    { camera, count, token },
  );
}

/** What `previewReaders` sees on a camera nobody else is watching. */
const everyReaderServed = () => Array.from({ length: previewViewerCap }, () => 200);

/** …and on a camera one other reader already holds: one fewer, and the last refused. */
const oneReaderShort = () => [
  ...Array.from({ length: previewViewerCap - 1 }, () => 200),
  503,
];

test("a preview the page walked away from is a preview the daemon stopped", async ({ page }) => {
  // **docs/11 H4, measured here rather than reasoned about.** `preview.stop()` cloned the
  // `<img>`, called `removeAttribute("src")` on the *clone* — which never had a request — and
  // replaced the original, which went on owning a live `multipart/x-mixed-replace` response.
  // The doc comment above it argued, correctly and at length, why removing the attribute beats
  // assigning `""`, and applied that argument to the wrong object. Nothing in the client ever
  // aborted the request, so every camera a tab had ever looked at stayed open and streaming
  // until the tab closed: D12's idle close can never fire for a camera in use, four returns to
  // one camera exhaust `limits::PREVIEW_MAX_VIEWERS_PER_CAMERA` with viewers nobody is
  // watching, and every one of them is a camera the *agent* — this project's primary
  // consumer — then meets as `Busy`.
  //
  // It is asserted through the daemon's refusal rather than through the page, because the page
  // cannot see it: from inside the document a detached element is gone whether or not its
  // request is. `previewReaders` says how.
  await openClient(page);
  await expect(page.locator("#preview-status")).toHaveText(`streaming ${cameraId}`);

  // **The positive control, and it is load-bearing.** Every assertion below is satisfied by a
  // probe that cannot see a held reader at all — a wrong camera, a wrong route, a token the
  // gate ignores — so the first thing established is that this probe *can* see one: the page
  // is watching this camera right now, and the last reader is refused because of it.
  await expect.poll(() => previewReaders(page, cameraId, previewViewerCap)).toEqual(
    oneReaderShort(),
  );

  await page.locator(`button[data-camera="${secondCameraId}"]`).click();
  await expect(page.locator("#preview-status")).toHaveText(`streaming ${secondCameraId}`);

  // The claim. Polled rather than asked once, because the release is the daemon's: the driver
  // notices its last reader has gone between frames, which is a condition it causes rather than
  // a duration this suite guessed at.
  await expect.poll(() => previewReaders(page, cameraId, previewViewerCap)).toEqual(
    everyReaderServed(),
  );

  // …and the camera the page moved *to* is held, which is the same probe answering the other
  // way round on the same run. Without it, a build whose preview had simply stopped working
  // would satisfy the line above.
  await expect.poll(() => previewReaders(page, secondCameraId, previewViewerCap)).toEqual(
    oneReaderShort(),
  );
});

test("the control panel on screen belongs to the camera on screen", async ({ page }) => {
  // **docs/11 M32, in the two halves it has.** `select()` set `state.camera` synchronously and
  // then awaited `wch_controls`; between those two the panel was neither cleared nor fenced, so
  // for the whole round trip — a device open and a control walk, minutes if a sweep is in front
  // of that actor — every widget on screen belonged to the previous camera while `write()` read
  // `state.camera` at *send* time. A click in that window wrote one camera's value to another
  // camera's control. And because the daemon spawns a task per inbound WS message, two answers
  // can come back out of order and paint the wrong panel permanently.
  //
  // The two halves are asserted separately because they are two defects: one is a window, the
  // other is an ordering, and a repair for either leaves the other standing.
  const wire = await interposed(page);
  await openClient(page);
  const brightness = () => control(page, "brightness").locator("input[type=number]");
  // Read rather than written down: the claims above this one drive brightness on the first
  // camera, so its value here is whatever the last of them left, and a literal would be this
  // claim asserting the order of the file it lives in. What has to be true is only that the two
  // cameras answer differently — which is what makes "whose panel is this" a question with an
  // answer — and the fixture arranges that on purpose.
  const firstBrightness = await brightness().inputValue();
  expect(firstBrightness).not.toBe(secondBrightness);

  // **Half one: the window, observed synchronously.** The click is dispatched from inside the
  // page and the panel is read in the same task, so what is measured is the state `select()`
  // leaves behind *before its first `await`* — the exact instant an operator's next click would
  // land in. Playwright's own auto-waiting cannot ask this question: every locator assertion
  // retries, so it would be satisfied by the correct panel arriving a moment later, which is
  // precisely the moment this defect is not about.
  const duringTheSwitch = await page.evaluate((camera) => {
    document.querySelector(`button[data-camera="${camera}"]`).click();
    return {
      controls: document.querySelectorAll("#control-panel .control").length,
      status: document.getElementById("controls-status").textContent,
    };
  }, secondCameraId);
  expect(duringTheSwitch.controls).toBe(0);
  expect(duringTheSwitch.status).toBe(`reading ${secondCameraId}'s controls…`);

  // …and the panel that arrives is the new camera's, told apart by what the *device* reports
  // rather than by its place on the page: this fixture's second camera holds a different
  // brightness on purpose.
  await expect(brightness()).toHaveValue(secondBrightness);

  // **Half two: the ordering.** Both answers are held off the page, released in the wrong
  // order, and the panel is asked whose it is. The first camera's answer is released *last* —
  // after the second camera's has already painted — which is the arrival a task-per-message
  // server can produce on its own and which used to repaint the previous camera's values over
  // the current camera's panel, permanently, with nothing on screen to say so.
  wire.hold((request) => request.method === "wch_controls");
  await page.locator(`button[data-camera="${cameraId}"]`).click();
  await expect(page.locator("#controls-status")).toHaveText(`reading ${cameraId}'s controls…`);
  await page.locator(`button[data-camera="${secondCameraId}"]`).click();
  await expect(page.locator("#controls-status")).toHaveText(
    `reading ${secondCameraId}'s controls…`,
  );
  await expect.poll(() => wire.held()).toBe(2);

  wire.hold(() => false);
  // Newest first: the second camera's answer paints, and then the first camera's — the stale
  // one — arrives on a page that has moved on.
  expect(wire.release()).toBe(true);
  await expect(brightness()).toHaveValue(secondBrightness);
  expect(wire.release()).toBe(true);

  // **How the stale answer is known to have been delivered**, which is the half an assertion
  // about an absence has to establish or it is asserting nothing. The close is sent on the same
  // socket, after it, and a WebSocket delivers in order — so a page that reports the closure has
  // already run its handler for every frame in front of it, this one included.
  wire.close();
  await expect(page.locator("#connection")).toHaveText(
    "the connection to webcam-handler-daemon closed; reload the URL webcam-handler-daemon printed",
  );
  await expect(brightness()).toHaveValue(secondBrightness);
});

test("a refused camera list at startup is a sentence rather than a silence", async ({ page }) => {
  // **docs/11 M33.** `main()` awaited `enumerate()` with no `try`/`catch` and was called with no
  // `.catch`, so a refused `wch_list` became an unhandled rejection: the banner went on reading
  // `connected`, the list stayed empty, D1's "an empty enumeration is diagnosed, not shrugged
  // at" was never reached — the throw happened before the diagnosis — and `main()` never got as
  // far as wiring `#take-photo` up at all. The identical call on the hotplug path **is** wrapped
  // (`watchDevices`), which is what makes this an omission rather than a policy, and the repair
  // gives both callers one sentence rather than a second copy of a sentence.
  //
  // The refusal is arranged on the wire because no fake backend will produce it: `wch_list` over
  // `synthetic_basic` always answers, which is exactly why nothing in this suite had ever seen
  // the failing path.
  const wire = await interposed(page);
  wire.refuse((request) => request.method === "wch_list", {
    code: -32012,
    message: "camera /dev/video0 is gone (unplugged, or its driver unbound)",
    data: { kind: "device_gone", path: "/dev/video0" },
  });
  // **Where the defect actually landed, recorded by the page itself.** The refusal did not go
  // nowhere — it went into an unhandled promise rejection, which is a console entry and nothing
  // else. `#take-photo` is disabled in the markup this daemon ships and was disabled in the
  // defective build too, so an assertion about that button cannot tell the two apart; a page that
  // rejected into nowhere can be told from one that did not, and this is the listener that tells.
  await page.addInitScript(() => {
    window.unhandled = [];
    addEventListener("unhandledrejection", (event) => {
      window.unhandled.push(String(event.reason));
    });
  });
  await page.goto(readyToOpenUrl);

  await expect(page.locator("#connection")).toHaveText(
    "connected; this daemon refused to list its cameras (device_gone: camera /dev/video0 is " +
      "gone (unplugged, or its driver unbound)), so the camera list beside this line is empty " +
      "or stale rather than this machine's",
  );
  await expect(page.locator("#camera-list button[data-camera]")).toHaveCount(0);
  // Asked after the banner rather than before it, because the banner is what establishes that
  // the refusal has been dealt with at all: an empty list of rejections on a page that has not
  // got there yet is a claim about nothing.
  expect(await page.evaluate(() => window.unhandled)).toEqual([]);

  // The positive control: the same page, over the same wire with the refusal withdrawn, lists
  // the cameras this daemon really has. Without it every assertion above is satisfied by a page
  // that never enumerates anything.
  //
  // The count is **derived from the harness's own fixture list** rather than written here as a
  // number. It was `2` until P9a added the 77-control layout camera, and the arm went red for a
  // reason that had nothing to do with what it claims — which is the shape of an assertion that
  // will go red again for the next reason too. What it is actually about is "more than none,
  // and exactly what this daemon serves".
  wire.allow();
  await openClient(page);
  await expect(page.locator("#camera-list button[data-camera]")).toHaveCount(fixtureCameras.length);
});

test("a recording answer in flight is not written under the next camera's picture", async ({
  page,
}) => {
  // **docs/11 L36.** `recording.js`'s `poll()` wrote its answer into `#recording-status` before
  // consulting `view.stopped`, so an answer already in flight when the handle was retired landed
  // afterwards: the previous camera's sentence, under the new camera's picture, in the one
  // element index.html gives a single writer precisely so it cannot be about something else.
  //
  // Driven by handing the module a stub rather than a daemon — `probePanel`'s arrangement, for
  // its reason — because what is being asserted is an *ordering inside one function*, and the
  // only way to make an answer arrive after a `stop()` reliably is to be the thing that answers.
  // The module is imported out of the origin this daemon is already serving, so what runs is the
  // file it ships rather than a copy of it.
  await openClient(page);
  const written = await page.evaluate(async () => {
    const module = await import("./recording.js");
    const node = document.createElement("p");
    // **Written into before the handle exists**, so that the line below is about `stop()` having
    // cleared it. A node created empty is a node that reads `""` in every build there has ever
    // been, and an assertion that holds in the defective build is weight rather than evidence.
    node.textContent = "a recording owns some camera or other";
    document.body.append(node);

    /** One `wch_record_status` answer this test decides the timing of. */
    const answer = { take: { started: 0, elapsed_ms: 1500, budget_ms: 10000 } };
    let deliver = null;
    const inFlight = new Promise((resolve) => {
      deliver = resolve;
    });
    const rpc = { call: () => inFlight };

    const handle = module.watch(rpc, "cam:whatever", node);
    // The retirement, while the first poll's call is still parked on `inFlight`.
    handle.stop();
    const afterStop = node.textContent;
    deliver(answer);
    // Two turns of the microtask queue and no clock at all: `poll`'s continuation was registered
    // on this promise before this line was, so it has already run by the time the second `await`
    // resolves. A `setTimeout` here would be a duration standing in for an ordering.
    await inFlight;
    await Promise.resolve();
    return { afterStop, afterAnswer: node.textContent };
  });
  expect(written.afterStop).toBe("");
  expect(written.afterAnswer).toBe("");

  // The positive control, and it is the whole of the claim's meaning: the same answer, on a
  // handle nobody retired, *is* written. Without it "the element stayed empty" is satisfied by a
  // module that never writes anything.
  const live = await page.evaluate(async () => {
    const module = await import("./recording.js");
    const node = document.createElement("p");
    document.body.append(node);
    let deliver = null;
    const inFlight = new Promise((resolve) => {
      deliver = resolve;
    });
    module.watch({ call: () => inFlight }, "cam:whatever", node);
    deliver({ take: { started: 0, elapsed_ms: 1500, budget_ms: 10000 } });
    await inFlight;
    await Promise.resolve();
    return node.textContent;
  });
  expect(live).toBe(
    "a recording owns this camera — 1.5 s of 10.0 s; the picture is the recording's own frames",
  );
});

test("a socket severed without a FIN stops reading as connected", async ({ page }) => {
  // **docs/11 L38, and the sentence in `rpc.js` that was false.** That module's header claimed
  // that "a socket that closed answers **everything at once**", and it did — for a socket that
  // *closed*. A link cut without a FIN never closes: `readyState` stays `OPEN`, the `close`
  // event never fires, and so N96's H5 repair — the one line that refuses a call on a dead
  // handle — never runs. Every call parks, the banner goes on reading `connected`, and
  // `#photo-status` reads "taking a photo …" until the tab is closed.
  //
  // The owner ruled on 2026-08-16 for a per-call timeout and an idle heartbeat, and this drives
  // the heartbeat, which is the half that answers a page nobody is clicking on. It runs on a
  // clock **this test owns** — Playwright's, installed before the page loads — because the bound
  // is tens of seconds and a rung that waited them out would be a rung nobody runs. That is the
  // same rule the Rust suites keep with `SteppedClock`, one language along.
  await page.clock.install();
  const wire = await interposed(page);
  await openClient(page);
  await expect(page.locator("#preview-status")).toHaveText(`streaming ${cameraId}`);

  // The cut: both sockets stay open and nothing crosses. This is what a dropped link, a NAT
  // table that forgot, or a laptop lid produces — and it is the one failure a page cannot be
  // told about, because there is nothing left to tell it.
  wire.sever();

  // Two heartbeat intervals: the first finds a silent socket and asks it something, the second
  // finds the question unanswered. `runFor` fires the timers in between rather than jumping over
  // them, which is what makes this the mechanism running rather than a number being reached.
  await page.clock.runFor(3 * 15_000);

  await expect(page.locator("#connection")).toHaveText(
    "the connection to webcam-handler-daemon closed; reload the URL webcam-handler-daemon printed",
  );
  // …and everything `socketClosed` owns follows from it, which is the point of ending the socket
  // rather than inventing a second story about a page whose connection has gone.
  await expect(page.locator("#take-photo")).toBeDisabled();
  expect(
    await page.evaluate(() => document.getElementById("preview-frame").hasAttribute("src")),
  ).toBe(false);
  await expect(page.locator("#sweep-status")).toHaveText(
    "the sweep stream ended because the connection to webcam-handler-daemon closed",
  );
});

test("an idle socket is kept by an answer, and a slow call is not a dead socket", async ({
  page,
}) => {
  // **The other three quarters of L38, and the reason they were missing is worth more than the
  // claim.** The claim above severs the link *before* any heartbeat is answered, so it drives the
  // failing path and only that; every other claim in this file keeps `heard` fresh through
  // `recording.js`'s one-second poll, so no claim in this rung had ever seen a heartbeat
  // **answered**. The whole mechanism rested on one sentence in `rpc.js`'s header — that
  // jsonrpsee answers `-32601` for a name it does not have — which this repository asserted for
  // the Unix transport and never for the TCP WebSocket a page opens. If that had been false,
  // every idle tab would have closed its own socket every thirty seconds and read "the connection
  // closed" against a healthy daemon, with this rung green throughout: rubric **A9**'s second
  // half, a claim about a dependency nobody had read.
  //
  // `crates/daemon/tests/web_client.rs` now measures the answer itself. This is the composed
  // half — the timer, the frame and the page all together — and it asserts three things a
  // severed link cannot show: that the heartbeat is answered, that a call which is merely slow is
  // refused as *slow* and leaves the connection alone, and that silence still ends it.
  await page.clock.install();
  const wire = await interposed(page);
  await openClient(page);

  // **The client's own socket is ended first, and that is what makes the rest of this claim
  // askable.** `recording.js` polls once a second for as long as a camera is selected, and the
  // heartbeat deliberately asks nothing while anything else is crossing — that is what makes it
  // cost an ordinary tab a twentieth of the traffic it already makes. So a page showing a camera
  // has no idle socket on it to observe, and every ping counted below would otherwise be
  // ambiguous about which connection sent it.
  wire.close();
  await expect(page.locator("#connection")).toHaveText(
    "the connection to webcam-handler-daemon closed; reload the URL webcam-handler-daemon printed",
  );

  // The idle socket, opened by importing the client's own module out of the origin this daemon is
  // serving — so what runs is `rpc.js` as shipped rather than a copy of it, and the URL is built
  // the way `crates/daemon/tests/http.rs` builds one: from this run's origin and this run's token.
  const socketUrl = `${origin.replace(/^http/, "ws")}/rpc?${new URLSearchParams({ token })}`;
  await page.evaluate(async (url) => {
    const { connect, SOCKET_CLOSED } = await import("./rpc.js");
    window.idle = { closed: false, socketClosed: SOCKET_CLOSED };
    window.idle.handle = await connect(url, {
      onClose: () => {
        window.idle.closed = true;
      },
    });
  }, socketUrl);

  // The positive control: this socket carries an ordinary call. Without it, "it was not closed"
  // is satisfied by a connection that was never usable in the first place.
  expect(await page.evaluate(() => window.idle.handle.call("wch_list"))).toHaveProperty(
    "cameras",
  );

  // **Two intervals per question, and one `runFor` each, with the answer waited for in between.**
  // Two, because the tick at exactly one interval never asks: `heard` is set when the *previous*
  // answer arrived, which is a moment after the interval was created, so that tick lands a few
  // milliseconds inside the window and the one after it is the one that asks. (The claim above
  // spends three intervals to close a socket for the same reason, and it is why a page notices a
  // severed link in under three heartbeats rather than in two.) And stepped, because the clock is
  // fake and inside the page while the answer travels a real socket — one leap across both
  // questions would be asking whether a cross-process round trip fits inside a macrotask, which
  // is a rung that passes when the machine is quick. What is waited on is the proxy's own count of
  // what it forwarded, which is the condition rather than a duration.
  await page.clock.runFor(2 * 15_000);
  await expect.poll(() => wire.answered("wch_ping")).toBeGreaterThanOrEqual(1);
  await page.clock.runFor(2 * 15_000);
  await expect.poll(() => wire.answered("wch_ping")).toBeGreaterThanOrEqual(2);
  // The claim: two heartbeats asked, two answered, and the socket that would have closed on the
  // second unanswered one is still open.
  expect(await page.evaluate(() => window.idle.closed)).toBe(false);

  // **A call nobody answers, on a socket that is still carrying.** Only this method's answers are
  // held, so the heartbeat goes on being answered and the connection goes on being alive — which
  // is the arrangement that separates the two bounds: one is about a call, the other about a
  // link.
  wire.hold((request) => request.method === "wch_list");
  await page.evaluate(() => {
    window.idle.slow = window.idle.handle.call("wch_list").then(
      () => ({ settled: "answered" }),
      (err) => ({
        settled: "refused",
        closed: err.reason === window.idle.socketClosed,
        kind: err.reason?.kind,
        message: err.message,
      }),
    );
  });
  // `fastForward` rather than `runFor`, and it is the honest instrument for this one: it is the
  // laptop lid, firing every due timer **once** at the far end instead of replaying two minutes
  // of them. What is under test here is the call's own bound, so the heartbeats in between are
  // machinery — and replaying them would put eight more round trips inside a fake clock, which is
  // the race the loop above steps around rather than one to walk into.
  await page.clock.fastForward(121_000);
  const slow = await page.evaluate(() => window.idle.slow);
  expect(slow.settled).toBe("refused");
  // The two used to be the same shape — a bare `Error` with no `kind` — and `recording.js`
  // branches on exactly that to decide "a call this page cannot make any more", so one slow
  // answer retired the recording line for the life of the tab (note **N159**).
  expect(slow.closed).toBe(false);
  expect(slow.kind).toBe("call_timed_out");
  expect(slow.message).toContain("did not answer wch_list within 120000 ms");
  expect(await page.evaluate(() => window.idle.closed)).toBe(false);

  // …and the other end of the same mechanism, on the same socket: hold *everything*, and the
  // heartbeat that goes unanswered ends the connection at the following tick. No round trip to
  // wait for here — the answers are the thing being withheld — so the clock may run straight
  // through both.
  wire.hold(() => true);
  await page.clock.runFor(3 * 15_000);
  await expect.poll(() => page.evaluate(() => window.idle.closed)).toBe(true);
});

test("a stale session list is not painted under the camera on screen", async ({ page }) => {
  // **docs/11 M32's third element, which the repair for the control panel did not reach.**
  // `app.js` fences on the camera before it calls `showSessions`, and that drops a *continuation*
  // — it cannot see a `wch_calibrate_list` already on the wire. Two quick camera clicks put two
  // of them there, the daemon spawns a task per inbound message, and the first camera's answer
  // can land last: the session list, the session detail and the calibration status of a camera
  // the operator has left, painted under the camera they are looking at, permanently. Note
  // **N154** claimed this was closed by the fence, on the ground that the list "is read once per
  // selection and has no second reader to reorder against" — it is read once *per selection*, and
  // two selections are two readers.
  //
  // Driven by handing the module stub answers rather than a daemon, which is the arrangement the
  // recording claim above uses and for its reason: what is asserted is an ordering *inside one
  // function*, and being the thing that answers is the only way to decide when an answer arrives.
  // It also lets the two answers differ — this daemon has no sessions for either camera, so two
  // real answers would be identical documents and "whose list is this" would have no observable
  // answer at all.
  await openClient(page);
  const painted = await page.evaluate(async () => {
    const module = await import("./calibration.js");
    const nodes = {
      status: document.createElement("p"),
      list: document.createElement("ul"),
      detail: document.createElement("div"),
    };
    for (const node of Object.values(nodes)) {
      document.body.append(node);
    }

    const answers = [];
    const rpc = {
      call: () => new Promise((resolve, reject) => answers.push({ resolve, reject })),
    };
    const onScreen = {
      sessions: [{ id: "s-on-screen", task_slug: "exposure", path: "/state/sessions/s-on-screen" }],
    };

    /** Two selections, the newer one answered first and the older one settled after it. */
    const round = async (settleStale) => {
      const stale = module.showSessions(rpc, "cam:the-one-left", nodes);
      const current = module.showSessions(rpc, "cam:the-one-on-screen", nodes);
      const [older, newer] = answers.splice(0, 2);
      newer.resolve(onScreen);
      await current;
      const shown = nodes.status.textContent;
      settleStale(older);
      await stale;
      // One turn of the microtask queue and no clock: the stale call's continuation was
      // registered on its promise before this line was, so it has already run.
      await Promise.resolve();
      return {
        shown,
        after: nodes.status.textContent,
        failed: nodes.status.classList.contains("failed"),
        sessions: nodes.list.querySelectorAll("button").length,
      };
    };

    return {
      answered: await round((older) => older.resolve({ sessions: [] })),
      refused: await round((older) =>
        older.reject(
          Object.assign(new Error("camera /dev/video0 is gone"), { kind: "device_gone" }),
        ),
      ),
    };
  });

  // The positive control, and it is the claim's meaning: the answer this view is waiting for *is*
  // painted. Without it, "the stale one changed nothing" is satisfied by a module that paints
  // nothing at all.
  expect(painted.answered.shown).toBe("1 session(s) recorded for this camera");
  expect(painted.answered.after).toBe("1 session(s) recorded for this camera");
  expect(painted.answered.sessions).toBe(1);
  // A stale **refusal** is the same defect wearing its loudest form: a red line about a camera
  // nobody is looking at, over a list that is correct.
  expect(painted.refused.after).toBe("1 session(s) recorded for this camera");
  expect(painted.refused.failed).toBe(false);
  expect(painted.refused.sessions).toBe(1);
});

// ------------------------------------------------------------- the workbench (D20)

test("the preview and the control being adjusted are visible together at every scroll position", async ({
  page,
}) => {
  // Design D20's requirement, stated testably by the design itself and asserted here in the
  // only place it can be: *the preview and the control being adjusted are simultaneously
  // visible at every scroll position of the control column, at the rung's pinned viewport
  // size.* The owner's session at the start of a development run is tuning — eyes on the
  // preview, hands on the controls — and before this shell the page was a single scrolling
  // document 3359px tall at this viewport, whose control panel began below the fold and ran
  // 2395px: adjusting anything at all meant scrolling the picture off the screen (note
  // **N262**).
  //
  // The camera is the **77-control** one on purpose. The ordinary fixture's eighteen very
  // nearly fit on a screen, so a layout claim made against it would pass against a page with
  // no two-pane arrangement at all — which is the fixture-one-parameter-away smell the rubric
  // rejects on sight.
  await openClient(page);
  await page.locator(`button[data-camera="${wideCameraId}"]`).click();
  await expect(page.locator("#controls-status")).toHaveText(/^77 controls/);

  const preview = page.locator("#preview-frame");
  const column = page.locator("#column");

  // The column really does overflow, or everything below is vacuous.
  const overflow = await column.evaluate((el) => el.scrollHeight - el.clientHeight);
  expect(overflow).toBeGreaterThan(400);

  // The document itself does not scroll: the shell is the viewport's height, and the one
  // scroll container on the page is the control column.
  expect(
    await page.evaluate(() => document.documentElement.scrollHeight <= window.innerHeight + 1),
  ).toBe(true);

  const previewBefore = await preview.boundingBox();
  const visible = (box) =>
    box !== null && box.y >= 0 && box.y + box.height <= page.viewportSize().height + 1;
  expect(visible(previewBefore)).toBe(true);

  // Every scroll position, sampled at the granularity a person scrolls at: the picture must
  // not move, and the control under the cursor must be on the screen beside it. A shell that
  // scrolled the document would move the preview on the very first step.
  for (let at = 0; at <= overflow; at += Math.max(1, Math.floor(overflow / 8))) {
    await column.evaluate((el, top) => el.scrollTo(0, top), at);
    const previewNow = await preview.boundingBox();
    expect(previewNow).toEqual(previewBefore);

    // …and something in the column is adjustable *here*, which is what "the control being
    // adjusted" means at this scroll position.
    const adjustable = page.locator("#column .control input, #column .control select");
    const onScreen = await adjustable.evaluateAll(
      (nodes, viewport) =>
        nodes.filter((node) => {
          const box = node.getBoundingClientRect();
          return box.height > 0 && box.top >= 0 && box.bottom <= viewport;
        }).length,
      page.viewportSize().height,
    );
    expect(onScreen).toBeGreaterThan(0);
  }

  // And the last one, explicitly: at the bottom of a 77-control column the picture is still
  // where it was.
  await column.evaluate((el) => el.scrollTo(0, el.scrollHeight));
  expect(await preview.boundingBox()).toEqual(previewBefore);
});

test("a narrow viewport stacks the shell and keeps the preview at the top", async ({ page }) => {
  // D20's other half: *on a viewport too narrow for two panes the shell stacks with the
  // preview sticky at the top*. The same claim, made the only way one column can make it —
  // and the reason it is asserted rather than left to the media query is that a sticky
  // element inside a scroll container that is not the document sticks to nothing, which is a
  // failure that looks exactly like a working page until you scroll.
  await openClient(page);
  await page.locator(`button[data-camera="${wideCameraId}"]`).click();
  await expect(page.locator("#controls-status")).toHaveText(/^77 controls/);
  await page.setViewportSize({ width: 700, height: 800 });

  const preview = page.locator("#preview-frame");
  const before = await preview.boundingBox();
  expect(before).not.toBeNull();

  // The document is the scroll container now, and the pane is stuck to its top.
  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
  const after = await preview.boundingBox();
  expect(after).not.toBeNull();
  expect(after.y).toBeGreaterThanOrEqual(0);
  expect(after.y).toBeLessThan(page.viewportSize().height);
});
