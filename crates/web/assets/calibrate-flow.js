// Driving a calibration session from the page (design D20).
//
// D8 reserved `selector: human` for a reviewer with eyes on the photographs, and until now
// nothing produced one: the CLI flow is the agent's, and P5's page could only *watch* a sweep
// somebody else had started. This module is that producer — start, plan, sweep, review the
// samples, pick one, apply, and put the camera back — and it is worth being precise about what
// it is not.
//
// **It is not a second state machine.** Every transition below is one of the eight
// `wch_calibrate_*` verbs the daemon already has, and the daemon's own refusals are this
// flow's guard rails: an out-of-order click meets `illegal_transition` and the page renders
// what the daemon said. The alternative — mirroring the state machine here so the page can
// grey out the wrong button — is a second copy of D8's rules that would drift, and would make
// the page's *belief* about a session the thing an operator trusts. What the page knows about
// a session comes from `wch_calibrate_status`, which is the document on disk.
//
// **It is not a session editor.** Task, goal and criteria are set at start; free-text notes
// stay CLI-side (D20's "what the page still is not").
//
// ## The sweep/preview collision, resolved by the page
//
// A sweep is minutes of exclusive capture, and D12 deliberately leaves it outside the suspend
// mechanism — so whichever streaming operation asks second meets `Busy` (note **N83**; E16
// measured it). The page therefore **ends its own preview before `calibrate_sweep`**, and
// during the sweep the preview pane *becomes* the sweep: progress from the subscription, and
// the freshest sample rendered through `/session-photo` as each one lands. That is truer than
// a live preview would be, not merely cheaper: a live view during a sweep shows the settle
// transients between samples, and what the operator is judging is the samples.

import { samplePhotoUrl } from "./credential.js";
import { el, fill } from "./dom.js";

/** How many samples the grid shows at once before it scrolls. Layout only. */
const GRID_COLUMNS = 4;

/**
 * Photographs per sweep, unless the operator says otherwise.
 *
 * Eight is a number an operator can look at in one screen and a click can pay for in seconds,
 * which are the two constraints a *button* has and a command line does not.
 */
const DEFAULT_SAMPLES = 8;

/**
 * The flow's own view of where it is.
 *
 * Deliberately thin — a label for the buttons and nothing a refusal could contradict. The
 * session's real state is whatever `wch_calibrate_status` last said, and this only records
 * which session the page is driving and which control it is reviewing.
 */
const flow = {
  camera: null,
  session: null,
  reviewing: null,
  status: null,
  sweeping: false,
  /// Controls this page asked for and was refused, so the queue can be got past.
  ///
  /// **Not a second state machine and not a skip**: the session document still has these
  /// controls queued and untouched, and `calibrate status` on any other surface says so. What
  /// this remembers is only that *this page* has already been told no — which is what stops
  /// "Sweep next" handing the operator the same refusal forever. A motorized control is the
  /// case that forced it: §5 says a plan that would move motors says so first, the page sends
  /// no `allow_motion` and therefore must not, and without this the owner's own PTZ camera
  /// wedges the flow at the thirteenth control in the queue.
  refused: new Set(),
};

/**
 * Wire the flow's controls to a socket.
 *
 * `preview` is the pair of functions that own the `<img>` — the page's, not this module's,
 * because the preview belongs to the shell and this is a view that borrows it for the length
 * of a sweep.
 */
export function mount(rpc, nodes, preview) {
  nodes.start.addEventListener("click", () => run(nodes, () => start(rpc, nodes)));
  nodes.plan.addEventListener("click", () => run(nodes, () => plan(rpc, nodes)));
  nodes.sweep.addEventListener("click", () => run(nodes, () => sweep(rpc, nodes, preview)));
  nodes.apply.addEventListener("click", () => run(nodes, () => apply(rpc, nodes)));
  nodes.restore.addEventListener("click", () => run(nodes, () => restore(rpc, nodes)));
  paint(nodes);
}

/** The camera the flow drives. Called on every switch; a session does not follow a camera. */
export function watching(camera, nodes) {
  flow.camera = camera;
  flow.session = null;
  flow.reviewing = null;
  flow.status = null;
  flow.refused.clear();
  fill(nodes.grid, []);
  paint(nodes);
}

/**
 * Run one step, and render whatever it answers — including a refusal.
 *
 * Every button goes through here, so the page has exactly one place where a D13 refusal
 * becomes a sentence on screen. The message is the daemon's own: an `IllegalTransition`
 * carries the instruction-last template D13 specifies (note **N212**), and rewording it here
 * would make the page's advice differ from the CLI's about the same session.
 */
async function run(nodes, step) {
  nodes.status.classList.remove("failed");
  try {
    await step();
  } catch (err) {
    nodes.status.classList.add("failed");
    // `err.kind` is the D13 discriminant the wire carries in `data.kind`, and it is shown
    // beside the message rather than instead of it: the kind is what a reader branches on and
    // the message is what tells them which control and which session.
    nodes.status.textContent = err.kind ? `${err.kind}: ${err.message}` : err.message;
  }
  paint(nodes);
}

async function start(rpc, nodes) {
  const task = nodes.task.value.trim();
  if (task === "") {
    // The one refusal this module makes on its own, and it is about a *form field* rather
    // than about a session: an empty task would reach the daemon as a request for a session
    // named by nothing, and the page can say so without a round trip. Everything else is the
    // daemon's to refuse.
    throw new Error("a session needs a task — what is this camera being calibrated for?");
  }
  const session = await rpc.call("wch_calibrate_start", {
    camera: flow.camera,
    task,
    goal: nodes.goal.value.trim(),
    criteria: [],
  });
  flow.session = { kind: "id", id: session.id };
  flow.reviewing = null;
  await refresh(rpc, nodes);
  nodes.status.textContent = `session ${session.id} started for ${task}`;
}

async function plan(rpc, nodes) {
  requireSession();
  const planned = await rpc.call("wch_calibrate_plan", {
    camera: flow.camera,
    session: flow.session,
    // Empty means *every control the camera has*, which is what an operator who clicked
    // "Plan" asked for; naming a subset is the CLI's flag and a form field this page
    // deliberately does not have.
    controls: [],
    // **Additions, not a reordering.** `order: true` treats the named controls as the queue's
    // new order — so an empty list with `order: true` is a request to reorder the queue to
    // nothing, which the daemon accepts and which plans exactly no controls. Found by driving
    // it: the page reported "planned" and the next click had nothing to sweep.
    order: false,
  });
  await refresh(rpc, nodes);
  // `queue` is omitted from the document when it is empty (`skip_serializing_if`), so this
  // reads a length off a key that may not be there — and a plan that queued nothing is a
  // sentence an operator needs rather than a crash.
  const queued = planned.queue?.length ?? 0;
  nodes.status.textContent =
    queued === 0
      ? "planned nothing — this camera has no control this build knows how to sweep"
      : `planned ${queued} control(s)`;
}

/**
 * Sweep the next control in the queue, with the preview stood down for the duration.
 *
 * The order matters and is the whole of N83's boundary: the preview is ended **before** the
 * request goes out, because a sweep arriving while this page holds the stream is refused with
 * `Busy` — which E16 measured over a second socket and recorded as a design question. This is
 * the answer.
 */
async function sweep(rpc, nodes, preview) {
  requireSession();
  const control = nextControl();
  if (control === null) {
    throw new Error(
      flow.refused.size === 0
        ? "every planned control has been swept — apply, or plan again"
        : `every control this page can sweep has been swept; ${[...flow.refused].join(", ")} ` +
          "was refused and is still queued — the command-line surface can sweep it, and a " +
          "motorized one needs --allow-motion",
    );
  }

  // How many photographs this click is worth, converted to the stride the planner takes.
  // **A sweep is minutes** — D8 says so and D20 builds the sweep-time pane around it — and
  // `SweepSpec::All` means every step from min to max, which on an ordinary 0..255 control is
  // 256 photographs. That is the right default for `webcam-handler-cli`, where somebody typed
  // a command and can wait; it is the wrong default for a button, where the cost has to be
  // visible before the click. So the operator sets a budget and the page turns it into a
  // stride, which is the same arithmetic `--samples` would do one surface over.
  const budget = Math.max(2, Number(nodes.samples.value) || DEFAULT_SAMPLES);
  const span = await rangeOf(rpc, control);
  const step = span === null ? 1 : Math.max(1, Math.floor(span / (budget - 1)));

  preview.stop();
  flow.sweeping = true;
  flow.reviewing = control;
  paint(nodes);
  nodes.status.textContent = `sweeping ${control} in ${budget} step(s)…`;
  try {
    await sweepOnceThePreviewIsGone(rpc, nodes, {
      camera: flow.camera,
      session: flow.session,
      request: { control, plan: { kind: "uniform", step } },
    });
  } catch (refusal) {
    // Remembered, so the next click offers the *next* control rather than this one again.
    // The refusal itself is not swallowed: it goes up to `run`, which prints the daemon's own
    // words — a motorized control's `illegal_transition` says `motion(allow_motion=false)`,
    // which is the sentence that tells an operator to use the command line for it.
    flow.refused.add(control);
    throw refusal;
  } finally {
    flow.sweeping = false;
    // The preview comes back whatever happened, including a refusal: a page that left the
    // pane blank after a `Busy` would have taken the operator's camera away to no purpose.
    preview.start();
  }
  await refresh(rpc, nodes);
  nodes.status.textContent = `${control} swept — pick the sample that looks right`;
}

/**
 * How long to wait for this page's own preview to let go of the camera, and how often to ask.
 *
 * **Ending the preview is not the same as the daemon knowing it ended.** `preview.stop()`
 * removes the `<img>`'s `src`, which aborts the request; the daemon retires a feed when its
 * last reader goes, and "goes" is the socket closing — so for a few milliseconds after the
 * click this page is still, truthfully, streaming the camera it is about to sweep. The sweep
 * that arrives in that window is refused `Busy`, which is correct (D12 leaves a sweep outside
 * the suspend mechanism, note **N83**) and is exactly the collision E16 measured.
 *
 * So the page waits for its own preview to drain, with a stated bound and a sentence on screen
 * while it does. It waits **only** for a `Busy` that names *this process* — D13's
 * `Occupation` is what makes that distinguishable — because a camera another process holds is
 * not a camera this wait can do anything about, and retrying at it would be a page hiding a
 * refusal an operator needs to read.
 */
const PREVIEW_RELEASE_TRIES = 20;
const PREVIEW_RELEASE_INTERVAL_MS = 100;

/** Send the sweep, waiting out this page's own preview if it is still holding the camera. */
async function sweepOnceThePreviewIsGone(rpc, nodes, params) {
  for (let attempt = 1; ; attempt += 1) {
    try {
      return await rpc.call("wch_calibrate_sweep", params);
    } catch (err) {
      // `this_process` is D13's `Occupation`, an internally-tagged word rather than a flag:
      // `streaming` is a node this daemon has a stream up on, which is what a preview leaves
      // behind for the moment it takes the socket to close. Any *other* occupation — a
      // recording, a command queue — is a different wait with a different remedy, and a page
      // that retried at it would be hiding a refusal an operator needs to read.
      const ours = err.kind === "busy" && err.data?.this_process === "streaming";
      if (!ours || attempt >= PREVIEW_RELEASE_TRIES) {
        throw err;
      }
      nodes.status.textContent =
        "waiting for this page's own preview to let the camera go…";
      await new Promise((done) => {
        setTimeout(done, PREVIEW_RELEASE_INTERVAL_MS);
      });
    }
  }
}

async function apply(rpc, nodes) {
  requireSession();
  const report = await rpc.call("wch_calibrate_apply", {
    camera: flow.camera,
    session: flow.session,
    // Partial, because a session where one control is still undecided is the normal shape of
    // an operator's afternoon — and the daemon's answer says what it applied and what it
    // skipped, which is the sentence that makes partial safe to offer.
    partial: true,
  });
  await refresh(rpc, nodes);
  nodes.status.textContent = `applied ${report.applied.length} control(s), ${report.skipped.length} left undecided`;
}

async function restore(rpc, nodes) {
  requireSession();
  const report = await rpc.call("wch_calibrate_restore", {
    camera: flow.camera,
    session: flow.session,
  });
  await refresh(rpc, nodes);
  nodes.status.textContent = report.complete
    ? "the camera is back where the session found it"
    : `restore put back ${report.restored.length} control(s) and could not put back ${report.failed.length}`;
}

/** Record the operator's choice: this sample, chosen by a human (D8's `human`). */
async function select(rpc, nodes, control, sample) {
  await rpc.call("wch_calibrate_select", {
    camera: flow.camera,
    session: flow.session,
    control,
    // The **applied** value, never the requested one: a sample is labelled with what the
    // camera actually held \[PF:6\], and selecting a requested value would record a choice at
    // a value no photograph was taken at.
    selection: { kind: "by_value", value: sample.applied, chosen_by: "human" },
  });
  await refresh(rpc, nodes);
  nodes.status.textContent = `${control} = ${sample.applied}, chosen by eye`;
}

/** Re-read the session document, which is the only thing this page believes about a session. */
async function refresh(rpc, nodes) {
  if (flow.session === null) {
    flow.status = null;
    return;
  }
  flow.status = await rpc.call("wch_calibrate_status", {
    camera: flow.camera,
    session: flow.session,
  });
  grid(rpc, nodes);
}

/**
 * The sample photographs of the control under review, with their scores, in a grid.
 *
 * Clicking one is the selection. The score is shown beside each because D8's whole posture is
 * that metrics *rank* and do not decide: the operator is being shown what the metric thought
 * and asked what they think.
 */
function grid(rpc, nodes) {
  const control = flow.reviewing;
  const session = flow.status?.session;
  const record = session?.controls?.[control];
  if (!record || !Array.isArray(record.samples) || record.samples.length === 0) {
    fill(nodes.grid, []);
    return;
  }

  // The value the session document says was chosen, and by whom. It lives on the control's
  // *status* — `ControlStatus::Calibrated { value, selector, … }` — because a selection is a
  // transition rather than a field, which is D8's shape and the reason the page reads it
  // rather than remembering what it clicked.
  // `ControlStatus` is tagged `status`, not `kind` — the two internally-tagged enums in this
  // document use different discriminant names, and reading the wrong one is a comparison that
  // is silently always false.
  const chosen = record.status?.status === "calibrated" ? record.status.value : null;
  fill(
    nodes.grid,
    record.samples.map((sample, index) => {
      const figure = el("figure", { class: "sample" });
      const image = el("img", {
        alt: `${control} at ${sample.applied}`,
        src: samplePhotoUrl({
          session: session.id,
          control,
          // **The reference is `(pass, requested)` and not `(pass, applied)`**, and the two
          // are different numbers on any device that clamps \[PF:6\]. The store names a
          // sample's photo after the value the sweep *asked for* — `photos/<control>/<from>/
          // <requested>.<ext>`, and `engine::calibrate`'s header says why: two requests that
          // clamp to one applied value are two samples and would otherwise overwrite each
          // other. The **applied** value is what a selection records, one function down.
          pass: passOf(sample),
          value: sample.requested,
        }),
      });
      const caption = el("figcaption", {}, [
        el("span", { class: "value mono" }, `${sample.applied}`),
        el("span", { class: "score" }, scoreline(sample)),
      ]);
      const button = el("button", { type: "button", class: "sample-pick" }, [image, caption]);
      button.setAttribute("aria-pressed", String(chosen !== null && chosen === sample.applied));
      button.addEventListener("click", () => run(nodes, () => select(rpc, nodes, control, sample)));
      figure.append(button);
      if (index === record.samples.length - 1) {
        figure.classList.add("freshest");
      }
      return figure;
    }),
  );
  nodes.grid.style.setProperty("--sample-columns", String(GRID_COLUMNS));
}

/**
 * Which sweep pass a sample belongs to, read out of the path the session document carries.
 *
 * A number, not a path — and the distinction is the whole security posture of the route it
 * feeds. The page never *sends* `sample.photo`: it reads one segment out of the layout the
 * store recorded (`photos/<control>/<from>/<requested>.<ext>`, D9) and sends that segment as a
 * number, and the daemon derives the file from the session's own document. There is nothing
 * here for a caller to traverse with, because nothing a caller wrote ever becomes a path.
 */
function passOf(sample) {
  const segments = String(sample.photo ?? "").split("/");
  // `photos / <control> / <from> / <file>` — the third segment. A document that does not have
  // that shape yields 0, which resolves to nothing and 404s, rather than an image built out of
  // a guess.
  const from = Number(segments.at(-2));
  return Number.isFinite(from) ? from : 0;
}

/** One sample's metric scores, shortest useful rendering. */
function scoreline(sample) {
  const metrics = sample.metrics ?? {};
  const names = Object.keys(metrics).sort();
  if (names.length === 0) {
    return "no metric scored this sample";
  }
  return names.map((name) => `${name} ${Number(metrics[name]).toFixed(3)}`).join(" · ");
}

/**
 * The next control the planner queued that has no samples yet.
 *
 * Read off the document rather than counted here, and *not* off `status.kind`: a control may
 * be `calibrated` from an earlier session pass or `deferred` by an operator, and either way
 * what decides whether there is a sweep left to run is whether photographs exist. The queue's
 * order is the operator's — `calibrate_plan` sorted it — so this walks it rather than the map.
 */
function nextControl() {
  const controls = flow.status?.session?.controls ?? {};
  const queue = flow.status?.session?.queue ?? [];
  for (const slug of queue) {
    if (flow.refused.has(slug)) {
      continue;
    }
    const record = controls[slug];
    if (!record || !Array.isArray(record.samples) || record.samples.length === 0) {
      return slug;
    }
  }
  return null;
}

/**
 * The span of a control's declared range, or `null` when it has none to speak of.
 *
 * Asked of the device through `wch_controls` rather than remembered from the panel: the panel
 * is another module's, and a stride computed from a range this module had cached would be a
 * stride for a camera that may since have been switched.
 */
async function rangeOf(rpc, control) {
  const report = await rpc.call("wch_controls", { camera: flow.camera });
  const desc = (report.controls ?? []).find((entry) => entry.slug === control);
  if (!desc || typeof desc.range?.min !== "number" || typeof desc.range?.max !== "number") {
    return null;
  }
  const span = desc.range.max - desc.range.min;
  return span > 0 ? span : null;
}

function requireSession() {
  if (flow.session === null) {
    throw new Error("start a session first");
  }
}

/**
 * Enable exactly the buttons whose verb could succeed, and disable nothing on a guess.
 *
 * The distinction matters: a button is disabled here only when the page *knows* there is
 * nothing to send — no camera, no session, a sweep already running — and never because the
 * page has an opinion about what the daemon would say. An out-of-order click is meant to reach
 * the daemon and come back as an `illegal_transition` the operator can read; that refusal is
 * this flow's guard rail and D20 says so.
 */
function paint(nodes) {
  const hasCamera = flow.camera !== null;
  const hasSession = flow.session !== null;
  nodes.start.disabled = !hasCamera || flow.sweeping;
  nodes.plan.disabled = !hasSession || flow.sweeping;
  nodes.sweep.disabled = !hasSession || flow.sweeping;
  nodes.apply.disabled = !hasSession || flow.sweeping;
  nodes.restore.disabled = !hasSession || flow.sweeping;
  nodes.flow.dataset.sweeping = String(flow.sweeping);
  nodes.flow.dataset.session = hasSession ? flow.session.id : "";
}
