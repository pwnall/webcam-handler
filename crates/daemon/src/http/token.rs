//! The per-run bearer token — the whole of the TCP transport's auth model (D11, docs/7 P5a).
//!
//! D11 gives the daemon's two transports two different answers to one question. The Unix
//! socket's is the filesystem: a `0700` directory, asserted rather than assumed
//! ([`crate::uds`]). TCP has no such thing to lean on — "loopback alone is not an auth
//! boundary on a multi-user machine" — so its answer is a secret this process mints at
//! startup and prints once, as a URL an operator can open.
//!
//! That makes this module's subject a secret **this process holds in memory**, which is a
//! different kind of thing from everything else in this crate, and three of its decisions
//! follow from that rather than from what a token is for.
//!
//! ## Where the bytes come from
//!
//! [`rustix::rand::getrandom`], which is `getrandom(2)`, which is the kernel's CSPRNG. Not a
//! userspace generator seeded once at startup, and not a hash of a pid and a clock: the
//! whole value of the token is that nobody can guess it, and the kernel is the one source on
//! this machine whose job that is.
//!
//! **`rustix` rather than a randomness crate**, and that is note **N39**'s argument reused
//! rather than a new one: this crate is `#![forbid(unsafe_code)]` and stays that way, rustix
//! is a safe wrapper over the syscall, and the crate is already a dependency here for the
//! `*at` family the socket directory needs. What this module adds to the manifest is the
//! `rand` **feature** of a pin the workspace already carries — and rustix declares it as
//! `rand = []`, so it enables no optional dependency and adds no package to the graph. A
//! crate adopted to spell one syscall that a dependency already spells would be a second
//! answer to "how does this workspace reach the kernel" (design §2.10, one home per law).
//!
//! **No flags.** Not `GRND_NONBLOCK`: its failure mode is `EAGAIN` on a pool that has not
//! been initialized yet, and the only sensible thing to do about that is wait, so asking for
//! it would move the waiting into this module and give it a second way to fail. Not
//! `GRND_RANDOM`: the blocking entropy pool is the wrong tool for a 32-byte key and Linux's
//! own documentation says so. The residual is stated rather than hidden — the one wait this
//! costs is the first initialization of the pool after a boot, which happens once, before
//! any user session exists, and long before anybody starts a webcam daemon. The alternative
//! trade is a token minted from an uninitialized pool, which is the one outcome that would
//! make every other sentence in this module worthless.
//!
//! ## Rendered as hex, by hand
//!
//! Lowercase hex, `hex_digit` and six lines around it. Two reasons it is not base64.
//! Design §2.8 puts `base64` in `webcam-handler-api` and says **here only**; widening a
//! dependency's home to save four lines is the kind of edit that entry exists to refuse. And
//! the token rides a query string (see [`Token::ready_to_open_url`]), where base64's alphabet
//! needs a URL-safe variant and a decision about padding, while hex needs neither and cannot
//! be corrupted by anything between here and a browser's address bar.
//!
//! ## It must not be printable by accident
//!
//! [`crate::logging`]'s doctrine, applied to a secret instead of to a photograph. That
//! module's rule is that a value which must never reach a log line does not get a derived
//! `Debug` — `engine::photo::Photograph` and `cli_core`'s twin are the standing instances,
//! and this is the same shape about a different kind of harm: a `tracing::debug!(?token)` or
//! an `.expect(&format!("{token:?}"))` added three sub-milestones from now would print the
//! key to the camera into the journal, and no lint can go red on a line like that.
//!
//! So [`Token`] has a hand-written `Debug` that redacts, **no `Display`**, and exactly one
//! rendering that yields the secret: [`Token::expose_secret`], whose name is the warning.
//! The workspace lints `missing_debug_implementations` at warn and `just ci` runs clippy
//! `-D warnings`, so the redacting impl is what satisfies the lint — a type with no `Debug`
//! at all would not have been an option, and would have been the wrong shape anyway (a
//! surrounding struct's derived `Debug` would then fail to compile rather than redact).
//!
//! ## Comparison is constant-time
//!
//! [`Token::verify`], and the argument for it is where the code is. A `==` on a secret is a
//! defect in this position and not a style preference: `PartialEq` on strings and slices
//! short-circuits at the first differing byte, which turns "how long did the daemon take to
//! say no" into "how many leading digits were right".

use schema::{Error, Result};

/// How many bytes of kernel randomness one token carries.
///
/// Thirty-two, so 256 bits, rendered as 64 hex digits. The bound it has to clear is that no
/// one can search the space: 2^256 is not a number an attacker on a LAN reduces by trying,
/// and the token lives for one run of one daemon. Sixteen bytes would also clear it; 32 is
/// the size at which nobody has to do the arithmetic to be sure, and the cost is 32 more
/// characters in a URL nobody types by hand.
///
/// **It lives here rather than in `webcam-handler-schema::limits`**, and the precedent is
/// [`crate::uds::SOCKET_DIR_MODE`] one module along — D11's *other* security constant, in the
/// crate that reads it. The rule AGENTS states is about bounds ("settle deadlines, sweep caps,
/// recording caps, channel depths, shutdown drains"), and what makes `schema::limits` the
/// right home for `DAEMON_SOCKET_FILE` is that **two** crates have to agree on it —
/// `webcam-handler-client` resolves the same path the daemon binds. Nothing outside this crate
/// mints or checks a token: the web client receives one and the CLI clients never see one. A
/// number moved to the shared crate for tidiness would be a number `webcam-handler-schema`
/// carries for a reader it does not have.
pub const TOKEN_BYTES: usize = 32;

/// The query parameter the ready-to-open URL carries the token in.
///
/// One spelling, read by [`Token::ready_to_open_url`] here and by the token gate that will
/// parse it (P5b). The two halves of a name that has to match on both sides of a browser are
/// exactly the kind of pair that drifts.
pub const TOKEN_QUERY_PARAM: &str = "token";

/// The bearer token for one run of one daemon.
///
/// Holds the *rendering* rather than the bytes, deliberately: hex is the form the token is
/// presented in — in a URL, in an `Authorization` header, in the line an operator copies —
/// so keeping it that way means [`Token::verify`] compares what arrived against what was
/// published, with no encoding step in the middle that could answer a different question
/// than the one the client asked.
///
/// It is not `Clone` and not `Copy`. Nothing about this build needs a second copy of a
/// secret, and a type that hands them out makes "how many copies of the key are in this
/// process" a question with no answer. The composition root holds one and shares it with the
/// listener behind an `Arc` if it needs to.
pub struct Token {
    /// [`TOKEN_BYTES`] bytes of kernel randomness as lowercase hex. Private, and reachable
    /// only through [`Token::expose_secret`] — this module's header says why.
    hex: String,
}

impl Token {
    /// Mint one token from the kernel's CSPRNG.
    ///
    /// Called once, from the composition root, on the path that opens the TCP listener. It
    /// reads the machine rather than taking a value, so it is a seam and lives in the shell
    /// half of this module rather than in a pure core — everything downstream of it
    /// ([`Token::verify`], [`Token::ready_to_open_url`], the whole of [`super::posture`])
    /// takes values and is asserted without one.
    ///
    /// # Errors
    ///
    /// [`Error::DeviceIo`] when `getrandom(2)` refuses, carrying the errno. That variant
    /// rather than [`Error::StorageIo`] because D13 makes it the home for "the kernel refused
    /// an operation for a reason with no more specific home" and the alternative would have to
    /// invent a path for a syscall that touches no filesystem. In practice this cannot fail on
    /// a running Linux system — the flags this passes admit `EINTR`, which is retried, and
    /// `EFAULT`/`EINVAL`, which are about arguments this function builds itself — so the
    /// failure that is actually being typed here is a kernel this build does not understand,
    /// and a daemon that answered it by minting a weak token would be answering it wrongly.
    pub fn mint() -> Result<Token> {
        let mut bytes = [0_u8; TOKEN_BYTES];
        fill_from_kernel(&mut bytes)?;
        Ok(Token {
            hex: hex_of(&bytes),
        })
    }

    /// The secret, as the client presents it. **The one rendering that yields it.**
    ///
    /// Named so that the call site reads as what it is. Every use of it is a place the token
    /// leaves this type — the URL printed at startup, and a test — and a reviewer looking for
    /// "where can this leak" has one word to grep for.
    #[must_use]
    pub fn expose_secret(&self) -> &str {
        &self.hex
    }

    /// Whether `presented` is this token, compared in constant time.
    ///
    /// # Why this is not `==`, stated as a claim and as what the claim excludes
    ///
    /// **The claim.** The loop below has no early exit and no data-dependent branch: it
    /// visits every byte of a candidate of the right length, accumulates the differences with
    /// `|=`, and reads the accumulator once at the end. So the time this takes does not
    /// depend on *where* the first wrong digit is, and an attacker who can time the daemon's
    /// refusals cannot recover the token one digit at a time — which is exactly what
    /// `str`'s own `PartialEq` would let them do, because it compares lengths and then bytes
    /// and returns at the first difference.
    ///
    /// **What it does not claim.** The *length* is public: a candidate of the wrong length is
    /// refused immediately and measurably. That is deliberate and costs nothing — the length
    /// is a build constant ([`TOKEN_BYTES`]), it is visible in the URL the daemon prints, and
    /// treating it as a secret would mean reading past the end of the shorter of the two
    /// values. Nothing here is claimed about layers this module does not own: the HTTP
    /// parser above it, the router that decided to call this, and the allocator all have
    /// timing of their own. And Rust makes no *guarantee* that this compiles without
    /// branches — the accumulate-then-compare shape is what a constant-time comparison crate
    /// would provide, minus the optimization barriers it also provides, and adopting one to
    /// buy those barriers is a trade that has not been made here because the attacker this
    /// defends against is remote and the barrier defends against a compiler.
    ///
    /// **Nothing in this crate's suite can go red on the timing property itself.** A `==`
    /// here would pass every test in this module, because what a test can see is the answer
    /// and the claim is about the clock; a timing assertion would be a benchmark pretending
    /// to be a test, and on a shared runner it would be a flake. So what defends this is the
    /// argument beside the code and the person reading it — stated plainly rather than left
    /// as an implied guarantee (rubric rule 2's other half: an assertion that cannot go red
    /// is worse than an argument that admits it is one). What the tests *do* defend is the
    /// answer at every shape a rewrite gets wrong first: a prefix, a suffix, an equal-length
    /// near miss, a case variant, and nothing at all.
    ///
    /// A `&str` and not bytes because a candidate arrives from a header value or a query
    /// parameter that a caller has already had to turn into text. Nothing is lost: the token
    /// is ASCII hex, so a value that is not valid UTF-8 is not the token, and a caller
    /// refusing it before it gets here reaches the same answer. What must not happen is a
    /// caller reaching that answer with `==`.
    #[must_use]
    pub fn verify(&self, presented: &str) -> bool {
        let expected = self.hex.as_bytes();
        let presented = presented.as_bytes();
        if presented.len() != expected.len() {
            return false;
        }
        let mut difference = 0_u8;
        // `zip` and not an index, for two reasons that happen to agree: the daemon denies
        // `clippy::indexing_slicing` outside tests, and an iterator pair has no bounds check
        // to be branched on.
        for (want, got) in expected.iter().zip(presented) {
            difference |= want ^ got;
        }
        difference == 0
    }

    /// The URL D11 asks the daemon to print — "generated per run and printed as a
    /// ready-to-open URL".
    ///
    /// **`bound` is the address the listener actually got, which is not the one it asked
    /// for.** D11's default bind is `127.0.0.1:0`, where the `0` means "any free port, the
    /// kernel picks", and D11 says so in the same breath: "default `127.0.0.1:0` → report the
    /// bound port". So this takes the address off the listener rather than the address off
    /// the command line; a URL built from the request would send an operator to port zero.
    ///
    /// **The token rides the URL because a browser cannot be told to send a header.** An
    /// operator opening a link performs a navigation, and a navigation carries the headers
    /// the browser chose; there is no way to attach `Authorization: Bearer …` to it short of
    /// an extension. So the first request has to authenticate with what a URL can carry, and
    /// that is a query parameter ([`TOKEN_QUERY_PARAM`]). Once the page is loaded its **own**
    /// requests — the WebSocket, the MJPEG preview — are made by code that can set a header,
    /// and that is the form they use; building that is P5b's, and it is named here because
    /// "the token is in the URL" is only half of how this daemon is talked to.
    ///
    /// The cost of the query form is real and is stated rather than hidden: a URL with a
    /// secret in it lands in the browser's history and its omnibox suggestions, and in the
    /// terminal scrollback of whoever started the daemon. It is bounded by the token's
    /// lifetime — one run of one daemon — which is the reason D11 makes it per-run rather
    /// than persisted.
    ///
    /// There was a fourth exposure on that list and it is **closed** rather than accepted: a
    /// `Referer` header *is* this URL, so an `<a href>` out of the page would have handed the
    /// token to a third party. Every response this listener writes carries
    /// `Referrer-Policy: no-referrer` (D11's 2026-08-12 amendment, `super::listener`'s
    /// `REFERRER_POLICY`), which costs a header and removes the class — the page P5a ships has
    /// no links and P5c's may, which is the wrong order in which to discover a header.
    ///
    /// The address is rendered by [`std::net::SocketAddr`]'s own `Display`, so an IPv6 bind
    /// comes out bracketed (`http://[::1]:34567/`) exactly as a URL needs it. A wildcard bind
    /// renders as the wildcard — `http://0.0.0.0:34567/` — which is what this daemon is
    /// actually bound to and what Linux resolves to loopback for a client on the same
    /// machine; substituting a nicer address would be printing a URL for a bind that was not
    /// made.
    #[must_use]
    pub fn ready_to_open_url(&self, bound: std::net::SocketAddr) -> String {
        format!(
            "http://{bound}/?{TOKEN_QUERY_PARAM}={secret}",
            secret = self.expose_secret()
        )
    }
}

impl std::fmt::Debug for Token {
    /// Redacted, for the reason `engine::photo::Photograph`'s is: the derived impl prints the
    /// field, and the field is the key to a camera. What is printed instead is the one thing
    /// about a token that is safe to say and useful to see — that this is a token, and how
    /// long it is.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        /// The secret, wearing the only rendering it may have inside a `Debug`.
        ///
        /// A shim rather than a string literal so the placeholder is not quoted: a `Debug`
        /// reading `hex: "<redacted>"` invites the reading that the token *is* that text.
        struct Redacted;
        impl std::fmt::Debug for Redacted {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("<redacted>")
            }
        }

        f.debug_struct("Token")
            .field("bytes", &TOKEN_BYTES)
            .field("hex", &Redacted)
            .finish()
    }
}

/// Fill `bytes` from the kernel's CSPRNG, or refuse.
///
/// A loop rather than one call, and the two things it handles are both the kernel's to do
/// rather than ours to assume. `getrandom(2)` documents that a request of 256 bytes or fewer
/// from the initialized pool is never short and never interrupted — but *documented* is not
/// *checked*, the number a syscall answers with is device-derived data in rubric B10's sense,
/// and the failure mode of believing it is a token with trailing zeroes that nothing would
/// ever notice.
///
/// So: a short answer continues from where it stopped, `EINTR` retries, an answer of zero
/// bytes is refused rather than spun on, and an answer *longer* than the buffer it was given
/// is refused as the impossible thing it is (`split_at_mut_checked` is what makes that a
/// typed refusal instead of a panic).
fn fill_from_kernel(bytes: &mut [u8]) -> Result<()> {
    use rustix::io::Errno;
    use rustix::rand::{GetRandomFlags, getrandom};

    let mut rest: &mut [u8] = bytes;
    while !rest.is_empty() {
        // `&mut *rest` is a reborrow: `Buffer` takes the slice by value, and `rest` is needed
        // again on the next turn.
        match getrandom(&mut *rest, GetRandomFlags::empty()) {
            Ok(0) => {
                return Err(kernel_refused(
                    None,
                    "getrandom(2) returned no bytes and no error".to_owned(),
                ));
            }
            Ok(taken) => {
                // Read before the split, which takes the borrow the refusal would need.
                let asked = rest.len();
                let Some((_filled, unfilled)) = rest.split_at_mut_checked(taken) else {
                    return Err(kernel_refused(
                        None,
                        format!("getrandom(2) claimed {taken} bytes into a {asked}-byte buffer"),
                    ));
                };
                rest = unfilled;
            }
            // A signal arrived while the pool was still initializing. The only case that
            // reaches here is a boot too young to have a webcam daemon on it, and the answer
            // is to ask again rather than to hand back a short token.
            Err(Errno::INTR) => {}
            Err(errno) => {
                return Err(kernel_refused(
                    Some(errno.raw_os_error()),
                    std::io::Error::from(errno).to_string(),
                ));
            }
        }
    }
    Ok(())
}

/// D13's answer for a syscall with no more specific home, named once.
///
/// `crate::uds`'s `errno_io` is the same law about a path; this is the same law about a
/// syscall that has none, which is why it answers [`Error::DeviceIo`] rather than
/// [`Error::StorageIo`] — see [`Token::mint`]'s `# Errors`.
fn kernel_refused(errno: Option<i32>, message: String) -> Error {
    Error::DeviceIo {
        operation: "getrandom(2), minting the web listener's bearer token".to_owned(),
        errno,
        message,
    }
}

/// Lowercase hex, two digits per byte.
///
/// Hand-written for the reason this module's header gives: `base64`'s home is
/// `webcam-handler-api` "here only" (design §2.8), and a hex renderer is smaller than the
/// argument for widening it.
fn hex_of(bytes: &[u8]) -> String {
    let mut rendered = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        rendered.push(char::from(hex_digit(byte >> 4)));
        rendered.push(char::from(hex_digit(byte & 0x0f)));
    }
    rendered
}

/// One nibble as its lowercase hex digit.
///
/// Total over `0..=15`, which is what both call sites guarantee — a `u8` shifted right by
/// four and a `u8` masked with `0x0f`. Arithmetic rather than a lookup table because
/// indexing a table is the operation this crate denies outside tests
/// (`clippy::indexing_slicing`), and a token renderer is not where that exemption gets
/// argued for.
const fn hex_digit(nibble: u8) -> u8 {
    if nibble < 10 {
        b'0' + nibble
    } else {
        b'a' + (nibble - 10)
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use super::*;

    /// A token, or a test that says why it could not have one.
    fn minted() -> Token {
        Token::mint().expect("the kernel has a CSPRNG")
    }

    #[test]
    fn a_minted_token_is_thirty_two_bytes_of_lowercase_hex() {
        let token = minted();
        let secret = token.expose_secret();

        assert_eq!(
            secret.len(),
            TOKEN_BYTES * 2,
            "a token of the wrong length is a token minted from the wrong number of bytes"
        );
        assert!(
            secret
                .bytes()
                .all(|digit| digit.is_ascii_digit() || (b'a'..=b'f').contains(&digit)),
            "not lowercase hex, so it is not what a URL and a header can carry unescaped"
        );
    }

    #[test]
    fn two_tokens_minted_in_one_process_differ() {
        // The one property that makes it a secret rather than a password: a second run — or a
        // second daemon on the same machine — gets a different token. A generator seeded from
        // a pid or a start-of-process clock passes every other test in this module and fails
        // this one.
        assert_ne!(minted().expose_secret(), minted().expose_secret());
    }

    #[test]
    fn verification_accepts_the_token_and_refuses_every_near_miss() {
        let token = minted();
        let secret = token.expose_secret().to_owned();

        assert!(token.verify(&secret), "the token did not verify as itself");

        // A prefix and a suffix: the two shapes a truncated copy-paste takes. Both are
        // refused by the length check, which is the part of `verify` that is deliberately not
        // constant-time.
        assert!(
            !token.verify(&secret[..secret.len() - 1]),
            "a prefix passed"
        );
        assert!(!token.verify(&secret[1..]), "a suffix passed");

        // The one that matters: same length, one digit different. This is what a timing
        // attack's candidates look like, and it is the only case that reaches the loop.
        let mut near_miss: Vec<char> = secret.chars().collect();
        near_miss[0] = if near_miss[0] == '0' { '1' } else { '0' };
        let near_miss: String = near_miss.into_iter().collect();
        assert_eq!(
            near_miss.len(),
            secret.len(),
            "the near miss is not a near miss"
        );
        assert_ne!(near_miss, secret);
        assert!(
            !token.verify(&near_miss),
            "an equal-length near miss passed"
        );

        // A case variant, because hex has two spellings and only one of them was minted. A
        // comparison that lowercased first would accept a token nobody was given.
        let shouted = secret.to_ascii_uppercase();
        assert_ne!(shouted, secret, "the token has no letters in it to shout");
        assert!(!token.verify(&shouted), "an upper-case variant passed");

        // And nothing at all, which is what an anonymous request presents.
        assert!(!token.verify(""), "the empty string passed");
    }

    #[test]
    fn the_debug_rendering_redacts_the_secret_and_the_named_rendering_yields_it() {
        // `crate::logging`'s doctrine, asserted rather than asserted-about: the derived
        // `Debug` would print the field, and this is the test that would go red if somebody
        // deleted the hand-written one.
        let token = minted();
        let secret = token.expose_secret().to_owned();
        assert_eq!(
            secret.len(),
            TOKEN_BYTES * 2,
            "an empty secret is in every string"
        );

        let rendered = format!("{token:?}");
        assert!(
            !rendered.contains(&secret),
            "the token printed itself into a debug rendering: {rendered}"
        );
        assert!(
            rendered.contains("redacted"),
            "a debug rendering that does not say it is hiding something: {rendered}"
        );

        // The other half of the claim, and it needs a rendering that is *not* the accessor
        // the secret was read out of, or it would assert that a string contains itself: the
        // URL is built through `expose_secret`, so a `Token` that had quietly stopped
        // yielding its secret would fail here as well as in the browser.
        let bound: SocketAddr = "127.0.0.1:34567".parse().expect("a bound address");
        assert!(
            token.ready_to_open_url(bound).contains(&secret),
            "the URL an operator opens does not carry the token"
        );
    }

    #[test]
    fn the_url_carries_the_bound_address_and_not_the_requested_one() {
        // D11's default asks for `127.0.0.1:0` and reports "the bound port", so the URL is
        // built from what the listener got. Port zero in an operator's browser is the failure
        // this test is shaped to catch.
        let token = minted();
        let bound: SocketAddr = "127.0.0.1:34567".parse().expect("a bound address");

        let url = token.ready_to_open_url(bound);

        assert_eq!(
            url,
            format!(
                "http://127.0.0.1:34567/?token={secret}",
                secret = token.expose_secret()
            )
        );
        assert!(
            !url.contains(":0"),
            "the requested port reached the URL an operator is asked to open: {url}"
        );
    }

    #[test]
    fn an_ipv6_url_is_bracketed_the_way_a_browser_needs_it() {
        // Free from `SocketAddr`'s own `Display`, and asserted because it is load-bearing:
        // `http://::1:34567/` is not a URL.
        let bound: SocketAddr = "[::1]:34567".parse().expect("a bound address");
        let url = minted().ready_to_open_url(bound);

        assert!(url.starts_with("http://[::1]:34567/?token="), "{url}");
    }

    #[test]
    fn hex_is_lowercase_and_two_digits_per_byte_including_the_leading_zero() {
        // The renderer on its own, over the four shapes a byte takes: zero, one that needs a
        // leading zero, one that needs both nibbles, and the top of the range. A renderer that
        // dropped leading zeroes would shorten tokens by a byte now and then, which nothing
        // else in this module could see.
        assert_eq!(hex_of(&[0x00, 0x0f, 0xa5, 0xff]), "000fa5ff");
        assert_eq!(hex_of(&[]), "");
        for nibble in 0..16_u8 {
            let digit = char::from(hex_digit(nibble));
            assert_eq!(
                digit.to_digit(16),
                Some(u32::from(nibble)),
                "hex_digit({nibble}) is {digit}"
            );
            assert!(!digit.is_ascii_uppercase());
        }
    }

    #[test]
    fn a_short_answer_from_the_kernel_is_continued_rather_than_trusted() {
        // The loop in `fill_from_kernel` exists for a case a test cannot arrange — the
        // syscall is not a seam — so what is asserted is the property the loop is there to
        // provide: the buffer comes back fully filled, and not one byte of it is the zero it
        // started as by accident. Over many tokens, because one all-zero byte is a 1-in-256
        // coincidence and a whole run of them is not.
        let mut seen_nonzero = [false; TOKEN_BYTES * 2];
        for _ in 0..16 {
            let token = minted();
            for (slot, digit) in seen_nonzero.iter_mut().zip(token.expose_secret().bytes()) {
                *slot |= digit != b'0';
            }
        }
        assert!(
            seen_nonzero.iter().all(|seen| *seen),
            "some position was '0' in all sixteen tokens, which is what a partially filled \
             buffer looks like"
        );
    }
}
