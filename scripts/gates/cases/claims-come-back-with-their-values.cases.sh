# Both-direction cases for `claims-come-back-with-their-values.sh`.
#
# Three claims, each with its inverse: a claim value releases itself in a `Drop`, or it is
# `#[must_use]` and the compiler refuses to let it be ignored, and there is a population to
# ask about at all. The second *green* arm matters as much as the failing ones — a gate that
# refused the `#[must_use]` half would force a `Drop` onto `Starting`, which is a witness
# with nothing to give back, and a gate that made the right answer impossible is a gate
# somebody turns off.
#
# shellcheck shell=bash

# A claim constructor answering with a type declared beside it, appended to `crate::preview`.
# The three arms below differ only in what the type carries, which is exactly the decision
# the rule is about.
seeded_claim() {
    local carries="$1"
    cat <<RS

/// Seeded by the gate selftest: a claim on this camera's frames, in a new type.
$carries
struct SeededClaim;

impl Previews {
    /// Seeded by the gate selftest.
    fn claim_seeded(&self) -> SeededClaim {
        SeededClaim
    }
}
RS
}

pass_case() {
    "$GATE"
}

# The `Drop` half, which is what `Reserved` and `Watchers` are: a new claim value that gives
# its claim back whatever happened to the caller is allowed, and must be.
pass_case_a_new_claim_value_that_releases_itself_in_a_drop() {
    local tree preview
    tree="$(gate_scratch_tree)"
    preview="$tree/crates/daemon/src/preview.rs"
    seeded_claim '#[derive(Debug)]' >>"$preview"
    cat >>"$preview" <<'RS'

impl Drop for SeededClaim {
    fn drop(&mut self) {}
}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The `#[must_use]` half, which is what `Starting` is: a claim with nothing to give back and
# something to *do*, where the compiler at the call site is the enforcement.
pass_case_a_new_claim_value_the_compiler_will_not_let_a_caller_ignore() {
    local tree
    tree="$(gate_scratch_tree)"
    seeded_claim '#[must_use = "seeded by the gate selftest"]
#[derive(Debug)]' >>"$tree/crates/daemon/src/preview.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The defect itself: a claim value with neither. This is the shape `Watchers` had until the
# B5b repair — the value note N171's whole repair hands around, released only by `hand_back`
# running, so a hand-over task cancelled between claiming a camera's frames and stowing them
# left a feed with no publisher for the life of the process (note **N177**).
fail_case_a_new_claim_value_with_neither_a_drop_nor_a_must_use() {
    local tree
    tree="$(gate_scratch_tree)"
    seeded_claim '#[derive(Debug)]' >>"$tree/crates/daemon/src/preview.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The two `Drop`s the tree has now, each removed on its own, because they discharge different
# claims and a gate that only saw one of them would be half a gate.
fail_case_the_drop_that_gives_a_recording_slot_back_is_gone() {
    local tree record
    tree="$(gate_scratch_tree)"
    record="$tree/crates/daemon/src/record.rs"
    sed 's/^impl Drop for Reserved {/impl Reserved {/' "$record" >"$record.seeded"
    mv "$record.seeded" "$record"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_drop_that_gives_a_cameras_frames_back_is_gone() {
    local tree preview
    tree="$(gate_scratch_tree)"
    preview="$tree/crates/daemon/src/preview.rs"
    sed 's/^impl Drop for Watchers {/impl Watchers {/' "$preview" >"$preview.seeded"
    mv "$preview.seeded" "$preview"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# And the `#[must_use]` in the other direction, so the second green arm above is a permission
# rather than a hole: losing it on `Starting` leaves a feed nobody owes a `STREAMON` to.
fail_case_the_must_use_on_the_witness_that_owes_a_stream_is_gone() {
    local tree preview
    tree="$(gate_scratch_tree)"
    preview="$tree/crates/daemon/src/preview.rs"
    grep -v '^#\[must_use = "a Starting nobody acts on' "$preview" >"$preview.seeded"
    mv "$preview.seeded" "$preview"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# Non-vacuity, and it is the arm this gate would otherwise be quietly wrong in: "every claim
# value releases itself" is trivially true of two modules with no claim constructors in them,
# which is what a rename would leave behind.
fail_case_no_claim_constructor_is_left_to_ask_about() {
    local tree file
    tree="$(gate_scratch_tree)"
    for file in "$tree/crates/daemon/src/record.rs" "$tree/crates/daemon/src/preview.rs"; do
        sed -E 's/\bfn (reserve|claim|hand_over|watchers|attach)\b/fn renamed_away/g' \
            "$file" >"$file.seeded"
        mv "$file.seeded" "$file"
    done
    WCH_GATE_ROOT="$tree" "$GATE"
}

# A module the rule is about that is not in the tree at all. Seeded rather than assumed,
# because "no violations" and "no subject" print the same way to a reader who is skimming.
fail_case_a_claim_module_left_the_tree() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$tree/crates/daemon/src/preview.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}
