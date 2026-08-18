//! Synthetic photographs on disk, for the document verbs that read files.
//!
//! **A file of its own, and the reason is a gate rather than tidiness.**
//! `atomic-write-home.sh` reports any file that names D9's own directories *and* contains a raw
//! write primitive as a bypass of `write_json_atomic`. It is a `grep`, so prose and environment
//! manipulation count alike, and its header says plainly that the rule stays and the code
//! adapts. Both of this suite's other files must name those directories in real code —
//! `wchc.rs` has an arm about the refusal a client makes when the runtime variable is unset, and
//! `support/fixture.rs` is the module that *builds* the pair of directories — so neither can
//! host a fixture write without turning itself into a reported bypass. This file names none of
//! them, which is the whole of why it exists.
//!
//! The pictures are **synthetic rather than captures**: a fixture taken off the fake backend
//! would put a backend into an arm whose whole subject is a verb that needs none. They are
//! encoded through `imaging::encode`, which is the writer whose output the reader under test
//! accepts.

use camino::{Utf8Path, Utf8PathBuf};

/// Write one photograph under `dir`, and answer where it went.
pub(crate) fn write_photograph(dir: &Utf8Path, name: &str, image: image::GrayImage) -> Utf8PathBuf {
    let path = dir.join(name);
    let bytes = imaging::encode::png(&imaging::Decoded::Gray(image)).expect("a fixture encodes");
    std::fs::write(&path, bytes).expect("writes the fixture");
    path
}

/// Write bytes that are not a photograph at all, and answer where they went.
///
/// Named rather than inlined so the arm that uses it reads as the claim it is making: what is
/// under test is the refusal for *content*, and bytes spelled out in the middle of the
/// assertions would look like part of the comparison.
pub(crate) fn write_not_a_photograph(dir: &Utf8Path, name: &str) -> Utf8PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, b"GIF89a and then some").expect("writes the fixture");
    path
}
