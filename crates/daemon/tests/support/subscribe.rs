//! One open subscription, over each of the two transports a subscription can use.
//!
//! Included by `subscriptions.rs` and by nothing else, which is what `support/wire.rs`
//! already does for the four verb suites and for the same reason (note **N49**): a
//! `#[path]`-included module is compiled into every binary that includes it, so an item with
//! one user has to live in a module with one includer.
//!
//! ## Why the generated client is not here
//!
//! `support/wire.rs`'s `Wire` drives `api::WchRpcClient`, which is a `ClientT`. The
//! subscriptions are on a *second* generated trait whose client is a
//! `SubscriptionClientT` — and `SubscriptionClientT::subscribe` answers
//! `jsonrpsee_core::client::Subscription`, **whose only constructor is private** over two
//! private types. No transport outside `jsonrpsee-core` can implement it, which is the
//! measured fact that made the T5 surface two traits (note **N57**, `crates/api/src/wire.rs`).
//! So the two things a subscription test can actually hold are:
//!
//! - **the server's own in-memory dispatch**, `Methods::subscribe(name, params, buf_size)`,
//!   which takes the per-connection buffer as an argument — and that argument is the lever
//!   the backpressure arm pulls, because it is the same bound
//!   `limits::WS_MESSAGE_BUFFER_CAPACITY` sets on a real connection;
//! - **a real WebSocket on the daemon's own `AF_UNIX` socket**, hand-driven the way
//!   `support/mod.rs`'s HTTP client is and for the reason its header gives: the socket
//!   carries HTTP/1.1 and the subscription surface is the *upgrade* on that same
//!   connection, which is the fact P4f's client transport has to be built against rather
//!   than discover. `soketto` is jsonrpsee-server's own WebSocket implementation, so the
//!   two halves of the frame layer cannot disagree about what a frame is.
//!
//! Nothing here waits on a clock. `receive_data` ends when the peer writes, which is the
//! same readiness signal `support::call`'s read-to-EOF already relies on.

use std::collections::VecDeque;

use camino::Utf8Path;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt as _};

/// A real WebSocket connection to the daemon, and the JSON-RPC on top of it.
///
/// One connection, many requests: which is what
/// `limits::RPC_MAX_SUBSCRIPTIONS_PER_CONNECTION` is a bound *on*, so a fixture that opened
/// a connection per subscription could not drive it at all.
pub(crate) struct Ws {
    sender: soketto::Sender<Compat<tokio::net::UnixStream>>,
    receiver: soketto::Receiver<Compat<tokio::net::UnixStream>>,
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

impl Ws {
    /// Upgrade a fresh connection to `socket`.
    ///
    /// # Panics
    ///
    /// If the daemon declines the upgrade, which is the assertion `tests/uds.rs` makes
    /// directly — a suite whose subject is what a subscription *carries* has nothing useful
    /// to say after a refused handshake.
    pub(crate) async fn connect(socket: &Utf8Path) -> Ws {
        let stream = tokio::net::UnixStream::connect(socket.as_std_path())
            .await
            .expect("the daemon is listening");
        let mut client = soketto::handshake::Client::new(stream.compat(), "localhost", "/");
        match client.handshake().await.expect("the handshake completes") {
            soketto::handshake::ServerResponse::Accepted { .. } => {}
            other => panic!("the daemon declined a WebSocket upgrade: {other:?}"),
        }
        let (sender, receiver) = client.into_builder().finish();
        Ws {
            sender,
            receiver,
            next_id: 1,
            queued: VecDeque::new(),
        }
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
    /// is one of the hostile directions `subscriptions.rs` walks. A helper that read an
    /// answer could not express it: what makes that case a case is that nobody is left to
    /// read one.
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
    /// outside — see this suite's header.
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
    /// because the one thing [`Watching`] cannot express is a connection carrying **two**
    /// subscriptions, and telling those apart is exactly what a client that subscribed twice
    /// has to do. Ends when the daemon writes; a stream that never delivers is a nextest
    /// `TIMEOUT` with a test's name on it rather than a hang.
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
}

/// One open subscription, over one of the two transports.
///
/// A single enum with a single `next` for `Wire`'s reason one level up: what these tests
/// compare is two pipes carrying one surface, so the code that turns a notification into a
/// value has to be the same on both sides of the comparison.
pub(crate) enum Watching {
    /// jsonrpsee's own in-memory dispatch, with the connection buffer as an argument.
    InMemory(jsonrpsee::core::server::Subscription),
    /// A real WebSocket, with the subscription's own id so a notification can be told from
    /// somebody else's.
    ///
    /// Boxed because soketto's framing buffers make this variant an order of magnitude
    /// larger than the other, and an enum whose size is its largest arm is a value every
    /// caller pays for — clippy names it, and here it costs one allocation per open
    /// subscription rather than a suppression.
    Ws {
        connection: Box<Ws>,
        subscription: Value,
    },
}

impl Watching {
    /// The next notification, decoded as the subscription's item type.
    ///
    /// `None` when the subscription **ended** — which is the observation the lag arm and
    /// the disconnect arm both make, and the reason this is an `Option` rather than an
    /// `expect`.
    pub(crate) async fn next<T: DeserializeOwned>(&mut self) -> Option<T> {
        match self {
            Watching::InMemory(subscription) => match subscription.next::<T>().await? {
                Ok((item, _id)) => Some(item),
                Err(err) => panic!("a notification did not decode: {err}"),
            },
            Watching::Ws {
                connection,
                subscription,
            } => loop {
                let params = connection.notification().await;
                if params.get("subscription") != Some(&*subscription) {
                    // Another stream on this connection. A test that holds two of them
                    // reads the connection itself (`Ws::notification`) rather than two
                    // `Watching`s, because a helper that queued the other one would need a
                    // second queue keyed by id — which is the transport P4f owns rather
                    // than the one this suite needs.
                    continue;
                }
                // A `subscription_error` frame is how a stream ends abnormally — jsonrpsee
                // has no JSON-RPC code left to send after the accept — so a frame with no
                // `result` is the end of this iterator rather than a decode failure. The
                // in-memory dispatch reports the same end the same way
                // (`Subscription::next` answers `None` for one), which is what lets the two
                // transports share this signature.
                let result = params.get("result")?;
                return Some(
                    serde_json::from_value(result.clone()).expect("a notification decodes"),
                );
            },
        }
    }

    /// The connection this subscription rides on, for a test that asks the daemon something
    /// else on it.
    ///
    /// The whole of P4e-i's story is *nothing a client does can wedge the daemon*, and half
    /// of that claim is about the **same** connection: a client whose subscription was
    /// refused, or whose stream just ended, must still be able to call a verb on the socket
    /// it already has. That question cannot be asked of the in-memory arm, which has no
    /// connection to share — so it panics rather than inventing an answer, and every caller
    /// of it is a test about a socket.
    ///
    /// # Panics
    ///
    /// On [`Watching::InMemory`], for the reason above.
    pub(crate) fn connection(&mut self) -> &mut Ws {
        match self {
            Watching::Ws { connection, .. } => connection,
            Watching::InMemory(_) => {
                panic!("the in-memory dispatch has no connection to ask a second question on")
            }
        }
    }
}
