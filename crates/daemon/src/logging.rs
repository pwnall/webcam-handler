//! Where the daemon's events go — design §2.6's fmt layer.
//!
//! > Logging: `tracing` everywhere; fmt layer foreground, journald layer (pure-Rust
//! > protocol, no libsystemd) under systemd.
//!
//! P4b lands the first half. The journald layer arrives with the rest of the systemd
//! integration (P4e-ii, the shutdown half of the split note N58 records), which is also
//! where `sd_notify` and socket activation live; adding it here would put a later phase's
//! criteria past their gate.
//!
//! ## Why installation lives in the composition root
//!
//! [`install`] is called by `wchd`'s `main` and by nothing else. A subscriber installed
//! from library code would be installed by every integration test that drove a server —
//! the daemon exists as a library precisely so tests can — and the first one to win would
//! then be capturing every other test's events. Same shape as the rest of the workspace:
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

/// The level the daemon logs at when the operator has not said otherwise.
///
/// `info` because the events at that level are the ones an operator running `wchd` in a
/// terminal or reading `journalctl` needs — the socket it is serving, a camera opened, a
/// camera closed on idle — and nothing below it is about their machine. `RUST_LOG`
/// overrides it with the usual `tracing-subscriber` syntax, which is why the `env-filter`
/// feature is worth its four transitive crates: raising the level on a daemon that is
/// already misbehaving must not require a restart flag it was not started with.
pub const DEFAULT_LOG_FILTER: &str = "info";

/// Install the fmt layer on stderr. Called once, from `main`.
///
/// Colour follows the terminal rather than the default (which is "always"): under systemd
/// stderr is a journal socket, and escape sequences in a journal are noise nobody asked
/// for.
pub fn install() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .init();
}
