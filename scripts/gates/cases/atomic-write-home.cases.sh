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
# Each arm names the sentence it is claiming (`gate_red_because`, note **N31**), and this file is
# where that rule found a broken arm rather than a weak one: `fail_case_nothing_to_scan` deleted
# every `.rs` file in the copy, which is a workspace `cargo metadata` will not load, so the
# predicate exited **101** from `gate_metadata` before it reached the population check the arm is
# named for — and 101 is non-zero, which is all the harness used to ask. Note **N241** has the
# account. Six of the arms below also share one sentence with a different file name in it, which
# is the other half of why the claim is worth writing down.
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
    gate_red_because 'crates/engine/src/sneaky.rs names the state directory' \
        env WCH_GATE_ROOT="$tree" "$GATE"
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
    gate_red_because 'crates/cli/src/oops.rs names the state directory' \
        env WCH_GATE_ROOT="$tree" "$GATE"
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
    gate_red_because 'crates/daemon/src/journal.rs names the state directory' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# The P3-review widening, proven: the same bypass spelled with std's *aliases* for the
# primitives the pattern already caught. `File::options()` is `OpenOptions::new()` and
# `File::create_new()` is `File::create()` with `O_EXCL`; before this arm the gate gave
# two byte-identical bypasses opposite verdicts because of how the open was spelled.
fail_case_bypass_spelled_with_file_options() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/daemon/src/journal_options.rs" <<'RS'
//! Seeded by the gate selftest: the same bypass, spelled the short way.
use std::io::Write;

pub fn note(dir: &camino::Utf8Path, line: &str) {
    let mut f = std::fs::File::options()
        .append(true)
        .create(true)
        .open(dir.join(schema::limits::SESSION_LOG_FILE))
        .unwrap();
    let _ = f.write_all(line.as_bytes());
}
RS
    gate_red_because 'crates/daemon/src/journal_options.rs names the state directory' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_bypass_spelled_with_file_create_new() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/daemon/src/journal_new.rs" <<'RS'
//! Seeded by the gate selftest: the same bypass, with O_EXCL on it.
use std::io::Write;

pub fn note(dir: &camino::Utf8Path, line: &str) {
    let mut f = std::fs::File::create_new(dir.join("log.ndjson")).unwrap();
    let _ = f.write_all(line.as_bytes());
}
RS
    gate_red_because 'crates/daemon/src/journal_new.rs names the state directory' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# The P4e-i widening, proven: the bypass spelled the way that sub-milestone taught this
# workspace to open a file for writing — through `rustix`, with the descriptor turned into
# a `std::fs::File`. Not one character of the pattern's std spellings appears in it, so
# before the widening this file was a session write nothing could see.
fail_case_bypass_spelled_with_rustix_flags() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/daemon/src/journal_rustix.rs" <<'RS'
//! Seeded by the gate selftest: the same bypass, opened the way P4e-i opens a photo's
//! destination.
use std::io::Write as _;

pub fn note(dir: &camino::Utf8Path, line: &str) {
    let fd = rustix::fs::open(
        dir.join("log.ndjson").as_std_path(),
        rustix::fs::OFlags::WRONLY | rustix::fs::OFlags::CREATE,
        rustix::fs::Mode::from_bits_truncate(0o600),
    )
    .unwrap();
    let _ = std::fs::File::from(fd).write_all(line.as_bytes());
}
RS
    gate_red_because 'crates/daemon/src/journal_rustix.rs names the state directory' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# The direction the widening must NOT take with it: `rustix::fs::open` is not itself a
# write. `daemon::uds::SocketDir` opens the runtime directory `O_PATH | O_DIRECTORY` with
# that function and names `XDG_RUNTIME_DIR` while doing it, so a pattern that matched the
# call rather than the flags would call the socket bind a state-directory bypass.
pass_case_a_read_only_rustix_open_beside_the_runtime_dir() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/daemon/src/dirfd.rs" <<'RS'
//! Seeded by the gate selftest: a directory held as a descriptor, which writes nothing.
pub fn hold(runtime_dir: &camino::Utf8Path) -> rustix::fd::OwnedFd {
    rustix::fs::open(
        runtime_dir.as_std_path(),
        rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::PATH,
        rustix::fs::Mode::empty(),
    )
    .unwrap()
}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_nothing_to_scan() {
    local tree md
    tree="$(gate_scratch_tree)"
    find "$tree" -name '*.rs' -delete
    # The graph is handed in, and that is not convenience. A workspace whose crate roots have
    # been deleted is one `cargo metadata` refuses to load, so the predicate exited 101 from
    # `gate_metadata` — three lines before the population check this arm is named for — and the
    # harness read 101 as "it went red" (note **N241**). Seeding through the documented seam is
    # `unsafe-scope.cases.sh`'s reasoning for its missing-crate-root arm: what goes red has to be
    # the predicate and not its input. The snapshot is re-pointed at the copy, so the engine the
    # metadata names is the engine whose sources are gone.
    md="$(gate_metadata_snapshot)"
    sed "s|$(gate_root)|$tree|g" "$md" >"$md.seeded"
    gate_red_because 'examined zero Rust source files' \
        env WCH_GATE_ROOT="$tree" WCH_GATE_METADATA="$md.seeded" "$GATE"
}

# Half two: the home itself is gone. Before P3a this was the tree's actual state, and the
# predicate reported a named skip; it must now be a failure.
fail_case_the_home_is_missing() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -rf "$tree/crates/engine/src/store.rs" "$tree/crates/engine/src/store"
    gate_red_because 'design §2.10 names it as the one home for atomic state writes' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# Half two: the home exists and has stopped being atomic — the rename is gone, so a
# reader can see a half-written document even though nothing "bypassed" anything.
fail_case_the_home_stopped_renaming() {
    local tree store
    tree="$(gate_scratch_tree)"
    store="$tree/crates/engine/src/store.rs"
    gate_seed 's/\.persist(/\.keep_but_do_not_rename(/g' "$store"
    gate_red_because 'the rename is what publishes the document' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# Half two: the home stopped taking the one advisory lock, so D9's cross-process safety
# has no mechanism left.
fail_case_the_home_stopped_locking() {
    local tree store
    tree="$(gate_scratch_tree)"
    store="$tree/crates/engine/src/store.rs"
    gate_seed 's/fd_lock::RwLock/some_other::Thing/g' "$store"
    gate_red_because 'the one advisory lock is taken with fd-lock' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# Half two: `write_json_atomic` itself renamed away, which is how the §2.10 home stops
# existing without anybody deleting a file.
fail_case_the_home_lost_its_named_function() {
    local tree store
    tree="$(gate_scratch_tree)"
    store="$tree/crates/engine/src/store.rs"
    gate_seed 's/pub fn write_json_atomic/pub fn save_the_json/g' "$store"
    gate_red_because 'write_json_atomic is defined' env WCH_GATE_ROOT="$tree" "$GATE"
}
