# Both-direction cases for `mutation-scope-is-decided.sh`.
#
# Five claims, and the arms are grouped in the predicate's order: every product file is decided,
# no `examine_globs` entry outlives its module, no marker outlives its subject, nothing is in both
# lists, and every marker carries a reason. Under those, the two vacuity arms — a walk that parsed
# nothing and answered green is the skip that reads as a pass (notes **N160**, **N231**, **N235**).
#
# **`fail_case_a_new_module_is_in_neither_list` is the one this predicate exists for.** It seeds
# exactly what happened three times across P8: a module lands in `crates/` and nobody adds it to
# either list, so the floor silently stops covering the tree and the file's own law — *"an
# exclusion that is not a decision with a date is an oversight wearing one"* — is broken with
# nothing able to say so. If any arm here stops going red, that is the one to look at first.
#
# **`pass_case_a_directory_marker_covers_a_module_added_under_it` is what stops this predicate
# from being a ratchet nobody can live with.** The register blankets the crates whose prose
# blankets them, so a new module inside `crates/daemon/` is decided the moment it is written; an
# arm that only ever seeded a red would have let a predicate that refused *every* new module pass
# this suite, and that predicate is one somebody turns off in a week.
#
# shellcheck shell=bash

pass_case() {
    "$GATE"
}

# ------------------------------------------------- claim 1: every product file is decided

# The defect, in the crate it last happened in. `crates/imaging/` has no directory blanket —
# every one of its modules is named — so a new file there is genuinely undecided.
fail_case_a_new_module_is_in_neither_list() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/imaging/src/newborn.rs" <<'RS'
//! Seeded by the gate selftest: a fold that landed in neither of the floor's two lists.

pub(crate) fn brightest(samples: &[u8]) -> Option<u8> {
    samples.iter().copied().max()
}
RS
    # shellcheck disable=SC2016  # the predicate's own message, backticks and all
    gate_red_because 'crates/imaging/src/newborn.rs is in neither `examine_globs` nor a `scope-out:` marker' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# The other half of claim 1, and the half that keeps the rule livable: where the prose blankets a
# crate the marker blankets it, and a module added under the blanket is decided by it.
pass_case_a_directory_marker_covers_a_module_added_under_it() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/daemon/src/newborn.rs" <<'RS'
//! Seeded by the gate selftest: a module under a crate the register blankets.

pub(crate) fn ready() -> bool {
    true
}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

# A new *crate*, not just a new module — the population is derived from `cargo metadata`, so this
# is the arm that proves the derivation happened rather than a `crates/*/src` glob being walked.
fail_case_a_new_workspace_member_is_in_neither_list() {
    local tree md
    tree="$(gate_scratch_tree)"
    mkdir -p "$tree/crates/telemetry/src"
    cat >"$tree/crates/telemetry/src/lib.rs" <<'RS'
//! Seeded by the gate selftest: a whole crate in neither of the floor's two lists.
RS
    # The member is added to the *graph* rather than to `Cargo.toml`, because `gate_metadata`
    # resolves with `--locked` and a new member is a lockfile change — which would make this arm
    # red on cargo's refusal rather than on the sentence it claims, the confusion note **N60**
    # records. The graph is doctored from the real one (`gate_metadata_snapshot`'s whole reason),
    # so the seeded tree differs from the shipped one in exactly the one way this case describes.
    md="$(gate_metadata_snapshot)"
    jq '
        (.packages[] | select(.name == "webcam-handler-web")) as $model
        | .packages += [ $model
            | .name = "webcam-handler-telemetry"
            | .id = "path+file:///seeded/crates/telemetry#webcam-handler-telemetry@0.1.0"
            | .manifest_path = (.manifest_path | sub("crates/web/Cargo.toml$"; "crates/telemetry/Cargo.toml")) ]
        | .workspace_members += [ "path+file:///seeded/crates/telemetry#webcam-handler-telemetry@0.1.0" ]
    ' <"$md" >"$md.seeded"
    # shellcheck disable=SC2016  # the predicate's own message, backticks and all
    gate_red_because 'crates/telemetry/src/lib.rs is in neither `examine_globs` nor a `scope-out:` marker' \
        env WCH_GATE_ROOT="$tree" WCH_GATE_METADATA="$md.seeded" "$GATE"
}

# The population's other quiet exit, and this arm holds both directions of it in one place. A
# member whose sources are not under `src/` contributes nothing to the walk — `gate_find` returns
# nothing for a directory that is not there — so the crate leaves the register's population and
# every claim above stays true of what is left. That is the shape of the defect this predicate
# exists for, one level up: not a module nobody decided, but a whole member nobody looked at.
# The green half is asserted against the shipped tree, where every member has a `src/` and no
# such skip may be printed, so the arm cannot pass by the skip being unconditional.
pass_case_a_member_with_no_src_directory_leaves_the_population_by_name() {
    local md heard
    md="$(gate_metadata_snapshot)"

    heard="$("$GATE")" || return 1
    # shellcheck disable=SC2016  # the predicate's own message, backticks and all
    if grep -q 'no `src/` directory' <<<"$heard"; then
        printf 'the shipped tree printed a src-less skip and every member has a src/:\n%s\n' "$heard" >&2
        return 1
    fi

    # The member is added to the *graph* rather than to `Cargo.toml`, for
    # `fail_case_a_new_workspace_member_is_in_neither_list`'s reason: `gate_metadata` resolves
    # with `--locked` and a new member is a lockfile change. Nothing is created on disk, which is
    # the whole case — `crates/telemetry/src` is not there.
    jq '
        (.packages[] | select(.name == "webcam-handler-web")) as $model
        | .packages += [ $model
            | .name = "webcam-handler-telemetry"
            | .id = "path+file:///seeded/crates/telemetry#webcam-handler-telemetry@0.1.0"
            | .manifest_path = (.manifest_path | sub("crates/web/Cargo.toml$"; "crates/telemetry/Cargo.toml")) ]
        | .workspace_members += [ "path+file:///seeded/crates/telemetry#webcam-handler-telemetry@0.1.0" ]
    ' <"$md" >"$md.seeded"

    heard="$(WCH_GATE_METADATA="$md.seeded" "$GATE")" || return 1
    printf '%s\n' "$heard"
    # shellcheck disable=SC2016  # the predicate's own message, backticks and all
    grep -q '^  SKIP  .*no `src/` directory.*webcam-handler-telemetry' <<<"$heard"
}

# ------------------------------------------------- claim 2: no `examine_globs` entry outlives its module

# The module goes and the scope entry stays, which is how a floor quietly reports against a
# smaller population than the file says it covers.
fail_case_an_examine_glob_names_a_module_that_is_gone() {
    local tree
    tree="$(gate_scratch_tree)"
    rm "$tree/crates/imaging/src/y4m.rs"
    # shellcheck disable=SC2016  # the predicate's own message, backticks and all
    gate_red_because '`examine_globs` names crates/imaging/src/y4m.rs, which is not a product source file in this tree' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# ------------------------------------------------- claim 3: no marker outlives its subject

# The mirror image, and the direction `unsafe-scope.sh`'s register is checked in too: an exclusion
# a reader is told is still being carried, for a module that is not there.
fail_case_a_marker_outlived_the_module_it_excluded() {
    local tree
    tree="$(gate_scratch_tree)"
    rm "$tree/crates/imaging/src/fixtures.rs"
    # `fixtures` is declared by the crate root, so the seeded tree would not build; this predicate
    # never compiles anything, and what it is being asked is whether the register still describes
    # the tree — which is a question about the file listing.
    # shellcheck disable=SC2016  # the predicate's own message, backticks and all
    gate_red_because 'the `scope-out:` marker for crates/imaging/src/fixtures.rs excludes no product file this tree has' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# The blanket's version of the same defect: a crate that moved wholesale into the floor leaves a
# directory marker deciding nothing, and a reader believes an argument that has stopped applying.
fail_case_a_directory_marker_covers_nothing_it_still_decides() {
    local tree
    tree="$(gate_scratch_tree)"
    # shellcheck disable=SC2016  # `sed`'s insert command, not a shell expansion
    gate_seed '/^examine_globs = \[/a\
    "crates/web/src/lib.rs",' "$tree/.cargo/mutants.toml"
    # shellcheck disable=SC2016  # the predicate's own message, backticks and all
    gate_red_because 'the `scope-out:` marker for crates/web/ excludes no product file this tree has' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# ------------------------------------------------- claim 4: nothing is in both lists

# Two lines saying opposite things about one file, which is exactly as undecided as no line at
# all — and the shape a careless widening leaves behind, because widening is adding a line and
# nothing made anybody delete the marker underneath it.
fail_case_a_file_is_named_by_both_lists() {
    local tree
    tree="$(gate_scratch_tree)"
    # shellcheck disable=SC2016  # `sed`'s insert command, not a shell expansion
    gate_seed '/^# scope-out: crates\/imaging\/src\/lib.rs /i\
# scope-out: crates/imaging/src/metrics.rs — seeded by the gate selftest' \
        "$tree/.cargo/mutants.toml"
    # shellcheck disable=SC2016  # the predicate's own message, backticks and all
    gate_red_because 'crates/imaging/src/metrics.rs is named by `examine_globs` and by a `scope-out:` marker' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# ------------------------------------------------- claim 5: every marker carries a reason

# A path with nothing after it is the oversight wearing a decision's clothes, which is the whole
# subject of this predicate written on one line.
fail_case_a_marker_carries_no_reason() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's@^# scope-out: crates/cli/ — rendering$@# scope-out: crates/cli/@' \
        "$tree/.cargo/mutants.toml"
    # shellcheck disable=SC2016  # the predicate's own message, backticks and all
    gate_red_because 'has a `scope-out:` line this gate cannot read as `scope-out: <path> — <reason>`' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# ------------------------------------------------- the vacuity arms

# A scope with no entries would leave every claim here true of nothing — the population of files
# "in scope" is empty, so no glob can outlive a module and nothing is in both lists — while the
# floor itself covers the whole tree with nothing.
fail_case_examine_globs_is_empty() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed '/^examine_globs = \[/,/^\]/{/^    "/d}' "$tree/.cargo/mutants.toml"
    # shellcheck disable=SC2016  # the predicate's own message, backticks and all
    gate_red_because 'examined zero `examine_globs` entries' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# The register deleted rather than one row of it: every exclusion becomes an absence again, which
# is the state before this predicate existed and must not read as a pass.
fail_case_the_register_is_gone() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed '/^# scope-out: /d' "$tree/.cargo/mutants.toml"
    # shellcheck disable=SC2016  # the predicate's own message, backticks and all
    gate_red_because 'examined zero `scope-out:` markers' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# No scope file at all: a mutation run with no scope has no meaning, and neither does a claim
# about which of its two lists a module is in.
fail_case_the_scope_file_is_gone() {
    local tree
    tree="$(gate_scratch_tree)"
    rm "$tree/.cargo/mutants.toml"
    gate_red_because '.cargo/mutants.toml is not in the tree under test' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# The population's own inverse. Every claim above is about files this walk found, so a workspace
# that reports no members answers all five of them vacuously — and this is also the arm that says
# the population really is read out of `cargo metadata` rather than from a path pattern.
fail_case_the_workspace_reports_no_members() {
    local md
    md="$(gate_metadata_snapshot)"
    jq '.workspace_members = []' <"$md" >"$md.seeded"
    gate_red_because 'examined zero product source files' \
        env WCH_GATE_METADATA="$md.seeded" "$GATE"
}
