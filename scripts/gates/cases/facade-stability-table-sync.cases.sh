# Both-direction cases for `facade-stability-table-sync.sh`.
#
# The predicate reconciles one document against several crates, so every arm here is one of the
# five ways a contract table stops being a contract: a module the table never answers about, a
# row written against a name the tree no longer has, a module answered twice, a facade surface
# that hands an embedder something its own table forbids, and a second copy of the list growing
# back in the design.
#
# **The import arms are a family and are meant to be read as one.** The claim-4 walk shipped
# able to read one spelling of an import, so a grouped `use crate::{…}` hid a forbidden module
# *and* took the surface population from three modules to one without failing — worse than
# silent, because `gate_require_nonzero` was satisfied by the survivor (note **N271**). The
# reader is `scripts/gates/rust-imports.awk` now, shared with `facade-is-the-composition.sh`, and
# the arms below cover the flat group, the rustfmt-broken group, the module-scope items the walk
# used to skip, an import that yields no module at all, and an import that never closes.
#
# **The arm this gate is named for is `fail_case_a_new_engine_module_joins_neither_column`.**
# That is the state the tree was actually in: the table classified thirteen of the engine's
# twenty modules and said nothing about the other seven, and no run of anything moved (note
# **N270**). A table with a hole in it reads exactly like a table without one.
#
# The seeds are Rust- and Markdown-shaped and never compiled: this predicate reads source, so a
# seeded violation has to be readable rather than buildable.
#
# shellcheck shell=bash

# A backtick, as a variable, for `browser-pins-sync.sh`'s reason: every claimed sentence below
# quotes a backticked name, and one written inside single quotes reads to the linter as an
# unexpanded command substitution.
tick='`'

_facade() {
    printf '%s' "$1/crates/engine/src/facade.rs"
}

_engine_lib() {
    printf '%s' "$1/crates/engine/src/lib.rs"
}

_imaging_lib() {
    printf '%s' "$1/crates/imaging/src/lib.rs"
}

_schema_lib() {
    printf '%s' "$1/crates/schema/src/lib.rs"
}

pass_case() {
    "$GATE"
}

fail_case_a_new_engine_module_joins_neither_column() {
    # The hole the predicate was written for. A module lands, nobody decides which column it is
    # in, and the table goes on reading like a complete answer — which is the difference between
    # a contract and a paragraph (notes **N153**, **N158**).
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^pub mod write;$|pub mod telemetry;\npub mod write;|' \
        "$(_engine_lib "$tree")" || return 0
    gate_red_because 'puts it in neither column' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_row_names_a_module_the_crate_no_longer_declares() {
    # The other direction, and the one a table nobody reconciles always drifts into: the row
    # survives the rename, promises something about a module nobody has, and would go on
    # covering whatever arrives next under that spelling.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^pub mod sweep;$|pub mod sweeps;|' "$(_engine_lib "$tree")" || return 0
    gate_red_because "declares no such ${tick}pub mod${tick}" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_module_is_answered_in_two_columns_at_once() {
    # A module in both the Yes row and the No row: an embedder holding the table holds whichever
    # answer they read first, and neither of them is wrong. Exactly-one is the claim, so the
    # arm that proves the count has to seed two rather than zero.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed "s|${tick}actor${tick}, ${tick}calibrate${tick}|${tick}actor${tick}, ${tick}settle${tick}, ${tick}calibrate${tick}|" \
        "$(_facade "$tree")" || return 0
    gate_red_because 'in 2 rows' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_table_names_a_crate_the_workspace_does_not_have() {
    # A *Where* cell is resolved against the manifests rather than against a path this script
    # believes, so a package renamed leaves the row promising something about a crate an
    # embedder cannot depend on.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|webcam-handler-testkit|webcam-handler-testkits|g' \
        "$(_facade "$tree")" || return 0
    gate_red_because 'no package under crates/ declares that name' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_table_header_is_reworded_away() {
    # The anchor. A reworded header empties every population below it, and an emptied population
    # passing is the failure this whole suite is written against — `wire-surface-sync.sh` charges
    # the same price for a reworded D10.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|May an embedder hold it?|May an embedder use it?|' \
        "$(_facade "$tree")" || return 0
    gate_red_because 'no longer carries the table header' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_row_answers_something_other_than_yes_or_no() {
    # The contract has two answers. A third is a row nothing downstream can act on, and a reader
    # that shrugged past it would be deciding an embedder's contract by not looking.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|\*\*Yes\*\*|**Maybe**|g' "$(_facade "$tree")" || return 0
    gate_red_because "answers '**Maybe**' for" env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_row_loses_one_of_its_four_columns() {
    # A row this reader cannot split into verdict, crate, modules and why is one it cannot
    # reconcile against anything — and reading three cells out of a four-cell row would silently
    # classify the modules under the wrong heading, which is worse than refusing.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed "s#| ${tick}oracle${tick} | it drives#| ${tick}oracle${tick} it drives#" \
        "$(_facade "$tree")" || return 0
    gate_red_because 'carries a stability-table row with' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_crate_is_answered_both_as_a_whole_and_module_by_module() {
    # `the whole crate` carries a verdict for every module in the crate, so a second row listing
    # some of them is a second contract over the same code.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed "s#| ${tick}oracle${tick} | it drives#| the whole crate | it drives#" \
        "$(_facade "$tree")" || return 0
    gate_red_because 'in one row and module by module in another' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_facade_signature_hands_back_a_type_the_table_forbids() {
    # The defect the G7 review found, as a class rather than as its instance. `Facade::photo`
    # took a `&mut dyn Destination` and answered a `Photograph` while the table forbade
    # `engine::photo`'s destinations by name, so the headline verb D18 promotes could not be
    # called by an embedder who stayed inside the contract. Seeded here on a different verb and
    # a different module, because the rule is about signatures and not about `photo`.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|    pub fn watch(&self) -> Result<Box<dyn HotplugWatch>> {|    pub fn watch(\&self) -> Result<crate::preview::Gap> {|' \
        "$(_facade "$tree")" || return 0
    gate_red_because "does not put ${tick}engine::preview${tick} in the **Yes** column" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_facade_imports_a_type_from_a_module_the_table_forbids() {
    # The same claim reached through the spelling the facade actually uses: `Destination` and
    # `Photograph` are named unqualified in the signature and come in through a `use`, so a
    # reader that only walked signatures would have missed the very instance this exists for.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^use crate::settle::MonotonicClock;$|use crate::store::SessionStore;|' \
        "$(_facade "$tree")" || return 0
    gate_red_because "does not put ${tick}engine::store${tick} in the **Yes** column" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

pass_case_a_module_added_to_both_the_crate_and_its_row_reconciles() {
    # The false branch of the both-directions claim, so it can go red. Both halves move in one
    # commit — which is what the failing arms are asking for — and the predicate must be green
    # on it, or every one of them is red for a reason that has nothing to do with the seed.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^pub mod y4m;$|pub mod tonemap;\npub mod y4m;|' \
        "$(_imaging_lib "$tree")" || return 0
    gate_seed "s|${tick}stream_stats${tick}, ${tick}video${tick}|${tick}stream_stats${tick}, ${tick}tonemap${tick}, ${tick}video${tick}|" \
        "$(_facade "$tree")" || return 0
    WCH_GATE_ROOT="$tree" "$GATE"
}

pass_case_a_module_added_to_a_crate_the_table_takes_whole_needs_no_row() {
    # `the whole crate` is a verdict for every module in it, present and future — that is what
    # the row buys and why it is written that way. A schema module added without touching the
    # table must stay green, or the row would be a module list wearing a shorter spelling.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^pub mod video;$|pub mod telemetry;\npub mod video;|' \
        "$(_schema_lib "$tree")" || return 0
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_facade_module_is_gone() {
    # The contract's one home. A facade this reader cannot open is not an absent finding — it is
    # D18's stability table having no side to reconcile the crates against, and a run that
    # congratulated the tree for it would be counting its own blindness as compliance.
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$(_facade "$tree")"
    gate_red_because 'is not a file' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_facade_imports_a_forbidden_module_inside_a_group() {
    # Note **N269**'s defect, written fresh in this predicate one file over: claim 4's import
    # scan matched `crate::` only when a lowercase letter followed, so `use crate::{…}` yielded
    # nothing at all. Measured on a copy — the forbidden module went unseen *and* the surface
    # population fell from three modules to one, with `gate_require_nonzero` satisfied by the
    # survivor (note **N271**). The import reader is shared with
    # `facade-is-the-composition.sh` now, so a group is the paths it carries.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^use crate::settle::MonotonicClock;$|use crate::{settle::MonotonicClock, store::SessionStore};|' \
        "$(_facade "$tree")" || return 0
    gate_red_because "does not put ${tick}engine::store${tick} in the **Yes** column" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_grouped_import_broken_across_lines_is_still_the_module() {
    # rustfmt writes this shape on its own once the list is long enough, so it is the form the
    # defect would actually arrive in — and it is the arm that holds the *joiner*: a flattener
    # with no joiner in front of it sees `use crate::{`, then a name, then `};`, and none of
    # those lines carries a path it can read.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^use crate::settle::MonotonicClock;$|use crate::{\n    settle::MonotonicClock,\n    store::SessionStore,\n};|' \
        "$(_facade "$tree")" || return 0
    gate_red_because "does not put ${tick}engine::store${tick} in the **Yes** column" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

pass_case_a_grouped_import_of_modules_the_table_allows_is_read_as_those_modules() {
    # The flattener's false branch, so it can go red. The facade's two imports written as one
    # group name exactly the modules the table already puts in the **Yes** column, so this must
    # stay green — and a flattener that produced no path would instead trip the refusal below
    # ("yielded no module name"), which is what makes this twin red-able rather than decorative.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^use crate::photo::{Destination, Photograph};$|use crate::{photo::{Destination, Photograph}, settle::MonotonicClock};|' \
        "$(_facade "$tree")" || return 0
    gate_seed 's|^use crate::settle::MonotonicClock;$||' "$(_facade "$tree")" || return 0
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_an_import_naming_this_crate_yields_no_module() {
    # The population's own guard, and the claim the silent shrink needed. `use crate::*;` names
    # this crate and hands this reader no module, so the surface it derives would quietly get
    # smaller rather than go red — which is exactly how the grouped import passed. An import
    # this reader cannot take a module out of is a finding, not a smaller number.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^use crate::settle::MonotonicClock;$|use crate::settle::MonotonicClock;\nuse crate::*;|' \
        "$(_facade "$tree")" || return 0
    gate_red_because 'and this reader took no module name out of it' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_an_import_never_closes() {
    # The joiner's own bound, and the reason it has one: an unterminated import would otherwise
    # swallow the rest of the module into one logical line and read the whole file as a single
    # statement. A reader that cannot tell where a statement ends must say so.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^use crate::photo::{Destination, Photograph};$|use crate::photo::{Destination, Photograph;|' \
        "$(_facade "$tree")" || return 0
    gate_red_because 'opens an import whose braces are still open' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_module_scope_public_item_names_a_module_the_table_forbids() {
    # Claim 4 walked only the `pub fn`s inside `impl Facade {`, so a free `pub fn` handing back
    # a `crate::preview::Gap` was invisible — measured on a copy, exit 0 (note **N271**).
    # `unreachable_pub` is a workspace lint, so a bare `pub` in this module is genuinely
    # reachable API and a module it names is one an embedder cannot avoid holding.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^impl Facade {$|pub fn gap_of(t: \&crate::photo::Taken) -> Option<\&crate::preview::Gap> {\n    t.gap.as_ref()\n}\n\nimpl Facade {|' \
        "$(_facade "$tree")" || return 0
    gate_red_because "does not put ${tick}engine::preview${tick} in the **Yes** column" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_module_scope_public_type_names_a_module_the_table_forbids() {
    # The same claim reached through an item that has no signature at all. A `pub type` alias
    # hands an embedder the module it aliases just as surely as a function does, and a walk
    # written around `pub fn` alone would read past it.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^impl Facade {$|pub type FacadeGap = crate::preview::Gap;\n\nimpl Facade {|' \
        "$(_facade "$tree")" || return 0
    gate_red_because "does not put ${tick}engine::preview${tick} in the **Yes** column" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_an_inline_pub_mod_joins_neither_column() {
    # Claim 1's population read `^pub mod x;` and could not see `pub mod x { … }`, so an inline
    # module joined neither column and nothing moved — the same "one spelling of the class"
    # defect one derivation over (note **N271**). The engine declares file modules exclusively
    # today, which is exactly why nothing had caught it.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^pub mod write;$|pub mod telemetry { pub fn hi() {} }\npub mod write;|' \
        "$(_engine_lib "$tree")" || return 0
    gate_red_because 'puts it in neither column' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_row_names_no_crate_at_all() {
    # A *Where* cell is where the verdict is aimed. A row that aims it at nothing carries a
    # promise about nothing in particular, which is the one thing a contract table may not do —
    # and a reader that shrugged past it would be deciding an embedder's contract by not looking.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed "s#| \\*\\*No\\*\\* | ${tick}webcam-handler-testkit${tick} | ${tick}oracle${tick} |#| **No** | the test oracles | ${tick}oracle${tick} |#" \
        "$(_facade "$tree")" || return 0
    gate_red_because 'names no crate' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_table_stops_answering_yes_about_the_engine_at_all() {
    # Claim 4 compares the facade's own surface against the engine's **Yes** rows. A table with
    # no such row would make every module on that surface forbidden — and, read the other way,
    # would leave claim 4 comparing against an empty set, which is the emptied population this
    # suite is written against. It is a failure with its own sentence rather than a cascade.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed "s#| \\*\\*Yes\\*\\* | ${tick}webcam-handler-engine${tick} |#| **No** | ${tick}webcam-handler-engine${tick} |#" \
        "$(_facade "$tree")" || return 0
    gate_red_because 'has no **Yes** row naming' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_facade_cannot_be_classified() {
    # The product/test split. A signature read out of the facade's own test module would
    # classify what the tests reach rather than what an embedder holds, so a file whose boundary
    # this reader cannot find is refused rather than read half-right.
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$(_facade "$tree")" <<'RS'

#[cfg(test)]
mod more_tests {}
RS
    gate_red_because 'so its product half cannot be told from its test half' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_design_starts_enumerating_the_table_again() {
    # The second home, collapsed and held collapsed. D18's bullet enumerated the Yes column in
    # prose until 2026-08-20 and had drifted six modules from the table with nothing able to
    # compare them (notes **N270**, **N271**). The bullet argues the rule now and the table is
    # the list; a module named back into the bullet is the enumeration starting over.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed "s|the engine.s pure cores, the facade|the engine's pure cores by name (${tick}pairing${tick}, ${tick}settle${tick}), the facade|" \
        "$tree/docs/12-claude-fable-design-v3.md" || return 0
    gate_red_because 'is a module' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_design_stops_naming_the_predicate_that_answers_which_modules() {
    # The pointer half. A bullet that states the rule and points at nothing is one the next
    # reader answers out of by writing the list back into it, which is how the second home got
    # there the first time — so the pointer is asserted rather than assumed.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed "s|${tick}facade-stability-table-sync.sh${tick} holds|the gate suite holds|" \
        "$tree/docs/12-claude-fable-design-v3.md" || return 0
    gate_red_because 'no longer names' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_designs_supported_composition_bullet_is_reworded_away() {
    # The marker. A reworded bullet empties both claims above it, and an emptied population
    # passing is the failure this whole suite is written against.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|\*\*The supported-composition contract\*\*|**The embedder contract**|' \
        "$tree/docs/12-claude-fable-design-v3.md" || return 0
    gate_red_because 'no longer carries D18' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_module_scope_public_struct_field_names_a_module_the_table_forbids() {
    # The third shape of a module-scope public item, and the branch the other two do not
    # reach: an item that closes when its braces balance rather than at a `;` or at an opening
    # one. A public field is a module an embedder holds whether or not any function mentions it.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^impl Facade {$|pub struct GapHolder {\n    pub gap: crate::preview::Gap,\n}\n\nimpl Facade {|' \
        "$(_facade "$tree")" || return 0
    gate_red_because "does not put ${tick}engine::preview${tick} in the **Yes** column" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}
