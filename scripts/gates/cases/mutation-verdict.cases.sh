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
# The other four classification directions are the ones those two do not cover: green where
# there was no verdict (an unproven criterion reading as proven), no-verdict where there was a
# real survivor (the floor weakened by its own new outcome), a register whose second direction
# stopped firing, and a floor that refuses a tree nobody touched — which is the gate that cries
# wolf on every run, and N60 records what that costs.
#
# The block at the end of this file is a different subject on the same program: not what a run
# concluded but whether it may begin, which is the owner's ruling of 2026-08-21. Its arms use the
# same seam and the same one-answer-changed discipline, over a second vocabulary of overrides.
#
# Each arm names the sentence it is claiming (`gate_red_because`, note **N31**), and nowhere is
# that more necessary than here: every stub below is red under the same sentence with a different
# *label* in front of it, and two of them are red under three of those sentences at once. An arm
# that read only the exit status would be satisfied by a stub that got some other outcome wrong,
# which is the whole subject of this predicate happening to its own selftest. The exit code the
# claim quotes comes from `$GATE_NO_VERDICT` rather than being typed, for the reason the stub
# gives itself.
#
# shellcheck shell=bash

# Write a floor-shaped program: it reads the fixture's one-word `kind` file and answers.
#
#   $1        where to write it
#   $2..      `<kind>=<green|finding|no-verdict>` overrides of the correct verdict over a
#             recorded result set, or `<case>=<start|refuse|quiet>` overrides of the correct
#             answer to "may this run begin at all" — the predicate's claim 5, where the
#             fixture is a whole git repository rather than a directory of result files. The
#             two vocabularies share no key, and the routing below is by name so that a stub
#             is still wrong in exactly one way.
#
# The shipped floor works out what it is looking at; a stub is told, because a stub that had
# to classify — or to decide for itself whether a commit is on a remote — would be a second
# copy of the thing under test.
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
declare -A preflight=(
    [refuse]=refuse [start]=start [escape]=start [iterate]=start
)
STUB
    local override
    for override in "$@"; do
        case "${override%%=*}" in
        refuse | start | escape | iterate)
            printf 'preflight[%s]=%s\n' "${override%%=*}" "${override#*=}" >>"$script"
            ;;
        *)
            printf 'verdict[%s]=%s\n' "${override%%=*}" "${override#*=}" >>"$script"
            ;;
        esac
    done
    # The no-verdict exit code is taken from `lib.sh` rather than typed here: a stub that
    # transcribed 75 would keep passing the day the convention moved, and this whole
    # predicate is about one number meaning one thing.
    cat >>"$script" <<STUB
# No recorded result set, so this floor is not being asked to classify anything: it is being
# asked whether it may start. The fixture repository carries its one-word \`kind\` beside its
# \`scripts/\`, for the same reason the result directories carry theirs.
if [[ -z "\$dir" ]]; then
    repo_kind="\$(cat "\$(dirname "\$0")/../kind" 2>/dev/null || printf 'unknown')"
    answer="\${preflight[\$repo_kind]:-start}"
    announce=0
    if [[ "\${WCH_MUTANTS_ALLOW_UNPUSHED:-0}" == "1" ]]; then
        answer="\${preflight[escape]:-start}"
        announce=1
    fi
    for arg in "\$@"; do
        [[ "\$arg" == "--iterate" ]] && answer="\${preflight[iterate]:-start}"
    done
    if [[ "\${WCH_MUTANTS_ITERATE:-0}" == "1" ]]; then
        answer="\${preflight[iterate]:-start}"
    fi
    # \`quiet\` is \`start\` with the escape's announcement dropped: the run happens and nothing
    # in the log says a precondition was turned off for it.
    if [[ "\$answer" == "quiet" ]]; then
        answer=start
        announce=0
    fi
    if [[ "\$answer" == "start" ]]; then
        if ((announce == 1)); then
            printf 'mutants: SKIP 1 — WCH_MUTANTS_ALLOW_UNPUSHED=1, so this run started without checking that the work exists anywhere off this machine\n'
        fi
        # Where a floor that started is stopped by the predicate's \`cargo\` shim: an empty
        # scope, which the shipped floor already has words for.
        printf 'mutants: FAIL — .cargo/mutants.toml selects no files; a mutation run over nothing cannot go red\n' >&2
        exit 1
    fi
    printf 'mutants: NO VERDICT — REFUSED to start: this checkout is not committed and pushed\n' >&2
    printf 'mutants:   remedy: commit and push, then re-run\n' >&2
    printf 'mutants:   or just mutants-iterate, which carries no such precondition\n' >&2
    printf 'mutants:   or WCH_MUTANTS_ALLOW_UNPUSHED=1, which starts anyway and says so out loud\n' >&2
    exit $GATE_NO_VERDICT
fi
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
    mktemp "$(gate_scratch_root)/wch-stub-floor.XXXXXXXX"
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
    gate_red_because \
        "a result set whose rows and summary disagree: the floor exited 1 where 'no-verdict' is exit $GATE_NO_VERDICT" \
        env WCH_GATE_MUTATION_FLOOR="$stub" "$GATE"
}

# Note N68, and what the shipped floor did until this commit: the tree it spent an hour
# reading changed underneath it, and it reported the difference as three acceptances that had
# become lies. A verdict computed over two different trees is void, not false.
fail_case_a_floor_that_reports_a_moved_input_as_a_finding() {
    local stub
    stub="$(_stub_path)"
    _stub_floor "$stub" moved=finding
    gate_red_because \
        "a tree that moved while the floor was reading it: the floor exited 1 where 'no-verdict' is exit $GATE_NO_VERDICT" \
        env WCH_GATE_MUTATION_FLOOR="$stub" "$GATE"
}

# The other direction, and the worse one. A run that could not answer must never be green:
# the criterion is not proven, and a phase gate that closes on it has closed on nothing.
fail_case_a_floor_that_is_green_when_it_produced_no_verdict() {
    local stub
    stub="$(_stub_path)"
    _stub_floor "$stub" absent=green
    gate_red_because \
        "a run that left no result set at all: the floor exited 0 where 'no-verdict' is exit $GATE_NO_VERDICT" \
        env WCH_GATE_MUTATION_FLOOR="$stub" "$GATE"
}

# The floor weakened by its own repair: a real unaccepted survivor answered with the
# third outcome, where "could not answer" is softer than the truth and reads as an
# environment problem somebody will re-run. A missing test must still fail loudly.
fail_case_a_floor_that_hides_a_survivor_behind_the_third_outcome() {
    local stub
    stub="$(_stub_path)"
    _stub_floor "$stub" unaccepted=no-verdict
    gate_red_because \
        "a survivor with no recorded acceptance: the floor exited $GATE_NO_VERDICT where 'finding' is exit 1" \
        env WCH_GATE_MUTATION_FLOOR="$stub" "$GATE"
}

# The register's second direction, silently stopped. An acceptance nobody re-checks is how
# N15's mistake gets made twice, and this arm is here because the third outcome must not
# become a place for that direction to quietly go.
fail_case_a_floor_that_stops_reporting_an_acceptance_that_no_longer_survives() {
    local stub
    stub="$(_stub_path)"
    _stub_floor "$stub" stale=green
    gate_red_because \
        "a recorded acceptance that no longer survives: the floor exited 0 where 'finding' is exit 1" \
        env WCH_GATE_MUTATION_FLOOR="$stub" "$GATE"
}

# The tree check that refuses everything. It would pass every arm above that asserts a
# no-verdict and would make the floor useless: a gate that cries wolf does not get believed,
# it gets re-run until it agrees (N60). The predicate's control arm — an unchanged tree must
# be green — is the only thing standing between those two.
fail_case_a_floor_that_refuses_a_tree_nobody_touched() {
    local stub
    stub="$(_stub_path)"
    _stub_floor "$stub" clean=no-verdict
    # Red under all three of the predicate's green expectations, because the clean fixture is
    # what each of them is driven over. The control arm is the one this is about.
    gate_red_because \
        "a tree that did not move: the floor exited $GATE_NO_VERDICT where 'green' is exit 0" \
        env WCH_GATE_MUTATION_FLOOR="$stub" "$GATE"
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
    gate_red_because \
        "a phase whose criterion could not answer: the phase runner exited 1, wanted $GATE_NO_VERDICT" \
        env WCH_GATE_PHASE_RUNNER="$stub" "$GATE"
}

# ---------------------------------------------------------------- the precondition on starting
#
# Claim 5's directions. The subject moves from "what did this run conclude" to "may this run
# begin", which is the owner's ruling of 2026-08-21: the full floor spends hours reading a tree
# and stressing the machine, so it refuses to start unless that work also exists somewhere else,
# and the triage mode — minutes, and the tool you reach for *before* committing — does not.
#
# Five directions, because the rule has five ways to be wrong and four of them are the ways a
# rule like this usually fails: it does not fire, it fires on everything, it swallows the mode it
# was never meant to cover, its escape does not work, or its escape works silently.

# The rule that does not fire, which is the ruling simply not implemented: hours of machine time
# spent on a checkout whose only copy is the machine doing the spending.
fail_case_a_floor_that_starts_the_full_run_on_work_nothing_else_holds() {
    local stub
    stub="$(_stub_path)"
    _stub_floor "$stub" refuse=start
    gate_red_because \
        "a full run whose commit is on no remote: the floor exited 1 where a refusal to start is exit $GATE_NO_VERDICT" \
        env WCH_GATE_MUTATION_FLOOR="$stub" "$GATE"
}

# The rule that fires on everything, which is the same defect `fail_case_a_floor_that_refuses_a_
# tree_nobody_touched` covers one claim up, and it is the one that decides whether any of this is
# usable: a floor that refuses a checkout which *is* committed and pushed is a floor nobody can
# run, and N60 records what a gate that cries wolf costs.
fail_case_a_floor_that_refuses_a_checkout_that_is_committed_and_pushed() {
    local stub
    stub="$(_stub_path)"
    _stub_floor "$stub" start=refuse
    gate_red_because \
        "a full run on a checkout committed and pushed: the floor exited $GATE_NO_VERDICT where a run that started and then met the shim's empty scope is exit 1" \
        env WCH_GATE_MUTATION_FLOOR="$stub" "$GATE"
}

# The split flattened. `--iterate` is the triage tool the tree-recording paragraph in
# `scripts/mutants.sh` is defending — run it after each development stage, on the dirty tree
# whose fix you are checking — and a precondition that reached it would have turned one ruling
# about long runs into a ban on the short one.
fail_case_a_floor_that_refuses_the_triage_mode_the_precondition_does_not_cover() {
    local stub
    stub="$(_stub_path)"
    _stub_floor "$stub" iterate=refuse
    gate_red_because \
        "iterate mode on a checkout that is on no remote: the floor exited $GATE_NO_VERDICT where a run that started and then met the shim's empty scope is exit 1" \
        env WCH_GATE_MUTATION_FLOOR="$stub" "$GATE"
}

# The escape that is not one. A deliberate long run on unpushed work stays possible — that is
# what makes this a precondition rather than a prohibition — and a named door that does not open
# leaves the caller with no verb out, which is docs/11's H2 in one variable.
fail_case_a_floor_whose_named_escape_does_not_let_the_run_through() {
    local stub
    stub="$(_stub_path)"
    _stub_floor "$stub" escape=refuse
    gate_red_because \
        "the named escape on a checkout that is on no remote: the floor exited $GATE_NO_VERDICT where a run that started and then met the shim's empty scope is exit 1" \
        env WCH_GATE_MUTATION_FLOOR="$stub" "$GATE"
}

# The escape that opens quietly, which is the half of `WCH_NO_MOTION=1`'s register that is easy
# to drop: the run happens, the precondition was turned off for it, and nothing in the log says
# so. AGENTS rule 3 has one word for that shape and it is "never silence".
fail_case_a_floor_whose_escape_starts_the_long_run_without_saying_so() {
    local stub
    stub="$(_stub_path)"
    _stub_floor "$stub" escape=quiet
    gate_red_because \
        "the escape started the full run without naming itself in a counted skip" \
        env WCH_GATE_MUTATION_FLOOR="$stub" "$GATE"
}
