//! Where the daemon's events go — design §2.6's fmt layer.
//!
//! > Logging: `tracing` everywhere; fmt layer foreground, journald layer (pure-Rust
//! > protocol, no libsystemd) under systemd.
//!
//! P4b landed the first half; P4e-ii's second half lands the other, so both layers named in
//! that sentence now exist and **this module is where the daemon chooses between them**.
//!
//! ## One layer, not two, and why that is the interesting part
//!
//! The obvious reading of design §2.6 is "install both and let each write where it writes".
//! That is wrong, and measurably so: under systemd the daemon's **stderr already is the
//! journal**, so a fmt layer beside a journald layer puts every line in the journal twice —
//! once as a structured entry and once as a `_TRANSPORT=stdout` copy of the rendered text. So
//! the choice is exclusive, and what decides it is not "are we under systemd" but the
//! narrower question systemd itself documents: is *this process's stderr* the journal?
//! `$JOURNAL_STREAM` carries a `device:inode` and the answer is a comparison against `fstat`
//! of stderr — `crate::systemd::stderr_is_the_journal`, whose comparison is a pure function
//! for the reason everything else in this workspace is (both directions have to be
//! assertable, and neither can be arranged by a test that may not write the environment).
//!
//! Asking that narrow question is also what makes the daemon behave correctly in the case the
//! broad one gets wrong: a `webcam-handler-daemon` started from a unit but with its stderr
//! redirected to a file inherits `$JOURNAL_STREAM` and is **not** on the journal, and a build
//! that trusted the variable would send its whole log to a socket nobody is reading.
//!
//! **A journald layer that cannot be built is a `warn` and the fmt layer, never silence.**
//! `tracing_journald::layer()` connects to `/run/systemd/journal/socket`, and a sandbox or a
//! container can refuse that connect while `$JOURNAL_STREAM` still matches. Falling back is
//! the direction AGENTS rule 3 requires: duplicated lines are noise, and a daemon logging into
//! a socket that is not there is silence.
//!
//! ## Why this changes nothing about the rest of the suite
//!
//! Every subprocess test and shell predicate in this project learns that a daemon is up by
//! **reading a line off its stderr** — `crates/daemon/tests/lock.rs`,
//! `crates/daemon/tests/systemd.rs`, `scripts/gates/uds-permissions.sh` and
//! `scripts/gates/socket-activation.sh`. None of them sets `$JOURNAL_STREAM`, and none of
//! them is started by systemd, so [`install`] takes the fmt branch in all of them and the
//! readiness line is exactly where it was. The one thing that *would* break them is a build
//! that installed the journald layer on the strength of the variable being *set*, which is
//! the failure the comparison above exists to prevent.
//!
//! ## Why installation lives in the composition root
//!
//! [`install`] is called by `webcam-handler-daemon`'s `main` and by nothing else. A subscriber
//! installed from library code would be installed by every integration test that drove a
//! server — the daemon exists as a library precisely so tests can — and the first one to win
//! would then be capturing every other test's events. Same shape as the rest of the workspace:
//! seams live in the shell, and the composition root is where the process's edges are.
//!
//! ## Stderr, not stdout
//!
//! `--json` answers are stdout's contract in this project, and the daemon has none of its
//! own; systemd and journald read stderr; and the subprocess tests in this workspace use
//! stdout as a synchronization pipe, which log lines would poison.
//!
//! ## A frame may contain a person
//!
//! AGENTS is unambiguous — "Camera frames never enter the repository, logs, or error
//! messages" — and this module is the moment that rule stops being hypothetical: before
//! P4b there was no subscriber anywhere in the workspace, so `tracing::debug!(?value)`
//! could not print anything. The two hand-written `Debug` impls that pre-argued this
//! (`engine::photo`, `cli_core`) are what keep a photograph from rendering as its bytes;
//! the rule for anything written here is narrower and needs no machinery: **no frame, no
//! photo payload, and nothing derived from one is ever a field on an event.** Paths,
//! counts, control slugs and D13 errors are what the daemon has to say. Note N36 records
//! that the rule has four subjects and no walkable population; this is not a fifth
//! subject, because nothing in this module holds pixels.

use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

/// The level the daemon logs at when the operator has not said otherwise.
///
/// `info` because the events at that level are the ones an operator running
/// `webcam-handler-daemon` in a terminal or reading `journalctl` needs — the socket it is
/// serving, a camera opened, a camera closed on idle — and nothing below it is about their
/// machine. `RUST_LOG` overrides it with the usual `tracing-subscriber` syntax, which is why
/// the `env-filter` feature is worth its four transitive crates: raising the level on a daemon
/// that is already misbehaving must not require a restart flag it was not started with.
pub const DEFAULT_LOG_FILTER: &str = "info";

/// Where this daemon's log lines are **kept**, which is a different question from which layer
/// renders them.
///
/// [`install`] answers it once, from the machine, and hands the answer back as a value so that
/// the one line in this daemon that writes a secret down on purpose can ask it (see
/// [`Sink::url_this_sink_may_keep`]). Two variants and no third, because the question this type
/// exists to answer has two answers:
///
/// - [`Sink::Stderr`] — wherever the operator pointed standard error: a terminal, a pipe, a
///   file of their own. D11 already prices that exposure in
///   `super::http::token::Token::ready_to_open_url`: "the terminal scrollback of whoever
///   started the daemon", bounded by the token's one-run lifetime.
/// - [`Sink::Journal`] — the systemd journal, which is a **persistent, indexed store readable
///   by every member of `systemd-journal` and `adm`**, and which keeps what it is given for as
///   long as its retention says rather than for as long as the daemon runs.
///
/// **The fallback branch is `Journal` too, and that is the point of naming the destination
/// rather than the layer.** When `$JOURNAL_STREAM` matches this process's stderr but the
/// journald socket cannot be opened, [`install`] renders to stderr — and *that stderr is the
/// journal*, which captures the lines as unstructured text. A type that recorded "which layer
/// did I install" would answer `Stderr` there and put the credential in the journal anyway.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sink {
    /// Standard error, wherever this operator pointed it.
    Stderr,
    /// The systemd journal, as structured entries or as captured stderr text.
    Journal,
}

/// What stands in for the credential in a rendering the journal keeps.
///
/// The spelling `super::http::token::Token`'s hand-written `Debug` uses, deliberately: a reader
/// who has seen one of them has seen the other, and two spellings of "this daemon removed
/// something here" would read as two different things having happened.
const REDACTED: &str = "<redacted>";

impl Sink {
    /// Which sink this process has, given whether its stderr is the journal.
    ///
    /// A pure function over the one fact that decides it, so both answers are assertable in a
    /// process that has neither — `crate::systemd::stderr_is_the_journal` is the seam and it
    /// reads a machine no test may arrange (`std::env::set_var` is a data race in Rust 2024,
    /// and this binary's tests share a process).
    #[must_use]
    fn of(stderr_is_the_journal: bool) -> Sink {
        if stderr_is_the_journal {
            Sink::Journal
        } else {
            Sink::Stderr
        }
    }

    /// The ready-to-open URL as **this** sink may keep it (owner ruling, 2026-08-16; note
    /// **N182**).
    ///
    /// D11 asks this daemon to print the web client's URL "as a ready-to-open URL", and that
    /// URL carries the run's bearer token in its query string because a navigation cannot be
    /// told to send a header (`super::http::token::Token::ready_to_open_url` argues the form
    /// and prices its exposures). So the line cannot be deleted, and the composition root's own
    /// comment is right that it is the one place in this daemon where a secret is written down
    /// on purpose.
    ///
    /// **What it is written down *into* is this module's decision, and it was not being made.**
    /// `tracing::info!(url = %…)` is not printing: it is an event handed to whichever layer
    /// [`install`] chose, and under [`Sink::Journal`] that is an indexable structured field in
    /// a store that outlives the run and is readable by two groups of accounts. Note **N94**
    /// removed a *different* producer of the same run-long credential from the same sink three
    /// days before this one was found; this is the other producer.
    ///
    /// The owner's ruling is the narrow one: **redact in the journal only**. One
    /// `tracing::info!` stays, an operator at a terminal still gets a URL they can click, and
    /// the persistent sink gets a URL with nothing in it worth keeping. What that costs is
    /// stated rather than hidden: a `webcam-handler-daemon` started from a unit *with* `--http`
    /// added to it publishes no way to reach its own web client, because the journal is the
    /// only output such a daemon has. The shipped unit has no `--http`
    /// (`packaging/systemd/wchd.service`), so reaching that cell takes an operator adding a
    /// flag — and an operator who adds it can read the token from a `--http` daemon run in a
    /// terminal, or ask for a different arrangement, which is the owner's to design.
    ///
    /// **Everything after the `?` goes, not just the token's value.** The redaction is by shape
    /// and not by search: this module never sees the secret, compares nothing against it, and
    /// therefore cannot fail to recognise it. A second query parameter added to that URL three
    /// sub-milestones from now is redacted by a rule that was written before it existed, which
    /// is the direction D11 errs in everywhere else.
    #[must_use]
    pub fn url_this_sink_may_keep<'a>(&self, url: &'a str) -> std::borrow::Cow<'a, str> {
        match self {
            Sink::Stderr => std::borrow::Cow::Borrowed(url),
            // A `?` with nothing after it is left alone: that is D11's token-less loopback
            // cell, where `super::http::listener::Serving::ready_to_open_url` deliberately
            // writes no `?token=` because an empty one reads as a token that failed to render
            // — and appending a redaction marker to it would invent that reading back.
            Sink::Journal => match url.split_once('?') {
                Some((addressed, query)) if !query.is_empty() => {
                    std::borrow::Cow::Owned(format!("{addressed}?{REDACTED}"))
                }
                _ => std::borrow::Cow::Borrowed(url),
            },
        }
    }
}

/// Install the one layer this process should have. Called once, from `main`.
///
/// The journald layer when stderr already is the journal, the fmt layer otherwise — this
/// module's header argues why that is exclusive rather than both, and why the question is
/// about *this stderr* rather than about systemd.
///
/// The filter sits on the registry rather than on either layer, so `RUST_LOG` means the same
/// thing on both paths: an operator raising the level on a misbehaving daemon must not get a
/// different answer depending on how it was started.
///
/// It returns the [`Sink`] it installed into, because one line this daemon writes has to know:
/// the ready-to-open URL carries a credential, and whether that credential is being written to
/// an operator's terminal or into a persistent journal is exactly this function's decision.
/// Returned rather than stored in a global for this module's own reason — a process-wide value
/// would be a second place the answer lives, readable by tests that never installed anything.
pub fn install() -> Sink {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER));
    let registry = tracing_subscriber::registry().with(filter);
    let sink = Sink::of(crate::systemd::stderr_is_the_journal());

    if sink == Sink::Journal {
        match crate::systemd::journald_layer() {
            Ok(journal) => {
                registry.with(journal).init();
                // After the subscriber exists, or the line that says which layer is
                // installed would be the one line no layer carried.
                tracing::debug!(
                    "stderr is this process's journal stream; logging structured entries \
                     through the journald layer instead of rendering them to stderr twice"
                );
                return sink;
            }
            Err(err) => {
                // Not silence, and not a failure to start: the daemon logs somewhere.
                registry.with(fmt_layer()).init();
                tracing::warn!(
                    error = %err,
                    "stderr is this process's journal stream but the journald socket could \
                     not be opened; falling back to rendering log lines on stderr, which \
                     the journal will still capture as unstructured text"
                );
                // **Still `Journal`**, and the fallback is exactly why this value is about the
                // destination: these lines are on their way to the journal as text.
                return sink;
            }
        }
    }

    registry.with(fmt_layer()).init();
    sink
}

/// The fmt layer, on stderr.
///
/// Colour follows the terminal rather than the default (which is "always"): under systemd
/// stderr is a journal socket, and escape sequences in a journal are noise nobody asked
/// for. A function rather than a line in [`install`] because two branches build it — the
/// ordinary one and the journald fallback — and two spellings of "how this daemon renders a
/// log line" is how they come to differ.
fn fmt_layer<S>() -> impl tracing_subscriber::Layer<S>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
}

/// A `tracing` writer a test can read back.
///
/// Thread-local through [`tracing::subscriber::set_default`], so it captures the test that
/// installed it and no other test's — [`install`] deliberately runs only from `main` for the
/// same reason, and a `try_init` race between two tests would otherwise decide which one saw
/// its own events.
///
/// It lives here rather than in the two modules that use it because "a line the daemon said"
/// is this module's subject: `crate::uds` asserts that a downgraded bind announces itself and
/// `crate::shutdown` asserts that an expired drain does, and both are the same claim — a
/// bound that was silently exceeded is worse than the thing it was bounding (AGENTS rule 3).
/// Two copies of the writer would be two answers to what "logged" means.
#[cfg(test)]
mod tests {
    use super::*;

    /// A URL of the shape `super::super::http::token::Token::ready_to_open_url` writes.
    ///
    /// A literal rather than a minted token, because the claims below are about *shape* and
    /// this module must never hold the secret — the end-to-end claim, over a real token and a
    /// real URL, is `token.rs`'s
    /// `the_url_the_journal_keeps_carries_no_token_and_the_one_an_operator_reads_does`.
    const READY_TO_OPEN: &str = "http://127.0.0.1:34567/?token=0123456789abcdef0123456789abcdef\
         0123456789abcdef0123456789abcdef";

    #[test]
    fn the_sink_is_decided_by_where_this_process_stderr_goes() {
        // Both answers, from the one fact that decides them. The seam that reads the machine
        // is `crate::systemd::stderr_is_the_journal`, which this process cannot arrange, so
        // the decision is asserted as the pure function it is.
        assert_eq!(Sink::of(true), Sink::Journal);
        assert_eq!(Sink::of(false), Sink::Stderr);
    }

    #[test]
    fn a_url_the_journal_would_keep_is_a_url_with_nothing_in_it_worth_keeping() {
        let kept = Sink::Journal.url_this_sink_may_keep(READY_TO_OPEN);

        assert!(
            !kept.contains("0123456789abcdef"),
            "the credential reached the persistent sink: {kept}"
        );
        // Everything after the `?`, and not the token's value alone: the redaction is by shape,
        // so a second parameter added to that URL later is redacted by a rule that predates it.
        assert_eq!(kept, format!("http://127.0.0.1:34567/?{REDACTED}"));
        assert_eq!(
            Sink::Journal.url_this_sink_may_keep("http://127.0.0.1:34567/?token=abc&camera=cam:x"),
            format!("http://127.0.0.1:34567/?{REDACTED}")
        );
    }

    #[test]
    fn an_operator_at_a_terminal_still_gets_the_url_d11_asks_for() {
        // The half of the ruling that keeps this from being "stop printing it". Byte-identical,
        // because a URL an operator has to repair before opening is not a ready-to-open URL.
        assert_eq!(
            Sink::Stderr.url_this_sink_may_keep(READY_TO_OPEN),
            READY_TO_OPEN
        );
    }

    #[test]
    fn a_url_that_carries_no_credential_is_not_made_to_look_like_one() {
        // D11's token-less loopback cell: `Serving::ready_to_open_url` answers the plain URL
        // there, deliberately without an empty `?token=` that would read as a token that failed
        // to render. A redaction that appended `?<redacted>` to it would invent exactly that
        // reading, in the one cell where there is no secret at all.
        let plain = "http://127.0.0.1:34567/";
        assert_eq!(Sink::Journal.url_this_sink_may_keep(plain), plain);
        assert_eq!(Sink::Stderr.url_this_sink_may_keep(plain), plain);
        // And a `?` with nothing after it is the same fact spelled differently.
        let empty = "http://127.0.0.1:34567/?";
        assert_eq!(Sink::Journal.url_this_sink_may_keep(empty), empty);
    }
}

#[cfg(test)]
#[derive(Clone, Default)]
pub(crate) struct CapturedLog(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

#[cfg(test)]
impl CapturedLog {
    /// Everything written so far, as the operator would have read it.
    pub(crate) fn rendered(&self) -> String {
        let bytes = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// Make this the thread's subscriber until the guard is dropped.
    fn installed(&self) -> tracing::subscriber::DefaultGuard {
        let subscriber = tracing_subscriber::fmt()
            .with_writer(self.clone())
            .with_ansi(false)
            .finish();
        tracing::subscriber::set_default(subscriber)
    }
}

#[cfg(test)]
impl std::io::Write for CapturedLog {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CapturedLog {
    type Writer = CapturedLog;

    fn make_writer(&'a self) -> CapturedLog {
        self.clone()
    }
}

/// Run `body` with everything it logs captured.
#[cfg(test)]
pub(crate) fn capturing<T>(body: impl FnOnce() -> T) -> (T, String) {
    let captured = CapturedLog::default();
    let guard = captured.installed();
    let value = body();
    drop(guard);
    (value, captured.rendered())
}

/// The same, for a body that awaits.
///
/// The guard is held **across** the awaits rather than around a `block_on`, which is what
/// makes this capture anything at all: the default subscriber is a thread-local, and a future
/// is polled many times. That costs the future its `Send`, which is why every caller is a
/// current-thread `#[tokio::test]` — the flavour `start_paused` already requires.
#[cfg(test)]
pub(crate) async fn captured<T>(body: impl Future<Output = T>) -> (T, String) {
    let captured = CapturedLog::default();
    let guard = captured.installed();
    let value = body.await;
    drop(guard);
    (value, captured.rendered())
}
