# Both-direction cases for `token-comparison-has-one-home.sh`.
#
# Five claims, and every one of them gets its inverse: the secret has one reader, the type
# refuses `==` and every operator that would perform one for it, `Debug` is hand-written and
# redacting, something still compares, and the field has exactly the readers the gate registers.
# The arms are grouped in that order.
#
# **The conversion arms and the two claim-5 arms are the P5 review's**, and they are here because
# a third shape got past the first four claims: `impl AsRef<str> for Token` in the home, plus one
# of `gate.rs`'s two credential paths rewritten as `token.as_ref() == candidate`, was measured
# green on every arm in this file and on the predicate as it then stood, with the timing leak
# live on the credential form that rides the URL. Note **N78** makes this gate the sole holder of
# that claim, so a hole in it is not backed up by a test anywhere.
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
    #
    # `$i` is `sed`'s insert-before-the-last-line command and not a shell expansion, so the
    # single quotes are the point rather than an oversight.
    # shellcheck disable=SC2016
    gate_seed '$i\
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
    gate_seed 's/every_one_verified &= token\.verify(candidate);/every_one_verified \&= token.expose_secret() == candidate;/' \
        "$tree/crates/daemon/src/http/gate.rs"
    # The file the reach is *from*, which is what separates this arm from the one below it:
    # claim 1's sentence is the same for every module that reads the secret, and the only thing
    # in it that says which module did is the name it opens with.
    gate_red_because "crates/daemon/src/http/gate.rs reads the token's secret outside" \
        env WCH_GATE_ROOT="$tree" "$GATE"
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
    gate_red_because "crates/daemon/src/http/leak.rs reads the token's secret outside" \
        env WCH_GATE_ROOT="$tree" "$GATE"
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
    gate_red_because 'names a rendering that yields the token and this gate cannot tell its product code from its test code' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# The accessor is what claim 1 counts readers of. Renamed, it has no readers, and a
# confinement over an empty population is the vacuous green this suite exists to refuse.
fail_case_the_secret_accessor_was_renamed() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/pub fn expose_secret(/pub fn secret(/' "$tree/crates/daemon/src/http/token.rs"
    gate_red_because 'no longer declares the one accessor that yields the secret' \
        env WCH_GATE_ROOT="$tree" "$GATE"
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
    # shellcheck disable=SC2016  # the predicate's own message, backticks and all
    gate_red_because 'implements `PartialEq` for `Token`' env WCH_GATE_ROOT="$tree" "$GATE"
}

# The one-word version of the same thing.
fail_case_the_token_derives_a_comparison() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/^pub struct Token {/#[derive(PartialEq, Eq, Hash)]\npub struct Token {/' \
        "$tree/crates/daemon/src/http/token.rs"
    # `derives` and not `implements`: the two are separate branches with separate sentences, and
    # an arm that claimed the hand-written one would have gone on passing over a predicate that
    # had stopped reading `#[derive(...)]` at all.
    # shellcheck disable=SC2016  # the predicate's own message, backticks and all
    gate_red_because 'derives `PartialEq` for `Token`' env WCH_GATE_ROOT="$tree" "$GATE"
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
    # shellcheck disable=SC2016  # the predicate's own message, backticks and all
    gate_red_because 'implements `Display` for `Token`' env WCH_GATE_ROOT="$tree" "$GATE"
}

# The conversion family, one arm per name. Each of these hands out a `&str`, a `String` or a
# wire rendering, and every one of them is a `==` on the secret at the call site with nothing in
# the diff for a reviewer to grep for.
#
# Appended below the file's `mod tests` on purpose: the trait rules read the whole file rather
# than its product half, so an arm seeded here is red under **claim 2 alone** — the reader
# reconciliation in claim 5 cannot see past a trailing test module, which is the residual
# `lib.sh` states and these arms are shaped not to lean on.
_impl_appended_to_the_token() {
    local body="$1" tree
    tree="$(gate_scratch_tree)"
    printf '\n%s\n' "$body" >>"$tree/crates/daemon/src/http/token.rs"
    printf '%s\n' "$tree"
}

# shellcheck disable=SC2016  # the predicate's own message, backticks and all, matched verbatim
fail_case_the_token_gains_an_as_ref() {
    local tree
    tree="$(_impl_appended_to_the_token 'impl AsRef<str> for Token {
    /// Seeded by the gate selftest.
    fn as_ref(&self) -> &str {
        &self.hex
    }
}')"
    gate_red_because 'implements `AsRef` for `Token`' env "WCH_GATE_ROOT=$tree" "$GATE"
}

# shellcheck disable=SC2016  # the predicate's own message, backticks and all, matched verbatim
fail_case_the_token_gains_a_borrow() {
    local tree
    tree="$(_impl_appended_to_the_token 'impl std::borrow::Borrow<str> for Token {
    /// Seeded by the gate selftest.
    fn borrow(&self) -> &str {
        &self.hex
    }
}')"
    gate_red_because 'implements `Borrow` for `Token`' env "WCH_GATE_ROOT=$tree" "$GATE"
}

# shellcheck disable=SC2016  # the predicate's own message, backticks and all, matched verbatim
fail_case_the_token_gains_a_deref() {
    local tree
    tree="$(_impl_appended_to_the_token 'impl std::ops::Deref for Token {
    type Target = str;

    /// Seeded by the gate selftest.
    fn deref(&self) -> &str {
        &self.hex
    }
}')"
    gate_red_because 'implements `Deref` for `Token`' env "WCH_GATE_ROOT=$tree" "$GATE"
}

# shellcheck disable=SC2016  # the predicate's own message, backticks and all, matched verbatim
fail_case_the_token_gains_a_to_string() {
    local tree
    tree="$(_impl_appended_to_the_token 'impl ToString for Token {
    /// Seeded by the gate selftest.
    fn to_string(&self) -> String {
        self.hex.clone()
    }
}')"
    gate_red_because 'implements `ToString` for `Token`' env "WCH_GATE_ROOT=$tree" "$GATE"
}

# shellcheck disable=SC2016  # the predicate's own message, backticks and all, matched verbatim
fail_case_the_token_gains_an_into() {
    local tree
    tree="$(_impl_appended_to_the_token 'impl Into<String> for Token {
    /// Seeded by the gate selftest.
    fn into(self) -> String {
        self.hex
    }
}')"
    gate_red_because 'implements `Into` for `Token`' env "WCH_GATE_ROOT=$tree" "$GATE"
}

# The one the workspace would most plausibly grow: a `Token` that serialises is a `Token` in a
# state file, a log line or a JSON answer.
# shellcheck disable=SC2016  # the predicate's own message, backticks and all, matched verbatim
fail_case_the_token_gains_a_serialize() {
    local tree
    tree="$(_impl_appended_to_the_token 'impl serde::Serialize for Token {
    /// Seeded by the gate selftest.
    fn serialize<S: serde::Serializer>(&self, out: S) -> Result<S::Ok, S::Error> {
        out.serialize_str(&self.hex)
    }
}')"
    gate_red_because 'implements `Serialize` for `Token`' env "WCH_GATE_ROOT=$tree" "$GATE"
}

# The other direction, and the reason the conversions need a rule of their own: this impl is
# written `for String`, so every rule spelled `for Token` looks straight past it.
# shellcheck disable=SC2016  # the predicate's own message, backticks and all, matched verbatim
fail_case_the_token_converts_into_a_string() {
    local tree
    tree="$(_impl_appended_to_the_token 'impl From<Token> for String {
    /// Seeded by the gate selftest.
    fn from(token: Token) -> String {
        token.hex
    }
}')"
    gate_red_because 'implements `From<Token>`' env "WCH_GATE_ROOT=$tree" "$GATE"
}

# ------------------------------------------------- claim 3: `Debug` is hand-written and redacts

# The derive prints the field, and the field is the key to a camera. This is the half no test
# can hold: a build where the hand-written impl was replaced by the attribute would print the
# token from any `tracing::debug!(?token)` written three sub-milestones from now.
fail_case_the_token_derives_its_debug() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/^pub struct Token {/#[derive(Debug)]\npub struct Token {/' \
        "$tree/crates/daemon/src/http/token.rs"
    # shellcheck disable=SC2016  # the predicate's own message, backticks and all
    gate_red_because 'derives `Debug` for `Token`' env WCH_GATE_ROOT="$tree" "$GATE"
}

# A hand-written impl that prints the field is the derive's behaviour typed out by hand. The
# field name is spelled here because a seed has to be a real edit to a real declaration; if it
# is ever renamed this arm stops seeding anything and the selftest says so out loud, which is
# the loud direction for a seeding failure to take.
fail_case_the_debug_impl_prints_the_field() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/\.field("hex", &Redacted)/.field("hex", \&self.hex)/' \
        "$tree/crates/daemon/src/http/token.rs"
    # shellcheck disable=SC2016  # the predicate's own message, backticks and all
    gate_red_because 'the hand-written `Debug` for `Token` in crates/daemon/src/http/token.rs names `self.hex`' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# The impl moved somewhere this gate is not looking, which reads exactly like a type that never
# had one — and a `Token` with no hand-written `Debug` is a `Token` whose `Debug` is derived or
# absent, both of which are the defect.
fail_case_the_hand_written_debug_went_away() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/^impl std::fmt::Debug for Token {/impl std::fmt::Debug for TokenRedaction {/' \
        "$tree/crates/daemon/src/http/token.rs"
    # shellcheck disable=SC2016  # the predicate's own message, backticks and all
    gate_red_because 'has no hand-written `impl … Debug for Token`' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# ------------------------------------------------- claim 4: something still compares

# A tree where the gate stopped calling the comparison satisfies every claim above and serves
# a live camera to whoever asks. `kill-is-never-a-fallback.sh`'s "the only caller went away"
# arm, about a different absence.
fail_case_the_gate_no_longer_calls_the_comparison() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/token\.verify(candidate)/!candidate.is_empty()/g' \
        "$tree/crates/daemon/src/http/gate.rs"
    # shellcheck disable=SC2016  # the predicate's own message, backticks and all
    gate_red_because "nothing in crates/daemon/src/http/gate.rs's product code calls \`.verify(\`" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_comparison_is_no_longer_declared() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/pub fn verify(/fn verify_disabled(/' "$tree/crates/daemon/src/http/token.rs"
    gate_red_because 'no longer declares the constant-time comparison' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# ------------------------------------------------- claim 5: the field's readers are registered

# **The arm this claim exists for.** A second inherent accessor is not a banned trait, does not
# name `expose_secret`, and leaves claims 1 to 4 exactly as green as they are on the shipped
# tree — while `token.as_str() == candidate` in `gate.rs` is the short-circuiting comparison the
# whole file is about. Seeded above the type's own `verify`, which is where a second accessor
# would actually be written.
# shellcheck disable=SC2016  # the predicate's own message, backticks and all, matched verbatim
fail_case_the_home_grows_a_second_accessor() {
    local tree file
    tree="$(gate_scratch_tree)"
    file="$tree/crates/daemon/src/http/token.rs"
    # The first `#[must_use]` in the file, which is the one above `expose_secret`: `0,/re/`
    # bounds the substitution to one match, because a seed applied at every accessor would be
    # three copies of one function and a seed nobody can read.
    gate_seed '0,/^    #\[must_use\]$/s||    /// Seeded by the gate selftest.\n    pub fn as_str(\&self) -> \&str {\n        \&self.hex\n    }\n\n\&|' "$file"
    gate_red_because 'inside `as_str`' env "WCH_GATE_ROOT=$tree" "$GATE"
}

# The same hole written as a free function beside the type, which is the shape `self` alone would
# not have seen.
# shellcheck disable=SC2016  # the predicate's own message, backticks and all, matched verbatim
fail_case_the_home_grows_a_free_reader_of_the_field() {
    local tree file
    tree="$(gate_scratch_tree)"
    file="$tree/crates/daemon/src/http/token.rs"
    gate_seed 's|^impl std::fmt::Debug for Token {|/// Seeded by the gate selftest.\npub fn secret_of(token: \&Token) -> \&str {\n    \&token.hex\n}\n\n\&|' "$file"
    gate_red_because 'inside `secret_of`' env "WCH_GATE_ROOT=$tree" "$GATE"
}

# The register's other direction, and it is the one `unsafe-scope.sh` pays for too: an entry with
# nothing behind it. A `verify` that compares the *accessor's* answer rather than the field reads
# no field, and that is a comparison one refactor away from comparing something that is not the
# secret — with claim 1 silent, because the home is the one file allowed to name the accessor.
# shellcheck disable=SC2016  # the predicate's own message, backticks and all, matched verbatim
fail_case_the_comparison_stops_reading_the_field() {
    local tree file
    tree="$(gate_scratch_tree)"
    file="$tree/crates/daemon/src/http/token.rs"
    gate_seed 's/let expected = self\.hex\.as_bytes();/let expected = self.expose_secret().as_bytes();/' "$file"
    gate_red_because '`verify` in crates/daemon/src/http/token.rs no longer reads' \
        env "WCH_GATE_ROOT=$tree" "$GATE"
}

# The green direction for the same claim, and it is the product/test split again: the home's own
# tests may hold the field. `token.rs`'s suite builds near misses and asserts lengths, and a
# reconciliation that counted those would be a gate somebody turns off — the same argument the
# first green arm in this file makes about `gate.rs`.
pass_case_a_test_in_the_home_may_read_the_field() {
    local tree file
    tree="$(gate_scratch_tree)"
    file="$tree/crates/daemon/src/http/token.rs"
    # `$i` is `sed`'s insert command, not a shell expansion (see the first green arm above).
    # shellcheck disable=SC2016
    gate_seed '$i\
\
    #[test]\
    fn seeded_by_the_gate_selftest() {\
        let token = minted();\
        assert_eq!(token.hex.len(), TOKEN_BYTES * 2);\
    }' "$file"
    WCH_GATE_ROOT="$tree" "$GATE"
}

# ------------------------------------------------- claim 6: the URL rendering's callers
#
# **The arm this claim exists for** (G6/L13, note **N183**). A fourth module that renders the
# ready-to-open URL is holding the token in a `String`, with no banned trait, no `expose_secret`
# and every other claim in this file green — which is exactly the state the tree was in when the
# review found the daemon writing that URL into the systemd journal.
# shellcheck disable=SC2016  # the predicate's own message, backticks and all, matched verbatim
fail_case_a_fourth_module_renders_the_url() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/daemon/src/http/banner.rs" <<'RS'
//! Seeded by the gate selftest: the URL that carries the token has a register of callers.

use super::token::Token;

pub(crate) fn announce(token: &Token, bound: std::net::SocketAddr) -> String {
    format!("open {}", token.ready_to_open_url(bound))
}
RS
    gate_red_because 'is not one of this gate' env "WCH_GATE_ROOT=$tree" "$GATE"
}

# The register's other direction: an entry with nothing behind it. D11 asks for the URL to be
# printed, so a composition root that stopped rendering it is either a daemon that no longer
# tells an operator how to reach its web client, or a rendering that has moved somewhere this
# gate is not looking.
# shellcheck disable=SC2016  # the predicate's own message, backticks and all, matched verbatim
fail_case_the_line_that_prints_the_url_stopped_calling_it() {
    local tree file
    tree="$(gate_scratch_tree)"
    file="$tree/crates/daemon/src/main.rs"
    gate_seed 's/web\.ready_to_open_url()/web.bound().to_string()/' "$file"
    gate_red_because 'registered as the one line D11 asks for' env "WCH_GATE_ROOT=$tree" "$GATE"
}

# **The revert this claim was widened for** (note **N185**). D11's URL still gets printed, the
# register still has three entries with three call sites behind them, and every other claim in
# this file is green — the only thing that changed is that the composition root hands `tracing`
# the whole URL again, so under systemd the run's bearer token goes back into a persistent
# `systemd-journal`/`adm`-readable store (note **N182**). Nothing in this workspace could go red
# on it before: `logging`'s tests and `token.rs`'s drive the redaction directly, and `main.rs` is
# a binary no integration suite can call into.
# shellcheck disable=SC2016  # the predicate's own message, backticks and all, matched verbatim
fail_case_the_line_that_prints_the_url_stopped_redacting_it() {
    local tree file
    tree="$(gate_scratch_tree)"
    file="$tree/crates/daemon/src/main.rs"
    gate_seed 's/logging\.url_this_sink_may_keep(&web\.ready_to_open_url())/web.ready_to_open_url()/' \
        "$file"
    gate_red_because 'product line(s) that do not also name' env "WCH_GATE_ROOT=$tree" "$GATE"
}

# The same claim from the register's other end: the redaction renamed out from under the entry
# that requires it. A gate that answered this with a message about `main.rs` would be naming a
# call site for a fact about a declaration, which is the wrong subject (note N60).
# shellcheck disable=SC2016  # the predicate's own message, backticks and all, matched verbatim
fail_case_the_redaction_the_composition_root_renders_through_was_renamed() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/\burl_this_sink_may_keep\b/url_for_this_sink/g' \
        "$tree/crates/daemon/src/logging.rs" "$tree/crates/daemon/src/main.rs" \
        "$tree/crates/daemon/src/http/token.rs"
    gate_red_because 'no longer declares' env "WCH_GATE_ROOT=$tree" "$GATE"
}

# The name itself, which every claim above quantifies over: renamed, the URL has no declaration
# to be about and the register has three entries naming nothing.
fail_case_the_url_rendering_was_renamed() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/pub fn ready_to_open_url(/pub fn printable_url(/' \
        "$tree/crates/daemon/src/http/token.rs"
    gate_red_because 'no longer declares the URL rendering claim 6 registers the callers of' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# The green direction, and the reason claim 6 is a register rather than a confinement: every
# suite that drives the web listener opens the URL it prints, and an integration test is test
# code from its first line — cargo compiles `tests/*.rs` only under `cargo test`, so there is no
# `#[cfg(test)]` marker in one to find (`lib.sh`'s `gate_test_region_start`). A gate that made a
# suite register itself would be a gate somebody turns off.
pass_case_a_suite_may_open_the_url_and_hold_the_secret() {
    local tree
    tree="$(gate_scratch_tree)"
    cat >"$tree/crates/daemon/tests/seeded_by_the_gate_selftest.rs" <<'RS'
//! Seeded by the gate selftest: an integration suite is test code in its entirety.

#[test]
fn the_url_a_suite_opens_carries_the_secret_it_was_minted_with() {
    let token = daemon::http::Token::mint().expect("the kernel has a CSPRNG");
    let bound = "127.0.0.1:34567".parse().expect("a bound address");
    assert!(token.ready_to_open_url(bound).contains(token.expose_secret()));
}
RS
    WCH_GATE_ROOT="$tree" "$GATE"
}

# ------------------------------------------------- the subjects themselves

# A renamed type takes every claim in this file with it: no declaration to read attributes off,
# no fields for the `Debug` claim to refuse, and nothing for an `impl PartialEq` to be *for*.
fail_case_the_token_type_was_renamed() {
    local tree
    tree="$(gate_scratch_tree)"
    gate_seed 's/\bpub struct Token\b/pub struct Bearer/' "$tree/crates/daemon/src/http/token.rs"
    gate_red_because 'no longer declares the type that holds the secret' \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

fail_case_the_token_module_moved() {
    local tree http
    tree="$(gate_scratch_tree)"
    http="$tree/crates/daemon/src/http"
    mv "$http/token.rs" "$http/bearer.rs"
    gate_red_because "crates/daemon/src/http/token.rs is missing; the token's home and the gate that calls its comparison" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# The module this confinement is chiefly about. Without it in the tree, the walk finds no
# product-code reader of the secret for the honest reason and the dishonest one at once.
fail_case_the_token_gate_module_moved() {
    local tree http
    tree="$(gate_scratch_tree)"
    http="$tree/crates/daemon/src/http"
    mv "$http/gate.rs" "$http/policy.rs"
    gate_red_because "crates/daemon/src/http/gate.rs is missing; the token's home and the gate that calls its comparison" \
        env WCH_GATE_ROOT="$tree" "$GATE"
}

# Seeded through the metadata rather than by deleting the crate, because deleting it makes
# `cargo metadata` itself fail — and an arm that goes red before the predicate runs proves
# nothing about the predicate.
fail_case_the_daemon_left_the_workspace() {
    local md
    md="$(gate_metadata_snapshot)"
    jq 'del(.packages[] | select(.name == "webcam-handler-daemon"))' "$md" >"$md.seeded"
    gate_red_because 'webcam-handler-daemon is not a workspace member; the token, its comparison and the gate that calls it have no home' \
        env WCH_GATE_METADATA="$md.seeded" "$GATE"
}
