#!/usr/bin/env bash
#
# The web client's prose cites Rust items by path, and this predicate is the thing that makes
# those citations claims rather than decoration.
#
# ## The defect class, and the instance that shipped
#
# `crates/web/assets/` is hand-written JavaScript that cannot `use` anything: every wire name,
# every bound and every rule it follows is a **second copy** of something a Rust crate owns, and
# the way this client keeps the two together is a doc comment naming the Rust home beside each
# copy — `daemon::http::rpc::RPC_PATH`, `schema::limits::CLIENT_REQUEST_TIMEOUT_MS`,
# `engine::preview::while_suspended`. A reader who wants to know whether the page is still right
# is sent to that path. A path that resolves to nothing sends them nowhere, and says nothing
# about the copy it was written to justify.
#
# It is not hypothetical. D20's `/session-photo` landed with `credential.js` citing
# `daemon::http::samples::SESSION_PHOTO_PATH`; the module is `session_photo` and there has never
# been a `samples` one. The spelling of the *route* happened to be right, so every test in the
# workspace stayed green while the sentence beside it pointed at a module that does not exist
# (note **N275**).
#
# ## What it checks, and what it deliberately does not
#
# **Two rules over two derived populations, because the class is "a citation that resolves to
# nothing" and a Rust path is one spelling of it** (AGENTS' "a ban on a defect names the class,
# not one spelling of it", note **N249**). The batch that landed the first rule shipped the second
# spelling beside it: `credential.js` and `crates/web/src/lib.rs` both told a reader that
# `scripts/gates/web-client-urls-sync.sh` reconciles the page's wire names, and no such file has
# ever existed — green here, because a `scripts/…` path was outside the population (note
# **N284**).
#
# **Rule one**: every `` `crate::path::to::Item` `` in a shipped asset resolves. The walk descends
# a segment at a time — a module while the segment is one (a file, a
# `foo/mod.rs`, or an inline `mod foo` in its parent), an item as soon as it is not — and the
# first segment that is neither is the finding, named with the file the walk had reached, so a
# reader is told where the path stopped being true rather than that it is wrong somewhere.
#
# **Rule two**: every backticked path into this repository — `` `scripts/gates/….sh` ``,
# `` `crates/…/….rs` `` — names a file that is there. It reads the same assets *and*
# `crates/web/src`, which is the crate that documents them and where the second half of the
# measured instance was: a claim about what reconciles this client is a claim wherever it is
# written.
#
# Recognising an item is deliberately coarse — a declaration keyword, a struct field, or an enum
# variant, matched as text. A Rust parser to decide whether `Session::criteria` is a field would
# be a second opinion about a language this repository does not otherwise read from a shell, and
# what the check is *for* is the module hop, which is where the measured defect was and where a
# rename lands. It is still not a word search: `samples` appears in `daemon::http`'s own prose
# eleven times and is declared nowhere, which is exactly how the shipped citation was wrong.
#
# **The population is the paths that name a crate**, and the shorthands are counted and named
# rather than silently passed over: `limits::PREVIEW_MAX_VIEWERS_PER_CAMERA`, `render::writes` and
# `WriteReport::disabled_automation` are how this client refers to homes whose crate is obvious
# from the sentence around them, and resolving one would mean this predicate guessing which crate
# was meant. Their *values* are held elsewhere —
# `the_bounds_the_page_runs_on_are_the_ones_this_build_declares` for the `limits` numbers — and a
# shorthand promoted to a full path joins this population the day it is written. A type-shaped
# first segment (`RestoreReport::is_complete`) is crate-less by exactly the same rule and is
# counted in the same bucket; it was invisible to the count until 2026-08-20, because the citation
# pattern anchored on a lowercase first segment and dropped twenty-two of them without saying so
# (note **N284**).
#
# **Every shipped asset, not every `.js`.** `index.html` carries citations too — D20's criteria
# field cites `schema::session::Session::criteria` in the comment beside it — and a population
# that read ten of twelve files was a derived population with two silent exclusions in it.
#
# It says nothing about whether the citation is *apt* — a page citing the wrong real constant is
# a review's finding, not this one's — and nothing about the values themselves, which is
# `the_urls_the_page_builds_are_the_routes_this_daemon_serves`'s subject (the names
# `daemon::http` owns) and `the_bounds_the_page_runs_on_are_the_ones_this_build_declares`'s (the
# numbers `schema::limits` owns). Three checks, three subjects: the values, the values, and the
# addresses of the homes they were copied from.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"
assets="$root/crates/web/assets"

if [[ ! -d "$assets" ]]; then
    gate_fail "no crates/web/assets; the client this predicate reads is not in this tree"
    gate_finish
fi

# The crates a citation may name, and where each one's source starts. Derived from the workspace
# rather than transcribed: a crate whose directory moved must move this list with it, and a
# citation naming a crate that is not here is reported rather than skipped.
declare -A crate_root=(
    [daemon]="crates/daemon/src"
    [schema]="crates/schema/src"
    [engine]="crates/engine/src"
    [api]="crates/api/src"
    [imaging]="crates/imaging/src"
    [web]="crates/web/src"
    [cli_core]="crates/cli-core/src"
)

for name in "${!crate_root[@]}"; do
    if [[ ! -d "$root/${crate_root[$name]}" ]]; then
        gate_fail "this predicate expects \`$name\` at ${crate_root[$name]}, and there is no such directory; a crate that moved takes every citation of it with it"
    fi
done

# Where module `$2` lives, starting from the file `$1`. Prints the module's file, or nothing.
module_file() {
    local from="$1" segment="$2" dir base
    dir="$(dirname "$from")"
    base="$(basename "$from" .rs)"
    # `foo.rs` beside `foo/`, and `foo/mod.rs` — the two layouts this workspace uses.
    if [[ "$base" != "lib" && "$base" != "mod" ]]; then
        dir="$dir/$base"
    fi
    if [[ -f "$dir/$segment.rs" ]]; then
        printf '%s\n' "$dir/$segment.rs"
        return 0
    fi
    if [[ -f "$dir/$segment/mod.rs" ]]; then
        printf '%s\n' "$dir/$segment/mod.rs"
        return 0
    fi
    # An inline `mod segment { … }`: the parent file is the module's file too.
    if grep -qE "^[[:space:]]*(pub(\([^)]*\))?[[:space:]]+)?mod[[:space:]]+${segment}[[:space:]]*\{" "$from"; then
        printf '%s\n' "$from"
        return 0
    fi
    return 1
}

# Whether the file `$1` declares an item, a field or a variant called `$2`.
#
# Text, and the header prices it. What it must *not* be is a word search: the module name this
# predicate exists to have caught appears in its parent's prose and is declared nowhere.
declares_item() {
    local file="$1" name="$2"
    grep -qE "\b(fn|const|static|struct|enum|type|trait|union|mod|macro_rules!)[[:space:]]+$name\b" "$file" && return 0
    # A struct field, and an enum variant with a payload or without one.
    grep -qE "^[[:space:]]*(pub([[:space:]]*\([^)]*\))?[[:space:]]+)?$name:" "$file" && return 0
    grep -qE "^[[:space:]]*$name([[:space:]]*[\{(]|,\$)" "$file" && return 0
    return 1
}

# Every crate-shaped path this file names in backticks, once each.
#
# The first segment is any identifier, not a lowercase one. Anchoring on lowercase dropped every
# citation whose head is a type name — `RestoreReport::is_complete`, `Applied::is_exact`, twenty-two
# of them — out of the population *and* out of the count of what was skipped, which is the one
# thing a derived population may not do (note **N284**).
citations_in() {
    # shellcheck disable=SC2016  # a backtick-delimited citation, matched literally
    grep -ohE '`[A-Za-z_][A-Za-z0-9_]*(::[A-Za-z0-9_]+)+`' "$1" | tr -d '`' | sort -u
}

# Every backticked path into this repository this file names, once each.
#
# Anchored on the directories this client's prose actually cites, so an ordinary sentence about
# `crates/web/assets/` is not read as a claim that a file called `assets/` exists: a path is a
# citation here when it ends in `.sh` or `.rs`, which is what a "go and read this" reference looks
# like.
paths_in() {
    # shellcheck disable=SC2016  # a backtick-delimited citation, matched literally
    grep -ohE '`[A-Za-z0-9_./-]+\.(sh|rs)`' "$1" | tr -d '`' | sort -u
}

files=0
citations=0
shorthands=0
paths=0
while IFS= read -r -d '' asset; do
    files=$((files + 1))
    while IFS= read -r path; do
        [[ -n "$path" ]] || continue
        # `::` is not a separator `read -a` splits on, so the path is split by hand.
        mapfile -t segments < <(printf '%s\n' "$path" | sed 's/::/\n/g')
        crate="${segments[0]}"
        where="${crate_root[$crate]:-}"
        if [[ -z "$where" ]]; then
            shorthands=$((shorthands + 1))
            continue
        fi
        citations=$((citations + 1))
        current="$root/$where/lib.rs"
        if [[ ! -f "$current" ]]; then
            gate_fail "${asset#"$root"/} cites \`$path\`, and $where/lib.rs is not there to start from"
            continue
        fi
        index=1
        # A module while the segment resolves as one, an item as soon as it does not — and
        # everything after the first item is a field, a variant or a method of it.
        while ((index < ${#segments[@]})); do
            segment="${segments[$index]}"
            if next="$(module_file "$current" "$segment")"; then
                current="$next"
                index=$((index + 1))
                continue
            fi
            break
        done
        while ((index < ${#segments[@]})); do
            segment="${segments[$index]}"
            if ! declares_item "$current" "$segment"; then
                gate_fail "${asset#"$root"/} cites \`$path\`, and ${current#"$root"/} declares no \`$segment\` — nor is it a module of it; a citation that resolves to nothing tells a reader nothing about the copy it stands beside"
                break
            fi
            index=$((index + 1))
        done
    done < <(citations_in "$asset")
done < <(gate_find "$assets" \( -name '*.js' -o -name '*.html' -o -name '*.css' \))

# Rule two, over the assets and the crate whose documentation is about them. `crates/web/src` is
# in the walk because that is where half the measured instance was: `lib.rs`'s header names what
# reconciles this client, and a name there that resolves to nothing misleads exactly the reader
# `credential.js`'s does.
while IFS= read -r -d '' cited; do
    while IFS= read -r path; do
        [[ -n "$path" ]] || continue
        paths=$((paths + 1))
        if [[ ! -e "$root/$path" ]]; then
            gate_fail "${cited#"$root"/} cites \`$path\`, and this tree has no such file; a citation that resolves to nothing tells a reader nothing about the copy it stands beside"
        fi
    done < <(paths_in "$cited")
done < <(
    gate_find "$assets" \( -name '*.js' -o -name '*.html' -o -name '*.css' \)
    gate_find "$root/crates/web/src" -name '*.rs'
)

gate_checked "$files" "shipped client modules read for Rust citations"
gate_require_nonzero "$files" "shipped client modules"
gate_checked "$citations" "crate-qualified Rust item paths cited by the shipped client"
gate_require_nonzero "$citations" "crate-qualified Rust item paths cited by the shipped client"
gate_checked "$paths" "repository paths cited by the client and by the crate that serves it"
gate_require_nonzero "$paths" "repository paths cited by the client and by the crate that serves it"
gate_note "$shorthands crate-less shorthand path(s) named in prose were not resolved: this predicate does not guess which crate \`limits::…\`, \`render::…\` or \`RestoreReport::…\` meant"

gate_finish
