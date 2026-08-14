//! One real `webcam-handler-daemon`, in a real process.
//!
//! Included by `lock.rs`, `systemd.rs` and `signals.rs` — the three suites whose subject is a
//! **process** rather than a surface: that the shipped binary holds the state directory for
//! as long as it runs (D9), that datagrams leave it for a service manager, and that a real
//! signal makes it tear down in an order (docs/7 P4e-ii). None of those is a claim an
//! in-process fixture can make, which is why all three fork a child, and until P4e-ii's third
//! task each of them carried its own copy of the spawning, the readiness read and the
//! `Drop`-safe stop. Three copies of one arrangement in the three files most likely to
//! disagree about what "the daemon is up" means is what this module exists to stop.
//!
//! **Who includes this is load-bearing, which is why the list above is a list** (note
//! **N49**). A `#[path]`-included module is compiled into *every* binary that includes it and
//! this workspace builds warnings as errors, so an item used by only some of them is a
//! `dead_code` failure in the rest. That fixes the contents of this module at the
//! **intersection** of what the three suites need, and it is why three things a reader might
//! expect here are deliberately somewhere else: the notify socket is `systemd.rs`'s alone,
//! what `systemd.rs` and `signals.rs` share beyond this is `support/supervised.rs`, and a
//! [`engine::store::SessionStore`] over the directory the daemon locks is a local of the two
//! suites that contend for it.
//!
//! ## Nothing here sleeps
//!
//! Starting: the test **reads a line from the daemon's stderr pipe**, and a read on a pipe
//! blocks until the writer writes — so "the daemon is up" arrives from the daemon rather than
//! from a guess about how long starting takes. The bound on that read is end-of-file: a
//! `webcam-handler-daemon` that cannot serve says why and exits, which closes the pipe, and
//! the wait fails with everything the daemon printed. This is the same bound
//! `crates/engine/tests/ crash_recovery.rs` accepts for the same reason, and it is why a line
//! is matched on a **path or a fragment** rather than on a sentence — a reworded log message
//! must not be able to turn a failing test into a hanging one.
//!
//! Stopping: `wait` returns when the kernel has reaped the process, which is after its
//! descriptors are closed, which is when the `flock` is gone.

use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStderr, Command, Stdio};

use camino::{Utf8Path, Utf8PathBuf};
use engine::paths::TempRuntimeDir;
use engine::store::TempStore;
use schema::paths::MapEnv;

/// A private pair of XDG directories, thrown away with the value.
///
/// Both are needed even by a suite that is about neither: `webcam-handler-daemon` refuses to
/// serve without a runtime directory to put its socket in (D11) and without a state directory
/// to lock (D9), so a fixture that supplied one would be testing the startup order's *first*
/// refusal instead of whatever it came for.
pub(crate) struct Scratch {
    state: TempStore,
    /// The `$XDG_RUNTIME_DIR` the daemon puts its socket directory inside.
    ///
    /// `pub(crate)` because `systemd.rs` binds its notify socket beside the daemon's own
    /// `webcam-handler` subdirectory of it, and an accessor for that would be an item the
    /// other two includers do not call (note **N49**). The field itself is read here, by
    /// every includer, through [`Scratch::env`] and [`Scratch::wchd`].
    pub(crate) runtime: TempRuntimeDir,
}

impl Scratch {
    pub(crate) fn new() -> Scratch {
        Scratch {
            state: TempStore::new().expect("a state directory"),
            runtime: TempRuntimeDir::new().expect("a runtime directory"),
        }
    }

    /// The environment a process started here would read.
    pub(crate) fn env(&self) -> MapEnv {
        self.runtime
            .env()
            .with("XDG_STATE_HOME", self.state.root().as_str())
    }

    /// The socket a `webcam-handler-daemon` started here would bind.
    ///
    /// Composed from the same two homes the daemon composes it from —
    /// `schema::paths::runtime_dir` and `schema::limits::DAEMON_SOCKET_FILE` — rather than
    /// written out, so a fixture cannot drift away from the thing it is waiting for.
    /// Deliberately not `daemon::uds::SocketDir::prepare`, which would *create* the directory
    /// and so answer the question the daemon's own startup check exists to ask.
    pub(crate) fn socket(&self) -> Utf8PathBuf {
        schema::paths::runtime_dir(&self.env())
            .expect("the fixture sets the runtime variable")
            .join(schema::limits::DAEMON_SOCKET_FILE)
    }

    /// A `webcam-handler-daemon` bound to these directories, not yet started, with nobody
    /// supervising it.
    ///
    /// The backend is whatever `webcam-handler-daemon` defaults to, because that is the only
    /// choice all three includers agree on: `lock.rs` contends for a state directory and does
    /// not care what is plugged in, while `systemd.rs` and `signals.rs` need a camera count
    /// they know and add `--backend fake --profile …` through `support/supervised.rs`.
    pub(crate) fn wchd(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_webcam-handler-daemon"));
        command
            // As environment variables rather than flags, because that is how the shipped
            // daemon finds both directories (note N2) — a test that passed paths in would not
            // be exercising the resolution an operator gets.
            .env("XDG_STATE_HOME", self.state.root().as_str())
            .env("XDG_RUNTIME_DIR", self.runtime.base().as_str())
            // Pinned rather than inherited: these suites learn that the daemon is serving by
            // reading the line it logs, and a developer with `RUST_LOG=warn` exported would
            // otherwise watch them wait for a line nobody was going to write.
            .env("RUST_LOG", "info")
            // Removed rather than left alone, in both directions. The suites are usually run
            // from a terminal, where none of these is set — but a developer running `cargo
            // nextest` from a `systemd-run` shell would inherit a *real* service manager's
            // socket and descriptors, and these daemons would notify somebody else's manager
            // and serve from somebody else's listener.
            .env_remove("NOTIFY_SOCKET")
            .env_remove("WATCHDOG_USEC")
            .env_remove("WATCHDOG_PID")
            .env_remove("JOURNAL_STREAM")
            .env_remove("LISTEN_FDS")
            .env_remove("LISTEN_PID")
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        command
    }
}

/// A running `webcam-handler-daemon`, stopped with the value.
///
/// [`Daemon::stop`] is idempotent and [`Drop`] calls it, so an assertion that fails part way
/// through does not leave a daemon holding a lock — and, more to the point, holding it over a
/// temporary directory that is about to be deleted underneath it.
pub(crate) struct Daemon {
    child: Child,
    stderr: BufReader<ChildStderr>,
    transcript: Vec<String>,
    stopped: bool,
}

impl Daemon {
    /// Start one from a prepared command. It has not necessarily got anywhere yet.
    pub(crate) fn spawn(mut command: Command) -> Daemon {
        let mut child = command
            .spawn()
            .expect("webcam-handler-daemon is built beside this test");
        let stderr = child.stderr.take().expect("stderr was piped");
        Daemon {
            child,
            stderr: BufReader::new(stderr),
            transcript: Vec::new(),
            stopped: false,
        }
    }

    /// Start one and wait until it has bound `socket`.
    ///
    /// The daemon announces the socket it is serving, so the line carrying that path is the
    /// event this waits for — an announcement from the process rather than a guess about how
    /// long starting takes. End-of-file without it means the daemon refused to start, and the
    /// panic carries everything it said.
    pub(crate) fn serving(command: Command, socket: &Utf8Path) -> Daemon {
        let mut daemon = Daemon::spawn(command);
        daemon.wait_for_line(socket.as_str());
        daemon
    }

    /// The next line the daemon wrote, or `None` when it has closed its stderr — which, with
    /// stderr piped, is when it has exited.
    pub(crate) fn next_line(&mut self) -> Option<String> {
        let mut line = String::new();
        match self.stderr.read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => {
                self.transcript.push(line.trim_end().to_owned());
                Some(line)
            }
        }
    }

    /// Read stderr until `wanted` appears on a line; panic at end-of-file without it.
    pub(crate) fn wait_for_line(&mut self, wanted: &str) -> String {
        while let Some(line) = self.next_line() {
            if line.contains(wanted) {
                return line;
            }
        }
        panic!(
            "webcam-handler-daemon exited without saying {wanted:?}; it said:\n{}",
            self.transcript()
        );
    }

    /// Everything the daemon has said so far, for a failure message.
    pub(crate) fn transcript(&self) -> String {
        self.transcript.join("\n")
    }

    /// The pid a lock record names and a service manager signals.
    pub(crate) fn pid(&self) -> i32 {
        i32::try_from(self.child.id()).expect("a pid fits in an i32")
    }

    /// Whether it is still there, asked without waiting for it.
    ///
    /// The observation that turns "the daemon was still running when this happened" from an
    /// inference into an assertion — which matters most where the *next* line is about
    /// something the daemon does on its way out: a dead daemon has released its lock and sent
    /// its last datagram too, and both would read as success.
    ///
    /// `try_wait` reaps the child when it answers `Some`, and [`Daemon::wait`] still answers
    /// afterwards: `std::process::Child` keeps the status it collected.
    pub(crate) fn still_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Wait for the kernel to reap it, and answer whether it exited cleanly.
    pub(crate) fn wait(&mut self) -> bool {
        self.stopped = true;
        self.child
            .wait()
            .expect("a spawned child can be waited for")
            .success()
    }

    /// Kill it if it is still there, and wait for the kernel to reap it.
    ///
    /// `SIGKILL` through `Child::kill`, deliberately, even though this build installs handlers
    /// for the two signals that would tear down in an order (`daemon::shutdown`): this is the
    /// teardown a *failed assertion* gets, and what it has to guarantee is that no daemon is
    /// left holding a temporary directory that is about to be deleted. An orderly stop is a
    /// thing `signals.rs` asserts, not a thing a `Drop` should be waiting for.
    pub(crate) fn stop(&mut self) {
        if !self.stopped {
            let _ = self.child.kill();
            let _ = self.wait();
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        self.stop();
    }
}
