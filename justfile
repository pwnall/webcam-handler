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

# ---------------------------------------------------------------- rungs

# R2 — the vivid virtual-driver rung. Runs where the module is loadable; reports a
# named, counted skip elsewhere (never silence).
rung-vivid:
    ./scripts/rung-vivid.sh

# R3 — real hardware. `#[ignore]`d suites, opt-in, motor-moving sweeps excluded
# unless WCH_ALLOW_MOTION=1.
smoke-hw:
    ./scripts/smoke-hw.sh

# Miri over the unsafe-adjacent pure decode units (design §2.5).
miri:
    ./scripts/miri.sh

# Regenerate committed generated artifacts (JSON Schema bundle, OpenRPC, completions,
# man pages). `schema-artifacts-current.sh` proves the committed copies match.
generate:
    cargo run --locked -p webcam-handler-xtask -- generate
