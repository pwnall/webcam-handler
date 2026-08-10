//! The unsafe boundary (design §2.5, rubric B10).
//!
//! Every `unsafe` block in the workspace lives under this directory, and
//! `scripts/gates/unsafe-scope.sh` derives that claim from the tree rather than trusting
//! it. What is here: an owned file descriptor, an aligned payload buffer, and the typed
//! ioctl calls. What is deliberately *not* here: any interpretation of what the kernel
//! said.
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
//! Ten blocks and one `unsafe impl`, one obligation each
//! (`clippy::multiple_unsafe_ops_per_block` is denied, so that is enforced rather than
//! claimed):
//!
//! | Where | Obligation |
//! |---|---|
//! | [`payload::Payload::bytes`] | every byte of the buffer is initialized |
//! | [`payload::Payload::bytes_mut`] | the same, with the exclusive borrow the mutable slice needs |
//! | `ioctl::call` | the pointer is valid and correctly sized for the request's declared width |
//! | `ioctl::call_enumerating` | the same; only the *interpretation* of the error differs |
//! | `ioctl::call_ext_ctrls` | the same, plus the one-entry control array — and its payload pointer, when it has one — that the kernel dereferences |
//! | `mmap::Mapping::map` | a null hint address and a descriptor the region belongs to |
//! | `mmap::Mapping::bytes` | the slice lies inside a region that is still mapped |
//! | `mmap::Mapping::drop` | the address and length are the ones `mmap` returned, unmapped once |
//! | `unsafe impl Send for mmap::Mapping` | the region is owned exclusively and is not thread-affine |
//! | `wait::readable` | one live `pollfd`, and a count of one to match |
//! | [`signal::term`] | two integers by value: the whole obligation is that the pid names one process, which is refused above the block |
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
pub(crate) mod wait;

use std::os::raw::c_int;

use camino::{Utf8Path, Utf8PathBuf};
use schema::error::{Error, Result};

pub(crate) use payload::Payload;

/// An open device node.
///
/// Closes on drop. The `v4l` crate's `open`/`close` are safe wrappers, so this type needs
/// no `unsafe` of its own — it lives here because the fd it owns is the capability every
/// block below spends.
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
        match v4l::v4l2::open(path.as_std_path(), libc::O_RDWR) {
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
        Some(libc::EBUSY) => Error::Busy {
            holders: crate::holders::of(path),
            path: path.to_owned(),
        },
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
}
