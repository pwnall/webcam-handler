#!/usr/bin/env bash
#
# Feature posture (design §2.8, rubric B9). Three doors this project keeps shut:
#
#   * `v4l` with any non-default feature. Its `libv4l` feature pulls `v4l-sys`, which
#     links LGPL libv4l — the one dependency edge that would make the whole binary
#     copyleft-encumbered.
#   * `image` with default features. The default set includes `avif`, which drags the
#     rav1e AV1 encoder into a tool that encodes PNG and passes JPEG through.
#   * TLS, anywhere. The daemon speaks over a Unix socket and loopback (D11); a TLS
#     stack would arrive with `webpki-roots`, whose CDLA-Permissive-2.0 is off the
#     allowlist, and with a certificate store nobody asked for.
#
# Read from `cargo metadata`, never from a manifest grep: a manifest that says
# `default-features = false` proves nothing when a *different* crate in the graph turns
# the same feature on. The resolved feature set is the only honest source.
#
# The `default` feature closure is computed from each package's own feature table, so
# "non-default" means what the upstream package means by it — not a list this file keeps.
#
# $WCH_GATE_METADATA is the selftest's seam: the failing arms feed doctored graphs, which
# is the only way to exercise an LGPL door without making the tree actually open it. The
# passing arm resolves the real workspace with `cargo metadata` (see `gate_metadata`).
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

# The per-crate postures design §2.8 states. Each named crate must be *present* in the
# graph: a posture rule about a crate that has silently left the tree is a rule that can
# no longer fail, so its absence is a violation rather than a pass.
# `features-off` names the features rather than the *set* they arrive in, because a ban on
# a defect names the class and not one spelling of it (AGENTS: N249, rubric A17). Asking
# only whether `default` was on left `avif` — the feature that drags the AV1 encoder — one
# explicit `features = ["avif"]` away from arriving green (note **N294**).
policy='[
  { "name": "v4l",   "rule": "subset-of-default" },
  { "name": "image", "rule": "features-off",
    "features": ["default", "default-formats", "avif", "avif-native"] }
]'

# Deliberately over-broad. A feature or crate name carrying `tls`/`ssl` that turns out to
# be innocent is resolved by a recorded note in docs/implementation-notes.md; a narrow
# pattern that misses one ships a TLS stack.
tls_name_pattern='^(rustls|rustls-[a-z0-9_-]+|native-tls|openssl|openssl-sys|openssl-probe|webpki|webpki-roots|tokio-rustls|tokio-native-tls|hyper-rustls|hyper-tls|schannel|security-framework)$'
tls_feature_pattern='tls|ssl'

# The same wall, for the AV1 stack `image`'s `avif` feature reaches (design D17: the
# `avif` -> rav1e drag "is the trap `feature-posture.sh` exists for"). A crate wall beside
# the feature wall because they fail differently: a feature ban catches this workspace
# turning `avif` on, and a name wall catches an AV1 encoder arriving through anybody else's
# default set — the resolved graph is what both read. Over-broad on purpose, and an
# innocent match is resolved by a recorded note the way a TLS one is.
av1_name_pattern='^(rav1e|ravif|dav1d|dav1d-sys|libdav1d-sys|aom|aom-sys|avif-serialize|avif-parse)$'

findings="$(gate_metadata | jq -r \
    --argjson policy "$policy" \
    --arg tlsname "$tls_name_pattern" \
    --arg tlsfeat "$tls_feature_pattern" \
    --arg av1name "$av1_name_pattern" '
    # The transitive closure of a package s own `default` feature, restricted to real
    # features (entries of the form "dep:x" and "x/y" name other crates, not features
    # of this one).
    def closure($features):
      def grow($acc):
        ( ( $acc + ( [ $acc[] | ($features[.] // []) ] | flatten ) ) | unique ) as $next
        | if ($next | length) == ($acc | length) then $acc else grow($next) end;
      grow(["default"]) | map(. as $f | select($features | has($f)));

    ( [ .packages[] | { key: .id, value: . } ] | from_entries ) as $pkgs
    | ( [ .resolve.nodes[] | { key: .id, value: (.features // []) } ] | from_entries ) as $enabled
    | (
      "COUNT\tpackages\t\([ $pkgs[] ] | length)\tpackages in the resolved graph"
    , "COUNT\tfeatures\t\([ $enabled[] | length ] | add // 0)\tenabled features across the graph"
    , "COUNT\trules\t\($policy | length)\tper-crate posture rules"

    # --- the per-crate postures -------------------------------------------------
    , ( $policy[] as $rule
        | [ $pkgs[] | select(.name == $rule.name) ] as $matches
        | if ($matches | length) == 0 then
            "VIOLATION\t\($rule.name) is not in the dependency graph; its posture rule (\($rule.rule)) can no longer fail"
          else
            $matches[] as $p
            | ($enabled[$p.id] // []) as $on
            | if $rule.rule == "subset-of-default" then
                ( closure($p.features) ) as $default
                | ( $on - $default ) as $extra
                | if ($extra | length) > 0 then
                    "VIOLATION\t\($p.name) \($p.version) has non-default feature(s) enabled: \($extra | join(", "))"
                  else empty end
              elif $rule.rule == "features-off" then
                ( ( $rule.features // [] ) - ( ( $rule.features // [] ) - $on ) ) as $banned
                | if ($banned | length) > 0 then
                    "VIOLATION\t\($p.name) \($p.version) has banned feature(s) enabled: \($banned | join(", "))"
                  else empty end
              else
                "VIOLATION\tunknown posture rule \($rule.rule) for \($rule.name)"
              end
          end )

    # --- the TLS wall -----------------------------------------------------------
    , ( $pkgs[] | select(.name | test($tlsname))
        | "VIOLATION\tTLS crate \(.name) \(.version) is in the graph" )
    , ( $enabled | to_entries[] | .key as $id | .value[]
        | select(test($tlsfeat; "i"))
        | "VIOLATION\tTLS-shaped feature \($pkgs[$id].name // $id)/\(.) is enabled" )

    # --- the AV1 wall -----------------------------------------------------------
    , ( $pkgs[] | select(.name | test($av1name))
        | "VIOLATION\tAV1 codec crate \(.name) \(.version) is in the graph" )
    )
')"

# Counts are keyed rather than matched on their prose, so the non-vacuity assertions
# below cannot be defeated by a reworded description. An unparsable or empty metadata
# document produces no COUNT records at all, which is exactly what these catch.
declare -A counts=([packages]=0 [features]=0 [rules]=0)
while IFS=$'\t' read -r kind key value description; do
    case "$kind" in
    COUNT)
        counts["$key"]="$value"
        gate_checked "$value" "$description"
        ;;
    VIOLATION) gate_fail "$key" ;;
    esac
done <<<"$findings"

gate_require_nonzero "${counts[packages]}" "packages in the resolved graph"
gate_require_nonzero "${counts[rules]}" "per-crate posture rules"

gate_finish
