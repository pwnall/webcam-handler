//! Never compiled — the reference below exists so `cargo machete` sees the dependency
//! as used rather than reporting the fixture as an unused-dependency finding.

/// Touch the copyleft fixture crate.
pub fn touch() {
    copyleft_fixture::nothing();
}
