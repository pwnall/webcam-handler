# Both-direction cases for `doc-comments-open-with-a-summary.sh`.
#
# The failing arms reproduce the shape of the B8 splice rather than inventing a violation: a
# summary paragraph replaced by the section heading that followed it, which is what is left of
# an item whose prose was taken by the item written above it. The passing arms hold the two
# directions the rule must not take with it — a heading *below* a summary is a section, and a
# module header is not an item's summary at all.
#
# shellcheck shell=bash

pass_case() {
    "$GATE"
}

# A shape the predicate must ALLOW, and the shape nearly every documented `Result`-returning
# function in this workspace has: a summary, a blank doc line, then `# Errors`. Seeded rather
# than merely relied on, because a predicate that matched a heading anywhere in a block would be
# red on the shipped tree and this arm says which half of that is checked.
pass_case_a_heading_below_a_summary_is_a_section() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/engine/src/documented.rs" <<'RS'
//! Seeded by the gate selftest: an item documented the way this workspace documents them.

/// What this does, in one line.
///
/// # Errors
///
/// Never; it is here so the arm has a section heading to not trip over.
pub fn documented() -> Result<(), ()> {
    Ok(())
}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The other direction the rule must not take with it: `//!` documents a *file*, is never printed
# by clap, and several module headers in this tree open on their own structure.
pass_case_a_module_header_is_not_an_items_summary() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/engine/src/headed.rs" <<'RS'
//! # A module header that opens on a heading
//!
//! Which is a style question about a file rather than a lost sentence about an item.
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The defect itself: `budget_ms`'s summary taken by the item written above it, leaving a bare
# `# Errors` where a sentence was. This is the B8 splice, seeded back into a copy.
fail_case_the_summary_was_taken_by_the_item_above_it() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^    /// How long this recording may run, in milliseconds\.$|    /// # Errors|' \
        "$tree/crates/schema/src/video.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The same violation under a deeper heading, so the check is about "the block opens on a
# heading" rather than about the one hash count the splice happened to leave.
fail_case_a_deeper_heading_is_still_a_missing_summary() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/engine/src/beheaded.rs" <<'RS'
//! Seeded by the gate selftest: an item whose first paragraph went somewhere else.

/// ## Why this is ours and not the writer's
///
/// The argument survived the edit and the sentence naming the function did not.
pub fn beheaded() {}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The non-vacuity arm: a walk that reads no doc comments proves nothing, and this claim would
# otherwise be true of a tree with none.
fail_case_nothing_to_read() {
    local tree
    tree="$(gate_scratch_tree)"
    find "$tree" -name '*.rs' -delete
    WCH_GATE_ROOT="$tree" "$GATE"
}
