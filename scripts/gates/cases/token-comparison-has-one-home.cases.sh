# Both-direction cases for `token-comparison-has-one-home.sh`.
#
# Four claims, and every one of them gets its inverse: the secret has one reader, the type
# refuses `==`, `Debug` is hand-written and redacting, and something still compares. The arms
# are grouped in that order.
#
# **`fail_case_the_gate_compares_the_secret_itself` is the one this predicate exists for.** It
# seeds the mutant that survived P5a — one `token.verify(candidate)` rewritten as
# `token.expose_secret() == candidate` — which is behaviourally identical, passes every test in
# the workspace, and turns "how long did the daemon take to say no" into "how many leading
# digits were right". If any arm in this file stops going red, that is the one to look at
# first.
#
# The green arm below it matters for the mirror-image reason: the daemon's own suite cannot
# assert that an equal-length near miss is refused without building one out of the token, so
# every existing reader of the secret outside its home is a test. A confinement gate that
# refused those would be a gate somebody turns off, and the arm is here to keep it from
# becoming one.
#
# shellcheck shell=bash

pass_case() {
    "$GATE"
}

# ------------------------------------------------- claim 1: the secret has one reader

# A test may hold the secret, and this is what that looks like — the shape all twelve of
# `gate.rs`'s existing readers already have, seeded once more inside its `mod tests`.
pass_case_a_test_may_hold_the_secret() {
    local tree file
    tree="$(gate_scratch_tree)"
    file="$tree/crates/daemon/src/http/gate.rs"
    # Before the file's last line, which is the closing brace of its one `mod tests`.
    sed -i '$i\
\
    #[test]\
    fn seeded_by_the_gate_selftest() {\
        let token = minted();\
        assert!(!token.expose_secret().is_empty());\
    }' "$file"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The mutant. `Token::verify` is a constant-time comparison and `==` on the exposed secret is
# not; nothing else in this workspace can tell them apart, because they answer the same thing
# for every input and differ only in how long they take to answer it.
fail_case_the_gate_compares_the_secret_itself() {
    local tree
    tree="$(gate_scratch_tree)"
    sed -i 's/every_one_verified &= token\.verify(candidate);/every_one_verified \&= token.expose_secret() == candidate;/' \
        "$tree/crates/daemon/src/http/gate.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The same reach from a module that has no test module at all, which is the other branch of
# the product/test split: with no `#[cfg(test)]` marker in the file, every line of it is
# product code and there is nothing to weigh the mention against.
fail_case_a_new_module_reaches_for_the_secret() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/daemon/src/http/leak.rs" <<'RS'
//! Seeded by the gate selftest: nothing outside the token's own module may hold the secret.

use super::token::Token;

pub(crate) fn is_the_token(token: &Token, presented: &str) -> bool {
    token.expose_secret() == presented
}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

# A file this predicate cannot classify is a failure and not a pass — `unsafe-scope.sh` already
# charges that price for a count it cannot read. Two `#[cfg(test)]` markers means the "one
# trailing test module" rule has no answer about where this file's product code ends, and the
# confinement would be reporting on a boundary it guessed at.
fail_case_a_second_test_module_makes_the_boundary_unreadable() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/crates/daemon/src/http/gate.rs" <<'RS'

#[cfg(test)]
mod more_tests {
    use super::*;

    #[test]
    fn seeded_by_the_gate_selftest() {
        assert!(!REFUSAL.is_empty());
    }
}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The accessor is what claim 1 counts readers of. Renamed, it has no readers, and a
# confinement over an empty population is the vacuous green this suite exists to refuse.
fail_case_the_secret_accessor_was_renamed() {
    local tree
    tree="$(gate_scratch_tree)"
    sed -i 's/pub fn expose_secret(/pub fn secret(/' "$tree/crates/daemon/src/http/token.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# ------------------------------------------------- claim 2: the type refuses `==`

# `token == candidate` is the timing leak wearing a different hat: `str`'s `PartialEq` compares
# lengths and then bytes and returns at the first difference, which is precisely what an
# attacker timing the refusals is counting.
fail_case_the_token_gains_an_equality_impl() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/crates/daemon/src/http/token.rs" <<'RS'

impl PartialEq for Token {
    /// Seeded by the gate selftest.
    fn eq(&self, other: &Token) -> bool {
        self.hex == other.hex
    }
}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The one-word version of the same thing.
fail_case_the_token_derives_a_comparison() {
    local tree
    tree="$(gate_scratch_tree)"
    sed -i 's/^pub struct Token {/#[derive(PartialEq, Eq, Hash)]\npub struct Token {/' \
        "$tree/crates/daemon/src/http/token.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# A `Display` is a secret that reaches a log line, a format string or a comparison without
# anybody having typed the word `expose`.
fail_case_the_token_gains_a_display() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >>"$tree/crates/daemon/src/http/token.rs" <<'RS'

impl std::fmt::Display for Token {
    /// Seeded by the gate selftest.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.hex)
    }
}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

# ------------------------------------------------- claim 3: `Debug` is hand-written and redacts

# The derive prints the field, and the field is the key to a camera. This is the half no test
# can hold: a build where the hand-written impl was replaced by the attribute would print the
# token from any `tracing::debug!(?token)` written three sub-milestones from now.
fail_case_the_token_derives_its_debug() {
    local tree
    tree="$(gate_scratch_tree)"
    sed -i 's/^pub struct Token {/#[derive(Debug)]\npub struct Token {/' \
        "$tree/crates/daemon/src/http/token.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# A hand-written impl that prints the field is the derive's behaviour typed out by hand. The
# field name is spelled here because a seed has to be a real edit to a real declaration; if it
# is ever renamed this arm stops seeding anything and the selftest says so out loud, which is
# the loud direction for a seeding failure to take.
fail_case_the_debug_impl_prints_the_field() {
    local tree
    tree="$(gate_scratch_tree)"
    sed -i 's/\.field("hex", &Redacted)/.field("hex", \&self.hex)/' \
        "$tree/crates/daemon/src/http/token.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The impl moved somewhere this gate is not looking, which reads exactly like a type that never
# had one — and a `Token` with no hand-written `Debug` is a `Token` whose `Debug` is derived or
# absent, both of which are the defect.
fail_case_the_hand_written_debug_went_away() {
    local tree
    tree="$(gate_scratch_tree)"
    sed -i 's/^impl std::fmt::Debug for Token {/impl std::fmt::Debug for TokenRedaction {/' \
        "$tree/crates/daemon/src/http/token.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# ------------------------------------------------- claim 4: something still compares

# A tree where the gate stopped calling the comparison satisfies every claim above and serves
# a live camera to whoever asks. `kill-is-never-a-fallback.sh`'s "the only caller went away"
# arm, about a different absence.
fail_case_the_gate_no_longer_calls_the_comparison() {
    local tree
    tree="$(gate_scratch_tree)"
    sed -i 's/token\.verify(candidate)/!candidate.is_empty()/g' \
        "$tree/crates/daemon/src/http/gate.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_comparison_is_no_longer_declared() {
    local tree
    tree="$(gate_scratch_tree)"
    sed -i 's/pub fn verify(/fn verify_disabled(/' "$tree/crates/daemon/src/http/token.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# ------------------------------------------------- the subjects themselves

# A renamed type takes every claim in this file with it: no declaration to read attributes off,
# no fields for the `Debug` claim to refuse, and nothing for an `impl PartialEq` to be *for*.
fail_case_the_token_type_was_renamed() {
    local tree
    tree="$(gate_scratch_tree)"
    sed -i 's/\bpub struct Token\b/pub struct Bearer/' "$tree/crates/daemon/src/http/token.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_token_module_moved() {
    local tree http
    tree="$(gate_scratch_tree)"
    http="$tree/crates/daemon/src/http"
    mv "$http/token.rs" "$http/bearer.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# The module this confinement is chiefly about. Without it in the tree, the walk finds no
# product-code reader of the secret for the honest reason and the dishonest one at once.
fail_case_the_token_gate_module_moved() {
    local tree http
    tree="$(gate_scratch_tree)"
    http="$tree/crates/daemon/src/http"
    mv "$http/gate.rs" "$http/policy.rs"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# Seeded through the metadata rather than by deleting the crate, because deleting it makes
# `cargo metadata` itself fail — and an arm that goes red before the predicate runs proves
# nothing about the predicate.
fail_case_the_daemon_left_the_workspace() {
    local md
    md="$(gate_metadata_snapshot)"
    jq 'del(.packages[] | select(.name == "webcam-handler-daemon"))' "$md" >"$md.seeded"
    WCH_GATE_METADATA="$md.seeded" "$GATE"
}
