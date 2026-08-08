//! The slug transform, pinned.
//!
//! Two readings of "lowercase and underscore" diverge on real control names, so design
//! D2 fixes one: lowercase, keep `[a-z0-9]` runs, collapse every other run to a single
//! separator, trim the separator off both ends. Control slugs separate with `_` (the
//! `v4l2-ctl` spelling agents already know); camera slugs separate with `-`.
//!
//! ```
//! # use schema::slug::{slugify, Separator};
//! assert_eq!(slugify("Zoom, Continuous", Separator::Underscore), "zoom_continuous");
//! assert_eq!(slugify("OBSBOT Tiny 3", Separator::Hyphen), "obsbot-tiny-3");
//! ```

/// Which character joins the runs of a slug.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Separator {
    /// `_` — control slugs, matching `v4l2-ctl`'s spelling.
    Underscore,
    /// `-` — camera id slugs (design D1).
    Hyphen,
}

impl Separator {
    /// The separator's character.
    #[must_use]
    pub const fn as_char(self) -> char {
        match self {
            Separator::Underscore => '_',
            Separator::Hyphen => '-',
        }
    }
}

/// Apply the D2 slug transform.
///
/// Non-ASCII characters count as "other" and collapse into the separator like any other
/// run; a name that slugs to nothing yields the empty string, and callers decide what
/// that means (`ControlSlug::new` rejects it).
#[must_use]
pub fn slugify(name: &str, sep: Separator) -> String {
    let sep = sep.as_char();
    let mut out = String::with_capacity(name.len());
    let mut pending_sep = false;
    for ch in name.chars() {
        let lowered = ch.to_ascii_lowercase();
        if lowered.is_ascii_lowercase() || lowered.is_ascii_digit() {
            if pending_sep && !out.is_empty() {
                out.push(sep);
            }
            pending_sep = false;
            out.push(lowered);
        } else {
            pending_sep = true;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names on the seed hardware, plus the shapes that make the transform
    /// ambiguous if it is not pinned. These are the fixtures docs/1 D2 promises.
    #[test]
    fn control_slugs_match_the_v4l2_ctl_spelling() {
        let cases = [
            ("Zoom, Continuous", "zoom_continuous"),
            (
                "Region of Interest Rectangle",
                "region_of_interest_rectangle",
            ),
            ("White Balance, Automatic", "white_balance_automatic"),
            ("White Balance Temperature", "white_balance_temperature"),
            ("Auto Exposure", "auto_exposure"),
            ("Exposure Time, Absolute", "exposure_time_absolute"),
            ("Power Line Frequency", "power_line_frequency"),
            ("Focus, Automatic Continuous", "focus_automatic_continuous"),
            ("Pan, Absolute", "pan_absolute"),
        ];
        for (name, want) in cases {
            assert_eq!(slugify(name, Separator::Underscore), want, "name={name}");
        }
    }

    #[test]
    fn runs_of_separators_collapse_and_edges_trim() {
        assert_eq!(slugify("  Foo -- Bar  ", Separator::Underscore), "foo_bar");
        assert_eq!(slugify("---", Separator::Hyphen), "");
        assert_eq!(slugify("A", Separator::Hyphen), "a");
    }

    #[test]
    fn digits_survive_as_their_own_run_members() {
        // The trailing-digit case D1 calls out: "OBSBOT Tiny 3" must not lose its 3, and
        // must not glue it on (which would produce `obsbot-tiny3`).
        assert_eq!(slugify("OBSBOT Tiny 3", Separator::Hyphen), "obsbot-tiny-3");
        assert_eq!(
            slugify("Integrated Camera: Integrated C", Separator::Hyphen),
            "integrated-camera-integrated-c"
        );
    }

    #[test]
    fn non_ascii_collapses_rather_than_leaking_through() {
        assert_eq!(
            slugify("Caméra Intégrée", Separator::Hyphen),
            "cam-ra-int-gr-e"
        );
    }
}
