#!/usr/bin/env bash
#
# Every phase-gate test selection selects more than zero tests (rubric rule 3, docs/9).
#
# The predecessor's costliest gate failure was not a gate that went red — it was a gate
# whose test filter had quietly stopped matching anything and stayed green for months. A
# selection that matches nothing is not a passing criterion; it is an absent one.
#
# The population is the `tests` rows of `phase-criteria.tsv`, so a criterion added there
# is covered here without anybody remembering to add it twice.
#
# The `grep -c` trap docs/9 names is avoided by construction: nothing here pipes into a
# counter whose zero-match exit status could be swallowed. `cargo nextest list`'s exit
# status is captured explicitly, and a build failure or a malformed filterset is a
# failure of this gate rather than a zero that looks like an answer.
#
# $WCH_GATE_NEXTEST_LIST is the selftest's seam: it names a program invoked as
# `<prog> <filterset>` that prints a nextest JSON listing, which lets the failing arms
# exercise the zero-selection and lister-failure paths without a workspace build. The
# passing arm always runs the real `cargo nextest list` over the real tree.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"
table="$root/scripts/gates/phase-criteria.tsv"

if [[ ! -f "$table" ]]; then
    gate_fail "no criteria table at $table; there are no selections to count"
    gate_finish
fi

list_tests() {
    local selection="$1"
    if [[ -n "${WCH_GATE_NEXTEST_LIST:-}" ]]; then
        "$WCH_GATE_NEXTEST_LIST" "$selection"
    else
        (cd "$root" && cargo nextest list --locked --offline --workspace \
            -T json -E "$selection" 2>/dev/null)
    fi
}

# How many tests the selection matched.
#
# **Counted from `filter-match.status`, and this is the whole gate.** `nextest list -T json`
# lists the *entire* workspace whatever `-E` says, and marks each testcase `matches` or
# `mismatch`; its `test-count` is the size of that whole listing, not of the selection.
# Measured on cargo-nextest 0.9.138: a filter matching nothing reports `test-count: 143`,
# and summing the per-suite `testcases` maps gives 143 as well.
#
# So the first version of this function could not return zero for any input — which made a
# gate whose entire subject is "prove no selection has silently gone to zero" green by
# construction, from the day it was written. That is the predecessor defect this suite was
# built to prevent, reproduced inside the check for it. Recorded as note N10.
count_matched() {
    jq '[ .. | objects | select(has("testcases")) | .testcases | to_entries[]
          | select(.value["filter-match"].status == "matches") ] | length'
}

# The (package, binary) pairs the matched tests live in — docs/9 asks selections to be
# compared as pairs, and a suite with no *matching* testcase matched nothing.
count_suites() {
    jq '[ .. | objects | select(has("testcases"))
          | select([ .testcases | to_entries[]
                     | select(.value["filter-match"].status == "matches") ] | length > 0) ]
        | length'
}

selections=0
commands=0
while IFS=$'\t' read -r phase kind selection what; do
    case "$phase" in
    '#'* | '') continue ;;
    esac

    case "$kind" in
    command)
        commands=$((commands + 1))
        # A criterion is a command someone can run. If it names a script, the script has
        # to be there: a phase gate whose criterion is a missing file fails at the worst
        # possible moment otherwise.
        script="${selection%% *}"
        if [[ "$script" == ./* && ! -x "$root/${script#./}" ]]; then
            gate_fail "$phase criterion runs $script, which is missing or not executable"
        fi
        ;;
    tests)
        selections=$((selections + 1))
        listing=""
        status=0
        listing="$(list_tests "$selection")" || status=$?
        if ((status != 0)); then
            gate_fail "$phase selection '$selection' could not be listed (exit $status); a selection that cannot be listed is not a selection that was checked"
            continue
        fi
        matched="$(printf '%s' "$listing" | count_matched)"
        suites="$(printf '%s' "$listing" | count_suites)"
        if ((matched == 0)); then
            gate_fail "$phase selection '$selection' selects zero tests — $what"
        else
            gate_note "$phase '$selection' selects $matched test(s) across $suites (package, binary) suite(s)"
        fi
        ;;
    *)
        gate_fail "$phase row has unknown kind '$kind'"
        ;;
    esac
done <"$table"

gate_checked "$selections" "phase-gate test selections listed"
gate_checked "$commands" "phase-gate command criteria resolved to runnable scripts"
gate_require_nonzero "$selections" "phase-gate test selections"

gate_finish
