#!/usr/bin/env bash
#
# The R1-web rung's browser pins are one fact, and this predicate is the reconciler that fact
# did not have.
#
# **The fact is `crates/daemon/tests/browser/package.json`**, and the direction is not
# arbitrary. That manifest is what `npm ci` installs, what the Playwright CLI reads when `just
# rung-web-install` fetches a browser, and what `pins.spec.mjs` compares the *launched* binary
# against — so it is the only copy with a mechanism behind it. Every other statement of the same
# three numbers describes it. When they disagree the manifest is right and the description is
# stale, which is why every message below names the file to edit rather than merely reporting a
# difference.
#
# ## The defect this closes, and who created it
#
# The pins were `pins.spec.mjs`'s business alone until the README grew a "The pin is threefold"
# sentence naming `@playwright/test` 1.62.1, Chromium build 1234 and Chrome 151.0.7922.34. That
# is a second home for one law (AGENTS' "one home per law"), and it was created in the same
# session that wrote this file: a version bump touches the manifest and the lockfile, `npm ci`
# and `pins.spec.mjs` both agree with the new number, every test in the workspace stays green,
# and the README quietly starts lying about which browser this project's evidence came from.
# Worse than a stale sentence, because the README is where somebody installing the rung reads
# what they are about to fetch. AGENTS rule 1: the defect class becomes something that can go
# red.
#
# There is a second hole and it is the one a test could not have closed. `pins.spec.mjs` is the
# assertion that the pin has no range operator in it — `^1.62.1` silently becomes 1.63 on the
# next install — and that file only runs where the host has node. On a host without node the
# rung declines by name (which is correct, and design §3.1 wants it that way), and with it the
# exactness claim goes away entirely. This predicate needs neither node nor a browser, so it
# holds that claim on every host and in upstream CI, where the rung has never once run.
#
# ## What it deliberately does not look at
#
# **Only the README.** These three numbers also appear in `docs/implementation-notes.md`
# (evidence **E16**'s transcripts) and in a `phase-criteria.tsv` `what` column, and both are
# *records of a run on a date* — "the rung ran on this host at P5d: 10 of 10 green in Chromium
# 151.0.7922.34, build 1234". A reconciler that swept the tree for the current values would
# demand that history be rewritten every time the pin moves, which is the opposite of what an
# append-only evidence register is for. A claim about what the rung installs and a record of
# what it once drove are different sentences that happen to contain the same number.
#
# ## The population is derived, and an unknown pin is a failure
#
# The pins come from the manifest — every `devDependencies` entry and every `wch` entry — and
# not from a list in this file. A pin the manifest declares that this predicate has no README
# sentence for is **red**, not silently skipped: a fourth pin added to the manifest is a fourth
# thing a reader of the README is entitled to know, and letting it pass would make "the README
# states the pins" quietly mean "the README states the pins somebody remembered".
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"
manifest_path="crates/daemon/tests/browser/package.json"
lockfile_path="crates/daemon/tests/browser/package-lock.json"
manifest="$root/$manifest_path"
lockfile="$root/$lockfile_path"
readme="$root/README.md"

# A backtick, as a variable, so the patterns below can be built inside double quotes without
# every one of them opening a command substitution.
tick='`'

for required in "$manifest" "$readme"; do
    if [[ ! -f "$required" ]]; then
        gate_fail "no ${required#"$root"/}; the browser pins have no home to reconcile against"
        gate_finish
    fi
done

if ! jq -e . "$manifest" >/dev/null 2>&1; then
    gate_fail "$manifest_path is not readable JSON; the fact this gate compares everything to cannot be read"
    gate_finish
fi

# ------------------------------------------------------------------ the fact, and its sentences

# The words README.md introduces one pin with, keyed by where the manifest keeps it.
#
# Two spellings, tab-separated: the name a failure message calls the pin, and the pattern that
# finds it in the prose — which for a package is its own name in backticks, because
# `@playwright/test` appears in this README as a package name and nowhere as a phrase.
#
# The fallback arm carries no default on purpose (AGENTS rule 6's shape, applied to a
# vocabulary this repository owns): the caller turns a `1` here into a named failure, because
# guessing a label for a pin nobody has written a sentence for would be this gate agreeing with
# a README that says nothing.
readme_label() {
    case "$1" in
    'dep:@playwright/test') printf '%s\t%s' '@playwright/test' "${tick}@playwright/test${tick}" ;;
    'wch:chromiumBuild') printf '%s\t%s' 'Chromium build' 'Chromium build' ;;
    'wch:chromiumVersion') printf '%s\t%s' 'Chrome version' 'Chrome version' ;;
    *) return 1 ;;
    esac
}

pins=0
readme_claims=0
exactness_claims=0

while IFS=$'\t' read -r kind key declared; do
    pins=$((pins + 1))
    if ! labels="$(readme_label "$kind:$key")"; then
        gate_fail "$manifest_path declares the pin '$key', which this predicate has no README sentence for; add the sentence to README.md and the mapping to browser-pins-sync.sh, or a reader is told about some of the pins and not this one"
        continue
    fi
    IFS=$'\t' read -r name pattern <<<"$labels"

    # An exact version, in the manifest, for every dependency: `^1.62.1` is a pin that becomes
    # 1.63 on the next install, and `pins.spec.mjs` — the only other place this is checked —
    # does not run on a host without node.
    if [[ "$kind" == "dep" ]]; then
        exactness_claims=$((exactness_claims + 1))
        if [[ ! "$declared" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
            gate_fail "$manifest_path pins $key as '$declared', which is not an exact version; a range operator is a pin that moves on the next install"
        fi
    fi

    # The README, read as one whitespace-normalized string rather than line by line: the
    # sentence that carries these values wraps, and where a paragraph happens to break is not
    # something a version check may have an opinion about.
    stated=0
    while IFS= read -r match; do
        [[ -n "$match" ]] || continue
        stated=$((stated + 1))
        readme_claims=$((readme_claims + 1))
        value="$(printf '%s\n' "$match" |
            grep -oE "${tick}[0-9]+([.][0-9]+)*${tick}" | tail -n 1 | tr -d "${tick}")"
        if [[ "$value" != "$declared" ]]; then
            gate_fail "README.md says $name is $value; $manifest_path pins $declared — the manifest is what the rung installs, so README.md is the file to edit"
        fi
    done < <(tr '\n' ' ' <"$readme" | tr -s ' ' |
        grep -oE "${pattern}[^${tick}]{0,40}${tick}[0-9]+([.][0-9]+)*${tick}" || true)

    if ((stated == 0)); then
        gate_fail "README.md names no version for $name; a pin a reader is no longer told about is the silent half of this defect, and it must not pass"
    fi
done < <(jq -r '
    ((.devDependencies // {}) | to_entries[] | "dep\t\(.key)\t\(.value)"),
    ((.wch // {}) | to_entries[] | "wch\t\(.key)\t\(.value)")
' "$manifest")

gate_checked "$pins" "pinned versions declared by the browser rung's manifest"
gate_require_nonzero "$pins" "pins in $manifest_path"
gate_checked "$readme_claims" "version claims README.md makes about those pins"
gate_require_nonzero "$readme_claims" "README.md claims about the browser pins"
gate_checked "$exactness_claims" "dependency pins checked for a range operator"
gate_require_nonzero "$exactness_claims" "dependency pins"

# ------------------------------------------------------------------ the sentence's own address

# The README describes the pins as "declared in <manifest>". If that path stops being where the
# manifest is, the sentence is a citation of nothing and the numbers beside it are unverifiable
# by the reader they exist for.
address_claims=1
if ! grep -Fq "$manifest_path" "$readme"; then
    gate_fail "README.md never names $manifest_path, so its account of the pins cites no file a reader can open"
fi
gate_checked "$address_claims" "README references to the manifest's own path"

# ------------------------------------------------------------------ the lockfile

# The third copy. `npm ci` refuses a lockfile that disagrees with its manifest — but only on a
# host that has npm, which upstream CI does not, so the disagreement would arrive as a failed
# install on somebody's laptop rather than as a red gate. Both statements of the version are
# compared: the root package's dependency spec, and the resolved `version` of the installed
# package, which is only required to equal the spec because the exactness claim above holds.
lock_claims=0
if [[ ! -f "$lockfile" ]]; then
    gate_skip 1 "no $lockfile_path, so the installed tree's own statement of the pins was not compared"
elif ! jq -e . "$lockfile" >/dev/null 2>&1; then
    gate_fail "$lockfile_path is not readable JSON; the tree npm would install cannot be compared with the manifest"
else
    while IFS=$'\t' read -r key declared; do
        for query in ".packages[\"\"].devDependencies[\"$key\"]" ".packages[\"node_modules/$key\"].version"; do
            stated="$(jq -r "$query // \"absent\"" "$lockfile")"
            lock_claims=$((lock_claims + 1))
            if [[ "$stated" != "$declared" ]]; then
                gate_fail "$lockfile_path states $key as '$stated' at $query; $manifest_path pins $declared — run npm ci under crates/daemon/tests/browser/ rather than editing either by hand"
            fi
        done
    done < <(jq -r '(.devDependencies // {}) | to_entries[] | "\(.key)\t\(.value)"' "$manifest")
    gate_checked "$lock_claims" "lockfile statements of a pinned dependency"
    gate_require_nonzero "$lock_claims" "lockfile statements of a pinned dependency"
fi

gate_finish
