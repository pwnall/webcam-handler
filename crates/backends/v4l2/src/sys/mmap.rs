//! One driver buffer, mapped into this process.
//!
//! `V4L2_MEMORY_MMAP` is the streaming method this project uses: the driver allocates the
//! buffers, `VIDIOC_QUERYBUF` reports where each one lives in the device's address space,
//! and `mmap` brings it here. The alternative — `USERPTR` — would put the allocation on
//! our side and buy nothing a webcam cares about.
//!
//! ## What the type is for
//!
//! An `mmap` without a matching `munmap` is a leak that survives every `close`, and a
//! `munmap` of a length that disagrees with the mapping is undefined. Both facts are one
//! `length`, so the mapping owns it and [`Drop`] spends it — there is no code path,
//! including a panic between `QUERYBUF` and `STREAMON`, that can unmap the wrong number
//! of bytes or forget to.
//!
//! ## `bytesused` is device-supplied
//!
//! Rubric B10: the driver's byte count arrives from outside and is not trusted as a
//! length. [`Mapping::bytes`] clamps it to the mapping, so a driver reporting a frame
//! larger than the buffer it handed us yields a short frame — which the battery's own
//! length check then reports — rather than a read past the end of a mapping.

use std::ffi::c_void;
use std::ptr::NonNull;

use schema::error::{Error, Result};

use super::Fd;

/// One driver buffer, live for as long as this value is.
#[derive(Debug)]
pub(crate) struct Mapping {
    start: NonNull<u8>,
    len: usize,
}

// SAFETY: a `Mapping` owns its region exclusively — nothing else in the process holds the
// address, because the only way to obtain one is `Mapping::map`, which never hands the
// pointer out. `mmap`ed memory is not thread-affine, so moving the owner to another
// thread changes nothing about the mapping's validity, and D12 gives each camera's
// buffers to exactly one actor thread anyway. Needed because T2's `Camera` is `Send` and
// the streaming state hangs off the camera.
unsafe impl Send for Mapping {}

impl Mapping {
    /// Map `length` bytes at the driver's `offset` for this device.
    ///
    /// # Errors
    ///
    /// [`Error::DeviceIo`] naming `mmap` — a mapping failure is a fact about this process
    /// (an rlimit, an exhausted address space), not about what the camera can do (E3).
    pub(crate) fn map(fd: &Fd, offset: u32, length: u32) -> Result<Mapping> {
        let len = usize::try_from(length).map_err(|_| mmap_failure("length does not fit"))?;
        if len == 0 {
            // A zero-length mapping is not a buffer, and `mmap` would refuse it with
            // `EINVAL` a layer further down where the message is less useful.
            return Err(mmap_failure(
                "the driver reported a zero-length buffer, which cannot hold a frame",
            ));
        }
        let offset = i64::from(offset);

        // SAFETY: the only obligation `mmap` itself places on the caller is that `start`
        // is null (so the kernel chooses the address) — which it is — and that `fd` is a
        // descriptor the mapping is valid for, which `Fd` guarantees by owning it for at
        // least the duration of this call. `len` and `offset` come from the driver's own
        // `QUERYBUF` reply for this device, so they describe a region it is offering.
        // Nothing is dereferenced here; the returned address is only recorded.
        let raw = unsafe {
            v4l::v4l2::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd.raw(),
                offset,
            )
        }
        .map_err(|error| mmap_failure(&error.to_string()))?;

        let start = NonNull::new(raw.cast::<u8>())
            .ok_or_else(|| mmap_failure("mmap returned a null address"))?;
        Ok(Mapping { start, len })
    }

    /// The first `used` bytes of the mapping, clamped to it.
    ///
    /// `used` is the driver's `bytesused`, which is device-supplied and therefore input
    /// (rubric B10). A driver claiming more than it gave us gets a short slice rather than
    /// an out-of-bounds read; the caller's own length check is where that becomes a
    /// finding.
    pub(crate) fn bytes(&self, used: u32) -> &[u8] {
        let len = usize::try_from(used).unwrap_or(self.len).min(self.len);
        // SAFETY: `self.start` is the address `mmap` returned for a region of `self.len`
        // readable bytes, and it is still mapped — `Drop` is the only thing that unmaps
        // it, and it cannot have run while `&self` is borrowed. `len <= self.len` by the
        // clamp above, so the slice lies entirely inside the region, and the returned
        // lifetime is tied to `&self`, so it cannot outlive the mapping.
        unsafe { std::slice::from_raw_parts(self.start.as_ptr(), len) }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: `self.start` and `self.len` are the address and length `mmap` returned
        // and were recorded together, so this is exactly the region that was mapped. This
        // is the only `munmap` of it — the type is not `Clone` and the pointer never
        // leaves — and `Drop` runs once.
        let unmapped = unsafe { v4l::v4l2::munmap(self.start.as_ptr().cast::<c_void>(), self.len) };
        // Nothing to tell anyone: the buffer is going away either way and there is no
        // caller left. Dropped deliberately, like `Fd`'s close.
        let _ = unmapped;
    }
}

fn mmap_failure(message: &str) -> Error {
    Error::DeviceIo {
        operation: "mmap".to_owned(),
        errno: None,
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_length_buffer_is_refused_before_it_reaches_mmap() {
        // A driver that reports a zero-length buffer has said something impossible, and
        // the message should name that rather than relaying an `EINVAL` from two layers
        // down. No device needed: the check is ahead of the syscall.
        let fd = Fd::open(camino::Utf8Path::new("/dev/null"));
        let Ok(fd) = fd else {
            // A host without /dev/null is not a host; but the arm still must not panic.
            return;
        };
        let error = Mapping::map(&fd, 0, 0).expect_err("a zero-length buffer is not a buffer");
        assert!(error.to_string().contains("zero-length"), "{error}");
    }
}
