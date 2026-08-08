#!/usr/bin/env bash
#
# `webcam-handler-testkit` is a dev-dependency and nothing else (design §2.8).
#
# It holds the conformance battery, the corpus loader and the container oracles, so it
# will grow dependencies chosen for convenience rather than for the shipped licence
# inventory. A normal or build edge onto it puts those in `wch`.
#
# Both directions matter, and the second one is the reason this file exists: a testkit
# nothing depends on would satisfy "no normal edge" perfectly while proving that the
# battery is wired to nothing. So the predicate also requires at least one dev edge.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

testkit="webcam-handler-testkit"

findings="$(gate_metadata | jq -r --arg testkit "$testkit" '
    ( [ .packages[] | { key: .id, value: . } ] | from_entries ) as $pkgs
    | ( [ .workspace_members[] ] ) as $members
    | ( [ $pkgs[] | select(.name == $testkit) | .id ] ) as $ids
    | ( [ .resolve.nodes[]
          | .id as $from
          | .deps[]
          | .pkg as $to
          | select($ids | index($to))
          | { from: $pkgs[$from].name,
              kinds: [ .dep_kinds[] | (.kind // "normal") ] } ] ) as $edges
    | (
      "COUNT\tmembers\t\($members | length)\tworkspace members inspected for edges onto \($testkit)"
    , "COUNT\tdevedges\t\([ $edges[] | select(.kinds | index("dev")) ] | length)\tdev-dependency edges onto \($testkit)"
    , ( if ($ids | length) == 0 then
          "VIOLATION\t\($testkit) is not in the graph; this rule has no subject"
        else empty end )
    , ( $edges[]
        | . as $e
        | ($e.kinds | map(select(. != "dev")))[]
        | "VIOLATION\t\($e.from) depends on \($testkit) as a \(.) dependency; it is dev-only" )
    )
')"

declare -A counts=([members]=0 [devedges]=0)
while IFS=$'\t' read -r kind key value description; do
    case "$kind" in
    COUNT)
        counts["$key"]="$value"
        gate_checked "$value" "$description"
        ;;
    VIOLATION) gate_fail "$key" ;;
    esac
done <<<"$findings"

gate_require_nonzero "${counts[members]}" "workspace members"
if ((counts[devedges] == 0)); then
    gate_fail "nothing dev-depends on $testkit — the absence of a normal edge proves nothing about a crate nobody uses"
fi

gate_finish
