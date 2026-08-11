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
//! 0. **Socket activation, asked about first** (P4e-ii). `listenfd` *removes* `LISTEN_FDS`
//!    and `LISTEN_PID` from the environment as it reads them, so the one call that writes
//!    this process's environment happens before anything reads it —
//!    [`daemon::systemd::Activation::from_env`] carries the argument and the residual. The
//!    answer decides steps 2 and 3: a socket a service manager bound is adopted, and a daemon
//!    nobody activated binds its own.
//! 1. **The signal handlers**, before this process owns anything at all — so there is no
//!    window in which a daemon that already holds the lock and the socket is killed by the
//!    disposition it was about to replace.
//! 2. **The state-directory lock** (D9), taken once and held for the process's lifetime.
//!    It is the daemon's mutual exclusion — a second `wchd` is refused right here — and
//!    it is also what licenses step 4: a leftover socket file can only be removed by a
//!    process that has already established no other daemon is alive. It is taken on **both**
//!    startup paths: systemd binding the socket says nothing about who owns the state
//!    directory, and two `wchd`s sharing one activated socket would be two writers of one
//!    session tree.
//! 3. **The socket directory**, created 0700 and *asserted* 0700 (D11) — on the self-bound
//!    path. On the activated path the directory is the one the *inherited socket* is in, and
//!    the same mode-and-owner check is made about it, by the same function; what cannot be
//!    made is note N39's substitution defence, because the bind already happened elsewhere.
//! 4. **The socket**, replacing a file left by a dead daemon, then bound — or, when a service
//!    manager passed one in, adopted rather than bound.
//! 5. **`READY=1`**, once there is a socket to serve on, with a status naming it and the
//!    backend. The camera count follows it and does not gate it (see below).
//! 6. **Serving**, until the process is told to stop.
//!
//! The backend is constructed before all of it, because a `--profile` that does not parse
//! is a usage mistake and reporting it should not first take a lock, make a directory and
//! bind a socket. Nothing about constructing a backend opens a camera (D12).
//!
//! ## What this build does about stopping, and what it does not claim
//!
//! It installs handlers for SIGTERM and SIGINT and treats them as one request (AGENTS's
//! "SIGTERM ≡ SIGINT"), and it stops in the order [`daemon::shutdown`] states and argues:
//! tell the supervisor, cancel every open subscription **with a reason**, stop accepting,
//! drain what was already accepted under [`schema::limits::DAEMON_SHUTDOWN_DRAIN_MS`], join
//! housekeeping, release the state directory. The order is the claim; that each of those
//! happens at all is mostly older news.
//!
//! **What is still not claimed.** A cancellation reaches tokio tasks, and a camera actor is
//! an OS thread by construction (D12) — so a command parked in a device ioctl is ended by the
//! process exiting, not by the token, and the drain bound is what keeps that from making the
//! stop unbounded (`daemon::shutdown`'s residual, note **N59**). The socket file is not
//! unlinked: the exits that matter are the ones that run no code at all, and whoever starts
//! next removes it under the state lock (`daemon::uds::SocketDir::bind`). And an un-drained
//! exit is survivable rather than a hole for the reason it always was: Linux releases an
//! `flock` when the last descriptor on its open file description closes, so a *killed* `wchd`
//! releases the state lock, and the same sentence covers every camera descriptor its actors
//! held.
//!
//! ## The systemd half, which is now here
//!
//! Note **N58** split this sub-milestone; this is the other side of it, and everything that
//! paragraph used to defer is in [`daemon::systemd`]. This build passes
//! [`daemon::systemd::Supervisor`] — the real `sd_notify` implementation — rather than
//! `daemon::shutdown::Unsupervised`, and it does so **unconditionally**: `sd_notify::notify`
//! answers `Ok(())` with `$NOTIFY_SOCKET` unset, so a `wchd` started from a shell opens
//! nothing and says nothing, which is exactly what `Unsupervised` provided, while a `wchd`
//! started from a unit gets `READY=1`, a `STATUS=` an operator can read in `systemctl status`,
//! a watchdog ping if the unit asked for one, and `STOPPING=1` before the teardown takes any
//! time. Socket activation and the journald layer are the same story told about a listener
//! and a log line.
//!
//! **The camera count does not gate readiness.** `READY=1` goes out as soon as there is a
//! socket, and the number of cameras this machine had at startup arrives afterwards as a
//! second `STATUS=`, from a blocking thread. `daemon::systemd::publish_camera_count` argues
//! it: enumeration is device work, a startup that waited for it would be a startup whose
//! duration is a property of the hardware, and an enumeration that fails is a status line
//! rather than a daemon that will not start (E3, AGENTS rule 7).
//!
//! **This daemon still never self-daemonizes.** There is no `fork` anywhere in this
//! workspace, no `setsid`, no PID file, and no `Type=forking` unit — the process a supervisor
//! starts is the process that serves, which is what makes `NotifyAccess=main` sufficient,
//! what makes the pid in D9's lock record the pid systemd knows about, and what makes the
//! `SIGTERM` a `systemctl stop` sends arrive at the handler installed above. Since P4e-ii
//! that is a claim something checks rather than a sentence here:
//! `scripts/gates/systemd-units.sh` derives it from the shipped unit files (`Type=notify`,
//! never `Type=forking`, no `PIDFile=`), and `scripts/gates/socket-activation.sh` starts a
//! real `wchd` under `systemd-socket-activate` and asserts the process that was started is
//! the one holding the socket.
//!
//! `std::process::exit` is lint-banned in this workspace precisely so the release above stays
//! true: main returns an [`ExitCode`] and the stack unwinds, releasing the lock on the way.
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
use daemon::shutdown::{self, Notifying, Shutdown, Signals};
use daemon::state::OwnedState;
use daemon::systemd::{self, Activation, Supervisor};
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

    // **The first thing this process does with its environment, because it is the only thing
    // that writes it.** `listenfd` unsets `LISTEN_FDS`/`LISTEN_PID` as it reads them — the
    // protocol requires that, so a child cannot re-adopt the descriptors — and every other
    // read of the environment in this function happens after it (`daemon::systemd::Activation`
    // states the residual: the runtime's worker threads already exist).
    let activation = Activation::from_env()?;

    // **Before this process owns anything**, which is the whole reason it is here rather than
    // at the call that consumes it: registration is what turns a signal from the kernel's
    // default disposition (terminate now) into `daemon::shutdown`'s order, so every instant
    // between it and the exit is an instant in which a stop is a *drain*. Registering after
    // the socket was bound would leave a window in which a `systemctl stop` killed a daemon
    // that was already serving — small, survivable (the header says why), and avoidable for
    // the price of one line in a different place. Measured on this host: an un-handled SIGINT
    // in that window exits 130 and logs nothing.
    let signals = Signals::real()?;

    // D9's daemon protocol, and this binding's lifetime is the lock's: `state` lives until
    // `run` returns, which is until the process stops serving. Never blocks — a second
    // `wchd` is answered with `StoreLocked` naming the holder rather than waiting for a
    // lock that, by this protocol's definition, will not be free.
    let state = OwnedState::take(&env)?;

    // Two startup paths, one socket. The lock above is taken on both — an activated socket
    // says nothing about who owns the state directory — and everything below this match is
    // identical, which is what stops "the daemon under systemd" and "the daemon in a
    // terminal" from becoming two daemons.
    let (listener, socket) = match activation {
        Activation::Own => {
            let dir = SocketDir::prepare(&env)?;
            let listener = dir.bind(state.lock())?;
            (listener, dir.socket_path())
        }
        // Bound by the service manager before this process existed. Its directory's mode and
        // owner were checked with the same function the branch above uses, and
        // `daemon::systemd::Activation::adopt` has already logged which path this is —
        // including what N39's defence cannot do about a bind that was not ours.
        Activation::Inherited { listener, socket } => (listener, socket),
    };

    // Taken before the backend is moved into the server, and read twice: once in the line an
    // operator reads on stderr, once in the `STATUS=` an operator reads in `systemctl status`.
    // Two spellings of "which backend is this" would be two answers to it.
    let backend_name = backend.name();

    // The one line an operator looks for, and the one every subprocess test and shell
    // predicate in this project waits on. A socket path is not a frame and not derived from
    // one (see `daemon::logging`).
    tracing::info!(socket = %socket, backend = backend_name, "wchd is serving");

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
    //
    // The stop token is made here and nowhere else, and handed to both halves of the process:
    // the daemon that *watches* it (every open subscription, the idle-sweep driver) and the
    // teardown that *cancels* it. A clone is the same token, so there is one answer to "is
    // this daemon stopping" however many places ask.
    let shutdown = Shutdown::new();
    let wchd = Wchd::new(
        backend,
        SessionStore::new(state.store().root()),
        state.token(),
        shutdown.clone(),
    );
    // D12's other half, and since P4e-ii the handle is **held**: the driver observes the token
    // and ends itself, and `serve_until_stopped` joins it, so "housekeeping ended" is a fact
    // this process waited for rather than a consequence of the runtime being dropped.
    let housekeeping = wchd.spawn_idle_sweeps();
    let mut serving = uds::serve(listener, daemon::server::mount(wchd.clone())?);

    // The real supervisor, passed unconditionally: with `$NOTIFY_SOCKET` unset every one of
    // these is a no-op, so a `wchd` in a terminal behaves exactly as it did when this was
    // `Unsupervised` (`daemon::systemd::Supervisor` has the argument). Behind an `Arc`
    // because two other things hold it — the startup status task and, from here, the teardown
    // — and the seam is `&dyn` by design: the *order* these are sent in relative to the
    // teardown is a claim `daemon::shutdown` makes, and a claim about a side effect that
    // leaves the process needs a recording double to be assertable at all.
    let notify: Arc<dyn Notifying> = Arc::new(Supervisor::new());
    notify.ready(&format!("serving on {socket} ({backend_name})"));

    // Second, and deliberately not part of readiness: enumeration is device work, and a
    // daemon whose startup waited for a camera would fail to start on a host whose camera is
    // wedged. It is a startup fact rather than a live one and the status says so.
    systemd::publish_camera_count(wchd, Arc::clone(&notify));

    // Only when a unit asked for one (`WatchdogSec=`), which is `$WATCHDOG_USEC` being set.
    // Joined below rather than through `serve_until_stopped`, whose `housekeeping` parameter
    // is D12's driver and singular by signature; by the time that call returns the token this
    // task watches has been cancelled, so the await is a join and not a wait.
    let watchdog = systemd::spawn_watchdog(&shutdown);

    // The daemon's main loop and its whole teardown; `daemon::shutdown`'s header is the order
    // and the argument for it. The `?` is the reason this answers a `Result`: a daemon that
    // gave up on `accept` has stopped serving, and reporting that as a clean exit would tell
    // `Restart=on-failure` not to restart it and leave the operator a socket file with nobody
    // behind it.
    let served = shutdown::serve_until_stopped(
        &mut serving,
        &shutdown,
        signals,
        notify.as_ref(),
        housekeeping,
    )
    .await;

    if let Some(watchdog) = watchdog {
        // A ping into a socket the service manager is about to stop listening on is harmless;
        // a task still pinging after the process claimed to have stopped is not. Awaited for
        // the same reason housekeeping is: an ending nothing waited for is a maybe.
        if let Err(err) = watchdog.await {
            tracing::warn!(error = %err, "the watchdog task did not end cleanly");
        }
    }

    // Said out loud rather than left to the end of a scope, because the order is the claim:
    // the state directory is released *after* the server has stopped answering and after every
    // subscription has been told why it ended. It is the orderly half of a release the kernel
    // performs anyway when the process dies (see this file's header).
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
