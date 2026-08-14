//! Pure imaging: bytes in, bytes out.
//!
//! Decode, convert, encode, EXIF-stamp, measure. Nothing here touches a device, a clock,
//! or the filesystem — the engine owns every sink, this crate owns every byte
//! transformation (design §2.10). That split is what lets the whole D6 photo pipeline be
//! tested on the R0 rung with no camera and no daemon.
//!
//! The modules, and the design decision each one is the home of:
//!
//! | Module | Home of |
//! |---|---|
//! | [`decode`] | the D6 source-format set: MJPG/JPEG, YUYV, NV12, GREY → pixels |
//! | [`encode`] | the D6 sink encodings: PNG, JPEG re-encode, binary PPM/PGM |
//! | [`photo`] | the pass-through-vs-re-encode decision (E6) |
//! | [`exif`] | capture metadata stamped onto JPEG bytes (D6) |
//! | [`avi`] | the D7 L0 video container, and an independent reader that distrusts it |
//! | [`metrics`] | the D8 metric set as pure functions over a luma image |
//! | [`fixtures`] | deterministic synthetic images for tests, the fake, and the corpus |
//!
//! ## A driver is untrusted input
//!
//! Rubric B10 was written for the `unsafe` side of the V4L2 boundary, but it applies
//! just as hard on this side of it: `bytesused`, `width`, `height` and `bytes_per_line`
//! all arrive from a driver, and a driver that reports more pixels than it delivered is
//! a truncated read waiting to happen. Every entry point in [`decode`] validates the
//! buffer against the geometry *before* touching a byte, and a short buffer is a typed
//! error — never a panic, never an out-of-bounds read. The crate-root lint block below
//! is what keeps that promise mechanical rather than remembered.
//!
//! ## A frame may contain a person
//!
//! Rubric A12: no error message this crate produces contains frame bytes. Lengths,
//! dimensions and format names only.
#![forbid(unsafe_code)]
// docs/9's "device/request-driven paths" lint set. Every path in this crate is
// frame-driven, so the whole crate is inside it. `not(test)` because a test asserting an
// invariant with `.expect("literal fixture")` is stating a precondition, not risking a
// device; docs/9 writes the same carve-out.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::as_conversions
    )
)]

mod fault;
mod vocabulary;

pub mod avi;
pub mod decode;
pub mod encode;
pub mod exif;
pub mod fixtures;
pub mod metrics;
pub mod photo;

pub use decode::{Decoded, SourceFormat, decode_frame};
pub use metrics::measure_all;
// `PhotoRendering` and `TransformApplication` are deliberately *not* re-exported: they
// live in `schema::capture` because they cross the wire in a `PhotoReport`, and a second
// path to the same type is a second name for it.
pub use photo::{Photo, render};
