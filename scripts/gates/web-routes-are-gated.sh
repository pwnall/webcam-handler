#!/usr/bin/env bash
#
# Every route the web listener registers is a route the token gate is over, and the only thing
# `webcam-handler-daemon` serves without D11's bearer token is a lookup in the embedded asset
# table (D11 and its 2026-08-12 amendment, note **N82**, AGENTS.md "Hardware and privacy").
#
# ## This exists because a ruling took a property away
#
# Until 2026-08-12 the gate was `Router::layer` over one router, so "every route this listener
# serves is behind the token" was true **by construction**: a request could not reach a handler
# without meeting the gate, there was no list to keep, and note N75 argued the arrangement at
# length. The owner then ruled that the static assets are open-source code rather than a secret
# and are served without authentication, and that only the resources which carry or drive the
# camera stay gated. The gate is now `route_layer` over the routes, and which paths those are
# is a *decision* — `daemon::http::CAMERA_BEARING_PATHS` — where it used to be a shape.
#
# A decision can be forgotten. A route added by P5c, P6 or anybody else that carries camera
# data and is not on that list is a live camera served to strangers, and AGENTS rule 1 has no
# exemption for a defect class that arrives with a ruling: "every anticipated or discovered
# defect class becomes a lint, a CI job, or a test that can go red".
#
# ## Why this is a gate and not a test
#
# The suite drives every path on the list anonymously over a real socket and requires the
# refusal (`crates/daemon/tests/preview.rs`, `crates/daemon/tests/http.rs`), which is the
# strongest thing a test can say — and it can only say it about paths **somebody named**. A
# route added without being named is exactly the path no test knows to ask for, so the
# behavioural half is structurally unable to see the defect this predicate is about. That is
# `kill-is-never-a-fallback.sh`'s argument ("a suite can only witness the verbs it drives")
# about a different absence, and the answer has the same shape: a claim over the source, that
# the set of route registrations and the set of names on the list are the same set.
#
# ## What "carries or drives the camera" is, as something a predicate can decide
#
# It is not decidable from a handler — nothing here can read a function and know whether a
# camera is behind it. So the checkable predicate is the one that errs closed and happens to be
# exact today: **a route is gated; the only ungated answer is the asset fallback.** The only
# reason this listener has a route at all is the camera — one drives it (`/rpc`) and one carries
# it (`/preview`) — and everything else it serves is a file compiled into the binary. A future
# route with no camera in it would be red here, and that is the intent rather than a limitation:
# this is where somebody has to make the argument, in a diff, instead of in a router.
#
# ## The four claims
#
#   1. **The list exists, is not empty, and names constants rather than literals.**
#      `CAMERA_BEARING_PATHS`'s entries are read out of its declaration, and each must be a
#      named constant that is declared in the daemon's `http` tree. A `"/preview"` typed into
#      the list would be a second spelling of a path, and the pair that stops matching.
#   2. **Every route registration names one of them.** `.route(`, `.route_service(`, `.nest(`
#      and `.nest_service(` take a path; in this workspace's product code every one of them must
#      pass a constant on the list, from inside the daemon's `http` modules. A route on a string
#      literal, on a constant nobody listed, or in another crate is the defect this gate is for.
#   3. **The ungated half is the asset fallback and nothing else.** `.fallback(` and
#      `.fallback_service(` appear once, in the listener's composition, and name the asset
#      handler. A second fallback is a second answer served without the token, which is the same
#      defect arriving through the door claim 2 does not watch.
#   4. **The gate is still installed over the routes.** `gate::check` appears exactly once in
#      product code and in the composition module, and `route_layer(` appears exactly once and
#      in the same file. Every claim above is true of a tree where the gate was deleted or
#      widened back over everything, which is `kill-is-never-a-fallback.sh`'s "the only caller
#      went away" arm in both directions at once.
#
# ## What this does **not** claim
#
# **It checks shape, and shape is not behaviour.** It sees that one `route_layer(` and one
# `gate::check` are in the composition; it cannot see what that layer wraps, and a composition
# that registered a third route *after* the `route_layer` line would satisfy every claim here.
# What catches that is the other half of the pair — the suite drives the list over a real
# socket, and a route that is on the list and not behind the gate answers something other than
# `401` — so the two halves are only worth having together, and each says so in its own header.
#
# It is also a `grep`: a route registered through a helper of somebody else's, or a router built
# by a macro, is invisible to claim 2. What claim 2 rests on is that this workspace registers
# routes the way axum documents them, in three files a reviewer can hold in their head.
#
# ## The matching rule and the populations
#
# `lib.sh`'s `gate_product_lines` reads a file's product half, so **prose does not count** and
# **test code does not count** — the same decision `token-comparison-has-one-home.sh` makes and
# for the same reason: the modules this is about argue at length about routing and name the very
# spellings matched here, and a test may legitimately build a router of its own. A file whose
# product/test boundary cannot be read is a failure and not a pass.
#
# Populations are derived (docs/9's second structural rule): the daemon's directory comes from
# `cargo metadata`, the files from the tree walk, and the list's entries from its own
# declaration. Every name treated as *policy* — the constant, the composition module, the gate
# function, the asset handler — is asserted to exist before anything is counted, so a rename or
# a move fails this gate rather than quietly emptying it.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

# The policy this gate is about, named once. Every name is asserted to exist below.
list_name="CAMERA_BEARING_PATHS"
gate_function="gate::check"
gate_layer="route_layer"
asset_handler="asset"
# The axum verbs that register something at a path, and the two that register the answer for
# every path that has none.
path_verbs='route|route_service|nest|nest_service'
fallback_verbs='fallback|fallback_service'

daemon_dir="$(gate_metadata |
    jq -r '.packages[] | select(.name == "webcam-handler-daemon") | .manifest_path' |
    head -n1 | xargs -r dirname)"

if [[ -z "$daemon_dir" ]]; then
    gate_fail "webcam-handler-daemon is not a workspace member; the web listener, its routes and the list that says which of them are gated have no home"
    gate_finish
fi

# The metadata may describe a different checkout than the tree under test (the selftest feeds
# mutated copies), so paths are resolved as suffixes of the tree under test.
daemon_suffix="${daemon_dir#"$root"/}"
http_rel="$daemon_suffix/src/http"
list_rel="$http_rel/mod.rs"
composition_rel="$http_rel/listener.rs"
list_file="$root/$list_rel"
composition="$root/$composition_rel"

# ------------------------------------------------------------------ the named policy exists

anchors=0
for required in "$list_file" "$composition"; do
    anchors=$((anchors + 1))
    if [[ ! -f "$required" ]]; then
        gate_fail "${required#"$root"/} is missing; the list of gated paths and the composition that gates them are what every claim here is about, and this one is not in the tree"
    fi
done

if [[ ! -f "$list_file" || ! -f "$composition" ]]; then
    # Nothing below can say anything without both, and a partial verdict about a missing list
    # would read as a verdict about the routes.
    gate_checked "$anchors" "module(s) this gate's claims are about"
    gate_finish
fi

list_start="$(gate_test_region_start "$list_file")"
composition_start="$(gate_test_region_start "$composition")"
for readable in "$list_rel:$list_start" "$composition_rel:$composition_start"; do
    if ((${readable##*:} < 0)); then
        gate_fail "${readable%%:*} does not carry the one trailing \`#[cfg(test)] mod\` this gate reads a file's product half by; every count below would be a count of nothing in particular"
    fi
done

list_product="$(gate_product_lines "$list_file" "$list_start")"
composition_product="$(gate_product_lines "$composition" "$composition_start")"

anchors=$((anchors + 1))
if ! grep -Eq -- "pub const ${list_name}[[:space:]]*:" <<<"$list_product"; then
    gate_fail "$list_rel no longer declares \`pub const $list_name\`; that list is what claim 2 compares every route against, so a tree without it is a tree this gate has nothing to say about"
fi

gate_checked "$anchors" "named declaration(s) and module(s) asserted to exist before anything is counted"
gate_require_nonzero "$anchors" "named declarations"

# ------------------------------------------------------------------ claim 1: the list
#
# Read out of the declaration rather than transcribed here. The entries are paths spelled as
# `module::CONSTANT`, and what claim 2 matches on is the constant's own name, so a route
# registered as `.route(PREVIEW_PATH, …)` inside the module that declares it and a list entry
# written `preview::PREVIEW_PATH` are the same name.

#
# The declaration is read as **one line whatever rustfmt did to it**: a third entry pushes the
# array past the width and the value moves to a line of its own, which is a formatting event and
# must not be a finding (note N60 — a gate that cries wolf gets re-run until it agrees, and the
# run after that is the one where a real finding is waved through). `cases/` has a green arm for
# the wrapped shape.
declaration="$(sed -n "/pub const ${list_name}[[:space:]]*:/,/;/p" <<<"$list_product" | tr '\n' ' ')"
entries="$(sed -n 's/.*=[[:space:]]*\[\([^]]*\)\].*/\1/p' <<<"$declaration")"

declare -a listed=()
if [[ -n "${entries// /}" ]]; then
    while IFS= read -r entry; do
        entry="${entry//[[:space:]]/}"
        [[ -n "$entry" ]] || continue
        listed+=("${entry##*::}")
    done < <(tr ',' '\n' <<<"$entries")
fi

gate_checked "${#listed[@]}" "path(s) on \`$list_name\`, read out of its declaration"
# An empty list makes claim 2 impossible to satisfy rather than vacuous, but it is worth its own
# sentence: a tree that emptied it has not narrowed the gate, it has removed the subject.
gate_require_nonzero "${#listed[@]}" "paths on \`$list_name\`"

list_claims=0
for name in "${listed[@]}"; do
    list_claims=$((list_claims + 1))
    if [[ ! "$name" =~ ^[A-Z][A-Z0-9_]*$ ]]; then
        gate_fail "\`$list_name\` names \`$name\`, which is not a constant; a path written into this list as a literal is a second spelling of it, and the route it is meant to be about would keep answering while this list stopped naming it"
        continue
    fi
    if ! grep -Rqs -E -- "pub const ${name}[[:space:]]*:" "$root/$http_rel"; then
        gate_fail "\`$list_name\` names \`$name\` and nothing in $http_rel declares it; the list has an entry no route can be registered on"
    fi
done

gate_checked "$list_claims" "entry/entries of \`$list_name\` checked to be a declared constant"

# ------------------------------------------------------------------ claim 2 and claim 3
#
# The whole tree, not just the daemon: a crate that grew a router of its own would be the same
# defect one directory further out, and `dependency-walls.sh` is what stops it linking axum
# rather than anything here.

listed_alternation="$(
    IFS='|'
    printf '%s' "${listed[*]}"
)"

scanned=0
registrations=0
fallbacks=0

while IFS= read -r -d '' file; do
    scanned=$((scanned + 1))
    rel="${file#"$root"/}"

    start="$(gate_test_region_start "$file")"
    if ((start < 0)); then
        if grep -Eq -- "\.($path_verbs|$fallback_verbs)\(" "$file"; then
            gate_fail "$rel registers a route or a fallback and this gate cannot tell its product code from its test code: it carries more than one \`#[cfg(test)]\` marker, or its marker does not open a \`mod\`. A file nobody can classify is a finding rather than a pass"
        fi
        continue
    fi
    product="$(gate_product_lines "$file" "$start")"

    # ---------------------------------------------------------------- claim 2
    while IFS= read -r registration; do
        [[ -n "$registration" ]] || continue
        registrations=$((registrations + 1))
        argument="$(sed -E 's/^[^(]*\((.*)$/\1/; s/[,)].*$//; s/[[:space:]]//g' <<<"$registration")"
        if [[ -z "$argument" ]]; then
            gate_fail "$rel registers a route whose path this gate cannot read (\`$registration\`); an argument spelled across lines is a registration nobody here can compare against \`$list_name\`"
            continue
        fi
        if [[ "$rel" != "$http_rel"/* ]]; then
            gate_fail "$rel registers a route (\`$argument\`) outside $http_rel; the web listener's routes are composed in one place, and a router built anywhere else is one \`$list_name\` says nothing about"
            continue
        fi
        if [[ ! "${argument##*::}" =~ ^($listed_alternation)$ ]]; then
            gate_fail "$rel registers a route on \`$argument\`, which is not one of the paths \`$list_name\` names (${listed[*]}); since D11's 2026-08-12 amendment the token gate is over the routes that list names and over nothing else, so this route is served to anybody who asks"
        fi
    done < <(grep -Eo -- "\.($path_verbs)\([^;]*" <<<"$product" || true)

    # ---------------------------------------------------------------- claim 3
    while IFS= read -r registration; do
        [[ -n "$registration" ]] || continue
        fallbacks=$((fallbacks + 1))
        argument="$(sed -E 's/^[^(]*\((.*)$/\1/; s/[,)].*$//; s/[[:space:]]//g' <<<"$registration")"
        if [[ "$rel" != "$composition_rel" ]]; then
            gate_fail "$rel answers a request that matched no route (\`$registration\`); the fallback is what this listener serves **without** the token, and it has one home ($composition_rel) for the same reason the gate does"
        elif [[ "$argument" != "$asset_handler" ]]; then
            gate_fail "$composition_rel falls back to \`$argument\` rather than to \`$asset_handler\`; what is served without a credential is the embedded asset table, and a fallback that reaches anything else is a second ungated answer"
        fi
    done < <(grep -Eo -- "\.($fallback_verbs)\([^;]*" <<<"$product" || true)
done < <(gate_rust_files)

gate_checked "$scanned" "Rust source file(s) scanned for a route registration and a fallback"
gate_require_nonzero "$scanned" "Rust source files"
gate_checked "$registrations" "route registration(s) in product code, each checked against \`$list_name\`"
# Zero registrations is a listener that serves no routes at all, which is a tree where every
# claim above holds and the camera is unreachable — a finding either way, and never a pass.
gate_require_nonzero "$registrations" "route registrations"
gate_checked "$fallbacks" "fallback registration(s), checked to be the asset table's"
gate_require_nonzero "$fallbacks" "fallback registrations"

# ------------------------------------------------------------------ claim 4: it is still gated
#
# Every claim above is true of a tree whose composition stopped installing the gate, and true of
# one that put it back over everything — the first serves the camera to anybody, the second
# serves the assets to nobody, and the owner ruled on both.

installs="$(grep -Ec -- "$gate_function" <<<"$composition_product" || true)"
layers="$(grep -Ec -- "\b$gate_layer\(" <<<"$composition_product" || true)"

if ((installs != 1)); then
    gate_fail "$composition_rel's product code names \`$gate_function\` $installs time(s), not once; the composition installs the token gate exactly once, and a tree that installs it zero times serves a live camera to anybody who asks"
fi
if ((layers != 1)); then
    gate_fail "$composition_rel's product code calls \`$gate_layer(\` $layers time(s), not once; \`$gate_layer\` is what puts the gate over the routes and not over the asset fallback (D11's 2026-08-12 amendment), and \`Router::layer\` in its place would gate the client's own source code — which the owner ruled it must not"
fi

elsewhere=()
while IFS= read -r -d '' file; do
    rel="${file#"$root"/}"
    [[ "$rel" != "$composition_rel" ]] || continue
    start="$(gate_test_region_start "$file")"
    ((start >= 0)) || continue
    if grep -Fq -- "$gate_function" <<<"$(gate_product_lines "$file" "$start")"; then
        elsewhere+=("$rel")
    fi
done < <(gate_rust_files)

if ((${#elsewhere[@]} > 0)); then
    gate_fail "\`$gate_function\` is installed from ${elsewhere[*]} as well as from $composition_rel; which routes are behind D11's token is one decision in one \`match\`, and a route that installs its own gate is a second home for it (design §2.10)"
fi

gate_checked 2 "claim(s) that the composition still installs the gate over the routes and nothing else"
gate_finish
