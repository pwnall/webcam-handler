# Both-direction cases for `atomic-write-home.sh`.
#
# The two failing arms are the two shapes a bypass takes: one inside the engine (a new
# module that reaches for the state directory itself) and one outside it (a composition
# root writing session state directly). The third proves the predicate cannot go green by
# scanning nothing.
#
# shellcheck shell=bash

pass_case() {
    "$GATE"
}

# A file inside the engine that is neither the store nor the paths module.
fail_case_bypass_inside_the_engine() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/engine/src/sneaky.rs" <<'RS'
//! Seeded by the gate selftest: a session write that dodges write_json_atomic.
use std::fs;

pub fn save(state_dir: &str, body: &str) {
    let _ = fs::write(format!("{state_dir}/session.json"), body);
}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

# A composition root writing state itself.
fail_case_bypass_outside_the_engine() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/cli/src/oops.rs" <<'RS'
//! Seeded by the gate selftest: the CLI writing into the state dir on its own.
pub fn save(session: &serde_json::Value) {
    let dir = std::env::var("XDG_STATE_HOME").unwrap_or_default();
    let file = std::fs::File::create(format!("{dir}/webcam-handler/session.json")).unwrap();
    serde_json::to_writer(file, session).unwrap();
}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_nothing_to_scan() {
    local tree
    tree="$(gate_scratch_tree)"
    find "$tree" -name '*.rs' -delete
    WCH_GATE_ROOT="$tree" "$GATE"
}
