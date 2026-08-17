# Both-direction cases for `kill-is-never-a-fallback.sh`.
#
# The predicate is an absence claim with three claims, so each gets its own inverse: the home
# going away, a second caller appearing, and the bare name reaching a caller's scope. The
# second-caller arms are the ones the gate exists for — they are the shape a future `Busy` retry
# would take, seeded in the two crates most likely to be tempted by it, in the file that already
# has the one legitimate call, and in the two import spellings that would make a second call site
# invisible to a count of qualified paths (note **N167**).
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
    # The file list, not just the count: the three second-caller arms all print *"called from 2
    # places"*, and the only thing in the sentence that says which of them ran is the file it
    # names beside the one legitimate call site.
    gate_red_because 'outside crates/backends/v4l2 (crates/engine/src/photo.rs:1' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_a_second_caller_in_the_daemon() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/crates/daemon/src/uds.rs" <<'RS'

fn retry_by_making_room(pid: i32) -> schema::Result<()> {
    v4l2::holders::terminate(pid)
}
RS
    gate_red_because 'crates/daemon/src/uds.rs:1)' env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_a_second_caller_in_the_file_that_already_has_one() {
    # **The arm this suite was missing, and the one shape it most needed.** The two arms above
    # seed *new* files, so a predicate that counted files rather than occurrences passed both of
    # them — and passed on a tree with two calls in `server.rs`, which is where the one
    # legitimate call lives and where `wch_photo` lives beside it. A `Busy` retry that signalled
    # the holder it had just diagnosed would be written here, four functions down from the verb
    # whose whole contract is naming its target, and until 2026-08-16 this gate reported
    # "1 call sites" about it (the G6 review's H3, note **N161**).
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/crates/daemon/src/server.rs" <<'RS'

fn make_room_for_the_retry(pid: i32) -> schema::Result<()> {
    v4l2::holders::terminate(pid)
}
RS
    # `server.rs:2` and not merely *"2 places"*: a count of **files** would report this tree as
    # one call site, which is precisely the defect N161 found, and an arm claiming only the
    # total would have gone on passing over a predicate that had regressed to counting files.
    gate_red_because '(crates/daemon/src/server.rs:2)' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_a_second_caller_that_imported_the_bare_name() {
    # **The way anybody would actually write it**, and the one the occurrence count cannot see:
    # the `use` line carries no `(` and the call carries no `holders::`, so half two counts one
    # call site over a tree with two. Measured green against the shipped predicate on 2026-08-16
    # (note **N167**), which is why claim 3 exists.
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/crates/engine/src/photo.rs" <<'RS'

use v4l2::holders::terminate;

fn free_the_device_the_easy_way(pid: i32) -> schema::Result<()> {
    terminate(pid)
}
RS
    gate_red_because 'import terminate rather than calling holders::terminate(' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_a_second_caller_that_imported_it_inside_a_wrapped_group() {
    # The same import with a second item beside it, which rustfmt then breaks across four lines
    # — so the `use` and the name it pulls in are not on the same line and a line-oriented grep
    # sees neither. Claim 3 reads the file with its whitespace squeezed out for exactly this.
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/crates/engine/src/photo.rs" <<'RS'

use v4l2::holders::{
    Holder,
    terminate,
};

fn free_the_device_the_easy_way(pid: i32, _seen: &[Holder]) -> schema::Result<()> {
    terminate(pid)
}
RS
    gate_red_because 'import terminate rather than calling holders::terminate(' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

pass_case_a_comment_about_the_import_is_not_an_import() {
    # Claim 3's own direction of `pass_case_a_comment_that_does_not_name_the_call`, and it is
    # load-bearing rather than symmetrical: the argument for banning the import is written in
    # comments that spell the import — in this predicate's header, in this file, and in note
    # N167 — so a claim that could not tell the two apart would be red on its own reasoning.
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/crates/engine/src/photo.rs" <<'RS'

// Deliberately not written as `use v4l2::holders::terminate;` — the qualified path is what
// lets `kill-is-never-a-fallback.sh` count the one legitimate call site (design §5).
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_only_caller_went_away() {
    # A tree where nothing calls the signal is a tree where `terminate_holder` does not
    # signal, which is a different defect and must not read as compliance.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/v4l2::holders::terminate(pid)/Ok(())/' "$tree/crates/daemon/src/server.rs"
    gate_red_because 'the explicit command design §5 requires has no implementation' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_home_no_longer_defines_the_signal() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/pub fn terminate(/fn terminate_disabled(/' "$tree/crates/backends/v4l2/src/holders.rs"
    gate_red_because 'no longer defines terminate(); half two would be counting call sites of nothing' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_syscall_module_is_gone() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$tree/crates/backends/v4l2/src/sys/signal.rs"
    gate_red_because 'src/sys/signal.rs is missing; the one home for signalling a camera'"'"'s holder has no address' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_backend_left_the_workspace() {
    local md
    md="$(gate_metadata_snapshot)"
    jq 'del(.packages[] | select(.name == "webcam-handler-v4l2"))' "$md" >"$md.seeded"
    gate_red_because 'webcam-handler-v4l2 is not a workspace member; the signal has no home to be confined to' \
        env WCH_GATE_METADATA="$md.seeded" "$GATE"
}
