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
    # The **direction**, not just the mismatch: this arm and the one below it seed opposite
    # defects and printed the same sentence until 2026-08-17 (note **N247**). A document behind
    # its schema is a serializer that stopped emitting a field; the other way round is a schema
    # behind its document, which on this tree means `just generate` was not run.
    gate_red_because 'the answer carries no captured_at, which the bundle requires' \
        env WCH_GATE_ROOT="$tree" "$GATE"
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
    gate_red_because 'the answer carries cameras, which the bundle does not declare' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_a_verb_answers_with_a_type_the_bundle_does_not_define() {
    local tree
    tree="$(gate_scratch_tree)"
    jq 'del(.["$defs"].ControlReport)' \
        "$tree/schemas/webcam-handler-schema.json" >"$tree/schemas/bundle.tmp"
    mv "$tree/schemas/bundle.tmp" "$tree/schemas/webcam-handler-schema.json"
    # shellcheck disable=SC2016  # the predicate's own sentence, backticks and $defs and all, matched verbatim
    gate_red_because 'does not match #/$defs/ControlReport in the committed bundle: the bundle defines no such type' \
        env WCH_GATE_ROOT="$tree" "$GATE"
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
    # shellcheck disable=SC2016  # the predicate's own sentence, backticks and $defs and all, matched verbatim
    gate_red_because 'does not match #/$defs/SessionStatus in the committed bundle: the answer carries session' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_bundle_is_missing() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$tree/schemas/webcam-handler-schema.json"
    gate_red_because "no schema bundle at schemas/webcam-handler-schema.json; 'just generate' writes it" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_corpus_has_no_profile_to_replay() {
    local tree
    tree="$(gate_scratch_tree)"
    # Without a profile the gate would have to fall back to attached hardware, which is
    # exactly the dependency it exists to avoid — so it must refuse rather than adapt.
    #
    # **This arm was red on the wrong sentence until 2026-08-17** (note **N248**): the predicate
    # had one branch for an empty corpus and for a corpus with nothing writable in it, so an
    # emptied `corpus/profiles/` was reported as *"no committed profile exposes a writable
    # integer control"* — true, and about the profiles that were no longer there.
    rm -f "$tree"/corpus/profiles/*.json
    gate_red_because 'there are no committed device profiles under corpus/profiles/' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# The branch the split above separated out, which had no arm of its own while the two shared a
# sentence: every profile still committed, and not one of them with a control this gate may
# write. That is what a re-capture of a fixed-function camera would leave behind, and the write
# rows would then have nothing to name — so it is a refusal rather than a shorter run.
fail_case_no_committed_profile_has_a_control_this_gate_may_write() {
    local tree file
    tree="$(gate_scratch_tree)"
    for file in "$tree"/corpus/profiles/*.json; do
        # `4` is V4L2_CTL_FLAG_READ_ONLY, which is the flag `writable_control` reads: every
        # control kept, every one of them refusing a write.
        jq '.invariant.controls = [ .invariant.controls[] | .flags.raw = 4 ]' \
            "$file" >"$file.seeded" && mv "$file.seeded" "$file"
    done
    gate_red_because 'exposes a writable integer control, so the write verbs cannot be exercised' \
        env WCH_GATE_ROOT="$tree" "$GATE"
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
    # The completeness sentence and not one of the validation ones: a mutant with a row missing
    # still validates every verb it kept, so what must go red here is the count of verbs the
    # table covers against the count `--help` offers.
    gate_red_because 'calibrate sweep' bash "$mutant"
}

# The same completeness claim asked of the node this walk learned to see at P8b: a verb that is
# **also a subtree**. `photo` takes a picture and `photo diff` compares two (D17), so a walk
# that read "has children" as "is only a prefix" would have stopped requiring a row for `photo`
# on the day the subcommand landed — with the leaf count unchanged, because `photo-diff`
# arrived in the same breath, which is the shape that makes this worth an arm rather than a
# comment. Seeded the way the arm above is, and for its reason: the alternative is building a
# `webcam-handler-cli` whose `photo` has no subcommand.
fail_case_a_verb_that_is_also_a_subtree_loses_its_validation_row() {
    local dir mutant
    dir="$(mktemp -d "$(gate_scratch_root)/wch-json-rows.XXXXXXXX")"
    mutant="$dir/$(basename "$GATE")"
    cp "$(dirname "$GATE")/lib.sh" "$dir/lib.sh"
    # `"photo|` and not `"photo`, so the `photo-diff` row beside it survives: dropping both
    # would leave the parent unvalidated *and* the child, and this arm is about the parent.
    grep -v '^    "photo|' "$GATE" >"$mutant"
    if grep -q '^    "photo|' "$mutant"; then
        printf 'selftest: the photo row was not removed\n' >&2
        return 0
    fi
    gate_red_because "which runs without naming a subcommand" bash "$mutant"
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
    gate_red_because 'a verb that cannot answer cannot be validated' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# ------------------------------------------------------------------ the failure document
#
# The rows added by the owner's ruling of 2026-08-15 (note **N127**). Both seams they use are
# in the *tree* — the bundle and the marker constant — so both arms below drive the real
# binary, the real corpus and the real predicate, which is the shape rubric rule 6 asks for.

# The schema side of the claim: a bundle with no `Failure` in it is a bundle a failure document
# cannot be validated against. This is the arm that would go red if somebody deleted the type's
# registration from `webcam-handler-xtask` and regenerated — the `--json` answers would all
# still validate and the refusals would stop being checked at all.
fail_case_the_bundle_does_not_define_the_failure_document() {
    local tree
    tree="$(gate_scratch_tree)"
    jq 'del(.["$defs"].Failure)' \
        "$tree/schemas/webcam-handler-schema.json" >"$tree/schemas/bundle.tmp"
    mv "$tree/schemas/bundle.tmp" "$tree/schemas/webcam-handler-schema.json"
    gate_red_because "does not match #/\$defs/Failure in the committed bundle: the bundle defines no such type" \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

# The failure document with its payload removed. `available` is what an unattended caller
# retries with, and a document that named the refusal and dropped the list would be the English
# sentence wearing braces — which is exactly the state note **N124** measured and this ruling
# repaired. Seeded in the *schema*, because the emitted document is the binary's and the binary
# comes from the real checkout: with `available` gone from `$defs/Failure`'s reachable `Error`,
# the document carries a property the bundle does not declare and the same jq that catches an
# envelope catches this.
fail_case_the_failure_document_carries_a_payload_the_bundle_does_not_declare() {
    local tree
    tree="$(gate_scratch_tree)"
    jq 'del(.["$defs"].Failure.properties.error)' \
        "$tree/schemas/webcam-handler-schema.json" >"$tree/schemas/bundle.tmp"
    mv "$tree/schemas/bundle.tmp" "$tree/schemas/webcam-handler-schema.json"
    gate_red_because "does not match #/\$defs/Failure in the committed bundle: the answer carries error, which the bundle does not declare" \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

# The other direction, and the one that keeps "unambiguously a failure" a checked claim: an
# answer wearing the marker a caller branches on. Seeded through the constant this gate reads
# out of the tree rather than by rebuilding a binary that emits one — point the predicate at
# `cameras`, which `list` really does answer with, and the first answering row is a successful
# verb carrying the marker that says a verb refused.
fail_case_an_answering_verb_carries_the_marker_that_says_a_verb_refused() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/^pub const FAILURE_MARKER: &str = "failed";$/pub const FAILURE_MARKER: \&str = "cameras";/' \
        "$tree/crates/schema/src/error.rs"
    gate_red_because 'answered with a document carrying' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

# And the constant has to be *there*: a gate that could not spell the marker would silently
# stop checking both halves of the claim, which is note N10's family — a predicate green while
# examining less than it says.
fail_case_the_tree_no_longer_declares_the_failure_marker() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed '/^pub const FAILURE_MARKER: &str =/d' "$tree/crates/schema/src/error.rs"
    gate_red_because 'no longer declares FAILURE_MARKER' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}
