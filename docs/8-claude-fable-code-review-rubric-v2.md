# webcam-handler — Code Review Rubric (v2)

*Doc 8 in the webcam-handler series, **v2 — first reconciliation**. Supersedes docs/3
(v1, now under `docs/historical/`). Grounded on the design
(docs/6) and enforced mechanically where possible by the gate suite (docs/9). The standing
meta-rule is inherited from the predecessor project and is the first thing to know about
this document: **reconcile the rubric against defects actually found at every phase
gate.** v1 was written before the repository existed; this revision is the
reconciliation its meta-rule scheduled ("due at the first gate whose defects it failed
to predict, and no later than G4"): it is grounded on the P1 review (four confirmed
defects) and the P2 review (thirty-one candidates, fifteen confirmed — notes entry E4).
Rules that fired now carry local citations; rules yet unfired keep their transfer tags
— two gates in, nothing has earned deletion. Part E's closing section is the
reconciliation record.*

Provenance tags: **[SB]** transferred from the lm-switchboard rubric (predecessor
project; its provenance chains are not restated here); **[V]** transferred from the
vmcell rubric (the grand-predecessor — its unsafe/FFI rules were paid for by real
defects, cited inline where the payment matters); **[BP]** best practice, unmatched to a
surfaced defect yet; **[PF:n]** surfaced by hardware probes (docs/6
§1.2, PF:1–16 — measured on real cameras and drivers); **[S:Nn]** / **[S:En]**
repo-surfaced, citing implementation-note case law and evidence entries — the tags v1
promised are now live.

Governing rules:

1. **If a checklist item below reaches human review, the gate that should have caught it
   is missing.** File the missing gate; a fix or feature lands **with** its gate, in the
   same PR. [SB]
2. **For every test: write the buggy implementation — and run the whole workspace.** For
   every validator: write the malformed fixture. A mutation verified at package scope
   proves nothing when the pinning test lives one crate away by design; an absence claim
   names where it looked. [SB]
3. **A test CI never executes is not a test; a fixture never replayed is not coverage.**
   Counted selections on every gate recipe; a rung that auto-skips (vivid, hardware,
   ffprobe oracles) reports a **named, counted skip**, never silence. [SB]
4. **Enumerate what the suite structurally cannot reach** — the register is design §3.3
   and is regenerated at each revision, not accreted. [SB]
5. **Hardware claims are validated against a device or a captured profile of one, with
   provenance.** Green against the fake proves engine logic, never device truth; the fake
   itself is held to resemblance (E5). [SB, adapted]
6. **Every gate predicate carries a self-test that proves both directions — and the
   inverse arm is driven by the thing under test, not by a model of it.** A gate with
   no failing case fails; a gate with only failing cases fails. Completeness checks are
   driven by something the compiler breaks — an exhaustive `match`, never a hand list,
   and a fixed-size array does not substitute. [SB] The addendum is paid-for local law
   [S:N10]: `counted-selections.sh` shipped green-by-construction because its selftest's
   failing arm used a stub that returned a shape the real tool never produces — the stub
   encoded the author's belief, the predicate agreed with the belief, and the two shook
   hands. Where a stub is unavoidable, one arm still runs the real tool.

### Enforcement legend

`lint` compiler/clippy deny · `CI` a CI job · `test` a test that must fail on the buggy
impl · `review` human/agent judgment, no mechanical gate yet.

---

## Part A — Cross-cutting principles

1. **Fail loud, typed, and early — and driver success is not semantic success.** [SB]
   [PF:6] A driver clamps an out-of-range write and reports success; only read-back tells
   the truth. Every ioctl result is checked; every write returns `{requested, applied}`;
   a clamp is a surfaced warning, never silent.

2. **The device is the only authority on itself.** [PF:1–PF:6][SB A3-adapted]
   Capabilities, ranges, menus, and pairings are enumerated live; transcriptions
   (the declared pairing table) are data marked `declared` until a probe upgrades them to
   `measured`, and measured wins conflicts. A cached profile is corpus or UX hint, never
   silently substituted for a live read.

3. **Represent the unknown; never panic on it.** [PF:1][PF:12] Unknown control types,
   undocumented flag bits, out-of-range currents and defaults are carried as data and
   displayed. A `match` over device-supplied vocabulary always has a payload-carrying
   fallback arm; `unwrap`/indexing on device-driven values is banned on those paths.
   The reviewer's question for every parse of device data: *what does this line do when
   next year's kernel adds a variant?*

4. **Availability is not capability.** [SB E-adapted][BP] EBUSY, ENODEV, a settle
   timeout, and EPERM each read exactly like "the camera can't do that" to a lazy caller.
   The D13 vocabulary keeps them distinct; no code path converts one into another; no
   test asserts a capability from an availability failure.

5. **One law, one home — and a law's home existing is not the law applied.** [SB] The
   §2.10 registry (pairing planner, atomic writes, settle policy, error mapping, command
   surface, wire surface) is the checklist; a second copy is a finding. Check call sites:
   a caller that bypasses the home (an ad-hoc `S_EXT_CTRLS` outside `Camera::set`, a
   `serde_json::to_writer` to the state dir outside `write_json_atomic`) is the same
   defect as a second copy.

6. **Byte fidelity where bytes are the product.** [SB A4-adapted] A `.jpg` photo from an
   MJPG stream is the camera's own bitstream (E6); calibration comparisons never smuggle
   a re-encode into the loop; the AVI muxer's size fields are proven against bytes
   written by re-parsing our own output. `test`

7. **Leave the camera as you found it.** [PF:3][design D4, §5] Sweeps and guarded writes
   snapshot-first and restore-after by default; restoration order (automation before
   manual) is load-bearing and tested; R3 suites assert restoration. A code path that can
   exit without restoring (panic, ctrl-C, crash) has a recovery story anchored on the
   persisted snapshot — reviewed against each new exit path.

8. **Assert on the plane the property lives on — and a typed declaration nothing reads
   is a defect.** [SB A10] A limits constant no code consumes, a schema field no test
   round-trips, an error variant with no producer, a settle deadline that bounds nothing:
   for every constant, config field, and vocabulary entry, ask what *reads* it and what
   goes red when it stops being read. The camouflage to watch for: a green plumbing test
   named for the whole.

9. **The fake is faithful, driven, and failure-injectable — and a divergence is a
   finding against whichever side is wrong.** [SB A9/A20][E5][S:E4] The fake replays
   captured profiles; its behavioral
   claims (clamping, INACTIVE coupling, frame response to controls) are asserted against
   the probe record; fault menus are exhaustive-match-walked enums. A fake capability no
   real device exhibits is a finding against the fake — and the P2 review supplied the
   converse case: the fake refused the `Bytes`-at-a-scalar write the real backend
   mis-dispatched, so the divergence convicted the real side [S:E4]. Ask which side is
   lying before assuming it is the stand-in.

10. **Requested is not applied, anywhere in the stack.** [PF:6] The pair survives every
    layer: engine, session sample records, RPC DTOs, CLI output. A layer that collapses
    them to one value is dropping the fact the whole doctrine exists to keep.

11. **Names are load-bearing — on both sides of the ioctl.** [SB] Kernel control names
    become our slugs and the agent-facing vocabulary; a slug derivation that drifts from
    `v4l2-ctl` spelling breaks every agent's muscle memory. When an implementation looks
    wrong against its name, ask which is lying.

12. **Privacy is a correctness property.** [design §5] A camera frame may contain a
    person. Frames never enter the repository, logs, or error messages; capture lands
    only where the caller pointed. Reviewed on every path that touches frame bytes; the
    content gate (docs/9) covers the repository half.

13. **A probe carries one variable — and a menu is not a switch.** [SB][S:E4] Empirical
    pair discovery drives one
    automation control at a time and diffs; a discovery pass that changes two things
    attributes to nothing. Same for calibration sweep design and for R3 test structure.
    The P2 review found the whole family in one probe [S:E4]: one menu alternative tried
    of three (silent no-pairs), un-undone residue measured into the next candidate's
    diff (invented pairs stamped `Measured`), one "off" value inferred for two controls
    a mode moved differently (a wrong recipe recorded). D3 now states the three rules;
    review any probe against them.

14. **Bounded everything.** [BP] Settle loops, sweep sample counts, recording caps,
    frame-channel depths, shutdown drains: every loop over device behavior has a
    deadline or cap from `webcam-handler-schema::limits`, and a test drives the bound.

---

## Part B — Review checklist

### B1 · Schema and serde

- [ ] Control model round-trips every variant including `Unknown { raw }` and sparse
      menus; serde field names are the wire/persistence contract — renames are breaking
      changes caught by fixture files, not just compile. `test`
- [ ] Out-of-range current/default values survive deserialization and re-serialize
      unchanged [PF:4][PF:5]. `test`
- [ ] Every persisted document carries `schema_version`; a foreign version is a distinct
      typed load error with a fixture per shipped version. `test`
- [ ] Error registry (D13): every variant has a producer, an RPC code (exhaustive match
      in `webcam-handler-api`), and a rendering; a variant missing any of the three stops a build or
      a test. `test` `CI`
- [ ] DTO JSON Schemas are emitted by xtask and committed; drift between types and
      committed schemas fails CI. `CI`

### B2 · The backend trait and webcam-handler-v4l2

- [ ] T1/T2 impls take and return `webcam-handler-schema` values only; no V4L2 type escapes the
      backend crate (grep-gated). `CI`
- [ ] Control enumeration uses the raw QUERY_EXT_CTRL loop; `v4l::query_controls` does
      not appear (the PF:1 panic is one `use` away — lint-banned by name). `lint` `CI`
- [ ] QUERYMENU loops min..=max and tolerates holes [PF:2]; menu items are a sparse map
      end to end. `test`
- [ ] Capture-node detection reads `device_caps`, never node ordering [PF:7]; metadata
      nodes are represented but unstreamable. `test`
- [ ] Grouping and fingerprint fields match the committed profiles of real devices;
      profile capture and profile replay are inverses over the corpus. `test`
- [ ] Every ioctl error path maps to a D13 variant — EBUSY to `Busy` with holders, EPERM
      to `PermissionDenied` with the hint, ENODEV mid-operation to `DeviceGone`; the
      mapping table is walked exhaustively. `test`
- [ ] Every index-walked enumeration ends on `EINVAL` **or** `ENOTTY`, through the one
      `call_enumerating` home [PF:15][S:E1 amendments] — a metadata node's control walk
      answering `DeviceIo` is the regression this row exists to block; `ENOTTY` from
      `QUERYCAP` stays an error. `test`
- [ ] Write dispatch is the **descriptor's** decision (`HAS_PAYLOAD`), never the
      caller's value variant [S:E4]; a `Bytes` value aimed at a scalar control is a
      typed refusal on both backends, and the fake and real backend agree (E5). The
      failure mode is not hypothetical: dispatched on the value, a heap address reaches
      the ioctl union and a PTZ motor takes an allocator's low bits as a target. `test`
- [ ] The uevent socket is owned code: subscription, parse (kobject-uevent), debounce,
      re-enumerate — each with a fault fixture (malformed packet, flood). `test`

### B3 · The engine

- [ ] The pairing planner never emits a manual write under live automation — property
      test plus the constructible inverse fixture (rule 2). `test`
- [ ] Measured pairs trump declared pairs; provenance is preserved into the device
      profile and visible in output. `test`
- [ ] Pair discovery honors D3's three probe rules [S:E4]: every alternative of a
      menu-valued automation control tried; residue isolated (a non-undoable candidate
      never becomes the next candidate's baseline); "off" recorded per freed control by
      menu-item name. Each rule has the test whose buggy probe fails it. `test`
- [ ] Snapshot/restore ordering (automation first, two-pass INACTIVE) has a test whose
      buggy reordering fails; restore reports per-control `{requested, applied}` and
      unrestorable controls with reasons. `test`
- [ ] The pre-sweep snapshot persists **before** the first write; the crash-recovery test
      kills between write and restore and recovers. `test`
- [ ] Sweep plans are total, range-clamped, step-aligned to *measured* step, never
      silently empty. `test`
- [ ] Session state machine: illegal transitions are typed errors, each with a test;
      status vocabulary changes are serde-fixture-breaking by design. `test`
- [ ] Camera actors: exclusive streaming enforced; a second capture request honors its
      `wait` flag; actor shutdown mid-stream releases the device (fd closed — asserted).
      `test`

### B4 · Imaging and capture

- [ ] Verbatim-JPEG sink writes camera bytes unmodified (hash-compared in tests) [E6];
      re-encode paths are explicit and named in output. `test`
- [ ] Settle policy: skip-frames and settle-for both bounded by deadline; the
      never-converges fake fault produces `SettleTimeout`, not a hang. `test`
- [ ] Negotiated format/size/rate is surfaced whenever it differs from requested (D5 —
      the D3 doctrine applied to formats). `test`
- [ ] Size selection asks the *range* the question [S:E4]: a stepwise/continuous
      frame-size entry answers with the closest deliverable size (`largest_within`),
      never its maximum corner; the seed cameras are all discrete, so only a synthetic
      stepwise fixture can keep this red-able — it exists and is loaded. `test`
- [ ] Metric functions are pure, fixture-tested in both directions (sharp scores above
      blurred; clipped scores above proper exposure on the clip metric). `test`
- [ ] The AVI muxer's committed byte fixtures cover header/movi/idx1; its output re-parses
      with an independent read path; size-field accounting is property-tested; caps
      (duration, size, disk-full) each have a test producing a valid file up to the last
      complete frame. `test`
- [ ] EXIF stamps are read back with the independent reader crate in tests, **from the
      file on disk** — a test named "an independent reader can read back" that asserts
      only on the report is the exact defect the P2 review found under that name
      [S:E4]. `test`
- [ ] No code path parses camera bytes past `SOS` [PF:16]: the APP1 splice walks header
      segments only, a header length past the buffer ends the walk without indexing
      past it, and the marker-shaped-scan fixture (hand-built — camera frames never
      enter the repo, A12) stays loaded. `test`

### B5 · Calibration and persistence

- [ ] All state-dir writes go through `write_json_atomic` (grep-gated) and the fd-lock
      protocol; the daemonless-CLI vs daemon lock interaction has tests for both
      orderings. `CI` `test`
- [ ] `log.ndjson` load drops a torn last line and only the last line — a torn middle
      line is corruption, typed. `test`
- [ ] Sample records store `applied` values [PF:6] and relative camino paths; a session
      directory relocates intact (test moves it and reloads). `test`
- [ ] `select` records selector identity; `apply` refuses on fingerprint mismatch naming
      the differing fields; `--partial` is the only path around uncalibrated controls.
      `test`
- [ ] Metrics rank, selectors decide: no code path auto-selects a value without recording
      `metric:<name>` as the selector. `review` `test`

### B6 · Daemon, API, transports

- [ ] The T5 trait is the only wire surface; every method has an integration test,
      counted against the registered `RpcModule`'s `method_names()` — derived from the
      real server's registration, never a hand list; a Rust trait does not reify its
      methods, and docs/9's method-count-walk row is the authoritative mechanism (a new
      registered method with no test breaks the count). `CI` `test`
- [ ] Error mapping D13→RPC codes is one exhaustive match; unknown-error leakage (an
      `anyhow` string crossing the wire) is a finding. `test` `review`
- [ ] Subscriptions: disconnect mid-sweep neither cancels the sweep nor leaks the
      subscription (both asserted). `test`
- [ ] Shutdown: SIGTERM and SIGINT each tested; open MJPEG/WS connections cancel within
      the bound; the state lock releases; sd-notify STOPPING emitted. `test`
- [ ] The TCP listener requires the token; token-less requests 401; non-loopback bind
      warns naming the exposure; the UDS directory is 0700 (asserted at startup and in
      tests). `test`
- [ ] The MJPEG route: latest-frame drop semantics proven with a stalled reader (capture
      frame counter advances while the reader's does not); CompressionLayer provably
      absent on the route. `test`
- [ ] Daemon never opens a camera before first use; idle close honors config; both
      observable via the status API and tested. `test`

### B7 · CLIs and the web client

- [ ] One command surface (T4): `wch` and `wchc` share verb definitions and rendering;
      the parity gate (read verbs byte-identical `--json` on the fake) stays green; a
      verb added to one binary only is unrepresentable by construction — review any
      change that would make it representable. `CI` `review`
- [ ] `--json` output is schema DTOs verbatim — no CLI-invented fields. `test`
- [ ] Human output: tables degrade when piped (anstream discipline); progress bars
      suspend around log lines; exit codes are documented and tested per error class.
      `test`
- [ ] The web client is vendored-or-vanilla only: no CDN URLs, no fetches off-origin
      (grep-gated); assets embed; any vendored lib carries its license file and an
      inventory entry. `CI`
- [ ] Web client renders from DTOs (sparse menus become selects with the right indices;
      flags surface). `test`
- [ ] The browser half is asserted **in the browser**, not assumed from the API: the
      pinned Playwright/Chromium rung (design §3.1 R1-web) covers rendering-from-DTO,
      the painting preview, WS reconnect, calibration-view subscription tracking, and
      token refusal; the suite self-skips
      counted without node, node is never a build dependency, and browser + package
      versions are pinned. A browser-half behavior verified only through the JSON it
      consumes is the "green plumbing test named for the whole" smell wearing a DOM.
      `CI` `test`
- [ ] Chrome is the target (owner ruling, design §2.7): a Firefox/Safari-only defect is
      recorded, not necessarily fixed, and never motivates a compatibility layer.
      `review`

### B8 · Hardware and privacy discipline

- [ ] No camera frame bytes in the repository — the content gate covers images; review
      covers what it cannot (frames smuggled into unrecognized containers, §3.3 item 6).
      `CI` `review`
- [ ] No frame bytes in logs or error messages at any level. `review` `test`
- [ ] R3 suites are `#[ignore]`d, recipe-named, restore what they touch, and skip
      motor-moving sweeps without `WCH_ALLOW_MOTION=1` (the as-built spelling of v1's
      `--allow-motion`). `CI` `test`
- [ ] Killing a device holder is a distinct explicit command; nothing kills as a
      fallback. `review`
- [ ] The `Privacy` control is honored, never worked around [PF:12]. `review`

### B9 · Dependencies, licenses, toolchain

- [ ] cargo-deny allowlist as design §2.8; every named ban present with its reason on the
      entry; the license gate's selftest proves a synthetic violation fires (rule 6).
      `CI`
- [ ] The `v4l` crate: default features only, version pinned; `libv4l` feature and the
      four LGPL-linkage crate families unreachable (deny + selftest). `CI`
- [ ] `image` with `default-features = false`, features `png, jpeg` only — the default
      `avif` feature drags an AV1 encoder; gate-asserted, because it compiles fine and
      nobody notices. `CI`
- [ ] No TLS features anywhere (keeps CDLA-licensed cert stores out); gate-asserted.
      `CI`
- [ ] One MSRV fact, sync-asserted across manifests and CI; `--locked` everywhere; no
      git dependencies; majors current at adoption. `CI`
- [ ] New transitive licenses (the `unicode-ident` Unicode-3.0 class) land as allowlist
      entries with rationale, never as blanket exceptions. `review` `CI`

### B10 · Unsafe, ioctls & the device boundary

Everything crossing the ioctl/netlink boundary is kernel-ABI-critical, and a lying driver
is attacker-shaped input. Transferred from the vmcell rubric, which paid for these rules
(its inline re-declaration of a kernel struct was a 22-byte out-of-bounds write onto
PID 1's stack that tested green because the bytes landed in padding):

- [ ] **One module says `unsafe`**: every unsafe block lives in
      `crates/backends/v4l2/src/sys/`; every *other crate* carries a root
      `#![forbid(unsafe_code)]`, and within `webcam-handler-v4l2` (where a crate-root
      forbid is impossible by construction) the `unsafe-scope.sh` gate confines the
      token to `src/sys/`, deriving the allowed path from the tree. [V] `lint` `CI`
- [ ] **No hand-declared kernel structs.** Layouts come from `v4l2-sys-mit`'s bindgen
      output; any hand-carried definition gets `const`-asserted `size_of` and per-field
      offset tests against the generated one — re-declaring inline is banned. [V]
      `test` `CI`
- [ ] **`// SAFETY:` proves the actual obligation** of its block — pointer validity +
      size for the ioctl at hand, initialized-union-field choice, mmap lifetime — one
      obligation per block (`clippy::multiple_unsafe_ops_per_block`); a false safety
      claim is a defect even when the code works. [V] `lint` `review`
- [ ] **Device-derived numbers are validated before use**: `bytesused` clamped to the
      mapped length before slicing, menu indices bounded, payload sizes checked against
      `elem_size × elems`, wire integers via `try_from` never `as` (cast lints denied in
      the backend crate). [V][PF:4][PF:6] `lint` `test`
- [ ] **Miri runs the unsafe-adjacent pure units** (raw-struct decoding over captured
      bytes) — and the population is real: the selection provably includes every
      Miri-reachable `unsafe` block, because the P1 job selected only safe decode units
      while the two reachable blocks sat outside it [S:E1 amendments]. Code in the sys
      module is shaped to keep that population growing rather than shrinking. [V] `CI`
- [ ] The uevent netlink parser treats packets as hostile bytes: malformed and truncated
      packet fixtures, no panic paths. `test`

### B11 · Lint-suppression hygiene

- [ ] Narrowest scope: `#[expect]` (not `#[allow]`) on the single statement or
      expression, with `reason = "..."` stating why the lint is wrong *here* — a stale
      expectation self-reports under `-D warnings` (`unfulfilled_lint_expectation`);
      `clippy::allow_attributes` + `allow_attributes_without_reason` deny the
      undisciplined forms in non-test code. [V] `lint`
- [ ] Suppression scope notes are claims: a config comment naming N sanctioned sites
      must match the tree — both directions. [SB] `CI`
- [ ] Repeated legitimate sites collapse into one helper carrying one suppression — one
      place to audit, one reason to keep true. [V] `review`
- [ ] Banned on device- and request-driven paths: `unwrap`, `expect`, `panic!`, slice
      indexing, `as` narrowing (use `try_from`). `lint`

---

## Part C — Tests that actually test

**Smells — reject on sight** (transfers [SB] plus hardware costumes):

- [ ] **Skip == pass, in any costume** — the hardware edition: a vivid/R3/oracle rung
      that reports green when the module/device/tool is absent instead of a named,
      counted skip.
- [ ] **The green plumbing test named for the whole** — a config-to-struct test titled
      as if it covered enforcement.
- [ ] **List-vs-list completeness** — iterating the population under check; fixed arrays
      included; the T5 method-count gate is driven by the registered `RpcModule`'s
      `method_names()`, never a hand list — a Rust trait does not reify its methods, so
      "walk the trait" is not an available mechanism (docs/9's method-count-walk row is
      the authoritative statement; B6 cites the same one).
- [ ] **The gate with no self-test** — both directions or it does not ship.
- [ ] **The stub that agrees with its author** [S:N10][PF:15] — an inverse arm driven by
      a model of the tool instead of the tool: the stub encodes the author's belief, the
      predicate agrees with the belief, and the two shake hands (rule 6's addendum).
      Same lesson in probe costume: the Python oracle that caught bare `OSError`
      recorded "0 controls" and never revealed *which* errno ended the loop — a second
      implementation only catches what it distinguishes.
- [ ] **Assertion inside a conditional** whose false branch means "cannot go red" — the
      P2 review's `InactiveFlip` arm put its ordering assertion inside an `if let` the
      defect itself would skip [S:E4].
- [ ] **The observation counter that counts non-observations** [S:E4] — a hardware arm
      that tallies "toggled an automation control" as evidence when the toggle moved
      nothing; an observation is a *diff*, not an attempt.
- [ ] **The fake validating the fake** — a calibration test asserting the fake's optimum
      using the fake's own model as the expectation; the expectation comes from the
      profile/fixture, independently stated.
- [ ] **Restoration by assumption** — an R3 test that restores in teardown but never
      asserts the restoration happened (teardown failures vanish).
- [ ] **Pixel-content assertions on real hardware** — lighting varies; assert
      invariants (decodability, dimensions, metric *orderings* under controlled
      perturbation), not pixels.
- [ ] **Sleeps as synchronization** — settle logic is a policy object with a stepped
      clock; a bare `sleep` in a test is a flake being scheduled.

**Positive requirements:**

- [ ] Every D13 variant: a producer fixture + a mapping test + a rendering test. `test`
- [ ] Every PF finding: a corpus-backed regression representation (the sparse menu, the
      out-of-range default, the clamp, the INACTIVE flip, the unknown type — each exists
      as data a test loads, not as prose). `test`
- [ ] Every fault-menu variant of every seam: exhaustive-match-walked, each with a test.
      `test`
- [ ] Both CLIs: subprocess tests over trees built to pass and to fail. `test`
- [ ] Counted selections on every gate recipe; every gate predicate in the selftest
      table with both directions. `CI`

---

## Part D — Required automated gates

Deployable contents live in docs/9. The doctrine: every defect family fails a lint, a CI
job, or a test that can go red — and **every gate proves it can go red** (rule 6). A
defect class surfacing in review without a gate files the gate in the same PR (rule 1).

---

## Part E — Running a review

- **Phase preflight**: name the gate in force (G0–G6, docs/7) and what green does not
  prove — the §3.3 structural gaps, and the standing hardware caveat: green on the fake
  is engine truth, not device truth (rule 5).
- **Ground in the settled registry**: D1–D13, T1–T6, E1–E6, the §7
  considered-and-not-adopted list, and the §1 non-goals are settled; a finding that
  re-litigates them without new evidence is noise. Implementation notes are case law.
- **Mutations at workspace scope; absence claims name where they looked** (rule 2).
- **Adversarial verification for every Critical/High finding** — attempt the refutation
  before reporting; a refuted finding teaches the rubric something either way.
- **Every finding carries** `file:line`, category, the red test or fixture it lacks, and
  a direction. Confirm cited lines before fixing.
- **Reconciliation at each phase gate** — the meta-rule. This revision is the first
  reconciliation; the next is due at G3 and each gate after, appended to the record
  below (a new doc version only when the accumulated deltas warrant one).

### The reconciliation record

**G1 (P1, four confirmed defects).** Predicted by the rubric: none as written — all
four were in territory the rubric *named* but under-specified. PF:15 (ENOTTY) fell
under A3's "what does this line do when next year's kernel adds a variant" but no
checklist row said "enumeration terminators are a vocabulary"; B2 has that row now. The
Miri-selection defect (a job whose population excluded the only reachable unsafe
blocks) was rule 3 wearing a new costume — the selection was counted but counted the
wrong thing. Lesson absorbed into the B10 Miri row's phrasing ("the population is
real").

**G2 (P2, fifteen confirmed of thirty-one candidates — notes E4).** The families and
where they landed: the ioctl-dispatch defect (B2's new descriptor row, A9's converse
case); the stepwise collapse (B4's new range row); the discovery menu family (A13, B3's
new probe-rules row); two gates checking less than claimed (rule 6's addendum, [S:N10]);
three tests that could not fail (Part C's cited smells). What the review did *not* find
is recorded in E4 and matters equally: no unsound `unsafe`, no aliasing defect in the
mmap path, no availability-to-capability conversion — the B10 and A4 rules held.

**Not yet fired, retained:** the B5 store rows and B6/B7 daemon/web/CLI-parity rows
await P3–P5 subjects; the muxer rows await P6. [BP]-tagged items stay until a gate
either pays for them or proves them dead — two gates in, nothing qualifies for
deletion.

---

## One-line summary

Make every anticipated defect class fail a lint, a CI job, or a test that can actually go
red — prove the checks can go red too (rule 6) — and hold the hardware doctrine: the
device is the only authority on itself, requested is not applied, unknown is represented,
availability is not capability, the camera is left as found, and no frame ever lands
where its owner didn't point.
