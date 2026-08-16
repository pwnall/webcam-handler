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
# ## The five claims
#
#   1. **The secret has one reader.** `Token::expose_secret` is the one rendering that yields
#      the token, and outside the module that defines it the accessor appears only inside
#      `#[cfg(test)]`. The token gate is the module this is chiefly about: a comparison written
#      in `gate.rs` needs the secret in hand, so `expose_secret` in that file's product code is
#      the defect arriving, spelled out, one line before the `==`.
#   2. **The type refuses `==`, and every operator that would perform one for it.** No
#      `PartialEq`, `Eq` or `Hash` on `Token`, derived or hand-written, and no `Display`.
#      `token == candidate` is the same defect wearing a different hat — `str`'s `PartialEq`
#      compares lengths and then bytes and returns at the first difference — and `Display` is
#      the other way a secret reaches a comparison or a log line without anybody typing the word
#      `expose`. **The P5 review widened this to the conversion family**, because four names
#      closed four doors and left the corridor open: `impl AsRef<str> for Token` and one
#      `token.as_ref() == candidate` in `gate.rs` was measured to leave every claim here green,
#      every one of this gate's arms green, and the timing leak live on the credential form that
#      rides the URL. So `AsRef`, `Borrow`, `Deref`, `ToString`, `Into` and `Serialize` join the
#      list — each one a way to obtain a `&str`, a `String` or a wire rendering from a `Token`
#      without the word this project greps for — and `From<Token>` joins it from the other
#      direction, since a conversion *out of* the type is written `for` something else and no
#      `for Token` rule would ever see it.
#
#      `PartialOrd` and `Ord` are deliberately **not** on either list, and the reason is that
#      they cannot be reached: both have `PartialEq` as a supertrait, so an ordering on `Token`
#      does not compile without the equality this already refuses. A name added here for
#      tidiness would be an arm that can only be seeded with code no compiler accepts, which is
#      a weaker arm than none.
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
#   5. **Inside the home, the field has exactly the readers this gate names.** Claims 1 and 2
#      are about the rest of the workspace and about the type's public operators; neither of
#      them can see a *second inherent accessor* written in `token.rs` itself. `pub fn
#      as_str(&self) -> &str { &self.hex }` is not a banned trait, is not `expose_secret`, and
#      hands the secret to `gate.rs` with claim 1 counting zero readers — the accessor
#      confinement's whole subject renamed out from under it. So the readers of `self.<field>`
#      in the home's product code are reconciled against the register this gate already keeps:
#      the accessor and the comparison, the same two names claims 1 and 4 are about.
#
#      Reconciled in **both** directions, `unsafe-scope.sh`'s residual-register shape: a reader
#      with no register entry is a new way out of the type, and a register entry that has
#      stopped reading the field is a `verify` comparing something other than the secret or an
#      `expose_secret` yielding something other than it. Either alone would leave the other half
#      of the pair a decoration. The field names come out of the declaration (`struct_fields`
#      below), never typed here, so a renamed field is still the field this refuses to see read
#      twice.
#
#      It quantifies over **every** field of the type and over a field access on **anything**,
#      both deliberately. A `Token` that grew a second, innocent field would be a type doing two
#      jobs — the one thing this whole file is about is that there is exactly one secret with
#      exactly one way out — and the claim would rather be told about that than assume it. And a
#      free `fn secret_of(token: &Token) -> &str { &token.hex }` beside the type is the same
#      second way out with `self` spelled differently, so the match is on the access and not on
#      the receiver.
#   6. **The *other* rendering that yields the secret has a register of its own.** Claims 1 and
#      5 are about `expose_secret` and about the field; `Token::ready_to_open_url` reaches the
#      same string by a third name. It is the URL D11 asks the daemon to print, the token rides
#      its query string because a navigation cannot be told to send a header, and until G6 every
#      claim in this file was true of a tree where anything at all called it (finding L13, note
#      **N183**). A caller holding that `String` is holding the secret with no `expose` in the
#      diff — one `split` away from comparing it, one `tracing::info!` away from a sink that
#      keeps it, which is the pair of defects G6 found together.
#
#      It cannot be confined the way the accessor is, because D11 *requires* it to be printed:
#      the composition root asks for it, and the listener that holds both halves — the bound
#      address and the token that was actually installed — is what renders it. So the claim is
#      claim 5's residual register applied to a call site instead of to a field, reconciled in
#      both directions against a named list with a reason per entry: the home that declares it,
#      the one delegate, and the one line that writes it down on purpose. A fourth product-code
#      caller is a finding; an entry that has stopped calling it is a finding too, because a
#      register with nothing behind it is how a claim comes to quantify over nothing.
#
#      Prose does not count here either (see the matching rule), which is what lets six modules
#      argue about this URL by name without becoming call sites — and a suite may call it
#      freely, because a test that drives the web listener has to open its URL.
#
#      **One entry carries a second requirement, and it is the only thing standing behind the
#      journal redaction** (note **N185**). The composition root's line is the one that hands the
#      URL to `tracing`, and under systemd `tracing` is a persistent store readable by two groups
#      of accounts — which is the defect note **N182** closed by routing that one rendering
#      through `daemon::logging::Sink::url_this_sink_may_keep`. That repair is **one line**, its
#      only production call site is `main.rs`, and `main.rs` cannot be driven from an integration
#      test: it is the binary beside the library, so `main::run` is not a name any suite can
#      call, and `logging`'s and `token.rs`'s tests both drive the redaction *directly*. Reverting
#      that line to `url = %web.ready_to_open_url()` therefore left every test in this workspace
#      green and left this claim green too, because the register asked only that `main.rs` **name**
#      the rendering — which the un-redacted line does. So an entry may name a spelling that has
#      to appear on the **same product line** as the rendering, and the composition root's does:
#      a URL written down on purpose is written down through the sink that decides how much of it
#      to keep.
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
# The second rendering that yields the secret: D11's ready-to-open URL, with the token in its
# query string. Claim 6 is about who may call it.
url_rendering="ready_to_open_url"
# Traits nothing may implement **for** `Token`: four that compare or print it, and six that
# convert it into something a `==` accepts. The header argues each family and says why the
# ordering traits are not here.
banned_traits=(PartialEq Eq Hash Display AsRef Borrow Deref ToString Into Serialize)
# Traits nothing may implement **taking** `Token`, whatever they are implemented for. A
# conversion out of the type is written `for String`, so no rule spelled `for Token` can see it.
banned_conversions=(From TryFrom PartialEq)
# The two functions that may read the field inside the home, which are the two names claims 1
# and 4 are already about. Derived from the policy above rather than written a second time.
field_readers_allowed=("$accessor" "$comparison")

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

# The redaction the composition root's line has to go through (note **N182**): under systemd
# `tracing` is a persistent, `systemd-journal`/`adm`-readable store, and this is the one call
# that decides how much of D11's URL such a store may keep. Named here beside the register that
# requires it, because it is a *policy* name like every other in this block.
url_redaction="url_this_sink_may_keep"

# Claim 6's register: the product-code files that may name the URL rendering, each with the
# reason it is there and, where one applies, a spelling that must appear on the **same line**.
# Three entries and no fourth, because D11's URL has exactly three jobs — being built, being
# handed to the composition root by the value that holds both of its halves, and being written
# down once.
#
# The third field is what stops the third job being done into a store nobody meant to write to.
# `main.rs` renders the URL into a `tracing` event, so the rendering and the redaction are one
# expression there; a line that names the first and not the second is the un-redacted build, and
# it is invisible to every test in this workspace because the composition root is a binary no
# suite can call into (the header's claim 6 argues both halves). An empty third field means the
# entry is registered on its file alone.
url_sites_allowed=(
    "$home_rel|the home, which builds it|"
    "$daemon_suffix/src/http/listener.rs|the one delegate, which holds the bound address and the installed token|"
    "$daemon_suffix/src/main.rs|the one line D11 asks for, which writes it down on purpose|$url_redaction"
)

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
    "pub fn $comparison\(|the constant-time comparison" \
    "pub fn $url_rendering\(|the URL rendering claim 6 registers the callers of"; do
    pattern="${declaration%%|*}"
    what="${declaration#*|}"
    anchors=$((anchors + 1))
    if ! grep -Eq -- "$pattern" <<<"$home_product"; then
        gate_fail "$home_rel no longer declares $what (\`$pattern\`); every claim this gate makes is about that name, so a tree without it is a tree this gate has nothing to say about"
    fi
done

# The redaction claim 6's third field requires, asserted where it lives rather than only where
# it is called. Without this, a rename would fail this gate at `main.rs` with a message about a
# call site, which is a verdict naming the wrong subject — the shape note **N60** charges for.
redaction_rel="$daemon_suffix/src/logging.rs"
anchors=$((anchors + 1))
# `\b` and not `\(`: the declaration carries a lifetime parameter (`<'a>`) between the name and
# its arguments, and a pattern that assumed the parenthesis followed the name would fail on the
# tree it is about — which is a gate red on the shipped build, the loudest way to be wrong.
if ! grep -Eq -- "pub fn $url_redaction\b" "$root/$redaction_rel" 2>/dev/null; then
    gate_fail "$redaction_rel no longer declares \`pub fn $url_redaction\`; that is the one call deciding how much of D11's URL a persistent sink may keep (note N182), and claim 6 requires the composition root to render through it by that name"
fi

gate_checked "$anchors" "named declaration(s) and module(s) asserted to exist before anything is counted"
gate_require_nonzero "$anchors" "named declarations"

# ------------------------------------------------------------------ claim 1: one reader
#
# The whole tree, not just the daemon: a helper crate that grew a reason to hold the secret
# would be the same defect one directory further out.

scanned=0
readers=0
declare -a witnesses=()
declare -a url_sites=()
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
                gate_fail "$rel implements \`$trait_name\` for \`$type_name\`; a token compared with \`==\` — or rendered, borrowed or converted into a \`&str\` and compared after — short-circuits at the first differing byte, which is the timing leak \`$comparison\` exists to not have"
            fi
        done
    fi

    # The same claim from the other direction: a conversion *out of* the type is implemented for
    # whatever it produces, so `impl From<Token> for String` names `$type_name` nowhere the rule
    # above looks. What it yields is the secret, and what the caller does with it is one `==`.
    for trait_name in "${banned_conversions[@]}"; do
        if grep -Eq -- "impl[^;{]*\b$trait_name<[[:space:]]*&?[[:space:]]*$type_name\b" <<<"$stripped"; then
            gate_fail "$rel implements \`$trait_name<$type_name>\`; a conversion taking the token hands its secret to whatever asked for one, and the caller that receives a \`String\` is a caller holding the key with no accessor named in the diff"
        fi
    done

    # Either rendering brings a file into the two claims below; a file that names neither is a
    # file with no way to hold the secret at all.
    names_accessor=0
    if grep -Fq -- "$accessor" <<<"$stripped"; then
        names_accessor=1
        readers=$((readers + 1))
    fi
    names_url=0
    if grep -Fq -- "$url_rendering" <<<"$stripped"; then
        names_url=1
    fi
    if ((names_accessor == 0 && names_url == 0)); then
        continue
    fi

    start="$(gate_test_region_start "$file")"
    if ((start < 0)); then
        gate_fail "$rel names a rendering that yields the token and this gate cannot tell its product code from its test code: it carries more than one \`#[cfg(test)]\` marker, or its marker does not open a \`mod\`. One trailing test module per file is what makes that readable, and a file nobody can classify is a finding rather than a pass"
        continue
    fi
    product="$(gate_product_lines "$file" "$start")"

    # ---------------------------------------------------------- claim 6, in the same walk
    #
    # The home is a site like any other here — it is on the register because it declares the
    # rendering — which is the difference from claim 1: the accessor is confined *to* the home,
    # and the URL is confined to a list the home is merely the first entry of.
    if ((names_url)) && (($(count_matching "$product" "$url_rendering") > 0)); then
        url_sites+=("$rel")

        # The register's third field, where the entry has one: **on the same line**, because
        # what it holds is that this call site does not render the URL raw. A line naming the
        # rendering without it is the redaction gone (note **N182**), and no test can see that
        # — this file's claim 6 header says why. Every such line is required to carry it, so a
        # second render added beside the first is a finding rather than a hole.
        for entry in "${url_sites_allowed[@]}"; do
            [[ "${entry%%|*}" == "$rel" ]] || continue
            required="${entry##*|}"
            [[ -n "$required" ]] || continue
            unguarded="$(grep -F -- "$url_rendering" <<<"$product" | grep -Fvc -- "$required" || true)"
            if ((unguarded > 0)); then
                gate_fail "$rel renders \`$url_rendering\` on $unguarded product line(s) that do not also name \`$required\`; this is the one line D11 asks the daemon to write a credential down on, \`$required\` is what decides how much of it the sink it is written into may keep, and a build that dropped it puts the run's bearer token into a persistent \`systemd-journal\`/\`adm\`-readable store (note N182). Nothing else in this workspace can go red on it: the composition root is a binary no suite can call into"
            fi
        done
    fi

    if ((names_accessor == 0)) || [[ "$rel" == "$home_rel" ]]; then
        continue
    fi

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

# ------------------------------------------------------------------ claim 6: the URL's callers
#
# The register reconciled both ways, `unsafe-scope.sh`'s shape and claim 5's, applied to a call
# site: a caller with no entry is a fourth place holding the secret, and an entry with no caller
# is a register describing a tree that has moved on.

for site in "${url_sites[@]}"; do
    permitted=0
    for entry in "${url_sites_allowed[@]}"; do
        if [[ "$site" == "${entry%%|*}" ]]; then
            permitted=1
        fi
    done
    if ((permitted == 0)); then
        gate_fail "$site names \`$url_rendering\` in its product code and is not one of this gate's ${#url_sites_allowed[@]} registered sites; that rendering is the token in a URL, so a caller holding its answer is holding the secret with no \`$accessor\` in the diff — one \`split\` from comparing it and one log line from a sink that keeps it (note N183)"
    fi
done

guarded=0
for entry in "${url_sites_allowed[@]}"; do
    expected="${entry%%|*}"
    # The middle field. Taken as "everything after the first `|`, less everything from the last"
    # rather than by a split, because a reason is prose and prose is where a `|` would turn up.
    why="${entry#*|}"
    why="${why%|*}"
    [[ -z "${entry##*|}" ]] || guarded=$((guarded + 1))
    seen=0
    for site in "${url_sites[@]}"; do
        if [[ "$site" == "$expected" ]]; then
            seen=1
        fi
    done
    if ((seen == 0)); then
        gate_fail "$expected is registered as $why and its product code no longer names \`$url_rendering\`; a register entry with nothing behind it is either a rendering that has moved somewhere this gate is not looking, or D11's ready-to-open URL having quietly stopped being printed"
    fi
done

gate_checked "${#url_sites[@]}" "product-code site(s) of \`$type_name::$url_rendering\`, reconciled against ${#url_sites_allowed[@]} registered site(s)"
gate_require_nonzero "${#url_sites[@]}" "callers of \`$url_rendering\`"
gate_checked "$guarded" "registered site(s) required to render the URL through \`$url_redaction\` on the same line"
# A register whose third fields have all been emptied is a register that has stopped holding the
# redaction, and every claim above would still be green. The count is the claim (note N185).
gate_require_nonzero "$guarded" "sites required to name \`$url_redaction\`"

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

# ------------------------------------------------------------------ claim 5: the field's readers
#
# The register is `field_readers_allowed` — the accessor and the comparison, which is the policy
# already named at the top of this file rather than a second list — and the tree side is every
# product-code line of the home that reads a field of the type, attributed to the function it
# sits in.
# Both sides are walked; a reader with no entry and an entry with no reader both fail, which is
# `unsafe-scope.sh`'s residual register applied to a field instead of to a token.
#
# **Attribution is by the last `fn` seen above the line**, which is the same order of rule the
# rest of this file uses: it fits in a sentence, a reader can check it by eye, and it does not
# need a Rust parser. What it costs is stated: a field read inside a nested item — a closure
# assigned nowhere, an `impl` inside a function — is attributed to the enclosing `fn`, and a read
# that sits under no `fn` at all is a **failure and not a pass**, because a line this cannot
# attribute is a reader nobody can reconcile.

field_alternation="$(
    IFS='|'
    printf '%s' "${token_fields[*]}"
)"

# Which function each product-code read of a field sits in, one name per read.
#
# A field access on **anything** and not only on `self`, because `fn secret_of(token: &Token) ->
# &str { &token.hex }` beside the type is the same second way out written as a free function. The
# struct literal in the constructor is `hex:` with no dot in front of it and is deliberately not
# a read — a value going *into* the type is how the token is minted.
readers_by_function() {
    local file="$1" start="$2" fields="$3"
    gate_product_lines "$file" "$start" |
        awk -v field_re="\\.($fields)([^A-Za-z0-9_]|\$)" '
            {
                if (match($0, /(^|[^A-Za-z0-9_])fn[ \t]+[A-Za-z_][A-Za-z0-9_]*/)) {
                    current = substr($0, RSTART, RLENGTH)
                    sub(/^.*fn[ \t]+/, "", current)
                }
                if ($0 ~ field_re) { print (current == "" ? "<no enclosing fn>" : current) }
            }
        '
}

mapfile -t field_reads < <(readers_by_function "$home" "$home_product_start" "$field_alternation")
mapfile -t reading_functions < <(printf '%s\n' "${field_reads[@]}" | grep -v '^$' | sort -u)

for reader in "${reading_functions[@]}"; do
    allowed=0
    for permitted in "${field_readers_allowed[@]}"; do
        if [[ "$reader" == "$permitted" ]]; then
            allowed=1
        fi
    done
    if ((allowed == 0)); then
        gate_fail "$home_rel reads a field of \`$type_name\` inside \`$reader\`, which is neither \`$accessor\` nor \`$comparison\`; a second way out of this type needs no banned trait and no \`$accessor\` in the diff — \`token.$reader() == candidate\` in $gate_rel is then the short-circuiting comparison with every other claim here still green"
    fi
done

for permitted in "${field_readers_allowed[@]}"; do
    seen=0
    for reader in "${reading_functions[@]}"; do
        if [[ "$reader" == "$permitted" ]]; then
            seen=1
        fi
    done
    if ((seen == 0)); then
        gate_fail "\`$permitted\` in $home_rel no longer reads a field of \`$type_name\`; the register this gate reconciles has an entry with nothing behind it, which is a comparison comparing something other than the secret or an accessor yielding something other than it"
    fi
done

gate_checked "${#field_reads[@]}" "read(s) of \`$type_name\`'s own field(s) in $home_rel's product code, reconciled against ${#field_readers_allowed[@]} registered reader(s)"
gate_require_nonzero "${#field_reads[@]}" "reads of the token's field inside its home"
gate_note "the field(s) ${token_fields[*]} are read in: ${reading_functions[*]}"

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
