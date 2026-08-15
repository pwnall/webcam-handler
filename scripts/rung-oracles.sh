#!/usr/bin/env bash
#
# The container-oracle rung — `ffprobe` and `mpv` over files this workspace wrote (docs/7 P6d,
# docs/9's "Oracle rung accounting" row: *"ffprobe/mpv validation where present, counted named
# skip where not"*).
#
# The rung itself is two suites: `crates/engine/tests/oracles.rs`, which records over the fake
# backend through the product's own path and asks both programs about the result, and
# `testkit::oracle`'s own arms, which ask whether the harness can tell a good file from a damaged
# one. This script is the *runner* — what `just rung-oracles` invokes and what `just gate-g6`
# records — and its whole job is the sentence docs/9 asks for: **the gate close records whether
# the rung ran or skipped, by name.**
#
# ## Design §3.1 names no oracle letter, and this script does not invent one
#
# R0, R1, R1-web, R2 and R3 are the rungs design §3.1 declares, and adding a sixth would amend a
# settled section. So this rung has a *name* and no letter: its fake-generated half hangs off the
# every-push rungs (it is not `#[ignore]`d, `just ci` runs it, `.config/nextest.toml` gives the
# binary `success-output = "final"` so its decline is never printed into a void) and its
# real-camera half is part of **R3**, in `crates/backends/v4l2/tests/hardware.rs`, where the
# cameras already are.
#
# It also lives in `scripts/` and not in `scripts/gates/`, which is a placement rather than a
# preference: `run-all.sh` and `selftest.sh` derive their population from the `scripts/gates/*.sh`
# listing, so a runner dropped there would be a predicate with no `cases/<name>.cases.sh` and the
# selftest would go red for the right reason about the wrong file. What *is* under
# `scripts/gates/` is `oracle-rung-accounting.sh`, which drives the four verdicts below through
# the recorded-log seam and proves each of them can happen.
#
# ## The four verdicts, and why the fourth one exists
#
# `scripts/rung-web.sh` is the shape this copies, including the arm that matters most: a run that
# printed **neither** a decline nor a run line is a `FAIL`, because a rung nobody can tell apart
# from one that was never invoked is the defect class the whole ladder exists for. A suite that
# stopped calling `OracleReport::print`, a filterset that stopped selecting anything, a rename
# that silently emptied the selection — each of those is green everywhere else and red here.
#
#   RAN      at least one `oracles: N container claim(s) …` line and no decline
#   SKIPPED  at least one `SKIP oracles: …` line, each naming what was missing and the remedy
#   FAIL     nextest itself was non-zero — which is not necessarily an oracle failure, and this
#            script says so rather than guessing
#   FAIL     neither line appeared
#
# Note the `grep -cE … || true` below: `grep` exits 1 on zero matches, which under `pipefail`
# would abort the script on the *good* path. That is the trap `rung-vivid.sh` and `smoke-hw.sh`
# both document, handled rather than tripped over.
#
# ## The seam: a recorded log is a fixture
#
# `$WCH_RUNG_ORACLES_ACCOUNT` points at a saved run log and this script does the accounting over
# it and stops: nothing is built and neither oracle is invoked. `$WCH_RUNG_ORACLES_STATUS` is the
# nextest exit status to account *with*, defaulting to 0, so the "nextest was red" arm is
# reachable too. It is the seam `scripts/mutants.sh` carries as `$WCH_MUTANTS_CLASSIFY` and
# `scripts/smoke-hw.sh` as `$WCH_SMOKE_HW_ACCOUNT`, for their reason — the accounting is a pure
# function of a text file, so it can be exercised against runs that already happened, and against
# runs this host cannot produce because it has both oracles installed. It announces itself in
# capitals because a mode that ran no oracle must never be mistakable for one that did.
set -euo pipefail

# The two suites. The engine binary is named by target because it has no `#[ignore]` to hang a
# prefix on; the testkit arms are named by module path, because they are unit tests of the
# harness and share the binary with the conformance battery.
selection='binary(oracles) + (package(webcam-handler-testkit) and test(/^oracle::/))'

# Sourced for `gate_scratch_root` and nothing else, which is `scripts/rung-web.sh`'s arrangement
# and its reason: where temporary data goes is one decision (the 2026-08-12 ruling, note N84),
# and a runner that spelled it itself would be one more copy of it. This is not a gate predicate
# and it keeps its own `rung-oracles:` reporting vocabulary.
#
# shellcheck source-path=SCRIPTDIR
# shellcheck source=gates/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/gates/lib.sh"

# Turn one run's log into one of the four verdicts. `status` is nextest's own exit code.
#
# A pure function of its two arguments, which is what makes the seam above worth having: every
# branch below is reachable from a text file and an integer.
account_for_the_run() {
    local log="$1" status="$2"

    if ((status != 0)); then
        # Not necessarily an oracle failure — a workspace that will not build reaches here too,
        # and saying "the rung failed" about that would be this script guessing. The two suites
        # are named because a reader who arrives here needs to know which ones to run by hand.
        printf 'rung-oracles: FAIL — nextest exited %s over %s; the suites are crates/engine/tests/oracles.rs and testkit::oracle\n' \
            "$status" "$selection" >&2
        return "$status"
    fi

    local declined
    declined="$({ grep -cE '^[[:space:]]*SKIP oracles:' "$log" || true; })"
    if ((declined > 0)); then
        printf 'rung-oracles: SKIPPED — the rung declined, %s time(s), each naming what was missing, what it therefore did not claim, and the remedy:\n' \
            "$declined"
        grep -E '^[[:space:]]*SKIP oracles:' "$log" | sed 's/^[[:space:]]*/rung-oracles:   /'
        return 0
    fi

    local ran
    ran="$({ grep -cE '^[[:space:]]*oracles: [0-9]+ container claim' "$log" || true; })"
    if ((ran == 0)); then
        printf 'rung-oracles: FAIL — the suites passed but said neither that they ran nor that they declined; one of the two is a lie\n' >&2
        return 1
    fi
    printf 'rung-oracles: RAN — %s file(s) validated:\n' "$ran"
    grep -E '^[[:space:]]*oracles: [0-9]+ container claim' "$log" |
        sed 's/^[[:space:]]*oracles: /rung-oracles:   /'
    return 0
}

# The recorded-log mode. See "the seam" in the header.
if [[ -n "${WCH_RUNG_ORACLES_ACCOUNT:-}" ]]; then
    printf 'rung-oracles: ACCOUNTING FOR THE RECORDED LOG AT %s (WCH_RUNG_ORACLES_ACCOUNT) — NOTHING IS BUILT AND NEITHER ORACLE IS INVOKED\n' \
        "$WCH_RUNG_ORACLES_ACCOUNT"
    if [[ ! -f "$WCH_RUNG_ORACLES_ACCOUNT" ]]; then
        printf 'rung-oracles: FAIL — %s is not a file; there is no recorded run to account for\n' \
            "$WCH_RUNG_ORACLES_ACCOUNT" >&2
        exit 1
    fi
    recorded_status="${WCH_RUNG_ORACLES_STATUS:-0}"
    if [[ ! "$recorded_status" =~ ^[0-9]+$ ]]; then
        printf 'rung-oracles: FAIL — WCH_RUNG_ORACLES_STATUS=%q is not an exit status\n' \
            "$recorded_status" >&2
        exit 1
    fi
    status=0
    account_for_the_run "$WCH_RUNG_ORACLES_ACCOUNT" "$recorded_status" || status=$?
    exit "$status"
fi

root="$(git rev-parse --show-toplevel)"
cd "$root"

log="$(mktemp "$(gate_scratch_root)/wch-rung-oracles.XXXXXXXX")"
trap 'rm -f "$log"' EXIT

status=0
# `--success-output final` is also in `.config/nextest.toml` for the engine binary; passing it
# here means this script says what it needs rather than inheriting it, and it is what makes the
# testkit arms' own declines visible too — those live in a lib-test binary the config does not
# name, because that binary is mostly the conformance battery.
cargo nextest run --locked --offline --workspace --no-tests=fail \
    --success-output final -E "$selection" 2>&1 | tee "$log" || status=$?

verdict=0
account_for_the_run "$log" "$status" || verdict=$?
exit "$verdict"
