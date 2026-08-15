#!/usr/bin/env bash
#
# Both-direction cases for `oracle-rung-accounting.sh`.
#
# The subject is a *runner's verdicts and exit codes*, not text in the tree, so the failing arms
# drive the predicate's one documented seam — $WCH_GATE_ORACLE_RUNNER, the runner-shaped program
# it accounts with. `pass_case` leaves it unset, which drives the real
# `scripts/rung-oracles.sh`: the arm rubric rule 6 requires [S:N10], and the reason the seam it
# reaches through is a *mode of the shipped runner* rather than a second implementation of its
# verdict logic.
#
# Each stub runner is the **correct** one with exactly one verdict changed, which is what makes
# an arm evidence about that verdict rather than about stubs in general. `_stub_runner` with no
# overrides is therefore itself a passing arm: a runner that keeps the four verdicts apart passes
# however it is spelled, which is how this predicate proves it is checking behaviour and not the
# shipped script's identity.
#
# The wrong answers are the four ways a rung stops being one, and the last is the one this whole
# arrangement exists for:
#
#   - **silent=ran** — a runner that treats "the suites passed and said nothing" as a run. That
#     is a rung indistinguishable from one that was never invoked, and it is green everywhere
#     else in this repository.
#   - **silent=exit0** — the same defect wearing the right word: it *says* FAIL and exits 0, so a
#     phase gate walks past it.
#   - **declined=ran** — a decline reported as a run, which is "skip == pass, in any costume"
#     (docs/8 Part C) at the rung level.
#   - **redrun=ran** — nextest's own non-zero exit swallowed, which turns a build that would not
#     compile into a green oracle rung.
#   - **quiet-decline** — the count without the reasons: the runner says SKIPPED and does not
#     reprint what was declined, so a reader is told a number and cannot price it. That is the
#     shape note **N71** found in `smoke-hw.sh` from the other end.
#   - **missing-log=green** — a seam handed a path that is not there, answering 0. A typo in a
#     fixture path would then read as a green rung.
#
# shellcheck shell=bash

# Write a runner-shaped program: it reads the recorded log the same way the shipped one does and
# answers with one of the four verdicts.
#
#   $1        where to write it
#   $2..      `<situation>=<answer>` overrides of the correct answer, from:
#               silent=ran | silent=exit0 | declined=ran | redrun=ran
#               quiet-decline | missing-log=green
#
# The shipped runner works out which situation it is in; a stub does the same, because the four
# situations are distinguished by two `grep`s and a variable — writing a stub that had to be
# *told* would make these arms evidence about the telling.
_stub_runner() {
    local script="$1"
    shift
    local overrides=" $* "

    cat >"$script" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
log="${WCH_RUNG_ORACLES_ACCOUNT:-}"
status="${WCH_RUNG_ORACLES_STATUS:-0}"
printf 'rung-oracles: ACCOUNTING FOR THE RECORDED LOG AT %s (WCH_RUNG_ORACLES_ACCOUNT) — NOTHING IS BUILT AND NEITHER ORACLE IS INVOKED\n' "$log"
STUB

    if [[ "$overrides" == *" missing-log=green "* ]]; then
        # SC2016: `$log` is a variable in the script being *written*, not in this one — the
        # whole point of the single quotes is that it must not expand here.
        # shellcheck disable=SC2016
        printf 'if [[ ! -f "$log" ]]; then exit 0; fi\n' >>"$script"
    else
        cat >>"$script" <<'STUB'
if [[ ! -f "$log" ]]; then
    printf 'rung-oracles: FAIL — %s is not a file; there is no recorded run to account for\n' "$log" >&2
    exit 1
fi
STUB
    fi

    if [[ "$overrides" == *" redrun=ran "* ]]; then
        printf 'status=0\n' >>"$script"
    fi
    cat >>"$script" <<'STUB'
if ((status != 0)); then
    printf 'rung-oracles: FAIL — nextest exited %s over the selection\n' "$status" >&2
    exit "$status"
fi
declined="$({ grep -cE '^[[:space:]]*SKIP oracles:' "$log" || true; })"
ran="$({ grep -cE '^[[:space:]]*oracles: [0-9]+ container claim' "$log" || true; })"
STUB

    if [[ "$overrides" == *" declined=ran "* ]]; then
        printf 'declined=0\n' >>"$script"
    fi
    if [[ "$overrides" == *" silent=ran "* ]]; then
        printf 'if ((declined == 0 && ran == 0)); then ran=1; fi\n' >>"$script"
    fi

    cat >>"$script" <<'STUB'
if ((declined > 0)); then
    printf 'rung-oracles: SKIPPED — the rung declined, %s time(s):\n' "$declined"
STUB
    if [[ "$overrides" != *" quiet-decline "* ]]; then
        printf "    grep -E '^[[:space:]]*SKIP oracles:' \"\$log\" | sed 's/^[[:space:]]*/rung-oracles:   /'\n" >>"$script"
    fi
    cat >>"$script" <<'STUB'
    exit 0
fi
if ((ran == 0)); then
    printf 'rung-oracles: FAIL — the suites passed but said neither that they ran nor that they declined; one of the two is a lie\n' >&2
STUB
    if [[ "$overrides" == *" silent=exit0 "* ]]; then
        printf '    exit 0\n' >>"$script"
    else
        printf '    exit 1\n' >>"$script"
    fi
    cat >>"$script" <<'STUB'
fi
printf 'rung-oracles: RAN — %s file(s) validated:\n' "$ran"
exit 0
STUB
    chmod +x "$script"
}

# Run the predicate against a stub runner written into a scratch directory.
_with_stub() {
    local dir
    dir="$(mktemp -d "$(gate_scratch_root)/wch-oracle-cases.XXXXXXXX")"
    _stub_runner "$dir/runner.sh" "$@"
    WCH_GATE_ORACLE_RUNNER="$dir/runner.sh" "$GATE"
    local status=$?
    rm -rf "$dir"
    return $status
}

# The shipped runner, unseamed. The arm rubric rule 6 asks for: at least one case drives the real
# tool, so the predicate is proven against the thing it is actually about.
pass_case() {
    "$GATE"
}

# A stub with nothing changed. Green, and it has to be: this predicate is about the four verdicts
# and not about which file produces them, so a correct runner spelled differently must pass.
pass_case_a_correct_runner_however_it_is_spelled() {
    _with_stub
}

fail_case_a_silent_run_reported_as_a_run() {
    _with_stub silent=ran
}

fail_case_a_silent_run_named_but_exiting_zero() {
    _with_stub silent=exit0
}

fail_case_a_decline_reported_as_a_run() {
    _with_stub declined=ran
}

fail_case_a_red_nextest_swallowed_into_a_green_rung() {
    _with_stub redrun=ran
}

fail_case_a_decline_counted_without_its_reasons() {
    _with_stub quiet-decline
}

fail_case_a_recorded_log_that_is_not_there_answering_green() {
    _with_stub missing-log=green
}

fail_case_no_runner_to_drive_at_all() {
    # The subject having left the build, which is `dependency-walls.sh`'s "the crate left the
    # workspace" arm charged to a script: a predicate whose program is gone must go red rather
    # than examine zero things and pass.
    WCH_GATE_ORACLE_RUNNER="/nonexistent/rung-oracles.sh" "$GATE"
}
