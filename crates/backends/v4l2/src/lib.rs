//! The V4L2 backend.
//!
//! This is the one crate in the workspace without `#![forbid(unsafe_code)]`: talking to
//! the kernel means ioctls and mmap. The token `unsafe` is confined to [`sys`] by
//! `scripts/gates/unsafe-scope.sh`, which derives the allowed path from the tree.
// Kernel-shaped integers are converted with `try_from`, never `as` (rubric B10).
#![deny(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
