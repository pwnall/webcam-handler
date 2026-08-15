#!/usr/bin/env bash
#
# The container-oracle rung keeps its four verdicts apart — docs/9's "Oracle rung accounting"
# row, and AGENTS rule 3: *"every auto-skipping rung (vivid, hardware, oracles) reports a named,
# counted skip — never silence."*
#
# ## Why this predicate exists at all
#
# `scripts/rung-oracles.sh` is a `g6` criterion **command**, not a predicate under
# `scripts/gates/`, and it has to be: `run-all.sh` and `selftest.sh` derive their population from
# this directory's listing, so a runner placed here would be a predicate the selftest demands
# both-direction arms for and would run on every push over a suite that takes seconds and a pair
# of external programs. `scripts/mutants.sh` is in exactly that position for exactly that reason,
# and `mutation-verdict.sh` is the predicate that was written when three notes in a row (N52,
# N66, N68) found its verdicts moving with the machine rather than with the code. This is that
# arrangement one rung along, written **with** the rung rather than after the third time.
#
# The four answers the runner must keep apart, and what each one is a statement about:
#
#   - **RAN** — the suites made claims and named how many. A statement about the files.
#   - **SKIPPED** — an oracle is not installed, so claims went unasked. A statement about the
#     *host*, and the one the whole "named, counted skip" rule is about.
#   - **FAIL, nextest was non-zero** — a statement about the run, which is deliberately *not*
#     reported as an oracle failure: a workspace that will not build reaches that branch too.
#   - **FAIL, neither line appeared** — a statement about the rung itself, and the arm that
#     matters most. A suite that stopped printing its accounting, a filterset that stopped
#     selecting anything, a rename that silently emptied the selection: each of those is green
#     everywhere else in this repository and red only here. A rung nobody can tell apart from one
#     that was never invoked is the defect class the ladder exists for.
#
# ## What it drives, and the seam — because a real run needs two programs and a build
#
# The rung compiles the workspace and shells out to `ffprobe` and `mpv`. Nothing in `just ci`
# should do that twice, and — more to the point — **this host has both oracles**, so two of the
# four verdicts above could not be produced here at all by running the real thing. But what is
# being checked is not the running, it is the *accounting*, and the accounting is a pure function
# of a log and an exit status.
#
# So a recorded log is a fixture: `$WCH_RUNG_ORACLES_ACCOUNT` points the shipped runner at one
# and `$WCH_RUNG_ORACLES_STATUS` supplies the exit status to account with. That is **a mode of
# the shipped script, not a reimplementation of it** — rubric rule 6's requirement that at least
# one arm runs the real tool (note N10). The verdict logic, the `grep -c` guards, the line
# shapes and the exit codes below are all the runner's own.
#
# The fixtures are **derived from the harness's own line shapes** rather than transcribed, which
# is docs/9's derived-population rule: the run line and the decline line are built here from the
# same words `testkit::oracle::OracleReport::run_line` and `decline_lines` produce, so a change
# to either shape that the runner's `grep` no longer matches turns this predicate red instead of
# turning the rung silent.
#
# ## The seam this predicate offers its own case file
#
#   $WCH_GATE_ORACLE_RUNNER   the runner-shaped program to drive. `pass_case` leaves it unset,
#                             which is the real `scripts/rung-oracles.sh`; the failing arms point
#                             it at runners that get exactly one of the four verdicts wrong.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

# The script under test comes from the real checkout rather than from `gate_root`: a scratch copy
# of the tree is what the seeded arms hand this predicate, and they replace the runner wholesale
# through the seam above. `mutation-verdict.sh` makes the identical distinction.
checkout="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
runner="${WCH_GATE_ORACLE_RUNNER:-$checkout/scripts/rung-oracles.sh}"

if [[ ! -x "$runner" ]]; then
    gate_fail "$runner is not an executable program; this gate has no rung runner to drive"
    gate_finish
fi
gate_note "driving $runner"

scratch="$(mktemp -d "$(gate_scratch_root)/wch-oracle-rung.XXXXXXXX")"
trap 'rm -rf "$scratch"' EXIT

# ------------------------------------------------------------------ the fixtures
#
# Three logs, each one a nextest transcript reduced to the lines the accounting reads. The
# indentation is nextest's: it indents a captured test's output, which is why the runner's
# patterns allow leading whitespace and why these fixtures carry it — a fixture written flush
# left would pass a runner whose anchors had lost their `[[:space:]]*`.

ran_log="$scratch/ran.log"
{
    printf '        PASS [   1.9s] (1/2) webcam-handler-engine::oracles a_recording_the_product_path_produced_is_the_file_two_third_parties_read_back\n'
    printf '    oracles: 4 container claim(s) about %s/take.avi checked by ffprobe and mpv\n' "$scratch"
    printf '    oracles: 4 container claim(s) about %s/take.y4m checked by ffprobe and mpv\n' "$scratch"
    printf '     Summary [   2.0s] 2 tests run: 2 passed, 0 skipped\n'
} >"$ran_log"

declined_log="$scratch/declined.log"
# SC2016: the backticks below are *inside* a decline sentence this fixture reproduces verbatim,
# not command substitutions this script wants performed. Spelling them any other way would make
# the fixture stop being what `testkit::oracle::OracleReport::decline_lines` emits, which is the
# one property that makes it evidence about the runner's own `grep`.
# shellcheck disable=SC2016
{
    printf '        PASS [   0.1s] (1/2) webcam-handler-engine::oracles a_recording_the_product_path_produced_is_the_file_two_third_parties_read_back\n'
    printf '    SKIP oracles: ffprobe is not on this host (no `ffprobe` on this host'"'"'s PATH), so 3 claim(s) about %s/take.avi went unasked — demux, frames, rate — and nothing here says whether libavformat demuxes this file into the frames, geometry and rate its summary claims; remedy: install ffmpeg'"'"'s ffprobe (Debian/Ubuntu: `apt install ffmpeg`)\n' "$scratch"
    printf '    SKIP oracles: mpv is not on this host (no `mpv` on this host'"'"'s PATH), so 1 claim(s) about %s/take.avi went unasked — playback — and nothing here says whether the file plays end to end; remedy: install mpv (Debian/Ubuntu: `apt install mpv`)\n' "$scratch"
    printf '     Summary [   0.2s] 2 tests run: 2 passed, 0 skipped\n'
} >"$declined_log"

# The one that matters: a suite that passed and said nothing about itself. Not an empty file —
# an empty log would also be what a runner that never started produces, and the claim here is
# narrower and worse: **tests ran, they were green, and no line says whether a single oracle was
# consulted.**
silent_log="$scratch/silent.log"
{
    printf '        PASS [   0.1s] (1/2) webcam-handler-engine::oracles a_recording_the_product_path_produced_is_the_file_two_third_parties_read_back\n'
    printf '        PASS [   0.1s] (2/2) webcam-handler-engine::oracles the_rate_libavformat_reads_back_is_the_mean_the_take_measured_in_either_container\n'
    printf '     Summary [   0.2s] 2 tests run: 2 passed, 0 skipped\n'
} >"$silent_log"

# ------------------------------------------------------------------- the four claims

checked=0

# Drive the runner over one recorded log and check its verdict.
#
# Three things per call, because a verdict is a word *and* an exit code and a runner that got
# either half wrong would be believed: the exit status, the word the transcript ends on, and —
# for the two `FAIL` arms — that the word went to standard error, where a reader skimming for it
# will find it.
expect_verdict() {
    local label="$1" log="$2" recorded_status="$3" want_status="$4" want_word="$5"
    checked=$((checked + 1))

    local out status=0
    out="$(WCH_RUNG_ORACLES_ACCOUNT="$log" WCH_RUNG_ORACLES_STATUS="$recorded_status" \
        "$runner" 2>&1)" || status=$?

    if ((status != want_status)); then
        gate_fail "$label: the runner exited $status over the recorded log and should have exited $want_status"
        printf '%s\n' "$out" | sed 's/^/        /' >&2
        return
    fi
    if ! grep -qE "rung-oracles: $want_word" <<<"$out"; then
        gate_fail "$label: the runner exited $status without saying '$want_word'; a verdict nobody can read is a verdict nobody can act on"
        printf '%s\n' "$out" | sed 's/^/        /' >&2
        return
    fi
    # Every mode announces that it read a recorded log rather than a camera-less machine's real
    # one. `smoke-hw.sh`'s seam says the same thing in capitals for the same reason: a mode that
    # ran no oracle must never be mistakable for one that did.
    if ! grep -q 'ACCOUNTING FOR THE RECORDED LOG' <<<"$out"; then
        gate_fail "$label: the runner accounted for a recorded log without saying so"
    fi
}

# RAN: claims were made and counted.
expect_verdict "the run verdict" "$ran_log" 0 0 "RAN"
# SKIPPED: an oracle was missing, and each decline is reprinted rather than summarised away.
expect_verdict "the decline verdict" "$declined_log" 0 0 "SKIPPED"
# FAIL, and about the *run* rather than about the files.
expect_verdict "the nextest-was-red verdict" "$ran_log" 3 3 "FAIL — nextest exited 3"
# FAIL, and the arm this predicate is most for.
expect_verdict "the said-nothing verdict" "$silent_log" 0 1 "FAIL — the suites passed but said neither"

# The decline verdict has to *reprint* what was declined, or "4 claim(s) declined" is a number a
# reader cannot price. Asked separately because the arm above turns on the word alone.
checked=$((checked + 1))
declined_out="$(WCH_RUNG_ORACLES_ACCOUNT="$declined_log" "$runner" 2>&1 || true)"
if ! grep -qE 'rung-oracles: +SKIP oracles: ffprobe' <<<"$declined_out"; then
    gate_fail "the decline verdict did not reprint the lines it counted; a count without its reasons is the skip that reads as a pass"
fi
if ! grep -qE 'remedy: install' <<<"$declined_out"; then
    gate_fail "the reprinted declines name no remedy; AGENTS' primary consumer has no hands to work one out with"
fi

# A seam that is handed nothing to account for is a failure rather than a quiet success — the
# shape `smoke-hw.sh`'s own seam takes, and the one that stops a typo'd path from reading as a
# green rung.
checked=$((checked + 1))
missing_status=0
WCH_RUNG_ORACLES_ACCOUNT="$scratch/there-is-no-such-log" "$runner" >/dev/null 2>&1 ||
    missing_status=$?
if ((missing_status == 0)); then
    gate_fail "the runner accounted for a log that does not exist and exited 0"
fi

gate_checked "$checked" "verdicts the oracle rung runner must keep apart"
gate_require_nonzero "$checked" "verdicts"
gate_finish
