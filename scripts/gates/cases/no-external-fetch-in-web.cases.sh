# Both-direction cases for `no-external-fetch-in-web.sh`.
#
# One failing arm per door the predicate closes, plus the green arm that proves a page
# built entirely from relative paths — which is what the real client will be — passes. The
# anchor in the green arm is deliberate: a link the user may click is not a subresource
# the page loads, and a gate that cannot tell them apart gets disabled.
#
# The web crate has no assets at P0, so every arm here seeds its own.
#
# shellcheck shell=bash

_web_tree() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/crates/web/assets"
    printf '%s\n' "$tree"
}

pass_case() {
    "$GATE"
}

pass_case_relative_assets_only() {
    local tree
    tree="$(_web_tree)"
    cat >"$tree/crates/web/assets/index.html" <<'HTML'
<!doctype html>
<link rel="stylesheet" href="/app.css">
<script type="module" src="/app.js"></script>
<a href="https://github.com/pwnall/webcam-handler">source</a>
HTML
    cat >"$tree/crates/web/assets/app.js" <<'JS'
import { render } from "./render.js";
const cameras = await fetch("/rpc", { method: "POST" });
const live = new WebSocket(`ws://${location.host}/ws`);
render(cameras, live);
JS
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_script_from_a_cdn() {
    local tree
    tree="$(_web_tree)"
    printf '%s\n' '<script src="https://cdn.example.com/chart.js"></script>' \
        >"$tree/crates/web/assets/index.html"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_stylesheet_from_a_cdn() {
    local tree
    tree="$(_web_tree)"
    printf '%s\n' '<link rel="stylesheet" href="//fonts.example.com/x.css">' \
        >"$tree/crates/web/assets/index.html"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_fetch_to_another_origin() {
    local tree
    tree="$(_web_tree)"
    printf '%s\n' 'fetch("https://telemetry.example.com/collect", { method: "POST" });' \
        >"$tree/crates/web/assets/app.js"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_module_import_from_a_cdn() {
    local tree
    tree="$(_web_tree)"
    printf '%s\n' 'import { h } from "https://esm.example.com/preact";' \
        >"$tree/crates/web/assets/app.js"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_websocket_to_another_origin() {
    local tree
    tree="$(_web_tree)"
    printf '%s\n' 'const s = new WebSocket("wss://relay.example.com/ws");' \
        >"$tree/crates/web/assets/app.js"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_css_pulls_a_webfont() {
    local tree
    tree="$(_web_tree)"
    printf '%s\n' '@import url(https://fonts.example.com/inter.css);' \
        >"$tree/crates/web/assets/app.css"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_web_crate_left_the_workspace() {
    local md
    md="$(gate_metadata_snapshot)"
    jq 'del(.packages[] | select(.name == "webcam-handler-web"))' "$md" >"$md.seeded"
    WCH_GATE_METADATA="$md.seeded" "$GATE"
}
