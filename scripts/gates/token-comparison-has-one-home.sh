#!/usr/bin/env bash
#
# The web listener's bearer token is compared in exactly one place, and the type is shaped so
# that there is nowhere else to do it (D11, docs/7 P5a, design §2.10 "one home per law",
# AGENTS.md "Hardware and privacy").
#
# ## This is a mutant that survived, not a box being ticked
#
# `daemon::http::token::Token::verify` is a constant-time comparison: no early exit, no
# data-dependent branch, the differences accumulated with `|=` and the accumulator read once at
# the end. Replace its body with `self.expose_secret() == presented` and **every test in this
# workspace still passes** — the answer is identical for every input, and the only thing that
# changed is how long the daemon takes to say no. `token.rs`'s own doc says exactly that, and
# says it as a limit rather than as a promise:
#
#   > Nothing in this crate's suite can go red on the timing property itself. A `==` here
#   > would pass every test in this module, because what a test can see is the answer and the
#   > claim is about the clock; a timing assertion would be a benchmark pretending to be a
#   > test, and on a shared runner it would be a flake.
#
# AGENTS rule 1 has no exemption for a defect class that is hard to see: "every anticipated or
# discovered defect class becomes a lint, a CI job, or a test that can go red". This is that
# job. It cannot measure the clock, so it holds the **shape** the timing argument stands on:
# the secret is readable in one named place, the type refuses every operator that would
# short-circuit on it, and the one comparison that exists is still the one being called.
#
# ## The four claims
#
#   1. **The secret has one reader.** `Token::expose_secret` is the one rendering that yields
#      the token, and outside the module that defines it the accessor appears only inside
#      `#[cfg(test)]`. The token gate is the module this is chiefly about: a comparison written
#      in `gate.rs` needs the secret in hand, so `expose_secret` in that file's product code is
#      the defect arriving, spelled out, one line before the `==`.
#   2. **The type refuses `==`.** No `PartialEq`, `Eq` or `Hash` on `Token`, derived or
#      hand-written, and no `Display`. `token == candidate` is the same defect wearing a
#      different hat — `str`'s `PartialEq` compares lengths and then bytes and returns at the
#      first difference — and `Display` is the other way a secret reaches a comparison or a log
#      line without anybody typing the word `expose`.
#   3. **`Debug` is hand-written and does not print the field.** A derived `Debug` on `Token`
#      prints the key to a camera into whatever formatted it, which is `crate::logging`'s
#      doctrine about a photograph applied to a secret. Only half of this is the gate's: the
#      derive is, and *what the hand-written impl prints* is asserted by
#      `the_debug_rendering_redacts_the_secret_and_the_named_rendering_yields_it` in `token.rs`,
#      a test that can go red. What the gate adds is the half that test cannot see — a derive
#      would satisfy nothing here and would still have to be caught before it printed.
#   4. **Something still compares.** `verify` is defined in the home and the token gate's own
#      product code calls it. A tree where nothing calls the comparison is a gate with nothing
#      behind it, and every claim above would be true of it — this is
#      `kill-is-never-a-fallback.sh`'s "the only caller went away" arm, about a different
#      absence.
#
# ## What this does **not** claim
#
# **It checks shape, and shape is not timing.** A hand-rolled comparison written *inside*
# `verify` itself — an early `return false` in the loop, a `!=` on the first byte — passes
# every claim above, because from out here it is the same function with the same name being
# called from the same place. Nothing in this suite can go red on that, and pretending
# otherwise would be worse than saying so: what defends it is the argument beside the code
# (`verify`'s doc states the claim, what it excludes, and that the length is deliberately
# public) and the person reading the diff. Rubric rule 2's other half — an assertion that
# cannot go red is worse than an argument that admits it is one — is why that argument is
# written where it is rather than approximated here.
#
# It says nothing about the layers underneath either: the HTTP parser, the router and the
# allocator have timing of their own, and `gate.rs`'s header records that residual where it
# belongs.
#
# ## The matching rule
#
# Line comments are stripped (`sed 's://.*::'`), then the tree is grepped. So **prose does not
# count**, and that is a decision this predicate makes differently from
# `kill-is-never-a-fallback.sh`, which counts it: what defends the timing claim *is* the
# argument beside the code, `token.rs`'s header names the accessor four times in the course of
# making it, and a gate that turned "writing about the secret" into a violation would push that
# argument out of the modules that need it. Block comments and string literals are not
# stripped, for `unsafe-scope.sh`'s reason — the workspace writes `//` comments and a rule that
# fits in one sentence beats one that needs a Rust parser.
#
# **Test code is everything from a file's one `#[cfg(test)]` marker to the end of it**, and that
# rule is `lib.sh`'s (`gate_test_region_start`, `gate_product_lines`) rather than this file's,
# because `web-routes-are-gated.sh` reads a file's product half by the identical rule and two
# copies of it is the pair that stops agreeing. What it costs here is stated there: a file this
# cannot classify is a **failure and not a pass**, and product code written *after* a trailing
# test module would not be seen. The arms in `cases/` seed both shapes.
#
# ## The populations
#
# Derived, not transcribed (docs/9's second structural rule, rubric B11's "a named set is a
# claim about the tree"): the daemon's directory comes from `cargo metadata`, the files to scan
# come from the tree walk, and `Token`'s fields — which claim 3 checks the `Debug` impl does
# not print — are read out of the declaration. What is *policy* rather than fact is named here
# and asserted to exist: the type, the accessor, the comparison, and the two modules. A rename,
# a deletion or a move fails this gate rather than quietly emptying it, which is the whole of
# what stops it going green while checking nothing.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

# The policy this gate is about, named once. Every name below is asserted to exist.
type_name="Token"
accessor="expose_secret"
comparison="verify"
banned_traits=(PartialEq Eq Hash Display)

daemon_dir="$(gate_metadata |
    jq -r '.packages[] | select(.name == "webcam-handler-daemon") | .manifest_path' |
    head -n1 | xargs -r dirname)"

if [[ -z "$daemon_dir" ]]; then
    gate_fail "webcam-handler-daemon is not a workspace member; the token, its comparison and the gate that calls it have no home to be confined to"
    gate_finish
fi

# The metadata may describe a different checkout than the tree under test (the selftest feeds
# mutated copies), so paths are resolved as suffixes of the tree under test.
daemon_suffix="${daemon_dir#"$root"/}"
home_rel="$daemon_suffix/src/http/token.rs"
gate_rel="$daemon_suffix/src/http/gate.rs"
home="$root/$home_rel"
gate_module="$root/$gate_rel"

# ------------------------------------------------------------------ reading the tree
#
# `gate_test_region_start` and `gate_product_lines` are `lib.sh`'s: `web-routes-are-gated.sh`
# needs the identical rule about what a file's test half is, and two copies of that rule is the
# pair that stops agreeing (the same argument `gate_tree_state` carries). The matching rule this
# header states is theirs, stated there as well.

# How many lines of $1 match the ERE $2. Zero is an answer, not a failure.
count_matching() {
    grep -Ec -- "$2" <<<"$1" || true
}

# ------------------------------------------------------------------ the named policy exists
#
# Every claim below is about one of these names. A tree that has renamed or moved any of them
# is a tree this gate would otherwise pass by quantifying over nothing.

anchors=0
for required in "$home" "$gate_module"; do
    anchors=$((anchors + 1))
    if [[ ! -f "$required" ]]; then
        gate_fail "${required#"$root"/} is missing; the token's home and the gate that calls its comparison are what every claim here is about, and this one is not in the tree"
    fi
done

if [[ ! -f "$home" ]]; then
    # Nothing below can say anything without it, and a partial verdict about a missing home
    # would read as a verdict about a comparison.
    gate_checked "$anchors" "module(s) this gate's claims are about"
    gate_finish
fi

home_product_start="$(gate_test_region_start "$home")"
home_product="$(gate_product_lines "$home" "$home_product_start")"

for declaration in \
    "pub struct $type_name\b|the type that holds the secret" \
    "pub fn $accessor\(|the one accessor that yields the secret" \
    "pub fn $comparison\(|the constant-time comparison"; do
    pattern="${declaration%%|*}"
    what="${declaration#*|}"
    anchors=$((anchors + 1))
    if ! grep -Eq -- "$pattern" <<<"$home_product"; then
        gate_fail "$home_rel no longer declares $what (\`$pattern\`); every claim this gate makes is about that name, so a tree without it is a tree this gate has nothing to say about"
    fi
done

gate_checked "$anchors" "named declaration(s) and module(s) asserted to exist before anything is counted"
gate_require_nonzero "$anchors" "named declarations"

# ------------------------------------------------------------------ claim 1: one reader
#
# The whole tree, not just the daemon: a helper crate that grew a reason to hold the secret
# would be the same defect one directory further out.

scanned=0
readers=0
declare -a witnesses=()
banned_alternation="$(
    IFS='|'
    printf '%s' "${banned_traits[*]}"
)"

while IFS= read -r -d '' file; do
    scanned=$((scanned + 1))
    rel="${file#"$root"/}"
    stripped="$(sed 's://.*::' "$file")"

    # ---------------------------------------------------------- claim 2, in the same walk
    #
    # An `impl PartialEq for Token` anywhere in the workspace is `==` on the secret, and the
    # module it lives in makes no difference to that.
    if grep -Eq -- "impl[^;{]*\b($banned_alternation)\b[^;{]*for[[:space:]]+$type_name\b" <<<"$stripped"; then
        for trait_name in "${banned_traits[@]}"; do
            if grep -Eq -- "impl[^;{]*\b$trait_name\b[^;{]*for[[:space:]]+$type_name\b" <<<"$stripped"; then
                gate_fail "$rel implements \`$trait_name\` for \`$type_name\`; a token compared with \`==\` — or rendered by a \`Display\` and compared after — short-circuits at the first differing byte, which is the timing leak \`$comparison\` exists to not have"
            fi
        done
    fi

    grep -Fq -- "$accessor" <<<"$stripped" || continue
    readers=$((readers + 1))

    if [[ "$rel" == "$home_rel" ]]; then
        continue
    fi

    start="$(gate_test_region_start "$file")"
    if ((start < 0)); then
        gate_fail "$rel names \`$accessor\` and this gate cannot tell its product code from its test code: it carries more than one \`#[cfg(test)]\` marker, or its marker does not open a \`mod\`. One trailing test module per file is what makes that readable, and a file nobody can classify is a finding rather than a pass"
        continue
    fi

    product="$(gate_product_lines "$file" "$start")"
    mentions="$(count_matching "$product" "$accessor")"
    if ((mentions > 0)); then
        gate_fail "$rel reads the token's secret outside \`#[cfg(test)]\` ($mentions line(s)); \`$accessor\` is the one rendering that yields it and $home_rel is the one place that may, because a caller holding the secret is one \`==\` away from comparing it itself"
    else
        witnesses+=("$rel")
    fi
done < <(gate_rust_files)

gate_checked "$scanned" "Rust source files scanned for a reader of the token's secret and for a comparison taught to \`$type_name\`"
gate_require_nonzero "$scanned" "Rust source files"
gate_checked "$readers" "file(s) that name \`$accessor\` at all, of which $home_rel is the home"
# The home names it, so this cannot be zero on a tree that has one — which makes a zero here a
# statement about the walk rather than about the tree, and a walk that read nothing is the
# vacuous green everything above is arranged to prevent.
gate_require_nonzero "$readers" "files naming \`$accessor\`"
if ((${#witnesses[@]} > 0)); then
    gate_note "\`$accessor\` outside $home_rel appears only inside \`#[cfg(test)]\`, in: ${witnesses[*]}"
else
    gate_note "no file outside $home_rel names \`$accessor\` at all"
fi

# ------------------------------------------------------------------ claim 2: the derive list
#
# The other half of claim 2 — the walk above catches a hand-written impl, and this catches the
# one-word version of it. The block read is everything between the blank line above the
# declaration and the declaration itself, which is where Rust puts a type's attributes and
# where every item in this workspace is separated from the previous one. Doc comments are
# dropped first: the paragraph above `Token` argues about `Clone` and `Copy` by name, and a
# gate that read prose as a derive list would forbid the argument.

declaration_block() {
    local file="$1" line
    line="$(grep -nE -- "^pub struct $type_name\b" "$file" | head -n1 | cut -d: -f1)"
    [[ -n "$line" ]] || return 1
    awk -v end="$line" '
        NR < end { buf[NR] = $0 }
        END {
            start = 1
            for (i = end - 1; i >= 1; i--) {
                if (buf[i] ~ /^[[:space:]]*$/) { start = i + 1; break }
            }
            for (i = start; i < end; i++) { print buf[i] }
        }
    ' "$file" | grep -v '^[[:space:]]*//' || true
}

derive_claims=0
if attributes="$(declaration_block "$home")"; then
    for trait_name in "${banned_traits[@]}" Debug; do
        derive_claims=$((derive_claims + 1))
        if grep -Eq -- "\b$trait_name\b" <<<"$attributes"; then
            if [[ "$trait_name" == Debug ]]; then
                gate_fail "$home_rel derives \`Debug\` for \`$type_name\`; the derived impl prints the field, and the field is the key to a camera — the hand-written impl below the type is what redacts it"
            else
                gate_fail "$home_rel derives \`$trait_name\` for \`$type_name\`; a derived comparison is the same short-circuiting \`==\` a hand-written one would be"
            fi
        fi
    done
else
    gate_fail "$home_rel has no \`pub struct $type_name\` whose attributes could be read; the derive claim has no subject"
fi

gate_checked "$derive_claims" "trait(s) the \`$type_name\` declaration is checked not to derive"
gate_require_nonzero "$derive_claims" "derive claims"

# ------------------------------------------------------------------ claim 3: Debug redacts
#
# The impl exists and is hand-written, and its body names no field of the struct. The field
# names come out of the declaration rather than being typed here, so a renamed field is still
# a field this refuses to see printed. What the impl prints *instead* is `token.rs`'s own
# test's to assert — see the header.

struct_fields() {
    awk -v type="$type_name" '
        found && /^\}/ { exit }
        found && /^[[:space:]]+(pub[[:space:]]+)?[A-Za-z_][A-Za-z0-9_]*[[:space:]]*:/ {
            sub(/^[[:space:]]+(pub[[:space:]]+)?/, "")
            sub(/[[:space:]]*:.*/, "")
            print
        }
        $0 ~ ("^pub struct " type "([ ({<]|$)") { found = 1 }
    ' "$1"
}

debug_impl_body() {
    awk -v type="$type_name" '
        !inside && $0 ~ ("^impl[^;{]*Debug[^;{]*for[[:space:]]+" type "([ {]|$)") { inside = 1 }
        inside { print }
        inside && /^\}/ { exit }
    ' "$1"
}

mapfile -t token_fields < <(struct_fields "$home")
gate_checked "${#token_fields[@]}" "field(s) of \`$type_name\` read out of its declaration"
# A type with no fields is not the type carrying this secret, and the claim below would have
# nothing to look for in the impl it is reading.
gate_require_nonzero "${#token_fields[@]}" "fields of \`$type_name\`"

debug_body="$(debug_impl_body "$home")"
if [[ -z "$debug_body" ]]; then
    gate_fail "$home_rel has no hand-written \`impl … Debug for $type_name\`; the workspace lints \`missing_debug_implementations\`, so a type without one does not build — which means the impl that is missing here has become a derive, or has moved somewhere this gate is not looking"
else
    debug_claims=0
    for field in "${token_fields[@]}"; do
        debug_claims=$((debug_claims + 1))
        if grep -Eq -- "self\.$field\b" <<<"$debug_body"; then
            gate_fail "the hand-written \`Debug\` for \`$type_name\` in $home_rel names \`self.$field\`; a \`Debug\` that reaches a field of this type is a \`tracing::debug!(?token)\` away from printing the key to a camera into the journal"
        fi
    done
    debug_claims=$((debug_claims + 1))
    if grep -Fq -- "$accessor" <<<"$debug_body"; then
        gate_fail "the hand-written \`Debug\` for \`$type_name\` in $home_rel calls \`$accessor\`; the redaction is the whole point of it being hand-written"
    fi
    gate_checked "$debug_claims" "claim(s) about what the hand-written \`Debug\` may name"
fi

# ------------------------------------------------------------------ claim 4: something compares
#
# Every claim above is true of a tree where the token gate stopped checking the token, and that
# tree is a worse defect than any of them. `kill-is-never-a-fallback.sh`'s "the only caller went
# away" arm is the model: the confinement is only worth having while there is something behind
# it.

if [[ -f "$gate_module" ]]; then
    gate_product_start="$(gate_test_region_start "$gate_module")"
    if ((gate_product_start < 0)); then
        gate_fail "$gate_rel does not carry the one trailing \`#[cfg(test)] mod\` this gate reads a file's product half by; the call count below would be a count of nothing in particular"
    else
        gate_product="$(gate_product_lines "$gate_module" "$gate_product_start")"
        calls="$(count_matching "$gate_product" "\.$comparison\(")"
        if ((calls == 0)); then
            gate_fail "nothing in $gate_rel's product code calls \`.$comparison(\`; the token gate is the one caller of the constant-time comparison, and a gate that stopped calling it is a listener admitting requests on some other basis — every other claim here is true of that tree"
        fi
        gate_checked "$calls" "call(s) to \`$type_name::$comparison\` in $gate_rel's product code"
    fi
fi

gate_finish
