#!/usr/bin/env bash
#
# The blessed helper is root. This gate is what keeps that contained (note N8).
#
# `wch-priv exec` grants `CAP_SYS_MODULE` to any program, and a process that can load a
# kernel module can do anything a kernel can do. There is therefore no meaningful
# *capability* boundary around this binary — the boundary is entirely about **who can
# execute it** and **what links it**. Those are exactly the two things a gate can check,
# so it checks them:
#
#   1. **A blessed copy is never committed.** `.wch-bin/` must be gitignored, and no file
#      under it may be tracked. Git does not preserve xattrs, so a committed copy would be
#      an un-capped binary that *looks* blessed — and a transport that did preserve them
#      would be shipping root to whoever cloned the repository.
#   2. **A blessed copy is owner-only.** If one exists, it is mode 0700. This is the whole
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
#
# What it cannot check, recorded rather than implied: that the *owner* is trustworthy,
# that no second session is logged in as them, and that nobody has already run
# `wch-priv exec /bin/sh`. Those were accepted when the exec-wrapper shape was chosen.
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

blessed="$root/$blessed_dir/wch-priv"
if [[ -e "$blessed" ]]; then
    mode="$(stat -c %a "$blessed")"
    owner="$(stat -c %U "$blessed")"
    if [[ "$mode" != "700" ]]; then
        gate_fail "$blessed_dir/wch-priv is mode $mode; a root-capable binary must be 0700 (owner only) — re-run \`just bless\`"
    fi
    if [[ "$owner" != "$(id -un)" ]]; then
        gate_fail "$blessed_dir/wch-priv is owned by $owner, not $(id -un)"
    fi
    gate_checked 1 "blessed helper checked for owner-only mode"
    gate_note "blessed copy present: mode $mode, owner $owner"
else
    # Legitimate and common: CI never blesses, and a fresh checkout has not either.
    gate_skip 1 "no blessed helper at $blessed_dir/wch-priv; nothing to check its mode on"
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

gate_finish
