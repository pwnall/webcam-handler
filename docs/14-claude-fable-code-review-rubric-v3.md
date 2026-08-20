# webcam-handler — Code Review Rubric (v3)

*Doc 14 in the webcam-handler series, **v3 — second revision**. Status: issued, adoption
pending (docs/12's adoption paragraph governs the set); supersedes docs/8 (v2) upon
adoption. Grounded on the design (docs/12) and enforced mechanically where possible by the
gate suite (docs/15). The standing meta-rule is inherited and is the first thing to know
about this document: **reconcile the rubric against defects actually found at every phase
gate.** v2 was the first reconciliation and then accreted four more (G3, G4, G6, and G5's
recorded absence) into a record longer than its rules; this revision does what a revision
is for — **the record's lessons move into the rows, and the record resets**. G1–G6's full
reconciliation history lives in docs/8 v2 (under `docs/historical/` after adoption) and is
cited from the rows it produced; this document's own record (Part E's closing section)
begins empty at G7. Nothing was deleted on the way through: every rule that fired keeps
its citation, and rules yet unfired keep their transfer tags — seven gates in, nothing has
earned deletion.*

Provenance tags: **[SB]** transferred from the lm-switchboard rubric; **[V]** from the
vmcell rubric (its unsafe/FFI rules were paid for by real defects); **[BP]** best
practice, unmatched to a surfaced defect yet; **[PF:n]** surfaced by hardware probes
(docs/12 §1.2, PF:1–28, continued in the notes); **[S:Nn]** / **[S:En]** repo-surfaced,
citing implementation-note case law and evidence entries; **[S:G6]** the G6 whole-tree
review (record: docs/11, moving to `docs/historical/` when a later review supersedes it).

Governing rules:

1. **If a checklist item below reaches human review, the gate that should have caught it
   is missing.** File the missing gate; a fix or feature lands **with** its gate, in the
   same PR. [SB]
2. **For every test: write the buggy implementation — and run the whole workspace.** For
   every validator: write the malformed fixture. A mutation verified at package scope
   proves nothing when the pinning test lives one crate away by design; an absence claim
   names where it looked. [SB]
3. **A test CI never executes is not a test; a fixture never replayed is not coverage.**
   Counted selections on every gate recipe; every auto-skipping rung reports a **named,
   counted skip**, never silence. [SB]
4. **Enumerate what the suite structurally cannot reach** — the register is design §3.3,
   regenerated at each revision, not accreted. [SB]
5. **Hardware claims are validated against a device or a captured profile of one, with
   provenance.** Green against the fake proves engine logic, never device truth; the fake
   itself is held to resemblance (E5) — and a resemblance claim about an event this rig
   cannot produce is `declared` until a rig that can produce it contributes the
   measurement (design §2.12, D19). [SB, adapted]
6. **Every gate predicate carries a self-test that proves both directions — the inverse
   arm is driven by the thing under test, and it names the sentence it goes red on.** A
   gate with no failing case fails; one with only failing cases fails. Completeness
   checks are driven by something the compiler breaks — an exhaustive `match`, never a
   hand list. The addendum is paid-for local law, five instances deep [S:N10, S:G6]: a
   count and the noun it is printed under are two claims; a stub's failing arm encodes
   its author's belief, so one arm always runs the real tool. The second half is N31's
   law, discharged into the harness at N240–N243 and now the floor rather than the
   ratchet: **every fail arm names the sentence it claims** (`gate_red_because`), because
   an arm red for the wrong reason reads as green about the right one — 368 of 368 at
   the v3 baseline, with the armless branches counted in N244 rather than swept.
7. **A rubric row names a class; only a walked population finds an instance of it.**
   [S:G6] Three of G6's four HIGH findings sat in classes this document already named,
   each row added because of an earlier instance — and each class had a population
   (the conformance battery, the SIGKILL suite, the call sites) that existed and was not
   pointed at it. Where this project walks a population closed by construction —
   `ErrorKind::ALL`, every `closed_vocabulary!`, the corpus loader, a derived selection —
   that review found nothing at all. **The rows that work are the ones with an `ALL`
   behind them.** So: every contract row below names its population and what closes it,
   Part E's preflight asks for both, and a row whose population is "the files a reviewer
   opens" is retained as a reading aid and is **not** prevention.
8. **The session that repairs is itself reviewed, by somebody other than its author,
   before it commits.** [S:E14, S:G6] Rule 1 commissions the gate for the finding;
   nothing commissions the gate for the fix. Measured at G6: three of eleven repair
   commits ended green with a regression no test asked about, three repairs were fresh
   instances of the class they repaired, and every one was caught by an independent
   reader of the diff, not by the suite. The reader gets the author's claims and the
   instruction that green CI is not evidence; the batch lands as one commit after its
   findings are repaired; and every paragraph a repair writes to justify itself is a
   claim usually one command from a verdict [S:N167] — a justification is not checked by
   the thing it justifies.

### Enforcement legend

`lint` compiler/clippy deny · `CI` a CI job · `test` a test that must fail on the buggy
impl · `review` human/agent judgment, no mechanical gate yet.

---

## Part A — Cross-cutting principles

1. **Fail loud, typed, and early — and driver success is not semantic success.** [SB]
   [PF:6] Every ioctl result checked; every write returns `{requested, applied}`; a clamp
   is a surfaced warning. For motor controls, `applied` means accepted, not achieved
   [PF:18].

2. **The device is the only authority on itself.** [PF:1–PF:6][SB] Live enumeration;
   transcriptions are `declared` until a probe makes them `measured`, and measured wins.
   A cached profile is corpus or UX hint, never a silent substitute — and a kernel
   constant is a transcription too: hand-copied bits are compared against the bindgen
   output that linked all along [S:G6 L11, N228].

3. **Represent the unknown; never panic on it.** [PF:1][PF:12] Unknown types, flags,
   out-of-range values carried as data; payload-carrying fallback arms on device
   vocabulary; and the v3 corollary — **represent the unavailable**: a comparison that
   cannot compute one of its answers states the reason as data (D17's SSIM) rather than
   refusing the answers it can compute.

4. **Availability is not capability.** [SB][BP] EBUSY, ENODEV, timeouts and EPERM stay
   distinct from "the camera can't"; no code path converts one into another — including a
   *tolerance* that folds EPERM and EIO into "no value", which converts three of the four
   classes at once [S:N196]. **And the conversion can happen a layer above the error**
   [S:E6][S:G6]: a state a transient failure strands with no verb out has turned an
   unplug into a permanent refusal — in memory (N24) and then on disk (docs/11 H2). For
   every state a failure can strand something in, name the transition out, and name who
   runs it when the process that entered the state is gone.

5. **One law, one home — and a law's home existing is not the law applied.** [SB] The
   §2.10 registry is the checklist; a second copy is a finding; so is a bypassing caller.
   Two v3 rows in that registry carry this principle's sharpest instances: a
   backend-contract refusal enforced per-backend instead of in the shared resolver is how
   H1 shipped [S:G6], and a claim released by "a later line runs" instead of by its own
   value is how M1/M2/H2 shipped [S:N169].

6. **Byte fidelity where bytes are the product.** [SB] Verbatim JPEG hash-compared; the
   muxer proven by re-parsing its own output through an independently derived reader; no
   re-encode smuggled into a comparison loop — which in v3 includes **no resample ever**
   inside `photo diff` (D17). `test`

7. **Leave the camera as you found it.** [PF:3][D4, §5] Snapshot-first, restore-after,
   automation before manual, restoration asserted; every exit path reviewed against the
   persisted snapshot — and a snapshot stamped after a driver-caused disturbance is
   refused, not replayed [PF:28, S:N86]. A guard that must run between perturbation and
   restore is a `Drop`, because a `continue` is an exit path too [S:G6 M31, N137].

8. **Assert on the plane the property lives on — and a typed declaration nothing reads is
   a defect.** [SB] For every constant, field and vocabulary entry: what reads it, and
   what goes red when it stops being read. Ask the converse — a constant the product
   reads whose value no test constrains [S:N70]. And ask it of *renderings*: a
   hand-written `Debug` whose content is load-bearing (a daemon log line) can be mutated
   to the empty string and satisfy every privacy assertion vacuously — assert the content
   beside the absence [S:N254].

9. **The fake is faithful, driven, and failure-injectable — and a divergence is a finding
   against whichever side is wrong.** [SB][E5][S:E4] Behavioral claims asserted against
   the probe record; fault menus exhaustive-match-walked, each fault consumed where it
   decides its answer [S:N232]. **Two doubles agreeing is evidence only where a fixture
   could have made them disagree** [S:G6] — the stand-in-versus-real family arrived three
   ways in one review (H1, M29, M30), all in the seam E5 was written for, and each closed
   by the fixture or the measurement that can tell the two rules apart. Ask of every
   resemblance claim **which fixture would show it false**. The dependency half fires as
   often: axum's `get()` answers `HEAD`, Chromium keeps a detached `<img>`'s request,
   `jsonrpsee` closes on one arm — claims about dependencies, none read [S:G6]. And the
   claim need not involve a dependency: three G6 repair justifications were refuted by
   one command each against this repository [S:N167].

10. **Requested is not applied, anywhere in the stack.** [PF:6] The pair survives every
    layer; a layer that collapses them is dropping the doctrine's fact — including a
    tolerance that swallows a write's readback refusal [S:N196].

11. **Names are load-bearing — on both sides of the ioctl.** [SB] Slugs match `v4l2-ctl`
    spelling; when an implementation looks wrong against its name, ask which is lying.

12. **Privacy is a correctness property.** [design §5] Frames never in the repository,
    logs or error messages; reviewed on every path that touches frame bytes; the v3
    additions are D20's one gated door for stored samples and A8's vacuity corollary
    [S:N254].

13. **A probe carries one variable — and a menu is not a switch.** [SB][S:E4] One
    automation control driven at a time; every menu alternative tried; residue isolated;
    "off" recorded per freed control by name. Review any probe against D3's three rules.

14. **Bounded everything.** [BP] Every loop over device behavior has a deadline or cap
    from `limits`, something reads each one, and a test drives the bound — **from both
    sides**: only the refusing half asserted is half a cap, and the boundary value
    belongs to a test [S:N255]. A caller-supplied number is bounded too [S:G6 M12], and
    the bound is checked at the door, before a motor moves, as well as at the gate
    [S:N147].

15. **A message is payload, and payload goes stale.** [S:N123][S:N129] A D13 message and
    the guide's `Do` column are the part of the payload the primary consumer reads first,
    and both go stale exactly where a variant grows a second caller — five instances in
    one phase, and the repair batch then shipped a remedy naming a verb the surface does
    not have [S:N220]. **The mechanical form: test the claim, not the wording — and the
    claim itself, not one notch short of it** [S:G6, N211]: drive the flag, require the
    kind; ask the product the same question the sentence answers; a rendering test
    asserting non-empty prose asserts that a sentence exists, the one thing never wrong.

16. **A refusal for the wrong reason reads as the right one.** [S:N250][S:N240] When two
    readings of the code both refuse an input, a test asserting the refusal proves
    nothing about which reading refused — the mutation floor's first real survivor sat in
    the credential gate behind twenty live request shapes, an absence list and a named
    test, all agreeing an arm was covered because both readings answer "refused" alone;
    they separate only beside a credential that *verifies*. Assert the reason: pair the
    failing input with a passing one that the wrong reading would admit; in the gate
    suite this is rule 6's name-the-sentence law, and in Rust it is choosing assertions
    that distinguish the readings, not merely the outcomes.

17. **A check that names one spelling of a class is read as covering the class.**
    [S:N249][S:N222] The rustdoc-link ban named `` [` `` and the escape spelling
    ``\[PF:n\]`` walked past it for a phase; the artifact sweep then found two more
    spellings of the same intent. Derive the class or enumerate its spellings in one
    place, put the fix at the one door every instance passes through (the surface's own
    emitter, not the five strings), and when a new spelling is found, widen the *ban*,
    not just the instance.

---

## Part B — Review checklist

Restated in full — this document stands alone — with the v3 deltas folded where they
landed.

### B1 · Schema and serde

- [ ] Control model round-trips every variant including `Unknown { raw }` and sparse
      menus; serde field names are the wire/persistence contract — renames are breaking
      changes caught by fixture files, not just compile. `test`
- [ ] Out-of-range current/default values survive deserialization and re-serialize
      unchanged [PF:4][PF:5]. `test`
- [ ] Every persisted document carries `schema_version`; a foreign version is a distinct
      typed load error with a fixture per shipped version; a change a `#[serde(default)]`
      can absorb does not bump the version, and the rule for one that cannot is written
      beside the constant [S:N151]. `test`
- [ ] Error registry (D13): every variant has a producer, an RPC code, a rendering that
      asserts the **claim** (A15), and an exit code — three exhaustive matches over one
      `ALL`; a variant missing any stops a build or a test. A wire-name spelling is
      asserted against the committed code table, never against the function that builds
      it [S:N252]. `test` `CI`
- [ ] DTO schemas and the OpenRPC document are emitted and committed; drift fails CI;
      the prose inside them speaks to a toolchain-less reader, in every spelling the ban
      knows (A17) [S:N148, N218, N222]. `CI`

### B2 · The backend trait and webcam-handler-v4l2

- [ ] T1/T2 impls take and return `webcam-handler-schema` values only; no V4L2 type
      escapes the backend crate (grep-gated). `CI`
- [ ] Control enumeration is the raw QUERY_EXT_CTRL loop; `v4l::query_controls` is
      lint-banned by name [PF:1]. `lint` `CI`
- [ ] QUERYMENU tolerates holes [PF:2]; menus are sparse maps end to end. `test`
- [ ] Capture-node detection reads `device_caps` and takes the **first** capture-capable
      member [PF:7][PF:19]; metadata nodes are represented, never streamed. `test`
- [ ] Grouping and fingerprints match the committed profiles; capture and replay are
      inverses over the corpus. `test`
- [ ] Every ioctl error path maps to a D13 variant; the mapping is walked exhaustively;
      one declined control read is carried valueless (`EBUSY` only — a wider tolerance
      converts A4's classes) and the absence is visible everywhere a value would be
      [S:N192, N195, N196]. `test`
- [ ] Every index-walked enumeration ends on `EINVAL` **or** `ENOTTY`, through the one
      `call_enumerating` home [PF:15]; `ENOTTY` from `QUERYCAP` stays an error. `test`
- [ ] Write dispatch is the **descriptor's** decision (`HAS_PAYLOAD`), never the caller's
      value variant — on both backends, with the array-control fixture that can tell the
      two rules apart loaded [S:E4][S:G6 M29, N135]. `test`
- [ ] The explicit-request contract lives in the shared resolver (`choose`), both
      backends inherit its refusals, and its population is the battery's
      `ExplicitRequest` arm on **both** backends [S:G6 H1; N134, N138]. `test`
- [ ] The uevent socket is owned code with hostile-bytes fixtures (malformed, truncated,
      flood). `test`

### B3 · The engine

- [ ] The pairing planner never emits a manual write under live automation — property
      test plus the constructible inverse (rule 2). `test`
- [ ] Measured pairs trump declared; provenance survives into the profile and output.
      `test`
- [ ] Pair discovery honors D3's three probe rules, each with the test whose buggy probe
      fails it [S:E4]; the probe's own restore orders automation-first [S:G6 M13, N143].
      `test`
- [ ] Snapshot/restore ordering has the test whose buggy reordering fails; restore
      reports per-control `{requested, applied}` and reasons; a post-disturbance
      snapshot is refused, not replayed [PF:28, S:N86]. `test`
- [ ] The pre-sweep snapshot persists **before** the first write; the crash suite kills
      a child running the real sweep path and recovers — including the
      stranded-`Sweeping` walk on every recovery arm, the no-snapshot one included
      [S:G6 H2; N139, N149]. `test`
- [ ] Sweep plans are total, range-clamped, step-aligned to *measured* step, never
      silently empty; planner adjustments reach the answer, the log and the live event
      [S:G6 M14, N145]. `test`
- [ ] Session state machine: illegal transitions typed, each with a test; status
      vocabulary changes are serde-fixture-breaking by design. `test`
- [ ] Camera actors: exclusive streaming; the `wait` flag honored; shutdown mid-stream
      releases the device (asserted); every claim on a camera is a value whose release
      is its own, `Weak`-witnessed and reaped [S:N169–N177]. `test`
- [ ] The selector resolves through the one parser and the one resolver, over the whole
      enumeration (B12's rows carry the populations). `test`

### B4 · Imaging and capture

- [ ] Verbatim-JPEG sinks write camera bytes unmodified (hash-compared) [E6]; re-encode
      paths are explicit and named in output. `test`
- [ ] Settle policy: bounded both ways — the never-converges fault yields
      `SettleTimeout`, and a caller-supplied deadline is capped **at the door**, before
      anything streams or moves [S:G6 M12; N144, N147]. `test`
- [ ] Negotiated format/size/rate surfaced whenever it differs (D5). `test`
- [ ] Size selection asks the range the question (`largest_within` on
      stepwise/continuous — the synthetic stepwise fixture stays loaded), and a named
      size narrows the ranking device-wide or refuses with `SizeRefusal` [S:E4;
      S:G6 H1b, N138]. `test`
- [ ] Metric functions are pure and fixture-tested in both directions; every pixel
      transform is walked over `Transform::ALL` [S:N207]. `test`
- [ ] The AVI muxer's byte fixtures cover header/movi/idx1; output re-parses through the
      independently derived reader; size accounting is property-tested and every derived
      size refuses by name rather than writing the crash placeholder [S:N204]; caps
      produce a valid file to the last complete frame; both containers reach `Measured`
      intervals [S:N106]. `test`
- [ ] EXIF read-back uses the independent reader, from the file on disk [S:E4];
      device-derived text is bounded before the u16 length field truncates it, and the
      truncation note reads its numbers off itself [S:N203]. `test`
- [ ] No code path parses camera bytes past `SOS` [PF:16]; a header length past the
      buffer ends the walk. `test`
- [ ] Raw decoders take the buffer `plane_bytes` says a driver owes — all of them, the
      padding-free final row admitted [S:G6 M16, N201]. `test`
- [ ] The stream-stats accumulator and the comparison core hold B12's rows (pure, total,
      bounded, represented unavailability). `test`

### B5 · Calibration and persistence

- [ ] All state-dir writes go through `write_json_atomic` (grep-gated) under the fd-lock
      protocol; both lock orderings tested; the home itself shows the atomic sequence
      and the classified parent-fsync contract (`Reach`) [S:G6 M10, N141]. `CI` `test`
- [ ] The session tree is private by mode and owner, refused-not-repaired, with the gate
      driving the shipped binary as a second opinion [S:G6 M11; N142, N150]. `CI` `test`
- [ ] `log.ndjson`: a torn last line drops on load (settled — N12); the tail heals at
      append under the held lock, and the heal carries the guard-tests it copied
      [S:G6 M9; N140, N253]. `test`
- [ ] Sample records store `applied` [PF:6] and relative paths; a session directory
      relocates intact; sample paths carry the sweep pass so refinement never overwrites
      evidence [S:N22, S:E6]. `test`
- [ ] `select` records selector identity — `human` included, from the page (B12);
      every mutating verb refuses on fingerprint mismatch naming fields [S:E6];
      `--partial` is the only path around uncalibrated controls. `test`
- [ ] The camera is given back by something a user can type [S:E6]; interruption records
      never invent a refusal nobody measured [S:N149]. `test`

### B6 · Daemon, API, transports

- [ ] The T5 declaration is the only wire surface; the method-count walk is driven from
      the registered `RpcModule`'s `method_names()`, never a hand list. `CI` `test`
- [ ] D13→RPC mapping is one exhaustive match; an `anyhow` string crossing the wire is a
      finding; the client keeps a delivered discriminant even over an unreadable payload
      [S:G6 M21, N215]. `test` `review`
- [ ] Subscriptions: disconnect mid-sweep neither cancels the sweep nor leaks the
      subscription. `test`
- [ ] Shutdown: SIGTERM and SIGINT each tested for real; the worst case is a bounded
      table beside the unit's `TimeoutStopSec`, per-connection sockets included
      [S:N174, N175]; the state lock releases; STOPPING emitted. `test`
- [ ] The token gates exactly `CAMERA_BEARING_PATHS` — two routes today, three once
      D20's `/session-photo` lands — and a route
      off that list is a finding whatever else it does (N82's partition, both halves);
      provenance runs before credentials, method-complete, reading every copy of the
      header it trusts [S:N180, N185]; the journald sink redacts the credential and the
      terminal does not [S:N182]; every credential presented must verify, truncated
      spellings included, pinned beside a verifying bearer (A16) [S:N74, N250]. `test`
      `CI`
- [ ] The MJPEG route drops for slow readers (proven with a stalled one) and carries no
      compression; HEAD answers about routes, never devices [S:N179]. `test`
- [ ] Cameras open on first use, close on idle, observable and tested; every claim comes
      back with its value (B3's row, gate-held) [S:N169–N177]. `test` `CI`

### B7 · CLIs and the web client

- [ ] One command surface: parity gate green, every uncompared verb in a named bucket
      with a reason — including the `document` bucket, whose exemption is the
      one-implementation argument (D15/D17). `CI` `review`
- [ ] `--json` is schema DTOs verbatim in both outcomes — exactly one document per run,
      and which type says whether it answered [S:N127, N216]. `test`
- [ ] Human output degrades when piped; exit codes are the D13 block, tested per kind
      [S:N128]. `test`
- [ ] Help and guide text reach their readers unescaped, in every spelling the ban
      names, undone at the one door both roots build their tree through (A17)
      [S:N123, N249]; every `Do`-cell lever really produces its failure (A15). `test`
- [ ] The web client is vendored-or-vanilla; no CDN, no off-origin fetch (grep-gated);
      renders from DTOs; prose counts of its files are reconciled by a test or not made
      [S:N153, N158]. `CI` `test`
- [ ] The browser half is asserted **in the browser** — the pinned Playwright/Chromium
      rung, claims manifest-counted both ways; a browser behavior verified only through
      the JSON the page consumes is not verified; B12 carries the workbench claims. `CI`
      `test`
- [ ] Chrome is the target (owner ruling, design §2.7); a Firefox/Safari-only defect is
      recorded, not necessarily fixed. `review`

### B8 · Hardware and privacy discipline

- [ ] No camera frame bytes in the repository — the content gate walks images and both
      containers; review covers what it cannot. `CI` `review`
- [ ] No frame bytes in logs or error messages — and no *vacuously satisfied* absence:
      load-bearing renderings assert their content beside the privacy claim (A8)
      [S:N254, N100]. `review` `test`
- [ ] Stored session frames leave the machine through exactly one gated,
      reference-addressed door (D20), and the absence of a second is a gate's population
      claim. `CI` `test`
- [ ] R3 suites are `#[ignore]`d, recipe-named, restore what they touch — motors
      included, `Drop`-guarded against mid-arm returns [S:G6 M31, N137]; motor suites run
      by default, `WCH_NO_MOTION=1` a named counted exclusion. `CI` `test`
- [ ] Killing a device holder is a distinct explicit command; nothing kills as a
      fallback (the gate counts call sites, not files [S:G6 H3, N161]). `review` `CI`
- [ ] The `Privacy` control is honored, never worked around [PF:12]. `review`

### B9 · Dependencies, licenses, toolchain

- [ ] cargo-deny allowlist as design §2.8; every named ban with its reason; the
      selftest's synthetic violation fires. `CI`
- [ ] The §2.8 registry table reconciles against the workspace manifest **both ways**
      [S:N133, N164]. `CI`
- [ ] Feature doors stay shut — `v4l` default-only, `image` without defaults, no TLS —
      read from the resolved graph, which is also what holds D17's adoption condition.
      `CI`
- [ ] One MSRV fact, sync-asserted; `--locked` everywhere; no git dependencies; majors
      current at adoption; a conditional adoption records its condition and measurement
      in the note that lands it. `CI` `review`
- [ ] New transitive licenses land as allowlist entries with rationale. `review` `CI`

### B10 · Unsafe, ioctls & the device boundary

- [ ] **One module says `unsafe`** — `crates/backends/v4l2/src/sys/`; every other crate
      root forbids; the residual-`unsafe` register reconciles both ways. [V] `lint` `CI`
- [ ] **No hand-declared kernel structs**; bindgen layouts; any forced hand-copy gets
      size/offset assertions; union-arm offsets are derived through `offset_of!`, never
      transcribed [S:N187]. [V] `test` `CI`
- [ ] **`// SAFETY:` proves the actual obligation** — the one its ioctls have, restated
      when the population under a comment changes [S:N190]; one obligation per block; a
      false safety claim is a defect even when the code works. [V] `lint` `review`
- [ ] **Device-derived numbers validated before use** — and the load-bearing clamp has
      the test whose inverse is a SIGSEGV, mutation-excluded regions included
      [S:N188, L22]. [V] `lint` `test`
- [ ] **Miri runs the unsafe-adjacent pure units, and the population is real** —
      provably including every Miri-reachable block. [V] `CI`
- [ ] Kernel names asked of bindgen are a declared build precondition with a gate, not a
      failed compile [S:N228, N236]. `CI`
- [ ] The netlink parser treats packets as hostile bytes. `test`

### B11 · Lint-suppression hygiene

- [ ] `#[expect]` with `reason =`, narrowest scope; the undisciplined forms denied. [V]
      `lint`
- [ ] Suppression scope notes are claims matched both directions [SB]; a wrapper's
      stated purpose is verified by removing it once, not by rereading it — the
      `cfg_attr` decoration was called load-bearing at three sites and refuted by one
      command [S:N167]. `CI` `review`
- [ ] Repeated legitimate sites collapse into one audited helper. [V] `review`
- [ ] Banned on device/request-driven paths: `unwrap`, `expect`, `panic!`, indexing,
      `as` narrowing — present at every shipped crate root, walked by the gate, never
      hand-copied trust [S:G6 L28, N165]. `lint` `CI`

### B12 · The v3 surfaces (D14–D20)

- [ ] **Selection has one parser and one resolver** — every spelling through
      `schema::selector::parse`, every match through `engine::resolve`; the scheme
      vocabulary is closed and walked; ambiguity carries every candidate; zero and many
      reuse the two kinds resolution always had. Population: the vocabulary's `ALL` over
      the committed corpus, with the shared-`usb_id` pair as the live ambiguity fixture
      [PF:13]. `test`
- [ ] **Selection never filters enumeration** — ids are stable under any selector (D1's
      ordinals are assigned over the whole machine); a backend constructor stays
      filter-free. `test` `review`
- [ ] **The address/identity split is documented where it is read**: `NodePath` resolves
      per call against the live listing [PF:22], and the guide's selector table carries
      the split — a `Do`-column claim, tested as one (A15). `test`
- [ ] **The projection is closed by destructuring** — a field added to
      `ProfileInvariant` fails to compile until sided; the comparison names sections and
      slugs; the corpus is walked as mutual negatives (every profile device-differs from
      every other, sections named) and identity-rewritten positives. The old private
      test mask is deleted, not kept beside the product's [A5]. `test`
- [ ] **Stream stats are pure, integer, and bounded by a stated bound** — retention up
      to `MAX_RECORDING_FRAMES`, degradation *on the answer*; gap accounting has a
      driven inverse through the fake's `FrameGap` fault; the accumulator and
      `declared_interval` agree on the mean over one take (one home, two readers,
      reconciled). `test`
- [ ] **The comparison core is total** — dimension mismatch is represented with a closed
      reason vocabulary, never refused; `MetricName::ALL` is the walked population so a
      new metric joins by existing; nothing resamples, ever [A6]; SSIM orders the
      committed blurred/sharp pairs in both directions like every metric before it.
      `test`
- [ ] **The facade is the composition** — the executor *crate's* only engine reach is the
      facade (`facade-is-the-composition.sh`, derived population over every file under
      `crates/cli/src`, and the ban is on the class of reach rather than one spelling of
      it: a grouped import, a restricted visibility, an `extern crate`, a glob and a
      sibling module are the same reach, and the import reader is shared rather than
      copied — `rust-imports.awk`, note **N271**); introduction carried a byte-equivalence
      criterion against the pre-move executor; the stability table matches the exports both
      ways (`facade-stability-table-sync.sh`), an engine module the facade's *public
      surface* forces on a caller is in the **Yes** column, and D18's own bullet names the
      table rather than restating it. `CI` `test`
- [ ] **The workbench's layout claim is asserted at the pinned viewport over the widest
      committed profile** (vivid's 77 controls) — preview and the adjusted control
      simultaneously visible at every scroll position; a friendly-profile pass is the
      fixture-one-parameter-away smell (Part C). `test`
- [ ] **`/session-photo` is camera-bearing and reference-addressed** — on
      `CAMERA_BEARING_PATHS` with both halves of the route partition extended in the
      same commit; caller text never becomes a filesystem path; HEAD answers about the
      route [S:N179]; anonymous, cross-site and out-of-session references each have
      their refusal arm. `CI` `test`
- [ ] **The human flow is the daemon's state machine, driven** — every page transition
      is a verb the daemon performs or refuses; `selector: human` lands in the session
      document and is asserted through a second socket, not through the page's own
      belief; an out-of-order click renders the refusal and the flow recovers; a session
      is drivable from the page and the CLI interchangeably. `test`
- [ ] **The D19 contract has hermetic twins and declining recipes** — every sentence
      driven over the fake's fault; `hw_gone_*` recipes decline by name on hosts that
      cannot arrange the loss; contributed evidence lands as an E-entry plus the E5
      resemblance re-check of the fault against it. `test` `CI`

---

## Part C — Tests that actually test

**Smells — reject on sight** (the v2 list stands; instance counts updated; three smells
new at v3):

- [ ] **Skip == pass, in any costume** — hardware, oracle, or a library `return` with no
      printed line [S:N160, N231, N235].
- [ ] **The green plumbing test named for the whole.**
- [ ] **List-vs-list completeness** — the population under check is derived or
      compiler-closed, never iterated beside itself.
- [ ] **The gate with no self-test** — both directions, sentence named (rule 6).
- [ ] **The stub that agrees with its author** [S:N10] — five instances; one arm runs
      the real tool.
- [ ] **Assertion inside a conditional** whose false branch cannot go red [S:E4].
- [ ] **The observation counter that counts non-observations** [S:E4].
- [ ] **The fixture that cannot exercise the rule** [S:E6][S:N70][S:G6] — six recorded
      instances; **the fixture is one parameter away from the case, and the parameter is
      the one a reader scanning for "does this test the rule" does not look at** — a
      profile, an identifier, a scalar, a viewport. The refuted seventh candidate stays
      in the row [S:G6]: a reviewer who dislikes an asserted behaviour and calls its
      fixture wrong has committed this row one level up; grep the notes for the subject
      *before* writing the candidate.
- [ ] **The fake validating the fake** — expectations come from the profile or fixture,
      independently stated.
- [ ] **The test that can only agree with the code** [S:N70] — the general form; ask of
      every double, fixture and constant: *what would this look like if it were wrong?*
      An exhaustive walk over a fault menu cannot see a variant the menu does not have.
- [ ] **The test that measures the helper** *(new)* [S:N252] — a `mod tests` shadow of
      the function under test makes every call site in the module coverage of the
      helper; and its sibling, **the self-referential expectation** — a test asking the
      function under test for its own expected value is red-able in one direction only
      (it sees a name that vanished, never one that changed). Expectations come from a
      committed table or an independent derivation.
- [ ] **The repair that copies a guard without the guard's test** *(new)* [S:N253] — a
      guard's test is part of the guard; a copy that brings the code and not the fixture
      re-creates the closed defect one function along.
- [ ] **The green run with no load stated** [S:N69] — for an ordering, a green run is
      evidence only with its load stated, at the load the defect was seen under. **Its
      mirror moved again at v3** [S:N209, N251]: the mutation floor's verdict moves with
      the machine in *both* directions, and the register's stopped-surviving direction
      has now fired four times and been wrong four times — under load it marks accepted
      mutants killable and real survivors caught (the eight-job run's "0 missed" hid
      nine, four of them real defects). A moved verdict is **a prompt to apply the
      mutant by hand on an idle machine, never a finding**; ask of any verdict that
      moved: which way did the load push it, and would I have re-run it had it pushed
      the other way?
- [ ] **Restoration by assumption**; **pixel-content assertions on real hardware**;
      **sleeps as synchronization** (two clock shapes — `SteppedClock` where the
      deadline is the subject, `FrozenClock` where it is not [S:N67]) — all as v2 states
      them.

**Positive requirements:** as v2 — every D13 variant with producer, mapping and a
rendering test that asserts the **claim** [S:N129, N211]; every PF finding as loaded
corpus; every fault-menu variant driven; both CLIs subprocess-tested both ways; counted
selections everywhere — plus, at v3: **every new contract names its population at birth**
(rule 7; the B12 rows model the form), and **every cap asserts both of its sides**
[S:N255].

---

## Part D — Required automated gates

Deployable contents live in docs/15. The doctrine: every defect family fails a lint, a CI
job, or a test that can go red — and every gate proves it can go red, naming the sentence
(rule 6). A defect class surfacing in review without a gate files the gate in the same PR
(rule 1).

---

## Part E — Running a review

- **Phase preflight**: name the gate in force (G7–G9, docs/13) and what green does not
  prove — the §3.3 register and the standing hardware caveat. **Then name the population
  each class will be walked over, and what closes it** (rule 7): an exhaustive `match`, a
  generated `ALL`, a selection derived from the tree.
- **Ground in the settled registry**: D1–D20, T1–T6, E1–E6, §7's rejected alternatives
  and §1's non-goals are settled; a finding that re-litigates them without new evidence
  is noise. Implementation notes are case law — grep them for the subject *before*
  writing a candidate.
- **Mutations at workspace scope; absence claims name where they looked** (rule 2) — and
  an absence list is itself a claim with arms: twenty live shapes and a named test agreed
  an arm was covered and a mutant disagreed [S:N250]; an absence claim about *overlapping
  refusals* needs A16's separating credential beside it.
- **A green run is evidence about a race only with its load stated** [S:N69]; a
  refutation attempted at the wrong load is not a refutation; a mutation verdict that
  moved with the machine is a prompt, not a finding [S:N251].
- **Two refutation stages, kept separate** [S:G6]: every lens attacks its own candidates
  before reporting (G6: 204 of 290 died there, 70%), and every survivor goes to an
  independent verifier whose default is REFUTED (G6: 38 confirmed, 35 narrowed, 13
  refuted — the second stage is mainly an *editor*). Keep the candidates that died;
  report a refuted finding anyway; lenses only read, so they run concurrently; repairs
  want isolated worktrees (N97, N98) with shared quantities named in both briefs and
  verified at the merge.
- **The repair session is reviewed too** — rule 8, in full, including the
  one-command-per-justification check [S:N167].
- **Every finding carries** `file:line`, category, the red test or fixture it lacks, and
  a direction; confirm cited lines before fixing. Where a finding can be measured through
  the shipped binaries rather than read, measure it and say so.
- **The review's own record is a dated evidence entry** — candidate count, verdict
  split, absence lists — written *before* the reconciliation. G4's went unwritten and its
  arithmetic is unrecoverable; G6's is docs/11 and is the shape.
- **Reconciliation at each phase gate** — the meta-rule. A gate closes when its
  reconciliation is in this document; G5's absence cost a named class five recurrences
  one gate later, and that sentence is retained here so the next skipped reconciliation
  has to be skipped in the face of it.

### The reconciliation record (v3)

Begins empty. G1–G6's record — the reconciliations, the "predicted / named-but-
under-specified / no-row" analyses, and the counts — lives in docs/8 v2, preserved under
`docs/historical/` at adoption and cited from the rows above. The first entry below will
be G7's.

---

## One-line summary

Make every anticipated defect class fail a lint, a CI job, or a test that can actually go
red — prove the checks go red for the reason they claim (rule 6 and A16), walk every
class over a population something closes (rule 7), review the repair as hard as the find
(rule 8) — and hold the hardware doctrine: the device is the only authority on itself,
requested is not applied, unknown is represented, availability is not capability, the
camera is left as found, and no frame ever lands where its owner didn't point.
