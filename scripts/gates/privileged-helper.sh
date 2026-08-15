#!/usr/bin/env bash
#
# The blessed helper is root. This gate is what keeps that contained (notes N8, **N125**).
#
# `CAP_SYS_MODULE` is root: a process that can load a kernel module can do anything a kernel
# can do. So a blessed copy is a root escalation for whoever can execute it, and no arrangement
# of verbs changes that. What P6e's reckoning changed is **what a blessed copy will do for the
# person who runs it**, and that turned one of this gate's paragraphs from an accepted risk into
# a checkable claim.
#
# Until P6e the header here said there was "no meaningful *capability* boundary around this
# binary" — true, because `webcam-handler-priv exec` handed `CAP_SYS_MODULE` to any program its
# caller named, and `exec /bin/sh` was a root shell somebody had agreed to (note N8: the owner
# chose the wrapper over a closed verb vocabulary, on the argument that only a wrapper can put a
# capability inside a *test process*). G6 was the recorded trigger to check whether that argument
# was ever spent. It was not — nothing in this workspace ever invoked the verb — so the verb is
# gone and the sentence it forced out of this file comes back as claim 5.
#
# Six claims, and claims 5 and 6 are the ones the narrowing bought:
#
#   1. **A blessed copy is never committed.** `.wch-bin/` must be gitignored, and no file
#      under it may be tracked. Git does not preserve xattrs, so a committed copy would be
#      an un-capped binary that *looks* blessed — and a transport that did preserve them
#      would be shipping root to whoever cloned the repository.
#   2. **A blessed copy is owner-only.** If one exists, it is mode 0700. This is still the
#      security boundary (`main.rs` says so in as many words), and `just bless` sets it
#      *before* `setcap` — but a restore, an rsync, or a `chmod -R` can widen it later, and
#      nothing else would notice.
#   3. **Nothing ships it.** No product crate may depend on `webcam-handler-priv`, so the
#      helper cannot reach a shipped binary's link graph by accident. Derived from
#      `cargo metadata`, not from a list.
#   4. **It stays free of `unsafe`.** `unsafe-scope.sh` already asserts every crate root
#      but the V4L2 backend carries `#![forbid(unsafe_code)]`; this re-states it for the
#      one crate where the consequence is different in kind, so a future edit that reaches
#      for `libc` has to argue with two gates instead of one.
#   5. **It cannot hand its capabilities to a program its caller names.** The helper's
#      *product* source may not spell the four things a program runner needs and cannot work
#      without: clap's `trailing_var_arg` and `allow_hyphen_values` (an argv arrives full of
#      other programs' flags), and `CommandExt` / `.exec(`, the std trait whose only purpose
#      is replacing this process with another one. `crates/priv/src/main.rs`'s own
#      `no_verb_hands_this_binarys_capabilities_to_a_program_the_caller_names` holds the same
#      claim over the clap tree; this holds it over the text, which is where a second route
#      that never reached clap at all — reading `std::env::args()` and exec'ing — would be.
#      Prose does not count, for `lib.sh`'s reason and acutely here: this file, `main.rs` and
#      note N125 all argue about the deleted verb *by name*.
#   6. **Every capability-carrying file under `.wch-bin/` is the one blessed helper, and it
#      carries exactly the blessing this tree declares.** Two defects in one walk, and neither
#      is visible to any test, because both are facts about a developer's filesystem rather
#      than about the code. A copy carrying **more** than `caps.rs` declares is privilege
#      nobody argued for — the whole class N8 exists about — and the widening need not be
#      malicious: it is what a tree blessed before the narrowing looks like the morning after.
#      A capability-carrying file under `.wch-bin/` that is **not** the helper is worse, and it
#      is not hypothetical: note **N126** records the `wch-priv` that the N90 rename orphaned,
#      still mode 0700, still root-capable, still carrying the `exec` verb — a working bypass
#      of a narrowing, sitting beside the narrowed binary, that `just bless` would not touch
#      and that a gate looking only at the name it expected could not see.
#
# What it still cannot check, recorded rather than implied: that the *owner* is trustworthy,
# that no second session is logged in as them, and that a module load is anything other than
# arbitrary code in ring 0. Narrowing the verbs narrowed who can ask, never what a yes costs.
# And claim 6 reads **capabilities**, which is what `just bless` grants and therefore the whole
# of what this project puts there — a setuid bit is a different mechanism and `getcap` is blind
# to it, so a `chmod u+s` under `.wch-bin/` is outside this predicate's sentence. Nothing here
# creates one; the limit is written down rather than left for somebody to assume otherwise.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

# The package, and where its blessed copy goes. Both read from the tree so a rename moves
# the gate with them.
package="webcam-handler-priv"
blessed_dir=".wch-bin"

package_dir="$(gate_metadata |
    jq -r --arg name "$package" \
        '.packages[] | select(.name == $name) | .manifest_path' |
    head -n1 | xargs -r dirname)"

if [[ -z "$package_dir" ]]; then
    gate_fail "$package is not a workspace member; this gate has no subject"
    gate_finish
fi

# ------------------------------------------------------------------ claim 1

checked=0
if [[ ! -f "$root/.gitignore" ]]; then
    gate_fail "no .gitignore; the blessed helper's directory cannot be excluded"
elif ! grep -Eq "^/?${blessed_dir}/?$" "$root/.gitignore"; then
    gate_fail "$blessed_dir/ is not in .gitignore; a capability-carrying binary must never be committable"
else
    checked=$((checked + 1))
fi

# The stronger half: nothing under it is actually tracked. A `.gitignore` entry does not
# retroactively untrack a file somebody added with `git add -f`.
if [[ -d "$root/.git" ]]; then
    tracked="$(git -C "$root" ls-files -- "$blessed_dir" | wc -l)"
    if ((tracked > 0)); then
        gate_fail "$tracked file(s) under $blessed_dir/ are tracked by git"
    fi
    checked=$((checked + 1))
else
    gate_skip 1 "not a git checkout, so tracked-file status cannot be read"
fi
gate_checked "$checked" "containment claim(s) about $blessed_dir/"

# ------------------------------------------------------------------ claim 2

blessed="$root/$blessed_dir/webcam-handler-priv"
if [[ -e "$blessed" ]]; then
    mode="$(stat -c %a "$blessed")"
    owner="$(stat -c %U "$blessed")"
    if [[ "$mode" != "700" ]]; then
        gate_fail "$blessed_dir/webcam-handler-priv is mode $mode; a root-capable binary must be 0700 (owner only) — re-run \`just bless\`"
    fi
    if [[ "$owner" != "$(id -un)" ]]; then
        gate_fail "$blessed_dir/webcam-handler-priv is owned by $owner, not $(id -un)"
    fi
    gate_checked 1 "blessed helper checked for owner-only mode"
    gate_note "blessed copy present: mode $mode, owner $owner"
else
    # Legitimate and common: CI never blesses, and a fresh checkout has not either.
    gate_skip 1 "no blessed helper at $blessed_dir/webcam-handler-priv; nothing to check its mode on"
fi

# ------------------------------------------------------------------ claim 3

dependents="$(gate_metadata | jq -r --arg name "$package" '
    ( [ .packages[] | select(.name == $name) | .id ] | first ) as $priv
    | .resolve.nodes[]
    | select(any(.deps[]; .pkg == $priv))
    | .id
')"

if [[ -n "$dependents" ]]; then
    while IFS= read -r id; do
        [[ -n "$id" ]] || continue
        gate_fail "$id depends on $package; the privileged helper must reach no shipped link graph"
    done <<<"$dependents"
fi
gate_checked 1 "dependency-graph check that nothing links $package"

# ------------------------------------------------------------------ claim 4

crate_root="$(gate_metadata | jq -r --arg name "$package" '
    .packages[] | select(.name == $name) | .targets[]
    | select(any(.kind[]; . == "bin")) | .src_path' | head -n1)"
rel="${crate_root#"$root"/}"

if [[ ! -f "$root/$rel" ]]; then
    gate_fail "$package declares a crate root at $rel, which is not in the tree under test"
elif ! grep -Eq '^#!\[forbid\(unsafe_code\)\]' "$root/$rel"; then
    gate_fail "$rel does not carry #![forbid(unsafe_code)]; a root-capable binary is the last place to hand-roll a pointer cast"
fi
gate_checked 1 "crate root checked for #![forbid(unsafe_code)]"

# ------------------------------------------------------------------ claim 5

# The module directory, derived from where cargo says the crate root is rather than written
# down, so a crate laid out differently moves this with it.
src_rel="$(dirname "$rel")"

# The four spellings a program runner needs. Each is here because the deleted verb used it:
# `trailing_var_arg` and `allow_hyphen_values` are what let an argv survive the parser intact,
# and `CommandExt` is the import that made `.exec(` available at all.
runner_spellings=(trailing_var_arg allow_hyphen_values CommandExt '.exec(')

examined=0
while IFS= read -r -d '' file; do
    start="$(gate_test_region_start "$file")"
    if ((start == -1)); then
        # The same price `unsafe-scope.sh` charges: a file whose product half cannot be told
        # from its test half is a file where this claim has no answer, and no answer is not a
        # pass.
        gate_fail "${file#"$root"/} has no readable test-module boundary, so its product code cannot be told from its tests"
        continue
    fi
    examined=$((examined + 1))
    product="$(gate_product_lines "$file" "$start")"
    for spelling in "${runner_spellings[@]}"; do
        if grep -Fq -- "$spelling" <<<"$product"; then
            gate_fail "${file#"$root"/} names \`$spelling\` in product code; that is how a verb hands this binary's capabilities to a program its caller chose (notes N8, N125)"
        fi
    done
done < <(gate_find "$root/$src_rel" -name '*.rs')

gate_require_nonzero "$examined" "source files under $src_rel/ — a walk that found none would be green having read nothing"
gate_checked "$examined" "helper source file(s) checked for a caller-named program"

# ------------------------------------------------------------------ claim 6

# The blessing has one home and it is the crate, not this script: `caps.rs` declares it,
# `webcam-handler-priv doctor --setcap-argument` prints it, `just bless` passes it to `setcap`,
# and this reads it back out of the source so a narrowing lands in one place. A second copy
# here is the thing that would stop agreeing.
mapfile -t blessing_lines < <(grep -rhs 'const BLESSING' "$root/$src_rel" 2>/dev/null || true)
if ((${#blessing_lines[@]} != 1)); then
    gate_fail "${#blessing_lines[@]} declaration(s) of BLESSING under $src_rel/; the capability list has one home or it has none"
    gate_finish
fi
declared="$(sed -n 's/.*"\([^"]*\)".*/\1/p' <<<"${blessing_lines[0]}")"
if [[ "$declared" != *+* ]]; then
    gate_fail "BLESSING reads '$declared', which is not a setcap argument (<capabilities>+<flags>)"
    gate_finish
fi
want_caps="$(tr ',' '\n' <<<"${declared%%+*}" | sort | paste -sd, -)"
want_flags="${declared##*+}"
gate_note "this tree declares the blessing $declared"

# A seam, for the reason `oracle-rung-accounting.sh` has one: capabilities cannot be *set*
# without root, so a case file cannot seed the violation on a real file. It hands this a
# recorded listing instead, and `pass_case` leaves it unset so one arm still runs the real
# tool (rubric rule 6, paid for by note N10).
getcap_program="${WCH_GATE_GETCAP:-getcap}"

if [[ ! -d "$root/$blessed_dir" ]]; then
    gate_skip 1 "no $blessed_dir/ in this tree, so there are no blessed capabilities to read"
elif ! command -v "$getcap_program" >/dev/null 2>&1; then
    gate_skip 1 "$getcap_program is not installed (libcap2-bin provides it), so what $blessed_dir/ actually carries cannot be read"
else
    listing="$("$getcap_program" -r "$root/$blessed_dir" 2>/dev/null || true)"
    carriers=0
    while IFS= read -r line; do
        [[ -n "$line" ]] || continue
        carriers=$((carriers + 1))
        carrier_path="${line%% *}"
        carried="${line##* }"
        if [[ "$carrier_path" != "$root/$blessed_dir/webcam-handler-priv" ]]; then
            gate_fail "$carrier_path carries capabilities ($carried) and is not the blessed helper; delete it — nothing re-blesses it, nothing checks it, and it outlives every narrowing (note N126)"
            continue
        fi
        carried_caps="$(tr ',' '\n' <<<"${carried%%=*}" | sort | paste -sd, -)"
        carried_flags="${carried##*=}"
        if [[ "$carried_caps" != "$want_caps" || "$carried_flags" != "$want_flags" ]]; then
            gate_fail "$blessed_dir/webcam-handler-priv carries $carried_caps=$carried_flags where this tree declares $want_caps=$want_flags; re-run \`just bless\` (a grant wider than the code asks for is privilege nobody argued for)"
        fi
    done <<<"$listing"

    if ((carriers == 0)); then
        # Legitimate and common: CI never blesses, and a fresh checkout has not either.
        gate_skip 1 "nothing under $blessed_dir/ carries capabilities, so this host has no blessed helper to compare against $declared"
    else
        gate_checked "$carriers" "capability-carrying file(s) under $blessed_dir/ checked against the declared blessing"
    fi
fi

gate_finish
