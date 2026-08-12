#!/usr/bin/env bash
#
# The T6 purity walls of design §2.8, as linkage facts:
#
#   * `schema`, `imaging`, `fake`, `cli-core` and `engine` link no tokio/axum/hyper. They
#     are the crates a test can run without a runtime, and that property is structural
#     rather than aspirational only if something checks it.
#
#     `engine` joined that list at P4b, when the camera actor landed there rather than in
#     the daemon. Design §2.1 puts it there — "the daemon's async tasks and the direct CLI
#     both talk to the same actor API" — and §2.8 gives the engine no runtime, which is
#     precisely why `engine::actor` names no reply channel of its own and hands the caller
#     a closure to build one with (note N41): a `tokio::sync::oneshot` in the signature
#     would force a runtime on `wch`. That argument stops being true the moment the engine
#     links tokio, and nothing about `engine::actor` would have to change for it to happen,
#     so it is a linkage fact here instead of a paragraph in a doc comment.
#   * `api` links no *web* stack. It is exempt from the tokio half, and only that half:
#     `#[rpc(server, client)]` cannot expand without `jsonrpsee-core`, which activates
#     tokio in both its `client` and `server` features. Measured, not assumed — see note
#     N5. The half that protects "only `daemon` links the web stack" still applies to it.
#   * Only the two composition roots — `webcam-handler-cli` and `webcam-handler-daemon` —
#     link a backend. The engine consumes `Box<dyn CameraBackend>` values the roots
#     construct, so "adding a backend needs no engine edit" (§2.11) is a fact about the
#     graph, not a promise.
#   * `webcam-handler-client` links no backend and no engine: the thin-client wall.
#   * `webcam-handler-web` links **neither** stack — no tokio, no axum, no hyper, no
#     tower-http. It is an asset crate: `rust-embed` and nothing else, which is why it is its
#     own row rather than a name added to one of the lists above (the pure wall does not cover
#     `tower-http`, and the wire wall allows tokio for a reason — note N5 — that has nothing to
#     say about this crate).
#
#     The wall is design §2.10's split made structural. "What these files are" is that crate's
#     law and "how bytes become an HTTP response" is the daemon's, and the crate's own header
#     says so; its manifest turns the same argument down at the feature level, declining
#     rust-embed's `axum-ex` integration precisely because "the axum one would give this crate
#     its own axum and tokio edges, and serving is `webcam-handler-daemon`'s half of the seam".
#     That is an argument in a comment until something can go red on it. What the wall buys is
#     concrete: `crates/web` stays a crate a test can read without a runtime, and the daemon
#     stays the one place a request is turned into a response — the arrangement that let
#     `daemon::http::listener` answer every path from one `fallback` over `web::get`, with no
#     route table in either crate.
#
#     **This wall is not in design §2.8's list**, which names the pure crates, the wire crate,
#     the composition roots and the thin client, and was written when `web` had no dependency
#     of its own to lose. The doc reconciliation is the next piece's; the gate is here because
#     the edge landed here (AGENTS rule 1: a fix lands with its gate).
#   * No V4L2 path escapes the V4L2 backend's own directory.
#
# Every population is computed: workspace membership, the normal-dependency closure of
# each member, and which crates are backends (any workspace member under
# `crates/backends/`) all come from `cargo metadata` and the tree. What cannot be
# computed is the *policy* — which role each crate plays is a design decision, not a
# fact — so the roles are named below and every named package is asserted to exist. A
# rename or a deletion turns the rule red instead of silently unenforced.
#
# Only *normal* dependency edges count. A dev-dependency does not ship (that wall is
# `testkit-is-dev-only.sh`) and a build-dependency does not link into the artifact.
#
# Honest limit, restated from docs/9: this predicate reads declared edges and greps
# source. A backend leaking V4L2 semantics through stringly-typed values rather than
# types is invisible to it, and the *behavioral* halves of T6 (which crate touches the
# filesystem, `/dev`, the state dir) are review-held.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

findings="$(gate_metadata | jq -r \
    --argjson pure '["webcam-handler-schema","webcam-handler-imaging","webcam-handler-fake","webcam-handler-cli-core","webcam-handler-engine"]' \
    --argjson async_stack '["tokio","axum","hyper"]' \
    --argjson wire '["webcam-handler-api"]' \
    --argjson web_stack '["axum","hyper","tower-http"]' \
    --argjson assets '["webcam-handler-web"]' \
    --argjson roots '["webcam-handler-cli","webcam-handler-daemon"]' \
    --arg client "webcam-handler-client" \
    --arg engine "webcam-handler-engine" '
    def reach($edges; $from):
      def grow($seen):
        ( ( $seen + ( [ $seen[] | ($edges[.] // []) ] | flatten ) ) | unique ) as $next
        | if ($next | length) == ($seen | length) then $seen else grow($next) end;
      grow([$from]) | map(select(. != $from));

    ( [ .packages[] | { key: .id, value: . } ] | from_entries ) as $pkgs
    | ( [ .resolve.nodes[]
          | { key: .id,
              value: [ .deps[] | select(any(.dep_kinds[]; .kind == null)) | .pkg ] } ]
        | from_entries ) as $edges
    | ( [ .workspace_members[] | { key: $pkgs[.].name, value: . } ] | from_entries ) as $members
    | ( [ $members | to_entries[]
          | select($pkgs[.value].manifest_path | test("/crates/backends/"))
          | .key ] ) as $backends
    | ( [ $members | to_entries[]
          | { key: .key,
              value: [ reach($edges; .value)[] | $pkgs[.].name ] } ]
        | from_entries ) as $links
    | (
      "COUNT\tmembers\t\($members | length)\tworkspace members"
    , "COUNT\tbackends\t\($backends | length)\tbackend crates under crates/backends/"
    , "COUNT\tedges\t\([ $links[] | length ] | add // 0)\tnormal-dependency links across the workspace"
    , "COUNT\tpurewall\t\(($pure | length) * ($async_stack | length))\t(pure crate, async-stack crate) pairs"
    , "COUNT\twirewall\t\(($wire | length) * ($web_stack | length))\t(wire crate, web-stack crate) pairs"
    , "COUNT\tassetwall\t\(($assets | length) * (($async_stack + $web_stack) | unique | length))\t(asset crate, runtime-or-web-stack crate) pairs"
    , "NOTE\t\([ $pkgs[] | .name ] | map(select(. as $n | $async_stack | index($n))) | unique | length) of \($async_stack | length) async-stack crates (\($async_stack | join(", "))) are in the graph at all"

    # Every named role must resolve to a real workspace member.
    , ( ( $pure + $wire + $assets + $roots + [ $client, $engine ] )[]
        | select($members[.] == null)
        | "VIOLATION\tpolicy names \(.), which is not a workspace member — the rule about it can no longer fail" )
    , ( if ($backends | length) == 0 then
          "VIOLATION\tno workspace member lives under crates/backends/; the backend wall has nothing to enforce"
        else empty end )

    # Wall 1 — the pure crates link no async/web stack.
    , ( $pure[] as $p
        | select($members[$p] != null)
        | ($links[$p] // []) as $deps
        | $async_stack[] as $forbidden
        | select($deps | index($forbidden))
        | "VIOLATION\t\($p) links \($forbidden) (T6: the pure crates carry no runtime)" )

    # Wall 1b — the wire crate links no web stack. It is allowed tokio and nothing else
    # from the async set; see the header and note N5 for why the exemption is that
    # narrow and what measurement forced it.
    , ( $wire[] as $w
        | select($members[$w] != null)
        | ($links[$w] // []) as $deps
        | $web_stack[] as $forbidden
        | select($deps | index($forbidden))
        | "VIOLATION\t\($w) links \($forbidden); only the daemon links the web stack (T6)" )

    # Wall 1c — the asset crate links neither stack. The union of the two sets rather than
    # either of them: what it may not have is a *reason to serve anything*, and both halves of
    # that arrive as a linkage edge. See the header for why it is its own row.
    , ( $assets[] as $a
        | select($members[$a] != null)
        | ($links[$a] // []) as $deps
        | (($async_stack + $web_stack) | unique)[] as $forbidden
        | select($deps | index($forbidden))
        | "VIOLATION\t\($a) links \($forbidden); it embeds files and answers values, and turning those bytes into an HTTP response is the law of the daemon (T6, design §2.10, §2.7)" )

    # Wall 2 — only the composition roots link a backend.
    , ( $members | keys[] as $m
        | select(($roots | index($m)) == null)
        | select(($backends | index($m)) == null)
        | ($links[$m] // []) as $deps
        | $backends[] as $backend
        | select($deps | index($backend))
        | "VIOLATION\t\($m) links the backend \($backend); only \($roots | join(" and ")) may" )

    # Wall 3 — the thin client.
    , ( select($members[$client] != null)
        | ($links[$client] // []) as $deps
        | ( $backends + [$engine] )[] as $forbidden
        | select($deps | index($forbidden))
        | "VIOLATION\t\($client) links \($forbidden); the thin client links no backend and no engine" )
    )
')"

declare -A counts=([members]=0 [backends]=0 [edges]=0 [purewall]=0 [wirewall]=0 [assetwall]=0)
while IFS=$'\t' read -r kind key value description; do
    case "$kind" in
    COUNT)
        counts["$key"]="$value"
        gate_checked "$value" "$description"
        ;;
    NOTE) gate_note "$key" ;;
    VIOLATION) gate_fail "$key" ;;
    esac
done <<<"$findings"

gate_require_nonzero "${counts[members]}" "workspace members"
gate_require_nonzero "${counts[purewall]}" "(pure crate, async-stack crate) pairs"
gate_require_nonzero "${counts[wirewall]}" "(wire crate, web-stack crate) pairs"
gate_require_nonzero "${counts[assetwall]}" "(asset crate, runtime-or-web-stack crate) pairs"

# ------------------------------------------------------------------ the grep half
#
# No V4L2 path outside the backend that owns the ioctls. The allowed directory is the
# manifest directory of `webcam-handler-v4l2` as `cargo metadata` reports it, so moving
# the crate moves the exemption with it.
#
# Line comments are stripped before matching: `v4l::device::Device::query_controls` is
# named in prose (it is the lint-banned method of PF:1) and prose is not linkage. Block
# comments and string literals are not stripped — the workspace writes `//` comments, and
# a rule simple enough to state in one sentence beats one that needs a Rust parser.
backend_dir="$(gate_metadata |
    jq -r '.packages[] | select(.name == "webcam-handler-v4l2") | .manifest_path' |
    head -n1 | xargs -r dirname)"

if [[ -z "$backend_dir" ]]; then
    gate_fail "webcam-handler-v4l2 is not in the workspace; the V4L2 containment rule has no subject"
    gate_finish
fi

# The metadata may describe a different checkout than the tree under test (the selftest
# feeds doctored graphs), so the exemption is matched by its path *suffix*.
backend_suffix="${backend_dir#"$root"/}"

scanned=0
escapes=0
while IFS= read -r -d '' file; do
    scanned=$((scanned + 1))
    rel="${file#"$root"/}"
    case "$rel" in
    "$backend_suffix"/*) continue ;;
    esac
    if sed 's://.*::' "$file" | grep -Eq '\bv4l(_sys)?::'; then
        gate_fail "$rel names a V4L2 path; V4L2 types stay inside $backend_suffix/"
        escapes=$((escapes + 1))
    fi
done < <(gate_rust_files)

gate_checked "$scanned" "Rust source files scanned for escaping V4L2 paths"
gate_require_nonzero "$scanned" "Rust source files"
gate_note "V4L2 paths are confined to $backend_suffix/ ($escapes escape(s) found)"

gate_finish
