// The photo trigger: one `wch_photo`, and what came back.
//
// ## It does not touch the preview, and that is the whole design
//
// The obvious client stops the preview, takes the photo and starts the preview again. That
// design was **rejected by the owner** (2026-08-12, note **N83**) along with its sibling —
// surfacing `Busy` in the UI with a "stop the preview" affordance — for a reason this file
// is downstream of: sequencing an exclusive device from JavaScript puts the invariant in
// the layer least able to enforce it, and it fails outright the moment a second tab is
// previewing the same camera. What actually happens is `engine::preview::while_suspended`,
// inside the one actor thread that owns the device: the stream is stopped around the
// capture and started again on every path out, including the panicking one.
//
// So this module holds no reference to the preview element, and that absence is the
// feature. Two consequences a reader should expect rather than debug:
//
// - **The picture pauses.** Nothing is published while the capture runs, so nothing is
//   *dropped* either — the daemon counts the pause as its own thing
//   (`Previews::watch_interrupted`) rather than folding it into a slow-reader count.
// - **The driver's frame sequence restarts at zero**, because `STREAMON` resets it. No page
//   holding an `<img>` can see that number at all; preview.js says why, and the answer to
//   "what does the UI do about the reset" is "nothing, and it could not".
//
// ## What a photo costs on this transport, said out loud
//
// The sink is `return_bytes`, so the frame crosses the JSON-RPC socket as base64 (D10) and
// lands in this page as a `data:` URL. The alternative sink writes a file on the *daemon's*
// host, which for a browser is the wrong machine and, on the two non-loopback cells of
// D11's matrix, somebody else's. A 4K MJPG frame is about 8 MB and roughly 10.7 MB encoded,
// which is past jsonrpsee's default response ceiling — `api::photo`'s header records it —
// so a large photo is refused by the transport rather than by the camera, and the refusal
// says so.
//
// **The bytes never leave the tab.** Nothing here logs them, stringifies the answer, or
// puts them in a message: a frame may contain a person (AGENTS, "Hardware and privacy"),
// and a console line is a place bytes reach a screen somebody else is sharing.

import { el, fill } from "./dom.js";

/** The MIME types this page can hand a browser for the formats `wch_photo` writes. */
const PAINTABLE = { jpeg: "image/jpeg", png: "image/png" };

/**
 * Take one photo of `camera` and render it beside its report.
 *
 * Everything about the request except the sink is left to the daemon's defaults — the
 * stream negotiation, the settle policy \[PF:11\], the transform, and `wait`. That is a
 * decision: `PhotoRequest`'s fields all carry `#[serde(default)]` so a request that names
 * only its sink means exactly what a request written before those fields existed meant, and
 * a page that pinned a resolution here would be negotiating a format on an operator's
 * behalf without being asked.
 */
export async function take(rpc, camera, { status, frame, report }) {
  status.classList.remove("failed");
  status.textContent = "taking a photo — the preview pauses and resumes by itself (N83)…";
  fill(report, []);
  try {
    const answer = await rpc.call("wch_photo", {
      camera,
      request: { sink: { kind: "return_bytes", format: "jpeg" } },
    });
    show(answer, { status, frame, report });
  } catch (err) {
    status.classList.add("failed");
    // The D13 name, kept: `busy` and `device_gone` and `settle_timeout` are three different
    // things to do next, and "the photo failed" is none of them (AGENTS rule 7).
    status.textContent =
      err.kind === null || err.kind === undefined
        ? `the photo failed: ${err.message}`
        : `${err.kind}: ${err.message}`;
  }
}

/** Paint the answer: the picture if a browser can paint it, and the report either way. */
function show(answer, { status, frame, report }) {
  const delivery = answer.report.delivery;
  const mime = PAINTABLE[delivery.format];
  if (mime === undefined || answer.bytes === undefined) {
    // A `ppm` sink, or a daemon that answered a report with no bytes. Both are
    // representable on this wire and neither is a picture this page can show, so it says
    // which of the two happened rather than leaving an empty frame.
    frame.removeAttribute("src");
    status.textContent =
      answer.bytes === undefined
        ? "the daemon answered a report with no bytes in it"
        : `the photo is ${delivery.format}, which this page cannot paint`;
  } else {
    frame.src = `data:${mime};base64,${answer.bytes}`;
    status.classList.remove("failed");
    status.textContent = "";
  }
  fill(report, describe(answer.report));
}

/**
 * The report, as the sentences that are actually load-bearing.
 *
 * `rendering` first, because it is the one an operator most needs and the one a UI is most
 * tempted to drop: verbatim bytes are the camera's own bitstream and a re-encode is partly
 * this project's codec (E6, D6), and a calibration sample that was re-encoded is ranking
 * something different from one that was not.
 */
function describe(taken) {
  const negotiated = taken.negotiated;
  const lines = [
    `${taken.width}×${taken.height} · ${renderingSentence(taken.rendering)}`,
    `negotiated ${fourcc(negotiated.pixel_format)} ${negotiated.width}×${negotiated.height}` +
      ` at ${intervalLabel(negotiated.interval)}`,
    `${taken.frames_settled} frame(s) discarded while the sensor settled [PF:11]`,
    `${taken.delivery.byte_count} bytes, delivered as ${taken.delivery.kind}`,
  ];
  // Every way the device did not give what was asked for. An empty list is a claim
  // (`NegotiatedStream::is_exact`), so it is stated rather than left as a silence. The
  // discriminant is spelled `field` rather than `kind` on this one enum, which is the sort
  // of thing a hand-written client discovers by reading the emitted document.
  for (const adjustment of negotiated.adjustments ?? []) {
    lines.push(`the device chose its own ${adjustment.field}`);
  }
  return lines.map((line) => el("div", {}, line));
}

/** Which of D6's three paths produced these bytes. */
function renderingSentence(rendering) {
  switch (rendering.kind) {
    case "verbatim":
      return `the camera's own ${fourcc(rendering.source)} bytes, untouched`;
    case "decoded_and_encoded":
      return `decoded from ${fourcc(rendering.source)} and re-encoded as ${rendering.target}`;
    case "converted_and_encoded":
      return `converted from ${fourcc(rendering.source)} and encoded as ${rendering.target}`;
    default:
      return `rendered by a path this page does not know: ${rendering.kind}`;
  }
}

/**
 * A `schema::camera::PixelFormat`, as the four characters it is.
 *
 * The DTO is a transparent newtype over `[u8; 4]`, so it crosses JSON as an array of four
 * numbers and every client renders the FourCC itself. `PixelFormat`'s `Display` does this in
 * Rust, and a browser cannot reach it — the same shape `ListHint::message` has.
 */
function fourcc(format) {
  return format.map((byte) => (byte >= 0x20 && byte < 0x7f ? String.fromCharCode(byte) : "·")).join("");
}

/** A `schema::camera::FrameInterval`, as frames per second where it has one. */
function intervalLabel(interval) {
  if (interval.kind === "discrete") {
    return `${(interval.denominator / interval.numerator).toFixed(0)} fps`;
  }
  // Stepwise and continuous intervals have no single rate; naming the kind is what is
  // actually known about them.
  return interval.kind;
}
