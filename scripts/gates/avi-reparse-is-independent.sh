#!/usr/bin/env bash
#
# The re-parse path docs/7 P6a requires is not the muxer's code, and there is nowhere in this
# directory for the two to start sharing one (docs/7 P6a, `crates/imaging/src/avi.rs`'s module
# doc, AGENTS rule 1).
#
# ## The property is structural, so nothing behavioural can hold it
#
# `crates/imaging/src/avi/` holds two independently derived implementations of one byte layout.
# `read.rs` is a reader written from the RIFF/AVI specification, before the muxer existed;
# `write.rs` is the muxer. docs/7 P6a asks for "an independent re-parse path that is **not** the
# writer's code", and `avi.rs`'s module doc makes the claim explicitly and at length:
#
#   > [`read`] is a reader for the RIFF/AVI specification; [`mod@write`] is the muxer. The two
#   > share **no** code — not a constant, not a FourCC, not a helper … a re-parse assembled from
#   > the muxer's own layout helpers agrees with the muxer by construction. It can catch a typo;
#   > it cannot catch the bugs a muxer actually ships, which are two halves of one wrong idea
#   > agreeing with each other.
#
# Today that sentence is true, and until this predicate landed **nothing could go red on it**.
# That is not the ordinary "somebody forgot to write the test" — no test can be written. A test
# sees values; this claim is about `use` statements and about where files sit.
#
# ## Why this is a gate and not a test
#
# The behavioural half already exists and is a `g6` criterion:
# `the_size_fields_agree_with_the_bytes_written_over_many_frame_length_sequences` drives the
# muxer's output back through `avi::read` and asserts every size field over thirty-two
# pseudo-random takes. It is the strongest thing a test can say here, and it is exactly as green
# against a re-parse that shares the muxer's constants as against one that does not — greener,
# in fact, because two sides that derive their FourCCs from one line agree by construction. So
# the day somebody factors a shared `FOURCC_MOVI` or a `chunk_header()` helper into a third
# module beside these two and has both sides import it, every test in this workspace passes, the
# `g6` row stays green, and the property P6a asked for is gone with nothing left to notice.
#
# That is `token-comparison-has-one-home.sh`'s argument in another costume — a defect class the
# suite is structurally unable to witness — and `web-routes-are-gated.sh`'s shape: a claim over
# the source, made where the decision is actually taken.
#
# ## The four claims
#
#   1. **The pair exists and is the whole population.** `read.rs` and `write.rs` are both under
#      `crates/imaging/src/avi/`, and they are the **only** `.rs` files there. The third-file
#      half is the one this gate is chiefly about: a third module is precisely where a shared
#      layout helper lands, it would satisfy every claim below, and it destroys the property
#      while the diff that adds it looks like tidying. So a third file is a violation, and
#      adding one deliberately means editing this predicate with an argument rather than adding
#      a file nobody weighs.
#   2. **The reader's product code names nothing from the writer** — no `write::`, no
#      `super::write`, no `crate::avi::write`, and no `use super::{write, …}`, which is the same
#      reach with the `::` moved.
#   3. **The writer's product code names nothing from the reader.** Its `#[cfg(test)]` half may,
#      and does, and must be allowed to: `write.rs`'s tests drive `avi::read` on purpose, and
#      that coupling **is** what P6a asked for. A predicate that forbade it would forbid the
#      criterion it exists to protect, and is a predicate somebody turns off.
#   4. **Both modules are still compiled.** `avi.rs` declares each module the directory holds,
#      and the crate root declares `avi`. Every claim above is true of a tree where one of them
#      stopped being in the build — an orphaned `.rs` file is not a compile error in Rust, so
#      the whole subtree can leave silently — and a wall whose subject has left the build has
#      stopped being able to fail. That is `dependency-walls.sh`'s "the crate left the
#      workspace" arm, charged here for a module.
#
# ## What this does **not** claim
#
# **It sees imports and file layout. It does not see two implementations that drifted into
# agreeing by copy-paste.** A `chunk_header()` body retyped into the reader from the muxer's
# source, or a FourCC read off the writer and pasted into the reader as a fresh literal, passes
# every claim here and costs exactly as much independence as a shared helper would. Nothing in
# this suite can go red on that, and pretending otherwise would be worse than saying so: what
# defends it is the argument in `avi.rs`'s module doc, the fact that the reader was written
# first, and the person reading the diff. Review carries that half.
#
# **A shared constant hoisted into `avi.rs` itself is one directory above claim 1's
# population.** Both modules already sit under that parent and the muxer already imports its
# vocabulary types from it, so a `pub(super) const FOURCC_MOVI` there would be red on nothing.
# It is left to review deliberately rather than approximated: `avi.rs` is the file whose own
# header argues the independence, so a FourCC arriving in it arrives in the one place a reader
# is being told it must not, and a rule that tried to tell a layout constant from a vocabulary
# type would be a rule nobody could state in a sentence.
#
# It is also a `grep`. Line comments are stripped (`lib.sh`'s `gate_product_lines`), so **prose
# does not count** — `read.rs` and `avi.rs` argue about the muxer by name for dozens of lines,
# and a gate that turned writing about the split into a violation would push the argument out of
# the modules that carry it. Block comments and string literals are not stripped, for
# `unsafe-scope.sh`'s reason: this workspace writes `//` comments, and a rule that fits in one
# sentence beats one that needs a Rust parser. A reference built by a macro is invisible to
# claims 2 and 3.
#
# It says nothing about whether the reader is *right*. An independently derived reader that
# misreads the specification is a different defect, and `read.rs`'s own suite is what holds it.
#
# ## The matching rule and the populations
#
# Test code is everything from a file's one `#[cfg(test)]` marker to the end of it, which is
# `lib.sh`'s rule (`gate_test_region_start`, `gate_product_lines`) rather than this file's,
# because `token-comparison-has-one-home.sh` and `web-routes-are-gated.sh` read a file's product
# half by the identical rule and a second copy of it is the copy that stops agreeing. Its house
# convention is kept here too: **a file whose boundary cannot be read — two `#[cfg(test)]`
# markers, say — is a failure and not a pass**, because a file nobody can classify is a file
# where claim 3's exemption has no edge.
#
# Populations are derived (docs/9's second structural rule): the crate's directory comes from
# `cargo metadata`, the modules come from the directory listing, and the declarations claim 4
# looks for are spelled from the stems that listing produced. What is *policy* rather than fact
# — the crate, the parent module, and the two names `read` and `write` — is named once below and
# asserted to exist before anything is counted, so a rename or a move fails this gate rather
# than quietly emptying it.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

# The policy this gate is about, named once. Every name is asserted to exist below.
crate_name="webcam-handler-imaging"
# The parent module: `src/<module>.rs` declares the pair, and `src/<module>/` holds it.
module="avi"
# The re-parse path P6a requires, and the muxer it is required not to be.
reader="read"
writer="write"

crate_dir="$(gate_metadata |
    jq -r --arg name "$crate_name" '.packages[] | select(.name == $name) | .manifest_path' |
    head -n1 | xargs -r dirname)"

if [[ -z "$crate_dir" ]]; then
    gate_fail "$crate_name is not a workspace member; the muxer, the reader written from the specification, and the independence between them have no home for this gate to be about"
    gate_finish
fi

# The metadata may describe a different checkout than the tree under test (the selftest feeds
# mutated copies), so paths are resolved as suffixes of the tree under test.
crate_suffix="${crate_dir#"$root"/}"
src_rel="$crate_suffix/src"
crate_root_rel="$src_rel/lib.rs"
parent_rel="$src_rel/$module.rs"
dir_rel="$src_rel/$module"
crate_root="$root/$crate_root_rel"
parent="$root/$parent_rel"
dir="$root/$dir_rel"

# ------------------------------------------------------------------ the named policy exists
#
# Every claim below is about one of these. A tree that has renamed, moved or deleted any of them
# is a tree this gate would otherwise pass by quantifying over nothing.

anchors=0
for required in "$crate_root" "$parent"; do
    anchors=$((anchors + 1))
    if [[ ! -f "$required" ]]; then
        gate_fail "${required#"$root"/} is missing; the crate root and the parent module are what keeps this pair in the build, and this one is not in the tree"
    fi
done

anchors=$((anchors + 1))
if [[ ! -d "$dir" ]]; then
    gate_fail "$dir_rel is not a directory in this tree; the two independently derived implementations of one AVI byte layout are what every claim here is about, so docs/7 P6a's re-parse path is not merely unchecked here — there is nothing left to check"
    gate_checked "$anchors" "named file(s) and directory/directories this gate's claims are about"
    gate_finish
fi

for named in "$reader" "$writer"; do
    anchors=$((anchors + 1))
    if [[ ! -f "$dir/$named.rs" ]]; then
        gate_fail "$dir_rel/$named.rs is missing; P6a's pair is the reader written from the specification and the muxer it is the adversary of, and a tree with one of them renamed or gone is a tree where the independence between them has no subject"
    fi
done

gate_checked "$anchors" "named file(s) and directory/directories asserted to exist before anything is counted"
gate_require_nonzero "$anchors" "named files and directories"

# ------------------------------------------------------------------ claim 1: the population
#
# Derived from the directory listing, never transcribed. Recursive on purpose: a shared helper
# parked at `avi/layout/mod.rs` is the same third module with a deeper path.

declare -a module_rels=()
declare -a stems=()
while IFS= read -r -d '' file; do
    module_rels+=("${file#"$root"/}")
    stems+=("$(basename "$file" .rs)")
done < <(gate_find "$dir" -name '*.rs')

gate_checked "${#module_rels[@]}" "Rust module file(s) under $dir_rel, read out of the directory rather than named here"
# A directory with no modules in it is the subject gone while the parent still declares it, and
# every claim below would quantify over nothing at all.
gate_require_nonzero "${#module_rels[@]}" "Rust module files under $dir_rel"

for stem in "${stems[@]}"; do
    if [[ "$stem" != "$reader" && "$stem" != "$writer" ]]; then
        gate_fail "$dir_rel/$stem.rs is a third module beside $reader.rs and $writer.rs; a directory holding two independently derived implementations of one byte layout is exactly where a shared FourCC or a chunk_header() helper lands, and a helper both sides import makes the re-parse agree with the muxer by construction — which is the one thing docs/7 P6a asks this pair not to do. Every other claim in this gate is true of that tree. Adding a module here is therefore a decision, and it is taken in this predicate with an argument rather than in a diff that reads like tidying"
    fi
done

gate_note "the modules under $dir_rel are: ${stems[*]}"

# ------------------------------------------------------------------ claims 2 and 3: no sibling
#
# One walk, quantified over the derived pair in both directions, because the claim is symmetric
# in product code and the asymmetry is entirely in the exemption: `write.rs`'s `#[cfg(test)]`
# half reaches for `avi::read` on purpose and must keep being allowed to.
#
# **What counts as naming a sibling** is the module's stem in a path position: preceded or
# followed by `::`. That covers `use super::write::…`, `use crate::avi::write;` and a bare
# `write::FOURCC_MOVI` at a call site, and it deliberately does not fire on an ordinary word —
# `write.rs`'s product code contains "already", and a rule that read the `read` inside it as a
# module reference would be the gate that cries wolf note **N60** bills for.
#
# **An import is read as an import, through the one home this suite has for reading one.** Until
# 2026-08-21 the matcher recognised a grouped import by its own punctuation — `[{,]` before the
# stem — and grep is line-based, so the stem landing first on a *continuation* line was invisible:
# `use super::{ … , \n    read,\n};`, which is what rustfmt writes whenever the fill breaks
# there, passed with a counted summary byte-identical to the unseeded tree's, measured with
# ordinary identifier lengths. Note **N269** had named this predicate as the house precedent the
# facade gates had missed, and note **N271** then built `scripts/gates/rust-imports.awk` as the
# one home and converted those two — leaving the cited precedent as the last reader with a copy
# of the narrow rule in it. Every statement is now joined across the lines rustfmt broke it over
# and flattened before the stem is looked for, so a group, a nest, an `as` rename, a restricted
# visibility and an `extern crate` are the paths they carry.
#
# **The two refusals the facade predicates carry are not needed here, and the reason is worth
# writing down rather than transferring.** A glob of the parent — `use super::*;` — brings the
# sibling in under its own name, so the call site still writes `read::recover_frames(…)` and the
# walk still sees it; a glob of the sibling itself — `use super::read::*;` — names the sibling in
# the import and is a violation on the spot. What remains is the joiner's own bound: an import
# whose braces never close is a statement this reader cannot find the end of, and that is refused
# below rather than joined into the rest of the file.

# The normaliser: `scripts/gates/rust-imports.awk` carries the joiner, the flattener and the
# dispatch, and this program is the hooks it calls. An import reaches the walk below as the paths
# it carries, on one line; every other line reaches it as itself.
normalise_program='
    function wch_emit_import(stmt, nr,   flat) {
        flat = wch_flatten(stmt)
        gsub(/\t/, " ", flat)
        print "LINE\t" nr "\t" flat
    }

    function wch_emit_other(line, nr) { print "LINE\t" nr "\t" line }

    function wch_emit_runaway(nr, span) { print "RUNAWAY\t" nr "\t" span }
'

scanned=0
sibling_claims=0
runaway_imports=0

for index in "${!module_rels[@]}"; do
    rel="${module_rels[$index]}"
    stem="${stems[$index]}"
    file="$root/$rel"

    start="$(gate_test_region_start "$file")"
    if ((start < 0)); then
        gate_fail "$rel carries more than one \`#[cfg(test)]\` marker, or a marker that does not open a \`mod\`, so this gate cannot tell its product code from its test code. Claim 3 exists to let the muxer's tests reach for the reader, and an exemption with no edge is an exemption over the whole file — a boundary nobody can read is a finding rather than a pass, which is \`unsafe-scope.sh\`'s price for a count it cannot read"
        continue
    fi

    scanned=$((scanned + 1))
    product=""
    while IFS=$'\t' read -r kind line text; do
        if [[ "$kind" == "RUNAWAY" ]]; then
            runaway_imports=$((runaway_imports + 1))
            gate_fail "$rel:$line opens an import whose braces are still open $text lines later, so this reader cannot tell where the statement ends; joining an unterminated one would swallow the rest of the file into a single logical line and the walk below would be reading one statement where the module has a hundred — close the import, or raise the budget in this predicate with the reason written beside it"
            continue
        fi
        product+="$text"$'\n'
    done < <(gate_product_lines "$file" "$start" |
        awk -f "$(gate_rust_imports_awk)" -f <(printf '%s' "$normalise_program"))

    for sibling in "${stems[@]}"; do
        [[ "$sibling" != "$stem" ]] || continue
        sibling_claims=$((sibling_claims + 1))
        if grep -Eq -- "::${sibling}\b|\b${sibling}::" <<<"$product"; then
            gate_fail "$rel names its sibling module \`$sibling\` in product code; the modules under $dir_rel are one byte layout derived twice on purpose, and a re-parse assembled from the muxer's own layout helpers agrees with the muxer by construction — it can catch a typo, it cannot catch two halves of one wrong idea agreeing with each other (docs/7 P6a, $parent_rel's module doc). A \`#[cfg(test)]\` half may reach across and the muxer's does; product code may not"
        fi
    done
done

gate_checked "$scanned" "module(s) whose product half was read for a reference to a sibling implementation"
gate_require_nonzero "$scanned" "modules with a readable product/test boundary"
gate_checked "$sibling_claims" "ordered pair(s) of modules checked for a product-code reference from one to the other, each import joined and flattened through \`scripts/gates/rust-imports.awk\` before the stem was looked for"
gate_checked "$runaway_imports" "imports this reader could not find the end of, refused rather than joined into the rest of the module"
# With the pair intact this is two. A zero means the walk read nothing, which is the vacuous
# green everything above is arranged to prevent rather than a tree with no coupling in it.
gate_require_nonzero "$sibling_claims" "ordered pairs of modules"

# ------------------------------------------------------------------ claim 4: still in the build
#
# An orphaned `.rs` file is not a compile error in Rust, so a module can leave the build without
# anything going red — and every claim above is true, comfortably and vacuously, of a tree where
# the reader is no longer compiled at all. Both levels are checked: the parent declares each
# module the directory holds, and the crate root declares the parent.

declaration_claims=0

parent_start="$(gate_test_region_start "$parent")"
if ((parent_start < 0)); then
    gate_fail "$parent_rel does not carry the one trailing \`#[cfg(test)] mod\` this gate reads a file's product half by; the declarations below would be counted out of a region nobody can identify"
else
    parent_product="$(gate_product_lines "$parent" "$parent_start")"
    for stem in "${stems[@]}"; do
        declaration_claims=$((declaration_claims + 1))
        if ! grep -Eq -- "\bmod[[:space:]]+${stem}[[:space:]]*;" <<<"$parent_product"; then
            gate_fail "$parent_rel no longer declares \`mod $stem;\`, so $dir_rel/$stem.rs is a file in the tree and not a module in the build; every other claim here is green on that tree, and a wall whose subject has left the build has stopped being able to fail — which is the way this gate would go quiet rather than red"
        fi
    done
fi

crate_root_start="$(gate_test_region_start "$crate_root")"
if ((crate_root_start < 0)); then
    gate_fail "$crate_root_rel does not carry the one trailing \`#[cfg(test)] mod\` this gate reads a file's product half by; the declaration below would be counted out of a region nobody can identify"
else
    declaration_claims=$((declaration_claims + 1))
    crate_root_product="$(gate_product_lines "$crate_root" "$crate_root_start")"
    if ! grep -Eq -- "\bmod[[:space:]]+${module}[[:space:]]*;" <<<"$crate_root_product"; then
        gate_fail "$crate_root_rel no longer declares \`mod $module;\`; the whole pair leaves the build in one line, and it takes docs/7 P6a's re-parse path, the muxer and every claim above with it while the crate still compiles"
    fi
fi

gate_checked "$declaration_claims" "module declaration(s) checked to still put this pair in the build"
gate_require_nonzero "$declaration_claims" "module declarations"

gate_finish
