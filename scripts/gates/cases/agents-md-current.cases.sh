# Both-direction cases for `agents-md-current.sh`.
#
# The predicate makes three claims and each one is proved here by breaking it in a copy of
# the tree: that it *finds* the source document without being told its name, that it
# compares the two copies byte for byte, and that it refuses to answer at all when the
# comparison would be vacuous.
#
# Every failing arm seeds exactly one divergence. Where the seeded change would otherwise
# create a *second* difference as a side effect — rewording the deploy sentence changes the
# doc, and the doc is one of the two files being compared — the same edit is applied to
# both copies, so the arm goes red for the reason it is named after and not for the
# byte-difference it dragged in behind it.
#
# shellcheck shell=bash

pass_case() {
    "$GATE"
}

# The predicate names neither file, and this is where that stops being a claim in a header
# comment. The series is reissued: doc 10 v2 becomes doc 11 v3 under a new filename, the
# deployed copy is unchanged and still byte-identical, and the gate must find the new
# source by the sentence it carries rather than go red — or worse, go green because it
# found nothing.
pass_case_the_source_document_may_be_reissued_under_another_number() {
    local tree
    tree="$(gate_scratch_tree)"
    mv "$tree/docs/10-claude-fable-agents-v2.md" "$tree/docs/11-claude-fable-agents-v3.md"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# Another document may write *about* the rule. This is not hypothetical and it is not a
# courtesy: the first version of the predicate read whole files, and `pass_case` went red on
# the shipped tree the day docs/9's predicate table gained the row describing this very gate,
# because the row quotes the sentence it documents. A gate that forbids documenting itself is
# a gate nobody can write around, so the search is the document's preamble — what it says
# about itself — and this arm holds that line from the other side.
pass_case_another_document_may_quote_the_rule_in_its_body() {
    local tree
    tree="$(gate_scratch_tree)"
    {
        printf '\n## A section discussing the deployment\n\n'
        # shellcheck disable=SC2016  # markdown backticks, quoting the rule under discussion
        printf 'AGENTS.md says: "Deploy at the repository root as `AGENTS.md`; the deployed copy tracks this file."\n'
    } >>"$tree/docs/6-claude-fable-design-v2.md"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The commit that edits the root copy because that is the file an agent has open. This is
# the defect class, in the direction it is most likely to arrive.
fail_case_the_deployed_copy_drifted() {
    local tree
    tree="$(gate_scratch_tree)"
    printf '\n9. A ninth non-negotiable rule that the doc has never heard of.\n' \
        >>"$tree/AGENTS.md"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The same defect from the other side: the docs series is edited and the deployed copy is
# left behind, which is the direction a docs-only session produces.
fail_case_the_source_moved_and_the_copy_did_not() {
    local tree doc
    tree="$(gate_scratch_tree)"
    doc="$tree/docs/10-claude-fable-agents-v2.md"
    printf '\n9. A ninth non-negotiable rule that the root copy has never heard of.\n' >>"$doc"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_deployed_copy_is_missing() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$tree/AGENTS.md"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# A rebase that drops the docs half, or a series renumbering that loses a file. The gate
# must not answer "nothing to compare, therefore fine".
fail_case_the_source_document_is_gone() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$tree/docs/10-claude-fable-agents-v2.md"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The sentence the whole predicate is derived from is reworded away. Applied to both files
# so the two copies stay byte-identical: what must go red here is "the source no longer
# declares a deployment", not a drift.
fail_case_the_source_no_longer_says_where_it_deploys() {
    local tree file
    tree="$(gate_scratch_tree)"
    for file in "$tree/docs/10-claude-fable-agents-v2.md" "$tree/AGENTS.md"; do
        sed 's/Deploy at the repository$/Keep this at the repository/' "$file" >"$file.seeded"
        mv "$file.seeded" "$file"
    done
    WCH_GATE_ROOT="$tree" "$GATE"
}

# Two documents claiming the same deployment is two answers to "which side was wrong", and
# the second one is the copy nobody remembers making.
fail_case_a_second_document_claims_the_deployment() {
    local tree
    tree="$(gate_scratch_tree)"
    cp "$tree/docs/10-claude-fable-agents-v2.md" "$tree/docs/11-claude-fable-agents-v3.md"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The deploy target is given as a path rather than a name at the root. Seeded in both
# copies for the reason the reworded-sentence arm gives: the finding under test is that the
# predicate refuses to resolve a target outside the tree it was handed, and a predicate
# that followed `../AGENTS.md` out of a scratch copy would compare the copy's doc against
# the *checkout's* root file and pass.
fail_case_the_deploy_target_is_not_a_name_at_the_root() {
    local tree file
    tree="$(gate_scratch_tree)"
    for file in "$tree/docs/10-claude-fable-agents-v2.md" "$tree/AGENTS.md"; do
        # shellcheck disable=SC2016  # markdown backticks quoting a filename, not a command
        sed 's|^root as `AGENTS\.md`|root as `../AGENTS.md`|' "$file" >"$file.seeded"
        mv "$file.seeded" "$file"
    done
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The comparison made unfalsifiable. Nothing here differs, the tree looks right, and a
# predicate without the same-file guard would report PASS over a population of one while
# proving nothing — which is why this arm exists rather than the guard being assumed
# defensive.
fail_case_the_deployed_copy_is_a_link_rather_than_a_copy() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$tree/AGENTS.md"
    ln -s docs/10-claude-fable-agents-v2.md "$tree/AGENTS.md"
    WCH_GATE_ROOT="$tree" "$GATE"
}
