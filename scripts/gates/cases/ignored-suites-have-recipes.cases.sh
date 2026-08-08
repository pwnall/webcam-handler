# Both-direction cases for `ignored-suites-have-recipes.sh`.
#
# Both halves are seeded, as docs/4 requires: an `#[ignore]`d test that no suite claims,
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
