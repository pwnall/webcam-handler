#!/usr/bin/env bash
#
# R2 — the `vivid` virtual-driver rung (design §3.1).
#
# `vivid` is a kernel module that presents a V4L2 capture device with no hardware behind
# it, which is the only way to exercise the real ioctl layer on a machine with no camera.
# It is opportunistic by nature: most runners cannot load it. That makes it the rung most
# likely to decay into green-by-absence, so this script never exits quietly — it either
# runs the suite or prints a **named, counted skip** saying which precondition was
# missing.
#
# What R2 proves and does not: the ioctl plumbing, not device quirks. `vivid` does not
# model INACTIVE-coupled menus with holes (design §3.3 item 4), so a green R2 is not
# evidence about real cameras.
#
# wch-suite: prefix=vivid_ recipe=rung-vivid
set -euo pipefail

# The suite selection. P1 fills in the tests; this line is the only thing that changes.
# Named by prefix so `ignored-suites-have-recipes.sh` can prove every `#[ignore]`d test
# belongs to a recipe.
# Anchored on a module boundary for the reason `smoke-hw.sh` documents: nextest names a
# lib unit test `tests::vivid_foo` and an integration test `vivid_foo`.
selection='test(/(^|::)vivid_/)'
prefix='vivid_'

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
    printf 'rung-vivid: SKIP %s — %s\n' "$skips" "$*"
}

# Does the suite exist at all? Asked of the source rather than of a test binary, so the
# answer costs nothing on a machine that could not run it anyway.
# `grep` exits non-zero on zero matches, and zero matches is the expected P0 answer —
# the exact `pipefail` trap docs/4 names, handled rather than tripped over.
suite_size="$({ grep -rlE "fn[[:space:]]+${prefix}" "$root/crates" --include='*.rs' 2>/dev/null || true; } | wc -l)"
if ((suite_size == 0)); then
    skip "no test named ${prefix}* exists yet: the V4L2 backend lands at P1, and the vivid suite lands with it"
    printf 'rung-vivid: 0 tests run, %s named skip(s)\n' "$skips"
    exit 0
fi

if [[ ! -d /sys/module/vivid ]]; then
    if command -v modinfo >/dev/null && modinfo vivid >/dev/null 2>&1; then
        skip "the vivid module is installed but not loaded; run \`sudo modprobe vivid\` (this script never loads kernel modules on someone's behalf)"
    else
        skip "the vivid module is not available on this host"
    fi
    printf 'rung-vivid: 0 tests run, %s named skip(s)\n' "$skips"
    exit 0
fi

printf 'rung-vivid: vivid is loaded; running %s\n' "$selection"
run_suite "$selection" rung-vivid
printf 'rung-vivid: suite run, %s named skip(s) before it started\n' "$skips"
