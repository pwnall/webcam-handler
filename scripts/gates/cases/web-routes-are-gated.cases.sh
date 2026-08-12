# Both-direction cases for `web-routes-are-gated.sh`.
#
# The predicate is a partition claim — the routes and the names on `CAMERA_BEARING_PATHS` are
# the same set — plus two claims about what is deliberately *outside* it (the asset fallback,
# and the one place the gate is installed). Each of the four gets its own inverses, and the arm
# this gate is named for is `fail_case_a_new_route_module_nobody_put_on_the_list`: that is the
# shape the defect the owner's 2026-08-12 ruling created actually takes — P5c or P6 adds a route
# that carries camera data, forgets the list, and every test in the workspace stays green
# because no test knows to ask for a path nobody wrote down.
#
# The seeds are Rust-shaped but never compiled: this predicate reads source, so a seeded
# violation has to be readable rather than buildable, and a case that ran cargo would be
# measuring something else.
#
# shellcheck shell=bash

# A file that registers `$2` as a route, written to `$1/$3`. Product code only and no test
# module, which is what makes it the honest shape of a new route module.
seed_route_module() {
    local tree="$1" path="$2" where="$3"
    cat >"$tree/$where" <<RS
//! A route somebody added.

use axum::Router;
use axum::routing::get;

pub const SNAPSHOT_PATH: &str = "/snapshot";

pub(super) fn mount(previews: crate::preview::Previews) -> Router {
    Router::new()
        .route($path, get(snapshot))
        .with_state(previews)
}

async fn snapshot() -> Vec<u8> {
    Vec::new()
}
RS
}

pass_case() {
    "$GATE"
}

pass_case_a_comment_may_name_a_route_it_does_not_register() {
    # The modules this gate is about argue about routing at length and name the very spellings
    # it matches on — `daemon::http::listener`'s header explains `route_layer` by writing it
    # out. Prose is stripped before matching, so this arm is the other direction of that: a
    # comment naming a route is not a route, and a gate that could not tell them apart would
    # push the argument out of the file that needs it.
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/crates/daemon/src/http/rpc.rs" <<'RS'

// Nothing here registers anything: a second endpoint would be `.route("/snapshot", get(…))`
// in this module and a name on `CAMERA_BEARING_PATHS`, and `.fallback(other)` would be a
// second thing served without the token.
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

pass_case_a_test_may_build_a_router_of_its_own() {
    # A suite that wants to assert something about a handler builds a router around it, on
    # whatever path it likes, and that router is never served. Test code is not counted — the
    # same decision `token-comparison-has-one-home.sh` makes about a test holding the secret,
    # and for the same reason: a confinement that refused those is one somebody turns off.
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/daemon/src/http/scratch.rs" <<'RS'
//! A module whose only router is in its tests.

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::routing::get;

    #[test]
    fn a_router_this_test_built() {
        let _router: Router = Router::new()
            .route("/anything-at-all", get(|| async {}))
            .fallback(|| async {});
    }
}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_new_route_module_nobody_put_on_the_list() {
    # **The arm this gate exists for.** A route on a path of its own, registered in the daemon's
    # own `http` tree, with `CAMERA_BEARING_PATHS` left alone — which is what a live camera
    # served to strangers looks like in a diff.
    local tree
    tree="$(gate_scratch_tree)"
    seed_route_module "$tree" '"/snapshot"' crates/daemon/src/http/snapshot.rs
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_new_route_on_a_constant_the_list_does_not_name() {
    # The same defect written tidily. A `pub const SNAPSHOT_PATH` beside the route reads like
    # every other path in this daemon and is still a path the gate is not over, so the claim is
    # about the list rather than about literals.
    local tree
    tree="$(gate_scratch_tree)"
    seed_route_module "$tree" SNAPSHOT_PATH crates/daemon/src/http/snapshot.rs
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_route_registered_outside_the_listener_module() {
    # A router built in another crate — reachable only if something serves it, which is exactly
    # the edit that would follow. The composition has one home (design §2.10) and this is the
    # arm that says so.
    local tree
    tree="$(gate_scratch_tree)"
    seed_route_module "$tree" '"/snapshot"' crates/engine/src/snapshot_route.rs
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_file_that_registers_a_route_and_cannot_be_classified() {
    # Two `#[cfg(test)]` markers, so "which half of this file is product code" has no answer.
    # A file this cannot classify is a finding rather than a pass — `unsafe-scope.sh`'s price
    # for a count it cannot read, charged here for a boundary it cannot find.
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/daemon/src/http/snapshot.rs" <<'RS'
//! A module whose product/test boundary has two answers.

#[cfg(test)]
mod first {}

pub(super) fn mount() -> axum::Router {
    axum::Router::new().route("/snapshot", axum::routing::get(|| async {}))
}

#[cfg(test)]
mod second {}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_second_fallback_answers_beside_the_assets() {
    # The door claim 2 does not watch: not a route, but an answer for every path that is not
    # one — served, since the ruling, without a credential.
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/daemon/src/http/snapshot.rs" <<'RS'
//! A second answer for a request that matched no route.

pub(super) fn mount() -> axum::Router {
    axum::Router::new().fallback(snapshot)
}

async fn snapshot() -> Vec<u8> {
    Vec::new()
}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_fallback_stops_being_the_asset_table() {
    # The same claim from the other side: the one fallback is still one, and it now reaches
    # something other than the embedded assets — which is a handler served to anybody.
    local tree
    tree="$(gate_scratch_tree)"
    sed -i 's/\.fallback(asset)/.fallback(anything_at_all)/' \
        "$tree/crates/daemon/src/http/listener.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_gate_went_back_over_everything() {
    # `Router::layer` in place of `route_layer` gates the client's own source code, which is
    # what the owner ruled it must not: the modules 401, the page does not load, and every
    # camera-bearing route is still gated so the suite's own refusal assertions stay green.
    local tree
    tree="$(gate_scratch_tree)"
    sed -i 's/routes\.route_layer(/routes.layer(/' "$tree/crates/daemon/src/http/listener.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_gate_is_no_longer_installed_at_all() {
    # Every other claim in this predicate is true of a tree that stopped gating anything —
    # `kill-is-never-a-fallback.sh`'s "the only caller went away" arm, about the one absence
    # that matters most here.
    local tree
    tree="$(gate_scratch_tree)"
    sed -i 's/routes\.route_layer(axum::middleware::from_fn_with_state(token, gate::check))/routes/' \
        "$tree/crates/daemon/src/http/listener.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_route_installs_a_gate_of_its_own() {
    # Which routes are behind the token is one decision in one `match`. A second install is a
    # second home for it, and the one that would be forgotten when the first one moves.
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/crates/daemon/src/http/scratch.rs" <<'RS'
//! A route that gates itself.

pub(super) fn mount(token: std::sync::Arc<super::token::Token>) -> axum::Router {
    axum::Router::new().layer(axum::middleware::from_fn_with_state(token, gate::check))
}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_list_is_empty() {
    # Not a narrower gate: a gate with no subject. Every route registration would then be a
    # route on a path the list does not name, and the count this predicate requires to be
    # non-zero says so before the comparison does.
    local tree
    tree="$(gate_scratch_tree)"
    sed -i 's/pub const CAMERA_BEARING_PATHS: \[&str; 2\] = \[rpc::RPC_PATH, preview::PREVIEW_PATH\];/pub const CAMERA_BEARING_PATHS: [\&str; 0] = [];/' \
        "$tree/crates/daemon/src/http/mod.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_list_names_a_literal_instead_of_the_constant() {
    # A path written twice is a path that can stop matching in one of the two places, and the
    # place it would stop matching is the router — leaving a list that still reads correctly
    # over a route nothing gates.
    local tree
    tree="$(gate_scratch_tree)"
    sed -i 's/= \[rpc::RPC_PATH, preview::PREVIEW_PATH\];/= ["\/rpc", "\/preview"];/' \
        "$tree/crates/daemon/src/http/mod.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_list_names_a_path_nothing_declares() {
    # The list is the population every other claim quantifies over, so an entry that resolves
    # to nothing is a gate over a path that does not exist — and a route that does exist with
    # nothing naming it.
    local tree
    tree="$(gate_scratch_tree)"
    sed -i 's/preview::PREVIEW_PATH\];/preview::SNAPSHOT_PATH];/' \
        "$tree/crates/daemon/src/http/mod.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_list_declaration_is_gone() {
    # A rename, which is the ordinary way a named policy stops being the one a gate reads.
    local tree
    tree="$(gate_scratch_tree)"
    sed -i 's/pub const CAMERA_BEARING_PATHS/pub const GATED_PATHS/' \
        "$tree/crates/daemon/src/http/mod.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_composition_module_is_missing() {
    # The file every claim about the gate is about. Its absence must not read as compliance.
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$tree/crates/daemon/src/http/listener.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_no_route_is_registered_at_all() {
    # A listener with no routes serves no camera, which is a different defect and equally not
    # a pass: the population every comparison here rests on would be empty.
    local tree
    tree="$(gate_scratch_tree)"
    sed -i 's/\.route(RPC_PATH, any(upgrade))/.with_state(())/' \
        "$tree/crates/daemon/src/http/rpc.rs"
    sed -i 's/\.route(PREVIEW_PATH, get(stream))/.with_state(())/' \
        "$tree/crates/daemon/src/http/preview.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_daemon_left_the_workspace() {
    # The predicate resolves the daemon's directory from `cargo metadata`, so a graph without
    # it is a graph where this gate has nothing to look at — and a gate that quietly passes
    # over an absent subject stops being able to fail at all.
    local md
    md="$(gate_metadata_snapshot)"
    jq 'del(.packages[] | select(.name == "webcam-handler-daemon"))' "$md" >"$md.seeded"
    WCH_GATE_METADATA="$md.seeded" "$GATE"
}

pass_case_a_wrapped_declaration_is_still_a_list() {
    # A third path pushes the array past rustfmt's width and the value moves to a line of its
    # own. That is a formatting event, and a predicate that read it as an empty list would go
    # red on `cargo fmt` — note **N60**'s cost, in a gate whose subject is a security boundary.
    # The seeded third entry is declared beside the route it would belong to, so this arm is the
    # legitimate shape of the *next* camera-bearing route rather than a doctored file.
    local tree
    tree="$(gate_scratch_tree)"
    sed -i 's|pub const CAMERA_BEARING_PATHS: \[&str; 2\] = \[rpc::RPC_PATH, preview::PREVIEW_PATH\];|pub const CAMERA_BEARING_PATHS: [\&str; 3] =\n    [rpc::RPC_PATH, preview::PREVIEW_PATH, preview::SNAPSHOT_PATH];|' \
        "$tree/crates/daemon/src/http/mod.rs"
    sed -i 's|^pub const CAMERA_QUERY_PARAM: &str = "camera";|pub const CAMERA_QUERY_PARAM: \&str = "camera";\n\n/// A third camera-bearing path, named on the list and gated with the rest.\npub const SNAPSHOT_PATH: \&str = "/snapshot";|' \
        "$tree/crates/daemon/src/http/preview.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}
