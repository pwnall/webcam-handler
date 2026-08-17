# Both-direction cases for `privileged-helper.sh`.
#
# The mode case is the one that matters: it is the *entire* security boundary around a
# binary that grants root, so a gate that could not see a widened mode would be checking
# nothing that counts.
#
# ## The seam three of these arms drive, and why they need one
#
# Claim 6 is about what a file on a developer's disk actually *carries*, and a case file cannot
# seed that violation the way the others seed theirs: setting a capability needs `CAP_SETFCAP`,
# which is root, so an arm that wanted a real over-blessed file would have to be run by the very
# privilege this gate exists to contain. So the predicate offers one documented seam —
#
#   $WCH_GATE_GETCAP   a `getcap`-shaped program to read the blessed directory with
#
# — and `pass_case` leaves it unset, which reads the real xattrs with the real tool: the arm
# rubric rule 6 requires [S:N10]. `_stub_getcap` writes a program that prints a *recorded*
# listing in getcap's own format, which is the same shape `oracle-rung-accounting.sh`'s runner
# seam takes and for the same reason.
#
# The three recorded listings are not invented. Two of them were read off this machine on
# 2026-08-15, while P6e's narrowing was landing (note **N125**, note **N126**): the blessed
# helper still carried the pre-narrowing `cap_net_admin,cap_sys_module` after `caps.rs` had
# stopped asking for it, and a `wch-priv` orphaned by the N90 rename was sitting beside it,
# mode 0700 and root-capable, carrying the `exec` verb the narrowing had just deleted. Both
# arms below are transcripts of a real defect rather than a shape somebody imagined.
#
# shellcheck shell=bash

# Write a `getcap`-shaped program that prints the recorded listing in "$@" instead of reading
# any xattr, and echo its path.
#
# Each argument is one line as `getcap -r` renders it, minus the directory prefix — the stub
# reads that off the directory it is asked about, so a listing stays correct in a scratch tree
# whose path nobody wrote down.
_stub_getcap() {
    local tree="$1" stub line
    shift
    stub="$tree/.wch-getcap-stub"
    {
        printf '#!/usr/bin/env bash\n'
        # SC2016 twice: `$2` and `$dir` are variables in the script being *written*, not in
        # this one, and the single quotes are what stops them expanding here. The stub reads
        # the directory out of its own arguments for the reason `_stub_runner` gives about
        # being *told* rather than working it out — a stub handed the answer is evidence
        # about the handing.
        # shellcheck disable=SC2016
        printf 'dir="${2:-}"\n'
        for line in "$@"; do
            # shellcheck disable=SC2016
            printf 'printf "%%s\\n" "$dir/%s"\n' "$line"
        done
    } >"$stub"
    chmod +x "$stub"
    printf '%s\n' "$stub"
}

# A scratch tree with a plausible blessed copy in it: mode 0700, owner-only, present.
_tree_with_a_blessed_copy() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/.wch-bin"
    printf '#!/bin/false\n' >"$tree/.wch-bin/webcam-handler-priv"
    chmod 0700 "$tree/.wch-bin/webcam-handler-priv"
    printf '%s\n' "$tree"
}

pass_case() {
    "$GATE"
}

# A second green arm: a tree that HAS a blessed copy, correctly owner-only, must pass.
# Without this the mode check would be vacuous on every machine that has not blessed —
# which is all of CI, and would have been every run of this selftest.
pass_case_a_correctly_blessed_copy_is_allowed() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/.wch-bin"
    printf '#!/bin/false\n' >"$tree/.wch-bin/webcam-handler-priv"
    chmod 0700 "$tree/.wch-bin/webcam-handler-priv"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_blessed_copy_is_group_or_world_executable() {
    local tree
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/.wch-bin"
    printf '#!/bin/false\n' >"$tree/.wch-bin/webcam-handler-priv"
    # 0755: exactly what a careless `chmod -R a+rX` or a restore-from-backup produces, and
    # on a real blessed copy it is a local root escalation for every user on the box.
    chmod 0755 "$tree/.wch-bin/webcam-handler-priv"
    gate_red_because 'is mode 755; a root-capable binary must be 0700 (owner only)' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_blessed_directory_is_not_gitignored() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed '/^\/\.wch-bin\/$/d' "$tree/.gitignore"
    gate_red_because '.wch-bin/ is not in .gitignore; a capability-carrying binary must never be committable' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_product_crate_links_the_privileged_helper() {
    local md
    md="$(gate_metadata_snapshot)"
    # The defect this closes: a helper that reaches a shipped binary's link graph puts
    # root-capable code in something a user might install. Seeded in the resolve graph
    # rather than in a manifest, because that is where the gate reads it.
    jq '
        ( [ .packages[] | select(.name == "webcam-handler-priv") | .id ] | first ) as $priv
        | ( .resolve.nodes[] | select(.id | test("webcam-handler-cli[^-]")) | .deps )
            += [ { "pkg": $priv, "name": "webcam_handler_priv", "dep_kinds": [ { "kind": null, "target": null } ] } ]
    ' "$md" >"$md.seeded"
    gate_red_because 'depends on webcam-handler-priv; the privileged helper must reach no shipped link graph' \
        env WCH_GATE_METADATA="$md.seeded" "$GATE"
}

fail_case_the_helper_stops_forbidding_unsafe() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/^#!\[forbid(unsafe_code)\]//' "$tree/crates/priv/src/main.rs"
    gate_red_because 'does not carry #![forbid(unsafe_code)]; a root-capable binary is the last place to hand-roll a pointer cast' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_helper_left_the_workspace() {
    local md
    md="$(gate_metadata_snapshot)"
    # Non-vacuity: with no subject, every claim above is trivially true, and a gate that
    # reports PASS over a helper that is no longer there is the worst kind of green.
    jq 'del(.packages[] | select(.name == "webcam-handler-priv"))' "$md" >"$md.seeded"
    gate_red_because 'webcam-handler-priv is not a workspace member; this gate has no subject' \
        env WCH_GATE_METADATA="$md.seeded" "$GATE"
}

# ------------------------------------------------------------------ claim 5

fail_case_a_verb_takes_a_program_its_caller_names() {
    local tree fragment
    tree="$(gate_scratch_tree)"
    fragment="$tree/.wch-exec-fragment"
    # **Written as the diff that would actually land**, which is the whole `exec` verb note N8
    # accepted and note N125 deleted, pasted back into the enum it used to live in. An arm that
    # seeded the bare word `exec` would be evidence about a grep rather than about the verb.
    cat >"$fragment" <<'RUST'
    /// Run a program with the capabilities, via the ambient set.
    Exec {
        /// The program, and everything to pass it.
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        argv: Vec<String>,
    },
RUST
    gate_seed "/^enum Verb {\$/r $fragment" "$tree/crates/priv/src/main.rs"
    # shellcheck disable=SC2016  # the predicate's own sentence, backticks and $defs and all, matched verbatim
    gate_red_because 'names `trailing_var_arg` in product code; that is how a verb hands this binary'"'"'s capabilities to a program its caller chose' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_the_helper_reaches_for_the_std_exec_trait() {
    local tree
    tree="$(gate_scratch_tree)"
    # The other route to the same place, and the one clap cannot see: a verb that never declares
    # an argv, reads `std::env::args()` itself and replaces this process with whatever it found.
    # `main.rs`'s own test walks the command tree and would be perfectly green on it.
    gate_seed 's/^mod caps;$/mod caps;\nuse std::os::unix::process::CommandExt as _;/' \
        "$tree/crates/priv/src/main.rs"
    # shellcheck disable=SC2016  # the predicate's own sentence, backticks and $defs and all, matched verbatim
    gate_red_because 'names `CommandExt` in product code; that is how a verb hands this binary'"'"'s capabilities to a program its caller chose' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_the_helper_reaches_for_the_exec_trait_in_a_checkout_somebody_cloned_under_tests() {
    # The same violation, from a checkout whose **path** has a `tests` component in it — which
    # is a fact about where somebody cloned this repository and must decide nothing.
    #
    # `lib.sh`'s `gate_test_region_start` reads a file as test code from line 1 when its path
    # has a `tests/` directory in it (cargo's rule for `crates/<pkg>/tests/*.rs`). Matched
    # against the *absolute* path, that rule answers "all test code" for every file in a
    # checkout under `~/tests/`, `gate_product_lines` hands claim 5 zero bytes, and this
    # predicate goes **vacuously green**: `examined` still counts one file per source, so the
    # non-vacuity floor is satisfied while the walk reads nothing (note **N185**).
    #
    # This arm is here rather than in six case files because the classifier is shared and one
    # arm over it is what stops it drifting; the other five callers are named in `lib.sh`'s
    # header. It is `privileged-helper.sh`'s because this is the claim whose failure is silent —
    # four of the six fail loudly on an empty product half, and the two that do not are this one
    # and `avi-reparse-is-independent.sh`'s import claim.
    local scratch tree under
    tree="$(gate_scratch_tree)"
    scratch="$(dirname "$tree")"
    under="$scratch/tests/checkout"
    mkdir -p "$scratch/tests"
    mv "$tree" "$under"
    gate_seed 's/^mod caps;$/mod caps;\nuse std::os::unix::process::CommandExt as _;/' \
        "$under/crates/priv/src/main.rs"
    # shellcheck disable=SC2016  # the predicate's own sentence, backticks and $defs and all, matched verbatim
    gate_red_because 'names `CommandExt` in product code; that is how a verb hands this binary'"'"'s capabilities to a program its caller chose' \
        env "WCH_GATE_ROOT=$under" "$GATE"
}

fail_case_the_helper_has_no_source_left() {
    local tree
    tree="$(gate_scratch_tree)"
    # Non-vacuity for claim 5, charged the way `avi-reparse-is-independent.sh` charges it: every
    # spelling the walk refuses is absent from a directory with nothing in it, so a walk that
    # found no files would report the strongest possible green over the emptiest possible tree.
    rm -f "$tree"/crates/priv/src/*.rs
    gate_red_because 'examined zero source files' env "WCH_GATE_ROOT=$tree" "$GATE"
}

# ------------------------------------------------------------------ claim 6

pass_case_a_blessed_copy_carrying_exactly_the_declared_grant_is_allowed() {
    local tree stub
    tree="$(_tree_with_a_blessed_copy)"
    stub="$(_stub_getcap "$tree" "webcam-handler-priv cap_sys_module=ep")"
    # The green arm claim 6 needs, and it does more than balance the two red ones: the listing
    # is written out here by hand while the expectation comes out of `caps.rs`, so widening the
    # source without widening this line turns *this* arm red. A grant cannot grow quietly on
    # either side of the comparison.
    env "WCH_GATE_ROOT=$tree" "WCH_GATE_GETCAP=$stub" "$GATE"
}

fail_case_the_blessed_copy_carries_more_than_the_tree_declares() {
    local tree stub
    tree="$(_tree_with_a_blessed_copy)"
    # Read off this machine on 2026-08-15: the copy blessed before P6e, against a tree that had
    # stopped asking for `cap_net_admin`. `just bless` used to call this "already blessed",
    # because its check asked whether each wanted capability was present and every superset
    # satisfies that. A narrowing nothing enforces is a paragraph.
    stub="$(_stub_getcap "$tree" "webcam-handler-priv cap_net_admin,cap_sys_module=ep")"
    gate_red_because 'a grant wider than the code asks for' \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_GETCAP=$stub" "$GATE"
}

fail_case_a_second_capability_carrying_file_sits_beside_the_helper() {
    local tree stub
    tree="$(_tree_with_a_blessed_copy)"
    # Also read off this machine, and the worse of the two (note N126): the `wch-priv` the N90
    # rename orphaned. The correctly blessed helper is in the listing beside it, which is the
    # point — every name-shaped check in this suite was looking straight at the right file while
    # a root-capable binary carrying the deleted `exec` verb sat next to it for two days.
    stub="$(_stub_getcap "$tree" \
        "webcam-handler-priv cap_sys_module=ep" \
        "wch-priv cap_net_admin,cap_sys_module=ep")"
    gate_red_because 'is not the blessed helper' \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_GETCAP=$stub" "$GATE"
}

fail_case_the_blessing_is_declared_in_two_places() {
    local tree
    tree="$(gate_scratch_tree)"
    # One home for the capability list, asserted. The failure this closes is the ordinary one:
    # somebody narrows the constant the justfile reads and leaves a second copy behind for the
    # gate to read, and the two answers disagree about what "blessed" means with nothing saying
    # so. A gate that took the first match would have picked a side silently.
    printf '\npub(crate) const BLESSING_OLD: &str = "cap_sys_module,cap_net_admin+ep";\n' \
        >>"$tree/crates/priv/src/modules.rs"
    gate_red_because 'has one home or it has none' env "WCH_GATE_ROOT=$tree" "$GATE"
}
