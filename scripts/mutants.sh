#!/usr/bin/env bash
#
# The mutation floor (docs/7 P3f, docs/9 Part 2).
#
# Every other rung asks "does the suite run this code?". This one asks the question that
# matters: **does the suite constrain it?** cargo-mutants edits one line of a pure core —
# flips a comparison, empties a `Vec`, returns the other `bool` — rebuilds, and runs the
# whole workspace suite. A mutant the suite still passes is a line no test is watching.
#
# ## The scope lives in `.cargo/mutants.toml`
#
# The planners, the state machine, the settle policy, the store, the metrics, and since
# P5e the three `daemon::http` folds that decide who may reach a camera. That file carries
# the reasoning for what is in and what is deliberately out, one paragraph per decision.
# This script does not restate the list — it asks cargo-mutants what the scope resolved to
# and prints the answer, and refuses to run on an empty one: a mutation job over zero files
# is the "check that examines nothing" this suite exists to prevent.
#
# It said "Six files" until P5e, by which time the scope was ten and about to be fourteen —
# a count transcribed into a header two widenings before anybody read it again, in the very
# file whose next sentence promises not to restate the list. Kept as a sentence about what
# is in scope and not as a number, because the number has a home and this is not it.
#
# ## Survivors are triaged, never tolerated
#
# `scripts/mutants-accepted.txt` is the list of survivors this project has looked at and
# accepted, each with the note that argues no hermetic test can kill it. The comparison
# runs in **both directions**:
#
#   - a survivor that is not on the list fails this job — that is a missing test;
#   - a line on the list that no longer survives also fails it — the mutant became
#     killable, and an acceptance nobody re-checks is how N15's mistake gets made twice.
#
# The list is an exception register, not the population: the population is derived from
# the tree by cargo-mutants on every run.
#
# ## Three outcomes, and the one that was missing until note N68
#
# This job used to have two words for three different things, and every one of them was
# spelled `FAIL`:
#
#   - **a finding** — a survivor with no acceptance, or an acceptance that stopped
#     surviving. This is a statement about the *code and the register*, it is what the job
#     exists to produce, and it exits **1**;
#   - **a green** — the floor ran and the register is clean both ways. Exit **0**;
#   - **no verdict** — the run could not answer at all. The disk filled (note N66: the run
#     died fifteen minutes in with `Disk quota exceeded` and printed FAIL), it was
#     interrupted, its baseline would not build, or the working tree it spent an hour
#     reading **changed underneath it** (note N68). None of those is a statement about the
#     code. It exits **$GATE_NO_VERDICT** — 75, `EX_TEMPFAIL`; `scripts/gates/lib.sh`
#     argues the number — which is non-zero, because an unproven criterion must never read
#     as green, and *distinct*, because a machine's shortfall wearing a defect's clothes is
#     what N52, N66 and N68 each cost a session.
#
# The three are a class, not three accidents: N52's verdict moved with **time** (`nproc`,
# via the test timeout), N66's with **space** (free space on `/tmp`), N68's with a **moving
# input** (an agent editing files this run was still reading). All three are the machine
# being reported as the code. N60 records the bill: "a gate that cries wolf does not get
# believed, it gets re-run at `-j1` until it agrees, and the run after that is the one
# where a real survivor is waved through".
#
# A missing configuration file, an empty scope and a run that generated zero mutants stay
# **findings**, deliberately: those are statements about the tree, and a floor that has
# been quietly disarmed is exactly what this job is for.
#
# ## The seam: the verdict is a function of a directory of text files
#
# cargo-mutants writes `caught.txt`, `missed.txt`, `timeout.txt` and `unviable.txt` under
# its output directory — one mutant per line — beside `outcomes.json`'s census. Everything
# below the run reads exactly those files and nothing else, so **a recorded run is a
# fixture**: point $WCH_MUTANTS_CLASSIFY at a directory holding them and this script
# classifies it and exits, building nothing and generating no mutants. A fixture may carry
# its own register (`accepted.txt`) and its own record of the tree the run started from
# (`tree-before.txt`), which is the only way to exercise "the input moved" without moving
# anybody's checkout.
#
# That is a mode of the shipped script, not a second implementation of it, and the
# difference matters: `scripts/gates/mutation-verdict.sh` proves the three outcomes in
# seconds by driving *this file* over recorded result sets, so what is proved is what runs
# (rubric rule 6, paid for by note N10). A real run takes forty minutes and is nobody's
# unit test; the classification is the part that was wrong, and the classification is the
# part that is now driven both ways.
#
# ## It is a rung, not a `just ci` step
#
# It rebuilds the workspace once per mutant, so it costs hours where `just ci` costs
# minutes (docs/9 records the measured number). Its cadence is a G4 criterion — docs/7
# commissioned it "before G4, not after" — and `phase-criteria.tsv` carries the row, so
# the schedule is mechanical rather than remembered.
#
# Like every rung here it reports a named, counted skip rather than exiting quietly when
# the tool it needs is absent: cargo-mutants is a dev tool and installing it is never a
# requirement of `just ci`.
#
# ## The full run refuses to start on work that exists only on this machine
#
# **The ruling (owner, 2026-08-21):** a mutation-floor run may take as long as it takes,
# *provided the tree is committed and pushed right before it starts*. It is the same day's
# second ruling about this job and it rests on the same fact as the first, one step further
# along: the floor is the one thing here that runs for hours while stressing the machine, the
# build-root move below addresses the crash, and this addresses what a crash costs. Hours of
# machine time spent on work that exists in exactly one place is how the work gets lost.
#
# **This is a different axis from the tree recording further down, and the two are one argument
# rather than two.** That block asks about *result validity* — one verdict describes one tree —
# and it says in as many words that running this on a dirty tree is ordinary and that "you had
# uncommitted work" is not a finding. Nothing here withdraws a word of it. What is added is a
# question it never asked: not "is this the same tree the run started on", but "does this work
# exist anywhere other than the machine that is about to spend hours stressing itself".
#
# So the precondition follows the split the two recipes already make:
#
#   - **`just mutants`** — the full floor, hours, the only mode that may answer PASS — refuses
#     to start unless the checkout is committed and that commit exists on a remote. A narrowed
#     re-run (`just mutants -F store.rs`) is still this mode and is still refused: how long a
#     narrowing takes is not something this script can know, and a caller who does know has the
#     escape below.
#   - **`just mutants-iterate`, and `--iterate` however it arrives** — the triage tool, minutes,
#     "run it after each development stage" — carries no such precondition. Requiring a push
#     there would break the ordinary use the recording block is defending: checking a fix
#     *before* committing it is precisely what iterate mode exists for, and a triage tool that
#     demanded a push first would be a triage tool nobody ran.
#
# **It reads the local remote-tracking refs and calls nothing over the network**, because the
# rest of this suite is offline by construction. A push is exactly what updates `refs/remotes/…`,
# so a commit contained by one of them is a commit that has left this machine. The limit is the
# other side of that: it trusts the last push or fetch this checkout made, and it cannot see a
# remote somebody else has moved — or one that has since lost the ref — in between. That is the
# right side to err on, because the loss it exists to prevent is work that was never pushed at
# all rather than work whose remote has drifted.
#
# **It refuses rather than skipping**, which is `just rung-vivid-managed`'s idiom and AGENTS
# rule 3's reason: a caller who typed `just mutants` asked for the full floor, so answering zero
# would be a skip that reads as a pass. The refusal names its remedy — commit and push, then
# re-run — the way that recipe names `just bless`.
#
# **It is not a finding, and it exits `$GATE_NO_VERDICT` for the reason the three-outcome
# vocabulary above exists at all.** A precondition nobody met is not a statement about the code
# or the register: no mutant was generated, no survivor was seen, no acceptance was disproved.
# It is not a resource shortfall either, and it does not claim to be one — but `EX_TEMPFAIL`'s
# own sentence, "temporary failure; the user is invited to retry", is exactly the claim, and
# here the retry is one `git push` away. Spelling it 1 would file a refusal to start in the
# column a missing test is filed in, which is note **N66**'s whole lesson; spelling it 0 would
# be the skip that reads as a pass.
#
# **One named, loud escape**, in the register `WCH_NO_MOTION=1` holds for the motor suites:
# `WCH_MUTANTS_ALLOW_UNPUSHED=1` starts the full run anyway and prints a counted, named skip
# saying what it is accepting. A deliberate long run on unpushed work stays possible; what it
# may not do is happen quietly.
set -euo pipefail

# `lib.sh` for `$GATE_NO_VERDICT` and the three `gate_tree_*` helpers, and for nothing
# else: this is a criterion command rather than a gate predicate, and it keeps its own
# `mutants:` reporting vocabulary. Sourcing it is the alternative to a second copy of the
# exit-code convention and of the tree recording `selftest.sh` already had — and a second
# copy of a law is the defect AGENTS.md names, not a style preference. The library has no
# side effects at source time beyond setting variables.
#
# shellcheck source-path=SCRIPTDIR
# shellcheck source=gates/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/gates/lib.sh"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# The checkout, asked of git *at this script's own location* rather than at `$PWD`, so a
# run from another directory cannot resolve a different repository. A tree git cannot
# identify — a source tarball, a stripped export — still has a root: this file lives in
# `scripts/`, so its parent is it. The fallback is what makes "no git" a skip below
# instead of an abort.
tree_watched=1
if ! root="$(git -C "$here" rev-parse --show-toplevel 2>/dev/null)"; then
    root="$(dirname "$here")"
    tree_watched=0
fi
config="$root/.cargo/mutants.toml"
accepted_file="$root/scripts/mutants-accepted.txt"
# Where `.cargo/mutants.toml`'s `output` key puts the report. Under `target/` so the
# tree-walking gates and the selftest's tree copies do not pay for a rung they never run;
# that file says why.
out_dir="$root/target/mutants.out"

skips=0
skip() {
    skips=$((skips + 1))
    printf 'mutants: SKIP %s — %s\n' "$skips" "$*"
}

# A statement about the code or the register. The wording is unchanged from before N68,
# because this outcome was never the ambiguous one.
finding() {
    printf 'mutants: FAIL — %s\n' "$*" >&2
    exit 1
}

# A statement about *this run*, which produced no verdict to report. First argument is the
# headline; the rest are detail lines. Never green, never a finding, and it says which it
# is in words as well as in its exit code, because the reader this is written for is
# skimming for the string "FAIL".
no_verdict() {
    local headline="$1" line
    shift
    printf 'mutants: NO VERDICT — %s\n' "$headline" >&2
    for line in "$@"; do
        printf 'mutants:   %s\n' "$line" >&2
    done
    printf 'mutants: this run answered nothing, so it is NOT a finding: no unaccepted survivor was seen and no acceptance was disproved. Exit %s (EX_TEMPFAIL); %s named skip(s)\n' \
        "$GATE_NO_VERDICT" "$skips" >&2
    exit "$GATE_NO_VERDICT"
}

# This run's own bookkeeping — survivor lists, the sorted register, a throw-away criteria
# table — which is kilobytes and goes where all test scratch goes since the 2026-08-12 ruling
# (note N84). The *build* root is a different root and is answered further down, at
# `build_root`, where the measurement that used to except it, the 2026-08-21 ruling that
# withdrew the exception, and the 2026-08-22 ruling that gave it a short top-level directory of
# its own all live.
scratch="$(mktemp -d "$(gate_scratch_root)/wch-mutants.XXXXXXXX")"
trap 'rm -rf "$scratch"' EXIT

# ---------------------------------------------------------------- the tree, recorded
#
# **The floor reads the working tree for the better part of an hour**, one mutant at a
# time, and its verdict is only about the tree it read. Note N68: a `just gate-g4` started
# at 08:14 was still running at 09:19 when an agent edited `crates/engine/tests/sweep.rs`,
# `crates/engine/src/calibrate.rs` and `crates/engine/src/settle.rs` — all inside the scope
# or feeding the suite that judges it — and the run reported three N25 acceptances as lies.
# They were not: an N25 acceptance argues the mutant is the *same program* given the
# callers' preconditions, and no test can distinguish identical programs (which is exactly
# how N60 settled its own false positive). The verdict was neither true nor false, it was
# **void**, and nothing in this script could say so.
#
# Recorded and compared, not asserted clean, for `selftest.sh`'s reason: running this on a
# dirty tree is ordinary — it is how you check a fix before committing it — and "you had
# uncommitted work" is not a finding. What is a finding is that the tree is not the one the
# run started on.
#
# **That sentence stands, and the precondition in the header is not a retraction of it**: this
# block is about whether one verdict describes one tree, and `refuse_unless_committed_and_pushed`
# below is about whether hours of work exist anywhere but here. The two answers differ, which is
# why they are two questions — `--iterate` reaches this recording on a dirty tree exactly as it
# always did, and it is only the run that takes hours that has anything to lose.
tree_before=""
if ((tree_watched == 1)) && gate_tree_watchable "$root"; then
    tree_before="$(gate_tree_state "$root")"
else
    tree_watched=0
    skip "git cannot describe $root (a source tarball, or no git), so \"the tree did not move while this run read it\" was not checked; a missing tool is not a violation"
fi

# Compare the tree against what was recorded before the run. Called before anything else
# is interpreted, because a tree that moved explains every downstream symptom — a red
# baseline, a vanished mutant, an acceptance that "stopped surviving" — and reporting the
# symptom instead of the cause is what N68 is about.
tree_did_not_move() {
    local after
    ((tree_watched == 1)) || return 0
    after="$(gate_tree_state "$root")"
    [[ "$tree_before" == "$after" ]] && return 0
    local -a changes=()
    mapfile -t changes < <(gate_tree_changes "$tree_before" "$after")
    no_verdict \
        "the tree moved while the floor was reading it; a mutation result describes one tree and this run read two" \
        "${changes[@]}" \
        "(\`<\` is the tree this run started on, \`>\` is the tree it ended on; the first line of each is the commit)" \
        "nothing here is a claim about the code: re-run the floor on a tree nobody is editing"
}

# ------------------------------------------- the work a long run could take down with it
#
# The header's 2026-08-21 precondition, and the header is where its argument lives: why the
# full run carries it and `--iterate` does not, why the answer is a refusal rather than a skip,
# why the exit code is the no-verdict one, and what "read offline" can and cannot see. Only the
# mechanics are here.

# The ways out, printed by every refusal, because a refusal that does not say what to do next
# is a wall. `just rung-vivid-managed` names `just bless` for the same reason.
declare -a preflight_remedy=(
    "remedy: commit and push, then re-run \`just mutants\` — the run is long, and that is what makes it survivable"
    "or \`just mutants-iterate\`, the triage mode this precondition deliberately does not cover: minutes rather than hours, and it never claims the scope is clean"
    "or WCH_MUTANTS_ALLOW_UNPUSHED=1, which starts the full run anyway and says out loud, counted, what it is accepting"
)

# A precondition that was not met, so nothing ran at all. `no_verdict` carries the exit code and
# the "this is not a finding" trailer; this adds the word a reader skims for and the remedy.
refuse() {
    local headline="$1"
    shift
    no_verdict "REFUSED to start: $headline" "$@" "${preflight_remedy[@]}"
}

# Is what this machine is about to spend hours on also somewhere else? Answered from the tree
# state already recorded above and from the refs a push updates, and answered before anything
# is listed, built or generated.
refuse_unless_committed_and_pushed() {
    local -a recorded=() uncommitted=() containing=() remotes=() why=()
    local head_line head_commit shown

    if [[ "${WCH_MUTANTS_ALLOW_UNPUSHED:-0}" == "1" ]]; then
        skip "WCH_MUTANTS_ALLOW_UNPUSHED=1, so the full floor started without checking that this work exists anywhere else; hours are being spent on a checkout that may have no copy off this machine, and a machine this run takes down takes that work with it (owner ruling, 2026-08-21)"
        return 0
    fi
    if ((tree_watched == 0)); then
        skip "git cannot describe $root, so \"this work exists somewhere other than this machine\" was not checked either; a missing tool is not a violation"
        return 0
    fi

    # The same recording the tree-move check compares against, read rather than re-sampled: a
    # refusal has to describe the state this run would have started from, and two `git status`
    # calls are two states.
    mapfile -t recorded <<<"$tree_before"
    head_line="${recorded[0]:-HEAD <none>}"
    head_commit="${head_line#HEAD }"
    uncommitted=("${recorded[@]:1}")

    if ((${#uncommitted[@]} > 0)); then
        refuse "the checkout has ${#uncommitted[@]} uncommitted change(s), and this run reads the tree for hours" \
            "${uncommitted[@]}" \
            "(those are \`git status --porcelain\` lines; an untracked file counts, because untracked work is the work with nowhere else to be)"
    fi
    if [[ "$head_commit" == "<none>" ]]; then
        refuse "this checkout has no commit at all, so there is nothing a remote could be holding" \
            "$head_line"
    fi

    mapfile -t containing < <(
        git -C "$root" for-each-ref --format='%(refname:short)' \
            --contains "$head_commit" refs/remotes/ 2>/dev/null
    )
    if ((${#containing[@]} > 0)); then
        printf 'mutants: %s is on %s, so this run is not the only copy of the work it reads\n' \
            "$head_line" "${containing[*]}"
        return 0
    fi

    mapfile -t remotes < <(
        git -C "$root" for-each-ref --format='%(refname:short)' refs/remotes/ 2>/dev/null
    )
    if ((${#remotes[@]} == 0)); then
        why+=("this checkout knows no remote-tracking ref at all, so nothing here could say the work had left the machine; a remote and a first push are the fix")
    else
        shown="${remotes[*]:0:3}"
        ((${#remotes[@]} > 3)) && shown="$shown …"
        why+=("${#remotes[@]} remote-tracking ref(s) are known here and none of them contains it ($shown)")
    fi
    refuse "$head_line is on no remote this checkout knows of, and this run takes hours" \
        "${why[@]}" \
        "read offline, from the refs a push updates: this cannot see a remote somebody else has moved, and it trusts the last push or fetch made from here"
}

if [[ ! -f "$config" ]]; then
    finding "no scope at $config; a mutation run with no scope has no meaning"
fi
if [[ ! -f "$accepted_file" ]]; then
    finding "no acceptance register at $accepted_file"
fi

# The scope, for the run branch; empty and unused when a recorded run is being classified.
declare -a scope=()
scope_desc=""

classify_dir="${WCH_MUTANTS_CLASSIFY:-}"
if [[ -n "$classify_dir" ]]; then
    # ------------------------------------------------------------ the recorded-run mode
    #
    # See "the seam" above. Nothing is built and no mutant is generated: the mutants in a
    # fixture were generated by a real run once, and what is under test here is the
    # classification of them.
    if [[ ! -d "$classify_dir" ]]; then
        finding "WCH_MUTANTS_CLASSIFY=$classify_dir is not a directory; there is no recorded run to classify"
    fi
    out_dir="$classify_dir"
    scope_desc="the recorded result set at $classify_dir"
    printf 'mutants: classifying the recorded result set at %s (WCH_MUTANTS_CLASSIFY); nothing is built and no mutant is generated\n' \
        "$classify_dir"
    if [[ -f "$classify_dir/accepted.txt" ]]; then
        accepted_file="$classify_dir/accepted.txt"
        printf 'mutants: judged against the register that travels with it, %s\n' "$accepted_file"
    fi
    if [[ -f "$classify_dir/tree-before.txt" ]] && ((tree_watched == 1)); then
        # A recorded before-state is the whole point of a fixture for N68's defect: the
        # only other way to exercise "the input moved" is to move somebody's checkout,
        # which no gate here is allowed to do.
        tree_before="$(cat "$classify_dir/tree-before.txt")"
        printf 'mutants: the tree state this run started from was read from %s\n' \
            "$classify_dir/tree-before.txt"
    fi
    tree_did_not_move
else
    # ------------------------------------------------------------ the real run
    if ! cargo mutants --version >/dev/null 2>&1; then
        skip "cargo-mutants is not installed (\`cargo install cargo-mutants\`); it is a dev tool, never a workspace dependency, and \`just ci\` does not need it"
        printf 'mutants: 0 mutants run, %s named skip(s)\n' "$skips"
        exit 0
    fi

    # Which mode this is, decided here rather than at the block that forwards the flag, because
    # the precondition on the next line is a rule about one of the two modes and cannot ask
    # after the fact. The forwarding block still owns the flag itself, and still owns the
    # accounting: one mode, two doors, and only one place that decides which door was used.
    iterating=0
    if [[ "${WCH_MUTANTS_ITERATE:-0}" == "1" ]]; then
        iterating=1
    fi
    for arg in "$@"; do
        [[ "$arg" == "--iterate" ]] && iterating=1
    done

    # The 2026-08-21 precondition, on the full run only. After the tool check, because a machine
    # that cannot run the floor at all has no hours to lose to it and a refusal there would be
    # the wolf note **N60** prices; before the scope listing, the space budget and the baseline
    # build, because a run that may not start should not first spend a minute finding that out.
    if ((iterating == 0)); then
        refuse_unless_committed_and_pushed
    fi

    # The scope, counted — and asked of the tool rather than transcribed from the config it
    # reads, so a glob that has stopped matching shows up here as a smaller number instead of
    # as a silently narrower floor (note N10's family in a mutation costume).
    mapfile -t scope < <(cargo mutants --list-files)
    if ((${#scope[@]} == 0)); then
        finding "$config selects no files; a mutation run over nothing cannot go red"
    fi
    scope_desc="${#scope[@]} file(s)"

    printf 'mutants: scope is %s file(s), per %s\n' "${#scope[@]}" "${config#"$root"/}"
    printf 'mutants:   %s\n' "${scope[@]}"

    # **Parallelism decides the verdict here, not only the runtime** (note **N251**). A handful
    # of daemon and client integration suites drive settle logic on a *real* clock against a real
    # five-second deadline, so a loaded machine hands them a correct `SettleTimeout` where the
    # test asked about something else — and every such failure marks its mutant **caught**, which
    # is the expensive direction. The 8-job run on this host reported `0 missed` over nine real
    # survivors; the 3-job run found them, four with no acceptance. So `nproc` below is the fast
    # default and not the trustworthy one: until those suites take a clock the test owns,
    # `WCH_MUTANTS_JOBS=1` is what a verdict can be believed at, at something between 13 and 19
    # hours.
    jobs="${WCH_MUTANTS_JOBS:-$(nproc)}"

    # Debug info off, and this is a space decision before it is a speed one.
    #
    # `just ci`'s own `target/` on this machine is 34 GiB, nearly all of it DWARF for the
    # workspace's test binaries. cargo-mutants gives each job a whole copy of the tree with its
    # own build directory, so seven jobs at the shipped profile is seven copies of that — an
    # order of magnitude more space than any `tmpfs` `/tmp` holds, and the failure mode is a run
    # that dies on ENOSPC an hour in having produced nothing. Measured: the first build
    # directory of the run before this setting reached 6.1 GiB on its own. The setting survives
    # the 2026-08-21 move of the build root onto the disk below, because seven copies of that is
    # tens of gibibytes on any root and this one now shares its filesystem with `target/`.
    #
    # Turning debug info off cannot change a verdict — it changes what a backtrace can say, not
    # what a test asserts — and it makes the links this job spends most of its time on much
    # cheaper. It is an environment override rather than a profile in `Cargo.toml`, so the
    # shipped build configuration is exactly what `just ci` uses and this job's difference from
    # it is one line, here, in the open.
    export CARGO_PROFILE_DEV_DEBUG=0
    export CARGO_PROFILE_TEST_DEBUG=0

    # Each job is a whole copy of the tree with its own build directory. So the constraint on
    # parallelism is **space and I/O, not cores** — and both were measured on the machine this
    # job was written on, on 2026-08-09, over 410 mutants at five jobs (note **E7**):
    #
    #   - `$TMPDIR` on `tmpfs`: about 7 mutants a minute.
    #   - the same run with the build directories moved onto the disk holding `target/`: under
    #     one a minute. Concurrent cargo builds are I/O bound long before they are CPU bound,
    #     and putting them on the same spindle as everything else costs an order of magnitude.
    #
    # **The second figure does not survive this repository's own later measurement, so it is
    # not quoted below as what the move costs.** Note **N251** records a run of 2026-08-18 that
    # pointed `WCH_MUTANTS_BUILD_ROOT` off the `tmpfs` — at "the 314 GiB filesystem", named by
    # the free space this very `df` prints — and finished 1132 mutants in 2h23 at eight jobs.
    # That is about eight mutants a minute with the build trees *not* on the `tmpfs`, which is
    # E7's `tmpfs` rate rather than E7's disk rate. N251 does not name the path and the
    # inference is worth stating as one: this host mounts exactly one non-`tmpfs` data
    # filesystem, and it is the one `target/` is on. So what the move below costs is **not
    # measured on the current workspace**: E7 measured an order of magnitude, N251 shows no
    # sign of one, the two runs differ in job count, scope and month, and nobody has compared
    # the two roots since. It is left open rather than guessed, and the ruling below does not
    # rest on it — which is the whole reason it can be left open.
    #
    # ## Where the build trees live: under `target/`, since the owner ruled on 2026-08-21
    #
    # E7's 7× is why this job held an exception from the 2026-08-12 ruling that put all test
    # scratch under `target/` (note **N84**), and **the owner withdrew the exception on
    # 2026-08-21**. The measurement is kept above rather than deleted, because a reversal that
    # loses the number it lost to is a reversal nobody can re-litigate — and because what the
    # exception was argued on is precisely what the ruling declines to weigh. It is outranked,
    # and what outranks it is the difference between the two failures rather than the
    # difference between the two speeds. That the price is now unmeasured (two paragraphs up)
    # changes nothing here: an argument that survives the worst price it was ever quoted does
    # not need the price.
    #
    # `/tmp` on this host is a 16 GiB `tmpfs`, which is RAM: a run that fills it is not a slow
    # build, it is a run competing with the memory the machine is running in, and in the
    # owner's words it "could crash the system and make the machine inaccessible" — with the SSD
    # wear and the unsaved work that follows. A run on the disk that holds `target/` is, at
    # worst, slow. A slow run is a price this project can pay; an inaccessible machine is not
    # one it can choose.
    #
    # **And the budget below does not make the tmpfs path safe, which this session measured.**
    # `just mutants` as shipped, run during the 2026-08-21 gate closes, trimmed itself out loud
    # exactly as designed — `/tmp has 13 GiB free, so 3 job(s) rather than 8 (about 3 GiB each,
    # 3 GiB held back)` — built and tested its baseline in 109 s and 75 s, and then died at 337
    # of 1131 mutants (154 caught, 183 unviable, 0 missed) with `Worker thread failed: failed to
    # overwrite "/tmp/cargo-mutants-webcam-handler-VIvqdP.tmp/crates/engine/src/session.rs":
    # Disk quota exceeded (os error 122)`. A build tree that run left behind measured 3.5 GiB
    # against the `per_job_gib=3` the budget had divided by. That is note **N66**'s finding
    # recurring in its own shape — the per-job figure understated on a workspace that has grown
    # again — and it is the answer to "the budget check makes the exception safe": the budget is
    # exactly as good as its measurement of a tree whose size nobody re-measures, and that
    # measurement has now been the thing that was wrong twice.
    #
    # **Two causes were present and the second is the one that cannot be fixed here**, which is
    # note **N52**'s reading of the identical death at the P4c boundary: the `df` is sampled once
    # at the start and cannot see what arrives afterwards, and this `/tmp` was also holding the
    # session's own scratch, which grew all day beside the run. So a per-job figure that is
    # current does not close this hole either — on a filesystem shared with everything else on
    # the machine, no one-shot budget can. That is an argument about the `tmpfs` rather than
    # about the arithmetic, and it is the second reason the default moves.
    #
    # So the build root defaults to a directory under `target/` — `gate_mutants_build_root`,
    # beside the two other scratch roots in `lib.sh` — and `WCH_MUTANTS_BUILD_ROOT` now points
    # the other way: it is how somebody who wants the tmpfs speed, and accepts what filling a
    # RAM-backed filesystem does to the machine, asks for it. What the ruling settles is what
    # this script chooses when nobody has asked for anything.
    #
    # Three things follow that the exception did not have. `target/` is gitignored and carries
    # cargo's `CACHEDIR.TAG`, so these trees are already declared regenerable and are invisible
    # to every gate that walks the tree; they now sit inside a directory this project names, so
    # `gate_scratch_sweep` reclaims them — including the `cargo-mutants-*` directories N84 had to
    # record as a residual, because the sweep empties this root entry by entry rather than
    # looking for the `wch…` names cargo-mutants does not write; and that reach cuts both ways,
    # because `just scratch-sweep` passes an age of zero and takes everything, so a sweep run
    # beside a live floor now takes the trees the floor is building in. It is the same exposure
    # every other scratch user already had, over a run that lasts hours rather than seconds.
    #
    # ## Its own short directory under `target/`, and the two bytes that bought it
    #
    # **The ruling (owner, 2026-08-22):** "it's ok to use multiple top-level directories under
    # `target/` to get shorter paths. We dictate that directory's shape via the Cargo
    # configuration and the code + scripts in the repository."
    #
    # This is not tidiness and the root is not a child of `target/wch-scratch/`, which is where
    # the 2026-08-21 ruling first put it. **The line below exports the build root as `$TMPDIR`,
    # and `engine::paths::TempRuntimeDir` builds its socket paths under `$TMPDIR` on purpose**,
    # because `sun_path` holds 107 usable bytes and a socket path is not something a test may
    # spend a checkout's depth on. That doc comment is where the budget is argued and is the one
    # to read beside this; `gate_mutants_build_root` carries the arithmetic for this root. The
    # short version is that the socket suffix is 35 bytes,
    # `<checkout>/target/wch-scratch/wch-mutants-build` is 74, and 109 is two bytes over the
    # bound: the first run under that default died in `crates/daemon/src/systemd.rs` with `a
    # short path: Os { code: 36, kind: InvalidFilename, message: "File name too long" }`, and
    # cargo-mutants reported it as a red baseline in an unmutated tree — a machine's shortfall
    # wearing a defect's clothes, which is this file's whole no-verdict vocabulary. The short
    # root is 49 bytes and leaves 23 to spare.
    #
    # `$WCH_GATE_SCRATCH` is no longer a second door onto this decision, and that is a change
    # from the 2026-08-21 default worth stating: the root has stopped hanging off
    # `gate_scratch_root`, so a caller who has moved every gate's scratch onto a `tmpfs` has
    # **not** moved these trees there with it. `WCH_MUTANTS_BUILD_ROOT` is the one door, which is
    # the variable the paragraphs above already name.
    build_root="${WCH_MUTANTS_BUILD_ROOT:-$(gate_mutants_build_root)}"
    mkdir -p "$build_root"
    # wch-scratch-exempt: cargo-mutants reads the build root out of the environment
    export TMPDIR="$build_root"

    # The job count is then trimmed to what the build root can actually hold, out loud, from a
    # per-job figure measured with the debug info off — **and one job's worth is held back**.
    #
    # The reserve is not caution, it is a defect this job already had (note **N66**). Dividing
    # the free space by the per-job figure spends the whole filesystem: on this host that was
    # five jobs at three GiB in a sixteen GiB `tmpfs`, 15/16 of it, and the P4f boundary run died
    # fifteen minutes in with `Disk quota exceeded` — because the figure was measured on a
    # workspace that has since grown a crate. What made that worse than slow is what it *said*:
    # the floor exited 137 and reported FAIL, which is the same verdict a surviving mutant gets.
    # A gate whose resource budget can spell itself as a survivor is N52's finding in a second
    # dimension, and N60 records what the reflex costs — a run that is re-run until it agrees is
    # a run nobody reads. So the figure stays the measurement it is, and the *budget* leaves a
    # tree's worth of room.
    #
    # The figure is four because that is what was last measured, rounded the safe way: the build
    # tree the 2026-08-21 run left behind was 3.5 GiB, this arithmetic is integer, and the
    # direction that has now failed twice is the one where the figure is smaller than a tree. It
    # is a floor rather than a ceiling — that run died before it finished, so its tree had not
    # finished growing either — and the next run that dies on space is evidence this number moved
    # again rather than evidence about the code.
    #
    # This is note **N52**'s discipline rather than its reversal. That entry held the figure at
    # three and wrote down why — a tree measured at 2.5 GiB during the P4c triage — and refused
    # to pad it, in its words, "on a guess". The number moves here because a tree was measured
    # again, at 3.5 GiB on 2026-08-21, and four is that measurement rounded up by the integer
    # division below. A figure raised to buy headroom would stop being a measurement; a figure
    # left at a superseded one stops being current. Re-measure it, do not argue with it.
    per_job_gib=4
    # A whole tree stays held back, and the reason changed with the root rather than expiring
    # with it. On the 135 GiB `df` reports free for this checkout the reserve costs nothing:
    # `fits` stays far above `jobs`, the run takes every core it asked for, and the subtraction
    # only bites when the room is nearly gone — which is when it should. What it now protects is
    # also different. The build trees share a filesystem with `target/` itself, which this run's
    # own baseline build is still growing while the mutants run, so the tree held back is room
    # for this job's own work and not only for the last mutant.
    reserve_gib="$per_job_gib"
    avail_gib="$(df -BG --output=avail "$build_root" | tail -1 | tr -dc '0-9')"
    fits=$(((avail_gib - reserve_gib) / per_job_gib))
    if ((fits < 1)); then
        # A resource shortfall, and since N68 it is spelled as one. It was a FAIL until
        # then — the same word a surviving mutant gets — which made the floor's verdict a
        # function of how much room `/tmp` happened to have.
        no_verdict \
            "$build_root has $avail_gib GiB free, and one build tree needs about $per_job_gib with $reserve_gib held back" \
            "this is a fact about the filesystem, not about the tests: set WCH_MUTANTS_BUILD_ROOT to a larger one and the same tree will answer"
    fi
    if ((fits < jobs)); then
        printf 'mutants: %s has %s GiB free, so %s job(s) rather than %s (about %s GiB each, %s GiB held back)\n' \
            "$build_root" "$avail_gib" "$fits" "$jobs" "$per_job_gib" "$reserve_gib"
        jobs="$fits"
    fi
    printf 'mutants: build directories under %s (%s GiB free, %s GiB held back)\n' \
        "$build_root" "$avail_gib" "$reserve_gib"
    # ## The two modes, and why only one of them may say PASS
    #
    # **Full** is the floor: every mutant in scope is generated and tested, so a green run
    # supports the *negative* claim — there is no unaccepted survivor in this scope. That is
    # the claim the `g4` criterion buys and the only one worth quoting.
    #
    # **Iterate** (`--iterate`, owner's request 2026-08-13) skips what a previous run already
    # caught, which turns a re-run from hours into the handful of mutants that were still
    # open. It is for development stages: run it after each one, and keep the full run for CI
    # and for a review pass. What it cannot do is the negative claim, and the reason is exact:
    # **the mutants it skips are precisely the ones a deleted test would have stopped
    # catching.** A test removed between two iterate runs is invisible to the second, because
    # its mutant is on last time's caught list and is never re-tested. So an iterate run can
    # still *find* a survivor — a positive claim, and a real finding when it fires — and it
    # can never certify their absence.
    #
    # Which is why this script refuses to print `PASS` for one. AGENTS rule 3 says an
    # auto-skipping rung reports a named, counted skip and never silence, and a run that
    # tested a third of the scope while printing the word the full run prints is the plainest
    # form of "skip reads as pass" this repository has a rule against. It ends on `PARTIAL`,
    # says how many it tested, and names the claim it is not making.
    #
    # Both spellings route through one decision, and since the precondition above needs the
    # answer before anything runs, that decision is taken there: `WCH_MUTANTS_ITERATE=1` and a
    # `--iterate` in `"$@"` set the same `iterating`. What stays here is the flag itself and the
    # accounting, because a flag that reached cargo-mutants without passing this block would
    # enable the mode with none of it — one mode with two doors, one of them unwatched.
    declare -a extra=()
    if ((iterating == 1)); then
        # Added here rather than left in `"$@"` so the flag is passed exactly once however it
        # arrived; cargo-mutants takes it twice without complaint, but a script that could not
        # say which door a mode came through is the thing this block exists to prevent.
        declare -a forwarded=()
        for arg in "$@"; do
            [[ "$arg" == "--iterate" ]] || forwarded+=("$arg")
        done
        set -- ${forwarded[@]+"${forwarded[@]}"}
        extra+=(--iterate)
        printf 'mutants: ITERATE — skipping mutants a previous run already caught; this run cannot certify that no survivor exists and is NOT a gate run\n'
    fi

    printf 'mutants: running %s job(s) over the workspace suite\n' "$jobs"
    started="$(date +%s)"
    status=0
    cargo mutants -j "$jobs" "${extra[@]}" "$@" || status=$?
    elapsed=$(($(date +%s) - started))
    printf 'mutants: cargo-mutants exited %s after %sm%ss\n' \
        "$status" "$((elapsed / 60))" "$((elapsed % 60))"

    # First, because a tree that moved explains everything below it.
    tree_did_not_move

    case "$status" in
    0 | 2 | 3) ;; # every mutant caught / some survived / some timed out — all triageable
    4)
        no_verdict \
            "the baseline suite is red in an unmutated tree, so no survivor list from this run means anything" \
            "the finding is whatever \`just ci\` says about the unmutated tree; this job has nothing to add until that is green"
        ;;
    *)
        # Everything else: killed (137 is the shape N66's disk failure took), interrupted,
        # a tool error. All of them are "the run stopped", none of them is a survivor.
        no_verdict \
            "cargo-mutants could not complete (exit $status); see ${out_dir#"$root"/}" \
            "exit 137 is the shape a killed run takes — out of memory, out of disk, or somebody stopped it (note N66)"
        ;;
    esac

    # A real run always writes its own summary, so its absence means the run did not
    # finish writing — which is a stopped run rather than a clean floor. A *fixture* may
    # legitimately have no `outcomes.json`; a fixture is a directory of text files, and the
    # cross-check below is skipped when there is nothing to cross-check against.
    if [[ ! -f "$out_dir/outcomes.json" ]]; then
        no_verdict \
            "cargo-mutants exited $status and left no summary at ${out_dir#"$root"/}/outcomes.json" \
            "a run that never wrote its own census did not finish, whatever its exit code said"
    fi
fi

# ---------------------------------------------------------------- the census
#
# Everything from here down is a function of `$out_dir` and `$accepted_file`, which is what
# makes a recorded run a fixture (see "the seam" above).
#
# The four text files are what the verdict is computed from; `outcomes.json` is
# cargo-mutants' own summary of the same run. **They are cross-checked**, and a
# disagreement is a *no verdict* rather than a finding: a result set with rows missing
# cannot say whether the missing rows were caught or survived, and guessing is how a run
# that died half way through gets read as a clean floor.
result_files=(caught.txt missed.txt timeout.txt unviable.txt)
declare -a absent=()
for f in "${result_files[@]}"; do
    [[ -f "$out_dir/$f" ]] || absent+=("$f")
done
if ((${#absent[@]} > 0)); then
    no_verdict \
        "the result set at $out_dir is incomplete: no ${absent[*]}" \
        "a verdict is computed from those rows; rows that are not there can be classified as neither caught nor surviving"
fi

# `grep -c ''` rather than `wc -l`: it counts a final line that has no newline on it, which
# `wc -l` does not, and a fixture written by hand is exactly where that difference shows up.
# It exits 1 on an empty file, which is not an error here.
line_count() {
    local n=""
    n="$(grep -c '' <"$1")" || true
    printf '%s\n' "${n:-0}"
}

caught="$(line_count "$out_dir/caught.txt")"
missed="$(line_count "$out_dir/missed.txt")"
timed_out="$(line_count "$out_dir/timeout.txt")"
unviable="$(line_count "$out_dir/unviable.txt")"
generated=$((caught + missed + timed_out + unviable))

if [[ -f "$out_dir/outcomes.json" ]]; then
    if ! command -v jq >/dev/null 2>&1; then
        skip "jq is not installed, so $out_dir/outcomes.json was not cross-checked against the rows; the rows themselves are the census"
    else
        declare -a claimed=()
        mapfile -t claimed < <(
            jq -r '[.total_mutants, .caught, .missed, .timeout, .unviable] | .[] | tostring' \
                "$out_dir/outcomes.json" 2>/dev/null || true
        )
        if ((${#claimed[@]} != 5)); then
            no_verdict \
                "$out_dir/outcomes.json is not a census this script can read" \
                "a run that was killed while writing its summary leaves exactly this, and a summary that cannot be read is not evidence of anything"
        fi
        declare -a mismatch=()
        [[ "${claimed[1]}" == "$caught" ]] || mismatch+=("outcomes.json says ${claimed[1]} caught, caught.txt holds $caught row(s)")
        [[ "${claimed[2]}" == "$missed" ]] || mismatch+=("outcomes.json says ${claimed[2]} missed, missed.txt holds $missed row(s)")
        [[ "${claimed[3]}" == "$timed_out" ]] || mismatch+=("outcomes.json says ${claimed[3]} timed out, timeout.txt holds $timed_out row(s)")
        [[ "${claimed[4]}" == "$unviable" ]] || mismatch+=("outcomes.json says ${claimed[4]} unviable, unviable.txt holds $unviable row(s)")
        [[ "${claimed[0]}" == "$generated" ]] || mismatch+=("outcomes.json says ${claimed[0]} mutants in total, the four files hold $generated between them")
        if ((${#mismatch[@]} > 0)); then
            no_verdict \
                "the run's own summary and the rows it wrote disagree, so this result set is not a complete description of any run" \
                "${mismatch[@]}" \
                "a run that died part way through leaves this (note N66 died at 23 of 525); so would a cargo-mutants that has grown a sixth category, in which case the fix is in this script and not in the tree"
        fi
    fi
fi

# A run that generated nothing is a green that proves nothing. A *finding*, not a
# no-verdict: the scope resolved to files and produced no mutants from them, which is a
# floor that has been disarmed rather than a machine that ran short.
if ((generated == 0)); then
    finding "zero mutants generated over ${scope_desc:-the scope}; a floor with no mutants cannot go red"
fi

# Line numbers move when a file is edited above them, so a survivor is keyed by what it
# does and where it lives, not by where it sat on the day it was triaged. Two mutants in
# one file can then share a key — `replace > with >=` twice in one function is two
# different comparisons — so the comparison is over **multisets**: duplicates are kept on
# both sides, and two survivors sharing a key need two lines in the register. Without that,
# accepting one of a pair would silently accept the other, which is note N10's family.
strip_position() { sed 's/^\([^:]*\.rs\):[0-9]*:[0-9]*: /\1: /'; }

# One `mutants:` prefix per line of a multi-line finding.
report() {
    local line
    while IFS= read -r line; do
        printf 'mutants:   %s\n' "$line"
    done
}

# A timed-out mutant is an uncaught mutant with a worse failure mode, so it is triaged
# beside the missed ones rather than counted as a pass.
#
# `LC_ALL=C` on both sorts and on the comparison: `sort` collates by locale and `comm`
# compares bytes, and the two disagree on any line with punctuation in it — which is every
# mutant name. Observed as `comm: file 1 is not in sorted order` on the first run that
# produced survivors.
cat "$out_dir/missed.txt" "$out_dir/timeout.txt" |
    strip_position | LC_ALL=C sort >"$scratch/survivors"
sed 's/#.*//' "$accepted_file" | sed 's/[[:space:]]*$//' | grep -v '^$' |
    LC_ALL=C sort >"$scratch/accepted" || true

unexpected="$(LC_ALL=C comm -23 "$scratch/survivors" "$scratch/accepted")"
stale="$(LC_ALL=C comm -13 "$scratch/survivors" "$scratch/accepted")"

printf 'mutants: %s generated — %s caught, %s missed, %s timed out, %s unviable\n' \
    "$generated" "$caught" "$missed" "$timed_out" "$unviable"
printf 'mutants: %s survivor(s), %s recorded acceptance(s)\n' \
    "$(wc -l <"$scratch/survivors")" "$(wc -l <"$scratch/accepted")"

failures=0
if [[ -n "$unexpected" ]]; then
    failures=$((failures + $(wc -l <<<"$unexpected")))
    printf 'mutants: FAIL — %s survivor(s) with no recorded acceptance:\n' \
        "$(wc -l <<<"$unexpected")" >&2
    report <<<"$unexpected" >&2
    printf 'mutants: each is a missing test, or an entry in %s with the note that argues why no hermetic test can kill it\n' \
        "${accepted_file#"$root"/}" >&2
fi
if [[ -n "$stale" ]]; then
    failures=$((failures + $(wc -l <<<"$stale")))
    printf 'mutants: FAIL — %s recorded acceptance(s) no longer survive; the mutant became killable and the entry is now a lie:\n' \
        "$(wc -l <<<"$stale")" >&2
    report <<<"$stale" >&2
    # N68's whole lesson in one line, printed where it is needed rather than kept in a
    # document nobody opens mid-triage.
    printf 'mutants: before deleting a line, read note N60: this direction has fired wrongly before, and an "equivalent" acceptance cannot become killable without the program changing\n' >&2
fi

if ((failures > 0)); then
    printf 'mutants: FAIL — %s finding(s) over %s mutants, %s named skip(s)\n' \
        "$failures" "$generated" "$skips" >&2
    exit 1
fi

if ((${iterating:-0} == 1)); then
    # Not `PASS`, and the difference is the whole point of the mode. See the block that adds
    # `--iterate`: what this run skipped is exactly what a deleted test would have stopped
    # catching, so "nothing survived here" is a statement about the mutants that ran and not
    # about the scope. Counted, named, and spelled differently from the word the full run
    # earns.
    printf 'mutants: PARTIAL — %s mutant(s) tested, %s caught, %s accepted survivor(s), %s named skip(s)\n' \
        "$generated" "$caught" "$(wc -l <"$scratch/accepted")" "$skips"
    printf 'mutants: iterate mode found no unaccepted survivor among the mutants it ran; it did NOT re-test the ones a previous run caught, so it makes no claim about the scope. "just mutants" is the run that does.\n'
    exit 0
fi

printf 'mutants: PASS — %s mutants, %s caught, %s accepted survivor(s), %s named skip(s)\n' \
    "$generated" "$caught" "$(wc -l <"$scratch/accepted")" "$skips"
