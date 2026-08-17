#!/usr/bin/env bash
#
# Every kernel name `webcam-handler-v4l2` asks bindgen for exists in this host's UAPI headers
# (the G6 review's finding 9 against B10, note **N236**).
#
# ## The defect this closes
#
# `v4l2-sys-mit`'s build script runs `bindgen` over **this host's** `<linux/videodev2.h>`, so the
# constants the crate can name are whatever the build host's headers happen to define. That has
# always been true and it never bit, because the product code named only long-standing struct
# types. Note **N228** changed it: `sys::decode`'s and `sys::ioctl`'s new test tables compare the
# schema's hand-copied numbers against the kernel's, and to do that they have to *name* the
# kernel's — including `V4L2_CTRL_TYPE_RECT`, `V4L2_CTRL_TYPE_HDR10_MASTERING_DISPLAY`,
# `V4L2_CTRL_TYPE_AV1_FRAME` and `V4L2_CTRL_FLAG_HAS_WHICH_MIN_MAX`, none of which this workspace
# had ever named before.
#
# On a host whose headers are older than the newest of those, `cargo test -p webcam-handler-v4l2`
# stops **compiling**. That is a red build for an environment reason with no named skip in front
# of it, which is the one thing AGENTS rule 3 forbids of a precondition — and worse, the message
# a builder gets is `cannot find value v4l2_ctrl_type_V4L2_CTRL_TYPE_RECT in module uapi`, which
# says nothing about headers, nothing about which package supplies them, and nothing about what
# the run therefore did not claim.
#
# ## What this checks, and why the population is derived
#
# The list is **every `uapi::`/`v4l_sys::` identifier under `crates/backends/v4l2/src/`**, not a
# list kept here: a constant added to a table next year is a precondition added to the build, and
# a hand-maintained list is how that stops being noticed. Each is turned back into the kernel's
# own spelling — bindgen prefixes an enum member with its enum's name, so
# `v4l2_ctrl_type_V4L2_CTRL_TYPE_RECT` is the header's `V4L2_CTRL_TYPE_RECT` — and looked for in
# the header.
#
# Two names are excluded, and the exclusions are narrow enough to state: anything containing
# `__bindgen`, which is a name bindgen *invents* for an anonymous union and no header has, and
# nothing else.
#
# ## What it is not
#
# Not a kernel-version check. The kernel version a constant first appeared in is upstream history
# this repository has no offline authority over, and "your headers are older than 6.5" would be a
# claim with nothing behind it. The name is the precondition, the name is what the compiler needs,
# and the name is what this reports — which is also the remedy a builder can act on (`apt install
# linux-libc-dev`, or a newer one).
#
# Not a check on what bindgen actually emitted, either: that would need the build to have run.
# This reads the header a fresh build would read, which is what a *precondition* check is for —
# it answers before the twenty minutes, not after.
#
# The header is `/usr/include/linux/videodev2.h` by default and `$WCH_UAPI_HEADER` overrides it —
# the documented seam the selftest points at a doctored copy. A host with no header at all gets a
# named, counted skip: that is a machine that cannot build this crate at all, which `cargo build`
# says first and louder.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"
src="$root/crates/backends/v4l2/src"
header="${WCH_UAPI_HEADER:-/usr/include/linux/videodev2.h}"

if [[ ! -d "$src" ]]; then
    gate_fail "there is no $src to walk; the crate this predicate is about is not in the tree under test"
    gate_finish
fi

# Every kernel name the crate asks bindgen for, deduplicated, in the header's own spelling.
kernel_names() {
    grep -rhoE '(uapi|v4l_sys)::[A-Za-z0-9_]+' "$src" |
        sed 's/.*:://' |
        grep -v '__bindgen' |
        sed 's/^.*\(V4L2_\)/\1/' |
        sort -u
}

mapfile -t names < <(kernel_names)

if ((${#names[@]} == 0)); then
    gate_fail "found no uapi:: identifiers under crates/backends/v4l2/src — either the crate stopped naming the kernel's constants or this walk stopped seeing them, and both are worth a look"
    gate_finish
fi

if [[ ! -f "$header" ]]; then
    gate_skip "${#names[@]}" "kernel names, because this host has no $header to check them against — a machine that cannot build \`webcam-handler-v4l2\` at all, which \`cargo build\` reports first (README, Required to build)"
    gate_finish
fi

checked=0
for name in "${names[@]}"; do
    checked=$((checked + 1))
    if ! grep -qE "(^|[^A-Za-z0-9_])$name([^A-Za-z0-9_]|$)" "$header"; then
        gate_fail "$header does not define $name, which crates/backends/v4l2/src asks bindgen for; this host's headers are older than what this crate's test target needs, and \`cargo test -p webcam-handler-v4l2\` will fail to compile rather than decline (note N236 — install a newer linux-libc-dev)"
    fi
done

gate_checked "$checked" "kernel names crates/backends/v4l2/src asks bindgen for, against $header"
gate_require_nonzero "$checked" "kernel names"

gate_finish
