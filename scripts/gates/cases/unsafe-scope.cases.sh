# Both-direction cases for `unsafe-scope.sh`.
#
# Four claims, each with its inverse: the token stays inside the sys module, every other
# crate root forbids unsafe code, the one crate that must *not* forbid it does not, and
# the residual-`unsafe` register in that module's `mod.rs` describes the tree it is in.
# The second green arm is the one that matters most — a scope gate that fails on the
# module it is supposed to permit would simply be turned off.
#
# shellcheck shell=bash

# Add one row to the residual-`unsafe` register and move its summary count with it, which
# is what a change that lands a new block actually has to do.
#
# The totals are recounted from the table rather than transcribed, so this survives the
# register growing. It does repeat the predicate's arithmetic, and that is acceptable here
# because it seeds the *green* shape: what proves the reconciliation bites is the pair of
# failing arms below, and each of those is driven against the real predicate with nothing
# standing in for it (rubric rule 6).
register_row() {
    local register="$1" row="$2" staged rows impls
    staged="$register.seeded"
    awk -v row="//! | $row |" '
        /^\/\/! \|/ { last = NR }
        { line[NR] = $0 }
        END { for (i = 1; i <= NR; i++) { print line[i]; if (i == last) print row } }
    ' "$register" >"$staged"
    rows="$(grep -c '^//! |' "$staged" || true)"
    impls="$(grep -c '^//! |.*unsafe impl' "$staged" || true)"
    # Two of those lines are the table's header and the rule under it.
    sed -E "s@^//! ([A-Za-z0-9]+) blocks and ([A-Za-z0-9]+) \`unsafe impl\`@//! $((rows - 2 - impls)) blocks and $impls \`unsafe impl\`@" \
        "$staged" >"$register"
    rm -f "$staged"
}

# One `unsafe` block, in a file the seeding arms drop into the exempt region.
seeded_block() {
    cat <<'RS'
//! Seeded by the gate selftest: this is the one directory allowed to say the token.

/// Hand back what the caller passed, through a block with nothing to prove.
pub(crate) fn probe(fd: i32) -> i32 {
    // SAFETY: seeded fixture. Nothing is dereferenced and the value is returned as it
    // arrived, so the block carries no obligation at all.
    unsafe { fd }
}
RS
}

pass_case() {
    "$GATE"
}

# The exemption is real: `unsafe` inside crates/backends/v4l2/src/sys/ is allowed. Since
# P4d it is allowed *with its register row*, because that is the shape a legitimate new
# block lands in — so this arm seeds both halves and must stay green.
pass_case_a_registered_unsafe_block_inside_the_sys_module() {
    local tree sys
    tree="$(gate_scratch_tree)"
    sys="$tree/crates/backends/v4l2/src/sys"
    mkdir -p "$sys"
    seeded_block >"$sys/seeded.rs"
    # SC2016 off: the register's Where column quotes Rust paths in backticks, and this is
    # the literal row text — there is nothing in it to expand.
    # shellcheck disable=SC2016
    register_row "$sys/mod.rs" '`seeded::probe` | seeded by the gate selftest'
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

# ------------------------------------------------- the register, in both directions

# A block that landed without its row. This is the drift the claim exists to catch: it is
# the exact shape a new kernel-facing edge written with `libc` rather than `rustix` would
# have had at P4d, and the sentence "the count below is unchanged by it" would have been
# quietly false from that commit on.
fail_case_an_unsafe_block_the_register_does_not_name() {
    local tree
    tree="$(gate_scratch_tree)"
    seeded_block >"$tree/crates/backends/v4l2/src/sys/seeded.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The other direction, which matters as much: a row that outlives the block it names
# leaves a reader believing an obligation is still being discharged somewhere.
fail_case_a_register_row_naming_a_block_that_is_not_there() {
    local tree
    tree="$(gate_scratch_tree)"
    # SC2016 off for the same reason as the green arm above: a literal register row.
    # shellcheck disable=SC2016
    register_row "$tree/crates/backends/v4l2/src/sys/mod.rs" \
        '`seeded::probe` | seeded by the gate selftest, and there is no such block'
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The table and the sentence above it are one claim written twice. A summary that has
# drifted from its own table is the number a reader trusts and nothing reconciles.
fail_case_the_register_summary_disagrees_with_its_own_table() {
    local tree register
    tree="$(gate_scratch_tree)"
    register="$tree/crates/backends/v4l2/src/sys/mod.rs"
    sed -E "s@^//! ([A-Za-z0-9]+) blocks and ([A-Za-z0-9]+) \`unsafe impl\`@//! 4 blocks and \2 \`unsafe impl\`@" \
        "$register" >"$register.seeded"
    mv "$register.seeded" "$register"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# A count spelled in a way the reconciliation cannot read is a failure and not a pass:
# "several blocks" is how a number stops being checkable at all, which is the state this
# claim exists to prevent rather than a lesser version of it.
fail_case_a_register_summary_no_reader_can_count() {
    local tree register
    tree="$(gate_scratch_tree)"
    register="$tree/crates/backends/v4l2/src/sys/mod.rs"
    sed -E "s@^//! ([A-Za-z0-9]+) blocks and @//! several blocks and @" \
        "$register" >"$register.seeded"
    mv "$register.seeded" "$register"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The heading is what names the register, so losing it is losing the anchor rather than
# losing prose: the rows would still reconcile, and nothing would say what they are.
fail_case_the_register_section_lost_its_heading() {
    local tree register
    tree="$(gate_scratch_tree)"
    register="$tree/crates/backends/v4l2/src/sys/mod.rs"
    grep -v 'The residual' "$register" >"$register.seeded"
    mv "$register.seeded" "$register"
    WCH_GATE_ROOT="$tree" "$GATE"
}
