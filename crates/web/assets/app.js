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
// ## `#connection` has five writers and one rule
//
// The line under the title is this page's single sentence about the daemon, and five places
// write it: the **credential check**, before anything has been attempted; the **connect
// attempt**, which either says `connected` or says how the handshake failed; **`watchDevices`**,
// which describes a *connected* page whose device-change stream failed; **`listRefused`**, which
// describes a connected page whose camera list is not this machine's; and **`socketClosed`**,
// which says the connection this page was given has ended. Their lifetimes are all different —
// the first is true for the whole run, the last is final — so "whoever wrote last" is not a
// rule, it is an accident, and it has produced a wrong sentence twice.
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
import * as flow from "./calibrate-flow.js";
import * as photo from "./photo.js";
import * as preview from "./preview.js";
import * as recording from "./recording.js";

/** Everything the page is showing right now. */
const state = {
  rpc: null,
  token: null,
  camera: null,
  /**
   * Which `wch_controls` the panel is waiting for, counted so a late answer can be told it is
   * late.
   *
   * **Not the camera, and the difference is the whole of the fence.** An answer knows which
   * camera it was asked about, so comparing cameras catches the ordinary case — an answer about
   * the camera the operator has walked away from. It does not catch two answers about the *same*
   * camera arriving out of order, which this daemon can produce on its own: `daemon::http::rpc`
   * spawns a task per inbound WS message, so a repaint asked for after a write and the one asked
   * for by the next write are two tasks, and the wire imposes no order on their answers. A
   * number that only goes up asks both cases the one question that covers them — *is this the
   * answer the panel is still waiting for?* — and an answer nobody is waiting for is dropped
   * rather than painted (docs/11 **M32**, note **N154**).
   */
  controlsRequest: 0,
  /** The live `<img>`, which `preview.watch` replaces on every camera switch. */
  frame: null,
  /**
   * The poll loop behind `#recording-status`, or `null` while nothing is being previewed.
   *
   * Held here for `state.frame`'s reason: it belongs to the camera on screen, so the camera
   * switch that repoints the `<img>` has to end this too — a loop left asking about the
   * previous camera is a page reporting a take nobody is watching, in a line beside a picture
   * of something else.
   */
  recording: null,
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
  // The recording line, whose one writer is recording.js — see index.html for why it is not
  // the caption above.
  recordingStatus: byId("recording-status"),
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
  // The human-driven flow's own controls (D20). One node table, so a renamed id is one
  // failure at startup rather than a `null` reached minutes later on a click.
  flow: byId("flow"),
  flowTask: byId("flow-task"),
  flowGoal: byId("flow-goal"),
  flowSamples: byId("flow-samples"),
  flowStart: byId("flow-start"),
  flowPlan: byId("flow-plan"),
  flowSweep: byId("flow-sweep"),
  flowApply: byId("flow-apply"),
  flowRestore: byId("flow-restore"),
  flowStatus: byId("flow-status"),
  flowGrid: byId("flow-grid"),
};

/// The flow's node table in the shape `calibrate-flow.js` names them.
const flowNodes = () => ({
  flow: nodes.flow,
  task: nodes.flowTask,
  goal: nodes.flowGoal,
  samples: nodes.flowSamples,
  start: nodes.flowStart,
  plan: nodes.flowPlan,
  sweep: nodes.flowSweep,
  apply: nodes.flowApply,
  restore: nodes.flowRestore,
  status: nodes.flowStatus,
  grid: nodes.flowGrid,
});

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
  // The flow borrows the preview for the length of a sweep — a sweep is minutes of exclusive
  // capture and whichever streaming operation asks second meets `Busy` (note N83, E16), so
  // the page stands its own preview down rather than racing itself. The two functions it is
  // handed are this module's, because the `<img>` belongs to the shell.
  flow.mount(state.rpc, flowNodes(), {
    // `preview.watch` and `preview.stop` *replace* the element — see preview.js on why a
    // torn-down `<img>` is a new one — so the pair below goes through `state.frame` exactly
    // as every other caller does. A flow that held its own reference would end a feed nobody
    // was watching and leave the one on screen running.
    stop: () => {
      state.frame = preview.stop(state.frame, nodes.previewStatus);
    },
    start: () => {
      if (state.camera !== null && state.socketOpen) {
        state.frame = preview.watch(state.frame, nodes.previewStatus, state.camera, state.token);
      }
    },
  });
  await watchDevices();
  // **Caught, because the alternative is a page that says nothing at all.** `enumerate` is one
  // `wch_list` and a refused one used to leave this function as an unhandled rejection: the
  // banner went on reading `connected`, the camera list stayed empty with no sentence beside it,
  // and D1's "an empty enumeration is diagnosed, not shrugged at" was never reached because the
  // throw happened before the diagnosis. The line after this one never ran either, so a page
  // that later recovered on a hotplug event had a photo button nothing had wired up. The same
  // call on the hotplug path was already wrapped, which is what made this an omission rather
  // than a policy (docs/11 **M33**, note **N155**).
  //
  // Not fatal, for `watchDevices`'s reason: the device-change stream is live, so a machine whose
  // enumeration failed once re-enumerates the next time anything is plugged in, and a page that
  // returned here would have thrown that away.
  await enumerate().catch(listRefused);

  nodes.takePhoto.addEventListener("click", takePhoto);
}

/**
 * What a refused `wch_list` means, in the one place both of its callers say it.
 *
 * Two callers, one fact: the startup enumeration and the re-enumeration a hotplug event asks
 * for. They differ only in what is left on screen — nothing yet, or a list that has stopped
 * being this machine's — and one sentence covers both because "the list is not what is plugged
 * in" is the part an operator can act on either way. Two sentences would be two copies that
 * drift, which is the shape `#connection` has already been wrong in twice.
 *
 * It opens with `connected`, and that is the ownership rule this module's header states rather
 * than a flourish: the socket really is up, this is a statement about a *call*, and a writer
 * that dropped the word would be claiming something about the connection it has no evidence
 * for. `socketClosed` is the writer that may say the connection has ended, and it is final.
 */
function listRefused(err) {
  say(
    nodes.connection,
    `connected; this daemon refused to list its cameras (${refusalSentence(err)}), so the ` +
      "camera list beside this line is empty or stale rather than this machine's",
    true,
  );
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

/**
 * Show one camera: its controls, its preview, its photo button and its sessions.
 *
 * **Everything on screen that belongs to a camera is taken down before anything slow starts.**
 * The control panel used to be the exception and it was the expensive one: `state.camera` moved
 * synchronously, `write()` reads it at *send* time, and the panel was left standing for the
 * whole `wch_controls` round trip — a device open and a control walk, minutes if a sweep is in
 * front of that actor. So every widget on screen belonged to the previous camera while a click
 * on one wrote to the new one, which is a control panel with no camera identity at all (docs/11
 * **M32**, note **N154**). Two things close it: the panel is emptied here, in the same task as
 * the click, so there is nothing left to misread or to click on; and the widgets the answer
 * paints are bound to the camera **that answer was about**, so a click can only ever reach the
 * device its own card came from.
 */
async function select(camera) {
  state.camera = camera;
  controls.forgetOutcomes();
  // A session does not follow a camera: `calibrate_apply`'s fingerprint check is what makes
  // that a refusal rather than a surprise (D8), and the page says the same thing by forgetting
  // which session it was driving the moment the operator looks at another device.
  flow.watching(camera, flowNodes());
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
    // Started beside the preview and ended with it, because the sentence is *about* the
    // picture: since the owner's ruling of 2026-08-14 (note **N117**) a recording feeds this
    // `<img>` its own frames, and a take D7 records raw feeds it nothing at all — so the one
    // thing a viewer cannot work out for themselves is that a recording is what they are
    // looking at, or waiting on.
    state.recording?.stop();
    state.recording = recording.watch(state.rpc, camera, nodes.recordingStatus);
  }
  refreshTakePhoto();
  nodes.photoFrame.removeAttribute("src");
  fill(nodes.photoReport, []);
  nodes.photoStatus.textContent = "";
  // The panel comes down now rather than when its replacement arrives, and the line above it
  // says which camera is being read — so the window between the click and the answer shows an
  // empty panel with a sentence in it instead of another camera's controls with nothing to say
  // they are another camera's. An empty panel is also the only honest thing to show: this page
  // has not been told what this camera's controls are yet.
  fill(nodes.panel, []);
  say(nodes.controlsStatus, `reading ${camera}'s controls…`);
  await refreshControls();
  // Is this still the camera the page is showing? A `select` whose `wch_controls` was answered
  // after the operator moved on would otherwise go on to *start* a session list for the camera
  // they left, in three elements that say nothing about which camera they are about.
  //
  // It is the same fence the panel gets and it closes the same half — a continuation that should
  // not run. The other half is not here and cannot be: a `wch_calibrate_list` already on the wire
  // when the operator clicks is an answer this function has no way to reach, so `calibration.js`
  // numbers its own reads and drops the ones a newer view has retired (note **N154**).
  if (state.camera !== camera) {
    return;
  }
  await calibration.showSessions(state.rpc, camera, {
    status: nodes.calibrationStatus,
    list: nodes.sessions,
    detail: nodes.session,
  });
}

/**
 * Re-read the control report and repaint the panel from it.
 *
 * **The answer is only painted if it is still the answer this panel is waiting for.** The
 * camera is captured here rather than read again at the end, and the request is numbered, so
 * both ways a late answer arrives are one comparison: an answer about a camera the operator has
 * walked away from, and an answer about *this* camera that overtook a newer one because the
 * daemon spawns a task per inbound message. A late answer is dropped in silence and deliberately
 * so — it is not a failure and there is nothing to tell anybody about it, and the sentence in
 * `#controls-status` belongs to the request that is still running.
 */
async function refreshControls() {
  const request = ++state.controlsRequest;
  // The camera this report will be *about*, held rather than re-read: it is what the widgets
  // below write to, so a card painted from this answer cannot reach a device the answer was
  // never about, however long the round trip took and whatever the operator clicked meanwhile.
  const camera = state.camera;
  try {
    const report = await state.rpc.call("wch_controls", { camera });
    if (request !== state.controlsRequest) {
      return;
    }
    say(
      nodes.controlsStatus,
      `${report.controls.length} controls, ${(report.pairs ?? []).length} automation pair(s)`,
    );
    controls.paint(nodes.panel, report, {
      write: (control, value) => write(camera, control, value),
      refresh: refreshControls,
    });
  } catch (err) {
    if (request !== state.controlsRequest) {
      return;
    }
    say(nodes.controlsStatus, refusalSentence(err), true);
    fill(nodes.panel, []);
  }
}

/**
 * One `wch_set` on `camera`, with D3's guard as the operator left it.
 *
 * The camera is an argument rather than a read of `state.camera`, and that is the repair rather
 * than a tidy-up: a widget belongs to the report it was painted from, so the device it writes to
 * is the device that report described. Reading the selection at *send* time made a click's
 * target depend on what the operator had done since the panel was painted, which is how a
 * brightness meant for one camera arrived at another (docs/11 **M32**).
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
function write(camera, control, value) {
  return state.rpc.call("wch_set", {
    camera,
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
        enumerate().catch(listRefused);
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
  // Before the preview is torn down and for the same reason: this line is written from calls on
  // a socket that has ended, and the sentence above is the one that covers every one of them.
  state.recording?.stop();
  state.recording = null;
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
