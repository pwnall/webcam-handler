# Both-direction cases for `unsafe-scope.sh`.
#
# Three claims, each with its inverse: the token stays inside the sys module, every other
# crate root forbids unsafe code, and the one crate that must *not* forbid it does not.
# The second green arm is the one that matters most — a scope gate that fails on the
# module it is supposed to permit would simply be turned off.
#
# shellcheck shell=bash

pass_case() {
    "$GATE"
}

# The exemption is real: `unsafe` inside crates/backends/v4l2/src/sys/ is allowed, and
# will be the normal state from P1 onwards.
pass_case_unsafe_inside_the_sys_module() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/crates/backends/v4l2/src/sys"
    cat >"$tree/crates/backends/v4l2/src/sys/ioctl.rs" <<'RS'
//! Seeded by the gate selftest: this is the one directory allowed to say the token.

/// # Safety
/// The caller guarantees `fd` is an open file descriptor.
pub unsafe fn probe(fd: i32) -> i32 {
    // SAFETY: seeded fixture; the obligation is the caller's, stated above.
    unsafe { fd }
}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_unsafe_outside_the_sys_module() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/crates/engine/src/lib.rs" <<'RS'

/// Seeded by the gate selftest.
pub fn seeded() {
    // SAFETY: nothing; this block exists to be caught.
    unsafe { core::hint::unreachable_unchecked() }
}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_crate_root_without_forbid() {
    local tree
    tree="$(gate_scratch_tree)"
    grep -v '^#!\[forbid(unsafe_code)\]' "$tree/crates/schema/src/lib.rs" \
        >"$tree/crates/schema/src/lib.rs.seeded"
    mv "$tree/crates/schema/src/lib.rs.seeded" "$tree/crates/schema/src/lib.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The exception is asserted in both directions: a `forbid` appearing on the V4L2 crate
# would mean the sys module had moved somewhere this gate is not looking.
fail_case_forbid_on_the_one_crate_that_must_not_have_it() {
    local tree root_file
    tree="$(gate_scratch_tree)"
    root_file="$tree/crates/backends/v4l2/src/lib.rs"
    {
        printf '#![forbid(unsafe_code)]\n'
        cat "$root_file"
    } >"$root_file.seeded"
    mv "$root_file.seeded" "$root_file"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# A crate root the manifest declares but the tree does not have. Seeded through the
# metadata rather than by deleting the file, because deleting it makes `cargo metadata`
# itself fail — and an arm that goes red before the predicate runs proves nothing about
# the predicate.
fail_case_crate_root_missing_from_the_tree() {
    local md
    md="$(gate_metadata_snapshot)"
    jq '
        ( .packages[]
          | select(.name == "webcam-handler-web")
          | .targets[]
          | select(any(.kind[]; . == "lib"))
          | .src_path ) = "/nonexistent/web/src/lib.rs"
    ' "$md" >"$md.seeded"
    WCH_GATE_METADATA="$md.seeded" "$GATE"
}
