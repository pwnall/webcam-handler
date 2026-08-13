#!/usr/bin/env bash
#
# R1-web — the pinned Playwright + Chromium browser rung (design §3.1, docs/7 P5d).
#
# The rung itself is a `webcam-handler-daemon` integration test
# (`crates/daemon/tests/web_browser.rs`): it starts the shipped listener over the fake
# backend and hands the URL to a child `playwright test`. This script is the *runner* — what
# `just rung-web` invokes and what `just gate-g5` records — and its whole job is the sentence
# docs/7 asks for: **the gate close records whether the rung ran or skipped, by name.**
#
# Unlike `rung-vivid.sh` and `smoke-hw.sh` this suite is **not** `#[ignore]`d, because design
# §3.1 puts R1-web on "every push where the host has node" rather than on demand. So this
# script is not how the rung gets run — `just ci` already runs it, and
# `.config/nextest.toml` gives the binary `success-output = "final"` so its decline can never
# be printed into a void. What this script adds is the accounting: it turns the test's own
# `SKIP r1-web:` line into a named, counted verdict at the end, so a phase gate that ran it
# says which of the two things happened.
#
# The rung needs node, the pinned Playwright, and the pinned browser; the *build* needs none
# of them, which is `node_is_never_a_build_dependency`'s claim in the same file.
set -euo pipefail

# The one binary. Named by target rather than by test-name prefix because there is no
# `#[ignore]` here to hang a prefix on: the suite is one integration test target, and
# `binary(web_browser)` is how nextest spells "that target".
selection='package(webcam-handler-daemon) and binary(web_browser)'

# Sourced for `gate_scratch_root` and nothing else, which is `scripts/mutants.sh`'s
# arrangement and its reason: where temporary data goes is one decision (the 2026-08-12
# ruling, note N84), and a runner that spelled it itself would be the twentieth copy of it.
# This is not a gate predicate and it keeps its own `rung-web:` reporting vocabulary.
#
# shellcheck source-path=SCRIPTDIR
# shellcheck source=gates/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/gates/lib.sh"

root="$(git rev-parse --show-toplevel)"
cd "$root"

log="$(mktemp "$(gate_scratch_root)/wch-rung-web.XXXXXXXX")"
trap 'rm -f "$log"' EXIT

status=0
# `--success-output final` is also in `.config/nextest.toml` for this binary; passing it here
# means this script says what it needs rather than inheriting it, and a config edit cannot
# silently make this runner blind.
cargo nextest run --locked --offline --workspace --no-tests=fail \
    --success-output final -E "$selection" 2>&1 | tee "$log" || status=$?

# `grep -c` exits non-zero on zero matches, which under `pipefail` would abort on the *good*
# path — the same trap `rung-vivid.sh` documents, handled rather than tripped over. nextest
# indents a captured test's output, so the anchor allows leading space.
declined="$({ grep -cE '^[[:space:]]*SKIP r1-web:' "$log" || true; })"

if ((status != 0)); then
    # Not necessarily a browser failure — a workspace that will not build reaches here too, and
    # saying "the rung failed" about that would be this script guessing. The trace is named
    # because it exists exactly when the browser was the thing that went red.
    printf 'rung-web: FAIL — nextest exited %s over %s; if the browser ran, its trace is under %s\n' \
        "$status" "$selection" "$(gate_scratch_root)/r1-web" >&2
    exit "$status"
fi

if ((declined > 0)); then
    printf 'rung-web: SKIPPED — the rung declined, %s time(s), each naming what was missing and what it therefore did not claim:\n' \
        "$declined"
    grep -E '^[[:space:]]*SKIP r1-web:' "$log" | sed 's/^[[:space:]]*/rung-web:   /'
    printf 'rung-web: run "just rung-web-install" to give this host the pinned Playwright and the pinned browser\n'
    exit 0
fi

# The other verdict, and it is stated rather than implied: a run that printed neither a
# decline nor this line would be a rung nobody can tell apart from one that was never invoked.
ran="$({ grep -cE '^[[:space:]]*r1-web: [0-9]+ browser claim' "$log" || true; })"
if ((ran == 0)); then
    printf 'rung-web: FAIL — the suite passed but said neither that it ran nor that it declined; one of the two is a lie\n' >&2
    exit 1
fi
printf 'rung-web: RAN — '
grep -E '^[[:space:]]*r1-web: [0-9]+ browser claim' "$log" | sed 's/^[[:space:]]*r1-web: //'
