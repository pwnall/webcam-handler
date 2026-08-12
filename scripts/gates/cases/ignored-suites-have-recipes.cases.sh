# Both-direction cases for `ignored-suites-have-recipes.sh`.
#
# Both halves are seeded, as docs/9 requires: an `#[ignore]`d test that no suite claims,
# and a suite declaration whose recipe does not exist or does not run the script that
# declared it.
#
# shellcheck shell=bash

pass_case() {
    "$GATE"
}

# The green arm that matters once P1 lands suites: an ignored test whose name matches a
# declared prefix is fine, and the gate must not object to the hardware rung existing.
pass_case_ignored_test_named_by_a_declared_suite() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/backends/v4l2/src/hardware_smoke.rs" <<'RS'
//! Seeded by the gate selftest.
#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "needs a camera; run with `just smoke-hw`"]
    fn hw_enumeration_matches_the_committed_profile() {}
}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

# ------------------------------------------- the attribute in prose, both directions (N72)
#
# The ignored-test half used to match the token wherever it appeared, so a `//` or `///` line
# that named it made the file's next `fn` an unrouted ignored test. It fails closed, so the
# cost was never a missed test — it was a tax on the essay comments this project's rubric
# asks for, and note **N72** paid it three times in one commit. These two arms are the
# tolerance and its limit, and the pair is the point: a stripper that reads prose correctly
# and code incorrectly is a worse gate than the one it replaced, because the first arm below
# would pass and nothing would notice.

pass_case_the_attribute_is_named_in_prose_and_not_read_as_one() {
    # Every shape the reducer has to handle, in one file: a module doc, a doc comment, a
    # trailing comment, a block comment on one line, a block comment spanning several, and a
    # string literal. None of them is an attribute, and the `fn` beneath each is a plain
    # function no suite prefix matches — so under the old rule this file alone was six
    # findings.
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/schema/src/prose.rs" <<'RS'
//! Seeded by the gate selftest: this module's tests are `#[ignore]`d by construction.
fn module_doc_named_it() {}

/// Runs under `#[ignore]` on a machine with a camera attached.
fn doc_comment_named_it() {}

fn trailing_comment_named_it() {} // and it is `#[ignore]`d over there

/* one line of #[ignore] in a block comment */
fn one_line_block_named_it() {}

/*
 * Several lines, and the middle one writes #[ignore] out in full.
 */
fn many_line_block_named_it() {}

fn a_string_named_it() {
    let _ = "#[ignore]";
}

// The `fn` below is what makes the string above load-bearing: the old rule needed a
// following function to misattribute the token to, and a seeded shape with nothing after it
// proves only that the file ended.
fn follows_the_string() {}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_real_ignored_test_hidden_among_prose_about_the_attribute() {
    # The limit of the tolerance. The same prose as the arm above, and one genuine unrouted
    # `#[ignore]`d test underneath it: a reducer that threw away too much of the line would
    # let this through, and this arm is what says so.
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/schema/src/prose_and_a_test.rs" <<'RS'
//! Seeded by the gate selftest: prose about `#[ignore]`, and then the real thing.
/// The word appears here too, inside `#[ignore]`, and here: "#[ignore]".
/* and once more, in a block: #[ignore] */
#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "no recipe runs this, which is the point"]
    fn hidden_in_the_prose_and_nobody_runs_it() {}
}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_an_attribute_sharing_a_line_with_a_block_comment_that_closed() {
    # The reducer resumes after `*/` rather than dropping the rest of the line, and this is
    # the only arm that can say so: the file's one unrouted test is declared on a line whose
    # first characters are a comment. Stop reading at `*/` and this file holds no ignored test
    # at all, the gate goes green, and the harness reports the arm as a problem.
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/schema/src/after_a_block.rs" <<'RS'
//! Seeded by the gate selftest.
#[cfg(test)]
mod tests {
    #[test]
    /* R3, one day */ #[ignore = "no recipe runs this either"]
    fn declared_after_a_closed_block_comment() {}
}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_ignored_test_no_suite_claims() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/schema/src/orphan.rs" <<'RS'
//! Seeded by the gate selftest.
#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "no recipe runs this, which is the point"]
    fn orphaned_suite_nobody_runs() {}
}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_ignored_test_outside_every_package() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/stray.rs" <<'RS'
//! Seeded by the gate selftest: a test file in no workspace member.
#[test]
#[ignore = "unreachable by any selection"]
fn hw_stray() {}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_declaration_names_a_recipe_that_does_not_exist() {
    local tree script
    tree="$(gate_scratch_tree)"
    script="$tree/scripts/smoke-hw.sh"
    sed 's/^# wch-suite: prefix=hw_ recipe=.*/# wch-suite: prefix=hw_ recipe=no-such-recipe/' \
        "$script" >"$script.seeded"
    mv "$script.seeded" "$script"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_recipe_does_not_run_the_script_that_declares_it() {
    local tree
    tree="$(gate_scratch_tree)"
    sed 's|\./scripts/smoke-hw\.sh|echo "not running the suite"|' "$tree/justfile" \
        >"$tree/justfile.seeded"
    mv "$tree/justfile.seeded" "$tree/justfile"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_no_suite_declarations_at_all() {
    local tree script
    tree="$(gate_scratch_tree)"
    for script in "$tree"/scripts/*.sh; do
        grep -v '^# wch-suite:' "$script" >"$script.seeded"
        mv "$script.seeded" "$script"
    done
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_justfile_deleted() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$tree/justfile"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# --------------------------------------- the exclusive-device test group, both directions
#
# The failure this half prevents is a *flake*: two hardware tests streaming from one
# camera, the loser reporting a correct `EBUSY`. A flake gets re-run rather than read, so
# the serialisation needs a gate rather than a memory.

fail_case_no_nextest_config_to_serialise_the_hardware_suites() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$tree/.config/nextest.toml"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_test_group_does_not_cap_itself_at_one_thread() {
    local tree
    tree="$(gate_scratch_tree)"
    sed 's/^max-threads = 1$/max-threads = 4/' "$tree/.config/nextest.toml" \
        >"$tree/.config/nextest.toml.seeded"
    mv "$tree/.config/nextest.toml.seeded" "$tree/.config/nextest.toml"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_declared_suite_prefix_is_outside_the_test_group() {
    local tree
    tree="$(gate_scratch_tree)"
    sed 's/ + test(\/(^|::)vivid_\/)//' "$tree/.config/nextest.toml" \
        >"$tree/.config/nextest.toml.seeded"
    mv "$tree/.config/nextest.toml.seeded" "$tree/.config/nextest.toml"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_group_is_assigned_but_never_defined() {
    local tree
    tree="$(gate_scratch_tree)"
    sed 's/^\[test-groups\.exclusive-device\]$/[test-groups.something-else]/' \
        "$tree/.config/nextest.toml" >"$tree/.config/nextest.toml.seeded"
    mv "$tree/.config/nextest.toml.seeded" "$tree/.config/nextest.toml"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_group_is_assigned_under_a_profile_the_recipes_never_use() {
    # `just ci` and `just smoke-hw` pass no `--profile`, so an override under
    # `[[profile.ci.overrides]]` serialises nothing while looking exactly right.
    local tree
    tree="$(gate_scratch_tree)"
    sed 's/^\[\[profile\.default\.overrides\]\]$/[[profile.ci.overrides]]/' \
        "$tree/.config/nextest.toml" >"$tree/.config/nextest.toml.seeded"
    mv "$tree/.config/nextest.toml.seeded" "$tree/.config/nextest.toml"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_filter_subtracts_what_it_appears_to_include() {
    # `test(/hw_/) - test(/hw_/)` mentions the prefix and holds none of it. The gate does
    # not reimplement nextest's expression language; it refuses to vouch for one it cannot
    # read, which is the honest answer and the safe one.
    local tree
    tree="$(gate_scratch_tree)"
    sed "s|^filter = .*|filter = 'test(/(^\|::)hw_/) + test(/(^\|::)vivid_/) - test(/(^\|::)hw_/)'|" \
        "$tree/.config/nextest.toml" >"$tree/.config/nextest.toml.seeded"
    mv "$tree/.config/nextest.toml.seeded" "$tree/.config/nextest.toml"
    WCH_GATE_ROOT="$tree" "$GATE"
}
