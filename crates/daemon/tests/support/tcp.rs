//! The one HTTP/1.1-over-TCP client this crate's tests use.
//!
//! Included by `http.rs` and by nothing else — the only suite whose subject is the transport
//! D11 makes opt-in. **Who includes this is load-bearing, which is why that is a sentence**
//! (note **N49**): a `#[path]`-included module is compiled into *every* binary that includes
//! it and this workspace builds warnings as errors, so an item used by only some of them is a
//! `dead_code` failure in the rest. Adding a consumer means every item here has to be used by
//! it — which is why this is a module of its own rather than three more items in
//! `support/mod.rs`, whose five includers speak JSON-RPC over `AF_UNIX` and would inherit a
//! TCP client none of them opens.
//!
//! ## Why it is hand-written
//!
//! `support/mod.rs`'s reason, one transport along: the thing under test is **what the socket
//! speaks**, and a client that abstracted the request away would assert that two libraries
//! agree rather than that this daemon serves HTTP. It also has to be able to send requests a
//! well-behaved client will not — two `Authorization` headers, a query string with the same
//! parameter twice, a token that is one digit wrong — and those are exactly the requests an
//! HTTP client library exists to stop you from making.
//!
//! Nothing here waits on a clock. `Connection: close` makes the server end the stream, so the
//! read finishes at end-of-file — a readiness signal from the peer, which is the same bound
//! `support::call` accepts and the same reason nothing in this workspace sleeps to
//! synchronize.

use std::net::SocketAddr;

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

/// Send one `GET` and read the whole answer back.
///
/// `headers` is a list rather than a map, and repeats are kept in order: a request carrying
/// two `Authorization` headers is well-formed HTTP and is one of the shapes the token gate has
/// an opinion about.
///
/// The io failure is **returned** rather than asserted away, for `support::call`'s reason:
/// "nobody is listening on that port" is the answer a client gets after the daemon has
/// stopped, and a helper that panicked on it would make the one thing this suite has to prove
/// about stopping untestable.
pub(crate) async fn get(
    bound: SocketAddr,
    target: &str,
    headers: &[(&str, &str)],
) -> std::io::Result<Answer> {
    let mut stream = TcpStream::connect(bound).await?;
    let mut framed = format!(
        "GET {target} HTTP/1.1\r\n\
         Host: {bound}\r\n\
         Connection: close\r\n"
    );
    for (name, value) in headers {
        framed.push_str(&format!("{name}: {value}\r\n"));
    }
    framed.push_str("\r\n");

    stream.write_all(framed.as_bytes()).await?;
    let mut answer = String::new();
    stream.read_to_string(&mut answer).await?;
    Ok(Answer::of(&answer))
}

/// One HTTP answer, split where the protocol splits it.
///
/// A value rather than a `String` the tests grep, because a suite about a **401** has to be
/// able to say `401` and not "contains 401" — a body that happened to carry the digits would
/// pass the second and mean nothing.
#[derive(Debug)]
pub(crate) struct Answer {
    head: String,
    body: String,
}

impl Answer {
    /// Split an answer at the blank line that ends its headers.
    fn of(answer: &str) -> Answer {
        let (head, body) = answer
            .split_once("\r\n\r\n")
            .unwrap_or_else(|| panic!("not an HTTP/1.1 answer: {answer:?}"));
        Answer {
            head: head.to_owned(),
            body: body.to_owned(),
        }
    }

    /// The status code, as a number.
    pub(crate) fn status(&self) -> u16 {
        let status_line = self.head.lines().next().unwrap_or_default();
        status_line
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or_else(|| panic!("no status code in {status_line:?}"))
    }

    /// One header's value, matched case-insensitively the way HTTP defines field names.
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.head.lines().skip(1).find_map(|line| {
            let (field, value) = line.split_once(':')?;
            field
                .trim()
                .eq_ignore_ascii_case(name)
                .then(|| value.trim())
        })
    }

    /// Everything after the headers.
    pub(crate) fn body(&self) -> &str {
        &self.body
    }
}
