//! One open subscription, over each of the two transports a subscription can use.
//!
//! Included by `subscriptions.rs` and by nothing else, which is what `support/wire.rs` already
//! does for the four verb suites and for the same reason (note **N49**): a `#[path]`-included
//! module is compiled into every binary that includes it, so an item with one user has to live
//! in a module with one includer — and that is *per item*, down to an enum variant nobody
//! constructs. [`Watching::InMemory`] is such a variant: only a suite holding a
//! `jsonrpsee_server::Methods` can build one, which is why the real-socket half of this
//! arrangement lives one file along in `support/ws.rs`, where `signals.rs` can include it
//! without inheriting a variant it cannot construct.
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
//! - **a real WebSocket on the daemon's own `AF_UNIX` socket** — [`crate::ws::Ws`], whose
//!   header states why it is hand-written.
//!
//! Nothing here waits on a clock. Both arms end when the daemon writes.

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::ws::Ws;

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
        connection: Box<Ws<tokio::net::UnixStream>>,
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
    pub(crate) fn connection(&mut self) -> &mut Ws<tokio::net::UnixStream> {
        match self {
            Watching::Ws { connection, .. } => connection,
            Watching::InMemory(_) => {
                panic!("the in-memory dispatch has no connection to ask a second question on")
            }
        }
    }
}
