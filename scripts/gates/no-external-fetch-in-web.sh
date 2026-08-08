#!/usr/bin/env bash
#
# The web client vendors or hand-writes everything: no CDN, no npm, no build step
# (design §2.7, AGENTS.md "Docs and dependencies"). A camera control panel that loads a
# script from someone else's origin gives that origin the page — and this page has a live
# camera in it.
#
# The asset directory is the `webcam-handler-web` package's own directory, derived from
# `cargo metadata`, so the gate follows the crate if it moves.
#
# Anchors (`<a href="https://…">`) are deliberately *not* violations: a link the user may
# click is not a subresource the page loads. Everything that fetches without being asked
# is below.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

web_dir="$(gate_metadata |
    jq -r '.packages[] | select(.name == "webcam-handler-web") | .manifest_path' |
    head -n1 | xargs -r dirname)"

if [[ -z "$web_dir" ]]; then
    gate_fail "webcam-handler-web is not a workspace member; the asset rules have no subject"
    gate_finish
fi

web_suffix="${web_dir#"$root"/}"
scan_dir="$root/$web_suffix"

# An absolute or protocol-relative URL whose host is written out: `https://cdn…`,
# `ws://relay…`, `//cdn…`. The trailing host character is load-bearing — it is what tells
# a hard-coded foreign origin apart from `ws://${location.host}/ws`, which is the page's
# own origin computed at runtime and is exactly how the real client will open its socket.
external='([a-zA-Z][a-zA-Z0-9+.-]*:)?//[A-Za-z0-9._-]'

# Each rule is a name and the pattern that catches it. Named so a failure says which
# door was left open, not just that one was.
rules=(
    "script-src	<script[^>]+src[[:space:]]*=[[:space:]]*[\"']?$external"
    "link-href	<link[^>]+href[[:space:]]*=[[:space:]]*[\"']?$external"
    "media-src	<(img|iframe|video|audio|source|track|embed|object)[^>]+(src|data)[[:space:]]*=[[:space:]]*[\"']?$external"
    "fetch	fetch\\([[:space:]]*[\"'\`]$external"
    "module-import	(^|[[:space:]])(import|from)[[:space:]]*\\(?[[:space:]]*[\"'\`]$external"
    "worker-import	importScripts\\([[:space:]]*[\"'\`]$external"
    "socket	new[[:space:]]+(WebSocket|EventSource)\\([[:space:]]*[\"'\`]$external"
    "css-import	@import[[:space:]]+(url\\()?[[:space:]]*[\"']?$external"
    "css-url	url\\([[:space:]]*[\"']?$external"
    "xhr-open	\\.open\\([[:space:]]*[\"'][A-Z]+[\"'][[:space:]]*,[[:space:]]*[\"'\`]$external"
)

assets=0
while IFS= read -r -d '' file; do
    assets=$((assets + 1))
    rel="${file#"$root"/}"
    for rule in "${rules[@]}"; do
        name="${rule%%	*}"
        pattern="${rule#*	}"
        if grep -Eqi -- "$pattern" "$file"; then
            gate_fail "$rel reaches off-origin ($name); the web client vendors everything it loads"
        fi
    done
done < <(gate_find "$scan_dir" \
    \( -name '*.html' -o -name '*.htm' -o -name '*.css' -o -name '*.js' \
    -o -name '*.mjs' -o -name '*.svg' -o -name '*.webmanifest' \))

gate_checked "${#rules[@]}" "off-origin patterns"
gate_require_nonzero "${#rules[@]}" "off-origin patterns"

if ((assets == 0)); then
    # docs/4 Part 2 commissions the arm that makes an empty asset directory a failure —
    # it lands at P5 with the assets themselves. Until then the emptiness is printed and
    # counted, so nobody reads this gate's green as "the assets were checked".
    gate_skip 0 "web asset files under $web_suffix/ — the client's HTML/CSS/JS lands at P5, and docs/4 Part 2 commissions the non-vacuity arm with it"
else
    gate_checked "$((assets * ${#rules[@]}))" "(asset file, off-origin pattern) pairs across $assets asset file(s)"
fi

gate_finish
