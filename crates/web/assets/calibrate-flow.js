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
// flow's guard rails: an out-of-order click is *sent*, and what an operator reads is the D13
// refusal that came back. Which refusal depends on the gesture — a second session for a task
// that already has one is `session_conflict`; `illegal_transition` is what a control the daemon
// will not sweep answers, which on this page means a motorized one. Note **N276** records which
// gestures reach which. The alternative — mirroring the state machine here so the page can
// grey out the wrong button — is a second copy of D8's rules that would drift, and would make
// the page's *belief* about a session the thing an operator trusts. What the page knows about
// a session comes from `wch_calibrate_status`, which is the document on disk.
//
// **It is not a session editor.** Task, goal and criteria are set at start — the criteria one
// per line, in priority order, because D8 records them for whoever judges and the *human*
// selector is the one this surface exists to produce; free-text notes stay CLI-side (D20's
// "what the page still is not").
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
//
// The subscription is **not this module's**. `wch_subscribe_calibration` takes no parameters and
// is per *client* — `crates/api`'s `WchEvents` argues that twice over — so a second one opened
// here would spend a slot from `limits::RPC_MAX_SUBSCRIPTIONS_PER_CONNECTION` to receive the
// events the one `calibration.js` already opened is receiving. app.js fans that one stream into
// both consumers and [`progress`] is this module's end of it, which filters to the session this
// page is driving: a sweep somebody else started is a line in the log beside, never a picture in
// this operator's pane.

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
  /**
   * Whether the preview pane has been lent to a sweep, which is the stylesheet's question and no
   * longer the buttons'.
   *
   * It was both until this batch: [`paint`] read it to disable the five verbs, which is note
   * **N279**'s repair written where the one verb that had the defect could reach it. That home
   * is now [`flow.inFlight`], which covers every verb rather than the one that was found, and
   * what is left here is the fact `data-sweeping` publishes — app.css draws the preview slot
   * differently while the sweep has it, so that an operator whose picture went away can see
   * where it went.
   */
  sweeping: false,
  /**
   * Who the sweep pane belongs to, for as long as a sweep has it. `null` when the preview does.
   *
   * **It outlives the `wch_calibrate_sweep` answer on purpose**, and that is the whole of its
   * reason for existing rather than being a boolean. A sweep's ending is
   * `CalibrationProgress::SweepFinished` or `SweepInterrupted` — `engine::calibrate`'s "one
   * start, one end" — and the answer to the call is *not* that event: they are produced by two
   * tasks and reach one socket in whichever order the daemon's scheduler chose (notes **N69**,
   * **N87**). A pane whose ownership ended with the answer therefore dropped the last sample and
   * the summary whenever the answer won, which is a delivered event made to look undelivered —
   * measured on this tree: the rung's own sweep-pane claim was red 1 run in 11 unloaded and 1 in
   * 4 under load, on `sweep_finished` never painting (note **N278**).
   *
   * `pending` is how many endings this pane is still owed: `SweepStarted` adds one and each
   * terminal event takes one away, which is `engine::calibrate`'s "one start, one end" counted at
   * the consumer. A count rather than a flag because **one click can announce several sweeps** —
   * the preview-drain retry sends `wch_calibrate_sweep` again, and each attempt that reaches the
   * device announces itself and is interrupted — so a flag set by the first attempt's ending
   * would tell the last attempt that its own ending had already arrived. Zero is also the honest
   * answer for a refusal that never announced anything (`Busy` at the actor's door,
   * `IllegalTransition` from the planner): there is no ending coming, and a pane that waited for
   * one would be a pane that never came back.
   */
  pane: null,
  /**
   * Controls the daemon said it will not sweep from here, so the queue can be got past.
   *
   * **Not a second state machine and not a skip**: the session document still has these
   * controls queued and untouched, and `calibrate status` on any other surface says so. What
   * this remembers is only that *this page* has already been told no — which is what stops
   * "Sweep next" handing the operator the same refusal forever. A motorized control is the
   * case that forced it: §5 says a plan that would move motors says so first, the page sends
   * no `allow_motion` and therefore must not, and without this the owner's own PTZ camera
   * wedges the flow at the thirteenth control in the queue.
   *
   * **Only `illegal_transition` is remembered, and AGENTS rule 7 is why.** `busy`, `device_io`,
   * `device_gone` and `permission_denied` are facts about a machine at a moment; interning one
   * here would convert "the camera was busy for two seconds" into "this page cannot sweep this
   * control", for the life of the camera selection and with no verb that undoes it. `rpc.js`'s
   * own header states the rule this would break. `illegal_transition` is the one refusal that is
   * a statement about the *control* — the planner will not sweep it from here — and it stays
   * true until something outside this page changes, which is what makes remembering it honest
   * (note **N285**).
   */
  refused: new Set(),
  /**
   * How many of this flow's verbs are on the wire right now, so [`paint`] can disable the button
   * for one the page has already sent.
   *
   * **A verb the shell knows is in flight is not a verb the shell may offer again**, and this is
   * where that is decided for every button rather than in each of them. `sweep()` learned it
   * first and locally: its button was disabled by `paint` four lines *after* a round trip, so an
   * ordinary double-click ran two sweeps (note **N279**). `start()` had the identical shape and
   * a louder ending — two `wch_calibrate_start` on the wire from one gesture, 28 runs out of 28,
   * the daemon answering the loser `session_conflict: … resume it, or finish it before starting
   * another`, and the page painting that in the refusal colour *about the session it had just
   * successfully created and was holding*. `start`'s own `read !== reads` fence cannot see it:
   * a refusal throws out of `rpc.call` in front of that check, so the fence covers the quiet
   * half and not the loud one. Plan, Apply and Restore were re-entrant the same way and benign
   * only because the daemon happens to be idempotent about them, which is not a property this
   * page is entitled to assume.
   *
   * A **count, not a flag**: [`run`] is also what a sample click goes through, so two of these
   * can be live at once and a flag cleared by whichever finished first would re-enable a button
   * whose verb is still out.
   *
   * This is not the second state machine this module's header refuses. It is the shell declining
   * to send a verb twice; what a verb *means* is still the daemon's, and an out-of-order click
   * still reaches it and comes back as a refusal an operator reads (note **N314**).
   */
  inFlight: 0,
};

/**
 * How long the pane waits for the sweep's own ending once the answer has landed.
 *
 * **A bound rather than a wait**, because a state a failure strands with no verb out is the
 * defect AGENTS rule 7 names (docs/11 **H2**): the terminal event is guaranteed by
 * `engine::calibrate`'s "one start, one end" and it is *emitted* before the answer, but delivery
 * is a socket's business and a socket can die between the two. So the pane comes back either
 * when the sweep says it ended or when this many milliseconds have passed, and on the ordinary
 * path the wait is one turn of the event loop because the event is already queued behind the
 * answer.
 *
 * **It is the client's own bound and it is reconciled as one.** No Rust would read it — it bounds
 * how long a *browser* waits on a socket only that browser can watch die — so `schema::limits` is
 * the wrong home for it, and
 * `crates/daemon/tests/web_client.rs`'s `the_bounds_the_page_runs_on_are_the_ones_this_build_declares`
 * carries that reason beside the name, in the derived walk of every number this client declares.
 * It is driven from both sides by the browser rung: the ending wins in the sweep-time pane's own
 * claim, and this bound wins in the one that holds the ending on the wire (note **N313**).
 */
const SWEEP_ENDING_WAIT_MS = 2000;

/**
 * Which read of this flow is the current one, so a late answer can be told it is not.
 *
 * **The control panel's rule, in the fourth element that had the same defect** (docs/11 **M32**,
 * notes **N154** and **N156**). `refresh` awaits `wch_calibrate_status` and then paints the
 * grid, and every verb below writes a sentence into `#flow-status` on the line after it; the
 * daemon spawns a task per inbound WS message, so two sample clicks put two reads on the wire
 * and the *first* one's answer can land last — marking the sample the operator did not choose
 * and saying so underneath, permanently, with nothing on screen to say otherwise. Measured in
 * Chromium against this module before the fence existed: the daemon had recorded 20 and the page
 * read "brightness = 10, chosen by eye".
 *
 * A number rather than a comparison of session ids, for `refreshControls`' reason (app.js): two
 * answers about the *same* session can also arrive out of order, and an id comparison cannot see
 * that.
 *
 * **Both halves are inside the fence, because they are two halves.** The grid repaint is
 * `refresh`'s own (N154's ordering); the sentence belongs to the caller and is written after the
 * await (N156's — "the sentence was written into the node before the question was asked"). So
 * `refresh` answers whether its read is still the current one and every caller gates its sentence
 * on that answer; a fence inside `refresh` alone leaves the louder half standing.
 */
let reads = 0;

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
  flow.pane = null;
  // **The sentence goes with the session it is about.** This function drops every other trace of
  // the session — the id, the control under review, the document, the grid, the pane — and left
  // `#flow-status` standing, so after a camera switch the line went on reading `session <uuid>
  // started for <task>` over a `#flow` whose `data-session` is empty and whose four verbs are
  // disabled: an operator told a session is open, shown no verb that touches it, and given
  // nothing that says why. It is the class note **N279** named one element along — a line whose
  // words and whose state were written by two different statements — and the fix is that the one
  // function whose whole job is dropping the session drops its sentence too (note **N314**).
  //
  // The colour goes with it. A refusal left in red under a camera it is not about is the same
  // wrong statement as the words, and louder.
  nodes.status.classList.remove("failed");
  nodes.status.textContent = "";
  // A camera switch is a new view, and there is no newer *read* to retire the one already on
  // the wire — N154's "a newer list is a newer view", which is the arrival a counter bumped only
  // by `refresh` cannot see. It retires an in-flight `wch_calibrate_start` as well as an
  // in-flight read: a session belongs to the camera it was started for, and a start answer that
  // lands after the operator has moved on would otherwise install one for the camera they left
  // and make every verb after it a `fingerprint_mismatch` (note **N280**).
  reads += 1;
  fill(nodes.grid, []);
  sweepView(nodes, false);
  paint(nodes);
}

/**
 * Run one step, and render whatever it answers — including a refusal.
 *
 * Every button goes through here, so the page has exactly one place where a D13 refusal
 * becomes a sentence on screen. The message is the daemon's own, instruction-last as D13
 * renders it (note **N212**), and rewording it here would make the page's advice differ from
 * the CLI's about the same session — which is the whole reason the browser rung asserts the
 * *sentence* rather than the discriminant beside it (note **N276**).
 *
 * **A step answers with its sentence rather than writing one**, and the colour is set in the
 * same statement as the words. The line is one node and two verbs can be in flight over it —
 * two sample clicks are the ordinary way, and a double-click was another until the sweep was
 * made non-re-entrant — so a step that wrote its own text left the class to whichever run had
 * touched it last: a success sentence painted in the refusal colour, and it stayed there until
 * the next click because only entry to this function cleared it (note **N279**). `undefined` is
 * how a step says its answer was about a session nobody is looking at any more; the line is then
 * left exactly as it was, which is [`refresh`]'s fence carried out to the caller.
 */
async function run(nodes, step) {
  // Cleared on the way in as well, for the sentences a step writes *while* it works — the
  // sweep's "sweeping brightness in 3 step(s)…" and the preview-drain wait — which are progress
  // rather than verdicts and must not inherit the previous click's refusal colour.
  nodes.status.classList.remove("failed");
  // **The buttons go down before the first await, which is what makes a double-click one verb.**
  // Both statements are synchronous and in the same task as the click, so the second half of an
  // ordinary double-click meets a disabled button rather than a second `paint` that has not
  // happened yet — [`flow.inFlight`] says what each of the five buttons cost before this was
  // here. Every button and every sample is wired through this function, so this is the one place
  // that has to know, and a verb added later inherits it by being a step.
  flow.inFlight += 1;
  paint(nodes);
  try {
    const said = await step();
    if (said !== undefined) {
      nodes.status.classList.remove("failed");
      nodes.status.textContent = said;
    }
  } catch (err) {
    nodes.status.classList.add("failed");
    // `err.kind` is the D13 discriminant the wire carries in `data.kind`, and it is shown
    // beside the message rather than instead of it: the kind is what a reader branches on and
    // the message is what tells them which control and which session.
    nodes.status.textContent = err.kind ? `${err.kind}: ${err.message}` : err.message;
  } finally {
    // `finally`, because a step that threw is a verb that is no longer in flight exactly as a
    // step that answered is — and a button left disabled by a refusal is the stranded state
    // AGENTS rule 7 and docs/11 **H2** are about.
    flow.inFlight -= 1;
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
  // **The assignment is fenced, not only the read that follows it.** `watching` nulls
  // `flow.session` on every camera switch because a session does not follow a camera; a start
  // answer that lands after the switch would put it back, and the page would then drive the
  // previous camera's session while the operator looked at another one — every verb after it
  // refused `fingerprint_mismatch`, which is the daemon protecting the hardware from a belief
  // this page had no business holding (note **N280**). The number is taken before the await for
  // [`refresh`]'s reason: a fence installed after the arrival cannot see it.
  const read = reads;
  const session = await rpc.call("wch_calibrate_start", {
    camera: flow.camera,
    task,
    goal: nodes.goal.value.trim(),
    criteria: criteriaFrom(nodes.criteria.value),
  });
  if (read !== reads) {
    return undefined;
  }
  flow.session = { kind: "id", id: session.id };
  flow.reviewing = null;
  if (!(await refresh(rpc, nodes))) {
    return undefined;
  }
  return `session ${session.id} started for ${task}`;
}

/**
 * The criteria typed into the start form, in the order they were typed.
 *
 * **One per line, and the order is the priority** — `schema::session::Session::criteria` is an
 * ordered `Vec<String>` and `webcam-handler-cli calibrate start --criterion` is repeatable for
 * the same reason, so a textarea whose lines are the entries is the same grammar with a
 * different keyboard. Splitting on commas instead would have made "sharpness, then colour" one
 * criterion or two depending on how somebody typed it.
 *
 * Blank lines are dropped rather than recorded: an empty criterion is a line the operator left,
 * not something the selector is judging against, and D8 says these exist because *whoever
 * chooses* is judging against something.
 */
function criteriaFrom(text) {
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line !== "");
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
  if (!(await refresh(rpc, nodes))) {
    return undefined;
  }
  // `queue` is omitted from the document when it is empty (`skip_serializing_if`), so this
  // reads a length off a key that may not be there — and a plan that queued nothing is a
  // sentence an operator needs rather than a crash.
  const queued = planned.queue?.length ?? 0;
  return queued === 0
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
  // The pane is announced as the sweep's before the first `await`, so the stylesheet says where
  // the preview went in the same task as the click rather than after a round trip.
  //
  // **The double-click is [`run`]'s to stop, not this line's.** These two statements were that
  // guard until this batch — note **N279** landed them here, on the one verb whose second click
  // had a visible ending — and `start` then turned out to have the identical shape and a louder
  // one. The fence moved to the function every button already goes through; what stays here is
  // the pane's own fact.
  flow.sweeping = true;
  paint(nodes);
  // Taken before anything is awaited, so a camera switch during the sweep retires the answer
  // rather than being overwritten by it — [`refresh`]'s fence, applied to the state this
  // function assigns as well as to the read it makes (note **N280**).
  const read = reads;
  try {
    const control = nextControl();
    if (control === null) {
      throw new Error(
        flow.refused.size === 0
          ? "every planned control has been swept — apply, or plan again"
          : `every control this page can sweep has been swept; the daemon would not sweep ` +
            `${[...flow.refused].join(", ")}, which ${flow.refused.size === 1 ? "is" : "are"} ` +
            "still queued — the command-line surface can, and a motorized control needs " +
            "--allow-motion",
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
    // The pane becomes the sweep here rather than on the first event, because the first event is
    // seconds away and the picture the operator was watching has just been taken off the screen:
    // an empty `<figure>` between the click and the first sample is the shell looking broken.
    borrowPane(nodes, flow.session.id, control);
    nodes.status.textContent = `sweeping ${control} in ${budget} step(s)…`;
    try {
      await sweepOnceThePreviewIsGone(rpc, nodes, {
        camera: flow.camera,
        session: flow.session,
        request: { control, plan: { kind: "uniform", step } },
      });
    } catch (refusal) {
      // Remembered only when the daemon said it *will not* sweep this control, so the next click
      // offers the next one rather than this refusal again — and never when it said the camera
      // was unavailable, because AGENTS rule 7 forbids this page turning a `busy` into "this
      // camera can't" for the life of a camera selection. The refusal itself is not swallowed
      // either way: it goes up to `run`, which prints the daemon's own words — a motorized
      // control's `illegal_transition` says `motion(allow_motion=false)`, which is the sentence
      // that tells an operator to use the command line for it.
      if (refusal.kind === "illegal_transition") {
        flow.refused.add(control);
      }
      throw refusal;
    } finally {
      // The preview comes back whatever happened, including a refusal: a page that left the
      // pane blank after a `Busy` would have taken the operator's camera away to no purpose.
      await handBackPane(nodes, preview);
    }
    if (read !== reads) {
      return undefined;
    }
    // **After the sweep, never before it.** This is what `grid` paints, and a refusal that left
    // it pointing at a control with no samples emptied the grid the operator was reviewing at
    // the next verb that repainted — the photographs and the `aria-pressed` record of their own
    // choice, gone with no sentence saying why (note **N281**).
    flow.reviewing = control;
    if (!(await refresh(rpc, nodes))) {
      return undefined;
    }
    return `${control} swept — pick the sample that looks right`;
  } finally {
    flow.sweeping = false;
    paint(nodes);
  }
}

/**
 * Hand the preview pane to the sweep that is about to start.
 *
 * The session id is copied rather than read later because it is the filter [`progress`] applies:
 * an event is this pane's when it names this session, and `flow.session` is a field a camera
 * switch nulls out from under it. The control is not recorded, because nothing here decides
 * anything about it — every line the pane paints names the control the *event* names, which is
 * the one the daemon is really sweeping.
 */
function borrowPane(nodes, session, control) {
  flow.pane = { session, pending: 0, waiting: [] };
  sweepView(nodes, true);
  nodes.progress.textContent = `${control}: waiting for the first sample…`;
}

/**
 * Give the pane back to the preview, once the sweep really has ended.
 *
 * **The answer is not the ending.** `engine::calibrate` guarantees one terminal event per
 * announced sweep and emits it before it returns, but the event and the answer travel on two
 * tasks and reach one socket in whichever order the daemon's scheduler chose — N87 wrote that
 * down about `webcam-handler-client`'s tail and N69 measured it carrying every event but the
 * last. So this waits for the pane's own ending before it takes the pane down, and the operator
 * sees the last sample and the count instead of a pane blanked microseconds before its ending
 * arrived (note **N278**).
 *
 * **It waits exactly while an ending is outstanding**, which is what `pending` counts: a refusal
 * that arrived *before* `SweepStarted` — the planner's, or `Busy` at the actor's door — announced
 * nothing and has no ending coming, and waiting on it would be seconds of nothing between a
 * refusal and the camera coming back. A sweep whose whole event sequence has already landed is
 * the same case for the same reason: there is nothing left to wait for, and the count says so.
 */
async function handBackPane(nodes, preview) {
  const pane = flow.pane;
  if (pane !== null && pane.pending > 0) {
    await Promise.race([
      new Promise((wake) => {
        pane.waiting.push(wake);
      }),
      new Promise((done) => {
        setTimeout(done, SWEEP_ENDING_WAIT_MS);
      }),
    ]);
  }
  flow.pane = null;
  // A sweep view left up over a live camera is a picture of the past labelled as the present.
  sweepView(nodes, false);
  preview.start();
}

/**
 * Show the sweep's two elements in the preview's slot, or take them down and clear them.
 *
 * Two elements rather than one — the freshest sample and the progress line — for the reason
 * index.html gives about `#preview-status` and `#recording-status`: two views sharing one node
 * overwrite each other within milliseconds and the loser is whichever wrote first. The `<img>`'s
 * `src` is cleared on the way out because a stale sample left in the DOM is a photograph of a
 * camera setting nobody is holding any more.
 */
function sweepView(nodes, showing) {
  nodes.view.hidden = !showing;
  if (!showing) {
    nodes.sample.removeAttribute("src");
    nodes.sample.alt = "";
    nodes.progress.textContent = "";
  }
}

/**
 * One live sweep event, painted into the pane the sweep borrowed (design D20).
 *
 * **Fanned in from the one subscription this page opens** — see the module header — and filtered
 * here rather than there: `wch_subscribe_calibration` carries every session this daemon is
 * running, and a sweep somebody else started belongs in the log beside, not in this operator's
 * pane. The guard is [`flow.pane`]'s ownership *and* the session id, because both can be wrong
 * independently: an event for another session while this page sweeps, and an event for this
 * session from a `webcam-handler-cli` run after this page's own sweep ended.
 *
 * **Ownership, and not "a sweep is in flight".** The guard used to read `flow.sweeping`, which is
 * retired the moment `wch_calibrate_sweep` answers — so the sweep's own ending was dropped
 * whenever the answer beat it to the socket, which is a race with no sequencing behind it and
 * which the rung caught (note **N278**). The pane owns itself until its ending arrives.
 *
 * The picture comes through `/session-photo` rather than through the preview channel, which is
 * D20's decision and not an economy: a live view during a sweep shows the settle transients
 * between samples, and what the operator is judging is the samples. Feeding sweep frames through
 * the preview's own machinery is design §8's item.
 */
export function progress(event, nodes) {
  const pane = flow.pane;
  if (pane === null || event.session !== pane.session) {
    return;
  }
  switch (event.progress) {
    case "sweep_started":
      // Counted, because it is what decides whether an ending is still coming: `engine::
      // calibrate`'s "one start, one end" promises a terminal event to a sweep that announced
      // itself and to no other.
      pane.pending += 1;
      nodes.progress.textContent = `${event.control}: 0/${event.total}`;
      return;
    case "value_set":
      // The camera has moved and the sensor is settling, which is seconds on a real device: the
      // bar advances here rather than only on the photograph, for the reason `CalibrationProgress`
      // emits this event at all.
      nodes.progress.textContent = `${event.control}: ${event.index}/${event.total}, settling at ${
        event.requested === event.applied ? event.applied : `${event.requested} → ${event.applied}`
      }`;
      return;
    case "sample_taken":
      // The reference is `(pass, requested)`, exactly as the grid builds it one function down and
      // for the same reason \[PF:6\]: the store names a sample's photo after the value the sweep
      // *asked for*, because two requests that clamp to one applied value are two samples.
      nodes.sample.src = samplePhotoUrl({
        session: event.session,
        control: event.control,
        pass: passOf(event),
        value: event.requested,
      });
      nodes.sample.alt = `${event.control} at ${event.applied}`;
      nodes.progress.textContent = `${event.control}: ${event.index}/${event.total} at ${event.applied}${scores(event.metrics)}`;
      return;
    case "sweep_finished":
      nodes.progress.textContent = `${event.control}: ${event.samples} sample(s) taken`;
      ended(pane);
      return;
    case "sweep_interrupted":
      // The refusal's D13 discriminant rides on the event so a consumer can branch without
      // parsing prose, and both halves are shown: the name to act on and the sentence to read.
      nodes.progress.textContent = `${event.control}: stopped after ${event.taken}/${event.total} — ${event.failure}: ${event.detail}`;
      ended(pane);
      return;
    default:
      // AGENTS rule 6 at a pane: a daemon newer than this page is exactly the case, and a
      // silently dropped event would leave a progress line that has stopped advancing looking
      // like a sweep that has stopped.
      nodes.progress.textContent = `an event this page does not know: ${event.progress}`;
  }
}

/**
 * One of the endings this pane was owed has arrived.
 *
 * One place rather than two arms, because "the sweep is over on screen" is one fact and
 * `SweepFinished`/`SweepInterrupted` are two spellings of it — `CalibrationProgress::is_terminal`
 * is the same partition one crate over.
 *
 * The floor at zero is AGENTS rule 6 at a counter: a terminal event this pane never saw the start
 * of is a fact about a session somebody else is also driving, carried rather than allowed to
 * drive the count negative and strand the next wait.
 */
function ended(pane) {
  pane.pending = Math.max(0, pane.pending - 1);
  if (pane.pending === 0) {
    for (const wake of pane.waiting.splice(0)) {
      wake();
    }
  }
}

/** A live sample's metric scores, when a metric produced any. */
function scores(metrics) {
  const entries = Object.entries(metrics ?? {});
  return entries.length === 0
    ? ""
    : ` · ${entries.map(([name, score]) => `${name} ${Number(score).toFixed(3)}`).join(" ")}`;
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
 * So the page waits, with a stated bound and a sentence on screen while it does. It waits **only**
 * for a `Busy` whose `Occupation` says *this daemon is streaming the node* — any other occupation
 * (a recording, a command queue) is a different wait with a different remedy, and retrying at one
 * would be a page hiding a refusal an operator needs to read.
 *
 * **`this_process` is the daemon, not this page**, and the sentence on screen says so. A second
 * client of the same daemon previewing the same camera reaches this loop too, and until
 * 2026-08-20 the page told the operator it was "waiting for this page's own preview to let the
 * camera go" while the holder was somebody else's tab — a page stating a reason it has no way to
 * know (note **N282**). What the predicate proves is the occupation; what the page says is the
 * occupation.
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
        "this daemon is still streaming the camera; waiting for the stream to end…";
      // **The pane says what is happening now.** A sweep that reached the device and met `EBUSY`
      // there has already announced itself and been interrupted, so the pane is holding
      // `stopped after 0/3 — busy: …` under a line that says the page is still waiting: two
      // statements about one moment, one of them false (note **N282**). The count of endings this
      // pane is owed is deliberately *not* touched here — each attempt announces itself and is
      // ended, and the count is what keeps one attempt's ending from answering for another's.
      if (flow.pane !== null) {
        nodes.progress.textContent = `${params.request.control}: waiting for the camera…`;
      }
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
  if (!(await refresh(rpc, nodes))) {
    return undefined;
  }
  return applySentence(report);
}

/**
 * What `wch_calibrate_apply` did, out of the fields it actually answers.
 *
 * **It answers a `schema::report::WriteReport`** — `camera`, `writes`, `disabled_automation` —
 * the same document `wch_set` answers, because applying a session *is* a guarded write. Until
 * 2026-08-20 this line read `report.applied.length` and `report.skipped.length`, two fields no
 * version of that type has ever carried, so every click on Apply threw a `TypeError` into
 * `#flow-status` after the verb had already succeeded: the camera was correct and the operator
 * was shown a JavaScript error (note **N273**).
 *
 * There is no "left undecided" count here and this line does not compute one. What the daemon
 * skipped under `partial: true` is a question about the *session document*, which the page
 * re-reads on the next line and shows in the grid; deriving it from D8's control statuses here
 * would be the second state machine this module's header refuses.
 */
function applySentence(report) {
  const writes = report.writes ?? [];
  if (writes.length === 0) {
    // The ordinary answer to applying a session nothing has been chosen in, and it is advice
    // rather than a count: `partial: true` means an undecided control is not a refusal.
    return "nothing was decided to apply — pick a sample first";
  }
  const parts = [`applied ${writes.length} write(s)`];
  const switched = report.disabled_automation ?? [];
  if (switched.length > 0) {
    // A guarded write changes more than the caller named, and that is a change to the camera an
    // operator is entitled to hear about at the moment it happens (D3; `render::writes` says the
    // same sentence one surface over).
    parts.push(`switched off to make them stick: ${switched.join(", ")}`);
  }
  const moved = writes.filter((write) => !sameValue(write.requested, write.applied));
  if (moved.length > 0) {
    // Both numbers, always \[PF:6\]. A layer that collapses `{requested, applied}` to one value
    // is dropping the fact the whole doctrine exists to keep.
    parts.push(
      moved
        .map((write) => `${write.slug} took ${valueText(write.applied)}, not ${valueText(write.requested)}`)
        .join("; "),
    );
  }
  return parts.join(" — ");
}

async function restore(rpc, nodes) {
  requireSession();
  const report = await rpc.call("wch_calibrate_restore", {
    camera: flow.camera,
    session: flow.session,
  });
  if (!(await refresh(rpc, nodes))) {
    return undefined;
  }
  return restoreSentence(report);
}

/**
 * What `wch_calibrate_restore` did, in the vocabulary the report is written in.
 *
 * **It answers a `schema::snapshot::RestoreReport`**: `outcomes` and `freed`. Until 2026-08-20
 * this line branched on `report.complete` and read `report.restored.length` /
 * `report.failed.length`, none of which the wire carries — `is_complete` is a Rust *method* — so
 * the ternary always took the else arm and the else arm always threw (note **N273**).
 *
 * **The verdict is deliberately not restated here.** "Is the camera back where the snapshot
 * found it" is `RestoreReport::is_complete`, and that rule is a policy rather than a reading:
 * `OwnedByAutomation` is a *success* because the control's owner is the one that owned it before
 * (note **N9**), and a `Restored` that did not land exactly is a failure. A copy of that rule in
 * this file would be a second home for it (design §2.10) and would go stale the day N9 is
 * amended. So this renders the outcome vocabulary one phrase per tag, names what the device
 * refused to put back, and carries `{requested, applied}` wherever the two differ — which is the
 * evidence an operator acts on, with the verdict left to the surface that owns it.
 */
function restoreSentence(report) {
  const outcomes = report.outcomes ?? [];
  const freed = report.freed ?? [];
  if (outcomes.length === 0 && freed.length === 0) {
    // Not "restored nothing": running restore twice is the ordinary way to reach this, and the
    // two would be indistinguishable from a count of zero. `render::restore` says it the same
    // way one surface over.
    return "this session carries no unconsumed pre-sweep snapshot; the camera was not written to";
  }
  const counted = new Map();
  const moved = [];
  const refusedBack = [];
  for (const outcome of outcomes) {
    counted.set(outcome.outcome, (counted.get(outcome.outcome) ?? 0) + 1);
    if (outcome.outcome === "restored" && !sameValue(outcome.applied.requested, outcome.applied.applied)) {
      moved.push(
        `${outcome.applied.slug} came back at ${valueText(outcome.applied.applied)}, not ${valueText(outcome.applied.requested)}`,
      );
    }
    if (outcome.outcome === "unrestorable") {
      refusedBack.push(`${outcome.control} (${outcome.reason?.kind ?? "no reason given"})`);
    }
  }
  const parts = [...counted].map(([tag, count]) => `${count} ${outcomeWords(tag)}`);
  if (moved.length > 0) {
    parts.push(moved.join("; "));
  }
  if (refusedBack.length > 0) {
    parts.push(`could not be put back: ${refusedBack.join(", ")}`);
  }
  if (freed.length > 0) {
    // A restore repairs the session as well as the camera, and this half has nothing to do with
    // the snapshot: a sweep killed before its first sample leaves a control every verb refuses
    // and nothing to write back (note **N139**).
    parts.push(`left mid-sweep by a process that is gone and given back: ${freed.join(", ")}`);
  }
  return `restore — ${parts.join("; ")}`;
}

/**
 * One `schema::snapshot::RestoreOutcome` tag, in words.
 *
 * `owned_by_automation` reads as a success because it is one (note **N9**): restoring an
 * automation control to "on" re-engages the manual control it governs, so on any device whose
 * INACTIVE flag follows the automation's value this is the *ordinary* outcome of every guarded
 * write's restore. A phrase that made it sound like a shortfall is how a field stops being read.
 */
function outcomeWords(tag) {
  switch (tag) {
    case "restored":
      return "put back";
    case "already_correct":
      return "already correct";
    case "owned_by_automation":
      return "owned by automation again, exactly as before";
    case "unrestorable":
      return "not put back";
    default:
      // A daemon newer than this page, shown rather than dropped (AGENTS rule 6).
      return `of an outcome this page does not know (${tag})`;
  }
}

/**
 * Whether two `schema::control::ControlValue`s are the same value.
 *
 * The wire spelling is `{kind, value}` — adjacently tagged, `kind` first — with `value` a number,
 * a string, or a byte array, so structural equality is the comparison and `===` is not. This is
 * `Applied::is_exact`'s equality and nothing more: it decides whether to show both numbers, never
 * whether a restore succeeded.
 */
function sameValue(requested, applied) {
  return (
    requested?.kind === applied?.kind && JSON.stringify(requested?.value) === JSON.stringify(applied?.value)
  );
}

/** One `ControlValue`, spelled the way `ControlValue`'s own `Display` spells it. */
function valueText(value) {
  return value?.kind === "bytes" ? `<${(value.value ?? []).length} bytes>` : String(value?.value);
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
  if (!(await refresh(rpc, nodes))) {
    return undefined;
  }
  return `${control} = ${sample.applied}, chosen by eye`;
}

/**
 * Re-read the session document, which is the only thing this page believes about a session.
 *
 * Answers **whether this read is still the current one**, and every caller gates its sentence on
 * that — see [`reads`] for the arrival that makes the fence a repair rather than a precaution,
 * and for why the caller's line is inside it rather than only the grid repaint. `false` is not a
 * failure: it is an answer about a session nobody is looking at any more, dropped in silence and
 * deliberately so.
 *
 * The answer is compared **before** it reaches `flow.status`, not after: assigning it and then
 * asking is the defect itself, because everything downstream reads that field.
 */
async function refresh(rpc, nodes) {
  const read = ++reads;
  if (flow.session === null) {
    flow.status = null;
    return true;
  }
  let status;
  try {
    status = await rpc.call("wch_calibrate_status", {
      camera: flow.camera,
      session: flow.session,
    });
  } catch (err) {
    // **The refusal arm is fenced too, and it is the louder half** — calibration.js:157 fences it
    // for the same reason and says so: a stale refusal painted over a current sentence is the
    // same wrong statement as a stale answer, in red. A current refusal is rethrown untouched, so
    // `run` still prints the daemon's own words; a stale one is dropped, because it is a red line
    // about a session nobody is looking at.
    if (read !== reads) {
      return false;
    }
    throw err;
  }
  if (read !== reads) {
    return false;
  }
  flow.status = status;
  grid(rpc, nodes);
  return true;
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
  // `inFlight` rather than `flow.sweeping`, which is what these five read until now: a sweep is
  // one of the verbs this counts, so the count covers everything the flag covered and the four
  // buttons the flag did not (see [`flow.inFlight`]). The flag stays because it is the *pane's*
  // fact — `data-sweeping` is what the stylesheet reads to say the preview has been lent out —
  // and that is a different question from whether a verb is on the wire.
  const busy = flow.inFlight > 0;
  nodes.start.disabled = !hasCamera || busy;
  nodes.plan.disabled = !hasSession || busy;
  nodes.sweep.disabled = !hasSession || busy;
  nodes.apply.disabled = !hasSession || busy;
  nodes.restore.disabled = !hasSession || busy;
  nodes.flow.dataset.sweeping = String(flow.sweeping);
  nodes.flow.dataset.session = hasSession ? flow.session.id : "";
}
