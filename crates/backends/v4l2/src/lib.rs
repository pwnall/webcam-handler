//! The V4L2 backend.
//!
//! This is the one crate in the workspace without `#![forbid(unsafe_code)]`: talking to
//! the kernel means ioctls and mmap. The token `unsafe` is confined to `src/sys/` by
//! `scripts/gates/unsafe-scope.sh`, which derives the allowed path from the tree.
//! (That module arrives with the ioctl layer at P1; until then the gate treats every
//! occurrence of the token as a violation, which is the honest reading of an empty
//! exemption.)
// Kernel-shaped integers are converted with `try_from`, never `as` (rubric B10).
#![deny(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
