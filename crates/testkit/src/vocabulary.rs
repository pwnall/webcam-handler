//! Closed-vocabulary machinery, so this crate's `ALL` lists cannot drift from their
//! enums.
//!
//! Rubric rule 6 bans hand lists for completeness checks. `webcam-handler-schema` solves
//! that with the same macro in `crates/schema/src/vocabulary.rs`, but its copy is
//! `pub(crate)` — a macro is not part of the schema's public vocabulary, and this crate
//! must not edit that one. So the generator lives here too, for the one vocabulary this
//! crate owns ([`crate::battery::BatteryArm`]).
//!
//! If schema ever exports the macro, this file deletes itself.

/// Define a closed vocabulary together with its `ALL`.
///
/// Generating both from one definition is the point: adding a variant adds it to `ALL` in
/// the same token, so a walk over `ALL` cannot silently shrink.
macro_rules! closed_vocabulary {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis enum $name {
            $( $(#[$vmeta])* $variant ),*
        }

        impl $name {
            /// Every variant, generated from the definition above.
            pub const ALL: &'static [$name] = &[ $( $name::$variant ),* ];
        }
    };
}

pub(crate) use closed_vocabulary;
