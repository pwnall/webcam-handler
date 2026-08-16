// Whether a recording owns the camera this page is previewing, and for roughly how long.
//
// The notes' Expected usage item 10 named two honest answers to a recording and a preview
// colliding on one camera, and the owner took the first: the preview is fed the recording's own
// frames (note **N117**). This module is the *second* one, which the ruling did not make
// unnecessary — it made it cheap. The page can show both, and there is one case where it has
// to: D7's raw fallback records YUYV, those bytes cannot go into an `<img>` labelled
// `image/jpeg`, and the daemon drops them — so the picture stops moving. A picture that stops
// moving with no sentence beside it is the failure item 10 was written against, and this line
// is that sentence.
//
// ## The daemon's answer is the only source of the fact
//
// Every word below comes out of one `wch_record_status` call. This module computes nothing the
// daemon did not say: not whether a take is running (`RecordStatus::is_running`'s question is
// answered by the presence of `ended`), not how long it has left (`elapsed_ms` and `budget_ms`
// are both the daemon's readings of *its* clock, and a page that subtracted a local `Date.now()`
// from them would be reporting the drift between two machines' clocks as a recording's
// progress). D10 says progress is polled and there is no recording subscription in v1, so this
// polls.
//
// **It also says nothing about the format**, and that is a deliberate omission rather than a
// gap: "which pixel formats a browser can paint" is `schema::camera::PixelFormat::is_compressed`
// and it has one home (design §2.10) that a page cannot reach. So the line reports the fact it
// was told — a recording owns this camera — and leaves the reader to see that the picture is
// paused, rather than deriving a claim about paintability out of a fourcc.
//
// ## What it costs, stated because a poll loop is a real cost
//
// One `wch_record_status` per [`POLL_MS`] per open tab, and each one is a live enumeration on
// the daemon's side (E2: the id is resolved against the machine every time, never out of a map
// that remembers it). That is the price of the fact, and the two things that bound it are that
// the loop runs only while this page is showing a preview and that it stops the moment the
// socket does. A second is chosen rather than `limits::CLIENT_RECORD_POLL_MS`'s 250 ms because
// the two loops are answering different questions: that one bounds how late a *client* notices
// its own take has finished, and this one bounds how stale a sentence a human is reading may be.
//
// ## Who writes this line, and who does not
//
// `#recording-status` has exactly one writer, which is this module — app.js's `#connection`
// rule ("no writer may make a statement it cannot know, and the last writer that still can
// wins") applied by construction rather than by care. When the socket ends, this stops writing
// and clears itself: `socketClosed` owns the one sentence that covers every stream on a dead
// connection, and a line still counting a recording up under a banner saying the connection is
// gone would be the page contradicting itself in two elements at once.
//
// **One writer is not one writer per moment**, which is the part that had to be repaired. A
// retired handle has a call in flight for as long as the daemon takes to answer it, so "this
// module writes the line" and "the handle that owns the line writes the line" are two different
// claims — and only the second one is worth anything. [`watch`]'s `poll` asks whether it is
// still the owner *after* its answer arrives as well as before it goes out.

// The *value*, not the transport: this module opens no socket and is handed its `rpc` by app.js.
// `SOCKET_CLOSED` is what "this socket has ended" is spelled as in this client, and importing the
// sentinel is what lets this loop branch on identity rather than on the absence of a field
// (calibration.js imports it for the same reason, one view along).
import { SOCKET_CLOSED } from "./rpc.js";

/** How often the daemon is asked, in milliseconds. See the header for why it is not 250. */
export const POLL_MS = 1000;

/**
 * Watch `camera`'s recording state and keep `node` saying what it is.
 *
 * Answers a handle whose `stop()` ends the loop and clears the line. The caller holds it for
 * exactly as long as the preview it belongs to — app.js replaces it on every camera switch,
 * for `preview.watch`'s reason: a loop left polling the previous camera is a page asking about
 * a device nobody is looking at.
 */
export function watch(rpc, camera, node) {
  const view = { stopped: false, timer: null };
  const stop = () => {
    view.stopped = true;
    if (view.timer !== null) {
      clearTimeout(view.timer);
      view.timer = null;
    }
    say(node, "");
  };

  const poll = async () => {
    if (view.stopped) {
      return;
    }
    try {
      const status = await rpc.call("wch_record_status", { camera });
      // **Asked again after the answer, not only before the call.** `stop()` happens while a
      // call is in flight every time an operator switches cameras — that is what it is for —
      // and a handle that wrote whatever came back afterwards put the *previous* camera's
      // sentence under the new camera's picture, in an element index.html gives one writer
      // precisely so it cannot be about something else (docs/11 **L36**, note **N156**). The
      // answer is not wrong and it is not a failure; it is about a camera nobody is looking at,
      // so it is dropped without a word.
      if (view.stopped) {
        return;
      }
      say(node, sentence(status), false);
    } catch (err) {
      // The same fence, on the same reasoning: a refusal about the previous camera is still
      // about the previous camera.
      if (view.stopped) {
        return;
      }
      // A call this page cannot make any more is not a fact about a recording, so this stops
      // rather than writing a sentence about a socket into a line about a camera. Anything
      // else — a camera that stopped resolving, a daemon that refused, an answer that took too
      // long — is reported here and the loop goes on, because the next answer may be different
      // and the text is idempotent.
      //
      // **Asked by identity, of `rpc.js`'s own sentinel.** The question here is "has this socket
      // ended", and until 2026-08-16 it was asked as "does this error carry no D13 name" — which
      // is also true of a call that merely timed out, and of jsonrpsee's own protocol refusals.
      // So a single slow answer on a live connection cleared this line and stopped the loop
      // permanently, in silence, until the operator re-selected the camera (note **N159**). The
      // sentinel cannot be forged by a daemon that sends a `kind` of `socket_closed`, which is
      // the reason it is a sentinel (`rpc.js`'s `SOCKET_CLOSED`).
      if (err.reason === SOCKET_CLOSED) {
        stop();
        return;
      }
      say(node, `this camera's recording state is unknown: ${why(err)}`, true);
    }
    if (!view.stopped) {
      // `setTimeout` after the answer rather than `setInterval`, so a slow daemon is polled
      // less often instead of accumulating a queue of calls a page will never catch up on.
      view.timer = setTimeout(poll, POLL_MS);
    }
  };
  poll();
  return { stop };
}

/**
 * One `RecordStatus`, as the sentence a person reads.
 *
 * Exported because it is the whole of this module that is worth asserting without a daemon: it
 * is a pure function from the document the wire carries to the text in the element, and every
 * decision about *what to say* is in it.
 */
export function sentence(status) {
  const take = status.take;
  if (take === null || take === undefined) {
    // No take, and no take that has ended and is waiting to be collected either — the two are
    // one answer on the wire and one answer here (note **N114**).
    return "";
  }
  if (take.ended !== null && take.ended !== undefined) {
    // A take this page watched finish. Said rather than blanked, because a picture that starts
    // moving again with no explanation is the same puzzle as one that stopped.
    return `a recording of this camera finished (${take.ended}); the preview is this camera's own stream again`;
  }
  return (
    `a recording owns this camera — ${seconds(take.elapsed_ms)} of ${seconds(take.budget_ms)}` +
    `; the picture is the recording's own frames`
  );
}

/** Milliseconds as a human's seconds. One decimal, because a take is seconds rather than hours. */
function seconds(ms) {
  return `${(ms / 1000).toFixed(1)} s`;
}

/**
 * A failure as this line names it: the D13 discriminant when there is one, and the message
 * otherwise.
 *
 * The fourth copy of three lines that app.js, controls.js, photo.js and calibration.js each carry,
 * and the duplication is real rather than hidden: `dom.js` is this client's one shared module and
 * it is a *DOM* helper, so a refusal renderer would be the wrong law in the only file every view
 * imports. What each copy is for is stated where it is — this one exists because a name is what a
 * reader can act on and `undefined:` in front of a sentence is what a page prints when it assumed
 * every failure carries one.
 */
function why(err) {
  const kind = err.kind ?? err.reason?.kind;
  return kind === null || kind === undefined ? err.message : `${kind}: ${err.message}`;
}

/** Write one status line, marking it as a failure or not. app.js's `say`, one element along. */
function say(node, text, failed = false) {
  node.classList.toggle("failed", failed);
  node.textContent = text;
}
