//! The aligned buffer an ioctl argument lives in.
//!
//! Two of this crate's `unsafe` blocks are here, and they are the two Miri can actually
//! execute — nothing in this module makes a syscall, so `scripts/miri.sh` selects it
//! alongside [`super::decode`]. The four blocks in [`super::ioctl`] are the ioctl calls
//! themselves, which Miri cannot cross; that split is why this type is a module rather
//! than a few lines inside one.

use std::mem::MaybeUninit;
use std::os::raw::c_void;

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
    use super::super::fields;
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
    fn every_byte_including_padding_stays_initialized_across_a_partial_write() {
        // The premise `bytes()`'s SAFETY comment rests on. `v4l2_query_ext_ctrl` has
        // padding between its 32-byte `name` and its 8-byte-aligned `minimum`; writing
        // one field must leave the rest — padding included — readable. Under Miri this
        // reads every byte, so an uninitialized one is caught rather than assumed absent.
        let mut payload = Payload::<v4l::v4l_sys::v4l2_query_ext_ctrl>::zeroed();
        fields::write_u32(payload.bytes_mut(), 0, 0x0098_0900).expect("the id field fits");
        let sum: u64 = payload.bytes().iter().map(|b| u64::from(*b)).sum();
        assert!(sum > 0, "the write is visible");
        assert_eq!(
            payload.bytes().len(),
            size_of::<v4l::v4l_sys::v4l2_query_ext_ctrl>()
        );
    }

    #[test]
    fn the_pointer_handed_to_the_kernel_addresses_the_same_bytes_the_views_do() {
        // The other half of the ioctl obligation, checked without an ioctl: the pointer
        // `as_mut_ptr` hands out and the slice `bytes` hands out must be the same region,
        // or the kernel would write somewhere the decoder never reads.
        let mut payload = Payload::<v4l::v4l_sys::v4l2_fmtdesc>::zeroed();
        let ptr = payload.as_mut_ptr();
        fields::write_u32(payload.bytes_mut(), 0, 0xabcd).expect("fits");
        assert_eq!(ptr.cast::<u8>().cast_const(), payload.bytes().as_ptr());
    }
}
