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
#      the engine — is the fallback AGENTS forbids, whatever it is called.
#
# ## Honest limits
#
# This is a `grep`, so it sees the spelling and not the semantics: a caller that reached the
# signal through a re-export under another name, or built the syscall itself, is invisible
# to half two — which is what half one is for, and why `unsafe-scope.sh` confining every
# `unsafe` block to `crates/backends/v4l2/src/sys/` is the other half of the same fence.
# Prose counts too, for `atomic-write-home.sh`'s reason: a comment that names the call is a
# call as far as this is concerned, so a comment about it says "the signal" instead.
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

scanned=0
callers=()
while IFS= read -r -d '' file; do
    scanned=$((scanned + 1))
    rel="${file#"$root"/}"
    # Inside the backend the name is the definition and its own tests; the confinement this
    # gate asserts is about everybody else.
    if [[ "$rel" == "$backend_suffix"/* ]]; then
        continue
    fi
    if grep -Eq "$call_pattern" "$file"; then
        callers+=("$rel")
    fi
done < <(gate_rust_files)

gate_checked "$scanned" "Rust source files scanned for a call to the signal"
gate_require_nonzero "$scanned" "Rust source files"

if ((${#callers[@]} == 0)); then
    gate_fail "nothing outside ${backend_suffix} calls holders::terminate(); the explicit command design §5 requires has no implementation, and this gate would be green about an absence"
elif ((${#callers[@]} > 1)); then
    gate_fail "holders::terminate( is called from ${#callers[@]} places outside ${backend_suffix} (${callers[*]}); killing a camera's holder is one explicit command and never a fallback (AGENTS, design §5, rubric B8)"
fi

gate_checked "${#callers[@]}" "call sites of the signal outside the backend crate"
gate_finish
