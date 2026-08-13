#!/usr/bin/env bash
#
# R3 — the real-hardware rung (design §3.1, docs/7 G1/G2).
#
# These suites are `#[ignore]`d and CI-independent: shared CI has no camera, and that is
# this plan's largest honest hole (docs/9's recorded limits). They run on demand, on a
# machine with a camera attached, and their results are recorded as evidence in
# docs/implementation-notes.md with transcripts — they never gate CI.
#
# **Motors run by default** (owner ruling, 2026-08-08; design §5). The rung exercises
# every control the hardware has, pan/tilt/zoom motors included — an untested motor is
# untested code, and the OBSBOT is a PTZ device. The knob is opt-OUT: WCH_NO_MOTION=1
# excludes the motor-moving suites (named by the `hw_motion_*` prefix) for runs where
# the camera is pointed at someone; the exclusion is a named, counted skip. Motor sweeps
# stay bounded by the `limits` caps, and every arm restores what it moved.
#
# Like every auto-skipping rung, this one reports a named, counted skip rather than
# exiting quietly.
#
# ## Why this rung runs every arm, and then checks that it did
#
# This script counted skips for three phases without running all the tests it counted them
# over, and the G4 adversarial review found it in the E13 transcript: the 2026-08-11 run at
# the three real cameras printed
#
#     warning: 1/18 tests were not run due to test failure (run with --no-fail-fast …)
#
# and `hw_switching_an_automation_control_moves_its_partners_inactive_bit` never started.
# nextest's default is fail-fast, `.config/nextest.toml` sets nothing else, and the
# invocation below asked for nothing else, so one red arm cancelled the rest of the run.
#
# Two things were wrong with that and both are this rung's own subject:
#
#   1. **The named, counted skip lost members without saying so.** The accounting greps
#      `SKIP` lines out of the log, and an arm that never ran prints none — so the count is
#      of the arms that happened to run before the cancellation, presented as the count of
#      the suite's declined claims. The arm that was dropped has *two* `SKIP (partial)`
#      paths of its own. A count that silently shrinks is worse than no count, because a
#      reader who sees "4 claim(s) declined" believes somebody looked at all of them; that
#      is AGENTS rule 3's subject ("never silence") arriving as a number instead of as
#      silence.
#   2. **A truncated run read as a complete one.** Nothing here parsed the cancellation
#      warning, so the only difference between "the suite ran and one arm failed" and "the
#      suite stopped early and seventeen eighteenths of it is unknown" was a line in the
#      scrollback. E13's own summary sentence was written from such a run.
#
# So: `--no-fail-fast`, and then a census.
#
# **Why `--no-fail-fast` and not `--max-fail=all`.** They are the same instruction —
# nextest treats the older flag as an alias for the newer one — so the choice is about who
# reads it. `--no-fail-fast` is the spelling nextest's own cancellation warning offers
# first, which means the person who arrived here by reading that warning finds the words
# they were given; it is also cargo-test's spelling, so it does not quietly raise the
# nextest version this rung requires, and a tool floor is a dependency rather than a
# convenience. A numeric `--max-fail=N` was rejected outright: a middle setting is a
# truncation with a nicer name, and this suite is eighteen arms in twenty-eight seconds —
# there is no budget here that needs a stop-early switch, and the whole product of the run
# is knowing what every arm said.
#
# **Why the flag is not enough on its own, and the census exists.** A flag is a statement
# of intent; it cannot go red. Drop it in an edit three phases from now and this defect
# comes back exactly as it was, with nothing to notice. Worse, the flag does not cover
# every way a run gets cut short: a SIGINT, a binary that aborts before its siblings get to
# start, the per-test `slow-timeout … terminate-after` in `.config/nextest.toml`, or a
# nextest that cancels for a reason this comment cannot anticipate. So the run is asked to
# account for itself in its own numbers — `Starting N tests` against the `N tests run` of
# its summary — and a shortfall is a loud failure with a non-zero exit. That is the same
# discipline `counted-selections.sh` applies to the gate table: the claim is not that the
# selection is right, it is that the run measured what it claims to have measured.
#
# The comparison is deliberately on the *fact* (how many ran) rather than on the *reason*
# (nextest's warning line). The reason is English prose that changes between versions and
# only exists for the causes somebody thought of; the two counts are printed by every run
# and disagree for all of them. When nextest does name a reason, it is quoted underneath —
# a cause is worth having, it is just not what the check turns on.
#
# A run whose census cannot be read at all is a failure too, not a pass. If a future
# nextest stops printing either line, this rung goes red saying so, which costs one commit
# to follow the format — the alternative is a rung that silently stops checking, and this
# whole comment is about what that costs.
#
# ## The seam: a recorded log is a fixture
#
# `$WCH_SMOKE_HW_ACCOUNT` points at a saved run log and this script does the accounting
# over it and stops: nothing is built, no node is opened, no camera is touched. It is the
# same seam `scripts/mutants.sh` carries as `$WCH_MUTANTS_CLASSIFY` and for the same
# reason — the accounting is a pure function of a text file, so it can be exercised
# against a run that already happened, including on a machine that must not point a
# camera at anybody. It announces itself in capitals for the reason that one does: a mode
# that skips the hardware must never be mistakable for a run that used it. `just smoke-hw`
# and the G4 criterion row set it never.
#
# wch-suite: prefix=hw_ recipe=smoke-hw
set -euo pipefail

# The suite selection. P1 fills in the tests; these two lines are the only thing that
# changes.
prefix='hw_'
motion_prefix='hw_motion_'

# Sourced for `gate_scratch_root` and nothing else, which is `scripts/mutants.sh`'s
# arrangement and its reason: where temporary data goes is one decision (the 2026-08-12
# ruling, note N84), and a runner that spelled it itself would be one more copy of it. This
# is not a gate predicate and it keeps its own `smoke-hw:` reporting vocabulary.
#
# shellcheck source-path=SCRIPTDIR
# shellcheck source=gates/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/gates/lib.sh"

root="$(git rev-parse --show-toplevel)"
skips=0

# Everything the run says about itself, read back out of its log: the claims the tests
# declined, and whether every test that was selected actually ran.
#
# Returns 0 when the run accounted for itself completely — which is not the same as the
# tests passing, and deliberately so: a red arm is the suite's verdict and belongs to
# nextest's exit status, while an unreadable or truncated census is this function's
# finding about the run itself. `run_suite` keeps them apart and reports both.
#
# nextest has no runtime-skip concept: a hardware test on a machine without that hardware
# passes, and a pass is indistinguishable from a claim actually made. That is "skip == pass
# in a costume" (docs/8 Part C) unless somebody counts them — so the tests print a line
# beginning `SKIP` when they decline to claim something, `--success-output final` makes
# those lines visible for passing tests too, and this function turns them into the named,
# counted skips the rest of the suite already reports.
#
# The count is printed even when it is zero. "No line" and "no declined claims" used to
# look identical in a transcript, which is a thin version of the same confusion the census
# below exists to end: a reader should be told that the question was asked.
#
# `grep -c` returns 1 on zero matches, which under `pipefail` would abort the script on
# the *good* path; the `|| true` is that trap handled rather than tripped over.
account_for_the_run() {
    local log="$1" label="$2" problems=0

    local declined
    declined="$({ grep -cE '^[[:space:]]*SKIP' "$log" || true; })"
    printf '%s: %s claim(s) declined by tests that ran — each named above\n' \
        "$label" "$declined"
    if ((declined > 0)); then
        grep -E '^[[:space:]]*SKIP' "$log" | sed "s/^/${label}:   /"
    fi

    # The census. Two lines nextest prints on every run:
    #
    #     Starting 18 tests across 40 binaries (943 tests skipped)
    #     Summary [  28.147s] 17/18 tests run: 16 passed, 1 failed, 943 skipped
    #
    # The summary carries the `ran/selected` pair only when they differ; a complete run
    # prints one number ("15 tests run"), and one test is singular in both lines. The
    # escape-stripping covers a log captured with colour forced on — nextest turns colour
    # off into a pipe, so this is the unusual case rather than the normal one, and a
    # census that could be defeated by `--color=always` is not a census.
    local plain started ran
    plain="$(sed 's/\x1b\[[0-9;]*[A-Za-z]//g' "$log")"
    started="$(printf '%s\n' "$plain" |
        sed -n 's/.*Starting[[:space:]]\{1,\}\([0-9]\{1,\}\)[[:space:]]\{1,\}tests\{0,1\}[[:space:]]\{1,\}across.*/\1/p' |
        tail -n1)"
    ran="$(printf '%s\n' "$plain" |
        sed -n 's/.*Summary[[:space:]]*\[[^]]*\][[:space:]]*\([0-9]\{1,\}\)\(\/[0-9]\{1,\}\)\{0,1\}[[:space:]]\{1,\}tests\{0,1\}[[:space:]]\{1,\}run:.*/\1/p' |
        tail -n1)"

    if [[ -z "$started" || -z "$ran" ]]; then
        problems=$((problems + 1))
        printf '%s: FAIL — this run printed no census this script could read (started=%q, ran=%q), so how much of the suite executed is unknown; a run that cannot say what it did is not a run that passed\n' \
            "$label" "$started" "$ran" >&2
    elif ((ran < started)); then
        problems=$((problems + 1))
        printf '%s: FAIL — %s of %s selected test(s) ran, so %s arm(s) never started and every claim above is over the ones that did; a truncated run must not read as a full one\n' \
            "$label" "$ran" "$started" "$((started - ran))" >&2
        # nextest's own reason, when it gave one. Context, not the verdict.
        printf '%s\n' "$plain" | grep -E 'were not run|Canceling|Cancelling' |
            sed "s/^[[:space:]]*/${label}:   /" >&2 || true
    else
        printf '%s: %s of %s selected test(s) ran — the suite is complete\n' \
            "$label" "$ran" "$started"
    fi

    return $((problems > 0 ? 1 : 0))
}

# Run the suite and account for what came back. See the header for why `--no-fail-fast` is
# here and why the census below it is not redundant with it.
run_suite() {
    local selection="$1" label="$2" log
    log="$(mktemp "$(gate_scratch_root)/wch-${label}.XXXXXXXX")"

    local status=0
    cargo nextest run --locked --offline --workspace --run-ignored all \
        --no-fail-fast --no-tests=fail --success-output final -E "$selection" 2>&1 |
        tee "$log" || status=$?

    local census=0
    account_for_the_run "$log" "$label" || census=$?
    rm -f "$log"

    # A failing arm outranks an unreadable census in what it tells the reader to do, and
    # both are already printed; the exit code carries whichever is non-zero.
    if ((status != 0)); then
        return "$status"
    fi
    return "$census"
}

skip() {
    skips=$((skips + 1))
    printf 'smoke-hw: SKIP %s — %s\n' "$skips" "$*"
}

# The recorded-log mode. See "the seam" in the header.
if [[ -n "${WCH_SMOKE_HW_ACCOUNT:-}" ]]; then
    printf 'smoke-hw: ACCOUNTING FOR THE RECORDED LOG AT %s (WCH_SMOKE_HW_ACCOUNT) — NO TEST IS RUN AND NO CAMERA IS TOUCHED\n' \
        "$WCH_SMOKE_HW_ACCOUNT"
    if [[ ! -f "$WCH_SMOKE_HW_ACCOUNT" ]]; then
        printf 'smoke-hw: FAIL — %s is not a file; there is no recorded run to account for\n' \
            "$WCH_SMOKE_HW_ACCOUNT" >&2
        exit 1
    fi
    account_for_the_run "$WCH_SMOKE_HW_ACCOUNT" smoke-hw
    exit $?
fi

if [[ "${WCH_NO_MOTION:-0}" == "1" ]]; then
    selection="test(/(^|::)${prefix}/) - test(/(^|::)${motion_prefix}/)"
    skip "motor-moving suites (${motion_prefix}*) are excluded by WCH_NO_MOTION=1; unset it to include them"
else
    selection="test(/(^|::)${prefix}/)"
    printf 'smoke-hw: motor-moving suites (%s*) are included — set WCH_NO_MOTION=1 to exclude them\n' \
        "$motion_prefix"
fi

# `grep` exits non-zero on zero matches, and zero matches is the expected P0 answer —
# the exact `pipefail` trap docs/9 names, handled rather than tripped over.
suite_size="$({ grep -rlE "fn[[:space:]]+${prefix}" "$root/crates" --include='*.rs' 2>/dev/null || true; } | wc -l)"
if ((suite_size == 0)); then
    skip "no test named ${prefix}* exists yet: the V4L2 backend lands at P1, and the hardware suites land with it"
    printf 'smoke-hw: 0 tests run, %s named skip(s)\n' "$skips"
    exit 0
fi

# The device is the only authority on itself, including on whether it is here.
nodes=0
for node in /dev/video*; do
    if [[ -e "$node" ]]; then
        nodes=$((nodes + 1))
    fi
done
if ((nodes == 0)); then
    skip "no /dev/video* node is present; R3 needs a camera attached"
    printf 'smoke-hw: 0 tests run, %s named skip(s)\n' "$skips"
    exit 0
fi

printf 'smoke-hw: %s capture node(s) present; running %s\n' "$nodes" "$selection"
suite_status=0
run_suite "$selection" smoke-hw || suite_status=$?
printf 'smoke-hw: suite run, %s named skip(s) before it started\n' "$skips"
exit "$suite_status"
