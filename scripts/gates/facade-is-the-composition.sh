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
# `impl Facade { … }` — the verbs an embedder holds — and, inside **every** method of that block,
# for the engine modules it calls into. That set is what the facade **encapsulates**: the modules
# a caller no longer has to name because the facade names them. A module renamed, a verb added, a
# verb whose assembly grows a sixth call — each moves this population on the day it lands, which
# a list written into this script would not.
#
# **A call is a call whatever the file imported.** `crate::resolve::list(…)`, `resolve::list(…)`
# after `use crate::resolve;`, `list(…)` after `use crate::resolve::list;`, and each of those
# under an `as` rename, are one composition move spelled four ways — so the file's own imports
# are read first, through the same one home the walk below uses (`scripts/gates/rust-imports.awk`),
# and the local names they bind are what the body walk resolves calls through. Until 2026-08-21
# only the fully-spelled form was read, and the consequence was measured: adding `use
# crate::resolve;` and dropping two `crate::` prefixes took `resolve` out of the encapsulated set
# with the run printing a smaller number, no sentence and exit 0 — after which the executor could
# assemble `engine::resolve::list` by hand and pass. That is note **N271**'s
# shrink-rather-than-fail shape, closed on the sibling predicate in the same commit that left it
# open here, so an import this reader cannot take a module out of — a glob of the crate, or a
# statement it cannot reduce to a path — is a counted refusal below rather than a smaller number.
#
# **And a path into this crate is three prefixes, not one.** The repair above resolved calls
# through the names an import binds and then asked whether the statement said `crate::`, which
# left `use super::resolve;` and `use crate::resolve::{self as r};` neither resolved nor refused:
# each took the encapsulated set from seven modules to six, printed the smaller number, named no
# sentence, exited 0, and passed a hand-assembled `engine::resolve::list(…)` in the executor on
# the next run — the same measurement, one spelling along, on the day the first one was closed
# (note **N328**). `super::` is rewritten to `crate::` in `rust-imports.awk` because this file is
# a top-level module of its crate; `self::` is not, because inside module `m` it names
# `crate::m::` and rewriting it would invent a module; and the refusal below asks
# `wch_names_this_crate` rather than looking for one prefix, so a statement this reader still
# cannot reduce is the counted refusal this paragraph promises rather than a quieter number.
#
# **Every method and not only the exported ones**, because a verb's assembly is not confined to
# its own body: a private helper the verb calls is still the facade naming the module, and a
# walk that read only `pub fn` bodies would let an assembly move one line down and leave this
# population without failing — the shrink-rather-than-fail shape note **N271** measured next
# door. `Facade::context` reaches `crate::profile::kernel_release` from exactly such a helper.
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
# entitled to them because it *is* one of the two blessed compositions (§2.11). So the engine
# modules those two verbs are assembled from are written down below as policy, with the argument
# here — how many there are is the array's own business and is counted in the output rather than
# stated in a sentence, because a prose count of code is a claim something reconciles or it is
# not made (notes **N153**, **N158**). The list is checked in four directions: each name is a
# module the engine still declares (a rename that emptied this list would be the quietest way to
# turn the gate off), none of them is one the facade has *started* encapsulating, each is a reach
# the executor still makes, and the whole list is the one the executor's own doc comment writes,
# reconciled against it both ways.
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
#   1. **Structural.** The facade file and the executor crate's sources exist, the facade
#      declares `impl Facade {`, every file walked classifies into product and test halves, the
#      engine declares a module vocabulary in its `lib.rs`, and `crates/engine/Cargo.toml` still
#      declares the lib name the walk below reads for. Each is a failure, never an empty
#      population, and each of the derived populations is `gate_require_nonzero`.
#   2. **The executor crate names no encapsulated module.** Every `engine::<module>` a product
#      line under `crates/cli/src` names, whose module the facade encapsulates, is a violation
#      unless it is one of the two declared root reaches. This is the whole commission.
#      **The subject is the crate rather than one file**: the population is the directory
#      listing, because `crates/cli/src/` holds one four-hundred-line file today and the obvious
#      next refactor splits it — a bypass moved into a sibling module passed here, measured,
#      with a summary byte-identical to the unseeded tree's (note **N271**). **A path grouped in
#      braces is the same path**: an import is joined across the lines rustfmt broke it over and
#      then flattened — nested groups, `self`, `as` renames and trailing commas alike — before
#      the walk reads it, and **every spelling of an import counts**, `pub(crate) use` and
#      `extern crate … as …` beside plain `use`, because a ban names the class and not one
#      spelling of it (notes **N249**, **N269**, **N271**; rubric A17).
#   3. **The executor still names the facade.** Zero `engine::facade` reaches is red on its own
#      sentence: an executor that stopped consuming the facade satisfies claim 2 by not being
#      the facade's consumer, which is the drift stated as compliance.
#   4. **The declared lifecycles exist, are still excluded, and are still reached.** Each is a
#      module the engine declares, none of them is in the derived encapsulated set, and the
#      executor still names each one — an exemption nobody exercises is note **N164**'s L32
#      class, and `sweep` sat in this list being exactly that until 2026-08-20.
#      The same list is the sentence the executor's own doc comment writes, reconciled against
#      it both ways: the prose said six where the policy said seven, and nothing could see it.
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
#   * **It reads source text, not a syntax tree** — and the class it bans is a *reach*, not a
#     spelling of one. `use engine::{pairing, write};`, the same list broken across lines by
#     rustfmt, a nested `use engine::{pairing::in_effect, …}`, a `self` inside a group,
#     `pub(crate) use engine::{…};` and `use engine::pairing as p;` all reduce to the one form
#     the walk reads: a module `as` rename is caught because the path is *named* before it is
#     renamed. This bullet used to claim the opposite about `as p` — measurably false, and the
#     wrong residual to carry.
#     **Two shapes cannot be reduced to a path, and neither is carried as a residual.** A
#     binding of the crate — `use engine as e;`, `extern crate engine as e;`, or `self as e`
#     inside an `engine::{…}` group — leaves every `e::pairing::…` below it a reach text cannot
#     follow. A glob — `use engine::*;`, or the `engine::{*, …}` the flattener rewrites into one
#     — leaves every `pairing::in_effect(…)` below it a *bare* path that names no crate at all,
#     which text cannot follow either, and nothing in this workspace's lints forbids one. Each
#     is its own failure below with its own counted zero, because a commission satisfied by
#     blindness is the one shape a predicate here may not have. **Both spellings of the crate
#     are read**, the lib name the manifest declares (`engine`, asserted below) and the package
#     ident a §8.11 name sweep would leave behind (`webcam_handler_engine`, which does not
#     resolve in this workspace today): `webcam_handler_engine::pairing` carries
#     `engine::pairing` inside it and is caught by the walk, and the binding form of it is
#     caught by the refusal, each with its own arm so that is a property rather than an
#     accident. What remains invisible is a path assembled by a macro, or reached from a crate
#     other than these two — and the *reader* is shared with
#     `facade-stability-table-sync.sh` rather than written twice, because two copies of "read a
#     Rust import" is how the first hole survived a revision (`rust-imports.awk`, note
#     **N271**).
#   * **It says nothing about the daemon**, and that is design rather than omission: §2.1 puts
#     the daemon's per-camera actor registry deliberately *outside* the facade's consumer
#     relationship, because an actor owns its camera across calls and the facade owns none.
#     `webcam-handler-daemon` naming `engine::actor` is correct and this gate has no opinion.
#   * **It does not check that a facade verb is right**, only that the executor goes through
#     one. Byte equivalence with the executor this replaced is
#     `crates/cli/tests/facade_equivalence.rs` (a one-time criterion, docs/13 P7d), and
#     `cli-parity.sh` is what owns the answers from here on.
#   * **A facade export the CLI never reaches is a note, not a violation.** Some of this surface
#     exists for embedders — the FR's consumer holds a hotplug watch; this binary runs one verb
#     and exits — so requiring the CLI to exercise the whole surface would be requiring the CLI
#     to grow verbs nobody asked for. What the notes buy is that the gap is *visible* in the
#     output, which is where the next person deciding whether an export earns its place will
#     look. **Which exports they are is the run's own answer and is not written here**: the list
#     was written out in this sentence until 2026-08-21 and differed from the computed one by a
#     name in each direction — the sentence named an export the CLI reaches, and the run named
#     one it calls under the other spelling — which is the second hand-written home of a derived
#     list that claim 4 of this same predicate exists to keep collapsed (notes **N153**,
#     **N269**).
#   * **It cannot tell an executor verb from a helper in the same crate.** The population is
#     every product line under `crates/cli/src`, which is stricter than "the executor" and
#     deliberately so: a helper function that did the engine assembly and handed the result to a
#     verb would be exactly the bypass this exists for, and scoping the walk to the `impl`
#     blocks — or to one file — would have read past it. The price is that the two
#     composition-root reaches above need naming, which is why they are named.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

# A backtick, as a variable, so the sentences below can be built inside double quotes without
# every one of them opening a command substitution.
tick='`'

facade_rel="crates/engine/src/facade.rs"
# The walk's population is the *directory*, and `main.rs` is named only as the one file that
# carries the policy sentence below — a doc comment has a home, a bypass does not (note
# **N271**).
executor_dir_rel="crates/cli/src"
executor_rel="$executor_dir_rel/main.rs"
engine_lib_rel="crates/engine/src/lib.rs"
facade="$root/$facade_rel"
executor="$root/$executor_rel"
engine_lib="$root/$engine_lib_rel"

# --------------------------------------------------------------- the declared policy
#
# The lifecycles D18 excludes from the facade, which `webcam-handler-cli` therefore assembles
# itself. The argument is in the header; each name is asserted below to be a module the engine
# declares, asserted *not* to be one the facade encapsulates, and asserted to be one the
# executor still reaches — and the same names are asserted to be the ones the executor's own doc
# comment writes. How many there are is not written here: the run counts them, and a prose
# count of code is a claim something reconciles or it is not made (notes **N153**, **N158**).
#
# `sweep` was here until 2026-08-20 and bought nothing: the executor names
# `engine::calibrate::{SweepContext, SweepRequest}` and has never named `engine::sweep`, so the
# line excused a reach nobody made (note **N164**'s L32 class, and note **N269**).
excluded_lifecycles=(record store lifecycle session calibrate progress)

# The two markers that bound the same sentence in the executor's own module doc. The prose is
# where a reader learns *why* these names are kept, so the two copies exist on purpose — what may
# not exist is a difference between them nothing can see.
policy_prose_open="The lifecycles this file assembles itself are"
policy_prose_close="and that list is the policy"

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

# The crate name the walk below reads for. `engine` is what dependents actually write, because
# the manifest declares it — and the package ident `webcam_handler_engine` is what a §8.11 name
# sweep would leave behind, which is why the walk reads both and this checks the one it can. A
# transcription nothing reconciles is the thing this suite exists to end, so the spelling is
# asserted against the manifest rather than believed.
engine_manifest_rel="crates/engine/Cargo.toml"
if ! grep -qxF 'name = "engine"' "$root/$engine_manifest_rel"; then
    gate_fail "$engine_manifest_rel no longer declares ${tick}name = \"engine\"${tick}; the walk below reads the executor for ${tick}engine::${tick} paths on the strength of that declaration, so a renamed lib target leaves this predicate matching a spelling nothing writes and passing over every reach — repoint the walk in the same commit as the rename (§8.11: a name sweep is always its own sub-milestone)"
fi
gate_checked 1 "declarations of the engine's lib name in $engine_manifest_rel, the spelling the walk below reads for"

# The engine's own module vocabulary, so every policy name below is checked against what the
# crate declares rather than against a list this script believes. Read through `gate_pub_mods`,
# which `facade-stability-table-sync.sh` shares: both spellings of a declaration count, because
# a derivation that saw `pub mod x;` and not `pub mod x { … }` would be banning one spelling of
# the class again (notes **N249**, **N271**).
declare -A engine_modules=()
engine_module_count=0
while IFS= read -r module; do
    [[ -n "$module" ]] || continue
    engine_modules["$module"]=1
    engine_module_count=$((engine_module_count + 1))
done < <(gate_pub_mods "$engine_lib")

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
facade_imports=0
facade_unread_imports=0
facade_globs=0

# The facade's own program, function definitions and one `END` only:
# `scripts/gates/rust-imports.awk` in front of it carries the joiner, the flattener and the
# dispatch, because "which paths does this file name" is a fact three predicates here read and a
# second copy of the reader is the second home §2.10 forbids (notes **N269**, **N271**).
#
# **The `END` is what makes the bindings arrive before the calls that use them.** The body lines
# are held and walked after the last statement has been read, so an import written below a method
# still decides how that method's calls are resolved; a reader that walked in file order would
# answer differently depending on where the import sat, which is a population that moves for a
# reason nobody can see.
#
# The backticks below are markdown in awk comments and in one message, not command
# substitution: this string is an awk program and never a shell word.
# shellcheck disable=SC2016
facade_program='
    # An import of this crate is read for the local names its methods may then call the engine
    # by. A call is a call whatever the statement above it looks like: `crate::resolve::list(`,
    # `resolve::list(` after `use crate::resolve;`, `list(` after `use crate::resolve::list;`,
    # and every one of those under an `as` rename. Until 2026-08-21 only the first spelling was
    # read, so moving one import line took a module out of the encapsulated set with the run
    # printing a smaller number and no sentence at all — the shrink note **N271** closed on the
    # sibling predicate and not here (the executor could then assemble that very module by hand
    # and pass).
    #
    # The rename map is read from the statement **as written**, because the flattener strips
    # `as X` after rebuilding the path — which is what makes the module visible, and what loses
    # the name the file will actually write at the call site.
    function wch_emit_import(stmt, nr,   flat, bare, rooted, rest, path, n, parts, local, took, raw, piece, ren) {
        print "USE\t" nr "\t"
        bare = stmt
        gsub(/\t/, " ", bare)
        gsub(/^[ ]+/, "", bare)
        # `super::resolve` is `crate::resolve` here — `facade.rs` is a top-level module of its
        # crate — and the two spellings become one in `rust-imports.awk` rather than in this
        # matcher, because the sibling predicate reads the same fact out of the same file.
        rooted = wch_reroot(stmt)
        flat = wch_flatten(rooted)
        if (match(flat, /crate::([a-z_][a-z0-9_]*::)*\*/)) {
            print "GLOB\t" nr "\t" bare
            return
        }
        delete renamed
        raw = rooted
        while (match(raw, /[A-Za-z_][A-Za-z0-9_]*[ \t]+as[ \t]+[A-Za-z_][A-Za-z0-9_]*/)) {
            piece = substr(raw, RSTART, RLENGTH)
            raw = substr(raw, RSTART + RLENGTH)
            split(piece, ren, /[ \t]+as[ \t]+/)
            renamed[ren[1]] = ren[2]
        }
        # And the one rename that loop cannot see: the last segment of `crate::resolve::{self as
        # r}` is `resolve`, so a map keyed on the item name binds nothing while the file calls
        # the module `r` (note **N328**).
        wch_self_renames(rooted, renamed)
        took = 0
        rest = flat
        while (match(rest, /crate::[a-z_][a-z0-9_]*(::[A-Za-z_][A-Za-z0-9_]*)*/)) {
            path = substr(rest, RSTART, RLENGTH)
            rest = substr(rest, RSTART + RLENGTH)
            n = split(path, parts, "::")
            took++
            local = parts[n]
            if (local in renamed) local = renamed[local]
            if (n == 2) { mod_binding[local] = parts[2]; continue }
            # A type is the *surface* rather than the composition, and the two halves of this
            # file are owned by two predicates: `facade-stability-table-sync.sh` reads what a
            # signature hands an embedder, this reads what a body calls. A capitalised item is
            # therefore not a binding here, which is the same line the fully-spelled matcher
            # below has always drawn.
            if (local ~ /^[a-z_]/) fn_binding[local] = parts[2]
        }
        if (took == 0 && wch_names_this_crate(rooted)) print "UNREAD\t" nr "\t" bare
    }

    function wch_emit_runaway(nr, span) { print "RUNAWAY\t" nr "\t" span }

    function wch_emit_other(line, nr) { held++; text[held] = line }

    END { for (i = 1; i <= held; i++) walk(text[i]) }

    # Every method, not only the exported ones. The **verbs** are the `pub fn`s — that is the
    # surface an embedder holds — but what a verb *composes* is not confined to its own body: a
    # private helper the verb calls is still the facade naming the module, and a walk that read
    # only `pub fn` bodies would let an assembly move one line down and quietly leave the
    # encapsulated population (note **N271**). `Facade::context` already reaches
    # `crate::profile::kernel_release` from exactly there.
    function walk(line,   frag, rest, token, tok, p) {
        if (line == "impl Facade {") { in_impl = 1; print "IMPL\t\t"; return }
        if (in_impl && line ~ /^\}/) { in_impl = 0; return }
        if (!in_impl) return
        if (match(line, /^    (pub[[:space:]]+)?fn [a-z_][A-Za-z0-9_]*/)) {
            frag = substr(line, RSTART, RLENGTH)
            sub(/^[[:space:]]*(pub[[:space:]]+)?fn[[:space:]]+/, "", frag)
            verb = frag
            if (line ~ /^    pub fn /) print "VERB\t" verb "\t"
            in_fn = 1
        }
        if (!in_fn) return
        rest = line
        while (match(rest, /crate::[a-z_][a-z0-9_]*::[a-z_][a-z0-9_]*\(/)) {
            token = substr(rest, RSTART + 7, RLENGTH - 7)
            rest = substr(rest, RSTART + RLENGTH)
            split(token, p, "::")
            print "CALL\t" verb "\t" p[1]
        }
        rest = line
        while (match(rest, /[A-Za-z_][A-Za-z0-9_]*::[a-z_][a-z0-9_]*\(/)) {
            tok = substr(rest, RSTART, RLENGTH)
            rest = substr(rest, RSTART + RLENGTH)
            split(tok, p, "::")
            if (p[1] in mod_binding) print "CALL\t" verb "\t" mod_binding[p[1]]
        }
        rest = line
        while (match(rest, /[A-Za-z_][A-Za-z0-9_]*\(/)) {
            tok = substr(rest, RSTART, RLENGTH - 1)
            rest = substr(rest, RSTART + RLENGTH)
            if (tok in fn_binding) print "CALL\t" verb "\t" fn_binding[tok]
        }
        if (line ~ /^    \}$/) in_fn = 0
    }
'

while IFS=$'\t' read -r kind field_a field_b; do
    case "$kind" in
    IMPL) saw_impl=1 ;;
    VERB) facade_verbs+=("$field_a") ;;
    USE) facade_imports=$((facade_imports + 1)) ;;
    UNREAD)
        facade_unread_imports=$((facade_unread_imports + 1))
        gate_fail "$facade_rel:$field_a names this crate in an import — ${tick}${field_b}${tick} — and this reader took no module name out of it; the encapsulated set below is derived from these statements and from the calls they let the methods spell, so an import it cannot read is one that silently shrinks the population rather than one that fails, and a shrunken set is a shorter list of modules the executor may assemble by hand — spell the import in a form this reader resolves, or widen ${tick}scripts/gates/rust-imports.awk${tick} in the same commit"
        ;;
    GLOB)
        facade_globs=$((facade_globs + 1))
        gate_fail "$facade_rel:$field_a imports a whole vocabulary of this crate unqualified — ${tick}${field_b}${tick}; every item it brings into scope can then be called as a bare ${tick}list(…)${tick} that names neither crate nor module, so this reader would take no module out of the call and the encapsulated set would quietly get smaller — import the paths the facade actually uses, or repoint this predicate at a reader that resolves globs, in the same commit"
        ;;
    RUNAWAY)
        gate_fail "$facade_rel:$field_a opens an import whose braces are still open $field_b lines later, so this reader cannot tell where the statement ends; joining an unterminated one would swallow the rest of the module into a single logical line and read the whole file as one import, which is a worse answer than none — close the import, or raise the budget in this predicate with the reason written beside it"
        ;;
    CALL)
        encapsulates["$field_b"]=1
        if [[ -z "${encapsulated_by[$field_b]:-}" ]]; then
            encapsulated_by["$field_b"]="$field_a"
        fi
        ;;
    esac
done < <(gate_product_lines "$facade" "$facade_start" |
    awk -f "$(gate_rust_imports_awk)" -f <(printf '%s' "$facade_program"))

if ((saw_impl == 0)); then
    gate_fail "$facade_rel declares no ${tick}impl Facade {${tick} block; the facade's exports are this predicate's population and a block renamed, made generic or wrapped in a macro leaves it reading nothing — restore the declaration in $facade_rel, or repoint this predicate, and never let the rename land alone (D18: the facade is the composition, and a composition nobody can enumerate is one nothing holds to it)"
    gate_finish
fi

gate_checked "${#facade_verbs[@]}" "verbs ${tick}engine::facade${tick} exports, read from ${tick}impl Facade${tick} in $facade_rel"
gate_require_nonzero "${#facade_verbs[@]}" "facade exports"
gate_checked "${#encapsulates[@]}" "engine modules those verbs compose, and which the executor must therefore not name itself"
gate_require_nonzero "${#encapsulates[@]}" "engine modules the facade encapsulates"
gate_checked "$facade_imports" "import statements in $facade_rel's product half — every visibility, ${tick}use${tick} and ${tick}extern crate${tick} alike, joined across the lines rustfmt broke each one over and flattened before it was read for the local names the methods above call the engine by"
gate_require_nonzero "$facade_imports" "import statements in the facade"
gate_checked "$facade_unread_imports" "imports naming this crate that yielded no module name, refused rather than allowed to shrink the encapsulated set above"
gate_checked "$facade_globs" "imports of a whole vocabulary of this crate, refused for the same reason: a bare ${tick}list(…)${tick} names no module for this reader to resolve it to"

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

# The third direction on the same list: the executor's own doc comment writes it out in prose,
# because that is where a reader learns *why* those names are kept, and until 2026-08-20 the two
# copies could differ by a whole name with nothing able to see it (note **N269**).
executor_prose="$(tr '\n' ' ' <"$executor" | tr -s ' ')"
if [[ "$executor_prose" != *"$policy_prose_open"* || "$executor_prose" != *"$policy_prose_close"* ]]; then
    gate_fail "$executor_rel no longer carries the sentence bounded by '$policy_prose_open' … '$policy_prose_close'; that sentence is where the argument for what this file keeps is written, and a reworded one empties this reconciliation's population — restore the markers, or move them here in the same commit as the rewording"
else
    policy_prose="${executor_prose#*"$policy_prose_open"}"
    policy_prose="${policy_prose%%"$policy_prose_close"*}"
    declare -A prose_lifecycles=()
    prose_named=0
    while IFS= read -r name; do
        [[ -n "$name" ]] || continue
        prose_lifecycles["$name"]=1
        prose_named=$((prose_named + 1))
    done < <(printf '%s' "$policy_prose" |
        grep -oE "${tick}engine::[a-z_][a-z0-9_]*${tick}" | tr -d "$tick" | sed 's/^engine:://' | sort -u)

    for module in "${excluded_lifecycles[@]}"; do
        if [[ -z "${prose_lifecycles[$module]:-}" ]]; then
            gate_fail "the policy list at the top of this predicate excuses ${tick}engine::${module}${tick} and $executor_rel's own sentence about what it assembles does not name it; the argument a reader trusts and the list a machine checks would then differ by a name, which is how ${tick}sweep${tick} survived here unexercised — write the name into the sentence, or take it out of the list"
        fi
    done
    for module in "${!prose_lifecycles[@]}"; do
        if [[ ! " ${excluded_lifecycles[*]} " == *" $module "* ]]; then
            gate_fail "$executor_rel's sentence about the lifecycles it assembles names ${tick}engine::${module}${tick} and the policy list at the top of this predicate does not; the prose would be excusing a reach nothing checks — add the name here, or take it out of the sentence"
        fi
    done
    gate_checked "$prose_named" "lifecycle names $executor_rel's own sentence writes, reconciled against the policy list in both directions"
    gate_require_nonzero "$prose_named" "lifecycle names in the executor's policy sentence"
fi

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
# Every `engine::<module>` the executor crate's product lines name, over **every file in the
# crate** rather than over one path. `crates/cli/src/` holds exactly `main.rs` today and that is
# precisely why the one-file walk had never been caught: a bypass moved into a sibling module of
# the same crate passed with a counted summary byte-identical to the unseeded tree's, measured
# (note **N271**), and the file is four hundred lines long with the obvious next refactor being
# to split it. The population is the directory listing, so the file that arrives is walked
# without anybody remembering to add it — `gate_predicates` derives its own population the same
# way and for the same reason.
#
# Comments are stripped by `gate_product_lines` for the header's reason: the argument for what
# the CLI keeps is written in this crate's own doc comments, in the very spellings this loop
# matches on.
executor_files=()
while IFS= read -r -d '' file; do
    executor_files+=("$file")
done < <(gate_find "$root/$executor_dir_rel" -name '*.rs' | sort -z)

# No `if` around the count: `$executor_rel` is asserted to be a file at the top of this
# predicate, so the listing below it cannot be empty and a branch that can never fire is a
# branch no arm can claim. What guards the population is the counted assertion, which is where
# an emptied walk would have to show itself.
gate_checked "${#executor_files[@]}" "${tick}.rs${tick} files under $executor_dir_rel, the executor crate's own sources — the population is the directory rather than one path, so a bypass moved into a sibling module is still read"
gate_require_nonzero "${#executor_files[@]}" "executor sources"

# The executor's own program, run once per file so that `NR` is the file's own line number.
# It is *function definitions only*: `scripts/gates/rust-imports.awk` in front of it carries the
# rules — the joiner, the flattener and the dispatch — because two predicates here read "which
# paths does this file name" and a second copy of that reader is the second home §2.10 forbids
# (notes **N269**, **N271**).
executor_program='
    function scan(line, nr,   token, path) {
        while (match(line, /engine::[a-z_][a-z0-9_]*/)) {
            token = substr(line, RSTART, RLENGTH)
            line = substr(line, RSTART + RLENGTH)
            path = token
            if (substr(line, 1, 2) == "::" && match(substr(line, 3), /^[A-Za-z_][A-Za-z0-9_]*/)) {
                path = token "::" substr(line, 3, RLENGTH)
            }
            print rel "\tREACH\t" nr "\t" substr(token, 9) "\t" path
        }
    }

    # The two shapes flattening cannot normalise away, each refused rather than carried as a
    # residual. A binding of the crate — use engine as e;, extern crate engine as e;, or a
    # self as e inside an engine::{…} group — leaves every e::pairing::… below it a path
    # text cannot follow. A glob — use engine::*;, and the engine::{*, …} the flattener
    # rewrites into one — leaves every pairing::in_effect(…) below it a *bare* path, which
    # text cannot follow either. Both spellings of the crate are read: the lib name the
    # manifest declares today, and the package ident a §8.11 name sweep would leave behind.
    function wch_emit_import(stmt, nr,   flat, bare) {
        print rel "\tUSE\t" nr
        bare = stmt
        gsub(/\t/, " ", bare)
        gsub(/^[ ]+/, "", bare)
        if (match(stmt, /(^|[^A-Za-z0-9_:])(::)?(engine|webcam_handler_engine)[ \t]+as[ \t]+[A-Za-z_][A-Za-z0-9_]*/) ||
            match(stmt, /engine::\{[^{}]*self[ \t]+as[ \t]/)) {
            print rel "\tBIND\t" nr "\t" bare
        }
        flat = wch_flatten(stmt)
        if (match(flat, /(^|[^A-Za-z0-9_:])(::)?(engine|webcam_handler_engine)::\*/)) {
            print rel "\tGLOB\t" nr "\t" bare
        }
        # Scanned either way, and never instead: a refusal above says this reader cannot
        # follow what the statement opened, and the paths it *can* still follow are facts the
        # rest of the run needs — a glob beside facade::Facade must not also read as an
        # executor that stopped consuming the facade.
        scan(flat, nr)
    }

    function wch_emit_other(line, nr) { scan(line, nr) }

    function wch_emit_runaway(nr, span) { print rel "\tRUNAWAY\t" nr "\t" span }
'

reaches=0
facade_reaches=0
use_statements=0
crate_bindings=0
crate_globs=0
unreadable_files=0
declare -A noted_module=()
declare -A reached_module=()
while IFS=$'\t' read -r rel kind field_a field_b field_c; do
    case "$kind" in
    UNREADABLE)
        unreadable_files=$((unreadable_files + 1))
        gate_fail "$rel carries more than one ${tick}#[cfg(test)]${tick} marker, or one that opens something other than a ${tick}mod${tick}, so its product half cannot be told from its test half; the executor crate is this predicate's subject and a subject with no boundary is one that could hide a bypass in either half"
        continue
        ;;
    USE)
        use_statements=$((use_statements + 1))
        continue
        ;;
    BIND)
        crate_bindings=$((crate_bindings + 1))
        gate_fail "$rel:$field_a binds the engine crate itself to a local name — ${tick}${field_b}${tick}; this predicate reads source text, so from that line on every ${tick}<alias>::pairing::…${tick} is a reach it cannot follow and the whole commission would be satisfied by blindness rather than by compliance — spell the engine's module paths out, or repoint this predicate at a reader that resolves aliases, in the same commit"
        continue
        ;;
    GLOB)
        crate_globs=$((crate_globs + 1))
        gate_fail "$rel:$field_a imports the engine crate's whole vocabulary unqualified — ${tick}${field_b}${tick}; every module it brings into scope can then be reached as a bare ${tick}pairing::in_effect(…)${tick} that names no crate at all, so this reader would see no reach and the commission would be satisfied by blindness rather than by compliance — import the paths the file actually uses, or repoint this predicate at a reader that resolves globs, in the same commit"
        continue
        ;;
    RUNAWAY)
        gate_fail "$rel:$field_a opens a ${tick}use${tick} whose braces are still open $field_b lines later, so this reader cannot tell where the import ends; joining an unterminated statement would swallow the rest of the file into one logical line and report every violation at this line number, which is a worse answer than none — close the import, or raise the budget in this predicate with the reason written beside it"
        continue
        ;;
    esac

    line="$field_a"
    module="$field_b"
    path="$field_c"
    reaches=$((reaches + 1))
    reached_module["$module"]=1

    if [[ "$module" == "facade" ]]; then
        facade_reaches=$((facade_reaches + 1))
        continue
    fi

    if [[ -n "${root_reach_paths[$path]:-}" ]]; then
        root_reach_used["$path"]=1
        gate_note "$rel:$line — ${tick}${path}${tick} is the composition root's own reach, declared at the top of this predicate and excused there"
        continue
    fi

    if [[ -n "${encapsulates[$module]:-}" ]]; then
        gate_fail "$rel:$line names ${tick}${path}${tick}, and ${tick}engine::${module}${tick} is a module ${tick}Facade::${encapsulated_by[$module]}${tick} already composes; a second assembly in the executor is what makes the facade a sibling that happens to agree today rather than the code this tool ships, and D18's promise that ${tick}cli-parity.sh${tick} transitively pins the facade's answers is only true while this crate has no other way in — call the facade verb from $rel, or, if this really is a lifecycle D18 excludes, say so in this predicate's policy list and argue it in $facade_rel's module doc"
        continue
    fi

    if [[ -n "${engine_modules[$module]:-}" ]]; then
        if [[ -z "${noted_module[$module]:-}" ]]; then
            noted_module["$module"]=1
            gate_note "$rel:$line — ${tick}engine::${module}${tick} is reached directly and the facade composes none of it; allowed, and printed so the allowance is visible rather than inferred from silence"
        fi
        continue
    fi

    gate_fail "$rel:$line names ${tick}${path}${tick} and $engine_lib_rel declares no module ${tick}${module}${tick}; this predicate decides what is allowed by asking which modules the facade composes, so a reach it cannot resolve is one it cannot judge — reconcile $rel with the engine's declarations rather than letting an unreadable path pass as an allowed one"
done < <(
    for file in "${executor_files[@]}"; do
        file_rel="${file#"$root"/}"
        start="$(gate_test_region_start "$file")"
        if ((start == -1)); then
            printf '%s\tUNREADABLE\t\t\t\n' "$file_rel"
            continue
        fi
        gate_product_lines "$file" "$start" |
            awk -v rel="$file_rel" \
                -f "$(gate_rust_imports_awk)" -f <(printf '%s' "$executor_program")
    done
)

gate_checked "$unreadable_files" "executor sources whose product half this reader could not tell from their test half, refused rather than walked half-read"
gate_checked "$reaches" "${tick}engine::${tick} paths the executor crate's product lines name under $executor_dir_rel"
gate_require_nonzero "$reaches" "engine reaches in the executor"
gate_checked "$use_statements" "import statements in those lines — every visibility, ${tick}use${tick} and ${tick}extern crate${tick} alike — each joined across the lines rustfmt broke it over and flattened before the walk read it; the population claim 2's grouping sentence rests on"
gate_require_nonzero "$use_statements" "import statements in the executor"
gate_checked "$crate_bindings" "bindings of the engine crate itself to a local name, which this reader refuses rather than carries as a residual"
gate_checked "$crate_globs" "imports of the engine crate's whole vocabulary, which this reader refuses for the same reason: a bare ${tick}pairing::in_effect(…)${tick} names no crate for it to follow"

if ((facade_reaches == 0)); then
    gate_fail "$executor_dir_rel names no ${tick}engine::facade${tick} path at all, in any of its product lines; an executor that has stopped consuming the facade satisfies every other claim here by not being the facade's consumer, and D18's whole mechanism — \"the direct CLI's executor is rebuilt on it … so the facade cannot drift from what webcam-handler-cli ships\" — is a sentence about a relationship that would no longer exist"
fi
gate_checked "$facade_reaches" "${tick}engine::facade${tick} reaches, the executor's one supported way into the engine"

for path in "${!root_reach_paths[@]}"; do
    if ((root_reach_used["$path"] == 0)); then
        gate_fail "this predicate excuses ${tick}${path}${tick} as the composition root's own reach and $executor_dir_rel no longer makes it in any of its product lines; an exemption with nothing behind it is note N164's L32 class — it costs nothing to carry and covers the next reach that arrives under the same name — so delete the row at the top of this predicate in the commit that removed the reach"
    fi
done

# The same question asked of the other policy list, which until 2026-08-20 nobody asked: a
# lifecycle the executor no longer assembles is an exemption nobody exercises, and `sweep` sat
# here being exactly that (notes **N164**, **N269**).
reached_lifecycles=0
for module in "${excluded_lifecycles[@]}"; do
    [[ -n "${engine_modules[$module]:-}" ]] || continue
    if [[ -n "${reached_module[$module]:-}" ]]; then
        reached_lifecycles=$((reached_lifecycles + 1))
    else
        gate_fail "the policy list at the top of this predicate excuses the executor's reach into ${tick}engine::${module}${tick} and $executor_dir_rel makes no such reach in any of its product lines; an exemption nobody exercises is note N164's L32 class — it costs nothing to carry and would excuse the next reach that arrives under the same name — so delete the line, or, if the verb moved onto the facade, delete it and put the executor on the facade verb in the same commit"
    fi
done
gate_checked "$reached_lifecycles" "excluded lifecycles the executor still assembles itself, which is what makes each exemption about something"
gate_require_nonzero "$reached_lifecycles" "excluded lifecycles the executor still reaches"

# The reverse direction, as notes: see the header for why an unreached export is not a
# violation. The population is printed either way, so a surface growing verbs nobody consumes is
# visible to the next reader deciding whether an export earns its place.
#
# Whitespace is squeezed out of the crate's product lines before the search, because rustfmt
# breaks a long call across lines (`self\n.facade\n.profile_probed(…)`) and a reader that missed
# those would report verbs as unreached that the file calls on the next line down. Every file in
# the crate is searched, for the walk's reason: a verb the crate calls from a sibling module is
# a verb the crate calls.
executor_calls=""
for file in "${executor_files[@]}"; do
    start="$(gate_test_region_start "$file")"
    ((start == -1)) && continue
    executor_calls+="$(gate_product_lines "$file" "$start" | tr -d ' \t\n')"
done
#
# **Both call forms count as a reach**, because a verb is reached however the crate spells the
# call: `self.facade.list(…)` on a value, and `Facade::new(…)` on the type. Until 2026-08-21 only
# the first was matched, so `new` — the composition root's one construction of the facade, at
# `crates/cli/src/main.rs:84` — was permanently in the printed list of exports the CLI never
# reaches, which is the one thing this note is for a reader to trust.
unreached_verbs=()
for verb in "${facade_verbs[@]}"; do
    if [[ "$executor_calls" != *"facade.${verb}("* && "$executor_calls" != *"Facade::${verb}("* ]]; then
        unreached_verbs+=("$verb")
    fi
done
gate_checked "${#unreached_verbs[@]}" "facade exports no file under $executor_dir_rel calls — embedder-facing surface rather than a finding, per this predicate's header: ${unreached_verbs[*]:-none}"

gate_finish
