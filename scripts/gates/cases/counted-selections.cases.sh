# Both-direction cases for `counted-selections.sh`.
#
# The pass arm runs the real `cargo nextest list` over the real workspace — the only
# listing that proves anything about the shipped selections. The failing arms use a stub
# lister so that a seeded table can be checked without rebuilding a copy of the workspace
# for each violation shape; what they exercise is the predicate's arithmetic and its
# error handling, which is where the `grep -c` zero-match trap lives.
#
# shellcheck shell=bash

# A lister that answers like nextest: a listing with one matched test for the schema
# package, and an empty listing for anything else.
_stub_lister() {
    local path="$1"
    cat >"$path" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
case "$1" in
*webcam-handler-schema*)
    printf '%s' '{"test-count":1,"rust-suites":{"webcam-handler-schema":{"testcases":{"slug::round_trips":{"kind":"test","ignored":false,"filter-match":{"status":"matches"}}}}}}'
    ;;
*)
    printf '%s' '{"test-count":0,"rust-suites":{}}'
    ;;
esac
SH
    chmod +x "$path"
}

pass_case() {
    "$GATE"
}

fail_case_selection_matches_no_tests() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    printf 'g0\ttests\ttest(no_such_test_exists)\ta criterion whose filter has rotted\n' \
        >>"$tree/scripts/gates/phase-criteria.tsv"
    WCH_GATE_ROOT="$tree" WCH_GATE_NEXTEST_LIST="$lister" "$GATE"
}

fail_case_lister_cannot_answer() {
    local tree
    tree="$(gate_scratch_tree)"
    WCH_GATE_ROOT="$tree" WCH_GATE_NEXTEST_LIST=/bin/false "$GATE"
}

fail_case_no_selections_at_all() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    grep -v '	tests	' "$tree/scripts/gates/phase-criteria.tsv" \
        >"$tree/scripts/gates/phase-criteria.tsv.seeded"
    mv "$tree/scripts/gates/phase-criteria.tsv.seeded" "$tree/scripts/gates/phase-criteria.tsv"
    WCH_GATE_ROOT="$tree" WCH_GATE_NEXTEST_LIST="$lister" "$GATE"
}

fail_case_criterion_runs_a_missing_script() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    printf 'g0\tcommand\t./scripts/gates/not-a-real-gate.sh\ta criterion that names a script nobody wrote\n' \
        >>"$tree/scripts/gates/phase-criteria.tsv"
    WCH_GATE_ROOT="$tree" WCH_GATE_NEXTEST_LIST="$lister" "$GATE"
}

fail_case_criteria_table_deleted() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$tree/scripts/gates/phase-criteria.tsv"
    WCH_GATE_ROOT="$tree" "$GATE"
}
