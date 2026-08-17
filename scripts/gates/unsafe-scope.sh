#!/usr/bin/env bash
#
# The unsafe boundary (design §2.5, rubric B10 [V], AGENTS.md "Writing code").
#
# Three claims, each checked in both directions:
#
#   1. The token `unsafe` appears only inside the V4L2 backend's `src/sys/` module. That
#      module is where ioctls and mmap live; everywhere else, an `unsafe` block is a
#      layering mistake before it is a memory-safety question.
#   2. Every crate root carries `#![forbid(unsafe_code)]` — except `webcam-handler-v4l2`,
#      which cannot and whose crate doc says why. The exception is asserted too: a
#      `forbid` that quietly appeared on the V4L2 crate would mean the sys module had
#      moved somewhere this gate is not looking.
#   3. The **residual-`unsafe` register** in that module's `mod.rs` — the counted table of
#      one row per block, and the sentence above it that states the totals — describes the
#      tree it is in. Rubric B11 treats a count written in prose as a claim like any
#      other, and a register nobody reconciles is how "ten blocks, one obligation each"
#      survives the eleventh block landing. Added at P4d, which is also what made the
#      claim load-bearing: P4d put a kernel-facing module (`sys::uevent`) inside the
#      exempt region and added *no* row, because `rustix` is a safe wrapper — "the count
#      below is unchanged by it" was a sentence a reader had no way to check, and the next
#      edge written with `libc` would have made it quietly false.
#
# Crate roots are the `src_path` of every lib and bin target `cargo metadata` reports, so
# a new crate is in the population the moment it is a workspace member. The exempt
# directory is the V4L2 package's manifest directory plus `src/sys/`, derived the same
# way — moving the crate moves the exemption.
#
# **The matching rule.** Line comments are stripped, then whole-word `unsafe` is matched.
# `unsafe_code` and `unsafe_op_in_unsafe_fn` do not match (`_` is a word character), which
# is what lets the `forbid` attributes and the lint policy live everywhere. Block comments
# and string literals are *not* stripped: the workspace writes `//` comments, and a rule
# that fits in one sentence beats one that needs a Rust parser. A string that must contain
# the word is spelled around, and that inconvenience is the price of a rule a reader can
# verify by eye.
#
# Claim 3 classifies by the same rule on both sides: a token followed by `impl` is an
# `unsafe impl`, in the tree and in a register row alike. So a row whose *obligation* cell
# happened to contain the words "unsafe impl" would be counted as one — the register would
# have to name a second `unsafe impl` in prose for that to bite, and spelling around it is
# the same inconvenience the paragraph above already charges.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

# The one crate allowed to say `unsafe`, and the one directory inside it.
unsafe_package="webcam-handler-v4l2"

package_dir="$(gate_metadata |
    jq -r --arg name "$unsafe_package" \
        '.packages[] | select(.name == $name) | .manifest_path' |
    head -n1 | xargs -r dirname)"

if [[ -z "$package_dir" ]]; then
    gate_fail "$unsafe_package is not a workspace member; the unsafe exemption has no subject"
    gate_finish
fi

# The metadata may describe a different checkout than the tree under test (the selftest
# feeds mutated copies), so paths are compared as suffixes of the tree under test.
package_suffix="${package_dir#"$root"/}"
sys_suffix="$package_suffix/src/sys"

# `src/sys/` arrives with the raw ioctl layer at P1. Until then the exemption covers no
# files at all, which makes claim 1 *stronger*, not weaker: any `unsafe` anywhere fails.
# Saying so out loud is the difference between "nothing to check" and "checked, and the
# allowed region is currently empty".
if [[ -d "$root/$sys_suffix" ]]; then
    exempt_files="$(gate_find "$root/$sys_suffix" -name '*.rs' | tr -cd '\0' | wc -c)"
    gate_note "the exempt region $sys_suffix/ exists and holds $exempt_files Rust file(s)"
else
    gate_note "the exempt region $sys_suffix/ does not exist yet (the raw ioctl layer lands at P1); zero files are exempt, so every occurrence of the token below is a violation"
fi

# ------------------------------------------------------------------ claim 1

scanned=0
offenders=0
while IFS= read -r -d '' file; do
    scanned=$((scanned + 1))
    rel="${file#"$root"/}"
    case "$rel" in
    "$sys_suffix"/*) continue ;;
    esac
    if sed 's://.*::' "$file" | grep -Eqw 'unsafe'; then
        gate_fail "$rel uses the token \`unsafe\` outside $sys_suffix/"
        offenders=$((offenders + 1))
    fi
done < <(gate_rust_files)

gate_checked "$scanned" "Rust source files scanned for the token \`unsafe\`"
gate_require_nonzero "$scanned" "Rust source files"
gate_note "$offenders file(s) outside $sys_suffix/ use the token"

# ------------------------------------------------------------------ claim 2

roots_checked=0
while IFS=$'\t' read -r package src_path; do
    rel="${src_path#"$root"/}"
    if [[ ! -f "$root/$rel" ]]; then
        gate_fail "$package declares a crate root at $rel, which does not exist in the tree under test"
        continue
    fi
    roots_checked=$((roots_checked + 1))
    if grep -Eq '^#!\[forbid\(unsafe_code\)\]' "$root/$rel"; then
        if [[ "$package" == "$unsafe_package" ]]; then
            gate_fail "$rel carries #![forbid(unsafe_code)], but $unsafe_package is the one crate that must not"
        fi
    elif [[ "$package" != "$unsafe_package" ]]; then
        gate_fail "$rel is a crate root without #![forbid(unsafe_code)]"
    fi
done < <(gate_metadata | jq -r '
    ( [ .workspace_members[] ] ) as $members
    | .packages[]
    | select(.id as $id | $members | index($id))
    | .name as $name
    | .targets[]
    | select(any(.kind[]; . == "lib" or . == "bin" or . == "proc-macro"))
    | "\($name)\t\(.src_path)"
')

gate_checked "$roots_checked" "crate roots checked for #![forbid(unsafe_code)]"
gate_require_nonzero "$roots_checked" "crate roots"

# ------------------------------------------------------------------ claim 3
#
# Both sides of the register are walked and neither is transcribed here. The tree side is
# the token count claim 1 already knows how to take, restricted to the exempt region and
# split by whether the token opens an `unsafe impl` rather than a block. The register side
# is the table's own rows, classified by the same rule. A block that lands with its row is
# green; a block with no row and a row with no block are both red, and that pair is what
# makes the table a claim rather than a decoration.
#
# The summary sentence above the table is read too, because it is the number a reader sees
# first. Its counts may be digits or English words up to twenty; a spelling this cannot
# read is a failure rather than a pass, since an unreadable count is precisely the state
# the claim exists to prevent.

# The small numerals the register's prose may use. A lexicon, not a population: it prints
# nothing for a word it does not know, and its caller fails on that rather than guessing.
gate_numeral() {
    case "${1,,}" in
    zero) printf '0' ;;
    one) printf '1' ;;
    two) printf '2' ;;
    three) printf '3' ;;
    four) printf '4' ;;
    five) printf '5' ;;
    six) printf '6' ;;
    seven) printf '7' ;;
    eight) printf '8' ;;
    nine) printf '9' ;;
    ten) printf '10' ;;
    eleven) printf '11' ;;
    twelve) printf '12' ;;
    thirteen) printf '13' ;;
    fourteen) printf '14' ;;
    fifteen) printf '15' ;;
    sixteen) printf '16' ;;
    seventeen) printf '17' ;;
    eighteen) printf '18' ;;
    nineteen) printf '19' ;;
    twenty) printf '20' ;;
    '' | *[!0-9]*) printf '%s' '' ;;
    *) printf '%s' "$1" ;;
    esac
}

register_rel="$sys_suffix/mod.rs"
register="$root/$register_rel"

tree_tokens=0
tree_impls=0
while IFS= read -r -d '' file; do
    stripped="$(sed 's://.*::' "$file")"
    tokens="$(printf '%s\n' "$stripped" | grep -oE '\bunsafe\b' | wc -l || true)"
    impls="$(printf '%s\n' "$stripped" | grep -oE '\bunsafe[[:space:]]+impl\b' | wc -l || true)"
    tree_tokens=$((tree_tokens + tokens))
    tree_impls=$((tree_impls + impls))
done < <(gate_find "$root/$sys_suffix" -name '*.rs')
tree_blocks=$((tree_tokens - tree_impls))

if [[ ! -d "$root/$sys_suffix" ]]; then
    gate_skip 0 "the exempt region does not exist yet, so there is no residual-\`unsafe\` register to reconcile"
elif ((tree_tokens == 0)); then
    gate_skip 0 "the exempt region holds no \`unsafe\` token, so its register has nothing to reconcile"
elif [[ ! -f "$register" ]]; then
    gate_fail "$sys_suffix/ holds $tree_tokens \`unsafe\` token(s) and there is no $register_rel to register them in"
else
    # SC2016 off for the literal: the heading really does contain backticks, `grep -F`
    # takes it verbatim, and there is nothing in it to expand.
    # shellcheck disable=SC2016
    if ! grep -Fq '## The residual `unsafe`, counted' "$register"; then
        gate_fail "$register_rel has no \"## The residual \`unsafe\`, counted\" section; that heading is what names the register this claim reconciles"
    fi

    # The table's data rows: every `//! |` line that is neither the header nor the rule
    # under it. One table lives under that heading and the heading is asserted above, so
    # the rows are the register.
    mapfile -t register_rows < <(grep -E '^//! \|' "$register" |
        grep -Ev '^//! \| *Where *\|' | grep -Ev '^//! \|-')
    row_count="${#register_rows[@]}"
    row_impls=0
    for row in "${register_rows[@]}"; do
        if [[ "$row" =~ unsafe[[:space:]]+impl ]]; then
            row_impls=$((row_impls + 1))
        fi
    done
    row_blocks=$((row_count - row_impls))

    # The two directions are two sentences, because they are two defects and a reader acts on
    # them differently: a block with no row is an obligation nobody has written down, and a row
    # with no block is a reader believing an obligation is still being discharged somewhere. One
    # sentence for both also left the selftest unable to say which of its two arms had fired —
    # they differ only in which number is the larger — which is note **N242**.
    if ((tree_blocks > row_blocks)); then
        gate_fail "$sys_suffix/ contains $tree_blocks \`unsafe\` block(s) and $register_rel registers $row_blocks: a block landed without its row, so an obligation is being carried that nothing has written down"
    elif ((tree_blocks < row_blocks)); then
        gate_fail "$register_rel registers $row_blocks \`unsafe\` block(s) and $sys_suffix/ contains $tree_blocks: a row outlived the block it names, so a reader is told an obligation is still being discharged somewhere"
    fi
    if ((row_impls != tree_impls)); then
        gate_fail "$register_rel registers $row_impls \`unsafe impl\`(s) and $sys_suffix/ contains $tree_impls"
    fi

    # Same reason as above: the sentence quotes `unsafe impl` in backticks, and this is a
    # regular expression rather than a command.
    # shellcheck disable=SC2016
    summary_re='^//! ([A-Za-z]+|[0-9]+) blocks? and ([A-Za-z]+|[0-9]+) `unsafe impl`'
    said_blocks=""
    said_impls=""
    while IFS= read -r line; do
        if [[ "$line" =~ $summary_re ]]; then
            said_blocks="${BASH_REMATCH[1]}"
            said_impls="${BASH_REMATCH[2]}"
            break
        fi
    done <"$register"

    if [[ -z "$said_blocks" ]]; then
        gate_fail "$register_rel has no \"<n> blocks and <n> \`unsafe impl\`\" sentence over its register; the totals a reader reads first are the ones most worth reconciling"
    else
        said_block_count="$(gate_numeral "$said_blocks")"
        said_impl_count="$(gate_numeral "$said_impls")"
        if [[ -z "$said_block_count" || -z "$said_impl_count" ]]; then
            gate_fail "$register_rel spells its totals '$said_blocks' and '$said_impls', which this gate cannot read; write them as digits or as English words up to twenty, because a count nobody can check is the state this claim exists to prevent"
        elif ((said_block_count != tree_blocks || said_impl_count != tree_impls)); then
            gate_fail "$register_rel says \"$said_blocks blocks and $said_impls \`unsafe impl\`\" while $sys_suffix/ holds $tree_blocks and $tree_impls"
        fi
    fi

    gate_checked "$row_count" "residual-\`unsafe\` register row(s) reconciled against $sys_suffix/"
    gate_require_nonzero "$row_count" "residual-\`unsafe\` register rows"
    # Both sides, always, so a red run prints the pair a reader has to reconcile rather
    # than only the verdict.
    gate_note "$sys_suffix/ holds $tree_blocks \`unsafe\` block(s) and $tree_impls \`unsafe impl\`(s); $register_rel registers $row_blocks and $row_impls"
fi

gate_finish
