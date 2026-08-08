#!/usr/bin/env bash
#
# Miri over the unsafe-adjacent pure units (design §2.5, docs/4 Part 2).
#
# **Miri cannot cross an ioctl.** It covers the half of the V4L2 sys module that is pure:
# the raw-struct-to-`ControlDesc` decode functions, written as functions over captured
# bytes precisely so this job has a real population. The ioctl calls themselves are
# exercised only on R2 and R3. That split is why "Miri green" must never be read as "the
# unsafe module is verified", and why this script prints what it ran.
#
# No suite declaration: these tests are not `#[ignore]`d — they run under `just ci` too,
# on the normal interpreter. This script runs them again under Miri.
set -euo pipefail

# The selection. P1 fills in the decode units; this line is the only thing that changes.
selection='package(webcam-handler-v4l2) and test(/^sys::decode/)'
marker='sys::decode'

root="$(git rev-parse --show-toplevel)"
skips=0

skip() {
    skips=$((skips + 1))
    printf 'miri: SKIP %s — %s\n' "$skips" "$*"
}

# `grep` exits non-zero on zero matches, and zero matches is the expected P0 answer —
# the exact `pipefail` trap docs/4 names, handled rather than tripped over.
suite_size="$({ grep -rl "$marker" "$root/crates" --include='*.rs' 2>/dev/null || true; } | wc -l)"
if ((suite_size == 0)); then
    skip "no ${marker} unit exists yet: the raw ioctl layer and its pure decode functions land at P1"
    printf 'miri: 0 tests run, %s named skip(s)\n' "$skips"
    exit 0
fi

if ! cargo +nightly miri --version >/dev/null 2>&1; then
    skip "Miri is not installed for the nightly toolchain (\`rustup +nightly component add miri\`)"
    printf 'miri: 0 tests run, %s named skip(s)\n' "$skips"
    exit 0
fi

printf 'miri: running %s\n' "$selection"
cargo +nightly miri nextest run --locked --offline --no-tests=fail -E "$selection"
printf 'miri: suite run, %s named skip(s)\n' "$skips"
