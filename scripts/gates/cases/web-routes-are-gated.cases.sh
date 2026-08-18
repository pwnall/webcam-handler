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
# **P9b is the first time that defect class had a real route to be about**, and it gets a second
# arm beside the generic one: `fail_case_the_session_photo_route_left_the_list` seeds the list
# losing a route that exists, which is the same hole a rebase or a tidy-up reaches from the
# opposite side. D20 asked for both halves of the pair to go red on this class before
# `/session-photo` landed; this is the half that reads the source.
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
    # The path as the predicate echoes it back, which is what separates this arm from the one
    # below: both trip claim 2's comparison, and the only thing in the sentence that says which
    # spelling was seeded is the argument it quotes.
    # shellcheck disable=SC2016  # the predicate's own sentence, backticks and $defs and all, matched verbatim
    gate_red_because 'registers a route on `"/snapshot"`, which is not one of the paths' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_session_photo_route_left_the_list() {
    # **The same hole, reached from the other direction, and about a route that really exists.**
    # `fail_case_a_new_route_module_nobody_put_on_the_list` seeds a route somebody forgot to add;
    # this seeds the list forgetting a route that is already there — a rebase that dropped a line,
    # a "tidy the array" edit, a merge that took the wrong side. The route keeps answering, the
    # `route_layer` keeps covering exactly the paths named beside it, and the calibration samples
    # a sweep photographed go out to anybody who asks.
    #
    # `/session-photo` rather than `/preview` because it is the entry the list *gained* (D20,
    # P9b), which makes it the one a future edit is most likely to treat as removable — and D20
    # asked for this class to have a live arm before the route landed.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed '/^    session_photo::SESSION_PHOTO_PATH,$/d' "$tree/crates/daemon/src/http/mod.rs"
    gate_seed 's/pub const CAMERA_BEARING_PATHS: \[&str; 3\]/pub const CAMERA_BEARING_PATHS: [\&str; 2]/' \
        "$tree/crates/daemon/src/http/mod.rs"
    # shellcheck disable=SC2016  # the predicate's own sentence, backticks and $defs and all, matched verbatim
    gate_red_because 'registers a route on `SESSION_PHOTO_PATH`, which is not one of the paths' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_session_photo_route_answers_a_method_this_gate_cannot_place() {
    # Claim 2's method half at the newest camera-bearing route, and it is here rather than only
    # at `/preview` because the two routes' `HEAD`s were added for the same reason and by the
    # same argument (note **N179**): a `HEAD` that fell through to the `GET` slot would do the
    # whole job — there a camera opened, here a session tree walked and a photograph read — for a
    # request that asked for no bytes. A spelling this gate cannot place is a method nobody is
    # counting, on the route that hands back stored frames.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/\.route(SESSION_PHOTO_PATH, get(sample)\.head(described))/.route(SESSION_PHOTO_PATH, get(sample).also_answer(described))/' \
        "$tree/crates/daemon/src/http/session_photo.rs"
    # shellcheck disable=SC2016  # the predicate's own message, backticks and all, matched verbatim
    gate_red_because 'which is not one of `axum::routing`' env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_a_new_route_on_a_constant_the_list_does_not_name() {
    # The same defect written tidily. A `pub const SNAPSHOT_PATH` beside the route reads like
    # every other path in this daemon and is still a path the gate is not over, so the claim is
    # about the list rather than about literals.
    local tree
    tree="$(gate_scratch_tree)"
    seed_route_module "$tree" SNAPSHOT_PATH crates/daemon/src/http/snapshot.rs
    # shellcheck disable=SC2016  # the predicate's own sentence, backticks and $defs and all, matched verbatim
    gate_red_because 'registers a route on `SNAPSHOT_PATH`, which is not one of the paths' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_route_registered_outside_the_listener_module() {
    # A router built in another crate — reachable only if something serves it, which is exactly
    # the edit that would follow. The composition has one home (design §2.10) and this is the
    # arm that says so.
    local tree
    tree="$(gate_scratch_tree)"
    seed_route_module "$tree" '"/snapshot"' crates/engine/src/snapshot_route.rs
    # shellcheck disable=SC2016  # the predicate's own sentence, backticks and $defs and all, matched verbatim
    gate_red_because 'registers a route (`"/snapshot"`) outside crates/daemon/src/http' \
        env WCH_GATE_ROOT="$tree" "$GATE"
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
    gate_red_because 'registers a route or a fallback and this gate cannot tell its product code from its test code' \
        env WCH_GATE_ROOT="$tree" "$GATE"
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
    # shellcheck disable=SC2016  # the predicate's own sentence, backticks and $defs and all, matched verbatim
    gate_red_because 'answers a request that matched no route (`.fallback(snapshot)`)' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_fallback_stops_being_the_asset_table() {
    # The same claim from the other side: the one fallback is still one, and it now reaches
    # something other than the embedded assets — which is a handler served to anybody.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/\.fallback(asset)/.fallback(anything_at_all)/' \
        "$tree/crates/daemon/src/http/listener.rs"
    # shellcheck disable=SC2016  # the predicate's own sentence, backticks and $defs and all, matched verbatim
    gate_red_because 'falls back to `anything_at_all` rather than to `asset`' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_gate_went_back_over_everything() {
    # `Router::layer` in place of `route_layer` gates the client's own source code, which is
    # what the owner ruled it must not: the modules 401, the page does not load, and every
    # camera-bearing route is still gated so the suite's own refusal assertions stay green.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/routes\.route_layer(/routes.layer(/' "$tree/crates/daemon/src/http/listener.rs"
    # The `route_layer` sentence and **not** the `gate::check` one: this seed leaves the gate
    # installed exactly once and only changes what it is installed over, which is the whole
    # difference between it and the arm below — a tree that 401s the client's own source code
    # against one that serves the camera to anybody.
    # shellcheck disable=SC2016  # the predicate's own sentence, backticks and $defs and all, matched verbatim
    gate_red_because 'calls `route_layer(` 0 time(s), not once' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_gate_is_no_longer_installed_at_all() {
    # Every other claim in this predicate is true of a tree that stopped gating anything —
    # `kill-is-never-a-fallback.sh`'s "the only caller went away" arm, about the one absence
    # that matters most here.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/routes\.route_layer(axum::middleware::from_fn_with_state(token, gate::check))/routes/' \
        "$tree/crates/daemon/src/http/listener.rs"
    # shellcheck disable=SC2016  # the predicate's own sentence, backticks and $defs and all, matched verbatim
    gate_red_because 'names `gate::check` 0 time(s), not once' \
        env WCH_GATE_ROOT="$tree" "$GATE"
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
    gate_red_because 'is installed from crates/daemon/src/http/scratch.rs as well as from' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_list_is_empty() {
    # Not a narrower gate: a gate with no subject. Every route registration would then be a
    # route on a path the list does not name, and the count this predicate requires to be
    # non-zero says so before the comparison does.
    local tree
    tree="$(gate_scratch_tree)"
    # A **range** seed, because rustfmt puts one entry per line at three of them and `sed` is
    # line-based: `/…/,/;/c\` replaces the whole declaration however it is wrapped, which is
    # what stops this arm going quiet the next time the array grows (note **N186**).
    gate_seed '/pub const CAMERA_BEARING_PATHS/,/^];$/c\pub const CAMERA_BEARING_PATHS: [\&str; 0] = [];' \
        "$tree/crates/daemon/src/http/mod.rs"
    # shellcheck disable=SC2016  # the predicate's own sentence, backticks and $defs and all, matched verbatim
    gate_red_because 'examined zero paths on `CAMERA_BEARING_PATHS`' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_list_names_a_literal_instead_of_the_constant() {
    # A path written twice is a path that can stop matching in one of the two places, and the
    # place it would stop matching is the router — leaving a list that still reads correctly
    # over a route nothing gates.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed '/pub const CAMERA_BEARING_PATHS/,/^];$/c\pub const CAMERA_BEARING_PATHS: [\&str; 3] = ["/rpc", "/preview", "/session-photo"];' \
        "$tree/crates/daemon/src/http/mod.rs"
    # shellcheck disable=SC2016  # the predicate's own sentence, backticks and $defs and all, matched verbatim
    gate_red_because 'names `"/rpc"`, which is not a constant' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_list_names_a_path_nothing_declares() {
    # The list is the population every other claim quantifies over, so an entry that resolves
    # to nothing is a gate over a path that does not exist — and a route that does exist with
    # nothing naming it.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/session_photo::SESSION_PHOTO_PATH,/session_photo::SNAPSHOT_PATH,/' \
        "$tree/crates/daemon/src/http/mod.rs"
    # shellcheck disable=SC2016  # the predicate's own sentence, backticks and $defs and all, matched verbatim
    gate_red_because 'names `SNAPSHOT_PATH` and nothing in crates/daemon/src/http declares it' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_list_declaration_is_gone() {
    # A rename, which is the ordinary way a named policy stops being the one a gate reads.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/pub const CAMERA_BEARING_PATHS/pub const GATED_PATHS/' \
        "$tree/crates/daemon/src/http/mod.rs"
    # shellcheck disable=SC2016  # the predicate's own sentence, backticks and $defs and all, matched verbatim
    gate_red_because 'no longer declares `pub const CAMERA_BEARING_PATHS`' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_composition_module_is_missing() {
    # The file every claim about the gate is about. Its absence must not read as compliance.
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$tree/crates/daemon/src/http/listener.rs"
    gate_red_because 'crates/daemon/src/http/listener.rs is missing; the list of gated paths and the composition that gates them' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_no_route_is_registered_at_all() {
    # A listener with no routes serves no camera, which is a different defect and equally not
    # a pass: the population every comparison here rests on would be empty.
    local tree
    tree="$(gate_scratch_tree)"
    # The method routers are matched loosely — `get(stream).head(described)` is what the
    # preview registers since G6/M23 — because what this arm removes is the `.route(` and not
    # what was behind it. Seeding through `gate_seed` is what makes the looseness safe: a seed
    # that stopped matching would leave both routes registered and report the gate green for the
    # one reason this file exists to refuse, and the helper says so about the *seed* rather than
    # letting the harness say it about the predicate (note **N186**).
    gate_seed 's/\.route(RPC_PATH,[^;]*)/.with_state(())/' \
        "$tree/crates/daemon/src/http/rpc.rs"
    gate_seed 's/\.route(PREVIEW_PATH,[^;]*)/.with_state(())/' \
        "$tree/crates/daemon/src/http/preview.rs"
    gate_seed 's/\.route(SESSION_PHOTO_PATH,[^;]*)/.with_state(())/' \
        "$tree/crates/daemon/src/http/session_photo.rs"
    # The registrations, not the methods below them: both vacuity branches fire when the last
    # `.route(` goes, and this arm's name is about the routes.
    gate_red_because 'examined zero route registrations' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_camera_bearing_route_answers_a_method_this_gate_cannot_place() {
    # Claim 2's method half (note **N185**). The path is right, the list is right, the gate is
    # installed — and the route answers a second method through a spelling that is not one of
    # `axum::routing`'s constructors, so the count this gate reports would be a count of
    # something else. Until G6 the predicate read the path argument and threw the rest away,
    # which is how `get(stream)` became `get(stream).head(described)` on the one route that
    # carries camera frames with neither half of the pair able to see it.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/\.route(PREVIEW_PATH, get(stream)\.head(described))/.route(PREVIEW_PATH, get(stream).also_answer(described))/' \
        "$tree/crates/daemon/src/http/preview.rs"
    # shellcheck disable=SC2016  # the predicate's own message, backticks and all, matched verbatim
    gate_red_because 'which is not one of `axum::routing`' env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_a_camera_bearing_route_has_a_method_router_this_gate_cannot_read() {
    # The other direction of the same claim, and the price `gate_test_region_start` charges
    # everywhere else in this suite: a boundary with no answer is a failure and not a pass.
    #
    # The seeded shape is an ordinary refactor — the method router bound to a name above the
    # registration — and not a doctored file, which is what makes it the right subject. The
    # *path* is still perfectly readable there, so claim 2's existing arms all stay green; what
    # is gone is any way to say which methods that route answers, and a route whose methods are
    # whatever the next reader assumes is one nobody is counting.
    local tree file
    tree="$(gate_scratch_tree)"
    file="$tree/crates/daemon/src/http/preview.rs"
    gate_seed 's/^    Router::new()$/    let camera_methods = get(stream).head(described);\n    Router::new()/' "$file"
    gate_seed 's/\.route(PREVIEW_PATH, get(stream)\.head(described))/.route(PREVIEW_PATH, camera_methods)/' "$file"
    gate_red_because 'cannot read the methods it answers' env "WCH_GATE_ROOT=$tree" "$GATE"
}

# The green direction for the method half: a third method on a camera-bearing route is a
# legitimate diff — `route_layer` wraps the whole `MethodRouter`, so it is gated by existing —
# and this predicate's job is to *count* it, not to refuse it. An arm that could not tell the
# two apart would be a gate somebody turns off the first time a route grows an `OPTIONS`.
pass_case_a_camera_bearing_route_may_grow_a_third_method() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/\.route(PREVIEW_PATH, get(stream)\.head(described))/.route(PREVIEW_PATH, get(stream).head(described).options(described))/' \
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
    gate_red_because 'webcam-handler-daemon is not a workspace member; the web listener, its routes and the list that says which of them are gated have no home' \
        env WCH_GATE_METADATA="$md.seeded" "$GATE"
}

pass_case_a_wrapped_declaration_is_still_a_list() {
    # The array is past rustfmt's width and the value is no longer on the `pub const` line — as
    # of P9b's third entry that is the *shipped* shape, and this arm now seeds one wrap further
    # still. Either way it is a formatting event, and a predicate that read it as an empty list
    # would go red on `cargo fmt` — note **N60**'s cost, in a gate whose subject is a security
    # boundary. The seeded fourth entry is declared beside the route it would belong to, so this
    # arm is the legitimate shape of the *next* camera-bearing route rather than a doctored file.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed '/pub const CAMERA_BEARING_PATHS/,/^];$/c\pub const CAMERA_BEARING_PATHS: [\&str; 4] = [rpc::RPC_PATH, preview::PREVIEW_PATH,\n    session_photo::SESSION_PHOTO_PATH, preview::SNAPSHOT_PATH];' \
        "$tree/crates/daemon/src/http/mod.rs"
    gate_seed 's|^pub const CAMERA_QUERY_PARAM: &str = "camera";|pub const CAMERA_QUERY_PARAM: \&str = "camera";\n\n/// A fourth camera-bearing path, named on the list and gated with the rest.\npub const SNAPSHOT_PATH: \&str = "/snapshot";|' \
        "$tree/crates/daemon/src/http/preview.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}
