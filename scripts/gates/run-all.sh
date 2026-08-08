#!/usr/bin/env bash
#
# Run every gate predicate over the tree. `just ci` runs this, then `selftest.sh`.
#
# The population is derived from the directory listing (`gate_predicates`), so a
# predicate that exists is a predicate that runs. Every predicate is attempted even after
# one fails: a CI run that stops at the first violation makes the second one cost another
# round trip.
set -uo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"
printf 'gates: running every predicate over %s\n\n' "$root"

total=0
failed=0
declare -a red=()

while IFS= read -r predicate; do
    total=$((total + 1))
    name="$(basename "$predicate" .sh)"
    printf -- '--- %s\n' "$name"
    if ! "$predicate"; then
        failed=$((failed + 1))
        red+=("$name")
    fi
    printf '\n'
done < <(gate_predicates)

if ((total == 0)); then
    printf 'gates: FAIL — no predicates found; a suite with no members cannot go red\n' >&2
    exit 1
fi

if ((failed > 0)); then
    printf 'gates: FAIL — %s of %s predicates red: %s\n' "$failed" "$total" "${red[*]}" >&2
    exit 1
fi

printf 'gates: PASS — %s predicates green\n' "$total"
