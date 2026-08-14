// JSON-RPC over one WebSocket: calls, subscriptions, and the typed refusal.
//
// Design §2.7 budgets this at "~50 lines", and the budget is the design saying what kind of
// thing this is. `connect` is fifty-three lines of code and `RpcError` is ten more, counted
// rather than claimed. The reason it can be that small is that jsonrpsee already owns the
// hard half — framing, batching, the subscribe/unsubscribe protocol, `-32601` for a name
// that is not registered — and a client that re-implemented any of it would be a second
// opinion about a protocol `webcam-handler-api` has exactly one home for (D10, T5).
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
 * The reason a subscription is handed when the socket goes out from under it.
 *
 * A **sentinel compared by identity**. A stream that ends for its own reasons arrives as the
 * daemon's own `params.error` — `shutting_down` and a failed source are different advice,
 * which is the whole reason the daemon spends a notification on it rather than closing the
 * socket — and there is no such frame for a socket that simply ended. Every other reason is
 * parsed out of a frame and no parsed object is ever `===` this one, so a consumer branching
 * on it cannot be fooled by a daemon that happens to send a `kind` of `socket_closed`. The
 * fields are there because a consumer that renders a reason rather than branching on it must
 * still get a sentence out of this one.
 */
export const SOCKET_CLOSED = Object.freeze({
  kind: "socket_closed",
  message: CLOSED_MESSAGE,
});

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

  socket.addEventListener("message", (event) => {
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
    // Every call still waiting is refused rather than left pending: a promise nobody will
    // ever settle is a spinner that spins forever, which is the shape a stopped daemon
    // would otherwise take in this page.
    for (const waiting of pending.values()) {
      waiting.reject(new Error(CLOSED_MESSAGE));
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

  return {
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
        return Promise.reject(new Error(CLOSED_MESSAGE));
      }
      const id = next++;
      return new Promise((resolve, reject) => {
        pending.set(id, { resolve, reject });
        socket.send(JSON.stringify({ jsonrpc: "2.0", id, method, params }));
      });
    },
    async subscribe(method, event, ended) {
      const id = await this.call(method);
      streams.set(id, { event, ended });
      return id;
    },
  };
}
