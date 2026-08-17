# Both-direction cases for `msrv-sync.sh`.
#
# One failing arm per copy of the fact: a member manifest, the CI workflow, the README,
# and the fact itself. A divergence in any one of them is the same defect — two answers to
# "what is the minimum Rust version" — and each has to be caught where it lives.
#
# Each arm names the sentence it is claiming (`gate_red_because`, note **N31**), and this file is
# one of the two where that turned an arm over: `fail_case_member_declares_its_own_version` used
# to seed **1.75**, which is below the floor edition 2024 imposes, so `cargo metadata` refused the
# workspace outright and the arm went red on "examined zero workspace members" — the vacuity
# branch — while the member-disagrees branch it was written for had no arm at all. The seeded
# version is now above that floor and still not the MSRV, which is the whole of what the arm
# means to say.
#
# shellcheck shell=bash

pass_case() {
    "$GATE"
}

fail_case_readme_states_another_version() {
    local tree
    tree="$(gate_scratch_tree)"
    sed 's/Rust 1\.[0-9]*/Rust 1.75/' "$tree/README.md" >"$tree/README.md.seeded"
    mv "$tree/README.md.seeded" "$tree/README.md"
    gate_red_because 'README.md tells readers Rust 1.75' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_ci_pins_another_version() {
    local tree workflow
    tree="$(gate_scratch_tree)"
    workflow="$tree/.github/workflows/ci.yml"
    sed 's/toolchain: "1\.[0-9.]*"/toolchain: "1.75"/' "$workflow" >"$workflow.seeded"
    mv "$workflow.seeded" "$workflow"
    # Red under the "no workflow pins the MSRV" branch as well, because the only pin there was is
    # the one this seed moved; the arm is about the *disagreeing* pin, so that is what it names.
    gate_red_because "pins toolchain '1.75', which is neither a floating channel nor the MSRV" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_ci_pins_nothing() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -rf "$tree/.github/workflows"
    gate_red_because 'no workflow files under .github/workflows/' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_member_declares_its_own_version() {
    local tree manifest
    tree="$(gate_scratch_tree)"
    manifest="$tree/crates/web/Cargo.toml"
    # Above edition 2024's own floor and below the MSRV: a member that disagrees, rather than a
    # manifest cargo will not load. See the header.
    sed 's/^rust-version\.workspace = true$/rust-version = "1.90"/' "$manifest" >"$manifest.seeded"
    mv "$manifest.seeded" "$manifest"
    gate_red_because 'resolves to rust-version 1.90, not' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_readme_states_no_version() {
    local tree
    tree="$(gate_scratch_tree)"
    sed 's/Rust 1\.[0-9]* or newer/A recent Rust/' "$tree/README.md" >"$tree/README.md.seeded"
    mv "$tree/README.md.seeded" "$tree/README.md"
    gate_red_because 'README.md states no Rust version' env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_fact_itself_is_gone() {
    local tree
    tree="$(gate_scratch_tree)"
    grep -v '^rust-version = ' "$tree/Cargo.toml" >"$tree/Cargo.toml.seeded"
    mv "$tree/Cargo.toml.seeded" "$tree/Cargo.toml"
    gate_red_because 'the one fact has no home' env WCH_GATE_ROOT="$tree" "$GATE"
}
