# Both-direction cases for `atomic-write-home.sh`.
#
# Two halves, both driven. Half one (no bypass) has three shapes: a bypass inside the
# engine, a bypass outside it, and — the P3a addition — a bypass that never says
# `state_dir` at all and is caught only because the predicate now knows D9's session-file
# names. Half two (the home is the home) is driven by removing the home and by removing
# each property the home must show.
#
# Every arm runs the real predicate over a real copy of the tree, per rubric rule 6 and
# note N10: nothing here stubs the thing under test, so an arm cannot agree with its
# author's belief about it.
#
# shellcheck shell=bash

pass_case() {
    "$GATE"
}

# A shape the predicate must ALLOW: the home may be a directory, and its submodules are
# the home too. A store split into `src/store/` files that use raw primitives is correct.
pass_case_the_home_may_be_a_directory() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/crates/engine/src/store"
    cat >"$tree/crates/engine/src/store/atomic.rs" <<'RS'
//! Seeded by the gate selftest: the home, split into a directory.
use std::fs::{self, File, OpenOptions};

pub fn save(state_dir: &str, body: &str) {
    let _ = fs::write(format!("{state_dir}/sessions/x/session.json"), body);
    let _ = File::create(format!("{state_dir}/log.ndjson"));
    let _ = OpenOptions::new();
}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
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

# The P3a widening, proven: a bypass that never names the state directory and is caught
# purely because it names D9's session files. Before the widening this file was invisible.
fail_case_bypass_that_only_names_the_session_files() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/daemon/src/journal.rs" <<'RS'
//! Seeded by the gate selftest: a writer that knows the layout but never says state_dir.
use std::io::Write;

pub fn note(dir: &camino::Utf8Path, line: &str) {
    let mut f = std::fs::File::create(dir.join("log.ndjson")).unwrap();
    let _ = f.write_all(line.as_bytes());
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

# Half two: the home itself is gone. Before P3a this was the tree's actual state, and the
# predicate reported a named skip; it must now be a failure.
fail_case_the_home_is_missing() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -rf "$tree/crates/engine/src/store.rs" "$tree/crates/engine/src/store"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# Half two: the home exists and has stopped being atomic — the rename is gone, so a
# reader can see a half-written document even though nothing "bypassed" anything.
fail_case_the_home_stopped_renaming() {
    local tree store
    tree="$(gate_scratch_tree)"
    store="$tree/crates/engine/src/store.rs"
    sed -i 's/\.persist(/\.keep_but_do_not_rename(/g' "$store"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# Half two: the home stopped taking the one advisory lock, so D9's cross-process safety
# has no mechanism left.
fail_case_the_home_stopped_locking() {
    local tree store
    tree="$(gate_scratch_tree)"
    store="$tree/crates/engine/src/store.rs"
    sed -i 's/fd_lock::RwLock/some_other::Thing/g' "$store"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# Half two: `write_json_atomic` itself renamed away, which is how the §2.10 home stops
# existing without anybody deleting a file.
fail_case_the_home_lost_its_named_function() {
    local tree store
    tree="$(gate_scratch_tree)"
    store="$tree/crates/engine/src/store.rs"
    sed -i 's/pub fn write_json_atomic/pub fn save_the_json/g' "$store"
    WCH_GATE_ROOT="$tree" "$GATE"
}
