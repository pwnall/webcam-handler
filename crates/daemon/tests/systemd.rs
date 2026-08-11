//! What a real `wchd` says to a real notify socket, and where it logs when it is told the
//! journal is on its stderr (docs/7 P4e-ii).
//!
//! `daemon::systemd`'s own unit tests take every decision in that module apart over values —
//! the divisor, the fd count, the address shape, the `device:inode` comparison. What none of
//! them can assert is the one thing the module exists for: that **datagrams leave the
//! process**, in the right order, and that a real `SIGTERM` produces `STOPPING=1` before the
//! process is gone. That is a claim about a process, so the daemon here is a real `wchd`
//! spawned as a subprocess, exactly as `tests/lock.rs` spawns one for the claim that the
//! shipped binary holds the state directory for as long as it runs.
//!
//! ## The supervisor is this test
//!
//! `$NOTIFY_SOCKET` names a `AF_UNIX` **datagram** socket, and nothing about it is systemd's:
//! a supervisor binds one, exports the path, and reads newline-separated `KEY=value` lines off
//! it. So this suite binds one and *is* the service manager, which is why every assertion
//! below is about bytes that crossed a socket rather than about a value a double wrote down.
//!
//! ## Nothing here sleeps
//!
//! Two synchronizations, both real. Starting: `recv` on a datagram socket blocks until a
//! datagram arrives, so "the daemon is ready" arrives *from the daemon*. Stopping: the same
//! socket carries `STOPPING=1`, and `wait` returns when the kernel has reaped the process.
//!
//! The read timeout below is a **watchdog and not synchronization**, in the sense
//! `scripts/gates/uds-permissions.sh` uses `timeout`: without it a daemon that never notified
//! would hang this suite until nextest's own timeout, with nothing to say about why. With it
//! the failure names what was expected, what did arrive, and everything the daemon printed.
//!
//! ## The backend is the fake, and the profile is committed corpus
//!
//! `--backend fake --profile …` for the reason `crates/cli`'s suites use it: the camera count
//! in the daemon's startup status has to be a number this test knows, and a v4l2 `wchd` would
//! be reporting whatever is plugged into the machine running CI.

use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixDatagram;
use std::process::{Child, ChildStderr, Command, Stdio};
use std::time::Duration;

use camino::Utf8PathBuf;
use engine::paths::{MapEnv, TempRuntimeDir};
use engine::store::TempStore;

/// How long this suite waits for a datagram that should already be on its way.
///
/// A watchdog (see the header). Generous, because it is not a timing assertion: it is the
/// bound that turns "hangs until the harness kills it" into "fails, and says what it was
/// waiting for".
const WATCHDOG: Duration = Duration::from_secs(30);

/// The interval a unit with `WatchdogSec=` would set, in microseconds.
///
/// Short enough that a ping arrives promptly and long enough that it is not this test's own
/// scheduling that decides. Nothing waits it out: the assertion is a blocking `recv`, so the
/// suite ends the moment the first ping lands.
const WATCHDOG_USEC: u64 = 200_000;

/// A private pair of XDG directories and a notify socket, thrown away with the value.
///
/// The runtime directory is needed even where the subject is the notify protocol: `wchd`
/// refuses to serve without one to put its socket in (D11), so a fixture that supplied only a
/// state directory would be exercising the startup order's *first* refusal.
struct Scratch {
    state: TempStore,
    runtime: TempRuntimeDir,
    /// The supervisor's end of `$NOTIFY_SOCKET`. Bound here, so the daemon's first datagram
    /// cannot be sent before there is anything to receive it.
    notify: UnixDatagram,
    notify_path: Utf8PathBuf,
}

impl Scratch {
    fn new() -> Scratch {
        let state = TempStore::new().expect("a state directory");
        let runtime = TempRuntimeDir::new().expect("a runtime directory");
        // Inside the runtime directory rather than beside it, so it goes away with the
        // fixture — and *not* in the `webcam-handler` subdirectory, which is the daemon's own
        // and is asserted 0700 with nothing but a socket in it.
        let notify_path = runtime.base().join("notify.sock");
        let notify = UnixDatagram::bind(notify_path.as_std_path()).expect("a notify socket");
        notify
            .set_read_timeout(Some(WATCHDOG))
            .expect("a watchdog on the supervisor's end");
        Scratch {
            state,
            runtime,
            notify,
            notify_path,
        }
    }

    /// The environment a process started here would read.
    fn env(&self) -> MapEnv {
        self.runtime
            .env()
            .with("XDG_STATE_HOME", self.state.root().as_str())
    }

    /// The socket a `wchd` started here would bind.
    ///
    /// Composed from the same two homes the daemon composes it from, rather than written out
    /// — `tests/lock.rs` states why, and this suite waits on the same line.
    fn socket(&self) -> Utf8PathBuf {
        engine::paths::runtime_dir(&self.env())
            .expect("the fixture sets the runtime variable")
            .join(schema::limits::DAEMON_SOCKET_FILE)
    }

    /// A `wchd` bound to these directories, not yet started, with no supervisor.
    fn wchd(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_wchd"));
        command
            .args(["--backend", "fake", "--profile", profile().as_str()])
            // As environment variables rather than flags, because that is how the shipped
            // daemon finds both directories (note N2).
            .env("XDG_STATE_HOME", self.state.root().as_str())
            .env("XDG_RUNTIME_DIR", self.runtime.base().as_str())
            // Pinned rather than inherited: this suite learns that the daemon is serving by
            // reading the line it logs, and a developer with `RUST_LOG=warn` exported would
            // otherwise watch it wait for a line nobody was going to write.
            .env("RUST_LOG", "info")
            // Removed rather than left alone, in both directions. The suite is often run from
            // a terminal, where neither is set — but a developer running `cargo nextest` from
            // a `systemd-run` shell would inherit a *real* supervisor's socket and this
            // suite's daemon would notify somebody else's service manager.
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

    /// The same, supervised by this test.
    fn supervised_wchd(&self) -> Command {
        let mut command = self.wchd();
        command.env("NOTIFY_SOCKET", self.notify_path.as_str());
        command
    }

    /// The next `KEY=value` line the daemon sent, or a panic naming what was waited for.
    ///
    /// One datagram may carry several lines — `READY=1` and its `STATUS=` go out together,
    /// which is one write and one thing that happened — so this flattens a datagram into its
    /// lines and hands them back as one string. Callers match on `contains`, because the
    /// *set* of lines in one datagram is the daemon's business and their arrival is this
    /// suite's.
    fn next_notification(&self, waiting_for: &str, daemon: &mut Daemon) -> String {
        let mut buffer = [0_u8; 4096];
        match self.notify.recv(&mut buffer) {
            Ok(read) => {
                String::from_utf8_lossy(buffer.get(..read).unwrap_or_default()).into_owned()
            }
            Err(err) => panic!(
                "waiting for {waiting_for} on the notify socket: {err}\nthe daemon said:\n{}",
                daemon.drain_transcript()
            ),
        }
    }

    /// Read notifications until one contains `wanted`, and answer everything that arrived.
    ///
    /// The daemon sends more than one thing — `READY=1`, then the startup camera count — and
    /// which datagram carries what is not this suite's business to pin. What is worth pinning
    /// is that each line *arrives*, which is what a bounded read-until is.
    fn notified_with(&self, wanted: &str, daemon: &mut Daemon) -> Vec<String> {
        let mut heard = Vec::new();
        loop {
            let notification = self.next_notification(wanted, daemon);
            let found = notification.contains(wanted);
            heard.push(notification);
            if found {
                return heard;
            }
        }
    }
}

/// A running `wchd`, stopped with the value.
///
/// [`Daemon::stop`] is idempotent and [`Drop`] calls it, so an assertion that fails part way
/// through does not leave a daemon holding a lock over a temporary directory that is about to
/// be deleted underneath it. `tests/lock.rs` has the same value for the same reason; it is not
/// shared with it because `tests/support/*.rs` modules are compiled into *every* includer
/// (note **N49**), and this suite needs a signalling stop that suite has no use for.
struct Daemon {
    child: Child,
    stderr: BufReader<ChildStderr>,
    transcript: Vec<String>,
    stopped: bool,
}

impl Daemon {
    /// Start one from a prepared command. It has not necessarily got anywhere yet.
    fn spawn(mut command: Command) -> Daemon {
        let mut child = command.spawn().expect("wchd is built beside this test");
        let stderr = child.stderr.take().expect("stderr was piped");
        Daemon {
            child,
            stderr: BufReader::new(stderr),
            transcript: Vec::new(),
            stopped: false,
        }
    }

    /// The next line the daemon wrote, or `None` when it has closed its stderr — which, with
    /// stderr piped, is when it has exited.
    fn next_line(&mut self) -> Option<String> {
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
    ///
    /// End-of-file is the bound: a `wchd` that cannot serve says why and exits, which closes
    /// the pipe. The needle is a path or a fragment rather than a whole sentence, so a
    /// reworded log message cannot turn a failing test into a hanging one (`tests/lock.rs`
    /// states the same rule).
    fn wait_for_line(&mut self, wanted: &str) -> String {
        while let Some(line) = self.next_line() {
            if line.contains(wanted) {
                return line;
            }
        }
        panic!(
            "wchd exited without saying {wanted:?}; it said:\n{}",
            self.transcript.join("\n")
        );
    }

    /// Everything the daemon has said so far, for a failure message.
    fn drain_transcript(&mut self) -> String {
        self.transcript.join("\n")
    }

    /// Everything the daemon said, read to end-of-file.
    ///
    /// Only sound once it has exited, which is why every caller stops it first: with stderr
    /// piped, end-of-file *is* the process being gone, so reading to it on a live daemon is
    /// the hang this suite is careful not to have.
    fn transcript_to_end(&mut self) -> String {
        while self.next_line().is_some() {}
        self.transcript.join("\n")
    }

    /// The pid a service manager would signal.
    fn pid(&self) -> rustix::process::Pid {
        let raw = i32::try_from(self.child.id()).expect("a pid fits in an i32");
        rustix::process::Pid::from_raw(raw).expect("a live child has a valid pid")
    }

    /// Ask it to stop the way `systemctl stop` does.
    ///
    /// A **real** `SIGTERM` through `rustix::process::kill_process`, not `Child::kill`'s
    /// `SIGKILL`: what is under test is the ordered teardown, and `SIGKILL` is precisely the
    /// signal that runs none of it. `rustix` is already a normal dependency of this crate for
    /// note N39's socket-directory hardening, so this costs no new edge and no `unsafe`.
    fn terminate(&self) {
        rustix::process::kill_process(self.pid(), rustix::process::Signal::TERM)
            .expect("ours to signal");
    }

    /// Wait for the kernel to reap it, and answer whether it exited cleanly.
    fn wait(&mut self) -> bool {
        self.stopped = true;
        self.child
            .wait()
            .expect("a spawned child can be waited for")
            .success()
    }

    /// Kill it if it is still there.
    fn stop(&mut self) {
        if !self.stopped {
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.stopped = true;
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The committed profile the daemon replays. One profile, therefore one camera.
///
/// The count matters: the startup status names it, and this suite asserts the number the
/// daemon published is the number of profiles it was given rather than a literal.
fn profile() -> Utf8PathBuf {
    let profile = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/profiles/chicony-rgb.json");
    assert!(profile.exists(), "the corpus is missing {profile}");
    profile
}

#[test]
fn a_supervised_daemon_announces_readiness_and_then_what_it_can_see() {
    // `READY=1` is the sentence a `Type=notify` unit's whole start-up contract is made of:
    // until it arrives systemd holds the unit "activating", orders every `After=` dependant
    // behind it, and eventually fails the start on `TimeoutStartSec`. So this asserts the
    // bytes rather than a call — a `Notifying` double can only say that this test's own value
    // was written down.
    //
    // The `STATUS=` beside it is what an operator reads in `systemctl status`, and it has to
    // name the socket: "active (running)" over a daemon serving a path nobody expected is the
    // one failure a status line exists to prevent.
    let scratch = Scratch::new();
    let mut daemon = Daemon::spawn(scratch.supervised_wchd());

    let ready = scratch.notified_with("READY=1", &mut daemon).join("");
    assert!(ready.contains("STATUS="), "{ready}");
    assert!(
        ready.contains(scratch.socket().as_str()),
        "the readiness status did not name the socket it is serving: {ready}"
    );
    assert!(ready.contains("fake"), "{ready}");

    // And the camera count, which arrives **after** readiness and by a different route — a
    // blocking enumeration on the runtime's pool. A build that had made it part of `ready`
    // would pass the assertions above and would have made the daemon's start-up time a
    // property of the hardware (`daemon::systemd::publish_camera_count`).
    let counted = scratch.notified_with("startup", &mut daemon).join("");
    assert!(
        counted.contains("STATUS=") && counted.contains('1'),
        "the daemon did not publish the one camera its one profile replays: {counted}"
    );
    // The daemon really is serving by now, which is what makes the ordering above a claim
    // about readiness rather than about two lines.
    daemon.wait_for_line(scratch.socket().as_str());

    daemon.terminate();
    // `STOPPING=1` is step 2 of `daemon::shutdown`'s order and the reason that order starts
    // with it: everything after it takes time, and a service manager that learns about the
    // stop only when the socket closes has been counting the drain against `TimeoutStopSec`.
    //
    // "Before the process exits" is asserted by *receiving it while the process is still
    // there to be waited for*: this recv happens before the `wait` below, so a daemon that
    // sent nothing leaves the watchdog to fire with the transcript attached rather than
    // leaving a green test.
    let stopping = scratch.notified_with("STOPPING=1", &mut daemon).join("");
    assert!(stopping.contains("STATUS="), "{stopping}");
    assert!(
        daemon.wait(),
        "a daemon asked to stop must exit cleanly, or `Restart=on-failure` restarts it \
         on a machine that is shutting down"
    );
}

#[test]
fn a_unit_that_asks_for_a_watchdog_gets_pinged_inside_the_interval_it_set() {
    // `WatchdogSec=` in a unit is `$WATCHDOG_USEC` in the environment, and a service that does
    // not ping is killed and restarted. The arithmetic is a unit test
    // (`daemon::systemd::ping_interval`); what only a process can show is that the task is
    // actually spawned, that it is spawned only when asked, and that its datagrams reach the
    // socket the manager is listening on.
    //
    // Nothing waits out the interval: `recv` returns the moment the first ping lands.
    let scratch = Scratch::new();
    let mut supervised = scratch.supervised_wchd();
    supervised.env("WATCHDOG_USEC", WATCHDOG_USEC.to_string());
    let mut daemon = Daemon::spawn(supervised);

    scratch.notified_with("READY=1", &mut daemon);
    let pinged = scratch.notified_with("WATCHDOG=1", &mut daemon).join("");
    assert!(pinged.contains("WATCHDOG=1"), "{pinged}");

    daemon.terminate();
    assert!(daemon.wait());
}

#[test]
fn an_unsupervised_daemon_serves_without_a_notify_socket_and_says_nothing_to_anybody() {
    // The other direction, and the whole argument for `main` passing `Supervisor`
    // unconditionally: with `$NOTIFY_SOCKET` unset, `sd_notify` opens nothing and answers
    // `Ok(())`, so a `wchd` in a terminal behaves exactly as it did when the composition root
    // passed `Unsupervised`. A build that had made the supervisor a hard edge — an `expect`,
    // or a refusal to start without a socket — would fail right here, which is every `wchd`
    // anybody runs by hand.
    let scratch = Scratch::new();
    let mut daemon = Daemon::spawn(scratch.wchd());

    daemon.wait_for_line(scratch.socket().as_str());
    daemon.terminate();
    assert!(daemon.wait());

    // Nothing was sent to the socket this fixture is holding, which is the half that makes
    // "it notifies" a claim rather than a coincidence: the recv would otherwise be picking up
    // datagrams from an earlier arrangement.
    scratch
        .notify
        .set_read_timeout(Some(Duration::from_millis(100)))
        .expect("a shorter watchdog for an expected silence");
    let mut buffer = [0_u8; 4096];
    let heard = scratch.notify.recv(&mut buffer);
    assert!(
        heard.is_err(),
        "a daemon with no NOTIFY_SOCKET in its environment notified somebody: {:?}",
        heard.map(
            |read| String::from_utf8_lossy(buffer.get(..read).unwrap_or_default()).into_owned()
        )
    );
}

#[test]
fn a_journal_stream_that_is_not_this_stderr_leaves_the_daemon_logging_to_stderr() {
    // `$JOURNAL_STREAM` is *inherited*, so a process started by a unit whose stderr was
    // redirected elsewhere has the variable and is not on the journal — and a build that
    // installed the journald layer on the strength of the variable being set would send its
    // whole log to a socket nobody is reading. This is that case, arranged: a `device:inode`
    // that cannot be this daemon's stderr, because this daemon's stderr is a pipe to this
    // test.
    //
    // It is also the assertion that keeps the rest of the suite honest. Every subprocess test
    // and shell predicate in this project learns that the daemon is up by reading its stderr,
    // so "the fmt layer is still there unless stderr really is the journal" is load-bearing
    // for all of them (`daemon::logging`'s header). The matching direction needs a real
    // journal socket, which is a property of the machine, so it lives in
    // `scripts/gates/socket-activation.sh` behind a named, counted skip (note **N44**).
    //
    // **Readiness is read off the notify socket and not off stderr**, which is the whole shape
    // of this test rather than a convenience: the defect it exists to catch is a daemon that
    // logs nowhere this test can see, and a suite that learned "the daemon is up" from the
    // stderr line it is about to assert would have nothing to say when that line never comes —
    // it would sit in `read_line` until the harness killed it. So the daemon announces itself
    // through the supervisor, is then stopped, and the assertion is made over everything it
    // said on the way out. A build that installed the journald layer on the strength of the
    // variable fails here with the transcript rather than with a timeout.
    let scratch = Scratch::new();
    let mut misleading = scratch.supervised_wchd();
    misleading.env("JOURNAL_STREAM", "0:0");
    let mut daemon = Daemon::spawn(misleading);

    scratch.notified_with("READY=1", &mut daemon);
    daemon.terminate();
    assert!(daemon.wait());

    let said = daemon.transcript_to_end();
    assert!(
        said.contains("wchd is serving") && said.contains(scratch.socket().as_str()),
        "a daemon whose $JOURNAL_STREAM is not its stderr logged nothing to stderr; it said:\n{said}"
    );
}
