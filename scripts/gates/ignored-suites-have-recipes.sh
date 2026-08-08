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

# ------------------------------------------------- the exclusive-device test group
#
# A camera is a physical thing and V4L2 allows one streamer per node, so two hardware
# tests running at once contend for it and the loser sees a correct `EBUSY` from a correct
# backend. `.config/nextest.toml` serialises them; this half asserts the serialisation
# still covers every suite that exists, because the failure mode is a *flake* — the kind
# of red that gets re-run rather than read.
#
# Every declared prefix must be in the group's filter. That is the right population here
# rather than an over-fit: a suite in this project is `#[ignore]`d precisely because it
# needs hardware or a loaded kernel module, and both of those are exclusive.

nextest_config="$root/.config/nextest.toml"
if [[ ! -f "$nextest_config" ]]; then
    gate_fail "no .config/nextest.toml; nothing stops two hardware suites streaming from one camera at once"
else
    group="$(sed -n 's/^[[:space:]]*test-group[[:space:]]*=[[:space:]]*.\([A-Za-z0-9_-]*\).*/\1/p' \
        "$nextest_config" | head -n1)"
    if [[ -z "$group" ]]; then
        gate_fail ".config/nextest.toml assigns no test-group; the hardware suites are not serialised"
    elif ! grep -Eq "^\[test-groups\.${group}\]" "$nextest_config"; then
        gate_fail ".config/nextest.toml assigns test-group '$group', which it does not define"
    elif ! awk -v group="$group" '
        $0 ~ "^\\[test-groups\\." group "\\]" { inside = 1; next }
        inside && /^\[/                       { inside = 0 }
        inside && /max-threads[[:space:]]*=[[:space:]]*1[[:space:]]*$/ { found = 1 }
        END { exit !found }
    ' "$nextest_config"; then
        gate_fail "test group '$group' does not cap itself at one thread, so it serialises nothing"
    fi

    # The *filter expressions* of the overrides that assign the group — not the file. The
    # first version of this check grepped the whole file and passed on a config whose
    # filter had lost a prefix, because the paragraph above the filter still mentioned it.
    # The selftest caught that, which is what the selftest is for.
    filters="$(awk -v group="$group" '
        /^\[\[/                                   { block = ""; assigned = 0; next }
        /^\[/                                     { block = ""; assigned = 0; next }
        /^[[:space:]]*filter[[:space:]]*=/        { block = $0 }
        $0 ~ "test-group[[:space:]]*=.*" group    { assigned = 1 }
        assigned && block != ""                   { print block; block = ""; assigned = 0 }
    ' "$nextest_config")"
    if [[ -z "$filters" ]]; then
        gate_fail "no override in .config/nextest.toml assigns '$group' to a filter, so the group holds nothing"
    fi

    covered=0
    for prefix in "${prefixes[@]}"; do
        if grep -Fq "$prefix" <<<"$filters"; then
            covered=$((covered + 1))
        else
            gate_fail "suite prefix '$prefix' is not in .config/nextest.toml's '$group' filter; two of its tests can stream from one camera at once"
        fi
    done
    gate_checked "$covered" "suite prefix(es) covered by the exclusive-device test group"
fi

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
