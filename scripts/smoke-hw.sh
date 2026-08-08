#!/usr/bin/env bash
#
# R3 — the real-hardware rung (design §3.1, docs/2 G1/G2).
#
# These suites are `#[ignore]`d and CI-independent: shared CI has no camera, and that is
# this plan's largest honest hole (docs/4's recorded limits). They run on demand, on a
# machine with a camera attached, and their results are recorded as evidence in
# docs/implementation-notes.md with transcripts — they never gate CI.
#
# **Motors wear** (design §5). Sweeps that move a pan/tilt/zoom motor are excluded unless
# WCH_ALLOW_MOTION=1, and the exclusion is by name: a motor-moving test is called
# `hw_motion_*`. Moving a motor is never an implicit default.
#
# Like every auto-skipping rung, this one reports a named, counted skip rather than
# exiting quietly.
#
# wch-suite: prefix=hw_ recipe=smoke-hw
set -euo pipefail

# The suite selection. P1 fills in the tests; these two lines are the only thing that
# changes.
prefix='hw_'
motion_prefix='hw_motion_'

root="$(git rev-parse --show-toplevel)"
skips=0

# Run the suite and account for the skips the *tests themselves* report.
#
# nextest has no runtime-skip concept: a hardware test on a machine without that hardware
# passes, and a pass is indistinguishable from a claim actually made. That is "skip == pass
# in a costume" (docs/3 Part C) unless somebody counts them — so the tests print a line
# beginning `SKIP` when they decline to claim something, `--success-output final` makes
# those lines visible for passing tests too, and this function turns them into the named,
# counted skips the rest of the suite already reports.
#
# `grep -c` returns 1 on zero matches, which under `pipefail` would abort the script on
# the *good* path; the `|| true` is that trap handled rather than tripped over.
run_suite() {
    local selection="$1" label="$2" log
    log="$(mktemp "${TMPDIR:-/tmp}/wch-${label}.XXXXXXXX")"

    local status=0
    cargo nextest run --locked --offline --workspace --run-ignored all \
        --no-tests=fail --success-output final -E "$selection" 2>&1 | tee "$log" || status=$?

    local declined
    declined="$({ grep -cE '^[[:space:]]*SKIP' "$log" || true; })"
    if ((declined > 0)); then
        printf '%s: %s claim(s) declined by tests that ran — each named above\n' \
            "$label" "$declined"
        grep -E '^[[:space:]]*SKIP' "$log" | sed "s/^/${label}:   /"
    fi
    rm -f "$log"
    return "$status"
}


skip() {
    skips=$((skips + 1))
    printf 'smoke-hw: SKIP %s — %s\n' "$skips" "$*"
}

if [[ "${WCH_ALLOW_MOTION:-0}" == "1" ]]; then
    selection="test(/(^|::)${prefix}/)"
    printf 'smoke-hw: WCH_ALLOW_MOTION=1 — motor-moving suites are included\n'
else
    selection="test(/(^|::)${prefix}/) - test(/(^|::)${motion_prefix}/)"
    skip "motor-moving suites (${motion_prefix}*) are excluded; set WCH_ALLOW_MOTION=1 to include them"
fi

# `grep` exits non-zero on zero matches, and zero matches is the expected P0 answer —
# the exact `pipefail` trap docs/4 names, handled rather than tripped over.
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
run_suite "$selection" smoke-hw
printf 'smoke-hw: suite run, %s named skip(s) before it started\n' "$skips"
