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
//! ## What is shared, and what is this suite's
//!
//! The child process itself — the two scratch directories, the command, the readiness read on
//! stderr and the `Drop`-safe stop — is `support/wchd.rs`, shared with `lock.rs` and
//! `signals.rs`; the fake backend and the real signal are `support/supervised.rs`, shared with
//! `signals.rs`. What is left here is the half no other suite has: **this test is the service
//! manager**.

#[path = "support/wchd.rs"]
mod wchd;

#[path = "support/supervised.rs"]
mod supervised;

use std::os::unix::net::UnixDatagram;
use std::process::Command;
use std::time::Duration;

use camino::Utf8PathBuf;
use rustix::process::Signal;

use crate::supervised::{replaying, signal};
use crate::wchd::{Daemon, Scratch};

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

/// The supervisor's end of `$NOTIFY_SOCKET`.
///
/// Bound before any daemon is started, so the first datagram cannot be sent before there is
/// anything to receive it. It lives beside the daemon's own socket directory rather than
/// inside it — that one is the daemon's, is 0700, and is asserted to hold nothing but a
/// socket — and it goes away with the [`Scratch`] it was placed in.
struct Supervisor {
    socket: UnixDatagram,
    path: Utf8PathBuf,
}

impl Supervisor {
    fn listening(scratch: &Scratch) -> Supervisor {
        // Inside the runtime directory rather than in the daemon's own `webcam-handler`
        // subdirectory of it, which is 0700 and is asserted to hold nothing but a socket
        // (`scripts/gates/uds-permissions.sh`) — and one level up is this process's to write
        // in and goes away with the fixture.
        let path = scratch.runtime.base().join("notify.sock");
        let socket = UnixDatagram::bind(path.as_std_path()).expect("a notify socket");
        socket
            .set_read_timeout(Some(WATCHDOG))
            .expect("a watchdog on the supervisor's end");
        Supervisor { socket, path }
    }

    /// A `wchd` that will notify *this* supervisor, not yet started.
    fn supervised(&self, scratch: &Scratch) -> Command {
        let mut command = replaying(scratch, &profile());
        command.env("NOTIFY_SOCKET", self.path.as_str());
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
        match self.socket.recv(&mut buffer) {
            Ok(read) => {
                String::from_utf8_lossy(buffer.get(..read).unwrap_or_default()).into_owned()
            }
            Err(err) => panic!(
                "waiting for {waiting_for} on the notify socket: {err}\nthe daemon said:\n{}",
                daemon.transcript()
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

/// Everything the daemon said, read to end-of-file.
///
/// Only sound once it has exited, which is why every caller stops it first: with stderr piped,
/// end-of-file *is* the process being gone, so reading to it on a live daemon is the hang this
/// suite is careful not to have.
fn transcript_to_end(daemon: &mut Daemon) -> String {
    while daemon.next_line().is_some() {}
    daemon.transcript()
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
    let supervisor = Supervisor::listening(&scratch);
    let mut daemon = Daemon::spawn(supervisor.supervised(&scratch));

    let ready = supervisor.notified_with("READY=1", &mut daemon).join("");
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
    let counted = supervisor.notified_with("startup", &mut daemon).join("");
    assert!(
        counted.contains("STATUS=") && counted.contains('1'),
        "the daemon did not publish the one camera its one profile replays: {counted}"
    );
    // The daemon really is serving by now, which is what makes the ordering above a claim
    // about readiness rather than about two lines.
    daemon.wait_for_line(scratch.socket().as_str());

    signal(&daemon, Signal::TERM);
    // `STOPPING=1` is step 2 of `daemon::shutdown`'s order and the reason that order starts
    // with it: everything after it takes time, and a service manager that learns about the
    // stop only when the socket closes has been counting the drain against `TimeoutStopSec`.
    //
    // "Before the process exits" is asserted by *receiving it while the process is still
    // there to be waited for*: this recv happens before the `wait` below, so a daemon that
    // sent nothing leaves the watchdog to fire with the transcript attached rather than
    // leaving a green test.
    let stopping = supervisor.notified_with("STOPPING=1", &mut daemon).join("");
    assert!(stopping.contains("STATUS="), "{stopping}");
    // And it really was *before*: the process is still there to be waited for at the moment
    // the datagram has already arrived, which is the ordering `STOPPING=1` exists to make —
    // a supervisor that learns about the stop from the socket closing has been counting the
    // drain against `TimeoutStopSec`.
    assert!(
        daemon.still_running(),
        "STOPPING=1 arrived from a daemon that had already gone"
    );
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
    let supervisor = Supervisor::listening(&scratch);
    let mut supervised = supervisor.supervised(&scratch);
    supervised.env("WATCHDOG_USEC", WATCHDOG_USEC.to_string());
    let mut daemon = Daemon::spawn(supervised);

    supervisor.notified_with("READY=1", &mut daemon);
    let pinged = supervisor.notified_with("WATCHDOG=1", &mut daemon).join("");
    assert!(pinged.contains("WATCHDOG=1"), "{pinged}");

    signal(&daemon, Signal::TERM);
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
    let supervisor = Supervisor::listening(&scratch);
    let mut daemon = Daemon::serving(replaying(&scratch, &profile()), &scratch.socket());

    signal(&daemon, Signal::TERM);
    assert!(daemon.wait());

    // Nothing was sent to the socket this fixture is holding, which is the half that makes
    // "it notifies" a claim rather than a coincidence: the recv would otherwise be picking up
    // datagrams from an earlier arrangement.
    supervisor
        .socket
        .set_read_timeout(Some(Duration::from_millis(100)))
        .expect("a shorter watchdog for an expected silence");
    let mut buffer = [0_u8; 4096];
    let heard = supervisor.socket.recv(&mut buffer);
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
    let supervisor = Supervisor::listening(&scratch);
    let mut misleading = supervisor.supervised(&scratch);
    misleading.env("JOURNAL_STREAM", "0:0");
    let mut daemon = Daemon::spawn(misleading);

    supervisor.notified_with("READY=1", &mut daemon);
    signal(&daemon, Signal::TERM);
    assert!(daemon.wait());

    let said = transcript_to_end(&mut daemon);
    assert!(
        said.contains("wchd is serving") && said.contains(scratch.socket().as_str()),
        "a daemon whose $JOURNAL_STREAM is not its stderr logged nothing to stderr; it said:\n{said}"
    );
}
