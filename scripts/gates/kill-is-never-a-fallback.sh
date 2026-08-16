#!/usr/bin/env bash
#
# Killing a camera's holder is an explicit command, and there is exactly one place in this
# workspace that can do it (AGENTS.md "Hardware and privacy", design §5, rubric B8, note
# N48):
#
#   > Killing a process that holds the camera is an explicit command naming its target,
#   > never a fallback.
#
# The T5 trait says the same from the other side — "nothing in this surface kills a process
# to get a device free" — and that is an **absence**, which is the one shape a test cannot
# see. `crates/daemon/tests/mutating_verbs.rs` drives a handful of verbs against a busy
# device and asserts the holder survives, but a suite can only witness the verbs it drives:
# a `Busy` retry added to `wch_photo` that signalled the holder it had just diagnosed would
# leave `just ci` green, because `photo` is not one of them. The structural claim is what
# closes it, and the structural claim is a call-site count.
#
# ## The predicate
#
#   1. **The home exists.** `webcam-handler-v4l2::holders::terminate` is the one function
#      that sends the signal, and `sys::signal` is the one place the syscall is spelled. A
#      gate that only counted call sites would be green forever on a tree where the home had
#      been deleted and the syscall inlined somewhere else.
#   2. **One caller, and it is the verb whose whole contract is naming its target.** Outside
#      the backend crate, `holders::terminate(` appears exactly once, in the daemon's
#      `terminate_holder` handler. A second call site anywhere — another handler, the CLI,
#      the engine, **or the next function down in the same file** — is the fallback AGENTS
#      forbids, whatever it is called. The count is of *occurrences* and not of files, which
#      is a repair rather than a restatement: see the paragraph above the walk.
#   3. **And the qualified spelling is the only way in**, because half two can only count
#      what it can see. `use …::holders::terminate;` followed by a bare `terminate(pid)` is
#      the most natural way anybody would write a second call site, and it is the shape half
#      two is blindest to: the `use` line carries no `(` and the call carries no `holders::`,
#      so a scratch copy with exactly that appended to `crates/engine/src/photo.rs` reported
#      `PASS … checked 1 call sites` (measured 2026-08-16, note **N167**). So no `use`
#      statement outside the backend crate may name `terminate` at all. That is not a
#      restriction anybody pays for — there is one legitimate caller and it already writes
#      the qualified path — and it is what keeps the count honest.
#
# ## Honest limits
#
# This is a `grep`, so it sees the spelling and not the semantics. What is invisible after
# claim 3 is narrower than it was and worth naming exactly: a caller that aliased the
# *module* (`use …::holders as h;` and then `h::terminate(pid)`), one that reached the signal
# through a re-export under another name, or one that built the syscall itself — which is
# what half one is for, and why `unsafe-scope.sh` confining every `unsafe` block to
# `crates/backends/v4l2/src/sys/` is the other half of the same fence. A call whose `(` sits
# on the next line is invisible to half two for the same reason and stays that way: `just ci`
# runs `cargo fmt --check`, which does not produce that shape.
#
# Prose counts too, for `atomic-write-home.sh`'s reason: a comment that names the call is a
# call as far as half two is concerned, so a comment about it says "the signal" instead.
# Claim 3 is the exception and it has to be, because the argument for banning the import is
# written in comments that name the import — so it reads its files with line comments
# stripped, which is `gate_product_lines`' trade and `lint-posture.sh`'s.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

# Derived from the package's manifest, so moving the backend moves the exemption.
backend_dir="$(gate_metadata |
    jq -r '.packages[] | select(.name == "webcam-handler-v4l2") | .manifest_path' |
    head -n1 | xargs -r dirname)"

if [[ -z "$backend_dir" ]]; then
    gate_fail "webcam-handler-v4l2 is not a workspace member; the signal has no home to be confined to"
    gate_finish
fi

backend_suffix="${backend_dir#"$root"/}"

# ------------------------------------------------------------------- half one: the home

home="$root/$backend_suffix/src/holders.rs"
syscall="$root/$backend_suffix/src/sys/signal.rs"
home_claims=0

for required in "$home" "$syscall"; do
    home_claims=$((home_claims + 1))
    if [[ ! -f "$required" ]]; then
        gate_fail "${required#"$root"/} is missing; the one home for signalling a camera's holder has no address"
    fi
done

if [[ -f "$home" ]]; then
    home_claims=$((home_claims + 1))
    if ! grep -Eq 'pub fn terminate\(' "$home"; then
        gate_fail "${backend_suffix}/src/holders.rs no longer defines terminate(); half two would be counting call sites of nothing"
    fi
fi

gate_checked "$home_claims" "properties of the one home for signalling a holder"
gate_require_nonzero "$home_claims" "properties of the signalling home"

# --------------------------------------------------------------- half two: one call site

# `holders::terminate(` and not `terminate(`: the bare name is a word English uses, and a
# gate that matched it would be red on a doc comment about terminating a session.
call_pattern='holders::terminate\('

# **Occurrences, not files** — and the difference is the whole claim (the G6 review's H3, note
# **N161**). Until 2026-08-16 this loop asked `grep -q` per file and appended one element per
# *file* that matched, then reported that array's length under the label "call sites". So a
# second `holders::terminate(` in a file that already had one was invisible, and the file that
# already has one is `crates/daemon/src/server.rs` — which is also where `wch_photo` lives. The
# gate was blind in precisely the place the paragraph at the top of this file names as its
# reason for existing, and it printed "1 call sites" while saying so.
#
# `{ grep -Eo … || true; }` rather than a bare pipeline: `grep` exits 1 on zero matches, 163 of
# the 164 files here have none, and this script runs under `pipefail` — the trap
# `scripts/rung-oracles.sh:44` and `scripts/gates/counted-selections.sh:36-39` both name.
# The import that would make a call site uncountable. Matched over the file with its line
# comments stripped and its whitespace squeezed out, so a `use` group broken across lines
# (`use v4l2::holders::{\n    terminate,\n    Holder,\n};`) is one run of characters and reads
# the same as the one-line form. `[^;]*` cannot cross the statement's own terminator, and `\<`
# is what keeps `crate::house::terminate` from matching the `use` inside `house`.
import_pattern='\<use[^;]*terminate'

scanned=0
call_sites=0
callers=()
importers=()
while IFS= read -r -d '' file; do
    scanned=$((scanned + 1))
    rel="${file#"$root"/}"
    # Inside the backend the name is the definition and its own tests; the confinement this
    # gate asserts is about everybody else.
    if [[ "$rel" == "$backend_suffix"/* ]]; then
        continue
    fi
    here="$({ grep -Eo "$call_pattern" "$file" || true; } | wc -l)"
    if ((here > 0)); then
        call_sites=$((call_sites + here))
        # `file:count`, so the violation message names how many are where. A list of file names
        # alone would report the two-in-one-file case as a single entry and leave a reader
        # looking for a second file that is not there.
        callers+=("$rel:$here")
    fi
    if sed 's://.*::' "$file" | tr -d '[:space:]' | grep -Eq "$import_pattern"; then
        importers+=("$rel")
    fi
done < <(gate_rust_files)

gate_checked "$scanned" "Rust source files scanned for a call to the signal"
gate_require_nonzero "$scanned" "Rust source files"

if ((call_sites == 0)); then
    gate_fail "nothing outside ${backend_suffix} calls holders::terminate(); the explicit command design §5 requires has no implementation, and this gate would be green about an absence"
elif ((call_sites > 1)); then
    gate_fail "holders::terminate( is called from ${call_sites} places outside ${backend_suffix} (${callers[*]}); killing a camera's holder is one explicit command and never a fallback (AGENTS, design §5, rubric B8)"
fi

gate_checked "$call_sites" "call sites of the signal outside the backend crate"

if ((${#importers[@]} > 0)); then
    gate_fail "${importers[*]} import terminate rather than calling holders::terminate( through its module; the qualified spelling is what makes the one call site countable, and a bare name in scope is a second call site this gate cannot see (AGENTS, design §5, rubric B8)"
fi
gate_checked 1 "that no file outside ${backend_suffix} puts the bare name in scope"
gate_finish
