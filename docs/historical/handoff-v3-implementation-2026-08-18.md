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

1. **`just gate-g7`.** It was last run mid-flight while another session was editing `cli-core`,
   and failed on exactly two rows — `run-all.sh` and `selftest.sh` — because `agent-guide-current`
   and `cli-parity` were red against a half-edited tree. A re-run was started at the end of this
   session (`/tmp/wchd-design/g7b.log`, which will not survive a reboot; just run it again).
   **Expect it to pass**; if it does not, the failure is real and new.
2. **The three gate closes** — docs/13 P7e, P8d, P9d. Each is: rows counted (`just gate-gN`), a
   review session in its own context, fixes, an evidence entry, and **the reconciliation written
   into docs/14's record**. That last one is the meta-rule and G5's skipped reconciliation cost
   five recurrences one gate later; docs/14's record section is still empty and says its first
   entry will be G7's. The shape to copy is docs/historical/8-…-v2.md's own record
   ("**G1 (P1, four confirmed defects).** Predicted by the rubric: …").
3. **An adversarial review was launched and its verdict was not collected.** Five read-only
   lenses (schema/wire, engine/imaging, daemon/web, tests-and-gates, false-prose) returned; the
   verifier was still running when this session ended. The workflow script is at
   `~/.claude/projects/-home-pwnall-workspace-webcam-handler/9245be84-6ece-4c2c-ac45-b0f4e7d5df31/workflows/scripts/v3-adversarial-review-wf_8d8d970a-90c.js`
   and the lens reports are in that run's `journal.jsonl`. **Re-run it rather than trusting a
   stale verdict** — the tree moved twice after the lenses read it (the facade rebuild and
   `photo diff`). It already earned its keep once: see "the mutation" below.
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
