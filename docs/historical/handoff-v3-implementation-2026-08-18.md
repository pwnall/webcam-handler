# Handoff — implementing the v3 document set, 2026-08-18

Written for the session that picks this up. **Obsolete once P7e/P8d/P9d close**; delete it then
rather than maintaining it. It lives under `docs/historical/` because it is a note about a
session, not about the product.

## Where the work stands

The v3 set (docs/12–16) is **adopted** and every sub-milestone docs/13 plans for P7–P9 has
landed, except the three *gate closes*. Nine commits, all pushed to `main`:

| Commit | What |
|---|---|
| `796babb` | P7a — the doc-set swap, `dependency-registry-sync.sh`, the `toml` disposition |
| `561c91c` | P7b — camera selectors (D14), the R3 twins run at three cameras (E19) |
| `7dd0c3e` | P7c/P7d/P8a/P8b/P8c — the projection and `profile compare`, `engine::facade`, stream stats, `imaging::compare`, D19's hermetic contract |
| `b3aa781` | P9a/P9b — the two-pane workbench shell, `/session-photo` and its route partition |
| `a907975` | P9c — the human-driven calibration flow |
| `1ddf472` | the preview showing the daemon's own refusal (N265); docs bookkeeping |
| `363beea` | two rustdoc links at private neighbours |
| `57b0d5e` | E20 — the whole R3 rung re-run at three cameras |
| `37e1633`, `d30a293` | P7d's other half: the CLI rebuilt on the facade, its gate, its criteria |
| `ccb74f7` | P8b's other half: the `photo diff` verb |

**State at `ccb74f7`**: `cargo nextest --workspace` is **1635 tests, all passing**; all **39**
gate predicates green (`run-all.sh`); `cargo fmt --check`, `clippy -D warnings`, `cargo doc -D
warnings`, `typos` and `cargo machete` all clean; `just gate-g8` and `just gate-g9` **PASS**; the
browser rung is 28 claims / 259 assertions.

## What is left, in the order I would do it

1. **`just gate-g7` wants one clean run, and nothing else is known to be wrong with it.** It was
   run twice this session and failed both times *for reasons that were not the gate*: the first
   run overlapped another agent's edits to `cli-core` (so `agent-guide-current` and `cli-parity`
   were red against a half-edited tree), and the second overlapped my own commits, which
   `selftest.sh` correctly reported as **"an arm changed the checkout"**. Run afterwards on a
   settled tree, `./scripts/gates/selftest.sh` is **PASS — 39 predicates, 94 pass arms, 411 fail
   arms, "the checkout is as the arms found it"**, and `run-all.sh` is green. So the two rows
   that failed are accounted for and `just gate-g7` should pass; run it once with nothing else
   touching the tree, and if it does not, the failure is real and new.

   The lesson is worth carrying: `selftest.sh` copies the tree per arm and compares before and
   after, so **any edit during its ~18 minutes is reported as a problem**. Do not commit while a
   phase gate is running.
2. **The three gate closes** — docs/13 P7e, P8d, P9d. Each is: rows counted (`just gate-gN`), a
   review session in its own context, fixes, an evidence entry, and **the reconciliation written
   into docs/14's record**. That last one is the meta-rule and G5's skipped reconciliation cost
   five recurrences one gate later; docs/14's record section is still empty and says its first
   entry will be G7's. The shape to copy is docs/historical/8-…-v2.md's own record
   ("**G1 (P1, four confirmed defects).** Predicted by the rubric: …").
3. **The adversarial review returned, and most of it is unrepaired.** Five read-only lenses and
   an independent verifier: **15 confirmed, 8 narrowed, 1 refuted**, every confirmation
   reproduced rather than argued. Three were repaired before this session ended (below); the
   rest are the next session's first job and are listed in full further down. The run is
   `wf_8d8d970a-90c`; its script and `journal.jsonl` are under
   `~/.claude/projects/-home-pwnall-workspace-webcam-handler/9245be84-6ece-4c2c-ac45-b0f4e7d5df31/`.
   Note that the lenses read a tree two commits older than `ca24f14` — the facade rebuild and
   `photo diff` landed after they started — so re-verify each finding against `HEAD` before
   repairing it, and re-run the workflow afterwards.

   **Repaired in `ca24f14`'s successor commit**, because two were regressions this session
   introduced and one was a test that could not fail:
   - `preview.js` — the failure-path probe `fetch` held a **second MJPEG stream open for the
     life of the tab** whenever it succeeded, spending one of four viewer slots with nothing on
     screen. It now cancels the body on a 200. Measured by the reviewer in this rung's own
     Chromium.
   - `calibrate-flow.js` — the flow **wedged permanently on a motorized control**: the page
     sends no `allow_motion` (§5 says it must not), the sweep is refused, no samples are
     written, and `nextControl()` returned the same slug forever. On the OBSBOT profile that is
     the thirteenth control in the queue. The page now remembers what it was refused and offers
     the next one, saying so.
   - `faults.rs` — `Fault::FrameGap`'s distinguishing claim (a lost run, not a stall) was
     asserted as `one_interval > 0`, which is true of a stall too; deleting the fake's clock
     advance left the workspace green. Now asserted against the interval the stream really has,
     and proven red on that exact deletion.
4. **`just ci` end to end, and the mutation floor** (`just mutants`) — both deferred at the
   owner's instruction and neither run this session. The floor is hours; N251's price sheet
   applies.

## Three findings this session produced that are the owner's, not mine

- **N261 — D5 ranks greyscale above colour.** `corpus/profiles/vivid.json` is the first committed
  device offering both, and an unspecified `photo` on it takes a **monochrome** 4K frame, because
  `Lossiness::Lossless` outranks `ChromaSubsampled` and `GREY` is lossless. The note states two
  readings and declines to choose. **Nothing was changed**; `RANKED_DEFAULT` records the measured
  answer. This wants a ruling.
- **N251's mutation-floor default** and **N238's process-failure kind** are still open from before
  and untouched.
- **The `photo diff`/`profile compare` document verbs are not in the guide's walkthroughs**, only
  in its derived tables. If the primary consumer should be *taught* them, that is prose in
  `xtask/src/guide.rs`.

## Things that will bite you, learned the hard way

- **`ssh-agent` hangs.** `git push` stalls at publickey against the VS Code forwarded agent
  (`$SSH_AUTH_SOCK=/run/user/1000/vscode-ssh-auth-sock-*`). Every push in the second half of this
  session used:
  `env -u SSH_AUTH_SOCK GIT_SSH_COMMAND='ssh -o BatchMode=yes -o IdentitiesOnly=yes -i ~/.ssh/id_ed25519' git push origin main`
- **`selftest.sh` takes no argument.** It always runs all 39 predicates (~18 minutes). To drive one
  predicate's arms, source `lib.sh`, set `GATE`, source the case file and call each arm.
- **`rust-embed` with `debug-embed` bakes `crates/web/assets/` at compile time.** After editing an
  asset you must rebuild `webcam-handler-web` *and* `webcam-handler-daemon` before a running
  daemon serves it. Symptom: a screenshot that stubbornly shows the old CSS.
- **A read-only review lens left a mutation in the tree** (`stream_stats.rs`'s
  `.filter(|delta| *delta > 0)` became `.filter(|_delta| true)`). It was caught by `git diff`
  before committing, reverted, **and it was a real gap** — two frames stamped at the same
  microsecond had no arm. The test that closes it is
  `two_frames_at_one_instant_span_no_interval_either`, and it is proven red on that exact
  mutation. **Check `git diff` after any review workflow.**
- **Do not `git stash` while a subagent is editing.** I did it once for thirty seconds and had to
  warn the agent to re-verify. Use explicit pathspecs on `git add` instead, and accept that a
  commit's untouched paths equal `HEAD`.
- Driving the workbench by hand: `XDG_RUNTIME_DIR` must be **short** (a Unix socket path is capped
  at 107 bytes, and the scratch dir this session was handed is too deep) and mode `0700`. I used
  `/tmp/wchd-design/`.

## Where the plan's shape was not followed, and why

`7dd0c3e` carries five sub-milestones. docs/13's execution record says so and gives the reason:
their generated artifacts are one JSON bundle and one guide between them, so any boundary
splitting them is red on `schema-artifacts-current.sh` at every commit but the last. The sizing
lesson (N54, "size by story") still stands; this is its cost, paid once and recorded rather than
smoothed over. If you split further work, split it where the *artifacts* split.

## The review's confirmed findings, unrepaired

Verbatim enough to act on; the reproduction for each is in the run's `journal.jsonl`.

1. **`compare::read` measures one picture two ways** (`imaging/src/compare.rs:140`) — JPEG and
   PNG go to luma through `image`'s Rec.709 `to_luma8`, the hand-written Netpbm path uses the
   BT.601 coefficients the rest of the crate uses. Same scene, two formats, different metrics.
2. **`compare::read` ignores EXIF Orientation** (`:141`) — which this build's own verbatim JPEG
   path *writes*: `photo --format jpeg --transform rot90` tags the file and keeps 2592×1944,
   the PNG of the same transform is 1944×2592, and `photo diff` refuses them a similarity score
   for differing dimensions. D17 says nothing about orientation, so this is a gap, not a ruling.
3. **The facade gate is one brace wide** (`facade-is-the-composition.sh:332`) — its walk matches
   `engine::[a-z_]+`, so `use engine::{pairing, write};` re-opens the bypass and the gate passes
   byte-identically. Reproduced both ways.
4. **`calibrate-flow.js`'s `refresh()` has no fence** (`:291`) — the N154/N156 defect closed in
   every sibling module: a late `calibrate_status` answer paints under whatever the operator is
   looking at now. `app.js:427` and `calibration.js:76` are the house pattern, two files away.
5. **The narrow-viewport browser claim asserts neither stacking nor scrolling**
   (`client.spec.mjs:1832`) — it checks the preview's box is on screen, which it is in both
   layouts.
6. **N262's stated measurement conditions contradict the harness** (`implementation-notes.md`) —
   the before/after numbers were taken at 1440×900 by hand and the rung pins 1280×720; the note
   reads as if they were the same run.
7. **Stale counts in three places** (`phase-criteria.tsv:244` and two others) — the g9 row still
   names the browser rung's pre-P9c figures.
8. **D16's and D17's pure cores are in neither `examine_globs` nor the "owed" paragraph** of
   `.cargo/mutants.toml` — which that file's own law calls an oversight wearing one.
9. **`/session-photo` has no proof its only consumer can reach it** (`credential.js:122`) — the
   page's five URL literals are reconciled with nothing on the daemon side.

Narrowed but worth reading: `wall_clock_skew_us` has no assertion anywhere; `RecordReport.stats`
is defaulted so its absence is indistinguishable from a zeroed take; the D14 vocabulary is
hand-written in two places besides `SelectorScheme::ALL`; `ProfileComparison`'s two fail-closed
verdicts are Rust-only and a `--json` consumer must rebuild the conjunction the code documents as
unsafe; the corpus format-tree arm's second half sits inside an `if let` whose false branch cannot
go red; and the sweep-time pane D20 describes does not exist yet (scheduled work the plan allows
to split, but the module header describes it as if it were there).