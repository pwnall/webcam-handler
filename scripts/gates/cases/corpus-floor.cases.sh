# Both-direction cases for `corpus-floor.sh`.
#
# The three claims each get an inverse, and the third one — "somebody replays it, not just
# parses it" — is the one worth having: a corpus that is merely deserialized would satisfy
# a weaker gate while proving nothing about the behaviour it was captured for.
#
# shellcheck shell=bash

pass_case() {
    "$GATE"
}

# A second green arm: a tree whose only coverage is a *walker* must pass. This is the
# shape the predicate is meant to encourage — a test that covers a profile added tomorrow
# without anybody editing it — so it must not be mistaken for a violation.
pass_case_coverage_by_a_whole_corpus_walk() {
    local tree
    tree="$(gate_scratch_tree)"
    # Strip every mention of the profiles by name, leaving only the walking calls.
    #
    # The whole list in one `gate_seed`, not one call per file inside the loop, and the
    # difference is what makes this arm say anything at all: a `grep -rl` that matched **nothing**
    # runs the loop body zero times, and this is a `pass_case_`, so the predicate would then be
    # run against an untouched tree and reported `ok` over an arm whose subject was never
    # produced (note **N186**). One call over the whole population turns "the profiles are no
    # longer named by hand anywhere" from a loop that may not have run into a claim that fails
    # loudly.
    local named=()
    mapfile -t named < <(grep -rl 'chicony-rgb\|chicony-ir\|obsbot-tiny3' "$tree" --include='*.rs')
    gate_seed 's/chicony-rgb//g; s/chicony-ir//g; s/obsbot-tiny3//g' "${named[@]}"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_corpus_is_empty() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$tree"/corpus/profiles/*.json
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_corpus_directory_is_gone() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -rf "$tree/corpus/profiles"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_profile_nobody_loads() {
    local tree
    tree="$(gate_scratch_tree)"
    # Claim 2 in isolation, which takes some care to seed. Removing the walkers is
    # necessary — with one present every profile is covered — but removing them alone
    # would ALSO empty the replayer set and turn the gate red via claim 3 instead. A case
    # that passes for the wrong reason proves nothing about the claim it names, so the
    # tree below keeps claim 3 satisfied by hand: a file that names each surviving profile
    # *and* constructs a backend.
    local walkers=()
    mapfile -t walkers < <(grep -rl 'corpus::load_all(\|corpus::profile_paths(' "$tree" --include='*.rs')
    gate_seed 's/corpus::load_all(/corpus_load_all_removed(/g; s/corpus::profile_paths(/corpus_profile_paths_removed(/g' "${walkers[@]}"

    {
        printf '// Seeded by the corpus-floor selftest: names every committed profile and\n'
        printf '// replays it, so claims 1 and 3 hold and only claim 2 can fire.\n'
        printf 'fn seeded_replay() {\n'
        for f in "$tree"/corpus/profiles/*.json; do
            printf '    let _ = "%s";\n' "$(basename "$f" .json)"
        done
        printf '    let _ = FakeBackend::new(Vec::new());\n'
        printf '}\n'
    } >"$tree/crates/backends/fake/tests/seeded_corpus_names.rs"

    # …and now the one profile nothing mentions.
    cp "$tree/corpus/profiles/chicony-rgb.json" "$tree/corpus/profiles/nobody-loads-me.json"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_profile_buried_where_the_loader_cannot_see_it() {
    local tree
    tree="$(gate_scratch_tree)"
    # `testkit::corpus` uses `read_dir` and does not recurse. A profile one directory down
    # is committed, reviewed, and never loaded by anything — dead corpus that a recursive
    # population would have counted as covered.
    mkdir -p "$tree/corpus/profiles/attic"
    cp "$tree/corpus/profiles/chicony-rgb.json" "$tree/corpus/profiles/attic/hidden.json"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_corpus_is_parsed_but_never_replayed() {
    local tree
    tree="$(gate_scratch_tree)"
    # The corpus still loads and every profile is reachable; nothing turns one back into a
    # device. Claims 1 and 2 stay satisfied, so only claim 3 can produce the red.
    local replayers=()
    mapfile -t replayers < <(grep -rl 'FakeBackend::' "$tree" --include='*.rs')
    gate_seed 's/FakeBackend::new(/FakeBackendNewRemoved(/g; s/FakeBackend::from_profile(/FakeBackendFromProfileRemoved(/g' "${replayers[@]}"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_rust_sources_are_gone() {
    local tree
    tree="$(gate_scratch_tree)"
    # Non-vacuity in the other direction: with no sources at all, "every profile is
    # covered" must not be vacuously true.
    find "$tree/crates" "$tree/xtask" -name '*.rs' -delete 2>/dev/null || true
    WCH_GATE_ROOT="$tree" "$GATE"
}
