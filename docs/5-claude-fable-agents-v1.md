# AGENTS.md — webcam-handler (v1)

Doc 5 in the webcam-handler series, **v1 — initial revision**. Deploy at the repository
root as `AGENTS.md`; the deployed copy tracks this file (one-directional; when they
drift, reconcile deliberately and record which side was wrong). Terse by design; the
reasoning lives in `docs/3-claude-fable-code-review-rubric-v1.md` (rubric) and
`docs/1-claude-fable-design-v1.md` (design).

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
jsonrpsee wire surface), binaries `wch` (direct CLI), `wchd` (daemon: JSON-RPC over UDS
always, web client over opt-in loopback TCP + token), `wchc` (daemon CLI client),
`webcam-handler-web` (vanilla-JS, embedded, Chrome-targeted — Firefox/Safari best-effort,
never a feature constraint). `wch` and `wchc` share one command surface via
`webcam-handler-cli-core` — a verb exists once; `wchc` links no backend and no engine.
Packages carry the full `webcam-handler-` prefix; directories are short
(`crates/engine/`); lib names are bare; binaries are `wch`/`wchd`/`wchc`.

Work is phase-gated (docs/2, G0–G6); each gate is a named, counted, re-runnable
`just gate-gN`.

## Read before changing anything

- `docs/implementation-notes.md` — exists from the first commit; recorded, justified
  deviations and measured hardware evidence land there as N-entries. The PF registry
  (the design-phase probe findings) lives in `docs/1 §1.2` and extends here. Do not
  "fix" entries listed in either place; record new justified deviations as notes. Retire
  an entry only on empirical disproof.
- `docs/1-…design-v1.md` — architecture. **D1–D13, T1–T6, E1–E6, §7's rejected
  alternatives, and the §1 non-goals are settled**; do not re-litigate without new
  evidence (a new probe finding is evidence; taste is not).
- `docs/3-…rubric-v1.md` — every rule below is expanded there with provenance.
- `docs/4-…gates-v1.md` — the gate suite; once the repo exists its files are
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
  on modern kernels — PF:1); the raw QUERY_EXT_CTRL loop is ours.
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
  never pixel content; restore what they touch and assert it; motor-moving sweeps only
  under `--allow-motion`.
- No `sleep` as synchronization — settle logic runs on a stepped clock in tests.
- The browser half is asserted in a real browser: the pinned Playwright/Chromium rung
  (design §3.1 R1-web) self-skips counted without node — node is never a build
  dependency — and a browser behavior verified only through the JSON the page consumes
  is not verified. Miri runs the unsafe-adjacent pure decode units.
- No assertion inside a conditional whose false branch cannot go red; no skip that reads
  as pass.

## Hardware and privacy

- **A frame may contain a person.** Camera frames never enter the repository, logs, or
  error messages; `corpus/images/` holds generated synthetic fixtures only
  (gate-enforced); test captures go to gitignored scratch dirs; the daemon records
  nothing it wasn't asked to.
- The web listener is opt-in, loopback + token by default; the UDS directory is 0700.
  Weakening either is an owner decision, not a convenience fix.
- Killing a process that holds the camera is an explicit command naming its target,
  never a fallback. The hardware `Privacy` control is honored, never worked around.
- PTZ motors wear: sweeps are bounded and never move motors as an implicit default.

## Done means

- `just ci` green locally and offline — fmt, clippy `-D warnings`, nextest, doc build,
  cargo-deny (permissive allowlist + named bans: no LGPL linkage — libv4l, libudev,
  libcamera, alsa; no MPL — colored, minimp4; no GPL codecs; IJG banned while its
  question stays dormant (design §8 item 1 — the default stack never needs it); no
  TLS features), all gate predicates self-tested both directions.
- The battery green for every backend, skips declared and checked both ways.
- For a phase-closing change: the docs/2 gate criteria hold as named, counted,
  re-runnable commands; hardware evidence recorded in the notes with transcripts.
- New device behavior landed as corpus + notes, not prose in a PR description.
- `--json` output round-trips against the committed schemas; `wch`/`wchc` parity holds.

## Docs and dependencies

- Docs state each fact once, present tense, trade-offs honest; measured findings cite
  PF/N entries; a claim about a device without a probe behind it is marked `declared`.
- Dependencies: permissive licenses only, enforced by cargo-deny (design §2.8 is the
  registry); versions pinned at adoption; no git dependencies; `v4l` default features
  only; `image` with `default-features = false, features = ["png", "jpeg"]`; majors
  current at adoption; MSRV one fact, sync-asserted; `--locked` everywhere.
- Build deps (bindgen, libclang, kernel headers) are acceptable; runtime external
  binaries are not (ffprobe/mpv appear only as test oracles). The web client vendors or
  hand-writes everything — no CDN, no npm.
