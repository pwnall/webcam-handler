# AGENTS.md — webcam-handler (v3)

Doc 16 in the webcam-handler series, **v3 — second revision**; supersedes docs/10 (v2, now
under `docs/historical/`). Deploy at the repository
root as `AGENTS.md`; the deployed copy tracks this file (one-directional; when they
drift, reconcile deliberately and record which side was wrong). Terse by design; the
reasoning lives in
`docs/14-…-code-review-rubric-v3.md` (rubric) and `docs/12-claude-fable-design-v3.md`
(design).

Redirect at the repository root from `CLAUDE.md`, which holds `@AGENTS.md` and nothing
else: `AGENTS.md` is the standardized, tool-neutral name, so the rules keep one home and a
Claude-specific reader is pointed at it rather than given a second copy (owner ruling,
2026-08-14; note **N102**).

## Who runs this, and why (owner, 2026-08-12; extended 2026-08-18)

`webcam-handler-daemon` runs on a computer whose cameras are **pointed at a device under
test**. Three consumers, shaped three ways:

- **An AI agent harness (Claude Code or similar) drives the client to photograph the
  device under test, to check its own work** — e.g. a display driver validated by
  photographing the device's display. Primary, continuous, unattended. The same agent
  wants **video**, to validate animations — which makes frame *timing* a payload rather
  than metadata.
- **The owner uses the web client** to check up on the cameras, to tune them with the
  preview and the controls side by side, and to **drive calibration by eye** (design D20
  — human-driven calibration; the CLI flow stays the agent's). Occasional, interactive,
  supervisory.
- **A sibling project's HIL harness (usb-teleporter)** consumes this tool as a pinned
  library and as a `--json` subprocess to prove a forwarded camera still honors the
  whole backend contract (design §1.3). It compares answers **across machines**, which
  is why identity and description are a stated partition (D15) and why frame timing is
  contract (D16).

Five consequences, because they decide trade-offs: **the primary consumer has no hands**
— a verb needing a call sequence, or a failure that reads as prose, is a defect for the
consumer that matters most. **The product is comparability across time** — two photos an
hour apart must differ only where the *device* differs; when quality and repeatability
conflict, repeatability wins. **The consumers overlap on one camera as the normal case**
(note N83). **The error vocabulary is read unsupervised**: `Busy` means retry,
`DeviceGone` means stop and tell the human, `PermissionDenied` means a setup problem.
**An answer about a device must survive the device moving** to another bus or machine —
identity fields say where, description fields say what, and nothing conflates them. The
full statement lives at the top of `docs/implementation-notes.md`.

## What this is

webcam-handler drives V4L2 webcams for humans and AI agents: enumerate cameras with
stable identities, select them by any spelling a caller holds (id prefix, `/dev` node
path, `bus:`, `usb:`, `serial:` — design D14), list capabilities, drive controls
(pan/tilt/zoom, focus, exposure — auto/manual pairing handled), take photos and videos
in-process (no ffmpeg, no v4l2-ctl at runtime), account for a stream's delivery health
from its own frames (D16), compare two profiles at the device level and two photographs
as peers (D15, D17), and run calibration sessions — agent-driven from the CLI,
human-driven from the web client (D8, D20). **Every D14–D20 surface is design v3's,
landing across docs/13's P7–P9**: between adoption and a surface's own sub-milestone,
the design is the commitment and the tree is the state, and the phase ledger says which
is which — the same present-tense-at-adoption reading every sentence below that names a
D14–D20 mechanism carries.

Rust 2024 workspace, one library + four consumers: `webcam-handler-schema` (every shared
type, the backend traits, `BackendKind`), `webcam-handler-imaging` (codecs, our AVI
muxer, metrics, the stream-stats and comparison cores), `webcam-handler-engine` (camera
actors, guarded writes, snapshot/restore, calibration, session store, `resolve`, and the
embedding **facade** — the blessed composition, which the direct CLI is rebuilt on, D18),
backends behind one trait (`webcam-handler-v4l2` real, `webcam-handler-fake` replaying
**captured device profiles**), `webcam-handler-api` (the one jsonrpsee wire surface),
binaries `webcam-handler-cli`, `webcam-handler-daemon` (JSON-RPC over UDS always, web
client over opt-in loopback TCP + token), `webcam-handler-client` (a lib as well as a
bin), `webcam-handler-web` (vanilla-JS, embedded, Chrome-targeted).
`webcam-handler-cli` and `webcam-handler-client` share one command surface via
`webcam-handler-cli-core` — a verb exists once; the client links no backend and no
engine. Every binary is named after the package it comes from (owner ruling, N90; the
short-name question is design §8.11 and **nothing renames before the owner rules — a
name sweep is always its own sub-milestone, and the `wch_*` wire namespace is a wire
break no sweep may touch**, N91).

Work is phase-gated (docs/13: P7–P9 open, gates G7–G9; P0–P6 are closed — the ledger is
`docs/historical/7-…-v2.md` and the G6 whole-tree review is `docs/11`); each gate is a
named, counted, re-runnable `just gate-gN` over `scripts/gates/phase-criteria.tsv` — one
row per criterion, added in the same commit as the thing it proves. Work lands in
session-sized sub-milestones ending at committed boundaries with `just ci` green and the
notes current; the phase review gets its own session.

## Read before changing anything

- `docs/implementation-notes.md` — case law: numbered N-entries, PF-entries continuing
  design §1.2's registry, dated append-only E-entries. **Read the last entry of each kind out
  of the file before citing or minting a number** — the highest N moves most sessions, and a
  count written here would be a claim nothing reconciles (N153, N158). Do not
  "fix" entries recorded there; record new justified deviations as notes; retire an
  entry only on empirical disproof. Citations of docs/1–10 in older entries resolve
  under `docs/historical/` (numbering preserved).
- `docs/12-…-design-v3.md` — architecture. **D1–D20, T1–T6, E1–E6, §7's rejected
  alternatives, and the §1 non-goals are settled**; do not re-litigate without new
  evidence (a new probe finding is evidence; taste is not).
- `docs/14-…-rubric-v3.md` — every rule below is expanded there with provenance.
- `docs/15-…-gates-v3.md` — the gate suite; the repo's files are authoritative and that
  doc records claims, commissioning and deltas.
- `vendor/v4l2-webcam-skill/` — the manual workflow this tool replaced; the operations
  map is design §1.1, and `docs/agent-guide.md` is the skill's generated successor.

## Non-negotiable rules

1. Every anticipated or discovered defect class becomes a lint, a CI job, or a test that
   can go red. A fix lands **with its gate**, in the same PR — and **the fix itself gets
   an independent adversarial reader before it commits**: three of eleven G6 repair
   commits were green with regressions no test asked about, and green `just ci` is not
   evidence about the fix (rubric rule 8).
2. Every test and every gate predicate must be able to fail — both directions, proven in
   `scripts/gates/selftest.sh`, **and every failing arm names the sentence it goes red
   on**, because a check red for the wrong reason reads as green about the right one
   (N240–N243; rubric A16). Write the red-on-inverse first. Mutations verify at
   workspace scope; absence claims name where they looked.
3. CI executes what it claims: counted selections, `--no-tests=fail`, and every
   auto-skipping rung reports a **named, counted skip** — never silence.
4. **The device is the only authority on itself.** Enumerate live; transcriptions —
   pairing tables and kernel constants alike — are `declared` data until a probe or the
   bindgen output makes them `measured`, and measured wins. New hardware behavior lands
   as a profile in `corpus/` + a note, the day it is seen.
5. **Requested is not applied.** Drivers clamp silently (probe-verified); every write
   reads back and every layer preserves `{requested, applied}` — and for motor controls
   `applied` means accepted, not achieved [PF:18].
6. **Represent the unknown — and the unavailable.** Unknown control types, sparse menus,
   out-of-range values, undocumented flags: carried as data, never panics, never
   "corrected". A `match` on device vocabulary has a payload-carrying fallback arm. A
   comparison that cannot compute one answer states the reason as data and answers the
   rest (D17).
7. **Availability is not capability.** EBUSY/ENODEV/EPERM/timeout stay distinct from
   "the camera can't"; no code or test converts one into the other — a *tolerance* that
   folds refusals into "no value" is the same conversion (N196), and so is a state a
   failure strands with no verb out (docs/11 H2).
8. **Leave the camera as you found it.** Snapshot before, restore after, automation
   before manual; persisted pre-sweep snapshots make crashes recoverable and recovery
   frees stranded sweeps on every arm; a snapshot stamped after a driver disturbance is
   refused, not replayed [PF:28]; tests assert restoration, and mid-arm exits are
   `Drop`-guarded (N137).

## Writing code

- Pure cores take values (pairing planner, sweep planner, settle policy, session state
  machine, metrics, stream stats, the comparison core); seams live in the shell and each
  has a real impl and a scriptable double with an exhaustive-match fault menu, each
  fault consumed where it decides its answer (N232).
- One home per law (design §2.10): control semantics in `webcam-handler-schema`; camera
  selection in `schema::selector` + `engine::resolve` (every spelling, one parser, one
  resolver — selection never filters enumeration); the profile's identity/device
  partition in `schema::profile`, closed by destructuring; guarded-write planning in
  `engine::pairing`; state-dir writes through `engine::store::write_json_atomic` under
  the one fd-lock; errors in the D13 registry with three exhaustive matches over one
  `ALL` (wire code, rendering, exit code); the wire surface is the T5 declaration; the
  command surface is the shared T4 core; the blessed in-process composition is
  `engine::facade` and the CLI consumes it. A second copy or a bypass is a defect.
- **A backend contract is enforced where both backends inherit it** — the shared
  resolver (`StreamRequest::choose` is the exemplar) — or it names the battery arm that
  walks it on both. A rule enforced by the fake and violated by the real backend was
  green on both (docs/11 H1); ask of every backend contract which arm would fail if one
  side stopped honouring it.
- **A claim on a camera comes back with its value** (N169): recording slots, preview
  watchers, the device's own STREAMON — released by `Drop` or `#[must_use]`,
  `Weak`-witnessed, reaped on every entry; a release that depends on a later line
  running is a camera the agent meets as `Busy` forever.
- Backends implement T1/T2 against schema values only; no V4L2 type escapes the backend
  crate. The `v4l` crate's `query_controls` is lint-banned (PF:1). Index-walked
  enumerations end on `EINVAL` **or** `ENOTTY` through `call_enumerating` (PF:15). Write
  dispatch is the *descriptor's* decision (`HAS_PAYLOAD`), never the caller's value
  variant — on both backends, with the array-control fixture loaded (N135). One declined
  control read is carried valueless (`EBUSY` only) and visibly absent everywhere a value
  would be (N192, N195, N196).
- Pair discovery follows D3's three probe rules: every menu alternative tried, residue
  isolated, "off" recorded per freed control by name.
- Photos: verbatim camera JPEG when the sink allows; negotiated results always surfaced;
  an explicit format or size is honored or refused from the one shared home — never
  substituted (docs/11 H1/H1b).
- Bounded everything: constants live in `webcam-handler-schema::limits`, something reads
  each one, a test drives each bound **from both sides** (N255), and caller-supplied
  numbers are capped at the door, before a motor moves (N147).
- Unsafe code (rubric B10): every `unsafe` block in `crates/backends/v4l2/src/sys/`;
  every other crate `#![forbid(unsafe_code)]`; `unsafe-scope.sh` confines the token and
  reconciles the residual register both ways. Bindgen output only; `// SAFETY:` proves
  the block's *actual* obligation — the one its ioctls have (N190). Device-derived
  numbers validated before use; wire integers via `try_from`; suppressions `#[expect]`
  with `reason=`; no `unwrap`/`expect`/`panic`/indexing on device- or request-driven
  paths, present at every shipped crate root and walked by `lint-posture.sh`.
- Daemon: per-camera actor threads own the device; exclusive streaming by construction;
  one take per camera and the sequence total (N114); a photo during a take is `Busy`
  with an `Occupation` naming this daemon's work (N217); cameras open on first use and
  close when idle; SIGTERM ≡ SIGINT; teardown is a bounded table, and expired
  connections are shut down, never awaited.
- **Text a surface prints reaches its reader unescaped, and a ban on a defect names the
  class, not one spelling of it** (N123, N249; rubric A17): rustdoc links and their
  escapes are undone at the one door each surface builds through; a D13 message and the
  guide's `Do` column are payload, tested by driving the claim (rubric A15).

## Writing tests

- Construct the buggy implementation first and watch it fail — at workspace scope. Write
  the malformed fixture for every validator.
- Fixtures enter tests as bytes/corpus. Device profiles are tool-captured, committed
  with provenance, immutable; re-capture replaces wholesale. Every PF-class finding
  exists as corpus a test loads, not prose. Expectations come from committed tables or
  independent derivations — never from the function under test (a self-referential
  expectation is red-able in one direction only, N252), and never from a test-module
  shadow of it (the helper-measuring smell, N252).
- The fake resembles: its claims are asserted against the probe record of the profile it
  replays; a fake capability no real device exhibits is a bug in the fake (PF:17, N136);
  a resemblance claim about an event this rig cannot produce is `declared` until a rig
  that can produces the measurement (D19).
- Hardware suites (`#[ignore]`d, recipe-named) assert invariants and metric *orderings*,
  never pixel content; restore what they touch and assert it — motor positions included,
  `Drop`-guarded. Motor-moving suites run by default (owner ruling, 2026-08-08);
  `WCH_NO_MOTION=1` excludes them, counted and named. The `hw_`/`vivid_` suites
  serialize in the one-thread `exclusive-device` nextest group.
- The vivid rung reaches paths the attached cameras cannot (77 controls, compound
  payloads): run `just rung-vivid-managed` after touching enumeration, the control walk,
  the format tree, writes, or streaming. It needs the blessed helper — `just bless`
  once, `just priv-doctor` to check; never `modprobe` by hand.
- Gate selftests prove both directions with the sentence named per arm; the inverse arm
  is driven by the thing under test (rule 6), and where a stub is unavoidable one arm
  still runs the real tool.
- The mutation floor (`just mutants`): a `cargo-mutants` run scoped by
  `.cargo/mutants.toml`, judged by the whole workspace. Every survivor is a new test or
  a reasoned acceptance citing its N-entry, checked both ways — **and a register entry
  that "stopped surviving" is a prompt to apply the mutant by hand on an idle machine,
  never a finding: that direction has fired four times and been wrong four times, and
  under load the floor both deletes true acceptances and hides real survivors**
  (N209, N251). Hours not minutes; never a `just ci` step; its absence a named, counted
  skip; the default jobs figure and its price are `mutants.sh`'s stated warning pending
  the owner's ruling.
- No `sleep` as synchronization — settle logic runs on a clock the test owns: a
  `SteppedClock` where the deadline is the subject, a `FrozenClock` where it is not
  (N60, N67).
- The browser half is asserted in a real browser: the pinned Playwright/Chromium rung
  self-skips counted without node — node is never a build dependency — and a browser
  behavior verified only through the JSON the page consumes is not verified. It is not
  `#[ignore]`d: `just ci` runs it where the host has node; `just rung-web` ends on RAN
  or SKIPPED. Claims are manifest-counted both ways. Miri runs the unsafe-adjacent pure
  decode units.
- No assertion inside a conditional whose false branch cannot go red; no skip that reads
  as pass — a bare library `return` included (N160, N231, N235); and when two readings
  of the code both refuse, assert the *reason*, with the input that separates them
  beside the one that fails (N250; rubric A16).

## Hardware and privacy

- **A frame may contain a person.** Camera frames never enter the repository, logs, or
  error messages; `corpus/images/` holds generated synthetic fixtures only
  (gate-enforced); test captures go to gitignored scratch under `target/wch-scratch/`;
  the daemon records nothing it wasn't asked to. The session tree is created private
  (0700/0600) and a wider tree is refused, never repaired (N142, N150). Stored session
  photographs leave the machine through exactly one door — D20's `/session-photo`
  (lands P9b), reference-addressed, on the gated list.
- The web listener is opt-in, loopback + token by default; the UDS directory is 0700.
  The token gates the routes that carry or drive the camera — `/rpc`, `/preview`, and
  (once D20 lands) `/session-photo`, which `daemon::http::CAMERA_BEARING_PATHS` names —
  and **not** the static assets, which are this project's own open-source code (owner
  ruling, 2026-08-12; N82). A route added without a gate is the defect class that ruling
  created: `web-routes-are-gated.sh` and `every_camera_bearing_route_is_behind_the_gate`
  are the two halves that go red on it. Provenance runs before credentials; every
  credential presented must verify, truncated spellings included (N74, N250); the
  journald sink redacts the token and the terminal does not (N182). Weakening any of
  this is an owner decision, not a convenience fix.
- Killing a process that holds the camera is an explicit command naming its target,
  never a fallback. The hardware `Privacy` control is honored, never worked around.
- PTZ motors wear: sweeps are bounded by the `limits` caps everywhere. In *tests* motors
  run by default (`WCH_NO_MOTION=1` opts out, counted); in the *product* a plan that
  would move motors says so first (`--allow-motion`).
- `webcam-handler-priv` (dev-only, root-equivalent — design §2.13, notes N8, N125,
  N126): boundary is the 0700 file mode, checked every `just ci`; blessing is
  `cap_sys_module+ep` over a closed verb vocabulary; it refuses to unload `uvcvideo`
  while any `/dev/video*` is open. **Never widen its verbs to take caller-supplied
  module names, paths or programs, and never add a capability, without amending N8 and
  N125**; `privileged-helper.sh` compares what a blessed copy carries against what the
  tree declares and refuses any other capability-carrying file in `.wch-bin/`.

## Done means

- `just ci` green locally and offline — fmt, clippy `-D warnings`, nextest, doc build,
  cargo-deny (permissive allowlist + named bans: no LGPL linkage, no MPL, no GPL codecs,
  no AGPL — `dssim` stays banned and D17's SSIM is MIT `image-compare` or owned code;
  IJG dormant; no TLS features), all gate predicates self-tested both directions with
  their sentences named.
- The battery green for every backend, skips declared and checked both ways.
- For a phase-closing change: the docs/13 gate criteria hold as named, counted,
  re-runnable commands; hardware evidence recorded in the notes with transcripts; the
  review's reconciliation written into docs/14's record — a gate closes when it is.
- New device behavior landed as corpus + notes, not prose in a PR description.
- **A doc comment in `webcam-handler-api`, `webcam-handler-schema` or
  `webcam-handler-cli-core` is an input to a committed artifact** — the OpenRPC document
  and schema bundle under `schemas/`, and `docs/agent-guide.md` from the clap tree — so
  editing one moves a committed file and `schema-artifacts-current.sh` /
  `agent-guide-current.sh` go red until `just generate` runs and the result is
  committed. clap prints those comments **to a user**: no rustdoc link and no escape of
  one reaches a terminal (N123, N249), and the guide is written for an unattended agent
  — imperative, exact, free of this repository's argument.
- **`wire_surface!` is an input to design D10** — `wire-surface-sync.sh` reconciles the
  namespace and every method/subscription name against the design's sentence, both
  directions, names spelled out (a shorthand is a member the reconciler cannot see). The
  namespace is a **wire break** and never a spelling (N91).
- **The §2.8 dependency registry is an input to a gate** — `dependency-registry-sync.sh`
  reconciles the design's table against `[workspace.dependencies]` both ways, so an
  adoption is not landed until the table row is (N133, N164).
- **A failing `--json` run prints a document too** (owner ruling, 2026-08-15; N127,
  N128): `schema::error::Failure` on stdout, the one human line on stderr, an exit code
  per kind from `cli_core::exit_code`. A `--json` run prints exactly one schema type,
  and which type says whether it answered; no answer carries `FAILURE_MARKER`.
- `--json` output round-trips against the committed schemas; `webcam-handler-cli` /
  `webcam-handler-client` parity holds via `./scripts/gates/cli-parity.sh` — byte-for-
  byte on every read verb and on the driven refusals, every other verb in a named bucket
  with a reason, the `document` bucket's reason being that one implementation serves
  both roots (design §2.7).

## Docs and dependencies

- Docs state each fact once, present tense, trade-offs honest; measured findings cite
  PF/N entries; a claim about a device without a probe behind it is marked `declared`; a
  prose count of code is a claim something reconciles, or it is not made (N153, N158).
- Dependencies: permissive licenses only, enforced by cargo-deny; versions pinned at
  adoption; no git dependencies; feature doors gate-held; MSRV one fact; `--locked`
  everywhere. **Adopting a crate that clears the bar is not an escalation** (owner
  ruling, 2026-08-09): judge the licence and solidity, take it, say what you concluded,
  and land the §2.8 table row with it. Moving the bar — a new allowlist entry, a lifted
  ban, an unpinned version — is the owner's. A *conditional* adoption (D17's
  `image-compare`) records its condition and the measurement that cleared it in the note
  that lands it.
- Build deps (bindgen, libclang, kernel headers — a declared, gated vintage
  precondition, N236) are acceptable; runtime external binaries are not (ffprobe/mpv are
  test oracles). The web client vendors or hand-writes everything — no CDN, no npm.
