//! One home for the errors this crate raises (design D13, rubric A5).
//!
//! The D13 registry is closed and has no imaging-local variant. [`Error::DeviceIo`] is
//! its no-more-specific-home arm, and its `operation` field is exactly the place to name
//! the imaging step that refused: every failure here is a frame the device produced that
//! cannot be turned into pixels, or our own encoding of one. `errno` is `None` because no
//! syscall was involved, and the registry models that as an option for this reason.
//!
//! Routing every refusal through one function is what makes the choice auditable — and
//! what makes it a one-line change if D13 ever grows a `FrameMalformed` variant, which
//! the imaging report recommends.

use schema::error::Error;

/// Report that an imaging step could not proceed.
///
/// `operation` names the step ("decode YUYV frame"), `message` says what was wrong in
/// terms of counts, dimensions and format names. **Never frame bytes** (rubric A12): a
/// frame may contain a person, and an error message travels to logs and to the wire.
pub(crate) fn imaging_failure(operation: &str, message: impl Into<String>) -> Error {
    Error::DeviceIo {
        operation: operation.to_owned(),
        errno: None,
        message: message.into(),
    }
}

/// The opening words of every short-buffer refusal.
///
/// Named rather than inlined because the tests assert on it: a decode that delegated its
/// length check to a conversion library would still return an error, and the *only*
/// thing distinguishing "we validated before touching the buffer" from "our dependency
/// noticed afterwards" is whose message came back. A mutation that deleted the check in
/// `decode_yuyv` survived until the tests pinned this string (recorded here so it is not
/// re-learned).
pub(crate) const SHORT_BUFFER_PREFIX: &str = "frame buffer holds";

/// Report a buffer that is shorter than the geometry says it must be.
///
/// The single most likely thing a driver gets wrong, and the one this crate must never
/// answer with an out-of-bounds read.
pub(crate) fn short_buffer(operation: &str, needed: usize, got: usize) -> Error {
    imaging_failure(
        operation,
        format!("{SHORT_BUFFER_PREFIX} {got} bytes but the reported geometry needs {needed}"),
    )
}
