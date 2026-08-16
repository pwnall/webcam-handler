#!/usr/bin/env bash
#
# Prove every gate predicate can go red — the rule the whole suite is built around
# (docs/9): *a gate written to close a defect is not itself tested against its own
# inverse, and the second arm is where it fails.*
#
# The table is the directory listing (`gate_predicates`), never a hand-maintained list.
# Each predicate must have a companion `cases/<name>.cases.sh` defining
#
#     pass_case()          run the predicate against the pristine tree; must exit 0
#     pass_case_<slug>()   a second green arm — a shape the predicate must *allow*
#     fail_case_<slug>()   seed one violation in a COPY of the tree (or point the
#                          predicate's documented seam at a doctored input) and run it;
#                          must exit non-zero
#
# An arm that seeds with `sed` seeds through `lib.sh`'s `gate_seed`, which fails loudly when
# the substitution matched nothing — and reports it *beside* the exit status rather than in it,
# because a `pass_case_` whose seed died exits 0 and would otherwise be printed as `ok` over an
# arm that never produced its subject (note **N186**). This file reads that report after every
# arm and lets it replace the ordinary verdict: the fact is about the seed, so the sentence has
# to be too.
#
# A predicate with no case file fails. A predicate with no green arm fails. A predicate
# with zero `fail_case_*` functions fails — a gate with only a passing arm is the defect
# class, not a gate.
#
# Cases run in a subshell with `lib.sh` sourced (so `gate_scratch_tree` is available) and
# $GATE set to the predicate under test. They never mutate the checkout: `gate_scratch_tree`
# copies it first, and every scratch copy lands under one directory this script removes.
set -uo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

gates_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
lib="$gates_dir/lib.sh"
cases_dir="$gates_dir/cases"

# Before this run makes any of its own: reclaim what an earlier run was killed before it
# could.
#
# The `trap` below and the per-arm `reclaim_scratch` further down are the normal path, and
# neither of them runs after `kill -9`, a full disk or a laptop that closed — which is
# precisely how the 76 abandoned copies E15 measured came to exist. A cleanup that only
# happens when the run finishes is a cleanup that is absent exactly when it is needed, so
# the *next* run does it: this call, and `just scratch-sweep` for a person who wants the
# room back now. A day is the presumed-live threshold, which is comfortably longer than
# anything here and shorter than an accumulation nobody notices; `gate_scratch_sweep` says
# what it took.
gate_scratch_sweep 1440

scratch="$(mktemp -d "$(gate_scratch_root)/wch-selftest.XXXXXXXX")"
export WCH_GATE_SCRATCH="$scratch"
trap 'rm -rf "$scratch"' EXIT

# Reclaim one arm's scratch before the next arm starts.
#
# Every seeded violation is a `gate_scratch_tree` copy of the whole checkout — 26 MiB on the
# tree this landed on — and until this function existed they all lived until the `EXIT` trap
# above. Measured on this workstation with the population at 22 predicates and 195 arms: a
# **12.3 GiB** high-water mark, against a `/tmp` that is a tmpfs with a user quota a little
# over 12. It went over during the G4 review, and what it said when it did was
#
#     tar: ./README.md: Cannot write: Disk quota exceeded
#     PROBLEM systemd-units pass_case_… exited 1; the predicate is red on a shape it must allow
#
# — a filesystem's ceiling reported as a predicate being wrong about the tree, in the arm
# that happened to be running when the room ran out. That is notes **N52**, **N66** and
# **N68** for the fourth time: a verdict that moves with the machine rather than with the
# code, wearing the same word a real finding gets. N60 records the bill ("a gate that cries
# wolf does not get believed, it gets re-run at `-j1` until it agrees").
#
# The alternative was a budget check and a `no_verdict`, which is what `scripts/mutants.sh`
# does — right there, wrong here. The floor genuinely needs gigabytes to hold parallel build
# trees; this harness needs **one** seeded tree at a time and was keeping 195 of them for no
# reason but the order the `rm` happened to be written in. A resource a suite does not use
# beats a resource it asks permission for.
#
# What this fixes is the *accumulation*, and that is the whole claim: the seeded copies no
# longer add up, so the term that grew with every predicate this suite gains is gone. It is
# not a promise about the total. Sampled every two seconds through a full run, the scratch
# filesystem's high-water mark went from over 12.1 GiB (where it hit the quota and the run
# died) to **9.5 GiB**, and the scratch directory now holds exactly one entry at a time —
# which is how the remaining 9.5 was identified rather than guessed at. Since the 2026-08-12
# ruling that room is under `target/` on a disk with 156 GB free rather than in a 16 GB
# tmpfs, so the same number is no longer near anything's ceiling; it is still the number, and
# it is still **one arm**:
# `counted-selections.sh`'s real-lister arm points `$WCH_GATE_ROOT` at a scratch copy and
# `counted-selections.sh:40` runs `cargo nextest list --workspace` with that copy as its
# working directory and no `CARGO_TARGET_DIR`, so cargo compiles the entire workspace into
# `<copy>/target` — 9.7 GiB and a cold build, thrown away seconds later. Named and not
# touched here: pointing it at the checkout's `target/` the way the schema-artifacts cases
# do is a change to somebody else's arm and to the rubric-rule-6 claim that arm carries.
#
# Nothing carries across arms and this cannot break one: each case seeds its own copy inside
# its own subshell, and the build directories that *are* shared between arms live at
# `target/gate-selftest/` (`_shared_target_dir` in the schema-artifacts cases, and the
# `cli-parity-fork` directory beside it) precisely so that a scratch wipe does not cost a
# rebuild. Since the 2026-08-12 ruling this directory is under `target/` as well, so the
# distinction is no longer which filesystem they are on — it is that those two are named,
# reused and outside `$scratch`, and this one is `mktemp`'d per run and wiped per arm.
# `-mindepth 1` rather than a `"$scratch"/*` glob, because the glob misses dotfiles and a
# stub left behind as `.something` would survive every sweep.
reclaim_scratch() {
    find "$scratch" -mindepth 1 -maxdepth 1 -exec rm -rf {} +
}

# What the working tree looked like before any arm ran.
#
# The header above states the law — cases never mutate the checkout — and until now that
# was a convention every case file had to keep on its own. It got broken: a full scratch
# filesystem made `gate_metadata_snapshot`'s `mktemp` fail, `feature-posture`'s arms
# expanded `"$md.seeded"` with an empty `$md`, and a zero-byte `.seeded` was written to
# the repository root. Nothing noticed, because a `fail_case_` that fails for the wrong
# reason exits non-zero and non-zero is what a failing arm is *supposed* to do.
#
# So the law gets a checker. Recorded before and compared after, rather than asserted
# clean: running the selftest on a dirty tree is the ordinary case for whoever is writing
# a gate, and "you had uncommitted work" is not a finding. `--porcelain` covers untracked
# files too, which is the shape the defect actually took. A tree without `git` — a source
# tarball — skips it, named and counted, because a missing tool is not a violation.
#
# The recording and the comparison now live in `lib.sh` (`gate_tree_state`), because
# `scripts/mutants.sh` needs the identical thing for note N68's defect — a run whose input
# moved while it was reading it — and two copies of a law is the defect AGENTS.md names.
# The shared version records the commit as well as the dirty state, which this check gains
# for free: an arm that committed something would have been invisible to porcelain alone.
tree_before=""
tree_watched=0
if gate_tree_watchable "$(gate_root)"; then
    tree_before="$(gate_tree_state "$(gate_root)")"
    tree_watched=1
fi

# Enumerate the case functions a case file defines. Derived from the file, so adding a
# violation shape is adding a function.
list_cases() {
    local casefile="$1" gate="$2"
    bash -c '
        set -uo pipefail
        GATE="$3"; export GATE
        # shellcheck disable=SC1090
        source "$1"
        # shellcheck disable=SC1090
        source "$2"
        declare -F | awk "{ print \$3 }" | grep -E "^(pass_case|fail_case_)" | sort
    ' _ "$lib" "$casefile" "$gate"
}

# Run one case. Deliberately without `set -e`: the arm's verdict is the exit status of
# the predicate the case runs, and an `-e` abort in a seeding step would be indis-
# tinguishable from the predicate going red — the one confusion this harness must not
# have.
run_case() {
    local casefile="$1" gate="$2" fn="$3"
    bash -c '
        set -uo pipefail
        GATE="$3"; export GATE
        # shellcheck disable=SC1090
        source "$1"
        # shellcheck disable=SC1090
        source "$2"
        "$4"
    ' _ "$lib" "$casefile" "$gate" "$fn" 2>&1
}

predicates=0
pass_arms=0
fail_arms=0
problems=0

report_problem() {
    problems=$((problems + 1))
    printf '  PROBLEM %s\n' "$*" >&2
}

# The seeding helper, proved both ways before any arm leans on it.
#
# The harness cannot be self-tested by the population it drives — that is docs/9's bootstrap
# limit and this file's header records it — so it is closed here instead, in the one direction
# that matters: a `gate_seed` that had stopped noticing a dead seed would return every
# `pass_case_` in `cases/` to reporting `ok` over an unseeded tree, silently, which is the defect
# note **N186** is about. `gate_seed_selfcheck` lives in `lib.sh` beside the thing it proves, so
# a person can run it on its own.
if gate_seed_selfcheck; then
    printf 'selftest: gate_seed is live on a seed that applies and loud on one that does not\n'
else
    report_problem "gate_seed does not report a dead seed; every seeded arm below could be running the predicate against an unmodified tree"
fi

# Resolved once, and after `$WCH_GATE_SCRATCH` is exported, so this names the same file the
# arms' `gate_seed` calls write to.
seed_report="$(gate_seed_report)"
rm -f "$seed_report"

while IFS= read -r gate; do
    predicates=$((predicates + 1))
    name="$(basename "$gate" .sh)"
    casefile="$cases_dir/$name.cases.sh"
    printf -- '--- %s\n' "$name"

    if [[ ! -f "$casefile" ]]; then
        report_problem "$name has no case file at cases/$name.cases.sh; a predicate nobody proved can go red is a predicate nobody has tested"
        continue
    fi

    mapfile -t cases < <(list_cases "$casefile" "$gate")
    have_pass=0
    these_fail_arms=0
    for fn in "${cases[@]}"; do
        case "$fn" in
        pass_case*) have_pass=$((have_pass + 1)) ;;
        fail_case_*) these_fail_arms=$((these_fail_arms + 1)) ;;
        esac
    done

    if ((have_pass == 0)); then
        report_problem "$name has no pass_case; nothing proves the predicate is green on the shipped tree"
    fi
    if ((these_fail_arms == 0)); then
        report_problem "$name has zero fail_case_* functions; a predicate with only a passing arm cannot be shown to go red"
    fi

    for fn in "${cases[@]}"; do
        output="$(run_case "$casefile" "$gate" "$fn")"
        status=$?
        # Read **before** the reclaim, which is what removes it: `gate_seed` writes into the
        # run's scratch directory and `reclaim_scratch` empties that between arms, so what is
        # there now belongs to the arm that just ran and to no other.
        dead_seed=""
        if [[ -s "$seed_report" ]]; then
            dead_seed="$(cat "$seed_report")"
        fi
        # This arm's verdict is in `$status` and `$output`; its scratch trees are dead
        # weight from here on. See `reclaim_scratch`.
        reclaim_scratch
        case "$fn" in
        pass_case*) pass_arms=$((pass_arms + 1)) ;;
        fail_case_*) fail_arms=$((fail_arms + 1)) ;;
        esac

        # A dead seed **replaces** the verdict rather than joining it. Both of the sentences
        # below would be about the predicate, and in a `pass_case_` the sentence would be
        # `ok` — an arm that never produced its subject, reported as proof (note **N186**).
        if [[ -n "$dead_seed" ]]; then
            report_problem "$name $fn seeds a spelling this tree no longer has, so the predicate ran against a tree the arm never changed; repair the arm's seed, not the predicate"
            printf '%s\n' "$dead_seed" | sed 's/^/        /'
            printf '%s\n' "$output" | sed 's/^/        /'
            continue
        fi

        case "$fn" in
        pass_case*)
            if ((status == 0)); then
                printf '  ok    %s\n' "$fn"
            else
                report_problem "$name $fn exited $status; the predicate is red on a shape it must allow"
                printf '%s\n' "$output" | sed 's/^/        /'
            fi
            ;;
        fail_case_*)
            if ((status != 0)); then
                printf '  ok    %s (predicate exited %s)\n' "$fn" "$status"
            else
                report_problem "$name $fn exited 0; the seeded violation did not turn the predicate red"
                printf '%s\n' "$output" | sed 's/^/        /'
            fi
            ;;
        esac
    done
    printf '\n'
done < <(gate_predicates)

printf 'selftest: %s predicates, %s pass arm(s), %s fail arm(s)\n' \
    "$predicates" "$pass_arms" "$fail_arms"

# The law this harness's header states, checked rather than trusted. See `tree_before`.
if ((tree_watched == 1)); then
    tree_after="$(gate_tree_state "$(gate_root)")"
    if [[ "$tree_before" != "$tree_after" ]]; then
        report_problem "an arm changed the checkout; cases seed violations in scratch copies, never in the tree"
        gate_tree_changes "$tree_before" "$tree_after" | sed 's/^/        /'
    else
        printf 'selftest: the checkout is as the arms found it\n'
    fi
else
    printf 'selftest: SKIP (1): no git checkout, so "the arms left the tree alone" was not checked\n'
fi

if ((predicates == 0)); then
    printf 'selftest: FAIL — no predicates found\n' >&2
    exit 1
fi
if ((problems > 0)); then
    printf 'selftest: FAIL — %s problem(s)\n' "$problems" >&2
    exit 1
fi
printf 'selftest: PASS — every predicate is green on the tree and red on each of its inverses\n'
