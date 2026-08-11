//! The client transport: one Unix socket, upgraded, wearing jsonrpsee's two transport traits.
//!
//! Design §2.6 asks for "a small client transport (~200 lines, modeled on reth's production
//! IPC adapter — vendored knowledge, not a git dependency)". **The model is the two trait
//! impls, not the framing**, and the difference matters: reth's IPC is newline-framed raw
//! JSON-RPC on a `UnixStream`, and `wchd` speaks **HTTP/1.1** on its socket because the
//! service jsonrpsee hands the daemon is an HTTP service (`daemon::uds`'s header states it,
//! and `crates/daemon/tests/support/mod.rs` writes the `POST` by hand so the fact is in the
//! repository rather than in a memory). So the shape here is:
//!
//! ```text
//! UnixStream::connect → soketto handshake → into_builder().finish()
//!                     → TransportSenderT / TransportReceiverT
//!                     → ClientBuilder::build_with_tokio
//! ```
//!
//! ## Why the upgrade rather than the `POST`
//!
//! A `POST /` carries calls and **only** calls: jsonrpsee's HTTP path builds
//! `RpcServiceCfg::OnlyCalls`, so a `wch_subscribe_*` sent that way is answered `-32603` —
//! not even a D13 code (note **N57**). Calls and subscriptions really are two capabilities
//! over this one socket, and `calibrate sweep` needs both at once: the sweep is a call whose
//! progress arrives on a subscription while it is still in flight. The WebSocket upgrade is
//! the one connection that carries both, which is why the client this feeds is
//! `jsonrpsee_core::client::Client` — the single value implementing `ClientT` *and*
//! `SubscriptionClientT`.
//!
//! `soketto` is jsonrpsee-server's own WebSocket implementation, so the two halves of the
//! frame layer cannot disagree about what a frame is. It is a normal dependency of `wchc`
//! where it is a dev-dependency of the daemon, and that asymmetry is the point: for the
//! daemon it is a fixture, for this binary it is the product.
//!
//! ## What is bounded here
//!
//! soketto's own default maximum message is 256 MB, which is somebody else's number for
//! somebody else's peer. The bound this connection takes is the one the *daemon* already
//! promises — [`limits::RPC_MAX_RESPONSE_BYTES`], the largest body `daemon::uds::serve`
//! will write — so the client accepts exactly what the server may send and not a byte more
//! (AGENTS, "bounded everything"; that constant gains its second reader here).
//!
//! ## What a failure is called
//!
//! Every refusal this module produces is [`Error::StorageIo`] naming the socket path it
//! tried, and [`connect`]'s own doc argues why that is the D13 variant rather than a
//! nineteenth one.

use camino::Utf8Path;
use jsonrpsee::core::client::{ReceivedMessage, TransportReceiverT, TransportSenderT};
use schema::error::{Error, Result};
use schema::limits;
use soketto::connection::Error as FrameError;
use soketto::data::{ByteSlice125, Data, Incoming};
use tokio::net::UnixStream;
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt as _};

/// The byte stream under the frames: a `tokio` socket wearing `futures::io`'s clothes.
///
/// The adapter is `tokio-util`'s `compat`, and it is the whole of that dependency: soketto
/// takes the `futures::io` traits and `tokio::net::UnixStream` speaks tokio's.
type Socket = Compat<UnixStream>;

/// The `Host` header the upgrade request carries.
///
/// A Unix socket has no host, and HTTP/1.1 requires the header anyway. `localhost` is what
/// the daemon's own subscription fixture sends (`crates/daemon/tests/support/ws.rs`), so the
/// request this client makes is the one that surface is already tested against.
const NO_HOST: &str = "localhost";

/// The resource the upgrade names.
///
/// The daemon mounts one service at the root; there is no second path to reach.
const ROOT: &str = "/";

/// The write half: JSON-RPC text frames out.
#[derive(Debug)]
pub struct Sender(soketto::Sender<Socket>);

/// The read half: whatever the daemon writes, classified.
#[derive(Debug)]
pub struct Receiver {
    frames: soketto::Receiver<Socket>,
    /// The buffer soketto reassembles a message into.
    ///
    /// Owned by the receiver rather than allocated per frame, because a subscription
    /// delivers thousands of these over one sweep and each one would otherwise be a fresh
    /// allocation. It is cleared before every read: soketto *appends*, and a buffer carried
    /// over would hand the next message the previous one's bytes in front of it.
    message: Vec<u8>,
}

/// Connect to `socket` and upgrade the connection to WebSocket.
///
/// # Errors
///
/// [`Error::StorageIo`] naming `socket`, for every way this can fail: nothing is listening
/// there, the directory is not the one this process expected, or something is listening that
/// declined the upgrade.
///
/// **That variant is chosen, not defaulted.** The D13 registry is closed and P4f adds
/// nothing to it — a nineteenth variant would move `D13_CODES`, the OpenRPC document and two
/// committed artifacts for a failure that never crosses the wire, because by construction
/// there is no wire when it happens. Of the eighteen, `StorageIo` is the only one that
/// carries a **path** as a first-class field, and the path is the whole of what a reader
/// needs: "no daemon is running" and "your `$XDG_RUNTIME_DIR` is not what you think" are
/// different problems with the same symptom, and only the path tells them apart. It is also
/// already the answer to the *sibling* question — [`schema::paths::runtime_dir`] refuses an
/// unset `$XDG_RUNTIME_DIR` with `StorageIo` — so the two halves of "where is the socket"
/// answer with one kind rather than two.
///
/// The rejected alternative was `DeviceIo`, which also carries an `errno`: it is the
/// registry's "the kernel refused an operation" variant, and `connect(2)` is a kernel
/// operation. It loses on the field that matters — its `operation` is a syscall name, not a
/// path — and on E3: `DeviceIo` is what a *camera* said, and a missing daemon is not a
/// statement about any camera.
pub async fn connect(socket: &Utf8Path) -> Result<(Sender, Receiver)> {
    let stream = UnixStream::connect(socket.as_std_path())
        .await
        .map_err(|error| unreachable(socket, error.raw_os_error(), &error.to_string()))?;

    let mut handshake = soketto::handshake::Client::new(stream.compat(), NO_HOST, ROOT);
    match handshake.handshake().await {
        Ok(soketto::handshake::ServerResponse::Accepted { .. }) => {}
        Ok(other) => {
            return Err(unreachable(
                socket,
                None,
                &format!(
                    "something is listening there, but it declined a WebSocket upgrade \
                     ({other:?}); that socket is not a wchd this build can talk to"
                ),
            ));
        }
        Err(error) => return Err(unreachable(socket, None, &error.to_string())),
    }

    let mut builder = handshake.into_builder();
    // The daemon's own response ceiling, read from the one place it is set. A client that
    // accepted more would be buffering what the server cannot send; one that accepted less
    // would refuse a 4K photo the daemon delivered perfectly well.
    builder.set_max_message_size(usize::try_from(limits::RPC_MAX_RESPONSE_BYTES).unwrap_or(
        // A 32-bit host, where the response ceiling does not fit a `usize`. Taking the
        // largest bound the platform can express is the honest reading: the number exists
        // to keep a peer from making this process allocate without limit, and `usize::MAX`
        // is the limit that platform has.
        usize::MAX,
    ));
    let (sender, frames) = builder.finish();
    Ok((
        Sender(sender),
        Receiver {
            frames,
            message: Vec::new(),
        },
    ))
}

/// The refusal for a socket this process could not talk JSON-RPC over.
///
/// One constructor so every arm of [`connect`] names the path the same way — see that
/// function's doc for why this variant.
fn unreachable(socket: &Utf8Path, errno: Option<i32>, message: &str) -> Error {
    Error::StorageIo {
        path: socket.to_owned(),
        errno,
        message: format!("cannot reach the daemon on this socket: {message}"),
    }
}

impl TransportSenderT for Sender {
    type Error = FrameError;

    async fn send(&mut self, msg: String) -> std::result::Result<(), FrameError> {
        // `send_text_owned` rather than `send_text`: the string is ours already and the
        // owned form is one copy fewer. The flush is not optional — soketto buffers, and a
        // request left in the buffer is a call the daemon never sees.
        self.0.send_text_owned(msg).await?;
        self.0.flush().await
    }

    async fn send_ping(&mut self) -> std::result::Result<(), FrameError> {
        // Implemented rather than defaulted, even though this build never enables pings
        // (`ClientBuilder`'s default is off and `Remote::connect` leaves it there). The
        // default implementation answers `Ok(())` without writing anything, and a transport
        // that reports success for a frame it did not send is a false claim that happens to
        // be harmless — which is still a defect (AGENTS, on `// SAFETY:` and on claims
        // generally). If a later build turns pings on, this one already sends them.
        match ByteSlice125::try_from(EMPTY_PING) {
            Ok(payload) => {
                self.0.send_ping(payload).await?;
                self.0.flush().await
            }
            // Unreachable: `ByteSlice125` refuses a payload over 125 bytes and this one is
            // empty. The arm exists so this function has no `expect` in it, and it refuses
            // rather than claiming the ping went out.
            Err(_) => Err(FrameError::UnexpectedOpCode(soketto::base::OpCode::Ping)),
        }
    }

    async fn close(&mut self) -> std::result::Result<(), FrameError> {
        // A real close frame, so the daemon's connection task ends on a peer that said
        // goodbye rather than on a read error. `wchc` runs one verb and exits, so this is
        // the ordinary end of every connection this binary opens.
        self.0.close().await
    }
}

/// The payload of a ping: none.
///
/// A ping's payload is echoed in the pong and this transport has nothing to correlate —
/// jsonrpsee's inactivity check counts pongs rather than matching them.
const EMPTY_PING: &[u8] = &[];

impl TransportReceiverT for Receiver {
    type Error = FrameError;

    async fn receive(&mut self) -> std::result::Result<ReceivedMessage, FrameError> {
        // Cleared rather than replaced: soketto appends to whatever is in the buffer, so a
        // carried-over message would arrive with the previous one glued to its front.
        self.message.clear();
        // One frame, one answer: soketto's `receive` already loops over continuation and
        // control frames internally, and every case below is terminal for this call.
        // `ReceivedMessage::Pong` is handed *up* rather than swallowed here, because
        // whether a pong is interesting is the client's question and not the transport's.
        match self.frames.receive(&mut self.message).await? {
            Incoming::Data(Data::Text(_)) => {
                let bytes = std::mem::take(&mut self.message);
                // A JSON-RPC document is UTF-8 by definition. A text frame that is not is a
                // protocol failure and is refused as one: decoding it lossily would hand
                // the client a document nobody wrote.
                String::from_utf8(bytes)
                    .map(ReceivedMessage::Text)
                    .map_err(|error| FrameError::Utf8(error.utf8_error()))
            }
            Incoming::Data(Data::Binary(_)) => {
                Ok(ReceivedMessage::Bytes(std::mem::take(&mut self.message)))
            }
            Incoming::Pong(_) => Ok(ReceivedMessage::Pong),
            // The peer said goodbye. Reported as the transport failure it is, so the
            // client's background task ends and every in-flight call is refused — rather
            // than as an empty message, which would leave a caller waiting for an answer
            // that is never coming.
            Incoming::Closed(_) => Err(FrameError::Closed),
        }
    }
}
