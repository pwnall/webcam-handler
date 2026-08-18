# Both-direction cases for `wire-surface-sync.sh`.
#
# One failing arm per copy of the fact and per way a copy can go quiet: a design that names
# another namespace (the sentence that shipped for three days), a macro renamed with the design
# left behind, a doc comment in the code that disagrees, a method the design has no sentence for
# (the shape `calibrate_restore` sat in from P4a to G6), the abbreviation that hid it, a method
# the design invents, a design that stops naming a namespace at all, a marker or an anchor
# reworded away so the population empties, a second wire surface, the committed document under
# another prefix, and each of the two facts gone entirely.
#
# Every arm names the sentence it is claiming (`gate_red_because`), because a seeded rename is
# red under three branches of this predicate at once — the design's claim, the code's sentences
# and the committed document's prefix — and an arm reading only the exit status cannot tell
# which one fired.
#
# shellcheck shell=bash

# A backtick, as a variable, for `browser-pins-sync.sh`'s reason: every seed and every claimed
# sentence below quotes a backticked name, and one written inside single quotes reads to the
# linter as an unexpanded command substitution.
tick='`'

_design() {
    printf '%s' "$1/docs/12-claude-fable-design-v3.md"
}

_surface() {
    printf '%s' "$1/crates/api/src/lib.rs"
}

pass_case() {
    "$GATE"
}

pass_case_the_design_may_wrap_between_the_label_and_the_list() {
    # A shape the predicate must allow. D10's method list wraps across ten lines already, and
    # where a paragraph happens to break is not something a name check may have an opinion
    # about; a line-by-line implementation would go red here.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/Methods (namespace/Methods\n(namespace/' "$(_design "$tree")" || return 0
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_design_names_the_namespace_a_rename_sweep_gave_it() {
    # The sentence that shipped. N90's sweep replaced `wch` with `webcam-handler-cli` across the
    # binary names and took D10 with it, and a client author reading D10 sends
    # `webcam-handler-cli_list` and is answered "method not found".
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed "s/(namespace ${tick}wch${tick})/(namespace ${tick}webcam-handler-cli${tick})/" \
        "$(_design "$tree")" || return 0
    gate_red_because "says the namespace is ${tick}webcam-handler-cli${tick}" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_macro_was_renamed_and_the_design_left_behind() {
    # The same disagreement from the other side, and the one the direction rule is for: the
    # macro is what jsonrpsee registers, so the design is still the file to edit.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/namespace = "wch";/namespace = "wchapi";/' "$(_surface "$tree")" || return 0
    gate_red_because "declares ${tick}wchapi${tick}" env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_doc_comment_in_the_code_names_another_namespace() {
    # Two of the three sentences the G6 review found were in Rust, not in the design: one inside
    # the invocation nine lines below the literal it contradicted, and this one. Neither is
    # reachable from D10, and nothing could go red on either.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed "s/the namespace is ${tick}wch${tick}/the namespace is ${tick}webcam-handler-cli${tick}/" \
        "$tree/crates/daemon/tests/uds.rs" || return 0
    gate_red_because 'crates/daemon/tests/uds.rs' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_macro_declares_a_method_the_design_has_no_sentence_for() {
    # Live in the shipped tree until G6: `calibrate_restore` was declared at P4a as the P3
    # review's one surface change and D10 named seven calibrate verbs for two phases after.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed "s/${tick}calibrate_restore${tick}, //" "$(_design "$tree")" || return 0
    gate_red_because "declares ${tick}calibrate_restore${tick} and" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_design_abbreviates_a_family_of_verbs() {
    # How the omission above hid: a shorthand is a member this predicate cannot see, so the
    # abbreviation is itself the defect and not merely the place one lived.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed "s/${tick}calibrate_[a-z]*${tick}/${tick}calibrate_*${tick}/g" \
        "$(_design "$tree")" || return 0
    gate_red_because "declares ${tick}calibrate_apply${tick} and" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_design_names_a_method_the_macro_does_not_declare() {
    # The other direction of the same population, and the one that costs a consumer a call: a
    # method a client is told to make and the daemon does not answer.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed "s/${tick}calibrate_list${tick},/${tick}calibrate_list${tick}, ${tick}calibrate_forget${tick},/" \
        "$(_design "$tree")" || return 0
    gate_red_because "names ${tick}calibrate_forget${tick}" env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_design_states_no_namespace_at_all() {
    # The silent case. A decision that stopped saying which namespace the wire uses is not a
    # decision that agrees with the macro.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed "s/Methods (namespace ${tick}wch${tick}):/Methods:/" "$(_design "$tree")" || return 0
    gate_red_because 'states no namespace for the wire surface' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_d10s_marker_was_reworded_away() {
    # A reworded decision empties the population, and an emptied population is what lets a
    # reconciler pass by examining nothing.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/One wire surface, one home (T5)/The wire/' "$(_design "$tree")" || return 0
    gate_red_because 'no longer carries the marker' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_method_lists_closing_anchor_is_gone() {
    # The same failure at the other end: without the anchor the list runs to the end of the
    # document and every backticked identifier in the design becomes a method claim.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/Binary results cross/Binary payloads cross/' \
        "$(_design "$tree")" || return 0
    gate_red_because 'cannot tell where the list ends' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_second_wire_surface_appears() {
    # D10 is one wire surface. A second invocation is a second surface and this predicate has no
    # way to say which of them the design describes, so it refuses rather than picking one.
    local tree
    tree="$(gate_scratch_tree)"
    printf 'wire_surface! {\n    namespace = "other";\n}\n' >"$tree/crates/api/src/second.rs"
    gate_red_because 'invocation(s) in the tree' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_committed_document_publishes_another_prefix() {
    # The artifact a consumer reads instead of the trait. `schema-artifacts-current.sh` proves
    # it matches what xtask emits today; what it cannot say is that the prefix in it is the one
    # D10 promises, because both would move together under a rename.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/"wch_list"/"wcb_list"/' "$tree/schemas/webcam-handler-openrpc.json" || return 0
    gate_red_because "publishes 'wcb_list'" env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_macro_no_longer_declares_a_namespace() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed '/namespace = "wch";/d' "$(_surface "$tree")" || return 0
    gate_red_because 'no longer declares' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_design_document_is_gone() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$(_design "$tree")"
    gate_red_because 'there is nothing to reconcile against' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_committed_document_is_gone() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$tree/schemas/webcam-handler-openrpc.json"
    gate_red_because 'without a Rust toolchain is missing' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}
