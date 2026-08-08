#!/usr/bin/env bash
#
# Prove every gate predicate can go red — the rule the whole suite is built around
# (docs/4): *a gate written to close a defect is not itself tested against its own
# inverse, and the second arm is where it fails.*
#
# The table is the directory listing (`gate_predicates`), never a hand-maintained list.
# Each predicate must have a companion `cases/<name>.cases.sh` defining
#
#     pass_case()          run the predicate against the pristine tree; must exit 0
#     pass_case_<slug>()   a second green arm — a shape the predicate must *allow*
#     fail_case_<slug>()   seed one violation in a COPY of the tree (or point the
#                          predicate's documented seam at a doctored input) and run it;
#                          must exit non-zero
#
# A predicate with no case file fails. A predicate with no green arm fails. A predicate
# with zero `fail_case_*` functions fails — a gate with only a passing arm is the defect
# class, not a gate.
#
# Cases run in a subshell with `lib.sh` sourced (so `gate_scratch_tree` is available) and
# $GATE set to the predicate under test. They never mutate the checkout: `gate_scratch_tree`
# copies it first, and every scratch copy lands under one directory this script removes.
set -uo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

gates_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
lib="$gates_dir/lib.sh"
cases_dir="$gates_dir/cases"

scratch="$(mktemp -d "${TMPDIR:-/tmp}/wch-selftest.XXXXXXXX")"
export WCH_GATE_SCRATCH="$scratch"
trap 'rm -rf "$scratch"' EXIT

# Enumerate the case functions a case file defines. Derived from the file, so adding a
# violation shape is adding a function.
list_cases() {
    local casefile="$1" gate="$2"
    bash -c '
        set -uo pipefail
        GATE="$3"; export GATE
        # shellcheck disable=SC1090
        source "$1"
        # shellcheck disable=SC1090
        source "$2"
        declare -F | awk "{ print \$3 }" | grep -E "^(pass_case|fail_case_)" | sort
    ' _ "$lib" "$casefile" "$gate"
}

# Run one case. Deliberately without `set -e`: the arm's verdict is the exit status of
# the predicate the case runs, and an `-e` abort in a seeding step would be indis-
# tinguishable from the predicate going red — the one confusion this harness must not
# have.
run_case() {
    local casefile="$1" gate="$2" fn="$3"
    bash -c '
        set -uo pipefail
        GATE="$3"; export GATE
        # shellcheck disable=SC1090
        source "$1"
        # shellcheck disable=SC1090
        source "$2"
        "$4"
    ' _ "$lib" "$casefile" "$gate" "$fn" 2>&1
}

predicates=0
pass_arms=0
fail_arms=0
problems=0

report_problem() {
    problems=$((problems + 1))
    printf '  PROBLEM %s\n' "$*" >&2
}

while IFS= read -r gate; do
    predicates=$((predicates + 1))
    name="$(basename "$gate" .sh)"
    casefile="$cases_dir/$name.cases.sh"
    printf -- '--- %s\n' "$name"

    if [[ ! -f "$casefile" ]]; then
        report_problem "$name has no case file at cases/$name.cases.sh; a predicate nobody proved can go red is a predicate nobody has tested"
        continue
    fi

    mapfile -t cases < <(list_cases "$casefile" "$gate")
    have_pass=0
    these_fail_arms=0
    for fn in "${cases[@]}"; do
        case "$fn" in
        pass_case*) have_pass=$((have_pass + 1)) ;;
        fail_case_*) these_fail_arms=$((these_fail_arms + 1)) ;;
        esac
    done

    if ((have_pass == 0)); then
        report_problem "$name has no pass_case; nothing proves the predicate is green on the shipped tree"
    fi
    if ((these_fail_arms == 0)); then
        report_problem "$name has zero fail_case_* functions; a predicate with only a passing arm cannot be shown to go red"
    fi

    for fn in "${cases[@]}"; do
        output="$(run_case "$casefile" "$gate" "$fn")"
        status=$?
        case "$fn" in
        pass_case*)
            pass_arms=$((pass_arms + 1))
            if ((status == 0)); then
                printf '  ok    %s\n' "$fn"
            else
                report_problem "$name $fn exited $status; the predicate is red on a shape it must allow"
                printf '%s\n' "$output" | sed 's/^/        /'
            fi
            ;;
        fail_case_*)
            fail_arms=$((fail_arms + 1))
            if ((status != 0)); then
                printf '  ok    %s (predicate exited %s)\n' "$fn" "$status"
            else
                report_problem "$name $fn exited 0; the seeded violation did not turn the predicate red"
                printf '%s\n' "$output" | sed 's/^/        /'
            fi
            ;;
        esac
    done
    printf '\n'
done < <(gate_predicates)

printf 'selftest: %s predicates, %s pass arm(s), %s fail arm(s)\n' \
    "$predicates" "$pass_arms" "$fail_arms"

if ((predicates == 0)); then
    printf 'selftest: FAIL — no predicates found\n' >&2
    exit 1
fi
if ((problems > 0)); then
    printf 'selftest: FAIL — %s problem(s)\n' "$problems" >&2
    exit 1
fi
printf 'selftest: PASS — every predicate is green on the tree and red on each of its inverses\n'
