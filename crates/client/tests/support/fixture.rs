//! A private pair of XDG directories, a `webcam-handler-daemon` inside them, and one
//! `webcam-handler-client` invocation's output.
//!
//! Included by `wchc.rs` and `hardware.rs` — the two suites whose subject is the **shipped
//! pair of binaries** rather than a value. Everything here was `wchc.rs`'s until P4g, and
//! that file's header said a shared module would be note **N49**'s hazard "bought for
//! nothing". The second includer is what buys it: `hardware.rs` starts the same daemon in the
//! same way and reads the same three things back, and a second copy of the spawning, the
//! readiness read and the `Drop`-safe stop would be two answers to "what does *the daemon is
//! up* mean" in the two files most likely to disagree — which is exactly the reasoning
//! `crates/daemon/tests/support/wchd.rs` gives for existing at all.
//!
//! **Who includes this is load-bearing, which is why the list above is a list** (note
//! **N49**). A `#[path]`-included module is compiled into *every* binary that includes it and
//! this workspace builds warnings as errors, so an item neither includer nor this module
//! itself uses is a `dead_code` failure. That fixes the contents here at the **union of what
//! is reachable from both**, and it is why three things a reader might expect are deliberately
//! in `wchc.rs` instead: the corpus lookup (`profile`), the camera it replays (`REPLAYED`),
//! and the repository root the first of those is resolved against are all facts about
//! replaying a *document*, which is the one thing the hardware suite is not doing.
//!
//! ## The backend is the caller's, and that is the whole of the split
//!
//! [`Fixture::spawn`] takes the daemon's arguments rather than a profile name. `wchc.rs`
//! passes `--backend fake --profile …` because its assertions are about a committed document;
//! `hardware.rs` passes `--backend v4l2` because its assertions are about what is plugged into
//! this machine. Everything either of them needs *around* that decision — two temporary homes,
//! the environment the daemon resolves its socket from, the service-manager variables that
//! must not be inherited, and the readiness read — is identical, and is here.
//!
//! ## Nothing here sleeps
//!
//! Starting the daemon waits on a **line from its stderr pipe** — a read that ends when the
//! writer writes — which is `crates/daemon/tests/support/wchd.rs`'s bound for its reason: a
//! daemon that cannot serve says why and exits, closing the pipe. There is no duration in
//! this file.
//!
//! Nothing reads that pipe after the readiness line, which is a claim about volume rather
//! than an oversight: a whole hardware run — three cameras enumerated, three photos taken, a
//! five-sample sweep run and restored — leaves **466 bytes** in it (measured, 2026-08-11),
//! against a pipe buffer of 64 KiB. A daemon that logged its way to a full pipe would block
//! on the write and the suite would meet it as a nextest deadline rather than as a hang, so
//! the failure has a reader either way.

use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStderr, Command, Stdio};

use camino::Utf8PathBuf;
use engine::paths::TempRuntimeDir;
use engine::store::TempStore;
use serde_json::Value;

/// The `webcam-handler-client` binary these suites drive, built by cargo alongside them.
pub(crate) fn wchc() -> Command {
    Command::new(env!("CARGO_BIN_EXE_webcam-handler-client"))
}

/// The `webcam-handler-daemon` binary beside it.
///
/// `CARGO_BIN_EXE_*` exists only for the package that declares the binary, so this is a
/// sibling lookup rather than an environment variable — and a missing sibling is a panic
/// naming the command that produces it, because "the daemon was not built" and "the daemon
/// misbehaved" must not look the same from here.
fn wchd() -> Command {
    let wchc = Utf8PathBuf::from(env!("CARGO_BIN_EXE_webcam-handler-client"));
    let wchd = wchc
        .parent()
        .expect("the test binary's directory")
        .join("webcam-handler-daemon");
    assert!(
        wchd.exists(),
        "webcam-handler-daemon is not beside webcam-handler-client at {wchd}; this suite \
         drives the shipped daemon, so build \
         the workspace (`cargo nextest run --workspace`, which is what `just ci` runs) \
         rather than this package alone"
    );
    Command::new(wchd)
}

/// A private pair of XDG directories and, when a test asks for one, a daemon in them.
pub(crate) struct Fixture {
    runtime: TempRuntimeDir,
    /// The daemon's `$XDG_STATE_HOME`.
    ///
    /// `pub(crate)` rather than an accessor because both includers write a file into it — a
    /// photo the daemon delivers by path has to land *somewhere* the test can read and the
    /// repository cannot (AGENTS' "test captures go to gitignored scratch dirs"), and a
    /// directory that is deleted with this value is that place.
    pub(crate) state: TempStore,
}

impl Fixture {
    pub(crate) fn new() -> Fixture {
        Fixture {
            runtime: TempRuntimeDir::new().expect("a runtime directory"),
            state: TempStore::new().expect("a state directory"),
        }
    }

    /// The socket a daemon started here would bind.
    ///
    /// Composed from the two homes the daemon composes it from, never written out — the
    /// same rule `crates/daemon/tests/support/wchd.rs` follows, so a fixture cannot drift
    /// away from the path it is waiting for.
    pub(crate) fn socket(&self) -> Utf8PathBuf {
        schema::paths::runtime_dir(&self.runtime.env())
            .expect("the fixture sets the runtime variable")
            .join(schema::limits::DAEMON_SOCKET_FILE)
    }

    /// A `webcam-handler-client` bound to these directories, with the arguments given.
    fn wchc(&self, args: &[&str]) -> Command {
        let mut command = wchc();
        command
            // Environment variables rather than flags, because that is how the shipped
            // client finds the socket (note N2): a test that passed a path in would not be
            // exercising the resolution an operator gets.
            .env("XDG_RUNTIME_DIR", self.runtime.base().as_str())
            .args(args);
        command
    }

    /// Run `webcam-handler-client` and collect everything the process produced.
    pub(crate) fn run(&self, args: &[&str]) -> Ran {
        let output = self
            .wchc(args)
            .output()
            .expect("webcam-handler-client runs");
        Ran {
            code: output.status.code().unwrap_or(-1),
            stdout: output.stdout,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    /// Start a `webcam-handler-daemon` with the arguments given, and wait until it is serving.
    ///
    /// `args` is where the two includers differ and the only place they do — see the header.
    pub(crate) fn spawn(&self, args: &[&str]) -> Daemon {
        let socket = self.socket();
        let mut command = wchd();
        command
            .env("XDG_RUNTIME_DIR", self.runtime.base().as_str())
            .env("XDG_STATE_HOME", self.state.root().as_str())
            // Pinned rather than inherited: these suites learn that the daemon is up by
            // reading the line it logs, and a developer with `RUST_LOG=warn` exported would
            // otherwise watch it wait for a line nobody was going to write.
            .env("RUST_LOG", "info")
            // Removed in both directions: run from a `systemd-run` shell, these daemons
            // would otherwise notify somebody else's service manager and serve from
            // somebody else's listener.
            .env_remove("NOTIFY_SOCKET")
            .env_remove("LISTEN_FDS")
            .env_remove("LISTEN_PID")
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        let mut child = command
            .spawn()
            .expect("webcam-handler-daemon is built beside this test");
        let stderr = BufReader::new(child.stderr.take().expect("stderr was piped"));
        let mut daemon = Daemon {
            child,
            stderr,
            said: Vec::new(),
        };
        // The daemon announces the socket it is serving. Waiting for *that* line is a
        // readiness signal from the process rather than a guess about how long starting
        // takes; end-of-file without it means it refused to start, and the panic carries
        // everything it said.
        daemon.wait_for(socket.as_str());
        daemon
    }
}

/// What one `webcam-handler-client` invocation produced.
pub(crate) struct Ran {
    pub(crate) code: i32,
    /// Bytes, not a string: a photo with no `-o` **is** standard output.
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: String,
}

impl Ran {
    /// The answer, asserting the verb succeeded.
    pub(crate) fn ok(&self) -> &[u8] {
        assert_eq!(
            self.code, 0,
            "webcam-handler-client failed: {}",
            self.stderr
        );
        &self.stdout
    }

    /// The `--json` answer as a document.
    ///
    /// A [`Value`] and not the schema type it deserializes into, deliberately: what both
    /// includers ask of it is what the document *does not* carry — the two probe fields
    /// `report_probe` puts on standard error instead, and the payload a path delivery must
    /// not have ridden along with. A typed parse cannot see a field the type does not
    /// declare, which is the half of `--json` that a typed parse would stop checking.
    pub(crate) fn json(&self) -> Value {
        serde_json::from_slice(self.ok()).unwrap_or_else(|error| {
            panic!(
                "not a JSON document ({error}): {}",
                String::from_utf8_lossy(&self.stdout)
            )
        })
    }
}

/// A running `webcam-handler-daemon`, killed with the value.
pub(crate) struct Daemon {
    /// `pub(crate)` because one includer stops the daemon *deliberately* and reads what the
    /// kernel says about how it went — a hardware run's last claim is that the process which
    /// held a camera for it exited on purpose and by request. An accessor pair for the pid
    /// and the status would be two items the other includer never calls (note **N49**), and
    /// the field is read here by [`Daemon::wait_for`]'s teardown regardless.
    pub(crate) child: Child,
    stderr: BufReader<ChildStderr>,
    said: Vec<String>,
}

impl Daemon {
    pub(crate) fn wait_for(&mut self, wanted: &str) {
        loop {
            let mut line = String::new();
            match self.stderr.read_line(&mut line) {
                Ok(0) | Err(_) => panic!(
                    "webcam-handler-daemon exited without saying {wanted:?}; it said:\n{}",
                    self.said.join("\n")
                ),
                Ok(_) => {
                    self.said.push(line.trim_end().to_owned());
                    if line.contains(wanted) {
                        return;
                    }
                }
            }
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        // `SIGKILL`, deliberately: this is the teardown a *failed assertion* gets, and what
        // it has to guarantee is that no daemon is left holding a temporary directory that
        // is about to be deleted. An orderly stop is `crates/daemon/tests/signals.rs`'s
        // subject, not a thing a `Drop` here should wait for.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
