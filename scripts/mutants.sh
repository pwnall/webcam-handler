#!/usr/bin/env bash
#
# The mutation floor (docs/7 P3f, docs/9 Part 2).
#
# Every other rung asks "does the suite run this code?". This one asks the question that
# matters: **does the suite constrain it?** cargo-mutants edits one line of a pure core —
# flips a comparison, empties a `Vec`, returns the other `bool` — rebuilds, and runs the
# whole workspace suite. A mutant the suite still passes is a line no test is watching.
#
# ## The scope lives in `.cargo/mutants.toml`
#
# Six files: the planners, the state machine, the settle policy, the store, the metrics.
# That file carries the reasoning for what is in and what is deliberately out. This script
# does not restate the list — it asks cargo-mutants what the scope resolved to and prints
# the answer, and refuses to run on an empty one: a mutation job over zero files is the
# "check that examines nothing" this suite exists to prevent.
#
# ## Survivors are triaged, never tolerated
#
# `scripts/mutants-accepted.txt` is the list of survivors this project has looked at and
# accepted, each with the note that argues no hermetic test can kill it. The comparison
# runs in **both directions**:
#
#   - a survivor that is not on the list fails this job — that is a missing test;
#   - a line on the list that no longer survives also fails it — the mutant became
#     killable, and an acceptance nobody re-checks is how N15's mistake gets made twice.
#
# The list is an exception register, not the population: the population is derived from
# the tree by cargo-mutants on every run.
#
# ## Three outcomes, and the one that was missing until note N68
#
# This job used to have two words for three different things, and every one of them was
# spelled `FAIL`:
#
#   - **a finding** — a survivor with no acceptance, or an acceptance that stopped
#     surviving. This is a statement about the *code and the register*, it is what the job
#     exists to produce, and it exits **1**;
#   - **a green** — the floor ran and the register is clean both ways. Exit **0**;
#   - **no verdict** — the run could not answer at all. The disk filled (note N66: the run
#     died fifteen minutes in with `Disk quota exceeded` and printed FAIL), it was
#     interrupted, its baseline would not build, or the working tree it spent an hour
#     reading **changed underneath it** (note N68). None of those is a statement about the
#     code. It exits **$GATE_NO_VERDICT** — 75, `EX_TEMPFAIL`; `scripts/gates/lib.sh`
#     argues the number — which is non-zero, because an unproven criterion must never read
#     as green, and *distinct*, because a machine's shortfall wearing a defect's clothes is
#     what N52, N66 and N68 each cost a session.
#
# The three are a class, not three accidents: N52's verdict moved with **time** (`nproc`,
# via the test timeout), N66's with **space** (free space on `/tmp`), N68's with a **moving
# input** (an agent editing files this run was still reading). All three are the machine
# being reported as the code. N60 records the bill: "a gate that cries wolf does not get
# believed, it gets re-run at `-j1` until it agrees, and the run after that is the one
# where a real survivor is waved through".
#
# A missing configuration file, an empty scope and a run that generated zero mutants stay
# **findings**, deliberately: those are statements about the tree, and a floor that has
# been quietly disarmed is exactly what this job is for.
#
# ## The seam: the verdict is a function of a directory of text files
#
# cargo-mutants writes `caught.txt`, `missed.txt`, `timeout.txt` and `unviable.txt` under
# its output directory — one mutant per line — beside `outcomes.json`'s census. Everything
# below the run reads exactly those files and nothing else, so **a recorded run is a
# fixture**: point $WCH_MUTANTS_CLASSIFY at a directory holding them and this script
# classifies it and exits, building nothing and generating no mutants. A fixture may carry
# its own register (`accepted.txt`) and its own record of the tree the run started from
# (`tree-before.txt`), which is the only way to exercise "the input moved" without moving
# anybody's checkout.
#
# That is a mode of the shipped script, not a second implementation of it, and the
# difference matters: `scripts/gates/mutation-verdict.sh` proves the three outcomes in
# seconds by driving *this file* over recorded result sets, so what is proved is what runs
# (rubric rule 6, paid for by note N10). A real run takes forty minutes and is nobody's
# unit test; the classification is the part that was wrong, and the classification is the
# part that is now driven both ways.
#
# ## It is a rung, not a `just ci` step
#
# It rebuilds the workspace once per mutant, so it costs hours where `just ci` costs
# minutes (docs/9 records the measured number). Its cadence is a G4 criterion — docs/7
# commissioned it "before G4, not after" — and `phase-criteria.tsv` carries the row, so
# the schedule is mechanical rather than remembered.
#
# Like every rung here it reports a named, counted skip rather than exiting quietly when
# the tool it needs is absent: cargo-mutants is a dev tool and installing it is never a
# requirement of `just ci`.
set -euo pipefail

# `lib.sh` for `$GATE_NO_VERDICT` and the three `gate_tree_*` helpers, and for nothing
# else: this is a criterion command rather than a gate predicate, and it keeps its own
# `mutants:` reporting vocabulary. Sourcing it is the alternative to a second copy of the
# exit-code convention and of the tree recording `selftest.sh` already had — and a second
# copy of a law is the defect AGENTS.md names, not a style preference. The library has no
# side effects at source time beyond setting variables.
#
# shellcheck source-path=SCRIPTDIR
# shellcheck source=gates/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/gates/lib.sh"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# The checkout, asked of git *at this script's own location* rather than at `$PWD`, so a
# run from another directory cannot resolve a different repository. A tree git cannot
# identify — a source tarball, a stripped export — still has a root: this file lives in
# `scripts/`, so its parent is it. The fallback is what makes "no git" a skip below
# instead of an abort.
tree_watched=1
if ! root="$(git -C "$here" rev-parse --show-toplevel 2>/dev/null)"; then
    root="$(dirname "$here")"
    tree_watched=0
fi
config="$root/.cargo/mutants.toml"
accepted_file="$root/scripts/mutants-accepted.txt"
# Where `.cargo/mutants.toml`'s `output` key puts the report. Under `target/` so the
# tree-walking gates and the selftest's tree copies do not pay for a rung they never run;
# that file says why.
out_dir="$root/target/mutants.out"

skips=0
skip() {
    skips=$((skips + 1))
    printf 'mutants: SKIP %s — %s\n' "$skips" "$*"
}

# A statement about the code or the register. The wording is unchanged from before N68,
# because this outcome was never the ambiguous one.
finding() {
    printf 'mutants: FAIL — %s\n' "$*" >&2
    exit 1
}

# A statement about *this run*, which produced no verdict to report. First argument is the
# headline; the rest are detail lines. Never green, never a finding, and it says which it
# is in words as well as in its exit code, because the reader this is written for is
# skimming for the string "FAIL".
no_verdict() {
    local headline="$1" line
    shift
    printf 'mutants: NO VERDICT — %s\n' "$headline" >&2
    for line in "$@"; do
        printf 'mutants:   %s\n' "$line" >&2
    done
    printf 'mutants: this run answered nothing, so it is NOT a finding: no unaccepted survivor was seen and no acceptance was disproved. Exit %s (EX_TEMPFAIL); %s named skip(s)\n' \
        "$GATE_NO_VERDICT" "$skips" >&2
    exit "$GATE_NO_VERDICT"
}

# This run's own bookkeeping — survivor lists, the sorted register, a throw-away criteria
# table — which is kilobytes and goes where all test scratch goes since the 2026-08-12 ruling
# (note N84). The *build* root is a different question and is answered further down, at
# `build_root`, where the measurement that decides it lives.
scratch="$(mktemp -d "$(gate_scratch_root)/wch-mutants.XXXXXXXX")"
trap 'rm -rf "$scratch"' EXIT

# ---------------------------------------------------------------- the tree, recorded
#
# **The floor reads the working tree for the better part of an hour**, one mutant at a
# time, and its verdict is only about the tree it read. Note N68: a `just gate-g4` started
# at 08:14 was still running at 09:19 when an agent edited `crates/engine/tests/sweep.rs`,
# `crates/engine/src/calibrate.rs` and `crates/engine/src/settle.rs` — all inside the scope
# or feeding the suite that judges it — and the run reported three N25 acceptances as lies.
# They were not: an N25 acceptance argues the mutant is the *same program* given the
# callers' preconditions, and no test can distinguish identical programs (which is exactly
# how N60 settled its own false positive). The verdict was neither true nor false, it was
# **void**, and nothing in this script could say so.
#
# Recorded and compared, not asserted clean, for `selftest.sh`'s reason: running this on a
# dirty tree is ordinary — it is how you check a fix before committing it — and "you had
# uncommitted work" is not a finding. What is a finding is that the tree is not the one the
# run started on.
tree_before=""
if ((tree_watched == 1)) && gate_tree_watchable "$root"; then
    tree_before="$(gate_tree_state "$root")"
else
    tree_watched=0
    skip "git cannot describe $root (a source tarball, or no git), so \"the tree did not move while this run read it\" was not checked; a missing tool is not a violation"
fi

# Compare the tree against what was recorded before the run. Called before anything else
# is interpreted, because a tree that moved explains every downstream symptom — a red
# baseline, a vanished mutant, an acceptance that "stopped surviving" — and reporting the
# symptom instead of the cause is what N68 is about.
tree_did_not_move() {
    local after
    ((tree_watched == 1)) || return 0
    after="$(gate_tree_state "$root")"
    [[ "$tree_before" == "$after" ]] && return 0
    local -a changes=()
    mapfile -t changes < <(gate_tree_changes "$tree_before" "$after")
    no_verdict \
        "the tree moved while the floor was reading it; a mutation result describes one tree and this run read two" \
        "${changes[@]}" \
        "(\`<\` is the tree this run started on, \`>\` is the tree it ended on; the first line of each is the commit)" \
        "nothing here is a claim about the code: re-run the floor on a tree nobody is editing"
}

if [[ ! -f "$config" ]]; then
    finding "no scope at $config; a mutation run with no scope has no meaning"
fi
if [[ ! -f "$accepted_file" ]]; then
    finding "no acceptance register at $accepted_file"
fi

# The scope, for the run branch; empty and unused when a recorded run is being classified.
declare -a scope=()
scope_desc=""

classify_dir="${WCH_MUTANTS_CLASSIFY:-}"
if [[ -n "$classify_dir" ]]; then
    # ------------------------------------------------------------ the recorded-run mode
    #
    # See "the seam" above. Nothing is built and no mutant is generated: the mutants in a
    # fixture were generated by a real run once, and what is under test here is the
    # classification of them.
    if [[ ! -d "$classify_dir" ]]; then
        finding "WCH_MUTANTS_CLASSIFY=$classify_dir is not a directory; there is no recorded run to classify"
    fi
    out_dir="$classify_dir"
    scope_desc="the recorded result set at $classify_dir"
    printf 'mutants: classifying the recorded result set at %s (WCH_MUTANTS_CLASSIFY); nothing is built and no mutant is generated\n' \
        "$classify_dir"
    if [[ -f "$classify_dir/accepted.txt" ]]; then
        accepted_file="$classify_dir/accepted.txt"
        printf 'mutants: judged against the register that travels with it, %s\n' "$accepted_file"
    fi
    if [[ -f "$classify_dir/tree-before.txt" ]] && ((tree_watched == 1)); then
        # A recorded before-state is the whole point of a fixture for N68's defect: the
        # only other way to exercise "the input moved" is to move somebody's checkout,
        # which no gate here is allowed to do.
        tree_before="$(cat "$classify_dir/tree-before.txt")"
        printf 'mutants: the tree state this run started from was read from %s\n' \
            "$classify_dir/tree-before.txt"
    fi
    tree_did_not_move
else
    # ------------------------------------------------------------ the real run
    if ! cargo mutants --version >/dev/null 2>&1; then
        skip "cargo-mutants is not installed (\`cargo install cargo-mutants\`); it is a dev tool, never a workspace dependency, and \`just ci\` does not need it"
        printf 'mutants: 0 mutants run, %s named skip(s)\n' "$skips"
        exit 0
    fi

    # The scope, counted — and asked of the tool rather than transcribed from the config it
    # reads, so a glob that has stopped matching shows up here as a smaller number instead of
    # as a silently narrower floor (note N10's family in a mutation costume).
    mapfile -t scope < <(cargo mutants --list-files)
    if ((${#scope[@]} == 0)); then
        finding "$config selects no files; a mutation run over nothing cannot go red"
    fi
    scope_desc="${#scope[@]} file(s)"

    printf 'mutants: scope is %s file(s), per %s\n' "${#scope[@]}" "${config#"$root"/}"
    printf 'mutants:   %s\n' "${scope[@]}"

    jobs="${WCH_MUTANTS_JOBS:-$(nproc)}"

    # Debug info off, and this is a space decision before it is a speed one.
    #
    # `just ci`'s own `target/` on this machine is 34 GiB, nearly all of it DWARF for the
    # workspace's test binaries. cargo-mutants gives each job a whole copy of the tree with its
    # own build directory, so seven jobs at the shipped profile is seven copies of that — an
    # order of magnitude more space than any `tmpfs` `/tmp` holds, and the failure mode is a run
    # that dies on ENOSPC an hour in having produced nothing. Measured: the first build
    # directory of the run before this setting reached 6.1 GiB on its own.
    #
    # Turning debug info off cannot change a verdict — it changes what a backtrace can say, not
    # what a test asserts — and it makes the links this job spends most of its time on much
    # cheaper. It is an environment override rather than a profile in `Cargo.toml`, so the
    # shipped build configuration is exactly what `just ci` uses and this job's difference from
    # it is one line, here, in the open.
    export CARGO_PROFILE_DEV_DEBUG=0
    export CARGO_PROFILE_TEST_DEBUG=0

    # Each job is a whole copy of the tree with its own build directory. So the constraint on
    # parallelism is **space and I/O, not cores** — and both were measured on the machine this
    # job was written on:
    #
    #   - `$TMPDIR` on `tmpfs`: about 7 mutants a minute.
    #   - the same run with the build directories moved onto the disk holding `target/`: under
    #     one a minute. Concurrent cargo builds are I/O bound long before they are CPU bound,
    #     and putting them on the same spindle as everything else costs an order of magnitude.
    #
    # So `$TMPDIR` is the default, and `WCH_MUTANTS_BUILD_ROOT` is the escape for a machine
    # whose `$TMPDIR` is too small to hold `jobs` build trees — slower, and worth it only
    # against not running at all.
    #
    # The job count is then trimmed to what the build root can actually hold, out loud, from a
    # per-job figure measured with the debug info off — **and one job's worth is held back**.
    #
    # The reserve is not caution, it is a defect this job already had (note **N66**). Dividing
    # the free space by the per-job figure spends the whole filesystem: on this host that was
    # five jobs at three GiB in a sixteen GiB `tmpfs`, 15/16 of it, and the P4f boundary run
    # died fifteen minutes in with `Disk quota exceeded` — because the figure was measured on a
    # workspace that has since grown a crate, and a build tree that is now a little over three
    # GiB needs the sixteenth. What made that worse than slow is what it *said*: the floor
    # exited 137 and reported FAIL, which is the same verdict a surviving mutant gets. A gate
    # whose resource budget can spell itself as a survivor is N52's finding in a second
    # dimension, and N60 records what the reflex costs — a run that is re-run until it agrees is
    # a run nobody reads. So the figure stays the measurement it is, and the *budget* leaves a
    # tree's worth of room.
    # **The one thing the 2026-08-12 ruling deliberately did not move** (note N84). That
    # ruling put test scratch under `target/` because a leak filled a tmpfs; this is not that
    # leak and moving it would be a knowing 7× regression in the measurement two paragraphs
    # up — the same run with its build directories on the disk that holds `target/` went from
    # about seven mutants a minute to under one. It is also already bounded in the way the
    # ruling asks for: the budget below reserves a whole build tree and answers a shortfall
    # with a no-verdict rather than a FAIL, the `EXIT` trap and cargo-mutants' own cleanup
    # take the trees on the normal path, and `gate_scratch_sweep` reaches this root too. What
    # the sweep does *not* reach is what cargo-mutants names itself, `cargo-mutants-*`, which
    # note N84 records as a residual rather than pretending otherwise.
    # wch-scratch-exempt: the mutation floor's build trees, measured at 7× on tmpfs (E7, N66)
    build_root="${WCH_MUTANTS_BUILD_ROOT:-${TMPDIR:-/tmp}}"
    mkdir -p "$build_root"
    # wch-scratch-exempt: cargo-mutants reads the build root out of the environment
    export TMPDIR="$build_root"

    per_job_gib=3
    reserve_gib="$per_job_gib"
    avail_gib="$(df -BG --output=avail "$build_root" | tail -1 | tr -dc '0-9')"
    fits=$(((avail_gib - reserve_gib) / per_job_gib))
    if ((fits < 1)); then
        # A resource shortfall, and since N68 it is spelled as one. It was a FAIL until
        # then — the same word a surviving mutant gets — which made the floor's verdict a
        # function of how much room `/tmp` happened to have.
        no_verdict \
            "$build_root has $avail_gib GiB free, and one build tree needs about $per_job_gib with $reserve_gib held back" \
            "this is a fact about the filesystem, not about the tests: set WCH_MUTANTS_BUILD_ROOT to a larger one and the same tree will answer"
    fi
    if ((fits < jobs)); then
        printf 'mutants: %s has %s GiB free, so %s job(s) rather than %s (about %s GiB each, %s GiB held back)\n' \
            "$build_root" "$avail_gib" "$fits" "$jobs" "$per_job_gib" "$reserve_gib"
        jobs="$fits"
    fi
    printf 'mutants: build directories under %s (%s GiB free, %s GiB held back)\n' \
        "$build_root" "$avail_gib" "$reserve_gib"
    declare -a extra=()
    if [[ "${WCH_MUTANTS_ITERATE:-0}" == "1" ]]; then
        # Development convenience only: re-runs skip what a previous run already caught. A
        # gate run never sets it, because "caught last time" is not a measurement of this tree.
        extra+=(--iterate)
        printf 'mutants: WCH_MUTANTS_ITERATE=1 — skipping mutants caught by a previous run; this is NOT a gate run\n'
    fi

    printf 'mutants: running %s job(s) over the workspace suite\n' "$jobs"
    started="$(date +%s)"
    status=0
    cargo mutants -j "$jobs" "${extra[@]}" "$@" || status=$?
    elapsed=$(($(date +%s) - started))
    printf 'mutants: cargo-mutants exited %s after %sm%ss\n' \
        "$status" "$((elapsed / 60))" "$((elapsed % 60))"

    # First, because a tree that moved explains everything below it.
    tree_did_not_move

    case "$status" in
    0 | 2 | 3) ;; # every mutant caught / some survived / some timed out — all triageable
    4)
        no_verdict \
            "the baseline suite is red in an unmutated tree, so no survivor list from this run means anything" \
            "the finding is whatever \`just ci\` says about the unmutated tree; this job has nothing to add until that is green"
        ;;
    *)
        # Everything else: killed (137 is the shape N66's disk failure took), interrupted,
        # a tool error. All of them are "the run stopped", none of them is a survivor.
        no_verdict \
            "cargo-mutants could not complete (exit $status); see ${out_dir#"$root"/}" \
            "exit 137 is the shape a killed run takes — out of memory, out of disk, or somebody stopped it (note N66)"
        ;;
    esac

    # A real run always writes its own summary, so its absence means the run did not
    # finish writing — which is a stopped run rather than a clean floor. A *fixture* may
    # legitimately have no `outcomes.json`; a fixture is a directory of text files, and the
    # cross-check below is skipped when there is nothing to cross-check against.
    if [[ ! -f "$out_dir/outcomes.json" ]]; then
        no_verdict \
            "cargo-mutants exited $status and left no summary at ${out_dir#"$root"/}/outcomes.json" \
            "a run that never wrote its own census did not finish, whatever its exit code said"
    fi
fi

# ---------------------------------------------------------------- the census
#
# Everything from here down is a function of `$out_dir` and `$accepted_file`, which is what
# makes a recorded run a fixture (see "the seam" above).
#
# The four text files are what the verdict is computed from; `outcomes.json` is
# cargo-mutants' own summary of the same run. **They are cross-checked**, and a
# disagreement is a *no verdict* rather than a finding: a result set with rows missing
# cannot say whether the missing rows were caught or survived, and guessing is how a run
# that died half way through gets read as a clean floor.
result_files=(caught.txt missed.txt timeout.txt unviable.txt)
declare -a absent=()
for f in "${result_files[@]}"; do
    [[ -f "$out_dir/$f" ]] || absent+=("$f")
done
if ((${#absent[@]} > 0)); then
    no_verdict \
        "the result set at $out_dir is incomplete: no ${absent[*]}" \
        "a verdict is computed from those rows; rows that are not there can be classified as neither caught nor surviving"
fi

# `grep -c ''` rather than `wc -l`: it counts a final line that has no newline on it, which
# `wc -l` does not, and a fixture written by hand is exactly where that difference shows up.
# It exits 1 on an empty file, which is not an error here.
line_count() {
    local n=""
    n="$(grep -c '' <"$1")" || true
    printf '%s\n' "${n:-0}"
}

caught="$(line_count "$out_dir/caught.txt")"
missed="$(line_count "$out_dir/missed.txt")"
timed_out="$(line_count "$out_dir/timeout.txt")"
unviable="$(line_count "$out_dir/unviable.txt")"
generated=$((caught + missed + timed_out + unviable))

if [[ -f "$out_dir/outcomes.json" ]]; then
    if ! command -v jq >/dev/null 2>&1; then
        skip "jq is not installed, so $out_dir/outcomes.json was not cross-checked against the rows; the rows themselves are the census"
    else
        declare -a claimed=()
        mapfile -t claimed < <(
            jq -r '[.total_mutants, .caught, .missed, .timeout, .unviable] | .[] | tostring' \
                "$out_dir/outcomes.json" 2>/dev/null || true
        )
        if ((${#claimed[@]} != 5)); then
            no_verdict \
                "$out_dir/outcomes.json is not a census this script can read" \
                "a run that was killed while writing its summary leaves exactly this, and a summary that cannot be read is not evidence of anything"
        fi
        declare -a mismatch=()
        [[ "${claimed[1]}" == "$caught" ]] || mismatch+=("outcomes.json says ${claimed[1]} caught, caught.txt holds $caught row(s)")
        [[ "${claimed[2]}" == "$missed" ]] || mismatch+=("outcomes.json says ${claimed[2]} missed, missed.txt holds $missed row(s)")
        [[ "${claimed[3]}" == "$timed_out" ]] || mismatch+=("outcomes.json says ${claimed[3]} timed out, timeout.txt holds $timed_out row(s)")
        [[ "${claimed[4]}" == "$unviable" ]] || mismatch+=("outcomes.json says ${claimed[4]} unviable, unviable.txt holds $unviable row(s)")
        [[ "${claimed[0]}" == "$generated" ]] || mismatch+=("outcomes.json says ${claimed[0]} mutants in total, the four files hold $generated between them")
        if ((${#mismatch[@]} > 0)); then
            no_verdict \
                "the run's own summary and the rows it wrote disagree, so this result set is not a complete description of any run" \
                "${mismatch[@]}" \
                "a run that died part way through leaves this (note N66 died at 23 of 525); so would a cargo-mutants that has grown a sixth category, in which case the fix is in this script and not in the tree"
        fi
    fi
fi

# A run that generated nothing is a green that proves nothing. A *finding*, not a
# no-verdict: the scope resolved to files and produced no mutants from them, which is a
# floor that has been disarmed rather than a machine that ran short.
if ((generated == 0)); then
    finding "zero mutants generated over ${scope_desc:-the scope}; a floor with no mutants cannot go red"
fi

# Line numbers move when a file is edited above them, so a survivor is keyed by what it
# does and where it lives, not by where it sat on the day it was triaged. Two mutants in
# one file can then share a key — `replace > with >=` twice in one function is two
# different comparisons — so the comparison is over **multisets**: duplicates are kept on
# both sides, and two survivors sharing a key need two lines in the register. Without that,
# accepting one of a pair would silently accept the other, which is note N10's family.
strip_position() { sed 's/^\([^:]*\.rs\):[0-9]*:[0-9]*: /\1: /'; }

# One `mutants:` prefix per line of a multi-line finding.
report() {
    local line
    while IFS= read -r line; do
        printf 'mutants:   %s\n' "$line"
    done
}

# A timed-out mutant is an uncaught mutant with a worse failure mode, so it is triaged
# beside the missed ones rather than counted as a pass.
#
# `LC_ALL=C` on both sorts and on the comparison: `sort` collates by locale and `comm`
# compares bytes, and the two disagree on any line with punctuation in it — which is every
# mutant name. Observed as `comm: file 1 is not in sorted order` on the first run that
# produced survivors.
cat "$out_dir/missed.txt" "$out_dir/timeout.txt" |
    strip_position | LC_ALL=C sort >"$scratch/survivors"
sed 's/#.*//' "$accepted_file" | sed 's/[[:space:]]*$//' | grep -v '^$' |
    LC_ALL=C sort >"$scratch/accepted" || true

unexpected="$(LC_ALL=C comm -23 "$scratch/survivors" "$scratch/accepted")"
stale="$(LC_ALL=C comm -13 "$scratch/survivors" "$scratch/accepted")"

printf 'mutants: %s generated — %s caught, %s missed, %s timed out, %s unviable\n' \
    "$generated" "$caught" "$missed" "$timed_out" "$unviable"
printf 'mutants: %s survivor(s), %s recorded acceptance(s)\n' \
    "$(wc -l <"$scratch/survivors")" "$(wc -l <"$scratch/accepted")"

failures=0
if [[ -n "$unexpected" ]]; then
    failures=$((failures + $(wc -l <<<"$unexpected")))
    printf 'mutants: FAIL — %s survivor(s) with no recorded acceptance:\n' \
        "$(wc -l <<<"$unexpected")" >&2
    report <<<"$unexpected" >&2
    printf 'mutants: each is a missing test, or an entry in %s with the note that argues why no hermetic test can kill it\n' \
        "${accepted_file#"$root"/}" >&2
fi
if [[ -n "$stale" ]]; then
    failures=$((failures + $(wc -l <<<"$stale")))
    printf 'mutants: FAIL — %s recorded acceptance(s) no longer survive; the mutant became killable and the entry is now a lie:\n' \
        "$(wc -l <<<"$stale")" >&2
    report <<<"$stale" >&2
    # N68's whole lesson in one line, printed where it is needed rather than kept in a
    # document nobody opens mid-triage.
    printf 'mutants: before deleting a line, read note N60: this direction has fired wrongly before, and an "equivalent" acceptance cannot become killable without the program changing\n' >&2
fi

if ((failures > 0)); then
    printf 'mutants: FAIL — %s finding(s) over %s mutants, %s named skip(s)\n' \
        "$failures" "$generated" "$skips" >&2
    exit 1
fi

printf 'mutants: PASS — %s mutants, %s caught, %s accepted survivor(s), %s named skip(s)\n' \
    "$generated" "$caught" "$(wc -l <"$scratch/accepted")" "$skips"
