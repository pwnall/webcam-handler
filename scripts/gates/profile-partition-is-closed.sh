#!/usr/bin/env bash
#
# The partitions D15 closes **by destructuring** are still destructured, so the compiler is
# still the thing that asks which side a new field belongs to.
#
# **The fact is the pattern.** docs/12's D15 says it in one sentence — `ProfileInvariant`'s
# partition "is stated by destructuring the struct into named sides … so a field added later
# fails to compile until somebody assigns it a side — closed in both directions by
# construction, which is the only mechanical defence this design trusts for a partition". Four
# functions in `webcam-handler-schema` are written that way, and each says so in a comment
# above its own pattern. The mechanism is real and it is also *invisible*: nothing about
# `let Self { formats, controls, measured_pairs } = self;` announces that the binding list is
# load-bearing rather than a spelling, and every reviewer who has ever simplified a pattern
# into `self.formats` has done it to code that compiled perfectly afterwards. A `..` is the
# same move wearing the pattern's own clothes — the destructuring is still there, it still
# reads as closed, and the next field is absorbed in silence.
#
# So this predicate does not check the partition. It checks the **mechanism that checks the
# partition**, which is the only part of D15 a person can delete without anything going red.
#
# ## Why this is a gate and not a test
#
# The defect is the *absence* of a compile error, and a compile error that no longer happens
# is not observable from inside the program it would have stopped. A `#[test]` can assert that
# `compare` gives the right answer for the fields that exist today — the schema suite does —
# and it will go on passing at full green the day a fifth field is added and quietly assigned
# no side, because the field it does not know about is by definition one no assertion mentions.
# A compile-fail fixture (docs/15 commissions one for P7c) proves the mechanism works *for one
# seeded field*; it says nothing about the day somebody reaches for `..` to make a warning go
# away. What can see that is a reader of the source text, which is what this is — the same shape
# `wire-surface-sync.sh` and `web-routes-are-gated.sh` use, and for the same reason: the claim is
# about what a file *says*, not about what it computes.
#
# ## The population is declared policy, and every member is asserted before anything is counted
#
# There is no mark on a struct that says "my partition is closed by destructuring", so which
# four rows these are cannot be derived — it is a decision, and it is written down once, below,
# in `declared_rows`. What *is* derived is the part that matters: the field names come out of
# each struct's own `pub struct … { … }` declaration, so the population this predicate walks
# grows the moment somebody adds a field, which is exactly the event it exists for. A list of
# field names transcribed into this script would go stale in the same commit as the defect and
# pass over it.
#
# Every declared name — the file, the struct, the `impl` block, the function — is asserted to
# exist before a single field is read, because a rename that emptied the population would
# otherwise be the quietest possible way to turn this gate off (docs/9's rule; `lib.sh`'s
# `GATE_HARNESS_FILES` is the same move for the harness).
#
# The function is named as `Impl::function` rather than as a bare name, and that is forced
# rather than decorative: `profile.rs` declares **two** functions called `sections`, one on
# each side of the same partition, and `camera.rs` declares two called `differing_fields`. A
# bare name would be an ambiguity this predicate would have to guess past. The `impl` a function
# lives in is also not the struct being destructured — `DeviceProfile::compare` destructures
# `ProfileInvariant` — so the two are separate columns rather than one inferred from the other.
# Both of `camera.rs`'s `differing_fields` are rows now. Until 2026-08-21 only `CameraInfo`'s
# was, and this header named the ambiguity while the row list resolved it in one direction: the
# neighbour it delegates the identity half to — the comparator behind
# `Error::FingerprintMismatch` and `lifecycle::belongs_to` — read its five fields through
# `self.` and could not be given a row at all, because claim 3 fails a function carrying zero
# patterns. A field added to `CameraFingerprint` was absorbed by the compiler, by all 1688
# tests and by this predicate at once, and two fingerprints differing only in it answered
# `matches = true` (note **N323**).
#
# ## The five claims
#
#   1. **Structural.** Each declared file exists, declares its struct as `pub struct <Name> {`
#      with at least one field, carries the declared `impl <Type> {` block, and that block
#      declares the named function. Each is a failure and never an empty population.
#   2. **Every field is assigned a side.** Every field name in the struct's declaration appears
#      as a binding in every `let <head> { … }` pattern inside the function. A field bound to
#      `_` **counts as assigned**: `CameraInfo::differing_fields` binds `id: _` and says in its
#      own comment that it does so "precisely so the two exclusions are visible in the code
#      instead of being inferred from what is missing from it", and a gate that read a deliberate
#      exclusion as a miss would be telling the author to hide it.
#   3. **The destructuring is there, on every side the partition has.** Each row declares how
#      many `let <head> { … }` patterns the function must carry, because `DeviceProfile::compare`
#      binds `ProfileInvariant` **twice** — once per profile — and a "simplification" that left
#      one side destructured and read the other through `other.invariant.formats` would close
#      half a partition while looking closed. Fewer patterns than the row declares is red.
#   4. **No pattern carries `..`.** A rest pattern in one of these is the defect in its purest
#      form: the destructuring survives, the reviewer's eye is satisfied, and the compiler has
#      stopped asking. It is red on sight rather than only when a field goes missing, because by
#      the time a field goes missing under a `..` nothing anywhere will say so.
#   5. **Nothing here may examine zero.** The rows, the fields walked and the patterns walked are
#      each `gate_require_nonzero`.
#
# ## Why test code needs no separate ruling here
#
# The five other predicates that read source text call `gate_test_region_start` to keep a
# module's own tests out of their population. This one does not need to and deliberately does
# not become that helper's seventh caller: every anchor it matches is at **column zero** —
# `^pub struct X {`, `^impl Y {` — and this workspace writes its tests inside an indented
# `#[cfg(test)] mod tests { … }`, so a test module's text is unreachable from these anchors by
# construction rather than by exclusion. A test that wrote an `impl DeviceDifference` of its own
# at column zero inside a `mod` block is not a shape rustfmt produces here.
#
# ## What this does **not** claim
#
#   * **It reads source text, not a syntax tree.** There is no Rust parser behind it: it finds a
#     struct by its declaration line, a function by its `fn` line inside a named `impl`, and a
#     pattern by the literal `let <head> {`. Anything that changes those spellings without
#     changing their meaning — a generic parameter on the `impl` line, a `pub(crate) struct`, a
#     macro that generates the function — reads to this predicate as an anchor that is gone,
#     which is a **failure** and not a pass. That direction is the deliberate one.
#   * **It cannot see a field bound inside a nested pattern or produced by a macro.** A field
#     assigned its side by `info: CameraInfo { .. }` one level down, or by a `let` this file
#     never sees because a macro wrote it, is a miss this reports. The remedy is to bind the
#     field at the top level of the pattern — which is what all four functions do today, and
#     what makes the mechanism legible to a human reader as well.
#   * **It does not check that the sides are *correct*.** Whether `formats` belongs to the
#     description half and `info` to the identity half is D15's judgement and this predicate has
#     no opinion about it: moving a field from one arm to the other, or assigning it to a side
#     that makes no sense, stays green here. All it asks is that somebody was **made** to decide.
#     `crates/schema`'s own suite and the corpus mutual-negative walk are what check the answer.
#   * **The `..` claim is about these four patterns only.** A rest pattern anywhere else in the
#     workspace is ordinary Rust and nothing here looks at it.
#   * **And the compiler remains the primary mechanism.** This gate protects the mechanism, not
#     the partition. On a tree where all four patterns are intact it proves nothing that the
#     build was not already proving; its whole value is on the tree where one of them has been
#     simplified away, which is the tree where the build has stopped proving anything at all.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

# A backtick, as a variable, so the sentences below can be built inside double quotes without
# every one of them opening a command substitution.
tick='`'

# --------------------------------------------------------------- the declared population
#
# Tab-separated, one row per partition D15 closes by destructuring:
#
#   file            the path, relative to the root, holding both the struct and the function
#   struct          the struct whose fields are the partition; its declaration is the population
#   impl            the `impl` block the function lives in — not always the struct itself
#   function        the function whose patterns assign the sides
#   head            the pattern head the function writes: the struct's name, or `Self`
#   patterns        how many `let <head> { … }` patterns the function must carry
declared_rows=(
    "crates/schema/src/profile.rs	ProfileInvariant	DeviceProfile	compare	ProfileInvariant	2"
    "crates/schema/src/camera.rs	CameraInfo	CameraInfo	differing_fields	CameraInfo	1"
    "crates/schema/src/camera.rs	CameraFingerprint	CameraFingerprint	differing_fields	CameraFingerprint	1"
    "crates/schema/src/profile.rs	DeviceDifference	DeviceDifference	sections	Self	1"
    "crates/schema/src/profile.rs	InvariantDifference	InvariantDifference	sections	Self	1"
)

gate_checked "${#declared_rows[@]}" "partitions docs/12's D15 closes by destructuring, declared as policy above"
gate_require_nonzero "${#declared_rows[@]}" "declared partitions"

# --------------------------------------------------------------- the two readers

# Print `DECL\t<0|1>` and one `FIELD\t<name>` line per field of `pub struct $2 { … }` in $1.
#
# Fields are taken at the declaration's own brace depth, so a nested type's members could not be
# mistaken for fields; doc comments, `//` comments and attributes (`#[serde(default)]`, which
# `ProfileInvariant::measured_pairs` carries) are skipped. Visibility is not required of a field:
# `InvariantDifference`'s two fields are private, which is the point of that type.
struct_fields() {
    awk -v name="$2" '
        $0 == "pub struct " name " {" { inside = 1; found = 1; next }
        inside && /^\}/ { inside = 0; next }
        !inside { next }
        /^[[:space:]]*(\/\/|#\[)/ { next }
        match($0, /^[[:space:]]*(pub[[:space:]]+)?[A-Za-z_][A-Za-z0-9_]*[[:space:]]*:/) {
            field = substr($0, RSTART, RLENGTH)
            sub(/^[[:space:]]*/, "", field)
            sub(/^pub[[:space:]]+/, "", field)
            sub(/[[:space:]]*:$/, "", field)
            print "FIELD\t" field
        }
        END { print "DECL\t" (found ? 1 : 0) }
    ' "$1"
}

# Print what $1's `impl $2 { … fn $3 … }` binds: `IMPL\t<0|1>`, `FN\t<0|1>`,
# `PATTERNS\t<n>`, one `BIND\t<pattern>\t<field>` per binding and `REST\t<pattern>` per `..`.
#
# The function body runs from its `fn` line to the first line closing at four spaces, which is
# what rustfmt writes for a member of a column-zero `impl` and what `just ci`'s `fmt-check` keeps
# true. Brace counting would be the obvious alternative and is the worse one here: these two
# functions build their answers with `format!("nodes[{index}].kind")`, so the body text carries
# braces inside string literals that no line-oriented reader can tell from block delimiters.
# Inside the pattern itself there are no strings, so the bindings *are* walked by brace depth.
function_bindings() {
    awk -v impl_name="$2" -v fn_name="$3" -v head="$4" '
        function emit(elem, idx,   name) {
            gsub(/[ \t\n]+/, " ", elem)
            sub(/^ +/, "", elem)
            sub(/ +$/, "", elem)
            if (elem == "") { return }
            if (elem == "..") { print "REST\t" idx; return }
            name = elem
            if (index(name, ":") > 0) { name = substr(name, 1, index(name, ":") - 1) }
            sub(/^(ref )?(mut )?/, "", name)
            sub(/ +$/, "", name)
            print "BIND\t" idx "\t" name
        }
        function split_pattern(content, idx,   i, ch, depth, cur) {
            depth = 0
            cur = ""
            for (i = 1; i <= length(content); i++) {
                ch = substr(content, i, 1)
                if (ch == "{" || ch == "[" || ch == "(") { depth++ }
                if (ch == "}" || ch == "]" || ch == ")") { depth-- }
                if (ch == "," && depth == 0) { emit(cur, idx); cur = ""; continue }
                cur = cur ch
            }
            emit(cur, idx)
        }
        function scan(s,   re, pos, brace, i, ch, depth, content, count) {
            re = "(^|[^A-Za-z0-9_])let[ \t\n]+" head "[ \t\n]*\\{"
            count = 0
            pos = 1
            while (pos <= length(s) && match(substr(s, pos), re)) {
                brace = pos + RSTART + RLENGTH - 2
                depth = 0
                content = ""
                for (i = brace; i <= length(s); i++) {
                    ch = substr(s, i, 1)
                    if (ch == "{" || ch == "[" || ch == "(") { depth++ }
                    else if (ch == "}" || ch == "]" || ch == ")") {
                        depth--
                        if (depth == 0) { break }
                    }
                    if (i > brace) { content = content ch }
                }
                count++
                split_pattern(content, count)
                pos = i + 1
            }
            print "PATTERNS\t" count
        }
        !in_impl && $0 == "impl " impl_name " {" { in_impl = 1; saw_impl = 1; next }
        in_impl && in_fn {
            body = body $0 "\n"
            if ($0 ~ /^    \}/) { in_fn = 0 }
            next
        }
        in_impl && /^\}/ { in_impl = 0; next }
        in_impl && $0 ~ ("^[[:space:]]*(pub([(][^)]*[)])?[[:space:]]+)?fn[[:space:]]+" fn_name "[[:space:]]*[(<]") {
            in_fn = 1
            saw_fn = 1
            body = body $0 "\n"
            next
        }
        END {
            print "IMPL\t" (saw_impl ? 1 : 0)
            print "FN\t" (saw_fn ? 1 : 0)
            scan(body)
        }
    ' "$1"
}

# --------------------------------------------------------------- the walk

fields_walked=0
patterns_walked=0

for row in "${declared_rows[@]}"; do
    IFS=$'\t' read -r rel_path struct_name impl_name fn_name head min_patterns <<<"$row"
    file="$root/$rel_path"
    where="$impl_name::$fn_name"

    if [[ ! -f "$file" ]]; then
        gate_fail "$rel_path is not a file, so the partition ${tick}${struct_name}${tick} states by destructuring in ${tick}${where}${tick} cannot be read at all; a declared row that has moved must be repointed here in the same commit, because an emptied population is how this predicate would stop watching D15's only mechanical defence without anybody noticing"
        continue
    fi

    fields=()
    declared=0
    while IFS=$'\t' read -r kind value; do
        case "$kind" in
        DECL) declared="$value" ;;
        FIELD) fields+=("$value") ;;
        esac
    done < <(struct_fields "$file" "$struct_name")

    if ((declared == 0)); then
        gate_fail "$rel_path declares no ${tick}pub struct ${struct_name} {${tick}; the field list of that declaration is this predicate's derived population, so a struct renamed, moved or respelled leaves ${tick}${where}${tick}'s partition unwatched — restore the name in $rel_path, or repoint this predicate's declared row, and never let the rename land alone"
        continue
    fi
    if ((${#fields[@]} == 0)); then
        gate_fail "$rel_path declares ${tick}pub struct ${struct_name} {${tick} with no fields this reader can see; a partition over an empty struct is one every pattern satisfies, so this reports it rather than passing over it — if the fields moved to a shape this predicate cannot read, $rel_path and this predicate are the two files to reconcile"
        continue
    fi

    gate_checked "${#fields[@]}" "fields of ${tick}${struct_name}${tick} whose side ${tick}${where}${tick} must assign, read from its declaration in $rel_path"
    fields_walked=$((fields_walked + ${#fields[@]}))

    saw_impl=0
    saw_fn=0
    patterns=0
    unset bound rest_at
    declare -A bound=()
    declare -A rest_at=()
    while IFS=$'\t' read -r kind value field; do
        case "$kind" in
        IMPL) saw_impl="$value" ;;
        FN) saw_fn="$value" ;;
        PATTERNS) patterns="$value" ;;
        BIND) bound["$value:$field"]=1 ;;
        REST) rest_at["$value"]=1 ;;
        esac
    done < <(function_bindings "$file" "$impl_name" "$fn_name" "$head")

    if ((saw_impl == 0)); then
        gate_fail "$rel_path has no ${tick}impl ${impl_name} {${tick} block to find ${tick}${fn_name}${tick} in; that block is where ${tick}${struct_name}${tick}'s partition is assigned its sides, and a predicate that cannot find it is one that would go green over a partition nobody is closing — restore the block in $rel_path or repoint this predicate's declared row"
        continue
    fi
    if ((saw_fn == 0)); then
        gate_fail "$rel_path's ${tick}impl ${impl_name}${tick} declares no ${tick}fn ${fn_name}${tick}; the function is where ${tick}${struct_name}${tick}'s fields are assigned their sides (docs/12 D15), so a rename that left this row behind takes the watch off the mechanism rather than the mechanism off the code — rename it here too, in the same commit"
        continue
    fi

    if ((patterns < min_patterns)); then
        gate_fail "$rel_path's ${tick}${where}${tick} binds ${tick}${head}${tick} in $patterns ${tick}let${tick} pattern(s) and D15's partition over ${tick}${struct_name}${tick} needs $min_patterns; a destructuring simplified into field access compiles perfectly and closes nothing, so the compiler stops asking which side a new field belongs to and no test can notice (docs/12 D15: \"a field added later fails to compile until somebody assigns it a side\") — restore the pattern in $rel_path"
        continue
    fi
    patterns_walked=$((patterns_walked + patterns))

    for ((index = 1; index <= patterns; index++)); do
        # The ordinal, so that two sides missing the same field read as two findings rather than as
        # one printed twice — `DeviceProfile::compare` binds the same struct on both profiles, and
        # a reader who cannot tell those lines apart has to go and count the patterns by hand.
        which="pattern $index of $patterns"
        if [[ -n "${rest_at[$index]:-}" ]]; then
            gate_fail "$rel_path's ${tick}${where}${tick} binds ${tick}${head}${tick} ($which) with a ${tick}..${tick} rest pattern; the destructuring survives and the compiler has stopped asking, which is D15's defence removed while still looking present — name every field of ${tick}${struct_name}${tick} in the pattern in $rel_path, binding to ${tick}_${tick} the ones this function deliberately ignores"
            continue
        fi
        for field in "${fields[@]}"; do
            if [[ -z "${bound[$index:$field]:-}" ]]; then
                gate_fail "$rel_path's ${tick}${where}${tick} binds ${tick}${head}${tick} ($which) without naming its field ${tick}${field}${tick}; a field no pattern names is a side nobody assigned, and the partition docs/12's D15 says is \"closed in both directions by construction\" is open at that field — name it in the pattern in $rel_path, or bind it to ${tick}${field}: _${tick} where the exclusion is deliberate the way ${tick}CameraInfo${tick} excludes ${tick}id${tick}"
            fi
        done
    done
done

gate_checked "$fields_walked" "struct fields walked against the patterns that must assign each one a side"
gate_require_nonzero "$fields_walked" "struct fields"
gate_checked "$patterns_walked" "destructuring patterns walked across the declared functions"
gate_require_nonzero "$patterns_walked" "destructuring patterns"

gate_finish
