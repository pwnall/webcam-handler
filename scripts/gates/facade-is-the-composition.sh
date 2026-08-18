#!/usr/bin/env bash
#
# `webcam-handler-cli`'s executor reaches `webcam-handler-engine` through `engine::facade` and
# nothing else, for everything the facade covers (design **D18**, docs/13 P7d).
#
# **The fact is the consumer relationship.** D18 does not promote the blessed call order and
# then hope both copies stay equal — it says "it is not a new layer: it is the `InProcess`
# assembly moved into the engine, **and the direct CLI's executor is rebuilt on it** … so the
# facade cannot drift from what `webcam-handler-cli` ships, and the CLI parity gate transitively
# pins the facade's answers". Every word of that argument rests on one property: that the
# executor has no *other* way to reach the engine. The day a verb goes back to assembling
# `engine::pairing` and `engine::write` itself, the facade stops being the code this project
# ships and becomes a second implementation that happens to agree today — which is precisely
# the upgrade risk the sibling project reported (an embedder re-verifying a five-module assembly
# at every revision), inverted onto us and made invisible, because nothing about a green
# `just ci` would say so. `cli-parity.sh` would go on comparing two roots byte for byte and
# would go on being right; it just would no longer be saying anything about the facade.
#
# ## Why this is a gate and not a test
#
# The claim is about which *paths a file names*, and a path not taken is not observable from
# inside the program. A `#[test]` can assert that `Facade::list` and the executor's `list`
# answer the same document — `crates/cli/tests/facade_equivalence.rs` is the one-time criterion
# that does exactly that — and it stays green forever on an executor that computes the same
# answer its own way, which is the defect. What can see a second assembly is a reader of the
# source text, which is what this is: the shape `wire-surface-sync.sh`,
# `web-routes-are-gated.sh` and `profile-partition-is-closed.sh` use, for the same reason.
#
# ## The population is derived from the facade's own exports
#
# Nothing is transcribed. `crates/engine/src/facade.rs` is read for the `pub fn`s inside
# `impl Facade { … }` — the verbs an embedder holds — and, inside each of those bodies, for the
# engine modules the verb calls into (`crate::<module>::<function>(`). That set is what the
# facade **encapsulates**: the modules a caller no longer has to name because the facade names
# them. A module renamed, a verb added, a verb whose assembly grows a sixth call — each moves
# this population on the day it lands, which a list written into this script would not.
#
# **A call and a type are deliberately different things.** `crate::settle::MonotonicClock` is
# constructed by `Facade::photo` and is *not* counted as encapsulation: the settle clock is
# vocabulary a composition root supplies, and the two lifecycles below take one as an argument.
# What is counted is a call into a module — the composition move D18 promoted. The rule is one
# sentence and it is why the derivation survives somebody spelling a type path in full.
#
# ## What the CLI keeps, why, and why that half is declared rather than derived
#
# D18 excludes two lifecycles from the facade on purpose: "calibration and recording lifecycles
# … are stateful compositions with a store lock, a session mutex and (in the daemon) an actor's
# thread behind them; an embedder that wants them wants the daemon or the CLI, and a facade
# method that half-owned a session would be a second lifecycle home." `webcam-handler-cli` is
# entitled to them because it *is* one of the two blessed compositions (§2.11). So the seven
# engine modules those two verbs are assembled from are written down below as policy, with the
# argument here, and each one is asserted to be a module the engine still declares before
# anything is counted — a rename that emptied this list would be the quietest way to turn the
# gate off. The list is checked in the other direction too: a declared lifecycle the facade has
# *started* encapsulating is a stale policy line, not an exemption.
#
# Two further reaches are the composition root's own rather than the executor's, and each is one
# path rather than a module:
#
#   * `engine::profile::read` builds the *backend* — it reads and version-checks the corpus
#     documents `--backend fake --profile …` replays. `Facade::new` takes a
#     `Box<dyn CameraBackend>`, so constructing one is by definition the caller's job and no
#     facade method could cover it. It touches no camera.
#   * `engine::photo::WhereverTheCallerSaid` is the destination seam, and the facade's own module
#     doc names it as something "a caller still supplies": where a photograph goes "is a fact
#     about the caller's process rather than about the camera" — this root blocks on a path a
#     person typed, the daemon must not block an actor thread on `open(2)` [N51]. A facade that
#     chose would be choosing for the wrong process.
#
# Both are policy, both are asserted to still be declared in the engine, and both are red when
# **unused** as well as when abused: an exemption nobody exercises is note **N164**'s L32 class,
# which sat in a registry for six phases with nothing able to notice.
#
# ## The seven claims
#
#   1. **Structural.** The facade file and the executor file exist, the facade declares
#      `impl Facade {`, both files classify into product and test halves, and the engine
#      declares a module vocabulary in its `lib.rs`. Each is a failure, never an empty
#      population, and each of the three derived populations is `gate_require_nonzero`.
#   2. **The executor names no encapsulated module.** Every `engine::<module>` the executor's
#      product lines name, whose module the facade encapsulates, is a violation unless it is one
#      of the two declared root reaches. This is the whole commission.
#   3. **The executor still names the facade.** Zero `engine::facade` reaches is red on its own
#      sentence: an executor that stopped consuming the facade satisfies claim 2 by not being
#      the facade's consumer, which is the drift stated as compliance.
#   4. **The declared lifecycles exist and are still excluded.** Each is a module the engine
#      declares, and none of them is in the derived encapsulated set.
#   5. **The declared root reaches are still real, still needed and still used.** Each names a
#      module the facade encapsulates (or the exemption is unnecessary), carries the declaration
#      the engine is supposed to still have, and is reached by the executor.
#   6. **Nothing is silently tolerated.** A reach into an engine module that is neither
#      encapsulated nor declared policy — `engine::settle`'s clock is the one today — is
#      **allowed and printed**, once per module, because an allowance nobody can see is the
#      silent skip AGENTS rule 3 forbids.
#   7. **Prose does not count and tests do not count.** The executor's doc comments argue about
#      `engine::record` and `engine::store` by name — that argument is what makes the boundary
#      legible, and a gate that turned writing it down into a violation would push it out of the
#      file that needs it.
#
# ## What this does **not** claim
#
#   * **It reads source text, not a syntax tree.** A path assembled by a macro, aliased through
#     `use engine::pairing as p;`, or reached from a crate other than these two is invisible
#     here. The alias shape is the honest residual: `use engine::pairing;` is caught (it names
#     the module), `as p` renames it out of view. Nothing in this workspace writes that, and the
#     remedy if it appears is a claim here, not a wider regex.
#   * **It says nothing about the daemon**, and that is design rather than omission: §2.1 puts
#     the daemon's per-camera actor registry deliberately *outside* the facade's consumer
#     relationship, because an actor owns its camera across calls and the facade owns none.
#     `webcam-handler-daemon` naming `engine::actor` is correct and this gate has no opinion.
#   * **It does not check that a facade verb is right**, only that the executor goes through
#     one. Byte equivalence with the executor this replaced is
#     `crates/cli/tests/facade_equivalence.rs` (a one-time criterion, docs/13 P7d), and
#     `cli-parity.sh` is what owns the answers from here on.
#   * **A facade export the CLI never reaches is a note, not a violation.** `watch`, `open_id`
#     and `backend` exist for embedders — the FR's consumer holds a hotplug watch; this binary
#     runs one verb and exits — so requiring the CLI to exercise the whole surface would be
#     requiring the CLI to grow verbs nobody asked for. What the notes buy is that the gap is
#     *visible* in the output, which is where the next person deciding whether an export earns
#     its place will look.
#   * **It cannot tell an executor verb from a helper in the same file.** The population is the
#     file's product lines, which is stricter than "the executor" and deliberately so: a helper
#     function that did the engine assembly and handed the result to a verb would be exactly the
#     bypass this exists for, and scoping the walk to the `impl` blocks would have read past it.
#     The price is that the two composition-root reaches above need naming, which is why they
#     are named.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

# A backtick, as a variable, so the sentences below can be built inside double quotes without
# every one of them opening a command substitution.
tick='`'

facade_rel="crates/engine/src/facade.rs"
executor_rel="crates/cli/src/main.rs"
engine_lib_rel="crates/engine/src/lib.rs"
facade="$root/$facade_rel"
executor="$root/$executor_rel"
engine_lib="$root/$engine_lib_rel"

# --------------------------------------------------------------- the declared policy
#
# The lifecycles D18 excludes from the facade, which `webcam-handler-cli` therefore assembles
# itself. The argument is in the header; each name is asserted below to be a module the engine
# declares, and asserted *not* to be one the facade encapsulates.
excluded_lifecycles=(record store lifecycle session calibrate progress sweep)

# The two reaches that belong to the composition root rather than to the executor.
# Tab-separated: the module, the item, and the declaration the engine must still carry for the
# exemption to be about anything.
root_reaches=(
    "profile	read	pub fn read("
    "photo	WhereverTheCallerSaid	pub struct WhereverTheCallerSaid"
)

# --------------------------------------------------------------- the anchors

for pair in "$facade_rel:$facade" "$executor_rel:$executor" "$engine_lib_rel:$engine_lib"; do
    rel="${pair%%:*}"
    path="${pair#*:}"
    if [[ ! -f "$path" ]]; then
        gate_fail "$rel is not a file, so D18's consumer relationship — the executor reaching the engine only through ${tick}engine::facade${tick} — has no two sides to reconcile; a boundary with no answer is a finding rather than a pass, and whichever half moved must repoint this predicate in the same commit"
        gate_finish
    fi
done

# The engine's own module vocabulary, so every policy name below is checked against what the
# crate declares rather than against a list this script believes.
declare -A engine_modules=()
engine_module_count=0
while IFS= read -r module; do
    [[ -n "$module" ]] || continue
    engine_modules["$module"]=1
    engine_module_count=$((engine_module_count + 1))
done < <(sed 's://.*::' "$engine_lib" |
    grep -oE '^pub mod [a-z_][a-z0-9_]*;' | sed 's/^pub mod //; s/;$//')

if ((engine_module_count == 0)); then
    gate_fail "$engine_lib_rel declares no ${tick}pub mod${tick} this reader can see, so there is no module vocabulary to check the excluded lifecycles against and every policy name below would pass by naming nothing — restore the declarations, or repoint this predicate at wherever the engine's modules are declared now"
    gate_finish
fi
gate_checked "$engine_module_count" "modules ${tick}webcam-handler-engine${tick} declares in $engine_lib_rel, the vocabulary this predicate's policy names are checked against"

# --------------------------------------------------------------- the derived population
#
# The facade's exports, and the engine modules each one composes. Read from the product half of
# the file: the facade's own test module calls `crate::resolve::list` to prove the facade *is*
# the composition, and a reader that counted it would be counting a test.
facade_start="$(gate_test_region_start "$facade")"
if ((facade_start == -1)); then
    gate_fail "$facade_rel carries more than one ${tick}#[cfg(test)]${tick} marker, or one that opens something other than a ${tick}mod${tick}, so its product half cannot be told from its test half; a file whose boundary has no answer is one where this predicate's population has none either"
    gate_finish
fi

facade_verbs=()
declare -A encapsulates=()
declare -A encapsulated_by=()
saw_impl=0
while IFS=$'\t' read -r kind field_a field_b; do
    case "$kind" in
    IMPL) saw_impl=1 ;;
    VERB) facade_verbs+=("$field_a") ;;
    CALL)
        encapsulates["$field_b"]=1
        if [[ -z "${encapsulated_by[$field_b]:-}" ]]; then
            encapsulated_by["$field_b"]="$field_a"
        fi
        ;;
    esac
done < <(gate_product_lines "$facade" "$facade_start" | awk '
    $0 == "impl Facade {" { in_impl = 1; print "IMPL\t1"; next }
    in_impl && /^\}/ { in_impl = 0; next }
    !in_impl { next }
    match($0, /^    pub fn [a-z_][A-Za-z0-9_]*/) {
        verb = substr($0, RSTART + 11, RLENGTH - 11)
        print "VERB\t" verb
        in_fn = 1
    }
    in_fn {
        line = $0
        while (match(line, /crate::[a-z_][a-z0-9_]*::[a-z_][a-z0-9_]*\(/)) {
            token = substr(line, RSTART + 7, RLENGTH - 7)
            line = substr(line, RSTART + RLENGTH)
            split(token, parts, "::")
            print "CALL\t" verb "\t" parts[1]
        }
        if ($0 ~ /^    \}$/) { in_fn = 0 }
    }
')

if ((saw_impl == 0)); then
    gate_fail "$facade_rel declares no ${tick}impl Facade {${tick} block; the facade's exports are this predicate's population and a block renamed, made generic or wrapped in a macro leaves it reading nothing — restore the declaration in $facade_rel, or repoint this predicate, and never let the rename land alone (D18: the facade is the composition, and a composition nobody can enumerate is one nothing holds to it)"
    gate_finish
fi

gate_checked "${#facade_verbs[@]}" "verbs ${tick}engine::facade${tick} exports, read from ${tick}impl Facade${tick} in $facade_rel"
gate_require_nonzero "${#facade_verbs[@]}" "facade exports"
gate_checked "${#encapsulates[@]}" "engine modules those verbs compose, and which the executor must therefore not name itself"
gate_require_nonzero "${#encapsulates[@]}" "engine modules the facade encapsulates"

for module in "${!encapsulates[@]}"; do
    if [[ -z "${engine_modules[$module]:-}" ]]; then
        gate_fail "$facade_rel's ${tick}${encapsulated_by[$module]}${tick} calls into ${tick}crate::${module}${tick} and $engine_lib_rel declares no such module; this predicate's population is derived from those calls, so a name it cannot resolve is a population it cannot trust — reconcile $facade_rel with $engine_lib_rel rather than leaving the encapsulated set half-read"
    fi
done

# --------------------------------------------------------------- the policy, both directions

for module in "${excluded_lifecycles[@]}"; do
    if [[ -z "${engine_modules[$module]:-}" ]]; then
        gate_fail "the policy list at the top of this predicate excuses the executor's reach into ${tick}engine::${module}${tick} and $engine_lib_rel declares no such module; a policy name the engine no longer has is an exemption for nothing and the next real bypass would hide behind it — rename it here in the same commit as the module (D18's excluded lifecycles are calibration and recording, and this list is that sentence made checkable)"
        continue
    fi
    if [[ -n "${encapsulates[$module]:-}" ]]; then
        gate_fail "the policy list at the top of this predicate excuses ${tick}engine::${module}${tick} as a lifecycle D18 keeps out of the facade, and $facade_rel's ${tick}${encapsulated_by[$module]}${tick} now composes it; the facade grew a verb the CLI is still assembling by hand, which is the second copy §2.10 forbids — put the executor on the facade verb and delete this line, in the same commit"
    fi
done
gate_checked "${#excluded_lifecycles[@]}" "engine modules D18 excludes from the facade, declared as policy above and asserted against the engine's own vocabulary"

declare -A root_reach_paths=()
declare -A root_reach_used=()
for row in "${root_reaches[@]}"; do
    IFS=$'\t' read -r module item declaration <<<"$row"
    path="engine::${module}::${item}"
    root_reach_paths["$path"]=1
    root_reach_used["$path"]=0

    module_file="$root/crates/engine/src/${module}.rs"
    if [[ -z "${engine_modules[$module]:-}" || ! -f "$module_file" ]]; then
        gate_fail "this predicate excuses the composition root's reach into ${tick}${path}${tick} and there is no ${tick}crates/engine/src/${module}.rs${tick} the engine declares; an exemption naming a module that has moved is one that can no longer be checked against anything — repoint it here in the same commit as the move"
        continue
    fi
    if ! grep -qF "$declaration" "$module_file"; then
        gate_fail "this predicate excuses ${tick}${path}${tick} on the strength of ${tick}${declaration}${tick} in crates/engine/src/${module}.rs and that declaration is not there; the exemption is written against a spelling the engine no longer has, so it would go on excusing a path nobody can follow — reconcile the row at the top of this predicate with crates/engine/src/${module}.rs"
    fi
    if [[ -z "${encapsulates[$module]:-}" ]]; then
        gate_fail "this predicate excuses ${tick}${path}${tick} as a composition-root reach into ${tick}engine::${module}${tick}, and the facade no longer composes that module at all; the exemption buys nothing — the reach is already outside claim 2's population — so delete the row rather than carrying an excusal whose reason has expired (note N164: an unneeded registry line sat unnoticed for six phases)"
    fi
done
gate_checked "${#root_reaches[@]}" "reaches declared above as the composition root's own — building the backend, and choosing where a photograph goes"

# --------------------------------------------------------------- the walk
#
# Every `engine::<module>` the executor's product lines name. Comments are stripped by
# `gate_product_lines` for the header's reason: the argument for what the CLI keeps is written
# in this file's own doc comments, in the very spellings this loop matches on.
executor_start="$(gate_test_region_start "$executor")"
if ((executor_start == -1)); then
    gate_fail "$executor_rel carries more than one ${tick}#[cfg(test)]${tick} marker, or one that opens something other than a ${tick}mod${tick}, so its product half cannot be told from its test half; the executor is this predicate's subject and a subject with no boundary is one that could hide a bypass in either half"
    gate_finish
fi

reaches=0
facade_reaches=0
declare -A noted_module=()
while IFS=$'\t' read -r line module path; do
    reaches=$((reaches + 1))

    if [[ "$module" == "facade" ]]; then
        facade_reaches=$((facade_reaches + 1))
        continue
    fi

    if [[ -n "${root_reach_paths[$path]:-}" ]]; then
        root_reach_used["$path"]=1
        gate_note "$executor_rel:$line — ${tick}${path}${tick} is the composition root's own reach, declared at the top of this predicate and excused there"
        continue
    fi

    if [[ -n "${encapsulates[$module]:-}" ]]; then
        gate_fail "$executor_rel:$line names ${tick}${path}${tick}, and ${tick}engine::${module}${tick} is a module ${tick}Facade::${encapsulated_by[$module]}${tick} already composes; a second assembly in the executor is what makes the facade a sibling that happens to agree today rather than the code this tool ships, and D18's promise that ${tick}cli-parity.sh${tick} transitively pins the facade's answers is only true while this file has no other way in — call the facade verb from $executor_rel, or, if this really is a lifecycle D18 excludes, say so in this predicate's policy list and argue it in $facade_rel's module doc"
        continue
    fi

    if [[ -n "${engine_modules[$module]:-}" ]]; then
        if [[ -z "${noted_module[$module]:-}" ]]; then
            noted_module["$module"]=1
            gate_note "$executor_rel:$line — ${tick}engine::${module}${tick} is reached directly and the facade composes none of it; allowed, and printed so the allowance is visible rather than inferred from silence"
        fi
        continue
    fi

    gate_fail "$executor_rel:$line names ${tick}${path}${tick} and $engine_lib_rel declares no module ${tick}${module}${tick}; this predicate decides what is allowed by asking which modules the facade composes, so a reach it cannot resolve is one it cannot judge — reconcile $executor_rel with the engine's declarations rather than letting an unreadable path pass as an allowed one"
done < <(gate_product_lines "$executor" "$executor_start" | awk '
    {
        line = $0
        while (match(line, /engine::[a-z_][a-z0-9_]*/)) {
            token = substr(line, RSTART, RLENGTH)
            line = substr(line, RSTART + RLENGTH)
            path = token
            if (substr(line, 1, 2) == "::" && match(substr(line, 3), /^[A-Za-z_][A-Za-z0-9_]*/)) {
                path = token "::" substr(line, 3, RLENGTH)
            }
            print NR "\t" substr(token, 9) "\t" path
        }
    }
')

gate_checked "$reaches" "${tick}engine::${tick} paths the executor's product lines name in $executor_rel"
gate_require_nonzero "$reaches" "engine reaches in the executor"

if ((facade_reaches == 0)); then
    gate_fail "$executor_rel names no ${tick}engine::facade${tick} path at all; an executor that has stopped consuming the facade satisfies every other claim here by not being the facade's consumer, and D18's whole mechanism — \"the direct CLI's executor is rebuilt on it … so the facade cannot drift from what webcam-handler-cli ships\" — is a sentence about a relationship that would no longer exist"
fi
gate_checked "$facade_reaches" "${tick}engine::facade${tick} reaches, the executor's one supported way into the engine"

for path in "${!root_reach_paths[@]}"; do
    if ((root_reach_used["$path"] == 0)); then
        gate_fail "this predicate excuses ${tick}${path}${tick} as the composition root's own reach and $executor_rel no longer makes it; an exemption with nothing behind it is note N164's L32 class — it costs nothing to carry and covers the next reach that arrives under the same name — so delete the row at the top of this predicate in the commit that removed the reach"
    fi
done

# The reverse direction, as notes: see the header for why an unreached export is not a
# violation. The population is printed either way, so a surface growing verbs nobody consumes is
# visible to the next reader deciding whether an export earns its place.
#
# Whitespace is squeezed out of the executor before the search, because rustfmt breaks a long
# call across lines (`self\n.facade\n.profile_probed(…)`) and a reader that missed those would
# report verbs as unreached that the file calls on the next line down.
executor_calls="$(gate_product_lines "$executor" "$executor_start" | tr -d ' \t\n')"
unreached_verbs=()
for verb in "${facade_verbs[@]}"; do
    if [[ "$executor_calls" != *"facade.${verb}("* ]]; then
        unreached_verbs+=("$verb")
    fi
done
gate_checked "${#unreached_verbs[@]}" "facade exports $executor_rel does not call — embedder-facing surface rather than a finding, per this predicate's header: ${unreached_verbs[*]:-none}"

gate_finish
