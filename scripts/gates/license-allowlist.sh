#!/usr/bin/env bash
#
# The dependency licence posture (design §2.8, rubric B9): permissive licences only, plus
# the named bans that keep this domain's known traps — LGPL `libv4l`/`libudev`/
# `libcamera`/`alsa-lib` linkage, the MPL wrappers, the GPL codec stacks, the IJG term —
# arriving by decision rather than by accident.
#
# The predicate drives `cargo deny` rather than reimplementing it, and always with the
# repository's own `deny.toml` (`-c`), so the fixtures the selftest points it at cannot
# drift from the policy the shipped tree is held to: delete a ban from `deny.toml` and
# the failing arm goes green, which the selftest reports.
#
# $WCH_GATE_MANIFEST is the selftest's seam. The failing arms aim it at the fixture
# workspaces under `fixtures/`, whose path dependencies manufacture a violation with no
# registry involved — `just ci` runs offline, so a failing arm that had to download a
# banned crate could not exist. The passing arm always resolves the real workspace.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"
manifest="${WCH_GATE_MANIFEST:-$root/Cargo.toml}"
config="$root/deny.toml"

if [[ ! -f "$config" ]]; then
    gate_fail "no deny.toml at $config — the licence policy has no home"
    gate_finish
fi

# The policy's own size. A `deny.toml` that has stopped saying anything passes every
# graph, so the entry counts are part of what this gate checks: an emptied ban list is a
# violation of the same rule the bans themselves enforce.
allowed_licenses="$(awk '
    /^\[licenses\]/ { section = "licenses"; next }
    /^\[/           { section = "" }
    section == "licenses" && /^allow[[:space:]]*=[[:space:]]*\[/ { inside = 1; next }
    inside && /^\]/ { inside = 0 }
    inside && /"/   { n++ }
    END             { print n + 0 }
' "$config")"

named_bans="$(awk '
    /^\[bans\]/ { section = "bans"; next }
    /^\[/       { section = "" }
    section == "bans" && /\{[[:space:]]*crate[[:space:]]*=[[:space:]]*"/ { n++ }
    END         { print n + 0 }
' "$config")"

gate_checked "$allowed_licenses" "allowlisted licence identifiers in deny.toml"
gate_checked "$named_bans" "named crate bans in deny.toml"
gate_require_nonzero "$allowed_licenses" "allowlisted licences"
gate_require_nonzero "$named_bans" "named crate bans"

crates="$(cargo metadata --locked --offline --format-version 1 \
    --manifest-path "$manifest" | jq '.packages | length')"
gate_checked "$crates" "crates in the graph rooted at $manifest"
gate_require_nonzero "$crates" "crates in the dependency graph"

deny_output=""
deny_status=0
deny_output="$(cargo deny --offline --locked --manifest-path "$manifest" \
    check bans licenses sources -c "$config" 2>&1)" || deny_status=$?
printf '%s\n' "$deny_output" | sed 's/^/  | /'
if ((deny_status != 0)); then
    gate_fail "cargo deny rejected the graph rooted at $manifest"
fi

gate_finish
