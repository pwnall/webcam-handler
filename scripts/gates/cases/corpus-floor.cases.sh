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
    grep -rl 'chicony-rgb\|chicony-ir\|obsbot-tiny3' "$tree" --include='*.rs' |
        while IFS= read -r file; do
            sed -i 's/chicony-rgb//g; s/chicony-ir//g; s/obsbot-tiny3//g' "$file"
        done
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
    # Remove every route to the corpus — the walkers *and* the names — then add a profile.
    # Without either, the new file is exactly the dead corpus this gate is named for.
    grep -rl 'corpus::load_all(\|corpus::profile_paths(' "$tree" --include='*.rs' |
        while IFS= read -r file; do
            sed -i 's/corpus::load_all(/corpus_load_all_removed(/g; s/corpus::profile_paths(/corpus_profile_paths_removed(/g' "$file"
        done
    cp "$tree/corpus/profiles/chicony-rgb.json" "$tree/corpus/profiles/nobody-loads-me.json"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_corpus_is_parsed_but_never_replayed() {
    local tree
    tree="$(gate_scratch_tree)"
    # The corpus still loads and every profile is reachable; nothing turns one back into a
    # device. Claims 1 and 2 stay satisfied, so only claim 3 can produce the red.
    grep -rl 'FakeBackend::' "$tree" --include='*.rs' |
        while IFS= read -r file; do
            sed -i 's/FakeBackend::new(/FakeBackendNewRemoved(/g; s/FakeBackend::from_profile(/FakeBackendFromProfileRemoved(/g' "$file"
        done
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
