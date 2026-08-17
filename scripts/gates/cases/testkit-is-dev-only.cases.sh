# Both-direction cases for `testkit-is-dev-only.sh`.
#
# The non-vacuity arm is the interesting one: "nothing depends on the testkit as a normal
# dependency" is trivially true of a testkit nothing depends on at all.
#
# Each arm names the sentence it is claiming (`gate_red_because`, note **N31**), and here that is
# not decoration: rewriting an edge's kind removes the *dev* kind at the same time, so three of
# these four seeds are red under the non-vacuity branch as well as under their own. An arm reading
# only the exit status could not tell which of the two fired, and the kind branch could rot to
# unreachable while every arm stayed comfortably non-zero.
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
    gate_red_because 'as a normal dependency; it is dev-only' \
        env WCH_GATE_METADATA="$md.seeded" "$GATE"
}

fail_case_build_edge_onto_the_testkit() {
    local md
    md="$(gate_metadata_snapshot)"
    jq '
        ( [ .packages[] | select(.name == "webcam-handler-testkit") | .id ] | first ) as $tk
        | ( .resolve.nodes[].deps[] | select(.pkg == $tk) | .dep_kinds )
            |= [ { "kind": "build", "target": null } ]
    ' "$md" >"$md.seeded"
    gate_red_because 'as a build dependency; it is dev-only' \
        env WCH_GATE_METADATA="$md.seeded" "$GATE"
}

fail_case_nothing_depends_on_the_testkit() {
    local md
    md="$(gate_metadata_snapshot)"
    jq '
        ( [ .packages[] | select(.name == "webcam-handler-testkit") | .id ] | first ) as $tk
        | .resolve.nodes[].deps |= map(select(.pkg != $tk))
    ' "$md" >"$md.seeded"
    gate_red_because 'nothing dev-depends on webcam-handler-testkit' \
        env WCH_GATE_METADATA="$md.seeded" "$GATE"
}

fail_case_testkit_left_the_graph() {
    local md
    md="$(gate_metadata_snapshot)"
    jq 'del(.packages[] | select(.name == "webcam-handler-testkit"))' "$md" >"$md.seeded"
    gate_red_because 'webcam-handler-testkit is not in the graph; this rule has no subject' \
        env WCH_GATE_METADATA="$md.seeded" "$GATE"
}
