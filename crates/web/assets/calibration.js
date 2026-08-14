// The calibration view: sessions on this machine, one session's state, and sweeps as they
// happen.
//
// ## A view, and deliberately not a console
//
// docs/7 P5c asks for "calibration session view over the subscription", and that is exactly
// the scope: this page reads `wch_calibrate_list`, `wch_calibrate_status` and the
// `wch_subscribe_calibration` stream, and starts nothing. A sweep is minutes of camera time
// and, on a PTZ camera, minutes of motor wear — design §5 puts a plan that would move
// motors behind an explicit decision, and AGENTS says the caps in `schema::limits` bound
// every sweep. A "sweep this control" button on a web page is that decision made by a click
// with no plan attached, so it is not here. What *is* here is the half that has no downside
// and that nothing else in this project offers: watching one.
//
// ## The stream carries every session, and the consumer filters
//
// `wch_subscribe_calibration` takes no parameters. That is not an omission — `crates/api`'s
// `WchEvents` argues it twice over: the subscription is per *client*, a client may watch a
// daemon running more than one session, and a session parameter would resolve a `SessionRef`
// at subscribe time against a store lock the subscribe path has no business taking. So every
// event carries its `session` id and this view shows the id on every line, rather than
// silently dropping the events belonging to sessions it is not looking at: **a sweep somebody
// else started is the case this stream exists for.** An agent driving `webcam-handler-client`
// and an operator watching this page are the shape it was built for, and the daemon suite that
// landed with this client asserts exactly that — the page's socket receives a sweep started on
// the Unix socket.
//
// ## A gap in this stream is self-healing, and that is why the bar can be trusted
//
// A subscriber that falls behind loses events here and is told how many — the opposite of
// the hotplug stream, which is closed instead. The reason is in the payload: `index` and
// `total` ride on every in-flight variant, so the next event repaints a correct bar, and a
// client that connects halfway through a twenty-minute sweep paints a truthful one from the
// first event it sees. This view therefore reads `index`/`total` off each event rather than
// counting events, which is the property that makes it right.
//
// ## Two paths that are not links
//
// A sample's `photo` is relative to its session directory and a `SessionListing::path` is
// absolute in the daemon's state directory (D9) — both on the *daemon's* filesystem, which
// for a browser is a machine it cannot open a file on even when it is the same one.
// `crates/api`'s header says it plainly: "a browser client (P5c) can open neither, and reads
// the documents rather than the files". So they are rendered as text. Turning them into
// links would produce a page full of `file:` URLs that a browser refuses to follow, which is
// worse than a path an operator can copy.

import { el, fill } from "./dom.js";
// The *value*, not the transport: this module still opens no socket and is still handed its
// `rpc` by app.js (app.js's header). `SOCKET_CLOSED` is the vocabulary a subscription's ending
// is spelled in, and importing the sentinel is what lets this view branch on identity rather
// than pattern-match a shape off the wire.
import { SOCKET_CLOSED } from "./rpc.js";

/** How many sweep events the live log keeps. A sweep is minutes; a page is not a journal. */
const LOG_DEPTH = 200;

/**
 * Subscribe to every sweep this daemon runs, and paint the events into `log`.
 *
 * Called once, at startup, rather than per camera: the stream is per client (see the
 * header), and re-subscribing on every camera switch would spend a slot from
 * `limits::RPC_MAX_SUBSCRIPTIONS_PER_CONNECTION` for each one.
 */
export async function watchSweeps(rpc, { status, log }) {
  fill(log, []);
  try {
    await rpc.subscribe(
      "wch_subscribe_calibration",
      (event) => append(log, line(event)),
      (reason) => {
        status.classList.add("failed");
        // The stream ended and said why. `shutting_down` and a failed source are different
        // advice — re-subscribe, or stop waiting — which is the whole reason the daemon
        // spends a notification on it rather than closing the socket.
        //
        // A socket that closed is the other ending, and it is not a frame: stringifying a
        // sentinel at an operator would be this view quoting a notification the daemon never
        // sent. What it says instead is the true thing, and it says it *at all* — until P5e
        // this callback was never reached on that path, so a subscription that had stopped
        // existing left this line reading "watching every sweep this daemon runs", which is the
        // silently-dead view index.html's two status lines were separated to prevent.
        status.textContent =
          reason === SOCKET_CLOSED
            ? "the sweep stream ended because the connection to webcam-handler-daemon closed"
            : `the sweep stream ended: ${JSON.stringify(reason)}`;
      },
    );
    status.classList.remove("failed");
    status.textContent = "watching every sweep this daemon runs";
  } catch (err) {
    status.classList.add("failed");
    status.textContent = `the daemon refused a sweep subscription: ${err.message}`;
  }
}

/**
 * List one camera's sessions, and show whichever one is chosen.
 *
 * `wch_calibrate_list` is the only method on this surface with an optional parameter, and it
 * is passed here because the page is showing one camera. Its answer is built from the
 * directory tree alone — nothing is parsed out of a session document (D9), which is why a
 * session this build cannot read still appears in the list and only fails when it is opened.
 */
export async function showSessions(rpc, camera, nodes) {
  fill(nodes.detail, []);
  try {
    const listing = await rpc.call("wch_calibrate_list", { camera });
    nodes.status.classList.remove("failed");
    nodes.status.textContent =
      listing.sessions.length === 0
        ? "no calibration sessions recorded for this camera"
        : `${listing.sessions.length} session(s) recorded for this camera`;
    fill(
      nodes.list,
      listing.sessions.map((session) =>
        el("li", {}, [
          el(
            "button",
            { type: "button", onclick: () => showSession(rpc, camera, session.id, nodes) },
            `${session.task_slug} · ${session.id}`,
          ),
          " ",
          el("span", { class: "mono note" }, session.path),
        ]),
      ),
    );
  } catch (err) {
    nodes.status.classList.add("failed");
    nodes.status.textContent = `the session list was refused: ${refusal(err)}`;
    fill(nodes.list, []);
  }
}

/** One session's document and its history. */
async function showSession(rpc, camera, id, nodes) {
  try {
    const status = await rpc.call("wch_calibrate_status", {
      camera,
      session: { kind: "id", id },
    });
    fill(nodes.detail, [
      el("p", {}, `${status.session.task} — ${status.session.goal}`),
      controlTable(status.session),
      el("p", { class: "note" }, `${status.log.length} entries in this session's log`),
    ]);
  } catch (err) {
    // A session written by another build is `schema_version_foreign`, which is a typed
    // refusal and never a best-effort parse (D9) — so it is named as itself rather than
    // rendered as "could not load".
    fill(nodes.detail, [el("p", { class: "note surprising" }, refusal(err))]);
  }
}

/** Where every control in a session stands, and what it was sampled at. */
function controlTable(session) {
  const rows = Object.entries(session.controls).map(([slug, control]) =>
    el("tr", {}, [
      el("td", { class: "mono" }, slug),
      el("td", {}, statusSentence(control.status)),
      el("td", {}, `${(control.samples ?? []).length} sample(s)`),
      el("td", { class: "mono note" }, samplesSentence(control.samples ?? [])),
    ]),
  );
  if (rows.length === 0) {
    return el("p", { class: "note" }, "this session has no controls queued yet");
  }
  return el("table", {}, [
    el("thead", {}, el("tr", {}, ["control", "status", "samples", "requested → applied"].map((h) => el("th", {}, h)))),
    el("tbody", {}, rows),
  ]);
}

/**
 * One `schema::session::ControlStatus`, as a sentence.
 *
 * The discriminant is spelled `status` on this enum rather than `kind`, which is the sort of
 * thing a hand-written client finds out by reading the emitted schema and gets wrong by
 * guessing.
 */
function statusSentence(state) {
  switch (state.status) {
    case "untouched":
      return "untouched";
    case "auto_disabled":
      return `automation off (${state.automation.join(", ")}), parked at ${state.parked_value ?? "—"}`;
    case "sweeping":
      return `sweeping ${state.done}/${state.total} (${planSentence(state.plan)})`;
    case "calibrated":
      // The score is optional because a metric may not have produced one, and D8 is
      // explicit that metrics rank rather than decide — so who chose is shown beside it.
      return `calibrated at ${state.value} ±${state.precision}, chosen by ${selectorSentence(state.selector)}${
        state.score === null || state.score === undefined ? "" : `, scoring ${state.score.toFixed(3)}`
      }`;
    case "deferred":
      return `deferred: ${state.reason}`;
    case "blocked":
      return `blocked: ${blockedSentence(state.reason)}`;
    default:
      return `a status this page does not know: ${state.status}`;
  }
}

/** Why a control cannot be calibrated on this device. */
function blockedSentence(reason) {
  switch (reason.kind) {
    case "read_only":
      return "the device reports it read-only [PF:12]";
    case "disabled":
      return "the device reports it disabled";
    case "inactive_without_partner":
      return "it is INACTIVE and no automation partner was found to release it [PF:3]";
    case "not_sweepable":
      return `its type has no ordered range to sweep (${reason.control_type})`;
    case "other":
      return reason.detail;
    default:
      return reason.kind;
  }
}

/** How a sweep chose its values. */
function planSentence(plan) {
  switch (plan.kind) {
    case "all":
      return "every step";
    case "uniform":
      return `every ${plan.step}`;
    case "log":
      return `${plan.points} logarithmic points`;
    case "explicit":
      return `${plan.values.length} named values`;
    default:
      return plan.kind;
  }
}

/** Who chose a value — D8's `metric:<name>`, `agent` or `human`. */
function selectorSentence(selector) {
  return selector.kind === "metric" ? `metric:${selector.name}` : selector.kind;
}

/**
 * The `{requested, applied}` pairs a control was sampled at.
 *
 * Both numbers, always. D3 applies inside a sweep too \[PF:6\], and a sample labelled with a
 * value the camera never held would poison every comparison built on it — so a view that
 * showed only what was asked for would be showing the wrong axis.
 */
function samplesSentence(samples) {
  return samples
    .slice(0, 8)
    .map((sample) => (sample.requested === sample.applied ? `${sample.applied}` : `${sample.requested}→${sample.applied}`))
    .join(" ");
}

/** One live progress event, as a line. */
function line(event) {
  const at = `${event.session.slice(0, 8)}`;
  switch (event.progress) {
    case "sweep_started":
      return `${at} sweep of ${event.control} started: ${event.total} samples (${planSentence(event.plan)})`;
    case "value_set":
      return `${at} ${event.index}/${event.total} ${event.control} := ${
        event.requested === event.applied ? event.applied : `${event.requested} → ${event.applied}`
      }`;
    case "sample_taken":
      return `${at} ${event.index}/${event.total} ${event.control} sampled at ${event.applied}${metrics(event.metrics)}`;
    case "sweep_finished":
      return `${at} sweep of ${event.control} finished after ${event.samples} samples`;
    case "sweep_interrupted":
      // The refusal's D13 discriminant rides on the event precisely so a consumer can
      // branch without parsing prose, and both are shown: the name to act on and the
      // sentence to read.
      return `${at} sweep of ${event.control} stopped after ${event.taken}/${event.total}: ${event.failure} — ${event.detail}`;
    default:
      return `${at} an event this page does not know: ${event.progress}`;
  }
}

/** A sample's metric scores, when a metric produced any. */
function metrics(scores) {
  const entries = Object.entries(scores ?? {});
  if (entries.length === 0) {
    return "";
  }
  return ` · ${entries.map(([name, score]) => `${name} ${score.toFixed(3)}`).join(" ")}`;
}

/** Append one line, keeping the log bounded and scrolled to the newest event. */
function append(log, text) {
  log.append(el("li", { class: "mono" }, text));
  while (log.children.length > LOG_DEPTH) {
    log.firstElementChild.remove();
  }
  log.scrollTop = log.scrollHeight;
}

/** A refusal, with its D13 name when it has one. */
function refusal(err) {
  return err.kind === null || err.kind === undefined ? err.message : `${err.kind}: ${err.message}`;
}
