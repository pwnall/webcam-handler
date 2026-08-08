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
//!   something true to say (docs/4's recorded limit).
//! - **Union reads stop being an obligation.** `v4l2_querymenu`'s union is two readings
//!   of the same 32 bytes; choosing between them by control type is a decoding decision
//!   made over a byte slice, not an `unsafe` field access that has to prove which arm was
//!   written.
//! - **Device-derived numbers are validated in safe code**, where the validation is
//!   ordinary and testable, before any of them reaches a length or an index.
//!
//! The residual `unsafe` is three obligations, one per block: viewing an initialized
//! payload as bytes, viewing it as mutable bytes, and the ioctl itself.

pub(crate) mod decode;
pub(crate) mod fields;
pub(crate) mod ioctl;

use std::mem::MaybeUninit;
use std::os::raw::{c_int, c_void};

use camino::{Utf8Path, Utf8PathBuf};
use schema::error::{Error, Result};

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
        Some(libc::EBUSY) => Error::Busy {
            path: path.to_owned(),
            holders: Vec::new(),
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

/// A correctly sized and correctly aligned buffer for one ioctl argument.
///
/// The kernel wants a pointer to `T`; this crate wants bytes. `MaybeUninit<T>` supplies
/// both: the allocation has `T`'s size and alignment, and [`Payload::zeroed`] makes every
/// byte of it — padding included — initialized before anyone looks.
pub(crate) struct Payload<T: Copy> {
    slot: MaybeUninit<T>,
}

impl<T: Copy> std::fmt::Debug for Payload<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Payload")
            .field("bytes", &format_args!("<{} bytes>", size_of::<T>()))
            .finish()
    }
}

impl<T: Copy> Payload<T> {
    /// A payload with every byte zero — the state every V4L2 ioctl argument starts in,
    /// because the kernel reads reserved fields and rejects non-zero ones.
    pub(crate) fn zeroed() -> Payload<T> {
        Payload {
            slot: MaybeUninit::zeroed(),
        }
    }

    /// The pointer the ioctl writes through.
    pub(crate) fn as_mut_ptr(&mut self) -> *mut c_void {
        self.slot.as_mut_ptr().cast::<c_void>()
    }

    /// The payload's bytes.
    pub(crate) fn bytes(&self) -> &[u8] {
        // SAFETY: every byte of `slot` is initialized — `zeroed()` is the only
        // constructor and it writes the whole allocation, padding included, and the only
        // subsequent writers (`bytes_mut` and the kernel through `as_mut_ptr`) overwrite
        // initialized bytes rather than deinitializing any. The slice borrows `self` for
        // its own lifetime and covers exactly `size_of::<T>()` bytes starting at a
        // pointer valid for that many.
        unsafe { std::slice::from_raw_parts(self.slot.as_ptr().cast::<u8>(), size_of::<T>()) }
    }

    /// The payload's bytes, for filling in request fields before the call.
    pub(crate) fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: as `bytes`, with `&mut self` giving the exclusive borrow the mutable
        // slice requires. Writing initialized bytes over initialized bytes keeps the
        // whole allocation initialized, which is what `bytes` relies on.
        unsafe {
            std::slice::from_raw_parts_mut(self.slot.as_mut_ptr().cast::<u8>(), size_of::<T>())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zeroed_payload_is_all_zero_and_the_right_size() {
        let payload = Payload::<v4l::v4l_sys::v4l2_capability>::zeroed();
        assert_eq!(
            payload.bytes().len(),
            size_of::<v4l::v4l_sys::v4l2_capability>()
        );
        assert!(payload.bytes().iter().all(|b| *b == 0));
    }

    #[test]
    fn writes_through_the_mutable_view_are_visible_through_the_shared_one() {
        let mut payload = Payload::<v4l::v4l_sys::v4l2_fmtdesc>::zeroed();
        fields::write_u32(payload.bytes_mut(), 0, 7).expect("index field is in range");
        assert_eq!(fields::read_u32(payload.bytes(), 0), Some(7));
    }

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
}
