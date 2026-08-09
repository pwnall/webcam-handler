#!/usr/bin/env bash
#
# The committed generated artifacts are what the Rust types say they are.
#
# Two artifacts, one rule. The JSON Schema bundle is committed so a consumer of `--json`
# output, a session file or a device profile does not need a Rust toolchain to know the
# shape of it; the OpenRPC document is committed so a consumer speaking JSON-RPC to the
# daemon does not need one to know the method surface (design D10: both are generated
# documentation, never a second source of truth). That convenience is a liability the
# moment a committed copy stops matching what it describes — a consumer validating
# against a stale bundle gets a green light for a document the tool will reject, and a
# client written from a stale OpenRPC document calls a method the daemon does not have.
# So the emitter is re-run and everything it writes is diffed, every CI run.
#
# **No filename appears below.** The population is the emitter's output on one side and
# the committed directory on the other: the first loop fails a file that is emitted and
# not committed, or committed and different, and the second fails a file that is
# committed with nothing emitting it. So the OpenRPC document P4a landed joined this
# comparison by being written rather than by this script learning its name, and the same
# is true of the next artifact. That is a claim about the two loops, and a claim is not a
# gate: `cases/schema-artifacts-current.cases.sh` carries one arm per direction *for the
# OpenRPC document specifically*, including the one docs/9 actually commissions — a T5
# trait that moved while its document stood still.
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

# An empty artifact directory is a defect, not a phase this repository is still in.
#
# It was a named skip until P4a, on the reading that "nothing is committed yet, so no
# committed copy can be stale" — a precondition that stopped being true when the bundle
# landed at P2 and could not be reached again except by something going wrong: a bad
# merge, a rebase that dropped the two added files, a .gitignore edit. Every one of those
# is exactly the drift this gate exists to catch, and every one of them would have been
# answered `PASS … 1 named skip`. A skip that reads as a pass is the defect class the
# whole suite exists to prevent (AGENTS rule 3, rubric Part C), so the branch is now the
# failure it always described.
gate_require_nonzero "$committed_count" "committed artifacts under $artifact_dir/"
if ((committed_count == 0)); then
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
