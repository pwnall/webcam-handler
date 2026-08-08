# Both-direction cases for `testkit-is-dev-only.sh`.
#
# The non-vacuity arm is the interesting one: "nothing depends on the testkit as a normal
# dependency" is trivially true of a testkit nothing depends on at all.
#
# shellcheck shell=bash

pass_case() {
    "$GATE"
}

fail_case_normal_edge_onto_the_testkit() {
    local md
    md="$(gate_metadata_snapshot)"
    jq '
        ( [ .packages[] | select(.name == "webcam-handler-testkit") | .id ] | first ) as $tk
        | ( .resolve.nodes[].deps[] | select(.pkg == $tk) | .dep_kinds )
            |= [ { "kind": null, "target": null } ]
    ' "$md" >"$md.seeded"
    WCH_GATE_METADATA="$md.seeded" "$GATE"
}

fail_case_build_edge_onto_the_testkit() {
    local md
    md="$(gate_metadata_snapshot)"
    jq '
        ( [ .packages[] | select(.name == "webcam-handler-testkit") | .id ] | first ) as $tk
        | ( .resolve.nodes[].deps[] | select(.pkg == $tk) | .dep_kinds )
            |= [ { "kind": "build", "target": null } ]
    ' "$md" >"$md.seeded"
    WCH_GATE_METADATA="$md.seeded" "$GATE"
}

fail_case_nothing_depends_on_the_testkit() {
    local md
    md="$(gate_metadata_snapshot)"
    jq '
        ( [ .packages[] | select(.name == "webcam-handler-testkit") | .id ] | first ) as $tk
        | .resolve.nodes[].deps |= map(select(.pkg != $tk))
    ' "$md" >"$md.seeded"
    WCH_GATE_METADATA="$md.seeded" "$GATE"
}

fail_case_testkit_left_the_graph() {
    local md
    md="$(gate_metadata_snapshot)"
    jq 'del(.packages[] | select(.name == "webcam-handler-testkit"))' "$md" >"$md.seeded"
    WCH_GATE_METADATA="$md.seeded" "$GATE"
}
