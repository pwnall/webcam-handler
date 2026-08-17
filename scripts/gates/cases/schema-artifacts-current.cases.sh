# Both-direction cases for `schema-artifacts-current.sh`.
#
# The drift and orphan arms run the *real* emitter — against a scratch tree where the seed
# is a file, and against a build directory of the arm's own where the seed is Rust (see
# `_isolated_target_dir`, which is where that distinction is paid for). They are the arms
# that prove the gate compares what it claims to.
#
# The remaining arms exercise the paths that must not be swallowed: an emitter that fails,
# and an xtask that no longer says where its artifacts go.
#
# ## Why the OpenRPC document gets four arms of its own
#
# The predicate names no filename — it walks the emitted tree and the committed directory
# — so the OpenRPC document joined the comparison at P4a by being written, with no
# widening of the script. "Covered by construction" is exactly the kind of sentence this
# suite exists to distrust, so it is spent here instead: a stale committed document, one
# emitted and never committed, one committed that nothing emits any more, and the drift
# docs/9's P4 row actually commissions — *drift from the wire trait*, seeded in
# `crates/api` and discovered by the real emitter rather than by an edit to the file it
# writes.
#
# ## Why the arms assert the message and not only the status
#
# `selftest.sh`'s verdict is an arm's exit status, and several of these seeds are red
# under more than one branch: dropping an emission makes the old name an orphan, and a
# hand-edited file is stale whichever artifact it is. An arm reading only the status
# cannot tell which branch fired, so a branch could rot to unreachable while its arm
# stayed comfortably non-zero — a gate green while checking less than it claims, which is
# the family note N10 was paid for. `gate_red_because` — `lib.sh`'s, since the P5 review gave it
# a third caller and two copies of a law is the pair that stops agreeing — therefore takes the
# message the arm is
# claiming, and reports *green* when the predicate went red for some other reason, because
# green is how this harness says "look at me".
#
# shellcheck shell=bash

pass_case() {
    "$GATE"
}

_shared_target_dir() {
    printf '%s/target\n' "$(gate_root)"
}

# A build directory of this arm's own — for arms that edit **Rust**, and the reason is a
# trap worth naming.
#
# Cargo decides freshness by mtime, and `gate_scratch_tree` copies with tar, which
# preserves mtimes: a scratch tree's files look exactly as old as the checkout's. So a
# build of *mutated* sources landing in the repository's own `target/` is reused by the
# next `cargo run` over the pristine checkout, whose files are no newer than the
# fingerprint that build just wrote. The seeded defect would escape the arm and become the
# repository's build until somebody touched a file — measured, not feared. Arms that edit
# JSON share `target/` because they change no input cargo looks at; arms that edit Rust
# get one of these, under `target/` so the tree-walking gates and the scratch copies prune
# it (the same reasoning `.cargo/mutants.toml` gives for `target/mutants.out`).
_isolated_target_dir() {
    local dir
    dir="$(gate_root)/target/gate-selftest/$1"
    mkdir -p "$dir"
    printf '%s\n' "$dir"
}

fail_case_committed_artifact_is_stale() {
    local tree
    tree="$(gate_scratch_tree)"
    printf '{ "hand": "edited" }\n' >"$tree/schemas/webcam-handler-schema.json"
    gate_red_because 'schemas/webcam-handler-schema.json is stale' \
        env "CARGO_TARGET_DIR=$(_shared_target_dir)" "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_committed_artifact_nothing_emits() {
    local tree
    tree="$(gate_scratch_tree)"
    printf '{ "openrpc": "1.3.2" }\n' >"$tree/schemas/openrpc.json"
    gate_red_because 'schemas/openrpc.json is committed but nothing emits it' \
        env "CARGO_TARGET_DIR=$(_shared_target_dir)" "WCH_GATE_ROOT=$tree" "$GATE"
}

# An emitter that cannot run is not evidence that the artifacts are current.
fail_case_emitter_fails() {
    gate_red_because 'the artifact emitter exited 1' \
        env "WCH_GATE_EMITTER=/bin/false" "$GATE"
}

# An emitter that runs and produces nothing must not read as "nothing has drifted".
#
# The sentence claimed is the *vacuity* one and not either of the two orphan lines beside
# it, because those are `fail_case_committed_artifact_nothing_emits`'s subject — one file
# with no generator — and this arm's subject is an emitter that generates none of them, so
# the comparison has no population at all. Claiming an orphan line here would make two arms
# assert the same branch and leave the branch that only this arm reaches unclaimed, which is
# the shape L25 is about.
fail_case_emitter_produces_nothing() {
    gate_red_because 'examined zero generated artifacts' \
        env "WCH_GATE_EMITTER=/bin/true" "$GATE"
}

# The branch that used to be a named skip, and the one arm no other seed reaches: every
# other case deletes or edits at most one file, so `committed_count` was never zero and
# nothing could tell that the empty-directory path answered PASS. A bad merge or a
# .gitignore edit takes both artifacts at once, which is precisely when this gate must
# speak.
fail_case_nothing_is_committed_at_all() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -rf "$tree/schemas"
    gate_red_because 'examined zero committed artifacts' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_xtask_no_longer_declares_its_artifact_directory() {
    local tree
    tree="$(gate_scratch_tree)"
    grep -v '^const ARTIFACT_DIR' "$tree/xtask/src/main.rs" >"$tree/xtask/src/main.rs.seeded"
    mv "$tree/xtask/src/main.rs.seeded" "$tree/xtask/src/main.rs"
    gate_red_because 'xtask no longer declares ARTIFACT_DIR' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

# ------------------------------------------------- the OpenRPC document, one arm per way

# Direction one of docs/9's P4 row: the committed document no longer says what the emitter
# says. Hand-edited here, which is how a document actually goes stale — somebody fixes a
# summary in the JSON instead of in the doc comment it was generated from.
fail_case_the_committed_openrpc_document_is_stale() {
    local tree
    tree="$(gate_scratch_tree)"
    printf '{ "openrpc": "1.3.2", "methods": [] }\n' \
        >"$tree/schemas/webcam-handler-openrpc.json"
    gate_red_because 'schemas/webcam-handler-openrpc.json is stale' \
        env "CARGO_TARGET_DIR=$(_shared_target_dir)" "WCH_GATE_ROOT=$tree" "$GATE"
}

# The direction a generated file reaches by never being committed at all: the emitter
# writes it, and the result is left in somebody's working tree. A consumer cloning the
# repository would find the daemon's method surface undocumented.
fail_case_the_openrpc_document_is_emitted_but_not_committed() {
    local tree
    tree="$(gate_scratch_tree)"
    rm "$tree/schemas/webcam-handler-openrpc.json"
    gate_red_because 'schemas/webcam-handler-openrpc.json is emitted but not committed' \
        env "CARGO_TARGET_DIR=$(_shared_target_dir)" "WCH_GATE_ROOT=$tree" "$GATE"
}

# The real emitter, minus one file — a stand-in for an xtask that has stopped writing the
# OpenRPC document while the committed copy survives it.
#
# The subtraction is applied to `xtask generate`'s output rather than to its source
# because deleting the write in a scratch tree would mean building mutated Rust, and the
# claim under test needs no rebuild: the branch is about the *committed* side of the
# comparison. What runs is still the shipped emitter (rubric rule 6), so the bundle it
# writes has to match the committed bundle for this arm to be about the OpenRPC document
# at all — a real drift anywhere else would surface as a different message and
# `gate_red_because` would refuse the arm.
_emitter_without_the_openrpc_document() {
    local root script
    root="$(gate_root)"
    script="$(mktemp "$(gate_scratch_root)/wch-emitter.XXXXXXXX")"
    # `--out` stands in for the repository root since P6e, so the emitted OpenRPC document is
    # under `schemas/` in the stand-in's output exactly as it is in the tree.
    cat >"$script" <<SH
#!/usr/bin/env bash
set -euo pipefail
cargo run --locked --offline --quiet --manifest-path "$root/Cargo.toml" \\
    -p webcam-handler-xtask -- generate --out "\$1" >/dev/null
rm -f "\$1/schemas/webcam-handler-openrpc.json"
SH
    chmod +x "$script"
    printf '%s\n' "$script"
}

# The orphan direction, for this document specifically: nothing emits it any more and the
# committed copy is still there. From that moment the file is hand-maintained while
# wearing a generated file's name, and nothing would ever disagree with it again — which
# is why the predicate treats "committed with no generator" as a failure rather than as
# one fewer thing to check.
fail_case_the_openrpc_document_is_committed_but_nothing_emits_it() {
    local emitter
    emitter="$(_emitter_without_the_openrpc_document)"
    gate_red_because 'schemas/webcam-handler-openrpc.json is committed but nothing emits it' \
        env "WCH_GATE_EMITTER=$emitter" "$GATE"
}

# The direction docs/9:146 names — "OpenRPC drift **from the wire trait**" — and the one
# no edit under `schemas/` can stand in for. The seed is a method's wire name in
# `crates/api/src/lib.rs`; nothing under `schemas/` is touched, and the gate must still go
# red, because the committed document describes a surface the trait no longer has.
#
# Only the real emitter can make that judgement, which is why this arm builds (rubric rule
# 6, paid for by note N10): a stub emitter would encode our belief that the document
# tracks the trait, and that belief is the thing under test. The mutation is chosen so the
# crate still *compiles* — it edits the `#[method(name = ..)]` literal and leaves the Rust
# function name alone — because an emitter that failed to build would take the
# `emitter exited` branch and prove the wrong thing. `gate_red_because` is what notices if it
# ever does.
fail_case_the_wire_trait_moved_and_the_openrpc_document_did_not() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/#\[method(name = "profile_capture")\]/#[method(name = "capture_profile")]/' \
        "$tree/crates/api/src/lib.rs"
    gate_red_because 'schemas/webcam-handler-openrpc.json is stale' \
        env "CARGO_TARGET_DIR=$(_isolated_target_dir wire-drift)" \
        "WCH_GATE_ROOT=$tree" "$GATE"
}
