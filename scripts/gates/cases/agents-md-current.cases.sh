# Both-direction cases for `agents-md-current.sh`.
#
# The predicate makes four claims and each one is proved here by breaking it in a copy of
# the tree: that it *finds* the source document without being told its name, that it
# compares the two copies byte for byte, that the redirect beside the deployed copy is a
# pointer and nothing else (owner ruling, 2026-08-14; note N102), and that it refuses to
# answer at all when either comparison would be vacuous.
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

# --------------------------------------------------------------------- the redirect
#
# The 2026-08-14 ruling put a second file at the root — `CLAUDE.md`, holding `@AGENTS.md` and
# nothing else — and the arms below are the same discipline one file along. Two things shape
# them. Every seeded declaration change is applied to **both** copies of the source document,
# for the reason the reworded-sentence arm above gives: the doc is one of the two files the
# predicate compares, so an edit to the doc alone drags a drift in behind it and the arm goes
# red for the wrong sentence. And every arm here but the missing-file one names the sentence it
# claims, through `gate_red_because`, because these branches crowd each other: delete the link
# check and its arm falls through to the wrong-reference message, delete that one and its arm
# falls through to the byte comparison, and each of them stays comfortably non-zero while the
# branch it was written for has stopped existing. Only the last in the chain announces itself
# by going green. That is the rot an arm reading an exit status alone cannot see.

# The redirect's name is the doc's to choose. A rename there must move the gate, not break
# it, exactly as a renumbering of the series does above — so the file moves and the sentence
# moves with it, and the predicate must stay green having been told nothing.
pass_case_the_redirect_follows_a_rename_in_the_document() {
    local tree file
    tree="$(gate_scratch_tree)"
    mv "$tree/CLAUDE.md" "$tree/CLAUDE-CODE.md"
    for file in "$tree/docs/10-claude-fable-agents-v2.md" "$tree/AGENTS.md"; do
        # shellcheck disable=SC2016  # markdown backticks quoting a filename, not a command
        sed 's|root from `CLAUDE\.md`|root from `CLAUDE-CODE.md`|' "$file" >"$file.seeded"
        mv "$file.seeded" "$file"
    done
    WCH_GATE_ROOT="$tree" "$GATE"
}

# A `.gitignore` that swallowed it, a checkout that never had it, a tidy-up that read one
# line and eleven bytes as clutter. The reader who opens the Claude-specific name gets
# nothing, and never learns the rules exist under another one.
fail_case_the_redirect_is_missing() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$tree/CLAUDE.md"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The defect class the ruling creates, in the shape it will actually arrive in: somebody has
# a thing to say to a Claude reader in particular and the redirect looks like the place. Now
# two sets of non-negotiable rules are in force and neither reader can tell which it holds.
fail_case_the_redirect_carries_more_than_the_reference() {
    local tree
    tree="$(gate_scratch_tree)"
    printf 'Also: always run the tests twice, and never on a Friday.\n' >>"$tree/CLAUDE.md"
    gate_red_because 'is not exactly' env "WCH_GATE_ROOT=$tree" "$GATE"
}

# One reference, pointing somewhere that is not the rules. Eleven bytes, still one line, and
# the tree looks exactly as right as it did before.
fail_case_the_redirect_points_at_another_file() {
    local tree
    tree="$(gate_scratch_tree)"
    printf '@README.md\n' >"$tree/CLAUDE.md"
    gate_red_because 'opens with' env "WCH_GATE_ROOT=$tree" "$GATE"
}

# The redirect replaced by a link to what it points at. A reader following it lands on the
# rules, which is why this is tempting and why it must be named: "the redirect resolves to
# the rules" becomes true by construction, the content claim stops being a claim, and the
# ruling's pointer has quietly become the second copy it refuses.
fail_case_the_redirect_is_a_link_to_the_deployed_copy() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$tree/CLAUDE.md"
    ln -s AGENTS.md "$tree/CLAUDE.md"
    gate_red_because 'a link, not a reference' env "WCH_GATE_ROOT=$tree" "$GATE"
}

# The declaration the whole section is derived from, reworded away. What must go red here is
# "no document declares a redirect" — a claim whose population is gone is a claim that
# quantifies over nothing, which is the vacuous pass this suite exists to refuse.
fail_case_the_source_no_longer_declares_a_redirect() {
    local tree file
    tree="$(gate_scratch_tree)"
    for file in "$tree/docs/10-claude-fable-agents-v2.md" "$tree/AGENTS.md"; do
        sed 's|^Redirect at the repository root from|A file at the repository root points from|' \
            "$file" >"$file.seeded"
        mv "$file.seeded" "$file"
    done
    gate_red_because 'no document under docs/ declares a redirect' env "WCH_GATE_ROOT=$tree" "$GATE"
}

# Two documents declaring the redirect is two answers to which file the pointer is, and the
# second one is the copy nobody remembers making — the deploy side's own failure mode,
# inherited by the sentence beside it.
fail_case_a_second_document_declares_the_redirect() {
    local tree
    tree="$(gate_scratch_tree)"
    # shellcheck disable=SC2016  # markdown backticks quoting filenames, not commands
    printf 'Redirect at the repository root from `CLAUDE.md`, which holds `@AGENTS.md` and nothing else.\n' \
        >"$tree/docs/12-a-second-declaration.md"
    gate_red_because 'each declare a redirect' env "WCH_GATE_ROOT=$tree" "$GATE"
}

# The redirect declared as a path rather than a name at the root, for the reason the deploy
# side's twin gives: a predicate that followed `../CLAUDE.md` out of a scratch copy would
# resolve it against the directory above the tree it was handed and report on a file no arm
# seeded.
fail_case_the_declared_redirect_is_not_a_name_at_the_root() {
    local tree file
    tree="$(gate_scratch_tree)"
    for file in "$tree/docs/10-claude-fable-agents-v2.md" "$tree/AGENTS.md"; do
        # shellcheck disable=SC2016  # markdown backticks quoting a filename, not a command
        sed 's|root from `CLAUDE\.md`|root from `../CLAUDE.md`|' "$file" >"$file.seeded"
        mv "$file.seeded" "$file"
    done
    gate_red_because 'not a plain file name' env "WCH_GATE_ROOT=$tree" "$GATE"
}

# The two sentences in the same preamble disagreeing: this one deploys as `AGENTS.md` and
# that one says the redirect holds a reference to something else. Both files on disk are
# untouched and correct — what is wrong is the instruction, and a predicate that derived the
# expected reference from the redirect sentence alone would follow it off the rules and call
# the result a pass.
fail_case_the_declared_reference_is_not_the_deployed_copy() {
    local tree file
    tree="$(gate_scratch_tree)"
    for file in "$tree/docs/10-claude-fable-agents-v2.md" "$tree/AGENTS.md"; do
        # shellcheck disable=SC2016  # markdown backticks quoting a filename, not a command
        sed 's|which holds `@AGENTS\.md`|which holds `@README.md`|' "$file" >"$file.seeded"
        mv "$file.seeded" "$file"
    done
    gate_red_because 'while the same preamble deploys as' env "WCH_GATE_ROOT=$tree" "$GATE"
}
