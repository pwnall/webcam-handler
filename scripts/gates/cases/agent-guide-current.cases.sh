# Both-direction cases for `agent-guide-current.sh`.
#
# The predicate's subject is a **generated manual**, so the interesting arm is not "somebody
# edited the Markdown" — it is *the command surface moved and the guide did not*, which is the
# drift docs/9's agent-guide row actually commissions and the only one a stub emitter cannot
# stand in for. `fail_case_the_command_surface_moved_and_the_guide_did_not` seeds a flag's
# value name in `crates/cli-core` and touches nothing under `docs/`; the real generator has to
# notice, which means it has to be built (rubric rule 6, paid for by note N10).
#
# The cheaper arms use the documented `$WCH_GATE_EMITTER` seam or edits that the predicate
# refuses before it ever reaches cargo, and the arms that do build share the checkout's own
# `target/` unless they edit Rust — the distinction `schema-artifacts-current.cases.sh`
# explains at `_isolated_target_dir`, and it is the same trap for the same reason: cargo
# decides freshness by mtime, `gate_scratch_tree` preserves mtimes, and a build of mutated
# sources landing in the repository's `target/` would be reused by the next run over the
# pristine checkout.
#
# Arms assert the message and not only the status, because several seeds are red under more
# than one branch: a guide deleted from the tree is both uncommitted and undiffable, and an
# arm reading only the exit status could not tell which branch fired.
#
# shellcheck shell=bash

pass_case() {
    "$GATE"
}

_shared_target_dir() {
    printf '%s/target\n' "$(gate_root)"
}

_isolated_target_dir() {
    local dir
    dir="$(gate_root)/target/gate-selftest/$1"
    mkdir -p "$dir"
    printf '%s\n' "$dir"
}

# The ordinary drift, and the way it actually happens: somebody fixes a sentence in the
# Markdown instead of in the code it was generated from. The edit is in the *derived* half —
# a verb's description — so the generator disagrees with it on the next run.
fail_case_the_committed_guide_is_stale() {
    local tree
    tree="$(gate_scratch_tree)"
    sed -i 's/^List the cameras attached to this machine$/List the cameras, improved by hand/' \
        "$tree/docs/agent-guide.md"
    gate_red_because 'docs/agent-guide.md is stale' \
        env "CARGO_TARGET_DIR=$(_shared_target_dir)" "WCH_GATE_ROOT=$tree" "$GATE"
}

# The direction a generated file reaches by never being committed: the generator writes it and
# the result is left in somebody's working tree. A clone would carry no manual at all.
fail_case_the_guide_is_emitted_but_not_committed() {
    local tree
    tree="$(gate_scratch_tree)"
    rm "$tree/docs/agent-guide.md"
    gate_red_because 'docs/agent-guide.md is emitted and not committed' \
        env "CARGO_TARGET_DIR=$(_shared_target_dir)" "WCH_GATE_ROOT=$tree" "$GATE"
}

# **The claim the docs/9 row makes**, and the arm no edit under `docs/` can stand in for: the
# surface moved and the manual did not. The seed is a flag's value name in the shared command
# surface — one line, still compiles, and it reaches the guide twice (the synopsis of every
# verb that takes a stream, and each of their option tables). Nothing under `docs/` is touched,
# and the gate must still go red, because the committed guide teaches a spelling the surface
# no longer prints.
#
# Only the real generator can make that judgement, which is why this arm builds. The mutation
# is chosen so the crate still *compiles* — a value name is a string literal clap renders and
# nothing matches on — because a generator that failed to build would take the `exited`
# branch and prove the wrong thing. `gate_red_because` is what notices if it ever does.
fail_case_the_command_surface_moved_and_the_guide_did_not() {
    local tree
    tree="$(gate_scratch_tree)"
    sed -i 's/value_name = "WxH"/value_name = "WIDTHxHEIGHT"/' \
        "$tree/crates/cli-core/src/lib.rs"
    if ! grep -q 'WIDTHxHEIGHT' "$tree/crates/cli-core/src/lib.rs"; then
        printf 'selftest: the value name was not seeded\n' >&2
        return 0
    fi
    gate_red_because 'docs/agent-guide.md is stale' \
        env "CARGO_TARGET_DIR=$(_isolated_target_dir agent-guide-surface-drift)" \
        "WCH_GATE_ROOT=$tree" "$GATE"
}

# A generator that cannot run is not evidence that the manual is current.
fail_case_the_generator_fails() {
    gate_red_because 'the guide generator exited' \
        env "WCH_GATE_EMITTER=/bin/false" "$GATE"
}

# A generator that runs and writes no guide must not read as "nothing has drifted". This is
# the orphan direction for a single-file artifact: from that moment the committed file is
# hand-maintained while wearing a generated file's name, and nothing would ever disagree with
# it again.
fail_case_the_generator_writes_no_guide() {
    gate_red_because 'the generator ran and wrote no docs/agent-guide.md' \
        env "WCH_GATE_EMITTER=/bin/true" "$GATE"
}

# The path is read out of the emitter rather than written in the predicate, so the emitter
# ceasing to declare it leaves this gate with nothing to check — which is a failure and not a
# quiet pass over zero files.
fail_case_xtask_no_longer_declares_where_the_guide_goes() {
    local tree
    tree="$(gate_scratch_tree)"
    grep -v '^pub(crate) const GUIDE_PATH' "$tree/xtask/src/guide.rs" >"$tree/xtask/src/guide.rs.seeded"
    mv "$tree/xtask/src/guide.rs.seeded" "$tree/xtask/src/guide.rs"
    gate_red_because 'xtask no longer declares GUIDE_PATH' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

# ------------------------------------------------- the deprecation pointer, both directions

# docs/7 P6e's other deliverable. A pointer that still names the old path sends the reader it
# exists for to a file that is not there — the failure a forwarding address has.
fail_case_the_deprecation_pointer_names_no_guide() {
    local tree
    tree="$(gate_scratch_tree)"
    sed -i 's|docs/agent-guide.md|docs/somewhere-else.md|g' "$tree/vendor/README.md"
    gate_red_because 'does not name docs/agent-guide.md' \
        env "CARGO_TARGET_DIR=$(_shared_target_dir)" "WCH_GATE_ROOT=$tree" "$GATE"
}

# And the case where there is no pointer at all. The vendored skill is a submodule, so the
# pointer lives beside it in this repository and a `rm` is exactly how it disappears — a
# rebase that drops an untracked-looking file, a tidy-up of `vendor/`.
fail_case_nothing_beside_the_vendored_skill_points_at_the_guide() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$tree"/vendor/*.md
    gate_red_because 'nothing under vendor/ points at docs/agent-guide.md' \
        env "CARGO_TARGET_DIR=$(_shared_target_dir)" "WCH_GATE_ROOT=$tree" "$GATE"
}
