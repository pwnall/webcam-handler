#!/usr/bin/env bash
#
# The committed agent usage guide is what the command surface says it is (docs/7 P6e).
#
# `docs/agent-guide.md` is the manual for AGENTS' primary consumer — an agent harness driving
# the client unattended — and it is **generated**: the verb vocabulary, every flag, every
# one-line description, the `--json` contract per verb and the whole D13 failure table are
# read out of `webcam-handler-cli-core` and `webcam-handler-schema` by `xtask/src/guide.rs`.
# Generation is the whole of the anti-drift claim, and generation only buys it while the
# committed bytes are the emitted bytes. A file that says `--guarded` where the surface has
# `--no-guard` costs the reader an exit code 2 it cannot diagnose, and the reader has no
# hands.
#
# So the emitter is re-run and the result diffed, every CI run — the same shape as
# `schema-artifacts-current.sh`, which does this for the two JSON artifacts.
#
# ## Why this is a second predicate rather than a widening of that one
#
# That gate's population is a *directory*: everything `xtask` writes under `ARTIFACT_DIR` on
# one side, everything committed there on the other, with no filename in the script. The
# guide is a single file outside that directory, so joining it would mean either moving the
# guide into `schemas/` — where a Markdown manual does not belong, and where the reader it is
# written for would not look — or teaching that predicate a second population and a second
# orphan rule. `agents-md-current.sh` is the precedent: a committed copy tracking a source is
# its own claim with its own arms, and it lives beside the schema gate rather than inside it.
#
# What this does share is the **derivation trick**: the path is read out of xtask's own
# `GUIDE_PATH` constant rather than written here, so moving the guide moves this gate with it
# and a guide whose emitter stopped declaring where it goes is a failure rather than a check
# that quietly examines nothing.
#
# ## The three ways it goes red, and the fourth this cannot see
#
#   * the committed guide differs from what the generator emits — the ordinary drift: a verb
#     added, a flag renamed, a description edited in the Markdown instead of in the code
#   * nothing is committed at that path — a generated file left in somebody's working tree
#   * the emitter runs and writes no guide — a generator that lost its emission, which would
#     otherwise leave the committed file hand-maintained while wearing a generated file's name
#
# The fourth is the *written* half: a walkthrough that teaches a command nobody can run
# regenerates identically every time. That claim belongs to `crates/cli/tests/agent_guide.rs`,
# which extracts every example from the committed file and runs it against the built binary.
# Both are `g6` rows for that reason — this predicate cannot check prose, and that suite
# cannot check the parts of the document that are not commands.
#
# $WCH_GATE_EMITTER is the selftest's seam, exactly as it is for the schema gate: a stand-in
# lets the cheap arms exercise the missing-guide and emitter-failure paths without a workspace
# build, while the arms that matter — a drifted guide, and a *command surface* that moved
# while the guide stood still — run the real `xtask generate`.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

guide_source="$root/xtask/src/guide.rs"
if [[ ! -f "$guide_source" ]]; then
    gate_fail "no guide generator at ${guide_source#"$root"/}; the agent guide has no emitter, so nothing here can be current"
    gate_finish
fi

guide_path="$(sed -n 's/^pub(crate) const GUIDE_PATH: &str = "\(.*\)";$/\1/p' "$guide_source" | head -n1)"
if [[ -z "$guide_path" ]]; then
    gate_fail "xtask no longer declares GUIDE_PATH; this gate reads the guide's location out of the emitter rather than repeating it, and cannot find what it must check"
    gate_finish
fi
gate_note "the emitter writes the agent guide to $guide_path"

committed="$root/$guide_path"
if [[ ! -f "$committed" ]]; then
    gate_fail "$guide_path is emitted and not committed; the manual every unattended caller reads is missing from the tree — run \`just generate\`"
    gate_finish
fi

regenerated="$(mktemp -d "$(gate_scratch_root)/wch-agent-guide.XXXXXXXX")"
trap 'rm -rf "$regenerated"' EXIT

emit_status=0
if [[ -n "${WCH_GATE_EMITTER:-}" ]]; then
    "$WCH_GATE_EMITTER" "$regenerated" >/dev/null || emit_status=$?
else
    # `--out DIR` writes the whole generated tree under DIR, each file at the path it is
    # committed at — so the guide lands at `$regenerated/$guide_path` and nothing is written
    # into the tree being judged.
    cargo run --locked --offline --quiet --manifest-path "$root/Cargo.toml" \
        -p webcam-handler-xtask -- generate --out "$regenerated" >/dev/null || emit_status=$?
fi

if ((emit_status != 0)); then
    gate_fail "the guide generator exited $emit_status; a gate that cannot regenerate cannot compare"
    gate_finish
fi

produced="$regenerated/$guide_path"
if [[ ! -f "$produced" ]]; then
    gate_fail "the generator ran and wrote no $guide_path; a committed guide nothing emits is a hand-maintained file wearing a generated file's name"
    gate_finish
fi

lines="$(wc -l <"$produced" | tr -d ' ')"
if ! diff -u "$committed" "$produced" >/dev/null; then
    gate_fail "$guide_path is stale — it does not match what the command surface emits; run \`just generate\` and commit the result"
    diff -u "$committed" "$produced" \
        --label "$guide_path (committed)" \
        --label "$guide_path (emitted now)" |
        head -n 40 | sed 's/^/  | /' || true
fi

gate_checked "$lines" "lines of the agent guide regenerated and diffed against the committed copy"
gate_require_nonzero "$lines" "lines in the emitted agent guide"

# ------------------------------------------------------------------ the deprecation pointer
#
# docs/7 P6e lands one more thing this predicate can hold: the vendored skill "gains its
# deprecation pointer". `vendor/v4l2-webcam-skill/` is a submodule of somebody else's
# repository, so the pointer cannot live inside it and lives beside it — and a pointer that
# named a guide at the old path after a move would send exactly the reader it exists for to a
# file that is not there.
#
# The population is derived: every Markdown file directly under `vendor/` that this repository
# owns, which is everything at that level except the submodule's own directory. `gate_find`
# prunes `vendor`, deliberately (the submodule is not ours to walk), so the glob is spelled out
# here and the one level is the statement.
shopt -s nullglob
pointers=("$root"/vendor/*.md)
shopt -u nullglob

if ((${#pointers[@]} == 0)); then
    gate_fail "nothing under vendor/ points at $guide_path; docs/7 P6e gives the vendored skill a deprecation pointer, and a submodule cannot carry one — it belongs beside it, in this repository"
    gate_finish
fi

named=0
for pointer in "${pointers[@]}"; do
    if grep -qF "$guide_path" "$pointer"; then
        named=$((named + 1))
    else
        gate_fail "${pointer#"$root"/} does not name $guide_path; a deprecation pointer that does not say what replaced the skill leaves a reader with a workflow this tool no longer uses and no forwarding address"
    fi
done

gate_checked "$named" "deprecation pointer(s) beside the vendored skill naming the generated guide"
gate_require_nonzero "$named" "deprecation pointers"

gate_finish
