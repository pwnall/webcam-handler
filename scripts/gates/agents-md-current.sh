#!/usr/bin/env bash
#
# The `AGENTS.md` at the repository root is the doc it says it is a copy of.
#
# Doc 10 of the webcam-handler series *is* the agent instructions, and its own opening
# paragraph declares the deployment: "Deploy at the repository root as `AGENTS.md`; the
# deployed copy tracks this file (one-directional; when they drift, reconcile deliberately
# and record which side was wrong)."
#
# Read that parenthesis again, because it is the whole reason this file exists. The
# sentence does not say the two cannot drift — it says **what to do when they do**. A
# reconciliation procedure written into a document is a project telling you, in its own
# words, that it expects this defect. AGENTS rule 1 is that "every anticipated or
# discovered defect class becomes a lint, a CI job, or a test that can go red", and until
# this predicate landed the most explicitly anticipated defect class in the repository was
# the one thing rule 1 did not cover: `grep -rn 'docs/10' scripts/` returned nothing, no
# gate compared them, no selftest case existed, and no recipe knew they were a pair.
#
# The discipline has held so far, which is the argument *for* the gate rather than against
# it: ten commits have moved this file since the v2 series was issued at `f6bc5d9`, and
# every one of the ten moved both copies in the same commit. Ten for ten by hand is a good
# record and it is also exactly the shape of a rule nobody has needed yet — the eleventh
# commit is the one that edits the root copy because that is the path an agent has open,
# or edits the doc because that is the path the docs series lives on, and ships. The cost
# of that is not cosmetic: the deployed copy is what every agent working in this tree
# reads, and the doc is what a review reads, so a divergence means two different sets of
# non-negotiable rules are in force at once and neither reader can tell.
#
# ## Byte-identical, and no allowance
#
# The comparison is `cmp` over the whole file, with no tolerance for a header, a banner, a
# generated preamble or a trailing marker — and that is a decision, taken by reading both
# openings rather than by assuming.
#
# The source doc's first line is `# AGENTS.md — webcam-handler (v2)`. It is *written in the
# deployed copy's voice*: it names itself by the deployed filename, addresses an agent
# working in the tree, and its second sentence is the deploy instruction. There is nothing
# in it that a root copy would need stripped and nothing a root copy would need added. At
# `f61b2ae` the two files are 15115 bytes each with the same digest, so byte-identity is
# not a bar this repository has to climb to — it is where it already is, and a gate should
# assert the strongest true thing.
#
# The alternative was an allowance — "ignore a leading front-matter block", say — and it
# was rejected because an allowance is a hole with a nice name. The moment the predicate
# tolerates one line it cannot see, a paragraph fits through the same door, and the failure
# mode is the one this gate exists to prevent wearing the gate's own approval. If a real
# divergence ever becomes necessary, it lands *here*, as a named exception with an argument
# and its own selftest arm, decided once — not as a tolerance nobody remembers granting.
#
# ## Nothing here is transcribed
#
# docs/9's second structural rule is that populations are derived, and this predicate names
# neither file. The source is found by asking which `docs/*.md` **says it deploys**, and
# the deployed filename is read out of that same sentence — the trick
# `schema-artifacts-current.sh` uses when it reads `ARTIFACT_DIR` out of xtask's source
# rather than repeating it. So a v3 reissue, a renumbering of the series, or a decision to
# deploy under some other name follows the document instead of requiring this script to be
# edited, and `cases/agents-md-current.cases.sh` proves that with a green arm that renames
# the source doc.
#
# `docs/historical/` is deliberately not scanned. `docs/historical/5-claude-fable-agents-v1.md`
# is v1 of this very file and still carries its own deploy sentence, so a recursive walk
# would find two sources and this gate would be red on the shipped tree for a reason that
# is not a defect. A superseded document's instruction is not an instruction — AGENTS'
# "Read before changing anything" is explicit that docs/1-5 live under `historical/` and
# that v2 supersedes them — so the population is `docs/*.md`, one level, and the exclusion
# is a statement rather than an accident of globbing.
#
# ## The direction is part of the finding
#
# "These two files differ" is not what the rule says. The rule is one-directional: the doc
# is the source and the root copy tracks it, so a failure message that treats them
# symmetrically invites the wrong repair — copying the root file over the doc, which is
# how a drift becomes the new truth. Every message below names which side is which and
# quotes the doc's own instruction to reconcile deliberately and record which side was
# wrong.
#
# ## The same defect class, one file along: the redirect (owner ruling, 2026-08-14)
#
# The house practice is the standardized, tool-neutral `AGENTS.md` plus a `CLAUDE.md` at the
# repository root holding `@AGENTS.md` and nothing else, so that a Claude-specific reader
# looking for the name it knows is pointed at the one home the rules have. It arrived here as
# an owner ruling and it is recorded as note **N102**; the doc's preamble is where it is
# declared, in the sentence this file reads it out of.
#
# A ruling that puts a second file at the root creates a second defect class with the same
# shape as the first. `AGENTS.md` can drift from doc 10; `CLAUDE.md` can stop being a pointer
# and grow content — a paragraph somebody thought was worth saying to a Claude reader in
# particular, a "quick summary" above the reference, a stale rule nobody deleted. The harm is
# the one this file's opening already describes, arriving through a different door: two sets
# of non-negotiable rules in force at once, and neither reader able to tell which one it is
# holding. AGENTS rule 1 has no exemption for a defect class that is anticipated by a ruling
# rather than by a parenthesis, so the claim lands here, beside the claim it rhymes with.
#
# ### "Nothing else" is exactly the reference and one trailing newline
#
# The file the ruling produced is 11 bytes: `@AGENTS.md` and a newline. That is what is
# asserted — byte for byte, no tolerance for a leading blank line, a trailing comment, a
# second line of any kind, or the *absence* of the newline.
#
# The strictness is the byte-identity argument above, made about a smaller file: this is not
# a bar the repository has to climb to, it is where the ruling already put it, and a gate
# asserts the strongest true thing. The tempting allowance is "let it carry a comment
# explaining the redirect", and it is the same hole with a nicer name — a file that may carry
# one explanatory line may carry a paragraph that contradicts `AGENTS.md`, and the reader who
# needs to be told which of the two is authoritative is exactly the reader who cannot be.
#
# A missing trailing newline is red as well, and that one is worth arguing rather than
# asserting: nothing is *harmed* by a file whose last byte is `d`. What is harmed is the
# claim's shape. "The redirect is these bytes" has no seam in it; "the redirect is these
# bytes, or those bytes, and here is the rule for which" is a predicate that admits a set,
# and a set with two members is a set somebody extends. Every tool that writes this file
# writes the newline, so the cost of the strict form is nothing and the cost of the tolerant
# one is the argument about where it stops.
#
# ### A link is refused, and refused before the bytes are read
#
# `CLAUDE.md` as a link to `AGENTS.md` is the same-file trap one file along: what a reader
# following it sees is the rules themselves, so "the redirect resolves to the rules" becomes
# true by construction and stops being a claim about content at all — the unfalsifiability
# the check above this one exists for. It is also, in the ruling's own terms, a second copy
# rather than a pointer: a tool that reads `CLAUDE.md` without following links gets a path
# instead of a reference, and one that follows links gets 18 kilobytes where a reference
# belongs.
#
# It is reported *before* the byte comparison because the comparison would otherwise answer
# it — with the wrong sentence. A link fails "is not exactly `@AGENTS.md` and a newline" by
# eighteen kilobytes, which names the size of the mistake and not its mechanism. The narrower
# case of a link to the *source document* is deliberately left to that comparison: it is the
# same verdict with the worse message, and buying the better message costs a branch nothing
# distinguishes from this one.
#
# ### Derived here too
#
# Neither the redirect's name nor the name it points at reaches the logic below as a literal.
# Both are read out of the doc's own sentence — "Redirect at the repository root from `X`,
# which holds `@Y`" — and `Y` is then required to be the name the deploy sentence carries,
# because a pointer at anything other than the deployed copy is a reader sent where the rules
# are not. So a rename in the document moves this gate rather than breaking it, and
# `cases/agents-md-current.cases.sh` proves that with a green arm that renames the redirect.
#
# The two names do appear once, in the message for a tree where *no* document declares a
# redirect — there is nothing left to derive from at that point, and quoting the sentence that
# went missing is the only help a reader gets. The deploy side's own zero-source message does
# the same thing for the same reason.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

# ------------------------------------------------------------------ find the source
#
# One level of `docs/`, every markdown file, asked the same question. Two details below are
# not incidental, and the second one was found the hard way.
#
# The newline squeeze is load-bearing: the deploy sentence wraps mid-phrase in the shipped
# file ("Deploy at the repository / root as `AGENTS.md`"), so a line-oriented grep finds
# nothing at all — which is a predicate that examines a real population and answers zero,
# the vacuous pass `gate_require_nonzero` exists to catch.
#
# **Only the preamble is searched — everything before the document's first `##` heading.**
# The first version of this predicate read whole files, and the selftest's `pass_case` went
# red on the shipped tree the moment docs/9 gained the row describing this gate, because
# that row *quotes the rule it documents*. Two sources, said the predicate, and it was
# looking at one declaration and one piece of prose about it. A gate that cannot tell those
# apart is a gate that forbids writing about itself, which is an absurd tax on exactly the
# documentation this repository runs on.
#
# The preamble is the right region rather than a convenient one: a statement about where a
# document deploys is a statement the document makes *about itself*, and this series makes
# those in its opening block — doc 10's is its second sentence, above "## What this is",
# where a reader meets it on opening the file. Anything below the first section heading is
# the document talking about the world. If the declaration is ever moved out of the
# preamble this predicate goes red saying no document declares a deployment, which is the
# correct answer to "the source stopped saying where it goes" and costs one commit to
# follow.
document_preamble() {
    sed -n '/^## /q;p' "$1"
}

deploy_instruction() {
    # The backticks are markdown, not command substitution: the sentence quotes the
    # deployed filename in code style and that is how the name is found.
    # shellcheck disable=SC2016
    document_preamble "$1" | tr '\n' ' ' | tr -s ' ' |
        grep -oE 'Deploy at the repository root as `[^`]+`' || true
}

# The redirect's declaration, read the same way and out of the same region: the sentence
# names the file that redirects and the name it holds a reference to, both in code style, and
# neither appears anywhere else in this predicate. The sentence wraps in the shipped file, so
# the newline squeeze above is load-bearing here for the same reason.
redirect_instruction() {
    # shellcheck disable=SC2016  # markdown backticks, as above
    document_preamble "$1" | tr '\n' ' ' | tr -s ' ' |
        grep -oE 'Redirect at the repository root from `[^`]+`, which holds `@[^`]+`' || true
}

docs_scanned=0
declare -a sources=()
declare -a instructions=()
declare -a redirect_sources=()
declare -a redirect_instructions=()
shopt -s nullglob
for doc in "$root"/docs/*.md; do
    docs_scanned=$((docs_scanned + 1))
    while IFS= read -r instruction; do
        [[ -n "$instruction" ]] || continue
        sources+=("$doc")
        instructions+=("$instruction")
    done < <(deploy_instruction "$doc")
    while IFS= read -r instruction; do
        [[ -n "$instruction" ]] || continue
        redirect_sources+=("$doc")
        redirect_instructions+=("$instruction")
    done < <(redirect_instruction "$doc")
done
shopt -u nullglob

gate_checked "$docs_scanned" "docs/*.md preambles asked whether they declare a deployed copy and a redirect to it"
gate_require_nonzero "$docs_scanned" "documents under docs/"

if ((${#sources[@]} == 0)); then
    gate_fail "no document under docs/ says where it deploys; the rule this gate enforces — 'Deploy at the repository root as AGENTS.md; the deployed copy tracks this file' — has no source, so either the series lost doc 10 or the sentence was reworded and this predicate must follow it"
    gate_finish
fi
if ((${#sources[@]} > 1)); then
    gate_fail "${#sources[@]} documents under docs/ each claim to deploy a copy (${sources[*]#"$root"/}); the deployment is one-directional from one source, and two sources is two answers to 'which side was wrong'"
    gate_finish
fi

source_doc="${sources[0]}"
source_rel="${source_doc#"$root"/}"
# shellcheck disable=SC2016  # markdown backticks, as above
target_name="$(printf '%s\n' "${instructions[0]}" | sed 's/.*`\([^`]*\)`.*/\1/')"
gate_checked 1 "deploy instruction(s) read out of the source document rather than transcribed here"

# The instruction says "at the repository root", so the name it carries is a bare
# filename. A target with a path separator in it would send everything below outside the
# tree under test — which for a predicate the selftest runs against mutated *copies* of
# the tree would mean comparing the copy's doc against the checkout's root file, and
# passing.
if [[ -z "$target_name" || "$target_name" == */* || "$target_name" == .* ]]; then
    gate_fail "$source_rel says it deploys as '$target_name', which is not a plain file name at the repository root; this predicate resolves the target under the tree it was given and cannot follow a path out of it"
    gate_finish
fi
gate_note "$source_rel is the source; it deploys at the repository root as $target_name"

deployed="$root/$target_name"
if [[ ! -f "$deployed" ]]; then
    gate_fail "$source_rel says it deploys at the repository root as $target_name, and there is no such file; the copy every agent in this tree actually reads is missing"
    gate_finish
fi

# ------------------------------------------------------------------ the same file twice?
#
# A symlink, or a hard link, makes the comparison below unfalsifiable: a file always equals
# itself, so this predicate would report PASS over a population of one while proving
# nothing at all. That is the same defect `gate_require_nonzero` names — a check that
# cannot fail — and it hides better here, because the tree would look right.
#
# It is reported as a finding rather than tolerated because the doc describes a *copy*
# ("the deployed copy tracks this file"), and a link is a different mechanism with
# different consequences: it cannot drift, which is a gain, but it also cannot carry the
# deliberate divergence the sentence's parenthesis contemplates, and a tool that reads
# `AGENTS.md` through an API that does not follow links sees the link target's path text
# instead of the rules. If this project decides a link is the deployment, the decision
# lands in the doc's own sentence and this branch changes with it.
if [[ "$deployed" -ef "$source_doc" ]]; then
    gate_fail "$target_name and $source_rel are the same file (a link, not a copy); comparing a file with itself cannot go red, so this gate would pass while proving nothing — the doc describes a deployed copy that tracks the source"
    gate_finish
fi

# ------------------------------------------------------------------ the comparison
#
# `cmp -s` is the verdict and `diff -u` is the explanation. Both run: a byte difference
# that produces no diff hunk — a trailing newline, a CRLF, a NUL — is exactly the kind a
# reader would otherwise be told about in a message with nothing under it, so the byte
# count and the line count are both reported and the diff is an excerpt rather than the
# proof.
lines_compared="$(wc -l <"$source_doc" | tr -d ' ')"
bytes_source="$(wc -c <"$source_doc" | tr -d ' ')"
bytes_deployed="$(wc -c <"$deployed" | tr -d ' ')"

if ! cmp -s "$source_doc" "$deployed"; then
    gate_fail "$target_name has drifted from $source_rel ($bytes_deployed bytes at the root, $bytes_source in the doc); the deployment is one-directional — the doc is the source and the root copy tracks it — so reconcile deliberately and record in the notes which side was wrong, rather than copying whichever file you happen to have open"
    diff -u "$source_doc" "$deployed" \
        --label "$source_rel (the source)" \
        --label "$target_name (the deployed copy)" |
        head -n 20 | sed 's/^/  | /' || true
fi

gate_checked "$lines_compared" "lines compared byte for byte between the source document and its deployed copy"
gate_require_nonzero "$lines_compared" "lines in the source document"

# ------------------------------------------------------------------ the redirect
#
# The header's third section argues every decision below. Everything here is derived from the
# sentence the source document carries, and the direction is the same one the messages above
# keep: the document declares, the root files track.
if ((${#redirect_sources[@]} == 0)); then
    gate_fail "no document under docs/ declares a redirect at the repository root; the 2026-08-14 ruling (note N102) puts one there — '\`CLAUDE.md\`, which holds \`@AGENTS.md\` and nothing else' — and a claim whose declaration is gone quantifies over nothing, so either the sentence was reworded and this predicate must follow it or the ruling was reversed and this section goes with it"
    gate_finish
fi
if ((${#redirect_sources[@]} > 1)); then
    gate_fail "${#redirect_sources[@]} documents under docs/ each declare a redirect at the repository root (${redirect_sources[*]#"$root"/}); the rules have one home and so does the sentence that points at it, and two declarations is two answers to which file the pointer is"
    gate_finish
fi

redirect_source_rel="${redirect_sources[0]#"$root"/}"
# shellcheck disable=SC2016  # markdown backticks, as above
redirect_name="$(printf '%s\n' "${redirect_instructions[0]}" | sed 's/.*from `\([^`]*\)`.*/\1/')"
# shellcheck disable=SC2016  # markdown backticks, as above
redirect_target="$(printf '%s\n' "${redirect_instructions[0]}" | sed 's/.*holds `@\([^`]*\)`.*/\1/')"
gate_checked 1 "redirect instruction(s) read out of the source document rather than transcribed here"

# A path here would send the rest of this section outside the tree under test, exactly as it
# would above: the selftest runs this predicate against mutated *copies*, and a target the
# predicate followed out of the copy would be answered by the checkout's own root file.
if [[ -z "$redirect_name" || "$redirect_name" == */* || "$redirect_name" == .* ]]; then
    gate_fail "$redirect_source_rel says the redirect at the repository root is '$redirect_name', which is not a plain file name there; this predicate resolves the redirect under the tree it was given and cannot follow a path out of it"
    gate_finish
fi
if [[ "$redirect_target" != "$target_name" ]]; then
    gate_fail "$redirect_source_rel says $redirect_name holds a reference to '$redirect_target' while the same preamble deploys as $target_name; the redirect exists to land a Claude-specific reader on the deployed copy, so a pointer at any other name is a reader sent where the rules are not"
    gate_finish
fi
gate_note "$redirect_source_rel is the source; $redirect_name at the repository root points at $target_name"

redirect="$root/$redirect_name"
if [[ ! -f "$redirect" ]]; then
    gate_fail "$redirect_source_rel says $redirect_name at the repository root holds '@$target_name', and there is no such file; a reader that opens the Claude-specific name finds nothing, and the rules it was pointed at are never reached"
    gate_finish
fi

if [[ "$redirect" -ef "$deployed" ]]; then
    gate_fail "$redirect_name and $target_name are the same file (a link, not a reference); a link makes 'the redirect resolves to the rules' true by construction rather than by content, and it is the second copy the ruling refuses — $redirect_source_rel describes a file holding '@$target_name'"
    gate_finish
fi

reference="@$target_name"
reference_bytes=$((${#reference} + 1))
redirect_bytes="$(wc -c <"$redirect" | tr -d ' ')"
redirect_opening="$(head -n 1 "$redirect")"

if [[ "$redirect_opening" != "$reference" ]]; then
    gate_fail "$redirect_name at the repository root opens with '$redirect_opening' where $redirect_source_rel declares '$reference'; the redirect is the reference and the reference is the deployed copy's name, so this points a Claude-specific reader somewhere else or nowhere"
elif ! printf '%s\n' "$reference" | cmp -s - "$redirect"; then
    gate_fail "$redirect_name at the repository root is not exactly '$reference' and a trailing newline ($reference_bytes bytes; it is $redirect_bytes); the redirect carries nothing else — a second line here is a second set of non-negotiable rules in force, and the rules live in $target_name"
    head -n 5 "$redirect" | sed 's/^/  | /'
fi

gate_checked "$redirect_bytes" "bytes in the redirect at the repository root, compared against the reference the source document declares"
gate_require_nonzero "$redirect_bytes" "bytes in the redirect at the repository root"

gate_finish
