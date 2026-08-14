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
#
# ## The two rules the P5 review added, and the idiom that got past the first ten
#
# The original ten match HTML **attributes** and JavaScript **verbs** — a `<script src>`, a
# `fetch(`, an `import`, a `new WebSocket(`. What this client actually does with its one real
# subresource is neither: `crates/web/assets/preview.js` writes `frame.src = previewUrl(camera,
# token)`, an **off-origin URL assigned to an element's property**, and that shape was measured
# to fire no rule at all. `frame.src = "https://cdn.example.com/x.jpg"` and `el("img", { src:
# "https://…" })` were both green on the ten, which is the gate being silent about the exact
# line the client's only subresource is loaded by.
#
# So `element-src` covers a subresource attribute written as a property assignment (`.src =`) or
# as an entry in an attribute object (`src:` — `dom.js`'s `el(tag, attrs)` is how this client
# builds markup), and `set-attribute` covers the same names reached through `setAttribute`.
#
# **`href` is deliberately not among those names**, and the reason is the anchor paragraph above
# rather than an oversight: `el("a", { href: "https://…" })` is how a page builds the link the
# header already permits, and a `grep` cannot see which element a property is being set on. What
# that costs is a `<link rel=stylesheet>` whose `href` is assigned in JavaScript — the markup
# form is `link-href`'s and stays covered, the scripted form is not, and it is written here
# rather than left for somebody to discover. The alternative was a rule that contradicts this
# file's own stated allowance, which is how a gate stops being believed.
#
# **An asset directory with nothing in it is a failure** (docs/9 Part 2's non-vacuity row,
# commissioned at P5a). Every rule below quantifies over the files this walk finds, so a walk
# that finds none makes all twelve of them vacuously true — the greenest this gate can be is the
# state where it is checking nothing at all. That arm could not land before the assets did:
# until P5a the emptiness was the honest answer and it was printed as a named, counted skip.
# `crates/web/assets/index.html` is what makes the stronger claim assertable, and the skip is
# gone with it.
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

# The subresource-bearing attribute names the two JavaScript rules below are about. `href` is
# not on the list and the header says why; every name here names bytes a browser fetches without
# being asked.
subresource_attributes='src|srcset|poster|data|action|formaction'

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
    "element-src	(\\.($subresource_attributes)[[:space:]]*=|(^|[^A-Za-z0-9_.-])($subresource_attributes)[[:space:]]*:)[[:space:]]*[\"'\`]$external"
    "set-attribute	\\.setAttribute\\([[:space:]]*[\"'\`]($subresource_attributes)[\"'\`][[:space:]]*,[[:space:]]*[\"'\`]$external"
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

gate_checked "$((assets * ${#rules[@]}))" "(asset file, off-origin pattern) pairs across $assets asset file(s) under $web_suffix/"

# The non-vacuity arm this gate's header argues for. A missing directory reaches here the same
# way an empty one does — `gate_find` walks what is there and says nothing about what is not —
# and both answers are the same defect: a client whose files this gate never opened.
gate_require_nonzero "$assets" "web asset files under $web_suffix/"

gate_finish
