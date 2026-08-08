//! Never compiled — the reference below exists so `cargo machete` sees the dependency
//! as used rather than reporting the fixture as an unused-dependency finding.

/// Touch the banned-by-name fixture crate.
pub fn touch() {
    colored::nothing();
}
