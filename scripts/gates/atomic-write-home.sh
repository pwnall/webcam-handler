#!/usr/bin/env bash
#
# One home for state-directory writes (design §2.10, rubric A5):
# `webcam-handler-engine::store::write_json_atomic`, under the one fd-lock. A session
# file written with `fs::write` is a session file that can be found half-written after a
# crash, and D9's crash-recovery story assumes it cannot be.
#
# The population is derived in two steps rather than by naming files: first find every
# Rust source that reaches for the state directory at all (it mentions `state_dir`,
# `XDG_STATE_HOME`, `session_dir` or `runtime_dir`), then, in each such file that is not
# the store or the paths module, look for a raw write primitive. A module that never
# names the state directory cannot be writing into it, and a module that does is exactly
# where a bypass would live.
#
# Honest limit: a file that receives the state path as an argument and never names it is
# invisible here. docs/4 records the same limit, and the P3 widening row adds the arm
# that runs over the real home once the session store exists.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

# What "reaching for the state directory" looks like in this codebase (note N2: the two
# XDG paths are resolved by `webcam-handler-engine::paths`, not by a crate).
state_dir_pattern='state_dir|XDG_STATE_HOME|XDG_RUNTIME_DIR|session_dir|runtime_dir'

# Raw write primitives. `write_json_atomic` is the home; everything here is a way around
# it.
raw_write_pattern='fs::write\(|File::create\(|OpenOptions::new\(|serde_json::to_writer|to_writer_pretty'

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

if ((reaching == 0)); then
    # Not a pass dressed up as one: the population is printed and counted, and docs/4's
    # P3 widening row is the commitment that makes it non-empty.
    gate_skip 0 "Rust files that name the state directory — the session store lands at P3 (docs/4 Part 2, 'Store bypass gate widened'); until then this predicate can only prove that nothing bypasses a home that does not exist"
else
    gate_checked "$reaching" "Rust files that name the state directory"
fi

gate_finish
