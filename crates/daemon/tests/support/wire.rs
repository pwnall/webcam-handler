//! The two transports docs/7 P4b names, as one thing the generated T5 client can drive.
//!
//! Shared by every suite that asks the daemon a question — four binaries: `read_verbs.rs`,
//! `mutating_verbs.rs`, `calibrate_verbs.rs` and `method_surface.rs` — for
//! `support/mod.rs`'s reason, one level up: a second `ClientT` would be a second encoder, and
//! the comparisons these suites make ("both wires answered the same") would then be comparing
//! two clients rather than two pipes. `support/mod.rs` states what adding a fifth costs.
//!
//! The split between this file and `support/mod.rs` is the one their headers already imply:
//! `support/mod.rs` is *what the daemon speaks* (HTTP/1.1 on a `UnixStream`), and this is
//! *what the suite asks it*. Included by path rather than as a submodule of `support` so
//! that `uds.rs`, which asserts the transport and drives no verbs, does not compile items it
//! never uses — which under `-D warnings` is a build failure and not a matter of taste.

use std::fmt;
use std::future::Future;

use api::codes::{self, ErrorObjectOwned};
use camino::Utf8PathBuf;
use jsonrpsee::core::DeserializeOwned;
use jsonrpsee::core::client::{BatchResponse, ClientT, Error as ClientError};
use jsonrpsee::core::params::BatchRequestBuilder;
use jsonrpsee::core::traits::ToRpcParams;
use jsonrpsee_server::Methods;
use schema::error::Error;
use serde_json::value::RawValue;

use crate::support;

/// One of the two transports docs/7 P4b names, as something the generated client can drive.
///
/// A single enum with a single [`ClientT`] implementation, rather than two clients: what
/// the suites compare is two pipes carrying one surface, so the code that turns a method
/// call into bytes has to be the same on both sides of the comparison or the comparison is
/// between two encoders.
#[derive(Debug, Clone)]
pub(crate) enum Wire {
    /// jsonrpsee's own in-memory dispatch — design §2.9's double for the RPC transport.
    ///
    /// It drives the real parsing, the real `param_kind = map` decoding, the real handler
    /// and the real `WireError → -320xx` conversion; everything except the socket. Its
    /// fault menu in §2.9 has exactly one entry ("disconnect mid-subscription"), which is
    /// P4e-i's subject and `tests/subscriptions.rs`'s, so this is a *speed* double and not
    /// a fault double — and nothing here pretends otherwise.
    InMemory(Methods),
    /// A real `AF_UNIX` socket, spoken to by a real HTTP/1.1 client (`support`).
    Uds(Utf8PathBuf),
}

impl Wire {
    /// One request in, the JSON-RPC response document out.
    async fn round_trip(&self, request: String) -> Result<String, ClientError> {
        match self {
            Wire::InMemory(methods) => {
                // The subscription buffer, which nothing this enum carries can use: the
                // two subscriptions live on `WchEvents`, whose generated client is bounded
                // by `SubscriptionClientT` and therefore reachable by no transport of ours
                // (note **N57**). `tests/subscriptions.rs` drives them through
                // `Methods::subscribe` and a real WebSocket instead.
                let (answer, _subscriptions) = methods.raw_json_request(&request, 1).await?;
                Ok(answer.to_string())
            }
            Wire::Uds(socket) => {
                // A socket nobody is listening on is a *transport* failure, kept distinct
                // from a refusal the daemon sent (E3, at the transport): `refusal` below
                // panics if the two are ever confused, and the suites that prove their two
                // wires are two depend on this arm being reachable.
                let answer = support::call(socket, &request)
                    .await
                    .map_err(|err| ClientError::Transport(Box::new(err)))?;
                Ok(support::body(&answer).to_owned())
            }
        }
    }
}

impl ClientT for Wire {
    fn request<R, Params>(
        &self,
        method: &str,
        params: Params,
    ) -> impl Future<Output = Result<R, ClientError>> + Send
    where
        R: DeserializeOwned,
        Params: ToRpcParams + Send,
    {
        // `params` is whatever the generated client built — an `ObjectParams` for every
        // method on this surface, because D10 makes every parameter named. It is
        // serialized here and never inspected: a transport that understood the surface
        // would be a second client.
        let request = envelope(method, params);
        async move {
            let answer = self.round_trip(request?).await?;
            decode(&answer)
        }
    }

    async fn notification<Params>(&self, _method: &str, _params: Params) -> Result<(), ClientError>
    where
        Params: ToRpcParams + Send,
    {
        // Refused rather than implemented, because T5 declares no notification: every
        // method answers, and a fire-and-forget call to one of them would be a request
        // whose refusal nobody could see. A transport double that silently accepted one
        // would be a capability this daemon does not have.
        Err(ClientError::Custom(
            "the T5 surface has no notifications".to_owned(),
        ))
    }

    async fn batch_request<'a, R>(
        &self,
        _batch: BatchRequestBuilder<'a>,
    ) -> Result<BatchResponse<'a, R>, ClientError>
    where
        R: DeserializeOwned + fmt::Debug + 'a,
    {
        // Batching is a transport property, not a surface one: the bound the daemon serves
        // under is `schema::limits::RPC_MAX_BATCH` and it is asserted where the transport
        // is, in `uds.rs`. Nothing this project ships batches — `wch` and `wchc` run one
        // verb per invocation — so a batching client here would be a client with no
        // product behind it.
        Err(ClientError::Custom(
            "these suites send one call at a time".to_owned(),
        ))
    }
}

/// The request id every call in these suites uses.
///
/// One call per connection, and the daemon echoes what it was sent, so the value only has
/// to be something [`decode`] can recognise the answer by — which it does not need to,
/// because there is never a second answer in flight.
const REQUEST_ID: u32 = 1;

/// Wrap a method name and the generated client's parameters in a JSON-RPC request.
fn envelope<Params: ToRpcParams>(method: &str, params: Params) -> Result<String, ClientError> {
    let params = params.to_rpc_params()?;
    // `None` for a method with no parameters at all (`wch_list`), and the key is left out
    // rather than sent as `null` — which `crates/api`'s own registration test measured to
    // be the shape a by-name server accepts.
    let request = match params {
        Some(params) => serde_json::json!({
            "jsonrpc": "2.0", "id": REQUEST_ID, "method": method, "params": params,
        }),
        None => serde_json::json!({ "jsonrpc": "2.0", "id": REQUEST_ID, "method": method }),
    };
    Ok(request.to_string())
}

/// Turn a JSON-RPC response document into the method's answer, or into a client error.
///
/// Parsed from the **text**, not through `serde_json::Value`: an error object carries its
/// `data` as a `RawValue`, which only serde_json's own text deserializer can capture, and
/// that payload is the whole D13 error `api::codes::typed` reconstructs.
fn decode<R: DeserializeOwned>(answer: &str) -> Result<R, ClientError> {
    let document: std::collections::BTreeMap<String, Box<RawValue>> = serde_json::from_str(answer)?;
    if let Some(error) = document.get("error") {
        return Err(ClientError::Call(serde_json::from_str::<ErrorObjectOwned>(
            error.get(),
        )?));
    }
    match document.get("result") {
        Some(result) => Ok(serde_json::from_str(result.get())?),
        None => Err(ClientError::Custom(format!(
            "a JSON-RPC response with neither a result nor an error: {answer}"
        ))),
    }
}

/// Recover the D13 error a refusal carried, the way `wchc` will.
///
/// The code is returned beside the error rather than checked here, so the caller asserts it
/// against the kind it expected: `codes::typed` already refuses an object whose code and
/// payload disagree, and re-checking that inside this helper would be a branch no input can
/// reach.
pub(crate) fn refusal<T: fmt::Debug>(answer: Result<T, ClientError>) -> (i32, Error) {
    match answer {
        Ok(answered) => panic!("expected a refusal, got {answered:?}"),
        Err(ClientError::Call(object)) => {
            let code = object.code();
            let error = codes::typed(&object).unwrap_or_else(|| {
                panic!("a refusal that is not a D13 error crossed the wire: {object:?}")
            });
            (code, error)
        }
        // The distinction E3 keeps everywhere else, at the transport: a camera's answer
        // must never arrive looking like a broken pipe, or the reverse.
        Err(other) => panic!("a typed refusal arrived as a transport failure: {other}"),
    }
}
