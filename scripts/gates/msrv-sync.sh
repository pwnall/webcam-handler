#!/usr/bin/env bash
#
# The MSRV is one fact (rubric B9): `rust-version` under `[workspace.package]` in the root
# `Cargo.toml`. Everything else that states a minimum Rust version is a copy, and a copy
# that disagrees is how a project ends up supporting a version it never builds on.
#
# The copies are found, not listed: every workspace member's resolved `rust_version` comes
# from `cargo metadata`, every CI workflow is whatever is under `.github/workflows/`, and
# the README is checked for any `Rust <x.y>` claim at all — including one that is merely
# out of date rather than absent.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

msrv="$(awk '
    /^\[workspace\.package\]/ { section = 1; next }
    /^\[/                     { section = 0 }
    section && /^rust-version[[:space:]]*=/ {
        # The quoted value, not a greedy strip: `gsub(/.*"/, "")` eats the whole line.
        if (match($0, /"[^"]*"/)) {
            print substr($0, RSTART + 1, RLENGTH - 2)
        }
        exit
    }
' "$root/Cargo.toml")"

if [[ -z "$msrv" ]]; then
    gate_fail "the root Cargo.toml declares no [workspace.package] rust-version; the one fact has no home"
    gate_finish
fi
gate_note "the MSRV fact is rust-version = \"$msrv\" in the root Cargo.toml"

# ------------------------------------------------------------------ the members

members=0
while IFS=$'\t' read -r name declared; do
    members=$((members + 1))
    if [[ "$declared" == "null" || -z "$declared" ]]; then
        gate_fail "$name declares no rust-version; it must inherit the workspace one (rust-version.workspace = true)"
    elif [[ "$declared" != "$msrv" ]]; then
        gate_fail "$name resolves to rust-version $declared, not $msrv"
    fi
done < <(gate_metadata | jq -r '
    ( [ .workspace_members[] ] ) as $members
    | .packages[]
    | select(.id as $id | $members | index($id))
    | "\(.name)\t\(.rust_version // "null")"
')

gate_checked "$members" "workspace members whose resolved rust-version was compared with the fact"
gate_require_nonzero "$members" "workspace members"

# ------------------------------------------------------------------ CI

# A floating channel is a deliberate choice (a nightly Miri job, for instance); a pinned
# version that is not the MSRV is a second, disagreeing fact.
workflows=0
toolchain_pins=0
msrv_pins=0
while IFS= read -r -d '' workflow; do
    workflows=$((workflows + 1))
    rel="${workflow#"$root"/}"
    while IFS= read -r value; do
        toolchain_pins=$((toolchain_pins + 1))
        case "$value" in
        stable | beta | nightly | nightly-*) ;;
        "$msrv") msrv_pins=$((msrv_pins + 1)) ;;
        *) gate_fail "$rel pins toolchain '$value', which is neither a floating channel nor the MSRV $msrv" ;;
        esac
    done < <(grep -hoE '^[[:space:]]*toolchain:[[:space:]]*[^[:space:]#]+' "$workflow" |
        sed 's/.*toolchain:[[:space:]]*//' | tr -d '"'"'")
done < <(gate_find "$root/.github/workflows" \( -name '*.yml' -o -name '*.yaml' \))

if ((workflows == 0)); then
    gate_fail "no workflow files under .github/workflows/; CI cannot agree with the MSRV it never states"
elif ((msrv_pins == 0)); then
    gate_fail "no workflow pins the MSRV $msrv; CI proves nothing about the version the manifest claims"
fi
gate_checked "$workflows" "CI workflow files"
gate_checked "$toolchain_pins" "toolchain pins in CI"

# ------------------------------------------------------------------ the README

readme="$root/README.md"
if [[ ! -f "$readme" ]]; then
    gate_fail "no README.md; the MSRV a reader is told about cannot be checked"
    gate_finish
fi

claims=0
while IFS= read -r claim; do
    claims=$((claims + 1))
    if [[ "$claim" != "$msrv" ]]; then
        gate_fail "README.md tells readers Rust $claim; the MSRV is $msrv"
    fi
done < <(grep -oE 'Rust [0-9]+\.[0-9]+(\.[0-9]+)?' "$readme" | sed 's/^Rust //')

gate_checked "$claims" "Rust-version claims in README.md"
if ((claims == 0)); then
    gate_fail "README.md states no Rust version; the build requirement is not a detail readers can be left to infer"
fi

gate_finish
