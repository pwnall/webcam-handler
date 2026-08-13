//! The MJPEG preview route: `multipart/x-mixed-replace`, one part per frame, ending when the
//! client goes away or the daemon does (design **D11**, **D12**, §2.6; docs/7 P5b).
//!
//! [`super::rpc`] is this module's neighbour and the two are the daemon's only routes that
//! carry a camera. That one hands a request to somebody else's service; this one owns its
//! response from the first byte to the last, because there is no library between a `DQBUF` and
//! a socket here — `crate::preview` publishes the camera's own JPEG bytes into a latest-frame
//! channel and this module frames them.
//!
//! ## This route is gated because it carries camera frames
//!
//! **Permanently, and by name.** The owner ruled (2026-08-12) that static assets are served
//! without authentication — they are open-source code, not a secret — and that only the
//! resources which carry or drive the camera stay behind D11's bearer token: the WebSocket
//! endpoint and this one (note **N82**). Since that ruling landed, the gate over this route is
//! no longer an accident of where it sits in a router — it is `super::listener::router`'s
//! `route_layer` over the routes, and this route is one of them. What keeps it that way is a
//! pair, because neither half can see what the other does:
//! `crates/daemon/tests/preview.rs`'s `every_camera_bearing_route_is_behind_the_gate` drives
//! every path in [`super::CAMERA_BEARING_PATHS`] over a real socket and requires the refusal,
//! and `scripts/gates/web-routes-are-gated.sh` requires every route registration in this crate
//! to name a path on that list — so a *third* route added without one is a finding rather than
//! a discovery.
//!
//! What is behind the credential here is not a document: it is a live view of whatever the
//! camera is pointed at, which on a laptop is a person's room (AGENTS: "a frame may contain a
//! person"). D11's own sentence is that "loopback + token because loopback alone is not an
//! auth boundary on a multi-user machine", and this is the route that sentence is most about —
//! a token-less preview on `127.0.0.1` is a webcam served to every process on the host.
//!
//! ## Why the response is written by a task rather than by this handler
//!
//! The body is a `tokio::sync::mpsc::Receiver` of already-framed parts, filled by a task that
//! owns the [`Viewer`], and the split buys two things that are hard to get any other way.
//!
//! **A bound on what one slow reader makes the daemon hold.**
//! [`limits::PREVIEW_READER_QUEUE_DEPTH`] is the channel's capacity, so a client that has
//! stopped reading pins one framed part plus whatever the kernel's socket buffer took — and
//! the *frames* it is not reading are not queued anywhere at all, because the latest-frame
//! channel behind [`Viewer::next`] keeps one value and overwrites it (`crate::preview`'s
//! header proves that property). The task then finds the newest frame when it comes back,
//! which is what "drops frames and never backpressures the device" (D12) means at this end of
//! the pipe.
//!
//! **A stop that does not depend on the client.** The writer watches the daemon's
//! [`crate::shutdown::Shutdown`] on *both* of the two places it can wait — for a frame, and
//! for room in the channel — because the second is where a stalled tab parks it. That is the
//! whole of "an open MJPEG tab must not hang shutdown" (design §2.6): `axum::serve`'s graceful
//! shutdown waits for a response that is being written, so a body that only ended when its
//! reader did would hold the listener open for as long as somebody's browser stayed open.
//! When the writer ends it drops its sender, the stream ends, the body ends, and the
//! connection finishes.
//!
//! ## Compression is excluded here, and the exclusion is structural
//!
//! `tower_http::CompressionLayer` arrives with this sub-milestone and is applied to the asset
//! half of the listener (`super::listener::router`). It is **not** applied to this route,
//! and that is composition rather than configuration: this route is merged into the router
//! after the layer has been applied to the others, so there is no predicate to get wrong and
//! no content-type list to keep current. `CompressionLayer`'s own default predicate would
//! happily compress this response — it declines `image/*`, and `multipart/x-mixed-replace` is
//! not `image/*` — which is exactly why the exclusion cannot be left to a default.
//!
//! What it would cost is not bytes. A compressing middleware buffers, and the thing it would
//! be buffering is the frame boundary a browser's `<img>` repaints on; the picture would
//! arrive in bursts, or after the stream ended, which for a live preview is the same as not
//! arriving. Neither is a thing gzip is *wrong* about — it is a thing gzip is not for.
//!
//! ## What this route answers with, and why it is two statuses rather than eighteen
//!
//! D13's vocabulary crosses the JSON-RPC wire with a code a client branches on. An `<img
//! src="/preview?camera=…">` branches on nothing: it either paints or fires `onerror`, and
//! the only thing a page can read is the status. So this route projects the whole vocabulary
//! onto **two** statuses — `404` for a camera that does not exist or whose name was ambiguous,
//! which is the one refusal a client can fix by asking for something else, and `503` for
//! everything else — and carries the D13 message in the body for the person reading devtools.
//!
//! That is deliberately *not* an exhaustive match. [`super::gate::REFUSAL`]'s doc argues the
//! same point from the other side: inventing an HTTP projection of the error registry here
//! would be a second home for the error law (design §2.10), and this route has no method
//! behind it to name in one. A projection with two arms and a paragraph is a projection; an
//! eighteen-arm match would be a registry.

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use schema::camera::CameraId;
use schema::{Error, ErrorKind, limits};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::preview::{Previews, Shot, Viewer};

/// One framed multipart part, as the response body carries it.
///
/// `Result` because `axum::body::Body::from_stream` takes a `futures_core::TryStream`, and
/// [`std::convert::Infallible`] because there is no error this body could report: a
/// `multipart/x-mixed-replace` response has no in-band way to say "something went wrong" —
/// the status line was written before the first frame existed — so every way this stream can
/// stop is an *end*, and the reader learns which one from the daemon's log or from asking
/// again. Naming the impossible variant is what stops a later edit inventing an error channel
/// the protocol cannot carry.
type Part = Result<Vec<u8>, std::convert::Infallible>;

/// The path the MJPEG preview answers on.
///
/// One spelling, with three readers that must agree — the route below, the suite, and P5c's
/// client, which builds `/preview?camera=…&token=…` from the URL its page was opened at. It is
/// also one of [`super::CAMERA_BEARING_PATHS`], which is the list that says this route stays
/// gated whatever a future router looks like.
pub const PREVIEW_PATH: &str = "/preview";

/// The query parameter naming the camera to preview.
///
/// `camera`, matching the parameter name every T5 method uses for the same thing (`crates/api`)
/// — a browser building this URL beside a JSON-RPC call should not have to remember that one
/// of them spells it differently.
pub const CAMERA_QUERY_PARAM: &str = "camera";

/// The boundary between parts.
///
/// A constant rather than a per-response random token, and the trade is worth stating.
/// Randomness buys protection against a payload that *contains* the delimiter; every part this
/// route writes carries a `Content-Length`, so a conforming reader never scans for the
/// delimiter inside a part and the protection has nothing to protect. What a constant buys is
/// a test that can assert the framing against a value rather than against a pattern it parsed
/// out of the response it is checking.
///
/// No `-` characters and no characters needing quoting in the `Content-Type` parameter, so the
/// header this builds is well-formed without a quoting rule of its own.
pub const BOUNDARY: &str = "wchframeboundary";

/// The content type of the stream itself.
///
/// Design §2.6 names it. `multipart/x-mixed-replace` is the only thing a browser will paint
/// successive frames of into one `<img>` element; it is not registered with IANA and never has
/// been, which is why the `x-` is part of the name rather than a leftover.
const STREAM_CONTENT_TYPE: &str = "multipart/x-mixed-replace";

/// The content type of one part.
const FRAME_CONTENT_TYPE: &str = "image/jpeg";

/// The header carrying this daemon's publication index for a part.
///
/// A reader that goes from index 7 to index 300 skipped 292 frames, and this is how it can
/// *see* that rather than infer it — which is what makes the drop semantics observable from
/// outside the daemon (docs/7 P5b's stalled-reader criterion). An unknown part header is
/// ignored by every browser, so it costs a client that does not want it nothing.
const FRAME_INDEX_HEADER: &str = "X-Wch-Frame-Index";

/// The header carrying the driver's own sequence number for a part.
///
/// A different fact from [`FRAME_INDEX_HEADER`] and kept beside it for that reason: a gap here
/// is frames the *kernel* dropped before this daemon saw them, and a gap there is frames this
/// daemon published and the reader was too slow for. Conflating the two would leave a client
/// unable to tell "my machine is busy" from "the camera is not keeping up".
///
/// **A value that goes backwards is the third thing it can say, and it is not an error.**
/// `STREAMON` restarts the driver's numbering at zero, so a client that sees this field drop is
/// looking at a stream that was stopped and started again — which since the owner's ruling of
/// 2026-08-12 is what a `wch_photo` taken during a preview does (note **N83**;
/// `crate::preview`'s header says what a viewer may conclude from the pause). It is passed
/// through exactly as the device reported it rather than rewritten into a continuation,
/// because this field's whole contract is that it is the *device's* number (D5) — and a
/// rewritten one would make a restart indistinguishable from a device that never stopped.
const FRAME_SEQUENCE_HEADER: &str = "X-Wch-Frame-Sequence";

/// What a request with no `camera=` parameter is told.
///
/// A sentence, for [`super::listener`]'s `NOT_FOUND` reason: the reader is a person who typed
/// a URL or a client one version behind, and a listing of this daemon's cameras would be a
/// camera inventory served to whoever asked. The token is still required to reach this line —
/// this is one of the two routes the 2026-08-12 ruling kept gated — but "the operator
/// authenticated" is not a reason to volunteer more than was asked for.
const NO_CAMERA: &str = "name a camera: /preview?camera=<id>\n";

/// Mount the preview route over `previews`.
///
/// Returns a `Router` for `super::listener::router` to merge, in [`super::rpc::mount`]'s
/// shape and for its reason: this module owns what its route does and owns nothing about where
/// the route sits. There is no gate applied here — which routes are behind D11's token is one
/// decision in one `match`, and a route that installed its own would be a second home for it
/// (design §2.10) — and no compression layer, which is the point.
///
/// `get` and not `any`: unlike the wire route, this endpoint's method policy is *this
/// module's*, because there is no service behind it with an opinion. A preview is a read of a
/// live resource and `GET` is what an `<img>` sends; anything else meets axum's own `405`,
/// which is the honest answer for a route that does exist and does not do that.
pub(super) fn mount(previews: Previews) -> Router {
    Router::new()
        .route(PREVIEW_PATH, get(stream))
        .with_state(previews)
}

/// One preview stream, or the refusal that stopped it starting.
///
/// The camera is attached to **before** the response head is written, which is what lets a
/// device refusal be a status: a handler that answered `200` and then discovered the camera
/// was busy would have to say so by ending an empty stream, and an `<img>` cannot tell that
/// apart from a camera that produced no frames.
async fn stream(State(previews): State<Previews>, request: Request) -> Response {
    let Some(requested) = camera_of(request.uri().query()) else {
        return (StatusCode::BAD_REQUEST, NO_CAMERA).into_response();
    };
    let requested = match CameraId::parse(&requested) {
        Some(id) => id,
        // The grammar is D1's and `CameraId::parse` is its one home; a name that is not an id
        // names no camera, which is the same fact a name that no camera answers to states.
        None => return refused(&unknown(&requested)),
    };

    let viewer = match previews.attach(&requested).await {
        Ok(viewer) => viewer,
        Err(err) => return refused(&err),
    };

    let (parts, body) = mpsc::channel(limits::PREVIEW_READER_QUEUE_DEPTH);
    tokio::spawn(write(viewer, parts, previews.shutdown().clone()));

    (
        [
            (
                header::CONTENT_TYPE,
                format!("{STREAM_CONTENT_TYPE}; boundary={BOUNDARY}"),
            ),
            // A camera frame is served, never stored (design §5: "the web client's preview is
            // served, never stored, and `wchd` records nothing it was not asked to record").
            // A cache is storage, and it is storage on the operator's disk with a picture of
            // their room in it, so the response says so rather than relying on
            // `multipart/x-mixed-replace` being un-cacheable in practice.
            (header::CACHE_CONTROL, "no-store".to_owned()),
        ],
        axum::body::Body::from_stream(ReceiverStream::new(body)),
    )
        .into_response()
}

/// Write `viewer`'s frames into `parts` until something ends the stream.
///
/// Three ways out, and each of them is one of the things this route promises:
///
/// - **the feed ended** — the camera closed, the device failed, or the daemon withdrew it, and
///   [`Viewer::next`] answers `None`;
/// - **the client went away** — hyper drops the body, the receiver goes with it, and `send`
///   answers `Err`. That is what stops the capture: this task ends, the viewer drops, the
///   feed's receiver count falls, and `crate::preview`'s driver retires the feed between
///   frames;
/// - **the daemon is stopping** — the cancellation is watched on *both* awaits, because the
///   one that matters is the `send`: a client that stopped reading parks this task there
///   (design §2.6).
///
/// `biased` on both `select!`s so a cancellation that arrives with a frame already available
/// wins. An unbiased `select!` picks at random, and "must not hang shutdown" is not a claim to
/// settle with a coin.
///
/// ## A surviving mutant, recorded rather than left for a review to find
///
/// **Deleting both `shutdown.cancelled()` arms leaves every test in this workspace passing**,
/// and the reason is worth stating because it is the same reason
/// [`super::listener::Serving::stopped`] had to grow a bound at all. Ending this body does not
/// end the *connection*: hyper still has a final chunk to write, and a client that has stopped
/// reading has a full socket, so the connection cannot finish however promptly the body does.
/// The stop therefore reaches [`schema::limits::WEB_LISTENER_STOP_MS`] and abandons the
/// listener in both builds, and the difference between them — a body that ended by itself
/// against one that was abandoned with the task — is not visible from the far side of a socket
/// that is not being read.
///
/// It is kept, and not because a mutant that survives is free. What it buys is that the ending
/// does not depend on the *ordering* of two other things: without it this task ends only when
/// `crate::preview`'s driver has noticed the cancellation, withdrawn the feed and dropped the
/// channel, which is a chain of three tasks where this is a fact one of them can act on
/// directly. A build that ended the stop the other way — by abandoning first — would meet the
/// same mutant with nothing left to argue, and that is precisely the build design §2.6 says
/// not to write.
async fn write(mut viewer: Viewer, parts: mpsc::Sender<Part>, shutdown: crate::shutdown::Shutdown) {
    loop {
        let shot = tokio::select! {
            biased;
            () = shutdown.cancelled() => break,
            shot = viewer.next() => match shot {
                Some(shot) => shot,
                None => break,
            },
        };
        let sent = tokio::select! {
            biased;
            () = shutdown.cancelled() => break,
            sent = parts.send(Ok(part(&shot))) => sent,
        };
        if sent.is_err() {
            break;
        }
    }
}

/// One multipart part: the delimiter, the headers a browser needs, and the frame.
///
/// A pure function over a [`Shot`] so the framing can be asserted without a socket — which
/// matters more here than usual, because every byte of it is load-bearing for a client this
/// project does not control. The shape is the one Chrome's multipart parser expects: the
/// delimiter on its own line, `Content-Type` and `Content-Length` (so the reader never has to
/// scan the payload for the boundary), a blank line, the bytes, and a `CRLF` separating them
/// from the delimiter that follows.
///
/// The two `X-Wch-` headers are this daemon's, and their argument is at their constants.
fn part(shot: &Shot) -> Vec<u8> {
    let head = format!(
        "--{BOUNDARY}\r\n\
         {content_type}: {FRAME_CONTENT_TYPE}\r\n\
         {content_length}: {length}\r\n\
         {FRAME_INDEX_HEADER}: {index}\r\n\
         {FRAME_SEQUENCE_HEADER}: {sequence}\r\n\
         \r\n",
        content_type = header::CONTENT_TYPE,
        content_length = header::CONTENT_LENGTH,
        length = shot.bytes().len(),
        index = shot.index(),
        sequence = shot.sequence(),
    );
    let mut part = Vec::with_capacity(head.len() + shot.bytes().len() + 2);
    part.extend_from_slice(head.as_bytes());
    part.extend_from_slice(shot.bytes());
    part.extend_from_slice(b"\r\n");
    part
}

/// The camera a request names, percent-decoded.
///
/// The query grammar is the one [`super::gate`] reads — split on `&`, then on the first `=` —
/// and this is a second *reader* of it rather than a second rule: that module answers "which
/// credentials does this request present", which is a security decision, and this one answers
/// "which camera", which is a name.
///
/// **Percent-decoded, where the token deliberately is not.** [`super::gate`]'s header argues
/// that decoding a credential is more ways to spell one secret, in the one place where more
/// ways is the wrong direction. A camera id is the opposite kind of thing: `cam:obsbot-tiny`
/// contains a `:`, `encodeURIComponent` renders it `cam%3Aobsbot-tiny`, and a route that
/// refused that would refuse the URL its own client is most likely to build. More spellings of
/// a *name* costs nothing, because the name is then matched against a live enumeration that
/// either has it or does not.
///
/// `+` is **not** read as a space: that is the `application/x-www-form-urlencoded` convention
/// for form bodies rather than a rule about query strings, and no camera id has a space in it
/// (D1's grammar is a slug), so reading it would only invent a second spelling of a name that
/// does not exist.
///
/// Duplicates: the **first** wins, and the difference from the gate's answer is the same
/// difference. Two disagreeing credentials have no right answer and are refused; two
/// disagreeing camera names have an ordinary one, and a request that asks for two cameras gets
/// the first — a stream of one of the two cameras it named, which is what any answer to that
/// request has to be.
fn camera_of(query: Option<&str>) -> Option<String> {
    query?
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(name, _)| *name == CAMERA_QUERY_PARAM)
        .map(|(_, value)| percent_decoded(value))
        .filter(|name| !name.is_empty())
}

/// `value` with its percent-escapes resolved.
///
/// Twenty lines rather than a dependency, and the reason is the same one note **N2** gives for
/// the XDG paths: this is a whole grammar in one sentence — `%` followed by two hex digits is
/// one byte — and a crate for it would be a package in the graph for a `match` on two
/// characters.
///
/// An escape that is not two hex digits is left **as written** rather than refused or dropped.
/// A camera id containing a stray `%` is a camera id that matches nothing, and the enumeration
/// is what says so; refusing here would mean this function had an opinion about D1's grammar,
/// which lives in `CameraId::parse` (design §2.10).
///
/// Bytes rather than characters, then one UTF-8 check at the end, because percent-escapes
/// encode bytes: `%C3%A9` is two escapes and one `é`, and a decoder that worked on `char`s
/// would produce two of something else. A result that is not UTF-8 is not a camera id, and
/// answers the empty string, which [`camera_of`] reads as "no camera named".
fn percent_decoded(value: &str) -> String {
    let raw = value.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(raw.len());
    let mut index = 0;
    while index < raw.len() {
        match raw.get(index) {
            Some(b'%') => match (hex(raw.get(index + 1)), hex(raw.get(index + 2))) {
                (Some(high), Some(low)) => {
                    out.push(high * 16 + low);
                    index += 3;
                }
                _ => {
                    out.push(b'%');
                    index += 1;
                }
            },
            Some(byte) => {
                out.push(*byte);
                index += 1;
            }
            None => break,
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

/// One hex digit's value, if that is what this byte is.
fn hex(byte: Option<&u8>) -> Option<u8> {
    match byte {
        Some(digit @ b'0'..=b'9') => Some(digit - b'0'),
        Some(digit @ b'a'..=b'f') => Some(digit - b'a' + 10),
        Some(digit @ b'A'..=b'F') => Some(digit - b'A' + 10),
        _ => None,
    }
}

/// The refusal for a name no camera answers to.
///
/// Built here rather than reached for, because the resolution never happened: `CameraId::parse`
/// refused the string before any enumeration, and the honest thing to tell a client is the
/// same thing it would have been told by a live enumeration that found nothing.
fn unknown(requested: &str) -> Error {
    Error::CameraUnknown {
        requested: requested.to_owned(),
    }
}

/// D13's answer, as the two statuses an `<img>` can act on.
///
/// See this module's header for why there are two of them and not eighteen. The body is the
/// error's own rendering, which carries no frame and no path this daemon minted — every D13
/// variant renders a device node, a camera name or an errno, and those are what an operator
/// needs to see in devtools.
fn refused(err: &Error) -> Response {
    let status = match err.kind() {
        // The two refusals that are about the *name in the URL* rather than about the machine.
        // A client can fix either by asking for a different camera, which is what `404` means.
        ErrorKind::CameraUnknown | ErrorKind::CameraAmbiguous => StatusCode::NOT_FOUND,
        // Everything else is this machine's state right now — busy, gone, refused, out of
        // threads — and `503` is the honest reading of all of them for a resource that exists.
        _ => StatusCode::SERVICE_UNAVAILABLE,
    };
    (
        status,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        )],
        format!("{err}\n"),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_camera_is_read_out_of_the_query_string_and_percent_decoded() {
        // The URL a browser builds. `encodeURIComponent("cam:obsbot-tiny")` is the middle
        // case, and a route that did not decode would refuse exactly the client this project
        // ships (design §2.7).
        assert_eq!(
            camera_of(Some("camera=cam:obsbot-tiny")).as_deref(),
            Some("cam:obsbot-tiny")
        );
        assert_eq!(
            camera_of(Some("camera=cam%3Aobsbot-tiny")).as_deref(),
            Some("cam:obsbot-tiny")
        );
        // Beside the credential, in both orders, because that is what the page's own request
        // carries and the gate has already read the other half of this string.
        assert_eq!(
            camera_of(Some("token=c0ffee&camera=cam%3Aone")).as_deref(),
            Some("cam:one")
        );
        assert_eq!(
            camera_of(Some("camera=cam%3Aone&token=c0ffee")).as_deref(),
            Some("cam:one")
        );
    }

    #[test]
    fn a_request_that_names_no_camera_names_no_camera() {
        // Every shape "nothing" takes, and the empty-value one is the interesting member: a
        // URL truncated at its `=` must reach the `400` this route answers rather than a
        // `CameraId::parse` of the empty string.
        for query in [None, Some(""), Some("token=c0ffee"), Some("camera=")] {
            assert_eq!(camera_of(query), None, "{query:?}");
        }
        // A parameter that merely contains the name is somebody else's — whole-name
        // comparison, exactly as the gate compares its own.
        assert_eq!(camera_of(Some("cameras=cam:one&xcamera=cam:two")), None);
    }

    #[test]
    fn a_percent_escape_that_is_not_one_is_left_where_it_was_written() {
        // The arm that says this decoder has no opinion about D1's grammar: a stray `%` is
        // carried through, and the name it makes is then matched against a live enumeration
        // that does not have it. Refusing here would be a second place that decides what a
        // camera id may contain.
        assert_eq!(percent_decoded("100%"), "100%");
        assert_eq!(percent_decoded("cam%zz"), "cam%zz");
        assert_eq!(percent_decoded("cam%3"), "cam%3");
        // And the multi-byte case, which is why this decodes bytes rather than characters:
        // two escapes are one character, and a decoder working on `char`s would make two.
        assert_eq!(percent_decoded("%C3%A9"), "\u{e9}");
        // Bytes that are not UTF-8 are not a camera id, and answer nothing rather than
        // something lossy.
        assert_eq!(percent_decoded("%FF%FE"), "");
    }

    #[test]
    fn the_two_statuses_are_the_projection_this_module_argues_for() {
        // Both arms, each with a refusal that really occurs on this path. `404` is the pair a
        // client can act on by asking for something else; `503` is everything about the
        // machine. A build that answered `500` for a busy camera would be telling a page that
        // the daemon is broken when the fact is that the camera is in use.
        assert_eq!(
            refused(&unknown("cam:nothing")).status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            refused(&Error::CameraAmbiguous {
                requested: "cam".to_owned(),
                candidates: Vec::new(),
            })
            .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            refused(&Error::Busy {
                path: camino::Utf8PathBuf::from("/dev/video0"),
                holders: Vec::new(),
            })
            .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            refused(&Error::DeviceGone {
                path: camino::Utf8PathBuf::from("/dev/video0"),
            })
            .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[test]
    fn one_part_is_the_delimiter_the_headers_the_frame_and_a_crlf() {
        // The framing a browser depends on, asserted as bytes rather than as a shape. Every
        // line of it is reachable by a mutant that leaves every other test in this workspace
        // green: a delimiter without its two leading hyphens, a missing blank line, a
        // `Content-Length` that counts the header as well as the frame. The payload here is
        // three bytes of nothing — this module never looks inside a frame, and a test that
        // needed pixels would be the integration suite over the fake.
        let framed = part(&crate::preview::shot(7, 42, vec![1, 2, 3]));
        assert_eq!(
            String::from_utf8_lossy(&framed),
            format!(
                "--{BOUNDARY}\r\n\
                 content-type: {FRAME_CONTENT_TYPE}\r\n\
                 content-length: 3\r\n\
                 {FRAME_INDEX_HEADER}: 7\r\n\
                 {FRAME_SEQUENCE_HEADER}: 42\r\n\
                 \r\n\u{1}\u{2}\u{3}\r\n"
            )
        );
    }

    #[test]
    fn the_path_and_the_boundary_are_the_shapes_a_browser_needs() {
        // `PREVIEW_PATH` has three readers that must agree (see the constant), and a value
        // that stopped being a path would register a route axum never matches. The boundary
        // has to be usable unquoted in a `Content-Type` parameter, which is what the second
        // assertion is: no delimiter characters, nothing needing an escape.
        assert!(PREVIEW_PATH.starts_with('/'), "{PREVIEW_PATH}");
        assert!(
            BOUNDARY
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
            "{BOUNDARY} needs quoting in a Content-Type parameter"
        );
        assert!(
            super::super::CAMERA_BEARING_PATHS.contains(&PREVIEW_PATH),
            "the preview is not on the list of paths that stay gated"
        );
    }
}
