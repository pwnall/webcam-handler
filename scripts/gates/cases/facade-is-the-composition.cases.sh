# Both-direction cases for `facade-is-the-composition.sh`.
#
# The predicate has one commission — the executor names no engine module the facade composes —
# and it is surrounded by the machinery that keeps that claim from going quiet: two derived
# populations, a declared policy list checked in both directions, two declared composition-root
# reaches, and the product/test split. Each of those has its own inverse here, because every one
# of them is a way the commission could stop being about anything while the gate stayed green.
#
# **The arm this gate is named for is `fail_case_the_executor_assembles_a_verb_from_the_engine_again`.**
# That is the shape the defect actually takes: a verb is written the old way — one that reads
# perfectly, compiles, answers correctly and passes `cli-parity.sh`, because parity compares two
# command-line roots and has no opinion about which code produced the bytes. From that commit on,
# `engine::facade` is a second implementation that happens to agree today, which is exactly the
# upgrade risk the sibling project reported, turned inwards.
#
# **The family worth reading as one is the import arms.** A reach is a reach whatever the
# statement around it looks like: a flat group, a nested one, the same list rustfmt broke across
# lines, a restricted visibility, an `extern crate`, a glob, the package ident instead of the lib
# name, and a bypass moved into a second file of the same crate. Every one of those was measured
# passing at some point in this predicate's two days of life, each with a counted summary
# byte-identical to the unseeded tree's (notes **N269**, **N271**), which is why they are arms
# rather than a sentence in the header. The two shapes that cannot be reduced to a path — a
# binding of the crate and a glob of it — are refused rather than excused, and have their own
# arms saying so.
#
# **The pair worth reading together is arm 8 and its passing twin.** A `crate::record::run(` call
# inside a facade *verb* is the facade quietly growing a lifecycle D18 excludes, and it is red;
# the same call inside the facade's own **test** module is a test driving the engine, and it is
# green. Nothing but the product/test split separates them, which is why both are here.
#
# The seeds are Rust-shaped and never compiled: this predicate reads source, so a seeded
# violation has to be readable rather than buildable, and a case that ran cargo would be
# measuring something else.
#
# Every failing arm names the sentence it claims. That is not decoration here: a seeded rename is
# routinely red under two branches at once — a module the engine stops declaring is both an
# unresolvable reach and a stale policy name — and an arm reading only the exit status cannot say
# which one it proved.
#
# shellcheck shell=bash

# A backtick, as a variable, for `browser-pins-sync.sh`'s reason: every claimed sentence below
# quotes a backticked path, and one written inside single quotes reads to the linter as an
# unexpanded command substitution.
tick='`'

_facade() {
    printf '%s' "$1/crates/engine/src/facade.rs"
}

_executor() {
    printf '%s' "$1/crates/cli/src/main.rs"
}

_engine_lib() {
    printf '%s' "$1/crates/engine/src/lib.rs"
}

pass_case() {
    "$GATE"
}

pass_case_a_comment_may_name_an_engine_module_it_does_not_call() {
    # The executor's doc comments argue about this boundary by name — the whole reason a reader
    # can tell `engine::record` from a bypass is that the file says why it is there — and prose
    # is stripped before matching. A gate that could not tell an argument from a call would push
    # the argument out of the file that needs it.
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$(_executor "$tree")" <<'RS'

// Nothing here calls anything: assembling `controls` by hand would be
// `engine::pairing::in_effect(&controls, Vec::new())` beside a `camera.controls()?`, and that
// is precisely what `self.facade.controls(requested)` replaced.
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

pass_case_the_executors_test_half_may_assemble_the_engine_itself() {
    # A suite that wants to assert something about a write plan calls the planner. Test code is
    # not counted — the defect is a *shipped* second assembly — and the classifier `lib.sh`
    # shares with six other predicates is what draws the line.
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$(_executor "$tree")" <<'RS'

#[cfg(test)]
mod tests {
    #[test]
    fn the_planner_answers_about_an_empty_write_list() {
        let report = engine::write::set_requested(&mut double(), &[], true);
        assert!(report.is_ok());
        let _ = engine::pairing::in_effect(&[], Vec::new());
    }
}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

pass_case_the_facades_own_tests_may_call_a_lifecycle_module() {
    # The other side of arm 8, and the reason the facade is read through the same classifier:
    # `engine::facade`'s test module already calls `crate::resolve::list` to prove the facade
    # *is* the composition, and a reader that counted a test's calls as encapsulation would
    # decide what the CLI may name from what the facade's tests happen to drive.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|        let facade = facade();|        let facade = facade();\n        let _ = crate::record::run();|' \
        "$(_facade "$tree")" || return 0
    WCH_GATE_ROOT="$tree" "$GATE"
}

pass_case_the_executor_may_reach_an_engine_module_the_facade_composes_none_of() {
    # Claim 6's allowance, seeded rather than relied on. `engine::settle`'s monotonic clock is
    # today's live instance — the two excluded lifecycles take one as an argument — and the rule
    # is general: what the facade does not compose, this predicate has no opinion about. It says
    # so in a note, which is the difference between an allowance and a silence.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^use engine::facade::Facade;$|use engine::facade::Facade;\nuse engine::actor;|' \
        "$(_executor "$tree")" || return 0
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_executor_assembles_a_verb_from_the_engine_again() {
    # The defect this predicate exists for. One verb goes back to composing the engine itself;
    # everything is green — the answer is right, the parity gate still compares two roots byte
    # for byte — and `engine::facade` has quietly become a sibling.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|        self.facade.controls(requested)|        let _ = engine::pairing::in_effect(\&[], Vec::new());\n        self.facade.controls(requested)|' \
        "$(_executor "$tree")" || return 0
    gate_red_because "names ${tick}engine::pairing::in_effect${tick}, and ${tick}engine::pairing${tick} is a module" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_bypass_arrives_as_a_bare_module_import() {
    # The same defect wearing the other spelling: `use engine::write;` and then `write::…` at the
    # call site, which names the module once and never again. A reader that only matched
    # `engine::<module>::<item>` would see the import as harmless and the call as local.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^use engine::facade::Facade;$|use engine::facade::Facade;\nuse engine::write;|' \
        "$(_executor "$tree")" || return 0
    gate_red_because "names ${tick}engine::write${tick}, and ${tick}engine::write${tick} is a module" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_executor_stops_naming_the_facade_at_all() {
    # Compliance by not being the consumer. Every other claim here is satisfied by an executor
    # that reaches nothing the facade composes because it reaches the facade for nothing —
    # D18's mechanism is a relationship, and a relationship with one party is a sentence about
    # nobody.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^use engine::facade::Facade;$|use engine::actor::Facade;|' \
        "$(_executor "$tree")" || return 0
    gate_red_because "names no ${tick}engine::facade${tick} path at all" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_facade_starts_composing_a_lifecycle_the_policy_still_excuses() {
    # The policy list read backwards. If the facade grows a recording verb and the executor is
    # left assembling its own, the two copies D18 collapsed are back — and the exemption at the
    # top of the predicate is what would hide it.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|        self.backend.watch()|        let _ = crate::record::run();\n        self.backend.watch()|' \
        "$(_facade "$tree")" || return 0
    gate_red_because 'as a lifecycle D18 keeps out of the facade' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_declared_lifecycle_is_no_longer_a_module_the_engine_declares() {
    # A policy name that has been renamed out from under this script. It would go on excusing a
    # module nobody has, and the next reach arriving under the old spelling would be excused
    # with it — which is the emptied-population failure every predicate here is written against.
    #
    # The seed is `record` and was `sweep` until 2026-08-20, when `sweep` left the policy list
    # for not being a reach anybody made (note **N269**). That is worth saying rather than
    # quietly rewriting: this arm was green for one commit — the list moved, the seed stayed —
    # which is note **N186**'s dead-seed class arriving through the other door, a live seed
    # against a subject that has gone.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^pub mod record;$|pub mod records;|' "$(_engine_lib "$tree")" || return 0
    gate_red_because "excuses the executor's reach into ${tick}engine::record${tick} and crates/engine/src/lib.rs declares no such module" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_declared_lifecycle_is_no_longer_reached() {
    # The other direction on the same list, and the one nobody asked until 2026-08-20: an
    # exemption is only about something while the executor still makes the reach it excuses.
    # `sweep` sat here for a phase excusing a reach this file has never made — note **N164**'s
    # L32 class, which the root reaches were already held to and the lifecycles were not.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|engine::progress::ProgressSink|engine::settle::ProgressSink|g' \
        "$(_executor "$tree")" || return 0
    gate_red_because 'makes no such reach' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_declared_root_reach_loses_the_declaration_it_was_written_against() {
    # The exemption for `engine::profile::read` is written against a function in the engine. If
    # that function is renamed, the row excuses a path nobody can follow — and says nothing about
    # whatever `engine::profile::read` means next.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^pub fn read(|pub fn read_document(|' \
        "$tree/crates/engine/src/profile.rs" || return 0
    gate_red_because "on the strength of ${tick}pub fn read(${tick}" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_declared_root_reach_is_no_longer_made() {
    # Note N164's L32 class, in this predicate's own policy: an exemption whose reach has gone
    # costs nothing to carry, and covers the next thing to arrive under the same name.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|engine::profile::read(path)|engine::store::read(path)|' \
        "$(_executor "$tree")" || return 0
    gate_red_because 'no longer makes it' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_executor_names_a_module_the_engine_does_not_declare() {
    # This predicate decides what is allowed by asking which modules the facade composes. A
    # reach it cannot resolve against the engine's own vocabulary is one it cannot judge, and
    # judging it green would be the wrong half of that uncertainty.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|engine::progress::ProgressSink|engine::progresses::ProgressSink|' \
        "$(_executor "$tree")" || return 0
    gate_red_because "declares no module ${tick}progresses${tick}" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_facade_composes_a_module_the_engine_does_not_declare() {
    # The derived population's own integrity. If the facade's calls stop resolving to modules
    # the engine declares, the encapsulated set is half-read — and a half-read set is a shorter
    # list of things the executor may not name.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|crate::pairing::|crate::pairings::|g' "$(_facade "$tree")" || return 0
    gate_red_because "calls into ${tick}crate::pairings${tick} and crates/engine/src/lib.rs declares no such module" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_facade_declares_no_impl_block() {
    # The anchor the whole population hangs from. A block renamed, made generic or wrapped in a
    # macro leaves this predicate reading nothing, which must be a failure and never a pass.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^impl Facade {$|impl Facade<'"'"'a> {|' "$(_facade "$tree")" || return 0
    gate_red_because "declares no ${tick}impl Facade {${tick} block" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_facade_exports_nothing() {
    # An `impl Facade` with no `pub fn` in it: the population is empty, so every reach in the
    # executor is allowed and the gate would pass by comparing nothing.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^    pub fn |    fn |' "$(_facade "$tree")" || return 0
    gate_red_because 'examined zero facade exports' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_facade_composes_nothing() {
    # The other empty population: verbs that call into no engine module encapsulate none, so the
    # executor may name anything. This is what a facade rewritten to delegate through some other
    # spelling would look like to this reader, and it has to be a finding.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|crate::|krate::|g' "$(_facade "$tree")" || return 0
    gate_red_because 'examined zero engine modules the facade encapsulates' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_executor_reaches_the_engine_nowhere() {
    # The third empty population. An executor naming no `engine::` path at all is not a subject
    # this predicate has anything to say about — most likely it has been renamed or moved — and
    # a gate that congratulated it would be counting its own blindness as compliance.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|engine::|engyne::|g' "$(_executor "$tree")" || return 0
    gate_red_because 'examined zero engine reaches in the executor' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_engines_module_declarations_cannot_be_read() {
    # No vocabulary to check the policy names against. Every declared lifecycle would report as
    # a module the engine no longer has, which is loud — and the point of the arm is that the
    # loudness is a failure rather than a pass over an unreadable input.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^pub mod |pub  mod |' "$(_engine_lib "$tree")" || return 0
    gate_red_because "declares no ${tick}pub mod${tick} this reader can see" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_executor_cannot_be_classified() {
    # Two `#[cfg(test)]` markers: the file's product half cannot be told from its test half, so
    # a bypass could sit in either. `unsafe-scope.sh` charges the same price for the same reason
    # — a boundary with no answer is a finding.
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$(_executor "$tree")" <<'RS'

#[cfg(test)]
mod one {}

#[cfg(test)]
mod two {}
RS
    gate_red_because 'crates/cli/src/main.rs carries more than one' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_facade_cannot_be_classified() {
    # The same question asked of the other file, and it matters as much: a facade whose test
    # half cannot be separated is one whose *encapsulated set* would be read out of test code.
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$(_facade "$tree")" <<'RS'

#[cfg(test)]
mod more {}
RS
    gate_red_because 'crates/engine/src/facade.rs carries more than one' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_facade_module_is_gone() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$(_facade "$tree")"
    gate_red_because 'crates/engine/src/facade.rs is not a file' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_executor_is_gone() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$(_executor "$tree")"
    gate_red_because 'crates/cli/src/main.rs is not a file' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_bypass_arrives_inside_a_grouped_import() {
    # The bare-import arm's defect wearing a brace. Measured at HEAD before the flattener
    # landed: the seeded tree passed with a summary **byte-identical** to the unseeded one,
    # because the walk matched `engine::[a-z_]…` and a `{` after the colons ended the match
    # attempt (note **N269**). A ban that names one spelling of a defect is not a ban on the
    # defect, which is note **N249**'s sentence one gate later.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^use engine::facade::Facade;$|use engine::facade::Facade;\nuse engine::{pairing, write};|' \
        "$(_executor "$tree")" || return 0
    gate_red_because "names ${tick}engine::pairing${tick}, and ${tick}engine::pairing${tick} is a module" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_bypass_arrives_inside_a_nested_group() {
    # One level further in, because the flattener is recursive and a reader that only split the
    # outermost group would report `engine::pairing::in_effect` as `engine::{pairing` — a module
    # name nothing declares, which is loud, or nothing at all, which is not.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^use engine::facade::Facade;$|use engine::facade::Facade;\nuse engine::{pairing::in_effect, write::set_requested};|' \
        "$(_executor "$tree")" || return 0
    gate_red_because "names ${tick}engine::pairing::in_effect${tick}, and ${tick}engine::pairing${tick} is a module" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_grouped_import_broken_across_lines_is_still_the_path() {
    # rustfmt writes this shape on its own once a list is long enough, so it is the form the
    # defect would actually arrive in. This is the arm that holds the *joiner*: a flattener with
    # no joiner in front of it sees `use engine::{`, then `pairing,`, then `write,`, then `};`,
    # and none of those four lines carries a path it can read.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^use engine::facade::Facade;$|use engine::facade::Facade;\nuse engine::{\n    pairing,\n    write,\n};|' \
        "$(_executor "$tree")" || return 0
    gate_red_because "names ${tick}engine::pairing${tick}, and ${tick}engine::pairing${tick} is a module" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_bypass_wears_the_package_ident_instead_of_the_lib_name() {
    # The other spelling of the crate. `crates/engine/Cargo.toml` declares `[lib] name =
    # "engine"`, so `webcam_handler_engine::pairing::…` does not resolve in this workspace
    # today — and §8.11's name sweep is a decision the owner has not made, after which it
    # would. The walk reads it as the reach it is, because `engine::pairing` sits inside the
    # longer spelling, and this arm is what says that is a property rather than an accident.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|        self.facade.controls(requested)|        let _ = webcam_handler_engine::pairing::in_effect(\&[], Vec::new());\n        self.facade.controls(requested)|' \
        "$(_executor "$tree")" || return 0
    gate_red_because "names ${tick}engine::pairing::in_effect${tick}, and ${tick}engine::pairing${tick} is a module" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_executor_binds_the_engine_crate_under_its_package_ident() {
    # And the binding wearing the same spelling: `use webcam_handler_engine as e;` is a path
    # this reader cannot follow for exactly the reason `use engine as e;` is, so the refusal
    # names both — a ban on a defect names the class, not one spelling of it.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^use engine::facade::Facade;$|use engine::facade::Facade;\nuse webcam_handler_engine as e;|' \
        "$(_executor "$tree")" || return 0
    gate_red_because 'binds the engine crate itself to a local name' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_executor_binds_the_engine_crate_to_a_local_name() {
    # The one shape flattening cannot normalise: after `use engine as e;` every `e::pairing::…`
    # is a path this reader cannot follow, and every claim below it would be satisfied by
    # blindness. The predicate's own header used to carry that as a residual it accepted; a
    # commission satisfied by not looking is the one shape a predicate here may not have, so it
    # is a refusal with a counted population instead.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^use engine::facade::Facade;$|use engine::facade::Facade;\nuse engine as e;|' \
        "$(_executor "$tree")" || return 0
    gate_red_because 'binds the engine crate itself to a local name' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_executor_binds_the_engine_crate_from_inside_a_group() {
    # The same binding, spelled the way a grouped import spells it. `self` inside
    # `engine::{…}` is the crate, and `self as e` binds it — which is why the flattener's
    # `self` rule (emit the prefix alone) is not enough on its own and the refusal is checked
    # against the unflattened statement.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^use engine::facade::Facade;$|use engine::{self as e, facade::Facade};|' \
        "$(_executor "$tree")" || return 0
    gate_red_because 'binds the engine crate itself to a local name' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_use_statement_never_closes() {
    # The joiner's own bound, and the reason it has one: an unterminated `use` would otherwise
    # swallow the rest of the file into one logical line, and every violation below it would be
    # reported at this line number. A reader that cannot tell where an import ends must say so
    # rather than answer about a line it invented.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^use engine::facade::Facade;$|use engine::facade::{Facade;|' \
        "$(_executor "$tree")" || return 0
    gate_red_because 'whose braces are still open' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_executors_sentence_drops_a_lifecycle_the_policy_still_excuses() {
    # The two copies of the same list, differing by one name. This is what the tree actually
    # shipped until 2026-08-20 — the prose said six where the policy said seven — and nothing
    # could see it, which is the whole reason the sentence is bounded by markers this predicate
    # reads (note **N269**).
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed "s|${tick}engine::calibrate${tick} and ${tick}engine::progress${tick},|${tick}engine::calibrate${tick},|" \
        "$(_executor "$tree")" || return 0
    gate_red_because 'own sentence about what it assembles does not name it' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_executors_sentence_grows_a_lifecycle_the_policy_does_not_have() {
    # The same reconciliation read the other way. A name in the prose and not in the list is a
    # reader being told a reach is blessed while nothing checks that it is, which is the softer
    # half of the same drift and the half a one-directional comparison would pass.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed "s|${tick}engine::calibrate${tick} and ${tick}engine::progress${tick},|${tick}engine::calibrate${tick}, ${tick}engine::progress${tick} and ${tick}engine::sweep${tick},|" \
        "$(_executor "$tree")" || return 0
    gate_red_because 'and the policy list at the top of this predicate does not' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_executors_policy_sentence_is_reworded_away() {
    # The marker half. A reworded sentence empties this reconciliation's population, and an
    # emptied population passing is the failure every other claim here is written against —
    # `wire-surface-sync.sh` charges the same price for a reworded D10.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|The lifecycles this file assembles itself are|The lifecycles this file keeps for itself are|' \
        "$(_executor "$tree")" || return 0
    gate_red_because 'no longer carries the sentence bounded by' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_engine_lib_target_is_renamed_out_from_under_the_walk() {
    # The walk reads the executor for `engine::` because `crates/engine/Cargo.toml` declares
    # that lib name. Rename the target and every reach in the file stops being one this reader
    # can see — the emptied-population failure again, arriving from the manifest rather than
    # from the source (§8.11: a name sweep is always its own sub-milestone).
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^name = "engine"$|name = "wch_engine"|' \
        "$tree/crates/engine/Cargo.toml" || return 0
    gate_red_because 'no longer declares' env WCH_GATE_ROOT="$tree" "$GATE"
}

pass_case_a_grouped_import_of_modules_the_facade_composes_none_of_is_allowed() {
    # The flattener's false branch, so it can go red. Two unencapsulated modules in one group
    # must still pass **and** must still print claim 6's note for each — and a flattener that
    # emitted a malformed name (`engine::{store` or `engine::store::SessionStore, session`)
    # would instead land on "declares no module", so this twin is red-able rather than
    # decorative.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^use engine::store::SessionStore;$|use engine::{store::SessionStore, session::reorder_queue};|' \
        "$(_executor "$tree")" || return 0
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_bypass_arrives_inside_a_restricted_visibility_import() {
    # The grouped-import arm's defect wearing a visibility. The repair for note **N269**
    # recognised a statement as an import only through `use` and `pub use`, so
    # `pub(crate) use engine::{pairing, write};` fell through to the plain-line reader, which
    # cannot see a brace group at all — the joiner, the flattener and the crate-binding refusal
    # were all skipped at once. Measured on a copy: exit 0, with the counted summary
    # byte-identical to the unseeded tree's (note **N271**). A ban names the class, and
    # `pub(crate)` is a spelling of `use`.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^use engine::facade::Facade;$|use engine::facade::Facade;\npub(crate) use engine::{pairing, write};|' \
        "$(_executor "$tree")" || return 0
    gate_red_because "names ${tick}engine::pairing${tick}, and ${tick}engine::pairing${tick} is a module" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_executor_binds_the_engine_crate_with_extern_crate() {
    # The crate binding wearing the other keyword. `extern crate engine as e;` is legal Rust
    # 2024, nothing in `[workspace.lints]` denies `unused_extern_crates`, and it leaves every
    # `e::pairing::…` below it a path this reader cannot follow — the same blindness
    # `use engine as e;` buys, reached through a statement the import rule did not recognise.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^use engine::facade::Facade;$|use engine::facade::Facade;\nextern crate engine as e;|' \
        "$(_executor "$tree")" || return 0
    gate_red_because 'binds the engine crate itself to a local name' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_executor_imports_the_engine_crate_wholesale() {
    # The residual the repaired header claimed it did not have. After `use engine::*;` every
    # module in the crate is reachable as a bare `pairing::in_effect(…)` that names no crate at
    # all, so the walk sees nothing and the commission is satisfied by blindness — measured on
    # a copy with the bypass beside it: exit 0, and adding the bypass to a tree that already
    # carried the glob moved not one number in the summary (note **N271**). It is not exotic
    # here — `use super::*;` appears in about ten crates — and no lint in this workspace
    # forbids it.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^use engine::facade::Facade;$|use engine::facade::Facade;\nuse engine::*;|' \
        "$(_executor "$tree")" || return 0
    gate_red_because "imports the engine crate's whole vocabulary unqualified" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_wholesale_import_arrives_inside_a_group() {
    # The same glob spelled the way a grouped import spells it. The flattener rewrites
    # `engine::{*, facade::Facade}` into `engine::*, engine::facade::Facade`, which is what
    # lets one refusal answer both spellings — and the reason the refusal is checked against the
    # *flattened* statement while the crate binding is checked against the unflattened one.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^use engine::facade::Facade;$|use engine::{*, facade::Facade};|' \
        "$(_executor "$tree")" || return 0
    gate_red_because "imports the engine crate's whole vocabulary unqualified" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_bypass_moves_into_a_second_file_of_the_executors_crate() {
    # The walk read one path until 2026-08-20, and `crates/cli/src/` holds exactly `main.rs`,
    # which is precisely why nothing had caught it: the file is four hundred lines long and the
    # natural next refactor splits it. Measured on a copy — a sibling module with the bypass in
    # it passed with the same counted summary as the unseeded tree (note **N271**). The
    # population is the directory now, so the file that arrives is walked without anybody
    # remembering to add it.
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/cli/src/verbs.rs" <<'RS'
use engine::pairing::in_effect;

pub fn plan_the_writes() {
    let _ = in_effect(&[], Vec::new());
}
RS
    gate_seed 's|^use engine::facade::Facade;$|mod verbs;\nuse engine::facade::Facade;|' \
        "$(_executor "$tree")" || return 0
    gate_red_because "names ${tick}engine::pairing::in_effect${tick}, and ${tick}engine::pairing${tick} is a module" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_second_file_of_the_executors_crate_cannot_be_classified() {
    # The product/test split is asked of every file the directory holds, not just of `main.rs`.
    # A sibling module whose boundary this reader cannot find is one that could hide a bypass in
    # either half, and refusing it is the only answer that is not a guess.
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/cli/src/verbs.rs" <<'RS'
pub fn plan_the_writes() {}

#[cfg(test)]
mod tests {}

#[cfg(test)]
mod more_tests {}
RS
    gate_seed 's|^use engine::facade::Facade;$|mod verbs;\nuse engine::facade::Facade;|' \
        "$(_executor "$tree")" || return 0
    gate_red_because 'so its product half cannot be told from its test half' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

pass_case_a_second_file_of_the_executors_crate_may_call_the_facade() {
    # The directory walk's false branch, so it can go red. A sibling module that goes through
    # `engine::facade` is the refactor this crate is entitled to make, and the walk must be
    # green on it — otherwise every arm above is red for a reason that has nothing to do with
    # its seed, and the population would be punishing the split rather than the bypass.
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/cli/src/verbs.rs" <<'RS'
use engine::facade::Facade;

pub fn list_them(facade: &Facade) {
    let _ = facade.list();
}
RS
    gate_seed 's|^use engine::facade::Facade;$|mod verbs;\nuse engine::facade::Facade;|' \
        "$(_executor "$tree")" || return 0
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_facade_composes_a_lifecycle_from_a_private_helper() {
    # The encapsulated population is what a *verb* composes, and a verb's assembly is not
    # confined to its own body: a private helper it calls is still the facade naming the module.
    # A walk that read only `pub fn` bodies would let an assembly move one line down and leave
    # the population silently — the same shrink-rather-than-fail shape note **N271** measured
    # next door. `Facade::context` already reaches `crate::profile::kernel_release` from exactly
    # such a helper, so this is the tree's own shape rather than an invented one.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|    fn context(&self, capturer: &str, now: Stamp) -> crate::profile::CaptureContext {|    fn recording(\&self) {\n        let _ = crate::record::run();\n    }\n\n    fn context(\&self, capturer: \&str, now: Stamp) -> crate::profile::CaptureContext {|' \
        "$(_facade "$tree")" || return 0
    gate_red_because 'as a lifecycle D18 keeps out of the facade' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}
