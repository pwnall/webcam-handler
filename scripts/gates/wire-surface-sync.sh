#!/usr/bin/env bash
#
# D10's account of the wire surface is one fact away from the wire surface, and this predicate
# is the reconciler that fact did not have.
#
# **The fact is the `wire_surface!` invocation** in `crates/api/src/lib.rs`: the namespace
# literal jsonrpsee is handed, and the `#[method]`/`#[subscription]` names it emits `METHODS`
# and `SUBSCRIPTIONS` from. That is the only copy with a mechanism behind it — the daemon
# registers it, the generated client calls it, and `xtask generate` writes the committed
# OpenRPC document out of it. Every other statement of the same two things *describes* it. When
# they disagree the macro is right and the description is stale, which is why every message
# below names the file to edit rather than merely reporting a difference.
#
# ## The defect this closes, and what it cost
#
# The G6 review's §6.3 found the design naming a namespace the wire has never had. N90's rename
# sweep of 2026-08-13 replaced `wch` with `webcam-handler-cli` across the binary names and took
# D10's sentence with it (`c397f60b`), while the macro two files away kept the literal — and
# note **N91**, written in the same session, is the catalogue that says this particular name
# *is a wire break* and must not be renamed in passing. The warning and the breach landed
# together. **Three sentences carried the wrong name, not one**: design D10, a doc comment
# nine lines below the literal it contradicted, and a transport suite's own explanation. What
# it costs is a client author reading D10 — which is where §2.10 sends them, D10 being the wire
# surface's one home — sending `webcam-handler-cli_list` and being answered "method not found".
#
# The same sentence had a second defect nobody had a way to see: it abbreviated the calibrate
# verbs as `calibrate_*` and listed seven of them, so `calibrate_restore` — declared at P4a as
# the P3 review's one surface change — was named nowhere in the design for two phases. A
# reconciler that only compared the namespace would have passed that tree. So the method list
# is a **derived population** here, and a member the design has no sentence for is red.
#
# ## Four moves, all of them `browser-pins-sync.sh`'s
#
#   1. *The direction is asserted, not discovered.* The macro is the fact; the prose describes.
#   2. *The population is derived and an unmatched member is a failure, never a skip.* Every
#      declared name must appear in D10's list, and every name D10's list states must be one
#      the macro declares (allowing the wire spelling, namespace and all).
#   3. *The prose is read whitespace-normalised.* D10's method list wraps across ten lines, and
#      where a paragraph happens to break is not something this may have an opinion about.
#   4. *Zero matches is a failure.* Every counter is `gate_require_nonzero`, and D10's own
#      marker is asserted to still be there — otherwise a reworded decision silently empties
#      the population and this passes by examining nothing.
#
# ## What it deliberately does not look at
#
# **`docs/implementation-notes.md` and `docs/historical/`, and it does not exclude them — it
# never reaches them.** The tree-wide half below walks the Rust sources and the design document,
# because those are the two places that *state the rule*: the code that implements it and the
# decision that owns it. The notes and the superseded v1 design are records of a run and of a
# day — N91's catalogue quotes the namespace as evidence, and `docs/historical/1-…-v1.md`
# records what D10 said in v1 — and a reconciler that swept them would demand history be
# rewritten every time a name moved. The same distinction `browser-pins-sync.sh` draws between
# a claim about what ships and a record of what once ran.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"
design_path="docs/6-claude-fable-design-v2.md"
design="$root/$design_path"
openrpc_path="schemas/webcam-handler-openrpc.json"
openrpc="$root/$openrpc_path"

# A backtick, as a variable, so the patterns below can be built inside double quotes without
# every one of them opening a command substitution.
tick='`'

if [[ ! -f "$design" ]]; then
    gate_fail "no $design_path; D10 is the wire surface's one home and there is nothing to reconcile against"
    gate_finish
fi

# ------------------------------------------------------------------ the fact

# The invocation is found rather than transcribed: a second `wire_surface!` in the workspace is
# a second wire surface, which D10 forbids outright, and a zero-th is the macro having been
# renamed out from under this file. Both are failures and neither may be guessed past.
invocations=()
while IFS= read -r -d '' file; do
    if grep -q '^wire_surface! {' "$file"; then
        invocations+=("$file")
    fi
done < <(gate_rust_files)

if ((${#invocations[@]} != 1)); then
    gate_fail "found ${#invocations[@]} \`wire_surface! {\` invocation(s) in the tree; D10 is one wire surface and this predicate needs exactly one fact to reconcile against"
    gate_finish
fi
surface="${invocations[0]}"
surface_path="${surface#"$root"/}"
gate_note "the wire surface is declared in $surface_path"

declared_ns="$(sed -n 's/^[[:space:]]*namespace = "\([A-Za-z0-9_]*\)";$/\1/p' "$surface" | head -n1)"
if [[ -z "$declared_ns" ]]; then
    gate_fail "$surface_path no longer declares \`namespace = \"…\";\` inside wire_surface!; the fact this gate compares everything to cannot be read"
    gate_finish
fi
gate_note "the wire namespace is \`$declared_ns\`, so the wire names are ${declared_ns}_list and so on"

# The names, off the attribute lines the macro consumes. `///` is excluded because the trait's
# own doc comment writes `#[method(blocking)]` while explaining why no method carries it, and a
# gate that read prose as a declaration would invent a method called `blocking`.
declared_names=()
while IFS= read -r name; do
    [[ -n "$name" ]] || continue
    declared_names+=("$name")
done < <(grep -E '^[[:space:]]*#\[(method|subscription)\(name = "[a-z_]+"' "$surface" |
    sed -n 's/.*name = "\([a-z_]*\)".*/\1/p' | sort -u)

gate_checked "${#declared_names[@]}" "method and subscription names declared by wire_surface!"
gate_require_nonzero "${#declared_names[@]}" "names declared by wire_surface!"
if ((${#declared_names[@]} == 0)); then
    gate_finish
fi

is_declared() {
    local candidate="$1" known
    for known in "${declared_names[@]}"; do
        [[ "$candidate" == "$known" ]] && return 0
    done
    return 1
}

# ------------------------------------------------------------------ D10's sentence

# One string, newlines collapsed: the sentence that carries this list wraps across ten lines.
prose="$(tr '\n' ' ' <"$design" | tr -s ' ')"

d10_marker='**D10 — One wire surface, one home (T5).**'
if [[ "$prose" != *"$d10_marker"* ]]; then
    gate_fail "$design_path no longer carries the marker '$d10_marker'; a reworded D10 empties this predicate's population, so it is a failure rather than a pass"
    gate_finish
fi

# `|| true` because a D10 that names no namespace is the finding below, not a pipeline error
# this script may die of: `set -o pipefail` would otherwise turn the silent case into an exit
# with nothing printed, which is the one shape a gate must never have.
stated_ns="$(printf '%s' "$prose" |
    { grep -oE "Methods \(namespace ${tick}[A-Za-z0-9_-]+${tick}\)" || true; } | head -n1 |
    sed "s/.*${tick}\\([A-Za-z0-9_-]*\\)${tick}.*/\\1/")"
if [[ -z "$stated_ns" ]]; then
    gate_fail "$design_path states no namespace for the wire surface; D10 is where a client author is sent to learn it, and a decision that stopped saying it must not pass"
    gate_finish
fi
gate_checked 1 "namespace claims D10 makes"
if [[ "$stated_ns" != "$declared_ns" ]]; then
    gate_fail "$design_path says the namespace is \`$stated_ns\`; $surface_path declares \`$declared_ns\` — the macro is what jsonrpsee registers, so $design_path is the file to edit (note N91: this name is a wire break, never a spelling)"
fi

# The method list, bounded at both ends and both anchors asserted. The opening anchor is the
# namespace clause's own `):`, so the namespace token itself is outside the list and is not
# mistaken for a method; the closing one is the sentence that follows the list.
list_end='Binary results'
region="${prose#*Methods (namespace}"
if [[ "$region" == "$prose" || "$region" != *'):'* ]]; then
    gate_fail "$design_path's D10 has no 'Methods (namespace …):' clause to read a method list out of"
    gate_finish
fi
region="${region#*):}"
if [[ "$region" != *"$list_end"* ]]; then
    gate_fail "$design_path's D10 no longer says '$list_end' after its method list, so this predicate cannot tell where the list ends; restore the anchor rather than letting the population run to the end of the document"
    gate_finish
fi
region="${region%%"$list_end"*}"

# Forward: every name the macro declares is a name D10 states.
stated_names=0
for name in "${declared_names[@]}"; do
    if [[ "$region" == *"${tick}${name}${tick}"* ]]; then
        stated_names=$((stated_names + 1))
    else
        gate_fail "$surface_path declares \`$name\` and $design_path's D10 lists no such method; a client author reading D10 is told about some of the surface and not this part of it — spell the name out rather than abbreviating a family"
    fi
done
gate_checked "$stated_names" "declared names D10 states"
gate_require_nonzero "$stated_names" "declared names stated by D10"

# Backward: every name D10 states is a name the macro declares. A wire spelling — the namespace
# and the name — is allowed and checked as the name it carries, because D10 legitimately quotes
# one to show what the separator does.
checked_tokens=0
while IFS= read -r token; do
    [[ -n "$token" ]] || continue
    checked_tokens=$((checked_tokens + 1))
    bare="${token#"${declared_ns}_"}"
    if ! is_declared "$bare"; then
        gate_fail "$design_path's D10 names \`$token\`, which $surface_path does not declare; a method a client is told to call and the daemon does not answer is the same defect as one it answers and D10 never named"
    fi
done < <(printf '%s' "$region" |
    grep -oE "${tick}[a-z][a-z0-9_]*${tick}" | tr -d "$tick" | sort -u)
gate_checked "$checked_tokens" "method-shaped names D10's list states"
gate_require_nonzero "$checked_tokens" "method-shaped names in D10's list"

# ------------------------------------------------------------------ every other sentence

# The third copy, and the reason this predicate is not a one-line diff of D10. Two of the three
# sentences the review found were in Rust: a doc comment inside the invocation, nine lines below
# the literal it contradicted, and a transport suite explaining why its probe method cannot be
# mistaken for a wire name. Both are prose a reader believes, neither is reachable from D10, and
# nothing could go red on either.
prose_claims=0
while IFS=: read -r file line claim; do
    [[ -n "$claim" ]] || continue
    prose_claims=$((prose_claims + 1))
    named="$(printf '%s' "$claim" | sed "s/.*${tick}\\([A-Za-z0-9_-]*\\)${tick}.*/\\1/")"
    if [[ "$named" != "$declared_ns" ]]; then
        gate_fail "${file#"$root"/}:$line says the namespace is \`$named\`; $surface_path declares \`$declared_ns\` — the sentence is the stale half"
    fi
done < <({
    gate_rust_files
    printf '%s\0' "$design"
} | xargs -0 grep -HnoE "namespace (is )?${tick}[A-Za-z0-9_-]+${tick}" 2>/dev/null || true)
gate_checked "$prose_claims" "sentences in the code and the design naming the wire namespace"
gate_require_nonzero "$prose_claims" "sentences naming the wire namespace"

# ------------------------------------------------------------------ the committed document

# The artifact a consumer reads instead of the trait (D10: generated documentation, never a
# second source of truth). `schema-artifacts-current.sh` proves it matches what xtask emits
# *today*; what it cannot say is that the prefix in it is the one D10 promises, because both
# would move together if the macro were renamed and the design left behind.
if [[ ! -f "$openrpc" ]]; then
    gate_fail "no $openrpc_path; the committed wire document a consumer reads without a Rust toolchain is missing"
elif ! jq -e . "$openrpc" >/dev/null 2>&1; then
    gate_fail "$openrpc_path is not readable JSON; the committed wire document cannot be compared with the macro"
else
    document_names=0
    while IFS= read -r wire_name; do
        [[ -n "$wire_name" ]] || continue
        document_names=$((document_names + 1))
        if [[ "$wire_name" != "${declared_ns}_"* ]]; then
            gate_fail "$openrpc_path publishes '$wire_name', which is not in the \`$declared_ns\` namespace $surface_path declares; run just generate rather than editing the document"
        fi
    done < <(jq -r '
        (.methods[]?.name),
        (.["x-subscriptions"][]?.name),
        (.["x-subscriptions"][]?.unsubscribe),
        (.["x-subscriptions"][]?.notification)
    ' "$openrpc" | sort -u)
    gate_checked "$document_names" "wire names published by the committed OpenRPC document"
    gate_require_nonzero "$document_names" "wire names in $openrpc_path"
fi

gate_finish
