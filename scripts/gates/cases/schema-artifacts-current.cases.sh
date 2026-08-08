# Both-direction cases for `schema-artifacts-current.sh`.
#
# The drift and orphan arms run the *real* emitter against a scratch tree, sharing the
# repository's target directory so the rebuild is the two crates that moved rather than
# the whole graph. They are the arms that prove the gate compares what it claims to.
#
# The remaining arms exercise the paths that must not be swallowed: an emitter that fails,
# and an xtask that no longer says where its artifacts go.
#
# shellcheck shell=bash

pass_case() {
    "$GATE"
}

_shared_target_dir() {
    printf '%s/target\n' "$(gate_root)"
}

fail_case_committed_artifact_is_stale() {
    local tree
    tree="$(gate_scratch_tree)"
    printf '{ "hand": "edited" }\n' >"$tree/schemas/webcam-handler-schema.json"
    CARGO_TARGET_DIR="$(_shared_target_dir)" WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_committed_artifact_nothing_emits() {
    local tree
    tree="$(gate_scratch_tree)"
    printf '{ "openrpc": "1.3.2" }\n' >"$tree/schemas/openrpc.json"
    CARGO_TARGET_DIR="$(_shared_target_dir)" WCH_GATE_ROOT="$tree" "$GATE"
}

# An emitter that cannot run is not evidence that the artifacts are current.
fail_case_emitter_fails() {
    WCH_GATE_EMITTER=/bin/false "$GATE"
}

# An emitter that runs and produces nothing must not read as "nothing has drifted".
fail_case_emitter_produces_nothing() {
    WCH_GATE_EMITTER=/bin/true "$GATE"
}

fail_case_xtask_no_longer_declares_its_artifact_directory() {
    local tree
    tree="$(gate_scratch_tree)"
    grep -v '^const ARTIFACT_DIR' "$tree/xtask/src/main.rs" >"$tree/xtask/src/main.rs.seeded"
    mv "$tree/xtask/src/main.rs.seeded" "$tree/xtask/src/main.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}
