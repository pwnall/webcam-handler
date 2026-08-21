# webcam-handler — Code Review Rubric (v3)

*Doc 14 in the webcam-handler series, **v3 — second revision**. Status: **adopted** at
docs/13 P7a (commit `796babb`, 2026-08-18); supersedes docs/8 (v2), which now lives under
`docs/historical/`. Grounded on the design (docs/12) and enforced mechanically where
possible by the gate suite (docs/15). The standing meta-rule is inherited and is the first
thing to know about this document: **reconcile the rubric against defects actually found at
every phase gate.** v2 was the first reconciliation and then accreted four more (G3, G4, G6,
and G5's recorded absence) into a record longer than its rules; this revision does what a
revision is for — **the record's lessons move into the rows, and the record resets**.
G1–G6's full reconciliation history lives in docs/8 v2 (under `docs/historical/`) and is
cited from the rows it produced; this document's own record (Part E's closing section) opens
with G7, G8 and G9 — one review over the three phases, three entries, and the rows they
reworded. Nothing was deleted on the way through: every rule that fired keeps its citation,
and rules yet unfired keep their transfer tags — ten gates in, `g0` through `g9`, nothing
has earned deletion.*

Provenance tags: **[SB]** transferred from the lm-switchboard rubric; **[V]** from the
vmcell rubric (its unsafe/FFI rules were paid for by real defects); **[BP]** best
practice, unmatched to a surfaced defect yet; **[PF:n]** surfaced by hardware probes
(docs/12 §1.2, PF:1–28, continued in the notes); **[S:Nn]** / **[S:En]** repo-surfaced,
citing implementation-note case law and evidence entries; **[S:G5]** the P5 web-client
review, whose reconciliation was skipped and whose findings are therefore cited from the
rows they belong in (record: `docs/historical/11-claude-web-client-code-review.md`);
**[S:G6]** the G6 whole-tree review (record: docs/11, moving to `docs/historical/` when a
later review supersedes it).

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
   value is how M1/M2/H2 shipped [S:N169]. **The quiet form is the sibling** [S:N323,
   N326]: where one function over a type is closed — by destructuring, by a compiler-
   walked `ALL` — the sibling reading the same type through `self.` is unclosed and
   nothing says so, because the closure is invisible from the side that lacks it. Two
   instances in one review, in two crates: an identity fingerprint compared field by field
   beside destructured neighbours and the sibling that delegates to it, and a human record
   table showing one of D16's delivery numbers under a doc claiming both renderings show
   the same facts. So when a population has been closed anywhere, the next question is who
   else reads that type — and when one renderer's population has been closed by
   destructuring, its sibling is where to look for the same sentence holding nothing up.

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
    [S:N147]. **And the population is the *doors*, not the values** [S:N321, N322, N329]:
    name every path on which the bound is enforced and drive the bound on each of them,
    because a bound applied at the second door is one the first door's failure shape
    escapes — a decode budget that refused through two doors with only one of them saying
    so, a file `photo diff` reads whole before the bounded reader is reached, and a
    length taken off `stat(2)`, which is the readable size of a regular file and of
    nothing else.

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
    not just the instance. **And an enumeration in one place is still an enumeration**
    [S:N271, N316, N317, N328, N340, N341]: where the class has a syntactic form the
    derivation is a shared *reader*, not a longer list, because a spelling added to the
    list repairs the instance while this row asks for a repair of the ban. Three
    spellings of a reachable public item were enumerated and the fourth was a method on
    an `impl` block; a path into a crate is three prefixes and both facade readers knew
    one of them; a ban on a count kept a comma at both ends of a word, so `tests,` was
    not the noun `tests`; and a branch check banned an alternation and could not see a
    clause of one. The house's own precedent is the shared import reader — one
    `rust-imports.awk` two gates read — and the question to ask of any list of spellings
    is what reads the *form* instead.

18. **An answer in flight belongs to the request that asked for it.** [S:N154, N156]
    [S:N310, N311, N314][S:G5 M7, G6 M32] Every painter of a remote answer carries, from
    the moment it asks, the thing that says whether the answer is still wanted, and
    consults it before it paints — **including on the refusal arm**, which is the half
    that goes unfenced, because a `catch` is written where the failure is thought about
    and not where the staleness is. What the predicate compares is a decision, not a
    habit: an identity where the identity can tell, a counter where it cannot — an
    operator who walks to another camera and back while a capture runs makes the ids
    match again while the slot has been emptied twice [S:N310] — and a fence borrowed
    from a sibling is a fence held up by a sentence that may be false of this path. The
    same rule holds for a shared status element with more than one writer: no writer may
    make a statement it cannot know, the last writer that still can wins, and a writer
    whose connection has ended declines rather than overwrites. **The population is every
    painter and every writer of the element, and nothing walks it** — the painters carry
    the fence one claim apiece and the writers of `#connection` are enumerated in that
    module's header, both by hand, so one added tomorrow inherits nothing and this row is
    the only thing that asks. A derivation over the writers is the gate this class is
    owed; until it exists the arms are per instance and the population is review. `test`
    `review`

19. **A count of the tree stated in prose is a claim something reconciles, or it is not
    made.** [S:N153, N158][S:N318, N319, N334][S:N339, N342] This covers a doc comment, a
    gate criterion's `what` field, a design table, a rubric row and a note alike, and a
    doc comment in `webcam-handler-api`, `webcam-handler-schema` or
    `webcam-handler-cli-core` is an input to a committed artifact, so the number travels
    to a reader who cannot see the tree at all. Where the figure is wanted, tie it to the
    builder that produces it and let the two go red together — a criterion row's count of
    its own tests is compared against the selection the same predicate measures three
    lines away, and the browser rung's claims are manifest-counted both ways. Where it is
    not, write the **enumeration**, which cannot go stale the way a bare numeral can, and
    which is the form every repair of this class in v3 has chosen. Two things make this
    its own row rather than a habit: the counts that survive a sweep are the ones in the
    crates, because a sweep of the *documents* does not think to ask which source file
    carries the same transcription; and a tree-wide predicate for the doc-comment half
    was priced at this gate and **declined** — the narrowest matcher that catches the
    shapes found selects a population of doc lines far beyond the instances, and the
    population small enough to afford, the lines inside `closed_vocabulary!` blocks, held
    no instance of the defect at all — so that half is review, said out loud, rather than
    a gate nobody wrote. `test` `review`

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
      two rules apart loaded [S:E4][S:G6 M29, N135] — **and in every surface that composes
      a write from the same descriptor**, the web client included, whose panel chose a
      widget from `type.kind` and sent an `int` to a payload control for a phase
      [S:N312]. The question to put to a law written at the backend trait is not "are
      both backends right" but "who else reads this descriptor and decides something with
      it"; the browser arm is *a compound control shows the bytes the device reported
      and offers nothing to write*, over the widest committed profile, with a plain scalar
      of the same declared type beside it. `test`
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
- [ ] The token gates exactly `CAMERA_BEARING_PATHS`, however many entries that is — D20's
      `/session-photo` landed at P9b and joined it — and a route
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
      renders from DTOs; a prose count of its files is A19's class, mechanical here as it
      is in a criterion row's count of its own tests — reconciled by a test, or not made
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
- [ ] **An identifier an answer carries is a selector a caller can build** [S:N325] —
      every identity field a `--json` document or a wire answer publishes has a spelling
      `schema::selector::parse` accepts, or it is a *counted* exception carrying the
      reason nobody can round-trip it: the primary consumer has no hands, and a value it
      can read and cannot write back is a call sequence in disguise. The population is
      `SelectorScheme::ALL` over every camera the corpus replays, each spelling rebuilt
      out of the serialized listing entry, so a scheme added tomorrow is either read
      back or listed — and the one live exception, a `usb_id` answered as two integers
      and parsed as hex, goes red on its own exception count the day the owner rules on
      the shape. `test`
- [ ] **Selection never filters enumeration** — ids are stable under any selector (D1's
      ordinals are assigned over the whole machine); a backend constructor stays
      filter-free. `test` `review`
- [ ] **The address/identity split is documented where it is read**: `NodePath` resolves
      per call against the live listing [PF:22], and the guide's selector table carries
      the split — a `Do`-column claim, tested as one (A15). `test`
- [ ] **The projection is closed by destructuring** — a field added to **any struct
      carrying the partition** fails to compile until it is sided, and the population is
      every function that projects identity from description, not the one the design
      names: `profile-partition-is-closed.sh`'s declared rows are the authority and
      today they are `DeviceProfile::compare`, `CameraInfo::differing_fields`,
      `CameraFingerprint::differing_fields`, `DeviceDifference::sections` and
      `InvariantDifference::sections`, each read for its `let` pattern, each refused a
      `..` rest, and the rows counted rather than described [S:N323]. The comparison
      names sections and slugs; the corpus is walked as mutual negatives (every profile
      device-differs from every other, sections named) and identity-rewritten positives.
      The old private test mask is deleted, not kept beside the product's [A5]. `CI`
      `test`
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
      table rather than restating it. **The column is also a claim about each Yes module's
      own surface** [S:N324]: a Yes module whose headline verb takes or returns a
      No-column type makes the column a fiction one module over from where the facade's
      own walk looks, so the population is every `use crate::` and every public signature
      of every **Yes** module of every crate the table classifies module by module,
      derived from `gate_pub_mods` filtered by the table so a module joins the walk the
      day it joins the column. The residual is stated rather than implied — a Yes module's
      submodule files are not walked, and an item re-exported under another name is read
      as the name the file writes. `CI` `test`
- [ ] **The workbench's layout claim is asserted at the pinned viewport over the widest
      committed profile** (vivid's 77 controls) — the preview's box does not move and a
      control card is wholly on screen at every scroll position; a friendly-profile pass
      is the fixture-one-parameter-away smell (Part C). **The other half of D20's
      sentence — that the control being *adjusted* is visible there — is asserted nowhere
      at a scroll position, because it is not true of this fixture** [S:N333]: the widest
      profile's run of unwritable compound cards is taller than the pinned viewport, so
      the strong instrument would go red on a page that satisfies D20, and the manifest
      says so in the row's own words rather than leaving the gap to a reader. A rubric
      row demanding an assertion the tree has measured to be false of it is this gate's
      own row-truth class [S:N335, N336]. `test`
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
  prove — the §3.3 register and the standing hardware caveat — **and what the gate in
  force does not run** [S:N318]: read the phase's own block of `phase-criteria.tsv` and
  say which predicates it adopts without self-testing them, because a phase can close on
  predicates nothing in that block has proven able to go red. Two v3 blocks opened
  without the suite-and-selftest pair every block before them opened with, and the
  difference between a criterion and a habit is the difference the table exists to
  record. **Then name the population each class will be walked over, and what closes it**
  (rule 7): an exhaustive `match`, a generated `ALL`, a selection derived from the tree.
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

G1–G6's record — the reconciliations, the "predicted / named-but-under-specified / no-row"
analyses, and the counts — lives in docs/8 v2, preserved under `docs/historical/` at
adoption and cited from the rows above. G7, G8 and G9 are **one review and three entries**,
because the three gates close separately and a row's silence here is a claim about one
phase at a time.

**The review all three entries are about.** One session over the whole of P7–P9, run at
`5496c02` to Part E, six lenses over populations named at preflight: **128 candidates
generated, 102 killed by the lens that raised them** (80%, against G6's 204 of 290 = 70%),
26 reported to independent verifiers whose default is REFUTED, and **26 verified: 9
confirmed as stated, 15 narrowed, 2 refuted**. Its dated evidence entry is E22, written
before this reconciliation as Part E asks. The repairs landed as one commit, `f9abe48`, with
notes **N310**–**N334**; at that commit the twenty-six dispositions are **19 repaired, 4
partial, 3 accepted**. Three of the four partials say in their own note what they did not
close; the fourth's residue — the false count `f9abe48` took out of the criterion row and
left standing in docs/15, with nothing anywhere able to go red on the next one — is closed
by the commit that carries this record, under notes **N339** and **N340**. The entries below
split the twenty-six by **subject** rather than by lens — eleven G7's, four G8's, seven
G9's, and four that span two gates and are argued once, under the gate whose block their
absence leaves thinnest. Splitting by subject is stated because a record that does not say
how it divided its population cannot be reconciled against the review it summarises.

---

**G7 (P7 — selection, the identity/device partition, and the facade; eleven findings; `g7`
closes at 25 criteria rows, measured over `scripts/gates/phase-criteria.tsv` at
`f9abe48`).**

**Predicted by the rubric — and the answer is *yes, twice, and it did not help*, which is
this record's most useful shape and its second appearance after G4's.** B12's facade row is
the strongest instance the v3 rubric has produced, because it does not merely name the
class, it names the *reader*: the ban is "on the class of reach rather than one spelling of
it: a grouped import, a restricted visibility, an `extern crate`, a glob and a sibling
module are the same reach, and the import reader is shared rather than copied —
`rust-imports.awk`, note **N271**". That row was written out of N271 at `962dff6`, two days
after the v3 issuance carried A17 in its unwidened form: the class row applied to one
surface, naming the shared reader before A17 itself asked for one — which is the gap P3
below closes. Three findings are instances of its class inside the two predicates it
cites by name and the third it descends from. `facade-is-the-composition.sh` derived its
encapsulated population from one spelling of a *call*, so adding `use crate::resolve;` to
the facade and dropping two `crate::` prefixes moved the population from seven modules to
six with exit 0 and no sentence named — and with that facade in place the executor's own
second assembly of the engine, the bypass this predicate is the whole commission for, ran
green (**N315**). `facade-stability-table-sync.sh`'s module-scope walk enumerated
`fn|type|struct|enum|union|trait|const|static` and missed the fourth spelling of a
reachable public item, a method on an inherent impl, which hands an embedder a **No**-column
type with both facade gates green and a counted summary byte-identical to the unseeded
tree's (**N316**). And `avi-reparse-is-independent.sh` — the file N269 had held up as "the
house precedent this missed" — was still carrying the matcher N271 disproved, blind to a
sibling stem wherever rustfmt's fill puts it first on a continuation line (**N317**).

Why a written row did not fire, in four parts, none of them "the row is badly written".
G4's three apply unchanged: a review instrument is read *after* the matcher is chosen; the
row's worked example wears costumes the defect did not — its five spellings are all import
syntax, and two of the three findings are about a call and an `impl` block; and rule 1
wants a named class to become a lint, a job or a test, and this class's only mechanical
proxy is the predicates' own selftest arms, which the repairing session wrote in the same
spelling it had just fixed. G6's fourth applies with a number against it — **a rubric row
names a class; only a walked population finds an instance of it** — and here the population
was closed and cheap: a grep for import-shaped matchers over the 47 shell scripts in
`scripts/gates/` returns exactly three predicates that ban a module reach by reading `use`, two
sharing the reader and one not.
Nobody walked it until the review did, and walking it cost one command.

The second *yes, and it did not help* is Part C's v3 addition, **the test that measures the
helper** and its sibling **the self-referential expectation** [S:N252]. B12's selection row
names its population correctly — the vocabulary's `ALL` over the committed corpus — and the
walk is real: nine readers of the vocabulary, all reached. What no walk reaches is the one
member every reader *renders*. Changing `SelectorScheme::example()`'s `BusPath` arm to
`"buspath:<interface-path>"` and leaving `parse` alone left the workspace at `5496c02`
green — `1688 tests run: 1688 passed, 39 skipped` — and the binary built from that tree then
refused `buspath:3-4:1.0` while teaching the caller to spell it that way, in the D13
refusal that exists so an unattended agent learns the grammar, in `info --help`, and in all
seventeen `<CAMERA>` argument rows of `docs/agent-guide.md` plus its `How to name a camera`
table (**N320**). Every test touching `example()` but one asserts
`rendered_message.contains(scheme.example())` — the renderer measured against its own
input, so a mutation moves both sides together. That is Part C's row, verbatim, at five of
the six test-side uses of `example()` in the tree, spread over four of the five crates that
touch it, two of them in the test module of the crate the row's own citation governs. The
sixth, in `xtask`'s guide suite, is not that shape and constrains the vocabulary no better:
it reads `example()` as a needle into a hand-written glossary row, and that row teaches no
spelling and points at `How to name a camera` instead, so the count it asserts was zero
before the mutation and zero after. It is also **A8's
converse for the third recorded time**, after G4's F2 and G6's L15–L18: a string the
*product* reads that no test constrains. The row asks both halves; the half nobody asks is
still the same half.

**What the rubric named but under-specified, and what its rows now say.** B12's
projection row is written as "a field added to `ProfileInvariant` fails to compile until
sided". D15's partition is five projections over five structs, not one: `DeviceProfile::compare`,
`CameraInfo::differing_fields`, `DeviceDifference::sections`, `InvariantDifference::sections`
and `CameraFingerprint::differing_fields`. Four are closed by destructuring and
`profile-partition-is-closed.sh` counts them; the fifth reads `self.bus_path`,
`self.usb_id`, `self.card`, `self.driver`, `self.serial` by field access, and it is the one
`engine::snapshot::restore` reads. A `pub firmware: Option<String>` added to
`CameraFingerprint` compiled after the 24 struct-literal sites the compiler demanded, ran
`1688 tests run: 1688 passed`, and left `run-all.sh` at `43 predicates green` with
`profile-partition-is-closed` printing `checked 4 partitions` — the gate green for the
reason it exists to catch (**N323**). The compiler asked twenty-four times "give this field
a value" and never once "which side of D15's partition is it". The row named one struct
where the law has five of them; P6 lands the population on that row instead.

B12's facade row also carries the stability-table clause — "an engine module the facade's
*public surface* forces on a caller is in the **Yes** column" — and it is written about the
facade's surface alone. The Yes column is a claim about *each Yes module's* own public
surface too, and one module over it was false: `engine::photo` is **Yes**, and
`photo::from_capture` took a `crate::capture::Capture` an embedder holding only the Yes
column cannot name, while `photo::take` returned a `Taken` carrying a `crate::preview::Gap`
— which `Facade::photo`'s own method doc says the opposite about, in as many words ("the
type never crosses this boundary"). The candidate called that sentence the module doc and
the verifier refused the framing: it sits at `facade.rs:297-300`, it is true of the facade's
own boundary, and the finding stands on the Yes column rather than on a contradiction. The
population was closed rather than sampled: 36 modules across engine, imaging and testkit,
every `use crate::` in every Yes module, 8 statements, two Yes-to-No imports of which one is
clean (**N324**). Proposal P7.

A17 is the third row to reword. It says "derive the class or enumerate its spellings in one
place" — and N316's repair enumerated in one place, and the enumeration was three-quarters
of the class. **An enumeration in one place is still an enumeration**; where a class has a
syntactic form, the derivation is a shared *reader*, which is what N271 concluded and what
this phase then failed to apply three times. Proposal P3.

**What the rubric had no row for — a derived list and a hand-written one in the same file,
with nothing between them.** `facade-is-the-composition.sh` computed the exports no file
under `crates/cli/src` calls and printed "new backend watch"; `new` is called at
`crates/cli/src/main.rs:84`, the composition root's one construction of the facade, and the
matcher could not see an associated-function call. The same paragraph's header sentence
named "`watch`, `open_id` and `backend`" as the embedder-facing three, and `open_id` is
reached at `main.rs:382`. Each list was wrong about one name, in the opposite direction
(**N315**). The note's own advertised purpose is that "the gap is *visible* in the output,
which is where the next person deciding whether an export earns its place will look" — so
what the next person read was that `Facade::new` earns no place in the root that cannot work
without it. This predicate had already ruled on the shape for its other policy list, which
is claim 4's whole reason for existing (N269: "The executor's own doc comment wrote the same
list in prose and wrote **six** where the policy had seven"). Two more findings are the same
class with the derived side absent altogether: the review found two prose counts of
`SelectorScheme::ALL.len()` surviving N308's sweep, one of them a hundred and thirty lines
above the vocabulary it counts, and the repair that walked the population found five
(**N319**); and a doc comment on `MAX_PHOTO_DECODE_BYTES` said "a **72**-byte PNG" where
N268 and the committed `png_declaring` builder say 68 — both written in `a71748a`, the first
of the nine commits, which `7f37bbf`'s counts sweep then reached everywhere but there (the
half **N319** opens with). B7 carries "prose counts of its files are reconciled by a test or
not made" [S:N153, N158] and scopes it to the web client's assets. The law is general, it is
AGENTS' law, and the instance that got through sat in a doc comment in
`webcam-handler-schema` — a crate whose doc comments AGENTS names as inputs to committed
artifacts, though this constant's is not one of them: `grep` finds no trace of the figure
under `schemas/`, so not even the artifact reconcilers could have gone red on it. Proposal
P4.

The second no-row class is **an identifier an answer carries that the caller cannot spell
back**. A USB id is two spellings on this tool's surfaces — the candidate said three and the
verifier settled it at two, because the hex string the selector grammar takes is
byte-identical to the one the human table, the EXIF `ImageDescription` and the guide print —
and the one the primary consumer parses, `usb:04f2:b83c`, is the one it cannot build from a
listing: `schemas/webcam-handler-openrpc.json` declares `params.camera` as that string and
`result.info.fingerprint.usb_id` as an object of two uint16, so neither an agent nor the
usb-teleporter harness can substitute one into the other. This is an **acceptance pending an
owner ruling** (**N325**), not a repair, and the residue is stated on the wire rather than
hidden. AGENTS states the consequence the class offends — *the primary consumer has no
hands* — and no rubric row asks it. Proposal P9.

**What the review did not find, and it matters equally.** Every camera-taking surface takes
a selector and not an id: **21** `camera` parameters in the T5 trait declaration in
`crates/api/src/lib.rs`, and **12** of `Facade`'s **17** `pub fn`s taking a camera at all —
every one of them `CameraSelector`, the other five naming no camera except `open_id`, whose
whole subject is an id already resolved. Nothing in `crates/web/assets` spells a selector:
`bus:`, `usb:`, `serial:` and `/dev/video` have zero hits over its twelve files, and the one
occurrence of `cam:` is a doc comment's example id, explaining why `previewUrl` encodes what
the daemon handed it. Nine readers of the vocabulary walked, all consistent after N303's and
N308's repairs except the rendering side above. Exactly one module-scope `pub` item in the
product half of `facade.rs`, which is why N315's finding is a predicate hole and not a
shipped defect. No `pub fn` on `Facade` narrows its seam's answer except `photo`, with two
candidate narrowings checked and killed against N30's rule and `DiscoveryReport`'s own
fields. No Yes-column module in `webcam-handler-imaging` or `webcam-handler-testkit` reaches
a No-column module. The imaging Yes row's "no clock and no file" is true — zero hits for
`std::fs`, `File::`, `Instant::now`, `SystemTime::now` over every product half in that
crate. `crates/cli/src` never calls `Facade::backend()`, so the escape-hatch-bypass
candidate has no instance. And `Facade` really is shareable, because
`CameraBackend: fmt::Debug + Send + Sync` is declared at the trait. Two of those absences
are candidates the lens killed itself and are recorded because a killed candidate is a
place somebody has already looked.

One non-finding is worth the space, because it is Part C's load smell used in the refusing
direction. Two daemon calibrate tests failed once, with `settle_timeout … 5399 ms (11 frames
seen)`, and the lens declined to report them: for an ordering, a run is evidence only with
its load stated, and a single red under an unstated load is no more a finding than a single
green is a refutation.

**Refuted, and recorded anyway.** docs/13's P7c *Proves* bullet still promises "the arm is a
compile-fail fixture" where what landed is `profile-partition-is-closed.sh` — because the
compiler already refuses a field nobody sided, so a `trybuild` harness would be a test that
the compiler works. The candidate's load-bearing claim was that no dated amendment exists;
two do, at docs/13:88 (the commit-named execution record, which says the criterion "landed as a
predicate rather than as the commissioned compile-fail fixture") and docs/13:146-153 (the
phase-wide paragraph ending "the tsv is the one that runs"), and the finder's census reached
neither. What
survives is a reader who opens P7c and stops there meeting a mechanism the tree rejected,
which is a P7e edit and not a defect class.

---

**G8 (P8 — stream stats, photograph comparison, the device-loss contract; four findings;
`g8` closes at 22 criteria rows, measured at `f9abe48`).**

**Predicted by the rubric.** A14 fired, in the sense that matters least and the sense that
matters most at once. A14 — "a test drives the bound — **from both sides**" [S:N255], and
"the bound is checked at the door, before a motor moves" [S:N147] — had a
bound, a constant, a reader and a two-sided test — and the bound was at the *decoder*.
`photo diff` handed a caller-named path to `std::fs::read` and allocated the whole file
before `MAX_PHOTO_DECODE_BYTES` was consulted, so N268's forbidden failure shape was still
reachable through the first door rather than the named one (**N322**). Beside it, the budget
refused through two doors and only one of them said so: a 68-byte PNG declaring 200000×200000
answered the sentence N268's arm asserts, naming the format and the budget, while a PNG
declaring 134217729×1 — one pixel of row past 512 MiB — answered `png decode failed: Memory
limit exceeded`, which is the answer an unattended reader cannot tell from a host that is
out of memory. Turning the construction-time ceiling off (`budget.max_alloc = None`) left
`1688 tests run: 1688 passed` at workspace scope: a survivor nothing drives (**N321**).
**A14's "both sides" clause is satisfied by one door, and the row does not ask how many
doors a bound has.** Proposal P2. That the class is real and not one verb's is N329's
measurement: after N322's repair, `--json restore … BIG` and `--json --profile BIG list`
were still killed at exit 137 with zero bytes on either stream, against a 3 GiB sparse file
under a 1 GiB cap.

**What the rubric named but under-specified: a population closed on one renderer and left
open on its sibling in the same file.** Every field of `StreamStats` (7), `IntervalStats`
(8) and `RecordReport` (9) traces to a producer, an assertion and a `required` entry in the
committed schema — the document is closed. The human `record` table showed none of D16's
instrument: `gap_events`, `wall_clock_skew`, `sequence_resets` and `clock_reversals` had
zero hits outside `webcam-handler-schema` and `webcam-handler-imaging`, while
`render.rs`'s own doc claims the two halves show the same facts, and the sibling
`render::photo` renders all nine of `PhotoReport`'s fields (**N326**). That is the
same class as G7's finding about `CameraFingerprint`: **a population closed on one of two
siblings**, in two crates, in one review. Part C has *the repair that copies a guard without
the guard's test* [S:N253]; its mirror — a closure the sibling never got — has
no row. Proposal P8.

**What the rubric had no row for.** A `g8` criterion stated a count of the tree that was
false: "The row above selects nine tests" where the selection lists ten, in the file whose counts
the immediately preceding batch had swept (**N318**). Its disposition at `f9abe48` was
**partial**, and the record says why: the instance was deleted and the class was left
ungated — nothing reconciled a count written in a criterion's `what` field against the
figure `counted-selections.sh` measures three lines away — while a second copy of the same
false count stood on at `docs/15-…-gates-v3.md:172`, in a file `f9abe48` itself revised.
**Both are closed by the commit that carries this record**, which is rule 1 working a commit
later than it asks rather than not working at all. `counted-selections.sh` reads every
cardinal that qualifies `test`/`tests` out of a `tests` row's own prose and compares it
against the selection it has just measured, stated as a class rather than as the phrase that
was caught — digits and words alike, hyphenated compounds included, anywhere in the cell,
with a rate exempted and the exemption driven by a passing arm (**N339**). The adversarial
reading rule 8 asks of every repair then found that repair's word reader keeping a comma at
both ends of every word, so finding #21's own sentence walked through the new ban the moment
a clause ran on after it, and widened the reader (**N340**). docs/15's copy of the figure was
corrected in the same commit and now says what the row holds. This is the third instance
in this review of the class AGENTS states as "a prose count of code is a claim something
reconciles, or it is not made", and it is why P4 landed against Part A rather
than against B7.

**What the review did not find.** Four generated vocabularies walked by their own `ALL`:
`MetricName::ALL` (5) through `measure`'s exhaustive match, `compare::photos`' delta walk,
`render::photo_comparison`'s table walk and an `ordered_fixtures` match with a
`directed + 1 == ALL.len()` non-vacuity count; `PhotoFormat::ALL` (3) reaching both the P5
and P6 branches of `read_netpbm`; `Fault::ALL` (12) through two exhaustive matches with
`no_fault_fires_unless_it_was_scripted` as the inverse and the four delegated variants
grep-checked into the daemon's subscription suite; `CapReached::ALL` (3) walked against
`VideoFormat::ALL` with a `checked == 2 * 3` count. No NaN and no non-finite value is
reachable in the `--json` comparison document — measured through the shipped binary on 1×1
and 4×4 Netpbm fixtures, degenerate for a 3×3 Laplacian and an 8×8 SSIM window, which
answered `sharpness 0.0` and finite SSIMs. And docs/11 H1's family was checked a third time
and held: the real camera's verbatim MJPEG bitstream is readable by `photo diff`, measured
on `/dev/video0` at 2592×1944 against the same camera's PNG.

One absence was **deliberately not reported**, and the reason is the settled-registry rule
working. No `Fault` variant drives `sequence_resets` or `clock_reversals` end to end,
because `Fault::ALL`'s only delivery-shaping variant is `FrameGap`. `FrameLedger`'s own doc
states the decision — those two are measurements rather than contract, per D16 and rule 6 —
and `imaging::stream_stats`' suite asserts both in both directions. A reviewer who greps the
tree for the subject before writing the candidate finds the ruling, which is what Part E's
grounding bullet is for.

---

**G9 (P9 — the operator's workbench; seven findings; `g9` closes at 12 criteria rows,
measured at `f9abe48`).**

**The P5 lesson first, because P9d states it as a condition of the close: a web-client
review's reconciliation is written, or the gate is not closed — G5's absence cost five
recurrences one gate later. It is still costing.** Four of G9's seven findings name a class
G5's review recorded in 2026-08-13/14, and none of those classes reached a rubric row,
because the reconciliation that would have carried them there was never written.

- G5's **M6** — "Eleven of `widgetFor`'s fifteen arms have never been rendered in a browser;
  the fixture carries four control types" — was left **Open**. G9's finding is the same
  unwalked population one gate later, with the aggravating fact that the fixture arrived in
  between: `corpus/profiles/vivid.json` entered the tree for P9b's *layout* fixture, the rung
  paints its 77-control panel on every run, and `wideCameraId` occurs in `client.spec.mjs`
  only in the two layout claims. The panel is painted and never inspected (**N312**).
- G5's **M7** — "A departed selected camera leaves the panel, the session list and the
  status lines stale and silent" — was left **Open**. G6's M32 closed the panel and the
  session list by carrying the camera as an argument (N154). G9 found the two elements that
  repair did not reach: the photo slot, **M32's fifth element and the one painter with no
  fence**, which paints a 2592×1944 MJPEG answer from the camera the operator walked away
  from under the next camera's card, labelled with a negotiation the camera on screen
  refuses (**N310**); and D20's flow, which did not exist at G5, where a camera switch
  strands the open session — `#flow-status` goes on naming it, every verb about it is
  disabled, and there is no way back to it (**N314**).
- G5's **H4** and **H5** — a page writing the right diagnosis and then destroying it, and a
  page still acting on a closed socket — were both **Fixed**. The class recurred in the
  writer the fix did not cover: a `wch_list` in flight when the socket dies rewrites the
  page's final sentence with one that opens "connected", because `rpc.js`'s close handler
  rejects pending calls *before* it calls `onClose` ("the order is the point"), so
  `socketClosed` writes synchronously and `listRefused` paints over it in the rejection's
  microtask (**N311**). `app.js`'s own header records this element being overwritten wrongly
  twice already; this is the third, the verifier found a fourth writer in the same session
  (`watchDevices`' rejected `subscribe`). A fifth writer of the same class sits outside
  `#connection` and is still unguarded — `calibration.js`'s `catch` writes "the daemon refused a
  sweep subscription: the connection to webcam-handler-daemon closed" nine lines below that
  file's own `SOCKET_CLOSED` guard — and no note in the tree records it; this record is where it
  is written down.

G5's own record introduces its MEDIUM and LOW findings with "Recorded so the next session
does not rediscover them". That is the record doing exactly half its job. The next session
did not rediscover them; it also did not fix them, because nothing in the rubric had grown a
row; and the session after that rediscovered them at a gate close. **A finding recorded
without a reconciliation is a finding filed where only its own reader will look.**

**Predicted by the rubric — yes, and it did not help, in the sharpest instance this review
produced.** B12's workbench row names its fixture exactly right: "asserted at the pinned
viewport over the widest committed profile (vivid's 77 controls) — preview and the adjusted
control simultaneously visible at every scroll position; a friendly-profile pass is the
fixture-one-parameter-away smell (Part C)". The row fired. The wide profile is loaded, the
claim runs at 1280×720, and the friendly-profile smell it warns about was avoided. **And the
claim was passing *because* of the defect.** Its instrument was `#column .control input,
#column .control select`, and three of `vivid`'s ten `HAS_PAYLOAD` cards carried a number or
text field only because the panel chose its widget from `type.kind` and never from the flag
— widgets every write to which came back `device_io … (errno 22)`. The strong reading was
being held up by dead fields. Measured rather than reasoned about, at nine sampled scroll
positions of a real rung run, at the pinned 1280×720 over the 77-control profile:
`#column .control:has(input, select)` gives **3, 3, 2, 2, 5, 5, 6, 0, 2**, the zero being the
compound run, which is taller than a 720 px viewport — so
the strong instrument goes red on a page that satisfies D20 perfectly well, and the title
and its D20 quotation are what had to move (**N333**). **A row that names a fixture gets the
fixture; it does not get a reader.** The population B12 named was loaded, painted, and
inspected by nothing until this review put a browser in front of it.

**Where the rubric had a row and the defect landed anyway, the second instance: N135's law,
closed on both backends and never carried to the client.** B2's row says write dispatch is
"the **descriptor's** decision (`HAS_PAYLOAD`), never the caller's value variant — on both
backends, with the array-control fixture that can tell the two rules apart loaded"
[S:E4][S:G6 M29, N135]. Both backends honour it. `controls.js` branched on `desc.type.kind`, and
`grep -rn "has_payload" crates/web crates/daemon/tests` returned nothing — the flag sits in
`desc.flags.known` beside the three names `readOnlyReason` already reads. The consequences
were both halves of the law: the device's reported payload was dropped from the panel (an
empty number field under a note saying the device did not say where to draw a slider, while
the neighbouring card rendered `4 bytes · 18 00 00 00` for the same shape), and every write
the field could make was refused (**N312**). The precision the verifier insisted on is carried
here rather than softened: design §2.3's sentence is written at the *backend trait*, so
calling the client a second home of that law is a reading rather than a quotation. The
finding does not need it. Rule 6 alone carries the read half, and a widget whose every
gesture is an EINVAL carries the write half. The honest class sentence is **a caller that
chooses the value variant from the type rather than the descriptor**, and N135 is its
precedent rather than its rule. The row's population is "both backends"; the surface that
broke it is a caller. Proposal P5.

**What the rubric had no row for, and this is the record's most valuable output: nothing
asks who else paints this element.** The stale-answer fence is now three gates deep — G5's
M7, G6's M32 and L36 (notes N154, N156), and G9's N310, N311 and N314 — and it has never had a
row. Every
repair so far has been *per writer*: N311 says so in its own words, that the header names
five writers and a sixth added tomorrow inherits nothing, and finding 7's residue names one
already outside the guard. That is a class with no population and therefore, in G4's words,
**not prevention**. Proposal P1 is written to rule 7's shape: the row names the population
and what closes it.

The second no-row class is one gate-suite finding that is really a doctrine finding: a bound
landed in JavaScript (`SWEEP_ENDING_WAIT_MS = 2000`, whose own doc calls it "a bound rather
than a wait" and cites docs/11 H2), and the check that reconciles the page's bounds walked a
two-row array literal in `rpc.js` and never opened `calibrate-flow.js`. Its sibling in the
very same test file derives its population out of the asset and says why in its own doc —
"a **derived population**, which is what makes the partition below a claim rather than a
list". **One check in that file took the lesson and its neighbour did not, and the next
batch added a member the neighbour cannot see** (**N313**). No new row: this is rule 7 as
written, firing, and the reconciliation's job is to record that a rule fired where a person
had already written the reason down one function away.

Finding 17 is this phase's own repair one button along: P9c made the sweep non-re-entrant at
the door and left Start re-entrant, so two clicks end with a red `session_conflict` about
the session the page has just successfully created (**N314**). Together with G7's three
facade findings, **the-defect-one-spelling-on recurred in two unrelated subsystems in one
review**, which is why P3 is worth its space.

**What the review did not find.** Every button and writing widget the page ships was clicked
against a live daemon in the pinned Chromium, with the wire frames recorded: the union of
`index.html`'s id table and every click/change listener across the ten asset modules, and
all seven widget classes `widgetFor` can return. **No dead button**; two dead widgets, which
are the finding above. Every RPC the page issues — 14 distinct methods and 2 subscriptions,
grepped out of the assets — reconciles against the 22 methods and 2 subscriptions the
`wire_surface!` declaration generates, every name existing and every param set matching the
trait signature. Roughly 170 wire-field accesses across all ten modules check out against
the schema types, including the two enums whose discriminant is not `kind`, and **all eleven
`switch`es over device or wire vocabulary carry a payload-carrying `default` arm** — A3 held
everywhere in the client. Every `/preview` and `/session-photo` response through a whole D20
flow was recorded: three previews (two 503s for the MJPEG-less camera, rendered as a named
refusal listing what would be accepted) and eight session-photos, all 200.

Two observations the lens declined to report belong here, because this record is the only
place they are written down. `app.js::enumerate` has no fence and two concurrent `wch_list`
calls **are** reachable — measured, both answers held and released newest-first — but the
fake's enumeration is static, so both answers are byte-identical and no stale paint can be
*shown*: the arrival is measured and the wrong paint is an argument, which is the right
threshold and the wrong conclusion to forget. And the sample grid's `<img>` elements carry
no `error` listener, so a `/session-photo` 404 is an invisible tile an operator can still
click and have recorded as `chosen_by: human`. Neither is a note; both are now on record.

One honesty item about the review itself: the G9 lens declares five populations and writes
four of them out. The fifth is not recoverable from its record, so **G9's absence list is
one population short and this entry does not claim otherwise.**

**Refuted, and the refutation is the P5 lesson paying out.** The `g9` criterion asserting
that the workbench is asserted in a browser does pass on a host with no node — the test
takes a bare `return` from `preconditions`, and driving its filterset with node removed gives
`1 test run: 1 passed`. That much is true and it is **not new**: it is recorded in N44's
closing sentence, which names the Playwright rung explicitly, and again in docs/15's
"Counted is not run" under a heading whose preamble reads "each line exists so a green run
is not read as more than it is", and AGENTS states the same posture as settled. The half
offered as new — that unlike its `g8` sibling the row's sentence does not warn its reader —
rests on a premise the verifier disproved by running it: the decline report prints in full
under the row's headline, naming what was missing and counting every claim and assertion not
run, because `.config/nextest.toml` gives `binary(web_browser)` `success-output = "final"`
and `phase.sh` inherits it. **A candidate died against a residual somebody had written down
twice.** That is precisely what the four recurrences above did not have, and it is the whole
argument for this section existing.

---

**The gate-suite findings, argued once: `g8` and `g9` opened without the pair every other
block opens with.** `run-all.sh` and `selftest.sh` appear in `phase-criteria.tsv` exactly
sixteen times, twice each for `g0` through `g7`, and zero times for `g8` and `g9`. So
`just gate-g8` ran four predicates and `just gate-g9` three, **none of them self-tested by
the gate that adopts them** — P8's and P9's own predicates would have closed their phases
without ever being proven able to go red, which is rule 6's requirement defeated by a row
set rather than by a missing arm (**N318**). Three siblings ride with it: no `g7` or `g8`
row ran `smoke-hw.sh`, though two v3 phases landed new R3 recipes; no `g7`, `g8` or `g9`
row ran `schema-artifacts-current.sh`, though P8a, P8b and P9 each moved `schemas/`; and `g8`
had no `agent-guide-current.sh` row though P8b's verb reached `docs/agent-guide.md`. The g7
halves are inert and the verifier said so — `g7` carries the `run-all.sh` row, which reaches
every predicate in the directory — so what survived is `g8`'s and `g9`'s. The repairs landed
`run-all.sh` and `selftest.sh` in both blocks, `smoke-hw.sh`, `schema-artifacts-current.sh` and
`agent-guide-current.sh` at `g8`, `schema-artifacts-current.sh` at `g9`, and `cli-parity.sh` at
`g7` for P7d's delegated claim. Two things are left standing rather than closed: `g9` still has
no `agent-guide-current.sh` row though P9 moved the guide, reached only through that block's new
`run-all.sh` row; and `phase.sh` still asserts nothing about a phase's row set —
its only row-set claim is the zero-rows branch — so completeness reaches a phase gate only
through the `run-all.sh` row each block now carries, and a future block omitting both would
be caught by `just ci` and not by its own `just gate-gN`. The fourth spanning finding is the
mutation floor's exclusion of `facade.rs`, whose written reason named `cli-parity.sh` and
`binary(facade_equivalence)` — neither of which can move a byte when `backend` or
`watch` change, because no file the CLI ships calls either and `facade_equivalence`'s
populations are seven read verbs (**N327**). The candidate said three exports and the verifier
returned two: `crates/cli/src/main.rs:84` does call `Facade::new`, and the predicate's own
unreached list could not see an associated-function call — which is G7's unreached-export finding
arriving from the other side, in a second file. Rule 8's one-command check [S:N167] found it in
one command, for the second gate running. Its disposition is **partial**: the amended reason
is still a prose claim about code that nothing reconciles, and that absence is accepted in
writing rather than closed.

**The repair session was reviewed, and twice the first repair was the defect one spelling
on.** G6 measured this sentence for the first time — three of eleven repair commits shipped a
regression a green `just ci` could not see — and P7–P9's repair session measured it again,
before committing rather than after. The facade predicates were taught to resolve a call
through the local name an import binds, and were still blind to `use super::resolve;` and to
`use crate::resolve::{self as r};`; reading a module reach now goes through one home,
`rust-imports.awk`, which reroots before it flattens (**N328**). And the caller-named-file
bound landed refusing by `stat(2)` size — "the readable length of a regular file and of
nothing else" — so `photo diff /dev/zero /dev/zero` was still OOM-killed with no document at
all: the door the bound named was not the first door, one spelling of the *file* on
(**N329**). **Both were caught by an independent reader of the diff, not by the
suite, and both were repairs to findings whose own class was "the ban names one spelling".**
Rule 8 and A17 are the same instruction read at two distances, and this is the evidence for
saying so.

---

### Rows this record landed

Every row this record argued for is a row now, landed by the commit that carries the record,
and an eleventh was found while landing them. They are named here by where they went and by
what the landing changed about them, not by what they say: the sentences belong to the rows,
and this document states each fact once.

**Part A gained two rows and three clauses.** P1 is **row 18** (`A18`), the stale-answer
fence, and it carries the amendment the landing forced on it: nothing in the tree derives the
population of painters and status writers, so the row states that population as owed and names
the derivation as the gate this class is still due, rather than claiming a fence the tree does
not hold. It also asks for a predicate that answers *is this still wanted* rather than for the
identity comparison one of its own instances refused, because an operator who walks to another
camera and back makes the ids match again over a slot `select` has twice emptied. P4 is **row
19** (`A19`), the prose-count row, and B7's web-client clause now cites it instead of being
the class's only home. P2 widened **A14** — the population of a bound is the doors, not the
values. P3 widened **A17** — an enumeration in one place is still an enumeration, so the
question to ask of any list of spellings is what reads the *form* instead. P8 widened
**A5**, for the reason below. And because row 18 cites the P5 web-client review, **P0**
landed first: the provenance block now defines the **[S:G5]** tag, which this document sent
readers to a record for and had never declared.

**Part B gained one clause and four movements in B12.** P5 widened B2's write-dispatch row to
every surface that composes a write from the same descriptor, the web client included, because
the question to put to a law written at the backend trait is not whether both backends are
right but who else reads that descriptor and decides something with it. In B12, P6 made
the projection row name the five functions that carry D15's partition and point at the
predicate that counts them rather than describing them; P7 made the facade row say that the
**Yes** column is a claim about each Yes module's own surface, with the two residuals stated
rather than implied; P9 added the row that an identifier an answer publishes is a selector a
caller can build, landed as `test` and not `review` because the walk over `SelectorScheme::ALL`
already exists and the owner's ruling on **N325** decides only whether `usb:`'s exception
survives it; and P11 repaired the workbench row, for which see below. P10 made **Part E's
preflight** ask what the gate in force does not run, because a phase can close on predicates
nothing in its own block has proven able to go red.

**P8 landed on A5, not as a new smell in Part C.** Part C is *Tests that actually test*, and
every smell in it is a smell in a test, a fixture, a stub or a gate. P8's two instances are
none of those: an identity fingerprint compared field by field beside destructured neighbours,
and a human record table showing one of D16's delivery numbers under a doc claiming both
renderings show the same facts, are **product** functions, and the finding in each case is that
the law's home existed one function away and this function did not route through it. That
sentence is already A5's — *a second copy is a finding; so is a bypassing caller* — so the
finding is the sharpest instance A5 has had at v3 rather than a smell of its own, and giving it
a second home in Part C would have been this document breaking the §2.10 argument it makes.

**P11 is the row this record's argument required and its ten proposals did not contain.** B12's
workbench row demanded that the preview and the control being *adjusted* be simultaneously
visible at every scroll position. Note **N333** retired that sentence at `f9abe48`, the commit
this session began from, having measured that the strong instrument goes red on a page that
satisfies D20 perfectly well: the widest committed profile's run of unwritable compound cards
is taller than the pinned viewport, and the writable-card count at the nine sampled scroll
positions was `3, 3, 2, 2, 5, 5, 6, 0, 2`. So the rubric was asking for an assertion the tree
had deliberately stopped making, and `claims.json` already recorded why. The row now states the
half that is asserted and says out loud that the other half is asserted nowhere at a scroll
position, with the reason. That is **N335** and **N336**'s class — a row describing a tree the
repository does not have — found in this document by the reading that found eleven of them in
`phase-criteria.tsv`, which is the argument for reading the rubric the way the criteria table
was read, at the next phase and at every phase after it.

---

### What closing each gate does not prove

**G7.** Closing `g7` proves that every spelling a caller holds resolves through one parser
and one resolver, that the projection is closed by destructuring on all five of the projections
that carry it, and that the executor crate's only engine reach is the facade — over this machine.
It does not touch §3.3 item 8: all hardware evidence is one machine, one kernel series, and
the cameras attached to it, so every `bus:`, `usb:` and `serial:` answer the R3 selector recipes
verified at four attached cameras (30 of 30 tests run, 30 passed, 96 s at `f9abe48`) is
that machine's vocabulary and not the class's. Item 11 stands untouched and is D15's whole
motivation: cross-machine comparison is corpus-shaped, the masked compare is asserted over
identity-rewritten pairs captured on one host, and "the forwarded camera describes itself
identically" is exactly the claim only the partner rig can measure. And the facade's
composition is proven by predicates over *source text*: N328's residual is stated — a
No-column type re-exported under a different name, or a signature assembled by a macro, is
still invisible to claim 6, and a Yes module's submodule files are not walked.

**G8.** Closing `g8` proves that the stream-stats accumulator, the comparison core and the
device-loss vocabulary are pure, total, bounded and walked over populations something
closes, and that D17's refusals are represented as data rather than thrown. It proves
nothing about causes. §3.3 item 3: USB bandwidth and multi-camera contention are not
modelled, and D16 makes their *symptoms* measurable without modelling their cause — E21's
first real numbers are three attached cameras on one kernel. Item 5: the muxer's
player-compatibility claim rests on ffprobe and mpv, which share one FFmpeg build and are
therefore honestly one parser plus a playability check. Item 4: the R2 vivid rung, green
this session at 9 of 9 over 77 controls and 83 formats through the blessed helper, proves
ioctl plumbing and not device quirks. And **item 9 is the one a reader must not
misread**: every sentence of D19 is `declared`. The five `hw_gone_*` recipes decline by name
on this host because the helper's interlock keeps real cycles camera-closed; the hermetic
twins prove this engine's behaviour under a scripted fault, not a device's behaviour when it
leaves. That word retires when a rig that can arrange real mid-stream loss contributes its
first E-entry, and not before.

**G9.** Closing `g9` proves that the workbench drives the daemon's verbs, that its refusals
render, that its layout claim holds at the pinned viewport over the widest committed
profile, and that `/session-photo` is camera-bearing, reference-addressed and gated. §3.3
item 7 bounds all of it: **the rung drives Chromium only, by ruling**, and Firefox and
Safari are unexercised, and B7's own row rules that a Firefox/Safari-only defect is recorded,
not necessarily fixed. Item 2's
D20 clause is the honest limit on the human flow: the rung proves the page drives the verbs,
not that a human picks well, and calibration efficacy on real optics is R3's alone. Item 6
stands where D20's door is: the privacy canary sniffs known formats and walks two
containers, so a frame in an unrecognised envelope passes it, and review carries that half.
And the rung is a **counted skip on a host without node** — `just ci` ran it here at 44
claims and 461 assertions, and docs/15's "Counted is not run" is the sentence that governs
every host where it declines. Counting is not running, at `g9` as everywhere else.

---

## One-line summary

Make every anticipated defect class fail a lint, a CI job, or a test that can actually go
red — prove the checks go red for the reason they claim (rule 6 and A16), walk every
class over a population something closes (rule 7), review the repair as hard as the find
(rule 8) — and hold the hardware doctrine: the device is the only authority on itself,
requested is not applied, unknown is represented, availability is not capability, the
camera is left as found, and no frame ever lands where its owner didn't point.
