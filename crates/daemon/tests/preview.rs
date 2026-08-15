//! The MJPEG preview over a real socket: N frames, a stalled reader, no compression, and a
//! stop that does not wait for a browser (design **D11**, **D12**, §2.6; docs/7 P5b).
//!
//! `web_rpc.rs` is this suite's neighbour and they divide P5b between them: that file's subject
//! is the wire surface reached over TCP, and this one's is the *other* route — the one that
//! carries frames rather than JSON, ends when its reader does rather than when its answer is
//! written, and is the only response this daemon produces that has no length.
//!
//! ## Its own fixture, and why it is not `support/fixture.rs`
//!
//! The shared fixture replays `synthetic_basic` twice, opens a Unix socket, writes a session
//! per camera and exists so that four suites cannot disagree about what "this camera" and
//! "this control" mean. This suite needs none of that and needs one thing that fixture cannot
//! give: a camera whose **MJPEG modes are small**. `synthetic_basic` offers 640×480, 1920×1080
//! and 3840×2160, and a preview of the first of those is a fixture rendering a quarter of a
//! million pixels per frame in a loop that runs as fast as the fake will answer — which is a
//! suite that measures this machine's JPEG encoder rather than this daemon's fan-out.
//!
//! So the profile is doctored, in the licence `support/fixture.rs`'s own `replaying`
//! constructor states: **a field is rewritten into a shape a device really exhibits.** 320×240
//! MJPG over 160×120 YUYV is a shape real webcams have, and it makes every frame in this file
//! a few kilobytes.
//! Note **N49** is the other half of the reason this is local rather than shared: a
//! `#[path]`-included module is compiled into every binary that includes it, so joining the
//! shared fixture would mean using every item in it.
//!
//! ## Nothing here waits on a clock
//!
//! Every wait in this file ends when the daemon says so. Frames end reads; the published
//! count, the skipped count and the live-feed count are `watch` channels the daemon writes and
//! this suite *awaits* (`Wchd::watch_preview_frames`, `watch_preview_drops`,
//! `watch_previewed_cameras`); the stop is a cancellation followed by a join with the bound
//! `limits::DAEMON_SHUTDOWN_DRAIN_MS` around it, so a build that hangs fails on an assertion
//! that names the bound rather than on a runner's timeout.
//!
//! The stalled-reader test needs one more thing to be exact, and it is a property of the
//! *client* rather than a delay: it connects with a deliberately tiny receive buffer
//! (`TcpSocket::set_recv_buffer_size`), so that a reader which stops reading stops the daemon's
//! writes within a few kilobytes instead of within however much the kernel felt like buffering.
//! That is what makes "the capture advanced while the reader did not" reachable in
//! milliseconds and not a race against a socket buffer nobody bounded.

use std::sync::Arc;
use std::time::Duration;

use camino::Utf8Path;
use daemon::http;
use daemon::server::Wchd;
use daemon::shutdown::Shutdown;
use engine::store::{LockProtocol, SessionStore, TempStore};
use fake::FakeBackend;
use schema::backend::CameraBackend;
use schema::camera::{CameraId, FrameInterval, FrameSize, FrameSizeInfo, PixelFormat};
use schema::limits;
use serde_json::Value;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpSocket, TcpStream};

// ------------------------------------------------------------------------- the fixture

/// One daemon, one small-MJPEG camera, and the web listener D11 opens.
///
/// Everything is held here because dropping any of it takes the state directory, the lock or
/// the listener with it — the arrangement a real `webcam-handler-daemon` has.
struct Preview {
    wchd: Wchd,
    backend: Arc<FakeBackend>,
    camera: CameraId,
    serving: http::Serving,
    token: String,
    shutdown: Shutdown,
    _lock: Arc<engine::store::StoreLock>,
    _state: TempStore,
}

impl Preview {
    /// Start one, over the listener `main` opens.
    ///
    /// Must be called from inside a tokio runtime.
    async fn start() -> Preview {
        Preview::listening(None).await
    }

    /// The same, over a listener whose accepted sockets have a small `SO_SNDBUF`.
    ///
    /// **The one knob the stalled-reader test needs, and it is a property of the socket rather
    /// than a delay.** "This reader has stopped reading" reaches the daemon only when the
    /// kernel stops taking bytes, so with default buffers a client can stop and the daemon can
    /// write a megabyte before anything notices — and a test that waited for that would be
    /// measuring `net.ipv4.tcp_wmem` on whatever machine it ran on. A few kilobytes on each
    /// side makes it a handful of frames, deterministically.
    ///
    /// Linux gives an accepted socket the listening socket's `SO_SNDBUF`, which is why this
    /// can be set once here rather than per connection. It is the only reason this constructor
    /// reaches `http::serve` rather than `http::open`: the two differ in exactly the three
    /// values `open` computes, so this decides the same posture `open` would and mints the
    /// same kind of token, and the suite's other seven tests go through `open` — the function
    /// `webcam-handler-daemon` itself calls.
    async fn with_send_buffer(bytes: u32) -> Preview {
        Preview::listening(Some(bytes)).await
    }

    /// Start one. Must be called from inside a tokio runtime.
    async fn listening(send_buffer: Option<u32>) -> Preview {
        let backend = Arc::new(
            FakeBackend::new(vec![small_mjpeg_camera()])
                .expect("the synthetic profile is this build's version"),
        );
        let camera = backend
            .enumerate()
            .expect("the fake enumerates what it replays")
            .first()
            .expect("one camera")
            .id
            .clone();

        let state = TempStore::new().expect("a state directory");
        let store = SessionStore::new(state.root());
        let lock = Arc::new(
            store
                .lock(LockProtocol::HeldForLifetime)
                .expect("nothing else holds a throw-away state directory"),
        );
        let shutdown = Shutdown::new();
        let wchd = Wchd::new(
            Arc::clone(&backend) as Arc<dyn CameraBackend>,
            SessionStore::new(state.root()),
            Arc::clone(&lock),
            shutdown.clone(),
        );
        let methods = daemon::server::mount(wchd.clone()).expect("the T5 surface mounts once");
        // The listener `main` opens, over the daemon's own fan-out and the daemon's own stop
        // token — the two values that make this suite about the shipped composition rather
        // than about one it arranged.
        let requested: std::net::SocketAddr = "127.0.0.1:0".parse().expect("a literal address");
        let serving = match send_buffer {
            None => http::open(requested, false, methods, wchd.previews(), shutdown.clone())
                .await
                .expect("loopback on an ephemeral port"),
            Some(bytes) => {
                let socket = TcpSocket::new_v4().expect("an IPv4 socket");
                socket
                    .set_send_buffer_size(bytes)
                    .expect("the kernel takes a send buffer size");
                socket
                    .bind(requested)
                    .expect("loopback on an ephemeral port");
                let listener = socket.listen(1).expect("a listening socket");
                let posture = daemon::http::Posture::of(requested, false);
                let token = Arc::new(daemon::http::Token::mint().expect("the kernel has a CSPRNG"));
                http::serve(
                    listener,
                    posture,
                    Some(token),
                    methods,
                    wchd.previews(),
                    shutdown.clone(),
                )
                .expect("a posture and a token that agree")
            }
        };
        let token = secret(&serving);

        // The baseline every counter below is read against. A fresh daemon has opened no
        // camera and started no stream (D12), and asserting it is what makes every later
        // difference a difference this suite caused.
        assert_eq!(backend.opens(), 0, "a fresh daemon has opened a camera");
        assert_eq!(backend.streams_started(), 0);
        assert_eq!(wchd.previewed_cameras(), 0);

        Preview {
            wchd,
            backend,
            camera,
            serving,
            token,
            shutdown,
            _lock: lock,
            _state: state,
        }
    }

    /// The request target an `<img>` on the daemon's own page would use.
    fn target(&self) -> String {
        format!(
            "{path}?{camera}={id}&{token}={secret}",
            path = http::PREVIEW_PATH,
            camera = http::CAMERA_QUERY_PARAM,
            // Percent-encoded, which is what `encodeURIComponent` produces for an id with a
            // `:` in it and therefore what the shipped client will send.
            id = self.camera.as_str().replace(':', "%3A"),
            token = http::TOKEN_QUERY_PARAM,
            secret = self.token,
        )
    }

    /// Open a preview stream and read its response head.
    async fn watching(&self) -> Stream {
        Stream::open(self.serving.bound(), &self.target(), None).await
    }

    /// Take a photo of this fixture's camera over the daemon's own JSON-RPC surface.
    ///
    /// Through `raw_json_request` rather than the typed server trait, because the subject is
    /// the verb a **client** calls: the request is the document a browser would send and the
    /// answer is the one it would parse, so a refusal that only exists in the projection is
    /// visible here and would not be through a direct method call.
    ///
    /// `settle` is the request's settle policy, spelled as JSON, and it is a parameter for one
    /// reason: a deadline that has already passed is how this suite fails a *capture*
    /// deterministically while a preview is running (see
    /// `a_capture_that_fails_mid_photo_still_leaves_the_preview_streaming` for why the fake's
    /// frame faults are the wrong instrument there).
    async fn photo(&self, settle: &str) -> serde_json::Value {
        self.call(
            "wch_photo",
            &format!(
                r#""request":{{"settle":{settle},"sink":{{"kind":"return_bytes","format":"jpeg"}}}}"#
            ),
        )
        .await
    }

    /// One JSON-RPC call about this fixture's camera, as a client would make it.
    ///
    /// `params` is everything after the camera, spelled as JSON text rather than built from the
    /// typed request: [`Preview::photo`]'s reason, which is that the subject is the document a
    /// *browser* sends. A serde round trip through the Rust type would assert that this build
    /// agrees with itself.
    async fn call(&self, method: &str, params: &str) -> serde_json::Value {
        let methods = daemon::server::mount(self.wchd.clone()).expect("a second reader of T5");
        let request = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"{method}","params":{{"camera":"{camera}",{params}}}}}"#,
            camera = self.camera.as_str()
        );
        let (answer, _subscriptions) = methods
            .raw_json_request(&request, 1)
            .await
            .expect("the surface answered");
        serde_json::from_str(&answer.to_string()).expect("the T5 surface answers JSON")
    }

    /// Start a recording of this fixture's camera into `path`.
    ///
    /// `format` is the pixel format to ask the device for, or `None` for whatever the ranking
    /// picks — which on this fixture is MJPG, because \[PF:9\]'s shape survives the rewrite and
    /// note **N85**'s re-ranking prefers the larger mode. The two callers that pass one are the
    /// two that are *about* the format: a take a browser can paint and a take it cannot.
    async fn record(&self, path: &Utf8Path, duration_ms: u64, format: Option<&str>) -> Value {
        let stream = format.map_or_else(String::new, |format| {
            format!(r#""stream":{{"pixel_format":"{format}"}},"#)
        });
        self.call(
            "wch_record_start",
            &format!(
                r#""request":{{{stream}"duration_ms":{duration_ms},"sink":{{"kind":"server_path","path":"{path}"}}}}"#
            ),
        )
        .await
    }

    /// Stop this fixture's camera's recording and collect it.
    async fn stop_recording(&self) -> Value {
        let methods = daemon::server::mount(self.wchd.clone()).expect("a second reader of T5");
        let request = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"wch_record_stop","params":{{"camera":"{camera}"}}}}"#,
            camera = self.camera.as_str()
        );
        let (answer, _subscriptions) = methods
            .raw_json_request(&request, 1)
            .await
            .expect("the surface answered");
        serde_json::from_str(&answer.to_string()).expect("the T5 surface answers JSON")
    }

    /// Wait until this daemon has published a frame past `seen`, or say it did not.
    ///
    /// **The one instrument the recording claims are made with**, and it is a `watch` the daemon
    /// writes rather than a duration: sampled *after* a `record_start` has answered, every
    /// publication past it is one the take's own driver made, because the preview's driver has
    /// left the device by then. A build in which a recording published nothing leaves this
    /// number exactly where it was.
    ///
    /// Bounded rather than left to the runner, for
    /// `a_photo_taken_during_a_preview_suspends_the_stream_and_the_preview_resumes`'s reason:
    /// the difference between "this test failed" and "this run timed out" is a diagnosis.
    async fn published_past(&self, seen: u64) -> bool {
        let mut published = self.wchd.watch_preview_frames();
        tokio::time::timeout(
            Duration::from_millis(limits::PREVIEW_FRAME_WAIT_MS * 4),
            published.wait_for(|count| *count > seen),
        )
        .await
        .is_ok()
    }
}

/// A recording duration no claim in this file waits out.
///
/// Every take below is ended by a `wch_record_stop`, so its declared duration exists only to be
/// **longer than every wait beside it** — and that is a finding rather than a formality. With a
/// take shorter than the four-turn timeouts these claims allow, the hand-back puts a driver back
/// on the feed and the preview starts publishing again, so "the tab kept getting frames" is
/// satisfied by a build in which the *recording* fed it nothing. Two of the mutants in note
/// **N117** survived exactly that way before this constant existed.
const A_TAKE_LONGER_THAN_THIS_TEST: u64 = 30_000;

/// The settle policy every photo in this file uses unless it is about the deadline.
///
/// `skip_frames: 0` because the fake synthesizes a frame per call and this suite is about the
/// suspension rather than about `PF:11`'s warm-up — ten discarded frames would be ten more
/// `DQBUF`s inside the window whose *length* is the thing being bounded.
fn default_settle() -> &'static str {
    r#"{"spec":{"kind":"skip_frames","frames":0},"deadline_ms":5000}"#
}

/// The photo out of a JSON-RPC answer.
///
/// **Nothing in this function prints a payload.** A photo answer carries the camera's bytes as
/// base64, so the failure message names the *error member* and never the whole document — a
/// frame may contain a person (AGENTS), and a panic message is a place bytes reach a terminal
/// and a CI log. `api::Base64Bytes`' own `Debug` makes the same promise one crate down; this is
/// the half that keeps a test from going around it.
fn photograph(answer: &serde_json::Value) -> api::PhotoResponse {
    let result = answer.get("result").unwrap_or_else(|| {
        panic!(
            "the photo was refused: {refused}",
            refused = refusal(answer)
        )
    });
    serde_json::from_value(result.clone()).expect("a T5 photo answer")
}

/// The refusal out of a JSON-RPC answer, as a string a message can carry.
///
/// The error member only. An answer that carried a photo has none, and saying so is more
/// useful than rendering a document this function has promised not to print.
fn refusal(answer: &serde_json::Value) -> String {
    match answer.get("error") {
        Some(error) => error.to_string(),
        None => "the call succeeded".to_owned(),
    }
}

/// The token out of the URL the daemon printed, never off the `Token` value.
///
/// `http.rs`'s reason, restated because it is the one that matters most: a build that published
/// one secret and gated on another would pass every assertion in this file if the suite reached
/// into the value instead of reading the line an operator copies.
fn secret(serving: &http::Serving) -> String {
    let url = serving.ready_to_open_url();
    url.split_once(&format!("?{param}=", param = http::TOKEN_QUERY_PARAM))
        .map(|(_url, token)| token.to_owned())
        .expect("D11's default cell publishes a token")
}

/// The committed profile with its modes rewritten small, MJPEG still on top.
///
/// A rewrite rather than a second committed document, and only into shapes a device really
/// has: 320×240 MJPG and 160×120 YUYV at 30 fps are ordinary webcam modes. What it buys is
/// that every frame in this file is kilobytes rather than a quarter-megapixel render, so the
/// suite exercises the daemon rather than the fake's JPEG encoder.
///
/// **Both formats are rewritten, and that is D5's 2026-08-13 amendment showing through.**
/// Until the owner re-ranked the format tree this function shrank MJPG alone and left YUYV at
/// 640×480, because the default landed on MJPG for being *enumerated first* whatever its
/// sizes said. Under the re-ranking a camera whose compressed format is its smallest is a
/// camera whose best photograph is uncompressed — correctly — so the fixture stopped being
/// the device these tests describe and started failing them on the verbatim claim. The shape
/// is unchanged in the way that matters: MJPG still tops out above YUYV, which is what
/// \[PF:9\] records and why sizes nest under formats at all.
fn small_mjpeg_camera() -> schema::profile::DeviceProfile {
    fn only(width: u32, height: u32) -> Vec<FrameSizeInfo> {
        vec![FrameSizeInfo {
            size: FrameSize::Discrete { width, height },
            intervals: vec![FrameInterval::Discrete {
                numerator: 1,
                denominator: 30,
            }],
        }]
    }

    let mut profile = testkit::fixtures::synthetic_basic();
    for format in &mut profile.invariant.formats {
        if format.pixel_format == PixelFormat::MJPG {
            format.sizes = only(320, 240);
        } else if format.pixel_format == PixelFormat::YUYV {
            format.sizes = only(160, 120);
        }
    }
    profile
}

// ------------------------------------------------------------------------- the client

/// One preview response, read incrementally.
///
/// Hand-written for `support/tcp.rs`'s reason and one more: that client reads to end-of-file,
/// and this response has no end until somebody causes one — reading it is the thing under
/// test. It also has to be able to **stop** reading, which is what the stalled-reader test is
/// about and what no HTTP client library will let a caller do.
struct Stream {
    socket: TcpStream,
    /// Bytes off the socket that have not been decoded yet.
    ///
    /// Separate from [`Stream::buffered`] because the body arrives **chunked**: this response
    /// has no length — it is a live camera — so HTTP/1.1 frames it as a sequence of
    /// length-prefixed chunks, and a client that read the socket as though it were the body
    /// would find hex digits between its frames. A browser's `<img>` never sees that layer;
    /// this suite writes its own client, so it decodes the layer a client library would.
    raw: Vec<u8>,
    /// Decoded body bytes that have not been parsed into a part yet.
    buffered: Vec<u8>,
    /// The response head, split at the blank line that ends it.
    head: String,
    /// Whether the body is chunked, read off the head rather than assumed.
    chunked: bool,
    /// Whether this response's **body** has ended, which is not the same question as whether
    /// the connection has.
    ///
    /// HTTP/1.1 keeps a connection alive past a chunked body's terminating zero-length chunk,
    /// so a reader that waited for end-of-file to learn that its preview had ended would wait
    /// for ever — which is what the first draft of
    /// `a_device_that_failed_mid_take_reaches_the_collector_and_never_the_readers_as_a_success`
    /// did, and it is a distinction a browser makes and a hand-written client has to make for
    /// itself. Set by [`Stream::decode`], read by [`Stream::fill`].
    ended: bool,
}

impl Stream {
    /// Connect, send one `GET`, and read as far as the end of the response head.
    ///
    /// `receive_buffer` is the client's `SO_RCVBUF`. `None` is the ordinary case; a small value
    /// is how the stalled-reader test makes "this reader has stopped" reach the daemon within a
    /// few frames rather than within however much the kernel decided to buffer.
    async fn open(
        bound: std::net::SocketAddr,
        target: &str,
        receive_buffer: Option<u32>,
    ) -> Stream {
        let socket = TcpSocket::new_v4().expect("an IPv4 socket");
        if let Some(bytes) = receive_buffer {
            socket
                .set_recv_buffer_size(bytes)
                .expect("the kernel takes a receive buffer size");
        }
        let mut socket = socket.connect(bound).await.expect("the listener is up");
        let request = format!(
            "GET {target} HTTP/1.1\r\n\
             Host: {bound}\r\n\
             Accept-Encoding: gzip\r\n\
             \r\n"
        );
        socket
            .write_all(request.as_bytes())
            .await
            .expect("the request was written");

        let mut stream = Stream {
            socket,
            raw: Vec::new(),
            buffered: Vec::new(),
            head: String::new(),
            chunked: false,
            ended: false,
        };
        stream.read_head().await;
        stream
    }

    /// Read until the blank line that ends the response head.
    async fn read_head(&mut self) {
        loop {
            if let Some(end) = find(&self.raw, b"\r\n\r\n") {
                self.head = String::from_utf8_lossy(&self.raw[..end]).into_owned();
                self.raw.drain(..end + 4);
                self.chunked = self
                    .header("transfer-encoding")
                    .is_some_and(|value| value.eq_ignore_ascii_case("chunked"));
                self.decode();
                return;
            }
            assert!(self.read().await > 0, "the response head never ended");
        }
    }

    /// The status code.
    fn status(&self) -> u16 {
        self.head
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse().ok())
            .unwrap_or_else(|| panic!("no status code in {head:?}", head = self.head))
    }

    /// One header's value, matched case-insensitively the way HTTP defines field names.
    fn header(&self, name: &str) -> Option<&str> {
        self.head.lines().skip(1).find_map(|line| {
            let (field, value) = line.split_once(':')?;
            field
                .trim()
                .eq_ignore_ascii_case(name)
                .then(|| value.trim())
        })
    }

    /// The next multipart part, parsed and checked against the framing this daemon writes.
    ///
    /// Every field is read rather than skipped, because the framing *is* the contract with a
    /// browser: a delimiter without its hyphens, a missing blank line or a `Content-Length`
    /// that disagrees with the payload are each a preview that does not paint, and each is a
    /// mutant a test that only counted bytes would miss.
    async fn part(&mut self) -> Part {
        let boundary = format!("--{boundary}\r\n", boundary = http::preview::BOUNDARY);
        let head = loop {
            if let Some(end) = find(&self.buffered, b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&self.buffered[..end]).into_owned();
                self.buffered.drain(..end + 4);
                break head;
            }
            assert!(self.fill().await > 0, "the stream ended mid-part");
        };
        let head = head
            .strip_prefix(&boundary)
            .unwrap_or_else(|| panic!("a part that does not start with the delimiter: {head:?}"))
            .to_owned();

        let field = |name: &str| -> String {
            head.lines()
                .find_map(|line| {
                    let (field, value) = line.split_once(':')?;
                    field
                        .trim()
                        .eq_ignore_ascii_case(name)
                        .then(|| value.trim().to_owned())
                })
                .unwrap_or_else(|| panic!("no {name} in {head:?}"))
        };
        assert_eq!(field("content-type"), "image/jpeg");
        let length: usize = field("content-length").parse().expect("a byte count");
        let index: u64 = field("x-wch-frame-index").parse().expect("a frame index");
        let sequence: u32 = field("x-wch-frame-sequence")
            .parse()
            .expect("a sequence number");

        // The payload, then the CRLF that separates it from the next delimiter. The
        // `Content-Length` is what says how much to take, which is the whole reason this
        // daemon writes one: a reader that scanned for the boundary instead would have to
        // trust that a JPEG never contains it.
        while self.buffered.len() < length + 2 {
            assert!(self.fill().await > 0, "the stream ended inside a frame");
        }
        let bytes: Vec<u8> = self.buffered.drain(..length).collect();
        let separator: Vec<u8> = self.buffered.drain(..2).collect();
        assert_eq!(separator, b"\r\n", "a part that does not end with a CRLF");

        Part {
            index,
            sequence,
            bytes,
        }
    }

    /// Read from the socket until at least one more decoded byte is available.
    ///
    /// Answers how many raw bytes arrived; zero is end of stream, which is what the shutdown
    /// test reads as "the response body ended".
    async fn fill(&mut self) -> usize {
        loop {
            if self.ended {
                return 0;
            }
            let read = self.read().await;
            if read == 0 {
                return 0;
            }
            let before = self.buffered.len();
            self.decode();
            if self.buffered.len() > before || self.ended {
                return read;
            }
        }
    }

    /// One read from the socket into the undecoded buffer.
    async fn read(&mut self) -> usize {
        let mut chunk = [0_u8; 8192];
        let read = self
            .socket
            .read(&mut chunk)
            .await
            .expect("the connection was readable");
        self.raw
            .extend_from_slice(chunk.get(..read).unwrap_or_default());
        read
    }

    /// Move every complete chunk out of the undecoded buffer and into the decoded one.
    ///
    /// The whole of HTTP/1.1's chunked grammar this suite needs: a hex length, a CRLF, that
    /// many bytes, a CRLF. Chunk extensions and trailers are not written by this daemon and are
    /// not read here — a client that accepted more than the server writes would be checking
    /// something other than what the server writes.
    fn decode(&mut self) {
        if !self.chunked {
            self.buffered.append(&mut self.raw);
            return;
        }
        loop {
            let Some(end) = find(&self.raw, b"\r\n") else {
                return;
            };
            let header = String::from_utf8_lossy(&self.raw[..end]).into_owned();
            let Ok(length) = usize::from_str_radix(header.trim(), 16) else {
                panic!("not a chunk length: {header:?}");
            };
            if self.raw.len() < end + 2 + length + 2 {
                return;
            }
            self.raw.drain(..end + 2);
            let chunk: Vec<u8> = self.raw.drain(..length).collect();
            self.raw.drain(..2);
            self.buffered.extend_from_slice(&chunk);
            if length == 0 {
                // The terminator. The socket stays open — this daemon speaks keep-alive — so
                // this flag is the only thing that says the body is over.
                self.ended = true;
                return;
            }
        }
    }
}

/// One frame off the wire.
///
/// The two numbers are two different facts and the suite asserts both: `index` is this
/// daemon's publication count, so a gap in it is frames *this reader* was too slow for, and
/// `sequence` is the driver's own, so a gap in that is frames the kernel dropped before the
/// daemon saw them. A part carrying one of them and not the other would leave a client unable
/// to tell those apart.
#[derive(Debug)]
struct Part {
    index: u64,
    sequence: u32,
    bytes: Vec<u8>,
}

impl Part {
    /// Whether these bytes are a JPEG: a start-of-image marker and an end-of-image marker.
    ///
    /// The frames are the fake's synthetic pattern, so asserting *something* about the bytes is
    /// legitimate here in a way it never is for a real camera — and this is the weakest thing
    /// worth asserting: not what the picture is, only that the daemon put a JPEG in an
    /// `image/jpeg` part rather than the YUYV buffer a build that skipped
    /// `engine::preview::start`'s negotiation check would have put there.
    fn is_jpeg(&self) -> bool {
        self.bytes.starts_with(&[0xff, 0xd8]) && self.bytes.ends_with(&[0xff, 0xd9])
    }
}

/// An equal-length, one-digit-different token.
///
/// `http.rs` and `web_rpc.rs` each carry the same six lines, and the duplication is deliberate
/// there and here: what they share is a *fact about the token* — 64 hex digits — and each suite
/// presents it to a different surface. It is the only candidate that reaches `Token::verify`'s
/// comparison loop at all, since everything shorter or longer is refused by the length check.
fn near_miss(secret: &str) -> String {
    let mut digits: Vec<char> = secret.chars().collect();
    let first = digits.first_mut().expect("a token is not empty");
    *first = if *first == '0' { '1' } else { '0' };
    digits.into_iter().collect()
}

/// The status code out of a whole answer read by [`get`].
///
/// A number rather than a `starts_with`, because the assertions it serves are *inequalities* —
/// "this is not the gate's 401 and not the asset table's 404" — and a prefix match cannot say
/// that about an answer whose status it does not know.
fn status_of(answer: &str) -> u16 {
    answer
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or_else(|| panic!("no status code in {answer:.64}"))
}

/// The first position of `needle` in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Send one ordinary `GET` and read the whole answer, for the asset half of the compression
/// claim.
async fn get(bound: std::net::SocketAddr, target: &str, headers: &[(&str, &str)]) -> String {
    let mut socket = TcpStream::connect(bound).await.expect("the listener is up");
    let mut request = format!(
        "GET {target} HTTP/1.1\r\n\
         Host: {bound}\r\n\
         Connection: close\r\n"
    );
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("\r\n");
    socket
        .write_all(request.as_bytes())
        .await
        .expect("the request was written");
    let mut answer = Vec::new();
    socket
        .read_to_end(&mut answer)
        .await
        .expect("the connection was readable");
    String::from_utf8_lossy(&answer).into_owned()
}

// ------------------------------------------------------------------------- the criteria

#[tokio::test]
async fn a_client_reads_successive_multipart_frames_off_one_camera() {
    // docs/7 P5b's first gate row. The response is a `multipart/x-mixed-replace` whose parts
    // are JPEGs, and "successive" is asserted as a *property of the indices* rather than as a
    // count: a build that published one frame forever would deliver the same bytes with the
    // same index and pass any assertion that only counted parts.
    let preview = Preview::start().await;
    let mut stream = preview.watching().await;

    assert_eq!(stream.status(), 200);
    assert_eq!(
        stream.header("content-type"),
        Some(
            format!(
                "multipart/x-mixed-replace; boundary={boundary}",
                boundary = http::preview::BOUNDARY
            )
            .as_str()
        )
    );
    // A camera frame is served, never stored (design §5).
    assert_eq!(stream.header("cache-control"), Some("no-store"));

    let mut seen: Vec<(u64, u32)> = Vec::new();
    for _ in 0..8 {
        let part = stream.part().await;
        assert!(part.is_jpeg(), "an image/jpeg part that is not a JPEG");
        assert!(!part.bytes.is_empty());
        seen.push((part.index, part.sequence));
    }
    // Both numbers advance, and they are two claims: the publication index says this daemon
    // published eight distinct frames, and the driver's sequence says the *device* delivered
    // eight distinct frames rather than one buffer handed out repeatedly.
    assert!(
        seen.windows(2).all(|pair| pair[1].0 > pair[0].0),
        "publications did not advance: {seen:?}"
    );
    assert!(
        seen.windows(2).all(|pair| pair[1].1 > pair[0].1),
        "the device delivered the same frame twice: {seen:?}"
    );

    // D12's lifecycle, from the other end: one camera opened, one stream started, one feed —
    // and the camera was opened by *this*, since the fixture asserted zero at the start.
    assert_eq!(preview.backend.opens(), 1);
    assert_eq!(preview.backend.streams_started(), 1);
    assert_eq!(preview.wchd.previewed_cameras(), 1);
}

#[tokio::test]
async fn the_capture_advances_while_a_stalled_reader_does_not() {
    // docs/7 P5b's second gate row, and the one that says **drop** rather than
    // **backpressure**. Three numbers make it and none of them is inferred:
    //
    //   * the daemon's published count, which advances while this client reads nothing;
    //   * this client's own count of parts, which is a local variable and stays at one;
    //   * the gap in the publication indices the client then reads off the wire, which is the
    //     frames that were overwritten in the one-slot channel while it was not looking — and
    //     the daemon's skipped count, which is the same fact counted at the other end.
    //
    // A build with a queue in place of the latest-frame `watch` fails here in whichever way it
    // was built. An **unbounded** queue keeps every frame for this reader, so the indices come
    // back contiguous and the skipped count never moves: the loop below runs out of attempts
    // and says so. A **bounded** one stops the capture when the reader stops, so the published
    // count never advances and the wait below never ends — which nextest turns into a named
    // failure with this test's name on it (`.config/nextest.toml`), rather than a hang.
    let preview = Preview::with_send_buffer(4096).await;
    let mut published = preview.wchd.watch_preview_frames();
    let mut skipped = preview.wchd.watch_preview_drops();

    // A deliberately tiny receive buffer, so "this reader has stopped" reaches the daemon
    // within a few frames rather than within however much the kernel decided to buffer. It is
    // a property of the client, not a delay.
    let mut stream = Stream::open(preview.serving.bound(), &preview.target(), Some(4096)).await;
    assert_eq!(stream.status(), 200);

    let first = stream.part().await;
    let read_by_this_client = 1_u64;
    assert_eq!(first.index, 1, "the first frame is the first publication");

    // ... and now this client reads nothing at all for a while.
    //
    // The wait ends when the daemon says it has published far past what this client took — an
    // event on a `watch`, not a duration and not a poll of a counter. `RUN_AHEAD` is bigger
    // than anything the socket buffers can hold at this frame size, so the frames past it had
    // nowhere to go but the one slot they overwrote.
    const RUN_AHEAD: u64 = 64;
    published
        .wait_for(|count| *count > first.index + RUN_AHEAD)
        .await
        .expect("the daemon is still publishing");
    let advanced = *published.borrow_and_update();
    assert!(
        advanced > first.index + RUN_AHEAD,
        "the capture did not advance past the stalled reader: {advanced}"
    );
    assert_eq!(
        read_by_this_client, 1,
        "this client read while it was stalled"
    );

    // Now it starts reading again, and finds the hole. The buffers drain first — a few frames
    // that were already written — and then the next frame is whatever is current, which is
    // tens of publications later. The loop is bounded so that a build which delivered
    // everything fails here rather than reading forever.
    let mut previous = first.index;
    let mut gap = None;
    for _ in 0..RUN_AHEAD {
        let part = stream.part().await;
        assert!(part.is_jpeg());
        if part.index > previous + 1 {
            gap = Some((previous, part.index));
            break;
        }
        previous = part.index;
    }
    let (before, after) = gap.expect("no frame was ever dropped: this reader was queued for");
    assert!(
        after > before + 1,
        "a gap that is not a gap: {before} -> {after}"
    );

    // The same fact from the daemon's side, counted rather than silent (rubric rule 3).
    let dropped = *skipped.borrow_and_update();
    assert!(
        dropped >= after - before - 1,
        "the daemon counted {dropped} drops for a gap of {}",
        after - before - 1
    );

    // And the reader was attached the whole time — a stalled reader is not a disconnected one,
    // so this has been one live camera with one live feed throughout.
    assert_eq!(preview.wchd.previewed_cameras(), 1);
    assert_eq!(preview.backend.streams_started(), 1);
}

#[tokio::test]
async fn the_preview_is_uncompressed_while_compression_is_on_everywhere_else() {
    // docs/7 P5b's third gate row, and it is two assertions because one of them proves
    // nothing: a build with no `CompressionLayer` at all would pass "the preview is
    // uncompressed" and fail the client it exists for. So the asset half is asserted to be
    // **compressed** in the same test, over the same listener, with the same request header.
    let preview = Preview::start().await;
    let bound = preview.serving.bound();

    // No credential on the asset half: since the 2026-08-12 ruling it needs none, and a request
    // that presented one here would read as though it did.
    let page = get(bound, "/", &[("Accept-Encoding", "gzip")]).await;
    assert!(page.starts_with("HTTP/1.1 200"), "{page:.64}");
    assert!(
        page.to_ascii_lowercase().contains("content-encoding: gzip"),
        "the assets are not compressed, so this test's other half proves nothing"
    );

    // The same listener, the same `Accept-Encoding`, the other route.
    let mut stream = preview.watching().await;
    assert_eq!(stream.status(), 200);
    assert_eq!(
        stream.header("content-encoding"),
        None,
        "the preview response is compressed"
    );

    // Not merely the absence of a header: the frame bytes are a JPEG on the wire, which is
    // what a reader gets only if nothing re-encoded the stream between the camera and the
    // socket.
    assert!(stream.part().await.is_jpeg());
}

#[tokio::test]
async fn a_stop_with_an_open_preview_tab_completes_inside_the_bound() {
    // docs/7 P5b's fourth gate row — design §2.6's "an open MJPEG tab must not hang shutdown",
    // as a number. The tab is provably open: a frame has been read off it and nothing has
    // closed it, so `axum::serve`'s graceful shutdown has a response in flight to wait for.
    //
    // **The tab is also provably *stalled*, and that is the case the requirement is about.** A
    // tab that is still draining its socket ends by itself the moment its feed is withdrawn,
    // so a build whose response body ignored the cancellation would pass a test written that
    // way and hang in front of a real browser that had been minimised. So this one stops
    // reading after its first frame and waits — on the daemon's published count, not on a
    // clock — until far more frames exist than the small socket buffers can hold, at which
    // point the writer is parked in a send that only the cancellation can end.
    //
    // The bound is `limits::DAEMON_SHUTDOWN_DRAIN_MS` and it is asserted rather than left to
    // the runner: a build whose preview body ignored the cancellation would hang forever, and
    // the difference between "this test failed" and "this run timed out" is the difference
    // between a diagnosis and a mystery.
    let preview = Preview::with_send_buffer(4096).await;
    let mut stream = Stream::open(preview.serving.bound(), &preview.target(), Some(4096)).await;
    assert!(stream.part().await.is_jpeg());

    let mut published = preview.wchd.watch_preview_frames();
    published
        .wait_for(|count| *count > 64)
        .await
        .expect("the daemon is still publishing");

    preview.shutdown.cancel();
    let stopped = tokio::time::timeout(
        Duration::from_millis(limits::DAEMON_SHUTDOWN_DRAIN_MS),
        preview.serving.stopped(),
    )
    .await;
    assert!(
        stopped.is_ok(),
        "an open preview tab held the stop past {ms} ms",
        ms = limits::DAEMON_SHUTDOWN_DRAIN_MS
    );
    stopped
        .expect("the timeout above")
        .expect("the server task ended");

    // The client's own end of it: whatever the kernel had already buffered for this stalled
    // reader arrives, and then the connection **ends** — end-of-file rather than a socket left
    // open by a daemon that has claimed to stop. Bounded, because a build that stopped its
    // listener without closing this connection would otherwise read here forever.
    let mut reads = 0;
    while stream.fill().await > 0 {
        reads += 1;
        assert!(reads < 4_096, "the tab outlived the daemon");
    }
}

#[tokio::test]
async fn two_tabs_on_one_camera_are_one_streamer() {
    // D12's "exclusive streaming by construction", at the case that would break it: two
    // readers of one camera. The second attaches to the feed the first created — one `open`,
    // one `STREAMON`, one feed — and both are served real frames, which is what says the
    // second tab joined a stream rather than being refused one.
    let preview = Preview::start().await;
    let mut first = preview.watching().await;
    assert!(first.part().await.is_jpeg());

    let mut second = preview.watching().await;
    assert_eq!(second.status(), 200);
    assert!(second.part().await.is_jpeg());

    assert_eq!(
        preview.backend.opens(),
        1,
        "two tabs opened two descriptors"
    );
    assert_eq!(
        preview.backend.streams_started(),
        1,
        "two tabs started two streams — V4L2 allows one streamer per node (D12)"
    );
    assert_eq!(preview.wchd.previewed_cameras(), 1);

    // One leaves and the other keeps painting: the capture belongs to the feed rather than to
    // whichever reader started it.
    drop(first);
    assert!(second.part().await.is_jpeg());
    assert_eq!(preview.wchd.previewed_cameras(), 1);

    // ... and when the last one leaves, the feed goes. Awaited on the daemon's own count,
    // because "the last tab closed" is an event — and bounded, because a build that never
    // noticed its reader had gone would otherwise hang rather than fail. The bound is derived
    // rather than picked: the driver asks the channel how many readers it has **between
    // frames**, so the answer arrives within one `limits::PREVIEW_FRAME_WAIT_MS` of the last
    // one leaving, and twice that is a whole turn of slack. Nothing waits on this to learn
    // anything (`.config/nextest.toml` makes the same argument about its own deadline).
    let mut feeds = preview.wchd.watch_previewed_cameras();
    drop(second);
    let retired = tokio::time::timeout(
        Duration::from_millis(limits::PREVIEW_FRAME_WAIT_MS * 2),
        feeds.wait_for(|live| *live == 0),
    )
    .await;
    assert!(
        retired.is_ok(),
        "the capture outlived its last reader by more than two frame waits"
    );
    retired
        .expect("the timeout above")
        .expect("the daemon is still running");
    assert_eq!(
        preview.backend.streams_started(),
        1,
        "the feed was restarted on its way out"
    );
}

#[tokio::test]
async fn every_camera_bearing_route_is_behind_the_gate() {
    // **The invariant the owner's 2026-08-12 ruling made load-bearing.** Before it, every route
    // was gated by one `Router::layer` over one router and this test was a belt beside braces.
    // After it the gate is a `route_layer` over the routes and the assets are outside it, so
    // "the camera is behind the token" is a property of a *list* — `daemon::http::CAMERA_BEARING_PATHS`
    // — and a list can be wrong. Note **N82** carries the ruling; this is the behavioural half
    // of what replaced the property it dissolved, and `scripts/gates/web-routes-are-gated.sh`
    // is the structural half (a route nobody put on the list is a route no test can drive).
    //
    // Four claims per path, and none implies the others:
    //
    //   1. **nothing is refused** — 401 with RFC 6750's challenge, which is the whole of what
    //      an anonymous client gets;
    //   2. **a near miss is refused**, in both credential forms, so the gate is *comparing*
    //      rather than looking for a parameter's presence — a route whose gate had been
    //      replaced by a "is there a token= in this URL" check passes claim 1 and fails this;
    //   3. **the token gets past, and what is behind is a route** — the answer is neither the
    //      gate's 401 nor the asset table's 404, so a path that is on this list and is *not*
    //      registered fails here rather than passing claim 1 by falling through to the assets;
    //   4. **the population is not empty**, because every claim above quantifies over it.
    //
    // Claim 3 is the one the ruling sharpened. While the assets were gated, a path on this list
    // that named no route still answered 401 — from the fallback — so the list could name
    // anything and claim 1 would hold. Now an unrouted path answers the asset table's 404 to an
    // anonymous request, and claim 1 catches it on its own.
    let preview = Preview::start().await;
    let bound = preview.serving.bound();
    let wrong = near_miss(&preview.token);

    let mut driven = 0_usize;
    for path in http::CAMERA_BEARING_PATHS {
        driven += 1;

        let anonymous = get(bound, path, &[]).await;
        assert!(
            anonymous.starts_with("HTTP/1.1 401"),
            "{path} answered a request with no credential: {anonymous:.64}"
        );
        assert!(
            // Case-insensitively: HTTP field names are, and hyper writes the canonical
            // lowercase form rather than the one `daemon::http::gate` spells.
            anonymous
                .to_ascii_lowercase()
                .contains("www-authenticate: bearer"),
            "{path} refused without the challenge RFC 6750 asks for"
        );

        for near in [
            get(
                bound,
                &format!("{path}?{token}={wrong}", token = http::TOKEN_QUERY_PARAM),
                &[],
            )
            .await,
            get(
                bound,
                path,
                &[("Authorization", &format!("Bearer {wrong}"))],
            )
            .await,
        ] {
            assert!(
                near.starts_with("HTTP/1.1 401"),
                "{path} admitted a one-digit-different token: {near:.64}"
            );
        }

        let credentialled = status_of(
            &get(
                bound,
                path,
                &[(
                    "Authorization",
                    &format!("Bearer {token}", token = preview.token),
                )],
            )
            .await,
        );
        assert_ne!(
            credentialled, 401,
            "{path} refused the token this run printed"
        );
        assert_ne!(
            credentialled, 404,
            "{path} is on the camera-bearing list and is not a route: the asset table answered it"
        );
    }
    assert!(
        driven > 0,
        "the camera-bearing list is empty, so every claim above quantified over nothing"
    );

    // The other side of the same ruling, asserted against the same listener in the same run:
    // the page is served to a request presenting **nothing**. A build that put the gate back
    // over everything would satisfy every claim above and fail here, which is what makes this
    // test about the split rather than about the gate alone.
    let page = get(bound, "/", &[]).await;
    assert!(
        page.starts_with("HTTP/1.1 200"),
        "the client's own page was not served anonymously: {page:.64}"
    );

    // And the preview with the credential is a `200` carrying frames, so the refusals above are
    // about the credential rather than about a route that refuses everybody.
    let watching = preview.watching().await;
    assert_eq!(watching.status(), 200);
}

#[tokio::test]
async fn a_preview_of_a_camera_that_is_not_there_is_refused_before_a_stream_exists() {
    // The two-status projection over a real socket, at both arms. A `404` for a name no camera
    // answers to and a `400` for a request that named none — and in both cases *no camera is
    // opened*, which is the half that says the refusal happened before the device rather than
    // by a stream that started and stopped.
    let preview = Preview::start().await;
    let bound = preview.serving.bound();
    let credential = format!(
        "{token}={secret}",
        token = http::TOKEN_QUERY_PARAM,
        secret = preview.token
    );

    let unknown = get(
        bound,
        &format!(
            "{path}?{camera}=cam%3Anothing-answers-to-this&{credential}",
            path = http::PREVIEW_PATH,
            camera = http::CAMERA_QUERY_PARAM,
        ),
        &[],
    )
    .await;
    assert!(unknown.starts_with("HTTP/1.1 404"), "{unknown:.64}");

    let unnamed = get(
        bound,
        &format!("{path}?{credential}", path = http::PREVIEW_PATH),
        &[],
    )
    .await;
    assert!(unnamed.starts_with("HTTP/1.1 400"), "{unnamed:.64}");

    assert_eq!(preview.backend.opens(), 0, "a refusal opened a camera");
    assert_eq!(preview.wchd.previewed_cameras(), 0);
}

#[tokio::test]
async fn a_photo_taken_during_a_preview_suspends_the_stream_and_the_preview_resumes() {
    // **The owner's ruling of 2026-08-12, over a real socket** — and the replacement for the
    // test that pinned the behaviour it overturned (note **N83**). Until today a `wch_photo`
    // taken while a preview held the stream met `Busy` from the device, because V4L2 allows
    // one streamer per node; now the photo command stops that stream, takes its frame and
    // starts it again inside the camera's own actor thread, so no client sequences anything.
    //
    // Four claims, none implying the others:
    //
    //   1. **the photo happens**, and its bytes are still the camera's own bitstream — the
    //      thing AGENTS calls the product ("verbatim camera JPEG when the sink allows");
    //   2. **the interruption is counted**, read off the daemon's own number rather than
    //      inferred from the frames — a pause publishes nothing and loses nothing, so it is
    //      invisible to every other counter this daemon keeps;
    //   3. **the preview resumes**: frames are published *after* the photo's answer that did
    //      not exist before it, awaited on the daemon's count rather than slept for;
    //   4. **exclusivity survives**: one descriptor, one feed, and exactly three streams —
    //      the preview's, the photo's own, and the resume.
    let preview = Preview::start().await;
    let mut stream = preview.watching().await;
    assert!(stream.part().await.is_jpeg());

    let taken = photograph(&preview.photo(default_settle()).await);
    // Claim 1. `bytes_match_the_delivery` is note N34's predicate — the daemon refused to
    // send an answer that fails it, and this is the client half of the same check.
    assert!(taken.bytes_match_the_delivery());
    assert!(
        taken.report.rendering.is_verbatim(),
        "a photo taken during a preview was re-encoded: {:?}",
        taken.report.rendering
    );
    let bytes = taken
        .bytes
        .as_ref()
        .expect("a return_bytes sink")
        .as_slice();
    assert!(
        bytes.starts_with(&[0xff, 0xd8]) && bytes.ends_with(&[0xff, 0xd9]),
        "the payload is not a JPEG bitstream"
    );

    // Claim 2, and it is a number this test *reads*: one photo, one interruption.
    assert_eq!(
        *preview.wchd.watch_preview_interruptions().borrow(),
        1,
        "the pause was not counted"
    );

    // Claim 3. The count is sampled **after** the answer arrived, so every frame past it was
    // published after the stream came back — a build that skipped the resume never moves it.
    // Bounded rather than left to the runner: the driver takes its next frame within one
    // `PREVIEW_FRAME_WAIT_MS`, so twice that is a whole turn of slack, and the difference
    // between "this test failed" and "this run timed out" is a diagnosis.
    let mut published = preview.wchd.watch_preview_frames();
    let resumed_at = *published.borrow_and_update();
    let flowing = tokio::time::timeout(
        Duration::from_millis(limits::PREVIEW_FRAME_WAIT_MS * 2),
        published.wait_for(|count| *count > resumed_at),
    )
    .await;
    assert!(
        flowing.is_ok(),
        "no frame was published in {ms} ms after the photo: the preview did not resume",
        ms = limits::PREVIEW_FRAME_WAIT_MS * 2
    );

    // ... and the tab is still being served, which is the same fact from the client's end.
    assert!(stream.part().await.is_jpeg());

    // Claim 4.
    assert_eq!(
        preview.backend.opens(),
        1,
        "the photo opened a second handle"
    );
    assert_eq!(
        preview.backend.streams_started(),
        3,
        "one preview, one photo and one resume is three streams — no more and no fewer"
    );
    assert_eq!(preview.wchd.previewed_cameras(), 1);
}

#[tokio::test]
async fn a_photo_with_no_preview_running_answers_exactly_as_it_did_before() {
    // The other direction of the test above, and the one that says the mechanism is
    // *conditional*: with nobody watching, `wch_photo` is the verb it has always been — one
    // stream, no interruption, no feed. A build that took the suspend path anyway would stop
    // a stream that was not running (harmless) and then **start one nobody stops**, which is
    // the second number here.
    let preview = Preview::start().await;
    let taken = photograph(&preview.photo(default_settle()).await);
    assert!(taken.bytes_match_the_delivery());
    assert!(taken.report.rendering.is_verbatim());

    assert_eq!(
        *preview.wchd.watch_preview_interruptions().borrow(),
        0,
        "a photo with nobody watching reported an interruption"
    );
    assert_eq!(
        preview.backend.streams_started(),
        1,
        "the photo's own stream, and no other"
    );
    assert_eq!(preview.wchd.previewed_cameras(), 0);
}

#[tokio::test]
async fn a_capture_that_fails_mid_photo_still_leaves_the_preview_streaming() {
    // AGENTS rule 8 at the place the code that exists to honour it could break it: a photo
    // that errors must not leave the camera dark for a tab that is still open.
    //
    // **The failure is a settle deadline of zero rather than one of the fake's frame faults,
    // and that is deliberate.** `Fault::FrameTimeout` and `Fault::DeviceGoneMidStream` are
    // consumed by whichever `next_frame` reaches them first, and while a preview is running
    // there is a `next_frame` in flight continuously — so a queued frame fault here would be
    // a race between the preview's turn and the photo's capture, and this project does not
    // write races. A deadline that has already passed fails the *capture* and nothing else,
    // deterministically, after the suspend and before the frame: `crates/engine`'s own suite
    // drives the same path off the scriptable double's fault menu, where the double is
    // exclusive and the injection is exact.
    let preview = Preview::start().await;
    let mut stream = preview.watching().await;
    assert!(stream.part().await.is_jpeg());

    let refusal = refusal(
        &preview
            .photo(r#"{"spec":{"kind":"skip_frames","frames":0},"deadline_ms":0}"#)
            .await,
    );
    assert!(
        refusal.contains("settle"),
        "a spent deadline was not reported as a settle failure: {refusal}"
    );

    // Counted on the failure path too — the interruption happened whatever the picture did,
    // and a gap counted only when the photo came out would under-report exactly the case an
    // operator would be investigating.
    assert_eq!(*preview.wchd.watch_preview_interruptions().borrow(), 1);

    // And the preview is streaming: frames published after the failed photo, then a part on
    // the wire. A build whose resume ran only on the success path leaves both dead.
    let mut published = preview.wchd.watch_preview_frames();
    let resumed_at = *published.borrow_and_update();
    let flowing = tokio::time::timeout(
        Duration::from_millis(limits::PREVIEW_FRAME_WAIT_MS * 2),
        published.wait_for(|count| *count > resumed_at),
    )
    .await;
    assert!(
        flowing.is_ok(),
        "a photo that failed left the preview stopped"
    );
    assert!(stream.part().await.is_jpeg());
    assert_eq!(preview.backend.streams_started(), 3);
    assert_eq!(preview.wchd.previewed_cameras(), 1);
}

#[tokio::test]
async fn two_photos_at_once_during_a_preview_never_hold_two_streams() {
    // **The claim that the suspend, the capture and the resume are one indivisible
    // operation**, which is the ruling's own word for where it had to live. It is true by
    // construction rather than by a lock — the whole sequence runs inside one
    // `engine::actor` command, on the thread that owns the device, and that thread takes one
    // command at a time in arrival order — so the thing that would break it is a build which
    // split the sequence across two commands, or ran it anywhere but there.
    //
    // Such a build fails here and nowhere else in this suite: with the sequence interruptible,
    // the second photo's `STREAMON` lands between the first photo's stop and its restart, and
    // one of the two meets `Busy` from the device (or the first photo's resume does). Two
    // requests in flight at once is the only shape that can reach that window.
    let preview = Preview::start().await;
    let mut stream = preview.watching().await;
    assert!(stream.part().await.is_jpeg());

    let (first, second) = tokio::join!(
        preview.photo(default_settle()),
        preview.photo(default_settle())
    );
    for (which, answer) in [("the first", &first), ("the second", &second)] {
        let taken = photograph(answer);
        assert!(
            taken.bytes_match_the_delivery(),
            "{which} photo answered a document that disagrees with itself"
        );
    }

    // Two suspensions, and five streams: the preview's, then a capture and a resume each.
    assert_eq!(*preview.wchd.watch_preview_interruptions().borrow(), 2);
    assert_eq!(
        preview.backend.streams_started(),
        5,
        "two photos during one preview is one stream per capture plus one resume each"
    );
    assert_eq!(preview.wchd.previewed_cameras(), 1);
    assert!(stream.part().await.is_jpeg(), "the tab stopped painting");
}

#[tokio::test]
async fn two_tabs_and_a_photo_between_them_are_still_one_streamer() {
    // D12's "exclusive streaming by construction" at the case the ruling could have broken:
    // two readers of one camera, and a photo in the middle of both. The suspension is the
    // *feed's* stream rather than a tab's, so neither tab is told anything and both keep
    // painting — and the counts say there was never a moment with two streamers on the node.
    let preview = Preview::start().await;
    let mut first = preview.watching().await;
    let mut second = preview.watching().await;
    assert!(first.part().await.is_jpeg());
    assert!(second.part().await.is_jpeg());
    assert_eq!(preview.backend.streams_started(), 1, "two tabs, one stream");

    let taken = photograph(&preview.photo(default_settle()).await);
    assert!(taken.bytes_match_the_delivery());

    assert!(
        first.part().await.is_jpeg(),
        "the first tab stopped painting"
    );
    assert!(
        second.part().await.is_jpeg(),
        "the second tab stopped painting"
    );
    assert_eq!(preview.backend.opens(), 1);
    assert_eq!(
        preview.backend.streams_started(),
        3,
        "two tabs and one photo is still one preview stream, suspended once"
    );
    assert_eq!(preview.wchd.previewed_cameras(), 1);
    assert_eq!(*preview.wchd.watch_preview_interruptions().borrow(), 1);
}

// ------------------------------------------------- P6c's second half: the recording feeds it
//
// The notes' Expected usage item 10 said P6 owed an answer to a recording and a preview
// colliding, and named the two honest options. The owner ruled on 2026-08-14 (note **N117**)
// for the expensive one: **while a recording runs it is the only streamer, and its frames feed
// the preview too.** The six claims below are that ruling and its edges, over a real socket.

#[tokio::test]
async fn a_recording_feeds_the_preview_that_was_already_watching_and_never_opens_a_second_stream() {
    // **The ruling itself**, from the side that can see it: a tab that was already watching
    // keeps getting pictures on the *same* HTTP response while a take runs, and the camera is
    // streamed exactly twice — once for the preview and once for the take.
    //
    // Four claims, and none implies the others:
    //
    //   1. the take really records — `frames_written` is non-zero, so the frames were not
    //      diverted to the viewers instead of the container;
    //   2. the preview really goes on — the daemon publishes past a count sampled *after*
    //      `record_start` answered, which is past the moment the preview's own driver left;
    //   3. those publications reach a reader — a part is read off the socket whose index is
    //      past that sample, on the response opened before the take began;
    //   4. exclusivity survives — two `STREAMON`s for the whole exchange. A build that let the
    //      preview keep its own stream would answer three, and one that let `attach` start a
    //      second would have been refused `Busy` by the fake, exactly as V4L2 refuses it.
    let preview = Preview::start().await;
    let scratch = engine::paths::TempRuntimeDir::new().expect("a throw-away directory");
    let path = scratch.base().join("watched.avi");
    let mut stream = preview.watching().await;
    let before = stream.part().await;
    assert!(before.is_jpeg());
    assert_eq!(preview.backend.streams_started(), 1);

    let started = preview
        .record(&path, A_TAKE_LONGER_THAN_THIS_TEST, None)
        .await;
    assert!(
        started.get("result").is_some(),
        "the recording was refused: {refused}",
        refused = refusal(&started)
    );
    // Sampled after the answer, so everything past it was published by the take's own driver.
    let handed_over_at = *preview.wchd.watch_preview_frames().borrow();
    assert_eq!(
        preview.backend.streams_started(),
        2,
        "a recording that took a preview over started more than one stream"
    );
    assert_eq!(preview.wchd.previewed_cameras(), 1);
    assert_eq!(preview.wchd.running_recordings(), 1);

    // Claim 2, then claim 3 — the second is the first arriving at a socket, and a build that
    // published into a channel nobody was subscribed to would satisfy neither.
    assert!(
        preview.published_past(handed_over_at).await,
        "a recording published no frame to the preview it took over"
    );
    let during = loop {
        let part = stream.part().await;
        if part.index > handed_over_at {
            break part;
        }
    };
    assert!(during.is_jpeg(), "a part the recording fed is not a JPEG");
    // **Asked after the frames, and it is what makes them the take's.** A hand-back puts a
    // driver back on the feed, so every claim above is also satisfiable by a build that fed the
    // viewers nothing and let the take end — which is what a hand-applied mutant did (note
    // **N117**, M1). The take is still running, so what has been read came off its stream.
    assert_eq!(preview.wchd.running_recordings(), 1, "the take ended early");

    let report = preview.stop_recording().await;
    let written = report
        .pointer("/result/summary/frames_written")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| {
            panic!(
                "the take was refused: {refused}",
                refused = refusal(&report)
            )
        });
    assert!(
        written > 0,
        "the container took no frame, so the fan-out was fed instead of the muxer"
    );
    // Claim 4, over the whole exchange: the preview's own stream, the take's, and the resume
    // the hand-back spawned. A build that started a second stream anywhere answers more.
    // Sampled into a local **before** the await, and the reason is a deadlock rather than
    // style: `watch::Receiver::borrow` hands back a `Ref` that holds the channel's read lock,
    // and the thing that would take the write lock is `Publisher::publish` — on the camera's
    // actor thread, inside a `DQBUF` command. A `Ref` alive across an `await` in this file
    // therefore wedges the camera it is asking about, which is exactly how the first draft of
    // this suite hung.
    let stopped_at = *preview.wchd.watch_preview_frames().borrow();
    assert!(
        preview.published_past(stopped_at).await,
        "the preview did not come back after the take"
    );
    assert_eq!(preview.backend.streams_started(), 3);
    assert_eq!(preview.backend.opens(), 1, "a second descriptor was opened");
}

#[tokio::test]
async fn a_preview_that_arrives_mid_recording_joins_the_take_rather_than_asking_for_a_stream() {
    // The edge with the least code behind it and the most that could go wrong: a tab opened
    // while a take is running. `Previews::attach` finds the feed the take created and takes its
    // existing second-tab branch, so nothing asks whether a recording is running — and the
    // proof that nothing does is the *stream count*, because a build that started one would
    // have been refused `Busy` by the fake (which enforces one streamer per node exactly as
    // V4L2 does) and this `watching()` would have failed instead of painting.
    let preview = Preview::start().await;
    let scratch = engine::paths::TempRuntimeDir::new().expect("a throw-away directory");
    let path = scratch.base().join("joined.avi");

    let started = preview
        .record(&path, A_TAKE_LONGER_THAN_THIS_TEST, None)
        .await;
    assert!(
        started.get("result").is_some(),
        "the recording was refused: {refused}",
        refused = refusal(&started)
    );
    assert_eq!(
        preview.backend.streams_started(),
        1,
        "the take's own stream"
    );

    let mut stream = preview.watching().await;
    assert_eq!(stream.status(), 200);
    let part = stream.part().await;
    assert!(part.is_jpeg(), "the tab that joined a take got no picture");
    assert_eq!(preview.wchd.running_recordings(), 1, "the take ended early");
    assert_eq!(
        preview.backend.streams_started(),
        1,
        "attaching to a camera that was recording started a second stream"
    );

    let report = preview.stop_recording().await;
    assert!(
        report
            .pointer("/result/summary/frames_written")
            .and_then(Value::as_u64)
            .is_some_and(|written| written > 0),
        "the take that fed a tab recorded nothing: {refused}",
        refused = refusal(&report)
    );
}

#[tokio::test]
async fn a_take_that_ends_gives_the_camera_back_and_the_tab_keeps_its_own_response() {
    // The third edge, and the one where the wrong answer reads as working: when a take ends,
    // the preview must either **resume its own stream** or **end**, and ending it would be a
    // second home for "how a preview ends" (`Previews::interrupted`'s posture, §2.10) as well
    // as a tab that goes dark every time an agent records. So the feed gets a driver again.
    //
    // What makes this more than "frames arrive" is the element identity of the *response*: the
    // same `Stream` is read either side of the take, so the picture came back on the request
    // the client already had rather than on one it would have had to open.
    let preview = Preview::start().await;
    let scratch = engine::paths::TempRuntimeDir::new().expect("a throw-away directory");
    let path = scratch.base().join("given-back.avi");
    let mut stream = preview.watching().await;
    assert!(stream.part().await.is_jpeg());

    preview
        .record(&path, A_TAKE_LONGER_THAN_THIS_TEST, None)
        .await;
    preview.stop_recording().await;

    // The hand-back runs on the take's driver before the container is closed, and `record_stop`
    // answers after the close — so by the time the call above returned, the feed had a driver
    // again. What is awaited here is its first frame.
    let resumed_at = *preview.wchd.watch_preview_frames().borrow();
    assert!(
        preview.published_past(resumed_at).await,
        "the preview did not resume when the take ended"
    );
    let after = stream.part().await;
    assert!(after.is_jpeg());
    assert_eq!(
        preview.wchd.previewed_cameras(),
        1,
        "the feed was withdrawn"
    );
    assert_eq!(preview.wchd.running_recordings(), 0);
    assert_eq!(
        preview.backend.streams_started(),
        3,
        "the preview's, the take's and the resume — no more and no fewer"
    );
}

#[tokio::test]
async fn a_device_that_failed_mid_take_reaches_the_collector_and_never_the_readers_as_a_success() {
    // AGENTS rule 7 across the two consumers at once. A camera that vanishes mid-take has two
    // audiences with different questions, and the answers must not be swapped:
    //
    //   * whoever collects the take is told the **device's own refusal**, never a report;
    //   * whoever is watching gets their stream **ended**, because the resume meets the same
    //     dead device — through `drive`'s existing refusal path, which is the one home for
    //     "how a preview ends".
    //
    // A build whose hand-back invented a success for the readers — a feed left in the registry
    // with nothing publishing into it — leaves the tab waiting forever on a camera that is
    // gone, which is the shape of a preview that reads as working and is not.
    //
    // The fault is exact here in a way note **N83** says it is not during a preview: while a
    // take runs, the take's driver is the only thing calling `next_frame`, so the queued
    // `DeviceGoneMidStream` is consumed by it and by nothing else.
    let preview = Preview::start().await;
    let scratch = engine::paths::TempRuntimeDir::new().expect("a throw-away directory");
    let path = scratch.base().join("vanished.avi");
    let mut stream = preview.watching().await;
    assert!(stream.part().await.is_jpeg());

    preview
        .record(&path, A_TAKE_LONGER_THAN_THIS_TEST, None)
        .await;
    // The take is **provably turning** before the camera is taken away, which is not a nicety:
    // a fault armed before the driver's first `DQBUF` is a fault the *stop* below outruns, and
    // the take then ends `Stopped` with a perfectly good report — this test failed that way one
    // run in eight before this line existed, which is the shape of an assertion that passes for
    // the wrong reason most of the time.
    let recording_at = *preview.wchd.watch_preview_frames().borrow();
    assert!(
        preview.published_past(recording_at).await,
        "the take never reached the device, so there was nothing for a fault to interrupt"
    );

    // **Held rather than queued**, which is the fault menu's own distinction and the right one
    // here: a queued fault fires once, so the camera would be back by the time the hand-back's
    // fresh driver asked it for a frame, and this claim is about a device that is *gone*. A
    // held fault is a condition, and "the camera did not come back" is a condition.
    preview.backend.hold_fault(fake::Fault::DeviceGoneMidStream);

    // Awaited rather than stopped: the take ends **on the device's own refusal**, which is the
    // ending this claim is about, and a `record_stop` racing it would sometimes end it on the
    // caller's word instead. `watch_finished_recordings` is the driver's own signal that it
    // reached an ending (`crate::record::Recordings::watch_finished`), and the `record_stop`
    // after it collects a take that is already over — which N114's fourth decision says is the
    // ordinary case rather than a special one.
    let mut finished = preview.wchd.watch_finished_recordings();
    let ended = tokio::time::timeout(
        Duration::from_millis(limits::PREVIEW_FRAME_WAIT_MS * 4),
        finished.wait_for(|count| *count > 0),
    )
    .await
    .is_ok();
    assert!(
        ended,
        "a camera that vanished did not end the take it was in"
    );

    let collected = preview.stop_recording().await;
    assert_eq!(
        collected.pointer("/error/code").and_then(Value::as_i64),
        Some(i64::from(api::codes::rpc_code(
            schema::ErrorKind::DeviceGone
        ))),
        "a camera that vanished was collected as a recording: {collected}"
    );

    // And the readers' end of it: whatever was buffered arrives and then the response **ends**,
    // rather than a tab left attached to a feed nothing will publish to again. Bounded, so a
    // build that left it open fails here rather than hanging.
    let mut reads = 0;
    while stream.fill().await > 0 {
        reads += 1;
        assert!(reads < 4_096, "the tab outlived the camera");
    }
    let mut feeds = preview.wchd.watch_previewed_cameras();
    let emptied = tokio::time::timeout(
        Duration::from_millis(limits::PREVIEW_FRAME_WAIT_MS * 4),
        feeds.wait_for(|live| *live == 0),
    )
    .await
    .is_ok();
    assert!(
        emptied,
        "a camera that vanished left a feed in the registry"
    );
}

#[tokio::test]
async fn a_take_a_browser_cannot_paint_shows_the_readers_nothing_and_says_how_much_nothing() {
    // The edge the ruling does not reach, answered rather than left to be discovered. D7's raw
    // fallback is a real answer — `record` a `.y4m` and the stream is YUYV — and those bytes in
    // an `<img>` labelled `image/jpeg` are a *broken* image rather than a wrong one. So
    // `Publisher::publish` drops them, and the drop is a **number** (rubric rule 3) because it
    // is the only observable a frozen preview has: nothing is published, so the published count
    // does not move and no reader falls behind.
    //
    // Both directions, because the guard has to be about the *format* rather than about
    // recordings: the MJPG take in the claims above publishes, and this one does not.
    let preview = Preview::start().await;
    let scratch = engine::paths::TempRuntimeDir::new().expect("a throw-away directory");
    let path = scratch.base().join("raw.y4m");
    let mut stream = preview.watching().await;
    let last = stream.part().await;
    assert!(last.is_jpeg());

    let started = preview
        .record(&path, A_TAKE_LONGER_THAN_THIS_TEST, Some("YUYV"))
        .await;
    assert_eq!(
        started.pointer("/result/take/negotiated/pixel_format"),
        Some(&Value::String("YUYV".to_owned())),
        "this take was meant to be one a browser cannot paint: {started}"
    );
    let handed_over_at = *preview.wchd.watch_preview_frames().borrow();

    let mut unpaintable = preview.wchd.watch_unpaintable_preview_frames();
    // `.is_ok()` on the spot, because `wait_for` answers a `Ref` that holds the channel's read
    // lock and the next writer is `Publisher::publish` on the camera's actor thread — see
    // `a_recording_feeds_the_preview_that_was_already_watching_and_never_opens_a_second_stream`
    // for the hang that costs.
    let dropped = tokio::time::timeout(
        Duration::from_millis(limits::PREVIEW_FRAME_WAIT_MS * 4),
        unpaintable.wait_for(|count| *count > 0),
    )
    .await
    .is_ok();
    assert!(dropped, "a YUYV take neither published nor counted a frame");
    assert_eq!(
        *preview.wchd.watch_preview_frames().borrow(),
        handed_over_at,
        "raw bytes were published into a route that serves image/jpeg"
    );
    assert_eq!(preview.wchd.running_recordings(), 1, "the take ended early");

    // ... and when the take ends the picture comes back, so the drop is a pause rather than a
    // way for a raw recording to kill a tab.
    preview.stop_recording().await;
    assert!(
        preview.published_past(handed_over_at).await,
        "the preview did not resume after a take it could not be fed from"
    );
    assert!(stream.part().await.is_jpeg());
}

#[tokio::test]
async fn a_record_start_refused_after_it_took_the_camera_gives_the_preview_back() {
    // **The path that is easy to build and easy to forget**, and a hand-applied mutant proved
    // it: with the hand-back deleted from `Recordings::withdraw`, the whole workspace suite
    // stayed green (note **N117**, mutant M9). A `record_start` claims the camera's frames
    // *before* any device work — that is what stops two loops dequeuing from one stream — so
    // every path out of it that is not a running take owes them back. There are three such paths
    // and they all go through one function, which is why `Reserved` carries the claim rather
    // than the handler holding it beside the slot.
    //
    // The refusal is reached the way a caller reaches it: `.avi` over a stream that negotiated
    // YUYV is D7's pairing saying this container cannot carry these frames — refused **after**
    // the negotiation, which is exactly the window where the preview has already been stopped
    // and the take does not exist yet.
    let preview = Preview::start().await;
    let scratch = engine::paths::TempRuntimeDir::new().expect("a throw-away directory");
    let path = scratch.base().join("mismatched.avi");
    let mut stream = preview.watching().await;
    assert!(stream.part().await.is_jpeg());

    let refused = preview
        .record(&path, A_TAKE_LONGER_THAN_THIS_TEST, Some("YUYV"))
        .await;
    assert_eq!(
        refused.pointer("/error/code").and_then(Value::as_i64),
        Some(i64::from(api::codes::rpc_code(
            schema::ErrorKind::FormatUnsupported
        ))),
        "this request was meant to be refused by D7's pairing: {refused}"
    );
    assert_eq!(
        preview.wchd.running_recordings(),
        0,
        "a refused record_start left a take behind"
    );

    // The preview is back — on the same response, from the daemon's own count — and the camera
    // is free for the next caller, which is the other half of "as this call found it".
    let refused_at = *preview.wchd.watch_preview_frames().borrow();
    assert!(
        preview.published_past(refused_at).await,
        "a refused recording left the tab watching a feed nothing publishes into"
    );
    assert!(stream.part().await.is_jpeg());
    assert_eq!(preview.wchd.previewed_cameras(), 1);
}

#[tokio::test]
async fn a_photo_during_a_take_is_told_to_retry_rather_than_stopping_the_takes_stream() {
    // Note **N118**, at the one place this build could break the measurement a recording exists
    // to carry. `engine::preview::while_suspended` stops whatever is streaming, takes a frame
    // and starts it again; `Camera::streaming` cannot say what the stream is *for*; and P6c made
    // a recording the second thing that streams across commands. A photo taken during a take
    // would therefore cost the take those frames and put a gap in the driver's timestamps that
    // D7's close-time rewrite spreads over the whole file — item 10's "a dropped frame must not
    // look like a slow transition", from the inside.
    //
    // So it is refused, with the word that tells an unattended caller to try again, and the
    // take is unharmed: it keeps recording and keeps feeding the tab. Both halves are asserted,
    // because a build that refused *every* photo would satisfy the first on its own — which is
    // what the last two lines are for.
    let preview = Preview::start().await;
    let scratch = engine::paths::TempRuntimeDir::new().expect("a throw-away directory");
    let path = scratch.base().join("undisturbed.avi");
    let mut stream = preview.watching().await;
    assert!(stream.part().await.is_jpeg());

    preview
        .record(&path, A_TAKE_LONGER_THAN_THIS_TEST, None)
        .await;
    // Turning before the photo, so the frame count at the end is a fact about a take that ran
    // rather than a race with the driver's first `DQBUF` —
    // `a_device_that_failed_mid_take_reaches_the_collector_and_never_the_readers_as_a_success`
    // is where that flake was measured and what it costs.
    let recording_at = *preview.wchd.watch_preview_frames().borrow();
    assert!(
        preview.published_past(recording_at).await,
        "the take never reached the device"
    );

    let refused = preview.photo(default_settle()).await;
    assert_eq!(
        refused.pointer("/error/code").and_then(Value::as_i64),
        Some(i64::from(api::codes::rpc_code(schema::ErrorKind::Busy))),
        "a photo during a take was not told to retry: {refused}"
    );
    assert_eq!(
        *preview.wchd.watch_preview_interruptions().borrow(),
        0,
        "the refused photo suspended a stream anyway"
    );
    assert_eq!(
        preview.backend.streams_started(),
        2,
        "the refused photo started a stream of its own"
    );

    let report = preview.stop_recording().await;
    assert!(
        report
            .pointer("/result/summary/frames_written")
            .and_then(Value::as_u64)
            .is_some_and(|written| written > 0),
        "the take did not survive the refused photo: {refused}",
        refused = refusal(&report)
    );

    // The other direction, so the refusal above is about a *running take* rather than about
    // photos: with the take collected, the same call answers a photograph.
    let taken = photograph(&preview.photo(default_settle()).await);
    assert!(taken.bytes_match_the_delivery());
}
