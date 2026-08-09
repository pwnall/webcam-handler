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

# --------------------------------------------------------------- reporting state

GATE_NAME="${GATE_NAME:-$(basename "${0}" .sh)}"
GATE_FAILURES=0
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
    if ((GATE_FAILURES > 0)); then
        printf 'FAIL %s — %s violation(s) over %s examined items\n' \
            "$GATE_NAME" "$GATE_FAILURES" "$GATE_TOTAL" >&2
        exit 1
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
gate_metadata_snapshot() {
    local out
    out="$(mktemp "${WCH_GATE_SCRATCH:-${TMPDIR:-/tmp}}/wch-metadata.XXXXXXXX")"
    (
        unset WCH_GATE_METADATA
        gate_metadata >"$out"
    )
    printf '%s\n' "$out"
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
