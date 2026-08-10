# webcam-handler — Implementation Plan (v2)

Doc 7 in the webcam-handler series, **v2 — first revision**. Status: current; supersedes
docs/2 (v1, now under `docs/historical/`). Consumes the design (docs/6); its gate
criteria are enforced by the gate suite (docs/9) and its review bar by the rubric
(docs/8). Section references of the form §n.m point into docs/6 unless prefixed.

**What changed from v1, and why.** Three phases were closed when this revision was
written, so v1's P0–P2 sections are replaced by a closure ledger — which the phases
closed since then join, one row each, rather than growing sections of their own. The
remaining phases are re-cut from four monolithic blocks into **session-sized
sub-milestones** (P3a, P3b, …), because v1's shape failed in a specific, observed way: a
phase that lands everything and then gates-and-reviews in one stretch runs its closing
session out of context mid-review — P2's did exactly that. The gate letters G3–G6 and
their criteria are unchanged; what changed is that criteria now accrete row by row in
`scripts/gates/phase-criteria.tsv` as sub-milestones land, and the phase-closing review
gets a session of its own. Two work items were also added on standing instructions v1 had
already recorded: the mutation floor docs/9 scheduled "before G4, not after" (P3f), and
the `wch-priv` narrowing N8 tied to G6 (P6e).

## Closure ledger — P0–P3

Recorded here so the plan does not restate what the notes and the criteria table
already prove. Evidence entries live in `docs/implementation-notes.md`.

| Phase | Closing commits | Criteria | Evidence | Review |
|---|---|---|---|---|
| P0 — foundations | `ddde6f7` | `g0`: 8 rows | — | gates selftested both directions from day one |
| P1 — V4L2 read path | `59f8293`, fixes through `b7f84c3` | `g1`: 16 rows | E1 (+ its amendments) | 4 confirmed defects, fixed; PF:14–15 and N7 landed, and PF:13 (recorded while P0 was open) became corpus |
| P2 — writes + photo | `52ec45c`, fixes in `7181aef` | `g2`: 25 rows | E2, E3, E4 | 31 candidates, 15 confirmed, fixed; PF:16 and N9 landed |
| P3 — calibration | `abafc25`, `856170a`, fixes in the commit carrying E6 | `g3`: 31 rows | E5, E6 | 31 candidates, 12 confirmed (9 distinct defects), fixed; PF:17–20, N11–N21 landed as the phase ran, N22–N24 with the fixes; the eighth calibrate verb (`calibrate restore`) is the review's one surface change |

Also landed along the way, outside any v1 phase: the privileged development helper
`wch-priv` (§2.13, note N8) and the managed R2 rung (`just rung-vivid-managed`), which
changed what later phases can test — the vivid arms and the P4 hotplug evidence below
exist because of it.

**Standing debts carried into this plan**, each already recorded where it arose:

- ~~`Error::Unimplemented` has two pinned producer surfaces, not one.~~ **Discharged at
  P4c, and the variant itself is gone at P4d.** P4c routed the whole T5 surface, taking
  `daemon::server::unrouted()`, its producer and its phase constant with it; N43 records
  what replaced the assertion (the *partition* over `api::METHODS` became the equality, so
  a twentieth method still cannot land unrouted and unremarked). P4d's uevent watch emptied
  the last row of `webcam-handler-v4l2::unimplemented_surface()`, and the variant, the
  list, its pinned test and D13's lowest RPC code were **deleted** — N6's scheduled death,
  taken by the exhaustive `Error::kind()` match exactly as N6 predicted. The registry is
  eighteen variants and `D13_CODES` is `-32029..=-32012`; no other code moved.
- ~~The camera actor's idle deadline is stamped when a command *starts*.~~ **Discharged at
  P4c**, by N45's clock-free shape: the actor declines the first idle pass after a command
  completes, which costs one sweep cadence and needs no clock — the other shape would have
  reversed N41 and needed a `SteppedClock` that is deliberately not `Sync`, making the fix
  untestable without a wait. Both directions have a test, and `CAMERA_IDLE_CLOSE_MS`'s doc
  carries the one-cadence overshoot beside the number it prices.
- ~~The daemon's socket directory is checked as an inode rather than a name, but the last
  window — holding the directory as a *descriptor* and binding relative to it — needs a
  syscall wrapper.~~ **Discharged at P4d** with `rustix` 1.1.4 (`fs`, `net`, `process`), no
  `unsafe` block and no `deny.toml` change. The directory is opened
  `O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC`, `fstat`ed, checked for mode **and** `st_uid`
  against `geteuid()`, and held open; the `statat`, the `unlinkat` and the bind all go
  through that descriptor. The literal ask turned out to be unreachable — Linux has no
  `bindat(2)` — so the bind goes through `/proc/self/fd/<dirfd>/wchd.sock`, which is the
  dirfd-relative bind spelled the way Linux offers it and was measured to land in the
  checked inode after a directory swap. Substitution is now *defeated* rather than
  detected. The residual is written into N39's amendment: `/proc` must be mounted (there is
  a named `warn!` fallback), and `$XDG_RUNTIME_DIR` itself is still resolved by name once.
- ~~`CAP_NET_ADMIN` was granted on an unverified prediction; P4d measures whether the
  uevent bind needs it (§8 item 10) and G6 consumes the answer.~~ **Measured at P4d, and
  the prediction is disproved** (PF:21): on kernel `7.0.0-29-generic` a process with an
  empty effective capability set binds `NETLINK_KOBJECT_UEVENT` group 1 *and receives* a
  whole `uvcvideo` cycle — 56 packets, `ENOBUFS` never raised, measured three times. Both
  halves are separate claims and both were taken; the R3 arm (E9) is the second one running
  through this workspace's own socket rather than a probe. N8's row carries the amendment.
  **P4d does not re-bless**: the narrowing is still G6's (P6e), which now has a fact to
  execute on instead of a prediction — `cap_sys_module` is untouched, because `modprobe`
  still needs it.
- D12's `wait` flag — "a second capture request queues or is refused with `Busy` per its
  `wait` flag" — is **re-deferred from P4c to P4e**, with its reason recorded (N42). P4c
  routes the capture verb, so the half of D12 that exists is reachable over the wire and
  asserted there: two photos in flight against one camera reach one actor, share one
  descriptor, and stream sequentially, and a caller past
  `limits::CAMERA_COMMAND_QUEUE_DEPTH` is refused. The flag that chooses between the two is
  three changes wearing one name — a field on a committed wire shape (nothing in the schema
  is called `wait`, so both bundles move), an enqueue that waits with a bound where
  `CameraActor::submit` has only a `try_send`, and a command-line spelling or an argued
  absence of one. P4e already owns the concurrency semantics of a client that goes away
  mid-operation, which is the same question from the other end, and P4c's own "Lands"
  sentence never names the flag.
- `terminate_holder` reached the wire at P4c with **no command-line spelling**, and the
  absence is counted rather than assumed (N48). `schema::report`'s header had already put
  `TerminationReport` in the OpenRPC document rather than the JSON Schema bundle for that
  reason. A T4 verb would move two derived gate populations — `json-validates.sh`'s, scraped
  from `wch --help`, and P4f's parity population — so until a sub-milestone wants the verb
  the method is reachable only by a client that speaks raw JSON-RPC. Nothing here schedules
  it.
- ~~The empty `CameraId` a hand-written client can send is a wildcard on every method that
  names a camera.~~ **Discharged at P4c** (note **N50**), and the reason it had been deferred
  was wrong: this half was bundled with `PixelFormat`'s and both were put off because "each
  moves a committed bundle". `Deserialize` and `JsonSchema` are separate derives, so routing
  the first through `CameraId::parse` changes which strings are accepted and not what the type
  emits — `./scripts/gates/schema-artifacts-current.sh` is green with both artifacts
  byte-identical, which is the disproof rather than the argument. `ControlSlug`'s empty case
  stays as it is and stays honest: it lands on `ControlUnknown`, and nothing chooses on the
  caller's behalf.
- `PixelFormat` crosses the wire as its four bytes where the CLI spells it `MJPG`, on `photo`
  and `calibrate_sweep` both — raised against the T5 surface at P4a, reachable by a
  hand-written client since P4c. The fix is a line, and this one really does move a committed
  bundle: `serde` and `schemars` both describe the field, so the emitted JSON Schema and the
  OpenRPC document change together. It wants its own commit and a gate diff rather than a
  ride-along, and nothing here schedules it.
- A `Sink::ServerPath` naming a **fifo** is refused by a `stat`, and a client that replaces the
  path between the `stat` and the write still parks the camera's actor thread (note **N51**).
  P4c closed the reachable shape — `mkfifo /tmp/x.jpg` plus one `wch_photo` used to wedge a
  camera until `wchd` restarted, and `/dev/stdout` used to put a frame in the journal — and
  the residual race needs the *open* to be non-blocking, whose flag has no portable spelling
  reachable from a crate that does not link `libc`. **P4e** owns it, with the bound on a
  submitted command that N42 already deferred there: `CameraActor::submit` has only a
  `try_send` and nothing times an actor command out, which is the same missing mechanism, and
  a command that cannot outlive a deadline turns the residual race into a refusal.
- The `wch-priv` powers are broader than demonstrated need, time-boxed to the plan; P6e
  executes the narrowing ruling (N8).
- ~~The mutation floor is commissioned before G4 (docs/9's recorded schedule); P3f.~~
  **Discharged 2026-08-09**: `just mutants` over the six pure cores, its survivors triaged,
  and the schedule mechanised as the `g4` row of `phase-criteria.tsv` rather than left as
  a date. Evidence E7; posture, cadence and cost in docs/9's gaps register.

## Standing conventions, in force from P0 — v2 restatement

Carried, with the mechanisms that now exist named:

- **`docs/implementation-notes.md` is case law.** Deviations land as N-entries, hardware
  behavior as PF-entries continuing the §1.2 registry, phase evidence as dated E-entries
  (append-only). Entries are written the day the thing is learned, not at phase close.
  Reviews do not re-report an entry; empirical disproof retires one.
- **A fix or feature lands with its gate, in the same PR** (rubric rule 1). The
  commissioning record is docs/9 Part 2.
- **Every criterion is a row.** `scripts/gates/phase-criteria.tsv` holds one row per
  criterion; `just gate-gN` runs a phase's rows and counts them;
  `counted-selections.sh` proves no row's selection has gone to zero. A sub-milestone
  that lands a criterion adds its row in the same commit — the gate is partially
  meaningful from a phase's first session, not only at its close.
- **Milestones are session-sized.** The unit of work is the sub-milestone: one
  implementation session ending at a committed boundary with `just ci` green and the
  notes current. A sub-milestone that turns out to be two splits — recorded in the
  notes — rather than stretching past what one session can carry.
- **The phase review is its own session.** The adversarial review (docs/8 Part E) runs
  against the phase's diff with fresh context; confirmed findings are fixed in a
  follow-up commit (the P1/P2 precedent). Landing and reviewing in one session is the
  failure mode this plan's shape exists to remove.
- **Corpus discipline** (§3.2): profiles tool-captured, committed with provenance,
  immutable; re-capture replaces wholesale. New device behavior lands as corpus + a note
  the day it is seen.
- **Hardware needs**: P3, P4 and P6 want the attached cameras for R3 evidence; P4
  additionally wants the blessed helper (hotplug cycling); P5 needs no camera (fake +
  browser). The vivid arms run anywhere the module is loadable via
  `just rung-vivid-managed`. All hardware suites — `hw_` and `vivid_` — serialize in
  the one-thread `exclusive-device` nextest group (§3.1): one streamer per node is the
  kernel's rule, not a suite inconvenience. Motor-moving suites (`hw_motion_*`) run
  with the rest of R3 by default (owner ruling, 2026-08-08 — §5 carries the split:
  test default motors-on, product default motors-off); `WCH_NO_MOTION=1` excludes them
  as a named, counted skip for runs where the camera is pointed at someone, and motor
  sweeps stay bounded by the `limits` caps.

## P3 — Calibration

The smallest thing an agent can use end-to-end (capture→sweep→score→apply, in-process)
lands before the daemon, so P4 wires transport around a working core. Gate **G3**
criteria are v1's, distributed below; `just gate-g3` accretes them.

### P3a — The session store (D9)

**Lands:** `engine::store`: the session directory layout under the XDG state dir
(`paths` already owns the two XDG roots — N2); `write_json_atomic` (tempfile in-dir →
`sync_all` → rename → fsync parent) as the one home for state writes; the one advisory
fd-lock at the state-dir root with both protocols (daemon holds for lifetime; daemonless
`wch` takes per mutating operation); `log.ndjson` append and load — a torn *last* line
is dropped, a torn middle line is typed corruption; fingerprint-matched session lookup;
the store fault menu as an exhaustive-match enum: disk full (`StorageIo`), lock held
(`StoreLocked { holder }`), torn line, foreign `schema_version`
(`SchemaVersionForeign`). The temp-dir store double for engine tests.

**Proves / gate rows:** every fault-menu variant driven both directions;
`atomic-write-home.sh` widened per its docs/9 P3 row — it learns the session-dir
patterns and gains the pass-direction arm over the now-real home, so the P0 predicate
stops being green about a home that did not exist.

### P3b — Session lifecycle and crash recovery

**Lands:** session create/resume wiring the D8 state machine (a P0 pure core) to the
store; the **persisted pre-sweep snapshot**, written before the first write of any
sweep; the recovery path — a loaded session with an unconsumed pre-sweep snapshot
restores it, reporting N9's four-outcome vocabulary (`OwnedByAutomation` is complete;
`StillInactive` is not); `IllegalTransition` refusals surfaced through the engine.

**Proves / gate rows:** the crash-recovery criterion — kill the process between write
and restore, recovery restores from the persisted snapshot; lock-interaction tests in
both orderings; state-machine refusal fixtures (`select` before sweep, `apply` with
nothing calibrated and no `--partial`).

### P3c — Sweep execution and scoring

**Lands:** the sweep executor: guarded set → settle → capture → score → record, one
sample row per value `{value, applied, photo, metrics, timestamp}` — `applied` because
D3 applies inside sweeps [PF:6]; per-sample metrics (the P0 imaging set); sample photos
under `photos/<control-slug>/` as relative camino paths with EXIF carrying the control
values in effect (P2 pipeline, PF:16 splice); empirical pair discovery at session start
(D3, with the three E4 probe rules); **the progress hook** — a schema-shaped event
stream the CLI renders now and P4e's subscription transports later, defined here so P4
wires transport around it instead of re-plumbing it.

**Proves / gate rows:** the G3 headline criterion — a scripted session over the
synthetic profile reaches `Calibrated` on a control whose fake frame-model has a known
optimum, and `metric:sharpness` selects that optimum, with the physics validated in
both directions (a wrong optimum must fail; the expectation stated from the fixture,
never from the fake's own model — rubric Part C). **R2 arm:** one real sweep over a
writable vivid control through the actual ioctl path (`rung-vivid-managed`) — the sweep
loop meets a real driver without touching a camera.

### P3d — The `calibrate` verbs

**Lands:** `wch calibrate start/plan/sweep/status/select/apply/list` in the T4 command
core; `select` records selector identity (`agent`/`human`/`metric:<name>`); `apply`
replays with D4 ordering against a fingerprint-matched camera and refuses naming the
differing fields; `--partial` as the only path around uncalibrated controls; indicatif
progress consuming the P3c hook; session files validate against the xtask-emitted
session schema in tests; `json-validates.sh` learns the new verbs (its verb population
derives from `--help`, so landing them without validation rows fails the gate).

**Proves / gate rows:** CLI subprocess tests over trees built to pass and to fail;
schema-validation rows.

### P3e — G3 close

**Lands:** any criteria rows not yet accreted; the R3 evidence run — a real calibration
session on the Chicony RGB over a brightness-class control *and* a bounded PTZ sweep on
the OBSBOT (motors run by default in testing — the §5 ruling; the sweep restores the
motor position and asserts it), sweep,
select, apply, restore asserted, recorded in the notes with transcripts (the G1/G2
carve-out: the recipe existing and selecting tests is the criterion; the run is
evidence).

**Then, in its own session:** the adversarial review over the P3 diff; confirmed
findings fixed in a follow-up commit; a dated evidence entry; rubric reconciliation
appended (docs/8 Part E). **Done** — E6 records it, and the ledger above carries the
phase's row. The one thing the review changed on the surface is an **eighth** calibrate
verb, `restore`, which spends the persisted pre-sweep snapshot: D4 and §5 say a sweep
leaves the camera as it found it and nothing shipped could (note N23). D10's `calibrate_*`
method list therefore lands at eight when P4c routes it.

### P3f — The mutation floor

Commissioned before G4, as docs/9 recorded at v1. **Lands:** a mutation-testing job
(`cargo-mutants`-class, a dev tool, never a dependency) scoped to the pure cores —
planners, state machine, settle, metrics, store logic — runnable as `just mutants`;
first-run survivor triage: every surviving mutant becomes a missing test or a recorded,
reasoned acceptance; the posture and cadence recorded in docs/9's delta section. Its own
session because the triage, not the wiring, is the work.

## P4 — Daemon and daemon client

Transport around the proven core. Gate **G4** criteria are v1's, distributed below.

### P4a — The wire trait (T5) and codes

**Lands:** `webcam-handler-api`: the jsonrpsee `#[rpc(server, client)]` trait — minus
the `record_*` methods, which join at P6 with their tests (D10 completes there and G6
says so) — over schema DTOs; the D13→RPC-code mapping as one exhaustive match (nineteen
codes at P4a; P4d shrank it to eighteen by deleting the lowest, as arranged); the
two-variant sink DTO semantics
(`ReturnBytes`/`ServerPath`, client-side cwd resolution before sending); xtask OpenRPC
emission, with `schema-artifacts-current.sh` widened to the OpenRPC bundle (docs/9 P4
row). N5's wall for this crate — tokio allowed, no axum/hyper/tower-http — is already
gate-asserted; re-verify nothing new leaked.

**Proves / gate rows:** the exhaustive code-mapping walk; DTO round-trip fixtures;
OpenRPC drift both directions.

### P4b — Daemon skeleton: UDS, lock, actors

**Lands:** `wchd`: the UDS server glue (tower-service over `UnixListener`; the one piece
of transport code we own, version-coupled to jsonrpsee and integration-tested on both
sides), socket directory 0700 asserted at startup; the state-dir lock held for the
daemon lifetime and `wch`'s held-lock refusal ("daemon owns the state — use wchc");
per-camera actor threads (D12) with open-on-first-use and idle close; read-verb routing
(`list`, `info`, `controls`, `get`, `calibrate status/list`); `tracing` fmt layer.

**Proves / gate rows:** read-verb integration tests over in-memory and UDS transports;
the UDS-permissions row (0700 asserted in test; other-uid check where CI permits, else
a named skip); open/idle observable via the status surface and tested.

### P4c — The full method surface

**Lands:** the mutating half over RPC: `set` (guarded flag), `snapshot`/`restore`,
`photo` (both sink variants), `discover_pairs`, `profile_capture`,
`terminate_holder { camera, pid }` (refuses when the pid no longer holds the device —
`HolderGone`), and the `calibrate_*` verbs routed to the P3 engine.

**Proves / gate rows:** the **T5 method-count walk** lands: the registered
`RpcModule`'s `method_names()` — derived from the real server registration, never a
hand list — compared against the integration-test inventory; a registered method with
no test stops the count. Every method exercised over the fake.

### P4d — Hotplug, the privilege measurement, and the death of `Unimplemented`

**Lands:** the AF_NETLINK uevent socket (~30 lines, inside `src/sys/` like every unsafe
or kernel-facing edge), `kobject-uevent` parsing, subsystem filter, debounce,
re-enumeration; `CameraBackend::watch` on the **V4L2 backend** — the fake's scripted
watch (add/remove from its fault menu) has existed since P0 and already passes the
battery's watch arm, so this sub-milestone brings the real backend to parity with it.
**The measurement:** bind `NETLINK_KOBJECT_UEVENT` *unprivileged
first* on this kernel and record the answer in the notes — §8 item 10; N8's
`CAP_NET_ADMIN` grant was a prediction, and G6's narrowing consumes the truth. **The
deletion:** `Error::Unimplemented` loses its last producer; the variant, its
`unimplemented_surface()` list, and its pinned test are deleted in this sub-milestone
(N6 retires; the exhaustive `Error::kind()` match and the api code match drive the
removal through the compiler).

**Proves / gate rows:** hostile-bytes fixtures for the netlink parser (malformed,
truncated, flood — rubric B10); the v4l2 watch's proof is those fixtures plus the R3
arm — the battery runs against the fake, whose watch arm has been green since P0;
**R3:** one `uvcvideo` cycle via the blessed helper with every camera closed
produces remove+add events through `watch` (the interlock honored — §3.3 item 9 keeps
mid-stream loss fake-only, and this arm's design respects that).

### P4e — Subscriptions and shutdown

**Lands:** `subscribe_events` (hotplug) and `subscribe_calibration` (transporting P3c's
progress hook); disconnect-mid-sweep semantics — the sweep continues, the subscription
is reaped, both asserted; shutdown discipline: SIGTERM ≡ SIGINT through
`CancellationToken` teardown, drain, store-lock release, `sd_notify(STOPPING)`;
`sd-notify` READY/STATUS, `listenfd` socket activation, journald layer under systemd;
the daemon never self-daemonizes.

**Proves / gate rows:** one real-delivery test per signal, each with an open
subscription and a mid-flight sweep; disconnect-mid-sweep both assertions;
sd-notify/listenfd behavior tests where the harness allows, named skips where not.

### P4f — `wchc` and parity

**Lands:** the UDS client transport (~200 lines, modeled on reth's IPC adapter —
vendored knowledge, not a git dependency); the T4 executor over the generated client;
connection bootstrapping and the daemon-not-running refusal; subscription rendering
(live sweep progress in `wchc`); documented exit codes.

**Proves / gate rows:** the **CLI parity gate**: `wch <verb> --json` and
`wchc <verb> --json` byte-identical on every read verb over the fake, the population
derived from the T4 verb list with local-only verbs named, never silently exempted;
`wchc` subprocess tests over pass and fail trees.

### P4g — G4 close

**Lands:** remaining criteria rows, counted; R3 evidence — the daemon against the real
cameras: a photo over UDS, a calibrate sweep over UDS with live `wchc` progress —
recorded in the notes. The **hotplug cycle is already recorded**, as note E9 at P4d, with
the transcript and the two mutants that arm was watched failing; G4's own entry cites it
rather than re-running it, because a transcript written twice is two transcripts nobody
can tell apart. **Then, in its own session:** the
adversarial review; fixes; evidence entry; rubric reconciliation (the per-gate cadence
docs/8 Part E schedules — this appends G4's instance to its record).

## P5 — The web client

Everything here runs against the fake backend; no camera required, so every rung is
deterministic. Gate **G5** criteria are v1's, distributed below.

### P5a — The TCP listener and the token gate

**Lands:** the opt-in axum listener (`--http [addr]`, default `127.0.0.1:0`, bound port
reported); per-run bearer token, printed as a ready-to-open URL; the D11 bind × token
matrix enforced *as written in D11* (the gate cites the paragraph, not a paraphrase):
token-less loopback only behind `--http-insecure-loopback`; non-loopback always
token-gated, no flag removes it, plus the warning naming what it exposes; rust-embed
asset serving skeleton.

**Proves / gate rows:** 401-without/200-with; one test per matrix cell; the
`no-external-fetch-in-web.sh` non-vacuity arm (an empty asset directory fails, so the
gate cannot go green scanning nothing).

### P5b — WS RPC and the MJPEG preview

**Lands:** the WS endpoint speaking the same JSON-RPC (the T5 trait has one home; WS is
a transport); the MJPEG preview route (`multipart/x-mixed-replace`) fed by the actor's
latest-frame watch channel; slow-consumer drop semantics; `CompressionLayer` excluded
from the preview route; shutdown with an open preview tab completes within the bound.

**Proves / gate rows:** a test client reads N multipart frames; the stalled-reader test
(capture frame counter advances while the reader's does not); the compression-exclusion
test (uncompressed preview with compression active elsewhere); shutdown-with-open-tab.

### P5c — The web client

**Lands:** vanilla ES modules, no build step, no npm, no CDN: the ~50-line
JSON-RPC-over-WebSocket helper, camera list, control panel generated from the
`controls` DTO (range → slider, sparse menu → select with the right indices [PF:2],
flags surfaced), live preview `<img>`, calibration session view over the subscription,
photo trigger. Chrome is the target (owner ruling, §2.7).

**Proves / gate rows:** protocol-level integration tests; whatever DTO-render logic is
assertable without a browser — the browser truth is P5d's, and a browser behavior
verified only through the JSON the page consumes is not verified (rubric B7).

### P5d — The R1-web rung

**Lands:** the pinned Playwright + Chromium suite (versions pinned, traces on failure),
subprocess-launched from a daemon integration-gate test that self-skips **counted and
named** without node — node is never a build dependency. Asserts in a real headless
Chromium: the control panel renders from live DTOs (a sparse menu becomes a select with
the right indices), the preview `<img>` paints successive MJPEG frames, WS JSON-RPC
round-trips and survives reconnect, the calibration view tracks its subscription, and
anonymous requests are refused.

**Proves / gate rows:** the docs/9 R1-web row; on the dev machine the rung runs, and the
gate close records whether it ran or skipped, by name.

### P5e — G5 close

**Lands:** remaining criteria rows, counted; evidence — the web E2E at both altitudes
(protocol + browser rung transcript) recorded in the notes. **Then, in its own
session:** the adversarial review; fixes; evidence entry; reconciliation.

## P6 — Video recording

Gate **G6** criteria are v1's, distributed below; the phase ends with the plan's two
standing reckonings (the agent guide, and the helper narrowing).

### P6a — The AVI muxer

**Lands:** `imaging::avi` (~300 lines, frozen format): RIFF header, `movi`, `idx1`;
duration and size caps; close-time header rewrite to the measured mean frame interval
(D7's CFR carve-out); crash-recoverable stream layout (documented: a crash leaves a
readable `movi` prefix); committed byte-expectation fixtures; an independent re-parse
path that is not the writer's code.

**Proves / gate rows:** byte-exact fixture tests; the chunk-accounting property test —
the muxer never emits a size field that disagrees with bytes written, proven by
re-parsing our own output.

### P6b — `record` in the engine and CLI

**Lands:** the Y4M sink (mono and 4:2:x are both in the Y4M vocabulary); the record
executor with duration/size caps from `limits`; disk-full mid-recording → typed error
and a valid file up to the last complete frame; `wch record`; the MJPG-only muxer
policy — non-MJPG requests get Y4M or `FormatUnsupported { available }`.

**Proves / gate rows:** cap-enforcement tests (duration, size, disk-full); CLI
subprocess tests; every fault leaves a parseable file.

### P6c — The wire completes

**Lands:** `record_start/stop/status` on the T5 trait (D10 is now complete), daemon
routing, `wchc` rendering; progress by polling `record_status` — no recording
subscription in v1.

**Proves / gate rows:** the method-count walk green over the now-complete trait (its
population grows by three); the parity population learns `record_status`.

### P6d — Oracles and the R3 recording

**Lands:** the ffprobe/mpv oracle harness over fake-generated AVI —
present-or-counted-skip (docs/9's oracle-accounting row); **R3:** a short real
recording on each attached camera, oracle-validated, with the declared-vs-wall-clock
duration bound measured on the real capture and evidence-recorded (the D7/§3.3 CFR
limitation, bounded rather than wished away).

**Proves / gate rows:** oracle validation green-or-named-skip; the duration-bound
criterion; evidence entry.

### P6e — The agent guide, G6 close, and the helper reckoning

**Lands:** the xtask-generated agent usage guide from the T4 command core (so it cannot
drift): wch/wchc vocabulary, `--json` contracts, the D13 error vocabulary, a
calibration walkthrough — the successor to the vendored skill's command sequences;
`vendor/v4l2-webcam-skill/` gains its deprecation pointer; guide examples smoke-checked
against the built binaries, regeneration diffs clean in CI (docs/9 row).

**G6 close:** all criteria rows counted; **then, in its own session,** the adversarial
review; fixes; evidence entry; reconciliation.

**Then the N8 ruling executes** (the owner's recorded trigger): answer the three
questions with the plan's evidence — which capabilities were actually spent (P4d's
measurement decides `CAP_NET_ADMIN`), whether `exec` ever did more than delegate to a
test process, whether anything routine still loads modules unattended — and narrow or
delete `wch-priv`, `just bless`, and `privileged-helper.sh` accordingly, recording the
outcome in the notes. This is a plan step, not a memory.

## Post-plan triggers (recorded, uncommissioned)

| Item | Trigger | Design ref |
|---|---|---|
| UVC H.264 → MP4 remux (L1) | hardware that exhibits `V4L2_PIX_FMT_H264` | D7, §8.3 |
| Control-change events (`VIDIOC_SUBSCRIBE_EVENT`) | live control sync in the web UI | §2.5, §8.4 |
| AV1 encode feature (rav1e) | a real offline-transcode/timelapse need | D7 L2 |
| `wch` auto-forward to daemon | refusal friction observed in real use | §8.7 |
| Session GC | a full disk | §8.8 |
| Cross-session query store (SQLite) | queries at scale | §7 |
| Audio | a license-clean path appears | §8.2 |
| Re-run N5's jsonrpsee measurement | any jsonrpsee bump — delete the api tokio exemption if the original wall becomes satisfiable | §2.8, note N5 |
| Re-check PF:16 against `little_exif` | any little_exif bump — the splice likely stays (it keeps a device-byte parse under our rules), but the *reason* changes from fix to defense | D6, PF:16 |
| Narrow or delete `wch-priv` | **P6e executes this**; the row stays here until it does | §2.13, note N8 |

## Risks to the plan

- **The context budget is a real resource.** v1 treated it as free and P2's closing
  session proved otherwise. Mitigation is structural: session-sized sub-milestones,
  reviews in their own sessions, a split-don't-stretch rule. The residual risk is a
  sub-milestone mis-sized anyway; the notes record splits so sizing improves.
- **P4's UDS glue** is the only transport code we own and is version-coupled to
  jsonrpsee. Contained: integration-tested both sides; the fallback posture (TCP-only,
  loopback + mandatory token, token file mode 0600 under `$XDG_RUNTIME_DIR`) is fixed
  in advance as a D11 amendment path, never a quiet cut.
- **The subscription seam lands in two phases** — the hook at P3c, the transport at
  P4e. If the hook is not schema-shaped from the start, P4 re-plumbs it; P3c's design
  requirement exists precisely to prevent this.
- **The hotplug interlock bounds the evidence.** Mid-stream device loss on real
  hardware is unmeasurable by design (§3.3 item 9); the fake's fault menu carries that
  behavior alone. Accepted and named rather than worked around.
- **P5's browser matrix is Chrome by owner ruling** (§2.7): protocol tests plus the
  Playwright/Chromium rung; Firefox/Safari defects land as notes and are fixed only
  when free.
- **Motor-moving tests run by default** (owner ruling, 2026-08-08 — §5). The accepted
  residual risk is a `just smoke-hw` physically sweeping a camera pointed at someone;
  the mitigations are the `WCH_NO_MOTION=1` opt-out, the `limits` motion caps, and
  restore-and-assert on every motion arm. The ruling is one script default plus one §5
  paragraph, so reversing it is a one-commit revert if it proves wrong.
- **Kernel/driver variance remains the standing unknown** — P1's highest-variance work
  is done, but every remaining phase still meets the kernel somewhere (P4's netlink,
  P6's frame timing). The buffer is unchanged: PF findings land as notes + corpus the
  day they appear, and no phase closes with an unexplained R3 failure.
