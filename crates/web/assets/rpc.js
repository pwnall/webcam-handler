// JSON-RPC over one WebSocket: calls, subscriptions, and the typed refusal.
//
// Design §2.7 budgets this at "~50 lines", and the budget is the design saying what kind of
// thing this is: a helper, not a framework. **It is overshot, and not by rounding.** `connect`
// is 112 lines of code — non-blank, non-comment, from its signature to its closing brace, which
// is the whole of what the budget is about. Sixty of them arrived on 2026-08-16 with the two
// liveness bounds the owner ruled for, and note **N158** prices the overshoot, says what it
// bought and says what would end it; the number in this sentence is **counted rather than
// claimed**, and counted by `crates/daemon/tests/web_client.rs`, which counts the function in
// the bytes this daemon serves and goes red on a header that states a size this file does not
// have. A count in a comment with nothing to check it is the defect this same repair pass closed
// one crate along (note **N153**), and it is not cheaper here for being about a module.
//
// The reason the rest of it can stay small is that jsonrpsee already owns the hard half —
// framing, batching, the subscribe/unsubscribe protocol, `-32601` for a name that is not
// registered — and a client that re-implemented any of it would be a second opinion about a
// protocol `webcam-handler-api` has exactly one home for (D10, T5).
//
// ## What it does, in three sentences
//
// A call is an id, a promise parked under it, and a frame on the wire; an answer with that
// id resolves or rejects it. A notification carries `params.subscription` instead of an id,
// so it is routed to whichever stream registered that id — which is how a page holding two
// subscriptions on one socket tells them apart, and jsonrpsee's subscriptions are per
// client rather than per session precisely so that a client must (`crates/api`'s
// `WchEvents`). A stream that ends abnormally arrives as `params.error`, because after the
// accept there is no JSON-RPC code left to carry a refusal on.
//
// And a socket that closed answers **everything at once**: every call still waiting is
// rejected, every call made afterwards is rejected before it is written, and every
// subscription is told. Nothing this module hands out stays quiet, because the one failure a
// page cannot render is the one it was never told about.
//
// ## …and a socket that did not close is the failure that sentence used to miss
//
// Every promise above hangs off the `close` event, and a link cut without a FIN never fires
// one. `readyState` stays `OPEN`, `send()` goes on succeeding into nothing, and the page's
// story about itself stops being false and starts being *absent*: calls park for ever, the
// banner reads `connected`, and `#photo-status` reads "taking a photo …" until the tab is shut
// (docs/11 **L38**, note **N157**). Until 2026-08-16 the paragraph above claimed that could not
// happen, which is the honest reason to record what closed it.
//
// The owner's ruling is two bounds, and they answer two different questions:
//
// - **[`CALL_TIMEOUT_MS`] bounds one call.** A promise nobody will settle is the same spinner
//   the `close` handler refuses to leave behind, arriving by a different road, so a call that
//   is not answered inside `schema::limits::CLIENT_REQUEST_TIMEOUT_MS` is rejected with a
//   sentence naming the wait. It does **not** end the connection: two minutes is a bound on
//   patience, not evidence about a socket, and a `wch_controls` queued behind a sweep is a
//   daemon working rather than a daemon gone (E3 — a timeout is not a verdict on the machine).
//   The rejection says which of the two it is (`err.reason`), because a consumer that could not
//   tell them apart acted on the wrong one (note **N159**).
// - **[`HEARTBEAT_MS`] bounds silence.** A page nobody is clicking on makes no calls, so the
//   bound above never fires and nothing would ever notice; the heartbeat is what puts a
//   question on an idle socket so that there is something to time out. A heartbeat that is
//   still unanswered when the next one falls due **does** end the connection, and it may,
//   because it is the one call on this surface that reaches no camera: `wch_ping` is not a
//   registered method, jsonrpsee answers `-32601` without a handler running, and silence in
//   answer to a question no device could slow down is a fact about the socket.
//
// ## The refusal is a value, not a string
//
// `RpcError` carries the D13 discriminant off `error.data.kind`, and every consumer in this
// client branches on *that* rather than on the message. It matters more here than it looks:
// `busy`, `device_gone` and `permission_denied` are three different facts about a machine
// and none of them means "this camera cannot" (AGENTS rule 7, design E3), so a UI that
// rendered them all as "unavailable" would be performing the exact collapse the error
// registry exists to prevent. The numeric `code` is kept beside it and used by nothing here
// — `data.kind` is a name and a name survives a renumbering.
//
// ## Two things a browser will not tell this module, recorded because they shape the UI
//
// A failed WebSocket *handshake* is not observable from script: the `error` event carries
// no status, so a page cannot tell "the token is wrong" (`401`) from "nothing is listening"
// (a refused connection) — the browser deliberately withholds it. `daemon::http::gate`'s
// `401` is a real, well-formed answer with a challenge and a sentence in it, and none of
// that reaches this file. So the failure this module reports is honestly vague, and app.js
// says the two candidates out loud rather than picking one.
//
// And the failure message must not contain the URL, because the URL is the token (D11).
// That is the one thing in this file that would otherwise be idiomatic and wrong: a
// `throw new Error(\`could not open ${url}\`)` writes a live camera's credential into the
// console, where it stays for as long as the tab does.

/**
 * What a closed socket says, in the one place it is spelled.
 *
 * Three things say it — the calls waiting at the close, the calls made after it, and every
 * registered subscription — because it is one fact about one socket. A page that spelled it
 * three ways would be three different-looking failures with a single cause.
 */
const CLOSED_MESSAGE = "the connection to webcam-handler-daemon closed";

/**
 * How long one call waits for its answer, in milliseconds.
 *
 * `schema::limits::CLIENT_REQUEST_TIMEOUT_MS`, which is `webcam-handler-client`'s number
 * because it is an answer to the same question — "how long before a client that has lost its
 * daemon says so" — and that constant's own doc prices it against the longest ordinary verb on
 * this surface. A browser cannot `use` a Rust constant, so this is a second copy of one, and
 * `crates/daemon/tests/web_client.rs` reconciles the two rather than trusting them.
 */
const CALL_TIMEOUT_MS = 120000;

/**
 * How long this socket may be silent before the page asks it something.
 *
 * `schema::limits::CLIENT_WS_HEARTBEAT_MS`, reconciled with it in the same place as the number
 * above.
 */
const HEARTBEAT_MS = 15000;

/**
 * What the heartbeat asks, and the reason it is a name this daemon does not answer.
 *
 * The question is "is this socket carrying anything", and the cheapest true answer to it is a
 * frame that comes back without a handler having run: jsonrpsee answers `-32601` for a method
 * that is not registered (this module's header), which reaches no camera, opens no device and
 * reads no store. So a heartbeat costs the daemon a parse and a reply, and its *silence* cannot
 * be a slow camera — which is what makes it evidence about the connection rather than about the
 * machine (AGENTS rule 7).
 *
 * A build that later registers this name breaks nothing here: any answer at all is the answer
 * this asks for, and [`connect`] treats a refusal exactly as it treats a result.
 */
const HEARTBEAT_METHOD = "wch_ping";

/** What a call that was never answered says. The wait is named, because the wait is the fact. */
function timedOutMessage(method) {
  return (
    `webcam-handler-daemon did not answer ${method} within ${CALL_TIMEOUT_MS} ms; the ` +
    "connection may still be open and carrying nothing"
  );
}

/**
 * The reason a subscription is handed, and a call is rejected with, when the socket goes out
 * from under it.
 *
 * A **sentinel compared by identity**. A stream that ends for its own reasons arrives as the
 * daemon's own `params.error` — `shutting_down` and a failed source are different advice,
 * which is the whole reason the daemon spends a notification on it rather than closing the
 * socket — and there is no such frame for a socket that simply ended. Every other reason is
 * parsed out of a frame and no parsed object is ever `===` this one, so a consumer branching
 * on it cannot be fooled by a daemon that happens to send a `kind` of `socket_closed`. The
 * fields are there because a consumer that renders a reason rather than branching on it must
 * still get a sentence out of this one.
 *
 * **It rides a rejected call as `err.reason` as well**, which is one value for one fact rather
 * than two vocabularies for it: the calls waiting at the close, the calls made after it and
 * every registered subscription are three arrivals of "this socket ended", and a consumer that
 * has to *do* something about that — `assets/recording.js` retires its poll loop — asks the same
 * question of all three.
 */
export const SOCKET_CLOSED = Object.freeze({
  kind: "socket_closed",
  message: CLOSED_MESSAGE,
});

/**
 * …and the reason a call is rejected with when *it* ran out of patience.
 *
 * A second sentinel rather than a second use of the first, because the two are different advice
 * and one consumer already acts on the difference. A closed socket is a page that can make no
 * further call — `recording.js` stops polling, and stopping is right. A call that timed out is
 * one slow answer on a connection that is still open and may still work, and the loop that meets
 * it should say so and ask again. Until 2026-08-16 both were a bare `Error`, so `err.kind` was
 * `undefined` for each and the one consumer branching on that shape read a slow daemon as a dead
 * one and retired its line, in silence, for the life of the tab (note **N159**).
 *
 * No `message` of its own: the wait is named per call by [`timedOutMessage`], and a sentinel
 * carrying a second, vaguer sentence would be a sentence somebody eventually renders instead.
 */
const CALL_TIMED_OUT = Object.freeze({ kind: "call_timed_out" });

/**
 * One failure of this module's own making, carrying the sentinel a consumer branches on.
 *
 * `Error` for the message, `reason` for the identity. A subclass per case would be `instanceof`
 * doing the same work with more surface, and the sentinels already exist because subscriptions
 * need them.
 */
function failure(reason, message) {
  return Object.assign(new Error(message), { reason });
}

/** A D13 refusal, as something a caller can branch on. */
export class RpcError extends Error {
  constructor(object) {
    super(object.message ?? "webcam-handler-daemon refused the call");
    this.name = "RpcError";
    this.code = object.code;
    this.data = object.data;
    // `null` when the refusal is not D13's — jsonrpsee's own `-32601`/`-32602` carry no
    // typed payload, and pretending they carry one would invent a device error out of a
    // protocol mistake (E3 again, one layer down).
    this.kind = object.data?.kind ?? null;
  }
}

/** Open one connection and answer the two verbs this client needs on it. */
export async function connect(url, { onClose } = {}) {
  const socket = new WebSocket(url);
  const pending = new Map();
  const streams = new Map();
  let next = 1;
  // Whether this socket ever opened, which decides whether `onClose` is a true statement.
  //
  // A refused handshake fires `error` **and then** `close`, so without this the caller was
  // told "the connection closed" about a connection it never had — and since `connect` rejects
  // on the same event, the caller's own, more accurate sentence was written first and then
  // overwritten by that one. P5d's browser rung found it: a page opened without a token said
  // "the connection to webcam-handler-daemon closed; reload the URL webcam-handler-daemon
  // printed" instead of naming the two things that are actually wrong, which is the sentence
  // an operator needs in the most common failure this page has. `onClose` means "the
  // connection I gave you has ended"; a connection that never started has not ended.
  let opened = false;
  // …and whether it has since ended, which is what makes this handle refuse rather than park.
  let closed = false;
  // When this socket last said anything at all, which is what "idle" is measured from. Every
  // frame counts and not only answers: a notification off a subscription is a daemon writing
  // to this socket, which is the whole of what the heartbeat is trying to find out.
  let heard = Date.now();
  // Whether a heartbeat has gone out and not come back. One bit rather than a timestamp,
  // because the only question asked of it is at the next tick and the tick is the clock.
  let beating = false;
  // The idle check's timer, declared here and started after the handshake. `null` is a state
  // that really happens: a refused handshake fires `close`, so the handler below runs on a
  // connection that never had a heartbeat to stop.
  let heartbeat = null;

  socket.addEventListener("message", (event) => {
    heard = Date.now();
    const frame = JSON.parse(event.data);
    const waiting = pending.get(frame.id);
    if (waiting !== undefined) {
      pending.delete(frame.id);
      if (frame.error === undefined) {
        return waiting.resolve(frame.result);
      }
      return waiting.reject(new RpcError(frame.error));
    }
    const stream = streams.get(frame.params?.subscription);
    if (stream === undefined) {
      return undefined;
    }
    if (frame.params.error === undefined) {
      return stream.event(frame.params.result);
    }
    streams.delete(frame.params.subscription);
    return stream.ended(frame.params.error);
  });

  socket.addEventListener("close", () => {
    closed = true;
    // The heartbeat exists to *reach* this handler on a socket that would never have got here
    // on its own; once here it has nothing left to ask, and a timer left running would be a
    // dead connection still spending a turn of the event loop every fifteen seconds for as
    // long as the tab lives.
    if (heartbeat !== null) {
      clearInterval(heartbeat);
      heartbeat = null;
    }
    // Every call still waiting is refused rather than left pending: a promise nobody will
    // ever settle is a spinner that spins forever, which is the shape a stopped daemon
    // would otherwise take in this page.
    for (const waiting of pending.values()) {
      waiting.reject(failure(SOCKET_CLOSED, CLOSED_MESSAGE));
    }
    pending.clear();
    // …and every subscription is **told**, rather than dropped on the floor. `streams.clear()`
    // on its own discarded every registration without calling one `ended`, so a view whose
    // stream had stopped existing went on saying it was watching — which is precisely the
    // defect index.html's two calibration status lines were split apart for (E16 §1): "a
    // calibration view that had silently stopped being live looked exactly like a healthy
    // one". app.js's hotplug watch died the same way and more quietly still, since neither its
    // retry nor its "the device-change stream ended twice" sentence could fire without it.
    for (const stream of streams.values()) {
      stream.ended(SOCKET_CLOSED);
    }
    streams.clear();
    // Last, and the order is the point: an `ended` handler writes about a *stream*, `onClose`
    // writes about the *connection* every stream was on, and the second is the sentence that
    // should be on screen when both have run.
    if (opened) {
      onClose?.();
    }
  });

  const refused = () => new Error("webcam-handler-daemon did not accept a WebSocket");
  await new Promise((resolve, failed) => {
    socket.addEventListener(
      "open",
      () => {
        opened = true;
        resolve();
      },
      { once: true },
    );
    socket.addEventListener("error", () => failed(refused()), { once: true });
  });

  const handle = {
    call(method, params = {}) {
      // **The one line that stops a dead handle from being a spinner.** WHATWG makes
      // `WebSocket.send()` throw only while the socket is still `CONNECTING`; on `CLOSING` and
      // `CLOSED` it **discards the frame and returns**, so a call made one instant after the
      // close would park a promise under an id no answer can ever carry — the exact shape the
      // `close` handler above refuses to leave behind, arriving by the next line of the same
      // page. Refusing here rather than at each caller is what makes it one decision: both
      // consumers of this handle already render a refusal, and neither can render a wait.
      //
      // `readyState` is asked as well as the flag because the two are not the same instant: a
      // socket enters `CLOSING` when something calls `close()`, and the `close` event that
      // sets the flag arrives afterwards. (Playwright's `routeWebSocket` mock throws where a
      // real socket discards — `playwright-core`'s `WebSocketMock.send`, measured 2026-08-14 —
      // so the browser rung sees this defect as a transport exception surfacing in a status
      // line rather than as the spinner an operator would meet. The refusal below is what
      // makes the two hosts agree.)
      if (closed || socket.readyState !== WebSocket.OPEN) {
        return Promise.reject(failure(SOCKET_CLOSED, CLOSED_MESSAGE));
      }
      const id = next++;
      return new Promise((resolve, reject) => {
        // **The bound on one call, which is the other half of what a dead socket used to do.**
        // The `close` handler refuses every promise it can see; a socket that never closes is
        // never seen, so the wait is bounded here as well. It is a bound on *this call* and not
        // on the connection: the timer is cleared by whichever of the two settlements arrives,
        // and the `pending` entry is removed with it so a very late answer finds no id to
        // resolve — which is the same rule the message handler already keeps.
        //
        // It is rejected as a **timeout and not as a closure**, which matters to exactly one
        // consumer and mattered enough to be a finding: a bare `Error` here has the same shape a
        // closed socket's rejection had, so `recording.js` — which stops its poll loop for good
        // on "a call this page cannot make any more" — retired the recording line on one slow
        // answer, without a word (note **N159**).
        const overdue = setTimeout(() => {
          pending.delete(id);
          reject(failure(CALL_TIMED_OUT, timedOutMessage(method)));
        }, CALL_TIMEOUT_MS);
        pending.set(id, {
          resolve: (result) => {
            clearTimeout(overdue);
            resolve(result);
          },
          reject: (err) => {
            clearTimeout(overdue);
            reject(err);
          },
        });
        socket.send(JSON.stringify({ jsonrpc: "2.0", id, method, params }));
      });
    },
    async subscribe(method, event, ended) {
      const id = await handle.call(method);
      streams.set(id, { event, ended });
      return id;
    },
  };

  /**
   * The idle check, and the only thing in this module that ends a connection of its own accord.
   *
   * Three states, asked in this order at every tick:
   *
   * 1. **something crossed recently** — nothing to do, and the ordinary case for any page that
   *    is showing a preview, since `assets/recording.js` polls once a second;
   * 2. **silent, and a heartbeat is already outstanding** — the question was asked a whole
   *    interval ago and the one call on this surface that cannot be slowed down by a camera has
   *    not come back, so the socket is not carrying anything. `close()` rather than a sentence
   *    of its own: the close handler above is where every waiting call is refused, every
   *    subscription is told and `onClose` writes the page's one statement about the connection,
   *    and a second story about a dead socket would be a second story to keep true;
   * 3. **silent, and nothing outstanding** — ask.
   *
   * So a severed link is noticed within two ticks of the last frame plus however long the first
   * tick lands after it, which is under three times [`HEARTBEAT_MS`] — a bound where this page
   * had none at all.
   *
   * Started after the handshake rather than beside the socket, because a connection that was
   * never accepted has no silence to measure: [`connect`] throws above this line and the caller
   * is told what it was told.
   */
  heartbeat = setInterval(() => {
    if (closed) {
      return;
    }
    if (Date.now() - heard < HEARTBEAT_MS) {
      return;
    }
    if (beating) {
      socket.close();
      return;
    }
    beating = true;
    // Any answer is the answer: a `-32601` refusal proves the socket exactly as a result would,
    // so this rejects only on the timeout or the close, and both of those are already somebody
    // else's sentence. Swallowed rather than reported for that reason — a heartbeat is not a
    // call this page made on anyone's behalf.
    handle
      .call(HEARTBEAT_METHOD)
      .catch(() => {})
      .finally(() => {
        beating = false;
      });
  }, HEARTBEAT_MS);

  return handle;
}
