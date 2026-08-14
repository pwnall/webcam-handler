// The client's composition root: read the credential, open the socket, and wire the four
// views to it.
//
// Everything that is a *decision* lives here and nowhere else — which camera is selected,
// what happens when the socket closes, what a missing token means — and the four modules
// beside it (controls, preview, photo, calibration) take the values they need as arguments.
// That split is the same one design §2.10 asks of the Rust: one home per law, and a module
// that reached for `location` or opened its own socket would be a second answer to a
// question this file already answers.
//
// ## The order the page comes up in, and why it is this order
//
// The token, then the socket, then `wch_list`, then everything else. Each step's failure is a
// different sentence and none of them is "something went wrong": a page with no token is an
// operator who did not use the URL `webcam-handler-daemon` printed, a socket that will not
// open is a wrong token or a stopped daemon, and an empty camera list is a diagnosis the
// daemon itself supplies (D1: "an empty enumeration is diagnosed, not shrugged at").
//
// ## `#connection` has four writers and one rule
//
// The line under the title is this page's single sentence about the daemon, and four places
// write it: the **credential check**, before anything has been attempted; the **connect
// attempt**, which either says `connected` or says how the handshake failed; **`watchDevices`**,
// which describes a *connected* page whose device-change stream or re-enumeration failed; and
// **`socketClosed`**, which says the connection this page was given has ended. Their lifetimes
// are all different — the first is true for the whole run, the last is final — so "whoever
// wrote last" is not a rule, it is an accident, and it has produced a wrong sentence twice.
//
// The rule is that **no writer may make a statement it cannot know, and the last writer that
// still can wins.** The credential check knows only what the URL carried, so the handshake
// failure *extends* its sentence rather than replacing it: a page that presented no token
// cannot be carrying the wrong one, and telling an operator otherwise sends them looking for a
// token they never had (E16 §2 found that overwrite the first time; P5e found the second, in
// the `catch` below). `watchDevices` opens its sentence with the word "connected" and therefore
// declines to write at all once the socket is what ended. And `socketClosed` is final, because
// from that point on the page's own advice is to reload.
//
// ## What this page will not do
//
// It has **no configuration surface**. There is no field for a daemon address, no stored
// token, and nothing written to `localStorage`: the page came from the daemon it is about,
// so its own origin is the only correct answer, and a client that could be pointed
// elsewhere is a client that can be pointed at somebody else's camera by whoever writes the
// query string. It also stores nothing: design §5 is that the preview is "served, never
// stored", and a page that cached a frame would be storage on the operator's disk with a
// picture of their room in it.

import { tokenFromLocation, wireUrl } from "./credential.js";
import { byId, el, fill } from "./dom.js";
import { SOCKET_CLOSED, connect } from "./rpc.js";
import * as controls from "./controls.js";
import * as calibration from "./calibration.js";
import * as photo from "./photo.js";
import * as preview from "./preview.js";

/** Everything the page is showing right now. */
const state = {
  rpc: null,
  token: null,
  camera: null,
  /** The live `<img>`, which `preview.watch` replaces on every camera switch. */
  frame: null,
  /**
   * Whether the socket this page was given is still usable.
   *
   * Not `state.rpc !== null`: the handle outlives its socket by design — it is what refuses
   * every later call — so "there is a handle" and "there is a connection" are two questions,
   * and every decision below that would otherwise have guessed asks this one.
   */
  socketOpen: false,
  /** Whether a photo is in flight, which is one of the three reasons the button is unusable. */
  taking: false,
};

/**
 * What a page with no `?token=` knows about itself before it has tried anything.
 *
 * Two candidates and this page cannot tell them apart — nothing served to it says which cell
 * of D11's matrix the daemon is in (note **N75**: in the token-less cell the gate is *absent*,
 * not permissive). Both are named rather than one guessed at. It is a constant because the
 * failed handshake below has to be able to *extend* it rather than overwrite it, and a second
 * copy of a sentence is a second copy that drifts.
 */
const NO_CREDENTIAL =
  "this page was opened without the ?token= the daemon prints. If webcam-handler-daemon was started " +
  "with --http-insecure-loopback there is no token and the camera routes are open; " +
  "otherwise the socket and the preview will be refused, and the URL webcam-handler-daemon printed is " +
  "the one to open.";

/** The elements index.html promises. Resolved once, so a rename fails loudly and early. */
const nodes = {
  connection: byId("connection"),
  cameras: byId("camera-list"),
  hints: byId("camera-hints"),
  previewFrame: byId("preview-frame"),
  previewStatus: byId("preview-status"),
  takePhoto: byId("take-photo"),
  photoStatus: byId("photo-status"),
  photoFrame: byId("photo-frame"),
  photoReport: byId("photo-report"),
  guarded: byId("guarded"),
  controlsStatus: byId("controls-status"),
  panel: byId("control-panel"),
  calibrationStatus: byId("calibration-status"),
  sessions: byId("session-list"),
  session: byId("session-detail"),
  // The sweep subscription's own line. It was `calibrationStatus` until P5d's browser rung
  // found the two views overwriting each other in the same element — index.html says what
  // that cost — and the fix is two elements rather than one because the two *lifetimes*
  // differ: the session list is re-read per camera and the subscription is opened once.
  sweepStatus: byId("sweep-status"),
  sweeps: byId("sweep-log"),
};

main();

async function main() {
  state.frame = nodes.previewFrame;
  state.token = tokenFromLocation();
  if (state.token === null) {
    say(nodes.connection, NO_CREDENTIAL, true);
  }

  try {
    state.rpc = await connect(wireUrl(state.token), { onClose: socketClosed });
  } catch (err) {
    // A browser does not hand a page the status of a failed WebSocket handshake, so this is
    // honestly two possibilities rather than one (rpc.js's header). Naming the wrong one would
    // be worse than naming both — and *which* two they are depends on what this page
    // presented, which is the branch.
    //
    // On a page opened with no `?token=` the first candidate is false by construction: there
    // is no token here to be the wrong one. Until P5e this line said it anyway, two statements
    // after the accurate sentence above and over the top of it — on the failure this module's
    // own header calls the most likely one, an operator who read the port off a log and opened
    // `/` by hand. So the token-less page keeps its diagnosis and has the handshake appended to
    // it, and the pair of them is what an operator with no token can act on.
    say(
      nodes.connection,
      state.token === null
        ? `${NO_CREDENTIAL} The socket was then refused (${err.message}) — a page that presented ` +
            "no token cannot be carrying the wrong one, so what is left is the gate above or " +
            "nothing listening on this port any more."
        : `${err.message}: either the token this page was opened with is not this run's, or ` +
            "nothing is listening on this port any more.",
      true,
    );
    return;
  }
  state.socketOpen = true;
  say(nodes.connection, "connected");

  await calibration.watchSweeps(state.rpc, {
    status: nodes.sweepStatus,
    log: nodes.sweeps,
  });
  await watchDevices();
  await enumerate();

  nodes.takePhoto.addEventListener("click", takePhoto);
}

/** Ask the daemon what cameras there are, and show what it says about what is missing. */
async function enumerate() {
  const listing = await state.rpc.call("wch_list");
  fill(
    nodes.cameras,
    listing.cameras.map((camera) =>
      el("li", {}, [
        el(
          "button",
          {
            type: "button",
            // The id as an attribute rather than as text a later pass parses back out:
            // a camera's `card` is the device's own string and may contain anything at
            // all, so matching on rendered text would be matching on device data.
            "data-camera": camera.id,
            "aria-pressed": String(camera.id === state.camera),
            onclick: () => select(camera.id),
          },
          [
            camera.card,
            el(
              "span",
              { class: "where mono" },
              `${camera.id} · ${camera.driver} · ${camera.nodes.map((node) => node.path).join(" ")}`,
            ),
          ],
        ),
      ]),
    ),
  );
  // The hints are D1's diagnosis of what is *not* in the list, and they are data rather than
  // prose: `ListHint` crosses the wire as `{kind, subject}` and the sentence a human reads
  // lives in `ListHint::message`, in Rust, where a browser cannot reach it. So this page is
  // a third renderer of the same two findings — the CLI and the daemon are the other two —
  // and the duplication is real rather than hidden.
  //
  // `?? []` because the healthy answer has no `hints` key at all (controls.js's `paint`
  // records the pattern): the field is `skip_serializing_if = "Vec::is_empty"`, so a machine
  // with nothing wrong with it sends a document this line would otherwise iterate as
  // `undefined`.
  fill(
    nodes.hints,
    (listing.hints ?? []).map((hint) => el("li", { class: "note surprising" }, hintSentence(hint))),
  );
  if (listing.cameras.length === 0) {
    nodes.cameras.append(
      el("li", { class: "note" }, "this daemon enumerated no cameras at all"),
    );
  } else if (state.camera === null) {
    await select(listing.cameras[0].id);
  }
}

/** One `schema::report::ListHint`, as a sentence. */
function hintSentence(hint) {
  switch (hint.kind) {
    case "driverless_usb_video_device":
      return `USB device ${hint.subject} presents a video-class interface with no V4L2 driver bound: the camera is plugged in and nothing is driving it [PF:14]`;
    case "node_unreadable":
      // The hint exists precisely so this is not read as "the camera cannot capture" —
      // availability is not capability (design E3, AGENTS rule 7).
      return `${hint.subject} could not be read, so the camera it belongs to is not listed; that is a fact about access to the node, not about what the camera can do`;
    default:
      return `${hint.kind}: ${hint.subject}`;
  }
}

/** Show one camera: its controls, its preview, its photo button and its sessions. */
async function select(camera) {
  state.camera = camera;
  controls.forgetOutcomes();
  for (const button of nodes.cameras.querySelectorAll("button[data-camera]")) {
    button.setAttribute("aria-pressed", String(button.dataset.camera === camera));
  }
  // The preview is repointed *before* anything slow, because leaving the old element
  // streaming would hold a second camera open for as long as the panel took to paint
  // (preview.js: the daemon retires a feed when its last reader goes).
  //
  // …and it is not repointed at all once the socket has gone. The preview is a *separate* HTTP
  // request whose token is as good as it ever was, so it would paint — which is exactly the
  // trap: `socketClosed` ended this feed deliberately, and a page that answered a click with
  // live video under a banner saying the connection is gone would be contradicting itself in
  // two elements at once. The two routes are independent (D11 gates both separately); this
  // page's story about them is not.
  if (state.socketOpen) {
    state.frame = preview.watch(state.frame, nodes.previewStatus, camera, state.token);
  }
  refreshTakePhoto();
  nodes.photoFrame.removeAttribute("src");
  fill(nodes.photoReport, []);
  nodes.photoStatus.textContent = "";
  await refreshControls();
  await calibration.showSessions(state.rpc, camera, {
    status: nodes.calibrationStatus,
    list: nodes.sessions,
    detail: nodes.session,
  });
}

/** Re-read the control report and repaint the panel from it. */
async function refreshControls() {
  try {
    const report = await state.rpc.call("wch_controls", { camera: state.camera });
    say(
      nodes.controlsStatus,
      `${report.controls.length} controls, ${(report.pairs ?? []).length} automation pair(s)`,
    );
    controls.paint(nodes.panel, report, { write, refresh: refreshControls });
  } catch (err) {
    say(nodes.controlsStatus, refusalSentence(err), true);
    fill(nodes.panel, []);
  }
}

/**
 * One `wch_set`, with D3's guard as the operator left it.
 *
 * `guarded` is a checkbox rather than a constant because the two answers are different
 * operations, not a preference. Guarded means the daemon *plans* the write: an automation
 * partner is switched off first and the report says which. Unguarded means the write is sent
 * as asked — and, measured rather than assumed, an unguarded write to an INACTIVE control is
 * **performed** rather than refused: `engine::pairing` takes the unguarded path before it
 * looks at the flag at all, so the value is written to a control something else is still
 * driving. (`ControlInactive` is the *guarded* path's refusal, for a control with no partner
 * to release \[PF:3\].) Hiding the choice would hide which of those an operator is doing,
 * which is the difference between changing an exposure and asking a camera to ignore one.
 */
function write(control, value) {
  return state.rpc.call("wch_set", {
    camera: state.camera,
    writes: [{ control, value }],
    guarded: nodes.guarded.checked,
  });
}

/** Take one photo of the selected camera. The preview is not touched — note **N83**. */
async function takePhoto() {
  state.taking = true;
  refreshTakePhoto();
  try {
    await photo.take(state.rpc, state.camera, {
      status: nodes.photoStatus,
      frame: nodes.photoFrame,
      report: nodes.photoReport,
    });
  } finally {
    state.taking = false;
    refreshTakePhoto();
  }
}

/**
 * The one owner of `#take-photo`'s `disabled`, and the reason there is exactly one.
 *
 * Three independent things make that button unusable — no camera has been chosen yet, a photo
 * is already in flight, and the socket this page was given has closed — and they arrive from
 * three directions, so with a flag set from four places the last write decided. `select` was
 * the one that got it wrong: it enabled the button unconditionally, so a click on a camera
 * after the socket died handed an operator a button whose call could never be answered, and
 * `#photo-status` would then have read "taking a photo …" for as long as the tab lived. One
 * function that computes the flag from all three cannot disagree with itself. index.html ships
 * the attribute *set*, so the button is unusable until this says otherwise rather than the
 * other way round.
 */
function refreshTakePhoto() {
  nodes.takePhoto.disabled = !state.socketOpen || state.camera === null || state.taking;
}

/**
 * Keep the camera list live off the hotplug stream.
 *
 * The two subscriptions this daemon offers end differently on purpose, and this is the one
 * that is **closed rather than resynced**: a `HotplugEvent` is a delta, a gap makes a
 * consumer's picture of the node tree wrong in a way it cannot detect, and the vocabulary
 * has no variant meaning "you missed some" (`crates/api`'s `WchEvents`). The documented
 * answer is to re-subscribe and re-enumerate, so that is what happens here — once, so a
 * daemon that keeps ending the stream does not become a page that keeps calling `wch_list`.
 *
 * A refusal is **not** fatal and is not silent. The watch is opened on the first
 * subscription rather than at startup precisely so a host where the uevent socket cannot be
 * opened — a container, or an LSM \[PF:21\] — still enumerates perfectly; a page that gave
 * up here would be converting "this daemon cannot watch for changes" into "this daemon has
 * no cameras", which is the conversion E3 exists to prevent.
 */
async function watchDevices(retry = true) {
  try {
    await state.rpc.subscribe(
      "wch_subscribe_events",
      () => {
        // The event names a *node*, never a camera — grouping is not a node property — so
        // the answer to any of them is to enumerate again rather than to patch the list.
        enumerate().catch((err) => say(nodes.connection, refusalSentence(err), true));
      },
      (reason) => {
        if (reason === SOCKET_CLOSED) {
          // Nothing about the *stream* ended: the connection under it did, and `socketClosed`
          // owns the one sentence that covers every subscription on it. Re-subscribing would
          // be a call on a dead handle — refused at once since P5e, which is the change that
          // made this branch reachable at all — and that refusal would land in `#connection` as
          // "connected; this daemon cannot watch for device changes …", a sentence that opens
          // by contradicting the one already there. This is the header's ownership rule doing
          // its one job: a writer that cannot know what it is about to claim does not write.
          return;
        }
        if (retry) {
          watchDevices(false);
        } else {
          say(nodes.connection, "connected; the device-change stream ended twice and the camera list is no longer live", true);
        }
      },
    );
  } catch (err) {
    say(
      nodes.connection,
      `connected; this daemon cannot watch for device changes (${refusalSentence(err)}), so the camera list is a snapshot`,
      true,
    );
  }
}

/** What a closed socket means for a page that is still on screen. */
function socketClosed() {
  // First, because every decision after it reads this: the handle is still here and still
  // answers, and what it answers is a refusal (rpc.js).
  state.socketOpen = false;
  say(nodes.connection, "the connection to webcam-handler-daemon closed; reload the URL webcam-handler-daemon printed", true);
  refreshTakePhoto();
  // The preview is a *separate* HTTP request and does not end with the socket, so it is
  // ended here rather than left painting frames from a daemon this page can no longer ask
  // anything about.
  state.frame = preview.stop(state.frame, nodes.previewStatus);
}

/** A refusal with its D13 name when it has one, and its message when it does not. */
function refusalSentence(err) {
  return err.kind === null || err.kind === undefined ? err.message : `${err.kind}: ${err.message}`;
}

/** Write one status line, marking it as a failure or not. */
function say(node, text, failed = false) {
  node.classList.toggle("failed", failed);
  node.textContent = text;
}
