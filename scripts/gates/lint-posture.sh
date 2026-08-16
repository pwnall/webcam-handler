#!/usr/bin/env bash
#
# The panic/indexing lint set AGENTS calls "clippy-enforced" is enforced by a hand-copied
# attribute at each crate root, and this is what checks the copies (the G6 review's L28, note
# **N165**).
#
# AGENTS.md, "Writing code": *"On device/request-driven paths no `unwrap`/`expect`/`panic`/
# indexing (clippy-enforced)."* docs/9 Part 1 says the same and names the mechanism —
# *"enforced by **module-scoped lint levels**"*. Until 2026-08-16 nothing walked for them, and
# the claim was true of most of the workspace and false of two product crates: `crates/engine`
# and `crates/schema` — the calibration engine and the vocabulary every device-derived value is
# parsed into — carried `#![forbid(unsafe_code)]` and nothing else. Neither cost anything to
# repair, which is the point: the drift was invisible, not expensive.
#
# ## Why this is an attribute per root and not one `[workspace.lints]` table
#
# Note **N1** settled it and the reasoning transfers exactly. Cargo's lints table cannot express
# `cfg`, and this set is deliberately `cfg_attr(not(test), …)`: a test asserting an invariant
# with `.expect("literal fixture")` is stating a precondition, not risking a device. N1 could
# put the two *suppression-hygiene* lints in the table because binding test code there is
# strictly stronger; binding it here is not — it would deny some 1935 in-crate and 1414
# integration-test sites, every one of them a fixture precondition.
#
# So the shape is the one N1's own "Adjacent" paragraph records for `#![forbid(unsafe_code)]`:
# a per-root attribute that cannot be uniform, plus a gate that checks the copies rather than
# trusting them. `unsafe-scope.sh`'s claim 2 is the same walk over the same population, and this
# predicate is deliberately its neighbour rather than a second claim inside it — the two answer
# different questions ("who may write `unsafe`" against "what may panic on a device's answer")
# and a reader looking for either should find a file named after it.
#
# ## The four, the fifth, and the split
#
# The set is `unwrap_used`, `expect_used`, `panic`, `indexing_slicing`. Three roots additionally
# deny `clippy::as_conversions` — `crates/imaging`, the two backends — and that is a *documented
# split rather than drift*: those are the crates that convert device-supplied integers, where
# a silent `as` truncation of a `bytesused` or an `elem_size` is the defect rubric B10 names by
# example. It is not required of the rest, and requiring it would be a decision this predicate
# has no standing to take. What is checked is that it never appears *alone*: a root that denies
# the fifth and not the four has taken the narrow half of a set whose whole point is the wide
# half.
#
# ## Product and dev-only
#
# The population is derived from `cargo metadata`, and so is the split: a package that declares
# `publish = false` is `crates/priv`, `crates/testkit` and `xtask` — the three that never ship.
# Their posture is a decision rather than a rule, and it is reported rather than required:
# `webcam-handler-priv` carries the set (it opens `/dev/video*` and parses `/proc` inside a
# root-capable process), `testkit` and `xtask` do not (a harness and a build-side emitter, whose
# `.expect` on a fixture is the very thing the `not(test)` carve-out is about).
#
# **The residual, stated rather than implied:** deleting the whole block from `crates/priv` would
# pass claim 1, because `publish = false` is the only derived signal that separates a shipped
# crate from a dev-only one and it puts `priv` on the wrong side of the line for this purpose.
# Claim 2 catches a *partial* deletion there, which is the shape drift actually takes. Closing
# the rest wants a derived answer to "does this crate act on a device path", which nothing in
# `cargo metadata` provides.
#
# ## Honest limits
#
# Line comments **and** block comments are stripped and the rest is read with whitespace squeezed
# out, which is `unsafe-scope.sh`'s trade: a rule that fits in one sentence beats one that needs
# a Rust parser. Both kinds, and not just the `//` kind `gate_product_lines` handles, because a
# block that has been commented out reads exactly like a block that is there — the review of the
# batch that landed this predicate measured `/* … */` around `crates/engine`'s whole block as a
# **PASS** (note **N167**). What is *not* stripped is a **string literal**, and `lib.sh:369` says
# the same of `gate_product_lines` — a root that spelled the whole block inside a string would
# satisfy this walk while binding no compiler. That one is left open on the grounds that it is
# not a shape drift takes: a commented-out block is what a bisect leaves behind, and a lost `!`
# is what a refactor leaves behind, but nobody arrives at a crate-root attribute inside a string
# by accident.
#
# The match is anchored at `#!` — the *inner* attribute form — because `#[cfg_attr(not(test),
# deny(…))]` is valid Rust that binds the item it precedes, and in `crates/engine/src/lib.rs`
# that item is a `#[cfg(test)] mod double;`: the set denied for a module that does not exist
# outside `cfg(test)`, which is the whole claim gone. A lost `!` in a refactor is the drift this
# predicate is for, and the same review measured it as a **PASS** too.
#
# So this sees the attribute and not its effect — a crate could carry the block and `#[expect]`
# every site inside it. What answers that is `cargo clippy -D warnings` over the workspace, which
# is a `just ci` step and a g6 criterion; this predicate is what stops the block from going
# missing in the first place, which clippy cannot notice at all.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

# The set, as the roots spell it. Written here because this file is the one home for the claim;
# every crate root is a copy of it, which is exactly what is being checked.
required_lints=(
    clippy::unwrap_used
    clippy::expect_used
    clippy::panic
    clippy::indexing_slicing
)
wider_lint='clippy::as_conversions'

# The opening of the block, whitespace squeezed out. Three things are part of the match and each
# one is a claim. `#!` because the *inner* attribute binds the crate and the outer one binds the
# next item — `#[cfg_attr(not(test), deny(…))]` above `crates/engine`'s `#[cfg(test)] mod double;`
# denies the set for a module that only exists under `cfg(test)`, which is the posture gone and
# valid Rust left behind. `not(test)` because a root that denied the set unconditionally would
# break every test in it, and one that denied it under some other `cfg` would be making a
# different claim than the one AGENTS states. `deny(` because that is the level AGENTS asserts.
block_open='#![cfg_attr(not(test),deny('

# What $1 denies: the lint names inside its `#![cfg_attr(not(test), deny(…))]` block, or nothing.
#
# Comments off first, for `gate_product_lines`' reason and acutely here — this file, the two
# crate roots repaired at G6 and notes N165 and N167 all name the lints in prose, and a walk that
# counted those would be red on the writing about the rule.
#
# **Both comment kinds**, which `gate_product_lines` deliberately does not do and this needs: the
# subject is one attribute per file rather than a population of lines, and a block comment around
# it is precisely how it stops being there while still reading as though it is. Line comments go
# per line, before the squeeze, because `//` runs to the end of *its* line; block comments go
# after it, because once the newlines are gone a `/* … */` that spanned twenty lines is one run
# of characters and the classic C-comment expression matches it in one pass.
denied_lints() {
    local file="$1" squeezed tail
    squeezed="$(sed 's://.*::' "$file" |
        tr -d '[:space:]' |
        sed 's|/\*[^*]*\*\+\([^/*][^*]*\*\+\)*/||g')"
    if [[ "$squeezed" != *"$block_open"* ]]; then
        return 0
    fi
    tail="${squeezed#*"$block_open"}"
    printf '%s\n' "${tail%%'))]'*}"
}

roots_checked=0
product_roots=0
dev_roots=0
wider_roots=0

while IFS=$'\t' read -r package publish src_path; do
    rel="${src_path#"$root"/}"
    if [[ ! -f "$root/$rel" ]]; then
        gate_fail "$package declares a crate root at $rel, which does not exist in the tree under test"
        continue
    fi
    roots_checked=$((roots_checked + 1))

    denied="$(denied_lints "$root/$rel")"
    present=0
    missing=()
    for lint in "${required_lints[@]}"; do
        if [[ "$denied" == *"$lint"* ]]; then
            present=$((present + 1))
        else
            missing+=("$lint")
        fi
    done
    wider=0
    if [[ "$denied" == *"$wider_lint"* ]]; then
        wider=1
        wider_roots=$((wider_roots + 1))
    fi

    # Claim 1: a shipped crate's root carries the whole set.
    if [[ "$publish" == "dev-only" ]]; then
        dev_roots=$((dev_roots + 1))
        gate_note "$rel is dev-only (publish = false) and denies $present of the ${#required_lints[@]}; its posture is a decision, not a rule"
    else
        product_roots=$((product_roots + 1))
        if ((present < ${#required_lints[@]})); then
            gate_fail "$rel is a shipped crate root that does not deny ${missing[*]} under cfg(not(test)); AGENTS calls this set clippy-enforced and the enforcement is this attribute (docs/9 Part 1, note N1)"
        fi
    fi

    # Claim 2: all four or none, everywhere — a root holding some of the set has drifted, and a
    # partial set is the shape drift takes: somebody adds the lint that bit them and stops.
    if ((present > 0 && present < ${#required_lints[@]})); then
        gate_fail "$rel denies $present of the ${#required_lints[@]} lints in the set and not ${missing[*]}; the set is taken whole or not at all"
    fi

    # Claim 3: the fifth is a widening of the four and never a substitute for them.
    if ((wider == 1 && present < ${#required_lints[@]})); then
        gate_fail "$rel denies $wider_lint without the whole panic/indexing set; the wider lint is an addition the device-facing crates make, never the narrow half of the set on its own"
    fi
done < <(gate_metadata | jq -r '
    # The population is every workspace target that has a crate root of its own, and it is
    # written as an *exclusion* rather than a list of kinds on purpose. `lib | bin | proc-macro`
    # was the first spelling and it silently drops a member that declares `crate-type =
    # ["rlib"]` or `["cdylib"]` — the target disappears, `roots_checked` gets smaller, and no
    # claim fires (note **N167**). No member does that today, which is exactly why nobody would
    # notice the day one did. What is genuinely not a crate root is a test, a bench, an example
    # or a build script: `tests/foo.rs` is compiled with `cfg(test)` on, where this whole set is
    # carved out, and `build.rs` runs on the host and touches no device.
    ( [ .workspace_members[] ] ) as $members
    | ["test", "bench", "example", "custom-build"] as $not_a_root
    | .packages[]
    | select(.id as $id | $members | index($id))
    | .name as $name
    | (if .publish == [] then "dev-only" else "shipped" end) as $publish
    | .targets[]
    | select(all(.kind[]; . as $k | $not_a_root | index($k)) | not)
    | "\($name)\t\($publish)\t\(.src_path)"
')

gate_checked "$roots_checked" "crate roots checked for the panic/indexing lint set"
gate_require_nonzero "$roots_checked" "crate roots"
gate_checked "$product_roots" "shipped crate roots required to carry the whole set"
gate_require_nonzero "$product_roots" "shipped crate roots"
gate_checked "$dev_roots" "dev-only crate roots whose posture is reported rather than required"
gate_checked "$wider_roots" "crate roots that additionally deny $wider_lint, the device-integer widening"
# Claim 4, and it is a claim rather than a vacuity guard: the widening is what keeps an `as` from
# silently truncating a `bytesused` or an `elem_size` (rubric B10's worked example), three crates
# took it deliberately, and a tree where none of them still does has lost it without a decision.
if ((wider_roots == 0)); then
    gate_fail "no crate root denies $wider_lint; the crates that convert device-supplied integers took that widening on purpose and a tree where none of them carries it has dropped it silently"
fi
gate_finish
