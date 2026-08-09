# Both-direction cases for `json-validates.sh`.
#
# The seams are the bundle and the binary. Doctoring the *bundle* is how the failing arms
# stay cheap: rebuilding `wch` per case would make the selftest take minutes, and the
# claim under test — "the emitted document matches the committed schema" — goes red just
# as truly when the schema moves as when the document does.
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
