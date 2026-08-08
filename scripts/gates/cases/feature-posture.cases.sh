# Both-direction cases for `feature-posture.sh`.
#
# Every failing arm doctors a snapshot of the *real* resolved graph, so the seeded tree
# differs from the shipped one in exactly the way the case name says. Opening the LGPL
# door for real — enabling `v4l/libv4l` in the workspace — is not something a test may do,
# which is why the predicate has the $WCH_GATE_METADATA seam at all.
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
    WCH_GATE_METADATA="$md.seeded" "$GATE"
}

# `image`'s default set includes `avif`, which drags the rav1e AV1 encoder.
fail_case_image_default_features() {
    local md
    md="$(gate_metadata_snapshot)"
    jq '(.resolve.nodes[] | select(.id | test("#image@")) | .features) += ["default"]' \
        "$md" >"$md.seeded"
    WCH_GATE_METADATA="$md.seeded" "$GATE"
}

fail_case_tls_feature_enabled() {
    local md
    md="$(gate_metadata_snapshot)"
    jq '(.resolve.nodes[0].features) += ["native-tls"]' "$md" >"$md.seeded"
    WCH_GATE_METADATA="$md.seeded" "$GATE"
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
    WCH_GATE_METADATA="$md.seeded" "$GATE"
}

# A posture rule about a crate that has left the graph is a rule that can no longer fail.
fail_case_posture_crate_left_the_graph() {
    local md
    md="$(gate_metadata_snapshot)"
    jq 'del(.packages[] | select(.name == "v4l"))' "$md" >"$md.seeded"
    WCH_GATE_METADATA="$md.seeded" "$GATE"
}

# An empty or unreadable graph must not read as "nothing wrong".
fail_case_empty_metadata() {
    local md
    md="$(mktemp "${WCH_GATE_SCRATCH:-${TMPDIR:-/tmp}}/wch-empty.XXXXXXXX")"
    : >"$md"
    WCH_GATE_METADATA="$md" "$GATE"
}
