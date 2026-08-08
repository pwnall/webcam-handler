# Both-direction cases for `msrv-sync.sh`.
#
# One failing arm per copy of the fact: a member manifest, the CI workflow, the README,
# and the fact itself. A divergence in any one of them is the same defect — two answers to
# "what is the minimum Rust version" — and each has to be caught where it lives.
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
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_ci_pins_another_version() {
    local tree workflow
    tree="$(gate_scratch_tree)"
    workflow="$tree/.github/workflows/ci.yml"
    sed 's/toolchain: "1\.[0-9.]*"/toolchain: "1.75"/' "$workflow" >"$workflow.seeded"
    mv "$workflow.seeded" "$workflow"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_ci_pins_nothing() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -rf "$tree/.github/workflows"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_member_declares_its_own_version() {
    local tree manifest
    tree="$(gate_scratch_tree)"
    manifest="$tree/crates/web/Cargo.toml"
    sed 's/^rust-version\.workspace = true$/rust-version = "1.75"/' "$manifest" >"$manifest.seeded"
    mv "$manifest.seeded" "$manifest"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_readme_states_no_version() {
    local tree
    tree="$(gate_scratch_tree)"
    sed 's/Rust 1\.[0-9]* or newer/A recent Rust/' "$tree/README.md" >"$tree/README.md.seeded"
    mv "$tree/README.md.seeded" "$tree/README.md"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_fact_itself_is_gone() {
    local tree
    tree="$(gate_scratch_tree)"
    grep -v '^rust-version = ' "$tree/Cargo.toml" >"$tree/Cargo.toml.seeded"
    mv "$tree/Cargo.toml.seeded" "$tree/Cargo.toml"
    WCH_GATE_ROOT="$tree" "$GATE"
}
