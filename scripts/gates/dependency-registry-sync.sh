#!/usr/bin/env bash
#
# Design §2.8's dependency registry and the root `Cargo.toml`'s `[workspace.dependencies]` table
# are one fact, and this predicate is the reconciler note **N133** asked for and the check whose
# absence L32 (note **N164**) priced.
#
# **The fact is the manifest.** `[workspace.dependencies]` is what cargo resolves, what
# `Cargo.lock` pins, what cargo-deny licence-checks and what `dependency-walls.sh` and
# `feature-posture.sh` walk. §2.8's table *describes* it — it is where AGENTS' "Docs and
# dependencies" sends a reader ("design §2.8 is the registry"), and where a person deciding
# whether an edge has been reviewed actually looks. When the two disagree the manifest is right
# about what the product links and the table is the stale half, which is why every sentence below
# names which file to edit rather than reporting that two things differ.
#
# ## Why this is a gate and not a test
#
# Neither of the two facts is reachable from a `#[test]`. `Cargo.toml`'s workspace table is
# consumed by cargo and by nothing in this workspace; §2.8's table is a markdown table in a
# document no crate reads. A Rust test that reconciled them would have to put a design-document
# parser inside the product's own dependency graph — a new edge, in the crate whose job is to
# police edges. So this is the shape docs/15 gives every manifest-against-prose reconciler in this
# suite (`msrv-sync.sh`, `browser-pins-sync.sh`, `wire-surface-sync.sh`): populations derived from
# both sides, walked in both directions, with an unmatched member a **failure** and never a skip.
#
# The defect class is measured rather than imagined. The G6 adversarial review found §2.8 wrong
# four ways at once [N133]: `tower`, `tokio-stream` and `caps` adopted and never registered — the
# third being the single third-party edge of the *root-equivalent* helper, so the crate with the
# strongest stated reason for its dependency list to be reviewed was the one the registry did not
# list; `tower-http` stated at 0.7 while the manifest pinned 0.6.11; and an evidence sentence that
# had stopped being true. Sixteen hours later the same review found the inverse [N164, L32]:
# `clap_complete` and `clap_mangen` pinned at P0 for an emitter nobody wrote, sitting in the
# manifest for six phases with no consumer and nothing able to notice. N133 and N164 both close
# with the same sentence — nothing goes red on this — and both name this predicate as the repair.
#
# ## The five claims
#
#   1. **Structural.** The design document is there, §2.8's heading is there, the registry's
#      header row `| Crate | Pin | License | Scope | Why |` is there, the manifest is there and it
#      has a `[workspace.dependencies]` table. Every one of those is asserted before anything is
#      counted, because a reworded section that silently empties a population is the way this
#      predicate would pass by examining nothing. Both populations are `gate_require_nonzero`.
#   2. **Direction A — manifest to table.** Every `[workspace.dependencies]` entry that is not a
#      workspace member of this workspace is registered by some row. The membership half is
#      derived from `cargo metadata`, never transcribed, so the nine `webcam-handler-*` path deps
#      are excluded structurally and a tenth crate joining the workspace excludes itself. A miss
#      is N133's class: a crate adopted and never registered, and the design table is the file to
#      edit.
#   3. **Direction B — table to manifest.** Every row whose pin cell carries no bold parenthetical
#      mark names crates the manifest declares. A miss is L32's class, and the manifest is the
#      file to edit — or the row is the thing to delete.
#   4. **Direction C — the pins agree.** For a row naming exactly one crate whose pin cell yields
#      exactly one version-shaped token, the manifest's requirement must begin with the row's pin
#      as a dot-delimited prefix: the table states the pin at adoption (`0.14`, `8`, `1`) and the
#      manifest states the requirement it was pinned to (`0.14.0`, `8.12.0`, `1.0.104`). `0.5`
#      against `0.6.1` is a disagreement; `0.14` against `0.14.0` is not. This is N133's second
#      drift, the one where the document states the higher number and the build uses the lower.
#   5. **A mark buys a row an absent manifest entry and never a hidden one.** §2.8's lead-in puts
#      the marks in the pin cell "where a version would otherwise be": **(lock only)** for an edge
#      the walls police that no manifest names, **(not yet an edge — …)** for an adoption this
#      revision decided and a later phase lands. A marked row is excused from direction B **and
#      still registers its crates for direction A**, which is the half that can go wrong quietly —
#      an implementation that dropped marked rows from the population would let a manifest entry
#      whose only registrar is a marked row go unregistered and unnoticed. Every honoured mark is
#      named in a note and counted with the rest, because an excusal nobody can see is the silent
#      skip AGENTS rule 3 forbids.
#
# ## The two shapes that make direction B one-directional, and why they are not symmetric
#
# `futures-timer 3.0.4` is a `[workspace.dependencies]` entry **no crate in this workspace names**.
# It is carried on purpose — jsonrpsee's async client reaches it, the lock resolves it, and an
# adoption visible only in `Cargo.lock` is one nobody reviewed — so the registry lists it and the
# lock, not the line, pins it. `hyper` is the mirror image: a real edge the daemon acquires through
# axum and jsonrpsee-server, in the table as **(lock only)** with no manifest row at all. A naive
# "every table row must have a consumer" branch is red on `futures-timer`, and a naive "every table
# row must have a manifest entry" branch is red on `hyper`. Neither is written here: consumers are
# not this predicate's subject, and the mark is what buys `hyper` its row. What separates the two
# from N164's `clap_complete` is that both say what they are, in the file, where the next reader
# looks.
#
# ## What this does **not** claim
#
#   * **It does not read `Cargo.lock`.** A pin the manifest states and the lock resolves higher is
#     invisible here; so is a transitive edge with no row and no mark. `dependency-walls.sh` and
#     `feature-posture.sh` are what read the *resolved* graph, and the honest split is that this
#     predicate reconciles two declarations while those two reconcile a declaration against a
#     resolution.
#   * **It does not verify a licence, a scope or a reason.** Columns 3, 4 and 5 are read past. The
#     licence claim is cargo-deny's (`license-allowlist.sh`), the scope column is prose about which
#     crates own an edge, and nothing checks that a "why" is true. A row can be right about its
#     crate and its version and wrong about everything else and this stays green.
#   * **It does not claim a registered crate has a consumer.** That is `futures-timer`'s shape
#     above, and deliberately not a branch — L32's residual is closed by the mark and the
#     disposition sentence being *written*, not by a linkage walk.
#   * **Some rows reconcile by name only, and they are counted rather than assumed.** A row naming
#     several crates (`serde` / `serde_json`, `thiserror` / `anyhow`, `anstream`/`anstyle`, the
#     three `tracing` crates) states a pin per crate in an order this predicate does not try to
#     pair up, and `hyper`'s pin cell holds a mark where a number would be. Those rows are checked
#     for membership in both directions and not for version, each one named in a note and the count
#     `gate_checked`, so the weaker check is visible in the output rather than inferred from the
#     absence of a message.
#   * **The `image` row's pin *is* checked, and that is a decision.** Its pin cell is
#     `` 0.25, `default-features=false, features=["png","jpeg"]` `` — a version and then the feature
#     posture that is the whole reason the row is interesting. The feature text yields no
#     version-shaped token, so the cell yields exactly one and the rule above applies to it
#     unchanged. Checking it costs nothing and buys the one row whose version is most likely to
#     move (the `avif` default that drags rav1e is a *major*-line concern), and the alternative —
#     special-casing the row into the names-only bucket — would have made the bucket a list of
#     exceptions instead of a consequence. What is *not* checked is the feature text itself; that
#     is `feature-posture.sh`'s claim, over the resolved graph, which is the stronger place for it.
#   * **The mark vocabulary is trusted, not validated.** Any `**(…)**` in a pin cell excuses the
#     row from direction B; this predicate does not know that "lock only" and "not yet an edge" are
#     the only two spellings §2.8 allows, so a row marked **(reasons)** would be honoured. It would
#     be honoured *loudly* — every mark is printed with its row — which is the trade: a closed
#     vocabulary transcribed here would be a policy list that goes stale against the document it is
#     reading, and the reader of a gate's output is the check on a mark nobody agreed to.
#   * **And a mark that has outlived its reason is not a finding here.** §2.8's `image-compare` row
#     says "the mark comes off the pin in the commit that adds the manifest row", so a
#     **(not yet an edge — …)** row whose manifest entry has since landed is a stale mark — and
#     this predicate stays green on it, because the two mark spellings differ in kind: **(lock
#     only)** asserts a permanent absence, while **(not yet an edge — …)** is transitional and its
#     own row schedules its removal. Adjudicating that schedule would make this gate's verdict a
#     function of which of two sibling commits landed first — a manifest row and a document edit
#     that belong together but are written by different hands. The commission is the two membership
#     directions and the versions; the mark's retirement is the reviewer's, and it is visible in
#     this predicate's output because every honoured mark is printed with its row.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"
design_path="docs/12-claude-fable-design-v3.md"
design="$root/$design_path"
manifest_path="Cargo.toml"
manifest="$root/$manifest_path"

# The two anchors that cannot be derived, because they are how §2.8 is *spelled* rather than
# something it declares. Both are asserted below before a single row is read.
section_heading='### 2.8 Workspace, dependencies, licenses'
table_header='| Crate | Pin | License | Scope | Why |'

# A backtick, as a variable, so the patterns and sentences below can be built inside double
# quotes without every one of them opening a command substitution.
tick='`'

if [[ ! -f "$design" ]]; then
    gate_fail "no $design_path; §2.8 is the dependency registry's one home and there is nothing to reconcile $manifest_path against"
    gate_finish
fi
if [[ ! -f "$manifest" ]]; then
    gate_fail "no $manifest_path at the workspace root; the manifest is the fact this predicate compares the registry to and it cannot be read"
    gate_finish
fi

# ------------------------------------------------------------------ the manifest

# `[workspace.dependencies]`, parsed off the file rather than out of `cargo metadata`, and that is
# forced rather than chosen: a workspace dependency **no member names** — `futures-timer` — appears
# in no package's dependency list and in no resolve node, so the graph cannot see the entry whose
# visibility is the whole reason the registry exists. The table is small and one entry per line, so
# the parse is a name, an `=`, and either a string or an inline table with a `version` key in it; a
# value that spans lines is accumulated until its braces balance, so an entry somebody wraps does
# not silently vanish from the population.
manifest_entries=()
declare -A manifest_version=()
saw_dependency_table=0
while IFS=$'\t' read -r kind name version; do
    case "$kind" in
    TABLE) saw_dependency_table="$name" ;;
    ENTRY)
        manifest_entries+=("$name")
        manifest_version["$name"]="$version"
        ;;
    esac
done < <(awk '
    function occurrences(s, c,   n, i) {
        n = 0
        for (i = 1; i <= length(s); i++) if (substr(s, i, 1) == c) n++
        return n
    }
    /^\[/ { in_deps = ($0 == "[workspace.dependencies]"); if (in_deps) seen = 1; next }
    !in_deps { next }
    /^[[:space:]]*#/ { next }
    /^[[:space:]]*$/ { next }
    {
        if (pending == "") {
            if ($0 !~ /^[A-Za-z0-9_.+-]+[[:space:]]*=/) next
            pending = $0
        } else {
            pending = pending " " $0
        }
        if (occurrences(pending, "{") > occurrences(pending, "}")) next
        line = pending
        pending = ""
        idx = index(line, "=")
        name = substr(line, 1, idx - 1)
        gsub(/[[:space:]]/, "", name)
        rest = substr(line, idx + 1)
        sub(/^[[:space:]]+/, "", rest)
        version = ""
        if (substr(rest, 1, 1) == "\"") {
            tail = substr(rest, 2)
            quote = index(tail, "\"")
            if (quote > 0) version = substr(tail, 1, quote - 1)
        } else if (match(rest, /version[[:space:]]*=[[:space:]]*"[^"]*"/)) {
            field = substr(rest, RSTART, RLENGTH)
            match(field, /"[^"]*"/)
            version = substr(field, RSTART + 1, RLENGTH - 2)
        }
        print "ENTRY\t" name "\t" version
    }
    END { print "TABLE\t" (seen ? 1 : 0) }
' "$manifest")

if ((saw_dependency_table == 0)); then
    gate_fail "$manifest_path declares no [workspace.dependencies] table; that table is the manifest-side fact §2.8's registry describes, and a registry with nothing to reconcile against passes by examining nothing"
    gate_finish
fi

# Workspace membership, derived. The nine `webcam-handler-*` path entries are this workspace's own
# crates and not adoptions, so they are excluded *structurally* — a tenth crate added to `members`
# excludes itself, and a crate that leaves the workspace starts needing a registry row on the same
# day it starts being a dependency.
declare -A is_member=()
members=0
while IFS= read -r member; do
    [[ -n "$member" ]] || continue
    is_member["$member"]=1
    members=$((members + 1))
done < <(gate_metadata | jq -r '
    ( [ .packages[] | { key: .id, value: .name } ] | from_entries ) as $names
    | .workspace_members[] | $names[.] // empty')

if ((members == 0)); then
    gate_fail "cargo metadata reports no workspace members for $manifest_path; without them this predicate cannot tell an adoption from one of this workspace's own crates, and would demand a registry row for every path dependency"
    gate_finish
fi
gate_checked "$members" "workspace members excluded from the adoption population, derived from cargo metadata"

third_party=()
for name in "${manifest_entries[@]}"; do
    [[ -n "${is_member[$name]:-}" ]] && continue
    third_party+=("$name")
done
gate_checked "${#third_party[@]}" "third-party [workspace.dependencies] entries in $manifest_path"
gate_require_nonzero "${#third_party[@]}" "third-party [workspace.dependencies] entries"

# ------------------------------------------------------------------ the registry table

row_lines=()
row_crates=()
row_pins=()
saw_heading=0
saw_header=0
while IFS=$'\t' read -r kind field_a field_b field_c; do
    case "$kind" in
    HEADING) saw_heading=1 ;;
    HEADER) saw_header=1 ;;
    ROW)
        row_lines+=("$field_a")
        row_crates+=("$field_b")
        row_pins+=("$field_c")
        ;;
    esac
done < <(awk -v heading="$section_heading" -v header="$table_header" '
    BEGIN { FS = "|" }
    $0 == heading { in_section = 1; print "HEADING\t1"; next }
    in_section && /^#/ { in_section = 0 }
    !in_section { next }
    $0 == header { in_table = 1; print "HEADER\t1"; next }
    !in_table { next }
    /^\|[[:space:]]*-/ { next }
    $0 !~ /^\|/ { in_table = 0; next }
    {
        crate = $2
        pin = $3
        gsub(/^[[:space:]]+|[[:space:]]+$/, "", crate)
        gsub(/^[[:space:]]+|[[:space:]]+$/, "", pin)
        print "ROW\t" NR "\t" crate "\t" pin
    }
' "$design")

if ((saw_heading == 0)); then
    gate_fail "$design_path no longer carries the heading '$section_heading'; a reworded section empties this predicate's population, so it is a failure rather than a pass"
    gate_finish
fi
if ((saw_header == 0)); then
    gate_fail "$design_path's §2.8 has no table whose header row is '$table_header'; the registry is that table and a renamed column is a population this predicate can no longer find — restore the header rather than letting the registry go unread"
    gate_finish
fi

gate_checked "${#row_lines[@]}" "rows in §2.8's dependency registry"
gate_require_nonzero "${#row_lines[@]}" "rows in §2.8's dependency registry"

# ------------------------------------------------------------------ the walk

# Every crate name any row registers, marked or not: the mark excuses a row from direction B and
# never from being the row that registers its crate.
declare -A registered_at=()
registered_names=0
marked_rows=0
names_only_rows=0
version_checked=0
unmatched_pin_rows=0

for index in "${!row_lines[@]}"; do
    line="${row_lines[$index]}"
    crate_cell="${row_crates[$index]}"
    pin_cell="${row_pins[$index]}"

    names=()
    while IFS= read -r name; do
        [[ -n "$name" ]] || continue
        names+=("$name")
    done < <(printf '%s' "$crate_cell" | grep -oE "${tick}[^${tick}]+${tick}" | tr -d "$tick")

    if ((${#names[@]} == 0)); then
        gate_fail "$design_path:$line is a row in §2.8's registry whose Crate cell ('$crate_cell') names no backticked crate; every row registers a crate and one that names none can neither be reconciled nor be seen to have stopped being"
        continue
    fi

    mark="$(printf '%s' "$pin_cell" | grep -oE '\*\*\([^)]*\)\*\*' | head -n1 || true)"
    pin_body="$(printf '%s' "$pin_cell" | sed 's/\*\*([^)]*)\*\*//g')"

    # The version-shaped tokens the pin cell offers, once the marks are out of it. Everything that
    # is not a number sequence — feature text, a `(journald)` aside, the `/` between two pins — is
    # dropped by the character class rather than by a list of exceptions, and `1.x` contributes the
    # number sequence in front of the `x` (design §2.8 writes tokio's pin that way).
    pins=()
    while IFS= read -r token; do
        [[ -n "$token" ]] || continue
        if [[ "$token" =~ ^[0-9]+(\.[0-9]+)*$ ]]; then
            pins+=("$token")
        elif [[ "$token" =~ ^([0-9]+(\.[0-9]+)*)\.x$ ]]; then
            pins+=("${BASH_REMATCH[1]}")
        fi
    done < <(printf '%s\n' "$pin_body" | tr -c 'A-Za-z0-9._-' '\n')

    for name in "${names[@]}"; do
        registered_names=$((registered_names + 1))
        if [[ -z "${registered_at[$name]:-}" ]]; then
            registered_at["$name"]="$line"
        fi
    done

    if [[ -n "$mark" ]]; then
        marked_rows=$((marked_rows + 1))
        gate_note "$design_path:$line — the ${crate_cell} row's pin carries the mark $mark, so it registers its crate(s) and is excused from the manifest direction"
    else
        for name in "${names[@]}"; do
            if [[ -n "${is_member[$name]:-}" ]]; then
                gate_fail "$design_path:$line registers ${tick}${name}${tick}, which is one of this workspace's own crates rather than an adoption; §2.8's registry is one row per [workspace.dependencies] *third-party* entry, so $design_path is the file to edit"
            elif [[ -z "${manifest_version[$name]:-}" ]]; then
                gate_fail "$design_path:$line registers ${tick}${name}${tick} and $manifest_path's [workspace.dependencies] declares no such entry; the manifest is what cargo resolves, licence-checks and locks, so $manifest_path is the file to edit — or delete the row (L32, note N164: clap_complete and clap_mangen sat in the registry for six phases with no consumer). A row that is deliberately not an edge says so with a mark in its pin cell."
            fi
        done
    fi

    # Direction C. One crate, one pin, and a manifest entry to compare it with; anything else is
    # reconciled by name alone and said so, out loud, with its row.
    if ((${#names[@]} == 1)) && ((${#pins[@]} == 1)); then
        name="${names[0]}"
        pin="${pins[0]}"
        have="${manifest_version[$name]:-}"
        if [[ -z "$have" ]]; then
            unmatched_pin_rows=$((unmatched_pin_rows + 1))
            gate_note "$design_path:$line — the ${tick}${name}${tick} row states the pin $pin and $manifest_path has no entry to check it against"
        else
            version_checked=$((version_checked + 1))
            if [[ "$have" != "$pin" && "$have" != "$pin".* ]]; then
                gate_fail "$design_path:$line pins ${tick}${name}${tick} at $pin and $manifest_path declares $have; the manifest is the version cargo resolves, so $design_path is the file to edit (note N133: the registry stated tower-http at 0.7 while the build used 0.6.11)"
            fi
        fi
    else
        names_only_rows=$((names_only_rows + 1))
        gate_note "$design_path:$line — the ${crate_cell} row states '$pin_cell' for ${#names[@]} crate(s), so it is reconciled by name and not by version"
    fi
done

gate_checked "$registered_names" "crate names §2.8's registry rows carry"
gate_require_nonzero "$registered_names" "crate names in §2.8's registry"
gate_checked "$version_checked" "rows whose pin is reconciled against the manifest's requirement"
gate_require_nonzero "$version_checked" "rows whose pin is reconciled against the manifest"
gate_checked "$names_only_rows" "rows reconciled by name only (several crates, or a pin cell with no single version-shaped token)"
gate_checked "$marked_rows" "rows excused from the manifest direction by a mark in the pin cell"
gate_checked "$unmatched_pin_rows" "rows stating a pin the manifest has no entry to check it against"

# Direction A. Every adoption is registered by some row, marked or not.
registered_entries=0
for name in "${third_party[@]}"; do
    if [[ -n "${registered_at[$name]:-}" ]]; then
        registered_entries=$((registered_entries + 1))
    else
        gate_fail "$manifest_path's [workspace.dependencies] declares ${tick}${name}${tick} and $design_path's §2.8 registry has no row for it; a crate adopted and never registered is note N133's defect — three of them at the G6 review, one being the only third-party edge of the root-equivalent helper — so $design_path is the file to edit, one row per entry"
    fi
done
gate_checked "$registered_entries" "third-party manifest entries registered by a row in §2.8"
gate_require_nonzero "$registered_entries" "third-party manifest entries registered by a row"

gate_finish
