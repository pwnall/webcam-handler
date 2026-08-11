//! The transport, end to end: a real client, a real socket, a real server.
//!
//! This suite is what docs/7 P4b means by "integration-tested on both sides", and it is
//! the layer that notices a jsonrpsee bump that still compiles. Everything it asserts is
//! about the *transport* — that a byte stream over `AF_UNIX` carries a JSON-RPC call and
//! its answer, and that the server this crate builds starts and stops on demand. What the
//! daemon answers is the T5 surface's business and is asserted where that lands.
//!
//! The client is `support::call`: a hand-written HTTP/1.1 `POST` on a `UnixStream`, shared
//! with `read_verbs.rs` so the two socket suites cannot disagree about what the daemon
//! speaks. That module's header says why it is hand-written rather than hyper's.

mod support;

use camino::Utf8Path;
use daemon::uds::{self, SocketDir};
use engine::paths::TempRuntimeDir;
use engine::store::{LockProtocol, StoreLock, TempStore};
use jsonrpsee_server::RpcModule;
use support::{body, call};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::UnixStream;

/// A running daemon transport, and everything that has to outlive it.
///
/// The state directory and the runtime directory are held as fields because dropping
/// either would take the lock and the socket with it — which is the arrangement a real
/// daemon has, and the reason this fixture is one value rather than four locals.
struct Serving {
    handle: uds::Serving,
    dir: SocketDir,
    _lock: StoreLock,
    _store: TempStore,
    _runtime: TempRuntimeDir,
}

impl Serving {
    /// Start the transport over a private socket directory. Must be inside a runtime.
    fn start() -> Serving {
        let store = TempStore::new().expect("a state directory");
        let lock = store
            .store()
            .lock(LockProtocol::HeldForLifetime)
            .expect("nothing else holds a throw-away state directory");
        let runtime = TempRuntimeDir::new().expect("a runtime directory");
        let dir = SocketDir::prepare(&runtime.env()).expect("a fresh, private socket directory");
        let listener = dir.bind(&lock).expect("nothing is in the way");

        Serving {
            handle: uds::serve(listener, probe_module()),
            dir,
            _lock: lock,
            _store: store,
            _runtime: runtime,
        }
    }

    fn socket(&self) -> camino::Utf8PathBuf {
        self.dir.socket_path()
    }
}

/// Connect, make one call, and **keep the connection open**.
///
/// The difference from [`call`] is the whole point: no `Connection: close`, and the answer
/// is read only far enough to prove the server accepted this connection and answered on
/// it. What comes back is the live stream, so a test can hold as many of them as it likes
/// — which is what a client that leaks connections does, and what the daemon's connection
/// bound exists to survive.
///
/// Reading one byte rather than a whole document is deliberate: it is the smallest thing
/// that proves the server got this far, and it needs no `Content-Length` parser. Nothing
/// waits on a clock; the read ends when the server writes.
async fn hold(socket: &Utf8Path, request: &str) -> std::io::Result<UnixStream> {
    let mut stream = UnixStream::connect(socket.as_std_path()).await?;
    let framed = format!(
        "POST / HTTP/1.1\r\n\
         Host: localhost\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         \r\n\
         {request}",
        request.len()
    );
    stream.write_all(framed.as_bytes()).await?;
    let mut first = [0_u8; 1];
    stream.read_exact(&mut first).await?;
    Ok(stream)
}

/// Ask for a WebSocket upgrade, and hand back whatever the server said.
///
/// The upgrade is a real one — the headers `soketto` looks for, and the nonce from RFC
/// 6455's own example — so that what the server says is a fact about the server rather than
/// about a malformed request. It is deliberately still hand-written rather than soketto's
/// client: this suite is about *what the socket speaks*, and the frame layer above the
/// handshake is `tests/subscriptions.rs`'s subject.
async fn upgrade(socket: &Utf8Path) -> std::io::Result<String> {
    let mut stream = UnixStream::connect(socket.as_std_path()).await?;
    let framed = "GET / HTTP/1.1\r\n\
         Host: localhost\r\n\
         Connection: Upgrade\r\n\
         Upgrade: websocket\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         Sec-WebSocket-Version: 13\r\n\
         \r\n";
    stream.write_all(framed.as_bytes()).await?;
    // The first chunk, not the whole document to end-of-file: a refusal that keeps the
    // connection alive would never reach EOF, and this only needs the status line.
    let mut head = vec![0_u8; 512];
    let read = stream.read(&mut head).await?;
    head.truncate(read);
    Ok(String::from_utf8_lossy(&head).into_owned())
}

/// One method, deliberately **not** a wire method.
///
/// The T5 surface is `webcam-handler-api`'s trait and the daemon mounts it with
/// `into_rpc()` — inventing a second registration path is the thing D10 exists to
/// prevent, and a transport suite that needed the real surface would be asserting the
/// routing as well as the pipe. `transport_probe` cannot be mistaken for a wire name: the
/// namespace is `wch` and `crates/api`'s own test pins every registered spelling.
fn probe_module() -> RpcModule<()> {
    let mut module = RpcModule::new(());
    module
        .register_method("transport_probe", |_params, _ctx, _| "pong")
        .expect("one method in a fresh module cannot collide");
    // A deliberately large answer, for the one bound that has no other way to be driven:
    // `RPC_MAX_RESPONSE_BYTES` exists because a 4K MJPG frame is ~8 MB \[PF:9\] and D10
    // answers a capture as base64 in the JSON result, which jsonrpsee's 10 MB default
    // refuses. Nothing routed at P4b returns anything that big, so without this the
    // constant would be a number with one writer and no reader until P4c discovered it as
    // a wire failure.
    module
        .register_method("transport_probe_large", |_params, _ctx, _| {
            "x".repeat(OVER_THE_JSONRPSEE_DEFAULT)
        })
        .expect("a second method in a fresh module cannot collide");
    module
}

/// Bigger than jsonrpsee's own 10 MB default response bound, and smaller than ours.
const OVER_THE_JSONRPSEE_DEFAULT: usize = 12 * 1024 * 1024;

#[tokio::test]
async fn a_json_rpc_call_and_its_answer_cross_a_unix_socket() {
    // The whole claim of this sub-milestone's transport half, in one round trip: a
    // `UnixStream` in the `io` slot of a function whose doc comment says "TCP", carrying
    // a request the server parses and an answer the client reads.
    let serving = Serving::start();

    let answer = call(
        &serving.socket(),
        r#"{"jsonrpc":"2.0","id":1,"method":"transport_probe","params":{}}"#,
    )
    .await
    .expect("the daemon is listening and answers");

    assert!(answer.starts_with("HTTP/1.1 200 OK"), "{answer}");
    assert_eq!(body(&answer), r#"{"jsonrpc":"2.0","id":1,"result":"pong"}"#);
}

#[tokio::test]
async fn an_unknown_method_is_refused_by_the_dispatcher_rather_than_by_the_socket() {
    // The other direction of the same claim, and the one that would be indistinguishable
    // from a broken pipe if the transport were merely "not erroring": a refusal is a
    // JSON-RPC document that travelled the whole way back, not a closed connection.
    let serving = Serving::start();

    let answer = call(
        &serving.socket(),
        r#"{"jsonrpc":"2.0","id":7,"method":"nothing_is_registered_here","params":{}}"#,
    )
    .await
    .expect("the daemon is listening and answers");

    assert!(answer.starts_with("HTTP/1.1 200 OK"), "{answer}");
    let json = body(&answer);
    assert!(json.contains("\"id\":7"), "{json}");
    assert!(json.contains("-32601"), "{json}");
}

#[tokio::test]
async fn a_batch_is_bounded_at_the_size_this_project_chose() {
    // jsonrpsee's default is `Unlimited`, which on the one socket the daemon always
    // serves is an unbounded loop over caller-supplied input — the thing AGENTS's
    // "bounded everything" rule exists for. The bound is `schema::limits`', and it is
    // read here rather than transcribed, so changing the constant moves this test with
    // it. -32010 is jsonrpsee's own "batch too large"; D13's block starts at -32012, so
    // there is no collision to reason about.
    let serving = Serving::start();
    let batch = |calls: u32| {
        let requests: Vec<String> = (0..calls)
            .map(|id| {
                format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"transport_probe","params":{{}}}}"#)
            })
            .collect();
        format!("[{}]", requests.join(","))
    };

    let at_the_bound = call(&serving.socket(), &batch(schema::limits::RPC_MAX_BATCH))
        .await
        .expect("the daemon is listening and answers");
    let answered = body(&at_the_bound);
    assert_eq!(
        answered.matches("pong").count(),
        usize::try_from(schema::limits::RPC_MAX_BATCH).expect("a small bound"),
        "{answered}"
    );

    let over = call(&serving.socket(), &batch(schema::limits::RPC_MAX_BATCH + 1))
        .await
        .expect("the daemon is listening and answers");
    let refused = body(&over);
    assert!(refused.contains("-32010"), "{refused}");
    assert!(!refused.contains("pong"), "{refused}");
}

#[tokio::test]
async fn the_server_stops_when_the_test_that_started_it_says_so() {
    // P4b's whole lifecycle claim, and the reason it exists: an integration test must be
    // able to end a server deterministically, because the alternative is a test that
    // sleeps and then kills something — which this workspace bans by name.
    //
    // It is *not* a claim that shutdown is graceful, that anything drains, or that a
    // stream is cancelled. Those landed at P4e-ii and are asserted where they live —
    // `daemon::shutdown`'s ordered teardown and its unit tests, and `tests/signals.rs`,
    // which drives both of them into a real process with a real signal. This test drives
    // `Serving` alone, which is the layer underneath all of it.
    let serving = Serving::start();
    let socket = serving.socket();

    // Alive first, so the assertion after the stop is about the stop.
    assert!(
        UnixStream::connect(socket.as_std_path()).await.is_ok(),
        "the daemon was not listening before it was asked to stop"
    );

    let mut serving = serving;
    serving.handle.stop();
    // Resolves once the accept loop and every connection it spawned are gone, and says
    // *why* it stopped. A server that ignored the stop handle would hang here rather than
    // passing, and one that reported a give-up as a clean stop would answer the wrong
    // thing — which is what `wchd`'s exit code is made of.
    serving
        .handle
        .stopped()
        .await
        .expect("the server was asked to stop");

    assert!(
        UnixStream::connect(socket.as_std_path()).await.is_err(),
        "something is still listening on {socket}"
    );

    // And the socket file is still there, because nothing unlinks it on the way out.
    // That is deliberate, and P4e-ii's orderly exit deliberately kept it that way: the exits
    // that matter run no code at all, so a cleanup only the orderly path performs is one the
    // failing path cannot rely on. It is exactly the leftover `SocketDir::bind` is written to
    // survive, and `tests/signals.rs` asserts the same file is still there after a real
    // SIGTERM has been drained.
    assert!(socket.as_std_path().exists(), "{socket}");
}

#[tokio::test]
async fn connections_are_bounded_at_the_number_this_project_chose() {
    // `limits::DAEMON_MAX_CONNECTIONS`'s doc says what the bound is for — "so a client
    // that leaks connections is refused rather than able to exhaust the daemon's file
    // descriptors, which on this process are also the camera's" — and jsonrpsee's
    // `max_connections` is not it: that permit is taken per in-flight HTTP *request* and
    // released with the response, so an accepted connection that then goes quiet consumes
    // none of it. Measured before this bound existed: with the cap at 32, 128 idle
    // connections were all accepted and held. This asserts the bound the doc describes.
    //
    // Both directions, at the boundary and deterministically: every held connection has
    // already been answered on, so the server has provably accepted it, and no test waits
    // for anything.
    let serving = Serving::start();
    let socket = serving.socket();
    let request = r#"{"jsonrpc":"2.0","id":1,"method":"transport_probe","params":{}}"#;
    let bound = usize::try_from(schema::limits::DAEMON_MAX_CONNECTIONS).expect("a small bound");

    let mut held = Vec::with_capacity(bound);
    for connection in 0..bound - 1 {
        held.push(
            hold(&socket, request)
                .await
                .unwrap_or_else(|err| panic!("connection {connection} was refused: {err}")),
        );
    }

    // One short of the bound, so the next caller is answered.
    let answer = call(&socket, request)
        .await
        .expect("the daemon is listening");
    assert_eq!(body(&answer), r#"{"jsonrpc":"2.0","id":1,"result":"pong"}"#);

    // ... and at the bound, the next caller is closed rather than answered. Not an error
    // document: there is no connection left to send one on, which is the point — the
    // refusal costs the daemon a descriptor for as long as it takes to drop one.
    held.push(
        hold(&socket, request)
            .await
            .expect("the last connection inside the bound"),
    );
    // Either an immediate end-of-stream or a reset, depending on how far the write got
    // before the daemon dropped its end. What matters is that no answer came back: the
    // refusal is the connection going away, because there is no connection left to send a
    // JSON-RPC error document on.
    match call(&socket, request).await {
        Ok(answered) => assert!(
            answered.is_empty(),
            "a connection past the bound was served: {answered}"
        ),
        Err(err) => assert_eq!(err.kind(), std::io::ErrorKind::ConnectionReset, "{err}"),
    }

    // And the connections already up are untouched by somebody else's refusal.
    let still_serving = held.first_mut().expect("the bound is not zero");
    still_serving
        .write_all(b"\r\n")
        .await
        .expect("a connection the daemon accepted before the bound was reached");
}

#[tokio::test]
async fn a_websocket_upgrade_is_accepted_now_that_this_build_bounds_it() {
    // **This assertion is inverted rather than deleted**, and the inversion is the whole
    // record of what P4e-i changed about the transport. Until it, `enable_ws` was off:
    // with it come two of jsonrpsee's numbers this project had not chosen —
    // `message_buffer_capacity`, which is a channel depth of the kind AGENTS says lives in
    // `schema::limits`, and `max_subscriptions_per_connection` — and shipping an unbounded,
    // untested transport for a consumer that did not exist yet is rubric A8's subject
    // (note N38). The consumer now exists, both numbers are `schema::limits`', and the
    // suite that drives them to their bounds is `tests/subscriptions.rs`.
    //
    // What is asserted here is only the *upgrade*: the same socket, the same accept loop,
    // one `GET` instead of a `POST`. That the upgraded connection then carries a
    // subscription is a claim about the surface and is made where the surface is.
    let serving = Serving::start();

    let answer = upgrade(&serving.socket())
        .await
        .expect("the daemon is listening");
    assert!(
        answer.starts_with("HTTP/1.1 101"),
        "the daemon declined the upgrade its subscriptions need: {answer}"
    );
    // The handshake really completed, rather than a 101 with no accept token — which is
    // what a server that had switched protocols without agreeing to *this* handshake would
    // send, and what a client would then hang on. The value is RFC 6455's own worked
    // example for the nonce `upgrade` sends.
    assert!(
        answer.contains("s3pPLMBiTxaQ9kYGzzhZRbK+xOo="),
        "the upgrade was answered without agreeing to the handshake: {answer}"
    );

    // The transport it already spoke is unaffected, which is what makes this a statement
    // about the upgrade rather than about the socket.
    let served = call(
        &serving.socket(),
        r#"{"jsonrpc":"2.0","id":2,"method":"transport_probe","params":{}}"#,
    )
    .await
    .expect("the daemon is listening");
    assert_eq!(body(&served), r#"{"jsonrpc":"2.0","id":2,"result":"pong"}"#);
}

#[tokio::test]
async fn a_request_body_is_bounded_at_the_size_this_project_chose() {
    // jsonrpsee's own default is 10 MB and this project's is `RPC_MAX_REQUEST_BYTES`; the
    // constant had one reader — the `ServerConfig` builder — and nothing that went red if
    // the call were dropped in a refactor. Read here rather than transcribed, so changing
    // the constant moves this test with it.
    let serving = Serving::start();
    let bound = usize::try_from(schema::limits::RPC_MAX_REQUEST_BYTES).expect("a small bound");

    // A well-formed request whose *padding* takes it past the bound: the refusal has to be
    // about the size, not about the JSON.
    let padded = |extra: usize| {
        format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"transport_probe","params":{{}},"pad":"{}"}}"#,
            "x".repeat(extra)
        )
    };
    let head = padded(0).len();

    let inside = call(&serving.socket(), &padded(bound - head))
        .await
        .expect("the daemon is listening");
    assert_eq!(body(&inside), r#"{"jsonrpc":"2.0","id":1,"result":"pong"}"#);

    // Past the bound the daemon stops reading, which a client writing the rest of its
    // body sees as a broken pipe. Either shape is the refusal; being *answered* is not.
    match call(&serving.socket(), &padded(bound - head + 1)).await {
        Ok(over) => assert!(
            !body(&over).contains("pong"),
            "a body past the bound was answered: {}",
            body(&over)
        ),
        Err(err) => assert!(
            matches!(
                err.kind(),
                std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
            ),
            "{err}"
        ),
    }
}

#[tokio::test]
async fn an_answer_past_jsonrpsees_own_default_still_crosses_the_socket() {
    // `RPC_MAX_RESPONSE_BYTES` is set from `schema::limits` precisely because jsonrpsee's
    // default would refuse the answer P4c's `wch_photo` has to give — a 4K MJPG frame is
    // ~8 MB [PF:9] and base64 is four bytes per three. The bound had one writer and no
    // reader; this is the reader, and it goes red if the `max_response_body_size` call is
    // ever dropped in a refactor rather than a sub-milestone later.
    let serving = Serving::start();

    let answer = call(
        &serving.socket(),
        r#"{"jsonrpc":"2.0","id":3,"method":"transport_probe_large","params":{}}"#,
    )
    .await
    .expect("the daemon is listening");

    let json = body(&answer);
    assert!(
        json.len() > OVER_THE_JSONRPSEE_DEFAULT,
        "the answer was truncated or refused: {} bytes",
        json.len()
    );
    assert!(
        json.contains(r#""id":3"#),
        "{}",
        &json[..json.len().min(200)]
    );
    assert!(
        usize::try_from(schema::limits::RPC_MAX_RESPONSE_BYTES).expect("a bound that fits")
            > OVER_THE_JSONRPSEE_DEFAULT,
        "this test's fixture is bigger than the bound it is meant to be inside"
    );
}
