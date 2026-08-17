# webcam-handler

Drive V4L2 webcams from a terminal, a daemon, a browser, or an AI agent: enumerate
cameras, read their real capabilities, move their controls, take photos and videos, and
run calibration sessions that record what worked.

Everything happens in-process. No `v4l2-ctl`, no `ffmpeg`, no external binaries at
runtime — the tool links Rust libraries and talks to the kernel itself.

**Status: under construction.** The architecture is settled (`docs/6`) and the work is
phase-gated (`docs/7`): P0–P6 are closed — P6's gate `G6` holds as 39 named, counted,
re-runnable criteria. `record` is here: an AVI muxer and a Y4M sink written in-process,
duration and size caps, the three wire methods, one verb on both command-line roots, and a
preview that is fed the recording's own frames. `docs/agent-guide.md` is generated from the
command surface it teaches. P6 closed with an adversarial review of the whole tree
(`docs/11`), its seventy-nine findings repaired, and the reconciliation those repairs owed the
rubric (`docs/8`).

## Deliverables

| | |
|---|---|
| `webcam-handler-cli` | the direct CLI — drives the engine in-process |
| `webcam-handler-daemon` | the daemon — JSON-RPC over a Unix socket always, opt-in loopback TCP for the browser |
| `webcam-handler-client` | the daemon CLI client — the same command surface as `webcam-handler-cli`, over the wire |
| web client | vanilla JS served by `webcam-handler-daemon`: camera list, live preview, controls, calibration |
| the library | `webcam-handler-engine` over a pluggable `CameraBackend` trait |

## Building

Rust 1.97 or newer, edition 2024. The V4L2 backend runs `bindgen` at build time, so the
build host needs libclang and the kernel UAPI headers:

```sh
sudo apt install clang libclang-dev linux-libc-dev   # Debian/Ubuntu
cargo build --locked --workspace
```

Nothing external is needed at *runtime*. `ffprobe` and `mpv` appear only as test oracles,
and `node` only for the browser test rung — neither is ever a build dependency.

Those two lines are everything a *build* needs. Working on the project needs more, and what
that is — required, optional, and what each optional one buys — is
[Development dependencies](#development-dependencies) below.

## Installing

Each binary installs from its own crate. Install the two you need, or all four.

```sh
cargo install --locked --path crates/daemon    # webcam-handler-daemon  — the daemon
cargo install --locked --path crates/client    # webcam-handler-client  — the daemon's CLI client
cargo install --locked --path crates/cli       # webcam-handler-cli     — the direct, daemon-less CLI
cargo install --locked --path crates/priv      # webcam-handler-priv    — dev-only; see AGENTS.md
```

They land in `~/.cargo/bin`. `--locked` is not decoration: it installs the dependency
versions this project tests against rather than whatever resolves today.

The binary is named after its crate, so the directory and the program differ on the same
line — `crates/daemon` builds `webcam-handler-daemon`. That is deliberate (note **N90**).

**`webcam-handler-priv` is dev-only and root-equivalent.** It exists to load two named kernel
modules for two test rungs, it is not part of using a camera, and it does nothing until it is
deliberately granted capabilities. Skip it unless you are working on this project.

## Using it

**Start with the daemon and the client.** Two programs rather than one buys you the thing
that matters with a webcam: **only one process may stream from a camera at a time**, and the
daemon is what owns that. With it running, a script, a browser tab and you at a terminal can
all use the same camera; without it they take turns and lose.

```sh
webcam-handler-daemon &                      # owns the cameras, serves a Unix socket
webcam-handler-client list                   # every camera, with a stable id per camera
webcam-handler-client photo cam:my-camera -o shot.jpg
webcam-handler-client record cam:my-camera -o clip.avi --duration 10s
```

`record` writes the camera's own MJPEG frames into an AVI, or raw frames into a `.y4m` —
the extension chooses, and the report says how many frames the file holds and what interval
they arrived at.

`webcam-handler-cli` is the same command surface with no daemon behind it — it opens the
device itself. Reach for it for a one-shot on a machine where nothing else wants the camera,
and for the daemon everywhere else.

Camera ids look like `cam:integrated-camera-integrated-c`. They are derived from what the
device says about itself, not from `/dev/video0`, because node numbers move between reboots
and between plug events — the id does not.

Add `--json` to any command to get the machine-readable form instead of a table; the shape is
committed under `schemas/` and validated in CI.

**If the thing driving this is a program rather than a person, point it at
[`docs/agent-guide.md`](docs/agent-guide.md).** That is the same surface written for an
unattended caller — every verb, every flag, which document each `--json` answers with, what
each failure means and what to do about it — and it is generated from the command surface, so
it cannot describe a flag this build does not have. The rest of this section is the human
tour.

### Your user needs access to the camera

On a desktop system logind grants it to the seat owner automatically; otherwise:

```sh
sudo usermod -aG video "$USER"   # log out and back in
```

The tool says so itself, with the path it could not open, rather than failing obscurely.

### A calibration session, start to finish

Calibration answers "what settings make this camera see this scene well?", and it answers it
by **taking photos and scoring them** rather than by guessing. A session is a durable record
on disk: you can stop half way, come back, and see what happened.

The example tunes brightness on one camera, for one named task:

```sh
CAM=cam:my-camera

# 1. Open a session. The task name is how you find it again; the goal is for you.
webcam-handler-client calibrate start "$CAM" --task desk --goal "a legible whiteboard"

# 2. Draft the queue: which controls will be tuned, in what order.
webcam-handler-client calibrate plan "$CAM" --task desk brightness

# 3. Sweep. A photo per value, each one scored. `--points 9` samples nine values across
#    the control's range; `--step` or `--values` if you want to say exactly which.
webcam-handler-client calibrate sweep "$CAM" --task desk brightness --points 9

# 4. Look at what happened — every sample, its score, and how the sweep ended.
webcam-handler-client calibrate status "$CAM" --task desk

# 5. Choose. Either let a metric decide, or name the value yourself.
webcam-handler-client calibrate select "$CAM" --task desk brightness --metric sharpness
webcam-handler-client calibrate select "$CAM" --task desk brightness --value 140 --by me

# 6. Write the chosen values to the camera.
webcam-handler-client calibrate apply "$CAM" --task desk
```

Two things are worth knowing before you run it on a camera you care about.

**The session snapshots the camera before it touches anything**, and `calibrate restore` puts
it back:

```sh
webcam-handler-client calibrate restore "$CAM" --task desk
```

**A sweep takes real photos and moves real controls.** On a pan/tilt/zoom camera it will not
move the motors unless you pass `--allow-motion` — motors wear out, so moving them is
something you ask for rather than something that happens to you.

`calibrate list` shows every session on the machine, newest first, if you have forgotten what
you called one.

### The browser

The daemon can serve a web client — camera list, live preview, controls, and the calibration
view — but **it does not listen on TCP unless you ask**:

```sh
webcam-handler-daemon --http
```

It prints a URL with a single-use token in it. Open that URL. The listener is loopback-only by
default and the token is what stands between a stranger on your machine and your camera.

## Development dependencies

**None of this is a runtime dependency, and the distinction is a rule rather than a
nicety.** The product shells out to nothing; everything below is either a *build*
dependency or a *test-time* one. `libclang` is consumed by a dependency's build script;
`ffprobe`, `mpv`, `node` and a pinned Chromium are oracles and harnesses that examine our
output from outside; `modprobe` is reached only through the dev-only `webcam-handler-priv`,
which note **N8** justifies on exactly that ground — a process boundary in a test rig is
not a link edge in a product. Design §2.8 keeps this category named and outside the shipped
licence inventory so it is chosen once. If a line inside `crates/` ever reaches for one of
these, that is the rule being broken and not a shortcut being taken.

Three levels follow and you can stop at the one you need. The first two are required; the
third is optional, and each entry there buys a rung. **No absence in the third level is
silent** — every auto-skipping rung reports a named, counted skip saying which precondition
was missing and what it therefore did not claim (AGENTS rule 3), so going without a tool
costs you a stated claim rather than a quiet green.

### Required to build

Already in [Building](#building) and not repeated here: `clang`, `libclang-dev`,
`linux-libc-dev`. The reason is one level down the graph — `v4l` pulls `v4l2-sys-mit`,
whose build script runs `bindgen` over the kernel UAPI headers (**PF:10**, accepted under
the owner's build-deps-are-fine ruling). No *workspace member* has a build script of its
own, and `node_is_never_a_build_dependency` in `crates/daemon/tests/web_browser.rs` asserts
that over every member manifest, which is what keeps a bundler from ever becoming part of
`cargo build`.

**`linux-libc-dev` has a vintage, and the test target is what has an opinion about it.**
bindgen reads *this host's* `<linux/videodev2.h>`, so the kernel names the V4L2 backend can
use are whatever the build host defines. The product code names only long-standing struct
types; the crate's **tests** compare every hand-copied kernel number in
`webcam-handler-schema` against the header's own (note **N228**), which means naming the
newest of them — `V4L2_CTRL_TYPE_RECT`, `V4L2_CTRL_TYPE_HDR10_MASTERING_DISPLAY`,
`V4L2_CTRL_TYPE_AV1_FRAME`, `V4L2_CTRL_FLAG_HAS_WHICH_MIN_MAX`. On headers that predate one
of those, `cargo build --workspace` is fine and `cargo test -p webcam-handler-v4l2` does not
compile. Rather than a version number this repository cannot verify offline,
`./scripts/gates/uapi-constants-are-declared.sh` checks the header for **every** kernel name
that crate asks bindgen for and names the missing one (note **N236**), so the answer arrives
as a gate with a remedy instead of as `cannot find value … in module uapi`.

### Required for `just ci`

`just ci` is the floor — fmt, clippy with `-D warnings`, nextest, the doc build,
`cargo-deny`, the hygiene step, then every gate predicate and the self-test that proves
each one can go red. It runs **offline** by construction, and `.github/workflows/ci.yml`
runs that same recipe verbatim so a green laptop and a green runner mean the same thing.
That workflow's install step is the authoritative list; this is it, plus `git`:

```sh
sudo apt install jq shellcheck git                                    # Debian/Ubuntu
cargo install --locked just cargo-nextest cargo-deny cargo-machete typos-cli
```

| tool | what runs it | without it |
|---|---|---|
| `just` | every recipe in this README, and CI's only command | nothing below is runnable |
| `cargo-nextest` | `just test`, and every rung — `just ci` passes `--no-tests=fail` | no test step at all |
| `cargo-deny` | `just deny`, offline, over `deny.toml` | the licence and ban walls stop being checked |
| `cargo-machete` | `just hygiene`, `--with-metadata` | unused dependencies accumulate unseen |
| `typos-cli` (binary `typos`) | `just hygiene` | — |
| `shellcheck` | `just hygiene`, over `scripts/*.sh scripts/gates/*.sh scripts/gates/cases/*.sh` | the gate suite is unlinted shell |
| `jq` | the gates themselves | the gate suite cannot run |
| `git` | `gate_root` in `scripts/gates/lib.sh` | gates cannot find the tree they are checking |

`jq` and `shellcheck` are load-bearing rather than cosmetic, because **the gates are shell
programs over `cargo metadata`**: `msrv-sync.sh` reads every workspace member's resolved
`rust-version` through `jq` to prove the MSRV is one fact, and `dependency-walls.sh`
computes the crate graph the same way. A host without `jq` does not get a degraded gate
suite, it gets none. `git` is the softer of the two — `WCH_GATE_ROOT` overrides the `git
rev-parse --show-toplevel` lookup, and `mutation-verdict.sh` degrades to a counted skip in a
checkout git cannot describe — but a normal working tree wants it.

Two more are on any Linux host already and are noted so their absence is diagnosable rather
than mysterious: `coreutils` supplies the `mkfifo` that `crates/daemon/tests/signals.rs` and
`crates/daemon/tests/mutating_verbs.rs` use to watch a daemon's stderr without a sleep, and
`systemd` supplies `systemd-socket-activate`, `systemd-run` and `journalctl`, which
`scripts/gates/socket-activation.sh` needs to hand the daemon a socket it did not open and
to read back a unit whose stderr is the journal. On a host without them that gate declines
four named claims and says which unit tests carry the argument instead. The same file's
neighbour, `uds-permissions.sh`, wants a second account and a non-interactive `sudo -n` to
try walking into the socket directory as somebody else; without either it declines by name,
having still checked the directory mode.

### Optional — each one buys a rung

| tool | apt / cargo | what it buys | what the decline costs |
|---|---|---|---|
| `nodejs`, `npm` | `sudo apt install nodejs npm` | **R1-web**, the browser rung | 24 browser claims, 206 assertions |
| pinned Playwright + Chromium | `just rung-web-install` | the same rung's actual browser | as above; node alone is not enough |
| `ffmpeg` (for `ffprobe`), `mpv` | `sudo apt install ffmpeg mpv` | the container oracles the recording rung uses | the AVI muxer is believed only by readers we wrote |
| `kmod`, the `vivid` module | `sudo apt install kmod`; see below | **R2**, the virtual-driver rung | 77 controls and compound payloads nothing else reaches |
| `libcap2-bin` + one `sudo` | `sudo apt install libcap2-bin`, then `just bless` | R2 without a manual `modprobe`, and the hotplug arms | R2 runs only where somebody already loaded the module |
| Miri on a nightly toolchain | `rustup toolchain install nightly && rustup +nightly component add miri` | `just miri` over the pure decode units | the unsafe-adjacent decoders go uninterpreted |
| `cargo-mutants` | `cargo install --locked cargo-mutants` | `just mutants`, the mutation floor | a G4 criterion cannot close |
| a camera | — | **R3**, `just smoke-hw` | every device claim rests on the fake's model of a device |

#### The browser rung (R1-web)

`just rung-web-install` is the whole install — `npm ci` under
`crates/daemon/tests/browser/`, then the Playwright CLI *it just installed* fetching the
browser build that CLI names. It is a recipe a human runs rather than something a test does
on your behalf, because it is the one step here that reaches the network, and neither half
may drift: `npx playwright` would happily resolve some other version and fetch some other
browser, which is the failure the pin exists to prevent. The pin is threefold —
`@playwright/test` at exactly `1.62.1` with no range operator, Chromium build `1234`, and
Chrome version `151.0.7922.34` — declared in `crates/daemon/tests/browser/package.json`
beside a committed `package-lock.json`, and *asserted* in `pins.spec.mjs`, because a version
being arranged and a version being checked are two different claims (evidence **E16**). Those
three numbers are stated twice — there, and in the paragraph you are reading — so
`scripts/gates/browser-pins-sync.sh` reconciles this prose against that manifest on every `just
ci`, and the manifest wins: it is what `npm ci` installs. The same gate holds the *exactness*
of the package pin on hosts where `pins.spec.mjs` never runs, which upstream CI is every time
(note **N131**). The lockfile's engines floor is Node 20 or newer; Ubuntu's `nodejs` clears it. `WCH_E2E_NODE`
points the rung at a node that is not on `PATH`, and `PLAYWRIGHT_BROWSERS_PATH` at a browser
cache that is not `~/.cache/ms-playwright`.

This rung is deliberately **not** `#[ignore]`d: design §3.1 puts it on every push where the
host has node, so `just ci`'s test step already runs it and `.config/nextest.toml` gives its
binary `success-output = "final"` so a decline can never be printed into a void. `just
rung-web` is the accounting on top — it ends on `RAN` or `SKIPPED`, which is what `just
gate-g5` records. Worth stating honestly: **the GitHub workflow installs no node**, so
upstream CI takes the decline every time and a developer's laptop is where this rung
actually runs. Going without it forfeits the half of the web client that only a browser can
establish — a sparse menu becoming a `<select>` carrying the device's own indices rather
than an option's position (**PF:2**), an INACTIVE control rendered enabled and badged
(**PF:3**) beside a READ_ONLY one rendered disabled (**PF:12**), a clamp moving the slider
back with both numbers (**PF:6**), and the credential split note **N82** created, where the
static assets load anonymously while `/preview` and the WebSocket upgrade do not. AGENTS
puts it plainly: a browser behavior verified only through the JSON the page consumes is not
verified.

#### The container oracles (`ffprobe`, `mpv`)

```sh
sudo apt install ffmpeg mpv   # ffprobe ships inside the ffmpeg package
```

These are the oracles the recording work is measured against. D7 L0 accepts a hand-written
AVI muxer at a stated price — "~300 lines *and* an ffprobe round-trip oracle" — and docs/7
**P6d** commissions that harness over generated AVI as present-or-counted-skip, with docs/9's
oracle-accounting row making a silently-absent oracle a defect in its own right. P6 is the
phase in flight, so install these before touching `crates/imaging/src/avi/`.

What their absence costs is specific and not small: the muxer's player-compatibility claim
falls back on readers this repository also wrote. `avi-reparse-is-independent.sh` and the
byte-exact fixture in `crates/imaging/fixtures/avi/` are real and they hold the format still
— but a fixture produced by the code under test proves only that the output has not drifted,
never that it is right. Only a program nobody here wrote can say whether a real player opens
the file. Both tools stay strictly test-time: the product's promise in this README's opening
paragraph is that it links libraries and talks to the kernel, and a `record` verb that
learned to spawn `ffmpeg` would end that.

#### The vivid rung (R2)

`vivid` is a kernel module presenting a V4L2 capture device with no hardware behind it —
the only way to exercise the real ioctl layer on a machine with no camera, and the only rung
that reaches 77 controls and compound payloads where the attached cameras offer 18 and 24.

```sh
sudo apt install kmod libcap2-bin   # modinfo/modprobe, and setcap/getcap for the helper
modinfo vivid                       # the check: on Ubuntu it ships in the running kernel's
                                    # own linux-modules package, already installed
just bless                          # once, needs sudo once; grants webcam-handler-priv its
                                    # capabilities, mode 0700, owner only
just priv-doctor                    # what the helper can do right now, and why not if it cannot
just rung-vivid-managed             # load, run, unload
```

`just bless` is idempotent and re-verifies rather than trusting its own stamp: an rsync or a
backup-restore strips a file's capabilities, and reporting "already blessed" over a copy that
is effectively un-capped would be skip-reads-as-pass wearing a filesystem. It also insists the
blessed copy carry **exactly** the capabilities the binary asks for, neither more nor fewer, so
a grant that grew out of band re-blesses back down instead of being waved through (note
**N125**). **Never `modprobe` by hand** — `scripts/rung-vivid.sh` refuses to load a kernel
module on your behalf, and `just rung-vivid-managed` is the supported path, loading through the
helper and unloading in a trap.

The helper itself is root-equivalent and dev-only; note **N8** is the argument for its existing
at all. **P6e executed the narrowing N8 scheduled for G6** (note **N125**): the blessing is
`cap_sys_module+ep` — `CAP_NET_ADMIN` was measured never to have been needed \[PF:21\] — and the
verb that ran an arbitrary program with those capabilities is deleted, because nothing in this
workspace had ever invoked it. What is left is a closed vocabulary of six verbs — `doctor`,
`vivid up|down|status`, `uvcvideo cycle|status` — over two compile-time module names. That is a smaller blast radius and not a small tool: loading a kernel
module is arbitrary code in ring 0, so the `0700` file mode is still the boundary.

Without the module, `just rung-vivid` prints a named, counted skip that distinguishes the two
cases it can tell apart — installed but not loaded, versus not available on this host — and
exits zero having claimed nothing. A green R2 is also not evidence about real cameras:
`vivid` does not model INACTIVE-coupled menus with holes, which is why R3 exists.

#### Miri, and the mutation floor

```sh
rustup toolchain install nightly && rustup +nightly component add miri
cargo install --locked cargo-mutants
```

`just miri` runs `cargo +nightly miri nextest run` over `sys::decode` and `sys::payload` —
the half of the unsafe module that makes no syscall. **Miri cannot cross an ioctl**, so a
green run is never "the unsafe module is verified"; the rest of that module is R2's and R3's
to exercise. Without the component the script names the missing precondition and prints the
one-line remedy.

`just mutants` is the floor that turns "the tests constrain the pure cores" from a claim into
a measurement, scoped in `.cargo/mutants.toml`. Budget tens of minutes at least: the last
full run this project recorded was **526 mutants in 42 m 07 s** (evidence E14), and
`imaging::avi` has joined the scope since, which N101's scoped pass measured at a further 318
— so the figure to plan against is the one your own run prints, not this sentence. That cost
is why it is a rung and a G4 criterion and never a `just ci` step. Run `just mutants-iterate` after each
development stage instead: it skips what a previous run already caught, so it costs the
handful still open, and it answers **PARTIAL** and never PASS, because the mutants it skips
are exactly the ones a deleted test would have stopped catching. Every survivor becomes a new
test or a reasoned acceptance in `scripts/mutants-accepted.txt` citing its N-entry, and the
register is checked both ways — an acceptance that stopped surviving fails the job. Without
`cargo-mutants` the recipe reports a named, counted skip; installing it is never an
escalation, but `just gate-g4` cannot close without a full run.

#### The hardware rung (R3)

R3 needs a camera rather than a package, plus your user in the `video` group — see
[Your user needs access to the camera](#your-user-needs-access-to-the-camera) above. `just
smoke-hw` runs the `#[ignore]`d `hw_*` suites against whatever is attached, restoring
everything it touches and asserting the restoration, motor positions included. **Motors move
by default** (owner ruling, 2026-08-08), because an untested motor is untested code;
`WCH_NO_MOTION=1` excludes the `hw_motion_*` suites as a named, counted skip for runs where
the camera is pointed at a person. Never run two hardware rungs at once — one streamer per
node is the kernel's rule, and `.config/nextest.toml` serialises `hw_` and `vivid_` in a
one-thread group because the alternative is a perfectly correct `EBUSY` from a perfectly
correct backend.

## Development

```sh
just ci          # everything CI runs, offline: fmt, clippy, tests, docs, licenses, gates
just gate-g0     # a phase gate — named, counted, re-runnable
just selftest    # proves every gate predicate can go red, in both directions
just smoke-hw    # the real-hardware rung; needs a camera, and moves motors by default —
                 # WCH_NO_MOTION=1 opts out (owner ruling, 2026-08-08)
just rung-web    # the browser rung, in a pinned headless Chromium; declines by name without
                 # node — `just rung-web-install` is what supplies it
```

`just --list` has the rest — `rung-vivid-managed`, `miri`, `mutants`, `generate`, `bless`.
What each of them needs installed, and what its decline costs you, is
[Development dependencies](#development-dependencies) above.

Read `AGENTS.md` before changing anything. The reasoning behind every rule it states lives
in `docs/`.

## License

MIT or Apache-2.0, at your option. Every dependency is permissively licensed, enforced by
`cargo-deny` (`deny.toml`) rather than by good intentions.
