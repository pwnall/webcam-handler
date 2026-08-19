# Both-direction cases for `luma-has-one-home.sh`.
#
# Four claims, and every one of them gets its inverse: the home exists and there is one of it,
# nobody borrows another crate's luma, no second set of coefficients lands anywhere, and the
# register of consumers reconciles in both directions. The arms are grouped in that order.
#
# **`fail_case_the_comparison_reader_converts_colour_itself` is the one this predicate exists
# for.** It seeds the tree as it stood before note **N266**: `compare::read` reaching `to_luma8`
# for JPEG and PNG while the Netpbm arm eight lines below it used the crate's own BT.601. That
# build passed every test in this workspace — the round-trip walk that covers `PhotoFormat::ALL`
# feeds a *grey* fixture, and grey is a fixed point of every luma definition there is — and
# answered `ssim 0.9688` comparing one scene's PNG against its own PPM. If any arm in this file
# stops going red, that is the one to look at first.
#
# **Four arms are what make claim 2 a ban on the class** (note **N249**, rubric A17), and it took
# all four because a family has more than one axis to vary along.
# `fail_case_a_new_module_reaches_for_someone_elses_luma` varies the *verb* — it seeds
# `into_luma8`, which is not the spelling that was found;
# `fail_case_the_same_conversion_is_reached_by_its_full_path` varies the *call syntax*, which is
# the axis every earlier arm held fixed while the rule was anchored on a literal dot; and
# the two `greyer.rs` arms vary the *name*, reaching `image`'s Rec. 709 through
# `ConvertBuffer::convert` and `FromColor::from_color`, neither of which contains the word
# "luma". An arm that only ever seeded `to_luma8` would leave a rule matching one member of a
# family reading as if it matched the family.
#
# **`fail_case_a_second_colour_model_lands_in_the_home_file` is claim 3's blind spot, closed.**
# The home may carry its own weights; the obvious way to say so — exempting the home *file* —
# stops the four standard sets from ever being checked against `decode.rs`, and a Rec. 709
# definition written inside the one home is then invisible to every claim this predicate makes.
#
# The green arm below the first pair matters for the mirror-image reason: an arm asserting that
# two colour models differ has to be able to name the other one, and a gate that refused a test
# for saying `to_luma8` is a gate somebody turns off.
#
# shellcheck shell=bash

pass_case() {
    "$GATE"
}

# ------------------------------------------------- claim 2: nobody borrows another crate's luma

# A suite may name the other colour model, and this is what that looks like: the arm in
# `compare.rs` that holds every format to a committed BT.601 table exists precisely because
# Rec. 709 answers differently, and stating the difference means writing it down.
pass_case_a_test_may_name_the_other_colour_model() {
    local tree file
    tree="$(gate_scratch_tree)"
    file="$tree/crates/imaging/src/compare.rs"
    # Before the file's last line, which is the closing brace of its one `mod tests`.
    #
    # `$i` is `sed`'s insert-before-the-last-line command and not a shell expansion, so the
    # single quotes are the point rather than an oversight.
    # shellcheck disable=SC2016
    gate_seed '$i\
\
    #[test]\
    fn seeded_by_the_gate_selftest() {\
        let bars = fixtures::colour_bars(8, 8);\
        let other = image::DynamicImage::ImageRgb8(bars).to_luma8();\
        assert_eq!(other.dimensions(), (8, 8));\
    }' "$file"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The defect itself, seeded back into the product code it was found in.
fail_case_the_comparison_reader_converts_colour_itself() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's@Ok(crate::decode::rgb_to_luma(&decoded.to_rgb8()))@Ok(decoded.to_luma8())@' \
        "$tree/crates/imaging/src/compare.rs"
    gate_red_because "crates/imaging/src/compare.rs calls the \`image\` crate's own luma conversion" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# The same defect under a different member of the family, in a module with no test region at all
# — which is the other branch of the product/test split, and the arm that says this rule matches
# a shape rather than a spelling.
fail_case_a_new_module_reaches_for_someone_elses_luma() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/imaging/src/thumbnail.rs" <<'RS'
//! Seeded by the gate selftest: colour becomes brightness in exactly one place.

use image::GrayImage;

pub(crate) fn brightness(bytes: &[u8]) -> Option<GrayImage> {
    Some(image::load_from_memory(bytes).ok()?.into_luma8())
}
RS
    gate_red_because "crates/imaging/src/thumbnail.rs calls the \`image\` crate's own luma conversion" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# The same method, reached by its full path — which is the form rustc itself prints when it
# refuses the method-call spelling in a `map`, so it is what an author writes next rather than an
# exotic evasion. `fail_case_a_new_module_reaches_for_someone_elses_luma` varies the *verb* and
# this one varies the *call syntax*, and it took both before claim 2's ban was on the family.
fail_case_the_same_conversion_is_reached_by_its_full_path() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's@Ok(crate::decode::rgb_to_luma(&decoded.to_rgb8()))@Ok(image::DynamicImage::to_luma8(\&decoded))@' \
        "$tree/crates/imaging/src/compare.rs"
    gate_red_because "crates/imaging/src/compare.rs calls the \`image\` crate's own luma conversion" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# The whole-buffer door, which names no luma at all: `ConvertBuffer::convert` into a `GrayImage`
# is byte-for-byte `to_luma8` — measured against `image` 0.25.10 on the same pixels, `[0,255,0]`
# answers 182 through both and 149 through this workspace's home — and it is the most idiomatic
# way an author gets a `GrayImage` out of an `RgbImage`. Claim 4's register cannot stand in for
# this: a file that reached *around* the home never names it, so the register never sees it.
fail_case_a_new_module_converts_the_whole_buffer_through_someone_elses_trait() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/imaging/src/greyer.rs" <<'RS'
//! Seeded by the gate selftest: somebody else's colour model, with no luma in the name.

use image::buffer::ConvertBuffer;
use image::{GrayImage, RgbImage};

pub(crate) fn brightness(rgb: &RgbImage) -> GrayImage {
    rgb.convert()
}
RS
    gate_red_because "crates/imaging/src/greyer.rs calls \`image\`'s whole-buffer conversion" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# The per-pixel door underneath both of the others: `FromColor::from_color` is where `image`
# actually writes `SRGB_LUMA`, so a file that reaches it has the Rec. 709 arithmetic without
# having typed a coefficient claim 3 could see or a name claim 2 used to match.
fail_case_a_pixel_is_converted_through_someone_elses_colour_space() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/imaging/src/greyer.rs" <<'RS'
//! Seeded by the gate selftest: one pixel at a time, through somebody else's colour space.

use image::{FromColor, Luma, Rgb};

pub(crate) fn brightness(pixel: &Rgb<u8>) -> Luma<u8> {
    let mut grey = Luma([0u8]);
    grey.from_color(pixel);
    grey
}
RS
    gate_red_because "crates/imaging/src/greyer.rs calls \`image\`'s per-pixel colour conversion" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# A file this predicate cannot classify is a failure and not a pass — `unsafe-scope.sh` already
# charges that price for a count it cannot read. Two `#[cfg(test)]` markers means the "one
# trailing test module" rule has no answer about where this file's product code ends, and every
# claim above would be reported off a boundary this guessed at.
fail_case_a_second_test_module_makes_the_boundary_unreadable() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/crates/imaging/src/compare.rs" <<'RS'

#[cfg(test)]
mod more_tests {
    use super::*;

    #[test]
    fn seeded_by_the_gate_selftest() {
        assert!(!READ_OP.is_empty());
    }
}
RS
    gate_red_because 'names a colour-to-brightness conversion and this gate cannot tell its product code from its test code' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# ------------------------------------------------- claim 3: no second set of coefficients

# The textbook constant a fresh author reaches for, spread over three lines rather than one —
# because three coefficients on three lines is a colour model just as much as three on one.
fail_case_a_textbook_coefficient_set_lands_in_another_file() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/imaging/src/preview_luma.rs" <<'RS'
//! Seeded by the gate selftest: a second definition of luma, written out longhand.

pub(crate) fn brightness(r: u8, g: u8, b: u8) -> u8 {
    let red = u32::from(r) * 299;
    let green = u32::from(g) * 587;
    let blue = u32::from(b) * 114;
    u8::try_from((red + green + blue) / 1000).unwrap_or(u8::MAX)
}
RS
    gate_red_because "crates/imaging/src/preview_luma.rs's product code carries all of \`299 587 114\`" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# The other half of claim 3, and the half that is *derived*: the home's own weights, copied. No
# list in the predicate names these numbers — they are read out of `luma_sample`'s body — so this
# arm is also what proves that reading happened.
fail_case_the_homes_own_weights_are_copied_into_another_file() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/imaging/src/preview_luma.rs" <<'RS'
//! Seeded by the gate selftest: the home's arithmetic, restated a second time.

pub(crate) fn brightness(r: u8, g: u8, b: u8) -> u8 {
    let weighted = 77 * u32::from(r) + 150 * u32::from(g) + 29 * u32::from(b);
    u8::try_from(weighted >> 8).unwrap_or(u8::MAX)
}
RS
    gate_red_because "crates/imaging/src/preview_luma.rs's product code carries all of \`29 77 150\`" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# A second *standardised* colour model inside the one home, which is the file every other claim
# here trusts: claim 1 looks for `luma_sample` and `rgb_to_luma` and a third function is neither,
# and claim 4 has `decode.rs` registered. Exempting the home file from the coefficient sets — the
# obvious spelling of "the home may carry its own weights" — makes this invisible everywhere.
fail_case_a_second_colour_model_lands_in_the_home_file() {
    local tree
    tree="$(gate_scratch_tree)"
    # Into the *product* half, ahead of `luma_to_rgb`. Appending to the file would put it below
    # the trailing `mod tests` and be stripped, which is a seed that proves the stripping rather
    # than the exemption — the shape note **N235** calls a skip that reads as a pass.
    #
    # `\i` is `sed`'s insert-before-the-matched-line command and not a shell expansion, so the
    # single quotes are the point rather than an oversight.
    # shellcheck disable=SC2016
    gate_seed '/^fn luma_to_rgb(/i\
pub(crate) fn preview_luma(r: u8, g: u8, b: u8) -> u8 {\
    let weighted = 2126 * u32::from(r) + 7152 * u32::from(g) + 722 * u32::from(b);\
    u8::try_from(weighted / 10000).unwrap_or(u8::MAX)\
}\
' "$tree/crates/imaging/src/decode.rs"
    gate_red_because "crates/imaging/src/decode.rs's product code carries all of \`2126 7152 722\`" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# The derivation's own inverse: a home whose scalar has stopped weighting three channels is a
# home this gate can no longer read a triple out of, and claim 3's derived half would quietly
# become a claim about nothing.
fail_case_the_home_stops_weighting_three_channels() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/let weighted = 77 \* u32::from(r) + 150 \* u32::from(g) + 29 \* u32::from(b);/let weighted = 128 * u32::from(g) + 128 * u32::from(b);/' \
        "$tree/crates/imaging/src/decode.rs"
    gate_red_because 'multiplies by 1 distinct literal(s) and a luma conversion weights three channels' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# ------------------------------------------------- claim 1: the home exists, and there is one

# Renamed, the scalar has no declaration, no callers and no coefficients to read — and a
# confinement over an empty population is the vacuous green this suite exists to refuse.
fail_case_the_scalar_was_renamed() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/pub(crate) fn luma_sample(/pub(crate) fn luma_of(/' \
        "$tree/crates/imaging/src/decode.rs"
    gate_red_because 'no longer declares the scalar that turns one colour sample into one brightness' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# A second definition under the *same* name, which is the version of this defect a reader is
# least likely to catch: two `rgb_to_luma`s in one crate, and every call site looks correct.
fail_case_a_second_definition_takes_the_homes_name() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/imaging/src/preview_luma.rs" <<'RS'
//! Seeded by the gate selftest: the home's name, a second time, over different arithmetic.

use image::{GrayImage, RgbImage};

pub(crate) fn rgb_to_luma(image: &RgbImage) -> GrayImage {
    let mut out = GrayImage::new(image.width(), image.height());
    for (destination, source) in out.pixels_mut().zip(image.pixels()) {
        let [r, g, b] = source.0;
        let weighted = u32::from(r) + u32::from(g) + u32::from(b);
        destination.0 = [u8::try_from(weighted / 3).unwrap_or(u8::MAX)];
    }
    out
}
RS
    # shellcheck disable=SC2016  # the predicate's own message, backticks and all
    gate_red_because 'crates/imaging/src/preview_luma.rs declares `luma_sample` or `rgb_to_luma` in its product code' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# ------------------------------------------------- claim 4: the register, both ways

# Forward: a fifth consumer of the colour model, correct in itself, that nobody declared.
fail_case_an_unregistered_module_converts_colour() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/imaging/src/thumbnail.rs" <<'RS'
//! Seeded by the gate selftest: a new consumer of the crate's colour model.

use image::{GrayImage, RgbImage};

pub(crate) fn brightness(image: &RgbImage) -> GrayImage {
    crate::decode::rgb_to_luma(image)
}
RS
    gate_red_because 'crates/imaging/src/thumbnail.rs names this crate'"'"'s colour model in its product code and is not one of this gate'"'"'s 2 registered consumers' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# **Backward, and this is the direction the gate was written for.** A registered consumer that
# has stopped naming the home is exactly what the defect looked like: `compare.rs` on the
# register, converting colour, and doing it with arithmetic of its own. Seeded here as the
# whole-file reach — both colour paths rewritten — because the register is about the file.
fail_case_a_registered_consumer_stopped_naming_the_home() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's@Ok(crate::decode::rgb_to_luma(&decoded.to_rgb8()))@Ok(decoded.into_luma8())@' \
        "$tree/crates/imaging/src/compare.rs"
    gate_seed 's@crate::decode::luma_sample(@local_luma(@' "$tree/crates/imaging/src/compare.rs"
    gate_red_because 'crates/imaging/src/compare.rs is registered as the comparison reader' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# Every claim above is true of a tree in which nothing calls the home at all, and that tree is a
# worse defect than any of them: a colour model with no callers is one every consumer has
# replaced. `kill-is-never-a-fallback.sh`'s "the only caller went away" arm is the model.
fail_case_nothing_calls_the_scalar_any_more() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/destination.0 = \[luma_sample(r, g, b)\];/destination.0 = [u8::try_from((u32::from(r) + u32::from(g) + u32::from(b)) \/ 3).unwrap_or(u8::MAX)];/' \
        "$tree/crates/imaging/src/decode.rs"
    gate_seed 's@crate::decode::luma_sample(@local_luma(@' "$tree/crates/imaging/src/compare.rs"
    # shellcheck disable=SC2016  # the predicate's own message, backticks and all
    gate_red_because 'nothing in this workspace'"'"'s product code calls `luma_sample(`' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}
