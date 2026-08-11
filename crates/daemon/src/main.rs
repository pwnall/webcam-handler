//! `wchd` — the webcam-handler daemon.
//!
//! One of the two composition roots (design §2.11). This file is the process's edges and
//! nothing else: the subscriber, the argument surface, the backend factory match, the
//! state directory's lock, the socket, and the exit code. Everything it composes lives in
//! the library beside it, so an integration test can build the same server without a
//! process.
//!
//! ## Startup order, which is load-bearing
//!
//! 1. **The state-directory lock** (D9), taken once and held for the process's lifetime.
//!    It is the daemon's mutual exclusion — a second `wchd` is refused right here — and
//!    it is also what licenses step 3: a leftover socket file can only be removed by a
//!    process that has already established no other daemon is alive.
//! 2. **The socket directory**, created 0700 and *asserted* 0700 (D11).
//! 3. **The socket**, replacing a file left by a dead daemon, then bound.
//! 4. **Serving**, until the process is told to stop.
//!
//! The backend is constructed before all of it, because a `--profile` that does not parse
//! is a usage mistake and reporting it should not first take a lock, make a directory and
//! bind a socket. Nothing about constructing a backend opens a camera (D12).
//!
//! ## What this build does about stopping, and what it does not claim
//!
//! Nothing here installs a signal handler, so SIGTERM and SIGINT both terminate the
//! process — that is the kernel's default disposition, not a parity this daemon
//! implements, and P4e-ii's "SIGTERM ≡ SIGINT" is a claim about *draining*, which this build
//! does not do. What is nevertheless true, and is why an un-drained exit is survivable
//! rather than a hole: Linux releases an `flock` when the last descriptor on its open
//! file description closes, so a killed `wchd` releases the state lock, and the same
//! sentence covers every camera descriptor its actors held. The socket file it leaves
//! behind is handled by whoever starts next (`daemon::uds::SocketDir::bind`).
//!
//! `std::process::exit` is lint-banned in this workspace precisely so that stays true:
//! main returns an [`ExitCode`] and the stack unwinds, releasing the lock on the way.
#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

use std::process::ExitCode;
use std::sync::Arc;

use camino::Utf8PathBuf;
use clap::Parser;
use daemon::logging;
use daemon::server::Wchd;
use daemon::state::OwnedState;
use daemon::uds::{self, SocketDir};
use engine::paths::SystemEnv;
use engine::store::SessionStore;
use schema::Result;
use schema::backend::{BackendKind, CameraBackend};

/// Serve webcam-handler over a Unix socket (design D10, D11).
#[derive(Debug, Parser)]
#[command(name = "wchd", version, about, long_about = None)]
struct Args {
    /// Which backend to drive.
    #[arg(long, value_name = "KIND", default_value = "v4l2", value_parser = backend_kind)]
    backend: BackendKind,

    /// Device profiles for the fake backend to replay. Repeatable.
    ///
    /// Required with `--backend fake`, and enforced by clap rather than at run time, for
    /// the reason `wch`'s identical flag states: a backend with nothing to replay
    /// enumerates nothing, and "no cameras" is exactly what an operator whose cameras had
    /// vanished would see. A usage mistake must not be spelled like a device answer.
    #[arg(long, value_name = "PATH", required_if_eq("backend", "fake"))]
    profile: Vec<Utf8PathBuf>,
}

/// `--backend`, parsed through the schema's own spelling.
///
/// Not a `ValueEnum` derive, for the reason `cli-core` newtypes the same vocabulary: the
/// spelling belongs to [`BackendKind::as_str`] and a derive would put a second copy of it
/// here. Not `cli_core::BackendKindArg` either — `wchd` links no `cli-core`, because T4 is
/// the *command* surface `wch` and `wchc` share and this daemon has no verbs, only a
/// backend to choose.
fn backend_kind(text: &str) -> std::result::Result<BackendKind, String> {
    BackendKind::parse(text).ok_or_else(|| {
        let known: Vec<&str> = BackendKind::ALL.iter().map(|kind| kind.as_str()).collect();
        format!(
            "unknown backend {text:?}; known backends: {}",
            known.join(", ")
        )
    })
}

/// The one place `wchd` names a backend.
///
/// The second of exactly two exhaustive matches over [`BackendKind`] (design §2.11): the
/// vocabulary is closed, so adding a backend stops *both* composition roots' builds until
/// the new one is wired, which is the compile-fail-on-new-backend property living where
/// the dependency edges already are.
///
/// # Errors
///
/// Whatever `engine::profile::read` refuses with for a `--profile` that is missing, is not
/// a device profile, or was written by a build this one does not speak.
fn backend_for(args: &Args) -> Result<Arc<dyn CameraBackend>> {
    match args.backend {
        BackendKind::V4l2 => Ok(Arc::new(v4l2::V4l2Backend::new())),
        BackendKind::Fake => {
            // `--profile` is `required_if_eq("backend", "fake")`, so an empty list here
            // cannot come from a command line.
            let mut profiles = Vec::with_capacity(args.profile.len());
            for path in &args.profile {
                profiles.push(engine::profile::read(path)?);
            }
            Ok(Arc::new(fake::FakeBackend::new(profiles)?))
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    logging::install();

    match run(&args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // The typed error, rendered once, on the stream systemd captures. D13 already
            // says which directory is wrong and what is wrong with it; there is nothing
            // for this line to add.
            tracing::error!(%error, "wchd cannot serve");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: &Args) -> Result<()> {
    let env = SystemEnv;
    let backend = backend_for(args)?;

    // D9's daemon protocol, and this binding's lifetime is the lock's: `state` lives until
    // `run` returns, which is until the process stops serving. Never blocks — a second
    // `wchd` is answered with `StoreLocked` naming the holder rather than waiting for a
    // lock that, by this protocol's definition, will not be free.
    let state = OwnedState::take(&env)?;

    let dir = SocketDir::prepare(&env)?;
    let listener = dir.bind(state.lock())?;

    // The one line an operator looks for. A socket path is not a frame and not derived
    // from one (see `daemon::logging`).
    tracing::info!(socket = %dir.socket_path(), backend = backend.name(), "wchd is serving");

    // The wire surface is `webcam-handler-api`'s T5 surface, and the daemon *mounts* it
    // with the generated `into_rpc()` rather than registering methods of its own —
    // inventing a second registration path is the thing D10 exists to prevent. Since P4e-i
    // that is two generated traits merged into one `Methods` by `daemon::server::mount`,
    // which every integration fixture goes through as well, so "one registration" is a
    // property of one function rather than of two call sites agreeing.
    //
    // A second `SessionStore` over the directory this process already owns, which is what
    // that type is for: "cheap to build and cheap to clone-by-rebuilding — it owns a path,
    // not a handle". The *lock* is the one `state` took at startup, shared rather than
    // retaken: `flock` denies a second open file description in this process exactly as it
    // denies another's, so a daemon that took a second would answer its own clients
    // `StoreLocked` naming its own pid (`daemon::state`'s header).
    let wchd = Wchd::new(
        backend,
        SessionStore::new(state.store().root()),
        state.token(),
    );
    // D12's other half. The handle is dropped rather than held: dropping a `JoinHandle`
    // detaches the task, and this build has nothing to say about when housekeeping ends —
    // it ends with the runtime, which ends with the process. P4e-ii owns stopping it in an
    // order.
    drop(wchd.spawn_idle_sweeps());
    let mut server = uds::serve(listener, daemon::server::mount(wchd)?);

    // Runs until the process is signalled. `stopped()` resolves when the accept loop and
    // every connection it spawned are gone, which is what an integration test uses; here
    // nothing calls `stop`, so this is the daemon's main loop — and the `?` is the whole
    // reason it answers a `Result`: a daemon that gave up on `accept` has stopped serving,
    // and reporting that as a clean exit would tell `Restart=on-failure` not to restart it
    // and leave the operator a socket file with nobody behind it.
    let served = server.stopped().await;

    // Said out loud rather than left to the end of a scope, because it is the one thing
    // this build does claim about stopping: the state directory is released, and released
    // *after* the server has stopped answering. It is the orderly half of a release the
    // kernel performs anyway when the process dies (see this file's header) — P4e-ii's
    // shutdown discipline is about the order, never about whether it happens.
    drop(state);
    served
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_argument_surface_is_the_one_clap_can_build() {
        // clap's own consistency checks — duplicate long names, a `required_if_eq` naming
        // an argument that does not exist, a default value the parser refuses. They are
        // debug assertions, so without this they fire on an operator's first run rather
        // than here.
        use clap::CommandFactory as _;
        Args::command().debug_assert();
    }

    #[test]
    fn the_default_backend_is_real_hardware_and_the_fake_needs_a_profile() {
        // A daemon started with no arguments drives the machine's cameras; the replay
        // backend is opt-in and cannot be selected without something to replay. Both
        // directions, because a `--backend fake` that defaulted to an empty profile list
        // would enumerate nothing and look exactly like a machine with no webcam.
        let default = Args::try_parse_from(["wchd"]).expect("no arguments is a valid daemon");
        assert_eq!(default.backend, BackendKind::V4l2);
        assert!(default.profile.is_empty());

        Args::try_parse_from(["wchd", "--backend", "fake"])
            .expect_err("a replay backend with nothing to replay");
        let replaying = Args::try_parse_from(["wchd", "--backend", "fake", "--profile", "a.json"])
            .expect("a profile to replay");
        assert_eq!(replaying.backend, BackendKind::Fake);
        assert_eq!(replaying.profile, vec![Utf8PathBuf::from("a.json")]);
    }

    #[test]
    fn an_unknown_backend_names_the_ones_this_build_has() {
        // The vocabulary is closed and the message is derived from it, so a third backend
        // joins the sentence by existing rather than by somebody remembering to add it.
        let refused = backend_kind("libcamera").expect_err("not a backend this build links");
        for kind in BackendKind::ALL {
            assert!(refused.contains(kind.as_str()), "{refused}");
        }
        assert_eq!(backend_kind("fake"), Ok(BackendKind::Fake));
        assert_eq!(backend_kind("v4l2"), Ok(BackendKind::V4l2));
    }

    #[test]
    fn the_factory_match_refuses_a_profile_it_cannot_read_rather_than_serving_no_cameras() {
        // The composition root's own failure, and the one an operator hits: a path that is
        // not there. It has to be a typed refusal from `engine::profile::read` — a daemon
        // that shrugged and started with an empty backend would answer `wch_list` with
        // "no cameras", which is E3's conversion at the composition root.
        let args = Args::try_parse_from([
            "wchd",
            "--backend",
            "fake",
            "--profile",
            "/nowhere/at/all.json",
        ])
        .expect("a well-formed command line");
        assert_eq!(
            backend_for(&args)
                .expect_err("there is no such profile")
                .kind(),
            schema::ErrorKind::StorageIo
        );

        // And the arm that needs no file: the real backend is constructible on any host,
        // because constructing one opens nothing (D12).
        let real = backend_for(&Args::try_parse_from(["wchd"]).expect("no arguments"))
            .expect("the v4l2 backend is a value, not a device");
        assert_eq!(real.kind(), BackendKind::V4l2);
    }
}
