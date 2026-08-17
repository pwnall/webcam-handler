//! The unsafe boundary (design §2.5, rubric B10).
//!
//! Every `unsafe` block in the workspace lives under this directory, and
//! `scripts/gates/unsafe-scope.sh` derives that claim from the tree rather than trusting
//! it. What is here: an owned file descriptor, an aligned payload buffer, the typed
//! ioctl calls, and — since P4d — the uevent netlink socket. What is deliberately *not*
//! here: any interpretation of what the kernel said.
//!
//! **The rule is "kernel-facing", not "says `unsafe`".** docs/7 P4d places the uevent
//! socket "inside `src/sys/` like every unsafe **or kernel-facing** edge", and design
//! §2.5 says the same. [`uevent`] contains no `unsafe` block at all, because `rustix` is
//! a safe wrapper whose `sockaddr_nl` comes from `linux-raw-sys` (bindgen output over the
//! kernel UAPI headers, which is what "no hand-declared kernel structs" asks for). It
//! belongs here anyway, and the count below is unchanged by it.
//!
//! ## The shape, and why it is this shape
//!
//! The kernel's answers cross into this crate as **bytes**, and [`decode`] turns bytes
//! into schema values with no `unsafe` at all — field offsets come from
//! [`core::mem::offset_of!`] over the bindgen structs, so the layout is *derived from the
//! generated bindings* and never transcribed (design §2.5 bans hand-declared kernel
//! structs; this keeps the ban true without needing the hand-copy escape hatch).
//!
//! Three things fall out of that:
//!
//! - **Miri has a real population.** [`decode`]'s units are pure functions over captured
//!   byte fixtures, so `scripts/miri.sh` executes the decoding half of this module
//!   without a device. Miri cannot cross an ioctl, and this is the split that gives it
//!   something true to say (docs/9's recorded limit).
//! - **Union reads stop being an obligation.** `v4l2_querymenu`'s union is two readings
//!   of the same 32 bytes; choosing between them by control type is a decoding decision
//!   made over a byte slice, not an `unsafe` field access that has to prove which arm was
//!   written.
//! - **Device-derived numbers are validated in safe code**, where the validation is
//!   ordinary and testable, before any of them reaches a length or an index.
//!
//! ## The residual `unsafe`, counted
//!
//! Eleven blocks and one `unsafe impl`. Each block performs **one unsafe operation** —
//! `clippy::multiple_unsafe_ops_per_block` is denied, so that half is enforced rather than
//! claimed — and each row below states what that operation's obligation is, which is a
//! sentence and not a lint. Two rows take two clauses to state one obligation, and that is
//! the honest shape: `ioctl::call`'s pointer is valid *and* the one struct it carries with
//! a `__user` member is only ever filled for the queue that does not follow it, and both
//! halves have to hold for the same single call to be sound. Splitting the sentence would
//! not split the operation (note **N199**).
//!
//! | Where | Obligation |
//! |---|---|
//! | [`Fd::as_fd`] | the descriptor is open for the whole of the borrow, which elision ties to `&self` |
//! | [`payload::Payload::bytes`] | every byte of the buffer is initialized |
//! | [`payload::Payload::bytes_mut`] | the same, with the exclusive borrow the mutable slice needs |
//! | `ioctl::call` | the pointer is valid and correctly sized for the request's declared width, and the one struct it carries with a `__user` pointer is only ever filled for the queue that does not follow it |
//! | `ioctl::call_enumerating` | the same pointer obligation as `ioctl::call`, over five enumeration structs that carry no `__user` member at all — so it is a *shorter* discharge, not the same one |
//! | `ioctl::call_ext_ctrls` | the same, plus the one-entry control array — and its payload pointer, when it has one — that the kernel dereferences |
//! | `mmap::Mapping::map` | a null hint address and a descriptor the region belongs to |
//! | `mmap::Mapping::bytes` | the slice lies inside a region that is still mapped |
//! | `mmap::Mapping::drop` | the address and length are the ones `mmap` returned, unmapped once |
//! | `unsafe impl Send for mmap::Mapping` | the region is owned exclusively and is not thread-affine |
//! | `wait::until_readable` | one live `pollfd`, and a count of one to match |
//! | [`signal::term`] | two integers by value: the whole obligation is that the pid names one process, which is refused above the block |
//!
//! P4d added a kernel-facing module — [`uevent`] — and **no row for it**, which is the
//! whole argument for reaching for `rustix` rather than `libc` at a new edge: the same
//! socket written with `libc` would have cost four blocks (`socket`,
//! `OwnedFd::from_raw_fd`, `bind`, `recv`) and a `sockaddr_nl` that a third-party crate
//! maintains by hand. It also generalised `wait::readable` into `wait::until_readable`
//! rather than adding a second `poll` wrapper beside it — one obligation, still stated
//! once, now discharged for two kinds of descriptor.
//!
//! The **one row P4d did add** is [`Fd::as_fd`], and it is a row that bought a type: the
//! generalised wrapper first took a bare `RawFd`, whose doc argued that a stale number was
//! harmless because `poll` answers `POLLNVAL`. That is true of a *closed* descriptor and
//! false of a *reused* one, which is the case a multi-threaded daemon actually produces.
//! Taking a `BorrowedFd` moves the obligation onto the compiler for the socket (an
//! `OwnedFd` borrows itself) and states it exactly once, here, for the device node — which
//! is strictly better than a paragraph asserting a borrow the signature did not make.
//!
//! Two movements worth recording. P2's **write** path added two ioctls and *removed* a
//! block: reads and writes of an `ext_ctrls` header carry the identical obligation, so
//! `call_ext_ctrls` states it once for all four calls rather than each call stating it
//! again in slightly different words. P2's **streaming** path added four, and every one of
//! them is about a buffer's lifetime rather than about an ioctl — which is why [`mmap`] is
//! a type with a `Drop` rather than a pair of free functions: the length that `munmap`
//! needs and the length that bounds a read are the same number, and a type is how they
//! stay the same number.
//!
//! Miri reaches the payload pair and cannot cross the rest, which is why [`payload`] is
//! its own module rather than a few lines here: `scripts/miri.sh` selects it by name.

pub(crate) mod decode;
pub(crate) mod fields;
pub(crate) mod ioctl;
pub(crate) mod mmap;
pub(crate) mod payload;
/// `kill(2)`, which is not V4L2 and is here because this is the only directory in the
/// workspace where the token `unsafe` is allowed. Its own header carries the argument.
pub(crate) mod signal;
/// The `NETLINK_KOBJECT_UEVENT` socket. Kernel-facing rather than unsafe — see the
/// header. `crate::watch` is its one caller.
pub(crate) mod uevent;
pub(crate) mod wait;

use std::os::fd::{AsFd, BorrowedFd};
use std::os::raw::c_int;

use camino::{Utf8Path, Utf8PathBuf};
use schema::error::{Error, Result};

pub(crate) use payload::Payload;

/// An open device node.
///
/// Closes on drop. The `v4l` crate's `open`/`close` are safe wrappers, so this type's only
/// `unsafe` is the one that hands the descriptor out as a borrow — it lives here because
/// the fd it owns is the capability every block below spends.
#[derive(Debug)]
pub(crate) struct Fd {
    fd: c_int,
    path: Utf8PathBuf,
}

impl Fd {
    /// Open a device node for reading and writing.
    ///
    /// # Errors
    ///
    /// The D13 variant matching `errno`: `EBUSY` is [`Error::Busy`], `EACCES`/`EPERM` is
    /// [`Error::PermissionDenied`], `ENOENT`/`ENODEV` is [`Error::DeviceGone`]. Keeping
    /// them apart is E3 — none of them is the camera saying what it cannot do.
    pub(crate) fn open(path: &Utf8Path) -> Result<Fd> {
        // `v4l::v4l2::open` builds a `CString` with `.unwrap()`, so a path containing an
        // interior NUL panics inside the dependency. Our paths come from a sysfs walk and
        // cannot contain one, but "cannot" is a claim about today's callers rather than
        // about the function, and this crate's rule is that no device- or request-driven
        // path panics.
        if path.as_str().as_bytes().contains(&0) {
            return Err(Error::DeviceGone {
                path: path.to_owned(),
            });
        }
        // O_NONBLOCK is deliberately absent: enumeration ioctls do not block, and the
        // streaming path (P2) wants a blocking DQBUF bounded by its own deadline.
        //
        // O_CLOEXEC is deliberately *present*, and it has to be said here because
        // `v4l::v4l2::open` passes its flags straight to `open(2)` and adds nothing (checked
        // in the pinned 0.14.0 source). A `/dev/video*` descriptor is the exclusive-access
        // capability D12 rests on and the thing `webcam-handler-priv`'s interlock counts
        // holders of, so leaking one into a child — this crate's own R3 arm spawns
        // `webcam-handler-priv uvcvideo cycle`, and `modprobe` execs further — would make the
        // helper refuse an unload while naming a process that is not the real holder. Same
        // argument `sys::uevent` states for the netlink socket, on the more valuable
        // descriptor.
        match v4l::v4l2::open(path.as_std_path(), libc::O_RDWR | libc::O_CLOEXEC) {
            Ok(fd) => Ok(Fd {
                fd,
                path: path.to_owned(),
            }),
            Err(error) => Err(open_error(path, &error)),
        }
    }

    /// The raw descriptor, for the ioctl calls in [`ioctl`].
    pub(crate) fn raw(&self) -> c_int {
        self.fd
    }

    /// The node this descriptor came from, so an error can name it.
    pub(crate) fn path(&self) -> &Utf8Path {
        &self.path
    }

    /// Take ownership of a descriptor that did not come from a device node.
    ///
    /// Test-only, and it exists so [`wait::readable`] can be exercised in both directions
    /// without a camera: a socket pair is the one descriptor a test can *make* readable
    /// on demand. The caller transfers ownership — `Drop` closes it — so a raw fd handed
    /// here must not be closed anywhere else.
    #[cfg(test)]
    pub(crate) fn from_raw_for_test(fd: c_int) -> Fd {
        Fd {
            fd,
            path: Utf8PathBuf::from("<test descriptor>"),
        }
    }
}

impl AsFd for Fd {
    fn as_fd(&self) -> BorrowedFd<'_> {
        // SAFETY: `borrow_raw` asks for two things, and both hold by construction. The
        // descriptor is open for the borrow's whole life: `self.fd` is what
        // `v4l::v4l2::open` returned to this value and to nothing else, `Fd` never hands
        // out ownership, its `Drop` is the only close, and elision ties the returned
        // lifetime to `&self` — so it cannot be closed under the borrow and its number
        // cannot have been reused. And it is never `-1`: `Fd::open` answers `Err` on that
        // value before a `Fd` exists, and `from_raw_for_test` is handed a descriptor a
        // socket pair produced.
        unsafe { BorrowedFd::borrow_raw(self.fd) }
    }
}

impl Drop for Fd {
    fn drop(&mut self) {
        // A close that fails has nothing left to tell us: the descriptor is gone either
        // way, and there is no caller to report to. Dropped deliberately, not by
        // oversight.
        let _ = v4l::v4l2::close(self.fd);
    }
}

/// Map an `open(2)` failure onto the D13 registry.
fn open_error(path: &Utf8Path, error: &std::io::Error) -> Error {
    match error.raw_os_error() {
        // D13's `holders`, populated where the refusal is made: `EBUSY` on its own is a
        // dead end for the reader, and the next thing they would do is run `fuser`.
        Some(libc::EBUSY) => Error::busy(path.to_owned(), crate::holders::of(path)),
        Some(libc::EACCES | libc::EPERM) => Error::PermissionDenied {
            path: path.to_owned(),
            hint: "add yourself to the `video` group, then log out and back in".to_owned(),
        },
        Some(libc::ENOENT | libc::ENODEV | libc::ENXIO) => Error::DeviceGone {
            path: path.to_owned(),
        },
        errno => Error::DeviceIo {
            operation: format!("open({path})"),
            errno,
            message: error.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_a_node_that_is_not_there_is_device_gone_not_a_capability_answer() {
        // E3 in the smallest possible test: the absence of a device says nothing about
        // what a device can do, so it must not arrive as anything that reads like it.
        let error = Fd::open(Utf8Path::new("/dev/video-nothing-is-here"))
            .expect_err("a node that does not exist cannot be opened");
        assert!(
            matches!(error, Error::DeviceGone { .. }),
            "expected DeviceGone, got {error}"
        );
    }

    #[test]
    fn a_path_with_an_interior_nul_is_refused_before_it_reaches_the_dependency() {
        // `v4l::v4l2::open` builds its `CString` with `.unwrap()`. Nothing in this crate
        // constructs such a path today, and "nothing does today" is not a property of the
        // function.
        let error = Fd::open(Utf8Path::new("/dev/video\u{0}0"))
            .expect_err("a path with an interior NUL cannot be opened");
        assert!(matches!(error, Error::DeviceGone { .. }), "{error}");
    }

    #[test]
    fn a_device_descriptor_is_not_inherited_by_anything_this_process_execs() {
        // `v4l::v4l2::open` hands its flags to `open(2)` unchanged, so `O_CLOEXEC` is ours to
        // pass or to forget — and this is the descriptor it matters most for. A `/dev/video*`
        // held across an `exec` makes every child a camera holder, which is what
        // `webcam-handler-priv`'s unload interlock counts: the R3 hotplug arm spawns
        // `webcam-handler-priv uvcvideo cycle`, `modprobe` execs further, and a leaked node
        // would have the helper refuse the unload while naming a process that never opened a
        // camera. Asserted off `F_GETFD` rather than off the flags argument, for
        // `the_socket_directory_is_created_private`'s reason: the argument is what was
        // requested and the descriptor is what was applied (AGENTS rule 5).
        //
        // `/dev/null` and not a camera: this is a property of `Fd::open`, and a suite that
        // needed a webcam attached to state it would state it on one machine in ten.
        let fd = Fd::open(Utf8Path::new("/dev/null"))
            .expect("/dev/null is on every Linux this crate builds for");
        let flags = rustix::io::fcntl_getfd(fd.as_fd()).expect("F_GETFD on our own descriptor");
        assert!(
            flags.contains(rustix::io::FdFlags::CLOEXEC),
            "a device node opened without O_CLOEXEC leaks into every child: {flags:?}"
        );
    }
}
