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

root="$(git rev-parse --show-toplevel)"
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

if [[ ! -f "$config" ]]; then
    printf 'mutants: FAIL — no scope at %s; a mutation run with no scope has no meaning\n' \
        "$config" >&2
    exit 1
fi
if [[ ! -f "$accepted_file" ]]; then
    printf 'mutants: FAIL — no acceptance register at %s\n' "$accepted_file" >&2
    exit 1
fi

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
    printf 'mutants: FAIL — %s selects no files; a mutation run over nothing cannot go red\n' \
        "$config" >&2
    exit 1
fi

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
build_root="${WCH_MUTANTS_BUILD_ROOT:-${TMPDIR:-/tmp}}"
mkdir -p "$build_root"
export TMPDIR="$build_root"

per_job_gib=3
reserve_gib="$per_job_gib"
avail_gib="$(df -BG --output=avail "$build_root" | tail -1 | tr -dc '0-9')"
fits=$(((avail_gib - reserve_gib) / per_job_gib))
if ((fits < 1)); then
    printf 'mutants: FAIL — %s has %s GiB free and one build tree needs about %s with %s held back; set WCH_MUTANTS_BUILD_ROOT to a larger filesystem\n' \
        "$build_root" "$avail_gib" "$per_job_gib" "$reserve_gib" >&2
    exit 1
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
elapsed=$(( $(date +%s) - started ))
printf 'mutants: cargo-mutants exited %s after %sm%ss\n' \
    "$status" "$((elapsed / 60))" "$((elapsed % 60))"

case "$status" in
0 | 2 | 3) ;; # every mutant caught / some survived / some timed out — all triageable
4)
    printf 'mutants: FAIL — the baseline suite is red in an unmutated tree; no survivor list from this run means anything\n' >&2
    exit 1
    ;;
*)
    printf 'mutants: FAIL — cargo-mutants could not complete (exit %s); see %s\n' \
        "$status" "${out_dir#"$root"/}" >&2
    exit 1
    ;;
esac

if [[ ! -f "$out_dir/outcomes.json" ]]; then
    printf 'mutants: FAIL — no outcomes at %s\n' "$out_dir/outcomes.json" >&2
    exit 1
fi

read -r generated caught missed timed_out unviable < <(
    jq -r '[.total_mutants, .caught, .missed, .timeout, .unviable] | @tsv' \
        "$out_dir/outcomes.json"
)

# A run that generated nothing is a green that proves nothing.
if ((generated == 0)); then
    printf 'mutants: FAIL — zero mutants generated over %s file(s); a floor with no mutants cannot go red\n' \
        "${#scope[@]}" >&2
    exit 1
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

scratch="$(mktemp -d "${TMPDIR:-/tmp}/wch-mutants.XXXXXXXX")"
trap 'rm -rf "$scratch"' EXIT

# A timed-out mutant is an uncaught mutant with a worse failure mode, so it is triaged
# beside the missed ones rather than counted as a pass.
#
# `LC_ALL=C` on both sorts and on the comparison: `sort` collates by locale and `comm`
# compares bytes, and the two disagree on any line with punctuation in it — which is every
# mutant name. Observed as `comm: file 1 is not in sorted order` on the first run that
# produced survivors.
{ cat "$out_dir/missed.txt" "$out_dir/timeout.txt" 2>/dev/null || true; } |
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
fi

if ((failures > 0)); then
    printf 'mutants: FAIL — %s finding(s) over %s mutants, %s named skip(s)\n' \
        "$failures" "$generated" "$skips" >&2
    exit 1
fi

printf 'mutants: PASS — %s mutants, %s caught, %s accepted survivor(s), %s named skip(s)\n' \
    "$generated" "$caught" "$(wc -l <"$scratch/accepted")" "$skips"
