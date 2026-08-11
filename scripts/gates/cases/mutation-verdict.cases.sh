#!/usr/bin/env bash
#
# Both-direction cases for `mutation-verdict.sh`.
#
# The subject is a *program's exit codes and vocabulary*, not text in the tree, so the
# failing arms drive the predicate's two documented seams — $WCH_GATE_MUTATION_FLOOR, the
# floor-shaped program it classifies fixtures with, and $WCH_GATE_PHASE_RUNNER, the
# phase-gate runner it hands a criterion row to. `pass_case` leaves both unset, which drives
# the real `scripts/mutants.sh` and the real `scripts/gates/phase.sh`: the arm rubric rule 6
# requires [S:N10], and the reason the seam is a *mode of the shipped floor* rather than a
# second implementation of its verdict logic.
#
# Each stub floor is the **correct** one with exactly one answer changed, which is what makes
# an arm evidence about that answer rather than about stubs in general. `_stub_floor` with no
# overrides is therefore itself a passing arm: a floor that keeps the three outcomes apart
# passes however it is spelled, which is how this predicate proves it is checking behaviour
# and not the shipped script's identity.
#
# The wrong answers are not invented. Two of them have dates:
#
#   - `truncated=finding` is note **N66**: the disk filled fifteen minutes in and the floor
#     printed FAIL, which is the word a surviving unaccepted mutant gets;
#   - `moved=finding` is note **N68**, and it is what the shipped floor did until this
#     commit: a `just gate-g4` started at 08:14 was still reading the tree at 09:19 when
#     three files in its scope were edited, and it reported three N25 acceptances as lies.
#     They were telling the truth — an N25 acceptance argues the mutant is the *same
#     program*, and no test distinguishes identical programs (N60 settled its own false
#     positive on exactly that proof).
#
# The other four are the directions those two do not cover: green where there was no verdict
# (an unproven criterion reading as proven), no-verdict where there was a real survivor (the
# floor weakened by its own new outcome), a register whose second direction stopped firing,
# and a floor that refuses a tree nobody touched — which is the gate that cries wolf on every
# run, and N60 records what that costs.
#
# shellcheck shell=bash

# Write a floor-shaped program: it reads the fixture's one-word `kind` file and answers.
#
#   $1        where to write it
#   $2..      `<kind>=<green|finding|no-verdict>` overrides of the correct answer
#
# The shipped floor works out what it is looking at; a stub is told, because a stub that had
# to classify would be a second copy of the thing under test.
_stub_floor() {
    local script="$1"
    shift
    cat >"$script" <<'STUB'
#!/usr/bin/env bash
set -uo pipefail
dir="${WCH_MUTANTS_CLASSIFY:-}"
kind="$(cat "$dir/kind" 2>/dev/null || printf 'unknown')"
declare -A verdict=(
    [clean]=green [unaccepted]=finding [stale]=finding
    [truncated]=no-verdict [absent]=no-verdict [moved]=no-verdict
)
STUB
    local override
    for override in "$@"; do
        printf 'verdict[%s]=%s\n' "${override%%=*}" "${override#*=}" >>"$script"
    done
    # The no-verdict exit code is taken from `lib.sh` rather than typed here: a stub that
    # transcribed 75 would keep passing the day the convention moved, and this whole
    # predicate is about one number meaning one thing.
    cat >>"$script" <<STUB
if ! git rev-parse --git-dir >/dev/null 2>&1; then
    printf 'mutants: SKIP 1 — git cannot describe this tree, so nobody checked whether it moved\n'
fi
case "\${verdict[\$kind]:-green}" in
green)
    printf 'mutants: PASS — %s recorded mutant(s), register clean both ways\n' \\
        "\$(grep -c '' <"\$dir/missed.txt" 2>/dev/null || printf 0)"
    exit 0
    ;;
finding)
    printf 'mutants: FAIL — a survivor with no recorded acceptance\n' >&2
    exit 1
    ;;
no-verdict)
    printf 'mutants: NO VERDICT — this run could not answer\n' >&2
    if [[ -f "\$dir/tree-before.txt" ]]; then tail -n1 "\$dir/tree-before.txt" >&2; fi
    exit $GATE_NO_VERDICT
    ;;
esac
STUB
    chmod +x "$script"
}

_stub_path() {
    mktemp "${WCH_GATE_SCRATCH:-${TMPDIR:-/tmp}}/wch-stub-floor.XXXXXXXX"
}

pass_case() {
    "$GATE"
}

# A floor that is not this repository's script and still keeps the three answers apart. The
# arm exists to prove the predicate asserts *behaviour*: without it, every failing arm below
# would be consistent with a predicate that simply checks it was handed the shipped file.
pass_case_a_differently_written_floor_that_keeps_the_three_outcomes_apart() {
    local stub
    stub="$(_stub_path)"
    _stub_floor "$stub"
    WCH_GATE_MUTATION_FLOOR="$stub" "$GATE"
}

# Note N66, mechanised. The filesystem filled, and the job said the same word it says about a
# surviving mutant. Nothing about the code was learned, and fifteen minutes of disk failure
# entered the record as a verdict.
fail_case_a_floor_that_spells_a_resource_shortfall_as_a_finding() {
    local stub
    stub="$(_stub_path)"
    _stub_floor "$stub" truncated=finding
    WCH_GATE_MUTATION_FLOOR="$stub" "$GATE"
}

# Note N68, and what the shipped floor did until this commit: the tree it spent an hour
# reading changed underneath it, and it reported the difference as three acceptances that had
# become lies. A verdict computed over two different trees is void, not false.
fail_case_a_floor_that_reports_a_moved_input_as_a_finding() {
    local stub
    stub="$(_stub_path)"
    _stub_floor "$stub" moved=finding
    WCH_GATE_MUTATION_FLOOR="$stub" "$GATE"
}

# The other direction, and the worse one. A run that could not answer must never be green:
# the criterion is not proven, and a phase gate that closes on it has closed on nothing.
fail_case_a_floor_that_is_green_when_it_produced_no_verdict() {
    local stub
    stub="$(_stub_path)"
    _stub_floor "$stub" absent=green
    WCH_GATE_MUTATION_FLOOR="$stub" "$GATE"
}

# The floor weakened by its own repair: a real unaccepted survivor answered with the
# third outcome, where "could not answer" is softer than the truth and reads as an
# environment problem somebody will re-run. A missing test must still fail loudly.
fail_case_a_floor_that_hides_a_survivor_behind_the_third_outcome() {
    local stub
    stub="$(_stub_path)"
    _stub_floor "$stub" unaccepted=no-verdict
    WCH_GATE_MUTATION_FLOOR="$stub" "$GATE"
}

# The register's second direction, silently stopped. An acceptance nobody re-checks is how
# N15's mistake gets made twice, and this arm is here because the third outcome must not
# become a place for that direction to quietly go.
fail_case_a_floor_that_stops_reporting_an_acceptance_that_no_longer_survives() {
    local stub
    stub="$(_stub_path)"
    _stub_floor "$stub" stale=green
    WCH_GATE_MUTATION_FLOOR="$stub" "$GATE"
}

# The tree check that refuses everything. It would pass every arm above that asserts a
# no-verdict and would make the floor useless: a gate that cries wolf does not get believed,
# it gets re-run until it agrees (N60). The predicate's control arm — an unchanged tree must
# be green — is the only thing standing between those two.
fail_case_a_floor_that_refuses_a_tree_nobody_touched() {
    local stub
    stub="$(_stub_path)"
    _stub_floor "$stub" clean=no-verdict
    WCH_GATE_MUTATION_FLOOR="$stub" "$GATE"
}

# The ambiguity moved one layer out instead of removed. This runner is `phase.sh` as it was
# before this commit: it runs the rows, and every non-zero status is `FAIL … criterion N
# failed`. The floor underneath it is the real one and gets all three answers right — and it
# makes no difference to what `just gate-g4` prints, which is the whole reason this arm
# exists.
fail_case_a_phase_runner_that_renders_every_non_zero_row_as_a_finding() {
    local stub
    stub="$(_stub_path)"
    cat >"$stub" <<'RUNNER'
#!/usr/bin/env bash
set -uo pipefail
phase="$1"
root="${WCH_GATE_ROOT:-.}"
table="$root/scripts/gates/phase-criteria.tsv"
failures=0
checked=0
while IFS=$'\t' read -r row_phase kind selection what; do
    [[ "$row_phase" == "$phase" ]] || continue
    checked=$((checked + 1))
    printf '\n  [%s.%s] %s\n' "$phase" "$checked" "$what"
    status=0
    (cd "$root" && eval "$selection") || status=$?
    if ((status != 0)); then
        printf '  FAIL  gate-%s: %s criterion %s failed (exit %s): %s\n' \
            "$phase" "$phase" "$checked" "$status" "$what" >&2
        failures=$((failures + 1))
    fi
done <"$table"
if ((failures > 0)); then
    printf 'FAIL gate-%s — %s violation(s) over %s examined items\n' \
        "$phase" "$failures" "$checked" >&2
    exit 1
fi
printf 'PASS gate-%s — %s items examined\n' "$phase" "$checked"
RUNNER
    chmod +x "$stub"
    WCH_GATE_PHASE_RUNNER="$stub" "$GATE"
}
