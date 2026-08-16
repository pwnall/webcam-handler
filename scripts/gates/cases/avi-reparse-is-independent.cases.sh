# Both-direction cases for `avi-reparse-is-independent.sh`.
#
# Four claims and every one of them gets its inverse: the pair is the whole population, the
# reader names nothing from the writer, the writer's *product* code names nothing from the
# reader, and both modules are still in the build. The arms are grouped in that order.
#
# **`fail_case_a_third_module_holds_the_shared_layout` is the one this predicate exists for.**
# It is the realistic diff: a `layout.rs` holding the FourCCs both sides had derived separately,
# declared in `avi.rs`, imported by the reader and by the muxer. Every test in this workspace
# passes on that tree — including the `g6` property test that re-parses the muxer's own output,
# which passes *more* comfortably, because two sides that read their constants off one line
# agree by construction. If any arm in this file stops going red, that is the one to look at
# first.
#
# **`pass_case_the_muxers_tests_may_drive_the_reader` is the one that matters most among the
# green arms**, and it is not a courtesy. docs/7 P6a asked for the muxer's output to be checked
# by a re-parse that is not the muxer's code, so `write.rs`'s `#[cfg(test)] mod tests` importing
# `crate::avi::read` **is the criterion**. A predicate that forbade it would forbid the thing it
# exists to protect, and is a predicate somebody turns off.
#
# The seeds are Rust-shaped but never compiled: this predicate reads source, so a seeded
# violation has to be readable rather than buildable, and a case that ran cargo would be
# measuring something else.
#
# shellcheck shell=bash

avi_dir="crates/imaging/src/avi"

pass_case() {
    "$GATE"
}

# ------------------------------------------------- claim 2: the reader names nothing from the muxer

# The ordinary shape: one `use` line at the top of the reader, which is how a shared constant
# arrives in a diff that looks like a cleanup.
fail_case_the_reader_imports_the_writer() {
    local tree file
    tree="$(gate_scratch_tree)"
    file="$tree/$avi_dir/read.rs"
    gate_seed 's|^use crate::fault::imaging_failure;|use super::write::FOURCC_MOVI;\nuse crate::fault::imaging_failure;|' "$file"
    gate_red_because "$avi_dir/read.rs names its sibling module" env "WCH_GATE_ROOT=$tree" "$GATE"
}

# The same reach with no `use` line to grep for: a fully qualified path at the one call site
# that wanted the constant. A rule that only read imports would look straight past this, and
# this is the spelling somebody reaches for precisely because it adds no line to the header.
fail_case_the_reader_reaches_a_writer_constant_by_a_fully_qualified_path() {
    local tree file
    tree="$(gate_scratch_tree)"
    file="$tree/$avi_dir/read.rs"
    gate_seed '/^#\[cfg(test)\]$/i\
/// Seeded by the gate selftest: the muxer as the authority on its own FourCC.\
fn seeded_movi_tag() -> [u8; 4] {\
    crate::avi::write::FOURCC_MOVI\
}\
' "$file"
    gate_red_because "$avi_dir/read.rs names its sibling module" env "WCH_GATE_ROOT=$tree" "$GATE"
}

# ------------------------------------------------- claim 3: the muxer's product code, only

fail_case_the_writer_imports_the_reader_in_product_code() {
    local tree file
    tree="$(gate_scratch_tree)"
    file="$tree/$avi_dir/write.rs"
    gate_seed 's|^use crate::fault::imaging_failure;|use super::read::read_stream;\nuse crate::fault::imaging_failure;|' "$file"
    gate_red_because "$avi_dir/write.rs names its sibling module" env "WCH_GATE_ROOT=$tree" "$GATE"
}

# The green direction for the same claim, and the arm this file's header calls the one that
# matters. `write.rs`'s `mod tests` already imports `crate::avi::read`; this seeds a second use
# of it inside that module, so the arm is about the *rule* rather than about the line that
# happens to be there.
pass_case_the_muxers_tests_may_drive_the_reader() {
    local tree file
    tree="$(gate_scratch_tree)"
    file="$tree/$avi_dir/write.rs"
    # Before the file's last line, which is the closing brace of its one `mod tests`.
    #
    # `$i` is `sed`'s insert-before-the-last-line command and not a shell expansion, so the
    # single quotes are the point rather than an oversight; the directive is here rather than
    # in a `.shellcheckrc` because it is true of this argument and of nothing else in the file.
    # shellcheck disable=SC2016
    gate_seed '$i\
\
    #[test]\
    fn seeded_by_the_gate_selftest() {\
        let parsed = crate::avi::read::read_stream(&[]);\
        assert!(parsed.is_err());\
    }' "$file"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The other green direction, and the reason line comments are stripped: `read.rs` and `avi.rs`
# argue about the muxer by name for dozens of lines, and `avi.rs`'s module doc spells out the
# very reach this gate refuses in order to explain why it is refused. A predicate that read
# prose as code would push that argument out of the modules that carry it.
pass_case_a_comment_may_name_the_other_implementation() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/$avi_dir/read.rs" <<'RS'

// Nothing here reaches for anything: a shared FourCC would be `use super::write::FOURCC_MOVI;`
// at the top of this file, or `crate::avi::write::FOURCC_MOVI` at the one call site that wanted
// it, and either one is the independence docs/7 P6a asked for spent on a saved line.
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

# ------------------------------------------------- claim 1: the pair is the whole population

# **The arm this gate exists for**, written as the diff that would actually land: the FourCCs
# both sides had derived separately, factored into a third module, declared in `avi.rs`, and
# imported by each. It is red under three claims at once — a third file, and a sibling reference
# from each of the two — so the assertion names the third-file sentence, which is the one no
# other arm here produces and the one that would still fire if somebody spelled the sharing
# without an import.
fail_case_a_third_module_holds_the_shared_layout() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/$avi_dir/layout.rs" <<'RS'
//! Seeded by the gate selftest: the RIFF/AVI tags, in one place instead of two.

pub(super) const FOURCC_MOVI: [u8; 4] = *b"movi";
pub(super) const FOURCC_IDX1: [u8; 4] = *b"idx1";
RS
    gate_seed 's|^pub mod read;|mod layout;\n\npub mod read;|' "$tree/crates/imaging/src/avi.rs"
    gate_seed 's|^use crate::fault::imaging_failure;|use super::layout::FOURCC_MOVI;\nuse crate::fault::imaging_failure;|' \
        "$tree/$avi_dir/read.rs" "$tree/$avi_dir/write.rs"
    gate_red_because 'is a third module beside' env "WCH_GATE_ROOT=$tree" "$GATE"
}

# The pair by name. A reader renamed out from under this gate takes docs/7 P6a's subject with
# it, and the remaining file would answer every other claim on its own.
fail_case_the_reader_was_renamed_away() {
    local tree
    tree="$(gate_scratch_tree)"
    mv "$tree/$avi_dir/read.rs" "$tree/$avi_dir/parse.rs"
    gate_red_because "$avi_dir/read.rs is missing" env "WCH_GATE_ROOT=$tree" "$GATE"
}

# The subject gone entirely. A predicate that answered PASS here would be reporting that the
# independence holds over a directory that is not there.
fail_case_the_pair_is_not_in_the_tree_at_all() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -rf "${tree:?}/$avi_dir"
    gate_red_because "$avi_dir is not a directory in this tree" env "WCH_GATE_ROOT=$tree" "$GATE"
}

# ------------------------------------------------- the boundary this gate reads a file by

# Two `#[cfg(test)]` markers, so "which half of this file is product code" has no answer — and
# claim 3's exemption, which is the whole reason the split is read at all, would then cover the
# entire file. A file nobody can classify is a finding rather than a pass.
fail_case_a_module_boundary_has_two_answers() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/$avi_dir/write.rs" <<'RS'

#[cfg(test)]
mod more_tests {
    use super::*;

    #[test]
    fn seeded_by_the_gate_selftest() {
        assert!(PROVISIONAL_INTERVAL_US > 0);
    }
}
RS
    gate_red_because 'cannot tell its product code from its test code' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

# ------------------------------------------------- claim 4: both modules are still compiled

# One deleted line, no compile error, and the reader stops being the thing the `g6` property
# test re-parses through — while every other claim in this predicate stays green.
fail_case_a_module_is_no_longer_declared() {
    local tree file
    tree="$(gate_scratch_tree)"
    file="$tree/crates/imaging/src/avi.rs"
    gate_seed '/^pub mod read;$/d' "$file"
    gate_red_because 'crates/imaging/src/avi.rs no longer declares' env "WCH_GATE_ROOT=$tree" "$GATE"
}

# The same absence one level up, and cheaper: the whole pair leaves the build in one line while
# the crate still compiles, because an orphaned `.rs` file is not an error in Rust.
fail_case_the_crate_root_no_longer_declares_the_parent() {
    local tree file
    tree="$(gate_scratch_tree)"
    file="$tree/crates/imaging/src/lib.rs"
    gate_seed '/^pub mod avi;$/d' "$file"
    gate_red_because 'crates/imaging/src/lib.rs no longer declares' env "WCH_GATE_ROOT=$tree" "$GATE"
}

# ------------------------------------------------- the crate itself

# Seeded through the metadata rather than by deleting the crate, because deleting it makes
# `cargo metadata` itself fail — and an arm that goes red before the predicate runs proves
# nothing about the predicate.
fail_case_the_imaging_crate_left_the_workspace() {
    local md
    md="$(gate_metadata_snapshot)"
    jq 'del(.packages[] | select(.name == "webcam-handler-imaging"))' "$md" >"$md.seeded"
    gate_red_because 'is not a workspace member' env "WCH_GATE_METADATA=$md.seeded" "$GATE"
}
