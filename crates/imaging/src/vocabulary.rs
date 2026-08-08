//! Machinery for closed vocabularies whose membership the compiler owns.
//!
//! Rubric rule 6 bans hand lists for completeness checks, and a `const ALL: &[Self]`
//! written next to an enum *is* a hand list. This macro generates the enum and its `ALL`
//! from one source, so adding a variant adds it to `ALL` in the same token.
//!
//! **This is a transcription of `webcam-handler-schema`'s `closed_vocabulary!`**, which
//! is `pub(crate)` there and therefore unreachable from this crate. A second copy of a
//! law is a rubric A5 finding, and this one is filed rather than defended: the fix is
//! for `webcam-handler-schema` to export the macro (`#[macro_export]`, or a `pub mod
//! vocabulary`), after which this file deletes itself and [`crate::decode::SourceFormat`]
//! uses the one home. The copy exists because the alternative — a hand-written `ALL` for
//! the D6 source-format set — is the exact defect rule 6 names.

/// Define a plain closed vocabulary together with its `ALL`.
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
