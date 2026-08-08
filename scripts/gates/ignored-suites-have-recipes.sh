#!/usr/bin/env bash
#
# An `#[ignore]`d test that no recipe runs is a test that will never run again.
#
# The hardware and virtual-driver rungs (design §3.1 R2/R3) are `#[ignore]`d by
# construction — shared CI has no camera. That makes them the easiest tests in the
# project to lose: nothing turns red when they stop being invoked. So the link between a
# suite and the recipe that runs it is made mechanical rather than remembered.
#
# The convention: each runner script declares the suites it owns with a marker line
#
#     # wch-suite: prefix=<test-name prefix> recipe=<just recipe>
#
# anchored at column zero — the indented copy above is prose, and the gate must not read
# its own documentation as a declaration. Every `#[ignore]`d test's name then begins with
# a declared prefix. The declarations are the one home for that mapping: the runner script
# that selects the suite is also the file that says it does, so the two cannot drift.
#
# Both halves are populations, not lists: the declarations come from `scripts/*.sh`, the
# ignored tests from the tree, the recipes from the justfile, and the packages from
# `cargo metadata`.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"
justfile="$root/justfile"

if [[ ! -f "$justfile" ]]; then
    gate_fail "no justfile; there are no recipes for a suite to be named by"
    gate_finish
fi

# ------------------------------------------------------------------ declarations

declare -a prefixes=()
declarations=0
while IFS= read -r line; do
    file="${line%%:*}"
    body="${line#*:}"
    prefix="$(sed -n 's/.*prefix=\([A-Za-z0-9_]*\).*/\1/p' <<<"$body")"
    recipe="$(sed -n 's/.*recipe=\([A-Za-z0-9_-]*\).*/\1/p' <<<"$body")"
    declarations=$((declarations + 1))

    if [[ -z "$prefix" || -z "$recipe" ]]; then
        gate_fail "$file has a malformed wch-suite declaration: $body"
        continue
    fi
    prefixes+=("$prefix")

    if ! grep -Eq "^${recipe}[[:space:]]*:" "$justfile"; then
        gate_fail "$file declares recipe '$recipe', which the justfile does not define"
        continue
    fi
    # The recipe must actually run the script that claims it, or the declaration is a
    # comment about something that is not happening.
    script_name="$(basename "$file")"
    if ! awk -v recipe="$recipe" '
        $0 ~ "^" recipe "[[:space:]]*:" { inside = 1; next }
        inside && /^[^[:space:]]/       { inside = 0 }
        inside                          { print }
    ' "$justfile" | grep -Fq "$script_name"; then
        gate_fail "the justfile recipe '$recipe' does not run $script_name, which declares it"
    fi
done < <(grep -rHn '^# wch-suite:' "$root/scripts" --include='*.sh' | sed 's/:[0-9]*:/:/')

gate_checked "$declarations" "wch-suite declarations in scripts/"
gate_require_nonzero "$declarations" "wch-suite declarations"

# ------------------------------------------------------------------ ignored tests

# Every workspace member's directory, longest-first, so a test file resolves to the most
# specific package that contains it (crates/backends/v4l2 before crates).
mapfile -t package_dirs < <(gate_metadata |
    jq -r '
        ( [ .workspace_members[] ] ) as $members
        | .packages[]
        | select(.id as $id | $members | index($id))
        | "\(.manifest_path)\t\(.name)"' |
    while IFS=$'\t' read -r manifest name; do
        printf '%s\t%s\n' "$(dirname "$manifest")" "$name"
    done | awk -F'\t' '{ print length($1) "\t" $0 }' | sort -rn | cut -f2-)

ignored=0
while IFS=$'\t' read -r file test_name; do
    ignored=$((ignored + 1))
    rel="${file#"$root"/}"

    package=""
    for entry in "${package_dirs[@]}"; do
        dir="${entry%%	*}"
        suffix="${dir#"$root"/}"
        if [[ "$rel" == "$suffix"/* ]]; then
            package="${entry#*	}"
            break
        fi
    done
    if [[ -z "$package" ]]; then
        gate_fail "$rel holds the ignored test '$test_name' but belongs to no workspace member; no recipe can select it"
        continue
    fi

    matched=0
    for prefix in "${prefixes[@]}"; do
        if [[ "$test_name" == "$prefix"* ]]; then
            matched=1
            break
        fi
    done
    if ((matched == 0)); then
        gate_fail "$package's ignored test '$test_name' ($rel) matches no declared suite prefix; add it to a suite or the recipe will never run it"
    fi
done < <(while IFS= read -r -d '' file; do
    awk -v path="$file" '
        /#\[[[:space:]]*ignore/ { pending = 1 }
        pending && match($0, /fn[[:space:]]+[A-Za-z0-9_]+/) {
            name = substr($0, RSTART, RLENGTH)
            sub(/fn[[:space:]]+/, "", name)
            printf "%s\t%s\n", path, name
            pending = 0
        }
    ' "$file"
done < <(gate_rust_files))

gate_checked "$ignored" "#[ignore]d tests matched against a declared suite prefix"
if ((ignored == 0)); then
    # Legitimate at P0: the first `#[ignore]`d suite is the R3 hardware rung, which lands
    # with the V4L2 backend at P1. The declaration half above is what carries this run.
    gate_skip 0 "#[ignore]d tests in the tree — the hardware and vivid suites land at P1; the ${declarations} suite declaration(s) above were still checked against the justfile"
fi

gate_finish
