// The live preview: one `<img>`, pointed at the daemon's `multipart/x-mixed-replace` route.
//
// Design §2.6 and §2.7 both name the mechanism: `multipart/x-mixed-replace` is the only
// thing a browser will paint successive frames of into one element, and an element is all
// this needs — no canvas, no decoder, no `MediaSource`. The daemon frames the camera's own
// JPEG bytes (`daemon::http::preview`) and Chrome repaints on each part.
//
// ## Three things an `<img>` cannot do, and what each one costs this page
//
// **It cannot set a header**, so the credential rides the query string. That is not this
// module's preference; it is why `daemon::http::gate` accepts the query form at all (note
// **N74**), and `credential.js` is the one place that writes it.
//
// **It cannot read a response header.** `X-Wch-Frame-Index` and `X-Wch-Frame-Sequence` are
// on every part the daemon writes, and no page holding an `<img>` will ever see either.
// That matters because of what the sequence field does: `STREAMON` restarts the driver's
// numbering, so it **goes back to zero** every time the stream is stopped and started —
// which, since the owner's ruling of 2026-08-12, is what taking a photo during a preview
// does (note **N83**). A client that assumed that number monotonic would be wrong. This
// client cannot assume anything about it, because it cannot read it, and that is the whole
// of the answer: **the page does nothing with the frame sequence, by construction rather
// than by policy.** The place the reset is observable is a reader that parses the multipart
// stream itself — the daemon's own suite does, and P5d's browser rung could — and the place
// it is *counted* is the daemon (`Previews::watch_interrupted`), because from a viewer's
// side "a photo was taken" and "the device restarted" are the same event.
//
// **It cannot report a status.** The route projects the whole D13 vocabulary onto two
// statuses — `404` for a camera that does not exist, `503` for everything else
// (`daemon::http::preview`'s header argues why two and not eighteen) — and a failed image
// load reaches script as an `error` event with nothing in it. So a preview that will not
// start is reported here as the two possibilities it actually is, and the typed answer is
// asked for on the socket instead, where D13 is a value (see app.js). Guessing which one it
// was would be this page inventing the distinction AGENTS rule 7 exists to keep.
//
// ## Stopping is done by taking the source away
//
// The daemon retires a feed when its last reader goes (`crate::preview`'s driver), and what
// "goes" means for a browser is that the request ends. Clearing the attribute is how a page
// ends it, so switching cameras clears it first: an `<img>` left pointed at the previous
// camera is a second live stream, on a second device, that nobody is looking at.
//
// **Taking the element out of the document is not taking the source away**, and the difference
// is measured rather than assumed: Chromium goes on writing parts to a detached `<img>` that
// still carries a `src`, and an explicit collection does not end it either. [`stop`]'s doc
// carries what believing otherwise cost (note **N152**).

import { previewUrl } from "./credential.js";

/**
 * Point `img` at one camera's preview, and answer the element that is now watching it.
 *
 * The answer matters: [`stop`] replaces the node rather than mutating it, so a caller that
 * kept the element it passed in would be holding a detached one. `status` is where the two
 * things a page can actually observe are written: that the browser painted something, and
 * that it did not.
 */
export function watch(img, status, camera, token) {
  const frame = stop(img, status);
  frame.addEventListener("load", () => {
    status.classList.remove("failed");
    status.textContent = `streaming ${camera}`;
  });
  frame.addEventListener("error", () => {
    status.classList.add("failed");
    // Both candidates, named. The daemon knows which; a page holding an `<img>` does not.
    status.textContent =
      `the browser could not load the preview for ${camera}: either this daemon has no ` +
      `such camera (404) or the camera is not available right now — busy, gone, or refused ` +
      `(503). The daemon's own sentence is in the network panel; the socket answers the ` +
      `typed refusal.`;
  });
  status.classList.remove("failed");
  status.textContent = `opening a preview of ${camera}…`;
  frame.src = previewUrl(camera, token);
  return frame;
}

/**
 * End whatever this element was watching.
 *
 * **Two things happen here and only one of them ends the stream.** The clone is *listener*
 * hygiene; the `removeAttribute` on `img` is the abort. Keeping them apart in the reader's
 * head is worth a paragraph, because this function had them the wrong way round from P5c
 * until the G6 review measured it in the pinned Chromium (docs/11 **H4**, note **N152**): it
 * removed the attribute from the clone, which had never made a request, and detached the
 * original, which went on owning the response. A detached `<img>` is *not* an aborted
 * request — Chromium keeps writing parts to it, and collecting the garbage does not end it —
 * so every camera the tab had looked at stayed open and streaming on the daemon for the life
 * of the tab.
 *
 * `removeAttribute` rather than assigning the empty string: an empty source is a *request*
 * — for this document's own URL, which is the page, which the browser then fails to decode
 * as an image — where removing the attribute is the element having no source at all. The
 * difference is one pointless request to the daemon per camera switch, and one spurious
 * `error` event that would land in the status line above. That argument was always right;
 * it was applied to the wrong object.
 *
 * The order is deliberate too. The attribute goes first, so that the element that owns the
 * request is still the one this function was handed, whatever `replaceWith` does to it.
 */
export function stop(img, status) {
  // **The abort.** The daemon retires a feed when its last reader goes, and for a browser
  // "goes" means the request ends — so this line is the whole of what `Previews::release`
  // sees, and without it D12's idle close can never fire for a camera an operator looked at,
  // `limits::PREVIEW_MAX_VIEWERS_PER_CAMERA` is spent on viewers nobody is watching, and each
  // one is a camera the agent then meets as `Busy`.
  img.removeAttribute("src");
  // A fresh element, so the listeners attached by a previous `watch` go with the old one.
  // Reattaching to the same node would leave two `error` handlers writing two sentences
  // into one status line, which is the shape "the preview failed twice" takes when it
  // failed once. The clone is made *after* the attribute is gone, so it carries no source of
  // its own to start a second request with.
  const replacement = img.cloneNode(false);
  img.replaceWith(replacement);
  status.classList.remove("failed");
  status.textContent = "";
  return replacement;
}
