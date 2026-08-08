# Both-direction cases for `dependency-walls.sh`.
#
# The linkage arms doctor a snapshot of the real graph — adding an edge to the workspace
# for real would mean committing the violation the gate exists to reject. The grep arm
# seeds a real source file in a scratch copy, because that half reads the tree.
#
# shellcheck shell=bash

# Add a normal dependency edge from one workspace member onto a package already in the
# graph, and print the doctored metadata's path.
_seeded_edge() {
    local from="$1" to="$2" md
    md="$(gate_metadata_snapshot)"
    jq --arg from "$from" --arg to "$to" '
        ( [ .packages[] | select(.name == $to) | .id ] | first ) as $target
        | ( [ .packages[] | select(.name == $from) | .id ] | first ) as $source
        | ( .resolve.nodes[] | select(.id == $source) | .deps )
            += [ { "name": $to, "pkg": $target, "dep_kinds": [ { "kind": null, "target": null } ] } ]
    ' "$md" >"$md.seeded"
    printf '%s\n' "$md.seeded"
}

pass_case() {
    "$GATE"
}

# T6: the pure crates carry no runtime.
fail_case_pure_crate_links_tokio() {
    WCH_GATE_METADATA="$(_seeded_edge webcam-handler-schema tokio)" "$GATE"
}

# The wire crate is exempt from the tokio half of wall 1 (note N5) and from nothing
# else: an axum edge on `webcam-handler-api` would put the web stack in `wchc`'s link
# graph through the shared trait.
fail_case_wire_crate_links_the_web_stack() {
    WCH_GATE_METADATA="$(_seeded_edge webcam-handler-api axum)" "$GATE"
}

# Only the two composition roots may construct a backend.
fail_case_non_root_links_a_backend() {
    WCH_GATE_METADATA="$(_seeded_edge webcam-handler-cli-core webcam-handler-fake)" "$GATE"
}

# The thin-client wall: `wchc` talks to the daemon and owns no camera.
fail_case_client_links_the_engine() {
    WCH_GATE_METADATA="$(_seeded_edge webcam-handler-client webcam-handler-engine)" "$GATE"
}

fail_case_client_links_a_backend() {
    WCH_GATE_METADATA="$(_seeded_edge webcam-handler-client webcam-handler-v4l2)" "$GATE"
}

# A rule whose subject has been renamed out of the workspace is a rule that cannot fail.
fail_case_policy_names_a_missing_member() {
    local md
    md="$(gate_metadata_snapshot)"
    jq '.workspace_members |= map(select(test("webcam-handler-client") | not))' \
        "$md" >"$md.seeded"
    WCH_GATE_METADATA="$md.seeded" "$GATE"
}

# If nothing lives under crates/backends/, the backend wall quantifies over nothing.
fail_case_no_backend_crates() {
    local md
    md="$(gate_metadata_snapshot)"
    jq '.packages |= map(.manifest_path |= sub("/crates/backends/"; "/crates/elsewhere/"))' \
        "$md" >"$md.seeded"
    WCH_GATE_METADATA="$md.seeded" "$GATE"
}

# The grep half: a V4L2 type escaping the backend that owns the ioctls.
fail_case_v4l2_path_outside_the_backend() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/engine/src/leak.rs" <<'RS'
//! Seeded by the gate selftest: the engine must never name a V4L2 type.
use v4l::device::Device;

pub fn leak(_device: &Device) {}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}
