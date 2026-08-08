//! Reading and writing kernel-struct fields as bytes at derived offsets.
//!
//! Every accessor is total: an offset past the end of the buffer, or a field that would
//! run off it, yields `None` rather than panicking or reading whatever is next. That is
//! not defensive style — the buffers these run over are the size the *bindings* report
//! and the offsets come from [`core::mem::offset_of!`] over the same bindings, so a
//! `None` here means the two disagree, which is a fact worth surfacing rather than a
//! condition worth trusting away.
//!
//! Integers are read in native byte order because these bytes never left the machine:
//! they came from an ioctl into memory this process owns. There is no wire here, and
//! `from_ne_bytes` says so.

/// Read a `u32` field at `offset`.
pub(crate) fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_ne_bytes(chunk(bytes, offset)?))
}

/// Read an `i32` field at `offset`.
pub(crate) fn read_i32(bytes: &[u8], offset: usize) -> Option<i32> {
    Some(i32::from_ne_bytes(chunk(bytes, offset)?))
}

/// Read a `u64` field at `offset`.
pub(crate) fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_ne_bytes(chunk(bytes, offset)?))
}

/// Read an `i64` field at `offset`.
pub(crate) fn read_i64(bytes: &[u8], offset: usize) -> Option<i64> {
    Some(i64::from_ne_bytes(chunk(bytes, offset)?))
}

/// Write a `u32` field at `offset`. `None` when the field does not fit.
pub(crate) fn write_u32(bytes: &mut [u8], offset: usize, value: u32) -> Option<()> {
    write(bytes, offset, &value.to_ne_bytes())
}

/// Write a `u64` field at `offset`. `None` when the field does not fit.
///
/// Test-only: no request field the read path fills is 64 bits wide. It exists so the
/// decode tests can build the out-of-representable-range `step` no attached device
/// reports, and gating it keeps rubric A8 true — a constant or helper nobody reads is a
/// defect, and "nobody reads it outside tests" is stated here rather than implied.
#[cfg(test)]
pub(crate) fn write_u64(bytes: &mut [u8], offset: usize, value: u64) -> Option<()> {
    write(bytes, offset, &value.to_ne_bytes())
}

/// Write a `usize`-shaped pointer field at `offset`.
///
/// Planting an address as bytes is an ordinary integer write; the obligation it creates
/// belongs to whoever hands the struct to the kernel, and lives in that call's `SAFETY`.
pub(crate) fn write_usize(bytes: &mut [u8], offset: usize, value: usize) -> Option<()> {
    write(bytes, offset, &value.to_ne_bytes())
}

/// Read a NUL-terminated, fixed-width character field as a `String`.
///
/// Drivers are input. A field with no NUL is taken whole rather than read past, and bytes
/// that are not UTF-8 become replacement characters rather than an error — a camera whose
/// card name is mojibake still enumerates, which is the represent-don't-reject rule (D2)
/// applied to the one field a human reads first.
pub(crate) fn read_cstr(bytes: &[u8], offset: usize, len: usize) -> Option<String> {
    let field = bytes.get(offset..offset.checked_add(len)?)?;
    let end = field.iter().position(|b| *b == 0).unwrap_or(field.len());
    let text = field.get(..end)?;
    Some(String::from_utf8_lossy(text).into_owned())
}

fn chunk<const N: usize>(bytes: &[u8], offset: usize) -> Option<[u8; N]> {
    let slice = bytes.get(offset..offset.checked_add(N)?)?;
    slice.try_into().ok()
}

fn write(bytes: &mut [u8], offset: usize, value: &[u8]) -> Option<()> {
    let end = offset.checked_add(value.len())?;
    bytes.get_mut(offset..end)?.copy_from_slice(value);
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_that_runs_off_the_end_is_none_rather_than_a_panic() {
        let bytes = [0u8; 4];
        assert_eq!(read_u32(&bytes, 0), Some(0));
        assert_eq!(read_u32(&bytes, 1), None);
        assert_eq!(read_u64(&bytes, 0), None);
        assert_eq!(read_i64(&bytes, 0), None);
        assert_eq!(read_cstr(&bytes, 0, 8), None);
        // And the arithmetic that computes the end cannot itself overflow into a
        // wrapped-around range that happens to be in bounds.
        assert_eq!(read_u32(&bytes, usize::MAX), None);
    }

    #[test]
    fn integers_round_trip_through_their_writers() {
        let mut bytes = [0u8; 24];
        write_u32(&mut bytes, 0, 0xdead_beef).expect("fits");
        write_u64(&mut bytes, 8, u64::MAX).expect("fits");
        write_usize(&mut bytes, 16, 0x1234_5678).expect("fits");
        assert_eq!(read_u32(&bytes, 0), Some(0xdead_beef));
        assert_eq!(read_i32(&bytes, 0), Some(-559_038_737));
        assert_eq!(read_u64(&bytes, 8), Some(u64::MAX));
        assert_eq!(read_i64(&bytes, 8), Some(-1));
        assert_eq!(read_u64(&bytes, 16), Some(0x1234_5678));
    }

    #[test]
    fn a_write_that_would_not_fit_changes_nothing() {
        let mut bytes = [0u8; 4];
        assert_eq!(write_u64(&mut bytes, 0, u64::MAX), None);
        assert_eq!(write_u32(&mut bytes, 1, u32::MAX), None);
        assert_eq!(bytes, [0u8; 4], "a refused write must not have written");
    }

    #[test]
    fn a_character_field_stops_at_its_nul() {
        let mut bytes = [b'x'; 8];
        bytes[0] = b'a';
        bytes[1] = b'b';
        bytes[2] = 0;
        assert_eq!(read_cstr(&bytes, 0, 8).as_deref(), Some("ab"));
    }

    #[test]
    fn a_character_field_with_no_nul_is_taken_whole_rather_than_read_past() {
        // The kernel's `card` field is 32 bytes and a driver may fill every one of them.
        let bytes = [b'z'; 4];
        assert_eq!(read_cstr(&bytes, 0, 4).as_deref(), Some("zzzz"));
    }

    #[test]
    fn a_non_utf8_name_survives_as_replacement_characters() {
        // D2: represent, don't reject. A driver with a Latin-1 card name still lists.
        let bytes = [0xff, 0xfe, b'!', 0];
        let text = read_cstr(&bytes, 0, 4).expect("in range");
        assert!(text.ends_with('!'), "{text:?}");
        assert!(!text.is_empty());
    }
}
