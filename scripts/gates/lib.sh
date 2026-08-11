# Shared helpers for the webcam-handler gate predicates.
#
# docs/9's structural rule is that a gate written to close a defect is not itself tested
# against its own inverse, and the second arm is where it fails. `selftest.sh` proves the
# second arm by pointing a predicate at a *mutated copy* of the tree, so no predicate may
# assume it is looking at the checkout it lives in: every path resolves under
# `gate_root`, which the selftest overrides with $WCH_GATE_ROOT.
#
# The counters serve the other half of the same rule (AGENTS.md rule 3): a check that
# examined zero things is a check that cannot fail. Every predicate reports what it
# examined and how many, and calls `gate_require_nonzero` wherever a zero would be
# vacuous rather than informative.
#
# shellcheck shell=bash

# The tree under test. Absolute, no trailing slash.
gate_root() {
    if [[ -n "${WCH_GATE_ROOT:-}" ]]; then
        local root="${WCH_GATE_ROOT%/}"
        if [[ ! -d "$root" ]]; then
            printf 'gate: WCH_GATE_ROOT=%s is not a directory\n' "$root" >&2
            return 1
        fi
        printf '%s\n' "$root"
    else
        git rev-parse --show-toplevel
    fi
}

# --------------------------------------------------------------- the third outcome
#
# A check has three answers, not two: it can pass, it can find something, and it can be
# **unable to answer at all**. The third one had no spelling in this suite until note N68,
# and the cost of that is three entries long: N52 (the mutation floor's verdict moved with
# `nproc`), N66 (it moved with the free space on `/tmp`) and N68 (it moved when an agent
# edited a file the run was still reading). All three reported the same word — FAIL — as a
# surviving mutant gets, and N60 records what that costs: "a gate that cries wolf does not
# get believed, it gets re-run at `-j1` until it agrees, and the run after that is the one
# where a real survivor is waved through".
#
# So a run that could not produce a verdict exits **75**, and that number is not invented:
# it is `EX_TEMPFAIL` from `sysexits.h` — "temporary failure; the user is invited to retry"
# — which is exactly the claim being made. The alternatives were considered and rejected:
#
#   - **0** is wrong because the criterion is *not proven*. An unprovable criterion that
#     reads as green is the "skip that reads as pass" AGENTS.md rule 3 forbids, wearing an
#     exit code.
#   - **1** is what a finding uses, and telling the two apart is the whole point.
#   - **2** is already taken by `phase.sh`'s usage error, and by cargo-mutants' "some
#     mutants survived" — a number that means two things in one pipeline means neither.
#
# It is non-zero, so nothing that composes these scripts can accidentally treat it as
# success; it is *distinct*, so anything that wants to can tell the machine's shortfall
# from the code's. `phase.sh` does; `scripts/mutants.sh` produces it.
GATE_NO_VERDICT=75

# --------------------------------------------------------------- reporting state

GATE_NAME="${GATE_NAME:-$(basename "${0}" .sh)}"
GATE_FAILURES=0
GATE_NO_VERDICTS=0
GATE_TOTAL=0
declare -a GATE_CHECKS=()
declare -a GATE_SKIPS=()

# Record that `count` things of a kind were examined. Printed by `gate_finish`, so a
# reader of CI output can see the population each claim rests on.
gate_checked() {
    local count="$1"
    shift
    GATE_CHECKS+=("$count $*")
    GATE_TOTAL=$((GATE_TOTAL + count))
}

# A skip that is named and counted, never silence (AGENTS.md rule 3).
gate_skip() {
    local count="$1"
    shift
    GATE_SKIPS+=("$count $*")
    printf '  SKIP  %s: %s (%s)\n' "$GATE_NAME" "$*" "$count"
}

# An informational line. Reasoning a reader needs; not a verdict.
gate_note() {
    printf '  note  %s\n' "$*"
}

gate_fail() {
    GATE_FAILURES=$((GATE_FAILURES + 1))
    printf '  FAIL  %s: %s\n' "$GATE_NAME" "$*" >&2
}

# Something this run could not answer — a resource it did not have, an interruption, an
# input that moved underneath it. Not a finding, and never a pass: see `GATE_NO_VERDICT`
# above. The word is spelled out rather than abbreviated because the reader this exists
# for is skimming a CI log for the string "FAIL".
gate_no_verdict() {
    GATE_NO_VERDICTS=$((GATE_NO_VERDICTS + 1))
    printf '  NO VERDICT  %s: %s\n' "$GATE_NAME" "$*" >&2
}

# A population of zero means the predicate examined nothing and therefore proved
# nothing. Callers use this wherever an empty population is a defect rather than a
# legitimate not-yet-landed state; where it is legitimate, they call `gate_skip`.
gate_require_nonzero() {
    local count="$1"
    shift
    if ((count == 0)); then
        gate_fail "examined zero $* — a check that examines nothing cannot go red"
    fi
}

gate_finish() {
    local line
    if ((${#GATE_CHECKS[@]} > 0)); then
        for line in "${GATE_CHECKS[@]}"; do
            printf '  ok    %s: checked %s\n' "$GATE_NAME" "$line"
        done
    fi
    # A finding outranks a no-verdict, and the ordering is deliberate: if this run both
    # found something and failed to answer something else, the thing it *found* is the
    # thing to act on, and "could not answer" would be the softer of the two words to end
    # on. The unanswered part is still named on the line above, so it is not lost.
    if ((GATE_FAILURES > 0)); then
        if ((GATE_NO_VERDICTS > 0)); then
            printf 'NO VERDICT %s — and %s check(s) could not produce a verdict at all\n' \
                "$GATE_NAME" "$GATE_NO_VERDICTS" >&2
        fi
        printf 'FAIL %s — %s violation(s) over %s examined items\n' \
            "$GATE_NAME" "$GATE_FAILURES" "$GATE_TOTAL" >&2
        exit 1
    fi
    if ((GATE_NO_VERDICTS > 0)); then
        printf 'NO VERDICT %s — %s check(s) could not answer over %s examined items; nothing here is a finding about the code, and nothing here is a pass either\n' \
            "$GATE_NAME" "$GATE_NO_VERDICTS" "$GATE_TOTAL" >&2
        exit "$GATE_NO_VERDICT"
    fi
    printf 'PASS %s — %s items examined, %s named skip(s)\n' \
        "$GATE_NAME" "$GATE_TOTAL" "${#GATE_SKIPS[@]}"
    exit 0
}

# --------------------------------------------------------------- tree traversal
#
# Populations are derived by walking the tree, never transcribed (docs/9's second
# structural rule). `target/`, `.git/` and `vendor/` are excluded everywhere: the first
# two are not source, and `vendor/v4l2-webcam-skill/` is a read-only upstream reference
# this project does not own.

# Print, NUL-separated, every file under $1 (default: the whole tree) matching the
# remaining `find` predicates.
gate_find() {
    local dir="$1"
    shift
    [[ -d "$dir" ]] || return 0
    find "$dir" \
        \( -name target -o -name .git -o -name vendor \) -prune -o \
        -type f "$@" -print0
}

# Every Rust source file in the tree under test.
gate_rust_files() {
    gate_find "$(gate_root)" -name '*.rs'
}

# --------------------------------------------------------------- cargo metadata
#
# `cargo metadata` is the authority on the dependency graph: manifests lie by omission
# (a feature enabled by a transitive dependency is invisible in the manifest that
# forbids it). $WCH_GATE_METADATA lets the selftest feed a doctored graph to the failing
# arm — the passing arm always resolves the real tree, because a predicate whose only
# input is injectable proves nothing about the repository.
gate_metadata() {
    if [[ -n "${WCH_GATE_METADATA:-}" ]]; then
        cat "$WCH_GATE_METADATA"
    else
        cargo metadata --locked --offline --format-version 1 \
            --manifest-path "$(gate_root)/Cargo.toml"
    fi
}

# --------------------------------------------------------------- the population
#
# The predicate population is the directory listing minus the harness. Neither
# `run-all.sh` nor `selftest.sh` keeps a list, so a predicate added to the directory is
# run and self-tested without anybody remembering to register it — which is the failure
# docs/9's derived-population rule exists to prevent.
#
# The harness names *are* a list, and that is the bootstrap limit docs/9 records: the
# selftest cannot test the harness. It is closed as far as it can be — each name is
# asserted to exist, so a rename fails loudly instead of quietly promoting a harness file
# into the population it drives.
GATE_HARNESS_FILES=(lib.sh run-all.sh selftest.sh phase.sh)

gate_predicates() {
    local dir file base harness is_harness
    dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    for harness in "${GATE_HARNESS_FILES[@]}"; do
        if [[ ! -f "$dir/$harness" ]]; then
            printf 'gate: harness file %s is missing from %s\n' "$harness" "$dir" >&2
            return 1
        fi
    done
    for file in "$dir"/*.sh; do
        base="$(basename "$file")"
        is_harness=0
        for harness in "${GATE_HARNESS_FILES[@]}"; do
            if [[ "$base" == "$harness" ]]; then
                is_harness=1
            fi
        done
        if ((is_harness == 0)); then
            printf '%s\n' "$file"
        fi
    done
}

# Write the real workspace's `cargo metadata` to a scratch file and echo the path. The
# failing arms doctor a copy of the real graph rather than inventing one, so a seeded
# violation differs from the shipped tree in exactly the one way the case describes.
#
# **It never echoes an empty path, and that is not defensiveness — it is a measured
# defect.** Seven case files spell their seeded copy `"$md.seeded"`, so a `$md` that came
# back empty makes that `.seeded`, resolved against the *checkout* — which is precisely
# what the paragraph below `gate_scratch_tree` forbids ("predicates have no side effects
# on the tree, and neither may the cases that exercise them"). It happened: a full scratch
# filesystem made `mktemp` fail, and a zero-byte `.seeded` appeared at the repository root.
# `run_case` deliberately runs without `set -e`, so nothing downstream was going to stop
# it, and inside a `fail_case_` a seeding failure is indistinguishable from the predicate
# going red — which is the one confusion this harness must not have. So the fallback is a
# path that is still *outside* the tree: a caller that ignores this failure writes
# somewhere harmless, and `selftest.sh` catches the class rather than this instance.
gate_metadata_snapshot() {
    local out scratch
    scratch="${WCH_GATE_SCRATCH:-${TMPDIR:-/tmp}}"
    if ! out="$(mktemp "$scratch/wch-metadata.XXXXXXXX")" || [[ -z "$out" ]]; then
        printf 'gate: no scratch file for a metadata snapshot in %s (full filesystem?)\n' \
            "$scratch" >&2
        out="$scratch/wch-metadata-unavailable.$$"
    fi
    (
        unset WCH_GATE_METADATA
        gate_metadata >"$out"
    )
    printf '%s\n' "$out"
}

# --------------------------------------------------------------- the tree, watched
#
# Two jobs here read the working tree for a long time and must not report a conclusion
# about a tree that changed while they were reading it:
#
#   - `selftest.sh` runs every case arm and then checks that no arm touched the checkout
#     (its `tree_before` block, and the paragraph there says why it is *recorded and
#     compared* rather than asserted clean — a developer's dirty tree is not a finding);
#   - `scripts/mutants.sh` reads the tree for the better part of an hour, one mutant at a
#     time, and note **N68** records the run whose input moved underneath it: a `just
#     gate-g4` started at 08:14 was still running at 09:19 when three files inside its
#     scope were edited, and it reported three acceptances as lies.
#
# One law, one home. The alternative was the same eight lines in both scripts, which is
# what AGENTS.md's "a second copy or a bypass is a defect" is about — and the copies would
# have drifted immediately, because `mutants.sh` needs the commit as well as the dirty
# state and `selftest.sh` did not have it.
#
# `scripts/mutants.sh` sources this file for exactly these three functions and
# `GATE_NO_VERDICT`. It is not a gate predicate and it keeps its own `mutants:` reporting
# vocabulary; sourcing a library is not adopting its output format.

# Is $1 a tree whose state git can report? A source tarball, or a checkout whose git is
# broken or absent, is not — and that is a missing tool rather than a violation, so every
# caller answers it with a named, counted skip.
gate_tree_watchable() {
    git -C "$1" rev-parse --git-dir >/dev/null 2>&1
}

# The state of the tree at $1, as one string: the commit, then `git status --porcelain`.
#
# Both halves are load-bearing and neither implies the other. `--porcelain` covers
# untracked files, which is the shape `selftest.sh`'s defect took (a stray `.seeded` at the
# repository root) and the shape an agent's new file takes. The commit covers the case
# porcelain cannot see at all: a `git checkout` or a rebase that moves HEAD and leaves the
# tree clean at both ends, which is the same "the input moved" event wearing a different
# hat.
gate_tree_state() {
    printf 'HEAD %s\n' "$(git -C "$1" rev-parse HEAD 2>/dev/null || printf '<none>')"
    git -C "$1" status --porcelain 2>/dev/null
}

# The lines by which two `gate_tree_state` recordings differ, `<` for the older and `>` for
# the newer. Callers print this verbatim: a reader's first question is always *which
# files*, and the answer is right there in the porcelain lines.
gate_tree_changes() {
    diff <(printf '%s\n' "$1") <(printf '%s\n' "$2") | grep -E '^[<>]' || true
}

# --------------------------------------------------------------- scratch copies

# Copy the tree under test somewhere writable and echo the path. Used by the selftest's
# failing arms to seed a violation without touching the checkout — predicates have no
# side effects on the tree, and neither may the cases that exercise them.
gate_scratch_tree() {
    local src dest
    src="$(gate_root)"
    dest="$(mktemp -d "${WCH_GATE_SCRATCH:-${TMPDIR:-/tmp}}/wch-tree.XXXXXXXX")"
    tar -C "$src" -cf - --exclude=.git --exclude=target . | tar -C "$dest" -xf -
    printf '%s\n' "$dest"
}
