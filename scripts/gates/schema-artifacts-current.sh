#!/usr/bin/env bash
#
# The committed generated artifacts are what the Rust types say they are.
#
# The JSON Schema bundle is committed so a consumer of `--json` output does not need a
# Rust toolchain to know the shape of it. That convenience is a liability the moment the
# committed copy stops matching the types: a consumer validating against a stale bundle
# gets a green light for a document the tool will reject. So the emitter is re-run and
# the output diffed, every CI run.
#
# The artifact directory is read out of xtask's own constant rather than repeated here,
# because a second copy of "where the artifacts live" is exactly the kind of drift this
# gate exists to catch.
#
# $WCH_GATE_EMITTER is the selftest's seam: a stand-in emitter lets the failing arms
# exercise the drift, orphan and emitter-failure paths without a workspace build. The
# passing arm always runs the real `xtask generate` — the diff this gate reports is a
# diff against the real types.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

xtask_main="$root/xtask/src/main.rs"
if [[ ! -f "$xtask_main" ]]; then
    gate_fail "no xtask at $xtask_main; the artifacts have no emitter"
    gate_finish
fi

artifact_dir="$(sed -n 's/^const ARTIFACT_DIR: &str = "\(.*\)";$/\1/p' "$xtask_main" | head -n1)"
if [[ -z "$artifact_dir" ]]; then
    gate_fail "xtask no longer declares ARTIFACT_DIR; this gate cannot find what it must check"
    gate_finish
fi
gate_note "xtask writes its artifacts to $artifact_dir/"

committed_dir="$root/$artifact_dir"
committed_count=0
if [[ -d "$committed_dir" ]]; then
    committed_count="$(gate_find "$committed_dir" | tr -cd '\0' | wc -c)"
fi

if ((committed_count == 0)); then
    # Not a silent pass: the emitter exists (xtask's `generate` writes the bundle) but
    # nothing is committed yet, so there is no committed copy to be stale. The real pass
    # arm lands the moment `$artifact_dir/` is committed — from then on this gate diffs
    # every run.
    gate_skip 0 "committed artifacts under $artifact_dir/ — nothing is committed yet, so no committed copy can be stale; the diffing pass arm lands with the committed bundle"
    gate_checked 1 "the emitter's artifact directory declaration"
    gate_finish
fi

regenerated="$(mktemp -d "${TMPDIR:-/tmp}/wch-artifacts.XXXXXXXX")"
trap 'rm -rf "$regenerated"' EXIT

emit_status=0
if [[ -n "${WCH_GATE_EMITTER:-}" ]]; then
    "$WCH_GATE_EMITTER" "$regenerated" >/dev/null || emit_status=$?
else
    cargo run --locked --offline --quiet --manifest-path "$root/Cargo.toml" \
        -p webcam-handler-xtask -- generate --out "$regenerated" >/dev/null || emit_status=$?
fi

if ((emit_status != 0)); then
    gate_fail "the artifact emitter exited $emit_status; a gate that cannot regenerate cannot compare"
    gate_finish
fi

compared=0
while IFS= read -r -d '' produced; do
    name="${produced#"$regenerated"/}"
    compared=$((compared + 1))
    if [[ ! -f "$committed_dir/$name" ]]; then
        gate_fail "$artifact_dir/$name is emitted but not committed; run \`just generate\`"
        continue
    fi
    if ! diff -u "$committed_dir/$name" "$produced" >/dev/null; then
        gate_fail "$artifact_dir/$name is stale — it does not match what the types emit; run \`just generate\`"
        diff -u "$committed_dir/$name" "$produced" | head -n 40 | sed 's/^/  | /' || true
    fi
done < <(gate_find "$regenerated")

while IFS= read -r -d '' committed; do
    name="${committed#"$committed_dir"/}"
    if [[ ! -f "$regenerated/$name" ]]; then
        gate_fail "$artifact_dir/$name is committed but nothing emits it; a generated file with no generator is a hand-edited one"
    fi
done < <(gate_find "$committed_dir")

gate_checked "$compared" "generated artifacts regenerated and diffed against the committed copies"
gate_checked "$committed_count" "committed artifacts under $artifact_dir/"
gate_require_nonzero "$compared" "generated artifacts"

gate_finish
