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
| `dependency-walls.sh` | T6 violations from `cargo metadata`, populations derived from §2.8's edge list: tokio/axum/hyper linked by `schema`/`imaging`/`fake`/`cli-core`/`engine` (`engine` joined at P4b — §2.1 puts the camera actor there and §2.8 gives the engine no runtime, which is the whole reason `engine::actor` names no reply channel of its own [S:N41]; membership means linkage and nothing else, so the engine still owns the state directory); **`api`'s own wall — no axum, no hyper, no tower-http, tokio allowed** (deviation N5: one `#[rpc(server, client)]` trait links tokio necessarily; re-run N5's measurement on any jsonrpsee bump — and since P4b that wall is live rather than hypothetical, because the daemon's `jsonrpsee-server` edge puts hyper in the graph [S:N38]); backends linked by anything but the two composition roots (`webcam-handler-cli`, `webcam-handler-daemon`); `client` linking a backend or the engine (the thin-client wall); V4L2 types escaping the backend (grep for `v4l::`/`v4l_sys::` outside `crates/backends/v4l2/`) | a scratch edge in each direction |
| `uds-permissions.sh` (P4b) | a world-usable camera daemon socket: it starts a real `wchd` against scratch XDG directories, learns from the daemon's own stderr that it is serving, and asserts the directory it served from is `0700` and owned by the user who ran it, that something was actually bound there, that a second uid cannot walk in — **where the host offers one, else a named counted skip**, and only after the *parent* is proven reachable, because `mktemp -d` is itself `0700` and would otherwise answer for the daemon [S:N44] — that a `wchd` handed a `0755` directory refuses to serve and leaves the mode alone, and that a `wchd` handed a **symlink** where its socket directory goes refuses however private the target is — because `stat` and `create_dir_all` both follow links, so a by-path check passes while the daemon serves from wherever the name currently points [S:N39] | `$WCH_GATE_WCHD` points the predicate at daemon-shaped programs that get D11 wrong one way each (umask decided the mode; announced a socket it never bound; repaired a widened directory instead of refusing; checked the mode by path so a symlink answered for its target; never started); `pass_case` drives the shipped `wchd` (rubric rule 6 [S:N10]) |
| `unsafe-scope.sh` | the token `unsafe` outside `crates/backends/v4l2/src/sys/` (rubric B10 [V]); the allowed path is derived from the tree, and the other crates' `#![forbid(unsafe_code)]` roots are asserted present | a seeded unsafe block elsewhere must fail; a missing `forbid` root must fail; the sys module must pass |
| `kill-is-never-a-fallback.sh` (P4c) | a second way to signal a camera's holder: the signal has one home (`webcam-handler-v4l2::holders::terminate`, forwarding to the one `unsafe` block in `sys/signal.rs`) and exactly one caller outside the backend crate — the daemon's `terminate_holder`. AGENTS' "never a fallback" is an **absence**, and an integration suite can only witness the verbs it drives, so a `Busy` retry added to `wch_photo` would leave every test green [S:N48] | a seeded second caller in the engine and one in the daemon must fail; a tree where nothing calls it must fail (the verb would not signal); a home that stopped defining `terminate` or lost `sys/signal.rs` must fail; a comment that names the signal without calling it must pass |
| `testkit-is-dev-only.sh` | `webcam-handler-testkit` on a normal edge (it may grow tooling deps that must never ship) | non-vacuity arm: fails if nothing dev-depends on it |
| `atomic-write-home.sh` | `serde_json::to_writer`/`fs::write` targeting the state dir *or D9's session files* outside `webcam-handler-engine::store` (rubric A5: bypassing the home); and, since P3a, the home itself failing to show D9's atomic sequence or the one `fd-lock` | a scratch bypass must fail — including one that names only `log.ndjson` and never `state_dir`, which is what the P3a widening bought; a home with no `write_json_atomic`, no rename, or no lock must fail; a missing home must fail; an empty population must fail (no more named skip); the shipped tree passes, and so must a home split across `src/store/` |
| `no-frame-bytes-in-repo.sh` | committed camera frames: content-sniffs images in the tree; allows only `corpus/images/` whose fixtures carry a `generated-by` provenance marker and dimensions below the fixture cap | a scratch JPEG without provenance must fail; a provenanced synthetic must pass |
| `no-external-fetch-in-web.sh` | CDN/script-src URLs, `fetch(` to non-relative origins, and `<script src` with a scheme in `webcam-handler-web` assets | one seeded violation per pattern; the P5 widening row adds the non-vacuity arm (an empty asset directory fails, so the gate cannot go green by scanning nothing) |
| `schema-artifacts-current.sh` | committed generated artifacts drifting from what emits them (re-runs xtask emit, diffs) — the JSON Schema bundle against the `webcam-handler-schema` types, and, since P4a, the OpenRPC document against the T5 trait. The predicate names no filename: it walks the emitted tree and the committed directory, so a new artifact joins the comparison by being written. An **empty** artifact directory is a failure, not a named skip: the "nothing is committed yet" branch stopped being reachable when the bundle landed at P2, and everything that reaches it now (a bad merge, a dropped rebase, a `.gitignore` edit) is the drift this gate exists to catch | a scratch type edit must fail; a hand-edited or uncommitted artifact must fail; a wire method renamed in `crates/api` without regenerating must fail; a `schemas/` with nothing in it must fail; clean tree passes |
| `counted-selections.sh` | gate recipes whose test filters select zero (selections compare `(package, binary)` pairs; the `grep -c`-under-`pipefail` trap — a zero count exits nonzero — is known and avoided by construction throughout `scripts/`). **Corrected once already [S:N10]**: `cargo nextest list -T json` lists the whole workspace whatever the filter says, so the count is `filter-match.status == "matches"`, never `test-count` — the original predicate could not count to zero and was green by construction from the day it shipped | a recipe naming a nonexistent test must fail; the selftest includes a **real-tool arm** (the actual nextest over a filter matching nothing), because the stubbed arm is what hid the defect |
| `msrv-sync.sh` | the one MSRV fact diverging across `Cargo.toml`s and CI | seeded divergence |
| `ignored-suites-have-recipes.sh` | every `#[ignore]`d suite named by a `just` recipe, every recipe selecting a real `(package, binary)`, and every declared `wch-suite:` prefix present in the `exclusive-device` nextest group (so a rung cannot be added or renamed without the serialization following it). Its test-group half reads only the group's own overrides and whole-prefix matches, both directions — the P2 review found it matching prefixes anywhere in a filter, including subtracted ones [S:E4] | both halves seeded |
| `corpus-floor.sh` (P1) | dead corpus: the profile directory empty, a committed profile no test reaches, or a corpus nobody *replays* (parsing is not replaying) | all three claims seeded both ways |
| `json-validates.sh` (P1) | a renderer that wraps the answer in an envelope, adds a field, or hand-builds an object — checked by running the built `wch` over the fake and validating **every verb `--help` offers, subcommands included** (writes included, performed against the replayed fake) against `#/$defs/<type>` in the committed bundle; the population is scraped at both levels — `wch --help`, then `wch <verb> --help` for each — and a row must match a leaf verb's name exactly, so the population cannot silently shrink; its honest limit is in the gaps below | seeded violation per pattern; clean tree passes |
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
| ~~**Store bypass gate widened**~~ | P3a | **landed**: `atomic-write-home.sh` now has two halves — the bypass scan, whose population learned D9's session-file names (`session.json`, `log.ndjson`, `sessions/`, the `schema::limits` constants) and is `gate_require_nonzero` rather than a named skip, and a new arm over the **home itself**, asserting six D9 properties of `engine::store`: `write_json_atomic` exists, the temp file is made in the destination directory, it is synced before the rename, the rename is what publishes, the parent is fsynced, and the one lock is an `fd_lock::RwLock`. Nine selftest arms, two of them green-direction. Its first run over the real home caught a false positive worth keeping: naming the home in a doc comment is deference, not a reach (`engine::photo` says why a photo is *not* written that way) | session writes dodging `write_json_atomic` or the lock — and, now, a home that quietly stopped being atomic or stopped taking the lock |
| ~~**Crash-recovery case**~~ | P3b | **landed**: `crates/engine/tests/crash_recovery.rs` — the parent spawns this test binary as a child, which takes the lock, performs a sweep write, announces itself on a pipe and blocks; the parent `SIGKILL`s it and asserts `status.signal() == Some(9)`, because a test that would still pass on a clean exit is not testing a crash. A fresh store then finds the lock free, resumes by fingerprint and task, and restores from the persisted snapshot. Selected by `just gate-g3` as `binary(crash_recovery)` | a crashed sweep leaving a camera mis-set with no recovery path |
| ~~**Vivid sweep arm**~~ | P3c | **landed and executed**: `vivid_a_calibration_sweep_sets_settles_captures_and_scores_through_the_real_ioctl_path` plans three step-aligned values from the driver's own range, writes them guarded, settles, captures, scores and records — probing the automation pairs first and restoring through the persisted pre-sweep snapshot afterwards, with the swept control's return asserted. Eight `vivid_*` tests green on `just rung-vivid-managed`, 0 skips. It also produced PF:17: `vivid`'s `u8_pixel_array` reshapes with the negotiated format, so the arm asserts the *control it swept* came back rather than that the whole restore report is complete | a sweep loop proven only against the fake's model of a driver |
| ~~**R3 calibration and motion arms**~~ | P3e | **landed and executed**: `hw_a_calibration_session_sweeps_a_brightness_control_selects_applies_and_restores` runs start → plan → sweep → select → apply → restore against every attached camera that offers a brightness-class control, and `hw_motion_a_bounded_ptz_sweep_returns_the_motor_to_where_it_started` is the one arm in the workspace that drives a motor — bounded to a few steps either side of home, asserting the planner's motion cap on the device's own range and the head's return at the device. `just smoke-hw` runs 15 tests green; `WCH_NO_MOTION=1` drops it to 14 with the exclusion as a named, counted skip. Evidence E5, and it produced PF:18 and N21 | calibration proven only against the fake's model of a camera; a motor nobody ever drove |
| ~~**Two gate populations widened at the P3 review**~~ | P3 review | **landed**: `json-validates.sh`'s verb population now recurses one level — for each top-level verb it scrapes `wch <verb> --help` and requires a row named `<verb>-<sub>` for every leaf, with an *exact* row match instead of the prefix match that let one `calibrate-start` row answer for the whole seven-verb subtree; a selftest arm deletes one `calibrate-*` row from a copy of the predicate and asserts it goes red. `atomic-write-home.sh`'s raw-write pattern gained `File::options(` and `File::create_new(` — std's own aliases for `OpenOptions::new()` and `File::create()` — with a fail arm for each: two byte-identical bypasses used to get opposite verdicts on how the open was spelled | the third and fourth instance of note N10's family — a gate green while checking less than it claims. Before the fix six of the seven verbs P3d landed could vanish from the gate with it still green |
| ~~**Mutation floor**~~ | P3f | **landed and run**: `just mutants` → `scripts/mutants.sh` over `cargo-mutants`, scoped by `.cargo/mutants.toml` to the six pure cores (`engine::{pairing, session, settle, store, sweep}`, `imaging::metrics`) and judged by the **whole workspace suite** per AGENTS rule 2. Survivors are compared against `scripts/mutants-accepted.txt` **in both directions** — an unlisted survivor fails the job, and so does a listed one that has stopped surviving, because an acceptance nobody re-checks is how N15's mistake gets made twice. The scope's exclusions are stated as absence from `examine_globs`, each with its reason, so widening the floor is adding a line — and P4a added three, `crates/api/src/{codes,photo,wire}.rs`, after the P4a review found two survivors there by hand that the floor could not see. First-run numbers, cost and triage: evidence E7; the widened run (478 mutants, 409 caught, 11 survivors all accepted, register clean both ways): evidence E8; posture and cadence in the gaps below. **The test timeout is pinned** (`minimum_test_timeout = 180.0`) since P4c, because the autoset floor of 20s made the verdict a function of how many jobs the machine ran: the same tree answered FAIL with 31 unaccepted survivors at 8 jobs and PASS with the register clean at 4, and all 31 were timeouts on healthy mutants. Note N52 carries the measurement and the reason it is a floor rather than a multiplier | tests that execute the cores without constraining them |
| ~~**T5 method-count walk**~~ | P4c | **landed**: `crates/daemon/tests/method_surface.rs`'s `every_method_the_daemon_registers_is_exercised_over_the_fake`, with a `g4` row of its own. Both sides are derived and neither is a list. The **registered** side is `method_names()` off the very `Methods` value the fixture serves — built by the generated `into_rpc()` over a real `Wchd`, which is the *daemon's* registration and not `api::METHODS` (note N28 draws that line). The **exercised** side is recorded at the transport by a `Recording` wrapper around the shared `Wire`: it writes down the method name the generated client handed it, on the way past, refusals included — so the spelling comes from `#[method(name = …)]`'s expansion on both sides of the comparison. Driven once per transport, and it is deliberately **one** test, because nextest runs each test in its own process and nineteen separate ones would each record a single name. Measured red both ways it can go: a twentieth method added to the trait and implemented but not driven fails the comparison naming it, and a daemon whose registration is missing a method the suite drives fails earlier, at the call, with `-32601`. Its four honest limits are in the register below | a wire method with no test |
| ~~**Schema-artifacts gate widened**~~ | P4a | **landed, and the script needed no logic change** — which is the interesting part. Its two loops name no filename (emitted-and-not-committed, emitted-and-different, committed-with-no-generator), so `schemas/webcam-handler-openrpc.json` joined the comparison by being written. "Covered by construction" is the kind of sentence this suite exists to distrust, so the coverage is spent in `cases/`: four arms for the OpenRPC document specifically — stale, emitted-but-not-committed, orphan, and the one this row actually commissions, a wire name edited in `crates/api/src/lib.rs` with nothing under `schemas/` touched and the **real** emitter left to find it (rubric rule 6, N10). The arms now assert the failure *message*, not just the status, because several seeds are red under more than one branch and the harness reads only the status (note N31); the Rust-editing arm builds into a directory of its own, because a mutated build sharing the checkout's `target/` poisons it (note N32). The P4a review then found the branch none of them reached — an empty `schemas/` answered `PASS … 1 named skip`, so deleting every committed artifact was green — and that skip is now `gate_require_nonzero` with an arm of its own. The document itself is emitted from `api::METHODS` and `api::rpc_code`, which are generated from the trait declaration rather than transcribed beside it (note N28) | OpenRPC drift from the wire trait |
| **Netlink hostile-bytes fixtures** | P4d | malformed, truncated, and flood packet fixtures against the uevent parser (rubric B10: a packet is attacker-shaped input); the R3 hotplug arm cycles `uvcvideo` via the blessed helper with all cameras closed (§3.3 item 9) and is evidence-recorded | a parser that trusts the kernel socket; hotplug proven only against scripted fakes |
| **CLI parity gate** | P4f | `wch` vs `wchc` byte-identical `--json` on every read verb over the fake; the parity population is derived from the T4 command core's verb list with local-only verbs (none at P4) named, never silently exempted | the T4 single-surface claim silently forking |
| **Signal parity tests** | P4e | one test per signal (SIGTERM, SIGINT), real delivery, drain asserted with open subscription + mid-flight sweep | the graceful path production triggers bypassing drain/release |
| ~~**UDS permissions**~~ | P4b | **landed, in two places and deliberately so**: the startup assertion is `daemon::uds::SocketDir::prepare`, which creates `0700`, *reads the mode back* and refuses — never repairs — anything else, with unit tests in both directions including one that asserts a rejected `0770` directory is unchanged afterwards [S:N39]. The other-uid half is `scripts/gates/uds-permissions.sh`, because nextest has no runtime-skip concept and `just ci` runs without `--success-output final`, so a declining `#[test]` is a skip that reads as a pass — the shell predicate's `gate_skip` is named and counted [S:N44]. Writing it also found the check's own vacuity trap: `mktemp -d` is `0700`, so a second uid is stopped by the *scratch* directory and the assertion never reaches its subject; the predicate widens everything above the socket directory and makes the parent's reachability a checked precondition | a world-usable camera daemon socket |
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
  asserts only the **linkage** halves of the T6 walls: the behavioral halves — the runtime-free
  crates other than `engine` touching no filesystem, only `v4l2` touching `/dev` and `/sys`,
  only the engine and composition roots touching the state dir — are review-held (design §2.8
  says the same, so neither document's green overstates the other's). Membership of the
  no-runtime list means one thing only, and `engine`'s presence on it since P4b is about
  tokio and nothing else: the state directory is its job.
- **The T5 method-count walk counts calls, not depth, and is one suite's inventory.** Four
  limits, and the row is only worth what their honesty is worth. (1) A method reached by one
  call with one shallow assertion satisfies it. What pushes back is structural rather than a
  proof: the walk's answer record has one field per method and every field is read by an
  assertion, and an unread field is `dead_code`, which this workspace compiles as an error —
  so an answer nobody looked at does not build. Depth belongs to the sibling suites
  (`read_verbs`, `mutating_verbs`, `calibrate_verbs`, each with its own row) and to review.
  (2) `cargo-nextest` gives every test its own process, so the walk is one test and sees what
  that test drove; a verb covered thoroughly in some other binary and not there still stops
  the count, which is the intended direction, but this is not a workspace-wide coverage
  claim. (3) It cannot see "registered but still refusing" — a build where every method
  answered `Unimplemented` would pass it, because they were called and they answered — so it
  is only meaningful paired with `daemon::server`'s
  `the_pinned_routing_is_the_whole_wire_surface_and_nothing_answers_unimplemented` (note
  N43) and with the walk's own per-answer assertions. (4) It cannot catch a rename: the
  client and the server are two expansions of *one* declaration, so a changed
  `#[method(name = …)]` moves both sides of the comparison together. `crates/api`'s
  `the_trait_registers_the_nineteen_wch_methods_and_nothing_else` is what holds that, and it
  is why that pin is a hand-written list on purpose.
- **A hang is bounded by nextest, not by a gate.** `.config/nextest.toml` gives the default
  profile `slow-timeout = { period = "60s", terminate-after = 3 }` (and the hardware group
  five minutes × four, because a bounded motion sweep legitimately takes minutes), so a test
  that stops finishing becomes a named `TIMEOUT` rather than a `just ci` that never returns.
  It is a deadline that turns a hang into a failure, not synchronisation —
  `uds-permissions.sh` makes the same argument for its own `timeout 60` — and it exists
  because the daemon suites bound themselves on the *other end* speaking: a child process's
  stderr, a camera actor's reply channel. A logging change that removed the line a fixture
  waits for used to turn `just ci` from red into never-finishing, which is the failure nobody
  debugs because it produces no message.
- **The UDS other-uid arm is a counted skip on a host with one account.** It needs a second
  account *and* a non-interactive `sudo`, neither of which is a property of the code; where
  either is missing, `uds-permissions.sh` reports a named, counted skip and the `0700`
  assertion is what carries the run. Counted is still not run — the same sentence this
  register already carries for the Playwright rung — and the mode assertion is the half that
  runs everywhere [S:N44].
- **The vivid rung proves ioctl plumbing, not device quirks** — vivid does not model
  INACTIVE-coupled menus with holes (design §3.3 item 4).
- **R3 hardware criteria are evidence-recorded, not CI-gating**: shared CI has no camera.
  The gate recipes assert the *recipes select tests*; the runs themselves land in
  the implementation notes with transcripts. This is the plan's largest honest hole and
  it is structural (design §3.3 item 1). The P1 run is entry E1 there; the P2 run is E3;
  the P3 run is E5; every phase close adds its own (docs/7's sub-milestone shape).
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
- ~~**Mutation testing is scheduled, not landed**~~ — **retired 2026-08-09**. `just mutants`
  is the job docs/7 P3f commissioned; its first run and the triage of every survivor are
  evidence entry E7. What the retirement leaves behind is a posture, a cadence and three
  limits, and they belong here rather than in a commit message:
  - **Posture: not a `just ci` step, and a G4 criterion.** The job rebuilds the workspace
    and runs all 643 tests once per mutant: 410 mutants took **21m17s** of wall clock on
    five parallel jobs (8-core machine, build
    trees in a `tmpfs` `$TMPDIR` — on disk the same run is an order of magnitude slower,
    measured). `just ci` costs minutes and must stay that way, so the floor is a rung. It
    is not left to memory either: `phase-criteria.tsv` carries a `g4` row running
    `./scripts/mutants.sh`, which is docs/7's "before G4, not after" made re-runnable.
    **Cadence: before a phase gate closes, and after any change to the six files in
    scope.** cargo-mutants is a dev tool — `just ci` never requires it, and the recipe
    reports a named, counted skip on a machine without it.
  - **The floor is six files, and the imperative shell is not in it.** `.cargo/mutants.toml`
    names what is out and why: the unsafe V4L2 edge (only decidable against a device — R2,
    R3 and Miri own that half), the CLI renderers (survivors there say "the golden output is
    not byte-asserted", which is a decision docs/6 already made), and `engine`'s shell
    modules — `lifecycle`, `calibrate`, `discover`, `capture`, `photo`, `snapshot`, `write`.
    The last exclusion is the load-bearing one, and it is a deferral rather than a
    judgement: E6's largest P3 finding was in `lifecycle`, and the seeded-defect campaigns
    of P3b/P3c/P3d ran against those same shell modules, so **this job did not reproduce
    their counts and does not claim to** — the populations barely intersect. The one that
    does intersect is P3a's, against `engine::store`, and E7 records what the tool found
    there against what P3a's commit message claimed.
  - **A recorded acceptance is a claim about the fault that can be injected, not about a
    class of property** [S:N15]. `scripts/mutants-accepted.txt` is checked in both
    directions for exactly that reason: a mutant that becomes killable turns the job red
    until its entry is deleted and its test written.

## Coverage map — rubric gate → enforcement

| Rubric rule/principle | Enforced by |
|---|---|
| Rule 6 (gates can go red — inverse arms driven by the real tool) | `selftest.sh`, table-driven, both directions, derived population, real-tool arms where stubs stand in [S:N10] |
| Rule 2 (a test that cannot go red is not a test) | `selftest.sh`'s inverse arms for the gates; for the pure cores, `just mutants` — a machine that writes the buggy implementation 410 ways and reports the lines nothing noticed [S:E7]; elsewhere the seeded-defect campaign each sub-milestone records in its commit |
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
