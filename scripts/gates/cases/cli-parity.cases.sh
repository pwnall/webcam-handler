# Both-direction cases for `cli-parity.sh`.
#
# The subject is *two running programs agreeing*, so the arms come in two shapes and the
# distinction matters more here than usual.
#
# **The defect the row exists for is a fork in the shared surface**, and there is exactly one
# honest way to seed it: edit the shared render path and build the two binaries. That arm costs
# a compile and it is the arm rubric rule 6 requires — a stub `webcam-handler-cli` and a stub
# `webcam-handler-client` that disagreed would prove that this predicate can spot a `diff`,
# which nobody doubts, and nothing at all about whether `webcam-handler-cli-core` can fork
# under the two roots that consume it [S:N10]. It edits **Rust**, so it builds into a target
# directory of its own (note **N32**: cargo decides freshness by mtime and `gate_scratch_tree`
# preserves them, so a mutated build sharing the checkout's `target/` becomes the checkout's
# build). Its price, stated rather than discovered: ~70 s and ~2 GB the first time, ~2 s
# afterwards, because `target/gate-selftest/cli-parity-fork/` persists between runs — the same
# trade `schema-artifacts-current.cases.sh` makes for its `wire-drift` arm, and the reason both
# live under `target/`, which every tree-walking gate and every scratch copy prunes.
#
# **The defects about the population** are seeded in a copy of the predicate's own row table,
# which is `json-validates.cases.sh`'s idiom and its reason: the table is the one thing here
# that is written down rather than derived, so the inverse of "every leaf falls in a named
# bucket" is a leaf whose row is gone — and seeding that in the *tree* would mean building a
# `webcam-handler-cli` with an eighteenth verb, per case. The mutated predicate still drives
# the real binaries, the real corpus and a real daemon; only the list under test is doctored.
#
# **The defects about the fixture** drive `$WCH_GATE_WCHD` and `$WCH_GATE_WCHC` at programs
# that get one thing wrong each: a daemon that is not there, a daemon that says it is there and
# is not, and a `webcam-handler-client` that cannot serve a verb `webcam-handler-cli` offers —
# which is what a local-only verb *is*, and the arm that keeps docs/9's "none at P4" a tested
# claim rather than a remembered one.
#
# Every arm asserts the failure **message** and not merely the status, because several of
# these seeds are red under more than one branch and the harness reads only the status (note
# **N31**).
#
# shellcheck shell=bash

pass_case() {
    "$GATE"
}

# A copy of the predicate with one `sed` applied to its row table, run against the real tree.
#
#   $1  a `sed` script over the table
#   $2  a string that must now BE in the mutant, or empty
#   $3  a string that must have *vanished* from the mutant, or empty
#
# Both directions of that check exist because a seed that silently stopped applying is an arm
# that reports a red it did not cause — and the predicate under test here is red under more
# than one branch, so the status alone would not notice. On a seed that did not take it
# prints `/bin/true`, which the caller runs, which exits 0, which the harness reports as a
# problem: the one outcome that cannot be mistaken for a working arm.
_mutated_predicate() {
    local script="$1" present="$2" gone="$3" dir mutant
    dir="$(mktemp -d "$(gate_scratch_root)/wch-parity-rows.XXXXXXXX")"
    mutant="$dir/$(basename "$GATE")"
    cp "$(dirname "$GATE")/lib.sh" "$dir/lib.sh"
    sed "$script" "$GATE" >"$mutant"
    chmod +x "$mutant"
    if [[ -n "$gone" ]] && grep -Fq -- "$gone" "$mutant"; then
        printf 'selftest: the seed did not apply — %s is still in the mutant\n' "$gone" >&2
        printf '%s\n' "/bin/true"
        return
    fi
    if [[ -n "$present" ]] && ! grep -Fq -- "$present" "$mutant"; then
        printf 'selftest: the seed did not apply — %s is not in the mutant\n' "$present" >&2
        printf '%s\n' "/bin/true"
        return
    fi
    printf '%s\n' "$mutant"
}

# ------------------------------------------------------------------ the fork
#
# The one this row is named for. `cli_core::render::json` is the single emitter behind every
# `--json` answer, and the seed makes it depend on which binary is running — one trailing byte
# under `webcam-handler-client` and none under `webcam-handler-cli`. That is the smallest
# possible version of the defect docs/9 describes as "the T4 single-surface claim silently
# forking", and it is invisible to every other gate: the document still validates against the
# bundle, the schema artifacts are unmoved, and each binary on its own looks perfect.
#
# The daemon stays the shipped one. What is under test is the two clients, and a mutated
# daemon would be a different claim.
fail_case_the_two_roots_render_one_document_two_ways() {
    local tree target
    tree="$(gate_scratch_tree)"
    target="$(gate_root)/target/gate-selftest/cli-parity-fork"
    mkdir -p "$target"

    # Anchored, and the anchor is checked below: `out.line(Stream::Stdout, &text)` appears
    # three times in that file and only one of them is the `--json` emitter.
    sed -i 's|^    out\.line(Stream::Stdout, &text)$|    let text = if std::env::args().next().is_some_and(\|a\| a.ends_with("webcam-handler-client")) { format!("{text} ") } else { text };\n    out.line(Stream::Stdout, \&text)|' \
        "$tree/crates/cli-core/src/render.rs"
    if ! grep -q 'ends_with("webcam-handler-client")' "$tree/crates/cli-core/src/render.rs"; then
        printf 'selftest: the fork was not seeded into the render path\n' >&2
        return 0
    fi

    if ! (cd "$tree" && CARGO_TARGET_DIR="$target" cargo build --locked --offline \
        -p webcam-handler-cli --bin webcam-handler-cli -p webcam-handler-client --bin webcam-handler-client >/dev/null 2>&1); then
        printf 'selftest: the forked tree did not build, so this arm proved nothing\n' >&2
        return 0
    fi

    gate_red_because 'these two renderings have forked' \
        env "WCH_GATE_WCH=$target/debug/webcam-handler-cli" "WCH_GATE_WCHC=$target/debug/webcam-handler-client" "$GATE"
}

# ------------------------------------------------------------------ the population

# The completeness half, straight out of the P3 review's finding: a leaf the surface offers
# and the table does not name. `snapshot` rather than a compared row, because a dropped
# *exempt* row is the case that looks harmless — nothing stops answering and no comparison
# changes; all that happens is that a verb quietly stops being accounted for.
fail_case_a_leaf_verb_that_no_row_puts_in_a_bucket() {
    local mutant
    mutant="$(_mutated_predicate '/^    "snapshot|/d' '' '"snapshot|')"
    gate_red_because "the surface offers 'snapshot' and no row above puts it in a bucket" \
        bash "$mutant"
}

# A read verb dropped from the comparison, by the route somebody would actually take: not
# deleting the row (the arm above catches that) but *relabelling* it, so the table still looks
# complete. It goes red on the exemption's own consequence — `webcam-handler-cli get` answers
# happily beside a running daemon, because reads take no lock, so "the store lock is what
# stands between these two roots" is false about it.
fail_case_a_read_verb_moved_quietly_into_the_session_exemption() {
    local mutant
    mutant="$(_mutated_predicate 's/^    "get|compared|/    "get|session|/' '"get|session|' '"get|compared|')"
    gate_red_because "webcam-handler-cli ran 'get' while a daemon held" bash "$mutant"
}

# The same move into the other exemption, which fails for the opposite reason: `info` has no
# stamp, so the two roots agree byte for byte and there is nothing for the exemption to be
# about. Without this, `stamped` would be a place to hide any read verb at all.
fail_case_a_read_verb_moved_quietly_into_the_stamped_exemption() {
    local mutant
    mutant="$(_mutated_predicate 's/^    "info|compared|/    "info|stamped|/' '"info|stamped|' '"info|compared|')"
    gate_red_because "'info' is byte-identical from both roots, so nothing about it needs exempting" \
        bash "$mutant"
}

# The bucket vocabulary, and the reason it is closed. A misspelt bucket in a table read by a
# `case` is a verb that falls through every arm — driven by nothing, compared with nothing,
# and counted as present because its row exists. This is note N10's family in one character.
fail_case_a_bucket_name_outside_the_closed_vocabulary() {
    local mutant
    mutant="$(_mutated_predicate 's/^    "get|compared|/    "get|read-only|/' '"get|read-only|' '"get|compared|')"
    gate_red_because "'get' is in bucket 'read-only'" bash "$mutant"
}

# The other direction of the population check: a row naming a verb the surface does not
# offer. A table nobody re-derives drifts this way — the verb goes, the row stays, and the
# count keeps reporting a population that includes a verb nothing can run.
fail_case_a_row_naming_a_verb_the_surface_does_not_offer() {
    local mutant
    mutant="$(_mutated_predicate 's/^    "list|compared|list"/    "list|compared|list"\n    "frobnicate|compared|frobnicate"/' '"frobnicate|compared|' '')"
    gate_red_because "a row above names 'frobnicate', which webcam-handler-cli --help does not offer" \
        bash "$mutant"
}

# ------------------------------------------------------------------ the fixture

# The arm that keeps "no local-only verbs at P4" a **tested** claim. This
# `webcam-handler-client` is the real one for every verb but `profile capture`, which it
# declines the way a client would decline a verb that only makes sense in the process that owns
# the camera. The predicate must name it rather than shrug: a verb one root cannot run is a
# local-only verb, and docs/9 requires it named in the table instead of discovered here.
fail_case_a_verb_wchc_cannot_serve_is_a_local_only_verb() {
    local stub wchc
    wchc="$(git rev-parse --show-toplevel)/target/debug/webcam-handler-client"
    stub="$(mktemp "$(gate_scratch_root)/wch-stub-wchc.XXXXXXXX")"
    cat >"$stub" <<STUB
#!/usr/bin/env bash
set -euo pipefail
for arg in "\$@"; do
    if [[ "\$arg" == capture ]]; then
        printf 'webcam-handler-client: profile capture runs in the process that owns the camera\n' >&2
        exit 1
    fi
done
exec "$wchc" "\$@"
STUB
    chmod +x "$stub"
    gate_red_because "webcam-handler-client cannot answer 'profile capture', which webcam-handler-cli offers" \
        env "WCH_GATE_WCHC=$stub" "$GATE"
}

# A daemon that never serves. Without the refusal this seeds, the gate would go on to run both
# roots, watch `webcam-handler-client` fail every row, and have to decide what that means — and
# the one answer it must never give is "pass" (AGENTS.md rule 3).
fail_case_a_daemon_that_never_serves_leaves_nothing_to_compare() {
    local stub
    stub="$(mktemp "$(gate_scratch_root)/wch-stub-wchd.XXXXXXXX")"
    cat >"$stub" <<'STUB'
#!/usr/bin/env bash
printf 'webcam-handler-daemon cannot serve\n' >&2
exit 1
STUB
    chmod +x "$stub"
    gate_red_because 'no daemon is serving' env "WCH_GATE_WCHD=$stub" "$GATE"
}

# The subtler half, and the one a readiness line alone would miss: a daemon that *announces*
# the socket and then goes away. Nothing is listening, so every `webcam-handler-client` row
# fails — and the failure this arm asserts is the comparison's, not the fixture's, which is how
# the two are told apart in the output.
fail_case_a_daemon_that_announces_a_socket_and_is_not_there() {
    local stub app_dir socket_file root
    root="$(gate_root)"
    app_dir="$(sed -n 's/^pub const APP_DIR: &str = "\([^"]*\)".*/\1/p' \
        "$root/crates/schema/src/paths.rs" | head -n1)"
    socket_file="$(sed -n 's/^pub const DAEMON_SOCKET_FILE: &str = "\([^"]*\)".*/\1/p' \
        "$root/crates/schema/src/limits.rs" | head -n1)"
    stub="$(mktemp "$(gate_scratch_root)/wch-stub-wchd.XXXXXXXX")"
    cat >"$stub" <<STUB
#!/usr/bin/env bash
set -euo pipefail
dir="\$XDG_RUNTIME_DIR/$app_dir"
mkdir -p "\$dir"
chmod 0700 "\$dir"
printf 'webcam-handler-daemon is serving socket=%s\n' "\$dir/$socket_file" >&2
STUB
    chmod +x "$stub"
    gate_red_because 'webcam-handler-client --json list exited' env "WCH_GATE_WCHD=$stub" "$GATE"
}
