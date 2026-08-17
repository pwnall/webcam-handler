# Both-direction cases for `uapi-constants-are-declared.sh`.
#
# Two inputs and therefore two families of arm: the **header**, which is the host and is reached
# through the predicate's documented `$WCH_UAPI_HEADER` seam, and the **code**, which is a
# `gate_scratch_tree` copy. The header arms are the ones that matter — the defect note **N236**
# records is a build host older than the crate, and the only honest way to drive it is to hand
# the predicate a header with the constant taken out.
#
# The doctored headers are made from the real one by deleting a line, so every arm below runs
# against a file that is a real `<linux/videodev2.h>` minus something, rather than a fiction.
# That is the same rule `oracle::ScriptedTools` follows for a missing binary.
#
# shellcheck shell=bash

pass_case() {
    "$GATE"
}

pass_case_a_header_that_defines_everything_the_crate_names() {
    # The real header copied rather than read in place, so the seam itself is exercised by a
    # green arm: a predicate whose override path is only ever driven by failing arms would pass
    # here for the wrong reason if the override silently did nothing.
    local scratch copy
    scratch="$(gate_scratch_root)"
    copy="$(mktemp "$scratch/wch-uapi-header.XXXXXXXX")"
    cp /usr/include/linux/videodev2.h "$copy"
    WCH_UAPI_HEADER="$copy" "$GATE"
}

fail_case_this_hosts_headers_predate_a_constant_the_crate_names() {
    # **The defect exactly as the G6 review found it.** `V4L2_CTRL_FLAG_HAS_WHICH_MIN_MAX` is one
    # of the four names note N228's tables introduced, and a build host whose `linux-libc-dev`
    # predates it cannot compile this crate's test target at all — a red build for an environment
    # reason, with no named skip in front of it and a compiler message that says nothing about
    # headers.
    local scratch copy
    scratch="$(gate_scratch_root)"
    copy="$(mktemp "$scratch/wch-uapi-header.XXXXXXXX")"
    grep -v 'V4L2_CTRL_FLAG_HAS_WHICH_MIN_MAX' /usr/include/linux/videodev2.h >"$copy"
    gate_red_because 'does not define V4L2_CTRL_FLAG_HAS_WHICH_MIN_MAX' \
        env "WCH_UAPI_HEADER=$copy" "$GATE"
}

fail_case_a_compound_control_type_the_crate_decodes_is_missing() {
    # The other half of the same vintage, and a different kind of name: `V4L2_CTRL_TYPE_RECT` is
    # an *enum member*, which bindgen emits under its enum's name, so this arm also proves the
    # walk turns `v4l2_ctrl_type_V4L2_CTRL_TYPE_RECT` back into what a header actually spells.
    local scratch copy
    scratch="$(gate_scratch_root)"
    copy="$(mktemp "$scratch/wch-uapi-header.XXXXXXXX")"
    grep -v 'V4L2_CTRL_TYPE_RECT' /usr/include/linux/videodev2.h >"$copy"
    gate_red_because 'does not define V4L2_CTRL_TYPE_RECT' \
        env "WCH_UAPI_HEADER=$copy" "$GATE"
}

fail_case_the_crate_asks_for_a_name_no_kernel_has() {
    # The population is derived from the code, so it has to grow with the code: a constant added
    # to a table next year is a build precondition added at the same time, and this arm is what
    # says the predicate would notice. Seeded in a scratch copy of the tree rather than in the
    # header, which is the direction a developer creates this in.
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/crates/backends/v4l2/src/sys/decode.rs" <<'RUST'

// Seeded by the gate selftest; not a real constant.
const _SEEDED: u32 = uapi::V4L2_CTRL_TYPE_NOTHING_LIKE_THIS;
RUST
    gate_red_because 'does not define V4L2_CTRL_TYPE_NOTHING_LIKE_THIS' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_the_crate_stopped_naming_the_kernel_at_all() {
    # AGENTS rule 3's vacuity case. A walk that found nothing would print a clean census over an
    # empty population, and "zero constants checked" is the shape a broken `grep` has as much as
    # a crate that genuinely stopped asking.
    local tree
    tree="$(gate_scratch_tree)"
    rm -rf "$tree/crates/backends/v4l2/src/sys"
    mkdir -p "$tree/crates/backends/v4l2/src/sys"
    gate_red_because 'found no uapi:: identifiers' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_the_crate_is_not_in_the_tree_under_test() {
    # The predicate names one crate by path, so a tree without it must say so rather than examine
    # nothing and pass — the same reason `gate_require_nonzero` exists one line down.
    local tree
    tree="$(gate_scratch_tree)"
    rm -rf "$tree/crates/backends/v4l2/src"
    gate_red_because 'is not in the tree under test' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}
