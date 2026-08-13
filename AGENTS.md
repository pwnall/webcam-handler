# AGENTS.md — webcam-handler (v2)

Doc 10 in the webcam-handler series, **v2 — first revision**; supersedes docs/5 (v1, now
under `docs/historical/`). Deploy at the repository
root as `AGENTS.md`; the deployed copy tracks this file (one-directional; when they
drift, reconcile deliberately and record which side was wrong). Terse by design; the
reasoning lives in `docs/8-claude-fable-code-review-rubric-v2.md` (rubric) and
`docs/6-claude-fable-design-v2.md` (design).

## Who runs this, and why (owner, 2026-08-12)

`wchd` runs on a computer whose cameras are **pointed at a device under test**. Two
consumers, shaped nothing alike:

- **An AI agent harness (Claude Code or similar) drives the client to photograph the device
  under test, to check its own work** — e.g. a display driver is validated by photographing
  the device's display. Primary, continuous, unattended. The same agent also wants
  **video**, to validate animations and transitions: a very desirable secondary use case,
  which makes P6 agent-facing rather than trailing, and makes frame *timing* a payload
  rather than metadata.
- **The owner uses the web client from time to time** to check up on the cameras, and to
  **calibrate them at the start of a development run**. Occasional, interactive, supervisory.

Four consequences, because they decide trade-offs rather than decorate them. **The primary
consumer has no hands** — a verb needing a call sequence, or a failure that reads as prose,
is a defect for the consumer that matters most. **The product is comparability across time,
not a good picture**: two photos an hour apart must differ only where the *device* differs,
so when image quality and repeatability conflict, repeatability wins. **The two consumers
overlap on one camera as the normal case**, not as a race (note N83). **The error vocabulary
is read unsupervised**: `Busy` means retry, `DeviceGone` means stop and tell the human,
`PermissionDenied` means a setup problem — collapsing them makes the agent guess.

The full statement, its reasoning, and what would change it are at the top of
`docs/implementation-notes.md`.

## What this is

webcam-handler drives V4L2 webcams for humans and AI agents: enumerate cameras, list
capabilities (formats, resolutions, frame rates, controls with menus/ranges/flags), drive
controls (pan/tilt/zoom, focus, exposure — with auto/manual pairing handled), take photos
and videos in-process (no ffmpeg, no v4l2-ctl at runtime), and run calibration sessions
(sweep a control, photo per value, score, track per-control status, apply later).

Rust 2024 workspace, one library + four consumers: `webcam-handler-schema` (every shared
type, the backend traits, `BackendKind`),
`webcam-handler-imaging` (codecs, our AVI muxer, metrics), `webcam-handler-engine` (camera actors, guarded
writes, snapshot/restore, calibration, session store), backends behind one trait
(`webcam-handler-v4l2` real, `webcam-handler-fake` replaying **captured device profiles**), `webcam-handler-api` (the one
jsonrpsee wire surface), binaries `wch` (direct CLI, in `webcam-handler-cli`), `wchd`
(daemon: JSON-RPC over UDS always, web client over opt-in loopback TCP + token, in
`webcam-handler-daemon`), `wchc` (daemon CLI client, in `webcam-handler-client` — a lib as
well as a bin, so a test can drive the executor a subprocess cannot observe),
`webcam-handler-web` (vanilla-JS, embedded, Chrome-targeted — Firefox/Safari best-effort,
never a feature constraint). `wch` and `wchc` share one command surface via
`webcam-handler-cli-core` — a verb exists once; `wchc` links no backend and no engine.
Packages carry the full `webcam-handler-` prefix; directories are short
(`crates/engine/`); lib names are bare; binaries are `wch`/`wchd`/`wchc`.

Work is phase-gated (docs/7, G0–G6; P0–P3 are closed); each gate is a named, counted,
re-runnable `just gate-gN` over `scripts/gates/phase-criteria.tsv` — one row per
criterion, added in the same commit as the thing it proves. Work lands in
session-sized sub-milestones (docs/7): each ends at a committed boundary with `just ci`
green and the notes current, and the phase review gets its own session.

## Read before changing anything

- `docs/implementation-notes.md` — case law: justified deviations as N-entries, hardware
  behavior as PF-entries (continuing the docs/6 §1.2 registry, PF:1–20), dated evidence
  as append-only E-entries. Do not
  "fix" entries listed there or in §1.2; record new justified deviations as notes. Retire
  an entry only on empirical disproof. Citations of `docs/1`–`docs/5` in older entries
  refer to the superseded v1 files under `docs/historical/` (v2 preserves their
  numbering).
- `docs/6-…design-v2.md` — architecture. **D1–D13, T1–T6, E1–E6, §7's rejected
  alternatives, and the §1 non-goals are settled**; do not re-litigate without new
  evidence (a new probe finding is evidence; taste is not).
- `docs/8-…rubric-v2.md` — every rule below is expanded there with provenance.
- `docs/9-…gates-v2.md` — the gate suite; the repo's files are
  authoritative and that doc records deltas.
- `vendor/v4l2-webcam-skill/` — the manual workflow this tool replaces; the operations
  map is design §1.1.

## Non-negotiable rules

1. Every anticipated or discovered defect class becomes a lint, a CI job, or a test that
   can go red. A fix lands **with its gate**, in the same PR.
2. Every test and every gate predicate must be able to fail — both directions, proven in
   `scripts/gates/selftest.sh`. Write the red-on-inverse first. Mutations verify at
   workspace scope; absence claims name where they looked.
3. CI executes what it claims: counted selections, `--no-tests=fail`, and every
   auto-skipping rung (vivid, hardware, oracles) reports a **named, counted skip** —
   never silence.
4. **The device is the only authority on itself.** Enumerate live; transcriptions are
   `declared` data until a probe makes them `measured`, and measured wins. New hardware
   behavior lands as a profile in `corpus/` + a note, the day it is seen.
5. **Requested is not applied.** Drivers clamp silently (probe-verified); every write
   reads back and every layer preserves `{requested, applied}`.
6. **Represent the unknown.** Unknown control types, sparse menus, out-of-range currents
   and defaults, undocumented flags — all carried as data, never panics, never
   "corrected". A `match` on device vocabulary has a payload-carrying fallback arm.
7. **Availability is not capability.** EBUSY/ENODEV/EPERM/timeout stay distinct from
   "the camera can't"; no code or test converts one into the other.
8. **Leave the camera as you found it.** Snapshot before, restore after, automation
   before manual on restore; persisted pre-sweep snapshots make crashes recoverable;
   tests assert restoration.

## Writing code

- Pure cores take values (pairing planner, sweep planner, settle policy, session state
  machine, metrics); seams live in the shell and each has a real impl and a scriptable
  double with an exhaustive-match fault menu.
- One home per law (design §2.10): control semantics in `webcam-handler-schema`; guarded-write
  planning in `webcam-handler-engine::pairing`; state-dir writes through
  `webcam-handler-engine::store::write_json_atomic` under the one fd-lock; errors in the D13
  registry with one exhaustive RPC-code match; the wire surface is the `webcam-handler-api` trait;
  the command surface is the shared T4 core. A second copy or a bypass is a defect.
- Backends implement T1/T2 against schema values only; no V4L2 type escapes
  `crates/backends/v4l2/`. The `v4l` crate's `query_controls` is lint-banned (it panics
  on modern kernels — PF:1); the raw QUERY_EXT_CTRL loop is ours. Index-walked
  enumerations end on `EINVAL` **or** `ENOTTY` through `call_enumerating` (PF:15).
  Write dispatch is the *descriptor's* decision (`HAS_PAYLOAD`), never the caller's
  value variant — a `Bytes` value at a scalar control is a typed refusal (design §2.3).
- Pair discovery follows D3's three probe rules: every menu alternative tried (a menu is
  not a switch), residue isolated between candidates, "off" recorded per freed control
  by menu-item name.
- Photos: verbatim camera JPEG when the sink allows (byte fidelity is the product);
  negotiated format/size always surfaced when it differs from requested.
- Bounded everything: settle deadlines, sweep caps, recording caps, channel depths,
  shutdown drains — constants live in `webcam-handler-schema::limits` and something reads each one.
- Unsafe code (rubric B10, transferred from vmcell): every `unsafe` block lives in
  `crates/backends/v4l2/src/sys/` — every other crate is `#![forbid(unsafe_code)]` at
  its root, and within `webcam-handler-v4l2` the `unsafe-scope.sh` gate confines the
  token to `src/sys/` (a crate-root forbid is impossible there). No hand-declared kernel structs (bindgen output only; a forced
  hand-copy gets size/offset assertions). `// SAFETY:` proves the block's *actual*
  obligation, one obligation per block; a false safety claim is a defect even when the
  code works. Device-derived numbers (`bytesused`, indices, payload sizes) are validated
  before use; wire integers via `try_from`, never `as`. Suppressions are `#[expect]`
  with `reason=`, narrowest scope. On device/request-driven paths no
  `unwrap`/`expect`/`panic`/indexing (clippy-enforced).
- Daemon: per-camera actor threads own the device; exclusive streaming by construction;
  cameras open on first use and close when idle; SIGTERM ≡ SIGINT; open MJPEG/WS
  streams are cancelled, never awaited, on shutdown.

## Writing tests

- Construct the buggy implementation first and watch it fail — at workspace scope. Write
  the malformed fixture that must trip each validator.
- Fixtures enter tests as bytes/corpus. Device profiles are captured by the tool,
  committed with provenance, immutable; re-capture replaces wholesale. Every PF-class
  finding exists as corpus a test loads, not prose.
- The fake resembles: its claims (clamping, INACTIVE coupling, frame response) are
  asserted against the probe record of the profile it replays. A fake capability no real
  device exhibits is a bug in the fake.
- Hardware suites (`#[ignore]`d, recipe-named): assert invariants and metric *orderings*,
  never pixel content; restore what they touch and assert it — motor positions
  included. Motor-moving suites (`hw_motion_*`) run by default (owner ruling,
  2026-08-08); `WCH_NO_MOTION=1` excludes them, counted and named, for runs where the
  camera points at a person. The `hw_`/`vivid_` suites serialize in the one-thread
  `exclusive-device` nextest group — one streamer per node is the kernel's rule; never
  run two hardware rungs concurrently.
- The vivid rung reaches paths the attached cameras cannot (77 controls, compound
  payloads): run `just rung-vivid-managed` after touching enumeration, the control
  walk, the format tree, writes, or streaming. It needs the blessed helper — `just
  bless` once (sudo), `just priv-doctor` to check; never `modprobe` by hand.
- Gate selftests prove both directions, and the inverse arm is driven by the thing
  under test, not a stub of it; where a stub is unavoidable, one arm still runs the
  real tool (rubric rule 6, paid for by N10).
- The mutation floor (`just mutants`) is where "the tests constrain the cores" stops
  being a claim: a `cargo-mutants` run over the planners, the state machine, settle, the
  store and metrics, scoped in `.cargo/mutants.toml`. Every survivor is a new test or a
  reasoned acceptance in `scripts/mutants-accepted.txt` citing its N-entry, and the
  register is checked both ways — an acceptance that stopped surviving fails the job. A
  dev tool and a G4 criterion, hours not minutes; never a `just ci` step, and its absence
  is a named, counted skip.
- No `sleep` as synchronization — settle logic runs on a clock the test owns, never on
  the real one. Two shapes: a `SteppedClock` where the deadline is the subject, and a
  `FrozenClock` where it is not — a deadline that cannot expire is what stops a loaded
  machine answering `SettleTimeout` to a test that asked about the device (N60).
- The browser half is asserted in a real browser: the pinned Playwright/Chromium rung
  (design §3.1 R1-web) self-skips counted without node — node is never a build
  dependency — and a browser behavior verified only through the JSON the page consumes
  is not verified. It landed at P5d as `crates/daemon/tests/web_browser.rs` driving
  `crates/daemon/tests/browser/`, and it is **not** `#[ignore]`d: design puts it on every
  push where the host has node, so `just ci` runs it and its decline names the missing
  precondition, its remedy and every claim it cost. `just rung-web-install` gives a host
  the pinned package and the pinned browser; `just rung-web` ends on RAN or SKIPPED, which
  is what `just gate-g5` records. Miri runs the unsafe-adjacent pure decode units.
- No assertion inside a conditional whose false branch cannot go red; no skip that reads
  as pass.

## Hardware and privacy

- **A frame may contain a person.** Camera frames never enter the repository, logs, or
  error messages; `corpus/images/` holds generated synthetic fixtures only
  (gate-enforced); test captures go to gitignored scratch dirs; the daemon records
  nothing it wasn't asked to.
- The web listener is opt-in, loopback + token by default; the UDS directory is 0700.
  The token gates the two routes that carry or drive the camera — `/rpc` and `/preview`,
  which `daemon::http::CAMERA_BEARING_PATHS` names — and **not** the static assets, which
  are this project's own open-source code (owner ruling, 2026-08-12; D11's amendment, N82).
  A route added later without a gate is the defect class that ruling created:
  `web-routes-are-gated.sh` and `every_camera_bearing_route_is_behind_the_gate` are the two
  halves that can go red on it. Weakening any of this is an owner decision, not a
  convenience fix.
- Killing a process that holds the camera is an explicit command naming its target,
  never a fallback. The hardware `Privacy` control is honored, never worked around.
- PTZ motors wear: sweeps are bounded by the `limits` caps everywhere. In *tests*
  motors run by default (owner ruling, 2026-08-08; `WCH_NO_MOTION=1` opts out); in the
  *product* a plan that would move motors still says so first (`--allow-motion` —
  design §5 carries the split).
- `wch-priv` (dev-only, root-equivalent — design §2.13, note N8): its boundary is the
  `0700` file mode, checked every `just ci`. Never widen its verbs to take
  caller-supplied module names or paths without amending N8; it refuses to unload
  `uvcvideo` while any `/dev/video*` is open — design tests around that, don't fight
  it. The narrowing reckoning is G6's (docs/7 P6e).

## Done means

- `just ci` green locally and offline — fmt, clippy `-D warnings`, nextest, doc build,
  cargo-deny (permissive allowlist + named bans: no LGPL linkage — libv4l, libudev,
  libcamera, alsa; no MPL — colored, minimp4, option-ext (the `directories` door, N2);
  no GPL codecs; IJG banned while its
  question stays dormant (design §8 item 1 — the default stack never needs it); no
  TLS features), all gate predicates self-tested both directions.
- The battery green for every backend, skips declared and checked both ways.
- For a phase-closing change: the docs/7 gate criteria hold as named, counted,
  re-runnable commands; hardware evidence recorded in the notes with transcripts.
- New device behavior landed as corpus + notes, not prose in a PR description.
- **A doc comment in `webcam-handler-api` or `webcam-handler-schema` is an input to a
  committed artifact, not just prose.** The OpenRPC document and the JSON Schema bundle
  under `schemas/` are emitted from those comments, so editing one — even fixing a stale
  sentence — moves a committed file, and `schema-artifacts-current.sh` goes red until
  `just generate` is run and the result committed. The gate is the backstop and it works;
  this line exists so the trap costs a command rather than a CI cycle.
- `--json` output round-trips against the committed schemas; `wch`/`wchc` parity holds —
  which since P4f is `./scripts/gates/cli-parity.sh` rather than an aspiration: it compares
  the two roots byte for byte on every read verb over the fake, and puts every other verb
  `wch --help` offers in a named bucket with a reason, because a verb neither compared nor
  named is the way this claim quietly stops being true.

## Docs and dependencies

- Docs state each fact once, present tense, trade-offs honest; measured findings cite
  PF/N entries; a claim about a device without a probe behind it is marked `declared`.
- Dependencies: permissive licenses only, enforced by cargo-deny (design §2.8 is the
  registry); versions pinned at adoption; no git dependencies; `v4l` default features
  only; `image` with `default-features = false, features = ["png", "jpeg"]`; majors
  current at adoption; MSRV one fact, sync-asserted; `--locked` everywhere.
  **Adopting a crate that clears that bar is not an escalation** (owner ruling,
  2026-08-09 — §2.8): judge the licence and whether it looks solid, take it, and say what
  you concluded. Moving the bar — a new allowlist entry, a lifted ban, an unpinned
  version — is the owner's. Never defer work on the ground that it "needs a dependency
  decision".
- Build deps (bindgen, libclang, kernel headers) are acceptable; runtime external
  binaries are not (ffprobe/mpv appear only as test oracles). The web client vendors or
  hand-writes everything — no CDN, no npm.
