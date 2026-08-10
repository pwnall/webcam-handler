//! Asking one process to let go of a camera (design §5, D10).
//!
//! This is the second half of [`crate::holders`]: that module answers *who* has the node,
//! and this one is the only way this workspace has of asking one of them to stop. It is
//! deliberately three lines of syscall with no policy in it — which pid, and whether it may
//! be signalled at all, are decisions `holders::terminate` and the daemon's routing make
//! before anything here runs.
//!
//! ## Why a `kill(2)` lives under a V4L2 backend's `sys/`
//!
//! Because there is nowhere else it *can* live, and because the other half is already here.
//! `kill(2)` has no safe wrapper in `std` — `std::process::Child::kill` signals a process
//! this one forked, which a camera's holder is not — so it needs either an `unsafe` block
//! or a new dependency. Every crate but this one is `#![forbid(unsafe_code)]` and
//! `scripts/gates/unsafe-scope.sh` confines the token to this directory, so the `unsafe`
//! option has exactly one address. Note **N48** records the alternative that was weighed
//! (a permissive syscall crate) and why it lost for two calls.
//!
//! The smell — signalling is not V4L2 — is real and is answered the same way
//! [`crate::holders`]'s own header answers it: the `/proc` walk is not V4L2 either, and it
//! lives here because this is the crate that produces [`schema::Error::Busy`] and has to be
//! able to name the process in it. Diagnosis and the one action available on a diagnosis
//! belong together; splitting them would put the two halves of one verb in two crates.

use std::os::raw::c_int;

use schema::error::{Error, Result};

/// `SIGTERM` — the only signal this product sends, and the only member of
/// [`schema::report::TerminationSignal`].
///
/// Named here rather than used inline so the one place the number enters the workspace is
/// the one place the closed vocabulary is honoured: a build that could reach `SIGKILL`
/// would need a second constant, which is a diff somebody has to write on purpose.
const SIGTERM: c_int = libc::SIGTERM;

/// Send `SIGTERM` to `pid`, and to nothing else.
///
/// # Errors
///
/// [`Error::IllegalTransition`] for a pid that is not a single process. This is not
/// defensive tidying: `kill(2)` gives non-positive pids **broadcast** meanings — `0` is
/// every process in this process group, `-1` is every process this uid may signal, and
/// anything below that is a process group — so a `pid` that arrived from a socket and was
/// passed through unchecked would turn design §5's "an explicit command naming its target"
/// into a way to kill everything the daemon can reach. The refusal is here, at the syscall,
/// rather than only at the caller, because this is the boundary where the meaning changes.
///
/// [`Error::PermissionDenied`] for `EPERM` — a process this uid may not signal — which is
/// an *availability* answer and must never be converted into "the process is gone" (E3,
/// AGENTS rule 7). [`Error::HolderGone`] for `ESRCH`: the process was there when the walk
/// looked and is not there now, which is the exact race the caller's re-verification
/// narrows and cannot close, so it is answered in the same words a caller already has to
/// handle. [`Error::DeviceIo`] for anything else, naming the errno.
pub(crate) fn term(pid: i32) -> Result<()> {
    if pid <= 0 {
        return Err(Error::IllegalTransition {
            from: format!("not_one_process({pid})"),
            op: "signal a camera's holder; kill(2) reads a pid of 0 or less as a process \
                 group or as every process this user may signal, and this command names \
                 one target"
                .to_owned(),
        });
    }

    // SAFETY: `kill(2)` takes two integers by value and returns an integer. It reads no
    // pointer, borrows nothing, and allocates nothing, so there is no lifetime, alignment
    // or aliasing obligation to discharge — the whole of the obligation is that `pid` and
    // `sig` are values the kernel is willing to interpret, and it validates both itself,
    // answering `ESRCH`/`EINVAL`/`EPERM` rather than misbehaving. The one interpretation
    // that is a hazard rather than an error — a non-positive `pid` meaning "a group" — is
    // refused above, before this block, so the process this call can reach is exactly the
    // one named.
    let sent = unsafe { libc::kill(pid, SIGTERM) };
    if sent == 0 {
        return Ok(());
    }

    let errno = std::io::Error::last_os_error();
    match errno.raw_os_error() {
        // The holder exited between the walk that found it and this call. Nothing was
        // signalled, and the honest answer is the one the caller already renders for a pid
        // that is not a holder.
        Some(libc::ESRCH) => Err(Error::HolderGone { pid }),
        // Another user's process, or one this uid may not signal. Availability, not
        // capability: converting this into `HolderGone` would tell a caller the process had
        // exited when it is running and holding their camera.
        Some(libc::EPERM) => Err(Error::PermissionDenied {
            path: camino::Utf8PathBuf::from(format!("/proc/{pid}")),
            hint: "this process belongs to another user; signal it from an account that may, \
                   or stop the program yourself"
                .to_owned(),
        }),
        code => Err(Error::DeviceIo {
            operation: format!("send SIGTERM to pid {pid}"),
            errno: code,
            message: errno.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use schema::ErrorKind;

    use super::*;

    #[test]
    fn a_pid_that_is_not_one_process_is_refused_before_the_syscall() {
        // The three broadcast spellings `kill(2)` accepts, none of which this command has
        // any business reaching. Nothing is sent — which this test can only claim by
        // construction, since a broadcast that *was* sent would take the test runner with
        // it; the value of the arm is that it runs at all.
        for pid in [0, -1, -42] {
            let refused = term(pid).expect_err("a broadcast pid");
            assert_eq!(refused.kind(), ErrorKind::IllegalTransition);
            assert!(refused.to_string().contains(&pid.to_string()), "{refused}");
        }
    }

    #[test]
    fn a_pid_nothing_answers_to_is_gone_rather_than_a_kernel_failure() {
        // The `ESRCH` arm, which is also the pid-reuse race's landing place. `pid_max` is
        // at most 2^22 on Linux, so a pid past it can never name a process; using an
        // impossible number rather than a merely-unused one keeps the arm from becoming
        // flaky on a machine that happened to recycle into it.
        let refused = term(i32::MAX).expect_err("no process has this pid");
        assert_eq!(refused.kind(), ErrorKind::HolderGone);
        assert_ne!(
            refused.kind(),
            ErrorKind::DeviceIo,
            "a pid nobody answers to is not the kernel failing"
        );
    }

    #[test]
    fn the_only_signal_this_build_can_send_is_the_one_the_vocabulary_has() {
        // `TerminationSignal` is closed with one member so that "which signal" is a product
        // decision rather than a wire field; this is the other end of that sentence — the
        // number this module reaches for is `SIGTERM` and there is no path to `SIGKILL`.
        assert_eq!(SIGTERM, libc::SIGTERM);
        assert_ne!(SIGTERM, libc::SIGKILL);
        assert_eq!(
            schema::report::TerminationSignal::ALL,
            &[schema::report::TerminationSignal::Term]
        );
    }
}
