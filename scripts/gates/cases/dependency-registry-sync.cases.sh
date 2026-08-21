# Both-direction cases for `dependency-registry-sync.sh`.
#
# One arm per branch and per shape the two facts really have. The passing arms are the load-bearing
# half here, because this predicate's whole difficulty is that §2.8's table is *not* a list of
# name/version pairs: a row may name three crates, a pin cell may carry feature text or a bracketed
# aside, and two rows deliberately have no manifest entry at all. A reconciler written against the
# simple shape would be red on the shipped tree, and a reconciler that answered that by skipping
# the awkward rows would be green while reading less than it claims (note N10's family). So the
# awkward shapes are seeded as arms that must stay green, and every failing arm names the sentence
# it claims — a seeded row is frequently red under two branches at once (an unregistered crate is
# also a row with no manifest entry), and an arm reading only the exit status cannot tell which
# fired.
#
# The version arms seed the **design** side rather than the manifest, deliberately: a changed
# version requirement in `Cargo.toml` invalidates `Cargo.lock`, and the predicate resolves
# `cargo metadata --locked` for its workspace-membership half — an arm that seeded the manifest
# would be measuring cargo's opinion of the lockfile rather than this predicate's opinion of the
# registry. The two arms that *do* seed the manifest add an entry nothing names, which is
# `futures-timer`'s shape and changes no resolution.
#
# shellcheck shell=bash

# A backtick, as a variable, for `browser-pins-sync.sh`'s reason: every seed and every claimed
# sentence below quotes a backticked crate name, and one written inside single quotes reads to the
# linter as an unexpanded command substitution.
tick='`'

_design() {
    printf '%s' "$1/docs/12-claude-fable-design-v3.md"
}

_manifest() {
    printf '%s' "$1/Cargo.toml"
}

# Add a row to §2.8's registry, after the last one, so no existing row's spelling is disturbed.
_append_row() {
    local tree="$1" row="$2"
    gate_seed "s@^\\(| ${tick}kamadak-exif${tick} .*\\)\$@\\1\\n${row}@" "$(_design "$tree")"
}

# Add a `[workspace.dependencies]` entry nothing in the workspace names.
_append_entry() {
    local tree="$1" entry="$2"
    gate_seed "s@^yuv = \"0.8.17\"\$@&\\n${entry}@" "$(_manifest "$tree")"
}

pass_case() {
    "$GATE"
}

pass_case_a_lock_only_row_is_not_reconciled() {
    # `hyper`'s shape, seeded a second time so the arm proves the *mark* and not the one row. The
    # seeded row would be red under the table-to-manifest direction on its own — nothing declares
    # `http-body` — and the mark in its pin cell is the only thing standing between it and a
    # violation. §2.8's lead-in is what buys it: a row marked **(lock only)** describes an edge the
    # walls police that no manifest names.
    local tree
    tree="$(gate_scratch_tree)"
    _append_row "$tree" \
        "| ${tick}http-body${tick} | **(lock only)** | MIT | daemon (via axum) | a second lock-only edge, seeded by the selftest |" ||
        return 0
    WCH_GATE_ROOT="$tree" "$GATE"
}

pass_case_a_row_may_name_several_crates_and_several_pins() {
    # Four shipped rows name two or three crates and state a pin per crate — `serde` / `serde_json`,
    # `thiserror` / `anyhow` (pins `2 / 1`), `anstream`/`anstyle` (`1.0.0 / 1.0.13`) and the three
    # `tracing` crates. Which pin belongs to which name is not something the table's own punctuation
    # settles, so those rows reconcile by membership and are counted as doing so; a predicate that
    # tried to pair them up would be red on the shipped tree, and one that dropped the rows would
    # stop registering six crates.
    local tree
    tree="$(gate_scratch_tree)"
    _append_row "$tree" \
        "| ${tick}png${tick} + ${tick}yuv${tick} | 0.18 / 0.8 | MIT/Apache | imaging | a second registration in the several-crate shape, seeded by the selftest |" ||
        return 0
    WCH_GATE_ROOT="$tree" "$GATE"
}

pass_case_a_manifest_row_nothing_names_is_registered_all_the_same() {
    # `futures-timer`'s shape: a `[workspace.dependencies]` entry no crate in this workspace names,
    # carried so the adoption is visible in the registry, with the lock rather than the line pinning
    # it. Consumers are not this predicate's subject and this arm is what says so — the naive
    # "every registered crate must have a consumer" branch would be red here and on the shipped
    # tree, and L32's residual is closed by the row *saying* what it is, not by a linkage walk.
    local tree
    tree="$(gate_scratch_tree)"
    _append_entry "$tree" 'once_cell = "1.21.3"' || return 0
    _append_row "$tree" \
        "| ${tick}once_cell${tick} | 1.21.3 | MIT/Apache | (nobody's; the lock pins it) | carried visibly, like futures-timer; seeded by the selftest |" ||
        return 0
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_crate_is_adopted_and_never_registered() {
    # N133's defect, three times over at the G6 review: `tower`, `tokio-stream` and `caps` were
    # adopted, argued for at length in `Cargo.toml`, and never learned by the sentence AGENTS sends
    # a reader to. The third is the single third-party edge of the root-equivalent helper.
    local tree
    tree="$(gate_scratch_tree)"
    _append_entry "$tree" 'once_cell = "1.21.3"' || return 0
    gate_red_because \
        "declares ${tick}once_cell${tick} and docs/12-claude-fable-design-v3.md's §2.8 registry has no row for it" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_registry_names_a_crate_the_manifest_dropped() {
    # L32's defect (note N164): `clap_complete` and `clap_mangen` sat in the registry and the
    # manifest for six phases for an emitter nobody wrote. Unmarked, so the row is claiming an edge
    # the product does not have.
    local tree
    tree="$(gate_scratch_tree)"
    _append_row "$tree" \
        "| ${tick}once_cell${tick} | 1.21.3 | MIT/Apache | schema | a pin with no manifest entry, seeded by the selftest |" ||
        return 0
    gate_red_because \
        "registers ${tick}once_cell${tick} and Cargo.toml's [workspace.dependencies] declares no such entry" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_two_disagree_about_a_version() {
    # N133's second drift: §2.8 read `tower-http 0.7` while `Cargo.toml` pinned 0.6.11 — the
    # document stating the higher number and the build using the lower. A minor that moves is the
    # shape this must catch, so the seed moves one: `0.6` against a manifest that declares `0.5.15`
    # is a disagreement, where `0.5` against `0.5.15` is the pin-at-adoption the table is for.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed "s@^| ${tick}zune-jpeg${tick} | 0.5 |@| ${tick}zune-jpeg${tick} | 0.6 |@" \
        "$(_design "$tree")" || return 0
    gate_red_because \
        "pins ${tick}zune-jpeg${tick} at 0.6 and Cargo.toml declares 0.5.15" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_mark_in_the_pin_cell_cannot_hide_a_crate_from_the_manifest_direction() {
    # §2.8's lead-in: "a mark buys a row an absent manifest entry and never a hidden one". The half
    # that can go wrong quietly is the second one — an implementation that dropped marked rows from
    # the population altogether would still be green on the shipped tree, because neither shipped
    # mark is the only registrar of anything, and the first adoption whose row carried a mark would
    # then go unregistered in silence.
    #
    # So the seed builds exactly that arrangement: an adoption whose only candidate row is marked,
    # with the row naming a near-miss spelling (`once-cell` for `once_cell` — the one every reader
    # of a crates.io name gets wrong once). The mark is right there and buys the entry nothing; the
    # manifest direction must still demand a row for it.
    local tree
    tree="$(gate_scratch_tree)"
    _append_entry "$tree" 'once_cell = "1.21.3"' || return 0
    _append_row "$tree" \
        "| ${tick}once-cell${tick} | **(lock only)** | MIT/Apache | (nobody's) | a marked row one hyphen away from the entry it looks like it registers, seeded by the selftest |" ||
        return 0
    gate_red_because \
        "declares ${tick}once_cell${tick} and docs/12-claude-fable-design-v3.md's §2.8 registry has no row for it" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_registry_row_names_no_crate_at_all() {
    # A row whose Crate cell lost its backticks registers nothing, so the crate it was about goes
    # unregistered and the row goes unreconciled — and both halves are silent unless the empty cell
    # is itself a finding. This is the emptied-population defect at row scale.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed "s@^| ${tick}jiff${tick} | 0.2 |@| jiff | 0.2 |@" "$(_design "$tree")" || return 0
    gate_red_because \
        "whose Crate cell ('jiff') names no backticked crate" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_registry_registers_one_of_this_workspaces_own_crates() {
    # The exclusion is derived from `cargo metadata`'s workspace members, and it runs both ways: a
    # path dependency needs no row, and a row for one is a registry claiming to have reviewed an
    # adoption that is this workspace's own code. Left unchecked it is also a way to satisfy the
    # manifest direction with a row that means nothing.
    local tree
    tree="$(gate_scratch_tree)"
    _append_row "$tree" \
        "| ${tick}webcam-handler-schema${tick} | 0.1.0 | MIT/Apache | workspace | our own crate, seeded by the selftest |" ||
        return 0
    gate_red_because \
        "which is one of this workspace's own crates rather than an adoption" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_section_2_8s_table_header_was_reworded_away() {
    # The population's anchor. A renamed column empties the table this predicate reads and every
    # claim above becomes vacuously true, which is the one shape a gate must never have — so a
    # header it cannot find is a failure and not a pass.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's@^| Crate | Pin | License | Scope | Why |$@| Package | Pin | License | Scope | Why |@' \
        "$(_design "$tree")" || return 0
    gate_red_because \
        "has no table whose header row is '| Crate | Pin | License | Scope | Why |'" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_section_was_renumbered_out_from_under_the_predicate() {
    # The other anchor, and the one a document revision moves: §2.8 is where AGENTS' "Docs and
    # dependencies" sends a reader, and a renumbering that left this predicate reading nothing would
    # be a green gate over an unread registry.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's@^### 2.8 Workspace, dependencies, licenses$@### 2.8 Workspace and dependencies@' \
        "$(_design "$tree")" || return 0
    gate_red_because \
        "no longer carries the heading '### 2.8 Workspace, dependencies, licenses'" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_design_document_is_gone() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$(_design "$tree")"
    gate_red_because \
        "there is nothing to reconcile Cargo.toml against" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_manifest_has_no_workspace_dependencies_table() {
    # The manifest-side fact gone. It is checked before `cargo metadata` is asked anything, on
    # purpose: a workspace whose dependency table has been renamed does not resolve at all, and a
    # predicate that met cargo's error first would report a broken toolchain where the finding is a
    # missing table.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's@^\[workspace.dependencies\]$@[workspace.dependencies-renamed]@' \
        "$(_manifest "$tree")" || return 0
    gate_red_because \
        "declares no [workspace.dependencies] table" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# ------------------------------------------------- claim 5: the Scope column, one direction

# **The defect as it was found, seeded back into the row it was found in** (note **N306**).
# `clap`'s cell said `cli-core` while `webcam-handler-daemon`, `webcam-handler-priv` and
# `webcam-handler-xtask` all declare it — and the crate with the strongest reason to be reviewed
# is the privileged helper, which is the very argument §2.8's own sentence gives for the column.
fail_case_a_scope_cell_understates_a_crates_real_edges() {
    local tree
    tree="$(gate_scratch_tree)"
    # shellcheck disable=SC2016  # the row's own backticks, quoted verbatim for `sed`
    gate_seed 's@^| `clap` | 4 | MIT/Apache | cli-core, daemon, priv, xtask |@| `clap` | 4 | MIT/Apache | cli-core |@' \
        "$(_design "$tree")"
    gate_red_because \
        "and \`webcam-handler-priv\` declares it as a normal dependency" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# The same claim over an edge that appears rather than a cell that shrinks — which is how it will
# actually happen: a crate picks up a dependency the workspace already registers, and nobody
# thinks to edit a table in a design document.
fail_case_a_member_takes_an_edge_no_row_scopes_it_to() {
    local md
    # The edge is added to the *graph* rather than to a manifest, because a new dependency is a
    # lockfile change and `gate_metadata` resolves with `--locked` — which would make this arm red
    # on cargo's refusal rather than on the sentence it claims (note **N60**). The graph is
    # doctored from the real one, so it differs from the shipped workspace in exactly this one way.
    md="$(gate_metadata_snapshot)"
    jq '
        (.packages[] | select(.name == "webcam-handler-imaging") | .dependencies[0]) as $model
        | (.packages[] | select(.name == "webcam-handler-imaging") | .dependencies)
            += [ $model | .name = "humantime" | .kind = null ]
    ' <"$md" >"$md.seeded"
    gate_red_because \
        "and \`webcam-handler-imaging\` declares it as a normal dependency" \
        env WCH_GATE_METADATA="$md.seeded" "$GATE"
}

# A *dev* edge is not a normal one and must stay green, or the claim would demand that every
# row list the crates whose test suites reach it — `image`'s three dev consumers, `tempfile`'s
# four — and the column would stop being about linkage. A gate that refused those is a gate
# somebody turns off.
pass_case_a_dev_only_edge_needs_no_scope_cell_entry() {
    local md
    md="$(gate_metadata_snapshot)"
    jq '
        (.packages[] | select(.name == "webcam-handler-imaging") | .dependencies[0]) as $model
        | (.packages[] | select(.name == "webcam-handler-imaging") | .dependencies)
            += [ $model | .name = "humantime" | .kind = "dev" ]
    ' <"$md" >"$md.seeded"
    WCH_GATE_METADATA="$md.seeded" "$GATE"
}

# The population's own inverse: a table whose every Scope cell had stopped naming a member would
# leave this claim true of nothing while reporting a green run, which is the skip that reads as a
# pass (note **N231**). Seeded by emptying every cell rather than by deleting rows, so the other
# four claims stay green and this arm is red for its own reason.
fail_case_no_row_scopes_anything_to_a_workspace_member() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's@^\(| `[^|]*| [^|]*| [^|]*\)| [^|]*|\(.*\)$@\1| workspace |\2@' \
        "$(_design "$tree")"
    gate_red_because \
        "examined zero rows whose Scope cell names a workspace member" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}
