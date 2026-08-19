#!/usr/bin/env bash
#
# Colour becomes brightness in exactly one place in this workspace (design §2.10 "one home per
# law", D8's comparability sentence, D16, D17; notes **N266**, **N267**).
#
# ## What went wrong, measured
#
# `imaging::compare::read` decoded JPEG and PNG through `image`'s `to_luma8`, which is Rec. 709
# (`SRGB_LUMA = [2126, 7152, 722] / 10000`), while the Netpbm arm eight lines below it carried an
# inline Rec. 601 in rationals and the rest of the crate — the YUV decode, `Decoded::luma`,
# calibration, every committed hardware `mean_luma` — used BT.601 in integers. Three homes, two
# definitions, one picture:
#
#     bar  RGB            BT.601  Rec.709  |  PNG  PPM  JPEG  capture
#      3   (  0,255,  0)     150      182  |  182  149   182      149
#      6   (  0,  0,255)      29       18  |   18   29    18       28
#
# 33 codes apart at worst, and `compare::photos` of one scene against itself — its PNG against its
# PPM — answered `ssim 0.9688` where a picture against itself is 1.0. Note **N266** carries the
# whole table.
#
# **No test in this workspace could go red on it.** The round-trip walk that exists
# (`every_format_this_build_writes_is_a_format_the_comparison_reads`) feeds a *grey* fixture, and
# grey is a fixed point of every luma definition there is: all three homes answer identically on
# it, so the walk over `PhotoFormat::ALL` was green on a tree with three colour models in it. The
# colour arm that goes red now landed with this gate; what the gate adds is the half a test cannot
# see — the *fourth* definition, in a file nobody has written yet.
#
# ## The four claims
#
#   1. **The home exists, and there is one of it.** `decode::luma_sample` is the scalar and
#      `decode::rgb_to_luma` is that scalar over a frame; each is declared exactly once in the
#      workspace's product code, in the home. A second definition of either name is the defect
#      arriving with the word "luma" in it, spelled out.
#   2. **Nobody borrows another crate's luma.** `image` will convert colour to grey for anybody
#      who asks, in Rec. 709, and this workspace asks nowhere. The ban is on the **family** and
#      not on the one spelling that was found (note **N249**, rubric A17): `to_luma8` was the
#      defect, and `into_luma8`, `to_luma16`, `to_luma_alpha8`, `grayscale()` and
#      `imageops::grayscale` are the same defect with a different number or a different verb on
#      it, so the pattern matches the shape of the name rather than any member of the list. The
#      *call syntax* is part of that shape too — `DynamicImage::to_luma8(&x)` is the same method
#      as `x.to_luma8()` and is the form rustc prints when it refuses the other — and so are the
#      two doors that reach the conversion without naming a luma at all: `ConvertBuffer::convert`
#      into a `GrayImage`, which is byte-for-byte `to_luma8`, and `FromColor::from_color`, which
#      is where `image` writes the Rec. 709 arithmetic both of them end at.
#   3. **No second set of coefficients.** Two halves, because a second definition can be written
#      two ways. The first is **derived from the home itself**: the multipliers are read out of
#      `luma_sample`'s own body, never typed here, and no other product file may carry all three
#      — which is what catches a copy-paste of the home into a second module, and what keeps the
#      claim true after somebody re-derives the weights. The second is the two *standard*
#      alternatives named as a family — Rec. 601 in rationals and Rec. 709, integer and float
#      spellings alike — because a fresh author writing a second conversion reaches for a
#      textbook constant rather than for this file's. Both halves are per file over its product
#      half, since three coefficients on three lines is one definition just as much as three on
#      one line. **What the home is exempt from is its own triple and nothing else**: the four
#      standard sets are checked against `decode.rs` like every other file, because a second
#      standardised colour model written *inside* the one home is the version of this defect no
#      other claim here can see — claim 1 looks for the home's two names and a third function has
#      neither, and claim 4 has the home registered.
#   4. **The home's callers are reconciled, both ways, against a register.** Forward: a product
#      file that converts colour to brightness and is not on the list is a fifth consumer nobody
#      declared, and it costs one line to declare. **Backward is the half this gate exists for**:
#      an entry whose product code has *stopped* naming the home is precisely the defect above —
#      `compare.rs` was a registered consumer of this crate's colour model that had quietly
#      grown its own, and every other claim here was green while it was. A register entry with
#      nothing behind it is `unsafe-scope.sh`'s residual read the second way, and
#      `kill-is-never-a-fallback.sh`'s "the only caller went away" is the same shape.
#
# ## What this does **not** claim
#
# **It cannot see a coefficient set nobody standardised.** A fourth definition written with
# freshly-derived weights — not the home's, not Rec. 601's, not Rec. 709's — passes claim 3, and
# nothing here would notice it until claim 4 asked why a new file was converting. That residual
# is real and it is stated rather than approximated: what closes it is the colour arm in
# `compare.rs`'s suite, which reads one scene through every format this build writes and holds
# all of them to a committed table, plus the diff reader. Rubric rule 2's other half — an
# assertion that cannot go red is worse than an argument that admits it is one — is why the
# sentence is here instead of a pattern that pretends to match "any weighted sum".
#
# **Claim 3's numbers are matched anywhere in a file's product half, not within one expression.**
# Three integers bounded by non-digits, on any three lines, is the rule — chosen so that a
# definition spread over three statements reads the same as one written on a line, and the price
# is that a file which happened to acquire all three of a set for unrelated reasons would be red
# under a colour-model sentence. That it has not happened is what the predicate's own green says,
# every run; if it ever fires, the repair is to require the numbers on a short span and to keep
# this sentence for the narrow match.
#
# It says nothing about whether BT.601 is the *right* luma for sRGB either. It is the one this
# workspace measured its hardware evidence through, which is the argument note **N266** makes;
# moving the whole crate to Rec. 709 is the owner's call and would move this gate's home, not
# remove it.
#
# ## The matching rule
#
# Line comments are stripped and the test region is dropped, both through `lib.sh`
# (`gate_test_region_start`, `gate_product_lines`), for the reason every caller of those gives:
# **prose does not count**, because the argument that defends this rule is written beside the
# code and names Rec. 709 four times in the course of making it — and a suite may name the other
# definition freely, since an arm proving that two colour models differ has to be able to say
# what the other one is. A file whose product half cannot be identified is a **failure and not a
# pass**.
#
# ## The populations
#
# Derived, not transcribed: the crate directory comes from `cargo metadata`, the files come from
# the tree walk, and claim 3's first half reads its numbers out of the home's declaration. What
# is *policy* — the home, the two function names, the two registered consumers — is named once
# below and asserted to exist, so a rename fails this gate rather than quietly emptying it.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

root="$(gate_root)"

# The policy this gate is about, named once. Every name here is asserted to exist.
scalar="luma_sample"
frame_converter="rgb_to_luma"

imaging_dir="$(gate_metadata |
    jq -r '.packages[] | select(.name == "webcam-handler-imaging") | .manifest_path' |
    head -n1 | xargs -r dirname)"

if [[ -z "$imaging_dir" ]]; then
    gate_fail "webcam-handler-imaging is not a workspace member; the crate that owns this project's colour model has no home for these claims to be about"
    gate_finish
fi

# The metadata may describe a different checkout than the tree under test (the selftest feeds
# mutated copies), so paths are resolved as suffixes of the tree under test.
imaging_suffix="${imaging_dir#"$root"/}"
home_rel="$imaging_suffix/src/decode.rs"
home="$root/$home_rel"

# Claim 4's register: the product-code files that may name the home, each with the reason it is
# there. Two entries, because this workspace reduces colour to brightness for two reasons — a
# captured frame being measured, and a stored photograph being read back for comparison.
consumers_allowed=(
    "$home_rel|the home, where the frame converter calls the scalar and the YUV decode reaches both"
    "$imaging_suffix/src/compare.rs|the comparison reader, whose compressed and Netpbm colour paths both measure through the home (D17)"
)

# Claim 2's family. Matched by the *shape* of the name, so a width or an alpha channel appended
# to any of them is the same finding: `image`'s colour-to-grey conversions all read `to_luma…`,
# `into_luma…` or `grayscale…`, whatever they are spelled with afterwards.
#
# **The call syntax is part of the shape, not part of the spelling.** `x.to_luma8()` and
# `DynamicImage::to_luma8(&x)` are one method reached two ways, and the second is the form rustc
# itself prints when it refuses the first (`help: consider wrapping the function in a closure`),
# so an anchor on the dot alone would be a ban on the punctuation rather than on the conversion.
# Each pattern therefore accepts either separator.
#
# **The whole-buffer conversion is banned by its trait's name.** `ConvertBuffer::convert` reaches
# the same Rec. 709 through `FromColor` and is the most idiomatic way to get a `GrayImage` out of
# an `RgbImage` — but `convert(` is an English word this workspace uses about other things, and a
# colour-model verdict printed over a `Duration::convert(` would be red for the wrong reason
# (rule 2). The trait has to be in scope to call the method, so naming the trait catches the call
# whatever the receiver is spelled like; the one way to have the method without the name is a
# glob import of its module, which is the second alternation.
#
# **Field-separated by a tab and not by the `|` its neighbours use**, because these fields are
# alternations and a `|` separator would cut each pattern at its first branch — which is a rule
# that reads as if it matched a family while matching one spelling of it, the exact failure this
# claim is about.
foreign_conversions=(
    $'(\\.|::)(to|into)_luma[A-Za-z0-9_]*\\(\tthe `image` crate\'s own luma conversion, which is Rec. 709'
    $'\\b(grayscale|grayscale_with_type)[[:space:]]*\\(\t`image`\'s greyscale conversion under its other name'
    $'\\bimageops::[A-Za-z_:]*grayscale\tthe same conversion reached through `imageops`'
    $'\\bLuma::from\\b\ta per-pixel conversion into `Luma` by somebody else\'s arithmetic'
    $'\\bConvertBuffer\\b|\\buse image::buffer::\\*\t`image`\'s whole-buffer conversion, which reaches the same Rec. 709 through `FromColor`'
    $'\\bFromColor\\b|(\\.|::)from_color[[:space:]]*\\(\t`image`\'s per-pixel colour conversion, which is where its Rec. 709 luma is actually written'
)

# Claim 3's second half: the two standard luma definitions that are *not* this project's, each as
# an ordered triple, in the integer and the float spelling a fresh author would reach for. A file
# carrying all three numbers of a set in its product half is a second definition of luma.
declare -a foreign_coefficient_sets=(
    "299 587 114|Rec. 601 in rationals — the definition this crate's own Netpbm arm used to carry"
    "2126 7152 722|Rec. 709, which is what \`image\`'s \`to_luma8\` computes"
    "0.299 0.587 0.114|Rec. 601 in floats"
    "0.2126 0.7152 0.0722|Rec. 709 in floats"
)

# ------------------------------------------------------------------ the named policy exists

anchors=0
anchors=$((anchors + 1))
if [[ ! -f "$home" ]]; then
    gate_fail "$home_rel is missing; it is the home every claim here is about, and a partial verdict about a missing home would read as a verdict about a colour model"
    gate_checked "$anchors" "named module(s) this gate's claims are about"
    gate_finish
fi

home_product_start="$(gate_test_region_start "$home")"
if ((home_product_start < 0)); then
    gate_fail "$home_rel does not carry the one trailing \`#[cfg(test)] mod\` this gate reads a file's product half by; the home's own declarations would be read off a boundary this guessed at"
    gate_checked "$anchors" "named module(s) this gate's claims are about"
    gate_finish
fi
home_product="$(gate_product_lines "$home" "$home_product_start")"

for declaration in \
    "fn $scalar\(|the scalar that turns one colour sample into one brightness" \
    "fn $frame_converter\(|that scalar over a whole frame"; do
    pattern="${declaration%%|*}"
    what="${declaration#*|}"
    anchors=$((anchors + 1))
    if ! grep -Eq -- "$pattern" <<<"$home_product"; then
        gate_fail "$home_rel no longer declares $what (\`$pattern\`); every claim this gate makes is about that name, so a tree without it is a tree this gate has nothing to say about"
    fi
done

gate_checked "$anchors" "named declaration(s) and module(s) asserted to exist before anything is counted"
gate_require_nonzero "$anchors" "named declarations"

# ------------------------------------------------------------------ claim 3, first half: the weights
#
# Read out of the home's own body rather than typed here, so a re-derivation of the coefficients
# is still a triple this refuses to see twice — and so the claim below cannot be satisfied by a
# gate and a home that have drifted apart.

home_function_body() {
    awk -v name="$1" '
        !inside && $0 ~ ("^(pub\\(crate\\) )?(pub )?(const )?fn " name "\\(") { inside = 1 }
        inside { print }
        inside && /^\}/ { exit }
    ' <<<"$home_product"
}

mapfile -t weights < <(home_function_body "$scalar" |
    grep -oE '[0-9]+[[:space:]]*\*' | grep -oE '[0-9]+' | sort -un)

gate_checked "${#weights[@]}" "coefficient(s) read out of \`$scalar\`'s own body in $home_rel"
if ((${#weights[@]} != 3)); then
    gate_fail "\`$scalar\` in $home_rel multiplies by ${#weights[@]} distinct literal(s) and a luma conversion weights three channels; the derived half of claim 3 has no triple to be about, which is a home that has changed shape underneath this gate rather than a tree that is clean"
fi

# The home's own triple joins the named sets, so a copy of it into a second file is the same
# finding as a textbook constant landing there.
#
# **It is kept apart from the four standard sets because only it is exempt, and only in the home.**
# Exempting the home *file* from every set — which is the obvious spelling and the wrong one —
# means none of the four standardised colour models is ever checked against `decode.rs`, so a
# Rec. 709 second definition written inside the one home is invisible to this claim, to claim 1
# (which looks for the home's two names, not for a third function), and to claim 4 (which has the
# home registered). The file the gate is most about would be the one file it did not read. So the
# exemption is the *triple*: the home may carry its own weights, and may not carry anybody else's.
own_triple="$(
    IFS=' '
    printf '%s' "${weights[*]}"
)"
own_triple_entry=""
if ((${#weights[@]} == 3)); then
    own_triple_entry="$own_triple|this project's own weights, copied out of $home_rel"
fi

# Does $1 (a file's product half) carry every number in the space-separated set $2?
carries_every_number() {
    local product="$1" number
    for number in $2; do
        # Escaped for the ERE, and bounded on both sides so `29` does not match `129` or `2.9`
        # — a bound this loose would otherwise turn every timeout constant in the tree red.
        local escaped="${number//./\\.}"
        grep -Eq -- "(^|[^0-9.])$escaped([^0-9]|\$)" <<<"$product" || return 1
    done
    return 0
}

# ------------------------------------------------------------------ the walk
#
# One pass over every Rust file in the tree under test, carrying claims 1, 2, 3 and 4.
#
# **The cheap raw grep is where claims 2 and 3 quantify**, and it runs over every file: a module
# that reached for somebody else's luma would not name this crate's home, so a walk that only
# looked at files which do would be a walk that could not see the defect. Reading a file's
# *product* half is what a hit then earns — the answer has to weigh a `//` comment differently
# from a line of code — and a file that is a candidate and cannot be classified is a failure and
# not a pass. Classifying every file in the tree instead would fail on the eleven that carry more
# than one `#[cfg(test)]` marker and have no colour in them at all, which is a verdict about a
# convention rather than about a colour model.

scanned=0
classified=0
declare -a consumers=()
declare -a scalar_calls=()
declare -a converter_calls=()
declare -a definitions=()
coefficient_claims=0
conversion_claims=0

while IFS= read -r -d '' file; do
    scanned=$((scanned + 1))
    rel="${file#"$root"/}"
    raw="$(cat "$file")"

    interesting=0
    declare -a hit_conversions=()
    for entry in "${foreign_conversions[@]}"; do
        conversion_claims=$((conversion_claims + 1))
        if grep -Eq -- "${entry%%$'\t'*}" <<<"$raw"; then
            hit_conversions+=("$entry")
            interesting=1
        fi
    done

    # The four standard sets are checked against every file, the home included; the home's own
    # triple is checked against every file except the home. See `own_triple_entry` above.
    declare -a sets_for_this_file=("${foreign_coefficient_sets[@]}")
    if [[ "$rel" != "$home_rel" && -n "$own_triple_entry" ]]; then
        sets_for_this_file+=("$own_triple_entry")
    fi

    declare -a hit_sets=()
    for entry in "${sets_for_this_file[@]}"; do
        coefficient_claims=$((coefficient_claims + 1))
        if carries_every_number "$raw" "${entry%%|*}"; then
            hit_sets+=("$entry")
            interesting=1
        fi
    done

    declares=0
    if grep -Eq -- "fn $scalar\(|fn $frame_converter\(" <<<"$raw"; then
        declares=1
        interesting=1
    fi
    names_home=0
    if grep -Eq -- "\b($scalar|$frame_converter)\b" <<<"$raw"; then
        names_home=1
        interesting=1
    fi

    ((interesting)) || continue

    start="$(gate_test_region_start "$file")"
    if ((start < 0)); then
        gate_fail "$rel names a colour-to-brightness conversion and this gate cannot tell its product code from its test code: it carries more than one \`#[cfg(test)]\` marker, or its marker does not open a \`mod\`. One trailing test module per file is what makes that readable, and a file nobody can classify is a finding rather than a pass"
        continue
    fi
    classified=$((classified + 1))
    product="$(gate_product_lines "$file" "$start")"

    # ---------------------------------------------------------- claim 2: somebody else's luma
    for entry in "${hit_conversions[@]}"; do
        pattern="${entry%%$'\t'*}"
        what="${entry#*$'\t'}"
        if grep -Eq -- "$pattern" <<<"$product"; then
            gate_fail "$rel calls $what; this workspace has one RGB→luma home (\`decode::$scalar\`, BT.601 in integers) and a second definition makes a photograph disagree with the stream it was taken from, and with the same picture written to another sink, by up to 33 codes (note N266)"
        fi
    done

    # ---------------------------------------------------------- claim 3: a second coefficient set
    for entry in "${hit_sets[@]}"; do
        set="${entry%%|*}"
        what="${entry#*|}"
        if carries_every_number "$product" "$set"; then
            gate_fail "$rel's product code carries all of \`$set\` — $what; three luma coefficients that are not this workspace's own are a second colour model wherever they sit, $home_rel included, whether they sit on one line or on three"
        fi
    done

    # ---------------------------------------------------------- claim 1: one definition each
    if ((declares)) && grep -Eq -- "fn $scalar\(|fn $frame_converter\(" <<<"$product"; then
        definitions+=("$rel")
    fi

    # ---------------------------------------------------------- claim 4: who converts
    ((names_home)) || continue
    grep -Eq -- "\b($scalar|$frame_converter)\b" <<<"$product" || continue
    consumers+=("$rel")

    # Calls, not mentions: the home declares each of them once without being a caller of it, and
    # claim 4's "something still converts" half is about callers.
    called="$(grep -Ev -- "fn ($scalar|$frame_converter)\(" <<<"$product")"
    if grep -Eq -- "(^|[^A-Za-z0-9_])$scalar\(" <<<"$called"; then
        scalar_calls+=("$rel")
    fi
    if grep -Eq -- "(^|[^A-Za-z0-9_])$frame_converter\(" <<<"$called"; then
        converter_calls+=("$rel")
    fi
done < <(gate_rust_files)

gate_checked "$scanned" "Rust source file(s) scanned for a second definition of luma and for a caller reaching around the one home"
gate_require_nonzero "$scanned" "Rust source files"
gate_checked "$conversion_claims" "check(s) that a file does not reach for another crate's colour-to-grey conversion"
gate_require_nonzero "$conversion_claims" "foreign-conversion checks"
gate_checked "$coefficient_claims" "check(s) that a file carries no complete set of luma coefficients it has no business with — the four standard sets against every file, $home_rel included, and the home's own triple against every file but the home"
gate_require_nonzero "$coefficient_claims" "coefficient-set checks"
gate_checked "$classified" "file(s) that named a colour-to-brightness conversion and had their product half read"
# A tree where nothing at all matched is a tree this walk read past, not a tree with one colour
# model: the home itself is a match, so a zero here is a statement about the walk.
gate_require_nonzero "$classified" "files whose product half was read"

# ------------------------------------------------------------------ claim 1: one definition each

for rel in "${definitions[@]}"; do
    if [[ "$rel" != "$home_rel" ]]; then
        gate_fail "$rel declares \`$scalar\` or \`$frame_converter\` in its product code and $home_rel is the one home for either; a second definition under the same name is the first thing a reader of this crate would take for the only one"
    fi
done
gate_checked "${#definitions[@]}" "file(s) declaring \`$scalar\` or \`$frame_converter\`, of which $home_rel is the home"
gate_require_nonzero "${#definitions[@]}" "declarations of the home's two functions"

# ------------------------------------------------------------------ claim 4: the register, both ways

for consumer in "${consumers[@]}"; do
    permitted=0
    for entry in "${consumers_allowed[@]}"; do
        [[ "$consumer" == "${entry%%|*}" ]] && permitted=1
    done
    if ((permitted == 0)); then
        gate_fail "$consumer names this crate's colour model in its product code and is not one of this gate's ${#consumers_allowed[@]} registered consumers; converting colour to brightness is a decision with a home, and a new consumer of it is a line in this register rather than a surprise in a diff"
    fi
done

for entry in "${consumers_allowed[@]}"; do
    expected="${entry%%|*}"
    why="${entry#*|}"
    seen=0
    for consumer in "${consumers[@]}"; do
        [[ "$consumer" == "$expected" ]] && seen=1
    done
    if ((seen == 0)); then
        gate_fail "$expected is registered as $why and its product code no longer names \`$scalar\` or \`$frame_converter\`; a register entry with nothing behind it is a consumer that has grown a colour model of its own, which is exactly the defect this gate was written for (note N266) — every other claim here is green while it is true"
    fi
done

gate_checked "${#consumers[@]}" "product-code consumer(s) of the crate's colour model, reconciled against ${#consumers_allowed[@]} registered consumer(s)"
gate_require_nonzero "${#consumers[@]}" "consumers of the colour model"

# ------------------------------------------------------------------ claim 4: something still converts
#
# Every claim above is true of a tree where the home has no callers at all, and that tree is a
# worse defect than any of them: a colour model nothing reaches is a colour model each caller has
# replaced with one of its own.

if ((${#scalar_calls[@]} == 0)); then
    gate_fail "nothing in this workspace's product code calls \`$scalar(\`; the one home has no callers, so every consumer of colour is reaching brightness some other way"
fi
if ((${#converter_calls[@]} == 0)); then
    gate_fail "nothing in this workspace's product code calls \`$frame_converter(\`; a frame's colour is reduced to brightness somewhere, and it has stopped being reduced here"
fi
gate_checked "${#scalar_calls[@]}" "file(s) calling \`$scalar\`: ${scalar_calls[*]:-none}"
gate_checked "${#converter_calls[@]}" "file(s) calling \`$frame_converter\`: ${converter_calls[*]:-none}"
gate_note "the coefficients read out of $home_rel are ${weights[*]}"

gate_finish
