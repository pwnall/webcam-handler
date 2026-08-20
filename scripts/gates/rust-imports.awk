# One reader for a Rust import statement, shared by every predicate that has to know which
# paths a file names.
#
# **Two predicates read the same fact and only one of them knew how.** Note **N269** records
# what that cost the first time: `facade-is-the-composition.sh` matched `engine::[a-z_]…`, a `{`
# ended the match attempt, and `use engine::{pairing, write};` — the exact bypass the predicate
# exists for — passed with a counted summary byte-identical to the unseeded tree's. The repair
# landed a joiner and a flattener in that one file, and `facade-stability-table-sync.sh` was
# written in the same commit with a fresh copy of the narrow rule and the same hole in it (note
# **N271**). A ban names the class and not one spelling of it (note **N249**, rubric A17), and
# two implementations of "read a Rust import" is the second home §2.10 forbids. This is the one
# home.
#
# ## What it normalises, and why each shape is here
#
#   * **A statement rustfmt broke across lines is joined** before anything reads it, because
#     rustfmt writes the multi-line group on its own once a list is long enough: a reader that
#     took lines one at a time would see `use engine::{`, then `pairing,`, then `};`, and none
#     of those three carries a path.
#   * **A group is flattened innermost-first**, so nesting, `self`, `as` renames and trailing
#     commas all reduce to the one fully-spelled form a caller's own matcher already reads.
#     Innermost-first is what makes splitting on commas safe: the matched body carries no
#     braces, so every comma in it separates that group's own items.
#   * **A glob survives the flattening as `<prefix>::*`**, which is what lets a caller refuse it
#     by name rather than discover it as a path that yielded nothing.
#   * **Every visibility and both import keywords count as a statement.** `pub(crate) use`,
#     `pub(super) use` and `extern crate … as …` are imports, and a rule that recognised only
#     `use` and `pub use` would drop the rest through to the plain-line hook, where a brace
#     group reads as nothing at all — which is note **N269**'s defect wearing a different
#     prefix. Both spellings were measured passing before this file existed.
#
# ## What the caller supplies
#
# This file carries the driver and the normalising functions. The predicate that includes it
# defines three hooks and nothing else has to change:
#
#   * `wch_emit_import(stmt, nr)` — one import statement, already joined, with `nr` the line the
#     statement *opened* on. Call `wch_flatten(stmt)` on it before matching paths; match the
#     unflattened `stmt` for the shapes flattening erases, a `self as e` binding among them.
#   * `wch_emit_other(line, nr)` — every line that is not part of an import statement.
#   * `wch_emit_runaway(nr, span)` — an import whose braces are still open `span` lines later.
#     A reader that cannot tell where a statement ends must say so: joining an unterminated one
#     would swallow the rest of the file into a single logical line and report every finding at
#     the opening line number, which is a worse answer than none. The bound is `wch_budget`,
#     which a caller may raise with `-v wch_budget=…` and an argument beside it.
#
# Include it in front of the predicate's own program:
#
#     awk -f "$(gate_rust_imports_awk)" -f <(cat <<'AWK' … AWK)
#
# The included file owns the rules; the caller's program is function definitions only, or the
# two rule sets both run over every line.

# mawk and gawk both return the substitution count from `gsub`, which is how a line's brace
# balance is read without a second pass over it.
function wch_braces(s,   c) {
    c = gsub(/\{/, "{", s)
    return c
}

function wch_unbraces(s,   c) {
    c = gsub(/\}/, "}", s)
    return c
}

# Rewrite the innermost group first and keep going until no `::{` is left.
function wch_flatten(line,   whole, cut, prefix, body, n, items, k, item, out, sep) {
    while (match(line, /[A-Za-z_][A-Za-z0-9_]*(::[A-Za-z_][A-Za-z0-9_]*)*::\{[^{}]*\}/)) {
        whole = substr(line, RSTART, RLENGTH)
        cut = index(whole, "::{")
        prefix = substr(whole, 1, cut - 1)
        body = substr(whole, cut + 3, length(whole) - cut - 3)
        n = split(body, items, ",")
        out = ""
        sep = ""
        for (k = 1; k <= n; k++) {
            item = items[k]
            gsub(/^[ \t]+/, "", item)
            gsub(/[ \t]+$/, "", item)
            if (item == "") continue
            # The rename is stripped *after* the path is rebuilt, so the module is named
            # before it is renamed and `use engine::pairing as p;` stays visible.
            sub(/[ \t]+as[ \t]+[A-Za-z_][A-Za-z0-9_]*$/, "", item)
            out = out sep ((item == "self") ? prefix : (prefix "::" item))
            sep = ", "
        }
        line = substr(line, 1, RSTART - 1) out substr(line, RSTART + RLENGTH)
    }
    return line
}

# Every legal spelling of a statement that brings a path into scope: any visibility qualifier,
# `use` or `extern crate`. Written once because both predicates below it were once written
# narrower, separately, and neither could see the other's hole.
function wch_is_import(line) {
    return line ~ /^[[:space:]]*(pub[[:space:]]*(\([^)]*\))?[[:space:]]+)?(use[[:space:]]|extern[[:space:]]+crate[[:space:]])/
}

BEGIN {
    wch_buffering = 0
    if (wch_budget == 0) wch_budget = 40
}

{
    if (wch_buffering) {
        wch_buf = wch_buf " " $0
        wch_depth += wch_braces($0) - wch_unbraces($0)
        if (wch_depth <= 0 && index($0, ";")) {
            wch_emit_import(wch_buf, wch_bufnr)
            wch_buffering = 0
            next
        }
        if (NR - wch_bufnr >= wch_budget) {
            wch_emit_runaway(wch_bufnr, NR - wch_bufnr)
            wch_buffering = 0
        }
        next
    }
    if (wch_is_import($0)) {
        wch_depth = wch_braces($0) - wch_unbraces($0)
        if (wch_depth > 0) {
            wch_buffering = 1
            wch_buf = $0
            wch_bufnr = NR
            next
        }
        wch_emit_import($0, NR)
        next
    }
    wch_emit_other($0, NR)
}

END {
    if (wch_buffering) wch_emit_runaway(wch_bufnr, NR - wch_bufnr)
}
