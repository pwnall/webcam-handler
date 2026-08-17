#!/usr/bin/env bash
#
# Every `///` doc comment opens with a summary line, never with a section heading.
#
# ## The defect this is about
#
# A new item spliced into the middle of an existing one's doc comment. It happened at B8:
# `RecordRequest::stream_for_container` was written between `budget_ms`'s prose and its
# `# Errors` section, so `stream_for_container` shipped documented as *"How long this recording
# may run, in milliseconds"* — carrying the cap's whole argument — and `budget_ms` was left with
# a bare `# Errors` and **no summary line at all**. Two facts on the wrong function and one
# destroyed, in a crate whose doc comments are inputs to committed artifacts.
#
# Nothing could see it. `cargo doc -D warnings` is happy: a doc comment exists. `clippy`'s
# `missing_errors_doc` is happy: an `# Errors` section exists. `schema-artifacts-current.sh` is
# happy: methods are not in the JSON Schema bundle, so the emitted files did not move. The one
# residue the splice leaves in the bytes is the item whose doc comment now *starts* on a
# markdown heading, and that is what this reads.
#
# ## Why the rule is worth having on its own
#
# rustdoc's first paragraph is the summary: it is what every module index, every search result
# and every `#[command]` help line shows. An item whose doc opens on `# Errors` has no summary
# anywhere those appear — and since P6e the same doc comments are printed **to a person** by
# clap and emitted into `docs/agent-guide.md` (note **N123**), where a heading arriving as the
# first line of a verb's help is not a formatting nit. So this is a claim about the shipped
# text, not only a tripwire for one editing accident.
#
# ## What it checks
#
# Every `*.rs` file under the tree, every `///` block in it, and the block's **first** line. A
# heading anywhere else is a section and is exactly right; a heading in the first line is a
# summary that is missing. `//!` module headers are deliberately out of scope: they document a
# file rather than an item, they are never printed by clap, and a module whose header opens on a
# heading is a style question rather than a lost sentence.
#
# ## Honest limits
#
#   - `#[doc = "…"]` attributes are not read. Nothing in this workspace documents an item that
#     way, and a rule that had to evaluate a `concat!` would need a Rust parser.
#   - It sees the *shape* of a splice, not a splice. Text moved onto the wrong item while both
#     items keep a summary line is invisible here and belongs to review — which is how the B8
#     finding was actually found. What this buys is that the destroyed half can never be silent.
#   - **The destroyed half is only visible when it keeps a doc comment.** B9 spliced a `const`
#     between `record_request`'s doc comment and `record_request`, so the constant wore the
#     function's prose and the function was left with **no `///` at all** — no block, so nothing
#     for this to read. Note **N222** measured the two rules that would have seen it and both
#     cost more than they are worth here: requiring every item to carry a doc comment lights up
#     272 items in this tree, and reading a paragraph-final sentence on its own line as a
#     spliced summary matches 12 places of which 11 are ordinary prose. Review is what finds
#     this shape; what the rule above still buys is that a *surviving* block cannot lose its
#     summary silently.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

# Print `<line number>\t<first line>` for every `///` block in $1 that opens on a markdown
# heading.
#
# A block is a run of `///` lines with no other kind of line in it; the run's first line is the
# summary. `////` is not a doc comment (rustdoc ignores four-slash runs) and neither is `//!`,
# so both close a block rather than continuing one — which is what the two negative character
# classes below are for.
blocks_that_open_on_a_heading() {
    awk '
        function verdict() {
            if (first ~ /^\/\/\/[[:space:]]*#+[[:space:]]/) { printf "%d\t%s\n", at, first }
        }
        {
            line = $0
            sub(/^[[:space:]]+/, "", line)
            if (line == "///" || line ~ /^\/\/\/[^\/!]/) {
                if (!open) { open = 1; first = line; at = FNR }
                next
            }
            if (open) { verdict(); open = 0 }
        }
        END { if (open) verdict() }
    ' "$1"
}

files=0
blocks=0
while IFS= read -r -d '' file; do
    rel="${file#"$root"/}"
    files=$((files + 1))
    # Counted so the claim below rests on a population rather than on a walk that found no
    # doc comments at all — a tree whose `///` lines had all become `//` would otherwise pass
    # this silently.
    blocks=$((blocks + $(grep -cE '^[[:space:]]*///([^/!]|$)' "$file" || true)))
    while IFS=$'\t' read -r number opening; do
        gate_fail "$rel:$number opens a doc comment on a section heading ($opening); rustdoc, clap and docs/agent-guide.md all show the first paragraph as the item's summary, and an item that starts on '# …' has none — this is the residue a doc comment spliced into another item's leaves behind (note **N208**)"
    done < <(blocks_that_open_on_a_heading "$file")
done < <(gate_rust_files)

gate_checked "$files" "Rust source files read for a doc comment with no summary line"
gate_require_nonzero "$files" "Rust source files"
gate_checked "$blocks" "doc comment lines in them"
gate_require_nonzero "$blocks" "doc comment lines"

gate_finish
