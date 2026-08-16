# The G6 adversarial review — the whole codebase

Doc 11 in the webcam-handler series, and the second that is a **record of a review rather than a
standard to review against**. The first — the P5 web-client review of 2026-08-13/14 — was filed
directly under `docs/historical/` on the ground that it "describes one tree, on one day, and it does
not become wrong when the code changes — it becomes *history*". The same is true of this one, so
slot 11 is best read as *the current review record*, and this document belongs beside its
predecessor once the next review supersedes it.

**Run 2026-08-15/16, against `3a7b9fa`**, closing docs/7's **P6e** requirement ("Then, in its own
session, the adversarial review; fixes; evidence entry; reconciliation") and rubric docs/8 **Part
E**. `just ci` was green at the start and the tree was clean; the baseline is in §1.3. This is the
evidence entry Part E asks for, written before the reconciliation and carrying the census, the
refutations and the absence lists.

**Scope: the entire workspace.** Every crate, every gate, the web client, the manifests and the
generated artifacts — 126,604 lines of Rust across fourteen packages, plus `scripts/`, the
`justfile` and `docs/`. The four prior reviews each took a phase; this one takes the tree.

## What it found, in one paragraph

**No Critical finding, no memory-safety defect, and nothing wrong with the `unsafe` boundary, the
credential layer, the atomic-write home or the error registry's shape.** Four HIGH findings: an
**explicit `--pixel-format` or `--size` request is silently substituted** rather than refused, in
violation of D5's own sentence and honoured only by the fake (H1); a **calibration sweep interrupted
before its first sample is unreachable by every verb**, which is the durable half N24 left open and
which `calibrate restore` does not repair (H2); the **gate enforcing "killing is never a fallback"
counts files where its own header claims call sites**, so it is blind in exactly the file its own
justification names (H3); and **`preview.stop()` never ends the MJPEG request**, so every camera the
web client has previewed keeps streaming for the life of the tab, holding cameras open against the
agent (H4). Three of the four are recurrences of classes the rubric already names — each of those
rows having been added *because of* an earlier instance. Below them, thirty-three MEDIUM findings
cluster in three places worth naming: the daemon's recording interlocks (reservations whose release
depends on a code path running), the **D13 messages and the agent guide**, which have now drifted
from the code five times in one phase, and a set of **stated-but-unenforced properties** — a
documented 405 that is a 200, a socket-activation check three documents promise and nothing makes,
an unbounded step in a bounded shutdown. The tree's engineering remains exceptional: 204 of 290
candidates were killed by the agent that raised them, and 168 absence claims record what was walked
and found sound.

---

## 1. Method

### 1.1 Twenty-two lenses, and what each was told

Two passes ran against a frozen tree, each lens read-only and forbidden from changing a file.
Sixteen lenses took a **subsystem**; six took a **cross-cutting axis** the subsystem readers cannot
see between them. Each lens's candidates then went to an **independent adversarial verifier** whose
brief was to refute them, defaulting to REFUTED when unconvinced.

| # | lens | scope |
|---|---|---|
| 1 | `schema` | the control model (D2), the D13 registry, T1/T2, `limits`, the slug transform, D1's identity, D5's ranking, the profile format |
| 2 | `imaging-avi` | the D7 L0 muxer, the independent re-parser, the close-time rewrite |
| 3 | `imaging-rest` | decode/encode/exif/photo/metrics/y4m/video/fixtures |
| 4 | `engine-capture` | the actor (D12), the capture pipeline (D5), settle, guarded writes (D3), snapshot/restore (D4), the preview feed |
| 5 | `engine-calibrate` | the D8 model and state machine, the sweep planner, the D3 probe, recording |
| 6 | `engine-store` | D9: `write_json_atomic`, the fd-lock, the layout, `log.ndjson`, the version refusal |
| 7 | `v4l2-safe` | enumeration and grouping, the uevent watch, busy diagnosis |
| 8 | `v4l2-unsafe` | every file under `src/sys/` — rubric B10 |
| 9 | `daemon-core` | the T5 server, the actor registry, events, recording, the Unix socket |
| 10 | `daemon-http` | the token, the gate, provenance, the listener, the MJPEG route |
| 11 | `daemon-lifecycle` | shutdown, systemd, logging, the composition root |
| 12 | `api` | the one wire surface, the D13 code map, the base64 transport, `schemas/` |
| 13 | `cli` | T4's command core, both composition roots, the client transport, the failure document |
| 14 | `fake-testkit` | the stand-in E5 governs, the battery, the corpus, the oracles |
| 15 | `web` | the ten ES modules, the embed |
| 16 | `gates` | `scripts/gates/`, `phase-criteria.tsv`, the `justfile`, nextest, deny, clippy, `xtask` |
| 17 | `design-deviation` | D1–D13, T1–T6, E1–E6, §2.10's homes, §2.8's edges, §1's non-goals, §3.3's register — walked item by item |
| 18 | `test-integrity` | Part C, over every `tests/` directory and every `#[cfg(test)]` module |
| 19 | `concurrency` | the actor, the runtime, the locks, the channels, shutdown — across crate boundaries |
| 20 | `performance` | the photo path, the recording path, the sweep, in *this* deployment |
| 21 | `panic-surface` | reachable `unwrap`/index/overflow/truncation on device- and request-driven paths |
| 22 | `privacy-security` | §5 and rubric A12/B8, D11's posture, the root-capable helper |

Each was given the same instructions from Part E: **what is settled** — D1–D13, T1–T6, E1–E6, §7's
rejected alternatives, §1's non-goals — and the standing instruction to grep
`docs/implementation-notes.md` for the relevant N-entry *before* raising a candidate, because a
candidate an N-entry already argues and prices is refuted rather than found. Each was told to
attempt its own refutation first, to confirm every `file:line` it cited by reading it, to make
absence claims name where they looked, and to **keep the candidates that died**. Lenses 10 and 15
were additionally pointed at `docs/historical/11-claude-web-client-code-review.md` and told not to
re-report its thirty-two findings.

### 1.2 What the reviewer did that the lenses could not

Three things, and they are where ten of the findings below came from.

**A live daemon.** The credential and provenance layers were probed with twenty request shapes
against a running `webcam-handler-daemon --backend fake --http`; the calibration walkthrough in the
README was executed end to end; `record`, `photo`, `preview` and their interactions were driven
against a real socket. **Ten of the findings below are measured** through the shipped binaries or a
live daemon rather than read off the source — H1, H1b, M5, M11, M19, M20, M23, L1, L19 and L29 —
one candidate died on measurement and two were narrowed by it.

**Adjudication where the lenses and their verifiers disagreed.** `daemon-http` and
`privacy-security` both raised the bearer token's arrival in the systemd journal at HIGH, and
`privacy-security`'s verifier refuted it; neither was right, and §4.6's M25 carries the resolution.
The traffic ran the other way too: a verifier refuted the reviewer's own reading of the torn-log
finding by producing the case law he had not found (§4.9).

**Reproduction attempts, with the load stated.** Part E: *"a green run is evidence about a race only
with its load stated."* Where a finding claimed a window, it was attempted against a live daemon and
the attempt is reported whether or not it succeeded — §4.10 records a confirmed defect that two
reproduction attempts did **not** hit, and saying so is the prioritisation signal.

### 1.3 The baseline

`just ci` green, exit 0, on `3a7b9fa`:

- **1388 tests run, 1388 passed, 29 skipped** (the `#[ignore]`d hardware and vivid suites).
- **`selftest`: 29 predicates, 62 pass arms, 294 fail arms** — every predicate green on the tree and
  red on each of its inverses, and the checkout as the arms found it.
- **`counted-selections`: 322 items examined, 0 named skips** — 130 phase-gate test selections, 117
  named alternation branches, 75 command criteria.
- One named, counted skip in the whole run: `uds-permissions`, for want of non-interactive
  privilege.
- The R1-web browser rung **ran** (node and the pinned Chromium present); the oracle rung **ran**
  (ffprobe and mpv present).

Two rungs were deliberately **not** run and their absence is not a skip this review counted: the R2
vivid rung, because running it loads and unloads kernel modules on the owner's machine and this
review was not asked to; and `just mutants`, whose own configuration says it is hours rather than
minutes. §5.4 is a finding about the second of those that does not need it run.

---

## 2. The census

**290 candidates considered · 204 killed by their own author · 86 reported · 86 independently
verified, of which 38 confirmed as stated, 35 narrowed, 13 refuted · 168 absence claims.**

The run is complete: 22 lenses, 22 verifiers, 32 agents, no errors.

Every number here is counted from the run's own record, not estimated.

| lens | considered | self-refuted | reported | absence claims |
|---|---|---|---|---|
| `schema` | 13 | 8 | 5 | 8 |
| `imaging-avi` | 13 | 11 | 2 | 7 |
| `imaging-rest` | 10 | 6 | 4 | 10 |
| `engine-capture` | 9 | 6 | 3 | 8 |
| `engine-calibrate` | 11 | 6 | 5 | 8 |
| `engine-store` | 11 | 8 | 3 | 7 |
| `v4l2-safe` | 10 | 7 | 3 | 8 |
| `v4l2-unsafe` | 17 | 13 | 4 | 10 |
| `daemon-core` | 11 | 8 | 3 | 7 |
| `daemon-http` | 12 | 7 | 5 | 8 |
| `daemon-lifecycle` | 9 | 6 | 3 | 6 |
| `api` | 13 | 10 | 3 | 7 |
| `cli` | 11 | 7 | 4 | 7 |
| `fake-testkit` | 12 | 7 | 5 | 9 |
| `web` | 18 | 12 | 6 | 10 |
| `gates` | 10 | 7 | 3 | 8 |
| `design-deviation` | 20 | 13 | 7 | 6 |
| `test-integrity` | 12 | 7 | 5 | 9 |
| `concurrency` | 13 | 9 | 4 | 6 |
| `performance` | 16 | 13 | 3 | 5 |
| `panic-surface` | 21 | 18 | 3 | 7 |
| `privacy-security` | 18 | 15 | 3 | 7 |
| **total (22 of 22)** | **290** | **204** | **86** | **168** |

By severity **as the lenses reported them**: 11 HIGH, 37 MEDIUM, 38 LOW, no Critical. Verification
moved most of the HIGHs down — the two concurrency ones to MEDIUM, one privacy one to a narrower
MEDIUM and one to LOW — which is what a verifier is for and why §3 carries three rather than ten. The
reviewer's own pass added thirteen candidates, three of which died on measurement; its survivors are
folded into the sections below rather than counted twice.

**This review carries 78 findings forward** — four HIGH (one with two halves), thirty-three MEDIUM,
thirty-eight LOW and three performance — after merging the duplicates two or three lenses found
independently and dropping what verification narrowed to nothing.

### 2.1 On comparing the false-positive rate with E4, E6 and P5

The honest answer is that it is not directly comparable, and saying why is more useful than a
number. Those reviews reported *raised* against *confirmed* from harnesses that ran refutation as a
separate pass. This one asks each lens to attack its own candidates **before** reporting, so
"raised" has two meanings:

- **Before reporting: 204 of 290 died, 70%.** That is the lens killing its own work, and it is where
  most of the filtering happened.
- **After reporting: of the 86 verdicts the independent verifiers returned, 38 were confirmed as
  stated, 35 were narrowed and 13 were refuted outright** — 44%, 40% and 15%.

Against E4 (52% refuted), E6 (61%) and P5 (59%), the second figure is the closest analogue and lands
in the same band. **What is new is the first figure**, and it is the harness observation worth
keeping: a lens instructed to refute itself first discards roughly five candidates for every two it
reports, before a second agent is spent on any of them. Both stages earn their place — the second
stage still narrowed or killed **57% of what reached it**.

### 2.2 The last lens changed the shape of the result

`web` returned last, and it is the reason this document has a fourth HIGH. It raised eighteen
candidates, killed twelve itself, and among the six it reported is **H4** — a preview stream that is
never ended — which the P5 review of **2026-08-13/14** did not find while reading the same module
and confirming fourteen other defects in it.

That is worth more than the finding. H4 is not visible in the source: `preview.stop()` looks
correct, its comment argues the right rule, and what makes it wrong is Chromium's answer to "is a
detached `<img>`'s streaming request aborted?" — which is a question about a dependency rather than
about the code, and which only a browser can answer. **The R1-web rung exists precisely to ask
Chromium questions and it does not ask this one**: it asserts that frames *paint*, never that they
*stop*. Rubric A9's second half — *a claim about a dependency nobody had read* — for the third time
in this review (with M23's `HEAD` and N70's `is_closed` before it).

It is also the review's best-verified finding, and the way it got there is worth copying: the lens
measured it in the pinned browser, and its verifier **re-ran the probe rather than accepting the
measurement**, which is the difference between a second opinion and a second reading.

---

## 3. The HIGH findings

Four findings. Three are recurrences of classes this project's own rubric already names — H1 of
rubric A9 / doctrine E5 (*a divergence between the stand-in and the real thing convicts whichever
side is wrong*), H2 of rubric A4's second half (*a state a transient failure can strand with no verb
out*), H3 of rule 6's addendum [S:N10] (*a selection counted but counting the wrong thing*) — and
every one of those rows was **added because of an earlier instance of the same class**; §9.1 is why
none of them fired. H4 is the one genuinely new shape, and it is a comment that reasons correctly
about the wrong object.

### H1 · D5's "an explicit request still wins" is not enforced, in either half

`crates/schema/src/capture.rs:121-158`, `crates/backends/v4l2/src/lib.rs:709-716` ·
**correctness / design-deviation** · the format half confirmed by the `design-deviation` lens, its
verifier, the `schema` lens independently and the reviewer; the size half measured by the reviewer
through the shipped binary.

D5's unamended half is unambiguous, and the 2026-08-13 ranking amendment goes out of its way to
leave it standing:

> **An explicit request still wins**: a caller that names a format and a size gets them or a typed
> refusal, and the ranking is only for the request that named neither.

Neither the named format nor the named size gets that treatment.

`StreamRequest::choose` does not do that:

```rust
let requested = self.pixel_format
    .and_then(|wanted| formats.iter().find(|f| f.pixel_format == wanted));
let (chosen, reason) = match requested {
    Some(named) => (named, ChoiceReason::Requested),
    None => rank_formats(formats, self.sink_fidelity)?,   // ← a named-but-absent format lands here
};
```

A caller that names a format the device does not enumerate is indistinguishable, at this line, from
a caller that named nothing — so the request falls into D5's ranking and the camera streams
something else. `V4l2Camera::negotiate` calls `choose` and raises `FormatUnsupported` **only** when
it returns `None`, which happens only for a device whose whole format list is empty or unreadable.

**`webcam-handler-fake` has the guard the real backend lacks**, three lines and a comment
(`crates/backends/fake/src/camera.rs:671-678`):

```rust
// A format the device does not offer at all is a refusal.
if let Some(wanted) = request.pixel_format
    && !formats.iter().any(|f| f.pixel_format == wanted) { return Err(FormatUnsupported { … }) }
```

Measured: `webcam-handler-cli --backend fake … photo --pixel-format NV12` against a camera offering
MJPG and YUYV answers `{"kind":"format_unsupported","requested":"NV12","available":["MJPG","YUYV"]}`
and exit 18. The same request against the same camera through `webcam-handler-v4l2` takes a
photograph.

**Failure scenario.** An agent validating a display driver asks the DUT camera for `--pixel-format
GREY` because that is what its comparison pipeline expects. The camera has no GREY mode. Instead of
`format_unsupported` — whose disposition in `docs/agent-guide.md` is *fix the request* — it receives
a photograph in MJPG, and the only trace is an `adjustments` entry the guide never tells it to read.
Two photographs an hour apart can then differ in encoding where the device did not differ at all,
which is the failure the whole product exists to prevent (*"the product is comparability across
time"*).

**Why nothing is red.** Both tests that pin the explicit-request contract run over the fake, which
honours it. This is **E5's doctrine paying a second time in the same direction as the first**: §2.3
records the P2 review finding that *"the fake refused the `Bytes`-at-a-scalar write that the real
backend mis-dispatched — a divergence between stand-in and real is a finding against whichever side
is wrong, and this time that was the real one."* Three phases later, same sentence, different
method.

**Missing test.** `crates/backends/v4l2/tests/hardware.rs::hw_a_format_the_camera_does_not_offer_is_refused_rather_than_substituted`,
and — because that rung is `#[ignore]`d — a `webcam-handler-testkit` battery arm that asks *every*
backend for an absent format and requires `FormatUnsupported`, so the conformance battery is what
catches the next backend that forgets. The battery is the right home: §2.11 step 4 calls it "the
definition of done".

**Direction.** Put the guard where both backends inherit it — in `choose`, which is the shared
resolver — by distinguishing "named and absent" from "named nothing". `choose` returning
`Option<ChosenFormat>` cannot express the difference, so it needs a third answer; the fake's
pre-filter then becomes redundant rather than load-bearing, which is the right end state for a rule
D5 states once.

#### H1b · A named size that nothing fits resolves to the format's *largest* mode

The size half is worse in a different way, because it affects both backends and is measurable
without hardware. `capture.rs:143-158`: when the `(Some(width), Some(height))` arm finds no entry
inside the request it ends `.map_or(default_size, …)`, and `default_size` (`:135-141`) is
`max_by_key` over **area**.

Measured through `webcam-handler-cli` over a committed profile whose MJPG list is
`[1920×1080, 3840×2160, 1280×720, 1280×960, 1920×1440]`:

| request | negotiated | ratio |
|---|---|---|
| `--size 320x240` | **3840×2160** | **108× the pixels asked for** |
| `--size 1280x720` | 1280×720 | exact |

The `Adjustment::Size` is reported, so D5's *reporting* half holds. What does not hold is the
choice — and D5 argues against it in its own words, about the neighbouring case:

> a stepwise or continuous frame-size entry answers with the closest size it can actually deliver
> (`largest_within`), **never collapsed to its maximum corner first** — a device offering 32..1920
> in steps of two can deliver a requested 640×480 exactly, and answering 1920×1080 "as an
> adjustment" would be false.

The rule the design states for stepwise entries is inverted for discrete ones the moment nothing
fits. "Smaller than everything you have" is answered with the largest thing there is, when the
smallest is the closest deliverable answer.

**And the manual is wrong about the remedy.** `docs/agent-guide.md`'s `format_unsupported` row tells
an agent the refusal is met by *"a `--size` or `--pixel-format` this device does not offer — ask for
one it does"*. **No producer of `format_unsupported` in the workspace is reachable from a size** —
grepped independently by the lens and by the reviewer. So an agent asking for a small frame is told
it will be refused, is not refused, and receives a frame 108× larger at roughly 800 KB instead of
15 KB, in a JSON field the guide never tells it to read. For a consumer that photographs a device
under test continuously, that is the fast-diff loop's whole budget.

**Direction.** When nothing fits, answer with the **smallest** offered mode, or refuse — either is a
decision, and the guide already describes the second. The current behaviour is neither. The
assertion to invert is `capture.rs:1358-1367`, which pins today's answer under the comment "nothing
in a 32-pixel-minimum range fits inside 8x8".

### H2 · A sweep killed before its first sample strands the control, and no verb can leave the state

`crates/engine/src/calibrate.rs:227` and `:394-398` · **robustness** · confirmed by the
`engine-calibrate` lens and by the reviewer reading both of its load-bearing claims.

**This is not a re-report of N24.** N24 closed the in-process half of exactly this hole and its own
*Retires when* clause names what it left open. What survives is the **durable** half.

`calibrate::run` commits `Sweeping { done: 0, total }` to `session.json` *before* the first camera
write. The only producer of an exit from that state is `calibrate.rs:394-398`, which runs
in-process on the interruption path — and does so best-effort (`let _ = lifecycle::commit_state(…)`),
so a full disk that stops the sweep also stops the repair.

If the process dies between the commit and the first `record_sample` — Ctrl-C, SIGKILL, a panic, a
power cut — nothing runs it, and N24's own walk of the closed exits then applies to a state that is
now on disk:

- `may_begin_sweep` refuses `Sweeping` (`session.rs:495-503`) → no re-sweep.
- `selectable` refuses `no_samples` (`session.rs:450-464`) → no `select`.
- `lifecycle::draft` touches only `Untouched` → `plan` is a no-op.
- `is_settled` is false, so `is_open` is true, so `calibrate start` for that (camera, task) answers
  `SessionConflict` **for the life of the state directory**. There is no session GC (N55) and no
  `abandon` verb.

**Verified independently:** `lifecycle::recover` — the durable recovery path, and what
`calibrate restore` runs — restores the camera snapshot and writes `draft.pre_snapshot = None`
(`lifecycle.rs:971-993`). It **never touches a control's status**. So the shipped repair verb puts
the camera back and leaves the session unusable.

**And the crash suite cannot see it.** `crates/engine/tests/crash_recovery.rs` contains no reference
to `calibrate::run`, `begin_sweep` or `Sweeping` — its one write goes through `lifecycle::sweep_write`
directly. The SIGKILL suite that exists to prove design §6's crash story therefore never puts a
control into the state the crash story is about. That is Part C's named smell — *a test whose
fixture cannot exercise the rule it pins* — for the fourth recorded time (see §7.2).

**Failure scenario.** `calibrate start --task dut`; `plan`; `sweep focus_absolute --all`; Ctrl-C
during the first sample's settle. `calibrate restore` puts the camera back. Every subsequent verb
for that (camera, task) refuses, permanently, and the agent has no hands. **A transient availability
failure has become a permanent capability refusal** — AGENTS rule 7 and rubric A4's second half,
which is the row E6 added *for this exact defect one layer down*.

**Missing test.**
`crash_recovery.rs::a_sweep_killed_before_its_first_sample_leaves_the_control_sweepable_again`: the
child runs `calibrate::run` and announces itself from inside the first sample, the parent SIGKILLs
it, runs `lifecycle::restore`, and then asserts a second `run` of the same control succeeds. Red
today at the second `run`.

**Direction.** Make `lifecycle::recover` the durable half of N24: after the camera is back, walk the
session's controls and `abandon_sweep` every one that is `Sweeping` with no samples, appending the
`SweepInterrupted` record N18 already defines. That also repairs the case where the in-process
best-effort commit failed, which is the second way into this state.

### H3 · The gate that enforces "killing is never a fallback" counts files, not call sites

`scripts/gates/kill-is-never-a-fallback.sh:96` · **test-integrity** · confirmed by the `gates` lens
and by the reviewer reading the predicate.

AGENTS rule: *"Killing a process that holds the camera is an explicit command naming its target,
never a fallback."* N48 point 2 makes this predicate the whole-tree half of that claim, and says why
the Rust test beside it is not enough: *"`scripts/gates/kill-is-never-a-fallback.sh` makes the claim
over the whole tree… The P4c review is why the second exists; **a `Busy` retry added to `wch_photo`
would have left the first green**."*

The predicate's own header states the claim as a call-site count:

> Outside the backend crate, `holders::terminate(` appears exactly once, in the daemon's
> `terminate_holder` handler. **A second call site anywhere** — another handler, the CLI, the engine
> — is the fallback AGENTS forbids, whatever it is called.

The implementation counts **files**:

```sh
if grep -Eq "$call_pattern" "$file"; then          # boolean per file
    callers+=("$rel")                              # one element per FILE
fi
…
elif ((${#callers[@]} > 1)); then                  # more than one FILE
```

So a second `holders::terminate(` **in a file that already has one** is invisible. The most likely
such file is `crates/daemon/src/server.rs` — where the one legitimate call lives, and where
`wch_photo` lives. **The gate is blind in precisely the place its own justification names.**

It is worse than silent about it: the closing line reports the file count under a call-site label —
`gate_checked "${#callers[@]}" "call sites of the signal outside the backend crate"` — so CI prints
"1 call sites" on a tree that could have five.

This is [S:N10]'s family for the fifth recorded time (G2 twice, G3 twice, here) — *a selection that
is counted but counts the wrong thing* — and it is the instance guarding the most consequential of
the eight non-negotiable rules.

**Missing test.** `scripts/gates/cases/kill-is-never-a-fallback.cases.sh` has no arm that adds a
second call to an existing caller file. That arm is the fix's proof and it is one `printf` long.

**Direction.** Count occurrences, not files: `n=$(grep -Eo "$call_pattern" "$file" | wc -l)` summed
across files, failing when the total is not exactly one. (`grep -c` counts *lines*, so it would
carry the same defect one step smaller.)

### H4 · `preview.stop()` never ends the MJPEG request, so every camera the page has previewed keeps streaming for the life of the tab

`crates/web/assets/preview.js:87` · **robustness** · found by the `web` lens, **measured in the
tree's own pinned Chromium**, and confirmed by the reviewer reading the function.

```js
const replacement = img.cloneNode(false);
replacement.removeAttribute("src");   // ← the clone, which never had a request
img.replaceWith(replacement);         // ← the original still carries src and still owns the stream
```

The doc comment above it does the right reasoning about the wrong object. It argues, correctly and
at length, why `removeAttribute` beats assigning `""` (*"an empty source is a **request** — for this
document's own URL"*) — and then applies it to `replacement`, a fresh clone that never had a request
to end. The element that owns the live `multipart/x-mixed-replace` response is `img`, which is
detached from the DOM and otherwise left alone. Nothing in this client ever aborts it.

**Measured** by the lens against Chromium 151.0.7922.34 — the build `@playwright/test@1.62.1` pins,
which is the rung's own browser: after clicking through five cameras once, the daemon still has
**five live `/preview` responses and is still writing parts to all five**, and an explicit
`HeapProfiler.collectGarbage` does not end the abandoned ones.

**Why it costs more than a leaked socket.** Each abandoned stream holds a camera *open and
streaming* on the daemon, so:

- D12's idle close can never fire for any camera the operator has looked at — the camera is in use.
- `limits::PREVIEW_MAX_VIEWERS_PER_CAMERA` is 4, so returning to a camera four times exhausts its
  viewer slots with viewers nobody is watching.
- Every one of those streams is a camera the *agent* — this project's primary consumer — will then
  meet as `Busy`.

**Why the P5 review did not find it.** That review's lens 3 read this module and confirmed fourteen
defects in the client, including the neighbouring `streams.clear()` bug (H6, note N96). This one
needs the browser to answer "is the request still open after the element is detached", which is a
question about Chromium rather than about the code — and the R1-web rung asserts that frames *paint*,
never that they *stop*.

**Missing test.** An R1-web arm that selects two cameras and asserts the daemon's preview count
returns to one — the preview suite already counts open feeds, so the assertion exists on the server
side and wants a browser driving it.

**Direction.** One line: `img.removeAttribute("src")` on the element that owns the request, before or
after `replaceWith`. The clone is still worth keeping for the listener hygiene the header argues; it
is the abort that is missing.

---

## 4. The MEDIUM findings

Grouped by where the repair lands. Every row carries `file:line`, and every one was read at that
line. Where the reviewer measured a finding against running binaries rather than reading it, the row
says **measured**.

### 4.1 The daemon's recording and photo interlocks

| # | what | where |
|---|---|---|
| M1 | **N118's `not_recording` guard is a check-then-act.** `wch_photo` asks `Recordings::not_recording`, releases the registry lock, and *then* awaits `open_destination` on the blocking pool and enqueues the actor command. A `record_start` that claims the slot inside that window puts its stream in front of the photo, which then does exactly what N118 exists to prevent: suspends the take's stream, so the take loses the frames and D7's close-time rewrite turns the gap into a slower mean interval for the whole file. Two lenses found it independently and both verifiers confirmed it | `crates/daemon/src/server.rs:1770` |
| M2 | **A cancelled `record_start` strands the slot for the daemon's life.** `Reserved` and `Watchers` have no `Drop` — the module says so, and gives the reason (`withdraw` takes the registry lock and a `Drop` cannot `await`) — so every refusal path must call `withdraw` explicitly. A handler future that is *dropped* rather than refused calls neither: the slot stays `Slot::Starting`, every later `record_start` for that camera answers `Busy`, and any preview handed over stays handed over. **Two reproduction attempts against a live daemon did not hit the window** (§4.10) | `crates/daemon/src/record.rs:448` |
| M3 | **`Recordings::collect`'s catch-all says the daemon is shutting down when it is not.** The first `match` gives N114's three decided answers; the second folds every non-`Ended` shape into one `DeviceIo` asserting shutdown. Two ordinary interleavings on a healthy daemon reach it, and in one of them a finished take's report is discarded | `crates/daemon/src/record.rs:657` |
| M4 | **A take whose camera vanished cannot be collected in the words D13 has for it.** `record_stop`/`record_status` resolve live first, so an unplug answers `CameraUnknown` — which D13 defines as "a name that never resolved — distinct from `DeviceGone`, a camera that *was* there". `RecordingEnd::DeviceFailed` exists and is unreachable through these verbs | `crates/daemon/src/server.rs:1962` |

M1 and M2 are the same shape as each other and as H2: **a reservation whose release depends on a
code path running.** The tree's answer everywhere else is a type whose `Drop` cannot be skipped
(`capture::grab`'s `StreamGuard`, `actor::Liveness`); these three are where an `await` made that
impossible and the explicit path was taken instead. That is a defensible choice each time it was
made, and the third instance is where it becomes a pattern worth a rule.

### 4.2 The V4L2 backend

| # | what | where |
|---|---|---|
| M5 | **The self-busy refusal names this daemon's own pid.** The arm refusing a second `start_stream` builds `Busy { holders: holders::of(self.fd.path()) }` — a `/proc` walk that finds the caller, since the caller holds the fd it is walking for — while the comment three lines above says *"The holder list is empty rather than naming this process."* N48 point 5 is the law it breaks: *"naming this process's pid would invite a client to kill the daemon it is talking to."* Caught one layer up by `daemon::server::not_this_daemon`, so it is a contradiction and a latent invitation rather than a live hole. **Verified by the reviewer** | `crates/backends/v4l2/src/lib.rs:589` |
| M6 | **One unreadable control ends the whole control enumeration**, and `EBUSY` from a control read is reported as another process holding the camera. `unreadable_current` folds exactly `EINVAL` and `EACCES`; everything else propagates out of the walk, so a device that declines one control answers `controls()` with an error rather than with the other seventeen. That is availability converted into capability at the level D2 exists to prevent | `crates/backends/v4l2/src/lib.rs:494` |
| M7 | **The listing and the explanation of what it dropped are two different readings of the device.** `probe_nodes` computes the unreadable set on every enumeration and `enumerate()` throws it away; `diagnose` re-probes. So the `NodeUnreadable` hint can describe a different moment than the listing it explains — and T1's whole reason for `diagnose` (N7) is that the two are one answer's two halves | `crates/backends/v4l2/src/lib.rs:142` |
| M8 | **`Unknown { raw: 0 }` fabricates a kernel discriminant.** `sys::decode::capture_interval` correctly answers `None` when the driver clears `V4L2_CAP_TIMEPERFRAME`; one line above the `sys` boundary that `None` becomes `FrameInterval::Unknown { raw: 0 }`, whose `raw` the schema documents as "the kernel's `type` discriminant". D2's "represent the unknown" is being satisfied with an invented number rather than a fourth answer | `crates/backends/v4l2/src/lib.rs:739` |

### 4.3 The engine and the store

| # | what | where |
|---|---|---|
| M9 | **A torn `log.ndjson` tail leaves `calibrate status` refusing that session for ever, and no verb heals it.** *Narrowed after refutation, and the refutation is recorded because it corrected the reviewer* (§4.9). The candidate as raised called the **refusal** the defect; it is not. Refusing an unparsable interior line is deliberate, argued (*"guessing at its contents would invent a session history"*) and **pinned against a seeded mutant**: note **N12** lists *"a torn last line dropped even when a terminator follows it"* among the nineteen buggy implementations a named test caught, so `store_faults.rs`'s assertion pins the right side and the reviewer was wrong to say otherwise. What survives is narrower and is not settled anywhere: `append_log` writes at whatever byte the last writer stopped at without inspecting the tail, so one crash plus one later append produces a file that `lifecycle::status` → `history` → `load_log` refuses permanently, with **no verb that repairs it** — H2's shape on the log. The repair invents nothing: heal at *append* time under the lock already held (terminate a parseable tail, `set_len` back to the last newline otherwise), which makes durable the drop `load_log` was already going to perform | `crates/engine/src/store.rs:273` |
| M10 | **`write_json_atomic`'s stated contract is false after the rename.** `fsync_dir` runs *after* `temp.persist(path)` has published the document, and its `Result` is the function's. A failure there returns `StorageIo` for a write that landed, while the published contract and `lifecycle::commit`'s doc both promise the destination is untouched — and `lifecycle::persist` believes them, so the in-memory session and the disk diverge | `crates/engine/src/store.rs:627` |
| M11 | **The session tree holding camera frames is created at the ambient umask.** **Measured**: directories `0775`, `log.ndjson` `0664`, and every calibration sample photo `0664`. `session.json` is `0600` — and that is the sharpest way to put it, because it is private by accident of `tempfile::Builder`'s default rather than by decision. Next door, D11's runtime directory is `0o700` *and* its mode is re-checked at startup with a wrong mode refused (N39). **Honest scope**: `~/.local/state` is `0700` on this machine, so nothing is exposed today; the exposure needs a `$XDG_STATE_HOME` elsewhere or a distro that leaves the parent at `0755`. Defence in depth D11 has and D9 does not | `crates/engine/src/store.rs:297` |
| M12 | **`SettlePolicy::deadline_ms` is uncapped**, so the only ceiling on how long one photo holds a camera's single actor thread is `MAX_SETTLE_ROUNDS × FRAME_DEADLINE_MS` — a backstop whose own doc prices it against a non-advancing camera, not against a caller's number. AGENTS' "bounded everything" wants the bound in `limits`, and `PREVIEW_SUSPEND_MAX_MS` is the argued ceiling already sitting beside it | `crates/schema/src/capture.rs:523` |
| M13 | **The D3 probe restores itself against an empty pair set.** `discover::pairs` calls `snapshot::take(camera, &[], now)`, and `take` derives each entry's `ControlRole` from that set — so every entry is roled `Manual` and D4's automation-first restore ordering degenerates to alphabetical for the probe's own restore. The one home for "which relationships are automation" is bypassed by the one code path whose whole job is to discover them | `crates/engine/src/discover.rs:126` |
| M14 | **`SweepAdjustment` is built for every sweep and discarded by both composition roots.** Four adjustment kinds and a `precision`, documented as "the same doctrine as D3's `{requested, applied}`, one layer up", reach no caller. Rubric A8's "a typed declaration nothing reads" | `crates/cli/src/main.rs:400` |

### 4.4 Imaging

| # | what | where |
|---|---|---|
| M15 | **The Y4M header asserts a chroma siting the module says it does not assert.** `Planar::tag` writes `C420` for NV12; the comment beside it and the module header both claim this leaves 4:2:0 siting unstated, because "naming one would be a claim about the device that nothing here measured". `C420` is not the neutral spelling — it is a siting | `crates/imaging/src/y4m.rs:252` |
| M16 | **`decode_yuyv` requires an exact `stride × height` buffer**, contradicting the shared `plane_bytes` rule its two siblings obey — whose doc says padding after the final row "is not something a driver owes us". A driver that delivers the last row unpadded is refused by one decoder and accepted by the other two | `crates/imaging/src/decode.rs:352` |
| M17 | **The Y4M sink's stride handling is asserted for mono only**, so `fill_c420`/`fill_c422`'s padded-plane arithmetic is uncovered — which is precisely the class N108's amendment records as its third, unanticipated defect in `decode_nv12`, closed there by N130 and left open here | `crates/imaging/src/y4m.rs:1001` |

### 4.5 The wire, the guide and the command surface

| # | what | where |
|---|---|---|
| M18 | **The guide's `format_unsupported` remedy names a flag that cannot produce it** — see §3 H1b. `docs/agent-guide.md`'s `Do` column is hand-written prose, so `agent-guide-current.sh` cannot catch it | `xtask/src/guide.rs:838` |
| M19 | **`busy` grew two in-daemon producers and neither the message nor the guide followed.** **Measured**: a photo during a take answers `{"kind":"busy","holders":[]}` rendering as *"held by an unidentified process"* — the holder is this daemon, and for a take it is bounded by the take's own duration. The guide's `busy` row says *"Another process is streaming from the camera … `--wait` asks the daemon to queue you"*; `--wait` is `Enqueue::WaitUntil` over the **actor command queue**, and `not_recording` is checked *before* the actor is touched, so it cannot reach this refusal. Measured: `photo --wait` during a take refuses in **207 ms**. An agent following the manual has exhausted the documented remedy. N129's pattern for the third time in one phase | `crates/schema/src/error.rs:625`, `docs/agent-guide.md:525` |
| M20 | **The client's `--backend` refusal is unreachable for the shape an agent types.** `required_if_eq("backend", "fake")` is a property of the *one shared tree* (T4), so on `webcam-handler-client` clap's requirement check fires before the executor's designed refusal. **Measured**: `webcam-handler-client --backend fake list` → *"the following required arguments were not provided: `--profile <PATH>`"*, exit 2 — a message naming a flag whose addition cannot help on this root (N123's class). The refusal only appears once both flags are supplied. **The test cannot reach it**: `wchc.rs:189` drives exactly the three vectors that dodge `required_if_eq` | `crates/cli-core/src/lib.rs:231` |
| M21 | **`typed()` loses a discriminant the wire delivered intact.** It answers `None` both for an error object that is not ours and for one whose code *is* ours (`rpc_code` is injective) but whose payload this build cannot deserialize; the client reports the second as a dead socket | `crates/api/src/codes.rs:213` |
| M22 | **The committed OpenRPC document and schema bundle publish 211 rustdoc intra-doc links** — identifiers that resolve to nothing in the file the reader has, in the two artifacts D10 commits so consumers need no Rust toolchain. One of them is the only place a cap an agent must respect is named. N123's defect class, one surface along | `xtask/src/main.rs:493` |

### 4.6 The daemon's HTTP surface

| # | what | where |
|---|---|---|
| M23 | **`HEAD /preview` opens the camera and runs a capture.** The module argues *"`get` and not `any` … `GET` is what an `<img>` sends; anything else meets axum's own `405`"*. axum's `get()` also answers `HEAD`. **Measured**: `HEAD /preview?token=…&camera=…` → **200** with `content-type: multipart/x-mixed-replace`, so the handler ran; `POST /preview` and `PUT /rpc` → 405, so the sentence is right about every method it was thinking of and wrong about the one the framework adds. Rubric A9's second half — a claim about a dependency nobody read — which is N70's F3 in a second framework | `crates/daemon/src/http/preview.rs:189` |
| M24 | **The authority the whole `Origin` rule rests on is read first-wins.** `addressed_to` takes `headers().get(HOST)` — the first `Host` line — while its sibling `admits` ten lines above uses `get_all` and a non-short-circuiting fold *precisely because* a first-wins read is a rule whose answer depends on which layer parsed the request last. The same argument applies here and was not applied | `crates/daemon/src/http/provenance.rs:320` |
| M25 | **The bearer token reaches the systemd journal.** Two lenses raised this at HIGH and one verifier refuted it; **both are wrong, and the resolution is the finding.** D11 requires the URL to be printed and `main.rs:377-384`'s own comment calls it *"the one place in this daemon where a secret is written down on purpose"* — so the line cannot be deleted. But `tracing::info!` is not printing: it goes to whichever layer `logging::install` chose, and under `stderr_is_the_journal()` that is the **journald** layer — a persistent, `systemd-journal`/`adm`-readable sink, with `url = %…` landing as an indexable structured field. That is the sink N94 removed a *different* producer of, for the same run-long credential, three days earlier. **What narrows it to MEDIUM**: the shipped unit (`packaging/systemd/wchd.service:42`) has no `--http`, so the combination takes an operator adding one flag | `crates/daemon/src/main.rs:381` |
| M26 | **Socket activation never checks the inherited descriptor is *listening***, though three places say it does. listenfd 1.0.2's `validate_socket` checks `S_ISSOCK`, `getsockname()`, `SO_TYPE` and `sa_family` — never `SO_ACCEPTCONN` — and `adopt` adds only a pathname check. §2.8 adopts listenfd *for* the half that is not the protocol: "validates that the descriptor really is a listening `AF_UNIX` stream socket, which is the check that stops a `from_raw_fd` on a passed-in number being a lie this process then serves from." It does not make that check | `crates/daemon/src/systemd.rs:350` |
| M27 | **Step 6 of the teardown is unbounded.** `stop_in_order` takes one deadline and spends it on steps 3 and 5; step 6 (`housekeeping.await`) has no timeout, and the idle-sweep driver it joins can be parked in a `spawn_blocking` pass waiting on a camera actor's reply channel behind a minutes-long command. Two docs assert the daemon's worst-case stop time is bounded | `crates/daemon/src/shutdown.rs:531` |


### 4.7 The command surface, the fake and the test kit

| # | what | where |
|---|---|---|
| M28 | **A note that cannot be printed on standard error becomes the verb's failure.** Every commentary line the surface writes to stderr is propagated with `?`, so a failed stderr write replaces an outcome the verb has already achieved: the JPEG is on stdout, the snapshot is on disk, the sweep is persisted — and the process reports `failed: true`, exit 27. Worse for `calibrate sweep --json`, which then prints **the answer document *and* a `Failure` document on stdout**, breaking §2.7's rule that "a `--json` invocation prints exactly one `webcam-handler-schema` type, and which type it is says whether the verb answered" | `crates/cli-core/src/lib.rs:1694` |
| M29 | **The fake dispatches a control write on `control_type`, not on the descriptor's `HAS_PAYLOAD` flag.** §2.3's contract note — added *because of* the P2 review's ioctl-dispatch defect — says "Dispatch belongs to the descriptor", and rubric B2 states the rule holds "on both backends, and the fake and real backend agree (E5)". They agree only because every control in `corpus/` and in `synthetic-basic.json` is either a plain scalar or a plain payload, so no fixture can separate the two rules. This is the **third** appearance of the E5-divergence family in this review | `crates/backends/fake/src/camera.rs:231` |
| M30 | **No committed profile carries a measured automation pair**, so the fake's PF:3 coupling model has zero corpus coverage. §3.2 requires every profile-shaped PF finding — and it names PF:3 — to be "representable in **and asserted from** at least one committed profile"; the corpus predicate asserts only that some captured flag word has the INACTIVE bit set, which is a static fact and not PF:3's finding ("INACTIVE tracks pairing **live, both directions**") | `crates/backends/fake/tests/corpus_replay.rs:243` |
| M31 | **The snapshot/restore battery arm can return between its perturbation and its restore, leaving the camera moved.** It writes every perturbation, re-reads to prove something moved, and `continue`s to the next camera if that read fails — before `snapshot.restore_order()` is walked. AGENTS rule 8 is non-negotiable, and §2.11 step 4 instructs the author of every new backend to run this suite **against their device** | `crates/testkit/src/battery.rs:737` |


### 4.8 The web client

| # | what | where |
|---|---|---|
| M32 | **The control panel has no camera identity.** `select()` sets `state.camera` synchronously and then awaits `wch_controls`; the panel is not cleared, disabled or fenced in between, and `write()` reads `state.camera` at *send* time. So for the whole round trip every widget on screen belongs to the previous camera and a click on one writes to the new one — and because the server spawns a task per inbound WS message, two `wch_controls` answers can arrive out of order and paint the wrong panel permanently. The round trip is a device open plus a control walk, and minutes if a sweep is in front of that actor | `crates/web/assets/app.js:245` |
| M33 | **A refused `wch_list` at startup is lost.** `main()` awaits `enumerate()` with no `try`/`catch` and is called with no `.catch`, so a refusal becomes an unhandled rejection: the banner still reads `connected`, the camera list stays empty, and D1's empty-enumeration diagnosis is never reached because the throw happened first. **The identical call on the hotplug path *is* wrapped** (`app.js:374`), which is what makes this an omission rather than a policy | `crates/web/assets/app.js:171` |

### 4.9 Where the review was wrong, and what corrected it

Part E asks for the refutations to be kept. Three are worth carrying because each corrected a
reviewer who had already convinced himself.

- **M9's original claim — that `store_faults.rs` pins the wrong behaviour — is false, and the
  verifier found the settled law the lens and the reviewer both missed.** Note **N12** lists
  *"a torn last line dropped even when a terminator follows it"* among the nineteen seeded buggy
  implementations that a named test caught. So refusing an unparsable interior line is not an
  oversight the test entrenched; it is the side the project chose, against a mutant that chose the
  other. M9 above is what survives after that correction, and it is narrower and better for it.
  **The general lesson is worth more than the finding**: a reviewer who dislikes an asserted
  behaviour and therefore calls the fixture wrong has committed Part C's error one level up, and the
  only defence is the one Part E already prescribes — grep the notes for the subject *before*
  writing the candidate, not after.
- **The bearer token in the journal** was raised at HIGH by two lenses and refuted by one verifier;
  neither was right. §4.6 M25 carries the adjudication.
- **A preview arriving mid-recording delivering no frames** was raised by the reviewer and **died by
  measurement**: 20 MB of multipart JPEG in six seconds during a take. N117 works; the first reading
  mistook `timeout`'s suppression of curl's `-w` line for an empty stream.

### 4.10 Reproduction attempts, with the load stated

Part E: *a green run is evidence about a race only with its load stated.*

- **M2 (the stranded slot)** was attempted twice against a live daemon with a preview open to widen
  `Previews::hand_over`, killing the client at 120 ms and at 350 ms. Both landed *after* the slot had
  become `Slot::Running`: the take ran server-side to its own duration, wrote its 4.7 MB file, ended,
  and the next `record_start` succeeded. **The finding is sound by construction and was not
  reproduced in this configuration** — with the fake backend the reserve→running interval is a few
  milliseconds. On real hardware it is a `STREAMOFF`, an `S_FMT`, a `REQBUFS` and a header write.
  That is the prioritisation signal, not a refutation.
---

## 5. The LOW findings

Recorded so the next session does not rediscover them. None is fixed.

### 5.1 Latent traps

| # | what | where |
|---|---|---|
| L1 | **`ControlRange::align_down` overflows, and the test that appears to pin its extremes is in the one range where it cannot.** Every operation on the line saturates except `offset - offset.rem_euclid(step)`. **Measured** on a faithful transcription compiled both ways, with a real range `{min: 2000, max: 10000, step: 100}` and `value = i64::MIN`: overflow-checks **on** → `attempt to subtract with overflow`; overflow-checks **off** (what `cargo install` produces — L2) → **`10000`**, the range's *maximum* for its most negative input. On `pan_absolute` that is a motor driven to its far limit, which is §2.3's own worked example of the P2 ioctl-dispatch defect. **Not live**: both production callers clamp first, and driving the shipped CLI with `i64::MIN`/`i64::MAX` on all three step>1 controls of a committed profile gave correct clamped answers. The guard is a documented precondition on a `pub` method of the shared vocabulary crate, not a type. The test (`control.rs:1148-1154`) builds `wide { min: i64::MIN, … }`, where `offset` is `0` and the line under test cannot misbehave, under the comment *"Saturating at the extremes rather than wrapping into a different answer"* | `crates/schema/src/control.rs:363` |
| L2 | **The profile the shipped binaries are built under is the one this repo never mentions.** `Cargo.toml` gives four profiles a paragraph each (N116 is a whole entry about one of them) and declares no `[profile.release]`. The README's install line is `cargo install --path crates/daemon`, and `cargo install` builds `--release` — so every binary an operator runs uses cargo's defaults, including `overflow-checks = false`. The arithmetic semantics under test are therefore not the shipped ones, which is what turns L1 from a panic a test would catch into a silently wrong value | `Cargo.toml:210` |
| L3 | **A card name that slugs to nothing can take another camera's natural slug.** `assign_ids` checks the `camera-<index>` fallback against `taken` and never against `reserved`, while the comment above asserts the collision is impossible — D1's "a natural slug always wins its own name", inverted | `crates/schema/src/camera.rs:139` |
| L4 | **`PixelFormat::parse` accepts non-ASCII text** its own documented grammar refuses, and the refusal test's non-ASCII case is refused for its *length* instead — so the assertion passes for a reason other than the one it names | `crates/schema/src/camera.rs:631` |
| L5 | **The EXIF APP1 payload is unbounded device-derived text in a 16-bit length field.** `stamp_jpeg` builds `ImageDescription` and `UserComment` from every control the device reported, with no bound, and `little_exif` computes the segment length as a `u16` that truncates rather than refusing. A 77-control device (vivid) is the shape that reaches it | `crates/imaging/src/exif.rs:104` |
| L6 | **The RIFF size is the one derived size in `finish` written through `saturating_sub`** where every sibling is checked and refuses by name, so an underflow would emit the crash placeholder as a successful close | `crates/imaging/src/avi/write.rs:531` |
| L7 | **The idle-sweep driver ends silently on any `JoinError`**, so a panicked pass permanently disables D12's idle close with nothing logged. `JoinError::is_panic` distinguishes the two cases and is not consulted | `crates/daemon/src/server.rs:858` |
| L8 | **`Serving::stopped`'s abort does not close the connections' sockets.** The bounded-join doc says the listener task's abort takes its sockets with it; `axum::serve` spawns an independent task per accepted connection that the abort cannot reach | `crates/daemon/src/http/listener.rs:438` |

### 5.2 Claims that have gone stale

| # | what | where |
|---|---|---|
| L9 | **`ioctl::call`'s one SAFETY comment covers ten ioctls and asserts "the struct holds no pointers".** `v4l2_buffer`, which `call` carries for `QUERYBUF`/`QBUF`/`DQBUF`, has `m.planes` — a `__user` pointer the v4l2 core reads. The code is correct for the single-planar API this build uses; the *obligation the comment discharges* is not the one the struct has, and rubric B10 says a false safety claim is a defect even when the code works | `crates/backends/v4l2/src/sys/ioctl.rs:615` |
| L10 | **Ten union-arm field offsets in `sys::decode` are transcribed integers** under a header stating *"Offsets are derived, never transcribed"*, which design §2.5 also requires | `crates/backends/v4l2/src/sys/decode.rs:245` |
| L11 | **Thirteen kernel ABI flag bits are hand-transcribed** in `schema::control::KnownFlag` while the bindgen constants they must equal are linked into the workspace and never compared | `crates/schema/src/control.rs:212` |
| L12 | **The T5 surface grew from nineteen methods to twenty-two at P6c and eight prose claims did not follow.** `engine/src/actor.rs:63` (*"T5 is pinned at nineteen methods and a `wch_status` would be a twentieth"* — the stated *reason* the status surface is a library accessor), `daemon/src/server.rs:14`, `:142`, `:2698`, `api/src/wire.rs:37`, `client/src/remote.rs:9`, `client/Cargo.toml:33`, `daemon/tests/web_rpc.rs:36`. The *test* was renamed to `…twenty_two_methods…`; the prose was not. Distinct from the correct "nineteenth **variant**" references, which are about D13's eighteen | eight sites |
| L13 | **`Serving::ready_to_open_url` is a second rendering that yields the secret**, outside every claim `token-comparison-has-one-home.sh` makes — the gate enforces that `expose_secret` appears only in `token.rs` and in tests, and this reaches the same string by another name | `crates/daemon/src/http/listener.rs:382` |
| L14 | **§3.3's structural-gap register has gone stale**: item 8 still says "three cameras" where the corpus holds five committed profiles and E15 validated five, and no item names the gap H1 lived in (a contract asserted only over the fake) | `docs/6-…-v2.md:1398` |

### 5.3 Declarations nothing reads

Rubric A8, four instances.

| # | what | where |
|---|---|---|
| L15 | **D8's `AutoDisabled` state and `SessionEvent::AutomationDisabled` have no producer** in the product; two doc comments claim readers that do not exist | `crates/engine/src/session.rs:76` |
| L16 | **`ChannelSink` and `limits::PROGRESS_QUEUE_DEPTH` have no product reader** — both composition roots bridge the progress seam their own way — and the daemon's doc comment says otherwise | `crates/engine/src/progress.rs:69` |
| L17 | **`AviHeaders::max_bytes_per_sec` is parsed, published and read by no assertion**: the one close-time-patched field the independent reader never confronts, which is what the independent reader is *for* | `crates/imaging/src/avi/read.rs:244` |
| L18 | **The recording path never derives `sink_fidelity`**, so a Y4M destination is ranked as one that passes compressed frames through — two of the field's three producers set it and the third does not | `crates/schema/src/capture.rs:58` |

### 5.4 Tests and gates

| # | what | where |
|---|---|---|
| L19 | **The mutation register's last entry names a mutant that no longer exists.** `AviWriter::declared_interval` is now a pure forwarder with **zero comparison operators** (measured: `grep -c '[<>]=' → 0`); P6d moved the arithmetic — the `> 0` span filter *and* the `frames_written >= 2` guard the comment distinguishes it from — into `crate::video::declared_interval`. `scripts/mutants.sh` checks the register **both ways**, and `imaging/src/video.rs` is in `examine_globs`, so the next run fails in both directions. The register's own header names this failure: *"an acceptance nobody re-checks is how that mistake gets made twice."* Every other entry was checked and still names a live function | `scripts/mutants-accepted.txt` |
| L20 | **The mutation floor has three imaging modules in the state its own config forbids** — `decode.rs` and `photo.rs` in neither `examine_globs` nor the written exclusions, which the file twice calls "an oversight wearing" a decision. One of them is where N130 found three mutants surviving 1381 tests | `.cargo/mutants.toml:257` |
| L21 | **Four of the six pixel transforms have no assertion anywhere in the workspace**; only `Rot90` is pinned. `oriented()` is the one place in the product where an orientation moves pixels | `crates/imaging/src/photo.rs:164` |
| L22 | **The `bytesused` clamp has no test in either direction.** It is the single most safety-critical device-derived-number validation in the workspace and the one design §2.5 and rubric B10 both name by example; `src/sys/` is excluded from the mutation floor by name, so nothing covers it | `crates/backends/v4l2/src/sys/mmap.rs:93` |
| L23 | **`representative`'s capture-node preference is unconstrained**: no test can tell it from `members.first()`, and it is the only thing making a camera's whole `CameraFingerprint` come from its *capture* node | `crates/backends/v4l2/src/enumerate.rs:126` |
| L24 | **The calibration view's index/total assertion is inside an `if let` the missing field skips** — the comment above states the property and the code below cannot fail on its absence | `crates/daemon/tests/web_client.rs:899` |
| L25 | **N31's repair is a per-case-file convention and 200 of 294 fail arms do not use it**, so most inverse arms can still go red for the wrong reason and be reported green | `scripts/gates/selftest.sh:217` |
| L26 | **The fake refuses a mis-sized compound payload and PF:17 says no driver does.** `payload_write`'s length check is asserted by the resemblance suite with the justification "because a driver checks `elem_size × elems`" — which is the opposite of what PF:17 measured on vivid. A fake capability no real device exhibits is, by rubric A9, a bug in the fake | `crates/backends/fake/src/camera.rs:438` |
| L27 | **Two `crates/priv` tests self-skip silently** on "no camera on this host" and "no permission", with no printed line — and one of them is the **only executable proof of §2.13's interlock**, the property that bounds what a root-capable binary may do. AGENTS rule 3's "named, counted skip — never silence" is honoured by `hw_`, `vivid_`, the oracles and R1-web, all of which print `SKIP:`; `.config/nextest.toml`'s `success-output` list names `oracles` and `web_browser` and not this binary. `g6 'package(webcam-handler-priv)' selects 19 test(s)` — the selection is counted; the vacuity is not | `crates/priv/src/modules.rs:491`, `:529` |
| L28 | **The panic/indexing lint set AGENTS calls "clippy-enforced" is missing from two product crate roots**, and no gate walks for its presence — so the claim is true of most of the workspace by hand-copied attribute rather than by construction | `crates/engine/src/lib.rs:36` |

### 5.5 Rendering

| # | what | where |
|---|---|---|
| L29 | **`IllegalTransition`'s `Display` garbles every caller whose `op` is a sentence.** The template `"cannot {op} from state {from}"` was written for D8's shape (`op: "select"`, which reads) and the variant now has ~11 producers across five crates, most putting a multi-clause instruction in `op`. **Measured** through the shipped binaries: *"cannot write a photo to …/x.tiff; this build writes .jpg, .png, .ppm **from state unwritable_extension(tiff)**"* and *"cannot honour --profile: … which drives a camera in this process **from state webcam-handler-client is a client**"*. `every_kind_renders_something_a_human_can_act_on` checks non-empty, no `{`, and `len > 12` over `Error::sample`, whose `IllegalTransition` sample is the one shape that reads. Three `cli-core` producers are unreachable from a command line (clap's `ArgGroup` refuses first — measured) and are excluded | `crates/schema/src/error.rs:278` |


### 5.6 The command surface, the fake and the gates

| # | what | where |
|---|---|---|
| L30 | **`--duration` under one millisecond truncates to zero** and records a header with no frames, reported as success. The same function *refuses* a value too large, on the argument that saturating "would turn a typo into the longest recording this build will refuse" — the small side got no such treatment | `crates/cli-core/src/lib.rs:736` |
| L31 | **`Transport::close`'s claim that it is "the ordinary end of every connection this binary opens" is false.** jsonrpsee reaches it only from its spawned `send_task`; `Remote`'s fields drop in declaration order, so the current-thread runtime is torn down before that task is polled again and the close frame is never sent | `crates/client/src/transport.rs:200` |
| L32 | **`clap_complete` and `clap_mangen` are pinned with no consumer**, and three prose sites — including §2.8's registry line and §6's tree — say `xtask` emits completions and man pages. Neither appears in `Cargo.lock`. A pin with no consumer in the table AGENTS calls the registry | `Cargo.toml:53` |
| L33 | **`oracle-rung-accounting.sh`'s header claims its fixtures are derived from `testkit::oracle`'s own line shapes; they are hand-transcribed `printf` literals in the predicate.** docs/9's derived-population rule, asserted and not applied — and `mutation-verdict.sh` next door shows the shape that would apply it | `scripts/gates/oracle-rung-accounting.sh:45` |
| L34 | **`RestorationClaim::account_for` produces a decline and emits it in one call**, and its only two unit tests drive it with cameras that do not exist. N121's repair — "a decline is *data* first and a line second" — was applied to `OracleReport` and not to this type | `crates/testkit/src/battery.rs:1737` |
| L35 | **`FakeCamera::next_frame` consumes three fault-menu entries on every call**, before it knows whether it will produce a frame, so a one-shot scripted fault can be removed from the queue by a call that reported a different one — a scripted claim that silently never fires. Rubric A9's fault-menu half | `crates/backends/fake/src/camera.rs:584` |


### 5.7 The web client

| # | what | where |
|---|---|---|
| L36 | **`recording.watch`'s in-flight answer is written after `stop()`.** `poll()` writes to the node *before* consulting `view.stopped`, so a `wch_record_status` answer already in flight lands in `#recording-status` after the handle was retired — the previous camera's sentence under the new camera's picture, in the element `index.html` gives one writer precisely so it cannot be about something else | `crates/web/assets/recording.js:74` |
| L37 | **`crates/web/src/lib.rs`'s header counts the client's files and modules, both counts are stale, and the same file states a different count forty lines down** — "ten files … eight ES modules" in the header against "one page, one stylesheet and nine modules" in `content_type`'s doc, over an `assets/` holding eleven files and nine modules. `recording.js` landed at P6c and the header did not move. Nothing can go red on either number | `crates/web/src/lib.rs:11` |
| L38 | **The RPC helper has no liveness of its own.** `call` refuses on a *closed* socket (N96's H5 repair) and there is no timeout and no ping, so a connection severed without a FIN leaves `readyState` at `OPEN` indefinitely: every call parks, the banner still reads `connected`, and `#photo-status` reads "taking a photo …" until the tab is closed. That is the shape `rpc.js:22-23` claims cannot happen. **This one is an owner question rather than a unilateral fix**: the honest alternative is to say in the header that liveness is D11's loopback posture and the non-loopback cells accept the hang | `crates/web/assets/rpc.js:188` |

---

## 6. Design deviations

The review walked D1–D13, T1–T6, E1–E6, §2.10's thirteen single-copy homes, §2.8's edge list and
licence posture, §1's non-goals, §5's hardware and privacy discipline, and §3.3's register. Seven
deviations survived verification. They divide, as the brief asked, into the ones with a reason a
reader can verify — which have been written into the implementation notes — and the ones without,
which are here. One runs the other way and is §6.3.

### 6.1 Justified — landed in the implementation notes

Two new entries were written, in the tree's own voice, recording the deviation and its reason:

- **N132 — T2 ships nine methods and design §2.3 declares eight, and the ninth is the one N83
  needed.** `Camera::streaming` landed with D12's 2026-08-12 amendment; the answer must come from the
  device rather than from a flag a layer above keeps, and it returns the negotiated stream because
  that is what `while_suspended` must *restore*. N7's fifth method on T1 got a Changes-from-v1 row
  because N7 predates the v2 revision; N83 postdates it, so the growth has had nowhere to land. The
  cost is self-announcing — a backend written against the document does not compile — which is why
  it is documentation debt rather than a defect.
- **N133 — §2.8's dependency registry has drifted three ways, and each is a different kind of
  drift.** Three adopted crates the registry never learned (`tower 0.5.3` and `tokio-stream 0.1.19`
  on the daemon, `caps 0.5.6` on the privileged helper); one version it states and the manifest does
  not use (`tower-http 0.7` against a pinned `0.6.11`); and one measurement quoted as the api wall's
  evidence that has stopped being true (*"no axum, no hyper, no tower in its tree"* — `cargo tree`
  now lists `tower 0.5.3`, `tower-layer`, `tower-service`). **The wall itself holds**:
  `dependency-walls.sh` checks `axum`/`hyper`/`tower-http` and none is present, so what drifted is
  the sentence offered as evidence, not the rule. Each adoption *was* decided correctly — the
  manifest carries the reasoning, which is the 2026-08-09 owner ruling working as written. The
  `caps` row is the one worth naming twice: §2.13 says every dependency the blessed helper links is
  attack surface inside a root-equivalent boundary, and `priv::modules` declines to link the
  product's crate graph on exactly that ground — so the crate with the strongest reason for its
  dependency list to be reviewed is the one whose single third-party edge the registry does not
  list.

### 6.2 Unjustified — reported here

| deviation | decision it contradicts | where |
|---|---|---|
| **An explicitly named format is substituted, and an explicitly named size resolves to the largest mode** | D5, amended: *"An explicit request still wins: a caller that names a format and a size gets them or a typed refusal"* | §3 H1, H1b |
| **A sweep interrupted before its first sample is unreachable by every verb**, and `lifecycle::recover` — the durable repair — never touches control status | D8's state machine has no backwards arrow; §6's crash story; N24's own *Retires when* | §3 H2 |
| **The D3 probe restores itself against an empty pair set**, so D4's automation-first ordering degenerates to alphabetical for the one code path whose job is discovering automation | §2.10: pairing has one home; D4's ordering is "load-bearing and tested" | M13 |
| **`terminate-holder` has a wire method and no command surface** | §1.1's operations map and §5 both call it "a distinct explicit command", and §1.1's premise is that an agent issues one call where the skill teaches a sequence | *refuted as new* — N48's closing paragraph already records it: *"Not landed, and named so the absence is counted."* Reported here only because §1.1's map still reads as though it shipped |
| **`AutoDisabled` and `SessionEvent::AutomationDisabled` have no producer**, so D8's vocabulary has a state the product cannot enter | D8's per-control vocabulary | L15 |
| **§3.3's register is stale** and has no item for "a contract asserted only over the fake" | rubric rule 4: the register is regenerated, not accreted | L14 |

### 6.3 Where the design is wrong and the code is right

One deviation runs the other way, and it is on a compatibility contract in the single home §2.10
names for it.

**D10 states the wrong JSON-RPC namespace.** `docs/6-…-v2.md:568` reads *"Methods (namespace
`webcam-handler-cli`): `list`, `info`, …"*. The wire is `wch`:

- `crates/api/src/lib.rs:108` — `wire_surface! { namespace = "wch"; … }`
- `crates/daemon/src/server.rs:160-183` — `ROUTED` lists twenty-two `wch_*` spellings
- `schemas/webcam-handler-openrpc.json` — 24 distinct `wch_*` method names over 52 lines, e.g.
  `"name": "wch_list"`

`git blame` puts the sentence at commit **`c397f60b`** — N90's rename sweep of 2026-08-13, which
replaced `wch` with `webcam-handler-cli` throughout and caught a *wire namespace* along with the
binary names. **Note N91, written in the same session, is the proof it was collateral**: its
catalogue of the prefix's five kinds of name says *"3. The JSON-RPC method namespace — `wch_*`, from
`#[rpc(namespace = "wch")]` (D10). **This one is a wire break**, and it is already short; a rename
that changed it would invalidate every committed artifact under `schemas/` and every client written
against them."* The note warning that this must not be renamed in passing sits beside a design
sentence where it already had been. `crates/api/src/lib.rs:971`'s own test comment cites the
document as saying what it no longer says: *"D10 says `namespace = "wch"`."*

Only that one sentence moved — the `WCH_*` environment variables, `wchd.sock` and `.wch-bin/` all
survived the sweep intact.

**What it costs**: a client author reading D10 — which is where D10 tells them to read, since §2.10
names it the wire surface's home — sends `webcam-handler-cli_list` and is answered "method not
found". Nothing can go red on it. The reconciler shape it wants is `browser-pins-sync.sh`'s
(note N131, landed two days before this was found): compare the literal in `wire_surface!` against
the sentence in D10, and fail rather than skip when the document names something the macro does not.

### 6.4 What was walked and found sound

Stated because an absence claim has to name where it looked.

- **D1's identity and grouping**: the interface-path key, the fingerprint's fields, prefix
  resolution and the collision rule all match PF:7/PF:13 and the committed profiles. The one
  deviation found is L3's fallback-slug hole.
- **D3's read-back and the three probe rules**: every menu alternative is tried, residue is isolated,
  "off" is recorded per freed control by name. The deviation found is M13, which is about the
  probe's *own* restore rather than about the rules.
- **D4's four-outcome vocabulary and automation-first ordering** in `snapshot::restore`.
- **D6's source-format set and the header-only EXIF splice** — the walk stops at `SOS`, a length past
  the buffer ends the walk, nothing indexes.
- **D9's one atomic-write home**: no state write in the workspace bypasses `write_json_atomic`; the
  lens grepped `serde_json::to_writer`, `fs::write` and `File::create` under the state dir and found
  none. The deviations found are about the contract's *statement* (M10) and its modes (M11).
- **D10's one wire surface** and the D13 code map: 22 methods, 2 subscriptions, one exhaustive
  match, codes contiguous and injective; every variant round-trips with its payload.
- **D11's bind × token matrix**, verified against a live daemon across twenty request shapes (§7.2).
- **D12's exclusive streaming**: the `Box<dyn Camera>` never leaves the actor thread; there is no
  second handle to a node in the workspace.
- **D13's eighteen variants**, `cli_core::exit_code`'s 10..=27 with no gap or duplicate, and the
  failure document's marker and nesting.
- **T4's one command surface** and **T6's purity walls**, re-measured from `cargo metadata` rather
  than trusted from the gate.
- **§1's non-goals**: nothing ships audio, transcoding, a second backend, TLS or a tracking loop.
- **§5's motor rule**: no product path moves a motor without `--allow-motion`.

---

## 7. The absence lists — what was checked and found sound

134 absence claims were recorded, each naming where it looked. This is the load-bearing half of a
review record, and the ones worth carrying forward are below. Nothing here is a claim that a class is
*impossible*; each is a claim that a named population was walked and nothing was found in it.

### 7.1 The unsafe boundary

**Every `unsafe` block in the workspace was read against the ioctl's own kernel contract**, not
against its own comment: `src/sys/{ioctl,mmap,payload,wait,uevent,signal,decode,fields}.rs`. The
findings are two stale *claims* (L9, L10) and one untested *guard* (L22). What was **not** found:

- **No unsound block.** Union-field initialisation for `querymenu`/`query_ext_ctrl`/`ext_controls`
  is correct; the mapping's `Send` impl is justified and its `Drop` unconditional; `bytesused` is
  clamped before slicing; the payload arithmetic (`elem_size × elems`) is checked; every wire
  integer goes through `try_from` and the crate denies the three cast lints.
- **No aliasing defect in the mmap path.** `take_frame` copies at `bytesused` *before* requeue
  (`lib.rs:818-848`, argued in its own doc), so the `&[u8]` is only ever taken while the buffer is
  userspace-owned. Verified independently by the reviewer.
- **No hand-declared kernel struct.** Every layout comes from `v4l2-sys-mit`'s bindgen output; the
  transcription found (L10) is of ten *offsets*, not of a struct.
- **The `ENOTTY`/`EINVAL` terminator (PF:15) is applied at every index-walked enumeration**, through
  the one `call_enumerating` home.

### 7.2 The credential and the transport

**Twenty request shapes were driven against a live daemon** and every answer matched D11, N82 and
N93–N95:

| shape | answer |
|---|---|
| anonymous `/`, `/app.js` | 200 — assets are open-source code (N82) |
| anonymous `/preview`, `/rpc` | 401 with `WWW-Authenticate: Bearer` |
| anonymous `/nothing-here` | 404, not 401 — N82's stated price |
| `Sec-Fetch-Site: cross-site`, any path incl. assets | 403 |
| `Sec-Fetch-Site: same-site` | 403 |
| `Sec-Fetch-Site:` an unrecognised value | 403 — errs closed |
| `Sec-Fetch-Site: same-origin` / `none` | admitted |
| `Origin: null` | 403 |
| `Origin:` own authority / uppercase scheme | admitted — the scheme compare is case-insensitive |
| `Origin: https://…` (wrong scheme) | 403 |
| `Origin: http://127.0.0.1.` (trailing dot) | 403 |
| cross-site **with a valid bearer token** | 403 — provenance runs before the gate, as documented |
| `Host: evil.example` + matching `Origin` | admitted — **the DNS-rebinding residual N93 names in writing** |
| every served response | carries `referrer-policy: no-referrer` |

The gate's own policy — every credential presented must verify, and at least one must be presented —
was read at `admits` and matches N74: `get_all` on both forms, a non-short-circuiting fold, no
percent-decoding, `?token` with no value counted as a credential that fails. **No new hole was found
in the credential layer.** The two findings here are about a *sink* (M25) and a *method* (M23), and
one is about the anchor the origin rule reads (M24).

### 7.3 The web client

- **No `innerHTML`, `outerHTML`, `insertAdjacentHTML`, `document.write`, `eval` or `new Function`
  anywhere in the ten ES modules** — checked by content, not by convention; `dom.js`'s header is why,
  and it is true. Device-supplied text reaches the DOM through `textContent`.
- **No external fetch**: no CDN, no npm at runtime, every reference resolves to an embedded asset.

### 7.4 The store and the state directory

- **No state write bypasses `write_json_atomic`.** `serde_json::to_writer`, `fs::write` and
  `File::create` were grepped under the state dir and none appears outside the one home.
- **No path traversal.** Both derived components of a session directory go through the one slug
  transform and neither can be empty; a hand-edited `Sample.photo` was traced and cannot escape the
  session tree.
- **Lock discipline holds**: every mutating method takes `&StoreLock` by signature, and the guard
  releases on drop.
- `parse_log` was walked against hand-built byte sequences — empty, one newline, whitespace-only,
  blank middle, torn tail, torn tail with terminator, invalid UTF-8 in both positions. The defect
  found (M9) is in `append_log`, not here.

### 7.5 What the review looked for and did not find at all

- **No unsound `unsafe`, and no memory-safety defect of any kind.**
- **No availability-to-capability conversion in the error registry itself.** Every D13 variant's
  producers were traced; the two conversions found are one layer up (M6's enumeration abort) and one
  layer of *rendering* (M19's message), which is E3's own distinction.
- **No second copy of a §2.10 law.** All thirteen homes were checked for a duplicate and for a
  bypassing caller; the deviations found are a *bypass* (M13) and a *statement* (M10), not a copy.
- **No frame bytes in the repository, in a log, or in an error message.** Every path frame bytes can
  take was traced. The two findings are a `Debug` derive on two photograph-holding types (a census
  N36 undercounted) and a file mode (M11).
- **No place where the D8 state machine's own refusals can be reached around.**
- **No unbounded loop over device behaviour without a `limits` constant behind it**, other than M12's
  caller-supplied deadline.
- **No gate predicate that cannot go red.** `selftest` runs 62 pass arms and 294 fail arms and the
  reviewer watched it green. The finding (L25) is that most fail arms do not use the *stronger*
  form N31 introduced, not that any of them is vacuous.

---

## 8. Performance

The consumer is an agent photographing a device under test, continuously and unattended, plus
recording and the occasional sweep. Measurements, not estimates, where they were cheap.

**Photo latency is the settle policy's, not the implementation's.** Five sequential photos over the
daemon, fake backend, debug build: **2.40–2.49 s** each, for a 3840×2160 MJPEG. Almost all of it is
the ten-frame settle plus the fake's own eleven 4K JPEG encodes per photo. PF:9 measured 2.0 s on
the real OBSBOT including the same settle and 0.48 s on the Chicony. **Nothing in the plumbing is
the cost**, and the default that dominates it is a correctness choice (PF:11) that should stay one.

**One copy per frame on the capture path**, and it is unavoidable: `mapping.bytes(used).to_vec()`
(`lib.rs:832`), because a `Frame` outlives the buffer by design. The muxer then writes `frame.bytes`
straight to the sink with no further copy. A preview attached to a take costs one clone per frame
(N117), which at 4K30 is ~15 MB/s of memcpy — immaterial here.

Three costs are real enough to name, all LOW:

| # | what | where |
|---|---|---|
| P1 | **`describe(id)` re-runs the whole control enumeration to answer about one control** — one `QUERY_EXT_CTRL` per control, a `QUERYMENU` sweep per menu control and a `G_EXT_CTRLS` per readable control. Both `get` and `set` call it, and a **guarded write pays the walk again per planned write**. On the 77-control vivid device that is a sweep's inner loop | `crates/backends/v4l2/src/lib.rs:286` |
| P2 | **A session's disk writes are quadratic in its sample count.** Every sample clones the whole `Session`, serialises it and publishes it through `write_json_atomic` — temp file, `sync_all`, rename, `fsync` of the parent. At `MAX_SWEEP_SAMPLES = 256` the last write carries 255 samples' worth of document, and each one costs two fsyncs | `crates/engine/src/lifecycle.rs:545` |
| P3 | **`Base64Bytes::serialize` materialises a second full-size buffer** whose only consumer is `serialize_str`, which immediately copies and escapes it again. A `ReturnBytes` photo therefore holds the frame, a 4/3 base64 `String`, and serde_json's growing buffer at once | `crates/api/src/photo.rs:92` |

P2 is the one worth acting on: it is on the long path, it is `fsync`-bound rather than CPU-bound,
and the fix (append samples to `log.ndjson` and rewrite `session.json` on a coarser schedule) is
already the shape D9 chose for the log.

**Nothing else measured is O(n²) in anything that grows**, and `rank_formats`' three passes over the
format list are microseconds at vivid's 83 formats × 747 size entries.

---

## 9. Reconciliation — what this review says about the rubric

Rubric Part E's meta-rule. Four observations, and the first is the useful one.

### 9.1 Three of the four HIGH findings are recurrences of classes the rubric already names, and the reason is the same each time

- **H2 is rubric A4's second half**, which reads: *"a state machine that a transient failure leaves
  in a state no verb can leave has turned an unplug into a permanent refusal without ever mistyping
  an error. For every state a failure can strand something in, name the transition out."* That
  sentence was **added to the rubric because of N24** — this exact state, one layer up.
- **H1 is rubric A9 / doctrine E5**, whose worked example in design §2.3 is the P2 review's
  `Bytes`-at-a-scalar finding: *"the fake refused the input the real backend mis-dispatched — a
  divergence is a finding against whichever side is wrong, and this time that was the real one."*
  Same doctrine, same direction, three phases later. **And it is not alone**: M29 is the *same
  §2.3 contract note* — "Dispatch belongs to the descriptor" — with the fake on the wrong side this
  time, and M30 is the corpus that would let either be caught. **Three instances of the
  stand-in-versus-real family in one review**, all of them in the seam E5 was written for.
- **H3 is rule 6's addendum [S:N10]** — *a selection counted but counting the wrong thing* — which
  the reconciliation record has already logged four times (G2 twice, G3 twice) and which the rubric
  calls "the row that keeps finding them is worth its space". It is worth its space again.

G4's reconciliation already answered "why did a written row not fire" with three parts, none of them
"the row is badly written", and all three apply again. This review can add a fourth that is
mechanical rather than human:

> **A rubric row names a class; only a walked *population* finds an instance of it.**

All three classes have a population, and none was pointed at them. For H1 the population is the
backend conformance battery, which §2.11 step 4 calls "the definition of done" — and
`arm_stream_lifecycle` constructs only `StreamRequest::default()`, so no arm of it can express *any*
explicit-request contract, on either backend. One arm that names a format the enumeration lacks
would have caught H1 the day the fake grew its guard. For H2 the population is the SIGKILL suite,
which exists, and drives `lifecycle::sweep_write` instead of `calibrate::run` — so it never reaches
a control the crash story is about.

For H3 the population is the *call sites*, and the predicate walks files instead — the population
was chosen correctly in prose and implemented one level too coarse.

Where this project *does* walk a population — `ErrorKind::ALL`, `LockProtocol::ALL`,
`SUBSCRIPTIONS`, `Program::ALL`, the corpus loader, `closed_vocabulary!` — the class is closed by
construction and this review found nothing in it. That contrast is the finding: **the rows that
work are the ones with an `ALL` behind them.** L21 (four of six transforms unasserted) and L26
(a fake capability PF:17 contradicts) are two more populations waiting for the same treatment.

### 9.2 Part C's named smell is now at five instances, and the fourth candidate was the reviewer's own

*A test whose fixture cannot exercise the rule it pins* was added at G3 and found again at G4. This
review found three more, all of them fixtures that sit just outside the case they are named for:

1. **H2** — the crash suite never puts a control into `Sweeping`, so the SIGKILL rung that exists to
   prove design §6's crash story cannot reach the state that story is about.
2. **L1** — `align_down`'s extremes test uses `min: i64::MIN`, the one range where the
   non-saturating subtraction is unreachable, under a comment claiming to test saturation.
3. **M20** — `the_flags_a_client_cannot_honour_are_refused_before_the_socket_is_touched` drives
   exactly the three argument vectors that dodge `required_if_eq`.

All three share a shape worth naming beside the smell itself: **the fixture is one parameter away
from the case**, and the parameter is the one a reader scanning for "does this test the rule" does
not look at — a range's `min`, a session's uuid, an argument vector's length.
The fourth instance the review *thought* it had — `store_faults.rs` pinning the wrong side of the
torn-log rule — **was itself refuted**, and the refutation is the more useful result: N12's list of
seeded mutants shows the assertion pins the side the project chose on purpose. A reviewer reading a
test that asserts a behaviour they dislike, and calling the fixture wrong, is the same error one
level up. §4.9 records it.

### 9.3 A class the rubric has no row for: the message *is* the payload, and it goes stale

Five instances in one phase, two of them already recorded as notes:

| | |
|---|---|
| N123 | `control_inactive` told callers to use `--guarded`, a flag the surface never had |
| N129 | `format_unsupported` told callers the *camera* offers formats it has never had |
| M18 | the guide's `format_unsupported` remedy names `--size`, which cannot produce it |
| M19 | `busy` renders "an unidentified process" for a holder this daemon knows precisely, and the guide promises `--wait` for a refusal `--wait` cannot reach |
| L29 | `IllegalTransition`'s template garbles every caller whose `op` is a sentence |

N129 states the law in prose — *"a D13 message is not prose beside the payload, it is the part of the
payload a caller reads first, and it goes stale exactly where a variant grew a second caller"* — and
it is not in the rubric. It should be, with the mechanical form N129's own repair used: **test the
claim, not the wording.** `a_container_refusal_never_says_the_camera_offers_a_format_it_has_never_had`
asks the binary what the camera enumerates, asks it again for the refusal, and refuses a message that
attributes one to the other. That shape generalises to every row of the guide's `Do` column that
names a flag: drive the flag, require the kind.

### 9.4 What the harness itself did

**Self-refutation does most of the filtering, and independent verification does most of the
sharpening.** Across the twenty-two lenses, **204 candidates died in the lens's own refutation pass**
before anything was reported, against 86 reported — a 2.4:1 reduction bought before a second agent is
spent on any of them. The verifiers then returned 86 verdicts of which **only 13 were outright
refutations and 35 were narrowings**: the second stage is not mainly a filter, it is mainly an
editor, and the review's severity distribution moved under it — eleven HIGHs reported, four carried
(§2). Both stages earn their place and they earn it differently, which is the argument for keeping
them separate rather than collapsing them into one "be careful" instruction.

**Concurrency was safe because the lenses only read.** Repairs, when they come, want isolated
worktrees for N98's reason; this review changed nothing but `docs/`.

---

## 10. Recommended order of repair

Ordered by what a defect costs the primary consumer, not by severity label.

1. **H1** — an explicit request that is silently substituted is the one defect on this list that
   corrupts the product's central claim (comparability across time) while reporting success. Fix
   `choose` so both backends inherit the refusal, then add the battery arm so the next backend
   cannot forget. The `--size` half wants an owner's decision — smallest offered, or refuse — and the
   guide's row moves with it (M18). **Do M29 in the same pass**: it is the same §2.3 contract note
   with the fake on the wrong side, and one battery arm can cover both.
2. **H2** — a stranded session is unrecoverable without a human editing JSON, and the primary
   consumer has no hands. `lifecycle::recover` is the right home and the repair is small.
3. **H4** — one line, and until it lands every camera the owner looks at through the web client is a
   camera the agent will meet as `Busy`. It is the cheapest repair on this list and it is third
   because the two above it are silent while this one is at least visible on the daemon's feed count.
4. **M9** — H2's shape on the log: heal the tail at append time so a crash plus an append cannot
   brick `calibrate status`. Note that the *refusal* is settled law (N12) and must not be touched;
   the repair is at the write, not at the read.
5. **M19 + M18 + L29** — one pass over what the registry *says*, because an agent obeying a wrong
   remedy is worse than an agent given none. Land N129's test shape (§9.3) with them.
6. **M1, M2, M3, M4** — the recording interlocks. M1 first: it is the one that loses frames.
7. **M25, M11** — the two exposures. Neither is live on the owner's machine today; both are cheap.
8. **H3** — the gate is three lines and it guards a non-negotiable rule; leaving a gate that reports
   a wrong number under a right label is worse than having no gate, because the number is what CI
   prints. Land the missing selftest arm with it.
9. **L19** — re-address the mutation register before the next `just mutants`, or the run fails for
   bookkeeping and the failure will be read as noise.
10. **M23, M26, M27, M24, M31, M32, M33** — the stated-but-unenforced properties, in the daemon, the
    battery and the page. M31 is the one that can leave a real camera moved; M32 is the one that can
    write to the wrong camera.
11. **L1 + L2 together** — the overflow and the profile that decides what it does. They are one
    decision.
12. Everything else, as the files are touched for other reasons. **L12, L14, L37 and the two notes'
    subjects (§6.1) are documentation debt for the next design revision** rather than work.

**Every repair on this list lands with its gate** — AGENTS rule 1 — and this document names the red
test each finding lacks so that requirement is a lookup rather than a design exercise.

---

## 11. What this review did not reach

Named up front, in §3.3's spirit, because a review's absences are as much a part of its record as
its findings.

1. **No hardware rung was run beyond what `just ci` ran.** R3 is `#[ignore]`d by design and R2
   loads kernel modules; neither was executed. Every finding about the V4L2 backend is therefore a
   code-reading finding, and **H1 in particular is confirmed by reading and by the fake's
   contradicting guard, not by a camera**. `hw_a_format_the_camera_does_not_offer_is_refused_rather_than_substituted`
   is both the repair's test and the measurement this review owes.
2. **The mutation floor was not run.** L19 and L20 are findings *about* it that do not need it run;
   what a run would add is the first workspace-scope result over the P6b/P6d additions, which the
   config's own header says is owed.
3. **H4's cost on a real rig is inferred, not measured.** The leak itself is confirmed three ways —
   the lens measured it against the pinned Chromium, the verifier **re-ran the probe independently**
   rather than taking it on trust, and the reviewer read the function — but all of that was against
   the fake backend. What a leaked preview costs when the cameras are real, and how quickly clicking
   around exhausts `PREVIEW_MAX_VIEWERS_PER_CAMERA`, is what the missing R1-web arm would settle.
4. **Firefox and Safari** are unexercised by anything, deliberately (§2.7, §3.3 item 7).
5. **Multi-host, multi-kernel and multi-camera-bandwidth behaviour** remain what §3.3 items 3 and 8
   say they are. Nothing in this review moves them.
6. **Concurrency findings are argued from code and, where attempted, from a stated load.** M2 was
   attempted twice and not reproduced (§4.10). No claim here rests on a green run treated as proof.
