#!/usr/bin/env bash
#
# `engine::facade`'s stability table and the crates it classifies are one fact (design **D18**,
# docs/14's commissioning line: "the stability table matches the exports both ways").
#
# **The fact is what the crates declare.** D18 asks the facade's module doc to carry "a stability
# table naming what an embedder may hold … and what it may not", and a table is a contract only
# while something holds it to the tree. Until this landed nothing did: the delivered table
# classified six of `webcam-handler-engine`'s twenty modules as **Yes** and seven as **No** and
# said nothing at all about `capture`, `discover`, `paths`, `profile`, `progress`, `snapshot` and
# `write` — so a new engine module joined neither column, an embedder reading the table learned
# nothing about seven of the twenty, and no run of anything moved (note **N270**). That is a
# paragraph, not a contract, and the difference is exactly what a reconciler makes.
#
# ## The five things it reconciles
#
#   1. **Every module in exactly one column.** For each crate the table classifies module by
#      module, every `pub mod` that crate declares appears in exactly one row's *What* cell.
#      Zero is the hole above; two is a table that answers a question two ways. Both spellings
#      of a declaration count — a file module's `;` and an inline module's `{` — through
#      `gate_pub_mods`, which is also what `facade-is-the-composition.sh` reads the engine's
#      vocabulary with: the class, not one spelling of it, and one home for it (note **N271**).
#   2. **Every named module still declared.** The other direction: a *What* cell naming a module
#      the crate no longer has is a promise about nothing, and it would go on covering whatever
#      arrives next under that spelling.
#   3. **Every named crate is a crate.** A *Where* cell names a package under `crates/`, found by
#      its own manifest rather than by a path this script believes.
#   4. **The facade's public surface never forces a module the table forbids.** Every
#      `crate::<module>` the facade's imports, its `impl Facade` `pub fn` *signatures*, or its
#      module-scope `pub` items name must sit in a **Yes** row for `webcam-handler-engine` —
#      because a headline verb an embedder cannot call from inside the Yes column makes the
#      column a fiction. This is the defect the review found and it is a class rather than an
#      instance: `Facade::photo` took a `&mut dyn Destination` and answered a `Photograph`, both
#      of them `engine::photo`'s and `engine::photo`'s destinations were forbidden by name. A
#      future verb handing back a `crate::preview::Gap` would be the same defect, and so would a
#      free `pub fn` or a `pub type` doing it beside the impl block — `unreachable_pub` is a
#      workspace lint, so a bare `pub` here is reachable API (note **N271**).
#      **An import is read through `scripts/gates/rust-imports.awk`**, the one home this suite
#      has for joining and flattening a Rust import, so a group is the paths it carries; and an
#      import that names this crate and yields no module at all is refused rather than allowed
#      to shrink the population, because that is precisely how the first version of this walk
#      failed — silently, from three modules to one, with `gate_require_nonzero` satisfied by
#      the survivor.
#   5. **The design names the table; it does not restate it.** D18's supported-composition
#      bullet in `docs/12` argues the *rule* — what belongs in each column and why — and names
#      this predicate as what answers which modules the rule lands on. It names no module of a
#      crate the table classifies, because a module named there is a second enumeration starting
#      over, and the last one drifted six modules from the table with nothing able to compare
#      them (notes **N270**, **N271**). `wire-surface-sync.sh` reconciles D10's list rather than
#      collapsing it, and the difference is which document a consumer of that surface reads: a
#      client author reads D10, an embedder reads this module's rustdoc.
#
# **Signatures, not bodies, and the distinction is the whole of D18.** `Facade::set` calls
# `crate::write::set_requested` and `write` is a **No** — correctly, because that call is what
# the facade *encapsulates*; an embedder never names it. What an embedder cannot avoid naming is
# what a signature hands it or asks of it. `facade-is-the-composition.sh` owns the body half of
# the same file and derives its population from the calls; this owns the surface half.
#
# ## What it deliberately does not claim
#
#   * **A crate the table does not name is promised nothing**, which the table says in its own
#     words. So an unnamed crate under `crates/` is a counted note here rather than a finding —
#     printed, because an allowance nobody can see is the silent skip AGENTS rule 3 forbids.
#   * **It says nothing about items.** The table's unit is a module, so a crate's public *types*
#     could move under a **Yes** module and this would not notice. That is the honest residual:
#     the contract D18 wrote is at module granularity, and a reconciler that invented a finer one
#     would be enforcing a rule the design did not make.
#   * **It reads the doc comment as a Markdown table**, not as rustdoc output. A row assembled by
#     a macro, or a table moved into an included file, is invisible — and the header anchor below
#     is what turns that into a failure rather than an empty pass.
#   * **It reads source text, not a syntax tree**, so the surface walk in claim 4 sees the
#     spellings a file writes rather than the items a compiler resolves: a type re-exported into
#     the facade under another name, or a signature assembled by a macro, is not read. What is
#     *not* residual is a spelling the reader could see and did not — an import it cannot take a
#     module out of is a finding, for the reason in claim 4.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

# A backtick, as a variable, so the sentences below can be built inside double quotes without
# every one of them opening a command substitution.
tick='`'

facade_rel="crates/engine/src/facade.rs"
facade="$root/$facade_rel"
engine_package="webcam-handler-engine"

# The one *What* cell spelling that carries a verdict for a whole crate rather than a module
# list. Written once, here, because both the parser and the failure messages name it.
whole_crate="the whole crate"

if [[ ! -f "$facade" ]]; then
    gate_fail "$facade_rel is not a file; D18's stability table is the contract an embedder reads and there is nothing here to reconcile against the tree"
    gate_finish
fi

# --------------------------------------------------------------- the crates
#
# Derived from the manifests rather than from a list: a package moved between directories keeps
# working, and a package renamed goes red in claim 3 where a reader can see which name moved.
declare -A crate_lib=()
crates_found=0
while IFS= read -r -d '' manifest; do
    name="$(sed -n '/^\[package\]/,/^\[/{s/^name = "\([^"]*\)".*/\1/p}' "$manifest" | head -n1)"
    [[ -n "$name" ]] || continue
    lib="$(dirname "$manifest")/src/lib.rs"
    [[ -f "$lib" ]] || continue
    crate_lib["$name"]="$lib"
    crates_found=$((crates_found + 1))
done < <(gate_find "$root/crates" -name Cargo.toml)

if ((crates_found == 0)); then
    gate_fail "no package under crates/ declares a name and a src/lib.rs this reader can find; the table below classifies crates by name, so a run with no crates to resolve them against would pass by comparing nothing"
    gate_finish
fi
gate_checked "$crates_found" "library crates under crates/, found by their own manifests — the vocabulary the table's *Where* cells are resolved against"

# --------------------------------------------------------------- the table
#
# The header row is the anchor. A reworded one empties every population below it, so it is a
# failure and never a pass — `wire-surface-sync.sh` charges the same price for a reworded D10.
header='| May an embedder hold it? | Where | What | Why |'
if ! grep -qF "//! $header" "$facade"; then
    gate_fail "$facade_rel no longer carries the table header '$header'; that row is where this predicate finds D18's stability table, and a reworded header leaves it reconciling an empty table against the tree — move the anchor here in the same commit as the rewording"
    gate_finish
fi

# `ROW TAB verdict TAB where TAB what`, one per table row, read out of the doc comment. The
# kind leads rather than trails, so a malformed row is reported as one rather than being read as
# a cell that happened to spell the sentinel — the separator row and the header itself are
# dropped, and everything else with four cells is a row.
rows=0
short_rows=0
declare -A verdict_of_row=()
declare -A where_of_row=()
declare -A what_of_row=()
while IFS=$'\t' read -r kind field_a field_b field_c; do
    if [[ "$kind" == "SHORT" ]]; then
        short_rows=$((short_rows + 1))
        gate_fail "$facade_rel:$field_b carries a stability-table row with $field_a column(s); every row answers four questions — the verdict, the crate, the modules and why — and a row this reader cannot split into those four is one it cannot reconcile against anything, so it is refused rather than read three cells deep under the wrong headings"
        continue
    fi
    [[ "$kind" == "ROW" ]] || continue
    verdict_of_row["$rows"]="$field_a"
    where_of_row["$rows"]="$field_b"
    what_of_row["$rows"]="$field_c"
    rows=$((rows + 1))
done < <(awk -F'|' '
    /^\/\/! \| May an embedder hold it\? \|/ { seen = 1; next }
    !seen { next }
    $0 !~ /^\/\/! \|/ { exit }
    $0 ~ /^\/\/! \|[-| ]*\|$/ { next }
    NF < 6 {
        printf "SHORT\t%d\t%d\t\n", NF - 2, NR
        next
    }
    {
        verdict = $2; where = $3; what = $4
        gsub(/^[ \t]+|[ \t]+$/, "", verdict)
        gsub(/^[ \t]+|[ \t]+$/, "", where)
        gsub(/^[ \t]+|[ \t]+$/, "", what)
        printf "ROW\t%s\t%s\t%s\n", verdict, where, what
    }
' "$facade")

gate_checked "$rows" "rows in D18's stability table in $facade_rel"
gate_checked "$short_rows" "table rows this reader could not split into four cells, refused rather than read three deep under the wrong headings"
gate_require_nonzero "$rows" "stability-table rows"
if ((rows == 0)); then
    gate_finish
fi

# --------------------------------------------------------------- claim 3, and the shape of a row

declare -A named_crate=()
declare -A whole_crate_row=()
declare -A listed_crate=()
declare -A classified_count=()
declare -A classified_verdict=()
named_modules=0

for ((i = 0; i < rows; i++)); do
    verdict="${verdict_of_row[$i]}"
    where="${where_of_row[$i]}"
    what="${what_of_row[$i]}"

    if [[ "$verdict" != '**Yes**' && "$verdict" != '**No**' ]]; then
        gate_fail "$facade_rel's stability table answers '$verdict' for ${tick}${where}${tick}; the column asks whether an embedder may hold it and the contract has two answers, so a third is a row nothing downstream can act on — say Yes or No, and put the nuance in the Why column where a reader will find it"
        continue
    fi

    packages=()
    while IFS= read -r package; do
        [[ -n "$package" ]] || continue
        packages+=("$package")
    done < <(printf '%s' "$where" | grep -oE "${tick}[a-z][a-z0-9-]*${tick}" | tr -d "$tick")
    if ((${#packages[@]} == 0)); then
        gate_fail "$facade_rel's stability table has a row whose *Where* cell names no crate ('$where'); a verdict about nothing in particular is the one thing a contract table may not carry"
        continue
    fi

    for package in "${packages[@]}"; do
        named_crate["$package"]=1
        if [[ -z "${crate_lib[$package]:-}" ]]; then
            gate_fail "$facade_rel's stability table classifies ${tick}${package}${tick} and no package under crates/ declares that name with a src/lib.rs; the row promises something about a crate an embedder cannot depend on — reconcile the table with the manifest in the same commit as whichever of the two moved"
            continue
        fi
        if [[ "$what" == "$whole_crate" ]]; then
            whole_crate_row["$package"]="$verdict"
            continue
        fi
        listed_crate["$package"]=1
        while IFS= read -r module; do
            [[ -n "$module" ]] || continue
            named_modules=$((named_modules + 1))
            if ! gate_pub_mods "${crate_lib[$package]}" | grep -qxF "$module"; then
                gate_fail "$facade_rel's stability table says ${tick}${package}${tick}'s ${tick}${module}${tick} is $verdict and that crate declares no such ${tick}pub mod${tick}; the row is written against a spelling the crate no longer has, so it promises nothing and would go on covering whatever arrives next under the name — rename it here in the same commit as the module"
                continue
            fi
            classified_count["$package/$module"]=$((${classified_count["$package/$module"]:-0} + 1))
            classified_verdict["$package/$module"]="${classified_verdict["$package/$module"]:-}$verdict "
        done < <(printf '%s' "$what" | grep -oE "${tick}[a-z_][a-z0-9_]*${tick}" | tr -d "$tick")
    done
done

gate_checked "${#named_crate[@]}" "crates D18's stability table names, each resolved against a manifest under crates/"
gate_require_nonzero "${#named_crate[@]}" "crates the stability table names"
gate_checked "$named_modules" "module names the table's module-listing rows carry, each asserted to be a ${tick}pub mod${tick} its crate still declares"
gate_require_nonzero "$named_modules" "module names in the stability table"

# A crate cannot be answered both ways at once: one row saying "the whole crate" and another
# listing its modules is two contracts, and which one an embedder is holding would depend on
# which row they read first.
for package in "${!whole_crate_row[@]}"; do
    if [[ -n "${listed_crate[$package]:-}" ]]; then
        gate_fail "$facade_rel's stability table classifies ${tick}${package}${tick} as '$whole_crate' in one row and module by module in another; an embedder reading the first row holds a different contract from one reading the second, and neither of them is wrong — collapse it to one row"
    fi
done

# --------------------------------------------------------------- both directions, per crate

classified_modules=0
for package in "${!listed_crate[@]}"; do
    [[ -n "${crate_lib[$package]:-}" ]] || continue
    while IFS= read -r module; do
        [[ -n "$module" ]] || continue
        count="${classified_count["$package/$module"]:-0}"
        if ((count == 0)); then
            gate_fail "${crate_lib[$package]#"$root"/} declares ${tick}pub mod $module${tick} and $facade_rel's stability table puts it in neither column; D18's contract is that an embedder is told what may be held and what may not, so a module in neither is a module the table is silent about — decide which column it belongs in, in the commit that adds it"
        elif ((count > 1)); then
            gate_fail "$facade_rel's stability table classifies ${tick}${package}${tick}'s ${tick}${module}${tick} in $count rows (${classified_verdict["$package/$module"]}); a module answered twice is a contract that answers a question two ways — name it in exactly one row"
        else
            classified_modules=$((classified_modules + 1))
        fi
    done < <(gate_pub_mods "${crate_lib[$package]}")
done
gate_checked "$classified_modules" "modules the table classifies in exactly one column, over the crates it classifies module by module"
gate_require_nonzero "$classified_modules" "modules classified in exactly one column"

# The stated default, printed rather than inferred from silence: a crate the table does not name
# is promised nothing, which the table says in its own words.
unnamed=0
for package in "${!crate_lib[@]}"; do
    if [[ -z "${named_crate[$package]:-}" ]]; then
        unnamed=$((unnamed + 1))
        gate_note "${tick}${package}${tick} is a crate D18's stability table does not name, and is therefore promised nothing; allowed, and printed so the silence is visible"
    fi
done
gate_checked "$unnamed" "library crates under crates/ the table names nowhere — the default it states, counted so a crate that quietly joined it is visible. The population is the same one the *Where* cells resolve against, so a binary-only package under crates/ is outside both numbers: it declares no modules an embedder could hold and the table has nothing to say about it"

# --------------------------------------------------------------- claim 4, the facade's surface

declare -A engine_yes=()
for ((i = 0; i < rows; i++)); do
    [[ "${verdict_of_row[$i]}" == '**Yes**' ]] || continue
    [[ "${where_of_row[$i]}" == *"${tick}${engine_package}${tick}"* ]] || continue
    while IFS= read -r module; do
        [[ -n "$module" ]] || continue
        engine_yes["$module"]=1
    done < <(printf '%s' "${what_of_row[$i]}" | grep -oE "${tick}[a-z_][a-z0-9_]*${tick}" | tr -d "$tick")
done
if ((${#engine_yes[@]} == 0)); then
    gate_fail "$facade_rel's stability table has no **Yes** row naming ${tick}${engine_package}${tick}'s modules; the facade's own signatures hand engine types to a caller, so a table with nothing an embedder may hold in that crate is one the facade itself contradicts"
fi

facade_start="$(gate_test_region_start "$facade")"
if ((facade_start == -1)); then
    gate_fail "$facade_rel carries more than one ${tick}#[cfg(test)]${tick} marker, or one that opens something other than a ${tick}mod${tick}, so its product half cannot be told from its test half; a signature read out of a test module would classify what the tests reach rather than what an embedder holds"
    gate_finish
fi

# The facade's own program, function definitions only: `scripts/gates/rust-imports.awk` in
# front of it carries the rules — the joiner, the flattener and the dispatch — because a
# grouped `use crate::{photo::{Destination, Photograph}, …}` is the same import as three flat
# ones, and this predicate shipped for one revision unable to read it (note **N271**). Two
# copies of "read a Rust import" is the second home §2.10 forbids, so there is one.
facade_program='
    function emit(text, nr, what,   token, found) {
        found = 0
        while (match(text, /crate::[a-z_][a-z0-9_]*/)) {
            token = substr(text, RSTART + 7, RLENGTH - 7)
            text = substr(text, RSTART + RLENGTH)
            print "MOD\t" nr "\t" token "\t" what
            found++
        }
        return found
    }

    # A use crate::… is a type this module pulled into scope, and every one of them today is
    # in a signature; reading the import is what catches the unqualified spelling a signature
    # uses afterwards (&mut dyn Destination, not &mut dyn crate::photo::Destination).
    # Flattened first, so a group is the paths it carries.
    #
    # **An import that mentions this crate and yields no module is refused**, and that is the
    # claim the shrinking population needed. When this predicate could not read a brace group,
    # rewriting the facades two imports as one group did not go red — it quietly took the
    # surface from three modules to one, and gate_require_nonzero was satisfied by the
    # survivor (note **N271**). A count that can fall silently is not a population; this is the
    # branch that makes the fall a finding.
    function wch_emit_import(stmt, nr,   bare) {
        if (emit(wch_flatten(stmt), nr, "import") == 0 && index(stmt, "crate::") > 0) {
            bare = stmt
            gsub(/\t/, " ", bare)
            gsub(/^[ ]+/, "", bare)
            print "UNREAD\t" nr "\t" bare "\t"
        }
    }

    function wch_emit_runaway(nr, span) { print "RUNAWAY\t" nr "\t" span "\t" }

    # Inside impl Facade, a pub fn up to its opening brace: what a signature hands a caller
    # or asks of them is what the caller cannot avoid naming, and the body below it is what the
    # facade *encapsulates* — the distinction is the whole of D18.
    #
    # At module scope, every pub item, because unreachable_pub is a workspace lint and a
    # bare pub fn here is therefore genuinely reachable API. A free function handing back a
    # crate::preview::Gap, a pub type aliasing one, a pub struct with one in a public
    # field: each is a module an embedder cannot avoid holding, and a walk gated on being
    # inside the impl block read past all three (note **N271**). A fn closes at its opening
    # brace for the reason above; every other item closes when its braces balance, because its
    # fields and its associated items are the surface.
    function wch_emit_other(line, nr) {
        if (line == "impl Facade {") { in_impl = 1; return }
        if (in_impl && line ~ /^\}/) { in_impl = 0; return }
        if (in_impl) {
            if (line ~ /^    pub fn /) { in_sig = 1; sig = ""; signr = nr }
            if (in_sig) {
                sig = sig " " line
                if (line ~ /\{[[:space:]]*$/ || line ~ /;[[:space:]]*$/) {
                    emit(sig, signr, "impl Facade signature")
                    in_sig = 0
                }
            }
            return
        }
        if (!item_open && line ~ /^pub[[:space:]]+(fn|type|struct|enum|union|trait|const|static)[[:space:]]/) {
            item_open = 1
            item_is_fn = (line ~ /^pub[[:space:]]+fn[[:space:]]/)
            item = ""
            itemnr = nr
            item_depth = 0
            item_saw_brace = 0
        }
        if (!item_open) return
        item = item " " line
        item_depth += wch_braces(line) - wch_unbraces(line)
        if (wch_braces(line) > 0) item_saw_brace = 1
        if (item_is_fn) {
            if (line ~ /\{[[:space:]]*$/ || line ~ /;[[:space:]]*$/) {
                emit(item, itemnr, "module-scope pub fn signature")
                item_open = 0
            }
            return
        }
        if (item_depth <= 0 && (item_saw_brace || line ~ /;[[:space:]]*$/)) {
            emit(item, itemnr, "module-scope public item")
            item_open = 0
        }
    }
'

declare -A surface_module=()
surface_modules=0
runaway_imports=0
unread_imports=0
while IFS=$'\t' read -r kind line module context; do
    if [[ "$kind" == "UNREAD" ]]; then
        unread_imports=$((unread_imports + 1))
        gate_fail "$facade_rel:$line names this crate in an import — ${tick}${module}${tick} — and this reader took no module name out of it; the surface an embedder cannot avoid holding is derived from these statements, so one it cannot read is one that silently shrinks the population rather than one that fails, which is exactly how a grouped import took this claim from three modules to one — spell the import in a form this reader resolves, or widen ${tick}scripts/gates/rust-imports.awk${tick} in the same commit"
        continue
    fi
    if [[ "$kind" == "RUNAWAY" ]]; then
        runaway_imports=$((runaway_imports + 1))
        gate_fail "$facade_rel:$line opens an import whose braces are still open $module lines later, so this reader cannot tell where the statement ends; joining an unterminated one would swallow the rest of the file into a single logical line and read the whole module as one import, which is a worse answer than none — close the import, or raise the budget in this predicate with the reason written beside it"
        continue
    fi
    [[ "$kind" == "MOD" && -n "$module" ]] || continue
    [[ -z "${surface_module[$module]:-}" ]] || continue
    surface_module["$module"]=1
    surface_modules=$((surface_modules + 1))
    if [[ -z "${engine_yes[$module]:-}" ]]; then
        gate_fail "$facade_rel:$line — the facade's $context names ${tick}crate::${module}${tick}, and D18's stability table does not put ${tick}engine::${module}${tick} in the **Yes** column; a verb an embedder cannot call without holding something the contract forbids makes the column a fiction, which is exactly what ${tick}Facade::photo${tick} did with ${tick}engine::photo${tick} — move the module into the Yes column with the reason written beside it, or take the type out of the surface"
    fi
done < <(gate_product_lines "$facade" "$facade_start" |
    awk -f "$(gate_rust_imports_awk)" -f <(printf '%s' "$facade_program"))
gate_checked "$surface_modules" "engine modules the facade's imports, its ${tick}impl Facade${tick} signatures and its module-scope public items name — the surface an embedder cannot avoid holding"
gate_require_nonzero "$surface_modules" "engine modules on the facade's own surface"
gate_checked "$runaway_imports" "imports this reader could not find the end of, refused rather than joined into the rest of the file"
gate_checked "$unread_imports" "imports naming this crate that yielded no module name, refused rather than allowed to shrink the population above"

# --------------------------------------------------------------- claim 5, the design's one home
#
# **D18 names the table; it does not restate it.** The design bullet enumerated the Yes column
# in prose until 2026-08-20, and the two copies had drifted three engine modules and three
# testkit modules apart with nothing able to compare them (notes **N270**, **N271**) — the same
# shape as the prose/policy pair `facade-is-the-composition.sh` reconciles, except that here the
# cheaper answer was available: delete the copy rather than reconcile it, because D18 already
# says the table *is* the contract and an embedder reads the rustdoc rather than the design.
#
# So what is held is the collapse itself: the bullet still argues the rule — which is what a
# design is for — and names this predicate as what answers "which modules", and it names no
# module of a crate the table classifies, because a module named there is a second list starting
# over. `wire-surface-sync.sh` reconciles D10's list instead, and the difference is which
# document a consumer of that surface actually reads.
design_rel="docs/12-claude-fable-design-v3.md"
design="$root/$design_rel"
design_open='**The supported-composition contract**'
design_close='Versioning honesty:'

if [[ ! -f "$design" ]]; then
    gate_fail "no $design_rel; D18 is the stability table's own decision and there is nothing here to hold the table's one-home claim against"
    gate_finish
fi

design_prose="$(tr '\n' ' ' <"$design" | tr -s ' ')"
if [[ "$design_prose" != *"$design_open"* || "$design_prose" != *"$design_close"* ]]; then
    gate_fail "$design_rel no longer carries D18's supported-composition bullet bounded by '$design_open' … '$design_close'; that bullet is where the design says the table is the contract, and a reworded one empties this reconciliation's population — restore the markers, or move them here in the same commit as the rewording"
    gate_finish
fi

design_bullet="${design_prose#*"$design_open"}"
design_bullet="${design_bullet%%"$design_close"*}"

if [[ "$design_bullet" != *"facade-stability-table-sync.sh"* ]]; then
    gate_fail "$design_rel's supported-composition bullet no longer names ${tick}facade-stability-table-sync.sh${tick}; the bullet states the rule and points at the table for the list, and a bullet that points at nothing is one a reader will answer out of by writing the list back into it — which is the second home this predicate exists to keep collapsed"
fi
gate_checked 1 "supported-composition bullets in $design_rel, each asserted to name the predicate that answers which modules the rule lands on"

design_named_modules=0
while IFS= read -r token; do
    [[ -n "$token" ]] || continue
    for package in "${!listed_crate[@]}"; do
        if [[ -n "${classified_count["$package/$token"]:-}" ]]; then
            design_named_modules=$((design_named_modules + 1))
            gate_fail "$design_rel's supported-composition bullet names ${tick}${token}${tick}, which is a module $facade_rel's stability table classifies for ${tick}${package}${tick}; the design states the rule and the table is the list, so a module named in the bullet is a second enumeration starting over — and the last one drifted from the table by six modules with nothing able to see it (notes **N270**, **N271**). Argue the rule in $design_rel and let the table answer which modules it lands on"
            break
        fi
    done
done < <(printf '%s' "$design_bullet" |
    grep -oE "${tick}[a-z_][a-z0-9_]*${tick}" | tr -d "$tick" | sort -u)
gate_checked "$design_named_modules" "modules D18's supported-composition bullet names, which is the count of second enumerations the design has started"

gate_finish
