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

# The same, onto a package the resolved graph does not contain — the package is fabricated
# beside the edge, with the two fields this predicate reads of it.
#
# `tower-http` is the case this exists for and the reason it is not over-engineering. Design
# §2.8 pins it in `[workspace.dependencies]` and **no crate names it yet**: the daemon's own
# manifest says "tower-http is still not here, because the thing that reads it is P5b's", so it
# is absent from `Cargo.lock` and from `cargo metadata`. `_seeded_edge` onto a name that is not
# in the graph makes `jq` exit non-zero, and an arm that goes red because its *seeding* failed
# proves nothing at all about the predicate — that is the one confusion this harness must not
# have (`selftest.sh`'s `run_case` states it). A forbidden name that cannot be seeded is a wall
# row nothing has ever shown going red, which is the state a wall is supposed to prevent
# elsewhere.
_seeded_foreign_edge() {
    local from="$1" to="$2" md
    md="$(gate_metadata_snapshot)"
    jq --arg from "$from" --arg to "$to" '
        ( "registry+https://example.invalid/seeded#\($to)@0.0.0" ) as $target
        | ( [ .packages[] | select(.name == $from) | .id ] | first ) as $source
        | .packages += [ { "name": $to,
                           "id": $target,
                           "manifest_path": "/seeded/\($to)/Cargo.toml",
                           "targets": [] } ]
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

# The engine names no runtime, and that is what lets its camera actor be a blocking OS thread
# whose reply channel the *caller* supplies — a tokio type in `engine::actor`'s signature would
# force a runtime on the daemonless `webcam-handler-cli` (note N41). A tokio edge here makes
# that argument false without changing a line of `engine::actor`, which is exactly the kind of
# silent repeal a wall is for.
fail_case_the_engine_grows_a_runtime() {
    WCH_GATE_METADATA="$(_seeded_edge webcam-handler-engine tokio)" "$GATE"
}

# The wire crate is exempt from the tokio half of wall 1 (note N5) and from nothing else: an
# axum edge on `webcam-handler-api` would put the web stack in `webcam-handler-client`'s link
# graph through the shared trait.
fail_case_wire_crate_links_the_web_stack() {
    WCH_GATE_METADATA="$(_seeded_edge webcam-handler-api axum)" "$GATE"
}

# The asset crate, in both halves of its wall. `webcam-handler-web` embeds a directory and
# answers values; the moment it links a server it is answering requests, and "what these files
# are" and "how bytes become an HTTP response" stop being two laws with one home each (design
# §2.10). Both arms exist because the two halves arrive by different routes and neither implies
# the other: `rust-embed`'s `axum-ex` feature is the server half, one line in a manifest away,
# and the runtime half is what any async convenience added to this crate would bring.
fail_case_the_asset_crate_grows_a_server() {
    WCH_GATE_METADATA="$(_seeded_edge webcam-handler-web axum)" "$GATE"
}

fail_case_the_asset_crate_grows_a_runtime() {
    WCH_GATE_METADATA="$(_seeded_edge webcam-handler-web tokio)" "$GATE"
}

# The middleware layer specifically, which neither of the lists this crate could have been
# added to covers: the pure wall does not name `tower-http` at all, and the wire wall allows
# tokio. A wall spelled as a union is only worth having if the part that is *only* in the union
# can turn it red.
fail_case_the_asset_crate_grows_a_middleware_stack() {
    WCH_GATE_METADATA="$(_seeded_foreign_edge webcam-handler-web tower-http)" "$GATE"
}

# Only the two composition roots may construct a backend.
fail_case_non_root_links_a_backend() {
    WCH_GATE_METADATA="$(_seeded_edge webcam-handler-cli-core webcam-handler-fake)" "$GATE"
}

# The thin-client wall: `webcam-handler-client` talks to the daemon and owns no camera.
fail_case_client_links_the_engine() {
    WCH_GATE_METADATA="$(_seeded_edge webcam-handler-client webcam-handler-engine)" "$GATE"
}

fail_case_client_links_a_backend() {
    WCH_GATE_METADATA="$(_seeded_edge webcam-handler-client webcam-handler-v4l2)" "$GATE"
}

# The other direction of the wall above, and the reason the asset crate is in the named-roles
# list as well as in a rule: a wall whose subject is no longer a workspace member is a wall
# that has stopped being able to go red, and it stops silently.
fail_case_the_asset_crate_left_the_workspace() {
    local md
    md="$(gate_metadata_snapshot)"
    jq '.workspace_members |= map(select(test("webcam-handler-web") | not))' \
        "$md" >"$md.seeded"
    WCH_GATE_METADATA="$md.seeded" "$GATE"
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
