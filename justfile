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
# strips its capabilities, and cargo rewrites `target/<profile>/webcam-handler-priv` for
# reasons unrelated to its source (a RUSTFLAGS re-fingerprint, a profile change). The copy
# under `.wch-bin/` keeps its caps across all that churn, so a re-bless is rare.
#
# The directory keeps its short name: `.wch-bin` is a scratch location, and note N90's
# ruling is about the names of binaries and crates. What lives inside it is the renamed
# helper — and a tree blessed before that rename kept a stale `.wch-bin/wch-priv` that this
# recipe will not overwrite and that `privileged-helper.sh` did not look at. That was left as
# a curiosity when the rename landed and it was not one: the stale copy is root-capable, still
# mode 0700, and still carries the `exec` verb P6e deleted, so the narrowing would have had a
# working bypass sitting beside it. `privileged-helper.sh`'s sixth claim now walks the whole
# directory rather than the one name it expects (note **N126**).
priv_bin := ".wch-bin/webcam-handler-priv"

# Idempotent: the stamp records the freshly-built binary's sha256, and the bless is
# skipped — no password prompt — until that changes. But the stamp alone would be a FALSE
# skip if the blessed copy lost its caps or its mode out of band (an rsync, a
# backup-restore, a filesystem move all strip xattrs), so the skip also re-verifies both.
# Reporting "already blessed" over a copy that is effectively un-capped is skip-reads-as-
# pass wearing a filesystem.
#
# Grant `webcam-handler-priv` the capabilities the dev loop needs. Needs sudo once.
bless:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build --locked --offline -p webcam-handler-priv
    built="target/debug/webcam-handler-priv"
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
    # **Exactly** the capabilities the binary asks for, and the flag set — both read out of
    # `getcap` and derived from the blessing rather than transcribed here, so the two cannot
    # disagree about what "already blessed" means. `getcap` prints `<path> cap_a,cap_b=ep`
    # where setcap took `+ep`, so the capability list is the last field and the flags follow
    # its `=`; both lists are sorted before comparison because getcap orders by capability
    # number and the source orders by argument.
    #
    # **"Exactly" is P6e's word** (note N125). The old test asked whether each wanted
    # capability was *present*, which every superset satisfies — so a copy carrying more than
    # the binary asks for was reported as "already blessed" and skipped, and the narrowing
    # this recipe now grants would have been undone on disk by anything that ran one `setcap`.
    # A grant nobody argued for is the whole class N8 exists about, and a skip that reads as
    # pass is how it would have survived. A wider grant now re-blesses down to the narrow one.
    caps_ok=0
    if [[ -n "$caps_now" ]]; then
        carried="${caps_now##* }"
        carried_caps="$(tr ',' '\n' <<<"${carried%%=*}" | sort | paste -sd, -)"
        carried_flags="${carried##*=}"
        want_sorted="$(tr ',' '\n' <<<"$want_caps" | sort | paste -sd, -)"
        if [[ "$carried_caps" == "$want_sorted" && "$carried_flags" == "$want_flags" ]]; then
            caps_ok=1
        fi
    fi

    if [[ -f "$stamp" && -f "$stable" && "$(cat "$stamp")" == "$h" \
          && "$mode_now" == "700" && "$caps_ok" == "1" ]]; then
        echo "bless: $stable already blessed (sha256 unchanged, caps exactly $want_caps+$want_flags, mode 0700); skipping setcap"
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

# Unlike the two rungs below it this suite is not `#[ignore]`d: design §3.1 puts it on "every
# push where the host has node", so `just ci`'s `test` step already runs it and
# `.config/nextest.toml` makes its decline visible there. This recipe is the *accounting* — it
# runs the same one binary and ends on a named verdict, RAN or SKIPPED, which is what
# `just gate-g5` records.
#
# R1-web — the pinned Playwright + Chromium browser rung.
rung-web:
    ./scripts/rung-web.sh

# Reaches the network, which is why it is a recipe somebody runs rather than something a test
# does on their behalf: `npm ci` installs exactly the tree `package-lock.json` pins, and the
# Playwright CLI it just installed fetches exactly the browser build that CLI names — so
# neither half can drift to whatever is newest.
#
# What R1-web needs and the build does not: the pinned Playwright and the pinned Chromium.
rung-web-install:
    #!/usr/bin/env bash
    set -euo pipefail
    cd crates/daemon/tests/browser
    npm ci
    # The pinned CLI, not `npx`: `npx playwright` would happily resolve some other version and
    # fetch some other browser, which is the whole failure the pin exists to prevent.
    node node_modules/@playwright/test/cli.js install chromium

# Like `rung-web` and unlike the two below it, this suite is not `#[ignore]`d: its
# fake-generated half belongs on every push (design §3.1 names R0, R1, R1-web, R2 and R3 and no
# oracle letter, so this rung has a name and no letter), and `just ci`'s `test` step already runs
# it. This recipe is the *accounting* — it runs the same selection and ends on a named verdict,
# RAN or SKIPPED, which is what `just gate-g6` records. The real-camera half is part of R3 and
# runs under `just smoke-hw`.
#
# The container oracles — ffprobe and mpv over files this workspace wrote.
rung-oracles:
    ./scripts/rung-oracles.sh

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
    # The precondition, named rather than left to the shell. Until P6e this machine had always
    # had a blessed copy, so nobody had met what this recipe says without one: `.wch-bin/webcam-handler-priv:
    # No such file or directory`, exit 127, from a recipe whose actual requirement is one sudo
    # command. A fresh clone meets it on the first try. Found by removing the stale blessed
    # copies the narrowing reckoning turned up (note **N126**), which is the sort of thing only
    # a state change finds.
    #
    # It **refuses** rather than skipping, and the difference is the whole of AGENTS rule 3: a
    # caller who typed `-managed` asked for the module to be loaded, so answering zero would be
    # a skip that reads as a pass. `just rung-vivid` is the arm that legitimately declines — it
    # runs the same suite against whatever is already loaded and reports a named, counted skip
    # when that is nothing.
    if [[ ! -x "{{priv_bin}}" ]]; then
        echo "rung-vivid-managed: REFUSED — no blessed helper at {{priv_bin}}, so this recipe cannot load vivid" >&2
        echo "  loading a kernel module needs cap_sys_module, and nothing here can grant itself that" >&2
        echo "  remedy: \`just bless\` (needs sudo, once), then \`just priv-doctor\` to confirm" >&2
        echo "  or: \`just rung-vivid\`, which runs the same suite without loading anything and says so by name" >&2
        exit 1
    fi
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

# The mutation floor over the pure cores (docs/7 P3f), **in full**. Hours, not minutes — it
# rebuilds the workspace once per mutant — so it is a rung and a G4 criterion, never a
# `just ci` step. Two hours and thirteen minutes over 624 mutants on the P5e machine, and
# that number is here rather than only in the notes because `just gate-g4` runs this recipe
# as a criterion: a session that budgets for a gate needs to know which of its rows is the
# expensive one, and nothing in `phase-criteria.tsv` says so.
#
# This is the only mode that may answer PASS. It tests every mutant in scope, so a green run
# supports the negative claim — there is no unaccepted survivor here — which is the claim the
# G4 criterion buys.
#
# cargo-mutants is a dev tool: without it the recipe reports a named, counted skip. Extra
# arguments reach cargo-mutants, which is how a triage session narrows a re-run
# (`just mutants -F store.rs`); the scope itself lives in `.cargo/mutants.toml`.
#
# The mutation floor in full — hours, the only mode that may answer PASS, and a G4 criterion.
mutants *args:
    ./scripts/mutants.sh {{args}}

# The same floor in **iterate** mode (owner's request, 2026-08-13): cargo-mutants skips the
# mutants a previous run already caught, so a re-run costs the handful still open rather than
# the whole scope. Run it after each development stage; keep the full run for CI and for a
# review pass.
#
# It ends on **PARTIAL** and never on PASS, and the reason is not bookkeeping: the mutants it
# skips are exactly the ones a deleted test would have stopped catching. Remove a test between
# two iterate runs and the second one never re-tests its mutant, because that mutant is on the
# previous run's caught list. So this mode can still *find* a survivor — a real finding when it
# fires — and can never certify their absence. `just mutants` is the run that does.
#
# It needs a previous run's `target/mutants.out/` to skip anything; with none it is simply the
# full run under a different verdict word.
#
# The mutation floor over what a previous run left open — minutes, and it answers PARTIAL.
mutants-iterate *args:
    ./scripts/mutants.sh --iterate {{args}}

# Regenerate committed generated artifacts (JSON Schema bundle, OpenRPC, completions,
# man pages). `schema-artifacts-current.sh` proves the committed copies match.
generate:
    cargo run --locked -p webcam-handler-xtask -- generate

# ---------------------------------------------------------------- scratch

# The other half of cleanup, and the half a trap cannot do (owner ruling, 2026-08-12; note
# N84). Every producer of scratch deletes its own when it finishes; none of them finishes
# after a `kill -9`, which is how 76 abandoned copies of this repository came to be sitting
# in a tmpfs (E15). `just ci` sweeps anything over a day old on its way past; this is the
# same sweep with no grace period, for when nothing is running.
#
# Takes an age in minutes as an argument (`just scratch-sweep 60`); 0 by default.
#
# Reclaim abandoned test scratch from both roots.
scratch-sweep *args:
    ./scripts/scratch-sweep.sh {{args}}
