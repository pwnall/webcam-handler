//! Machinery for closed vocabularies whose membership the compiler owns.
//!
//! Rubric rule 6 bans hand lists for completeness checks: "driven by something the
//! compiler breaks — an exhaustive `match`, never a hand list, and a fixed-size array
//! does not substitute." A `const ALL: &[Self]` written next to an enum *is* a hand
//! list. These macros generate the enum and its `ALL` from one source, so the list
//! cannot drift from the type: adding a variant adds it to `ALL` in the same token.

/// Define an enum whose variants each own a bit, plus the decoding both directions.
///
/// Generates `ALL`, `bit()`, `decode(raw)` (the known bits present) and
/// `unknown_bits(raw)` (everything else — carried as data, never dropped: next year's
/// kernel bit is data, not a surprise \[PF:12\]).
macro_rules! bit_vocabulary {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident = $bit:expr ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis enum $name {
            $( $(#[$vmeta])* $variant ),*
        }

        impl $name {
            /// Every variant, generated from the definition above.
            pub const ALL: &'static [$name] = &[ $( $name::$variant ),* ];

            /// The bit this variant occupies.
            #[must_use]
            pub const fn bit(self) -> u32 {
                match self { $( $name::$variant => $bit ),* }
            }

            /// The known bits present in `raw`, in declaration order.
            #[must_use]
            pub fn decode(raw: u32) -> Vec<$name> {
                Self::ALL.iter().copied().filter(|f| raw & f.bit() != 0).collect()
            }

            /// The bits of `raw` this vocabulary does not name.
            #[must_use]
            pub fn unknown_bits(raw: u32) -> u32 {
                let known = Self::ALL.iter().fold(0u32, |acc, f| acc | f.bit());
                raw & !known
            }
        }
    };
}

/// Define a plain closed vocabulary (no bits) together with its `ALL`.
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

pub(crate) use bit_vocabulary;
pub(crate) use closed_vocabulary;
