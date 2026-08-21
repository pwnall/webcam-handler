#!/usr/bin/env bash
#
# Every product module is in the mutation floor or excluded from it on purpose
# (`.cargo/mutants.toml`'s own law; note **N162**; AGENTS.md rule 1).
#
# That file states the rule twice and in its own words: *"an exclusion that is not a decision
# with a date is an oversight wearing one."* Its exclusions are spelled as **absence from
# `examine_globs`**, deliberately, so that widening the floor is adding a line rather than
# deleting one — which means a file nobody has thought about and a file somebody argued out of
# scope look identical from the outside. Until this predicate existed the difference was a
# paragraph a reader had to trust, and its own honest residual said what would happen: the next
# module added to `crates/` would be in neither list on the day it landed. Three did —
# `imaging/stream_stats.rs`, `imaging/compare.rs` and `engine/facade.rs`, all across P8 — and
# nothing in this workspace could say so.
#
# Five claims, each checked in both directions:
#
#   1. **Every product source file is decided.** It is named by `examine_globs` (in scope), or a
#      `scope-out:` marker covers it (out of scope, with a reason a reader can find). A file in
#      neither is the one state that file forbids.
#   2. **Every `examine_globs` entry names a file that is there.** A glob that outlived its
#      module widens the floor over nothing and quietly shrinks the population the run reports
#      against.
#   3. **Every marker decides at least one file** the floor does not already cover. A marker whose
#      subject was deleted, or whose whole subject moved *into* `examine_globs`, is a reader told
#      an exclusion is still being carried when it is not — the direction `unsafe-scope.sh`'s
#      residual register is also checked in.
#   4. **No file marker names a path `examine_globs` already names.** A file in both lists is as
#      undecided as a file in neither: the two say opposite things and nothing here can rank them.
#      Directory markers are exempt from this by design, because a directory marker is a blanket
#      and the prose it indexes blankets with exceptions — `crates/daemon/` is out except for its
#      three `http` folds, `crates/backends/v4l2/` except for `hotplug.rs`.
#   5. **Every marker carries a reason.** The marker grammar is `scope-out: <path> — <reason>`,
#      and a marker with an empty reason is exactly the oversight wearing a decision's clothes
#      that this whole predicate is about.
#
# **The population is derived, not listed.** It is the `src/` tree of every workspace member
# `cargo metadata` reports, so a new crate joins the moment it is a member and a new module joins
# the moment it is written. Integration suites under `crates/*/tests/` are deliberately outside
# it: cargo compiles them only under `cargo test`, they ship in no binary, and a mutation of a
# test is a question about whether the tests test the tests — which is note **N15**'s lesson and
# the reason `crates/testkit/` is out of the floor by argument as well.
#
# **The one way a member can leave that population is by name.** A crate whose sources are not
# under `src/` contributes no files, and a derived population that can be emptied for one member
# in silence is the derivation undone — the same class one level up from the module this exists
# to catch. Every member has a `src/` today, so what is reported is a counted, named skip of
# zero: the walk says it looked (AGENTS rule 3, notes **N160**, **N231**, **N235**), and
# `pass_case_a_member_with_no_src_directory_leaves_the_population_by_name` holds both directions
# of that sentence.
#
# **The predicate reads only the path.** The reason cell is for the reader and the argument it
# indexes lives in the prose above the register in `.cargo/mutants.toml`; if this compared reasons
# there would be two homes for one law and they could disagree (design §2.10). The register is in
# that file rather than scattered through the sources it names for the same reason, and the
# argument for that choice is written where the register is.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"
scope_rel=".cargo/mutants.toml"
scope="$root/$scope_rel"

if [[ ! -f "$scope" ]]; then
    gate_fail "$scope_rel is not in the tree under test; the mutation floor has no scope, so nothing here is decided either way"
    gate_finish
fi

# ------------------------------------------------------------------ the two lists
#
# Both are parsed out of the file rather than transcribed here, per docs/9's derived-population
# rule: a predicate holding its own copy of the scope would go green on a scope it no longer
# describes.

mapfile -t globs < <(awk '
    /^examine_globs[[:space:]]*=[[:space:]]*\[/ { inside = 1; next }
    inside && /^\]/                             { inside = 0 }
    inside {
        while (match($0, /"[^"]+"/)) {
            print substr($0, RSTART + 1, RLENGTH - 2)
            $0 = substr($0, RSTART + RLENGTH)
        }
    }
' "$scope")

# The marker grammar, and claim 5 lives in the split: a line that opens `scope-out:` and does not
# reach `<path> — <reason>` is reported as a marker with no reason rather than silently skipped,
# because a marker this cannot read is the state the register exists to prevent.
markers=()
marker_lines=0
reasonless=0
while IFS= read -r line; do
    marker_lines=$((marker_lines + 1))
    if [[ "$line" =~ ^#[[:space:]]*scope-out:[[:space:]]+([^[:space:]]+)[[:space:]]+—[[:space:]]+([^[:space:]].*)$ ]]; then
        markers+=("${BASH_REMATCH[1]}")
    else
        gate_fail "$scope_rel has a \`scope-out:\` line this gate cannot read as \`scope-out: <path> — <reason>\`: ${line#\# }"
        reasonless=$((reasonless + 1))
    fi
done < <(grep -E '^#[[:space:]]*scope-out:' "$scope" || true)

gate_checked "${#globs[@]}" "\`examine_globs\` entr(ies) read out of $scope_rel"
gate_require_nonzero "${#globs[@]}" "\`examine_globs\` entries"
gate_checked "${#markers[@]}" "\`scope-out:\` marker(s) read out of $scope_rel"
gate_require_nonzero "${#markers[@]}" "\`scope-out:\` markers"
if ((reasonless > 0)); then
    gate_note "$reasonless of $marker_lines \`scope-out:\` line(s) do not carry a reason"
fi

# ------------------------------------------------------------------ the population
#
# Member manifest directories, expressed relative to the metadata's own workspace root and then
# resolved against the tree under test — `unsafe-scope.sh`'s move, and for its reason: the
# metadata may describe a different checkout than the tree the claims are about.

meta_root="$(gate_metadata | jq -r '.workspace_root')"

mapfile -t member_srcs < <(gate_metadata | jq -r '
    ( [ .workspace_members[] ] ) as $members
    | .packages[]
    | select(.id as $id | $members | index($id))
    | "\(.name)\t\(.manifest_path)"
')

# **A member whose sources are not under `src/` leaves the population by name, not by silence.**
# `gate_find` returns nothing for a directory that is not there, so the whole value of deriving
# this population — that a member joins it by being a member — would be undone quietly by one
# crate laid out with a `path = ` in its `[lib]`. Every member has a `src/` today; what this
# reports is that it looked and how many it found nothing for (AGENTS rule 3, notes **N160**,
# **N231**, **N235**).
files=()
srcless=0
srcless_names=""
for member in "${member_srcs[@]}"; do
    name="${member%%$'\t'*}"
    dir="$(dirname "${member#*$'\t'}")"
    rel="${dir#"$meta_root"/}"
    [[ "$rel" == "$meta_root" ]] && rel=""
    src="$root${rel:+/$rel}/src"
    if [[ ! -d "$src" ]]; then
        srcless=$((srcless + 1))
        srcless_names="${srcless_names:+$srcless_names, }$name (${rel:-.})"
        continue
    fi
    while IFS= read -r -d '' file; do
        files+=("${file#"$root"/}")
    done < <(gate_find "$src" -name '*.rs')
done

gate_checked "${#files[@]}" "product source file(s) walked for a decision about the mutation floor"
gate_require_nonzero "${#files[@]}" "product source files"
if ((srcless > 0)); then
    gate_skip "$srcless" "workspace member(s) with no \`src/\` directory, whose modules this walk therefore never offered to the register: $srcless_names"
fi

# ------------------------------------------------------------------ claim 1: every file decided

# Membership sets, built once. Bash associative arrays rather than a grep per file: the population
# is a hundred and twenty files against fifty-odd list entries, and the quadratic spelling is the
# one somebody quietly narrows later.
declare -A in_scope=()
for glob in "${globs[@]}"; do
    in_scope["$glob"]=1
done

# Which marker covers a path, if one does. A marker ending in `/` is a directory blanket and
# covers everything under it; anything else names one file.
covering_marker() {
    local path="$1" marker
    for marker in "${markers[@]}"; do
        case "$marker" in
        */) [[ "$path" == "$marker"* ]] && {
            printf '%s\n' "$marker"
            return 0
        } ;;
        *) [[ "$path" == "$marker" ]] && {
            printf '%s\n' "$marker"
            return 0
        } ;;
        esac
    done
    return 1
}

declare -A marker_decides=()
undecided=0
in_floor=0
excluded=0
for file in "${files[@]}"; do
    if [[ -n "${in_scope[$file]:-}" ]]; then
        in_floor=$((in_floor + 1))
        continue
    fi
    if marker="$(covering_marker "$file")"; then
        marker_decides["$marker"]=$((${marker_decides[$marker]:-0} + 1))
        excluded=$((excluded + 1))
        continue
    fi
    gate_fail "$file is in neither \`examine_globs\` nor a \`scope-out:\` marker in $scope_rel; an exclusion that is not a decision with a date is an oversight wearing one"
    undecided=$((undecided + 1))
done

# Every term counted while walking rather than subtracted afterwards: `${#globs[@]}` is the number
# of *entries* and claim 2 below is what says they are files, so a difference would have made this
# line quietly wrong on exactly the tree claim 2 is red about.
gate_note "${#files[@]} product file(s): $in_floor in the floor's scope, $excluded excluded by a marker, $undecided undecided"

# ------------------------------------------------------------------ claim 2: no glob outlives its file

declare -A is_product=()
for file in "${files[@]}"; do
    is_product["$file"]=1
done

for glob in "${globs[@]}"; do
    if [[ -z "${is_product[$glob]:-}" ]]; then
        gate_fail "\`examine_globs\` names $glob, which is not a product source file in this tree; a scope entry that outlived its module widens the floor over nothing"
    fi
done

# ------------------------------------------------------------------ claim 3: no marker decides nothing
#
# A file marker that `examine_globs` also names decides nothing either, and claim 4 is the sentence
# for it: reporting both would print two findings about one line and leave a reader to work out
# which is the cause and which the consequence, which is note **N242**'s cost in one gate.

for marker in "${markers[@]}"; do
    if [[ "$marker" != */ && -n "${in_scope[$marker]:-}" ]]; then
        continue
    fi
    if ((${marker_decides[$marker]:-0} == 0)); then
        gate_fail "the \`scope-out:\` marker for $marker excludes no product file this tree has that \`examine_globs\` does not already cover; a marker that outlived its subject tells a reader an exclusion is still being carried"
    fi
done

# ------------------------------------------------------------------ claim 4: nothing is in both lists

for marker in "${markers[@]}"; do
    case "$marker" in
    */) continue ;;
    esac
    if [[ -n "${in_scope[$marker]:-}" ]]; then
        gate_fail "$marker is named by \`examine_globs\` and by a \`scope-out:\` marker; a file in both lists is as undecided as a file in neither"
    fi
done

gate_finish
