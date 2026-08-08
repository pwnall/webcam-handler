# webcam-handler — Implementation Plan (v1)

Doc 2 in the webcam-handler series, **v1 — initial revision**. Status: current. Consumes
the design (docs/1); its gate criteria are enforced by the gate suite (docs/4) and its
review bar by the rubric (docs/3). Section references of the form §n.m point into docs/1
unless prefixed.

**Shape of the plan.** Seven phases, P0–P6, each closed by a gate G0–G6. The ordering rule
is the rung ladder (§3.1, R0–R3): everything provable without a device lands before the code
that needs one, and every piece of device behavior we learn is captured into the corpus
the same phase we learn it. Calibration (P3) lands *before* the daemon (P4) deliberately:
the direct CLI over the library is the smallest thing an AI agent can use end-to-end, and
proving the whole capture→sweep→score→apply loop in-process first means the daemon phase
wires transport around a working core instead of debugging both at once.

**Standing conventions, in force from P0:**

- **`docs/implementation-notes.md` exists from the first commit.** Recorded, justified
  deviations from docs/1–5 land there as numbered entries (N1, N2, …): what the doc says,
  what the repo does, why, and what would retire the entry. Hardware surprises — new PF
  findings — land there too, with the probe transcript. Entries are case law; reviews do
  not re-report them (docs/3 Part E).
- **A fix or feature lands with its gate, in the same PR** (rubric rule 1). The
  commissioned-gates table in docs/4 Part 2 names which phase each gate lands with.
- **Every phase gate is a named, counted, re-runnable command** (`just gate-g0` …
  `just gate-g6`): a criterion that cannot be re-run is a criterion nobody will re-check
  (predecessor evidence: a "held" gate whose selection had silently gone to zero).
- **Corpus discipline**: device profiles are captured by the tool, committed with
  provenance, immutable once committed (§3.2). The three probe-era profiles (chicony-rgb,
  chicony-ir, obsbot-tiny3) are committed at P1 the day `profile capture` works.
- **Hardware needs**: P1–P3 and P6 want at least one attached UVC camera for the R3 smoke
  recipes (the dev machine has three logical cameras, one of them PTZ — ideal); every
  R3 suite is `#[ignore]`d and CI-independent. The `vivid` rung (R2) is wired at P1 and
  auto-skips **with a counted, named skip** where the module is unavailable. The browser
  rung (R1-web, pinned Playwright + Chromium) arrives at P5 with the same skip
  discipline: it runs where the host has node, self-skips counted elsewhere, and node is
  never a build dependency.

## P0 — Foundations (no device code)

**Lands:**

- Workspace scaffold: edition 2024, `rust-version` pinned, resolver 3; crates `webcam-handler-schema`
  (including the T1/T2 traits and `BackendKind`), `webcam-handler-imaging`,
  `webcam-handler-engine`, `webcam-handler-fake`, `webcam-handler-testkit`,
  `webcam-handler-xtask`, with `webcam-handler-v4l2`, `webcam-handler-api`,
  `webcam-handler-cli-core`, `webcam-handler-cli`, `webcam-handler-client`,
  `webcam-handler-daemon`, `webcam-handler-web` as empty-but-compiling members so the
  dependency walls (T6) are gate-checkable from day one. Crate-root lint policy in force
  from the scaffold: `unsafe_op_in_unsafe_fn`, `clippy::undocumented_unsafe_blocks`,
  `clippy::missing_safety_doc`, `clippy::multiple_unsafe_ops_per_block` denied
  everywhere; `#![forbid(unsafe_code)]` on every crate except `webcam-handler-v4l2`,
  which confines `unsafe` to `src/sys/` (design §2.5; the scope gate is in docs/4).
- CI + the gate harness: `just ci` (fmt, clippy `-D warnings`, nextest `--no-tests=fail`,
  doc build, cargo-deny with the §2.8 allowlist and named bans, typos, machete,
  shellcheck), `scripts/gates/*.sh` under a table-driven `selftest.sh` requiring both
  directions per predicate from the first gate onward (docs/4).
- `webcam-handler-schema`: the control model (D2) including `Unknown` types, sparse menus, raw+decoded
  flags; camera identity (D1); the error registry (D13); session-state types (D8) with
  `schema_version`; limits module; serde + schemars derives with round-trip property tests.
- `webcam-handler-imaging`: PNG encode path, the D6 source-format conversions (YUYV→RGB
  and NV12→RGB via `yuv`, GREY widening), JPEG decode, and the metric set (Laplacian
  variance, clip fractions, mean luma, RMS contrast) with synthetic-fixture ordering tests
  (sharp > blurred, both directions).
- `webcam-handler-engine` pure cores: pairing planner, sweep planner, settle policy, session state
  machine — each with its inverse fixtures (a plan that would write a manual control under
  live automation must be constructible and refused).
- `webcam-handler-fake`: profile replay (T3 format defined here), scripted faults, control-graph
  simulation of clamping [PF:6] and INACTIVE coupling [PF:3], synthetic frames responding
  to control values.
- A hand-authored `crates/testkit/fixtures/synthetic-basic.json` exercising every
  PF-derived edge: sparse menu, out-of-range default, out-of-range current value, unknown
  control type, READ_ONLY control. (Real profiles arrive at P1 under `corpus/profiles/`,
  which stays uniformly tool-captured — §3.2; the synthetic fixture keeps P0 hermetic and
  stays forever as the minimal-repro fixture, living with the testkit.)

**Gate G0:** `just ci` green and offline; every shipped gate predicate in `selftest.sh`
with both directions (counted); schema round-trip property tests cover every control type
variant including `Unknown`; the fake passes the backend conformance battery (§2.11 step
4 — the battery itself lands here, run against the fake); metric ordering tests green;
cargo-deny proves the allowlist and each named ban fires on a synthetic violation
(both-directions selftest for the license gate).

## P1 — The V4L2 read path

**Lands:**

- `webcam-handler-v4l2` enumeration: sysfs scan, USB-interface grouping [PF:7], `QUERYCAP
  device_caps` capture-node detection, identity/fingerprint derivation (D1).
- The raw control layer: `QUERY_EXT_CTRL` loop with `NEXT_CTRL|NEXT_COMPOUND`, sparse
  `QUERYMENU` [PF:2], `G_EXT_CTRLS` reads; formats/sizes/intervals enumeration (per-format
  nesting [PF:9]).
- `wch` (direct CLI) with the T4 command core: `list`, `info`, `controls` (table +
  `--json`), `profile capture`.
- The R2 `vivid` rung wired: enumeration + control-model invariants against the virtual
  driver where loadable, counted skip elsewhere.
- R3 hardware smoke recipes (`just smoke-hw`): enumeration matches the attached device's
  committed profile; controls enumerate without panic on every attached node (the PF:1
  regression test, forever).
- Miri wired over the unsafe-adjacent pure units (the raw-struct→`ControlDesc` decode
  path is written as pure functions over captured bytes for exactly this — §2.5).
- The three real device profiles committed with provenance; profile-derived regression
  fixtures asserted in `webcam-handler-fake` replay (the PF:2/PF:4/PF:5 rows exist as corpus, §3.2).

**Gate G1:** the battery's read arms green over the committed v4l2-captured profiles —
stated honestly: profile replay is the *fake's* mechanism, so these arms prove the
control-model and enumeration logic against real-device shapes, while the v4l2 crate's
own ioctl truth lives on R2 (vivid, where loadable) and R3; `wch list/info/controls
--json` output validates against the emitted JSON Schema; the R3 recipe exists and
selects tests, and its dev-machine run — `profile capture` reproducing the committed
profiles byte-identically in the *invariant* section, modulo provenance and the volatile
state block (T3 defines the split; current values and INACTIVE-class flags change with
use and compare loosely) — is recorded as evidence in the notes (the same carve-out G2
uses); the vivid rung reports run-or-named-skip, never silence; Miri
green over the sys-decode units; zero clippy/deny violations, with the v4l `libv4l`
feature ban (feature-posture gate) proven by selftest. Re-runnable: `just gate-g1`,
counted selections.

## P2 — Writes and photo capture

**Lands:**

- `Camera::set` with read-back (D3) and the `{requested, applied}` contract; clamp
  surfaced as warning [PF:6].
- Guarded set: declared pairing table (data), empirical pair discovery
  (`controls --discover-pairs`, INACTIVE-diff method [PF:3]), merge with
  measured-beats-declared provenance (E1).
- Snapshot/restore (D4) with ordering (automation first) and the two-pass INACTIVE
  handling — in-memory at this phase; the *persisted* pre-sweep snapshot lands with the
  session store at P3, where the crash-recovery gate lives.
- Streaming capture: mmap stream, format negotiation with negotiated-result reporting,
  settle policy (D5), frame extraction.
- The photo pipeline (D6): verbatim-JPEG sink, PNG sink, EXIF stamping; `wch get/set/
  snapshot/restore/photo`.
- R3 suites: write/read-back and INACTIVE flip on safe controls (white balance, not PTZ);
  snapshot → perturb → restore → assert byte-identical control state; photo capture
  produces a decodable image at the negotiated size. Every R3 suite restores what it
  touched (§5) — asserted, not promised.

**Gate G2:** battery write/stream arms green on fake replay including the fault menu
(clamp, INACTIVE flip, settle-never-converges, device-gone mid-stream); guarded-set
planner property tests green including the inverse; `wch photo` E2E against the fake
produces EXIF-verified output (read back with the independent reader crate in tests),
including a photo from the GREY-format chicony-ir profile (D6's "grayscale is not
optional" made a criterion); `just gate-g2` counts its criteria; hardware smoke re-run on the dev machine recorded in
the notes (a gate-g2 criterion is that the *recipe exists and selects tests*; the hardware
run itself is evidence recorded, not CI-gating — §3.3 item 1).

## P3 — Calibration

**Lands:**

- The session store (D9): directory layout, `write_json_atomic`, `log.ndjson` with
  torn-line tolerance, fd-lock protocol, fingerprint matching.
- The calibration engine (D8): session lifecycle, per-control status vocabulary, sweep
  execution (guarded set → settle → capture → score → record) with persisted pre-sweep
  snapshot, sample records with `applied` values, metric computation per sample.
- `wch calibrate start/plan/sweep/status/select/apply/list`; `select` records the
  selector identity (`agent`/`human`/`metric:<name>`); `apply` replays with D4 ordering
  against a fingerprint-matched camera; indicatif progress in the CLI.
- Session-schema JSON Schema emitted by xtask; sessions validate against it in tests.

**Gate G3:** full calibration loop E2E on the fake backend: a scripted session over the
synthetic profile reaches `Calibrated` on a control whose fake frame-model has a known
optimum, and `metric:sharpness` selects that optimum (the fake's physics validated in both
directions — a wrong optimum must fail); crash-mid-sweep test kills the process between
write and restore, and recovery restores from the persisted snapshot; store fault menu
walked (disk full, lock held, torn log line, foreign schema_version); `wch calibrate` CLI
subprocess tests over pass and fail trees; `just gate-g3` counted.

## P4 — Daemon and daemon client

**Lands:**

- `webcam-handler-api`: the T5 jsonrpsee trait — **minus the `record_*` methods, which
  join at P6 with their tests** (D10 is completed at P6; the method-count walk's
  population grows by three there and G6 says so) — error-code mapping (one exhaustive
  match over D13), DTO schemas including the photo/record sink DTO; `terminate_holder`
  and `profile_capture` and `discover_pairs` land here with the rest; xtask emits the
  OpenRPC/JSON-Schema bundle.
- `webcam-handler-daemon` (`wchd`): UDS server (tower-service glue), per-camera actors (D12), the
  state-dir lock held for the daemon lifetime, subscriptions (hotplug via the uevent
  socket + parser; calibration progress), idle camera close, SIGTERM/SIGINT parity with
  CancellationToken stream teardown, sd-notify readiness/status, listenfd socket
  activation, tracing with journald layer under systemd.
- `webcam-handler-client` (`wchc`): the same T4 command core over the generated client + the ~200-line
  UDS client transport; subscription rendering (live sweep progress).
- `wch` learns the held-lock refusal message (D9/D13).

**Gate G4:** the full command surface E2E over UDS against the fake backend — every T5
method exercised by at least one integration test, counted against the registered
`RpcModule`'s `method_names()` (built from the real server; docs/4's T5 method-count-walk
row is the authoritative statement of the mechanism — a new registered method with no
test stops the count); subscription
tests cover disconnect-mid-sweep; shutdown tests: SIGTERM and SIGINT each drive
drain-and-release (one test per signal), with an open subscription and a mid-flight sweep;
the lock protocol proven (daemon up ⇒ `wch` mutating ops refuse with the named holder);
`wchc`/`wchd` subprocess tests; parity check: `wch <verb> --json` and `wchc <verb> --json`
byte-identical output on the fake backend for every read verb (the T4 single-surface claim
made mechanical); `just gate-g4` counted.

## P5 — The web client

**Lands:**

- Opt-in TCP listener (D11): loopback default, bearer token minted per run, ready-to-open
  URL printed; axum serving rust-embed'ed `webcam-handler-web` assets; WS JSON-RPC endpoint; MJPEG
  preview route fed by the actor's latest-frame watch channel (slow-consumer drop
  semantics); CompressionLayer excluded from the preview route.
- The vanilla-JS client: camera list, control panel generated from the `controls` DTO
  (sliders/selects honoring sparse menus and flags), live preview, calibration session
  view over the subscription, photo trigger.
- Protocol-level integration tests: token enforcement (401 without, works with), MJPEG
  stream framing (a test client reads N multipart frames), WS RPC round-trip.
- **The R1-web browser rung (design §3.1)**: a pinned Playwright suite, Chromium project
  only, launched as a subprocess from a `webcam-handler-daemon` integration-gate test
  that self-skips (counted, named) without node; browser + package versions pinned;
  traces on failure. Asserts in a real headless Chromium: the control panel renders from
  live DTOs (a sparse menu becomes a select with the right indices), the preview `<img>`
  paints successive MJPEG frames, WS JSON-RPC round-trips and survives reconnect, the
  calibration view tracks its subscription, anonymous requests are refused.

**Gate G5:** web E2E against the fake backend at both altitudes — protocol tests plus the
Playwright rung green (or its skip counted and named on hosts without node; on the dev
machine it runs); the slow-consumer test proves frame dropping (a stalled reader does not
stall capture — asserted via frame counters); shutdown with an open preview tab completes
within the bound; the D11 bind × token matrix enforced as written there (token-less TCP
refused except via the one named loopback-only flag; non-loopback never token-less);
`just gate-g5` counted.

## P6 — Video recording

**Lands:**

- The AVI/MJPEG muxer in `webcam-handler-imaging` (~300 lines): header, `movi`, `idx1`, size/duration
  caps, close-time header rewrite to the measured mean frame interval (D7),
  crash-recoverable stream layout; committed byte-expectation fixtures plus the
  ffprobe/mpv oracle in CI over fake-generated recordings — present-or-counted-skip,
  docs/4 — and on the R3 real capture (external tools as *test oracles only*, per §1).
- Y4M sink for raw capture; `wch record` / `record_start/stop/status` over both CLIs and
  the API (progress via `record_status` polling — D10 defines no recording subscription);
  the `record_*` methods complete the T5 trait here, with their tests joining the
  method-count walk.
- **The agent usage guide**: xtask-generated from the T4 command core (so it cannot
  drift), covering the wch/wchc vocabulary, `--json` contracts, the D13 error
  vocabulary, and a calibration walkthrough — the successor to the vendored skill's
  command sequences; `vendor/v4l2-webcam-skill/` gains a deprecation pointer to it.
- R3: a short real recording on each attached camera, oracle-validated.

**Gate G6:** muxer unit fixtures byte-exact; fuzz/property pass on the muxer's chunk
accounting (never emits a size field that disagrees with bytes written — asserted by
re-parsing our own output with an independent reader path); oracle validation over
fake-generated AVI green where ffprobe is present, counted skip where not; the
declared-vs-wall-clock duration bound runs on the R3 real capture and is
evidence-recorded in the notes (the same carve-out G1/G2 use); cap enforcement tests
(duration, size, disk full mid-recording → typed error, valid file up to the last
complete frame); the T5 method-count walk green over the now-complete trait; the
generated agent guide's examples smoke-checked against the built binaries;
`just gate-g6` counted.

## Post-v1 triggers (recorded, uncommissioned)

| Item | Trigger | Design ref |
|---|---|---|
| UVC H.264 → MP4 remux (L1) | hardware that exhibits `V4L2_PIX_FMT_H264` | D7, §8.3 |
| Control-change events (`VIDIOC_SUBSCRIBE_EVENT`) | live control sync in the web UI | §2.5, §8.4 |
| AV1 encode feature (rav1e) | a real offline-transcode/timelapse need | D7 L2 |
| `wch` auto-forward to daemon | refusal friction observed in real use | §8.7 |
| Session GC | a full disk | §8.8 |
| Cross-session query store (SQLite) | queries at scale | §7 |
| Audio | a license-clean path appears | §8.2 |

## Risks to the plan

- **P1 is the highest-variance phase**: the raw control layer meets real kernels. Buffer:
  PF findings already cover the known traps (types, menus, flags); new ones land as notes
  + corpus the day they appear, and the phase does not close with an unexplained R3
  failure.
- **P4's UDS glue** is the only piece of transport code we own; it is version-coupled to
  jsonrpsee. Contained: integration-tested both sides. If the glue ever forces the
  fallback (TCP-only for v1), that is an amendment to D11, not a quiet cut, and its
  posture is fixed in advance: loopback + mandatory token, with `wchc` reading the token
  from a 0600 file the daemon writes under `$XDG_RUNTIME_DIR`.
- **P5 browser matrix is Chrome by owner ruling** (design §2.7): protocol-contract tests
  plus the automated R1-web Playwright/Chromium rung, with manual verification limited
  to rendering fidelity beyond DOM assertions (design §3.3 item 7); Firefox/Safari
  quirks land as notes and are fixed only when free.
- **Motor-moving tests** (PTZ) stay opt-in even within R3 (`--allow-motion`), so a
  careless `just smoke-hw` cannot physically sweep a camera pointed at someone (§5).
