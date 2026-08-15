# Both-direction cases for `json-validates.sh`.
#
# The seams are the bundle and the binary. Doctoring the *bundle* is how the failing arms stay
# cheap: rebuilding `webcam-handler-cli` per case would make the selftest take minutes, and the
# claim under test — "the emitted document matches the committed schema" — goes red just as
# truly when the schema moves as when the document does.
#
# One arm does exercise the other direction, by doctoring the committed *profile* the
# answers are derived from, so the gate is not only sensitive to its schema input.
#
# shellcheck shell=bash

pass_case() {
    "$GATE"
}

fail_case_the_bundle_requires_a_property_the_answer_does_not_have() {
    local tree
    tree="$(gate_scratch_tree)"
    jq '.["$defs"].CameraList.required += ["captured_at"]' \
        "$tree/schemas/webcam-handler-schema.json" >"$tree/schemas/bundle.tmp"
    mv "$tree/schemas/bundle.tmp" "$tree/schemas/webcam-handler-schema.json"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_answer_carries_a_property_the_bundle_does_not_declare() {
    local tree
    tree="$(gate_scratch_tree)"
    # The envelope defect, seen from the schema side. The property has to be one the
    # answer *always* carries: `hints` is `skip_serializing_if = "Vec::is_empty"` and the
    # fake backend diagnoses nothing, so seeding on that one would never fire — a case
    # that cannot go red is the defect this whole harness exists to catch, and it caught
    # this one.
    jq 'del(.["$defs"].CameraList.properties.cameras)' \
        "$tree/schemas/webcam-handler-schema.json" >"$tree/schemas/bundle.tmp"
    mv "$tree/schemas/bundle.tmp" "$tree/schemas/webcam-handler-schema.json"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_verb_answers_with_a_type_the_bundle_does_not_define() {
    local tree
    tree="$(gate_scratch_tree)"
    jq 'del(.["$defs"].ControlReport)' \
        "$tree/schemas/webcam-handler-schema.json" >"$tree/schemas/bundle.tmp"
    mv "$tree/schemas/bundle.tmp" "$tree/schemas/webcam-handler-schema.json"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_calibration_answer_stops_matching_the_bundle() {
    local tree
    tree="$(gate_scratch_tree)"
    # The P3d rows, specifically. They are the only ones that need a *session* to exist
    # before they can answer, so an arm that goes red on one of them proves the whole
    # start-to-apply sequence ran rather than being skipped: `SessionStatus` is emitted by
    # `calibrate status`, which cannot answer at all unless `start`, `plan` and `sweep`
    # already did.
    jq 'del(.["$defs"].SessionStatus.properties.session)' \
        "$tree/schemas/webcam-handler-schema.json" >"$tree/schemas/bundle.tmp"
    mv "$tree/schemas/bundle.tmp" "$tree/schemas/webcam-handler-schema.json"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_bundle_is_missing() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$tree/schemas/webcam-handler-schema.json"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_corpus_has_no_profile_to_replay() {
    local tree
    tree="$(gate_scratch_tree)"
    # Without a profile the gate would have to fall back to attached hardware, which is
    # exactly the dependency it exists to avoid — so it must refuse rather than adapt.
    rm -f "$tree"/corpus/profiles/*.json
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The completeness half, and the only arm whose seam is the predicate's own row table rather
# than the tree. The P3 review's finding was that six of the seven `calibrate` subverbs could
# vanish from that table with the gate still green, so the shape of the inverse is exactly "a
# row is missing": there is no way to seed it in the tree without rebuilding
# `webcam-handler-cli` with an eighth subverb, which would cost a full compile per selftest
# run. The mutated *predicate* still drives the real binary, the real bundle and the real
# corpus — only the list under test is doctored, which is rule 2's "construct the buggy
# implementation" rather than N10's stub that agrees with its author.
fail_case_a_calibrate_subverb_loses_its_validation_row() {
    local dir mutant
    dir="$(mktemp -d "$(gate_scratch_root)/wch-json-rows.XXXXXXXX")"
    mutant="$dir/$(basename "$GATE")"
    cp "$(dirname "$GATE")/lib.sh" "$dir/lib.sh"
    # One row, named rather than positional, so a reordering of the table does not silently
    # turn this arm into a no-op.
    grep -v '^    "calibrate-sweep|' "$GATE" >"$mutant"
    if grep -q '^    "calibrate-sweep|' "$mutant"; then
        printf 'selftest: the calibrate-sweep row was not removed\n' >&2
        return 0
    fi
    bash "$mutant"
}

# The other half of the completeness claim, and since docs/7 P6e it has a seam in the *tree*
# rather than only in the predicate: the verb-to-document mapping moved to
# `crates/cli-core/json-contracts.tsv`, which three readers share (note **N122**). A row
# deleted there is a verb this gate can no longer validate and the agent guide can no longer
# teach — so it must be a failure here, and not a row this predicate quietly skips.
#
# Seeded in a scratch tree rather than in the predicate, which is what the arm above has to
# do and this one does not: the table is data now, so the buggy input can be built without
# doctoring the checker (rule 2's "construct the buggy implementation" in its stronger form).
fail_case_a_verb_loses_its_json_contract() {
    local tree
    tree="$(gate_scratch_tree)"
    grep -v '^controls	' "$tree/crates/cli-core/json-contracts.tsv" \
        >"$tree/crates/cli-core/json-contracts.tsv.seeded"
    mv "$tree/crates/cli-core/json-contracts.tsv.seeded" \
        "$tree/crates/cli-core/json-contracts.tsv"
    gate_red_because 'names no document for' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

# And the direction that makes the *lookup* meaningful: two tables is two answers, so a second
# copy of the mapping anywhere in the tree is refused rather than silently preferred by
# whichever `sort` happened to reach first.
fail_case_the_contract_table_has_a_second_home() {
    local tree
    tree="$(gate_scratch_tree)"
    cp "$tree/crates/cli-core/json-contracts.tsv" "$tree/schemas/json-contracts.tsv"
    gate_red_because 'exactly one home' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

fail_case_a_verb_stops_answering() {
    local tree
    tree="$(gate_scratch_tree)"
    # The other direction: the binary, not the schema. A profile whose document version
    # this build does not read makes every verb refuse, and a gate that reported green on
    # four failed commands would be checking nothing.
    for f in "$tree"/corpus/profiles/*.json; do
        jq '.schema_version = 99' "$f" >"$f.tmp" && mv "$f.tmp" "$f"
    done
    WCH_GATE_ROOT="$tree" "$GATE"
}
