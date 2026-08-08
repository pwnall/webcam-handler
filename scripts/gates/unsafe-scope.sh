#!/usr/bin/env bash
#
# The unsafe boundary (design §2.5, rubric B10 [V], AGENTS.md "Writing code").
#
# Two claims, both checked in both directions:
#
#   1. The token `unsafe` appears only inside the V4L2 backend's `src/sys/` module. That
#      module is where ioctls and mmap live; everywhere else, an `unsafe` block is a
#      layering mistake before it is a memory-safety question.
#   2. Every crate root carries `#![forbid(unsafe_code)]` — except `webcam-handler-v4l2`,
#      which cannot and whose crate doc says why. The exception is asserted too: a
#      `forbid` that quietly appeared on the V4L2 crate would mean the sys module had
#      moved somewhere this gate is not looking.
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

gate_finish
