# webcam-handler — Automated quality gates (v3)

Doc 15 in the webcam-handler series, **v3 — second revision**. Status: issued, adoption
pending (docs/12's adoption paragraph governs the set); supersedes docs/9 (v2) upon
adoption. Gates for docs/14 (rubric v3, Part D), consumed by docs/13's phase gates. The
convention in force from the first commit is unchanged: **the repository's files are
authoritative; this document records the commissioned set, deltas, and rationale.** A
drift between this document and the repo's files is a defect in whichever one the
evidence says is wrong.

This revision brings the record to the v3 baseline (`799ee73`: 36 predicates, 82 pass
arms, 368 fail arms all naming their sentence, 207 phase-criteria rows, 1532 tests) and
makes one structural repair to the document itself. **The v2 register was hand-written
prose at full argumentative density, and it drifted**: it was ten rows short at the G6
reconciliation while the predicates' own headers — where the notes' case law had been
landing all along — never were. The v3 register is therefore **one checkable claim per
predicate**, with each predicate's full rationale, claims and case inventory living where
they are already authoritative and self-tested: the script's own header and its
`cases/*.cases.sh`. The v2 register's long-form rows are preserved in docs/9 v2 under
`docs/historical/` and are cited from the notes; nothing argued there is unsaid, it is
just no longer *transcribed* here, because a transcription is the defect class half this
suite exists to catch.

## The structural rules

Carried from v2, with the third upgraded from convention to harness:

1. **Every multi-line CI predicate lives in `scripts/gates/*.sh`**, and all of them run
   under the table-driven `selftest.sh`, which requires **both directions per gate** — no
   failing case fails, only failing cases fails — over a population derived from the
   directory listing, never a hand list.
2. **Checks must not name locations they can drift from** — populations are derived
   (from `cargo metadata`, from a `find`, from an exhaustive match, from the subject's
   own source), never transcribed.
3. **The inverse arm is driven by the thing under test and names the sentence it goes
   red on.** The first half is N10's law (where a stub is unavoidable, one arm still
   runs the real tool). The second half was N31's per-case-file convention, ratcheted
   through N240 and discharged into the harness at N243: `gate_red_because` records the
   claimed sentence, the harness fails an arm that stays green *or* goes red without
   printing it, the register (`named-arm-register.txt`) is the whole population checked
   both ways, seeds are hash-verified alive (`gate_seed`, N186), both recorders are
   self-checked before the first predicate, and the checkout is proven unmutated around
   every arm. An arm red for the wrong reason reads as green about the right one — that
   is L25's lesson and rubric A16's, and the harness now holds it for every arm.

## Part 1 — The suite as built (v3 baseline)

### `just ci` (offline, ≡ CI by construction)

`fmt-check` → `lint` (clippy `--locked --workspace --all-targets -- -D warnings`) →
`test` (`cargo nextest run --locked --workspace --no-tests=fail`, retries 0) → `doc`
(`RUSTDOCFLAGS="-D warnings"`) → `deny` (`cargo deny --offline check bans licenses
sources`) → `hygiene` (typos, cargo machete, shellcheck over `scripts/` including
`scripts/gates/`) → `gates` (`run-all.sh`, then `selftest.sh`). `ci.yml` runs `just ci`
verbatim; the divergence target stays zero.

### Lint configuration

As v2 built it: one `[workspace.lints]` table (N1), suppression hygiene binding tests
too, `#![forbid(unsafe_code)]` on every crate root but the backend's (gate-checked, not
trusted), the panic/indexing set present at every shipped root and **walked by
`lint-posture.sh`** rather than hand-copied trust (N165), cast lints denied on the one
crate that reads kernel-shaped integers, `disallowed-methods` carrying the PF:1 ban and
the workspace-wide sleep ban (N3).

### The predicate register — one claim per predicate, 43 predicates

The count is the check: `run-all.sh` prints the same number every run, and `selftest.sh`
fails on a predicate with no case file. Full rationale: each script's header.

| Predicate | The claim |
|---|---|
| `agent-guide-current.sh` | the committed agent guide is what the command surface re-emits |
| `agents-md-current.sh` | root `AGENTS.md` is byte-identical to the doc whose preamble declares the deployment; `CLAUDE.md` is exactly the redirect |
| `atomic-write-home.sh` | one home for state writes, under the one lock, atomic in the home itself |
| `avi-reparse-is-independent.sh` | the muxer and its re-parser share no code, constant or helper |
| `browser-pins-sync.sh` | the three browser pins are one fact and every description matches |
| `claims-come-back-with-their-values.sh` | a claim on a camera is released by its value, never by a later line running |
| `cli-parity.sh` | one command surface: byte-identical `--json` (answers **and** refusals, with exit codes) across both roots; every uncompared verb in a named bucket with a reason |
| `corpus-floor.sh` | no dead corpus: profiles exist, are reachable, and are *replayed* |
| `counted-selections.sh` | every criteria row still selects more than zero tests |
| `dependency-registry-sync.sh` | design §2.8's registry and `[workspace.dependencies]` are one fact, reconciled name by name and pin by pin, both directions; a mark in the pin cell excuses a row from the manifest direction only, and every mark honoured is counted and named |
| `dependency-walls.sh` | the T6 linkage walls hold, from `cargo metadata` |
| `doc-comments-open-with-a-summary.sh` | no `///` block opens with a heading (the splice shape) |
| `facade-is-the-composition.sh` | the direct CLI's executor crate reaches the engine only through `engine::facade`, over a population derived from the facade's own methods and a walk over every file under `crates/cli/src`; the ban is on the class of reach and not one spelling of it — every visibility and both import keywords are read through the shared `rust-imports.awk`, joined and flattened before the walk sees them, and the two shapes that cannot reduce to a path (a binding of the crate, a glob of it) are refused with counted populations of their own; the two policy lists — the lifecycles D18 excludes and the two root-only reaches — are checked in every direction there is, the lifecycles against the executor's own doc sentence among them |
| `facade-stability-table-sync.sh` | D18's stability table is reconciled against the crates it names, both ways: every module in exactly one column, every named module still declared, every named crate a crate — every engine module the facade's imports, `impl Facade` signatures or module-scope public items name sits in the **Yes** column, with an import that yields no module refused rather than shrinking the population — and D18's own bullet in docs/12 names the table rather than restating it |
| `feature-posture.sh` | the four feature doors stay shut (`v4l` defaults; `image`'s `avif` family, banned by name; TLS; the AV1 crate family), from the resolved graph |
| `ignored-suites-have-recipes.sh` | every `#[ignore]`d suite is owned by a recipe and serialized where it must be |
| `json-validates.sh` | every verb's `--json` — answers and refusals — validates against the committed bundle; no answer wears the failure marker; and both arms of D17's SSIM reason vocabulary are produced and counted, since the bundle check cannot descend into a `$ref` |
| `kill-is-never-a-fallback.sh` | terminating a holder has one home, one caller, counted by call site |
| `license-allowlist.sh` | the permissive allowlist plus every named ban, selftested with a violation |
| `lint-posture.sh` | the panic/indexing lint set is at every shipped root, all-or-none, population from metadata |
| `luma-has-one-home.sh` | colour becomes brightness in one place: one declaration, no borrowed conversion, no second coefficient set, consumers reconciled both ways |
| `msrv-sync.sh` | the MSRV is one fact and every copy agrees |
| `mutation-scope-is-decided.sh` | every product module is in the floor's scope or excluded from it by a marker with a reason; no entry and no marker outlives the module it named |
| `mutation-verdict.sh` | the mutation floor keeps PASS / FAIL / NO-VERDICT apart, and the phase runner preserves the distinction |
| `no-external-fetch-in-web.sh` | the web client loads nothing off-origin |
| `no-frame-bytes-in-repo.sh` | no camera frames in the repository; fixtures carry provenance and bounded extents, containers walked |
| `oracle-rung-accounting.sh` | the oracle rung's four verdicts stay apart; declines reprint their reasons; line shapes derived from the product |
| `profile-partition-is-closed.sh` | the four partitions D15 closes **by destructuring** are still destructured — the mechanism the compiler enforces, which a pattern "simplified" into field access silently reopens while compiling perfectly; field names derived from each struct's own declaration |
| `privileged-helper.sh` | the blessed helper stays contained: exact caps, mode, no stray capability-carrying file, no verb that runs a caller-named program |
| `schema-artifacts-current.sh` | committed generated artifacts are what the types emit |
| `scratch-has-one-home.sh` | temporary data has one home under `target/`; the tree-copier cannot copy itself |
| `shipped-profile-is-declared.sh` | the shipped build profile is declared, checks on, carve-outs closed to `"*"` |
| `socket-activation.sh` | a real daemon under a real service manager serves the inode it inherited and journals as structured entries |
| `state-dir-permissions.sh` | the session tree is private, refused-not-repaired, driven through the shipped binary |
| `systemd-units.sh` | shipped units re-derive every value from the tree's constants |
| `testkit-is-dev-only.sh` | the testkit ships on dev edges only, and at least one exists |
| `token-comparison-has-one-home.sh` | the bearer token is compared in one place and the type refuses every operator that would short-circuit on it |
| `uapi-constants-are-declared.sh` | every kernel name asked of bindgen exists in this host's headers, or the decline names the package |
| `uds-permissions.sh` | the socket directory is private, and a daemon handed anything else refuses |
| `unsafe-scope.sh` | `unsafe` lives in one module; every other root forbids; the residual register reconciles both ways |
| `web-assets-cite-real-rust-items.sh` | every Rust item path and every repository path the shipped web client's prose cites resolves, in the assets and in the crate that serves them — a citation that names nothing sends a reader nowhere about the second copy it stands beside |
| `web-routes-are-gated.sh` | every camera-bearing route is named on the list and the list is behind the gate; only the asset fallback is open |
| `wire-surface-sync.sh` | the `wire_surface!` macro and D10's sentence reconcile, member by member, both directions |

Infrastructure (no case files, covered by review plus the derived-table rule): `lib.sh`,
`phase.sh`, `run-all.sh`, `selftest.sh`; plus `named-arm-register.txt`,
`phase-criteria.tsv`, `cases/`, `fixtures/`.

### The phase-gate mechanism

One row per criterion in `phase-criteria.tsv` (`phase`, `kind`, selection, docs/13's
words); `phase.sh` runs and counts a phase's rows (`just gate-g0` … and `gate-g7`+ as
docs/13's phases open); `counted-selections.sh` proves no selection went to zero; rows
land in the same commit as what they prove. Baseline: 207 rows, g0=9 g1=16 g2=25 g3=32
g4=42 g5=44 g6=39 — the closed phases, which do not move. `g7`, `g8` and `g9` accrete row by
row as docs/13's phases run, and how many each holds today is what `phase.sh` counts when the
gate is run; a figure for them written here would be stale by the next sub-milestone (notes
**N153**, **N158**).

### The battery, the rungs, the floor

- **The conformance battery** (`webcam-handler-testkit::battery`): eight arms —
  enumeration, control model, write read-back, snapshot/restore inverse (Drop-guarded),
  stream lifecycle, explicit request, hotplug watch, fault menu — walked by an
  exhaustive match, skips declared with reasons and checked both directions.
- **The rungs**: R1-web (pinned Playwright/Chromium; 24 claims / 206 assertions at
  baseline, manifest-counted both ways; RAN-or-SKIPPED through `rung-web`); R2
  (`rung-vivid-managed` via the blessed helper); R3 (`smoke-hw`, fail-fast off, census
  compared, motor suites default-on); the oracle rung (`rung-oracles`, four verdicts);
  Miri (`just miri`, the population provably including every reachable block).
- **The mutation floor** (`just mutants`): the files `.cargo/mutants.toml`'s `examine_globs`
  names — held to the tree in both directions by `mutation-scope-is-decided.sh` since P9,
  which is why the list is not counted here — exclusions as
  dated absences with reasons, whole-workspace judgment, register checked both ways,
  three exits (green / finding / NO VERDICT). **The v3 posture change is a re-pricing,
  not a mechanism** [N251–N255]: the register's stopped-surviving direction has fired
  four times and been wrong four times — under load it deletes true acceptances and
  waves real survivors through (the eight-job run's "0 missed" hid nine, four of them
  real) — so a moved verdict is **a prompt to apply the mutant by hand on an idle
  machine, never a finding**, `mutants.sh` says so where the number is chosen, and the
  honest jobs figure on this machine is 1 (13–19 h) with the default left at `nproc` as
  an owner decision recorded in docs/13's trigger table.

## Part 2 — Gates commissioned by the v3 phases

Per rubric rule 1, each lands in the same PR as the structure it guards; per rule 6, with
both-direction, sentence-naming cases. Struck through as they land (the repo is
authoritative; this table is the commissioning record).

| Gate | Phase | Enforces | Catches |
|---|---|---|---|
| ~~**Adoption reconciliation**~~ (landed) | P7a | the repointed `wire-surface-sync.sh` green against docs/12's D10 sentence; the two `cases/*.cases.sh` seeds repointed with it; `agents-md-current.sh` following docs/16's preamble (deploy and redirect declarations both); the historical move leaving no gate file reading a moved path | a half-adopted document set — some readers on v2, some on v3, nothing red |
| ~~**`dependency-registry-sync.sh`**~~ (landed) | P7a | design §2.8's registry table against `[workspace.dependencies]`, **both directions**: a manifest row the table lacks, a table row the manifest lacks, a version that disagrees. **Delta (2026-08-20):** the *Scope* column — the column v3 added specifically so an edge could not hide inside the crate whose dependency list most needs reviewing — was read past, and was wrong about six rows: `clap` said `cli-core` while the daemon, the privileged helper and `xtask` all declare it, `jsonrpsee` named `api` and `client` and not the daemon (the one edge N38 is about), and `camino`, `uuid`, `image` and `schemars` each named one or two of a real set of three to ten. Claim 5 now requires a row to name every workspace member with a **normal** dependency edge on the crate, over a population of the rows whose cell names a member at all — the rest a counted, named skip, since `workspace`, `roots` and `(pin, not linked)` are legitimate cells naming nobody. The converse direction is deliberately not checked and the header says why: several cells name a member the crate reaches transitively and say so in prose (note **N306**) | the N133 class — three crates adopted and never registered, a version stated wrong, and (L32) a pin with no consumer; the reconciler both those findings priced |
| ~~**Selector criteria**~~ (landed) | P7b | the parser over the closed vocabulary, both directions per spelling; corpus ambiguity (the shared-`usb_id` pair); id stability under any selector; `NodePath` re-resolving across a scripted renumbering [PF:22] | a spelling that parses to the wrong selector; a filter smuggled into enumeration; an address treated as identity |
| ~~**Derived populations absorb the new verbs**~~ (landed) | P7b–P8b | the arm that *proves the construction* per the schema-artifacts precedent: `json-validates.sh`, `cli-parity.sh` and `agent-guide-current.sh` each demonstrated red on a seeded v3-verb defect, since their populations are scraped from `--help` and the contracts table rather than named. **Delta (2026-08-20):** two of the three carried such an arm and the third did not. `agent-guide-current.sh`'s surface-drift arm seeds `value_name = "WxH"`, a P6-era flag on the stream verbs, so the guide's *v3* population — the selector table generated from `SelectorScheme::ALL` and the `<CAMERA>` help rendered from the same vocabulary — was "covered by construction" asserted rather than demonstrated, which is this row's own stated catch. `fail_case_the_selector_vocabulary_moved_and_the_guide_did_not` seeds one scheme's sample spelling and watches the committed guide go stale in the two places it reaches, two hundred lines apart (note **N303**) | "covered by construction" asserted instead of demonstrated — the sentence this suite exists to distrust |
| ~~**`document` bucket in `cli-parity.sh`**~~ (landed) | P7c | the fifth bucket with its one-implementation argument in the header; a document verb relabelled out of it fails | a document verb quietly acquiring an executor dependency (a socket, a store) while exempted from comparison |
| ~~**Projection closure**~~ (landed, as a predicate rather than a fixture) | P7c | **delta**: this row asked for a compile-fail fixture, and what landed is `profile-partition-is-closed.sh` — because the compiler *already* refuses a field nobody sided, and a `trybuild` harness would therefore be a test that the compiler works. What can go wrong without anyone noticing is the *mechanism*: a destructuring "simplified" into field access compiles perfectly and silently reopens the partition, which is what the predicate reads for, over field names derived from each struct's own declaration. The corpus mutual-negative walk landed as asked | a new invariant field silently joining neither side of the identity/device partition, **and** a pattern quietly stopping being one |
| ~~**`facade-is-the-composition.sh`**~~ (landed) | P7d | the CLI executor's only engine reach is the facade, population derived from the facade's exports; plus the one-time byte-equivalence criterion at introduction. **Delta (2026-08-20):** as commissioned it banned one spelling — the walk matched `engine::[a-z_]…`, so a grouped `use engine::{pairing, write};` yielded zero reaches and the predicate passed with a summary byte-identical to the unseeded tree (note **N269**). **Second delta, same day:** the repair itself banned one spelling of an *import* — `pub(crate) use`, `extern crate … as`, `use engine::*;` and a bypass moved into a second file of the same crate all passed, each with a byte-identical summary (note **N271**). It now reads every import through the shared `rust-imports.awk`, walks every file under `crates/cli/src`, refuses a binding of the crate and a glob of it with counted populations of their own, derives the encapsulated set from every method of `impl Facade` rather than the exported ones alone, holds each excluded lifecycle to still being reached, and reconciles the policy list against the executor's own doc sentence both ways | the facade and the CLI drifting into siblings — the FR's own upgrade-risk, inverted onto us; and, since the deltas, that same drift wearing a brace, a visibility, a glob or a second file |
| ~~**`facade-stability-table-sync.sh`**~~ (landed) | P7d | docs/14's commissioning line — "the stability table matches the exports both ways" — built in `wire-surface-sync.sh`'s shape: the table parsed out of the facade's module doc and reconciled against every crate it names, in both directions, plus the claim the review found missing (an engine module a facade **signature** forces on a caller must be in the **Yes** column). **Delta (2026-08-20, the day it landed):** it shipped with note **N269**'s hole written fresh — a grouped `use crate::{…}` hid a forbidden module *and* took the surface population from three to one with `gate_require_nonzero` satisfied by the survivor; the walk read only `pub fn`s inside `impl Facade`; and the module derivation could not see an inline `pub mod` (note **N271**). It now shares the import reader, walks the module-scope public items too, refuses an import it can take no module out of, and holds D18's own bullet to naming the table rather than restating it | the N270 class — a contract table with a hole in it, silent about seven of the engine's twenty modules and reading exactly like a complete one; the shape that hole made possible, a headline verb an embedder cannot call without holding something the table forbids; and, since the delta, the same table read through a brace |
| ~~**Stats criteria + `FrameGap` fault**~~ (landed) | P8a | the accumulator's both-direction arms; the fault's exhaustive-menu membership; the one-home reconciliation with `declared_interval`; truncation stated on the answer. **Delta (2026-08-20):** three of D16's own sentences had no criterion at all. Its two *frame* fields were asserted against one backend, in that backend's own fault suite, so nothing above the ioctl decoder would have gone red if the real driver's sequence went constant — docs/11 H1 verbatim, with the battery's `FRAMES_PER_CYCLE` constant documenting the missing assertion (note **N290**); `wall_clock_skew_us` had no assertion anywhere and `test(/skew/)` selected zero, while sitting inside the mutation floor's `examine_globs` (note **N297**); and `RecordReport::stats` carried an argumentless `#[serde(default)]`, so a document with no stats was the same document as a take that delivered nothing (note **N291**). The clause "the committed schemas carry them" also named an artifact that did not exist: `Frame` is not a wire type, and the three semantics now live on `StreamStats`' own doc, which the bundle carries. **Second delta, same day** (note **N298**): the repair over-reached in three places and under-reached in two. It made a *clock reversal under an advancing sequence* a breach of the frame contract while `StreamStats::clock_reversals` counts the same device event as a measurement — one event, two incompatible answers, and a rig whose uvcvideo timestamps go non-monotonic would have turned the R3 stream arm, the R2 vivid arm and the battery red at once; it asked the ledger for a verdict after an availability refusal, so a camera another process grabbed mid-cycle was answered with a claim about `Frame::sequence` (rule 7, the conversion N138 removed from the same file); and the criterion row claiming both-backend enforcement selected nine tests, none of which links `webcam-handler-v4l2` — `battery::run` has no real-backend caller — so the R2 rung is a counted criterion beside it now. Under-reached: `frames_delivered`'s new committed sentence says "both muxers" and only the AVI muxer had an arm, so the identical hoist in `y4m.rs` left the whole workspace green; the claim moved to `imaging::video`'s `VideoFormat::ALL` × `CapReached::ALL` walk. And the R3 rung's first ordering was a `>=` over two readings of one accumulator, which no reachable state can violate | gap accounting with no driven inverse; a second interval home; silent truncation; and, since the delta, a frame field the real backend could stop filling in silence, a skew nobody measured, and an unmeasured take wearing a measured one's document; and, since the second delta, a measurement carried as a contract, a refusal carried as an incapacity, a both-backend claim no hermetic run walks, a both-muxer claim one muxer holds, and an ordering with no false branch |
| ~~**D17 adoption measurement**~~ (landed) | P8b | the resolved-graph check recorded in the landing note; `feature-posture.sh` is the standing backstop that makes the trap impossible to re-open silently. **Delta (2026-08-20):** the backstop held one spelling of the trap — the rule was `default-off` and its body asked whether the feature literally named `default` was on, while the drag runs `default` → `default-formats` → `avif` → `dep:ravif`, so one explicit `features = ["avif"]` arrived green and `deny.toml` names no AV1 crate (note **N294**). The features are banned by name now, and the AV1 crate family has the over-broad name wall the TLS stack has, with one selftest arm per wall | `image-compare` re-enabling `image`'s defaults through feature unification — the avif→rav1e drag; and, since the delta, that same drag arriving without the word `default` anywhere near it |
| ~~**`luma-has-one-home.sh`**~~ (landed) | P8b | the crate's one RGB→luma home declared once; no product code reaching another crate's colour-to-grey conversion, banned as a family of names, call syntaxes and trait doors rather than as one spelling; no file carrying a complete set of luma coefficients it has no business with — the four standard sets against every file including the home, and the home's own triple, read out of its own body, against every file but the home; the register of consumers reconciled **both ways** | the N266 class — a comparison reader measuring JPEG in Rec. 709 and PPM in BT.601, 33 codes apart, with a scene scoring 0.9688 against itself and every test green, because the walk that covered every format fed a grey fixture |
| ~~**`hw_gone_*` decline accounting**~~ (landed) | P8c | the recipes decline by name on hosts that cannot arrange mid-stream loss, counted through the existing census machinery. **Delta (2026-08-20):** two of D19's five clauses had no recipe at all — neither committed recipe opened a hotplug watch, so the partner rig could not measure the removal from the tree it was handed, and nothing could re-attach, so the return clause had no producer on either side of the rig (note **N300**). One recipe per clause now, over three variables rather than one (`WCH_DEVICE_RETURN` joins `WCH_DEVICE_LOSS`, so a rig that can only detach declines the return by name instead of failing it, and `WCH_DEVICE_UNDER_TEST` says which camera the commands must target), and the protocol a contributed E-entry must follow is note **N299** rather than a `const` inside a test file. **Second delta, same day** (note **N301**): the return recipe threw away the one answer that would have proved the device left and then searched the fresh listing with `CameraFingerprint::matches`, which compares `bus_path` — so it could only go green on a rig that re-attached at the *same* address, i.e. the one topology D19's last sentence has nothing to say about, and a `WCH_DEVICE_LOSS` command that detached nothing produced a transcript indistinguishable from a real measurement. The loss is a claim now, before the return is attempted, and the lookup is D15's split; the loss-arranging recipes put the camera back through a `Drop` guard or say in the transcript that they did not; and the take recipe's post-loss loop is bounded, so a no-op arrangement is a red arm naming the variable rather than a nextest timeout | D19's recipes rotting into silence before the partner rig ever runs them — and, since the delta, a rig handed recipes for two of the sentences and no way to address the protocol; and, since the second delta, a recipe that measures nothing and reports it as evidence |
| ~~**`mutation-scope-is-decided.sh`**~~ (landed) | P9 | every product source file — the `src/` tree of every workspace member `cargo metadata` reports — is in `.cargo/mutants.toml`'s `examine_globs` or covered by one of its `scope-out: <path> — <reason>` markers; no `examine_globs` entry outlives the module it named; no marker outlives the module it excluded; nothing is named by both lists; every marker carries a reason. Fourteen arms, including a new module under a crate the register names file by file (red) and one under a crate it blankets (green), and a new *workspace member* seeded into the graph rather than into `Cargo.toml`, because a member is a lockfile change. The fourteenth is the population's other quiet exit and holds both directions in one arm: a member whose `src/` is not there contributes no files at all, so it leaves the derived population — the shipped tree must print no such skip and the seeded graph must print one naming the member (AGENTS rule 3) | note **N162**'s residual, which that file predicted and which came true three times: `imaging/stream_stats.rs`, `imaging/compare.rs` and `engine/facade.rs` landed across P8 in neither list, so the floor silently stopped covering the tree and no arm anywhere could go red on it (note **N302**) |
| ~~**The stop's four-wait table**~~ (landed as a delta to `systemd-units.sh`) | P9 | `TimeoutStopSec` compared against the whole of what the *process* takes to stop — `2 x DAEMON_SHUTDOWN_DRAIN_MS + WEB_LISTENER_STOP_MS`, both terms derived from `limits.rs` — rather than against the drain alone, with the summary line printing the sum and its terms | a drain raised from 20 s to 21.5 s passing the gate while the four waits sum to exactly the 45 s the unit allows, so systemd's SIGKILL lands on the ordered teardown the pair exists to protect — measured, not reasoned, and the gate printed "the daemon's own bound is what fires" while it happened (note **N304**) |
| ~~**`/session-photo` route partition**~~ (landed) | P9b | both halves in the same commit as the route: `web-routes-are-gated.sh` arms for the third list entry, and `every_camera_bearing_route_is_behind_the_gate` driving anonymous-401 / token-200 / cross-site-403 / out-of-session-404 / HEAD-opens-nothing | the N82 defect class's first live exercise: a camera-bearing route added without its gate |
| ~~**Workbench claims**~~ (landed) | P9a–P9c | the R1-web additions: the layout claim at the pinned viewport over the vivid profile; live-tuning round trips; the human flow end to end with `selector: human` asserted through a second socket; the sweep-time pane swap; refusal rendering and recovery. **Delta (2026-08-20):** the sweep-time pane was split off when the flow landed and the deferral was not recorded, so this row was struck through over a pane no line of code built (note **N277**); the flow's own claim stopped at the grid click, leaving Apply and Restore — both rendering fields no version of the wire has carried — unclicked (note **N273**); the narrow-viewport claim asserted a box position true in *both* layouts, so it could not go red on the stacking its title names (note **N262**); and the refusal arm asserted the D13 discriminant and not the message. All four landed together with the fences and the field the flow was missing | the browser half of D20 asserted only through the JSON the page consumes — and, since the delta, a claim that walks a flow and stops before its last two steps |
| ~~**`/session-photo`'s consumer**~~ (landed) | P9c | the third camera-bearing route reconciled with the page that is its only consumer: `the_urls_the_page_builds_are_the_routes_this_daemon_serves` compares the nine wire names `credential.js` declares with `daemon::http`'s own constants, both directions, and holds the routes the page builds and the routes on `CAMERA_BEARING_PATHS` to being one set — read out of the embedded bytes a browser is served; `web-assets-cite-real-rust-items.sh` resolves the Rust paths the client's prose cites; and the browser claims assert the grid's photographs decode and that the same reference without a token does not | note **N275**'s class — a wire name copied into a page with nothing reconciling it, and a doc comment naming a module (`daemon::http::samples`) that has never existed, both green in every suite this workspace runs |
| ~~**The write-during-suspend claim**~~ (landed) | P9a | D20's live-tuning sentence driven at last: a `wch_set` from a second connection while a photo holds the camera comes back with `{requested, applied}`, the device keeps it, and the pair costs one descriptor, one suspension and three streams | a design sentence with no driven twin, on the path AGENTS rule 7 is about — and specifically a build that answers `Busy` to whatever arrives inside a suspend, which is what this daemon did to photos during previews until 2026-08-12 |

## Recorded gaps and honest limits (v3)

Carried, regenerated, and extended — each line exists so a green run is not read as more
than it is:

- **The frame-content gate sniffs known formats and two containers**; a frame in an
  unrecognized envelope passes; review carries that half.
- **`token-comparison-has-one-home.sh` checks shape, and shape is not timing** — a
  short-circuit written *inside* `verify` passes all its claims; the argument beside the
  code and the diff reader carry the residual.
- **`luma-has-one-home.sh` cannot see a coefficient set nobody standardised** — a fourth
  luma written with freshly-derived weights passes its coefficient claim; what closes
  the residual is its consumer register asking why a new file converts at all, plus the
  colour arm in `imaging::compare`'s suite that holds every format to a committed table.
- **`web-routes-are-gated.sh` sees registrations, not wrapping** — its behavioral twin
  drives the listed paths; neither half is worth having alone.
- **`avi-reparse-is-independent.sh` sees imports and layout, and neither is derivation**
  — a retyped constant passes; the module doc and the writing order carry the residual.
- **`json-validates.sh` is not a JSON Schema validator** — required-present and
  nothing-undeclared only, direction-named since N247.
- **Miri cannot cross an ioctl**; the ioctls are R2/R3's.
- **R3 is evidence-recorded, never CI-gating**; the recipes are gated, the runs land as
  E-entries.
- **Counted is not run**: the UDS other-uid arm, the socket-activation arms and the
  Playwright rung each decline by name on hosts missing their preconditions, and a
  counted skip is still a thing that did not run.
- **The gates walk the filesystem, not the repository** [N97]: agent worktrees under the
  checkout turn predicates red on copies correct where they live; run gates with no
  worktrees present; the per-predicate `git ls-files` fix stays named and untaken (some
  predicates must walk the filesystem — frames, scratch).
- **Predicate branches with no failing arm** [N244] — recorded per branch, retiring
  branch by branch, and amended as new predicates land; plus the three `uds-permissions`
  branches needing a second account and non-interactive privilege this host declines. The
  register of named arms is complete: `selftest.sh` prints how many fail arms name their
  sentence, out of how many there are, on every run, which is the reconciled form — a
  number written here would be a prose count of code that nothing checks (notes **N153**,
  **N158**). Armless *branches* are the open residue, and the two claims are deliberately
  not conflated.
- **The mutation floor's verdict is a function of the machine, four entries deep**
  (N52 time, N66 space, N68 moving input, N251 load-vs-real-clocks) — the floor is a G4+
  criterion and a dev tool, never a `just ci` step, its absence a named counted skip, its
  moved verdicts prompts (Part 1). Retires when the daemon/client suites take an owned
  clock, which is the named repair.
- **The selftest harness cannot test gates whose subject is the harness** (bootstrap);
  review plus the derived-table rule cover it.
- **The workbench layout claim is one viewport** — the pinned one, over the widest
  committed profile, **and one more**: the narrow-viewport twin asserts the stacked shell
  at 700×800 and at no other width. Layout truth between and beyond those two is Chrome's
  continuity plus a manual glance; the claims' honesty is their named viewports, not an
  implied "all".
- **The write-during-suspend claim asserts the queue's effect and not its order** — "lands
  *after* resume" needs an observable this daemon does not publish, and note **N274**
  carries the two measurements that ruled out the two candidates. It retires when
  `CameraActivity` carries the `busy` flag `engine::actor` already keeps.
- **`web-assets-cite-real-rust-items.sh` resolves addresses, not aptness** — a page citing
  the *wrong* real constant passes, and a crate-less shorthand is counted and named rather
  than guessed at: `limits::…` and `render::…`, and a type-shaped head like
  `RestoreReport::is_complete`, which is crate-less by the same rule. What holds the values
  is the pair of reconcilers in the daemon's web-client suite. It checks **two** spellings
  of one class — a Rust path and a repository path — because the batch that landed the
  first shipped the second beside it, in the same comment (note **N284**).
- **`mutation-scope-is-decided.sh` cannot see a blanket whose reason has stopped
  describing the modules under it.** A directory marker satisfies the predicate by
  existing, and its reason cell is prose the predicate reads past on purpose (one home
  for the argument, design §2.10) — so `scope-out: crates/schema/ — derives and data`
  decides the whole of `crates/schema/src` — `selector.rs`, D14's one parser, and
  `profile.rs`, D15's identity/device partition and `DeviceProfile::compare`, among them —
  on a sentence that was true of that crate before either of those landed. The *engine*
  homes of both laws are in the **owed** block; the schema homes are blanketed out. What is red today is a module in
  neither list; what is not is a decision that has gone stale, and closing that needs the
  owner's ruling on whether the two files are owed or the reason is rewritten (note
  **N302**).
- **D19's contract is locally undriveable beyond the fake** by design (§2.13's
  interlock); the recipes exist to decline, until a rig that can arrange the event runs
  them.

## Coverage map — rubric rule → enforcement

| Rubric rule/principle | Enforced by |
|---|---|
| Rule 6 (gates go red, for the reason they claim) | `selftest.sh` — derived population, both directions, sentence-per-arm, live seeds, self-checked recorders |
| Rule 2 (a test that cannot go red is not a test) | the selftest's inverse arms; `just mutants` for the cores; the seeded-defect campaign each sub-milestone records |
| Rule 3 (CI executes what it claims) | `counted-selections.sh`, `--no-tests=fail`, `ignored-suites-have-recipes.sh`, the rung accountings |
| Rule 7 (populations behind rows) | the `closed_vocabulary!` walks, derived selections, and Part 2's population-proving arms |
| Rule 8 (the repair is reviewed) | process, per docs/13's conventions — deliberately not a gate; the gate-shaped half is that a repair's gate lands in the repair's PR (rule 1) |
| A16 (refusals for the right reason) | the harness's sentence-per-arm law; the separating-credential tests in Rust suites |
| A17 (a class, not a spelling) | derived bans (the escape sweep at the T4 door); widened patterns landing with the instance; `rust-imports.awk` — the one import reader both facade predicates include, with the crate-binding and glob refusals beside it (notes **N269**, **N271**) |
| A2/A3 (device authority, represent unknown) | battery arms + PF corpus + the `query_controls` ban + `uapi-constants-are-declared.sh` |
| A5 (one home) | `atomic-write-home.sh`, `dependency-walls.sh`, the T5 walk, `cli-parity.sh`, `facade-is-the-composition.sh`, `facade-stability-table-sync.sh`, `dependency-registry-sync.sh` |
| A7 (leave as found) | battery + R3 twins + crash-recovery + the Drop-guard arms |
| A12/§5 (privacy) | `no-frame-bytes-in-repo.sh`, `state-dir-permissions.sh`, the route partition (every entry on `daemon::http::CAMERA_BEARING_PATHS`, `/session-photo` included since P9b), `no-external-fetch-in-web.sh`, token + UDS gates |
| B7 (the browser half in the browser) | the R1-web rung + its manifest counts + skip accounting |
| B9 (licenses) | `license-allowlist.sh` + `feature-posture.sh` |
| B10 (unsafe boundary) | `unsafe-scope.sh` + lint policy + cast lints + Miri + the SIGSEGV-inverse clamp test |
| E5 (resemblance) | capture/replay inverses; fake-vs-probe assertions; measured-pair corpus; fault-menu walks |
| E6 (byte fidelity) | verbatim-JPEG hashes; muxer self-parse through the independent reader |
