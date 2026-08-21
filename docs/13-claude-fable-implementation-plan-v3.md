# webcam-handler — Implementation Plan (v3)

Doc 13 in the webcam-handler series, **v3 — second revision**. Status: **adopted**, at P7a
(commit `796babb`, 2026-08-18); supersedes docs/7 (v2), which now lives under
`docs/historical/` with its P0–P6 closure ledger
intact — that ledger is closed history and this document carries it **by reference, not by
copy**: the phases, closing commits, criteria counts, evidence entries and review records
for P0–P6 are docs/7's and stay there. Consumes the design (docs/12); gate criteria are
enforced by the gate suite (docs/15) and the review bar by the rubric (docs/14). Section
references §n.m and D-numbers point into docs/12 unless prefixed.

**What changed from v2, and why.** v2's shape — session-sized sub-milestones, criteria
accreting row by row, the phase review in its own session — survived contact with four
phases and a whole-tree review and is kept without modification. What v2 could not
know is what its own execution taught, and three of those lessons now bind the *planning*
rather than the sessions:

- **Size by story, not by subsystem** [N54]: P4d was two sub-milestones wearing one name,
  and a falling false-positive rate on a large diff is a saturation signal, not quality.
  Every sub-milestone below is one story.
- **Count the terminal rungs at planning time** [N54]: each sub-milestone below names the
  rungs that must end RAN (or counted-SKIPPED) before it commits, so the cost is priced
  when the work is cut, not discovered at the boundary.
- **The repair loop is part of the estimate** (rubric Part E; the G6 measurement):
  every implementation batch gets an independent adversarial read before it commits, and
  two rounds is the norm — three of eleven G6 repair commits were green with regressions
  no test asked about. A sub-milestone's session budget includes its reader.

## The P0–P6 record

Closed. `docs/historical/7-claude-fable-implementation-plan-v2.md` holds the ledger:
seven phases (P0–P6), gates `g0`–`g6` (207 criteria rows at the v3 baseline), evidence
entries E1–E18, six adversarial reviews (five phase-scoped, P1–P5; and G6 over the whole
tree — `docs/11`), and the notes' case law through N255. The v3 baseline tree is `799ee73`:
`just ci` green at 1532 tests, 36 gate predicates, 82 pass arms and 368 fail arms all
naming their sentence, R1-web at 24 claims/206 assertions, five committed profiles (one
carrying measured pairs), and the mutation floor's post-review triple run triaged
[N251–N255].

## Standing conventions, in force from P0 — v3 restatement

Carried, with the v3 additions marked:

- **`docs/implementation-notes.md` is case law.** N-entries the day a thing is learned;
  PF-entries for hardware behavior; E-entries append-only. Reviews do not re-report an
  entry; empirical disproof retires one.
- **A fix or feature lands with its gate, in the same PR** (rubric rule 1). The
  commissioning record is docs/15 Part 2.
- **Every criterion is a row** in `scripts/gates/phase-criteria.tsv`; `just gate-gN` runs
  and counts a phase's rows; `counted-selections.sh` proves no selection went to zero;
  rows land in the same commit as the thing they prove.
- **Milestones are session-sized**; a sub-milestone that turns out to be two splits,
  recorded, rather than stretching.
- **The phase review is its own session** (docs/14 Part E), and **every implementation
  batch gets an independent adversarial reader before it commits** *(v3 — promoted from
  G6 practice to convention)*: the reader is read-only and parallel-safe, gets the
  author's claims and the instruction that green CI is not evidence, and the batch lands
  as one commit after the reader's findings are repaired.
- **Corpus discipline** (§3.2): tool-captured, provenance, immutable, wholesale
  replacement; new device behavior lands as corpus + a note the day it is seen.
- **Hardware needs**: P7 wants the attached cameras for selector twins; P8 wants them for
  the D16/D19 recording arms; P9 needs no camera (fake + browser). All `hw_`/`vivid_`
  suites serialize in the one-thread `exclusive-device` group; motor suites run by
  default with `WCH_NO_MOTION=1` as the counted opt-out (owner, 2026-08-08).
- **Doc-comment edits move committed artifacts** — the schema bundle, the OpenRPC
  document, `docs/agent-guide.md` — so `just generate` rides any surface change
  (`schema-artifacts-current.sh` and `agent-guide-current.sh` are the backstops).
- **No rename in passing** *(v3 — N90/N91/N126's lesson as a convention)*: a name sweep
  is always its own sub-milestone with the orphaned-artifact checklist, and the wire
  namespace is a wire break no sweep may touch.

## Execution record (live, reconciled at each gate close)

Written here as it happens, docs/7's ledger discipline applied to this document. **The
sub-milestone boundaries below are coarser than the plan's in one place and it is recorded
rather than smoothed over**: P7c, P7d, P8a, P8b and P8c's hermetic half landed in one commit,
because their generated artifacts are one JSON bundle and one guide between them and a
boundary that split them would have been red on `schema-artifacts-current.sh` at every commit
but the last. The sizing lesson stands and this is its cost, paid once and stated.

| Sub-milestone | Landed | What the tree gained |
|---|---|---|
| P7a | `796babb` | the v3 set is the set of record; `dependency-registry-sync.sh`; the `toml` pin's disposition (N256); the first five `g7` rows |
| P7b | `561c91c` | `schema::selector`, the widened resolver, both roots and the wire; the R3 twins run once at three cameras (**E19**) |
| P7c, P7d, P8a, P8b, P8c | `7dd0c3e` | the projection and `profile compare`; `engine::facade`; the stream-stats accumulator and `Fault::FrameGap`; `imaging::compare` with `image-compare` adopted (N260); D19's hermetic contract and the declining `hw_gone_*` recipes |
| P9a, P9b | `b3aa781` | the two-pane workbench shell over the 77-control fixture; `/session-photo` and both halves of its route partition |
| P9c (flow) | `a907975` | the human-driven flow, `selector: human` produced at last, and the preview-release wait N264 records — **the sweep-time pane was split off here under P9c's own sizing licence, and the deferral was not written down**, which left three sentences in the tree describing a pane no line of code built (note **N277**) |
| P7d, P8b (the command-line halves) | `1ddf472`…`b12dd9d` | the block this table had not recorded, added here rather than left out because the ledger says it is live: the direct CLI rebuilt on the facade so the two cannot drift (`37e1633`); `photo diff` as the second document verb, its dimension-mismatch reason surviving all the way to a person (`ccb74f7`); D18's two `g7` rows and docs/15's *delta* for P7c's projection criterion, which landed as a predicate rather than as the commissioned compile-fail fixture (`d30a293`); the preview's own refusal sentence reaching the page (**N265**); the R3 rung re-run against three attached cameras after four things moved underneath it (**E20**); and the three repairs an adversarial reading of that batch returned, with the rest handed to the next session in writing |
| P8b (repair) | `a71748a` | one home for RGB→luma: `compare::read` measured JPEG and PNG in Rec. 709 and Netpbm in a third spelling of BT.601, so a scene scored 0.9688 against itself and `photo diff` disagreed with the calibration reported for the same photograph (note **N266**), with `luma-has-one-home.sh` holding the *class*; and a photograph read at the orientation its own EXIF declares (note **N267**) |
| P7d (repair) | `962dff6` | one home for reading a Rust import, `scripts/gates/rust-imports.awk`, after a grouped `use engine::{…}` took the facade gate's population to zero and printed a summary byte-identical to the unseeded tree — the one-home law defeated by a brace, in five other spellings besides (notes **N269**, **N271**); `facade-stability-table-sync.sh` beside it, and the stability table made true of the crates it names in both directions |
| P9c (sweep view) | `e9b1633` | the pane swap D20 asks for: `#sweep-view` taking the preview's slot for the length of a sweep and giving it back **on the sweep's own terminal event** rather than on the call's answer (N278), with progress and the freshest sample off the one `wch_subscribe_calibration` this page opens; plus what P9c's own claims had stopped short of — the two verbs nothing had clicked (N273), the M32 fence the flow was missing over its read *and its assignment* (N154, N156, N280), `/session-photo`'s consumer reconciled with the daemon (N275), and D20's criteria field. The batch's own adversarial reading landed with it: a non-re-entrant sweep (N279), a `busy` no longer interned as an incapacity (N285), a grid a refusal no longer empties (N281), a wait that says what its predicate proves (N282), driven arms for the two guards nothing could redden (N283), and the citation predicate widened to the class it names (N284) |
| P7c (repair) | `ac0a38a` | the format-tree distinction published as `DeviceVerdict` rather than as two Rust methods — D15's type exists for the `--json` subprocess consumer, and that consumer was being told to rebuild a conjunction N89 forbids (note **N286**) — computed on read and never cached; the three D15 claims nothing could go red on, armed — a section list, a rendered caveat and a conditional's false branch, which note **N287** enumerates because they are three different places, not three sections; and PF:17's status restated where T3 claimed the element count absorbed |
| P8a (repair) | `bcd2826` | the frame contract asked of **both** backends: `FrameLedger` as its one home with three callers pushing into it, so the claim rides whatever backend is in front of it (note **N290**); `wall_clock_skew_us` asserted for the first time (**N297**); `RecordReport.stats` required rather than `#[serde(default)]`-ed (**N291**); and the reading of that batch that found a measurement carried as a contract and a refusal carried as an incapacity (**N298**). The R3 stream arm ran at three attached cameras and the numbers are evidence entry **E21** |
| P8c (repair) | `8c78cf0` | the fake losing a camera the way a machine does — the removal announced per node the camera owned, `FakeBackend::device_returns` putting it back at an address its caller names, and every door into a vanished camera refusing rather than only the listing (notes **N299**, **N300**); one `hw_gone_*` recipe per D19 clause over three variables, red on an arrangement that detaches nothing (**N301**), and the protocol a contributed E-entry must follow |
| P9 (the floor's scope) | `556e4d2` | `mutation-scope-is-decided.sh`: every product source file of every workspace member `cargo metadata` reports is in `.cargo/mutants.toml`'s `examine_globs` or carries a dated `scope-out:` marker with a reason, both directions — three P8 modules had landed in neither list and nothing could go red on it (note **N302**); D14's selector vocabulary given one home in the generated guide (**N303**, **N308**); `calibrate list`'s camera positional armed; and `systemd-units.sh` comparing the whole sum the stop takes rather than one term of it (**N304**) |
| P7–P9 (the documents) | *this batch* | the documents reconciled against the tree the eight batches above moved. The notes' "Expected usage" preamble gains the third consumer and the identity/description trade-off — the statement docs/12 §1 and `AGENTS.md` both end by pointing at, and which was not there to point at; **N262**'s stated measurement conditions are corrected in place and its "after" figures re-measured at the viewport the rung pins, over the fixture the claim is made on; and every live count in this document, docs/12, docs/15, `AGENTS.md`, `README.md` and `phase-criteria.tsv` is measured and restated, anchored to the commit it was true at, or replaced by the thing that reconciles it |

Counts at `556e4d2`, the last commit in the table with a hash, every one of them read out of
that commit's own tree and re-measured for this line rather than copied from another document:
**43 gate predicates** (`scripts/gates/*.sh` less the four harness files `gate_predicates`
excludes), **502 fail arms across 43 case files** (`selftest.sh` prints the pair on every run
and is the authority; these are what the files hold), 24 `g7` rows, 17 `g8` rows, 9 `g9` rows,
and the browser rung at **37 claims and 387 assertions**, which are `browser/claims.json`'s own
two numbers and `web_browser.rs`'s floor. The docs-only batch below `556e4d2` adds no
predicate, no case file, no criteria row and no test, so the same six figures described the tree
this sentence was committed into.

**The P7e/P8d/P9d gate-close review's repairs move four of the six** (2026-08-21), and the four
are re-measured here rather than reasoned about from the diff. The browser rung is **44 claims and
461 assertions** — still `browser/claims.json`'s own two numbers and `web_browser.rs`'s floor,
raised in the same diff as the seven claims that landed the G9 lens's findings (notes
**N310**–**N314**). The suite is **514 fail arms across the same 43 case files**, twelve of them
the red-on-inverse arms for the five gate repairs the review returned (notes
**N315**–**N318** and **N323**). And
the criteria table is **25 `g7` rows, 22 `g8` rows, 12 `g9` rows**: g8 and g9 each gain the
`run-all.sh` + `selftest.sh` pair that g0 through g7 open with and those two blocks opened without,
g8 gains the `smoke-hw.sh` row P8c's own **Proves** bullet was owed and the two reconcilers whose
committed artifacts P8 moved, g9 gains the artifact reconciler for the same reason, and g7 names
the `cli-parity.sh` its P7d bullet delegates a claim to in as many words (note **N318**). The
predicate count is the one figure of the six this batch leaves alone: **43**, because every repair
was to a predicate that already existed.

**The commit is named rather than called "the last of those", and naming it corrected one number
and removed another** (2026-08-20). A sentence whose antecedent is "the last row of the table
above" becomes
false the moment a row is added, which is what happened; and the predicate figure read `39` from
the day it was written, when the tree at that commit and at the two before it held 38 — nobody
could check it, because the sentence named nothing to check it against (**N153**, **N158**). The
test total is not restated at all: nothing here can reconcile it without building that commit,
and a number in this document that only a build can confirm is the same claim in a smaller
disguise. Reconciling the live counts is P9d's, docs/7's ledger discipline applied to this
document; what belongs here as it happens is the boundary — and the eight batches from
`1ddf472` to `556e4d2` inclusive are why the figures above moved by five predicates, twelve
`g7` rows, ten `g8` rows and four `g9` rows without a single gate closing. The count of
batches and the deltas are one claim, not two: `a907975` holds 38 predicates, 12 `g7` rows, 7
`g8` rows and 5 `g9` rows, `556e4d2` holds 43, 24, 17 and 9, and dropping the last row of the
table from the population changes three of the four figures — so "the batches between the two
commits", which excludes the second, was the wrong population for these numbers.

## P7 — Adoption and the consumer contracts

The smallest coherent story: the v3 documents take effect, and the sibling ledger's two
highest-value requests (selection, comparison) plus the facade land — everything a
library consumer needs before its own bring-up starts. Gate `g7`.

**Where the criteria are, 2026-08-20.** The **Proves** bullets below were commitments when this
plan was adopted; what enforces them is `scripts/gates/phase-criteria.tsv`'s `g7` block, which
accreted row by row with the work that earned each one — the convention above — and whose size
at `556e4d2` the execution record states. Several arrived only in the repair batches — the three
D15 claims nothing could go red on (a section list, a rendered caveat and a conditional's
false branch, note **N287**) and D18's facade criteria among them — so a bullet here and a
row there are the same claim at two ages, and the tsv is the one that runs. **`g7` is not
closed**: P7e is the close and it has not happened.

### P7a — Adoption of the v3 document set

**Lands:** docs/6, 7, 8, 9, 10 move under `docs/historical/`; `wire-surface-sync.sh`'s
`design_path` repoints at docs/12, and so do the two selftest case files that seed the
old paths (`cases/wire-surface-sync.cases.sh`, `cases/agents-md-current.cases.sh`) — the
complete set of path-reading gate files, verified; docs/16's deploy **and redirect**
sentences move into its preamble and the root `AGENTS.md` becomes its byte-identical
copy in the same commit docs/10 leaves; `CLAUDE.md` unchanged (`@AGENTS.md`);
`dependency-registry-sync.sh` lands against §2.8's table (docs/15 Part 2), with the
`toml` pin's L32-class disposition — remove it or land its consumer — decided and
recorded in the same commit; the first `g7` rows land (the doc-set swap itself is a
criterion: the gates that derive their subjects run green on the new set, and
`wire-surface-sync.sh` reconciles docs/12's D10 sentence). **Proves:** the successor
documents are the documents of record, every reconciler followed, and the registry
reconciles both ways from day one. **Terminal rungs:** none beyond `just ci`.
**Sizing:** half a session; pairs with P7b.

### P7b — Camera selectors (D14)

**Lands:** `schema::selector` (the closed five-spelling vocabulary and the one parser);
`engine::resolve::camera` widened to the selector; both roots' camera positionals and the
wire's `camera` parameter routed through it; D10's parameter prose and the agent guide
regenerated; the refusals reusing `CameraUnknown`/`CameraAmbiguous` with the
scheme-vocabulary message. **Proves (criteria):** every spelling parses and mis-parses in
both directions; the corpus's shared-`usb_id` pair answers `CameraAmbiguous` naming both;
a serial-less device matches no `serial:`; `NodePath` resolves against the live listing
(a fake hotplug renumbering moves the answer — the PF:22 semantics, asserted); selection
never filters enumeration (ids stable under any selector — the D1 ordinal claim);
`--json` failures round-trip against the bundle. **Terminal rungs:** R3 selector twins
(`hw_a_selector_finds_the_camera_its_fingerprint_names`, ambiguity on the Chicony pair)
run once and recorded. **Sizing:** one session including its reader.

### P7c — The device projection and `profile compare` (D15)

**Lands:** the destructuring projection and `ProfileComparison` DTO in
`schema::profile`; `DeviceProfile::compare`/`device_matches`; the `profile compare`
document verb on both roots (T4's below-the-executor clause; `cli-parity.sh` gains the
`document` bucket with its argument); `corpus_replay` deletes its private mask and
consumes the projection; schemas and guide regenerated. **Proves:** partition closure (a
field added to `ProfileInvariant` breaks the compile until sided — the arm is a
compile-fail fixture); every committed profile device-equals its identity-rewritten self
and device-differs from every other, sections named; the format-tree-only distinction
survives into the DTO; identity deltas match `differing_fields`. **Terminal rungs:** none
beyond `just ci` (corpus-shaped by design; §3.3 item 11 stays open and says why).
**Sizing:** one session.

### P7d — The embedding facade (D18)

**Lands:** `engine::facade`; the direct CLI's `InProcess` executor rebuilt as
parse-and-render around facade calls; the stability table in the module doc; the
`facade-is-the-composition.sh` predicate (the CLI names no engine composition module but
the facade — population derived from the facade's own exports); `facade-stability-table-sync.sh`
(the stability table reconciled against the crates it names, both directions, and against
D18's own bullet for the one-home claim). **Proves:** the executor crate's only engine
reach is the facade (gate, both directions, and the ban is on the *class* of reach rather
than on one spelling of it — a grouped import, a restricted visibility, an `extern crate`,
a glob and a second file of the same crate are all the same reach); facade
answers byte-identical to the pre-move executor on every read verb over the fake (a
one-time equivalence criterion, then the parity gate owns it transitively); the facade
refuses a bad selector in the words the composition uses; every engine module the facade's
own surface forces on a caller is in the table's **Yes** column. The store-lock refusal is
**not** a facade criterion and cannot be one: no `Facade` method touches the session
store, because D18 excludes the store-locked lifecycles, so that claim lives in
`cli-parity.sh`, which drives the session-writing verbs against a store a daemon holds and
counts how many it drove. **Terminal rungs:** none new.
**Sizing:** one session; the risk is churn in `cli/src/main.rs`, contained by the
equivalence criterion.

### P7e — G7 close

All `g7` rows counted; the review session (docs/14 Part E — populations named at
preflight: the selector vocabulary walk, the projection destructuring, the facade's
export list); fixes; evidence entry; reconciliation into docs/14's record. **Then** the
notes and this document's live counts reconciled.

## P8 — The instruments

Stream health, photograph comparison, and the device-loss contract — the measurement
story. Gate `g8`.

**Where the criteria are, 2026-08-20.** As P7: what enforces the **Proves** bullets below is
`scripts/gates/phase-criteria.tsv`'s `g8` block, counted in the execution record at `556e4d2`,
and it more than doubled after the work first landed — `7dd0c3e` left 7 rows and `556e4d2`
holds 17. The ten in between came from three batches rather than the two a reader would guess:
D17's luma repair (`a71748a`) armed four, D16's frame contract (`bcd2826`) five, and D19's
device-loss recipes (`8c78cf0`) one. The D17 four are the ones an audit of this phase's growth
would otherwise miss, and they make the paragraph's point rather than weakening it — they
arrived because a scene scored 0.9688 against itself (**N266**), which is the same shape as a
contract asserted against one backend and a double that could not produce three of D19's
sentences. **`g8` is not closed**: P8d is the close.

### P8a — Stream stats (D16)

**Lands:** `imaging::stream_stats::Accumulator` (+ its place beside
`imaging::video`'s interval home); `Frame.sequence`/`timestamp_us` contract tests;
`Fault::FrameGap` in the fake's menu (exhaustive-match walked like every fault);
`RecordReport.stats` filled by the record path (wall-clock skew there and only there);
schemas and guide regenerated. **Proves:** gap accounting from constructed vectors (both
directions — a gap counted, an unbroken run zero); percentile exactness within the
retained bound and the stated degradation past it (the truncation is on the answer, never
silent); the fake's gap fault produces the dropped count end to end through `record`;
`declared_interval` and the accumulator agree on the mean over the same take (one home,
two readers, reconciled). **Terminal rungs:** one R3 recording arm re-run to record real
stats on a healthy camera (the numbers are evidence, not assertions — orderings only).
**Sizing:** one session.

### P8b — `photo diff` (D17)

**Lands:** `imaging::compare` (total core, SSIM-unavailability representation); the
`image-compare` adoption **measurement** — the resolved feature graph checked clean
(`feature-posture.sh` is the standing backstop) and the lockfile diff recorded in the
note; if dirty, the owned-SSIM fallback lands instead and the note says so; the
`photo diff` document verb; schemas and guide regenerated. **Proves:** metric deltas over
the committed synthetic fixtures in both orders; `MetricName::ALL` walked (a sixth metric
joins by existing — asserted); dimension mismatch answers the reason vocabulary, never a
refusal; SSIM ranks a blurred fixture below its original against the sharp pair (the same
both-directions shape the metrics already carry); `--json` round-trips. **Terminal
rungs:** none new. **Sizing:** one session.

### P8c — The device-loss contract (D19)

**Lands:** the D19 contract as R1 tests over the fake's `DeviceGoneMidStream` — a photo
answers `DeviceGone` (never `SettleTimeout`/`Busy`); a take finalizes valid-to-last-frame
with the end named and the stats carried; a preview ends and the slot reaps; the hotplug
removal arrives bounded; **a later return is a new arrival whose fingerprint says it is
the same device at a different address** — plus the fake's own model brought up to the
contract it stands in for (the loss announces its removal, `FakeBackend::device_returns`
puts the camera back, and a vanished camera refuses every door into it rather than only
leaving the listing, notes N299–N300); the committed `hw_gone_*` recipes — one per clause,
over `WCH_DEVICE_UNDER_TEST`, `WCH_DEVICE_LOSS` and `WCH_DEVICE_RETURN` — that self-skip
counted ("needs an arrangeable mid-stream device loss") on every local host; and the contributed-evidence
protocol in the notes (what an E-entry from the partner rig must carry, note N299).
**Proves:** every sentence of D19 has a driven hermetic twin — the criterion's selection
is the union that names each one, the preview's arm included by reference to the `g6` row
that already drives it — and the hardware recipes exist, are recipe-named, and decline by
name. **What moved in the design, and why:** D19's recording bullet named `record_stop` as
the collector of the loss-time report, and `record_stop` is the one caller that hands back
the device's refusal instead [N115]; the bullet was amended and the residue named — the
loss-time stats are reachable in-process and not on the wire. **Terminal rungs:** the
`hw_gone_*` recipes run once locally to prove the *decline* (counted, named — the skip
path is the testable half here). **Sizing:** one session.

### P8d — G8 close

As P7e: rows counted, review in its own session, fixes, evidence, reconciliation.

## P9 — The operator's workbench (D20)

The web client's design pass, landed. No camera required — fake plus browser throughout;
the R1-web rung is this phase's terminal rung and its cost is named up front (the rung
grows by roughly a third; every sub-milestone below prices its claims). Gate `g9`.

**Where the criteria are, 2026-08-20.** As P7 and P8: the `g9` block in
`scripts/gates/phase-criteria.tsv` carries this phase's criteria, counted in the execution
record at `556e4d2`, and it grew after the browser work landed rather than with it: the four
rows added since `a907975` are `/session-photo`'s consumer, the client's cited Rust items, the
mutation floor's scope, and the write-during-suspend claim — while the sweep-time pane went
into the wording of the row that was already there. **`g9` is not closed**: P9d is the
close, and it is also where this document's own live counts are reconciled.

### P9a — The workbench shell and live tuning

**Lands:** the two-pane viewport-height shell (a preview pane nothing scrolls under —
`100dvh` grid, `overflow: hidden`, the pane a non-scrolling item of it — an independently
scrolling control column, and a stacked-narrow fallback where `position: sticky` is the
mechanism because there the document *is* the scroll container); the tuning arrangement over
the existing guarded writes and M32/N154 identity fences; the vivid-profile layout fixture.
**Proves (browser claims):** preview and the adjusted control simultaneously visible at
every scroll position, at the pinned viewport, against 77 controls; a clamp moves the
slider on screen with both numbers; a write during a photo-suspend is queued rather than
refused; the stale-panel fences hold under the new layout. **Terminal rungs:**
R1-web. **Sizing:** one session.

*The write-during-suspend bullet is narrowed, 2026-08-20 (note **N274**), and the narrowing is
the honest half rather than a retreat.* It read "lands after resume (queued, not lost)", and
the claim that landed asserts the queue's **effect** — the `wch_set` from a second connection
is answered with its `{requested, applied}` pair, the device is holding the new value
afterwards, and the pair costs one descriptor, one suspension and three streams. It does not
assert the *ordering*, because asserting "after the resume" needs the claim to know the actor
had already begun the photo, and nothing this daemon publishes says so: `last_used_ms` moves
for the preview's own turns, and the fake's stream count is behind the per-camera lock the
actor re-takes for every frame of a settle — both measured, both letting the write in first or
never showing the window at all. It retires when `CameraActivity` carries the `busy` flag
`engine::actor` already keeps, which is one `watch` away and is N274's named repair.

### P9b — `/session-photo` and the sample grid

**Lands:** the `/session-photo` route (GET + HEAD twin; reference-addressed, path derived
server-side through D9's rules); `CAMERA_BEARING_PATHS` grows its third entry **in the
same commit**, with both halves of the route-gating partition extended
(`web-routes-are-gated.sh` arms; `every_camera_bearing_route_is_behind_the_gate` drives
the new path anonymous-401/token-200/cross-site-403); the sample grid view reading the
session document it already has. **Proves:** the route serves exactly the session tree's
own samples by reference (a reference outside the session answers 404, a
caller-shaped path never touches the filesystem — arms in both directions); HEAD answers
about the route and opens nothing [N179's shape]; the privacy §5 clause holds (no other
door serves a stored frame — the gate's population proves the absence). **Terminal
rungs:** R1-web. **Sizing:** one session — the route is small and the gate arms are the
work, which is the right proportion for a camera-bearing door.

### P9c — Human-driven calibration

**Lands:** the start → plan → sweep → review → select(`human`) → apply → restore flow on
the page, sequenced over the eight existing verbs and the live subscription; the
sweep-time pane swap (progress + freshest sample through `/session-photo`); D13 refusals
rendered as the flow's guard rails. **Proves (browser claims):** the full flow end to end
against the fake — start, plan, sweep, review, select, **apply and restore** — with
`selector: human`, the goal and the criteria the operator typed landing in the session
document (asserted through `calibrate status` on a second socket — the page and the wire
agree); a `session_conflict` from an out-of-order click renders **the whole of** its
instruction-last sentence and the flow recovers; the sweep view paints each sample as its
event lands; the CLI can drive a session the page started (one state machine, two hands —
**owed an assertion**, not asserted: it is true of the product and no browser claim drives
it, because none of the rung's claims spawns a CLI or client binary at all, and the "second
socket" above is a raw WebSocket reading `wch_calibrate_status`. The word is corrected here
rather than left standing, since note **N305** records the same half as unasserted and one
fact is stated once). **The "and vice versa" half is struck (2026-08-20, note **N305**): the
shipped page has no verb for it.** `flow.session` is
assigned in exactly one place, from `wch_calibrate_start`'s answer, and the session list's
click hands its document to a painter rather than to the flow — so a page cannot pick up a
session the CLI opened, and the daemon's own refusal for the second `calibrate start`
("resume it, or finish it before starting another") names an instruction this client cannot
carry out. A Proves bullet nothing can drive is the state docs/9's derived-population rule
exists to prevent; the adopt path and the claim that crosses the hands are owed together and
are named in **N305** rather than left standing here as a claim.
**Terminal rungs:** R1-web. **Sizing:** one to two sessions — split between flow and
sweep-view if the first session says so.

*Amended 2026-08-20 (note **N276**), and the row is weaker on paper and stronger in the
rung.* It asked for an `IllegalTransition`, and no gesture this page offers produces one:
the grid offers only samples that exist, `Sweep next` cannot be clicked twice into the same
control, `Apply` always sends `partial: true` — which the daemon accepts — and the flow
disables the verbs whose precondition it can see. `session_conflict` is the refusal the
page's own buttons reach, and what the claim now holds is the **message**, not the
discriminant: the arm asserted the kind alone, so a build that dropped D13's message body
stayed green. The original wording becomes drivable the day the rung serves a profile with
a motorized control, which is `flow.refused`'s reason for existing.

*The second clause of that amendment is itself amended, later the same day (**N279**).* The
first version rested on "`Sweep next` skips a control that already has samples", and a
double-click reaches `illegal_transition` past exactly that reasoning — during the sweep the
control has no samples yet. The sweep is now non-re-entrant at the door and the absence claim
rests on that guard, which a browser arm drives.

### P9d — G9 close

Rows counted; the review session — with the P5 lesson standing (a web-client review's
reconciliation is written, or the gate is not closed; G5's absence cost five recurrences
one gate later); fixes; evidence; reconciliation. **This is also where the v3 plan's own
live counts are reconciled**, docs/7's ledger-discipline applied to this document.

## Post-plan triggers (recorded, uncommissioned)

| Item | Trigger | Ref |
|---|---|---|
| UVC H.264 → MP4 remux (L1) | hardware that exhibits `V4L2_PIX_FMT_H264` | D7, §8.3 |
| Control-change events | the workbench's stale-panel cost observed in real use | D20, §8.4 |
| AV1/WebM ingestion output (L2) | a real model-vendor ingestion need; an actual upload attempt is what makes N103's table `measured` | §7; N103 |
| The `wch-` rename | the owner's ruling on §8.11; lands as its own sub-milestone with the N90/N126 checklist, never in passing | §8.11; N90, N91, N126 |
| A process-failure D13 kind | the owner's ruling on N238's question (wire + exit-code change) | §8.12; N238 |
| D19's first contributed evidence | the partner rig runs the `hw_gone_*` recipes; lands as an E-entry + the E5 resemblance check of the fake's fault against it | D19; §3.3 items 9, 11 |
| Cross-machine profile comparison evidence | the same rig runs `profile compare` direct-vs-forwarded; retires §3.3 item 11's `declared` | D15; §3.3 item 11 |
| Session GC | a *measured* store-size quantity (instrumentation first — N55's re-phrasing stands) | §8.8; N55 |
| `webcam-handler-cli` auto-forward | refusal friction observed | §8.7 |
| Audio | a license-clean path appears | §8.2 |
| Mutation-floor default jobs | the owner's ruling on N251's price sheet: `nproc` (hides survivors under load — measured, four real defects waved through) vs 1 (13–19 h); the interim posture is `mutants.sh`'s stated warning | N251 |
| Re-run N5's jsonrpsee measurement | any jsonrpsee bump | §2.8, N5 |
| Re-check PF:16 against `little_exif` | any little_exif bump | D6, PF:16 |

## Risks to the plan

- **The context budget is a real resource** — unchanged, and the v3 shape holds the v2
  mitigations: session-sized sub-milestones, reviews and readers in their own contexts,
  split-don't-stretch.
- **P9 concentrates rung cost.** The browser rung grows by roughly a third and it is the
  slowest suite in `just ci` on node hosts. Mitigation: claims are priced per
  sub-milestone above; a claim that cannot be written tersely is a sub-milestone split
  signal, and the rung's manifest count keeps growth deliberate.
- **The adoption swap is a single-commit hazard**: five documents move, one gate repoints
  and one deploys, and any partial state is red somewhere by design (the gates' own
  arms). Mitigation: P7a is half a session *because* it must be one commit; the checklist
  is docs/12's adoption paragraph, verbatim.
- **An owner ruling can land mid-phase** (the rename, N238's kind, the mutants default).
  The convention holds: a ruling executes as its own sub-milestone against its checklist;
  nothing lands in passing. The plan's phases do not depend on any of the three.
- **The partner dependency is asymmetric by design**: nothing in P7–P9 blocks on
  usb-teleporter; the two `declared` items (§3.3 items 9 and 11) retire on contributed
  evidence whenever it arrives, and staying `declared` forever costs only honesty
  already paid.
- **Kernel/driver variance remains the standing unknown** — unchanged; PF findings land
  as notes + corpus the day they appear, and no phase closes with an unexplained R3
  failure.
