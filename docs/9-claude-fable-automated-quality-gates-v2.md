# webcam-handler — Automated quality gates (v2)

Doc 9 in the webcam-handler series, **v2 — first revision**. Supersedes docs/4 (v1, now
under `docs/historical/`). Gates for docs/8 (rubric
v2, Part D), consumed by docs/7's phase gates. Convention inherited from the predecessor
and in force from the first commit: **the repository's files are
authoritative; this document records the commissioned set, deltas, and rationale.** A
drift between this document and the repo's files is a defect in whichever one the
evidence says is wrong. This revision brings the record up to the G2-closed tree: Part 1
describes the suite as built (including where deviations N1, N3 and N5 moved it), Part
2's commissioning table strikes what landed, and the gaps register is regenerated.

**The structural rule this suite is built around** (predecessor's costliest lesson,
adopted here from day one rather than learned again): *a gate written to close a defect
is not itself tested against its own inverse, and the second arm is where it fails.*
Therefore: every multi-line CI predicate lives in `scripts/gates/*.sh`; all of them run
under one table-driven `scripts/gates/selftest.sh` that requires **both directions per
gate** — a predicate with no failing case fails the selftest, and one with only failing
cases fails it too. Compiled predicates (Rust tests standing in for a shell gate) are
deviations to record in the implementation notes where they happen, not silent
exceptions. Second structural rule, same provenance: **checks must not name locations
they can drift from** — populations are derived (from `cargo metadata`, from a `find`,
from an exhaustive match), never transcribed lists. Third structural rule, paid for
locally this time [S:N10]: **the inverse arm is driven by the thing under test, not by a
model of it** — `counted-selections.sh` shipped green-by-construction because its
failing arm used a stub returning a shape the real tool never produces; where a stub is
unavoidable (a seeded table is cheaper than a rebuilt workspace), one arm still runs the
real tool, and that arm is what notices when a tool release changes shape.

## Part 1 — The suite as built (P0 baseline + P1/P2 accretions)

Landed with the workspace scaffold and grown since; G0 required all of it green and
self-tested, and every later gate runs it again (`just ci` includes `gates`).

### `just ci` (offline, ≡ CI by construction)

- `cargo fmt --check`; `cargo clippy --workspace --all-targets -- -D warnings` with the
  banned-API set below; `cargo nextest run --workspace` with `--no-tests=fail` and
  `retries = 0`; `cargo doc` build; `cargo deny check bans licenses sources` (advisories
  live in the networked job — keeps `ci` offline); `typos`; `cargo machete`;
  `shellcheck` over `scripts/` **including `scripts/gates/`**; `--locked` everywhere.
- `ci.yml` runs `just ci` verbatim. Divergences between local and CI are recorded in this
  document when they appear; the target count is zero (the daemon needs no CI-resident
  credentials — this project's advantage over its predecessor).

### clippy configuration (lint gates)

- `disallowed-methods`: `v4l::device::Device::query_controls` (panics on unknown control
  types — PF:1; our raw layer is the home), `std::process::exit` outside `main`,
  `std::thread::sleep` **workspace-wide** (deviation N3: `clippy.toml` has no notion of
  test-vs-not, and a global ban with named `#[expect(..., reason)]` exceptions is
  auditable where a test-only ban is not; there are no production sleep sites yet).
- `disallowed-types`: none at P0; grows with evidence.
- On device/request-driven paths (enforced by module-scoped lint levels):
  `unwrap_used`, `expect_used`, `panic`, `indexing_slicing`, `as_conversions`.

**The lint policy's home is `[workspace.lints]`** (deviation N1 — one table in the root
`Cargo.toml`, every crate opting in with `[lints] workspace = true`), not the
thirteen-copy crate-root attribute block v1 wrote. The policy: deny
`unsafe_op_in_unsafe_fn`, `clippy::undocumented_unsafe_blocks`,
`clippy::missing_safety_doc`, `clippy::multiple_unsafe_ops_per_block` (one obligation
per SAFETY), `clippy::allow_attributes`, `clippy::allow_attributes_without_reason`.
Cargo's table cannot express `cfg(test)`, so the two suppression-hygiene lints bind
test code as well — strictly *stronger* than the v1 text: test suppressions also write
`#[expect(..., reason = "...")]`. One attribute cannot live in the table:
`#![forbid(unsafe_code)]` stays a crate-root attribute on **every crate except
`webcam-handler-v4l2`** (which must not have it; its `unsafe` is confined to `src/sys/`
by `unsafe-scope.sh` below), and the gate asserts each root carries it — the copies are
gate-checked, not trusted. `webcam-handler-v4l2` additionally denies
`clippy::cast_possible_truncation`, `cast_sign_loss`, `cast_possible_wrap` — the
`try_from`-not-`as` rule as a lint on the one crate that reads kernel-shaped integers.

### `scripts/gates/` — the predicate population, as built

| Predicate | Catches | Both-direction cases |
|---|---|---|
| `license-allowlist.sh` (drives `cargo deny`) | any copyleft/off-list license entering the tree; each named ban (`v4l-sys`, `v4l2-sys`, `udev`/`libudev-sys`/`tokio-udev`, `libcamera*`, `alsa*`/`cpal`, `colored`, `minimp4`, `env-libvpx-sys`/`vpx-encode`/`webm`, `ffmpeg-next`/`-sys`, `x264`/`x264-dev`/`x265`, `dssim`, `jpeg-encoder`/`turbojpeg`/`mozjpeg`, `option-ext` — N2's `directories` door; the authoritative population is `deny.toml`, each entry carrying its reason) | a scratch manifest **with a committed lockfile** carrying a banned dep must fail under `cargo deny --offline` (the selftest never resolves the network — that is how the failing arm stays inside an offline `just ci`); the shipped tree must pass |
| `feature-posture.sh` | `v4l` with non-default features (the `libv4l` LGPL door); `image` with default features (the `avif`→rav1e drag); any TLS feature (CDLA cert stores); checked from `cargo metadata`, not manifest grep | a scratch manifest flipping each posture must fail |
| `dependency-walls.sh` | T6 violations from `cargo metadata`, populations derived from §2.8's edge list: tokio/axum/hyper linked by `schema`/`imaging`/`fake`/`cli-core`; **`api`'s own wall — no axum, no hyper, no tower-http, tokio allowed** (deviation N5: one `#[rpc(server, client)]` trait links tokio necessarily; re-run N5's measurement on any jsonrpsee bump); backends linked by anything but the two composition roots (`webcam-handler-cli`, `webcam-handler-daemon`); `client` linking a backend or the engine (the thin-client wall); V4L2 types escaping the backend (grep for `v4l::`/`v4l_sys::` outside `crates/backends/v4l2/`) | a scratch edge in each direction |
| `unsafe-scope.sh` | the token `unsafe` outside `crates/backends/v4l2/src/sys/` (rubric B10 [V]); the allowed path is derived from the tree, and the other crates' `#![forbid(unsafe_code)]` roots are asserted present | a seeded unsafe block elsewhere must fail; a missing `forbid` root must fail; the sys module must pass |
| `testkit-is-dev-only.sh` | `webcam-handler-testkit` on a normal edge (it may grow tooling deps that must never ship) | non-vacuity arm: fails if nothing dev-depends on it |
| `atomic-write-home.sh` | `serde_json::to_writer`/`fs::write` targeting the state dir outside `webcam-handler-engine::store` (rubric A5: bypassing the home) | a scratch bypass must fail; a clean tree passes (P0's pass arm — the P3 widening row adds "the real home exists and passes", so this predicate is never vacuously green about a home that does not yet exist) |
| `no-frame-bytes-in-repo.sh` | committed camera frames: content-sniffs images in the tree; allows only `corpus/images/` whose fixtures carry a `generated-by` provenance marker and dimensions below the fixture cap | a scratch JPEG without provenance must fail; a provenanced synthetic must pass |
| `no-external-fetch-in-web.sh` | CDN/script-src URLs, `fetch(` to non-relative origins, and `<script src` with a scheme in `webcam-handler-web` assets | one seeded violation per pattern; the P5 widening row adds the non-vacuity arm (an empty asset directory fails, so the gate cannot go green by scanning nothing) |
| `schema-artifacts-current.sh` | committed JSON Schema bundles drifting from the `webcam-handler-schema` types (re-runs xtask emit, diffs); the OpenRPC half joins at P4 with the trait it documents (Part 2) | a scratch type edit must fail; clean tree passes |
| `counted-selections.sh` | gate recipes whose test filters select zero (selections compare `(package, binary)` pairs; the `grep -c`-under-`pipefail` trap — a zero count exits nonzero — is known and avoided by construction throughout `scripts/`). **Corrected once already [S:N10]**: `cargo nextest list -T json` lists the whole workspace whatever the filter says, so the count is `filter-match.status == "matches"`, never `test-count` — the original predicate could not count to zero and was green by construction from the day it shipped | a recipe naming a nonexistent test must fail; the selftest includes a **real-tool arm** (the actual nextest over a filter matching nothing), because the stubbed arm is what hid the defect |
| `msrv-sync.sh` | the one MSRV fact diverging across `Cargo.toml`s and CI | seeded divergence |
| `ignored-suites-have-recipes.sh` | every `#[ignore]`d suite named by a `just` recipe, every recipe selecting a real `(package, binary)`, and every declared `wch-suite:` prefix present in the `exclusive-device` nextest group (so a rung cannot be added or renamed without the serialization following it). Its test-group half reads only the group's own overrides and whole-prefix matches, both directions — the P2 review found it matching prefixes anywhere in a filter, including subtracted ones [S:E4] | both halves seeded |
| `corpus-floor.sh` (P1) | dead corpus: the profile directory empty, a committed profile no test reaches, or a corpus nobody *replays* (parsing is not replaying) | all three claims seeded both ways |
| `json-validates.sh` (P1) | a renderer that wraps the answer in an envelope, adds a field, or hand-builds an object — checked by running the built `wch` over the fake and validating **every verb `--help` offers** (writes included, performed against the replayed fake) against `#/$defs/<type>` in the committed bundle; a verb without a validation row fails, so the population cannot silently shrink; its honest limit is in the gaps below | seeded violation per pattern; clean tree passes |
| `privileged-helper.sh` (with N8) | the blessed helper widening: any product crate depending on `crates/priv/`, or the blessed copy's mode wider than `0700` (a restore or `chmod -R` can widen it long after the bless — the mode *is* the security boundary, §2.13) | both directions |

`selftest.sh` is table-driven over the whole population; adding a predicate without cases
fails the selftest itself (the table is derived from the directory listing, not
maintained by hand).

**The phase-gate mechanism** (built at P1, now the standing shape): one row per
criterion in `scripts/gates/phase-criteria.tsv` — `phase`, `kind` (`tests` = a nextest
filterset run with `--no-tests=fail`; `command` = run from the repo root), the
selection, and what it establishes in docs/7's words. `scripts/gates/phase.sh` runs one
phase's rows and counts them (`just gate-g0` … `gate-g6`), and `counted-selections.sh`
proves every `tests` row still selects more than zero — the predecessor's defect was a
held gate whose selection had silently gone to zero. Criteria accrete row by row as
docs/7's sub-milestones land; a criterion is added by adding a line, in the same commit
as the thing it proves.

### The battery (backend conformance)

`webcam-handler-testkit::battery` — the T1/T2 conformance suite every backend runs (design §2.11):
enumeration sanity, control-model invariants (unknown types round-trip; sparse menus
preserved; out-of-range values survive), write read-back, snapshot/restore inverse,
stream lifecycle, hotplug watch lifecycle, fault-menu coverage. Arms are walked by an
exhaustive match; a skip is a declared variant with a written reason and is checked in
both directions (an arm that ran while declared skipped fails; an undeclared non-run
fails). `webcam-handler-fake` runs it from P0. `webcam-handler-v4l2`'s relationship to
the battery is stated honestly (docs/7 G1): the read arms run over the fake replaying
the committed v4l2-captured profiles — proving the model against real-device shapes —
while the v4l2 crate's own ioctl truth lives on the R2 (vivid) and R3 (hardware) rungs;
its write arms run as R3 hardware twins, evidence-recorded per docs/7 G2, never
CI-gated (recorded: notes E3, thirteen hardware tests including writes, clamps,
INACTIVE flips, snapshot/restore, streaming and photo on two devices).

## Part 2 — Gates commissioned by later phases

Per rubric rule 1, each lands in the same PR as the structure it guards; per rule 6, each
lands with both-direction cases. Struck through as they land (the repo is authoritative;
this table is the commissioning record).

| Gate | Phase | Enforced by | Catches |
|---|---|---|---|
| ~~**Profile capture/replay inverse**~~ | P1 | **landed**: `crates/backends/fake/tests/corpus_replay.rs` replays every committed profile through the battery and asserts the fake rewrites only the id and the backend field; `hw_profile_capture_reproduces_the_committed_invariant_section` closes the capture half on hardware | `profile capture` and fake replay drifting apart — the corpus quietly ceasing to resemble devices (E5) |
| ~~**vivid rung skip accounting**~~ | P1 | **landed and executed**: four `vivid_*` tests, declared by `scripts/rung-vivid.sh`'s `wch-suite` marker and checked by `ignored-suites-have-recipes.sh`; all four green on their first run (evidence E2). The rung scripts additionally count the skips *tests* report at run time, so a hardware test that declines a claim is named rather than passing quietly | the virtual-driver rung decaying into green-by-absence on every runner |
| ~~**Miri over the sys-decode units**~~ | P1 | **landed, and corrected inside P1's own review cycle**: `scripts/miri.sh` selects `sys::decode` **or** `sys::payload` — 23 units over the captured ioctl replies in `crates/backends/v4l2/fixtures/`, including the two Miri-reachable `unsafe` blocks (`Payload::bytes`/`bytes_mut`). The original decode-only selection (19 units) covered no unsafe block at all, which the P1 review caught (E1's amendments). The decoders take `&[u8]` at `offset_of!`-derived offsets so the population stays real | undefined behavior in the one crate allowed to have any |
| ~~**PF regression fixtures loaded**~~ | P1 | **landed**: `scripts/gates/corpus-floor.sh` (three claims: non-empty, every profile reachable, at least one test *replays* rather than merely parses). `corpus_replay.rs` additionally asserts each device-behavior PF finding is exhibited by a committed profile, and names the ones deliberately absent | dead corpus — a profile nobody replays |
| ~~**`--json` validates against the bundle**~~ | P1 | **landed, wider than commissioned**: `scripts/gates/json-validates.sh` runs the built `wch` over the fake backend and checks **every** verb's answer — writes included — against `#/$defs/<type>` in the committed bundle, with the verb population derived from `--help`; its honest limit (not a full JSON Schema validator) is in the gaps below | a renderer that wraps the answer in an envelope, adds a field, or hand-builds an object — `schema-artifacts-current.sh` proves the bundle matches the *types*, and nothing proved the *output* matched either |
| ~~**Guarded-set inverse**~~ | P2 | **landed**: property test + constructible-inverse fixture in the battery write arms | a manual write under live automation slipping through the planner |
| ~~**Snapshot-restore assertion**~~ | P2 | **landed**: battery arm perturb → restore → byte-compare; R3 twins asserted it on two devices (notes E3: "snapshot(15/22) → perturb → restore, every control back"), with N9's fourth outcome counted complete | restoration by assumption (rubric C smell) |
| ~~**EXIF read-back**~~ | P2 | **landed**: kamadak-exif reads what little_exif wrote, from the file on disk — the P2 review tightened the arm that asserted only on the report [S:E4]; PF:16's splice fixture rides the same suite | write-only EXIF claims |
| **Store bypass gate widened** | P3a | `atomic-write-home.sh` learns the session-dir patterns as they land, and gains the pass-direction arm over the now-real home | session writes dodging `write_json_atomic` or the lock |
| **Crash-recovery case** | P3b | kill-between-write-and-restore test in `just gate-g3`'s counted selection | a crashed sweep leaving a camera mis-set with no recovery path |
| **Vivid sweep arm** | P3c | one real sweep over a writable vivid control through the actual ioctl path, in the managed rung's counted selection | a sweep loop proven only against the fake's model of a driver |
| **Mutation floor** | P3f | a `cargo-mutants`-class job over the pure cores, `just mutants`, survivors triaged to missing tests or recorded acceptances — the "before G4, not after" schedule v1 recorded, now a docs/7 milestone | tests that execute the cores without constraining them |
| **T5 method-count walk** | P4c | the registered `RpcModule`'s `method_names()` — built from the real server, which is what the compiler enforces — compared against the integration-test inventory; derived from the running registration, never a hand list (a Rust trait does not reify its methods, so "exhaustive match" is the wrong mechanism and this row says the real one) | a wire method with no test |
| **Schema-artifacts gate widened** | P4a | `schema-artifacts-current.sh` learns the OpenRPC bundle when the T5 trait and its xtask emission land | OpenRPC drift from the wire trait |
| **Netlink hostile-bytes fixtures** | P4d | malformed, truncated, and flood packet fixtures against the uevent parser (rubric B10: a packet is attacker-shaped input); the R3 hotplug arm cycles `uvcvideo` via the blessed helper with all cameras closed (§3.3 item 9) and is evidence-recorded | a parser that trusts the kernel socket; hotplug proven only against scripted fakes |
| **CLI parity gate** | P4f | `wch` vs `wchc` byte-identical `--json` on every read verb over the fake; the parity population is derived from the T4 command core's verb list with local-only verbs (none at P4) named, never silently exempted | the T4 single-surface claim silently forking |
| **Signal parity tests** | P4e | one test per signal (SIGTERM, SIGINT), real delivery, drain asserted with open subscription + mid-flight sweep | the graceful path production triggers bypassing drain/release |
| **UDS permissions** | P4b | startup assertion + test: socket dir 0700, socket unusable by a scratch other-uid check where CI permits, else a named skip | a world-usable camera daemon socket |
| **Token enforcement** | P5a | 401-without/200-with tests; the D11 bind × token matrix enforced as written there — token-less TCP refused on every interface except via the one named loopback-only flag; non-loopback never token-less | the web listener shipping open, on any interface |
| **The R1-web Playwright rung** | P5d | a pinned Playwright + Chromium suite, subprocess-launched from a daemon integration-gate test; self-skips **counted and named** without node (node never a build dependency); versions pinned; traces on failure; asserts render-from-DTO, painting preview, WS reconnect, calibration-view subscription tracking, and token refusal in a real browser | the browser half asserted only from the API; browser regressions invisible until a human opens a tab |
| **Web-fetch gate non-vacuity** | P5a | `no-external-fetch-in-web.sh` gains the arm that fails on an empty asset directory | the web gate going green by scanning nothing |
| **MJPEG drop semantics** | P5b | stalled-reader test asserting capture counter advances independently | a slow browser tab backpressuring the capture path |
| **Compression exclusion** | P5b | test asserting the preview route's response is uncompressed with compression middleware active elsewhere | CompressionLayer swallowing multipart framing |
| **Muxer self-parse** | P6a, P6d | AVI output re-parsed by an independent reader path; size-field property test; committed byte fixtures; declared-vs-wall-clock duration bound on the R3 oracle run | a muxer that only our writer believes |
| **Oracle rung accounting** | P6d | ffprobe/mpv validation where present, counted named skip where not | oracle checks silently not running |
| **Agent-guide freshness** | P6e | the xtask-generated agent usage guide's examples smoke-checked against the built binaries; regeneration diffs clean in CI | the agent-facing doc drifting from the command surface it teaches |

## Recorded gaps and honest limits (v2)

- **The frame-content gate sniffs known image formats.** A camera frame embedded in an
  unrecognized container passes it; review carries that half (design §3.3 item 6).
- **`dependency-walls.sh` reads declared edges and source greps.** A backend leaking
  V4L2 semantics through stringly-typed values (not types) is invisible to it. And it
  asserts only the **linkage** halves of the T6 walls: the behavioral halves — pure
  crates touching no filesystem, only `v4l2` touching `/dev` and `/sys`, only the engine
  and composition roots touching the state dir — are review-held (design §2.8 says the
  same, so neither document's green overstates the other's).
- **The vivid rung proves ioctl plumbing, not device quirks** — vivid does not model
  INACTIVE-coupled menus with holes (design §3.3 item 4).
- **R3 hardware criteria are evidence-recorded, not CI-gating**: shared CI has no camera.
  The gate recipes assert the *recipes select tests*; the runs themselves land in
  the implementation notes with transcripts. This is the plan's largest honest hole and
  it is structural (design §3.3 item 1). The P1 run is entry E1 there; the P2 run is E3;
  every phase close adds its own (docs/7's sub-milestone shape).
- ~~**The R2 vivid suite is written and unrun**~~ — **retired 2026-08-08**. The privileged
  helper (note N8) loads the module, and all four `vivid_*` tests passed on their first
  execution; evidence entry E2 records the transcript and what the rung covers that the
  seed hardware cannot (77 controls against 18/24, and ten compound payload reads against
  one). The rung's own honest limit stands unchanged: it proves ioctl plumbing, not device
  quirks (design §3.3 item 4).
- **`json-validates.sh` is not a JSON Schema validator.** The offline toolchain has none,
  so it enforces the checkable core: every `required` property present, and no property
  the schema does not declare. Types, formats, nested shapes and array element schemas are
  unchecked. That catches the defect it was written for — an envelope, an extra field, a
  hand-built object — and this line is what keeps its green from being read as full
  validation.
- **Miri cannot cross an ioctl.** It covers the pure decode half of the unsafe module
  plus the two reachable `unsafe` blocks (`Payload::bytes`/`bytes_mut` — added after the
  P1 review found the original selection covered no unsafe block at all; E1's
  amendments record the correction); the ioctl calls themselves are exercised only on
  R2/R3. The split is deliberate (§2.5 shapes the code for it) and this line is what
  keeps "Miri green" from being read as "the unsafe module is verified".
- **The Playwright rung is Chromium-only and node-host-dependent** (owner ruling +
  design §3.3 item 7): Firefox/Safari are unexercised, and a CI host without node
  reduces the browser half to the protocol tests — the skip is counted, but counted is
  not run.
- **The selftest harness cannot test gates whose subject is the harness** (bootstrap
  limit); `selftest.sh` itself is covered by review plus the derived-table rule.
- **`privileged-helper.sh` checks the mode and the dependency edge, nothing more.** The
  helper's real boundary is "who has an account on this machine" (§2.13, N8); no gate
  can defend against a second session as the same user, and this line keeps the gate's
  green from being read as a security review of `crates/priv/`.
- **Mutation testing is scheduled, not landed.** docs/7 P3f commissions the job before
  G4, on v1's recorded schedule; until it lands, "the tests constrain the cores" rests
  on rubric rule 2 discipline alone.

## Coverage map — rubric gate → enforcement

| Rubric rule/principle | Enforced by |
|---|---|
| Rule 6 (gates can go red — inverse arms driven by the real tool) | `selftest.sh`, table-driven, both directions, derived population, real-tool arms where stubs stand in [S:N10] |
| Rule 3 (CI executes what it claims) | `counted-selections.sh`, `--no-tests=fail`, `ignored-suites-have-recipes.sh`, skip accounting on vivid/oracle rungs |
| A2/A3 (device authority, represent unknown) | battery control-model arms + PF corpus fixtures + the `query_controls` lint ban |
| A5 (one home) | `atomic-write-home.sh`, `dependency-walls.sh`, T5 method walk, CLI parity gate |
| A7 (leave as found) | snapshot-restore battery arm + R3 twins + crash-recovery case |
| A10 (requested ≠ applied) | battery write arms; sample-record schema fixtures |
| A12/§5 (privacy) | `no-frame-bytes-in-repo.sh`, `no-external-fetch-in-web.sh`, token + UDS gates |
| B7 (the browser half asserted in the browser) | the R1-web Playwright rung + its skip accounting |
| B9 (licenses) | `license-allowlist.sh` + `feature-posture.sh`, both self-tested |
| B10 (unsafe/ioctl boundary) [V] | `unsafe-scope.sh` + crate-root lint policy + cast lints on the backend crate + the Miri job |
| E5 (resemblance) | profile capture/replay inverse; fake-vs-probe-record assertions |
| E6 (byte fidelity) | verbatim-JPEG hash tests; muxer self-parse |
