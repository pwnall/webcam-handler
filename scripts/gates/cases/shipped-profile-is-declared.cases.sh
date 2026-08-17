# Both-direction cases for `shipped-profile-is-declared.sh`.
#
# The subject is one table in one manifest, so the failing arms are the ways that table stops
# saying what it says: the whole thing deleted (which is the state the G6 review found the tree
# in — note **N225**), the value flipped, the value replaced by something that is neither, a
# member carved back out of it — **in each of the three TOML spellings that says so**, because
# two of them walked around the first version of this predicate (note **N234**) — a dependency
# carved out by name, a carve-out written in a shape the walk declines to guess at, and a profile
# written into a member manifest where cargo throws it away. One arm doctors `cargo metadata`
# instead, because the *population* for claim 2 is derived from it: a graph with no members is a
# walk that examined nothing.
#
# Seeds land in scratch copies and nothing here builds. The claim is about what the manifest
# instructs cargo to do, so a seeded manifest that would not resolve is still exactly the shape
# being asked about — and a `cargo build` per arm would put minutes into a predicate that reads
# forty lines of TOML. The two bypasses **were** compiled, once, when they were found: `rustc`
# dropped `-C overflow-checks=on` for `webcam-handler-imaging` in both trees while this gate
# printed `PASS`. That transcript is N234's, and it is why these arms exist rather than a
# tightening nobody demonstrated.
#
# shellcheck shell=bash

pass_case() {
    "$GATE"
}

pass_case_the_same_tables_spelled_with_quotes_are_the_same_tables() {
    # TOML says `[profile."release"]` and `[profile.release]` are one table, and this walk said
    # they were two until note **N234** — which is the whole of how the quoted carve-out below
    # got past it. Quoting is normalised now, so a manifest that spells every one of these three
    # tables the other way must stay green, and the fail arms prove the normalisation did not
    # simply stop looking.
    local tree
    tree="$(gate_scratch_tree)"
    python3 - "$tree/Cargo.toml" <<'PY'
import sys
path = sys.argv[1]
text = open(path).read()
text = text.replace('[profile.release]\n', '[profile."release"]\n')
text = text.replace('[profile.release.package."*"]\n', "[profile.release.package.'*']\n")
open(path, "w").write(text)
PY
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_shipped_profile_is_not_declared_at_all() {
    # The defect as G6 found it: four profiles documented at length and no `[profile.release]`,
    # so `cargo install --locked --path crates/daemon` — the README's own line — built every
    # operator's binary at cargo's defaults.
    local tree
    tree="$(gate_scratch_tree)"
    python3 - "$tree/Cargo.toml" <<'PY'
import re, sys
path = sys.argv[1]
text = open(path).read()
text = text[: text.index("[profile.release]")]
open(path, "w").write(text)
PY
    gate_red_because 'the root manifest declares no [profile.release]' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_the_shipped_binaries_stop_checking_their_arithmetic() {
    # The table still there and the one value in it flipped, which is what an "it is a few
    # percent faster" edit looks like from a diff. `align_down` is the worked example: with
    # checks off it answered a range's *maximum* for `i64::MIN` (note **N224**).
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed '0,/^overflow-checks = true$/s//overflow-checks = false/' "$tree/Cargo.toml"
    gate_red_because 'does not set overflow-checks = true' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_the_value_is_neither_true_nor_false() {
    # A key that is present and unreadable is not the same failure as one that is absent, and
    # this arm exists because the walk answers a *string*: a match on `false` alone would let
    # `overflow-checks = "true"` — a TOML string, which cargo refuses — read as green here and
    # fail at build time instead.
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed '0,/^overflow-checks = true$/s//overflow-checks = "true"/' "$tree/Cargo.toml"
    gate_red_because 'does not set overflow-checks = true' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_one_workspace_member_is_carved_back_out_of_the_checks() {
    # Claim 1 undone one name at a time, which is the shape that would not read as a change to
    # the shipped arithmetic at all: the table above it still says `true`.
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/Cargo.toml" <<'TOML'

[profile.release.package.webcam-handler-imaging]
overflow-checks = false
TOML
    gate_red_because 'carves the workspace member webcam-handler-imaging out' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_a_member_is_carved_out_under_a_quoted_name() {
    # **The likelier of the two bypasses, because it is this manifest's own house style**:
    # `[profile.release.package."*"]` two lines up quotes its name, so the next carve-out written
    # by hand quotes its name too. The first version of claim 3 built the section name unquoted
    # and compared it to the raw bracket text, so this one matched nothing and passed (N234).
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/Cargo.toml" <<'TOML'

[profile.release.package."webcam-handler-imaging"]
overflow-checks = false
TOML
    gate_red_because 'carves the workspace member webcam-handler-imaging out' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_a_member_is_carved_out_by_a_key_under_the_package_table() {
    # The other bypass, and the same carve-out again: TOML lets the package table be written
    # once with an inline table per member, which is the shape a person reaches for when there
    # are two of them. Nothing in the old walk looked inside `[profile.release.package]` at all.
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/Cargo.toml" <<'TOML'

[profile.release.package]
webcam-handler-imaging = { overflow-checks = false }
TOML
    gate_red_because 'carves the workspace member webcam-handler-imaging out' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_a_dependency_is_carved_out_by_name() {
    # N225 measured the split against dependencies **as a class** and priced it at 17% of a
    # codec-bound photo. One dependency named on its own is a different decision with no
    # measurement behind it, and the gate's job is to make somebody write the sentence.
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/Cargo.toml" <<'TOML'

[profile.release.package.zune-jpeg]
overflow-checks = true
TOML
    gate_red_because 'carves the dependency zune-jpeg out' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_the_carve_outs_are_written_in_a_shape_this_check_will_not_guess_at() {
    # A `sed` walk that met an inline `package = { … }` and shrugged would be a predicate whose
    # silence is unearned — the one thing AGENTS rule 3 forbids of every rung. So the shape that
    # is not decoded is a finding with a name, and the remedy is in the message.
    local tree
    tree="$(gate_scratch_tree)"
    python3 - "$tree/Cargo.toml" <<'PY'
import sys
path = sys.argv[1]
text = open(path).read()
text = text.replace(
    '[profile.release]\noverflow-checks = true\n',
    '[profile.release]\noverflow-checks = true\n'
    'package = { "*" = { overflow-checks = false } }\n',
)
open(path, "w").write(text)
PY
    gate_red_because 'which this check will not guess at' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_a_profile_is_declared_where_cargo_throws_it_away() {
    # Cargo reads profiles from the workspace root and warns about one in a member — a warning
    # nobody sees on a green build. The file says the decision is there and the binary does not
    # have it, which is worse than the decision not being written down.
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/crates/daemon/Cargo.toml" <<'TOML'

[profile.release]
overflow-checks = false
TOML
    gate_red_because 'cargo reads profiles from the workspace root and ignores this one' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_a_graph_with_no_workspace_members() {
    # Claim 2 walks a derived population, so an empty one examines nothing and can go red about
    # nothing — AGENTS rule 3's vacuity case, which this must say rather than report a clean
    # tree. Claim 3 does not walk it: since note **N234** it asks the manifest who is carved out
    # rather than asking, member by member, whether that member is, and the member list is left
    # to say which of the answers is one of ours.
    local md
    md="$(gate_metadata_snapshot)"
    jq '.workspace_members = []' "$md" >"$md.seeded"
    gate_red_because 'examined zero workspace member manifests' \
        env "WCH_GATE_METADATA=$md.seeded" "$GATE"
}
