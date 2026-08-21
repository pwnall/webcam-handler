#!/usr/bin/env bash
#
# Every phase-gate test selection selects more than zero tests (rubric rule 3, docs/9).
#
# The predecessor's costliest gate failure was not a gate that went red — it was a gate
# whose test filter had quietly stopped matching anything and stayed green for months. A
# selection that matches nothing is not a passing criterion; it is an absent one.
#
# The population is the `tests` rows of `phase-criteria.tsv`, so a criterion added there
# is covered here without anybody remembering to add it twice.
#
# ## A row is not one claim, and counting it as one was the P5 review's finding
#
# Two dozen selections in that table are **alternations** — `test(/(a_photo_taken_during_a_
# preview|a_photo_with_no_preview_running_answers|a_capture_that_fails_mid_photo|…)/)` — and each
# named branch is a claim its `what` column describes at length. A row like that selects more
# than zero tests as long as **one** branch still matches, so four of the five could rot away
# with this gate green and the phase criterion going on describing five things. That is the
# predecessor defect this file's own header describes, one nesting level in: not a filter that
# stopped matching anything, but a filter that stopped matching most things.
#
# So an alternation is split and every branch is counted. The branches come out of the regex by
# distributing its groups — `^p::(a|b)` becomes `^p::a` and `^p::b` — because a branch checked
# without its prefix would be a different question from the one the row asks.
#
# **And a clause that is not a union is a branch of one.** That split was entered only for a
# regex carrying a `|`, which bans one spelling of the defect rather than the class of it (note
# **N249**, rubric A17): the class is *a criterion naming a test nothing checks it still selects*,
# and an alternation is where it was first seen. A row reaching a claim through a lone `test()`
# clause beside another disjunct held that claim by nothing — the row stayed above the
# zero-selection check on the strength of its other disjuncts, and the day the lone clause's test
# was renamed the criterion went on naming a test the listing no longer holds, with this gate
# green. Rows of this table are written that way today, in unions spelled `or` and in unions
# spelled with nextest's `+`, so the population below is **every `test(/…/)` clause in a
# selection**, split into the branches it is a union of, a clause with no `|` splitting into
# itself. Both refusals apply to the widened population unchanged: a clause this gate cannot
# split is a counted refusal and never a silent pass, which is the price charged above one
# nesting level along. That widening is this section's second delta rather than its first draft,
# and the row that prompted it was one this same batch had just widened to name an arm a crate
# down — the defect being repaired, reproduced inside the repair, which is note **N10**'s shape.
#
# A clause standing in a *conjunction* joins that population as well, and its check is implied by
# the zero-selection check above: every test such a row selected satisfied every conjunct of it,
# so no conjunct of it can come up empty. It is counted anyway rather than filtered out, because
# filtering would be this gate deciding which of a row's clauses are load-bearing from the shape
# the selection is written in today — and a row gains a disjunct the day somebody widens it,
# which is how the row that prompted all this arrived.
#
# **The spellings this does not reach, named rather than left silent.** `test(name)` and
# `test(=name)` carry no regex, so they are not in the population below and a rename inside one
# is not caught here. They are gated all the same, and by the check above rather than by this
# one: every such clause in this table stands in a conjunction, where a name that stops matching
# takes the whole selection to zero and the zero-selection check fails with its own sentence. The
# day one of them lands beside a disjunct is the day that stops being true, and the remedy is to
# widen `selection_regexes` and match those clauses as the substrings nextest reads them as.
#
# **The matching of a branch is done here rather than by a second `cargo nextest list` per
# branch**, and the trade is stated because note **N10** is about exactly this class. What the
# real tool decides is which testcases the row selected: that answer is `nextest list -T json`'s
# and this gate does not model it. What is done locally is asking which of *those* names each
# branch accounts for, with `grep -E` standing in for Rust's `regex` — and the two are held
# equal by construction rather than by hope: a regex is accepted only if every character of it
# is in `[A-Za-z0-9_:^$()|]`, an alphabet on which POSIX EREs and `regex` agree exactly, and a
# selection outside it is a **failure and not a pass** (`gate_test_region_start`'s price for a
# boundary it cannot read, charged for a regex). The alternative was seventy-odd extra listings
# at about 1.5 s each, which buys the same answer for a minute and a half of every `just ci`.
#
# ## A count of tests written in the prose is a claim about the selection beside it
#
# The G7–G9 review's finding #21: a `what` column said "The row above selects nine tests" about
# a selection the tool lists ten names for. It was false on the one commit that ever wrote it,
# and nothing could go red on it — this file measured the true figure and printed it three lines
# from the sentence it falsified, and compared it to nothing but zero. The phrase was deleted
# (note **N318**) and the class was left open, which is what AGENTS rule 1 is about.
#
# So the rule, stated as a class rather than as the one spelling that was caught (note **N249**):
# **in a `tests` row, a cardinal that immediately qualifies `test`/`tests` is a claim about how
# many tests that row selects, and it must equal the number measured above.** Where in the cell
# it appears, and whether it is written in digits or in words, is not part of it — `23 tests`,
# `five tests` and `**1381 of 1381 tests**` are one claim in three spellings, so the emphasis
# markers come off before the phrase is read. Neither is the punctuation the sentence wraps the
# phrase in: `nine tests,` is the claim `nine tests` makes, and the comma comes off with the full
# stop. That last clause is this paragraph's second delta rather than its first draft — the comma
# was deliberately *kept*, to protect a thousands separator, and the price was that the review of
# the landing commit could put finding #21's own sentence back into a seeded table with a clause
# running on after it and watch the gate stay green.
#
# **A rate is not a count.** `one test per matrix cell` and `one test apiece` say how the row's
# tests are distributed over some other population and nothing about how many there are, so they
# are exempt: `g4` and `g5` describe several matrix selections that way, and a rule that read a
# rate as a count would be demanding those rows lie about themselves. The exemption is exercised
# by a passing arm in the case file, because an exemption nobody drives is an exemption nobody
# checked.
#
# **`zero` is not in the lexicon**, and the reason is that the claim it would make is already
# gated: a row that really selects zero tests goes red at the check above, with a sentence about
# its own selection. What is left for this check to find are sentences like g4's "a reader who
# tidies this to `binary(webcam-handler-client)` selects nothing and this row runs zero tests",
# which is a hypothesis about a mis-edit rather than a count of the row as it stands.
#
# **The limits, stated here rather than left silent** — and stated without a cardinal in front
# of them, because a count of one's own paragraphs is the kind of figure that drifts the first
# time one is added and reconciles against nothing when it does. A `command` row has no measured
# selection, so this claim says nothing about one, and the phrase the table carries in such a
# row — g8's "Not one test the row above selects links `webcam-handler-v4l2`" — is a negation
# rather than a count and goes unread here, because there is no number beside it to compare
# against. The lexicon of number *words* runs from `one` to `ninety-nine`, the bare multiples of
# ten and the hyphenated compounds alike; a larger number spelled out in words would be missed,
# which costs nothing while every count in this table above twenty is written in digits. That the
# bare tens are in it is this paragraph's second delta rather than its first draft — the tens
# table was consulted for `thirty-one` and not for `thirty`, so seven of its eight words named a
# number nothing read; `twenty` was the one that worked, and only because it is the last entry of
# the units table as well. Note **N249** again, in the direction that misses in silence.
# A thousands group written with a **space** is the limit that remains, and it errs in the other
# direction: `1 381 tests` is read as its last group and claims 381, so a row spelling its
# thousands that way is refused out loud rather than passed over, naming a sentence whose author
# can respell the number. It is a spelling a `what` column could arrive in — this repository
# writes thousands that way in Rust prose — and it stays a limit rather than a hole for exactly
# that reason: the loud direction costs a re-spelling, the quiet one costs a false claim nobody
# sees.
#
# And the rate exemption is `per` and `apiece` and deliberately **not** `each`, which is the
# plainest English spelling of the same rate: `each` is also the distributive adverb, so
# `the three tests each drive a real refusal` is a true count of the row that exempting the word
# would swallow in silence. A row that wants the rate has `per` to say it with, and the refusal
# below names that remedy; the trade is a loud refusal of honest prose against a quiet acceptance
# of false prose, and this file takes the loud one every time.
#
# The `grep -c` trap docs/9 names is avoided by construction: nothing here pipes into a
# counter whose zero-match exit status could be swallowed. `cargo nextest list`'s exit
# status is captured explicitly, and a build failure or a malformed filterset is a
# failure of this gate rather than a zero that looks like an answer.
#
# $WCH_GATE_NEXTEST_LIST is the selftest's seam: it names a program invoked as
# `<prog> <filterset>` that prints a nextest JSON listing, which lets the failing arms
# exercise the zero-selection and lister-failure paths without a workspace build. The
# passing arm always runs the real `cargo nextest list` over the real tree.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"
table="$root/scripts/gates/phase-criteria.tsv"

if [[ ! -f "$table" ]]; then
    gate_fail "no criteria table at $table; there are no selections to count"
    gate_finish
fi

list_tests() {
    local selection="$1"
    if [[ -n "${WCH_GATE_NEXTEST_LIST:-}" ]]; then
        "$WCH_GATE_NEXTEST_LIST" "$selection"
    else
        (cd "$root" && cargo nextest list --locked --offline --workspace \
            -T json -E "$selection" 2>/dev/null)
    fi
}

# How many tests the selection matched.
#
# **Counted from `filter-match.status`, and this is the whole gate.** `nextest list -T json`
# lists the *entire* workspace whatever `-E` says, and marks each testcase `matches` or
# `mismatch`; its `test-count` is the size of that whole listing, not of the selection.
# Measured on cargo-nextest 0.9.138: a filter matching nothing reports `test-count: 143`,
# and summing the per-suite `testcases` maps gives 143 as well.
#
# So the first version of this function could not return zero for any input — which made a
# gate whose entire subject is "prove no selection has silently gone to zero" green by
# construction, from the day it was written. That is the predecessor defect this suite was
# built to prevent, reproduced inside the check for it. Recorded as note N10.
count_matched() {
    jq '[ .. | objects | select(has("testcases")) | .testcases | to_entries[]
          | select(.value["filter-match"].status == "matches") ] | length'
}

# The names of the tests the selection matched, one per line. What the branch check below
# quantifies over, and it is the tool's own answer rather than a re-derivation of it.
matched_names() {
    jq -r '[ .. | objects | select(has("testcases")) | .testcases | to_entries[]
             | select(.value["filter-match"].status == "matches") | .key ] | .[]'
}

# The (package, binary) pairs the matched tests live in — docs/9 asks selections to be
# compared as pairs, and a suite with no *matching* testcase matched nothing.
count_suites() {
    jq '[ .. | objects | select(has("testcases"))
          | select([ .testcases | to_entries[]
                     | select(.value["filter-match"].status == "matches") ] | length > 0) ]
        | length'
}

# ------------------------------------------------------- reading a count out of a row's prose
#
# Every cardinal in $1 that immediately qualifies `test`/`tests` and is not part of a rate, one
# per line as `<number><TAB><the phrase as it was written>`. The header above states the rule and
# its limits; this is only the reading of it.
counted_test_claims() {
    awk -v what="$1" '
    # The word as a reader sees it: the punctuation a sentence wraps around a word is not part
    # of the word, and a **comma is punctuation like any other** — this said otherwise until the
    # review of the commit that added it, on the reasoning that a comma is how a thousand gets
    # written, and it was keeping the comma at both *ends* of every word to protect a separator
    # that only ever stands in the middle of one. The cost was the hole: `nine tests,` cleaned to
    # `tests,`, which is not the noun, so the sentence the whole rule exists for escaped it the
    # moment its clause ran on. The same keeping was under the rate exemption, which reads the
    # word *after* the noun: `one test, per signal` arrived with the comma still on `per`, so the
    # rate was silent for the reason the noun was rather than for one of its own, and a repair
    # that read the noun through punctuation and the rate through none would have refused it as
    # a count of one against a row that selects two. Stripping only the two ends leaves `1,381`
    # with its separator and takes the comma off `1,381,`, which is the distinction wanted.
    function clean(x) {
        gsub(/^[^A-Za-z0-9-]+/, "", x)
        gsub(/[^A-Za-z0-9-]+$/, "", x)
        return x
    }
    # The number $1 names, or -1 for a word that names no number.
    #
    # **A bare multiple of ten is a number word like any other**, and this consulted `ten` only
    # for a hyphenated compound until the review of the batch that landed the rule: `thirty-one`
    # was a count this gate held and `thirty` was a word it read as no number at all, so
    # `thirty tests` through `ninety tests` passed with the summary saying `checked 0 count(s)`
    # about them. One spelling of the ban standing in for the class of it (note **N249**, rubric
    # A17), in the direction that misses in silence rather than the one that refuses out loud.
    # `twenty` reaches `unit` first and both tables give it 20, so the order of the two lookups
    # is not load-bearing.
    function cardinal(x,   at, tens_word, unit_word) {
        if (x ~ /^[0-9][0-9,]*$/) { gsub(/,/, "", x); return x + 0 }
        if (x in unit) return unit[x]
        if (x in ten) return ten[x]
        if (x ~ /^[a-z]+-[a-z]+$/) {
            at = index(x, "-")
            tens_word = substr(x, 1, at - 1)
            unit_word = substr(x, at + 1)
            if ((tens_word in ten) && (unit_word in unit) && unit[unit_word] >= 1 && unit[unit_word] <= 9)
                return ten[tens_word] + unit[unit_word]
        }
        return -1
    }
    BEGIN {
        # `zero` is deliberately absent; the header says why.
        n = split("one two three four five six seven eight nine ten eleven twelve thirteen " \
                  "fourteen fifteen sixteen seventeen eighteen nineteen twenty", u, " ")
        for (i = 1; i <= n; i++) unit[u[i]] = i
        n = split("twenty thirty forty fifty sixty seventy eighty ninety", t, " ")
        for (i = 1; i <= n; i++) ten[t[i]] = (i + 1) * 10

        # Emphasis is punctuation the reader sees through, so `**five** tests` is read as
        # `five tests`. Replaced by a space rather than deleted, so that a marker between two
        # words cannot glue them into a third one that is in neither vocabulary.
        gsub(/[*`]/, " ", what)
        n = split(what, w, /[ \t\n]+/)
        for (i = 1; i < n; i++) {
            noun = tolower(clean(w[i + 1]))
            if (noun != "test" && noun != "tests") continue
            word = clean(w[i])
            value = cardinal(tolower(word))
            if (value < 0) continue
            after = (i + 2 <= n) ? tolower(clean(w[i + 2])) : ""
            if (after == "per" || after == "apiece") continue
            printf "%d\t%s %s\n", value, word, clean(w[i + 1])
        }
    }'
}

# --------------------------------------------- splitting a clause into the branches it unions
#
# Three functions, and the only interesting one is the middle: a regex is a union of branches,
# and the branches are what the row's prose is a list of. A regex with no `|` in it is the union
# of one branch, and `expand_alternation` already answers that way — which is why the widening
# above is a deleted guard in the loop rather than a second code path beside these.

# Every `test(/…/)` regex in a selection, one per line. `test(name)` and `test(=name)` carry no
# regex, so they are not matched here; the header says where their claim is gated instead.
selection_regexes() {
    grep -oE 'test\(/[^/]*/\)' <<<"$1" | sed -e 's|^test(/||' -e 's|/)$||' || true
}

# The branches $1 is a union of, one per line. Exit 1 when the parentheses do not balance.
#
# Written as a distribution rather than a split, because a branch has to be asked the row's own
# question: `^photo::tests::a_photo_(with_no_preview|whose_settle_budget)` is two claims about
# the `photo` module, and `with_no_preview` on its own is a claim about the whole workspace.
# Groups are taken one at a time and the rest of the string is carried through each alternative,
# so a regex with two groups comes out as their product — which is what a regex engine would
# match, and there are none in this table today.
expand_alternation() {
    local re="$1"
    local depth=0 i char piece='' open=-1 close=-1
    local -a parts=()

    # A top-level `|` first: everything either side of it is a branch in its own right.
    for ((i = 0; i < ${#re}; i++)); do
        char="${re:i:1}"
        case "$char" in
        '(')
            depth=$((depth + 1))
            piece+="$char"
            ;;
        ')')
            depth=$((depth - 1))
            ((depth < 0)) && return 1
            piece+="$char"
            ;;
        '|')
            if ((depth == 0)); then
                parts+=("$piece")
                piece=''
            else
                piece+="$char"
            fi
            ;;
        *) piece+="$char" ;;
        esac
    done
    ((depth != 0)) && return 1
    parts+=("$piece")
    if ((${#parts[@]} > 1)); then
        for piece in "${parts[@]}"; do
            expand_alternation "$piece" || return 1
        done
        return 0
    fi

    # No top-level `|`: find the first group that holds one, and distribute it.
    depth=0
    for ((i = 0; i < ${#re}; i++)); do
        char="${re:i:1}"
        if [[ "$char" == '(' ]]; then
            ((depth == 0)) && open="$i"
            depth=$((depth + 1))
        elif [[ "$char" == ')' ]]; then
            depth=$((depth - 1))
            ((depth < 0)) && return 1
            if ((depth == 0)); then
                close="$i"
                if [[ "${re:open + 1:close - open - 1}" == *'|'* ]]; then
                    break
                fi
                open=-1
                close=-1
            fi
        fi
    done
    ((depth != 0)) && return 1

    if ((open < 0 || close < 0)); then
        # Nothing left to distribute: this is one branch.
        printf '%s\n' "$re"
        return 0
    fi

    local prefix="${re:0:open}" suffix="${re:close + 1}" inner alternative
    inner="${re:open + 1:close - open - 1}"
    parts=()
    depth=0
    piece=''
    for ((i = 0; i < ${#inner}; i++)); do
        char="${inner:i:1}"
        case "$char" in
        '(')
            depth=$((depth + 1))
            piece+="$char"
            ;;
        ')')
            depth=$((depth - 1))
            piece+="$char"
            ;;
        '|')
            if ((depth == 0)); then
                parts+=("$piece")
                piece=''
            else
                piece+="$char"
            fi
            ;;
        *) piece+="$char" ;;
        esac
    done
    parts+=("$piece")
    for alternative in "${parts[@]}"; do
        expand_alternation "$prefix$alternative$suffix" || return 1
    done
}

selections=0
clauses=0
branches=0
counts=0
commands=0
declare -A phase_rows=()
declare -A phase_suite=()
declare -A phase_selftest=()
while IFS=$'\t' read -r phase kind selection what; do
    case "$phase" in
    '#'* | '') continue ;;
    esac
    phase_rows["$phase"]=$((${phase_rows["$phase"]:-0} + 1))

    case "$kind" in
    command)
        commands=$((commands + 1))
        case "${selection%% *}" in
        ./scripts/gates/run-all.sh) phase_suite["$phase"]=1 ;;
        ./scripts/gates/selftest.sh) phase_selftest["$phase"]=1 ;;
        esac
        # A criterion is a command someone can run. If it names a script, the script has
        # to be there: a phase gate whose criterion is a missing file fails at the worst
        # possible moment otherwise.
        script="${selection%% *}"
        if [[ "$script" == ./* && ! -x "$root/${script#./}" ]]; then
            gate_fail "$phase criterion runs $script, which is missing or not executable"
        fi
        ;;
    tests)
        selections=$((selections + 1))
        listing=""
        status=0
        listing="$(list_tests "$selection")" || status=$?
        if ((status != 0)); then
            gate_fail "$phase selection '$selection' could not be listed (exit $status); a selection that cannot be listed is not a selection that was checked"
            continue
        fi
        matched="$(printf '%s' "$listing" | count_matched)"
        suites="$(printf '%s' "$listing" | count_suites)"
        if ((matched == 0)); then
            gate_fail "$phase selection '$selection' selects zero tests — $what"
            continue
        fi
        gate_note "$phase '$selection' selects $matched test(s) across $suites (package, binary) suite(s)"

        # Every count of tests the row's own prose makes, against the number just measured.
        while IFS=$'\t' read -r claimed phrase; do
            [[ -n "$claimed" ]] || continue
            counts=$((counts + 1))
            ((claimed == matched)) && continue
            gate_fail "$phase selection '$selection' selects $matched test(s), and its criterion says '$phrase' — in a \`tests\` row a cardinal that immediately qualifies \`test\`/\`tests\` is a claim about how many tests that row selects, so this one claims $claimed and the row holds $matched. Either the prose or the selection has gone stale; if the number counts some other population, say which population, so that it stops qualifying the bare noun — '$claimed workspace tests' is a claim about the workspace and not about this row. A rate is not a count and is not read as one, so '$claimed test(s) per …' and '$claimed test(s) apiece' are left alone"
        done < <(counted_test_claims "$what")

        # Every branch of every `test(/…/)` clause in this row, against the names this row
        # selected. A row that still matches something is exactly where a lost branch hides, and
        # a clause carrying no `|` is a branch of one — the header says why the loop is entered
        # for it rather than skipped.
        names="$(printf '%s' "$listing" | matched_names)"
        while IFS= read -r regex; do
            clauses=$((clauses + 1))
            # `tr -d` and not a `[[ =~ ]]` bracket expression, because a backslash inside a
            # POSIX bracket expression is a literal member of the set — so the one spelling that
            # most needs catching, `\d`, would have satisfied a guard written that way.
            # shellcheck disable=SC2016  # a character set for `tr`, not a string to expand
            outside="$(printf '%s' "$regex" | tr -d 'A-Za-z0-9_:^$()|')"
            if [[ -n "$outside" ]]; then
                gate_fail "$phase selection '$selection' carries a \`test()\` clause this gate cannot split — 'test(/$regex/)' uses '$outside', outside the alphabet [A-Za-z0-9_:^\$()|] that POSIX EREs and Rust's regex agree on character for character. A branch this cannot count is a claim nobody is counting, so it is a failure rather than a pass; spell the selection inside that alphabet, or teach this gate the construct and its two engines' agreement about it"
                continue
            fi
            if ! expanded="$(expand_alternation "$regex")"; then
                gate_fail "$phase selection '$selection' has a \`test()\` clause with unbalanced parentheses — 'test(/$regex/)'"
                continue
            fi
            while IFS= read -r branch; do
                [[ -n "$branch" ]] || continue
                branches=$((branches + 1))
                hits="$(grep -cE -- "$branch" <<<"$names" || true)"
                if ((hits == 0)); then
                    gate_fail "$phase selection '$selection' selects $matched test(s), and none of them is one its branch 'test(/$branch/)' names — the row still matches something, so the zero-selection check above stays green while the criterion goes on describing a claim nothing selects: $what"
                fi
            done <<<"$expanded"
        done < <(selection_regexes "$selection")
        ;;
    *)
        gate_fail "$phase row has unknown kind '$kind'"
        ;;
    esac
done <"$table"

# --------------------------------------------------------------- every phase runs the suite
#
# **A phase that adopts predicates and never runs their self-test closes on rules nothing has
# proven able to fail.** The pair — `run-all.sh` over the tree and `selftest.sh` over its own
# inverses — is the first thing that lands in a phase block and every one of g0 through g7
# carries it; g8 and g9 opened without it and reached their closing review that way, so
# `just gate-g8` would have run three predicates and `just gate-g9` three, none of them
# self-tested by the gate adopting them, and a P9-era repair to a predicate would have sat
# inside no open phase gate at all. Three and not four in g8's case, because a `command` row is
# not a predicate by being a row: the fourth one names `./scripts/rung-vivid.sh`, which lives
# outside `scripts/gates/` and is the R2 rung rather than a gate, so `gate_predicates` has never
# returned it and neither script above would have run it. Nothing in docs/13, docs/15 or this
# table's header grants a phase a dispensation, so the rule is asserted here rather than left to
# the reader who writes the next block. It is checked against the row set rather than against a
# list of phase names: the phases are whatever the table declares.
phases_checked=0
while IFS= read -r phase; do
    [[ -n "$phase" ]] || continue
    phases_checked=$((phases_checked + 1))
    if [[ -z "${phase_suite[$phase]:-}" ]]; then
        gate_fail "$phase declares ${phase_rows[$phase]} criteria and none of them runs ./scripts/gates/run-all.sh; every phase from g0 to g7 buys the whole predicate suite over the tree with one row, so a phase without it closes while the predicates it adopted are green only in \`just ci\` — add the row in the words its siblings use, in the same commit as whatever left it out"
    fi
    if [[ -z "${phase_selftest[$phase]:-}" ]]; then
        gate_fail "$phase declares ${phase_rows[$phase]} criteria and none of them runs ./scripts/gates/selftest.sh; a gate that runs its predicates without running their inverses closes a phase on rules nothing has proven able to go red, which is AGENTS rule 2 read as prose rather than as a criterion — add the row in the words its siblings use, in the same commit as whatever left it out"
    fi
done < <(printf '%s\n' "${!phase_rows[@]}" | sort)
gate_checked "$phases_checked" "phases the table declares, each asserted to carry both the predicate suite and its self-test"
gate_require_nonzero "$phases_checked" "phases in the criteria table"

gate_checked "$selections" "phase-gate test selections listed"
gate_checked "$clauses" "\`test(/…/)\` clause(s) in those selections, each split into the branches it is a union of"
gate_checked "$branches" "named branch(es) of those clauses, each counted against the tests its own row selected — a clause carrying no alternation is a branch of one"
gate_checked "$counts" "count(s) of tests written into a \`tests\` row's prose, each compared against the number that row selects"
gate_checked "$commands" "phase-gate command criteria resolved to runnable scripts"
gate_require_nonzero "$selections" "phase-gate test selections"
# Not `gate_require_nonzero`: a table whose every selection names its tests exactly, carrying no
# regex clause anywhere, is a legitimate table, and the day this suite has one is the day this
# line would be a gate failing over prose style. Both figures are counted and named above all the
# same, because a branch check that quietly stopped finding branches is the same vacuous green
# one nesting level along. The two notes below are two sentences rather than one because the two
# zeroes mean different things: `branches` is zero whenever `clauses` is, and it is **also** zero
# when every clause there was got refused above — a run that is already red, and that says so
# here in its own words rather than leaving a reader to infer it from a count of nothing.
if ((clauses == 0)); then
    gate_note "no selection in the table carries a \`test(/…/)\` clause, so no branch was counted"
elif ((branches == 0)); then
    gate_note "every \`test(/…/)\` clause in the table was refused above, so no branch was counted"
fi
# Not `gate_require_nonzero` either, and for the same reason one nesting level along: a table
# whose prose never counts its own tests is a legitimate table, and this rule exists to stop a
# number being wrong rather than to require one. It is counted and named above so that the day
# the population silently goes to zero is a line in the report rather than a quiet green.
if ((counts == 0)); then
    gate_note "no \`tests\` row's prose counts the tests it selects, so no count was compared"
fi

gate_finish
