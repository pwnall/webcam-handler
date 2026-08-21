# Both-direction cases for `counted-selections.sh`.
#
# The pass arm runs the real `cargo nextest list` over the real workspace — the only
# listing that proves anything about the shipped selections. The failing arms use a stub
# lister so that a seeded table can be checked without rebuilding a copy of the workspace
# for each violation shape; what they exercise is the predicate's arithmetic and its
# error handling, which is where the `grep -c` zero-match trap lives.
#
# shellcheck shell=bash

# A lister that answers the way nextest **actually** answers, which is not the way the
# first version of this file assumed.
#
# `nextest list -T json` lists the *entire* workspace whatever `-E` says, and marks each
# testcase `matches` or `mismatch`; `test-count` is the size of that whole listing. The
# original stub returned `{"test-count":0,"rust-suites":{}}` for a non-matching filter — a
# shape nextest never produces — and the predicate agreed with the stub, so a gate that
# could not report zero for any real input passed its own both-directions selftest. PF:15
# records the same lesson about a Python probe: a second implementation only catches what
# it distinguishes. See note N10.
_stub_lister() {
    local path="$1"
    cat >"$path" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
# One matched test for the schema package; for anything else, the same full listing
# with the entry marked `mismatch` — which is what the tool does.
case "$1" in
*webcam-handler-schema*)
    printf '%s' '{"test-count":1,"rust-suites":{"webcam-handler-schema":{"testcases":{"slug::round_trips":{"kind":"test","ignored":false,"filter-match":{"status":"matches"}}}}}}'
    ;;
*)
    printf '%s' '{"test-count":1,"rust-suites":{"webcam-handler-schema":{"testcases":{"slug::round_trips":{"kind":"test","ignored":false,"filter-match":{"status":"mismatch","reason":"expression"}}}}}}'
    ;;
esac
SH
    chmod +x "$path"
}

# A table whose only `tests` row is this one, so an arm about one row is not also an arm about
# the hundred the stub lister would answer `mismatch` for.
#
#   $1  the scratch tree
#   $2  the selection
#   $3  the `what` column
_only_tests_row() {
    local tree="$1" selection="$2" what="$3" table
    table="$tree/scripts/gates/phase-criteria.tsv"
    grep -v '	tests	' "$table" >"$table.seeded"
    printf 'g0\ttests\t%s\t%s\n' "$selection" "$what" >>"$table.seeded"
    mv "$table.seeded" "$table"
}

pass_case() {
    "$GATE"
}

# The green direction for the branch check, and the reason it is here rather than left to
# `pass_case`: a splitter that silently produced *no* branches would satisfy every failing arm
# below by never disagreeing with anything. Both branches of this row select, and the row is the
# only `tests` row in the table it is checked in.
pass_case_every_branch_of_an_alternation_selects() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" 'package(webcam-handler-schema) and test(/^(slug::round|slug::)/)' \
        'two branches, both of which the lister answers for'
    WCH_GATE_ROOT="$tree" WCH_GATE_NEXTEST_LIST="$lister" "$GATE"
}

# **The P5 review's finding.** The row still selects a test, so the zero-selection check above is
# green and always would have been; what has rotted is one of the two claims its `what` column
# describes. Eleven `g5` rows are alternations of two to five named claims each, so this is the
# shape a criterion decays into rather than a hypothetical one.
fail_case_a_branch_of_an_alternation_selects_nothing() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" \
        'package(webcam-handler-schema) and test(/^(slug|zzz_no_such_module)::/)' \
        'two claims, one of which nothing selects any more'
    gate_red_because "none of them is one its branch 'test(/^zzz_no_such_module::/)' names" \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_NEXTEST_LIST=$lister" "$GATE"
}

# A regex this gate cannot split is a **failure and not a pass**, which is the price
# `gate_test_region_start` charges for a file boundary it cannot read, charged here for an
# alphabet. Green would mean "some of these claims are unchecked and nothing says which".
fail_case_an_alternation_this_gate_cannot_split() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" 'package(webcam-handler-schema) and test(/^(slug|zzz[0-9]+)::/)' \
        'an alternation whose branches this gate has no rule for'
    gate_red_because 'outside the alphabet' \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_NEXTEST_LIST=$lister" "$GATE"
}

# The other way a splitter can be handed something it cannot answer about. Inside the alphabet,
# so the arm above does not fire and this branch is the one under test.
fail_case_an_alternation_with_unbalanced_parentheses() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" 'package(webcam-handler-schema) and test(/^(slug|zzz::/)' \
        'an alternation with a parenthesis nobody closed'
    gate_red_because 'unbalanced parentheses' \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_NEXTEST_LIST=$lister" "$GATE"
}

# ---------------------------------- the same rule, for a clause that carries no alternation
#
# **The review of the batch above, one nesting level in.** The branch loop was entered only for a
# regex containing a `|`, so a row that reached a claim through a **lone** `test()` clause held
# that claim by nothing: the row stayed above the zero-selection check on the strength of its
# other disjuncts, and the day the lone clause's test was renamed the criterion went on naming a
# test the listing no longer holds, with this gate green. An alternation is one spelling of that
# defect and not the class of it (note **N249**, rubric A17), so the population is every
# `test(/…/)` clause in a selection and a clause with no `|` is a branch of one. Rows of this
# table reach a claim through such a clause today, in unions spelled with `or` and with nextest's
# `+`, so the shape is the table's own rather than a hypothetical one. These four arms are the
# widening's two directions and the two refusals it inherits.

# **The hole itself**, in the shape those rows are written in: a union whose other disjunct still
# selects, so the zero-selection check above is green and always would have been, beside a lone
# clause naming something the listing does not hold.
fail_case_a_lone_test_clause_names_a_test_the_row_no_longer_selects() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" \
        'package(webcam-handler-schema) + (package(webcam-handler-daemon) and test(/zzz_a_lone_clause_nothing_selects/))' \
        'a union whose other disjunct still selects, beside a clause that carries no alternation'
    gate_red_because "none of them is one its branch 'test(/zzz_a_lone_clause_nothing_selects/)' names" \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_NEXTEST_LIST=$lister" "$GATE"
}

# The green direction, and the reason it is its own arm rather than left to `pass_case`: a
# widening that refused every clause it could not find a `|` in would satisfy the arm above while
# turning the honest majority of this table red, and `pass_case` would say so without saying
# which shape did it. Both clauses here carry no alternation and both name something the lister
# answers for, and they are joined by the union operator the affected rows use.
pass_case_a_lone_test_clause_that_still_names_something_stays_green() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" \
        'package(webcam-handler-schema) and (test(/^slug::/) + test(/round_trips$/))' \
        'two clauses, neither of them a union, and the lister answers for what each one names'
    WCH_GATE_ROOT="$tree" WCH_GATE_NEXTEST_LIST="$lister" "$GATE"
}

# The alphabet refusal, over the widened population. A clause this gate cannot split is a
# **failure and not a pass** whether or not it is a union, because green would mean "a claim in
# this row went unchecked and nothing says which one". The arm names the clause as well as the
# alphabet, so a reader can tell it from the alternation arm above by the sentence alone.
fail_case_a_lone_test_clause_outside_the_alphabet_is_a_refusal() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" 'package(webcam-handler-schema) and test(/^slug[0-9]::/)' \
        'a clause with no alternation, spelled outside the alphabet the two engines agree on'
    gate_red_because "'test(/^slug[0-9]::/)' uses '[-]'" \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_NEXTEST_LIST=$lister" "$GATE"
}

# The other way the splitter can be handed a clause it cannot answer about, over the widened
# population. Inside the alphabet, so the arm above does not fire; and unbalanced without a `|`
# anywhere, which is the shape that reached the splitter for the first time with this widening —
# before it, a regex like this was skipped and its claim was held by nothing at all.
fail_case_a_lone_test_clause_with_unbalanced_parentheses_is_a_refusal() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" 'package(webcam-handler-schema) and test(/^(slug::/)' \
        'a clause with no alternation and a parenthesis nobody closed'
    gate_red_because "unbalanced parentheses — 'test(/^(slug::/)'" \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_NEXTEST_LIST=$lister" "$GATE"
}

# ------------------------------------- a count of tests in the prose, against the selection
#
# The G7–G9 review's finding #21. A `what` column said "The row above selects nine tests" about a
# selection the tool lists ten names for; the phrase was deleted (note **N318**) and the class was
# left with nothing that could go red on the next one. These arms are that class, and they are
# three rather than one because the rule is about **a cardinal qualifying the noun** and not about
# any one way of writing it — a ban that names one spelling is a ban on one spelling (note
# **N249**, rubric A17). The stub lister answers for exactly one test, so each seeded number is a
# claim the row demonstrably does not hold.

fail_case_a_row_counts_more_tests_than_its_selection_holds() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" 'package(webcam-handler-schema)' \
        'the 3 tests this row selects, which is one test and has been for a while'
    gate_red_because "its criterion says '3 tests'" \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_NEXTEST_LIST=$lister" "$GATE"
}

# The same claim spelled in words, which is how this table's smaller numbers are written: a rule
# that read `23` and not `seven` would have been green on most of the rows it exists for.
fail_case_a_count_written_in_words_is_the_same_claim_as_one_written_in_digits() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" 'package(webcam-handler-schema)' \
        'the seven tests this row selects, one per member of the vocabulary'
    gate_red_because "its criterion says 'seven tests'" \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_NEXTEST_LIST=$lister" "$GATE"
}

# The tens, spelled bare, which the word reader knew only as the left half of a compound: `ten`
# was consulted for `thirty-one` and never for `thirty`, so `thirty tests` through `ninety tests`
# named a number nothing read and the summary went on saying `checked 0 count(s)` about them.
# One spelling of the ban standing in for the class of it (note **N249**, rubric A17) — and in
# the direction that costs a silent miss rather than a loud refusal, which is the direction this
# rule exists for. The arm is red on `forty tests` against a row the lister answers one test for.
fail_case_a_count_written_as_a_bare_multiple_of_ten_is_still_a_count() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" 'package(webcam-handler-schema)' \
        'the forty tests this row selects, one for every control the walk reaches'
    gate_red_because "its criterion says 'forty tests'" \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_NEXTEST_LIST=$lister" "$GATE"
}

# And the same claim under this table's own house emphasis — the phrase that started all of this
# was written as `**1381 of 1381 tests**`, so a reader that stopped at the asterisk would have
# missed the instance it was written for.
fail_case_a_count_wrapped_in_emphasis_is_still_a_count() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" 'package(webcam-handler-schema)' \
        'each edit passed the **eleven** tests this row selects and turns none of them red'
    gate_red_because "its criterion says 'eleven tests'" \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_NEXTEST_LIST=$lister" "$GATE"
}

# **The exemption, driven.** A rate says how the row's tests are spread over some other
# population and nothing about how many of them there are, so `three tests per matrix cell` and
# `four tests apiece` are not counts of this row — and both numbers here are wrong *as* counts,
# which is what makes the green meaningful. `g4`'s signal-parity row and `g5`'s two matrix rows
# are worded this way, so an exemption that had quietly stopped applying would be a refusal those
# rows have no honest answer to; and an exemption no arm exercises is an exemption nobody checked.
pass_case_a_rate_over_another_population_is_not_a_count_of_this_row() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" 'package(webcam-handler-schema)' \
        'the bind by token matrix, three tests per matrix cell and four tests apiece over the two transports'
    WCH_GATE_ROOT="$tree" WCH_GATE_NEXTEST_LIST="$lister" "$GATE"
}

# **The remedy the refusal names, driven.** The failure above tells the author to say which
# population the number counts so that it stops qualifying the bare noun, and a refusal whose
# advice nobody has run is advice. This is `g6`'s repaired sentence in miniature: 1381 is a count
# of the workspace suite standing in a row that selects one test, and naming the population is
# what makes it true prose rather than a claim about this row.
pass_case_a_number_that_names_the_population_it_counts_is_not_a_claim_about_this_row() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" 'package(webcam-handler-schema)' \
        'each of the three one-identifier edits passed **1381 of 1381 workspace tests** and each turns this row red'
    WCH_GATE_ROOT="$tree" WCH_GATE_NEXTEST_LIST="$lister" "$GATE"
}

# ---------------- the same rule, through the punctuation this table's sentences actually carry
#
# **The review of the commit above.** The rule was landed with the word reader keeping a comma at
# both ends of every word, so that `1,381` would survive it — and the price was that `tests,` was
# not the noun `tests`, which is to say that finding #21's own sentence escaped the ban the moment
# a clause ran on after it. `the row above selects nine tests, one per hole` passed a table whose
# row holds one test, and the summary said `checked 0 count(s)`: a ban on one spelling of the
# spelling it was already a ban on (note **N249**, rubric A17). These three arms are the two
# directions that hole had and the property the comma was being kept for.

# The hole itself, in the shape the phrase it exists for is written in. The trailing `one per
# hole` is a rate over another population and is read as nothing, so this arm is red about the
# nine and about nothing else.
fail_case_a_count_a_clause_runs_on_after_is_still_a_count() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" 'package(webcam-handler-schema)' \
        'the row above selects nine tests, one per hole'
    gate_red_because "its criterion says 'nine tests'" \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_NEXTEST_LIST=$lister" "$GATE"
}

# The other direction of the same repair, and the reason it is a separate arm: the comma has to
# come off the word **after** the noun as well, or the fix above converts `g4`'s signal-parity
# sentence — written with the comma, as `one test, per signal` — from a rate this rule ignores
# into a count this rule refuses. A repair that reads the noun through punctuation and the rate
# through none would be red on honest prose and would have looked green in every arm above.
pass_case_a_rate_stays_a_rate_through_the_comma_the_sentence_sets_it_off_with() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" 'package(webcam-handler-schema)' \
        'one test, per signal, delivered for real to a daemon that is mid-sweep'
    WCH_GATE_ROOT="$tree" WCH_GATE_NEXTEST_LIST="$lister" "$GATE"
}

# And the property the comma was kept for in the first place, held by an arm rather than by the
# rule that cost the hole: a thousands separator stands **inside** a word and a strip of its two
# ends leaves it there. The arm is red on the phrase as written, `1,381 tests`, so a reader that
# went back to deleting every comma would go green here on `1381 tests` and be told it named the
# wrong sentence — which is the only way this claim can be checked from outside.
fail_case_a_thousands_separator_is_part_of_the_number_and_not_punctuation_round_it() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" 'package(webcam-handler-schema)' \
        'the 1,381 tests this row selects'
    gate_red_because "its criterion says '1,381 tests'" \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_NEXTEST_LIST=$lister" "$GATE"
}

fail_case_selection_matches_no_tests() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    printf 'g0\ttests\ttest(no_such_test_exists)\ta criterion whose filter has rotted\n' \
        >>"$tree/scripts/gates/phase-criteria.tsv"
    # The seeded row by name. The stub answers `mismatch` for every row it was not written for,
    # so this arm is loudly red about the whole table as well; what it is *named* for is the one
    # row it added, and that is the sentence it claims.
    gate_red_because "g0 selection 'test(no_such_test_exists)' selects zero tests" \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_NEXTEST_LIST=$lister" "$GATE"
}

# The arm the stub cannot provide: the *real* tool, and a filter that matches nothing.
#
# The stub above encodes our belief about nextest's output; this encodes nextest's. If a
# release changes the shape — drops `filter-match`, renames the status — the predicate
# starts counting the whole workspace again and this is the only case that notices.
fail_case_a_real_selection_that_matches_nothing_counts_zero() {
    local tree
    tree="$(gate_scratch_tree)"
    printf 'g0\ttests\tpackage(webcam-handler-schema) and test(/^zzz_no_such_module::/)\ta filter that has rotted\n' \
        >>"$tree/scripts/gates/phase-criteria.tsv"
    gate_red_because "g0 selection 'package(webcam-handler-schema) and test(/^zzz_no_such_module::/)' selects zero tests" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_lister_cannot_answer() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_red_because 'a selection that cannot be listed is not a selection that was checked' \
        env WCH_GATE_ROOT="$tree" WCH_GATE_NEXTEST_LIST=/bin/false "$GATE"
}

fail_case_no_selections_at_all() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    grep -v '	tests	' "$tree/scripts/gates/phase-criteria.tsv" \
        >"$tree/scripts/gates/phase-criteria.tsv.seeded"
    mv "$tree/scripts/gates/phase-criteria.tsv.seeded" "$tree/scripts/gates/phase-criteria.tsv"
    gate_red_because 'examined zero phase-gate test selections' \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_NEXTEST_LIST=$lister" "$GATE"
}

fail_case_criterion_runs_a_missing_script() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    printf 'g0\tcommand\t./scripts/gates/not-a-real-gate.sh\ta criterion that names a script nobody wrote\n' \
        >>"$tree/scripts/gates/phase-criteria.tsv"
    gate_red_because 'g0 criterion runs ./scripts/gates/not-a-real-gate.sh, which is missing or not executable' \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_NEXTEST_LIST=$lister" "$GATE"
}

fail_case_criteria_table_deleted() {
    local tree
    tree="$(gate_scratch_tree)"
    rm -f "$tree/scripts/gates/phase-criteria.tsv"
    gate_red_because '/scripts/gates/phase-criteria.tsv; there are no selections to count' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# --------------------------------------------------- every phase block runs the suite and its
# self-test
#
# The claim landed with the two rows it is about: g8 and g9 were the only blocks in the table
# without the `run-all.sh` + `selftest.sh` pair that g0 through g7 each open with, and no reader
# of the table could go red on that. These are its inverses, one per half, because the two
# `gate_fail`s are two sentences and an arm that seeded both would not say which one fired.
#
# The seed removes the pair from **one** phase rather than from every block that carries it: the
# claim is per-phase, and a table with the pair gone everywhere would be red under a rule that
# only ever looked at the first block. `_only_tests_row` runs first so the stub lister has one
# selection to answer for and the arm is about the row set rather than about a hundred rows the
# stub says `mismatch` to.
#
#   $1  the scratch tree
#   $2  the phase whose row to remove
#   $3  the script the removed row runs
_drop_command_row() {
    local tree="$1" phase="$2" script="$3" table
    table="$tree/scripts/gates/phase-criteria.tsv"
    awk -F'\t' -v phase="$phase" -v script="$script" \
        '!($1 == phase && $2 == "command" && $3 == script)' "$table" >"$table.seeded"
    if ! cmp -s "$table" "$table.seeded"; then
        mv "$table.seeded" "$table"
        return 0
    fi
    rm -f "$table.seeded"
    # `gate_seed_died` and not a second `printf` into the same file: the one home for "say a
    # seed did not apply" writes the sentence on **stderr** as well as into the report, and an
    # arm run by hand — which is the ordinary move while a gate is being written — is a console
    # with nothing on it otherwise, a `return 0` from a `fail_case_` reading as a pass. This was
    # the only file in `cases/` that named `gate_seed_report` itself (note **N330**).
    gate_seed_died "no $script row to remove from $phase: the seed this arm is built on is gone"
    return 1
}

fail_case_a_phase_block_never_runs_the_predicate_suite() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" 'package(webcam-handler-schema)' \
        'one selection the stub answers for, so this arm is about the row set'
    _drop_command_row "$tree" g0 ./scripts/gates/run-all.sh || return 0
    gate_red_because 'none of them runs ./scripts/gates/run-all.sh' \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_NEXTEST_LIST=$lister" "$GATE"
}

fail_case_a_phase_block_never_runs_the_predicates_self_test() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" 'package(webcam-handler-schema)' \
        'one selection the stub answers for, so this arm is about the row set'
    _drop_command_row "$tree" g0 ./scripts/gates/selftest.sh || return 0
    gate_red_because 'none of them runs ./scripts/gates/selftest.sh' \
        env "WCH_GATE_ROOT=$tree" "WCH_GATE_NEXTEST_LIST=$lister" "$GATE"
}

# The green direction the two arms above need, and it is not `pass_case`: what has to be shown is
# that the rule reads **each phase's own rows** rather than the table as a whole. A phase that
# carries both rows is green while a *different* phase carries neither in the same table would be
# a rule nothing could rely on — so this seeds the pair into a phase that has it and takes the
# suite row out of nothing, leaving a table every block satisfies by a route the reader can see.
pass_case_a_phase_that_carries_the_pair_twice_is_still_a_phase_that_carries_it() {
    local tree lister
    tree="$(gate_scratch_tree)"
    lister="$tree/stub-lister.sh"
    _stub_lister "$lister"
    _only_tests_row "$tree" 'package(webcam-handler-schema)' \
        'one selection the stub answers for, so this arm is about the row set'
    printf 'g0\tcommand\t./scripts/gates/run-all.sh\ta second naming of the suite, which is still a naming of it\n' \
        >>"$tree/scripts/gates/phase-criteria.tsv"
    WCH_GATE_ROOT="$tree" WCH_GATE_NEXTEST_LIST="$lister" "$GATE"
}
