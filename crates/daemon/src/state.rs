//! The state directory, owned for the daemon's lifetime (D9).
//!
//! Design D9 gives the state directory one advisory `fd-lock` and two protocols over it:
//! "the daemon holds it exclusively for its lifetime; a daemonless `wch` takes it per
//! mutating operation". This module is the first half — [`OwnedState::take`] is the only
//! producer of [`LockProtocol::HeldForLifetime`] in the shipped tree, and holding the value
//! it returns *is* the lifetime.
//!
//! ## Why it is here rather than three lines in `main`
//!
//! Two reasons, and neither is tidiness. The lock is what licenses everything that follows
//! it — [`crate::uds::SocketDir::bind`] takes a `&StoreLock` by signature because a
//! leftover socket is only stale if no other daemon is alive — so the order in which the
//! daemon establishes ownership is a thing worth being able to *test*, and a composition
//! root is the one part of a binary an integration test cannot call. And what a second
//! `wchd` is answered with is a typed value, not a line on stderr: a test that asserted the
//! rendered sentence would pass on a build that refused for the wrong reason.
//!
//! ## What this crate deliberately does not do with the lock
//!
//! It never takes a second one, and it never calls `flock` itself. `engine::store` is the
//! one home for the lock (design §2.10), every mutating store method takes a `&StoreLock`
//! by signature, and the daemon's answer to "which lock" is to pass [`OwnedState::lock`] —
//! the one it already holds. In particular the daemon must never call
//! [`engine::store::SessionStore::with_lock`], which is D9's *daemonless* protocol.
//!
//! **What that mistake does is worse than hanging, which is why it is written down here.**
//! It does not deadlock: `flock` denies a second open file description in the same process
//! exactly as it denies another process's, `StoreLock::take` uses `try_write` and never
//! blocks, and the call returns in milliseconds with
//! [`schema::Error::StoreLocked`] — naming *this process's own pid* as the obstruction and
//! rendering D9's advice, "daemon owns the state (and likely the camera) — use wchc", to a
//! client that is already using the daemon. So the symptom is a client told to go and use
//! `wchc` against the `wchd` it is talking to, and an operator sent looking for a lock
//! leak. `the_daemons_own_lock_refuses_the_daemonless_protocol_rather_than_deadlocking` in
//! `tests/lock.rs` pins it, so the shape is a red test rather than a paragraph.
//!
//! Read verbs take no lock at all, under either protocol, and that is what makes a
//! lifetime hold survivable for the operator sitting at the same machine: `wch calibrate
//! status` and `wch calibrate list` keep working while `wchd` runs.

use engine::paths::Env;
use engine::store::{LockProtocol, SessionStore, StoreLock};
use schema::Result;

/// The state directory this process owns, and the lock that says so.
///
/// Dropping it releases the lock — the release is the *close* of the descriptor, so a
/// `wchd` that is killed releases it too (`engine::store::StoreLock`'s own paragraph is the
/// home of that argument). That is why P4b can exit without a signal handler and still
/// leave the state directory usable; P4e's "store-lock release" is about doing it in an
/// order, not about doing it at all.
#[derive(Debug)]
#[must_use = "dropping this releases the state directory, which is the daemon's to hold"]
pub struct OwnedState {
    store: SessionStore,
    lock: StoreLock,
}

impl OwnedState {
    /// Take D9's lock for this process's lifetime.
    ///
    /// The environment arrives as a parameter for `engine::paths`'s reason — the process
    /// environment cannot be mutated in-process without `unsafe` in Rust 2024, so a daemon
    /// that read `std::env` directly could not be tested against a state directory of the
    /// test's choosing, and could not be tested in parallel with anything.
    ///
    /// This never blocks. D9 is explicit that a held lock is *reported*, not waited on, and
    /// the daemon needs that as much as `wch` does: the holder it finds may be another
    /// `wchd` that will hold the lock until somebody stops it, and a daemon that waited for
    /// that would look like a daemon that hung on startup.
    ///
    /// # Errors
    ///
    /// [`schema::Error::StoreLocked`] when somebody else holds the state directory —
    /// carrying who, and which protocol they follow, so the refusal can say whether waiting
    /// would help. A second `wchd` is refused here, and this is the daemon's whole mutual
    /// exclusion: there is no pid file and no "is a daemon running" probe, because the lock
    /// is the only answer that cannot go stale.
    /// [`schema::Error::StorageIo`] when the state directory cannot be resolved or the lock
    /// file cannot be opened.
    pub fn take(env: &dyn Env) -> Result<OwnedState> {
        let store = SessionStore::from_env(env)?;
        let lock = store.lock(LockProtocol::HeldForLifetime)?;
        Ok(OwnedState { store, lock })
    }

    /// The session store rooted at the directory this process owns.
    #[must_use]
    pub fn store(&self) -> &SessionStore {
        &self.store
    }

    /// The held lock, as the token every mutating store method asks for.
    ///
    /// Handing it out rather than hiding it is the point: `engine::store` expresses "under
    /// the lock" as a `&StoreLock` argument, so a daemon that passes this one is provably
    /// writing under the lock it took at startup rather than under a second one it forgot
    /// to release.
    #[must_use]
    pub fn lock(&self) -> &StoreLock {
        &self.lock
    }
}

#[cfg(test)]
mod tests {
    use engine::paths::MapEnv;
    use engine::store::TempStore;
    use schema::error::Error;
    use schema::{ErrorKind, Result};

    use super::*;

    /// An environment whose `$XDG_STATE_HOME` is a throw-away directory.
    ///
    /// `SessionStore::from_env` appends `webcam-handler` to it, so the store this produces
    /// is rooted one level below `temp.root()` — which is exactly what the shipped daemon
    /// does with a real `$XDG_STATE_HOME`, and why the test drives the environment rather
    /// than handing a path in.
    fn env_over(temp: &TempStore) -> MapEnv {
        MapEnv::from_pairs(&[("XDG_STATE_HOME", temp.root().as_str())])
    }

    /// A store over the directory `env_over` resolves to, for a second contender.
    fn peer(env: &MapEnv) -> Result<SessionStore> {
        SessionStore::from_env(env)
    }

    #[test]
    fn taking_the_state_directory_holds_it_until_the_value_is_dropped() {
        // The claim in one test: a daemon owns the directory for as long as it exists, and
        // for no longer. Both halves matter — a hold that outlived the process would make
        // the state directory unusable until a reboot, and one that did not last would let
        // a second `wchd` in.
        let temp = TempStore::new().expect("a state directory");
        let env = env_over(&temp);
        let store = peer(&env).expect("a resolvable state directory");

        let held = OwnedState::take(&env).expect("nobody holds it yet");
        assert_eq!(held.lock().protocol(), LockProtocol::HeldForLifetime);
        assert_eq!(held.store().root(), store.root());
        assert!(
            store.holder().is_some(),
            "the daemon took the lock without leaving a record of it"
        );

        drop(held);
        assert!(store.holder().is_none(), "the lock outlived the daemon");
        drop(
            OwnedState::take(&env).expect("released with the value, so the next daemon can start"),
        );
    }

    #[test]
    fn a_second_daemon_is_refused_by_the_lock_and_told_who_has_it() {
        // No pid file, no "is a daemon running" probe: the lock is the mutual exclusion,
        // and the refusal names the holder because that is what an operator checks.
        let temp = TempStore::new().expect("a state directory");
        let env = env_over(&temp);

        let first = OwnedState::take(&env).expect("nobody holds it yet");
        let err = OwnedState::take(&env).expect_err("the first daemon owns it");
        assert_eq!(err.kind(), ErrorKind::StoreLocked);
        let Error::StoreLocked {
            holder: Some(holder),
            protocol,
        } = &err
        else {
            panic!("a second daemon must be told who has the state directory: {err:?}");
        };
        assert_eq!(
            holder.pid,
            i32::try_from(std::process::id()).expect("a pid fits")
        );
        assert_eq!(*protocol, Some(LockProtocol::HeldForLifetime));

        drop(first);
        drop(OwnedState::take(&env).expect("the first daemon let go"));
    }

    #[test]
    fn without_a_state_directory_there_is_nothing_to_own() {
        // `engine::paths` owns this refusal; what is asserted here is that the daemon asks
        // it rather than guessing a directory to put a lock file in.
        let err = OwnedState::take(&MapEnv::empty()).expect_err("nothing is set");
        assert_eq!(err.kind(), ErrorKind::StorageIo);
        assert!(err.to_string().contains("XDG_STATE_HOME"), "{err}");
    }
}
