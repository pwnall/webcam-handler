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

docs_scanned=0
declare -a sources=()
declare -a instructions=()
shopt -s nullglob
for doc in "$root"/docs/*.md; do
    docs_scanned=$((docs_scanned + 1))
    while IFS= read -r instruction; do
        [[ -n "$instruction" ]] || continue
        sources+=("$doc")
        instructions+=("$instruction")
    done < <(deploy_instruction "$doc")
done
shopt -u nullglob

gate_checked "$docs_scanned" "docs/*.md preambles asked whether they declare a deployed copy"
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

gate_finish
