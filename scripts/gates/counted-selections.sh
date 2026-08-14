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

# ------------------------------------------------------------------ splitting an alternation
#
# Three functions, and the only interesting one is the middle: a regex is a union of branches,
# and the branches are what the row's prose is a list of.

# Every `test(/…/)` regex in a selection, one per line. `test(name)` and `test(=name)` carry no
# regex and no alternation, so they are not matched here and need no splitting.
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
branches=0
commands=0
while IFS=$'\t' read -r phase kind selection what; do
    case "$phase" in
    '#'* | '') continue ;;
    esac

    case "$kind" in
    command)
        commands=$((commands + 1))
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

        # Every branch of every alternation in this row, against the names this row selected.
        # A row that still matches something is exactly where a lost branch hides.
        names="$(printf '%s' "$listing" | matched_names)"
        while IFS= read -r regex; do
            [[ "$regex" == *'|'* ]] || continue
            # `tr -d` and not a `[[ =~ ]]` bracket expression, because a backslash inside a
            # POSIX bracket expression is a literal member of the set — so the one spelling that
            # most needs catching, `\d`, would have satisfied a guard written that way.
            # shellcheck disable=SC2016  # a character set for `tr`, not a string to expand
            outside="$(printf '%s' "$regex" | tr -d 'A-Za-z0-9_:^$()|')"
            if [[ -n "$outside" ]]; then
                gate_fail "$phase selection '$selection' carries an alternation this gate cannot split — 'test(/$regex/)' uses '$outside', outside the alphabet [A-Za-z0-9_:^\$()|] that POSIX EREs and Rust's regex agree on character for character. A branch this cannot count is a claim nobody is counting, so it is a failure rather than a pass; spell the selection inside that alphabet, or teach this gate the construct and its two engines' agreement about it"
                continue
            fi
            if ! expanded="$(expand_alternation "$regex")"; then
                gate_fail "$phase selection '$selection' has an alternation with unbalanced parentheses — 'test(/$regex/)'"
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

gate_checked "$selections" "phase-gate test selections listed"
gate_checked "$branches" "named branch(es) of those selections' alternations, each counted against the tests its own row selected"
gate_checked "$commands" "phase-gate command criteria resolved to runnable scripts"
gate_require_nonzero "$selections" "phase-gate test selections"
# Not `gate_require_nonzero`: a table with no alternation in it anywhere is a legitimate table,
# and the day this suite has one is the day this line would be a gate failing over prose style.
# It is still counted and named above, because a branch check that quietly stopped finding
# branches is the same vacuous green one nesting level along.
if ((branches == 0)); then
    gate_note "no selection in the table carries an alternation, so no branch was counted"
fi

gate_finish
