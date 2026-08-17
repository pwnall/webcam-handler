# Both-direction cases for `feature-posture.sh`.
#
# Every failing arm doctors a snapshot of the *real* resolved graph, so the seeded tree
# differs from the shipped one in exactly the way the case name says. Opening the LGPL
# door for real — enabling `v4l/libv4l` in the workspace — is not something a test may do,
# which is why the predicate has the $WCH_GATE_METADATA seam at all.
#
# Each arm names the sentence it is claiming (`gate_red_because`, note **N31**). Two pairs here
# are one seed apart and would be indistinguishable by exit status alone — a TLS *crate* in the
# graph against a TLS-shaped *feature* turned on, and a posture rule broken against the crate it
# is about leaving the graph — so the arm that swapped its subject would go on reporting `ok`.
#
# shellcheck shell=bash

pass_case() {
    "$GATE"
}

# The `libv4l` feature pulls `v4l-sys`, which links LGPL libv4l.
fail_case_v4l_non_default_feature() {
    local md
    md="$(gate_metadata_snapshot)"
    jq '(.resolve.nodes[] | select(.id | test("#v4l@")) | .features) += ["libv4l"]' \
        "$md" >"$md.seeded"
    gate_red_because 'has non-default feature(s) enabled: libv4l' \
        env WCH_GATE_METADATA="$md.seeded" "$GATE"
}

# `image`'s default set includes `avif`, which drags the rav1e AV1 encoder.
fail_case_image_default_features() {
    local md
    md="$(gate_metadata_snapshot)"
    jq '(.resolve.nodes[] | select(.id | test("#image@")) | .features) += ["default"]' \
        "$md" >"$md.seeded"
    gate_red_because 'has its default feature set enabled' \
        env WCH_GATE_METADATA="$md.seeded" "$GATE"
}

fail_case_tls_feature_enabled() {
    local md
    md="$(gate_metadata_snapshot)"
    jq '(.resolve.nodes[0].features) += ["native-tls"]' "$md" >"$md.seeded"
    # The crate the feature lands on is whichever package the graph happens to list first, so
    # the claim names the feature and not its host.
    gate_red_because '/native-tls is enabled' env WCH_GATE_METADATA="$md.seeded" "$GATE"
}

fail_case_tls_crate_in_graph() {
    local md
    md="$(gate_metadata_snapshot)"
    jq '.packages += [{
            "name": "webpki-roots",
            "version": "1.0.0",
            "id": "registry+https://example.invalid#webpki-roots@1.0.0",
            "features": {},
            "manifest_path": "/nonexistent/webpki-roots/Cargo.toml",
            "targets": []
        }]' "$md" >"$md.seeded"
    gate_red_because 'TLS crate webpki-roots 1.0.0 is in the graph' \
        env WCH_GATE_METADATA="$md.seeded" "$GATE"
}

# A posture rule about a crate that has left the graph is a rule that can no longer fail.
fail_case_posture_crate_left_the_graph() {
    local md
    md="$(gate_metadata_snapshot)"
    jq 'del(.packages[] | select(.name == "v4l"))' "$md" >"$md.seeded"
    gate_red_because 'v4l is not in the dependency graph' \
        env WCH_GATE_METADATA="$md.seeded" "$GATE"
}

# An empty or unreadable graph must not read as "nothing wrong".
fail_case_empty_metadata() {
    local md
    md="$(mktemp "$(gate_scratch_root)/wch-empty.XXXXXXXX")"
    : >"$md"
    # Red on the rule count as well — a document with no packages has no policy either — and the
    # population this arm is about is the graph.
    gate_red_because 'examined zero packages in the resolved graph' \
        env WCH_GATE_METADATA="$md" "$GATE"
}
