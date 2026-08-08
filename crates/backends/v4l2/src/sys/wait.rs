//! Waiting for a frame, with a bound the caller chose.
//!
//! `Fd::open` deliberately omits `O_NONBLOCK`, so `VIDIOC_DQBUF` blocks until a buffer is
//! ready — which is the behaviour we want everywhere except in the one place it matters:
//! a camera that stops delivering would hold the actor thread forever, and "the camera is
//! wedged" would be indistinguishable from "the camera is slow" (E3). `poll` is what turns
//! the block into a deadline the caller set.
//!
//! The wait is *here* rather than in the settle policy on purpose. The policy is a pure
//! fold over `(frame arrived, what time is it)` on a steppable clock (design D5), and it
//! stays pure precisely because the blocking read is somebody else's problem. This module
//! is that somebody.

use std::time::Instant;

use schema::error::{Error, Result};

use super::Fd;

/// Wait until `fd` has a buffer ready, or until `deadline`.
///
/// `Ok(false)` means the deadline arrived first — an answer, not a failure, and the
/// distinction is E3's: a caller that treated a timeout as an error could not tell a slow
/// camera from a broken one.
///
/// # Errors
///
/// [`Error::DeviceGone`] when the device reports itself gone (`POLLHUP`/`POLLNVAL`, which
/// is what unplugging a USB camera mid-stream produces), and [`Error::DeviceIo`] for a
/// `poll` failure that is not an interruption. `EINTR` is retried against the same
/// deadline rather than surfaced: a signal is not news about the camera.
pub(crate) fn readable(fd: &Fd, deadline: Instant) -> Result<bool> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        // A deadline already spent means "do not block", not "block forever": `poll` reads
        // a negative timeout as infinite, which is the one value this must never pass, and
        // a `Duration` cannot be negative. Sub-millisecond remainders round *up*, so a
        // budget of half a millisecond still buys one attempt instead of being truncated
        // into a non-wait.
        let millis = remaining.as_millis()
            + u128::from(remaining.subsec_millis() * 1_000_000 != remaining.subsec_nanos());
        let timeout = i32::try_from(millis).unwrap_or(i32::MAX);

        let mut pollfd = libc::pollfd {
            fd: fd.raw(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `poll` reads and writes the array it is given for the declared number of
        // entries. `pollfd` is one live, exclusively borrowed, correctly aligned
        // `libc::pollfd`, the count passed is 1, and the binding outlives the call — so
        // the kernel writes exactly one `revents` field, inside this stack slot.
        let ret = unsafe { libc::poll(std::ptr::from_mut(&mut pollfd), 1, timeout) };

        if ret < 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EINTR) {
                // The deadline is recomputed at the top of the loop, so an interrupted
                // wait resumes against the *original* deadline rather than restarting it.
                continue;
            }
            return Err(Error::DeviceIo {
                operation: "poll".to_owned(),
                errno: error.raw_os_error(),
                message: error.to_string(),
            });
        }
        if ret == 0 {
            return Ok(false);
        }

        // `POLLHUP` on a video node is a camera that left. Reporting it here rather than
        // letting `DQBUF` answer `ENODEV` a moment later costs nothing and keeps the
        // diagnosis at the layer that saw it.
        if pollfd.revents & (libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err(Error::DeviceGone {
                path: fd.path().to_owned(),
            });
        }
        // `POLLERR` is what a V4L2 node raises for a dequeued buffer carrying an error
        // flag, among other things; it is not on its own a reason to stop, and the caller
        // finds out what it meant by dequeuing.
        return Ok(pollfd.revents & (libc::POLLIN | libc::POLLERR) != 0);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_deadline_already_spent_returns_without_blocking() {
        // The bug this rules out is a hang, so it is written as one that would hang: a
        // negative `poll` timeout blocks forever, and `Instant::now()` is already past a
        // deadline in the past. `/dev/null` is always readable, so the *other* direction
        // of this test would return `true` immediately either way — what is asserted is
        // that the call returns at all, promptly.
        let Ok(fd) = Fd::open(camino::Utf8Path::new("/dev/null")) else {
            return;
        };
        let started = Instant::now();
        let answer = readable(&fd, Instant::now() - Duration::from_secs(1));
        assert!(answer.is_ok(), "{answer:?}");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "an expired deadline blocked for {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_descriptor_with_nothing_on_it_times_out_and_one_with_something_does_not() {
        // Both directions, because "readable" is worthless as a function nobody has seen
        // say no — and a `poll` wrapper that answered `true` unconditionally would pass
        // every streaming test on a camera that works.
        //
        // A socket pair rather than a camera: it is the one thing in the standard library
        // that is a descriptor whose readability a test can turn on. `into_raw_fd` hands
        // ownership to `Fd`, which closes it, so there is no double close.
        use std::io::Write as _;
        use std::os::fd::IntoRawFd as _;

        let Ok((quiet, mut sender)) = std::os::unix::net::UnixStream::pair() else {
            return;
        };
        let quiet = Fd::from_raw_for_test(quiet.into_raw_fd());

        assert_eq!(
            readable(&quiet, Instant::now() + Duration::from_millis(20)),
            Ok(false),
            "a socket nobody has written to must not report itself readable"
        );

        sender.write_all(b"x").expect("write to a socket pair");
        assert_eq!(
            readable(&quiet, Instant::now() + Duration::from_secs(1)),
            Ok(true),
            "a socket with a byte waiting is readable"
        );
    }
}
