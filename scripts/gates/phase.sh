#!/usr/bin/env bash
#
# The phase gate: `./scripts/gates/phase.sh g0`, which is what `just gate-g0` runs.
#
# Named, counted and re-runnable, because docs/7's standing convention says a criterion
# nobody can re-run is a criterion nobody will re-check. The criteria live in
# `phase-criteria.tsv`, one row each; this file is the runner, not the list.
#
# A phase with no rows fails, and that is a feature: a gate passing because it was asked
# to check nothing is the defect class this whole suite exists to prevent. Criteria accrete
# row by row, in the commit that lands the thing each row proves, so every phase spends some
# time in that state and says so out loud while it is there — `g5` left it at P5a with the
# ten rows the TCP listener and the token gate earned, and `g6` left it at P6a with one.
# **Since P6e every phase g0–g6 has rows**, so the empty-phase branch below now guards a
# *future* gate rather than describing a current one, which is the only honest reason to
# keep it. How many rows each phase holds is the table's to report and not this header's:
# a count written here is a count that stops being true in the next commit that lands a
# criterion, which is note N10's family wearing a comment.
#
# ## A criterion has three answers, and this runner keeps them apart
#
# A `command` row that exits `$GATE_NO_VERDICT` (75, `EX_TEMPFAIL` — see `lib.sh`) is
# saying "I could not answer", not "the answer is no": it ran out of disk, it was
# interrupted, its baseline would not build, or the tree moved while it was reading it.
# `scripts/mutants.sh` is the row that says it today (notes N52, N66, **N68**).
#
# Rendering that as `FAIL … criterion N failed` is not a small infidelity. The whole point
# of the third outcome is that nobody re-runs a gate at `-j1` until it agrees and then
# waves the next real finding through (N60), and a gate table that flattens the three
# answers back into two would have moved the ambiguity out one layer rather than removing
# it. So a no-verdict row is reported as one, this gate ends on `NO VERDICT` when that is
# all that happened, and the exit code carries the distinction out to whoever ran
# `just gate-g4`. It is still non-zero: an unproven criterion never closes a phase.
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

    # The three answers, kept apart. `tests` rows cannot produce the third one — nextest
    # has no such exit code — so this only ever fires for a `command` row, and today
    # `scripts/mutants.sh` is the only command that produces it.
    if ((status == GATE_NO_VERDICT)); then
        gate_no_verdict "$phase criterion $checked could not produce a verdict (exit $status), so it is neither proven nor disproven: $what"
    elif ((status != 0)); then
        gate_fail "$phase criterion $checked failed (exit $status): $what"
    fi
done <"$table"

gate_checked "$checked" "criteria for $phase"
if ((checked == 0)); then
    gate_fail "$phase has no criteria in $(basename "$table"); a phase gate with nothing to check cannot close a phase"
fi

gate_finish
