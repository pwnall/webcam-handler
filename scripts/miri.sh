#!/usr/bin/env bash
#
# Miri over the unsafe-adjacent pure units (design §2.5, docs/9 Part 2).
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

# The selection: the two `sys` modules that make no syscall.
#
# `decode` is the pure byte-to-schema half. `payload` holds the two blocks that view an
# initialized buffer as bytes, and they are exactly what Miri exists to check here — so
# leaving them out (as an earlier selection did) meant the job covered only safe code. No
# count of the rest is written here: `crates/backends/v4l2/src/sys/mod.rs` carries the
# residual-`unsafe` register and `scripts/gates/unsafe-scope.sh` reconciles it against the
# tree both ways, so a second tally in this comment would be a number nobody checks. What
# is true of every block *not* selected is the reason: they sit behind an `ioctl`, an
# `mmap`, a `poll`, a `kill` or a descriptor a device node opened, and Miri crosses none of
# those. `sys::tests` is excluded for that reason too — it opens a device node.
#
# **P4d's uevent parser is deliberately not here, and that is a decision rather than an
# oversight.** `hotplug::trigger` is a pure fold over bytes, so it looks like it belongs —
# but this job's population is "unsafe-adjacent", and `hotplug` is adjacent to no `unsafe`
# at all: `sys::uevent` is written with `rustix`, a safe wrapper, so the whole hotplug edge
# contains zero blocks. There is no aliasing, provenance or initialisation question for
# Miri to answer about a `&[u8]` that `std` produced. What that parser needs is *coverage*
# of hostile shapes, and it has it — seven committed fixtures and a twenty-thousand-round
# fixed-seed walk — which under Miri would cost minutes and prove the same thing. The
# mutation floor examines it instead (`.cargo/mutants.toml`).
selection='package(webcam-handler-v4l2) and (test(/^sys::decode/) or test(/^sys::payload/))'
marker='sys::decode'

root="$(git rev-parse --show-toplevel)"
skips=0

skip() {
    skips=$((skips + 1))
    printf 'miri: SKIP %s — %s\n' "$skips" "$*"
}

# `grep` exits non-zero on zero matches, and zero matches is the expected P0 answer —
# the exact `pipefail` trap docs/9 names, handled rather than tripped over.
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
