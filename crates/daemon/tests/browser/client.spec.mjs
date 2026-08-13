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
// ## Nothing here sleeps, and nothing asserts a pixel
//
// Every wait is Playwright auto-waiting on a condition the daemon or the browser causes — an
// element's text, an attribute, an image whose mean luma has moved. The suite's only timeout
// is the config's, and it exists so a condition that never happens is a named failure instead
// of a rung that never finishes. Where a frame is examined at all it is examined for an
// *ordering* the fake declares (`fake::frames`: brightness is a monotonic gain), never for a
// value, which is the same rule the hardware suites run under.

import { expect, test } from "@playwright/test";

import { cameraId, control, moduleCount, openClient, origin, token } from "./harness.mjs";

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

test("the calibration view tracks a sweep it did not start", async ({ page }) => {
  // **The preview is kept off this page, and the reason is a finding rather than a
  // convenience.** A sweep and a live preview on the same camera collide: `engine::preview::
  // while_suspended` is what lets a *photo* interleave with a preview (note N83) and
  // `engine::photo::take` is its only caller, so `wch_calibrate_sweep` against a camera this
  // page is previewing is refused `Busy` — measured here, at 2026-08-13, by this rung failing
  // that way. That is the deployment's own arrangement (the owner watches the client *while*
  // calibrating from `wchc`), so it is recorded in this sub-milestone's report rather than
  // worked around silently; what is worked around is only this claim's need for a free camera.
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
  await expect(log.first()).toHaveText(
    `${short} sweep of brightness started: 2 samples (2 named values)`,
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
  // nothing. The sentence is the honest one: a browser does not hand a page the status of a
  // failed WebSocket handshake, so the page names both candidates instead of guessing.
  await expect(page.locator("#connection")).toHaveClass(/failed/);
  await expect(page.locator("#connection")).toHaveText(
    "wchd did not accept a WebSocket: either the token this page was opened with is not this " +
      "run's, or nothing is listening on this port any more.",
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

test("the page reports a lost socket and works again on the next one", async ({ page }) => {
  // **How the socket is cut, and why it is cut this way.** What is being asserted is `rpc.js`'s
  // `close` handler and `app.js`'s answer to it — the path a stopped daemon or a dropped link
  // takes — and a test that closed the socket politely from inside the page would reach neither.
  // `context.setOffline(true)` was tried first and does not close an established WebSocket in
  // this Chromium, which is worth recording because it looks like it should. So the connection
  // is proxied and then dropped from the middle: the page's socket really ends while the daemon
  // is still up and still holding its side, which is the arrangement a lost link produces.
  let dropped = null;
  await page.routeWebSocket((url) => url.pathname === "/rpc", (socket) => {
    socket.connectToServer();
    dropped = socket;
  });

  await openClient(page);
  await expect(page.locator("#preview-status")).toHaveText(`streaming ${cameraId}`);

  dropped.close();
  await expect(page.locator("#connection")).toHaveText(
    "the connection to wchd closed; reload the URL wchd printed",
  );
  await expect(page.locator("#take-photo")).toBeDisabled();
  // The preview is a *separate* HTTP request and would otherwise keep painting frames from a
  // daemon this page can no longer ask anything about, so it is ended too — and ending it is
  // how a browser tells the daemon its last reader has gone.
  expect(
    await page.evaluate(() => document.getElementById("preview-frame").hasAttribute("src")),
  ).toBe(false);

  // And the reconnect, which is the client's own advice ("reload the URL wchd printed") taken
  // literally: the same token opens a second socket on the same daemon, and a full round-trip
  // over it repaints the panel from the device.
  await page.unrouteAll();
  await openClient(page);
  await expect(page.locator("#preview-status")).toHaveText(`streaming ${cameraId}`);
  await expect(control(page, "auto_exposure").locator("select")).toHaveValue("3");
});
