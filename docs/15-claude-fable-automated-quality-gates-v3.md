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

### The predicate register — one claim per predicate, 40 predicates

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
| `facade-is-the-composition.sh` | the direct CLI's executor reaches the engine only through `engine::facade`, over a population derived from the facade's own exports; the two policy lists — the lifecycles D18 excludes and the two root-only reaches — are checked both ways |
| `feature-posture.sh` | the three feature doors stay shut (`v4l` defaults, `image` defaults, TLS), from the resolved graph |
| `ignored-suites-have-recipes.sh` | every `#[ignore]`d suite is owned by a recipe and serialized where it must be |
| `json-validates.sh` | every verb's `--json` — answers and refusals — validates against the committed bundle; no answer wears the failure marker |
| `kill-is-never-a-fallback.sh` | terminating a holder has one home, one caller, counted by call site |
| `license-allowlist.sh` | the permissive allowlist plus every named ban, selftested with a violation |
| `lint-posture.sh` | the panic/indexing lint set is at every shipped root, all-or-none, population from metadata |
| `luma-has-one-home.sh` | colour becomes brightness in one place: one declaration, no borrowed conversion, no second coefficient set, consumers reconciled both ways |
| `msrv-sync.sh` | the MSRV is one fact and every copy agrees |
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
g4=42 g5=44 g6=39.

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
- **The mutation floor** (`just mutants`): 21 files in `examine_globs`, exclusions as
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
| ~~**`dependency-registry-sync.sh`**~~ (landed) | P7a | design §2.8's registry table against `[workspace.dependencies]`, **both directions**: a manifest row the table lacks, a table row the manifest lacks, a version that disagrees | the N133 class — three crates adopted and never registered, a version stated wrong, and (L32) a pin with no consumer; the reconciler both those findings priced |
| ~~**Selector criteria**~~ (landed) | P7b | the parser over the closed vocabulary, both directions per spelling; corpus ambiguity (the shared-`usb_id` pair); id stability under any selector; `NodePath` re-resolving across a scripted renumbering [PF:22] | a spelling that parses to the wrong selector; a filter smuggled into enumeration; an address treated as identity |
| ~~**Derived populations absorb the new verbs**~~ (landed) | P7b–P8b | the arm that *proves the construction* per the schema-artifacts precedent: `json-validates.sh`, `cli-parity.sh` and `agent-guide-current.sh` each demonstrated red on a seeded v3-verb defect, since their populations are scraped from `--help` and the contracts table rather than named | "covered by construction" asserted instead of demonstrated — the sentence this suite exists to distrust |
| ~~**`document` bucket in `cli-parity.sh`**~~ (landed) | P7c | the fifth bucket with its one-implementation argument in the header; a document verb relabelled out of it fails | a document verb quietly acquiring an executor dependency (a socket, a store) while exempted from comparison |
| ~~**Projection closure**~~ (landed, as a predicate rather than a fixture) | P7c | **delta**: this row asked for a compile-fail fixture, and what landed is `profile-partition-is-closed.sh` — because the compiler *already* refuses a field nobody sided, and a `trybuild` harness would therefore be a test that the compiler works. What can go wrong without anyone noticing is the *mechanism*: a destructuring "simplified" into field access compiles perfectly and silently reopens the partition, which is what the predicate reads for, over field names derived from each struct's own declaration. The corpus mutual-negative walk landed as asked | a new invariant field silently joining neither side of the identity/device partition, **and** a pattern quietly stopping being one |
| ~~**`facade-is-the-composition.sh`**~~ (landed) | P7d | the CLI executor's only engine reach is the facade, population derived from the facade's exports; plus the one-time byte-equivalence criterion at introduction | the facade and the CLI drifting into siblings — the FR's own upgrade-risk, inverted onto us |
| ~~**Stats criteria + `FrameGap` fault**~~ (landed) | P8a | the accumulator's both-direction arms; the fault's exhaustive-menu membership; the one-home reconciliation with `declared_interval`; truncation stated on the answer | gap accounting with no driven inverse; a second interval home; silent truncation |
| ~~**D17 adoption measurement**~~ (landed) | P8b | the resolved-graph check recorded in the landing note; `feature-posture.sh` is the standing backstop that makes the trap impossible to re-open silently | `image-compare` re-enabling `image`'s defaults through feature unification — the avif→rav1e drag |
| ~~**`luma-has-one-home.sh`**~~ (landed) | P8b | the crate's one RGB→luma home declared once; no product code reaching another crate's colour-to-grey conversion, banned as a family of names, call syntaxes and trait doors rather than as one spelling; no file carrying a complete set of luma coefficients it has no business with — the four standard sets against every file including the home, and the home's own triple, read out of its own body, against every file but the home; the register of consumers reconciled **both ways** | the N266 class — a comparison reader measuring JPEG in Rec. 709 and PPM in BT.601, 33 codes apart, with a scene scoring 0.9688 against itself and every test green, because the walk that covered every format fed a grey fixture |
| ~~**`hw_gone_*` decline accounting**~~ (landed) | P8c | the recipes decline by name on hosts that cannot arrange mid-stream loss, counted through the existing census machinery | D19's recipes rotting into silence before the partner rig ever runs them |
| ~~**`/session-photo` route partition**~~ (landed) | P9b | both halves in the same commit as the route: `web-routes-are-gated.sh` arms for the third list entry, and `every_camera_bearing_route_is_behind_the_gate` driving anonymous-401 / token-200 / cross-site-403 / out-of-session-404 / HEAD-opens-nothing | the N82 defect class's first live exercise: a camera-bearing route added without its gate |
| ~~**Workbench claims**~~ (landed) | P9a–P9c | the R1-web additions: the layout claim at the pinned viewport over the vivid profile; live-tuning round trips; the human flow end to end with `selector: human` asserted through a second socket; the sweep-time pane swap; refusal rendering and recovery | the browser half of D20 asserted only through the JSON the page consumes |

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
- **Eight predicate branches have no failing arm** [N244] — recorded per branch, retiring
  branch by branch; plus the three `uds-permissions` branches needing a second account
  and non-interactive privilege this host declines. The register of named arms is
  complete (368/368); armless *branches* are the open residue, and the two claims are
  deliberately not conflated.
- **The mutation floor's verdict is a function of the machine, four entries deep**
  (N52 time, N66 space, N68 moving input, N251 load-vs-real-clocks) — the floor is a G4+
  criterion and a dev tool, never a `just ci` step, its absence a named counted skip, its
  moved verdicts prompts (Part 1). Retires when the daemon/client suites take an owned
  clock, which is the named repair.
- **The selftest harness cannot test gates whose subject is the harness** (bootstrap);
  review plus the derived-table rule cover it.
- **The workbench layout claim is one viewport** — the pinned one, over the widest
  committed profile. Layout truth at other sizes is Chrome's continuity plus a manual
  glance; the claim's honesty is its named viewport, not an implied "all".
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
| A17 (a class, not a spelling) | derived bans (the escape sweep at the T4 door); widened patterns landing with the instance |
| A2/A3 (device authority, represent unknown) | battery arms + PF corpus + the `query_controls` ban + `uapi-constants-are-declared.sh` |
| A5 (one home) | `atomic-write-home.sh`, `dependency-walls.sh`, the T5 walk, `cli-parity.sh`, `facade-is-the-composition.sh`, `dependency-registry-sync.sh` |
| A7 (leave as found) | battery + R3 twins + crash-recovery + the Drop-guard arms |
| A12/§5 (privacy) | `no-frame-bytes-in-repo.sh`, `state-dir-permissions.sh`, the route partition (two routes today, the third at P9b), `no-external-fetch-in-web.sh`, token + UDS gates |
| B7 (the browser half in the browser) | the R1-web rung + its manifest counts + skip accounting |
| B9 (licenses) | `license-allowlist.sh` + `feature-posture.sh` |
| B10 (unsafe boundary) | `unsafe-scope.sh` + lint policy + cast lints + Miri + the SIGSEGV-inverse clamp test |
| E5 (resemblance) | capture/replay inverses; fake-vs-probe assertions; measured-pair corpus; fault-menu walks |
| E6 (byte fidelity) | verbatim-JPEG hashes; muxer self-parse through the independent reader |
