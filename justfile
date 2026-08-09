# webcam-handler task runner.
#
# `just ci` is what CI runs, verbatim, and it runs offline. Every phase gate is a
# named, counted, re-runnable recipe (`just gate-g0` … `just gate-g6`).

set shell := ["bash", "-euo", "pipefail", "-c"]

_default:
    @just --list

# Everything CI runs, in CI's order. Offline by construction: no `cargo update`, no
# advisory database fetch (that lives in the networked job).
ci: fmt-check lint test doc deny hygiene gates

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all --check

lint:
    cargo clippy --locked --workspace --all-targets -- -D warnings

test:
    cargo nextest run --locked --workspace --no-tests=fail

doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --locked --workspace --no-deps

deny:
    cargo deny --offline check bans licenses sources

hygiene:
    typos
    cargo machete --with-metadata
    shellcheck scripts/*.sh scripts/gates/*.sh scripts/gates/cases/*.sh

# Every gate predicate, then the self-test that proves each can go red.
gates:
    ./scripts/gates/run-all.sh
    ./scripts/gates/selftest.sh

# The self-test alone (both directions per predicate).
selftest:
    ./scripts/gates/selftest.sh

# ---------------------------------------------------------------- phase gates

# G0 — foundations: gates self-tested, schema round-trips, fake passes the battery.
gate-g0:
    ./scripts/gates/phase.sh g0

# G1 — the V4L2 read path.
gate-g1:
    ./scripts/gates/phase.sh g1

# G2 — writes and photo capture.
gate-g2:
    ./scripts/gates/phase.sh g2

# G3 — calibration.
gate-g3:
    ./scripts/gates/phase.sh g3

# G4 — daemon and daemon client.
gate-g4:
    ./scripts/gates/phase.sh g4

# G5 — the web client.
gate-g5:
    ./scripts/gates/phase.sh g5

# G6 — video recording.
gate-g6:
    ./scripts/gates/phase.sh g6

# ------------------------------------------------------- the privileged helper

# Where the blessed copy lives. Outside `target/` on purpose: writing a binary file
# strips its capabilities, and cargo rewrites `target/<profile>/wch-priv` for reasons
# unrelated to its source (a RUSTFLAGS re-fingerprint, a profile change). The copy under
# `.wch-bin/` keeps its caps across all that churn, so a re-bless is rare.
priv_bin := ".wch-bin/wch-priv"

# Idempotent: the stamp records the freshly-built binary's sha256, and the bless is
# skipped — no password prompt — until that changes. But the stamp alone would be a FALSE
# skip if the blessed copy lost its caps or its mode out of band (an rsync, a
# backup-restore, a filesystem move all strip xattrs), so the skip also re-verifies both.
# Reporting "already blessed" over a copy that is effectively un-capped is skip-reads-as-
# pass wearing a filesystem.
#
# Grant `wch-priv` the capabilities the dev loop needs. Needs sudo once.
bless:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build --locked --offline -p webcam-handler-priv
    built="target/debug/wch-priv"
    stable="{{priv_bin}}"
    stamp="$(dirname "$stable")/.blessed"
    mkdir -p "$(dirname "$stable")"

    # The capability list has one home: the binary itself. Asking it means the bless and
    # the runtime check cannot drift apart.
    blessing="$("$built" doctor --setcap-argument)"
    want_caps="${blessing%%+*}"
    want_flags="${blessing##*+}"

    h="$(sha256sum "$built" | cut -d' ' -f1)"
    caps_now="$(getcap "$stable" 2>/dev/null || true)"
    mode_now="$(stat -c %a "$stable" 2>/dev/null || true)"
    # Every capability the binary asks for, plus the flag set, read out of `getcap` —
    # derived from the blessing rather than transcribed here, so the two cannot disagree
    # about what "already blessed" means. `getcap` prints `=ep` where setcap took `+ep`.
    caps_ok=1
    for want in ${want_caps//,/ }; do
        [[ "$caps_now" == *"$want"* ]] || caps_ok=0
    done
    [[ "$caps_now" == *"=$want_flags" ]] || caps_ok=0

    if [[ -f "$stamp" && -f "$stable" && "$(cat "$stamp")" == "$h" \
          && "$mode_now" == "700" && "$caps_ok" == "1" ]]; then
        echo "bless: $stable already blessed (sha256 unchanged, caps +$want_flags, mode 0700); skipping setcap"
        exit 0
    fi

    # Staged, then moved into place only once it is actually blessed. Writing $stable
    # directly means a bless that cannot finish — no terminal for sudo, a wrong password,
    # a Ctrl-C — replaces a *working* helper with an un-capped one. It fails closed, which
    # is the right direction, but it leaves you worse off than before you ran it. Observed,
    # then fixed.
    #
    # `mv` within one directory is a rename: the inode is untouched, so the capabilities
    # set below survive the move. `cp` would not — writing a file strips its xattrs, which
    # is the same fact that puts $stable outside target/ in the first place.
    staged="$stable.staging"
    trap 'rm -f "$staged"' EXIT
    cp -f "$built" "$staged"
    # Mode BEFORE setcap, and this ordering is the security boundary rather than a
    # detail: between the copy and the chmod the file is world-executable, and after the
    # setcap it would be world-executable *and* root-capable. Narrow it first.
    chmod 0700 "$staged"
    if ! sudo setcap "$blessing" "$staged"; then
        echo "bless: setcap failed; $stable is unchanged" >&2
        exit 1
    fi
    mv -f "$staged" "$stable"
    echo "$h" >"$stamp"
    echo "bless: $stable (re)blessed — $want_caps, mode 0700, owner only"
    # The last word goes to the binary, which *performs* an ambient raise rather than
    # predicting one — so a green line here means delegation has actually been exercised,
    # not merely that `getcap` looks right. That distinction is why this recipe ends here.
    "$stable" doctor

# What the helper can currently do, and why not if it cannot.
priv-doctor:
    @{{priv_bin}} doctor 2>/dev/null || cargo run --locked --offline -q -p webcam-handler-priv -- doctor

# ---------------------------------------------------------------- rungs

# Runs where the module is loadable; reports a named, counted skip elsewhere.
#
# R2 — the vivid virtual-driver rung.
rung-vivid:
    ./scripts/rung-vivid.sh

# Separate from `rung-vivid` on purpose: that recipe is what CI and a camera-less
# contributor run, and it must keep working with no privileged helper in sight.
#
# R2, loading vivid with the blessed helper first and unloading it after.
rung-vivid-managed:
    #!/usr/bin/env bash
    set -euo pipefail
    {{priv_bin}} vivid up --devices 1
    trap '{{priv_bin}} vivid down || true' EXIT
    ./scripts/rung-vivid.sh

# Motor-moving suites included by default (owner ruling, 2026-08-08); WCH_NO_MOTION=1
# excludes them for runs where the camera is pointed at someone.
#
# R3 — real hardware, on the ignore-attributed suites.
smoke-hw:
    ./scripts/smoke-hw.sh

# Miri over the unsafe-adjacent pure decode units (design §2.5).
miri:
    ./scripts/miri.sh

# The mutation floor over the pure cores (docs/7 P3f). Hours, not minutes — it rebuilds
# the workspace once per mutant — so it is a rung and a G4 criterion, never a `just ci`
# step. cargo-mutants is a dev tool: without it the recipe reports a named, counted skip.
# Extra arguments reach cargo-mutants, which is how a triage session narrows a re-run
# (`just mutants -F store.rs`); the scope itself lives in `.cargo/mutants.toml`.
mutants *args:
    ./scripts/mutants.sh {{args}}

# Regenerate committed generated artifacts (JSON Schema bundle, OpenRPC, completions,
# man pages). `schema-artifacts-current.sh` proves the committed copies match.
generate:
    cargo run --locked -p webcam-handler-xtask -- generate
