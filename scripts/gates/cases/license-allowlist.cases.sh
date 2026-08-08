# Both-direction cases for `license-allowlist.sh`.
#
# The failing arms never touch the network: two of them point cargo-deny at the fixture
# workspaces under `fixtures/`, whose path dependencies manufacture the violation locally,
# and two of them empty the policy itself — because a `deny.toml` that has stopped saying
# anything is a licence gate that has stopped working, and it fails exactly like a
# violation does.
#
# shellcheck shell=bash

pass_case() {
    "$GATE"
}

fail_case_off_allowlist_license() {
    WCH_GATE_MANIFEST="$(gate_root)/scripts/gates/fixtures/offlicense/Cargo.toml" "$GATE"
}

fail_case_named_ban() {
    WCH_GATE_MANIFEST="$(gate_root)/scripts/gates/fixtures/banned-crate/Cargo.toml" "$GATE"
}

fail_case_emptied_ban_list() {
    local tree
    tree="$(gate_scratch_tree)"
    awk '
        /^deny = \[/  { print "deny = []"; skip = 1; next }
        skip && /^\]/ { skip = 0; next }
        skip          { next }
                      { print }
    ' "$tree/deny.toml" >"$tree/deny.toml.seeded"
    mv "$tree/deny.toml.seeded" "$tree/deny.toml"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_emptied_allowlist() {
    local tree
    tree="$(gate_scratch_tree)"
    awk '
        /^allow = \[/ { print "allow = []"; skip = 1; next }
        skip && /^\]/ { skip = 0; next }
        skip          { next }
                      { print }
    ' "$tree/deny.toml" >"$tree/deny.toml.seeded"
    mv "$tree/deny.toml.seeded" "$tree/deny.toml"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_deny_toml_deleted() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$tree/deny.toml"
    WCH_GATE_ROOT="$tree" "$GATE"
}
