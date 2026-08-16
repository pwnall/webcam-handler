#!/usr/bin/env bash
#
# A claim's release belongs to the value that holds it (note **N169**, AGENTS rule 8,
# design §2.10).
#
# ## What this exists for, and why note N169 said it could not
#
# The G6 review found one shape twice and named it: something is claimed — a camera's
# recording slot, a camera's fan-out, the right to start a stream — and the claim comes
# back only if *a particular line runs later*. Everything that can stop that line from
# running is a way for the claim to outlive its owner: a cancelled handler future, an
# interleaving that empties a slot between two lock acquisitions, a task that panicked.
# Note N169 wrote the rule and then said this about enforcing it:
#
#   > What is **not** enforced mechanically is the rule itself: nothing in the tree can
#   > see a *new* claim released by a code path. […] a lint that could tell "a value whose
#   > `Drop` matters" from "a value" would have to know which fields are claims.
#
# That is true of a lint over the whole workspace and it was too strong about **these two
# modules**. `crate::record` and `crate::preview` name their claim constructors from a
# small closed vocabulary — a `reserve`, a `claim`, a `hand_over`, an `attach` — and what
# each answers with is a type declared beside it. So the population is derivable, and the
# obligation on it is one of two shapes:
#
#   * **`impl Drop`** — the release runs whatever happened to the caller. `Reserved` and
#     `Watchers` are both this, and `Watchers` was not: it was the value the whole of note
#     N171's repair hands around, and dropping one left a feed with no publisher that
#     every later tab on that camera subscribed to and heard nothing from (note **N177**).
#   * **`#[must_use]`** — for a claim with nothing to give *back*, only something to do.
#     `Starting` is the one: it is a debt to start a driver, and a `Drop` could not
#     discharge it because starting the driver is what the holder was asked for. The
#     compiler refusing to let the value be ignored is the enforcement, and the attribute
#     being *here* is what this checks.
#
# ## The population, derived rather than listed
#
#   1. **The modules** are named below and each must exist: a rule with no subject is a
#      rule that cannot go red.
#   2. **The constructors** are functions in those modules whose name begins with one of
#      the claim verbs. A declaration whose return type this cannot read is a **failure**
#      rather than a pass, for `unsafe-scope.sh`'s reason about a count it cannot read.
#   3. **The claim types** are the type names in those return positions that are declared
#      in the same two modules. `Arc<…>` is deleted from the return type first, and that
#      is the one modelling decision here: an `Arc` is shared ownership, which is the
#      opposite of a claim one holder owes back, so `Previews::reserve`'s `Arc<Feed>` is a
#      registry entry rather than a claim on one.
#
# Test code is excluded through the shared `gate_test_region_start`, because a fixture that
# hands itself a claim value is not the defect this is about. **A module that helper cannot
# classify is read whole**, which is the opposite of the other two callers' answer and for a
# stated reason: those two ask "is there a violation *above* the tests", so an unreadable
# boundary hides one, while this asks "is every claim value released", so an unreadable
# boundary can only add subjects. `crate::preview` is exactly that file — it carries a
# `#[cfg(test)]` on a single constructor as well as one on its test module — and reading it
# whole makes this gate stricter than the rule, never laxer.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

# The two modules that own this daemon's claims. Named rather than discovered: what makes
# the population derivable is the *vocabulary* these two write, and a third module would
# have to adopt it deliberately.
modules=(
    "crates/daemon/src/record.rs"
    "crates/daemon/src/preview.rs"
)

# The verbs a claim-taking constructor is spelled with, as a `grep -E` alternation. Closed
# on purpose: a fifth verb is a decision, and the note says so.
verbs='reserve|claim|hand_over|watchers|attach'

for module in "${modules[@]}"; do
    if [[ ! -f "$root/$module" ]]; then
        gate_fail "$module is not in the tree under test; this rule has no subject"
        gate_finish
    fi
done

# ------------------------------------------------------- the types those modules declare

# Where each module's product code ends: the shared classification, with an unreadable answer
# read as "the whole file" — see the header for why that direction is the safe one here. Taken
# once, and said out loud once, so the rest of this predicate reads a number.
declare -A tests_begin_at=()
for module in "${modules[@]}"; do
    start="$(gate_test_region_start "$root/$module")"
    if ((start < 0)); then
        gate_note "$module carries more than one #[cfg(test)] marker, so it is read whole; that can only add subjects to this rule, never remove them"
        start=0
    fi
    tests_begin_at["$module"]="$start"
done

declared="$(
    for module in "${modules[@]}"; do
        gate_product_lines "$root/$module" "${tests_begin_at["$module"]}" |
            grep -hoE '^[[:space:]]*(pub([[:space:]]*\([^)]*\))?[[:space:]]+)?(struct|enum)[[:space:]]+[A-Z][A-Za-z0-9_]*' |
            awk '{ print "TYPE\t" $NF }'
    done
)"

mapfile -t declared_types < <(awk -F'\t' '$1 == "TYPE" { print $2 }' <<<"$declared" | sort -u)
gate_checked "${#declared_types[@]}" "type(s) declared in the two claim modules"
gate_require_nonzero "${#declared_types[@]}" "declared types"

declares() {
    local wanted="$1" name
    for name in "${declared_types[@]}"; do
        [[ "$name" == "$wanted" ]] && return 0
    done
    return 1
}

# ------------------------------------------------------------------ the claim constructors

constructors=0
unreadable=0
claims=""

for module in "${modules[@]}"; do
    path="$root/$module"
    start="${tests_begin_at["$module"]}"
    last='$'
    if ((start > 1)); then
        last=$((start - 1))
    fi
    while IFS=: read -r line _; do
        constructors=$((constructors + 1))
        # The declaration, which rustfmt may have wrapped: from the `fn` line up to the
        # brace that opens the body, joined. Five lines is more than any signature in
        # these modules takes and the arm below is what happens when it is not enough.
        declaration="$(sed -n "${line},$((line + 4))p" "$path" | tr '\n' ' ')"
        signature="${declaration%%\{*}"
        if [[ "$signature" == "$declaration" ]]; then
            gate_fail "$module:$line declares a claim constructor whose signature this gate cannot read to its opening brace"
            unreadable=$((unreadable + 1))
            continue
        fi
        # A constructor that answers nothing is not a claim constructor. Counted above, so
        # the population is still reported honestly.
        [[ "$signature" == *"->"* ]] || continue
        returned="${signature##*->}"
        # Shared ownership is the opposite of a claim somebody owes back — see the header.
        returned="$(sed -E 's/Arc<[^<>]*>//g' <<<"$returned")"
        while IFS= read -r candidate; do
            [[ -n "$candidate" ]] || continue
            declares "$candidate" || continue
            claims+="$candidate"$'\t'"$module"$'\n'
        done < <(grep -oE '\b[A-Z][A-Za-z0-9_]*\b' <<<"$returned" | sort -u)
    done < <(sed -n "1,${last}p" "$path" |
        grep -nE "(^|[^A-Za-z0-9_])fn[[:space:]]+(${verbs})[A-Za-z0-9_]*[[:space:]]*[(<]")
done

gate_checked "$constructors" "claim-shaped constructor(s) found in ${modules[*]}"
gate_require_nonzero "$constructors" "claim-shaped constructors"

mapfile -t claim_types < <(cut -f1 <<<"$claims" | grep -v '^$' | sort -u)
gate_checked "${#claim_types[@]}" "claim type(s) those constructors answer with"
gate_require_nonzero "${#claim_types[@]}" "claim types"

# ------------------------------------------------- the obligation, one of two shapes each

# Whether $2 is declared in $1 with a `#[must_use]` attached to it.
#
# The raw lines rather than `gate_product_lines`, because what is being read *is* the
# attribute block: stripping line comments would turn the doc comment above a declaration
# into blank lines, and a walk that treated those as part of the block would reach the item
# before it. Doc comments are therefore walked over as themselves, and anything that is
# neither a comment nor an attribute ends the block.
must_use_at() {
    awk -v name="$2" '
        { lines[NR] = $0 }
        !found && $0 ~ "^[[:space:]]*(pub([[:space:]]*\\([^)]*\\))?[[:space:]]+)?(struct|enum)[[:space:]]+" name "([[:space:]]*[<({;]|[[:space:]]*$)" {
            found = NR
        }
        END {
            if (!found) { exit 1 }
            for (i = found - 1; i >= 1; i--) {
                if (lines[i] ~ /^[[:space:]]*#\[must_use/) { print "yes"; exit 0 }
                if (lines[i] !~ /^[[:space:]]*(\/\/|#\[)/) { break }
            }
            print "no"
        }
    ' "$1"
}

released=0
for claim in "${claim_types[@]}"; do
    module="$(awk -F'\t' -v want="$claim" '$1 == want { print $2; exit }' <<<"$claims")"
    if grep -rqE "^impl[[:space:]]+Drop[[:space:]]+for[[:space:]]+${claim}\b" "$root/crates/daemon/src"; then
        released=$((released + 1))
        gate_note "$claim (answered by a claim constructor in $module) gives its claim back in a Drop"
        continue
    fi
    if [[ "$(must_use_at "$root/$module" "$claim")" == "yes" ]]; then
        released=$((released + 1))
        gate_note "$claim (answered by a claim constructor in $module) is #[must_use], so the compiler refuses to let it be ignored"
        continue
    fi
    gate_fail "$claim is answered by a claim constructor in $module and has neither an \`impl Drop\` nor a \`#[must_use]\`: a claim released only by a code path is a claim a cancelled future, a panic or an interleaving can keep for ever (notes N169, N171, N176, N177)"
done

gate_checked "$released" "claim type(s) whose release does not depend on a code path running"
if ((unreadable > 0)); then
    gate_note "$unreadable claim constructor(s) had a signature this gate could not read; each is a failure above"
fi

gate_finish
