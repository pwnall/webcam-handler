# Both-direction cases for `profile-partition-is-closed.sh`.
#
# The predicate has one job — prove that D15's partitions are still *destructured* — and the
# reason it needs this many arms is that a destructuring can stop being one in more ways than it
# can be one. Three of the arms below seed a shape it must **allow**, because a gate over source
# text that went red on a field somebody assigned a side correctly would be a gate somebody turns
# off: a field bound to `_` is an exclusion made visible (`CameraInfo` binds `id` that way and
# says so), and a field added to a struct *and* to its pattern is the whole workflow this gate
# exists to make mandatory rather than one to punish.
#
# The failing arms walk the branches in the order a real regression would produce them: the field
# added on one side only, the pattern simplified into field access, the `..` that keeps the
# pattern's shape and drops its meaning, one of the two sides of `compare` quietly stopping, and
# then the four declared anchors — struct, function, `impl` block, file — each renamed or removed
# so the population would empty rather than the check failing.
#
# Every arm names the sentence it is claiming, because several of these seeds are red under more
# than one branch by construction: a `..` that replaces a field leaves that field unnamed as well,
# and a renamed struct is also a struct whose fields cannot be read. An arm reading only the exit
# status could not tell which of the two fired, which is the way a branch rots to unreachable
# while its arm stays comfortably non-zero.
#
# shellcheck shell=bash

# A backtick, as a variable, for `browser-pins-sync.sh`'s reason: every claimed sentence below
# quotes a backticked name, and one written inside single quotes reads to the linter as an
# unexpanded command substitution.
tick='`'

_profile() {
    printf '%s' "$1/crates/schema/src/profile.rs"
}

_camera() {
    printf '%s' "$1/crates/schema/src/camera.rs"
}

pass_case() {
    "$GATE"
}

pass_case_a_field_bound_to_underscore_counts_as_assigned() {
    # The shape `CameraInfo::differing_fields` ships: `id: _` is how that function makes its two
    # deliberate exclusions visible "instead of being inferred from what is missing from it". The
    # shipped tree already contains one, so this seeds a *second* — a field added to the struct and
    # bound to `_` in the pattern — to prove the acceptance is a rule this predicate holds rather
    # than an accident of which fields exist today.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^    pub backend: crate::backend::BackendKind,$|&\n    pub seeded_exclusion: bool,|' \
        "$(_camera "$tree")" || return 0
    gate_seed 's|^            backend,$|&\n            seeded_exclusion: _,|' \
        "$(_camera "$tree")" || return 0
    WCH_GATE_ROOT="$tree" "$GATE"
}

pass_case_a_field_added_to_the_struct_and_to_its_pattern_stays_green() {
    # The workflow this gate exists to make mandatory, performed correctly: a fourth description
    # section on `DeviceDifference`, named in `sections`'s pattern the way the other three are.
    # This is the arm that says the predicate is a check on the mechanism and not a freeze on the
    # struct — without it, "add a field" and "add a field without deciding" would be indistinguish-
    # able from the outside.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^    pub measured_pairs: bool,$|&\n    pub seeded_side: bool,|' \
        "$(_profile "$tree")" || return 0
    gate_seed '/^impl DeviceDifference {$/,/^}$/ s|^            measured_pairs,$|&\n            seeded_side,|' \
        "$(_profile "$tree")" || return 0
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_field_is_added_to_the_struct_and_not_to_the_pattern() {
    # The defect D15's destructuring exists to make impossible, seeded at the one struct whose
    # partition is the design's own example. In the real tree the compiler catches this; here the
    # seed is text, and what the arm proves is that the *gate* catches it too — which is what
    # matters on the day the pattern has been simplified and the compiler no longer would.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^    pub measured_pairs: Vec<AutomationPair>,$|&\n    pub seeded_side: bool,|' \
        "$(_profile "$tree")" || return 0
    gate_red_because "without naming its field ${tick}seeded_side${tick}" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_destructuring_is_simplified_into_field_access() {
    # The regression this predicate was written for, in its purest form: somebody reads
    # `let Self { formats, controls, measured_pairs } = self;`, sees three bindings used once
    # each, and replaces the pattern with field access. It compiles, every test passes, and the
    # partition is open from that commit onwards.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed '/^impl DeviceDifference {$/,/^}$/ s|^        let Self {$|        let formats = \&self.formats;|' \
        "$(_profile "$tree")" || return 0
    gate_red_because "binds ${tick}Self${tick} in 0 ${tick}let${tick} pattern(s)" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_rest_pattern_absorbs_the_next_field() {
    # The same defect wearing the pattern's own clothes, and the reason the `..` claim is red on
    # sight rather than only when a field goes missing: the destructuring is still there, it still
    # reads as closed to a reviewer, and the compiler has stopped asking. By the time a field is
    # added under this, nothing anywhere would say so.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed '/^impl DeviceDifference {$/,/^}$/ s|^            measured_pairs,$|            ..|' \
        "$(_profile "$tree")" || return 0
    gate_red_because "with a ${tick}..${tick} rest pattern" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_one_side_of_the_compare_stops_destructuring() {
    # `DeviceProfile::compare` binds `ProfileInvariant` twice — once per profile — and this is the
    # half of the claim a field walk alone would miss: the first pattern still names all four
    # fields, so every field is assigned a side, and the other profile is read through
    # `other.invariant.…` with nothing to stop a fifth field being forgotten on that side. The
    # range is anchored on the *closing* line of the first pattern so that only the second one is
    # seeded.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed '/^        } = &self.invariant;$/,/^        } = &other.invariant;$/ s|^        let ProfileInvariant {$|        let other_invariant = \&other.invariant;|' \
        "$(_profile "$tree")" || return 0
    gate_red_because "in 1 ${tick}let${tick} pattern(s) and D15's partition" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_struct_is_renamed() {
    # A rename must fail loudly and never empty the population: the field list *is* the population,
    # so a struct this predicate can no longer find is a partition it can no longer say anything
    # about — which, without this branch, would read as "nothing to report".
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^pub struct DeviceDifference {$|pub struct DescriptionDifference {|' \
        "$(_profile "$tree")" || return 0
    gate_red_because "declares no ${tick}pub struct DeviceDifference {${tick}" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_struct_declares_no_fields() {
    # The emptied-population arm from the other end: the declaration is still there and this reader
    # can see nothing in it. A partition over an empty struct is one every pattern satisfies, so a
    # predicate that stayed green here would be green precisely when it had stopped reading.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed '/^pub struct DeviceDifference {$/,/^}$/{/^    pub /d}' \
        "$(_profile "$tree")" || return 0
    gate_red_because 'with no fields this reader can see' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_function_is_renamed() {
    # The function is a declared anchor for the same reason the struct is, and it is the anchor
    # most likely to move: `sections` is a name two types in this file already share, so a third
    # spelling is an ordinary refactor — and one that must take this predicate's row with it.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed '/^impl DeviceDifference {$/,/^}$/ s|    pub fn sections(|    pub fn which_sections(|' \
        "$(_profile "$tree")" || return 0
    gate_red_because "declares no ${tick}fn sections${tick}" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_impl_block_is_renamed() {
    # The third anchor, and the one that carries the predicate's one piece of real inference: the
    # `impl` a function lives in is not the struct being destructured — `DeviceProfile::compare`
    # destructures `ProfileInvariant` — so the block name is declared policy and a rename of it
    # leaves the function unfindable rather than merely differently spelled.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's|^impl DeviceProfile {$|impl DeviceProfileParts {|' \
        "$(_profile "$tree")" || return 0
    gate_red_because "has no ${tick}impl DeviceProfile {${tick} block" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_declared_file_is_gone() {
    # A boundary with no answer is a finding rather than a pass. A file that moved takes two rows'
    # worth of partition out of this predicate's sight, and the remedy is a repointed row in the
    # same commit as the move.
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$(_camera "$tree")"
    gate_red_because 'is not a file, so the partition' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}
