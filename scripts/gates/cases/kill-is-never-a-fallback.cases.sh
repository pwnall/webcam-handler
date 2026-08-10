# Both-direction cases for `kill-is-never-a-fallback.sh`.
#
# The predicate is an absence claim with two halves, so each half gets its own inverse: the
# home going away, and a second caller appearing. The second-caller arms are the ones the
# gate exists for — they are the shape a future `Busy` retry would take, seeded in the two
# crates most likely to be tempted by it.
#
# shellcheck shell=bash

pass_case() {
    "$GATE"
}

pass_case_a_comment_that_does_not_name_the_call() {
    # The gate is a grep, so its own header warns that prose counts. This arm is the other
    # direction of that: prose that talks *about* signalling without spelling the call is
    # not a second call site, and a gate that could not tell them apart would push authors
    # into leaving the reasoning out.
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/crates/engine/src/photo.rs" <<'RS'

// Nothing here signals anything: a camera another process holds is `Error::Busy` with the
// holders named, and the explicit command that asks one of them to let go is the daemon's
// `wch_terminate_holder` (design §5).
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_second_caller_in_the_engine() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/crates/engine/src/photo.rs" <<'RS'

fn free_the_device_the_easy_way(pid: i32) -> schema::Result<()> {
    v4l2::holders::terminate(pid)
}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_second_caller_in_the_daemon() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/crates/daemon/src/uds.rs" <<'RS'

fn retry_by_making_room(pid: i32) -> schema::Result<()> {
    v4l2::holders::terminate(pid)
}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_only_caller_went_away() {
    # A tree where nothing calls the signal is a tree where `terminate_holder` does not
    # signal, which is a different defect and must not read as compliance.
    local tree
    tree="$(gate_scratch_tree)"
    sed -i 's/v4l2::holders::terminate(pid)/Ok(())/' "$tree/crates/daemon/src/server.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_home_no_longer_defines_the_signal() {
    local tree
    tree="$(gate_scratch_tree)"
    sed -i 's/pub fn terminate(/fn terminate_disabled(/' "$tree/crates/backends/v4l2/src/holders.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_syscall_module_is_gone() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$tree/crates/backends/v4l2/src/sys/signal.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_backend_left_the_workspace() {
    local md
    md="$(gate_metadata_snapshot)"
    jq 'del(.packages[] | select(.name == "webcam-handler-v4l2"))' "$md" >"$md.seeded"
    WCH_GATE_METADATA="$md.seeded" "$GATE"
}
