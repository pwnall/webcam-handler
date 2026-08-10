//! The one JSON-RPC-over-`AF_UNIX` client this crate's tests use.
//!
//! Shared by every suite that reaches the socket — five binaries: `uds.rs`, which asserts the
//! transport itself, and `read_verbs.rs`, `mutating_verbs.rs`, `calibrate_verbs.rs` and
//! `method_surface.rs`, which assert what the daemon answers over it (the last four through
//! `support/wire.rs`) — because a second copy of the framing would be a second opinion about
//! what the daemon speaks, in the files most likely to disagree about it.
//!
//! **Who includes this is load-bearing, which is why the list above is a list** (note
//! **N49**). A `#[path]`-included module is compiled into *every* binary that includes it and
//! this workspace builds warnings as errors, so an item used by only some of them is a
//! `dead_code` failure in the rest. Adding a consumer therefore means every item here has to
//! be used by it, and adding an item means every existing consumer has to use it — which is
//! also why these three headers name their includers rather than saying "the suites".
//!
//! ## Why it is hand-written
//!
//! jsonrpsee's own client speaks TCP or WebSocket over a URL; the UDS client transport is
//! P4f's (design §2.6: "a small client transport (~200 lines, modeled on reth's production
//! IPC adapter — vendored knowledge, not a git dependency"). Pulling hyper in as a
//! dev-dependency to avoid twenty lines of HTTP would also hide the thing these suites are
//! for: the socket carries **HTTP/1.1**, because the service jsonrpsee hands us is an HTTP
//! service, and P4f has to build against that fact rather than discover it. Writing the
//! request by hand is how these files state it.

use camino::Utf8Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

/// Send one JSON-RPC request over the daemon's socket and read the whole answer back.
///
/// `Connection: close` so the server ends the stream and `read_to_string` terminates on
/// EOF — a readiness signal from the peer rather than a timeout, which is the same reason
/// nothing in this workspace sleeps to synchronize.
///
/// The io failure is **returned**, not asserted away: "nobody is listening on that socket"
/// is the answer a client gets when no daemon is running, and a helper that panicked on it
/// would make that case untestable — which is precisely the case `read_verbs.rs` uses to
/// prove its two wires are two, and the refusal P4f's `wchc` has to render.
pub(crate) async fn call(socket: &Utf8Path, request: &str) -> std::io::Result<String> {
    let mut stream = UnixStream::connect(socket.as_std_path()).await?;
    let framed = format!(
        "POST / HTTP/1.1\r\n\
         Host: localhost\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {request}",
        request.len()
    );
    stream.write_all(framed.as_bytes()).await?;
    let mut answer = String::new();
    stream.read_to_string(&mut answer).await?;
    Ok(answer)
}

/// The JSON body of an HTTP answer.
pub(crate) fn body(answer: &str) -> &str {
    answer
        .split_once("\r\n\r\n")
        .map(|(_headers, body)| body)
        .unwrap_or_else(|| panic!("not an HTTP/1.1 answer: {answer}"))
}
