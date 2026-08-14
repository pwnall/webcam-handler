# Both-direction cases for `no-external-fetch-in-web.sh`.
#
# One failing arm per door the predicate closes, plus the green arm that proves a page
# built entirely from relative paths — which is what the real client will be — passes. The
# anchor in the green arm is deliberate: a link the user may click is not a subresource
# the page loads, and a gate that cannot tell them apart gets disabled.
#
# Since P5a the crate ships an asset of its own (`crates/web/assets/index.html`), so each arm
# below seeds *beside* it rather than into an empty directory. That is worth stating because it
# changes what the failing arms prove: a seeded `app.js` is now the second file in the walk and
# the gate is red because of that one file, not because the walk had exactly one thing in it —
# the arms that overwrite `index.html` and the arms that add a file are both red for the same
# per-file reason. The last two arms are the other side of the same landing: with the
# non-vacuity arm in the predicate, an empty directory and a missing one are failures, and
# they are the shapes that would otherwise make every rule above quantify over nothing.
#
# ## Every arm names its rule, and the P5 review is why
#
# docs/9 Part 2 says "one seeded violation per pattern", and three patterns had none —
# `media-src`, `worker-import` and `xhr-open` were carried by a `selftest.sh` that requires arms
# per *predicate* rather than per claim, so their absence was invisible to the harness that
# exists to notice exactly that. They have arms now, and so do the two rules the review added.
#
# The other half is `css-url`, which had an arm it could not be told apart by: the seeded
# `@import url(https://…)` is red under `css-import` **and** under `css-url`, and an arm reading
# only the exit status cannot say which — so either rule could have rotted to unreachable with
# its arm still comfortably non-zero. Every arm below therefore asserts the rule name in the
# failure message through `gate_red_because` (`lib.sh`), and `css-url` gets a seed no `@import`
# appears in.
#
# shellcheck shell=bash

_web_tree() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/crates/web/assets"
    printf '%s\n' "$tree"
}

# Seed one line into one asset file and require the predicate to go red naming `$rule`.
#
#   $1  the rule name the predicate must print
#   $2  the asset file, relative to the asset directory
#   $3  the line to seed
_seeded_asset_is_red_as() {
    local rule="$1" file="$2" line="$3" tree
    tree="$(_web_tree)"
    printf '%s\n' "$line" >"$tree/crates/web/assets/$file"
    gate_red_because "($rule)" env "WCH_GATE_ROOT=$tree" "$GATE"
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
const frame = el("img", { src: `/preview?camera=${name}` });
frame.src = previewUrl(name, token);
frame.setAttribute("src", "/preview?camera=" + name);
render(cameras, live, frame);
JS
    WCH_GATE_ROOT="$tree" "$GATE"
}

# ------------------------------------------------- the markup rules

fail_case_script_from_a_cdn() {
    _seeded_asset_is_red_as script-src index.html \
        '<script src="https://cdn.example.com/chart.js"></script>'
}

fail_case_stylesheet_from_a_cdn() {
    _seeded_asset_is_red_as link-href index.html \
        '<link rel="stylesheet" href="//fonts.example.com/x.css">'
}

# One of the three docs/9's "one seeded violation per pattern" asked for and nothing had.
# `<img>` is the tag that matters here: this client's one real subresource is a preview image,
# and an `<img>` pointed at somebody else's origin hands that origin the page's referrer, its
# cookies and the fact that a camera is being watched.
fail_case_an_image_from_another_origin() {
    _seeded_asset_is_red_as media-src index.html \
        '<img alt="chart" src="https://cdn.example.com/x.jpg">'
}

# ------------------------------------------------- the JavaScript verbs

fail_case_fetch_to_another_origin() {
    _seeded_asset_is_red_as fetch app.js \
        'fetch("https://telemetry.example.com/collect", { method: "POST" });'
}

fail_case_module_import_from_a_cdn() {
    _seeded_asset_is_red_as module-import app.js \
        'import { h } from "https://esm.example.com/preact";'
}

# The second of the three that had no arm. A worker is the one place in a page where an import
# is spelled as a function call, so `module-import` does not see it.
fail_case_a_worker_imports_from_a_cdn() {
    _seeded_asset_is_red_as worker-import worker.js \
        'importScripts("https://cdn.example.com/worker-helper.js");'
}

fail_case_websocket_to_another_origin() {
    _seeded_asset_is_red_as socket app.js \
        'const s = new WebSocket("wss://relay.example.com/ws");'
}

# The third. `XMLHttpRequest` is what a vendored third-party snippet uses, which is exactly the
# code nobody in this repository would have written and everybody would have pasted.
fail_case_an_xhr_to_another_origin() {
    _seeded_asset_is_red_as xhr-open app.js \
        'const x = new XMLHttpRequest(); x.open("GET", "https://telemetry.example.com/beacon");'
}

# ------------------------------------------------- the stylesheet rules

fail_case_css_pulls_a_webfont() {
    _seeded_asset_is_red_as css-import app.css \
        '@import url(https://fonts.example.com/inter.css);'
}

# `css-url` with no `@import` anywhere near it. The arm above is red under both rules — an
# `@import url(…)` satisfies each of them — so until this seed existed either rule could have
# stopped matching with the harness none the wiser, which is note **N10**'s family measured in a
# gate's own case file.
fail_case_css_pulls_a_background_image() {
    _seeded_asset_is_red_as css-url app.css \
        '.badge { background: url(https://cdn.example.com/dot.png) no-repeat; }'
}

# ------------------------------------------------- the two rules the P5 review added

# **The idiom this client actually uses.** `crates/web/assets/preview.js` writes `frame.src =
# previewUrl(camera, token)`, and an off-origin literal in that position was measured green on
# all ten of the rules that preceded this arm.
fail_case_an_element_property_is_pointed_off_origin() {
    _seeded_asset_is_red_as element-src preview.js \
        'frame.src = "https://cdn.example.com/x.jpg";'
}

# The same assignment written as markup this client builds through `dom.js`'s `el(tag, attrs)`,
# which is where an attribute is a key in an object rather than a property on a node.
fail_case_an_attribute_object_carries_an_off_origin_url() {
    _seeded_asset_is_red_as element-src dom.js \
        'const node = el("img", { src: "https://cdn.example.com/x.jpg" });'
}

fail_case_set_attribute_points_off_origin() {
    _seeded_asset_is_red_as set-attribute dom.js \
        'node.setAttribute("src", "https://cdn.example.com/x.jpg");'
}

# ------------------------------------------------- the population itself

# docs/9 Part 2's non-vacuity row, in the shape it was commissioned for: the client's files
# are deleted and the twelve rules above have nothing left to be true about. A gate that answered
# PASS here would be reporting on a page nobody wrote.
fail_case_the_asset_directory_is_empty() {
    local tree
    tree="$(_web_tree)"
    find "$tree/crates/web/assets" -mindepth 1 -delete
    gate_red_because 'examined zero web asset files' env "WCH_GATE_ROOT=$tree" "$GATE"
}

# The same emptiness wearing the other hat. `gate_find` returns nothing for a directory that
# is not there, so a crate that lost `assets/` outright reads exactly like a crate whose assets
# are all clean — which is the one reading this must not have.
fail_case_the_asset_directory_is_gone() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -rf "$tree/crates/web/assets"
    gate_red_because 'examined zero web asset files' env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_web_crate_left_the_workspace() {
    local md
    md="$(gate_metadata_snapshot)"
    jq 'del(.packages[] | select(.name == "webcam-handler-web"))' "$md" >"$md.seeded"
    gate_red_because 'webcam-handler-web is not a workspace member' \
        env "WCH_GATE_METADATA=$md.seeded" "$GATE"
}
