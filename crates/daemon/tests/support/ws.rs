//! One real WebSocket connection to the daemon, and the JSON-RPC on top of it.
//!
//! Included by `subscriptions.rs`, which drives the two subscriptions an in-process daemon
//! registers, by `signals.rs`, which watches one across a **real signal** to a real `wchd`
//! (docs/7 P4e-ii), since P5b by `web_rpc.rs`, which opens the same JSON-RPC over the TCP
//! listener's WebSocket route, and since P5c by `web_client.rs`, which opens the socket the
//! shipped page opens — and a *second* one on the Unix socket, because the page's calibration
//! view watches sweeps another client started. Four includers rather than one, which is why
//! this is a module of its own and `support/subscribe.rs` — [`crate::subscribe::Watching`],
//! whose in-memory arm only a suite with a `Methods` value can construct — is still next door:
//! a `#[path]`-included module is compiled into every binary that includes it, so an item with
//! one user has to live in a module with one includer, down to an enum variant nobody
//! constructs (note **N49**).
//!
//! That rule is what shaped `web_client.rs`'s test list as much as its subject did, and it is
//! worth recording rather than leaving as a coincidence: every item here has a user there
//! because each one turned out to be a claim that suite genuinely owed. [`Ws::connect`] is the
//! agent on the Unix socket; [`Ws::write`] and [`Ws::answer`] are the sweep that has to be in
//! flight while the page reads its events; [`Ws::notification`] is the calibration view; and
//! [`Ws::ending`] is the *hotplug* stream, which ends differently from the calibration one
//! `web_rpc.rs` already covers.
//!
//! ## Why it is generic over the byte stream, and why that is one constructor and not two
//!
//! The daemon serves the same JSON-RPC over `AF_UNIX` and over TCP, and the *whole* of the
//! difference is which stream the frames are on — which is the claim `web_rpc.rs` exists to
//! make, so a second frame reader beside this one would make it a comparison between two test
//! clients. [`Ws::upgrade`] therefore takes any tokio stream; [`Ws::connect`] is the Unix
//! convenience two of the three includers use, and it is written in terms of `upgrade` rather
//! than beside it.
//!
//! A `connect_tcp` sibling would be an item with one includer, which note N49 says is a
//! `dead_code` failure in the other two. So [`Ws::upgrade`] is what a TCP suite calls
//! directly, and it answers a `Result` rather than panicking, because a *refused* upgrade is
//! something only the gated transport can produce and is one of the things that suite is for.
//!
//! ## Why it is hand-written
//!
//! `support/wire.rs`'s `Wire` drives `api::WchRpcClient`, which is a `ClientT`. The
//! subscriptions are on a *second* generated trait whose client is a `SubscriptionClientT` —
//! and `SubscriptionClientT::subscribe` answers `jsonrpsee_core::client::Subscription`,
//! **whose only constructor is private** over two private types. No transport outside
//! `jsonrpsee-core` can implement it, which is the measured fact that made the T5 surface two
//! traits (note **N57**, `crates/api/src/wire.rs`). So a real subscription is reached the way
//! `support/mod.rs`'s HTTP client reaches a call, and for the reason its header gives: the
//! socket carries HTTP/1.1 and the subscription surface is the *upgrade* on that same
//! connection, which is the fact P4f's client transport has to be built against rather than
//! discover. `soketto` is jsonrpsee-server's own WebSocket implementation, so the two halves
//! of the frame layer cannot disagree about what a frame is.
//!
//! Nothing here waits on a clock. `receive_data` ends when the peer writes, which is the same
//! readiness signal `support::call`'s read-to-EOF already relies on.

use std::collections::VecDeque;

use camino::Utf8Path;
use serde_json::{Value, json};
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt as _};

/// A real WebSocket connection to the daemon, and the JSON-RPC on top of it.
///
/// One connection, many requests: which is what
/// `limits::RPC_MAX_SUBSCRIPTIONS_PER_CONNECTION` is a bound *on*, so a fixture that opened
/// a connection per subscription could not drive it at all.
pub(crate) struct Ws<S> {
    sender: soketto::Sender<Compat<S>>,
    receiver: soketto::Receiver<Compat<S>>,
    /// The next request id. Ids are matched on the way back rather than assumed, because a
    /// notification can arrive between a request and its answer — which is the whole point
    /// of a duplex transport and the thing an HTTP client never has to think about.
    next_id: u32,
    /// Notifications that arrived while a call was waiting for its answer.
    ///
    /// **Queued, not discarded**, and that is the difference between a suite that tests the
    /// daemon and one that tests the scheduler: on a duplex connection an answer and a
    /// notification are in flight together, so a helper that dropped whichever lost the race
    /// would make a delivered event look like an undelivered one — the test would hang, and
    /// it would hang for a reason on this side of the socket. [`Ws::notification`] drains
    /// this before it reads another frame.
    ///
    /// Bounded by what one test asks for and nothing else, which is why it is a test-side
    /// `VecDeque` rather than one of `schema::limits`' numbers: the *daemon's* bound on
    /// unread notifications is `limits::WS_MESSAGE_BUFFER_CAPACITY`, and it is measured
    /// elsewhere in this suite by a subscriber that never reads at all.
    queued: VecDeque<Value>,
}

impl Ws<tokio::net::UnixStream> {
    /// Upgrade a fresh connection to the daemon's Unix socket, at the root path.
    ///
    /// `/` and not [`daemon::http::RPC_PATH`], deliberately: the Unix transport has **no
    /// routing at all** — jsonrpsee's service is the whole of what answers there, and the
    /// request target it is handed is ignored, which is one of the differences `web_rpc.rs`
    /// establishes is *not* a difference in what the JSON-RPC means.
    ///
    /// # Panics
    ///
    /// If the daemon declines the upgrade, which is the assertion `tests/uds.rs` makes
    /// directly — a suite whose subject is what a subscription *carries* has nothing useful
    /// to say after a refused handshake. The transport with a gate in front of it is TCP's,
    /// and that suite calls [`Ws::upgrade`] and reads the refusal.
    pub(crate) async fn connect(socket: &Utf8Path) -> Ws<tokio::net::UnixStream> {
        let stream = tokio::net::UnixStream::connect(socket.as_std_path())
            .await
            .expect("the daemon is listening");
        Ws::upgrade(stream, "localhost", "/")
            .await
            .unwrap_or_else(|status| panic!("the daemon declined a WebSocket upgrade: {status}"))
    }
}

impl<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin> Ws<S> {
    /// Perform the handshake on `stream`, or answer the status the daemon refused with.
    ///
    /// `host` is what goes in the `Host` header and `target` is the request line's path —
    /// including its query string, which is how a browser's `new WebSocket(url)` presents a
    /// credential and therefore how the gated transport is opened at all (note **N74**,
    /// `daemon::http::rpc`).
    ///
    /// The refusal is a **status and not a panic** because a declined upgrade is a real answer
    /// on the transport D11 gates: an anonymous socket and one carrying a near-miss token both
    /// end here, and both are things `web_rpc.rs` asserts rather than survives.
    pub(crate) async fn upgrade(stream: S, host: &str, target: &str) -> Result<Ws<S>, u16> {
        let mut client = soketto::handshake::Client::new(stream.compat(), host, target);
        match client.handshake().await.expect("the handshake completes") {
            soketto::handshake::ServerResponse::Accepted { .. } => {}
            soketto::handshake::ServerResponse::Rejected { status_code } => {
                return Err(status_code);
            }
            // Nothing in this daemon redirects, and a redirect that appeared would be a
            // routing decision nobody made — named rather than folded into the refusal above,
            // so it would fail as itself.
            other => panic!("the daemon answered a WebSocket upgrade with {other:?}"),
        }
        let (sender, receiver) = client.into_builder().finish();
        Ok(Ws {
            sender,
            receiver,
            next_id: 1,
            queued: VecDeque::new(),
        })
    }

    /// One JSON-RPC frame, whatever it is.
    ///
    /// Ends when the server writes. A subscription that never delivers turns this into a
    /// nextest `TIMEOUT` — a named failure with a test's name on it — rather than a hang,
    /// which is what `.config/nextest.toml`'s deadline exists to give.
    async fn frame(&mut self) -> Value {
        let mut bytes = Vec::new();
        self.receiver
            .receive_data(&mut bytes)
            .await
            .expect("the daemon writes a frame");
        serde_json::from_slice(&bytes).expect("a JSON-RPC document")
    }

    /// Send one request and read frames until its answer arrives.
    ///
    /// Whatever else arrives first is put on [`Ws::queued`] rather than thrown away — see
    /// that field for why, which is the one thing about this transport an HTTP client never
    /// has to think about.
    pub(crate) async fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        self.write(&request.to_string()).await;
        loop {
            let frame = self.frame().await;
            if frame.get("id") == Some(&json!(id)) {
                return frame;
            }
            self.queued.push_back(frame);
        }
    }

    /// Put one message on the wire and read nothing back.
    ///
    /// [`Ws::call`]'s other half, and the only way a test can hand the daemon something no
    /// answer will ever be collected for — a request the client dies in the middle of, which
    /// is one of the hostile directions `subscriptions.rs` walks — or one whose answer is
    /// collected much later, which is how `signals.rs` holds a sweep in flight across a
    /// signal. A helper that read an answer could not express either: what makes the first a
    /// case is that nobody is left to read one, and what makes the second one is that the
    /// client has other things to do first.
    pub(crate) async fn write(&mut self, message: &str) {
        self.sender
            .send_text(message)
            .await
            .expect("the connection takes a message");
        self.sender.flush().await.expect("the message is written");
    }

    /// The next *answer* on this connection, whichever request it belongs to.
    ///
    /// [`Ws::call`]'s shape for a client with many requests in flight and no interest in
    /// which one comes back — the only shape in which "how many of these were answered
    /// while the device was held" is a question at all, because matching ids would mean
    /// naming a request that may be the one still waiting. Notifications are queued rather
    /// than discarded, for [`Ws::queued`]'s reason.
    ///
    /// Ends when the daemon writes. A daemon that answered none of them is a nextest
    /// `TIMEOUT` with a test's name on it, which is exactly what a wedge looks like from
    /// outside — see `subscriptions.rs`'s header.
    pub(crate) async fn answer(&mut self) -> Value {
        loop {
            let queued = self
                .queued
                .iter()
                .position(|frame| frame.get("id").is_some())
                .and_then(|position| self.queued.remove(position));
            if let Some(answered) = queued {
                return answered;
            }
            let frame = self.frame().await;
            if frame.get("id").is_some() {
                return frame;
            }
            self.queued.push_back(frame);
        }
    }

    /// The next notification on this connection, whichever subscription it belongs to.
    ///
    /// Answers to calls are skipped rather than treated as the end of anything: on a duplex
    /// connection the two are interleaved by construction, which is the whole reason
    /// [`Ws::queued`] exists.
    ///
    /// It hands back the notification's `params` — the subscription id *and* the payload —
    /// because the one thing [`crate::subscribe::Watching`] cannot express is a connection
    /// carrying **two** subscriptions, and telling those apart is exactly what a client that
    /// subscribed twice has to do. Ends when the daemon writes; a stream that never delivers
    /// is a nextest `TIMEOUT` with a test's name on it rather than a hang.
    pub(crate) async fn notification(&mut self) -> Value {
        loop {
            let frame = match self.queued.pop_front() {
                Some(queued) => queued,
                None => self.frame().await,
            };
            if let Some(params) = frame.get("params") {
                return params.clone();
            }
        }
    }

    /// Read until `subscription`'s stream **ends**, and answer the payload that ended it.
    ///
    /// "A stream ended" is not the claim any caller of this wants to make; "*this* stream
    /// ended, and here is the reason a client would branch on" is — `daemon::events` sends
    /// [`daemon::events::WATCH_STOPPED`] when a source failed and
    /// [`daemon::events::SHUTTING_DOWN`] when the process is going away, and "re-subscribe
    /// now" and "this daemon is going away" are different advice. The payload arrives as
    /// jsonrpsee's `subscription_error` notification, which is the only carrier left after the
    /// accept, so this reads `params.error` and hands it back whole rather than reducing it to
    /// a `bool`.
    ///
    /// Frames that belong to something else are **queued**, for [`Ws::queued`]'s reason: a
    /// connection carrying two subscriptions ends both, and a helper that dropped whichever
    /// came second would make the second call hang. Frames this subscription *delivered*
    /// before its end are dropped, which is the one thing this helper discards and is why it
    /// is not spelled "the next frame must be the end": a producer that emitted while the
    /// teardown was running did nothing wrong, and a caller that wanted those events would
    /// have read them.
    ///
    /// Ends when the daemon writes. A daemon that ended a stream by closing the socket instead
    /// — which is exactly what step 3 of `daemon::shutdown`'s order exists to prevent — fails
    /// here on the read rather than answering, and one that left the stream open is a nextest
    /// `TIMEOUT` with a test's name on it.
    pub(crate) async fn ending(&mut self, subscription: &Value) -> Value {
        loop {
            let queued = self
                .queued
                .iter()
                .position(|frame| notifies(frame, subscription))
                .and_then(|position| self.queued.remove(position));
            let params = match queued {
                Some(frame) => frame["params"].clone(),
                None => {
                    let frame = self.frame().await;
                    if !notifies(&frame, subscription) {
                        self.queued.push_back(frame);
                        continue;
                    }
                    frame["params"].clone()
                }
            };
            if let Some(reason) = params.get("error") {
                return reason.clone();
            }
        }
    }
}

/// Whether `frame` is a notification on `subscription`.
fn notifies(frame: &Value, subscription: &Value) -> bool {
    frame
        .get("params")
        .and_then(|params| params.get("subscription"))
        == Some(subscription)
}
