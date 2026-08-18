// The bearer token, and the only two URLs in this client that carry it.
//
// ## Where the token comes from
//
// `webcam-handler-daemon` prints a ready-to-open URL and an operator opens it, so the
// credential arrives as this document's own query string (D11). `location.search` is where the
// page reads it from, and that is not a new exposure: every script running in this origin
// could already read it, and `daemon::http::rpc`'s header prices the three residuals that are
// real (the devtools network panel, an intermediary's access log, the browser history — the
// last of which is the *navigation's* cost rather than this module's).
//
// ## Why the token is in a URL and not in a header
//
// Not a preference — the two things this page has to reach cannot carry a header at all. A
// `new WebSocket(url)` takes a URL and a subprotocol list and nothing else, and an `<img>`
// takes an attribute; neither has an API for `Authorization`. The query form is what
// `daemon::http::gate` already accepts, and note **N74** is the rule it accepts it under.
//
// ## The rule this module exists to keep: one credential per request, and only where the
// camera is
//
// N74's gate admits a request only when *every* credential it presents verifies. That
// makes "belt and braces" — a token in the query *and* in a header, or twice in the query —
// a request that is refused the moment the two spellings drift, and there is no reason for
// this client to ever send two. So there is exactly one place in this directory that writes
// the token into anything, it writes it once per URL, and the URLs it writes are the two
// routes on `daemon::http::CAMERA_BEARING_PATHS`.
//
// The other half of the same rule is an absence: **nothing sends the token to a static
// asset.** Since the owner's ruling of 2026-08-12 the client's own files are served
// unauthenticated (note **N82**, D11's amendment), so a credential attached to `app.css`
// would be a secret handed to a route that does not check it and cannot use it — one more
// copy of it in one more log, bought for nothing. The module graph is ordinary `import`
// statements for exactly that reason.

/** D11's query parameter, spelled as `daemon::http::token::TOKEN_QUERY_PARAM` spells it. */
const TOKEN_PARAM = "token";

/** `daemon::http::rpc::RPC_PATH`. */
const RPC_PATH = "/rpc";

/** `daemon::http::preview::PREVIEW_PATH`. */
const PREVIEW_PATH = "/preview";

/** `daemon::http::samples::SESSION_PHOTO_PATH` — the third camera-bearing route (D20). */
const SESSION_PHOTO_PATH = "/session-photo";

/** `daemon::http::preview::CAMERA_QUERY_PARAM`. */
const CAMERA_PARAM = "camera";

/**
 * The token this page was opened with, or `null` when it was opened without one.
 *
 * `null` is a real and reachable answer rather than a defensive branch, and it has two
 * causes worth telling apart in the caller: D11's one token-less cell
 * (`--http-insecure-loopback`, where the daemon mints no token and the gate is not
 * installed at all — note **N75**), and an operator who opened `/` by hand after reading
 * the port off a log. The first works; the second reaches a `401` on the socket. This
 * module cannot tell them apart — nothing served to this page says which cell the daemon
 * is in — so it answers what it knows and app.js says what it means.
 */
export function tokenFromLocation() {
  return new URLSearchParams(location.search).get(TOKEN_PARAM);
}

/**
 * The WebSocket URL for this daemon's JSON-RPC endpoint.
 *
 * Built from `location` rather than from anything configured: the page came from the
 * daemon it is about, so its own origin is the answer, and a client with a *configurable*
 * daemon address would be a client that can be pointed at somebody else's camera by
 * whoever writes the query string. The scheme is the document's own, upgraded — `wss:` for
 * an `https:` document — which costs nothing today (D11 binds loopback) and is what stops
 * this from being the line that downgrades a connection if a reverse proxy ever appears in
 * front of it (note **N79** names that shape).
 */
export function wireUrl(token) {
  const scheme = location.protocol === "https:" ? "wss:" : "ws:";
  const origin = `${scheme}//${location.host}${RPC_PATH}`;
  // A `null` token presents **nothing**, rather than an empty one. N74's rule is that every
  // credential a request presents must verify and it must present at least one, and it
  // counts a malformed credential as one that *failed* rather than as none at all — so
  // `?token=` with nothing after it is a wrong credential, which is a different request
  // from an anonymous one even though both meet a `401`. In D11's token-less cell, where
  // there is no gate to meet, it would be a parameter with no meaning at all.
  return token === null ? origin : `${origin}?${TOKEN_PARAM}=${encodeURIComponent(token)}`;
}

/**
 * The `<img>` source for one camera's MJPEG preview.
 *
 * Relative, so the browser resolves it against this document's origin and the scheme
 * question above does not arise twice. `encodeURIComponent` on the camera id because D1's
 * ids contain a `:` (`cam:obsbot-tiny`); `daemon::http::preview` percent-*decodes* the
 * camera and deliberately does not decode the token, and its header says why the asymmetry
 * is right — more spellings of a name cost nothing, more spellings of a secret are the one
 * place more ways is the wrong direction.
 */
export function previewUrl(camera, token) {
  const query = new URLSearchParams();
  query.set(CAMERA_PARAM, camera);
  if (token !== null) {
    // [`wireUrl`]'s reason: no credential is not an empty credential.
    query.set(TOKEN_PARAM, token);
  }
  return `${PREVIEW_PATH}?${query}`;
}

/**
 * The `<img>` source for one calibration sample, addressed by reference (design D20).
 *
 * **No path crosses this function**, and that is the whole shape of the route: the caller
 * names a session, a control, a sweep pass and the value the sample was taken at, and the
 * daemon derives the file from the session store's own layout rules (D9). A page that sent
 * `sample.photo` — the relative path the session document carries — would be handing caller
 * text to a filesystem, which is the one thing the route is built not to do.
 *
 * The token rides the query for `previewUrl`'s reason: an `<img>` sets no headers, and this
 * route serves stored camera frames, so it is behind the same gate as its two siblings.
 */
export function samplePhotoUrl({ session, control, pass, value }, token = tokenFromLocation()) {
  const query = new URLSearchParams();
  query.set("session", session);
  query.set("control", control);
  query.set("pass", String(pass));
  query.set("value", String(value));
  if (token !== null) {
    query.set(TOKEN_PARAM, token);
  }
  return `${SESSION_PHOTO_PATH}?${query}`;
}
