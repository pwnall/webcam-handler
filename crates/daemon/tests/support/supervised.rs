//! A `webcam-handler-daemon` a service manager would recognise: one that replays a known
//! device, and one that can be sent a real signal.
//!
//! Included by `systemd.rs` and `signals.rs`, and by nothing else — the two suites that put a
//! *supervisor's* half of the contract on the other end of a real process. `support/wchd.rs`
//! is the part all three subprocess suites share; this is the part these two do, and it is a
//! second module rather than three more items over there because note **N49** makes an item
//! with two users out of three a `dead_code` failure in the third.
//!
//! ## Why the backend is the fake here and not there
//!
//! `--backend fake --profile …` for the reason `crates/cli`'s suites use it: both of these
//! suites assert something about *what the daemon can see* — a camera count in a startup
//! status, a control swept to a value — and a v4l2 `webcam-handler-daemon` would be reporting
//! whatever is plugged into the machine running CI. `lock.rs` contends for a state directory
//! and needs no such thing, which is why the flag pair is here rather than in the shared
//! command.

use camino::Utf8Path;
use rustix::process::Signal;

use crate::wchd::{Daemon, Scratch};

/// A `webcam-handler-daemon` replaying `profile`, not yet started.
pub(crate) fn replaying(scratch: &Scratch, profile: &Utf8Path) -> std::process::Command {
    let mut command = scratch.wchd();
    command.args(["--backend", "fake", "--profile", profile.as_str()]);
    command
}

/// Send a real signal to a real process.
///
/// `rustix::process::kill_process` and not `Child::kill`'s `SIGKILL`: what these suites are
/// about is the ordered teardown, and `SIGKILL` is precisely the signal that runs none of it.
/// `rustix` is already a normal dependency of this crate for note N39's socket-directory
/// hardening, so this costs no new edge, no `libc` and no `unsafe`.
pub(crate) fn signal(daemon: &Daemon, signal: Signal) {
    let pid = rustix::process::Pid::from_raw(daemon.pid()).expect("a live child has a valid pid");
    rustix::process::kill_process(pid, signal).expect("ours to signal");
}
