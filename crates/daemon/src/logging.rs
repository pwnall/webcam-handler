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

/// Install the one layer this process should have. Called once, from `main`.
///
/// The journald layer when stderr already is the journal, the fmt layer otherwise — this
/// module's header argues why that is exclusive rather than both, and why the question is
/// about *this stderr* rather than about systemd.
///
/// The filter sits on the registry rather than on either layer, so `RUST_LOG` means the same
/// thing on both paths: an operator raising the level on a misbehaving daemon must not get a
/// different answer depending on how it was started.
pub fn install() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER));
    let registry = tracing_subscriber::registry().with(filter);

    if crate::systemd::stderr_is_the_journal() {
        match crate::systemd::journald_layer() {
            Ok(journal) => {
                registry.with(journal).init();
                // After the subscriber exists, or the line that says which layer is
                // installed would be the one line no layer carried.
                tracing::debug!(
                    "stderr is this process's journal stream; logging structured entries \
                     through the journald layer instead of rendering them to stderr twice"
                );
                return;
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
                return;
            }
        }
    }

    registry.with(fmt_layer()).init();
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
