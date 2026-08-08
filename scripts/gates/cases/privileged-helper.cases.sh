# Both-direction cases for `privileged-helper.sh`.
#
# The mode case is the one that matters: it is the *entire* security boundary around a
# binary that grants root, so a gate that could not see a widened mode would be checking
# nothing that counts.
#
# shellcheck shell=bash

pass_case() {
    "$GATE"
}

# A second green arm: a tree that HAS a blessed copy, correctly owner-only, must pass.
# Without this the mode check would be vacuous on every machine that has not blessed —
# which is all of CI, and would have been every run of this selftest.
pass_case_a_correctly_blessed_copy_is_allowed() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/.wch-bin"
    printf '#!/bin/false\n' >"$tree/.wch-bin/wch-priv"
    chmod 0700 "$tree/.wch-bin/wch-priv"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_blessed_copy_is_group_or_world_executable() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/.wch-bin"
    printf '#!/bin/false\n' >"$tree/.wch-bin/wch-priv"
    # 0755: exactly what a careless `chmod -R a+rX` or a restore-from-backup produces, and
    # on a real blessed copy it is a local root escalation for every user on the box.
    chmod 0755 "$tree/.wch-bin/wch-priv"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_blessed_directory_is_not_gitignored() {
    local tree
    tree="$(gate_scratch_tree)"
    grep -v '^/\.wch-bin/$' "$tree/.gitignore" >"$tree/.gitignore.tmp"
    mv "$tree/.gitignore.tmp" "$tree/.gitignore"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_product_crate_links_the_privileged_helper() {
    local md
    md="$(gate_metadata_snapshot)"
    # The defect this closes: a helper that reaches a shipped binary's link graph puts
    # root-capable code in something a user might install. Seeded in the resolve graph
    # rather than in a manifest, because that is where the gate reads it.
    jq '
        ( [ .packages[] | select(.name == "webcam-handler-priv") | .id ] | first ) as $priv
        | ( .resolve.nodes[] | select(.id | test("webcam-handler-cli[^-]")) | .deps )
            += [ { "pkg": $priv, "name": "webcam_handler_priv", "dep_kinds": [ { "kind": null, "target": null } ] } ]
    ' "$md" >"$md.seeded"
    WCH_GATE_METADATA="$md.seeded" "$GATE"
}

fail_case_the_helper_stops_forbidding_unsafe() {
    local tree
    tree="$(gate_scratch_tree)"
    sed -i 's/^#!\[forbid(unsafe_code)\]//' "$tree/crates/priv/src/main.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_helper_left_the_workspace() {
    local md
    md="$(gate_metadata_snapshot)"
    # Non-vacuity: with no subject, every claim above is trivially true, and a gate that
    # reports PASS over a helper that is no longer there is the worst kind of green.
    jq 'del(.packages[] | select(.name == "webcam-handler-priv"))' "$md" >"$md.seeded"
    WCH_GATE_METADATA="$md.seeded" "$GATE"
}
