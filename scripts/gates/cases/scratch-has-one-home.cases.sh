# Both-direction cases for `scratch-has-one-home.sh`.
#
# Every arm seeds into a `gate_scratch_tree` copy and points `$WCH_GATE_ROOT` at it, which is
# the ordinary idiom here and is also the only one available: three of the four things this
# predicate reads — the shell scripts, the Rust sources, and the copier itself — are files in
# the tree under test, and the fourth (a copy, actually made) is deliberately not injectable.
#
# **What is not seeded is a copier with `--exclude=target` removed and then *run*.** The
# predicate's own header says why, and the arms below hold the same line: the shipped copier
# without that word copies 233 GB into the tree. `fail_case_the_copier_stops_excluding_target`
# seeds the removal and lets the static half catch it, which is the failure the gate exists to
# notice; the dynamic half stays pointed at the real function, so a run of this suite never
# performs the defect.
#
# shellcheck shell=bash
#
# The seeds are literal text written into a copy of a script, so the `$`-expressions in
# them must survive this file unexpanded — which is what the disable says.

pass_case() {
    "$GATE"
}

# A copy of the tree with nothing changed is still green — the arm that separates "this
# predicate is red on the shipped tree" from "this predicate is red on any copy of it",
# which matters here because the predicate makes a copy of its own while it runs.
pass_case_a_pristine_copy_of_the_tree() {
    local tree
    tree="$(gate_scratch_tree)"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# Prose is not code, in both languages. The arguments that defend this rule name every
# spelling it forbids — this file does it four times — so a predicate that could not tell a
# sentence from a call would push the reasoning out of the files that need it.
# shellcheck disable=SC2016
pass_case_prose_may_name_every_forbidden_spelling() {
    local tree
    tree="$(gate_scratch_tree)"
    {
        printf '\n# A comment about ${TMPDIR:-/tmp} and about calling mktemp with no argument.\n'
    } >>"$tree/scripts/gates/lib.sh"
    {
        printf '\n// A comment about tempfile::tempdir() and about TempDir::new().\n'
    } >>"$tree/crates/schema/src/paths.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# An exemption with a reason beside it is allowed, and this arm is the one that keeps the
# escape hatch honest: the three exemptions in the shipped tree are only defensible if the
# mechanism is a documented, counted one rather than whatever the predicate happens to miss.
# shellcheck disable=SC2016
pass_case_an_exempted_line_with_its_reason() {
    local tree
    tree="$(gate_scratch_tree)"
    {
        printf '\n# wch-scratch-exempt: an arm proving the marker works\n'
        printf 'seeded_scratch="${TMPDIR:-/tmp}"\n'
    } >>"$tree/scripts/rung-vivid.sh"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# ------------------------------------------------------------------ the second reach

# The regression this predicate exists for, in the language it is easiest to have it in.
# shellcheck disable=SC2016
fail_case_a_script_reaches_for_the_platform_temporary_directory() {
    local tree
    tree="$(gate_scratch_tree)"
    printf '\nlog="$(mktemp "${TMPDIR:-/tmp}/wch-somewhere.XXXXXXXX")"\n' \
        >>"$tree/scripts/rung-vivid.sh"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# `mktemp -d` with no template is the same defect wearing no `/tmp` at all: the fallback is
# in `mktemp`, not in the line.
# shellcheck disable=SC2016
fail_case_a_script_calls_mktemp_with_no_directory() {
    local tree
    tree="$(gate_scratch_tree)"
    printf '\nwork="$(mktemp -d)"\n' >>"$tree/scripts/rung-web.sh"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The Rust half — `tempfile::tempdir()` typed out of habit, which is the sentence the ruling
# would otherwise be undone by.
fail_case_a_rust_test_calls_tempfile_tempdir() {
    local tree
    tree="$(gate_scratch_tree)"
    printf '\nfn a_habit() { let _dir = tempfile::tempdir().expect("temp dir"); }\n' \
        >>"$tree/crates/engine/src/photo.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The same defect through the constructor that does not say "temp dir" anywhere in it.
fail_case_a_rust_test_calls_temp_dir_new() {
    local tree
    tree="$(gate_scratch_tree)"
    printf '\nfn another_habit() { let _dir = tempfile::TempDir::new().expect("temp dir"); }\n' \
        >>"$tree/crates/testkit/src/corpus.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# ------------------------------------------------------------------ the homes

# Half one is not decoration: with the home deleted, half two is a count of bypasses of
# nothing, and would be green on a tree where every call site had gone back to `$TMPDIR`.
fail_case_the_shell_home_is_deleted() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/^gate_scratch_root()/gate_scratch_root_renamed()/' "$tree/scripts/gates/lib.sh"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_rust_home_is_deleted() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/^pub fn scratch_root()/pub fn scratch_root_renamed()/' \
        "$tree/crates/schema/src/paths.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The two languages drifting apart is the defect that makes one sweep reclaim half of what it
# was written to reclaim — and neither side is wrong on its own, which is why nothing else
# could see it.
fail_case_the_two_languages_name_different_directories() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/^pub const SCRATCH_DIR: &str = "[^"]*"/pub const SCRATCH_DIR: \&str = "somewhere-else"/' \
        "$tree/crates/schema/src/paths.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# ------------------------------------------------------------------ the recursion trap

# The whole reason the static half exists. Seeded and *not* run: the predicate reads the
# copier out of the tree under test, and the copy it makes for its dynamic half is made by
# the real `gate_scratch_tree` this case file is sourced beside.
fail_case_the_copier_stops_excluding_target() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/local excludes=(--exclude=.git --exclude=target)/local excludes=(--exclude=.git)/' \
        "$tree/scripts/gates/lib.sh"
    if awk '/^gate_scratch_tree\(\)/, /^}/' "$tree/scripts/gates/lib.sh" |
        grep -q -- '--exclude=target'; then
        printf 'selftest: the seed did not apply — the copier still excludes target\n' >&2
        return 0
    fi
    WCH_GATE_ROOT="$tree" "$GATE"
}

# And the derived one, which is the half that survives an edit to the line above it.
fail_case_the_copier_stops_excluding_the_scratch_root() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed '/excludes+=(--exclude=/d' "$tree/scripts/gates/lib.sh"
    if awk '/^gate_scratch_tree\(\)/, /^}/' "$tree/scripts/gates/lib.sh" |
        grep -q 'excludes+='; then
        printf 'selftest: the seed did not apply — the copier still derives its own exclusion\n' >&2
        return 0
    fi
    WCH_GATE_ROOT="$tree" "$GATE"
}
