#!/usr/bin/env bash
#
# The profile every installed binary is built under is declared in the root manifest, and it
# declares the arithmetic this workspace's tests run under (the G6 review's **L2**, note
# **N225**).
#
# `Cargo.toml` gave four profiles a paragraph each — `dev`, `dev.package."*"`,
# `dev.package.webcam-handler-imaging` (note **N116** is a whole entry about that one) and
# `test` — and declared **no `[profile.release]`**. The README's install line is `cargo install
# --locked --path crates/daemon`, and `cargo install` builds `--release`, so every binary an
# operator runs was built at cargo's defaults: three tables describing how this workspace is
# *developed* and nothing describing how it *ships*.
#
# ## Why the arithmetic in particular
#
# `dev` and `test` both have overflow checks on by default and cargo's release default is off,
# so without a declaration every assertion this suite makes about a number is an assertion about
# a build nobody installs. That is not hypothetical here: `ControlRange::align_down` was measured
# returning a range's **maximum** for its most negative input with checks off, and panicking with
# them on (note **N224**) — one defect wearing two faces depending on who compiled it. This
# project's whole posture is that a wrong number is worse than a stopped one, and a manifest is
# where that gets said once.
#
# ## The three claims, and what each is for
#
# 1. **`[profile.release]` exists and sets `overflow-checks = true`.** The declaration itself,
#    and the value the tests share. A tree that dropped either is back where L2 found it.
# 2. **No workspace member declares a profile of its own.** Cargo reads profiles from the
#    *workspace root* and ignores a `[profile.…]` in a member with a warning nobody reads on a
#    green build. A shipped-behaviour decision written where it does not ship is worse than one
#    not written at all, because the file says it is there.
# 3. **The release profile carves out `"*"` and nothing else.** The split this workspace takes
#    is against *dependencies as a class* — `[profile.release.package."*"]`, measured at 17% of a
#    codec-bound photo and none of it ours. A second carve-out is a different decision: a
#    workspace member taken back out is claim 1 undone one name at a time, and a *named*
#    dependency taken out is a shipped-behaviour choice that N225's measurement does not cover.
#    Either way the answer is the same one — say it in the note first.
#
#    **A whitelist, and that is the repair** (note **N234**). This claim first shipped asking
#    "does the root set `overflow-checks` for member X?", one member name at a time, against the
#    raw bracket text. Two ordinary TOML spellings of the same carve-out walked around it —
#    `[profile.release.package."webcam-handler-imaging"]`, which is the manifest's *own* house
#    style two lines away, and `webcam-handler-imaging = { overflow-checks = false }` written
#    under `[profile.release.package]`. Both were driven against a scratch tree and both left the
#    gate green while `rustc` dropped the flag. Asking instead "who is carved out at all" removes
#    the name-matching that both bypasses exploited, and it is a shorter sentence.
#
# ## Honest limits
#
# This reads the manifest, not a binary: it says what cargo is instructed to do, and a
# `RUSTFLAGS` in somebody's environment can still override it. Nothing available to a gate
# closes that — the measurement that does is in note N225, where `cargo install --locked --path
# crates/daemon -v` was run and its `-C overflow-checks=on` counted against the nine workspace
# crates in the daemon's graph.
#
# **What it will not decode, it fails on rather than passes over.** TOML has more spellings for
# a nested table than a `sed` walk should be trusted with, so the two shapes below are findings
# in their own right, named as such: `package = { … }` written as an inline table under
# `[profile.release]`, and a `release.…` dotted key under a bare `[profile]`. Neither is in this
# manifest, and a gate whose silence has to be earned is the whole of AGENTS rule 3.
#
# Quoting is normalised — `["]` and `[']` are stripped from every table header and key, because
# `[profile."release"]` and `[profile.release]` are the same table to cargo and were not to this
# walk. Package names cannot contain a dot, so a name's own quotes never widen a path.
#
# `#` comments are stripped before any section is recognised, because this file and the
# manifest's own paragraphs both write `[profile.release]` in prose and a walk that counted
# those would be red on the writing about the rule. Quoted `#` inside a value is not handled;
# no profile key in this manifest has one, and a rule that fits in one sentence beats one that
# needs a TOML parser.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"
manifest="$root/Cargo.toml"

if [[ ! -f "$manifest" ]]; then
    gate_fail "there is no Cargo.toml at $root; nothing here can say what the shipped binaries are built under"
    gate_finish
fi

# The value of `$2` in section `$1` of manifest `$3`, or nothing. Comments off first, then a
# tiny section walk: `[name]` opens a table and every `key = value` under it belongs to it.
# Header and key are unquoted before comparison, so `[profile."release"]` answers for
# `profile.release` — TOML says they are one table and this walk used to say they were two.
manifest_value() {
    local section="$1" key="$2" file="$3"
    sed 's/#.*//' "$file" | awk -v want="$section" -v key="$key" '
        function unquote(s,   sq) {
            sq = sprintf("%c", 39)
            gsub(/"/, "", s)
            gsub(sq, "", s)
            gsub(/[[:space:]]/, "", s)
            return s
        }
        /^[[:space:]]*\[/ {
            line = $0
            sub(/^[[:space:]]*\[/, "", line)
            sub(/\].*$/, "", line)
            here = unquote(line)
            next
        }
        here == want && /=/ {
            name = $0
            sub(/[[:space:]]*=.*$/, "", name)
            if (unquote(name) != key) next
            value = $0
            sub(/^[^=]*=[[:space:]]*/, "", value)
            gsub(/[[:space:]]+$/, "", value)
            print value
        }
    '
}

# Every table header in manifest `$1`, comments off and quoting normalised.
manifest_sections() {
    sed 's/#.*//' "$1" |
        sed -n 's/^[[:space:]]*\[\([^]]*\)\].*$/\1/p' |
        tr -d "\"' "
}

# Every package the root manifest's release profile carves out, as `<spelling>\t<name>`,
# whatever TOML shape says it. A shape this walk declines to guess at is emitted with the
# name `!undecodable`, which the caller turns into a finding — see "Honest limits" above.
release_carve_outs() {
    sed 's/#.*//' "$1" | awk '
        function unquote(s,   sq) {
            sq = sprintf("%c", 39)
            gsub(/"/, "", s)
            gsub(sq, "", s)
            gsub(/[[:space:]]/, "", s)
            return s
        }
        function first_segment(s) {
            sub(/\..*$/, "", s)
            return s
        }
        /^[[:space:]]*\[/ {
            line = $0
            sub(/^[[:space:]]*\[/, "", line)
            sub(/\].*$/, "", line)
            here = unquote(line)
            if (here ~ /^profile\.release\.package\./) {
                name = here
                sub(/^profile\.release\.package\./, "", name)
                # A trailing `.build-override` or the like is a sub-table of the same
                # carve-out; the package name is what this claim is about.
                print "a [profile.release.package.<name>] table\t" first_segment(name)
            }
            next
        }
        /=/ {
            key = $0
            sub(/[[:space:]]*=.*$/, "", key)
            key = unquote(key)
            if (key == "") next
            if (here == "profile.release.package") {
                print "a key under [profile.release.package]\t" first_segment(key)
                next
            }
            if (here == "profile.release" && key ~ /^package(\.|$)/) {
                if (key ~ /^package\./) {
                    rest = key
                    sub(/^package\./, "", rest)
                    print "a package.<name> dotted key under [profile.release]\t" first_segment(rest)
                } else {
                    print "an inline `package = { … }` table under [profile.release]\t!undecodable"
                }
                next
            }
            if (here == "profile" && key ~ /^release(\.|$)/) {
                print "a release.… key under [profile]\t!undecodable"
                next
            }
        }
    '
}

# ------------------------------------------------------------------ claim 1

declared=0
if manifest_sections "$manifest" | grep -qx 'profile.release'; then
    declared=1
else
    gate_fail "the root manifest declares no [profile.release]; \`cargo install\` builds --release, so the profile every operator's binary is built under is cargo's default and this repository says nothing about it (note N225)"
fi
gate_checked 1 "root manifest for a declared [profile.release]"

checks="$(manifest_value 'profile.release' 'overflow-checks' "$manifest")"
if ((declared == 1)); then
    if [[ "$checks" != "true" ]]; then
        gate_fail "[profile.release] does not set overflow-checks = true (it says '${checks:-nothing}'); the tests run with overflow checks on and the shipped binaries would not, so every assertion this suite makes about a number would be about a build nobody installs (notes N224, N225)"
    fi
    gate_checked 1 "[profile.release] overflow-checks, against the value the test profile runs at"
fi

# ------------------------------------------------------------------ claims 2 and 3
#
# The member list is derived from `cargo metadata` rather than from the directory listing, for
# `lint-posture.sh`'s reason: a member added to the workspace is one this walk must cover
# without anybody remembering to add it here.

members_checked=0
declare -a member_names=()

while IFS=$'\t' read -r package manifest_path; do
    members_checked=$((members_checked + 1))
    member_names+=("$package")
    rel="${manifest_path#"$root"/}"
    if [[ ! -f "$root/$rel" ]]; then
        gate_fail "$package declares a manifest at $rel, which does not exist in the tree under test"
        continue
    fi

    # Claim 2: a profile table in a member manifest is a decision cargo throws away.
    while IFS= read -r section; do
        case "$section" in
        profile | profile.*)
            gate_fail "$rel declares [$section]; cargo reads profiles from the workspace root and ignores this one, so a shipped-behaviour decision written here does not ship (note N225)"
            ;;
        *) ;;
        esac
    done < <(manifest_sections "$root/$rel")
done < <(gate_metadata | jq -r '
    ( [ .workspace_members[] ] ) as $members
    | .packages[]
    | select(.id as $id | $members | index($id))
    | "\(.name)\t\(.manifest_path)"
')

gate_checked "$members_checked" "workspace member manifests, for a profile table cargo would ignore"
gate_require_nonzero "$members_checked" "workspace member manifests"

# Claim 3: `"*"` is the one carve-out, and every other name is somebody's decision to declare.
carve_outs_checked=0
while IFS=$'\t' read -r spelling name; do
    [[ -n "$name" ]] || continue
    carve_outs_checked=$((carve_outs_checked + 1))
    if [[ "$name" == '!undecodable' ]]; then
        gate_fail "the root manifest carves packages out of [profile.release] through $spelling, which this check will not guess at; write it as [profile.release.package.<name>] so the shipped profile stays one readable fact, and amend note N225 with what it is for"
        continue
    fi
    [[ "$name" == '*' ]] && continue
    for member in "${member_names[@]}"; do
        if [[ "$name" == "$member" ]]; then
            gate_fail "the root manifest carves the workspace member $name out of [profile.release] through $spelling; the release split is against dependencies as a class (\`package.\"*\"\`), and taking one of ours back out is claim 1 undone quietly (notes N224, N225)"
            continue 2
        fi
    done
    gate_fail "the root manifest carves the dependency $name out of [profile.release] through $spelling; N225 measured the split against dependencies as a class and says nothing about one by name, so this is a shipped-behaviour decision with no note behind it"
done < <(release_carve_outs "$manifest")

gate_checked "$carve_outs_checked" "per-package carve-outs of the release profile in the root manifest"

# The population claim 3 rests on is the release profile's *tables*, not its carve-outs: a
# manifest that carves nothing out satisfies the claim honestly and would report zero, while a
# walk that has stopped seeing the profile at all reports zero for a reason that is a defect.
# `gate_require_nonzero` is therefore attached to the tables — and the count also rises with
# each bypass, so it distinguishes "nothing is carved out" from "the walk cannot see what is".
release_tables="$(manifest_sections "$manifest" | grep -c '^profile\.release' || true)"
gate_checked "$release_tables" "release-profile tables in the root manifest"
gate_require_nonzero "$release_tables" "release-profile tables in the root manifest"

gate_finish
