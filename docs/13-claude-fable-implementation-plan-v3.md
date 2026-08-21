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
| P7–P9 (the documents) | `7f37bbf` | the documents reconciled against the tree the eight batches above moved. The notes' "Expected usage" preamble gains the third consumer and the identity/description trade-off — the statement docs/12 §1 and `AGENTS.md` both end by pointing at, and which was not there to point at; **N262**'s stated measurement conditions are corrected in place and its "after" figures re-measured at the viewport the rung pins, over the fixture the claim is made on; and every live count in this document, docs/12, docs/15, `AGENTS.md`, `README.md` and `phase-criteria.tsv` is measured and restated, anchored to the commit it was true at, or replaced by the thing that reconciles it |
| P7–P9 (a race, kept out of the prose commit) | `5496c02` | the ban on counting turns of the scheduler, which is a `sleep` with the units filed off and which N3 banned only in the spelling that has units: an arm waited `for _ in 0..8 { tokio::task::yield_now().await; }` before asserting a feed had not been released twice and went red under load about something that had not happened. The racer is named — `hand_back` on a feed somebody is reading hands it to a fresh driver, which asks a backend replaying no cameras for a `STREAMON`, is refused, and retires its own feed — the arm now reads the runtime's own live-task count either side of the drop with no await between, and `clippy.toml` holds the class with three narrow `#[expect]`s in N3's auditable shape (note **N309**). Found by the reader verifying the documents batch and kept out of it, because a race repaired inside a prose commit is a race nobody can find again |
| P7e, P8d, P9d (the review, and its repairs) | `f9abe48` | the gate-close review, run to docs/14 Part E as one session over three gates: six lenses — four over the populations this plan named at preflight, and two cross-phase, over the rows' own populations and over the eight batches' repairs, which rubric rules 7 and 8 ask for rather than this plan — generated 128 candidates and killed 102 themselves, twenty-six reached an independent verifier whose default is REFUTED, and nine were confirmed, fifteen narrowed and two refuted — the per-lens arithmetic and the absence lists are evidence entry **E22**. What a whole-phase view saw that eight per-batch readings had not: N135's payload dispatch — the descriptor decides, never the caller's value variant — is closed on both backends and was never carried to the web client, which picked its widget off `type.kind` while `HAS_PAYLOAD` sat unread beside three names it does read, and **a landed D20 claim was green because of it**, an `<input>` on screen at every scroll position only because three compound cards rendered fields whose every write was an `EINVAL` (**N312**); the photo answer had no fence and painted under the next camera's card, M32's fifth element and the one painter every sibling module's repair had skipped (**N310**); and `g8` and `g9` ran neither the predicate suite nor its self-test, so those two phases would have closed on a habit where every block before them closed on a criterion — `just ci` ran the pair at every green boundary, which is why the louder reading, that nothing had proved those predicates can fail, is the false one (**N318**). Twice the first repair was the defect one spelling on: reading a module reach goes through the one home `rust-imports.awk`, which reroots before it flattens, after both facade predicates were taught `crate::` and still let `super::` and a renaming import shrink the population in silence (**N315**, **N328**); and the caller-named-file bound refuses by what it has read rather than by `stat(2)`'s answer, which is about a regular file and about nothing else, so `photo diff /dev/zero /dev/zero` was still OOM-killed with no document (**N329**) |
| P7e, P8d, P9d (the closes) | *this batch* | the three gates closed. `just gate-g7`, `just gate-g8` and `just gate-g9` count the criteria table's 25 `g7`, 22 `g8` and 12 `g9` rows (`just gate-g7`, `PASS gate-g7 — 25 items examined, 0 named skip(s)`, 17m13s; `just gate-g8`, `PASS gate-g8 — 22 items examined, 0 named skip(s)`, 18m46s; `just gate-g9`, `PASS gate-g9 — 12 items examined, 0 named skip(s)`, 17m00s); the review's own record is evidence entry **E22**, written before the reconciliation as docs/14 Part E requires, and the hardware the closes owed is **E23** — the R3 rung at the four attached cameras, the Dell 4K webcam reachable for the first time, and the vivid rung at 77 controls through the blessed helper; docs/14's reconciliation record gains the first three entries it has ever held, which is Part E's meta-rule kept rather than skipped — a gate closes when its reconciliation is in that document, and the one G5 skipped cost a named class five recurrences a gate later. **The rows were audited before they were counted**, because the `what` column is the half of a criterion that says what the tree proves and nothing had ever read it: six readers over all fifty-nine `g7`, `g8` and `g9` rows, each finding then put to an independent skeptic whose default was that the row was fine; forty-three held and sixteen did not (**N335**). Eleven state something false about the tree — among them a count of one where three predicates read a design document by literal path, "Nineteen camera-taking verbs" over a tree with sixteen, a uniqueness **N291**'s own correction had already retracted, a pin resting on a card-name collision the six committed profiles do not have, and a remainder reached by subtracting a rung from a population of gate predicates that rung is not in — and five claim more than the row's own selection can see. Three of those five get the arm rather than the rewording, which is rule 1's preference, and each was driven with a defect its author did not pick: D15's format-tree permission standing beside an identity delta, `engine::facade` holding no camera between calls — witnessed through a backend that counts opens, because on the fake a retained handle is invisible to the next one — and the daemon's listing identity after a camera is lost (**N337**, **N338**). One of those three moves its row as well: the D15 identity row names its arms one at a time, so a fourth arm is a fourth name in the selection, while the other two select by module and by a name the row already carried and their arm lands inside a row nothing rewrites. The remaining two of the five gain the selection that holds their sentence, and one of those widenings named the arm it had just bought through a lone `test()` clause, which was the one thing the branch check could not see — the defect being repaired, one nesting level in (**N336**). Two classes the review had left ungated close here. A `tests` row's count of its own tests is compared, at last, against the number `counted-selections.sh` measures and prints three lines away — the figure `f9abe48` had repaired by deleting the phrase and nothing had been left able to go red on the next one (**N339**) — and that ban's own adversarial reader found it was itself one spelling: a comma kept at both ends of every word to protect `1,381` protected nothing, because a thousands separator only ever stands in the middle of a word, and `tests,` is not the noun `tests` (**N340**). The branch check stopped banning an alternation and started reading every `test()` clause: the guard that skipped a regex carrying no `\|` is deleted, so a lone clause is a branch of one, and the live population was 25 clauses over 16 rows rather than the six the finding named, because nextest spells union `+` as well as `or` — 168 named branches become 260, the 92 extra being the 92 lone clauses, and not one row goes red, which is what makes it a hole closed rather than a hole found (**N341**). A class is priced before it is declined, rather than declined in silence: a cardinal in a doc comment beside the vocabulary it counts is 137 lines under the narrowest matcher that sees all five live instances, 65 under a tight noun list that misses one of the two *wrong* ones, and 2 inside `closed_vocabulary!` blocks, where an exhaustive walk would be affordable and both figures were true — so the five are repaired the way **N319** repaired its own, one of them (`preview.rs`'s "a total function over five values") false since the commit that added the sixth member (**N342**); and the reconciler this close does land is honest about its own size, covering one of the audit's eight shapes and none of the other seven, which **N335** states rather than implies. Read adversarially, the same doc comment's arm turned out to be a hand list a seventh ending joins in silence (**N344**), and `imaging::compare`'s header-only fixtures stopped weighing whatever they liked under a `< 1_024` bound that would have passed a builder grown nine hundred bytes of raster: they weigh what a committed table derives from the two formats' own framing, PNG 68 as 8 + 25 + 23 + 12 and JPEG 150 (**N343**). `just generate` moved both committed artifacts with the batch, because `RecordingEnd`'s doc sentence is an input to the schema bundle and to the OpenRPC document. P7c's **Proves** bullet is corrected beside itself rather than three rows and a section away; and this document's live counts are reconciled, including the one it had wrong about the tree it was committed into |

Counts at `556e4d2`, the last of the eight implementation batches, every one of them read out of
that commit's own tree and re-measured for this line rather than copied from another document:
**43 gate predicates** (`scripts/gates/*.sh` less the four harness files `gate_predicates`
excludes), **502 fail arms across 43 case files** (`selftest.sh` prints the pair on every run
and is the authority; these are what the files hold), 24 `g7` rows, 17 `g8` rows, 9 `g9` rows,
and the browser rung at **37 claims and 387 assertions**, which are `browser/claims.json`'s own
two numbers and `web_browser.rs`'s floor. The docs-only batch below `556e4d2` adds no
predicate, no case file, no criteria row and no test, so the same six figures described the tree
this sentence was committed into.

**The P7e/P8d/P9d gate-close review's repairs move five of the six** (2026-08-21), and the five
are re-measured on the tree they land in rather than reasoned about from the diff. The browser
rung is **44 claims and 461 assertions** — still `browser/claims.json`'s own two numbers and
`web_browser.rs`'s floor, raised in the same diff as the seven claims that landed the G9 lens's
findings (notes **N310**–**N314**); no file under `crates/web/` moves after that, so the pair
describes the tree this sentence is committed into as well. The suite is **533 fail arms across
the same 43 case files**, and it reached that figure in two steps this block owns. **Twenty-two**
are the red-on-inverse arms for the five gate repairs the review returned (notes
**N315**–**N318** and **N323**) — the figure and its attribution are one claim, and the five
files carry it: between `556e4d2` and `f9abe48` `avi-reparse-is-independent` goes 10 → 12,
`counted-selections` 9 → 11, `facade-is-the-composition` 38 → 45, `facade-stability-table-sync`
25 → 34 and `profile-partition-is-closed` 9 → 11, no sixth case file moves, and the 502 stated
above plus those twenty-two is 524. The remaining **nine** are the closes' own and stand in one
file, `counted-selections.cases.sh` going 11 → 20: three for the ban on a `tests` row's count of
its own tests (**N339**), two more for the spelling that ban turned out to be (**N340**), three
for the branch check reading a lone `test()` clause as a branch of one (**N341**), and one for
the bare multiple of ten that ban could not read (**N345**); 524 plus those nine is 533. **This
sentence first read 514, and twelve, and the two were one error**: 502 plus twelve is 514, so the
total was derived from the attribution rather than counted. Where the twelve itself came from is
not recoverable, and the diff is not where — `git diff 556e4d2 f9abe48 -- scripts/gates/cases/`
adds twenty-two `fail_case_` lines and removes none. That is the class note **N334** records
against docs/15, met twice in one batch's own prose. It is corrected at P9d, where this
document's live counts are reconciled, and the per-file arithmetic above is what reconciles it. And
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
at `556e4d2`, and again at `f9abe48`, the execution record states. Several arrived only in the
repair batches — the three D15 claims nothing could go red on (a section list, a rendered
caveat and a conditional's false branch, note **N287**) and D18's facade criteria among them —
so a bullet here and a row there are the same claim at two ages, and the tsv is the one that
runs. **`g7` closed on 2026-08-21** over the table's 25 rows (`just gate-g7`, `PASS gate-g7 —
25 items examined, 0 named skip(s)`, 17m13s), the last of them the `cli-parity.sh` row P7d's
bullet had been delegating a claim to without naming it (note
**N318**), and its rows audited one by one before they were counted, several of them reworded at
the close to what the tree does (note **N335**); P7e is the close and what it cost is below.

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
field added to `ProfileInvariant` breaks the compile until sided — *this bullet commissioned a
compile-fail fixture and the tree holds a predicate instead:
`scripts/gates/profile-partition-is-closed.sh` reads the declaration and the destructuring and
counts the partitions it closed, which is five at `f9abe48` and was four until the review found
`CameraFingerprint` closed by nothing, note **N323**. The delta was recorded in docs/15 and in
the execution record above rather than here, which is the correction P7e owed this bullet*);
every committed profile device-equals its identity-rewritten self
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

**Closed 2026-08-21.** The criteria table's 25 `g7` rows counted by `just gate-g7`
(`just gate-g7`, `PASS gate-g7 — 25 items examined, 0 named skip(s)`, 17m13s). The review ran
to docs/14 Part E as one session over all three gates rather than three, because the three
phases share a tree and a lens reading P9's browser claims against P7's facade is walking one
population and not three; its two G7 lenses took the populations this plan named at preflight
— the selector vocabulary walk and the identity/device projection destructuring in the first,
the facade's export list and both of its new predicates in the second — and eight of the
twenty-six findings that reached the verifier came back from them.

What they cost is at `f9abe48`. `SelectorScheme::example()` is what every reader of the
vocabulary prints and nothing reconciled it with the grammar `parse` accepts (**N320**). The
facade's encapsulated population was derived from one spelling of a call, so an unqualified
import shrank it in silence and reopened the bypass the predicate exists for, and the module
reach now has one home in `rust-imports.awk` (**N315**, **N328**); the module-scope walk
enumerated three spellings of a reachable public item and missed the fourth, a method on an
inherent impl (**N316**); the Yes column was not closed, because `engine::photo` is Yes and its
public API cannot be used without two No modules (**N324**); and D15's partition was closed by
destructuring on four structs and by nothing on the fifth, `CameraFingerprint`, which is the one
the restore guard reads (**N323**). Five present-tense transcriptions of
`SelectorScheme::ALL.len()` survived N308's sweep, in Rust doc comments and test prose nothing
generates from; the numbers are deleted rather than gated, because none of the five reaches a
committed artifact — `CameraSelector`'s hand-written `json_schema` overrides its rustdoc — so the
reader is a maintainer and a phrase naming no cardinality cannot go stale (**N319**). What
**N319** did not do was record what it was leaving open, and the close found the class four more
times: a cardinal in a doc comment beside the vocabulary it counts is now priced — 137 lines under
the narrowest matcher that sees every instance, 65 under a tight noun list, 2 inside the
`closed_vocabulary!` blocks where an exhaustive walk would be affordable — and declined with those
numbers rather than in silence, the instances repaired and the absence written down as a limit
(**N342**).

The review's own record is evidence entry **E22**, written before the reconciliation as Part E
requires; the reconciliation is in docs/14's record, whose first entry this is. **Then** the
notes and this document's live counts were reconciled, at P9d.

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
sentences. **`g8` closed on 2026-08-21** over the table's 22 rows (`just gate-g8`, `PASS
gate-g8 — 22 items examined, 0 named skip(s)`, 18m46s): the five above the seventeen are the
review's own, because `g8` was one of the two phase blocks that ran neither the predicate
suite nor its self-test, and because three populations were owed a row in the same walk: the
`smoke-hw.sh` claim P8c's **Proves** bullet had already made, and the two reconcilers whose
committed artifacts P8 moved — `schema-artifacts-current.sh`, a criterion of no v3 phase, and
`agent-guide-current.sh`, a criterion of no `g8` (note **N318**). P8d is the close.

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

**Closed 2026-08-21.** The criteria table's 22 `g8` rows counted by `just gate-g8`
(`just gate-g8`, `PASS gate-g8 — 22 items examined, 0 named skip(s)`, 18m46s), five of them
landed by the review itself for the reasons the paragraph above states. The G8 lens took the
instruments as its population — `MetricName::ALL`, `PhotoFormat::ALL`, `Fault::ALL` and
`CapReached::ALL`, every field of `StreamStats` and `RecordReport`, D17's refusal paths and
D19's sentences — and both of its findings were about a bound that only half-existed: the
decode budget refused through two doors and only one of them said so, so turning the other one
off left the whole workspace green (**N321**), and `photo diff` allocated the whole of a
caller-named file before any bound was consulted, because the door the bound named was not the
first door (**N322**, and **N329** for the repair that stopped measuring the file with
`stat(2)`). A cross-phase lens added the third: the human `record` table showed none of D16's
instrument under a renderer doc claiming both halves show the same facts (**N326**). The close
then found that this block's own criteria said things the tree does not do, and that one of
the repairs for it bought a claim the branch check could not see until the check stopped
reading only alternations (**N335**, **N336**, **N341**).

This gate's hardware is evidence entry **E23** — the R3 rung, including D16's stream arm, at the
four attached cameras, and the `rung-vivid` row run with the module present, so the transcript
holds `rung-vivid: suite run, 0 named skip(s) before it started` beside the counted decline the
portable posture produces. The review, its own evidence entry **E22** and the reconciliation
are P7e's, run once across the three gates.

## P9 — The operator's workbench (D20)

The web client's design pass, landed. No camera required — fake plus browser throughout;
the R1-web rung is this phase's terminal rung and its cost is named up front (the rung
grows by roughly a third; every sub-milestone below prices its claims). Gate `g9`.

**Where the criteria are, 2026-08-20.** As P7 and P8: the `g9` block in
`scripts/gates/phase-criteria.tsv` carries this phase's criteria, counted in the execution
record at `556e4d2`, and it grew after the browser work landed rather than with it: the four
rows added since `a907975` are `/session-photo`'s consumer, the client's cited Rust items, the
mutation floor's scope, and the write-during-suspend claim — while the sweep-time pane went
into the wording of the row that was already there. **`g9` closed on 2026-08-21** over the
table's 12 rows (`just gate-g9`, `PASS gate-g9 — 12 items examined, 0 named skip(s)`, 17m00s),
three of them the review's for the reason `g8`'s five were (note **N318**): P9d is the close,
and it is also where this document's own live counts are reconciled — the one it had wrong
about its own tree included.

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

**Closed 2026-08-21.** The criteria table's 12 `g9` rows counted by `just gate-g9`
(`just gate-g9`, `PASS gate-g9 — 12 items examined, 0 named skip(s)`, 17m00s). The G9 lens
drove the workbench in the pinned Chromium against a live daemon rather than reading it, and
five of the twenty-six came back from it. The two the execution record above states are what
that cost bought: the widget chosen off `type.kind` (**N312**) and the unfenced photo answer
(**N310**). The other three: a `wch_list` in flight when the socket died rewrote the page's
final sentence with one that opens "connected" (**N311**); a camera switch stranded the open
session with no way back to it, and `Start` was re-entrant, so two clicks ended in a red
`session_conflict` about the session the page had just created (**N314**). The way back to a
stranded session is the owner's ruling and is not made here, which
**N314** records.

The P5 lesson held: the reconciliation is written into docs/14's record, and the gate closed on
it. **This is also where the v3 plan's own live counts are reconciled**, docs/7's ledger
discipline applied to this document — the counts paragraphs above are re-anchored at the tree the
closes commit, and the fail-arm figure this document had wrong about the tree it was committed
into is corrected there with the per-file arithmetic that reconciles it. The same discipline was
applied to the criteria table itself before the three gates counted it, which is the execution
record's row above and notes **N335**–**N344**.

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
