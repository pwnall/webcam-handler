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
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^pub mod sweep;$|pub mod sweeps;|' "$(_engine_lib "$tree")" || return 0
    gate_red_because "excuses the executor's reach into ${tick}engine::sweep${tick}" \
        env WCH_GATE_ROOT="$tree" "$GATE"
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
