//! D9's two locking protocols, where they actually meet.
//!
//! > Cross-process safety is one advisory `fd-lock` at the state-dir root: the daemon holds
//! > it exclusively for its lifetime; a daemonless `wch` takes it per mutating operation;
//! > `wch` finding it held reports "daemon owns the state (and likely the camera) — use
//! > wchc" rather than corrupting or blocking (D13).
//!
//! Both orderings are here, because each one can pass while the other is broken. A daemon
//! that never took the lock would still refuse a second daemon — the *first* one would
//! win on nothing, and the suite would be green — and a daemon that took it but reported
//! the wrong protocol would send an operator off to start a `wchc` against a `wch` that is
//! already finished.
//!
//! ## What is real here, and what is not
//!
//! The daemon is a **real `wchd` process**, because the fact being asserted is that the
//! shipped binary holds the state directory for as long as it runs — a claim about a
//! process, which an in-process fixture cannot make. The `wch` half is this test process
//! calling [`engine::store::SessionStore::with_lock`], which is the one door every mutating
//! `wch` verb goes through (`crates/cli/src/main.rs` states that invariant on the
//! executor); the same refusal reaching a *user* through the `wch` binary is asserted in
//! `crates/cli/tests/calibrate.rs`, where that binary is already driven as a subprocess.
//!
//! ## Nothing here sleeps
//!
//! Two processes are synchronized twice, both times by a real signal. Starting: the test
//! **reads a line from the daemon's stderr pipe**, so "the daemon is up" arrives from the
//! daemon rather than from a guess about how long it takes. Stopping: `wait` returns when the
//! kernel has reaped the process, which is after its descriptors are closed, which is when
//! the `flock` is gone. Both live in `support/wchd.rs`, which states the bound on each.

#[path = "support/wchd.rs"]
mod wchd;

use daemon::state::OwnedState;
use engine::store::{LockProtocol, SessionStore};
use schema::ErrorKind;
use schema::error::Error;

use crate::wchd::{Daemon, Scratch};

/// A store over the directory this fixture's environment resolves to.
///
/// Resolved through `from_env` rather than taken from a `TempStore` on purpose:
/// `engine::paths::state_dir` appends `webcam-handler` to `$XDG_STATE_HOME`, so the directory
/// a `wchd` locks is one level below the fixture's root. A contender built any other way
/// would be contending for a different file and would pass every assertion below by never
/// meeting anybody.
///
/// A local rather than a method on the shared [`Scratch`]: `systemd.rs` includes that module
/// too and never contends for the state directory, and note **N49** makes an item its
/// binaries do not all use a `dead_code` failure in the ones that do not.
fn store(scratch: &Scratch) -> SessionStore {
    SessionStore::from_env(&scratch.env()).expect("the fixture sets both variables")
}

/// A `wchd` over these directories, serving.
fn serving(scratch: &Scratch) -> Daemon {
    Daemon::serving(scratch.wchd(), &scratch.socket())
}

/// Take the lock the way every mutating `wch` verb does, and hand back what happened.
///
/// The body is empty on purpose. What is contended is the lock, not what runs under it, and
/// `with_lock` is the whole of D9's daemonless protocol — take, run, release — so calling it
/// with nothing inside asserts the protocol rather than a session document.
fn mutate(store: &SessionStore) -> schema::Result<()> {
    store.with_lock(|lock| {
        assert_eq!(lock.protocol(), LockProtocol::PerOperation);
        Ok(())
    })
}

#[test]
fn a_running_daemon_owns_the_state_directory_and_sends_a_wch_to_wchc() {
    // Ordering one: the daemon is there first. This is the case D9 writes the sentence for.
    let scratch = Scratch::new();
    let mut daemon = serving(&scratch);

    let err = mutate(&store(&scratch)).expect_err("the daemon owns the state directory");
    assert_eq!(err.kind(), ErrorKind::StoreLocked);
    let Error::StoreLocked {
        holder: Some(holder),
        protocol,
    } = &err
    else {
        panic!("a `wch` meeting a daemon must be told who has it: {err:?}");
    };
    // The typed facts, not the sentence: the pid is the daemon this test started, and the
    // protocol is the one that makes waiting pointless.
    assert_eq!(
        holder.pid,
        daemon.pid(),
        "the refusal named a process other than the daemon"
    );
    assert_eq!(holder.comm.as_deref(), Some("wchd"));
    assert_eq!(*protocol, Some(LockProtocol::HeldForLifetime));
    // And then the sentence, once, because a typed error nobody renders is a refusal
    // nobody acts on.
    assert!(
        err.to_string()
            .contains("daemon owns the state (and likely the camera) — use wchc"),
        "{err}"
    );

    // The inverse, and the whole reason a lifetime hold is survivable: the lock is the
    // daemon's for exactly as long as the daemon is.
    daemon.stop();
    mutate(&store(&scratch)).expect("a dead daemon holds nothing");
}

#[test]
fn a_daemon_that_meets_a_wchs_momentary_lock_refuses_to_start_and_does_not_advertise_wchc() {
    // Ordering two: the `wch` is there first. Nothing about this arrangement needs a second
    // process — two opens of one file are two `flock` contenders inside one process, which
    // is what `TempStore` exists to make available — so the contention is real and the test
    // is synchronous.
    let scratch = Scratch::new();
    let momentary = store(&scratch)
        .lock(LockProtocol::PerOperation)
        .expect("nothing holds a fresh state directory");

    let err = OwnedState::take(&scratch.env()).expect_err("a `wch` is mid-operation");
    assert_eq!(err.kind(), ErrorKind::StoreLocked);
    let Error::StoreLocked {
        holder: Some(holder),
        protocol,
    } = &err
    else {
        panic!("a daemon meeting a `wch` must be told who has it: {err:?}");
    };
    assert_eq!(
        holder.pid,
        i32::try_from(std::process::id()).expect("a pid fits in an i32")
    );
    assert_eq!(*protocol, Some(LockProtocol::PerOperation));
    // The other direction of the one decision `LockProtocol::advice` makes: a lock that
    // will be free in a moment is not a reason to go and start another program.
    let rendered = err.to_string();
    assert!(!rendered.contains("wchc"), "{rendered}");
    assert!(rendered.contains("free shortly"), "{rendered}");

    // The same refusal through the binary an operator runs. Read as one line rather than
    // waited on: a `wchd` that wrongly *started* would never exit, and a test that read to
    // end-of-file would hang instead of failing.
    let mut refused = Daemon::spawn(scratch.wchd());
    let said = refused
        .next_line()
        .expect("wchd says why it will not serve");
    assert!(said.contains("the state directory is locked"), "{said}");
    assert!(!said.contains(scratch.socket().as_str()), "{said}");
    refused.stop();

    // And released, the same daemon starts — so the refusal was about the lock and not
    // about this fixture.
    drop(momentary);
    serving(&scratch).stop();
}

#[test]
fn a_daemons_lifetime_hold_stops_the_mutating_protocol_and_leaves_reading_alone() {
    // What the daemon's hold must *not* break. D9 gives the daemon the state directory for
    // its whole life, and that is only survivable for the operator sitting at the same
    // machine because reading takes no lock at all — the law `crates/cli` states on
    // `calibrate_status`: "a `wch calibrate status` that refused while a daemon held the
    // lock would be a status verb nobody can use on the machine the sessions are on".
    let scratch = Scratch::new();
    let store = store(&scratch);

    // With no daemon anywhere, the daemonless protocol takes and releases, twice over,
    // leaving nothing held. This is the arm that would go red if a lifetime hold had leaked
    // into the per-operation path.
    mutate(&store).expect("nobody holds it");
    mutate(&store).expect("the first operation released it");
    assert!(
        store.holder().is_none(),
        "a per-operation lock outlived its operation"
    );

    let mut daemon = serving(&scratch);
    assert_eq!(
        mutate(&store).expect_err("the daemon owns it").kind(),
        ErrorKind::StoreLocked
    );
    // And the reads answer anyway, because they never ask for the lock.
    assert!(
        store
            .all_sessions()
            .expect("reading is not a state write")
            .is_empty()
    );
    // "While running" as an observation rather than an inference: a daemon that had died on
    // its way up would leave the lock free too, and this assertion would then be reporting the
    // wrong thing about the right file.
    assert!(daemon.still_running(), "the daemon is not there to hold it");
    assert!(store.holder().is_some(), "the daemon let go while running");

    daemon.stop();
    mutate(&store).expect("both protocols are usable again");
}

#[test]
fn the_daemons_own_lock_refuses_the_daemonless_protocol_rather_than_deadlocking() {
    // The trap `daemon::state`'s header names, pinned so it is a red test rather than a
    // paragraph. P4c routes the mutating verbs, and the shape every one of them will be
    // written from is `crates/cli`'s executor, where `store.with_lock(|lock| …)` is
    // correct — a `wch` process holds no lock. Copied into a handler here it compiles and
    // it does not hang, which is the part worth knowing: `flock` denies a second open file
    // description in the same process just as it denies another process's, and
    // `StoreLock::take` never blocks.
    //
    // So the daemon would answer its own client `StoreLocked`, naming its own pid and
    // advising `wchc` — to a caller that is already using `wchc`. That reads as a lock
    // leak, which is the wrong thing to go and look for.
    let scratch = Scratch::new();
    let held = OwnedState::take(&scratch.env()).expect("nobody holds it yet");

    let err = held
        .store()
        .with_lock(|_lock| Ok(()))
        .expect_err("the daemonless protocol against a lock this process already holds");

    assert_eq!(err.kind(), ErrorKind::StoreLocked);
    let Error::StoreLocked {
        holder: Some(holder),
        protocol,
    } = &err
    else {
        panic!("the refusal named nobody: {err:?}");
    };
    assert_eq!(
        holder.pid,
        i32::try_from(std::process::id()).expect("a pid fits in an i32"),
        "the refusal named a process other than this one"
    );
    assert_eq!(*protocol, Some(LockProtocol::HeldForLifetime));
    // And the sentence it would send over the wire, which is the reason this is a defect
    // rather than a curiosity: a client told to go and use `wchc` by the daemon `wchc`
    // talks to.
    assert!(err.to_string().contains("use wchc"), "{err}");

    // The token the daemon already has is the one that works, and it is the whole fix.
    held.store()
        .all_sessions()
        .expect("reading takes no lock at all");
    drop(held);
    store(&scratch)
        .with_lock(|_lock| Ok(()))
        .expect("released with the value");
}
