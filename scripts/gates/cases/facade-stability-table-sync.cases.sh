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
    # And the module's own file, because claim 6 reads one: a **Yes** row is a promise about a
    # public surface, so a module in that column with no source is a promise nothing checks and
    # a counted refusal (`fail_case_a_yes_module_has_no_file_this_reader_can_find` is that arm).
    # These two arms differ by exactly this line, which is what makes each of them about one
    # thing.
    printf '//! A module this arm added.\n' >"$tree/crates/imaging/src/tonemap.rs"
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

fail_case_an_inherent_impl_hands_an_embedder_a_module_the_table_forbids() {
    # **The fourth spelling of a reachable public item, and the one this walk could not see.**
    # Measured at HEAD on a copy: the same leak that goes red as a free `pub fn` passed as a
    # `pub fn` on an inherent impl of a module-scope `pub` type, with a counted summary
    # byte-identical to the unseeded tree — because the walk entered `impl Facade {` alone and
    # every other `impl … {` fell through to the module-scope branch, which matches nothing at
    # an impl header and therefore dropped every line inside it. `unreachable_pub` is a
    # workspace lint and `facade` is a `pub mod`, so `PreviewLens::gap` is genuinely API.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^impl Facade {$|pub struct PreviewLens;\n\nimpl PreviewLens {\n    pub fn gap(\&self) -> Option<crate::preview::Gap> {\n        None\n    }\n}\n\nimpl Facade {|' \
        "$(_facade "$tree")" || return 0
    gate_red_because "does not put ${tick}engine::preview${tick} in the **Yes** column" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_trait_impl_hands_an_embedder_a_module_the_table_forbids() {
    # The same class where there is no `pub` to read at all: a trait impl makes its items as
    # public as the trait, so an associated type or a method signature naming a **No**-column
    # module is a module an embedder cannot avoid holding — and a walk that had been widened to
    # `pub` items on impl blocks would still have read past this one. The arm is what makes
    # "every associated signature the language makes reachable" the population rather than a
    # fifth keyword in a list (notes **N249**, **N271**).
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^impl Facade {$|impl Iterator for Facade {\n    type Item = crate::preview::Gap;\n\n    fn next(\&mut self) -> Option<Self::Item> {\n        None\n    }\n}\n\nimpl Facade {|' \
        "$(_facade "$tree")" || return 0
    gate_red_because "does not put ${tick}engine::preview${tick} in the **Yes** column" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

pass_case_a_private_helper_on_an_inherent_impl_is_not_the_surface() {
    # The false branch of the two arms above, so they can go red for the reason they claim. A
    # method that is *not* written `pub`, on an inherent impl of a type in this file, is
    # reachable by nobody outside the crate — and D18's contract is about what an embedder
    # cannot avoid holding. A walk that read every line inside every impl block would fail here,
    # and it would be failing on the encapsulation half of the file, which is
    # `facade-is-the-composition.sh`'s subject and not this one's.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^impl Facade {$|pub struct PreviewLens;\n\nimpl PreviewLens {\n    fn gap(\&self) -> Option<crate::preview::Gap> {\n        None\n    }\n}\n\nimpl Facade {|' \
        "$(_facade "$tree")" || return 0
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_facade_imports_a_forbidden_type_through_super() {
    # **Claim 4's matcher knew one of the three prefixes a path into this crate has.** Measured
    # on a copy: `use super::preview::Gap as PreviewGap;` beside a `pub fn last_gap(&self) ->
    # Option<PreviewGap>` on `impl Facade` — a **No**-column type handed to an embedder from the
    # headline surface — ran item-for-item the unseeded tree, `checked 3 engine modules … the
    # surface an embedder cannot avoid holding`, and passed (note **N328**). `facade.rs` is a
    # top-level module of its crate, so `super::preview` *is* `crate::preview`, and
    # `rust-imports.awk` is where the two spellings become one because the sibling predicate
    # reads the same file.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^use crate::settle::MonotonicClock;$|use crate::settle::MonotonicClock;\nuse super::preview::Gap as PreviewGap;|' \
        "$(_facade "$tree")" || return 0
    gate_seed 's|^impl Facade {$|impl Facade {\n    pub fn last_gap(\&self) -> Option<PreviewGap> {\n        None\n    }\n|' \
        "$(_facade "$tree")" || return 0
    gate_red_because "does not put ${tick}engine::preview${tick} in the **Yes** column" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_facade_spells_a_forbidden_module_inline_through_super() {
    # The same leak with no import to read at all, which is why the rewriting happens inside
    # `emit` rather than at the import hook: a signature is free to spell the whole path, and
    # `-> Option<super::preview::Gap>` passed on the same copy with the same count (note
    # **N328**). The pair is the point — one arm proves the import half, this one proves that a
    # walk widened only at the import hook would still have been half a reader.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^impl Facade {$|impl Facade {\n    pub fn last_gap(\&self) -> Option<super::preview::Gap> {\n        None\n    }\n|' \
        "$(_facade "$tree")" || return 0
    gate_red_because "does not put ${tick}engine::preview${tick} in the **Yes** column" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_yes_module_asks_its_holder_for_a_forbidden_type_it_imported() {
    # **Claim 6's own arm, seeded as the tree shipped it.** `engine::photo` is **Yes**;
    # `photo::take` takes an `OpenCamera<'_>`, which a private `use crate::actor::OpenCamera;`
    # binds — so the signature names no module at all and the forbidden one is reachable only
    # through the import. That is the shape claim 4 cannot see, because claim 4 reads
    # `facade.rs`; it stood in the tree on the day claim 4 landed and was found by review rather
    # than by anything runnable (note **N328**). The seed is one character short of the repair:
    # turn the `pub use` back into a `use` and the escape is gone.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^pub use crate::actor::OpenCamera;$|use crate::actor::OpenCamera;|' \
        "$tree/crates/engine/src/photo.rs" || return 0
    gate_red_because "names ${tick}crate::actor${tick}, which the table puts in the **No** column" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_yes_module_spells_a_forbidden_module_in_a_public_signature() {
    # The other half of claim 6's reader: a path spelled outright, with no import to resolve it
    # through. Seeded into `engine::resolve`, which is **Yes** and imports nothing of this crate
    # today, so the arm is about the signature walk rather than about the binding map — and a
    # reader that only resolved bindings would pass it.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^pub fn list(|pub fn peek_gap() -> Option<crate::preview::Gap> {\n    None\n}\n\npub fn list(|' \
        "$tree/crates/engine/src/resolve.rs" || return 0
    gate_red_because "names ${tick}crate::preview${tick}, which the table puts in the **No** column" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

pass_case_a_yes_module_may_re_export_what_it_hands_over() {
    # **The exemption, driven both ways, because an exemption nothing can falsify is a hole.** A
    # bare `pub use` of a **No**-column item is how a **Yes** module answers for its own surface
    # — `photo::Gap` and `photo::OpenCamera` are that repair — so a name bound that way binds
    # nothing in claim 6's map. First the seeded re-export and the signature that uses it must be
    # green; then the very same two lines with the `pub` taken off the import must be red,
    # because otherwise the exemption above is one nothing could tell from silence.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^pub fn list(|pub use crate::preview::Gap;\n\npub fn peek_gap() -> Option<Gap> {\n    None\n}\n\npub fn list(|' \
        "$tree/crates/engine/src/resolve.rs" || return 0
    WCH_GATE_ROOT="$tree" "$GATE" || return 1

    gate_seed 's|^pub use crate::preview::Gap;$|use crate::preview::Gap;|' \
        "$tree/crates/engine/src/resolve.rs" || return 0
    local heard
    heard="$(WCH_GATE_ROOT="$tree" "$GATE" 2>&1)" && {
        printf '%s\n' "$heard"
        printf 'the same signature stayed green with the re-export demoted to a private import\n' >&2
        return 1
    }
    grep -Fq "names ${tick}crate::preview${tick}, which the table puts in the **No** column" <<<"$heard"
}

fail_case_a_yes_module_has_no_file_this_reader_can_find() {
    # The population's own refusal. Claim 6 derives its files from `gate_pub_mods` filtered by
    # the table, so a module in the **Yes** column whose source this reader cannot locate is a
    # public surface nothing checks — and an unwalked module is exactly the shrink
    # `gate_require_nonzero` cannot see, one file rather than one import along. Two seeds,
    # because the module has to be both declared and classified before it is missing.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^pub mod sweep;$|pub mod ghost;\npub mod sweep;|' "$(_engine_lib "$tree")" || return 0
    gate_seed "s|${tick}discover${tick}, ${tick}facade${tick}, ${tick}pairing${tick}|${tick}discover${tick}, ${tick}facade${tick}, ${tick}ghost${tick}, ${tick}pairing${tick}|" \
        "$(_facade "$tree")" || return 0
    gate_red_because 'to read its surface out of' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_yes_module_imports_a_whole_vocabulary_of_its_own_crate() {
    # The shape claim 6's reader cannot reduce to a path, on the Yes modules' side. After `use
    # crate::*;` any item of any module can be written bare in a signature, so a **No**-column
    # type would reach an embedder through this module with the walk reading a clean surface —
    # a commission satisfied by blindness, which is the one thing a predicate here may not be.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^use schema::backend::CameraBackend;$|use schema::backend::CameraBackend;\nuse crate::*;|' \
        "$tree/crates/engine/src/resolve.rs" || return 0
    gate_red_because 'imports a whole vocabulary of this crate unqualified' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_an_import_in_a_yes_module_yields_no_module_name() {
    # And the other refusal, for `self::` — the prefix that is deliberately not rewritten,
    # because inside module `m` it names `crate::m::` and rewriting it would invent a module.
    # An import this reader cannot reduce is a signature it will resolve against an incomplete
    # map, so it is a counted refusal rather than a quieter number (note **N328**).
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^use schema::backend::CameraBackend;$|use schema::backend::CameraBackend;\nuse self::helpers::Thing;|' \
        "$tree/crates/engine/src/resolve.rs" || return 0
    gate_red_because 'and this reader took no module name out of it' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}
