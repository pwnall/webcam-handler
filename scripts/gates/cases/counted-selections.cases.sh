# Both-direction cases for `counted-selections.sh`.
#
# The pass arm runs the real `cargo nextest list` over the real workspace — the only
# listing that proves anything about the shipped selections. The failing arms use a stub
# lister so that a seeded table can be checked without rebuilding a copy of the workspace
# for each violation shape; what they exercise is the predicate's arithmetic and its
# error handling, which is where the `grep -c` zero-match trap lives.
#
# shellcheck shell=bash

# A lister that answers the way nextest **actually** answers, which is not the way the
# first version of this file assumed.
#
# `nextest list -T json` lists the *entire* workspace whatever `-E` says, and marks each
# testcase `matches` or `mismatch`; `test-count` is the size of that whole listing. The
# original stub returned `{"test-count":0,"rust-suites":{}}` for a non-matching filter — a
# shape nextest never produces — and the predicate agreed with the stub, so a gate that
# could not report zero for any real input passed its own both-directions selftest. PF:15
# records the same lesson about a Python probe: a second implementation only catches what
# it distinguishes. See note N10.
_stub_lister() {
    local path="$1"
    cat >"$path" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
# One matched test for the schema package; for anything else, the same full listing
# with the entry marked `mismatch` — which is what the tool does.
case "$1" in
*webcam-handler-schema*)
    printf '%s' '{"test-count":1,"rust-suites":{"webcam-handler-schema":{"testcases":{"slug::round_trips":{"kind":"test","ignored":false,"filter-match":{"status":"matches"}}}}}}'
    ;;
*)
    printf '%s' '{"test-count":1,"rust-suites":{"webcam-handler-schema":{"testcases":{"slug::round_trips":{"kind":"test","ignored":false,"filter-match":{"status":"mismatch","reason":"expression"}}}}}}'
    ;;
esac
SH
    chmod +x "$path"
}

# A table whose only `tests` row is this one, so an arm about one row is not also an arm about
# the hundred the stub lister would answer `mismatch` for.
#
#   $1  the scratch tree
#   $2  the selection
#   $3  the `what` column
_only_tests_row() {
    local tree="$1" selection="$2" what="$3" table
    table="$tree/scripts/gates/phase-criteria.tsv"
    grep -v '	tests	' "$table" >"$table.seeded"
    printf 'g0\ttests\t%s\t%s\n' "$selection" "$what" >>"$table.seeded"
    mv "$table.seeded" "$table"
}

pass_case() {
    "$GATE"
}

# The green direction for the branch check, and the reason it is here rather than left to
# `pass_case`: a splitter that silently produced *no* branches would satisfy every failing arm
# below by never disagreeing with anything. Both branches of this row select, and the row is the
# only `tests` row in the table it is checked in.
pass_case_every_branch_of_an_alternation_selects() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" 'package(webcam-handler-schema) and test(/^(slug::round|slug::)/)' \
        'two branches, both of which the lister answers for'
    WCH_GATE_ROOT="$tree" WCH_GATE_NEXTEST_LIST="$lister" "$GATE"
}

# **The P5 review's finding.** The row still selects a test, so the zero-selection check above is
# green and always would have been; what has rotted is one of the two claims its `what` column
# describes. Eleven `g5` rows are alternations of two to five named claims each, so this is the
# shape a criterion decays into rather than a hypothetical one.
fail_case_a_branch_of_an_alternation_selects_nothing() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" \
        'package(webcam-handler-schema) and test(/^(slug|zzz_no_such_module)::/)' \
        'two claims, one of which nothing selects any more'
    gate_red_because "none of them is one its branch 'test(/^zzz_no_such_module::/)' names" \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_NEXTEST_LIST=$lister" "$GATE"
}

# A regex this gate cannot split is a **failure and not a pass**, which is the price
# `gate_test_region_start` charges for a file boundary it cannot read, charged here for an
# alphabet. Green would mean "some of these claims are unchecked and nothing says which".
fail_case_an_alternation_this_gate_cannot_split() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" 'package(webcam-handler-schema) and test(/^(slug|zzz[0-9]+)::/)' \
        'an alternation whose branches this gate has no rule for'
    gate_red_because 'outside the alphabet' \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_NEXTEST_LIST=$lister" "$GATE"
}

# The other way a splitter can be handed something it cannot answer about. Inside the alphabet,
# so the arm above does not fire and this branch is the one under test.
fail_case_an_alternation_with_unbalanced_parentheses() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" 'package(webcam-handler-schema) and test(/^(slug|zzz::/)' \
        'an alternation with a parenthesis nobody closed'
    gate_red_because 'unbalanced parentheses' \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_NEXTEST_LIST=$lister" "$GATE"
}

fail_case_selection_matches_no_tests() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    printf 'g0\ttests\ttest(no_such_test_exists)\ta criterion whose filter has rotted\n' \
        >>"$tree/scripts/gates/phase-criteria.tsv"
    # The seeded row by name. The stub answers `mismatch` for every row it was not written for,
    # so this arm is loudly red about the whole table as well; what it is *named* for is the one
    # row it added, and that is the sentence it claims.
    gate_red_because "g0 selection 'test(no_such_test_exists)' selects zero tests" \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_NEXTEST_LIST=$lister" "$GATE"
}

# The arm the stub cannot provide: the *real* tool, and a filter that matches nothing.
#
# The stub above encodes our belief about nextest's output; this encodes nextest's. If a
# release changes the shape — drops `filter-match`, renames the status — the predicate
# starts counting the whole workspace again and this is the only case that notices.
fail_case_a_real_selection_that_matches_nothing_counts_zero() {
    local tree
    tree="$(gate_scratch_tree)"
    printf 'g0\ttests\tpackage(webcam-handler-schema) and test(/^zzz_no_such_module::/)\ta filter that has rotted\n' \
        >>"$tree/scripts/gates/phase-criteria.tsv"
    gate_red_because "g0 selection 'package(webcam-handler-schema) and test(/^zzz_no_such_module::/)' selects zero tests" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_lister_cannot_answer() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_red_because 'a selection that cannot be listed is not a selection that was checked' \
        env WCH_GATE_ROOT="$tree" WCH_GATE_NEXTEST_LIST=/bin/false "$GATE"
}

fail_case_no_selections_at_all() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    grep -v '	tests	' "$tree/scripts/gates/phase-criteria.tsv" \
        >"$tree/scripts/gates/phase-criteria.tsv.seeded"
    mv "$tree/scripts/gates/phase-criteria.tsv.seeded" "$tree/scripts/gates/phase-criteria.tsv"
    gate_red_because 'examined zero phase-gate test selections' \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_NEXTEST_LIST=$lister" "$GATE"
}

fail_case_criterion_runs_a_missing_script() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    printf 'g0\tcommand\t./scripts/gates/not-a-real-gate.sh\ta criterion that names a script nobody wrote\n' \
        >>"$tree/scripts/gates/phase-criteria.tsv"
    gate_red_because 'g0 criterion runs ./scripts/gates/not-a-real-gate.sh, which is missing or not executable' \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_NEXTEST_LIST=$lister" "$GATE"
}

fail_case_criteria_table_deleted() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$tree/scripts/gates/phase-criteria.tsv"
    gate_red_because '/scripts/gates/phase-criteria.tsv; there are no selections to count' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# --------------------------------------------------- every phase block runs the suite and its
# self-test
#
# The claim landed with the two rows it is about: g8 and g9 were the only blocks in the table
# without the `run-all.sh` + `selftest.sh` pair that g0 through g7 each open with, and no reader
# of the table could go red on that. These are its inverses, one per half, because the two
# `gate_fail`s are two sentences and an arm that seeded both would not say which one fired.
#
# The seed removes the pair from **one** phase rather than from every block that carries it: the
# claim is per-phase, and a table with the pair gone everywhere would be red under a rule that
# only ever looked at the first block. `_only_tests_row` runs first so the stub lister has one
# selection to answer for and the arm is about the row set rather than about a hundred rows the
# stub says `mismatch` to.
#
#   $1  the scratch tree
#   $2  the phase whose row to remove
#   $3  the script the removed row runs
_drop_command_row() {
    local tree="$1" phase="$2" script="$3" table
    table="$tree/scripts/gates/phase-criteria.tsv"
    awk -F'\t' -v phase="$phase" -v script="$script" \
        '!($1 == phase && $2 == "command" && $3 == script)' "$table" >"$table.seeded"
    if ! cmp -s "$table" "$table.seeded"; then
        mv "$table.seeded" "$table"
        return 0
    fi
    rm -f "$table.seeded"
    # `gate_seed_died` and not a second `printf` into the same file: the one home for "say a
    # seed did not apply" writes the sentence on **stderr** as well as into the report, and an
    # arm run by hand — which is the ordinary move while a gate is being written — is a console
    # with nothing on it otherwise, a `return 0` from a `fail_case_` reading as a pass. This was
    # the only file in `cases/` that named `gate_seed_report` itself (note **N330**).
    gate_seed_died "no $script row to remove from $phase: the seed this arm is built on is gone"
    return 1
}

fail_case_a_phase_block_never_runs_the_predicate_suite() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" 'package(webcam-handler-schema)' \
        'one selection the stub answers for, so this arm is about the row set'
    _drop_command_row "$tree" g0 ./scripts/gates/run-all.sh || return 0
    gate_red_because 'none of them runs ./scripts/gates/run-all.sh' \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_NEXTEST_LIST=$lister" "$GATE"
}

fail_case_a_phase_block_never_runs_the_predicates_self_test() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" 'package(webcam-handler-schema)' \
        'one selection the stub answers for, so this arm is about the row set'
    _drop_command_row "$tree" g0 ./scripts/gates/selftest.sh || return 0
    gate_red_because 'none of them runs ./scripts/gates/selftest.sh' \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_NEXTEST_LIST=$lister" "$GATE"
}

# The green direction the two arms above need, and it is not `pass_case`: what has to be shown is
# that the rule reads **each phase's own rows** rather than the table as a whole. A phase that
# carries both rows is green while a *different* phase carries neither in the same table would be
# a rule nothing could rely on — so this seeds the pair into a phase that has it and takes the
# suite row out of nothing, leaving a table every block satisfies by a route the reader can see.
pass_case_a_phase_that_carries_the_pair_twice_is_still_a_phase_that_carries_it() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" 'package(webcam-handler-schema)' \
        'one selection the stub answers for, so this arm is about the row set'
    printf 'g0\tcommand\t./scripts/gates/run-all.sh\ta second naming of the suite, which is still a naming of it\n' \
        >>"$tree/scripts/gates/phase-criteria.tsv"
    WCH_GATE_ROOT="$tree" WCH_GATE_NEXTEST_LIST="$lister" "$GATE"
}
