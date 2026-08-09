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
