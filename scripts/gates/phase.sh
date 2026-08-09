#!/usr/bin/env bash
#
# The phase gate: `./scripts/gates/phase.sh g0`, which is what `just gate-g0` runs.
#
# Named, counted and re-runnable, because docs/7's standing convention says a criterion
# nobody can re-run is a criterion nobody will re-check. The criteria live in
# `phase-criteria.tsv`, one row each; this file is the runner, not the list.
#
# A phase with no rows fails. `just gate-g4` today reports that G4 has no criteria yet,
# which is the truth; a gate that passes because it was asked to check nothing is the
# defect class this whole suite exists to prevent.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"
table="$root/scripts/gates/phase-criteria.tsv"

if [[ $# -ne 1 ]]; then
    printf 'usage: %s <phase>   (one of: %s)\n' "$0" \
        "$(awk -F'\t' '!/^#/ && NF >= 4 { print $1 }' "$table" | sort -u | tr '\n' ' ')" >&2
    exit 2
fi
phase="$1"
GATE_NAME="gate-$phase"

if [[ ! -f "$table" ]]; then
    gate_fail "no criteria table at $table"
    gate_finish
fi

known_phases="$(awk -F'\t' '!/^#/ && NF >= 4 { print $1 }' "$table" | sort -u | tr '\n' ' ')"
gate_note "phases with criteria: ${known_phases:-none}"

checked=0
while IFS=$'\t' read -r row_phase kind selection what; do
    case "$row_phase" in
    '#'* | '') continue ;;
    esac
    if [[ "$row_phase" != "$phase" ]]; then
        continue
    fi
    checked=$((checked + 1))
    printf '\n  [%s.%s] %s\n' "$phase" "$checked" "$what"

    status=0
    case "$kind" in
    command)
        # Criteria are commands run from the repository root, so a criterion reads the
        # same way a human would run it.
        (cd "$root" && eval "$selection") || status=$?
        ;;
    tests)
        (cd "$root" && cargo nextest run --locked --offline --workspace \
            --no-tests=fail -E "$selection") || status=$?
        ;;
    *)
        gate_fail "row $checked of $phase has unknown kind '$kind'"
        continue
        ;;
    esac

    if ((status != 0)); then
        gate_fail "$phase criterion $checked failed (exit $status): $what"
    fi
done <"$table"

gate_checked "$checked" "criteria for $phase"
if ((checked == 0)); then
    gate_fail "$phase has no criteria in $(basename "$table"); a phase gate with nothing to check cannot close a phase"
fi

gate_finish
