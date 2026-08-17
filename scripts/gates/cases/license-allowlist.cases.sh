# Both-direction cases for `license-allowlist.sh`.
#
# The failing arms never touch the network: two of them point cargo-deny at the fixture
# workspaces under `fixtures/`, whose path dependencies manufacture the violation locally,
# and two of them empty the policy itself — because a `deny.toml` that has stopped saying
# anything is a licence gate that has stopped working, and it fails exactly like a
# violation does.
#
# Each arm names the sentence it is claiming (`gate_red_because`, note **N31**), and this file is
# where that rule paid for itself: `fail_case_emptied_allowlist` was red on the wrong branch for
# as long as it had existed. Its seed wrote `allow = []` and deleted the list's closing bracket,
# and the predicate's counter — which opens on `^allow…[` and closes on `^]` — then ran to the
# end of the file counting every quoted line it met, reporting **28** allowlisted identifiers
# where the shipped tree has 12. `gate_require_nonzero` was therefore never reached, the arm went
# red only because cargo-deny rejects a graph against an empty allowlist, and the size branch it
# was written for had no arm at all. The seed now leaves the brackets where they were and empties
# what is between them, which is what makes the count zero; both empty-policy arms are written
# the same way so the pair cannot drift apart again.
#
# shellcheck shell=bash

pass_case() {
    "$GATE"
}

# The two cargo-deny arms name the diagnostic rather than the gate's own "cargo deny rejected
# the graph" line, which both of them would match: the fixtures exist to prove the *licence*
# clause and the *ban* clause separately, and an arm that named the shared sentence would stay
# green if a fixture started failing the other one.
fail_case_off_allowlist_license() {
    gate_red_because 'error[rejected]: failed to satisfy license requirements' \
        env WCH_GATE_MANIFEST="$(gate_root)/scripts/gates/fixtures/offlicense/Cargo.toml" "$GATE"
}

fail_case_named_ban() {
    gate_red_because 'is explicitly banned' \
        env WCH_GATE_MANIFEST="$(gate_root)/scripts/gates/fixtures/banned-crate/Cargo.toml" "$GATE"
}

fail_case_emptied_ban_list() {
    local tree
    tree="$(gate_scratch_tree)"
    awk '
        /^deny = \[/  { print; skip = 1; next }
        skip && /^\]/ { skip = 0; print; next }
        skip          { next }
                      { print }
    ' "$tree/deny.toml" >"$tree/deny.toml.seeded"
    mv "$tree/deny.toml.seeded" "$tree/deny.toml"
    gate_red_because 'examined zero named crate bans' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_emptied_allowlist() {
    local tree
    tree="$(gate_scratch_tree)"
    awk '
        /^allow = \[/ { print; skip = 1; next }
        skip && /^\]/ { skip = 0; print; next }
        skip          { next }
                      { print }
    ' "$tree/deny.toml" >"$tree/deny.toml.seeded"
    mv "$tree/deny.toml.seeded" "$tree/deny.toml"
    gate_red_because 'examined zero allowlisted licences' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_deny_toml_deleted() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$tree/deny.toml"
    gate_red_because 'the licence policy has no home' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}
