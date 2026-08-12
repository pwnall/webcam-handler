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
//! constructor states: **a field is rewritten into a shape a device really exhibits.** 160×120
//! MJPG is a mode real webcams offer, and it makes every frame in this file a few kilobytes.
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

use daemon::http;
use daemon::server::Wchd;
use daemon::shutdown::Shutdown;
use engine::store::{LockProtocol, SessionStore, TempStore};
use fake::FakeBackend;
use schema::backend::CameraBackend;
use schema::camera::{CameraId, FrameInterval, FrameSize, FrameSizeInfo, PixelFormat};
use schema::limits;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpSocket, TcpStream};

// ------------------------------------------------------------------------- the fixture

/// One daemon, one small-MJPEG camera, and the web listener D11 opens.
///
/// Everything is held here because dropping any of it takes the state directory, the lock or
/// the listener with it — the arrangement a real `wchd` has.
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
    /// `wchd` itself calls.
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

/// The committed profile with its MJPEG modes rewritten small.
///
/// A rewrite rather than a second committed document, and only into a shape a device really
/// has: 160×120 at 30 fps is an ordinary webcam mode. What it buys is that every frame in this
/// file is kilobytes rather than a quarter-megapixel render, so the suite exercises the daemon
/// rather than the fake's JPEG encoder. The YUYV entry is left exactly as it was, because a
/// camera that offers only one format is not the shape \[PF:9\] records.
fn small_mjpeg_camera() -> schema::profile::DeviceProfile {
    let mut profile = testkit::fixtures::synthetic_basic();
    for format in &mut profile.invariant.formats {
        if format.pixel_format == PixelFormat::MJPG {
            format.sizes = vec![FrameSizeInfo {
                size: FrameSize::Discrete {
                    width: 160,
                    height: 120,
                },
                intervals: vec![FrameInterval::Discrete {
                    numerator: 1,
                    denominator: 30,
                }],
            }];
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
            let read = self.read().await;
            if read == 0 {
                return 0;
            }
            let before = self.buffered.len();
            self.decode();
            if self.buffered.len() > before {
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

    let page = get(
        bound,
        &format!(
            "/?{token}={secret}",
            token = http::TOKEN_QUERY_PARAM,
            secret = preview.token
        ),
        &[("Accept-Encoding", "gzip")],
    )
    .await;
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
    // The owner's 2026-08-12 ruling keeps two routes gated — the WebSocket endpoint and this
    // one — and `daemon::http::CAMERA_BEARING_PATHS` is the list. The population is **read off
    // that list** rather than written here, so a third camera-bearing route added without a
    // gate is a failure in this test rather than a discovery in a review.
    //
    // Today the gate covers every route, so this passes for a reason wider than the list. That
    // is the point of driving the list rather than the router: the assertion survives the
    // narrowing, and it is the narrowing that would otherwise be the moment a route quietly
    // lost its gate.
    let preview = Preview::start().await;
    for path in http::CAMERA_BEARING_PATHS {
        let anonymous = get(preview.serving.bound(), path, &[]).await;
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
    }

    // And the preview with the credential is a `200`, so the assertions above are about the
    // credential rather than about a route that refuses everything.
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
async fn a_photo_while_a_preview_is_running_is_refused_by_the_device_and_the_preview_survives() {
    // The cost `crate::preview`'s header states, pinned so that changing it is a red test
    // rather than a surprise. One streamer per node is the kernel's rule (D12), so a `photo`
    // taken while a preview holds the stream meets `Busy` **from the device** — which is E3's
    // distinction (availability is not capability) and is the same answer a second application
    // on this machine would get.
    //
    // docs/7 P5c wants a photo trigger beside a live preview, so this is a behaviour that will
    // have to change; `engine::preview`'s header names the mechanism. What must not happen
    // meanwhile is the *preview* dying because somebody pressed the button, and the last two
    // assertions are that.
    let preview = Preview::start().await;
    let mut stream = preview.watching().await;
    assert!(stream.part().await.is_jpeg());

    let methods = daemon::server::mount(preview.wchd.clone()).expect("a second reader of T5");
    let request = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"wch_photo","params":{{"camera":"{camera}","request":{{"sink":{{"kind":"return_bytes","format":"jpeg"}}}}}}}}"#,
        camera = preview.camera.as_str()
    );
    let (answer, _subscriptions) = methods
        .raw_json_request(&request, 1)
        .await
        .expect("the surface answered");
    let answer = answer.to_string();
    assert!(
        answer.contains("busy"),
        "a photo during a preview was not refused as busy: {answer}"
    );

    // The preview is unharmed: still one stream, still painting.
    assert!(stream.part().await.is_jpeg());
    assert_eq!(preview.backend.streams_started(), 1);
    assert_eq!(preview.wchd.previewed_cameras(), 1);
}
