# Both-direction cases for `lint-posture.sh`.
#
# The subject is an attribute copied into every crate root, so the failing arms seed the ways a
# copy stops being one: the block deleted, one lint of the set deleted, the block moved behind a
# `cfg` nobody enables, **the block moved onto the next item by a lost `!`**, **the block
# commented out rather than removed**, the wider lint taken without the set it widens, and the
# wider lint gone from every root that had it. The two in bold are the ones the review of the
# batch that landed this predicate measured as green (note **N167**); the arms are what the
# repairs cost.
#
# Three arms doctor `cargo metadata` instead, because the *population* is derived from it: a
# population that came back empty, one that names a file the tree does not have, and one whose
# members declare a crate type the walk was not expecting are each a predicate proving nothing
# rather than a tree that is clean.
#
# Seeds land in scratch copies. Nothing here is compiled — the claim is about what the attribute
# says, so a seeded root that would not build is still exactly the shape being asked about.
#
# shellcheck shell=bash

pass_case() {
    "$GATE"
}

pass_case_a_root_that_only_writes_about_the_set_does_not_carry_it() {
    # The arm the header's "both comment kinds are stripped" sentence owes, and it replaces one
    # that claimed to seed a state and did not (note **N167**): `crates/testkit` is already bare
    # in the shipped tree, so seeding it bare was `pass_case` written twice.
    #
    # This seeds a state the tree does not have — the block commented out rather than deleted,
    # once in each comment syntax, which is what a bisect or a "try it without this" leaves
    # behind. Both name **three** of the four deliberately: if either stripping regressed, the
    # walk would read three lints out of prose and claim 2 ("the set is taken whole or not at
    # all") would go red, so this arm can fail rather than merely not-fail. It is a dev-only root
    # for the same reason `fail_case_a_dev_only_root_took_half_the_set` is: claim 1 stays silent
    # there and the arm is evidence about the stripping alone.
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/crates/testkit/src/lib.rs" <<'RUST'

// Left behind by somebody trying the harness without the set:
// #![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

/*
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]
*/
RUST
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_shipped_root_lost_the_whole_set() {
    # The defect as G6 found it: `crates/engine` and `crates/schema` carried `forbid(unsafe_code)`
    # and nothing else, and the claim AGENTS calls clippy-enforced was enforced at neither.
    local tree
    tree="$(gate_scratch_tree)"
    python3 - "$tree/crates/engine/src/lib.rs" <<'PY'
import re, sys
path = sys.argv[1]
text = open(path).read()
text = re.sub(r"#!\[cfg_attr\(\n(?:.*\n)*?\)\]\n", "", text, count=1)
open(path, "w").write(text)
PY
    gate_red_because 'crates/engine/src/lib.rs is a shipped crate root that does not deny' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_a_shipped_root_kept_three_of_the_four() {
    # How drift actually arrives: somebody adds the lint that bit them, or removes the one that
    # was in the way, and the set stops being a set. Claim 1 names what is missing; this arm is
    # about the *partial* state, which claim 2 holds over every root including the dev-only ones.
    #
    # Seeded at `crates/api`, which carries the four and not the wider fifth, so the arm's own
    # branch is the one that fires alongside claim 1 rather than three of them at once.
    local tree
    tree="$(gate_scratch_tree)"
    sed -i '/clippy::panic,$/d' "$tree/crates/api/src/lib.rs"
    gate_red_because 'crates/api/src/lib.rs denies 3 of the 4 lints in the set' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_a_dev_only_root_took_half_the_set() {
    # Claim 2 is the one claim that runs over dev-only roots, and this is why: a harness that
    # denies `unwrap_used` and not `indexing_slicing` has made a decision nobody wrote down, and
    # the reported posture would say "denies 1 of the 4" while nothing went red.
    local tree
    tree="$(gate_scratch_tree)"
    sed -i '/^#!\[forbid(unsafe_code)\]/a #![cfg_attr(not(test), deny(clippy::unwrap_used))]' \
        "$tree/crates/testkit/src/lib.rs"
    gate_red_because 'the set is taken whole or not at all' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_the_wider_lint_taken_without_the_set_it_widens() {
    # `as_conversions` is an addition the device-integer crates make on top of the four, never a
    # substitute for them. Seeded at a dev-only root so that claim 1 stays silent and this arm is
    # evidence about claim 3 alone.
    local tree
    tree="$(gate_scratch_tree)"
    sed -i '/^#!\[forbid(unsafe_code)\]/a #![cfg_attr(not(test), deny(clippy::as_conversions))]' \
        "$tree/xtask/src/main.rs"
    gate_red_because 'without the whole panic/indexing set' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_every_root_dropped_the_device_integer_widening() {
    # The three crates that convert device-supplied integers took `as_conversions` deliberately
    # (rubric B10's own worked example is a truncated `bytesused`). A tree where none of them
    # still carries it has dropped a decision, and no other check in this workspace can see it.
    local tree
    tree="$(gate_scratch_tree)"
    sed -i '/clippy::as_conversions/d' \
        "$tree/crates/imaging/src/lib.rs" \
        "$tree/crates/backends/fake/src/lib.rs" \
        "$tree/crates/backends/v4l2/src/lib.rs"
    gate_red_because 'has dropped it silently' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_the_set_moved_behind_a_cfg_nobody_enables() {
    # `not(test)` is part of what is matched, and this is the reason. A set denied under a feature
    # nothing turns on reads exactly like a set denied on every shipped build, and the difference
    # is the whole claim: an unenabled `cfg` is the panic lints removed with the attribute left
    # behind for whoever reads the file next.
    local tree
    tree="$(gate_scratch_tree)"
    sed -i 's/^    not(test),$/    feature = "paranoid",/' "$tree/crates/engine/src/lib.rs"
    gate_red_because 'crates/engine/src/lib.rs is a shipped crate root that does not deny' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_the_block_landed_on_the_next_item_instead_of_the_crate() {
    # A lost `!`, which is what a refactor leaves behind and what the first shipping of this
    # predicate could not see (note **N167**). `#[cfg_attr(not(test), deny(…))]` is valid Rust:
    # in `crates/engine/src/lib.rs` it binds the `#[cfg(test)] mod double;` on the next line, so
    # the set is denied for a module that does not exist outside `cfg(test)` and the crate is
    # bound by nothing. Indistinguishable from the shipped file to a reader in a hurry, which is
    # why the `#!` is part of the match rather than assumed.
    local tree
    tree="$(gate_scratch_tree)"
    sed -i 's/^#!\[cfg_attr($/#[cfg_attr(/' "$tree/crates/engine/src/lib.rs"
    gate_red_because 'crates/engine/src/lib.rs is a shipped crate root that does not deny' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_the_block_was_commented_out_rather_than_removed() {
    # The other half of the same blindness, and the one the header used to deny having: block
    # comments were not stripped, so `/* … */` around the whole attribute was a **PASS** over a
    # crate root that denied nothing (note **N167**). This is the shape a bisect leaves — the
    # block is still in the file, still readable, and binds no compiler.
    local tree
    tree="$(gate_scratch_tree)"
    python3 - "$tree/crates/engine/src/lib.rs" <<'PY'
import re, sys
path = sys.argv[1]
text = open(path).read()
text = re.sub(r"(#!\[cfg_attr\(\n(?:.*\n)*?\)\]\n)", r"/*\n\1*/\n", text, count=1)
open(path, "w").write(text)
PY
    gate_red_because 'crates/engine/src/lib.rs is a shipped crate root that does not deny' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_a_root_the_graph_calls_an_rlib_leaves_the_population() {
    # The population is derived from target *kinds*, and the first spelling listed the three it
    # expected (`lib`, `bin`, `proc-macro`). A member declaring `crate-type = ["rlib"]` or
    # `["cdylib"]` would have dropped out in silence — `roots_checked` gets smaller and no claim
    # fires, which is note **N10**'s family again (note **N167**). No member does that today, so
    # the arm doctors the graph rather than the tree: `engine`'s lib is relabelled `rlib` **and**
    # pointed at a file that is not there, so the arm is red exactly when the target is still in
    # the population. Under the kind list it was green.
    local md
    md="$(gate_metadata_snapshot)"
    jq '(.packages[] | select(.name == "webcam-handler-engine") | .targets[] | select(.kind[] == "lib"))
        |= (.kind = ["rlib"] | .src_path = "/nonexistent/engine/src/lib.rs")' \
        "$md" >"$md.seeded"
    gate_red_because 'which does not exist in the tree under test' \
        env "WCH_GATE_METADATA=$md.seeded" "$GATE"
}

fail_case_a_crate_root_the_graph_names_is_not_in_the_tree() {
    # The population is derived, so it can point somewhere the tree does not go — a moved module,
    # a stale lock, a `src_path` nobody followed. Reading zero lints out of a file that is not
    # there would otherwise be indistinguishable from a root that denies nothing.
    local md
    md="$(gate_metadata_snapshot)"
    jq '(.packages[] | select(.name == "webcam-handler-engine") | .targets[] | select(.kind[] == "lib") | .src_path) = "/nonexistent/engine/src/lib.rs"' \
        "$md" >"$md.seeded"
    gate_red_because 'which does not exist in the tree under test' \
        env "WCH_GATE_METADATA=$md.seeded" "$GATE"
}

fail_case_a_graph_with_no_workspace_members() {
    # A walk over an empty population examines nothing and can go red about nothing, which is
    # AGENTS rule 3's vacuity case: this predicate must say so rather than report a clean tree.
    local md
    md="$(gate_metadata_snapshot)"
    jq '.workspace_members = []' "$md" >"$md.seeded"
    gate_red_because 'examined zero crate roots' \
        env "WCH_GATE_METADATA=$md.seeded" "$GATE"
}
