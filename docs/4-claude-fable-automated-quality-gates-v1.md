# webcam-handler — Automated quality gates (v1)

Doc 4 in the webcam-handler series, **v1 — initial revision**. Gates for docs/3 (rubric
v1, Part D), consumed by docs/2's phase gates. Convention inherited from the predecessor
and in force from the first commit: **once the repository exists, its files are
authoritative; this document records the commissioned set, deltas, and rationale.** A
drift between this document and the repo's files is a defect in whichever one the
evidence says is wrong.

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
from an exhaustive match), never transcribed lists.

## Part 1 — The P0 baseline suite

Lands with the workspace scaffold; G0 requires all of it green and self-tested.

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
  `std::thread::sleep` in test code (rubric C: sleeps as synchronization).
- `disallowed-types`: none at P0; grows with evidence.
- On device/request-driven paths (enforced by module-scoped lint levels):
  `unwrap_used`, `expect_used`, `panic`, `indexing_slicing`, `as_conversions`.

**Crate-root lint policy** (every crate root, from the P0 scaffold; rubric B10/B11 [V]):

```rust
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks, clippy::missing_safety_doc,
        clippy::multiple_unsafe_ops_per_block)]   // one obligation per SAFETY
#![cfg_attr(not(test), deny(clippy::allow_attributes,
                            clippy::allow_attributes_without_reason))]
```

plus `#![forbid(unsafe_code)]` on **every crate except `webcam-handler-v4l2`**, whose
crate doc says why it drops it and whose `unsafe` is confined to `src/sys/` by
`unsafe-scope.sh` below. `webcam-handler-v4l2` additionally denies
`clippy::cast_possible_truncation`, `cast_sign_loss`, `cast_possible_wrap` — the
`try_from`-not-`as` rule as a lint on the one crate that reads kernel-shaped integers.

### `scripts/gates/` — the P0 predicate population

| Predicate | Catches | Both-direction cases |
|---|---|---|
| `license-allowlist.sh` (drives `cargo deny`) | any copyleft/off-list license entering the tree; each named ban (`v4l-sys`, `v4l2-sys`, `udev`/`libudev-sys`/`tokio-udev`, `libcamera*`, `alsa*`/`cpal`, `colored`, `minimp4`, `env-libvpx-sys`/`vpx-encode`/`webm`, `ffmpeg-next`/`-sys`, `x264`/`x265`, `dssim`, `jpeg-encoder`/`turbojpeg`/`mozjpeg`) | a scratch manifest **with a committed lockfile** carrying a banned dep must fail under `cargo deny --offline` (the selftest never resolves the network — that is how the failing arm stays inside an offline `just ci`); the shipped tree must pass |
| `feature-posture.sh` | `v4l` with non-default features (the `libv4l` LGPL door); `image` with default features (the `avif`→rav1e drag); any TLS feature (CDLA cert stores); checked from `cargo metadata`, not manifest grep | a scratch manifest flipping each posture must fail |
| `dependency-walls.sh` | T6 violations from `cargo metadata`, populations derived from §2.8's edge list: tokio/axum/hyper linked by `schema`/`imaging`/`fake`/`api`/`cli-core`; backends linked by anything but the two composition roots (`webcam-handler-cli`, `webcam-handler-daemon`); `client` linking a backend or the engine (the thin-client wall); V4L2 types escaping the backend (grep for `v4l::`/`v4l_sys::` outside `crates/backends/v4l2/`) | a scratch edge in each direction |
| `unsafe-scope.sh` | the token `unsafe` outside `crates/backends/v4l2/src/sys/` (rubric B10 [V]); the allowed path is derived from the tree, and the other crates' `#![forbid(unsafe_code)]` roots are asserted present | a seeded unsafe block elsewhere must fail; a missing `forbid` root must fail; the sys module must pass |
| `testkit-is-dev-only.sh` | `webcam-handler-testkit` on a normal edge (it may grow tooling deps that must never ship) | non-vacuity arm: fails if nothing dev-depends on it |
| `atomic-write-home.sh` | `serde_json::to_writer`/`fs::write` targeting the state dir outside `webcam-handler-engine::store` (rubric A5: bypassing the home) | a scratch bypass must fail; a clean tree passes (P0's pass arm — the P3 widening row adds "the real home exists and passes", so this predicate is never vacuously green about a home that does not yet exist) |
| `no-frame-bytes-in-repo.sh` | committed camera frames: content-sniffs images in the tree; allows only `corpus/images/` whose fixtures carry a `generated-by` provenance marker and dimensions below the fixture cap | a scratch JPEG without provenance must fail; a provenanced synthetic must pass |
| `no-external-fetch-in-web.sh` | CDN/script-src URLs, `fetch(` to non-relative origins, and `<script src` with a scheme in `webcam-handler-web` assets | one seeded violation per pattern; the P5 widening row adds the non-vacuity arm (an empty asset directory fails, so the gate cannot go green by scanning nothing) |
| `schema-artifacts-current.sh` | committed JSON Schema bundles drifting from the `webcam-handler-schema` types (re-runs xtask emit, diffs); the OpenRPC half joins at P4 with the trait it documents (Part 2) | a scratch type edit must fail; clean tree passes |
| `counted-selections.sh` | gate recipes whose test filters select zero (`grep -c` exits nonzero on zero under `pipefail` — the trap is known; selections compare `(package, binary)` pairs) | a recipe naming a nonexistent test must fail |
| `msrv-sync.sh` | the one MSRV fact diverging across `Cargo.toml`s and CI | seeded divergence |
| `ignored-suites-have-recipes.sh` | every `#[ignore]`d suite named by a `just` recipe and every recipe selecting a real `(package, binary)` | both halves seeded |

`selftest.sh` is table-driven over the whole population; adding a predicate without cases
fails the selftest itself (the table is derived from the directory listing, not
maintained by hand).

### The battery (backend conformance)

`webcam-handler-testkit::battery` — the T1/T2 conformance suite every backend runs (design §2.11):
enumeration sanity, control-model invariants (unknown types round-trip; sparse menus
preserved; out-of-range values survive), write read-back, snapshot/restore inverse,
stream lifecycle, hotplug watch lifecycle, fault-menu coverage. Arms are walked by an
exhaustive match; a skip is a declared variant with a written reason and is checked in
both directions (an arm that ran while declared skipped fails; an undeclared non-run
fails). `webcam-handler-fake` runs it from P0. `webcam-handler-v4l2`'s relationship to
the battery is stated honestly (docs/2 G1): the read arms run over the fake replaying
the committed v4l2-captured profiles — proving the model against real-device shapes —
while the v4l2 crate's own ioctl truth lives on the R2 (vivid) and R3 (hardware) rungs;
its write arms run as R3 hardware twins, evidence-recorded per docs/2 G2, never
CI-gated.

## Part 2 — Gates commissioned by later phases

Per rubric rule 1, each lands in the same PR as the structure it guards; per rule 6, each
lands with both-direction cases. Struck through as they land (the repo is authoritative;
this table is the commissioning record).

| Gate | Phase | Enforced by | Catches |
|---|---|---|---|
| ~~**Profile capture/replay inverse**~~ | P1 | **landed**: `crates/backends/fake/tests/corpus_replay.rs` replays every committed profile through the battery and asserts the fake rewrites only the id and the backend field; `hw_profile_capture_reproduces_the_committed_invariant_section` closes the capture half on hardware | `profile capture` and fake replay drifting apart — the corpus quietly ceasing to resemble devices (E5) |
| ~~**vivid rung skip accounting**~~ | P1 | **landed and executed**: four `vivid_*` tests, declared by `scripts/rung-vivid.sh`'s `wch-suite` marker and checked by `ignored-suites-have-recipes.sh`; all four green on their first run (evidence E2). The rung scripts additionally count the skips *tests* report at run time, so a hardware test that declines a claim is named rather than passing quietly | the virtual-driver rung decaying into green-by-absence on every runner |
| ~~**Miri over the sys-decode units**~~ | P1 | **landed**: `scripts/miri.sh` selects `sys::decode`, 19 units green over the captured ioctl replies in `crates/backends/v4l2/fixtures/`. The decoders take `&[u8]` at `offset_of!`-derived offsets precisely so the population is real | undefined behavior in the one crate allowed to have any |
| ~~**PF regression fixtures loaded**~~ | P1 | **landed**: `scripts/gates/corpus-floor.sh` (three claims: non-empty, every profile reachable, at least one test *replays* rather than merely parses). `corpus_replay.rs` additionally asserts each device-behavior PF finding is exhibited by a committed profile, and names the ones deliberately absent | dead corpus — a profile nobody replays |
| **`--json` validates against the bundle** | P1 (uncommissioned) | `scripts/gates/json-validates.sh`: runs the built `wch` over the fake backend and checks each read verb's answer against `#/$defs/<type>` in the committed bundle | a renderer that wraps the answer in an envelope, adds a field, or hand-builds an object — `schema-artifacts-current.sh` proves the bundle matches the *types*, and nothing proved the *output* matched either |
| **Guarded-set inverse** | P2 | property test + constructible-inverse fixture in the battery write arms | a manual write under live automation slipping through the planner |
| **Snapshot-restore assertion** | P2 | battery arm: perturb → restore → byte-compare control state; R3 twin asserts on hardware | restoration by assumption (rubric C smell) |
| **EXIF read-back** | P2 | independent-reader test (kamadak-exif reads what little_exif wrote — a gate-commissioned oracle gets its §2.8 dependency entry at commissioning time, as this one has) | write-only EXIF claims |
| **Store bypass gate widened** | P3 | `atomic-write-home.sh` learns the session-dir patterns as they land, and gains the pass-direction arm over the now-real home | session writes dodging `write_json_atomic` or the lock |
| **Crash-recovery case** | P3 | kill-between-write-and-restore test in `just gate-g3`'s counted selection | a crashed sweep leaving a camera mis-set with no recovery path |
| **T5 method-count walk** | P4 | the registered `RpcModule`'s `method_names()` — built from the real server, which is what the compiler enforces — compared against the integration-test inventory; derived from the running registration, never a hand list (a Rust trait does not reify its methods, so "exhaustive match" is the wrong mechanism and this row says the real one) | a wire method with no test |
| **Schema-artifacts gate widened** | P4 | `schema-artifacts-current.sh` learns the OpenRPC bundle when the T5 trait and its xtask emission land | OpenRPC drift from the wire trait |
| **CLI parity gate** | P4 | `wch` vs `wchc` byte-identical `--json` on every read verb over the fake; the parity population is derived from the T4 command core's verb list with local-only verbs (none at P4) named, never silently exempted | the T4 single-surface claim silently forking |
| **Signal parity tests** | P4 | one test per signal (SIGTERM, SIGINT), real delivery, drain asserted with open subscription + mid-flight sweep | the graceful path production triggers bypassing drain/release |
| **UDS permissions** | P4 | startup assertion + test: socket dir 0700, socket unusable by a scratch other-uid check where CI permits, else a named skip | a world-usable camera daemon socket |
| **Token enforcement** | P5 | 401-without/200-with tests; the D11 bind × token matrix enforced as written there — token-less TCP refused on every interface except via the one named loopback-only flag; non-loopback never token-less | the web listener shipping open, on any interface |
| **The R1-web Playwright rung** | P5 | a pinned Playwright + Chromium suite, subprocess-launched from a daemon integration-gate test; self-skips **counted and named** without node (node never a build dependency); versions pinned; traces on failure; asserts render-from-DTO, painting preview, WS reconnect, calibration-view subscription tracking, and token refusal in a real browser | the browser half asserted only from the API; browser regressions invisible until a human opens a tab |
| **Web-fetch gate non-vacuity** | P5 | `no-external-fetch-in-web.sh` gains the arm that fails on an empty asset directory | the web gate going green by scanning nothing |
| **MJPEG drop semantics** | P5 | stalled-reader test asserting capture counter advances independently | a slow browser tab backpressuring the capture path |
| **Compression exclusion** | P5 | test asserting the preview route's response is uncompressed with compression middleware active elsewhere | CompressionLayer swallowing multipart framing |
| **Muxer self-parse** | P6 | AVI output re-parsed by an independent reader path; size-field property test; committed byte fixtures; declared-vs-wall-clock duration bound on the R3 oracle run | a muxer that only our writer believes |
| **Oracle rung accounting** | P6 | ffprobe/mpv validation where present, counted named skip where not | oracle checks silently not running |
| **Agent-guide freshness** | P6 | the xtask-generated agent usage guide's examples smoke-checked against the built binaries; regeneration diffs clean in CI | the agent-facing doc drifting from the command surface it teaches |

## Recorded gaps and honest limits (v1)

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
  The gate-g1/g2 recipes assert the *recipes select tests*; the runs themselves land in
  the implementation notes with transcripts. This is the plan's largest honest hole and
  it is structural (design §3.3 item 1). The P1 run is recorded as entry E1 there.
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
- **Miri cannot cross an ioctl.** It covers the pure decode half of the unsafe module;
  the ioctl calls themselves are exercised only on R2/R3. The split is deliberate (§2.5
  shapes the code for it) and this line is what keeps "Miri green" from being read as
  "the unsafe module is verified".
- **The Playwright rung is Chromium-only and node-host-dependent** (owner ruling +
  design §3.3 item 7): Firefox/Safari are unexercised, and a CI host without node
  reduces the browser half to the protocol tests — the skip is counted, but counted is
  not run.
- **The selftest harness cannot test gates whose subject is the harness** (bootstrap
  limit); `selftest.sh` itself is covered by review plus the derived-table rule.
- **No mutation testing at v1.** Commissioned for the first reconciliation (rubric meta-
  rule) once there is a test population worth hunting; the predecessor's evidence says
  schedule it before G4, not after.

## Coverage map — rubric gate → enforcement

| Rubric rule/principle | Enforced by |
|---|---|
| Rule 6 (gates can go red) | `selftest.sh`, table-driven, both directions, derived population |
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
