#!/usr/bin/env bash
#
# One home for state-directory writes (design §2.10, rubric A5):
# `webcam-handler-engine::store::write_json_atomic`, under the one fd-lock. A session
# file written with `fs::write` is a session file that can be found half-written after a
# crash, and D9's crash-recovery story assumes it cannot be.
#
# The predicate has two halves, and it grew its second at P3a (docs/9 Part 2, "Store
# bypass gate widened"):
#
#   1. **No bypass.** The population is derived in two steps rather than by naming files:
#      first find every Rust source that reaches for the state directory at all — it
#      mentions `state_dir`, `XDG_STATE_HOME`, `session_dir`, `runtime_dir`, or one of
#      D9's session-tree names (`sessions/`, `session.json`, `log.ndjson`, and the
#      `schema::limits` constants they are spelled by) — then, in each such file that is
#      not the store or the paths module, look for a raw write primitive. A module that
#      never names the state directory or its files cannot be writing into it; a module
#      that does is exactly where a bypass would live. P3a is what made the session-file
#      half of that pattern match anything.
#
#   2. **The home exists and is the home.** Until P3a there was no session store, so the
#      first half could only prove that nothing bypassed a home that did not exist — a
#      predicate green about nothing. It now asserts the home is present, that it defines
#      `write_json_atomic`, and that the atomic sequence D9 specifies (a temp file in the
#      destination directory, `sync_all`, `rename`, `fsync` of the parent) and the one
#      advisory `fd-lock` are both in it. A home that quietly stopped being atomic, or
#      stopped taking the lock, would otherwise satisfy half one forever.
#
# Honest limit, unchanged by the widening: a file that receives the state path as an
# argument and never names it, and never names a session file either, is invisible to
# half one. docs/9 records the same limit. Half two is a check on the home's own text and
# does not narrow it.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

# What "reaching for the state directory" looks like in this codebase (note N2: the two
# XDG paths are resolved by this workspace rather than by a path crate — the state half in
# `webcam-handler-engine::paths`, the runtime half in `webcam-handler-schema::paths`, split
# because only the state half is a storage fact; D9 fixes the session tree's file names,
# and `schema::limits` is where they are spelled).
#
# `write_json_atomic` is deliberately NOT in this pattern, and the first run of the
# widened gate is why: `engine::photo` names it in a doc comment explaining why a photo is
# *not* written that way. Naming the home is evidence of deference, not of a reach.
#
# The general shape, met again at the P3 review and recorded here rather than worked
# around: this is a `grep`, so **prose counts**. A module that spells `session.json` only
# in a comment joins the population, and if it also contains any raw write — a test writing
# a fixture to a temp directory, say — the gate calls it a bypass. The narrow fix (strip
# comments before matching) would cost the population its only defence against a bypass
# that names a session file and writes through a variable path, so the rule stays as it is
# and a comment that means "the session document" says that instead of naming the file.
state_dir_pattern='state_dir|XDG_STATE_HOME|XDG_RUNTIME_DIR|session_dir|runtime_dir'
state_dir_pattern+='|SESSIONS_DIR|SESSION_FILE|SESSION_LOG_FILE|SESSION_PHOTOS_DIR'
state_dir_pattern+='|STORE_LOCK_FILE|sessions/|session\.json|log\.ndjson'

# Raw write primitives. `write_json_atomic` is the home; everything here is a way around
# it.
#
# `File::options(` and `File::create_new(` are std's own aliases for two of the others —
# `File::options()` *is* `OpenOptions::new()` and `File::create_new()` is
# `File::create()` with `O_EXCL` — and they were missing until the P3 review. Two
# byte-identical bypasses that differed only in how the open call was spelled got
# opposite verdicts from this gate, which is the "green while checking less than it
# claims" family (note N10) rather than the naming limit the header records.
#
# The `OFlags::` alternatives are the same family again, added at P4e-i because that
# sub-milestone taught this workspace a *new* way to obtain a writable file:
# `daemon::server::open_destination` opens through `rustix` and turns the descriptor into a
# `std::fs::File`, which none of the spellings above can see (note **N51**'s discharge).
# The match is on the write-shaped **flags** rather than on `rustix::fs::open(`, because
# the call itself is not a write — `daemon::uds::SocketDir` opens the socket directory
# `O_PATH | O_DIRECTORY` with the same function, and reading a directory is not a bypass of
# anything. The limit this shares with the rest: flags assembled behind a `const` are a
# variable, and this is a grep.
raw_write_pattern='fs::write\(|File::create\(|File::create_new\(|File::options\(|OpenOptions::new\(|serde_json::to_writer|to_writer_pretty'
raw_write_pattern+='|OFlags::(WRONLY|RDWR|CREATE|TRUNC|APPEND)'

# The home itself, and the module that resolves the paths. Both are derived from the
# engine package's location so that moving the crate moves the exemption.
engine_dir="$(gate_metadata |
    jq -r '.packages[] | select(.name == "webcam-handler-engine") | .manifest_path' |
    head -n1 | xargs -r dirname)"

if [[ -z "$engine_dir" ]]; then
    gate_fail "webcam-handler-engine is not a workspace member; the state-dir home has no address"
    gate_finish
fi

engine_suffix="${engine_dir#"$root"/}"
home_files=("$engine_suffix/src/store.rs" "$engine_suffix/src/paths.rs")

# ------------------------------------------------------------------ half one: no bypass

scanned=0
reaching=0
while IFS= read -r -d '' file; do
    scanned=$((scanned + 1))
    rel="${file#"$root"/}"
    grep -Eq "$state_dir_pattern" "$file" || continue
    reaching=$((reaching + 1))

    exempt=0
    for home in "${home_files[@]}"; do
        if [[ "$rel" == "$home" || "$rel" == "$home"/* ]]; then
            exempt=1
        fi
    done
    # The store's own submodules are the home too, if it ever becomes a directory.
    if [[ "$rel" == "$engine_suffix/src/store/"* ]]; then
        exempt=1
    fi
    if ((exempt == 1)); then
        continue
    fi

    if grep -Eq "$raw_write_pattern" "$file"; then
        gate_fail "$rel names the state directory and writes with a raw primitive; state writes go through ${engine_suffix}/src/store.rs::write_json_atomic"
    fi
done < <(gate_rust_files)

gate_checked "$scanned" "Rust source files scanned"
gate_require_nonzero "$scanned" "Rust source files"

gate_checked "$reaching" "Rust files that name the state directory or its files"
# The P0 predicate reported a named skip here, because the session store had not landed
# and the population was legitimately empty. It has landed (docs/7 P3a), so an empty
# population is now a signal that the derivation broke — a renamed constant, a moved
# module — and not a phase that has not arrived.
gate_require_nonzero "$reaching" "Rust files that name the state directory or its files"

# ------------------------------------------------------------------ half two: the home

# The home may be one file or a directory; both are addresses, and the gate is indifferent
# to which, so a split does not break it.
store_file="$root/$engine_suffix/src/store.rs"
store_dir="$root/$engine_suffix/src/store"
home_sources=()
if [[ -f "$store_file" ]]; then
    home_sources+=("$store_file")
fi
if [[ -d "$store_dir" ]]; then
    while IFS= read -r -d '' file; do
        home_sources+=("$file")
    done < <(gate_find "$store_dir" -name '*.rs')
fi

if ((${#home_sources[@]} == 0)); then
    gate_fail "there is no ${engine_suffix}/src/store.rs (or src/store/); design §2.10 names it as the one home for atomic state writes, and half one is vacuous without it"
    gate_finish
fi

# The claims below `grep` the home's files directly rather than a variable holding their
# text. That is not a style choice: `printf '%s' "$text" | grep -q PAT` under `pipefail`
# returns 141 whenever `grep` matches early enough to SIGPIPE the `printf`, so the check
# fails *because* it succeeded — nondeterministically, by input size. It is the `grep`
# -under-`pipefail` trap docs/9 names, in a shape the script had to meet once to believe.
#
# What the home must contain to *be* the home. Each claim is one line of D9, and the
# absence of any one of them is a home that has quietly stopped doing its job.
#
#   claim<TAB>pattern<TAB>what its absence means
claims=$(
    cat <<'CLAIMS'
write_json_atomic is defined	pub fn write_json_atomic	design §2.10's named home does not exist
the temp file is made in the destination directory	tempfile_in	a temp file elsewhere makes the rename a cross-device copy, which is not atomic
the temp file is synced before the rename	sync_all	the rename can be reordered ahead of the contents
the rename is what publishes the document	\.persist\(	nothing atomically replaces the destination
the parent directory is fsynced after the rename	fsync_dir	the rename itself does not survive a power cut (note N12)
the one advisory lock is taken with fd-lock	fd_lock::RwLock	D9's cross-process safety has no mechanism
CLAIMS
)

home_claims=0
while IFS=$'\t' read -r claim pattern consequence; do
    [[ -n "$claim" ]] || continue
    home_claims=$((home_claims + 1))
    if ! grep -Eq "$pattern" "${home_sources[@]}"; then
        gate_fail "the store home does not show that $claim (no /$pattern/): $consequence"
    fi
done <<<"$claims"

gate_checked "${#home_sources[@]}" "source files making up the state-write home"
gate_checked "$home_claims" "D9 properties asserted of the home itself"
gate_require_nonzero "$home_claims" "D9 properties of the home"

gate_finish
