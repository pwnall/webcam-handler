# webcam-handler — Implementation notes

Case law. Recorded, justified deviations from the doc series land here as numbered
**N-entries**; new hardware behavior lands here as **PF-entries continuing the design's
§1.2 registry**. Reviews do not re-report an entry; they retire one only on empirical
disproof.

Each entry states: what the doc says, what the repo does, why, and what would retire it.

**Before making a design trade-off, read "Expected usage" immediately below.** It is not case
law and it is not an entry — it is the thing the entries are *for*, recorded because until
2026-08-12 it lived only in the owner's head, and a trade-off judged without it is a
trade-off judged against a guess.

**Doc series versioning (2026-08-08):** docs/6–10 (v2) supersede docs/1–5 (v1, now under
`docs/historical/`; v2 preserves v1's section and registry numbering, so the citations
below still resolve). The v2 revision absorbed the design- and gate-facing halves of the
entries below and PF:13–16 into the current docs; each absorbed entry carries an
**Absorbed:** line naming the new home. Absorption does not retire an entry — these
remain the measurement record and the reasoning of record. Entries dated before this
line cite docs/1–5; later entries cite docs/6–10.

---

# Expected usage — who runs this, and what they are doing with it

**Stated by the owner, 2026-08-12.** Design §1 says what the tool *does*; this says what it
is *for*, which is the half that decides trade-offs. Nothing below is a new requirement — it
is the deployment every existing requirement was implicitly about, written down so the next
design iteration argues against the real case instead of an imagined one.

## The deployment

`wchd` runs on a computer whose one or more cameras are **pointed at a device under test**.
Two consumers reach it, and they are shaped nothing alike:

- **An AI agent harness — Claude Code or similar — drives the client to take photos of the
  device under test, to check its own work.** This is the primary consumer and the
  continuous one. The worked example the owner gave: *developing a display driver is
  validated by photographing the device under test's display.* The agent writes code, runs
  it on the device, photographs the result, and decides from the photograph whether what it
  wrote works.
- **The same agent also wants to record video of the device under test** (owner, 2026-08-12,
  amending the paragraph above). Photographs are "definitely the primary use case"; video is
  a "very desirable secondary" one, and it exists for the thing a photograph cannot answer:
  **animations and transitions**. A still frame cannot tell a correct fade from a stutter,
  or a 200 ms transition from a 2 s one — so the questions video answers are questions about
  *time*, and item 10 below is what that costs.
- **The owner uses the web client from time to time**, to check up on the cameras, and to
  **calibrate them at the beginning of a development run**. Occasional, interactive,
  supervisory.

**The development machine is deliberately not that deployment** (owner, 2026-08-12). It
carries every camera the owner could find — five logical cameras across four USB devices as
of today — each pointed in a random direction in a room, with **no device under test in
front of any of them**. The rig is chosen for *variety of hardware*, which is what finds
PF-entries, and not for fidelity to the deployment above. Two things follow, and both are
about how to read the evidence rather than about how to write code. Hardware evidence
(E-entries, the `hw_` suites) establishes what a *driver* does and does not establish
anything about a real run's framing or aim — so a motor left off-target on this machine
costs nothing today and would cost a whole run in the deployment, which is why **PF:25 is
recorded at its deployment severity rather than at the severity it had when it was found**.
And the privacy rule (item 8) binds *harder* here, not softer: a camera pointed at a random
part of a room is more likely to hold a person than one aimed at a circuit board.

## What follows for trade-offs

1. **The primary consumer has no hands.** A verb that needs a sequence of calls, a flag whose
   meaning depends on state the caller has to remember, or a failure that reads as prose is a
   defect *for the consumer that matters most* — not a rough edge. This is why the tool is
   built for agents to drive (design §1) and why `--json` fidelity and the T4 one-verb-once
   surface are product features rather than conveniences. It is also why the `wch`/`wchc`
   parity gate exists: the harness may hold either root.

2. **The product is comparability across time, not a good-looking picture.** Two photos of
   the same device taken an hour apart must differ *only where the device differs*. That is
   what calibration (D8), snapshot/restore (D4) and D3's guarded writes are for, and it is
   why auto-exposure and auto-white-balance are adversaries rather than features: they make
   the camera a variable in an experiment about something else. **A prettier default that
   moves between shots is worse than a duller one that does not.** When a trade-off is
   between image quality and repeatability, repeatability wins, and this paragraph is the
   reason.

3. **Byte fidelity has a named consumer now.** "Verbatim camera JPEG when the sink allows"
   (AGENTS, D6) is not aesthetics: the agent may diff two photographs or feed them to a
   vision model, and a re-encode inserts differences the device under test did not make. A
   pipeline that silently re-encodes is a pipeline that fabricates evidence in a test.

4. **The two consumers overlap, and that is the normal case rather than an exception.** The
   owner's preview tab and the agent's photo land on one camera at the same time — that is
   the ordinary Tuesday of this deployment, not a race to be documented. It is exactly what
   note **N83**'s suspend/resume was built for, and this section promotes that mechanism from
   a nicety to a core requirement. The same sentence covers exclusive streaming (D12), the
   `Busy` holder diagnosis, and every place two callers meet one device.

5. **Idle is the resting state.** Photographs arrive "from time to time" across a long run,
   so the camera is unheld for most of it. Open-on-first-use and close-when-idle (D12) are
   right, and a daemon that held the device continuously would block every other program on
   that machine for the large majority of the time nothing is being photographed.

6. **The failure vocabulary decides the agent's next move, and it decides it unsupervised.**
   AGENTS rule 7 — availability is not capability — is load-bearing here rather than
   fastidious: `Busy` means *retry*, `DeviceGone` means *stop and tell the human*,
   `PermissionDenied` means *this is a setup problem and no amount of retrying fixes it*. An
   error that collapses them makes the agent guess, and an agent that guesses wrong either
   spins against a camera that will never come back or abandons a run over a camera that was
   fine. D13's registry is the interface to that decision.

7. **Motors move the experiment, not just the lens.** A PTZ camera aimed at a device under
   test is aimed *deliberately*, usually by the owner at the start of the run. A sweep that
   leaves it pointing somewhere else has invalidated every photograph taken afterwards, not
   only the one being taken — the agent will keep working, and its evidence will silently be
   about the wrong thing. AGENTS rule 8 ("leave the camera as you found it") is protecting
   the validity of a development run, which is a stronger claim than tidiness.

8. **The privacy rule does not relax.** The usual subject is a circuit board or a display,
   not a person — and the rule stays exactly as written. The camera is in a room, the
   motor-moving opt-out (`WCH_NO_MOTION`) exists precisely because a camera can be pointed at
   people, and a default that costs nothing should not be weakened on the strength of a
   typical case.

9. **Calibration is a start-of-run activity whose result outlives the session that made it.**
   D9's inspectable session directories and "apply later" match the shape the owner
   described: calibrate once, interactively, then let many unattended agent-driven photographs
   use the result.

10. **Video is an agent-facing feature, and it re-prices P6.** The recording phase is last in
    the plan, which reads as "least important" and is not: an agent validating an animation
    needs it, and no photograph substitutes. Four things follow that a trailing nice-to-have
    would not have had to answer.

    **Frame timing is the payload.** For a photograph the timestamps are metadata; for a
    transition they *are* the measurement, because "did this take 200 ms or 2 s" is the
    question being asked. D7's close-time rewrite of the AVI header to the **measured** mean
    frame interval is therefore load-bearing rather than a tidy-up, and a recording that
    silently reports nominal fps while delivering something else is answering the agent's
    question wrongly. Whatever P6 does about a variable capture rate, it must not make a
    dropped frame look like a slow transition.

    **Byte fidelity extends to it, for item 3's reason.** MJPEG remuxed verbatim keeps the
    frames the camera produced; a re-encode inserts motion artefacts exactly where the agent
    is looking for them. A transcoding step would fabricate evidence about smoothness, which
    is the one property being judged.

    **The bounds are the agent's, not a human's.** A transition is seconds; the caps in
    `schema::limits` must comfortably hold one without an agent learning to work around
    them, while still bounding a runaway. A cap tuned for a human who notices a growing file
    is the wrong cap for an unattended caller that will not.

    **A recording and the preview collide in a way a photograph does not, and P6 owes an
    answer.** Note **N83**'s suspend/resume works because a photograph holds the stream for
    milliseconds; a recording holds it for the whole take, so the same trick would leave the
    owner's preview tab dark for seconds or minutes with no explanation. Item 4 says the two
    consumers overlapping is the ordinary case, so "the preview simply stops" is not
    automatically acceptable — the honest options are that the preview is fed *from* the
    recording's own frames while it runs, or that the preview is told, in a way the page can
    render, that a recording owns the camera and for roughly how long. Deciding this is P6's
    and it is written here so P6 does not discover it late, the way P5 nearly discovered
    N76.

## What would change this

A second human on another machine (which is what D11's non-loopback cells and note **N79**'s
reverse-proxy shape are already about); a device under test that must be *filmed* rather than
sampled, which is P6's recording and would make the preview path a product surface rather
than a supervisory one; or an agent harness that wants to watch a stream continuously rather
than take photographs from time to time, which would move the MJPEG preview from the second
consumer's column into the first's and re-price everything in item 4.

---

## N1 — The lint policy lives in `[workspace.lints]`, and tests are not exempt

**Doc:** docs/4 writes the crate-root lint policy with
`#![cfg_attr(not(test), deny(clippy::allow_attributes, clippy::allow_attributes_without_reason))]`
— test code exempt from the suppression-hygiene lints.

**Repo:** the whole policy is one `[workspace.lints]` table in the root `Cargo.toml`, and
every crate opts in with `[lints] workspace = true`. Cargo's lints table cannot express
`cfg(test)`, so the two suppression-hygiene lints apply to test code as well.

**Why:** the alternative is thirteen hand-maintained copies of the same attribute block —
a second copy of a law, which rubric A5 calls a finding, in the one place docs/4 also
demands the policy be uniform. The deviation is strictly *stronger* than the documented
policy (test code must write `#[expect(..., reason = "...")]` too), so nothing it changes
can weaken a gate.

**Retires when:** Cargo grows cfg-conditional lint tables, or the test-side strictness
proves to cost more than it buys (measured in suppressions written, not in irritation).

**Adjacent:** `#![forbid(unsafe_code)]` is *not* in the workspace table, because
`webcam-handler-v4l2` must not have it. It stays a crate-root attribute on the other
twelve, and `scripts/gates/unsafe-scope.sh` asserts each root carries it — so the copies
are gate-checked, not trusted.

**Absorbed (2026-08-08):** docs/9 Part 1 documents `[workspace.lints]` as the policy home; this entry remains the reasoning of record.

---

## N2 — `directories` is not used; the two XDG paths are ours

**Doc:** design §2.8 lists `directories 6` among the core picks.

**Repo:** no dependency. `webcam-handler-engine::paths` resolves `$XDG_STATE_HOME`
(fallback `$HOME/.local/state`) and `webcam-handler-schema::paths` resolves
`$XDG_RUNTIME_DIR`, both directly. (Path citation amended at P4f, when the runtime half
moved so the thin client could reach it without linking the engine — note **N64**. The
count and the reasoning below are unchanged: it is still two paths and still ~thirty
lines, now with one home each.)

**Why:** `directories 6.0.0 → dirs-sys 0.5.0 → option-ext 0.2.0`, and `option-ext` is
**MPL-2.0**. The license allowlist rejected it on the scaffold's first `cargo deny` run —
the gate paid for itself before a single line of domain code existed. The crate is on the
ban list now, with this note as the reason, so it cannot return by accident.

We need exactly two paths, both fixed by one paragraph of the XDG Base Directory
specification, on one platform. Vendoring a cross-platform path library to get them is a
worse trade than owning thirty lines.

**Retires when:** `directories` sheds `option-ext` (upstream has an open issue about it),
or the tool grows a real need for cross-platform path conventions.

**Absorbed (2026-08-08):** docs/6 §2.8 carries the drop and the `option-ext` ban.

---

## N3 — `std::thread::sleep` is banned workspace-wide, not just in tests

**Doc:** docs/4's `disallowed-methods` bans `std::thread::sleep` *in test code*.

**Repo:** `clippy.toml` is a workspace-global file with no notion of test-vs-not, so the
ban is global. Legitimate production sites (there are none yet; a settle backoff would be
one) take a narrow `#[expect(clippy::disallowed_methods, reason = "…")]`.

**Why:** the same one-home argument as N1. A global ban with named exceptions is auditable
— `grep` finds every exception and each carries a reason; a test-only ban is not.

**Retires when:** clippy grows per-target `disallowed-methods`.

**Absorbed (2026-08-08):** docs/9 Part 1 documents the workspace-global ban.

---

## N4 — The D13 registry gained four variants

**Doc:** design D13 lists fourteen error variants and calls the registry closed.

**Repo:** `webcam-handler-schema::error::Error` has eighteen. The four additions:

| Variant | Why the fourteen could not cover it |
|---|---|
| `CameraUnknown { requested }` | `wch photo cam:nope` has to fail as something. D13 has `DeviceGone` (a camera that *was* there) but nothing for a name that never resolved. |
| `CameraAmbiguous { requested, candidates }` | D1 grants prefix resolution (`cam:obsbot`); a prefix matching two cameras must name both, and no existing variant carries candidates. |
| `DeviceIo { operation, errno, message }` | D13 maps `EBUSY`, `EPERM` and `ENODEV` to typed variants, which leaves every other `errno` — `EINVAL` on a format negotiation, `EIO` mid-stream — with nowhere to land except an `anyhow` string, and rubric B6 calls a string crossing the wire a finding. |
| `StorageIo { path, errno, message }` | D9's own fault menu lists "full disk", and the fourteen have no variant for a filesystem failure. |

**Why this is completion rather than re-litigation:** D13's stated purpose is that "every
variant carries what the caller needs to act", and the closed-ness is what makes the
`webcam-handler-api` code mapping exhaustive. Both additions of the `*Io` pair keep that
property — they are typed, they carry `errno` and the operation, and a caller can
distinguish them from a capability answer (E3). The alternative was a stringly escape
hatch, which would have broken the doctrine the registry exists to hold.

`ErrorKind` is generated with its `ALL` by the `closed_vocabulary!` macro, and
`Error::kind()` is an exhaustive match, so every one of the eighteen is walked by the
round-trip, rendering, and (from P4) RPC-code tests. Adding a nineteenth without wiring it
in does not compile.

**Retires when:** nothing retires it; docs/1 D13 should absorb these four at its next
revision. Recorded here rather than edited into the design because the design is v1 and
this is repo case law (docs/2's standing conventions).

**Absorbed (2026-08-08):** docs/6 D13 carries the four variants.

---

## N5 — `webcam-handler-api` is exempt from the tokio half of the T6 wall

**Doc:** design §2.8 states the purity wall as "`schema`, `imaging`, `fake`, `api`,
`cli-core` link no tokio/axum/hyper", gate-asserted from `cargo metadata`.

**Repo:** `scripts/gates/dependency-walls.sh` applies the full three-crate ban to
`schema`, `imaging`, `fake` and `cli-core`. `webcam-handler-api` gets its own wall —
**no axum, no hyper, no tower-http** — and is allowed tokio.

**Why:** measured on 2026-08-08 against jsonrpsee 0.26.0, three feature sets built in a
scratch crate containing nothing but a `#[rpc(server, client)]` trait:

| `jsonrpsee` features | Result |
|---|---|
| `macros` | does not compile — the expansion needs `IntoResponse` and `RpcModule` |
| `macros`, `client-core` | does not compile — the server half is unresolved |
| `macros`, `server-core` | does not compile — the client half is unresolved |
| `macros`, `client-core`, `server-core` | compiles; `jsonrpsee-core → tokio` (`rt`, `sync`, `time`, `macros`) |

`jsonrpsee-core` activates tokio in *both* its `client` and `server` features, and the
macro's expansion references it. So a crate holding one `#[rpc(server, client)]` trait
links tokio, necessarily. Making the features optional on our side does not help either:
the two composition roots enable them and cargo unifies.

That leaves three options. Splitting the trait so client and server halves live in
separate crates would give us two wire surfaces, which is the thing D10/T5 exists to
prevent. Hand-rolling JSON-RPC is §7's recorded "shape of last resort" and costs us the
80% of jsonrpsee we actually use. Narrowing the wall costs us the least, provided the
narrowing is to exactly what the wall was protecting.

**What the wall was protecting**, read from §2.8's own sentence: "only `daemon` links the
web stack". That property is intact and now gate-asserted for `api` specifically —
measured today, `api` pulls no axum, no hyper, no tower. Tokio arriving as a transitive
library dependency of a JSON-RPC codec is not the defect the rule was written against;
`wchc` linking hyper would be, and that half still fails loudly.

**What is not covered:** that `api` never *starts* a runtime or spawns a task. Linkage
cannot see that. It joins the behavioral halves of T6 that §2.8 already declares
review-held — so this note widens the review surface by one claim rather than pretending
the gate grew.

**Retires when:** jsonrpsee splits a runtime-free core out of `jsonrpsee-core`, or the
wire surface stops being jsonrpsee. Re-run the table above on any jsonrpsee bump; if a
version makes the original wall satisfiable, delete the exemption and this note.

**Absorbed (2026-08-08):** docs/6 §2.8 and docs/9's dependency-walls row carry the api wall; the bump-triggered re-measurement is a docs/7 post-plan row.

---

## PF:13 — `bus_info` is per-USB-device, not per-logical-camera

**Measured** 2026-08-08 on kernel 7.0.0-29-generic, against the same seed hardware as the
docs/1 §1.2 registry. Continues that registry; cite it as `[PF:13]`.

`VIDIOC_QUERYCAP` reports `bus_info` for both Chicony logical cameras as the identical
string `usb-0000:00:14.0-4`, even though they are separate USB *interfaces* (`3-4:1.0` RGB
and `3-4:1.2` IR) hosting separate capture nodes with different formats. The card names do
differ (`Integrated Camera: Integrated C` vs `… Integrated I`), but that is the vendor's
courtesy, not a guarantee.

**Consequences, both load-bearing:**

1. **Grouping must come from the sysfs USB interface path**, never from `bus_info` —
   PF:7's rule, now with the counter-example that shows why the easier field does not
   work. Two cameras that share `bus_info` would collapse into one group.
2. **`CameraFingerprint::bus_path` holds the interface path** (`3-4:1.2`), not `bus_info`.
   A fingerprint built on `bus_info` could not tell the IR camera from the RGB one, and
   `calibrate apply` would happily replay an IR session onto the RGB sensor.

Measured node facts, for the P1 enumeration tests:

| Node | Interface | `device_caps` | Kind |
|---|---|---|---|
| video0 | 3-4:1.0 | `0x04200001` | capture (Chicony RGB) |
| video1 | 3-4:1.0 | `0x04a00000` | metadata |
| video2 | 3-4:1.2 | `0x04200001` | capture (Chicony IR) |
| video3 | 3-4:1.2 | `0x04a00000` | metadata |
| video4 | 3-1:1.0 | `0x04200001` | capture (OBSBOT Tiny 3) |
| video5 | 3-1:1.0 | `0x04a00000` | metadata |

USB ids: Chicony `04f2:b83c` serial `"0001"`; OBSBOT `3564:ff02`, no serial — PF:8 holds.

**Retires when:** never, unless the kernel starts reporting per-interface `bus_info`. It
becomes corpus at P1, where the three committed profiles pin these node tables.

**Absorbed (2026-08-08):** docs/6 §1.2 and D1; this entry remains the measurement record.

---

## N6 — D13 gained a nineteenth variant, `Unimplemented`, and it is scheduled to die

**Doc:** docs/1 D13 lists the error registry; note N4 already recorded four additions and
argued each was *completion* of the registry's stated purpose rather than a new escape
hatch. Design §2.3 states the T1/T2 traits as total: `Camera` has eight methods and every
backend implements all eight.

**Repo:** `webcam-handler-schema::error::Error` has nineteen variants. The new one is

```rust
Unimplemented { operation: String, arrives_in: String }
```

and at P1 it is returned by exactly five methods — `Camera::{set, start_stream,
next_frame, stop_stream}` (which arrive at P2) and `CameraBackend::watch` (P4).

**Why:** docs/2 splits one *total* trait across three phases. P1 lands the V4L2 read path;
the write path is P2's and hotplug is P4's, each with its own gate. But the trait must be
total to compile at P1, so the four-and-one methods that have not landed must return
something, and every other candidate is a lie:

| Candidate | Why it is worse |
|---|---|
| `panic!`/`todo!` | "plugging in a webcam cannot panic the library" is the whole reason this crate exists (PF:1). A panic is also banned on device-driven paths by the crate's lint set. |
| `Error::DeviceIo` | blames the kernel for our release schedule. A bug report filed against `uvcvideo` is a real cost. |
| `Error::FormatUnsupported` with an empty list | a **capability** answer. E3's entire subject is that "the camera can't" and "we didn't" must never be spelled the same way. |
| Implementing the write path early | over-scoping; G2's criteria — clamp warnings, read-back, the guarded-set planner — are where writes are proven, and landing them unproven at P1 would put them past their gate rather than before it. |
| Splitting the trait so P1 implements a narrower one | two backend contracts instead of one, which is what T1/T2 exists to prevent, and `wch` would need a backend-specific path — the exact "one home" violation §2.10 forbids. |

**Why this is not the escape hatch N4 warned about:** it is typed, it carries the two
things a caller acts on (which operation, which phase), and it says *this build* rather
than *this device*. `schema`'s own test asserts the rendering names the build and the
phase, and asserts the kind is distinct from `DeviceIo`.

**What keeps it honest:** `webcam-handler-v4l2::unimplemented_surface()` is the one list
of methods that answer it, and a test pins the list's size and contents. P2 cannot land
its four without editing that test, and when the fifth goes at P4 the function has no
rows left. The variant is therefore scheduled to become unconstructed.

**Retires when:** P4 closes and no crate constructs it. At that point deleting the variant
is a one-line change that the exhaustive `Error::kind` match will drive.

**Absorbed (2026-08-08):** docs/6 D13/§2.3 document the transitional variant; docs/7 P4d schedules the deletion. The retirement condition stands unchanged.

### Retired at P4d, 2026-08-10 — the condition was met, and this is what it cost

The condition above named its own mechanism, so the retirement follows it rather than
narrating around it. `CameraBackend::watch` on the V4L2 backend was the last producer; P4d's
uevent socket landed it (note N53), `unimplemented_surface()` had no rows left, and the
variant was deleted — `ErrorKind::Unimplemented`, `Error::Unimplemented`, the `kind()` and
`sample()` arms, and the RPC code. **The registry is eighteen variants.**

**"A one-line change the exhaustive match will drive" was nearly right, and the part that was
right is the part that mattered.** It was not one line — the variant, the kind, two match
arms, an RPC code, a fixture row and three tests — but **every one of those was found by the
compiler**, not by a grep and not by a reviewer: `Error::kind()`, `Error::sample()`,
`api::codes::rpc_code`, `schema`'s `an_unfinished_operation_blames_the_build_rather_than_the_device`
and `calibrate_verbs.rs`'s two `assert_ne!`s each stopped compiling until they were dealt
with, which is exactly what `codes.rs`'s header says the match-over-`ErrorKind` exists to buy.
Nothing that *derives* from the registry needed a hand edit at all — `xtask`'s OpenRPC error
emitter, `cli-core`'s exit-code walk and `codes.rs`'s own round-trip walks read
`ErrorKind::ALL` and followed the deletion silently, and the two `schemas/` artifacts
regenerated to 122 deleted lines and **zero added ones**. That is the payoff of "one home per
law" (§2.10), and it is worth stating because a deletion is where a second hand-maintained
list surfaces if there is one. There was not one.

**The wire cost was one endpoint.** P4a placed the variant on `-32030` deliberately, the
lowest code in D13's block, so that its deletion could raise `D13_CODES.start()` to `-32029`
and move nothing else. That is what happened: `crates/api/fixtures/d13-rpc-codes.tsv` lost one
row and no other byte, and the eighteen codes a P4c client already knows are unchanged.
`deleting_the_lowest_variant_in_the_registry_moved_one_endpoint_and_no_code_at_all` asserts it
against a hand-written transcription of the P4c table — a hand list on purpose, because the
committed fixture moves *with* the registry and so could never notice both sides being
renumbered together.

**What the schedule cost overall.** Five backend methods answered it at P1 (this entry's own
count), four of them landed at P2, and thirteen T5 methods joined at P4b (N43) — so the peak
was fourteen methods across two pinned surfaces, both empty by the time the variant went.
Every producer was retired by a *landing* rather than by a rewrite, which is what the two
pinned lists were for: each phase had to edit a test that counted them. N4's warning — that a
registry gains escape hatches — did not come true here, and the mechanism that stopped it was
those lists, not the reviewers.

**Two producers were not deletions, and both are recorded rather than absorbed:**

- **`engine::profile`'s `#[cfg(test)] StubCamera`** — the one N43 flagged and left for P4d.
  It now answers `Error::IllegalTransition { from: "stub_camera", op }`, naming the method
  attempted. Three of the surviving eighteen were ruled out by law before the pick: a panic
  breaks PF:1's rule, `FormatUnsupported` is a capability answer about a device that was
  never asked (E3), and `DeviceIo` blames a kernel that is not in that test at all. This is
  the same family N46 chose `IllegalTransition` for — "the request names something this
  object does not do" — so **N46's population is now four call sites, not three**, and its
  retirement clause ("at which point the three call sites move together") should be read
  with this one added.
- **`daemon::tests::calibrate_verbs`'s walk** lost one of its two absence claims, because
  `assert_ne!(error.kind(), ErrorKind::Unimplemented)` is a line that no longer compiles. The
  test is now `no_calibrate_verb_answers_store_locked`. **The suite checks one thing fewer**,
  and that is written into the test's own comment, its module header and `phase-criteria.tsv`
  rows 106 and 107 rather than left to evaporate with the assertion: the claim did not
  weaken (no verb can answer a variant that does not exist) but it stopped being *checked*
  from the wire, and a criterion row describing an assertion that is gone is note N10's
  family.

**One name was deliberately kept.** `daemon::server`'s
`the_pinned_routing_is_the_whole_wire_surface_and_nothing_answers_unimplemented` still names
the variant. Its assertion — `ROUTED` *is* `api::METHODS` — is untouched and can still go
red, the second half of its name is now *more* true than when it was written, and N43 cites
the name in an entry that is case law rather than prose to be tidied. The test's own comment
says all of that, so a reader who greps the deleted variant lands on an explanation instead
of a puzzle.

**Retires when:** now.

---

## PF:14 — A UVC camera's VideoStreaming interface never has a V4L2 binding

**Measured** 2026-08-08 on kernel 7.0.0-29-generic against the docs/1 §1.2 seed hardware.
Continues that registry; cite it as `[PF:14]`.

D1 requires `list` to diagnose an empty enumeration rather than shrug at it: scan for USB
video-class interfaces with no `video4linux` binding and report "USB camera present
without a V4L2 driver". Implemented as literally written — *per interface* — that check
reports **every healthy camera on this machine** as driverless.

`/sys/bus/usb/devices`, with `bInterfaceClass` and the presence of a `video4linux/`
subdirectory:

| Interface | `bInterfaceClass` | `bInterfaceSubClass` | `video4linux/` |
|---|---|---|---|
| `3-4:1.0` | `0e` | `01` (VideoControl) | `video0`, `video1` |
| `3-4:1.1` | `0e` | `02` (VideoStreaming) | **absent** |
| `3-4:1.2` | `0e` | `01` (VideoControl) | `video2`, `video3` |
| `3-4:1.3` | `0e` | `02` (VideoStreaming) | **absent** |
| `3-1:1.0` | `0e` | `01` (VideoControl) | `video4`, `video5` |
| `3-1:1.1` | `0e` | `02` (VideoStreaming) | **absent** |

A UVC device exposes both halves of the class. `uvcvideo` binds the VideoControl interface
and hangs the capture nodes off it; the VideoStreaming interface is claimed by the same
driver but carries no `video4linux` directory of its own. Three cameras, all working,
three interfaces that look unbound.

**Consequence:** the question is asked **per USB device**, not per interface. A device is
diagnosed as driverless when it presents at least one video-class interface and *none* of
its interfaces has a V4L2 binding. Filtering on `bInterfaceSubClass == 01` would also work
today, but it encodes more of the UVC descriptor layout than the diagnosis needs — the
user's question is "is something driving this camera", and that is a property of the
device.

The false-positive direction is the one that matters: a wrong hint appears exactly when
`list` is *not* empty of real cameras on other buses, i.e. when the user is least likely
to question it. `sysfs::unbound_video_devices_in` is tested in both directions, and a live
test asserts that a host with V4L2 nodes diagnoses nothing.

**Retires when:** `uvcvideo` starts binding nodes to the VideoStreaming interface, or the
diagnosis moves to a source that reports driver binding directly.

**Absorbed (2026-08-08):** docs/6 §1.2 and D1; this entry remains the measurement record.

---

## N7 — `CameraBackend` gained a fifth method, `diagnose`

**Doc:** design §2.3 states T1 as four methods — `name`, `enumerate`, `open`, `watch` —
and calls the trait "the pluggability seam". Separately, D1 requires that "an empty
enumeration is diagnosed, not shrugged at": `list` with zero cameras is to scan sysfs for
USB video-class interfaces with no video4linux binding and report "USB camera present
without a V4L2 driver".

**Repo:** `CameraBackend` has a fifth method with a default body:

```rust
fn diagnose(&self) -> Vec<crate::report::ListHint> { Vec::new() }
```

`webcam-handler-v4l2` overrides it with the PF:14 scan; `webcam-handler-fake` inherits the
empty default.

**Why:** D1's requirement needs a channel, and the two alternatives are both worse.

- *Put the scan in the client.* Then something above the seam has to know it is holding a
  V4L2 backend, which means a second exhaustive `match` on `BackendKind` — the "second
  home" §2.10 calls a defect, in a codebase whose composition roots exist precisely so
  that match happens once.
- *Return the hints from `enumerate`.* That changes the signature every backend and every
  caller already uses, to carry a field almost every call ignores, and makes "the cameras"
  and "why there might be fewer than you expect" one value when they are two facts.

A defaulted method costs a backend that has nothing to say exactly nothing, which is the
honest position for one replaying a document: the fake's enumeration is complete by
construction, so it has no absence to explain.

**Why this is completion rather than re-litigation:** §2.3's stated purpose for T1 is that
the engine consumes backends without naming them. Adding `diagnose` *serves* that purpose —
without it, the one thing D1 asks for could only be built by naming one. The method takes
and returns schema values like every other, and adds no policy: `ListHint::message` renders
the sentence once, in `webcam-handler-schema`, so the CLI and the daemon cannot describe the
same finding differently.

**What it does not do:** it is not a general "backend status" channel. `HintKind` is a
closed vocabulary with one variant, and a second one needs the same justification this one
had — a requirement in the design that cannot otherwise be met.

**Retires when:** nothing retires it; docs/1 §2.3 should absorb the method at its next
revision, as N4 says of the four error variants.

**Absorbed (2026-08-08):** docs/6 §2.3 carries `diagnose` in T1.

---

## N8 — A dev-only binary in this workspace carries root-equivalent capabilities

**Doc:** design §1 states "runtime external binaries are not [acceptable]" (ffprobe and mpv
appear only as test oracles). §2.8's licence inventory forbids LGPL linkage. Nothing in
docs/1–5 anticipates a privileged helper, because nothing in the *product* needs privilege.

**Repo:** `crates/priv/` builds `wch-priv`, a binary that a one-time `just bless` grants
`cap_sys_module,cap_net_admin+eip`. It loads and unloads `vivid`, cycles `uvcvideo`, and —
via `wch-priv exec` — runs an arbitrary program with those capabilities in its ambient set.
It shells out to `/usr/sbin/modprobe`.

**Why it exists:** three things the project needs are impossible without privilege, and
each of them was, until now, gated on a human typing a sudo password:

| Need | Why privilege |
|---|---|
| The R2 rung | `vivid` is a kernel module. As of P1 the suite had **never executed** (entry E1) purely because nothing could load it. |
| `Error::DeviceGone` and P4 hotplug against real hardware | A laptop camera is soldered down. Cycling `uvcvideo` is the only way to make one disappear. |
| The P4 uevent socket | ~~Binding `NETLINK_KOBJECT_UEVENT` needs `CAP_NET_ADMIN`. *Unverified* — the probe was blocked — so this capability is granted ahead of proof, which is recorded here rather than discovered later.~~ **Disproved 2026-08-10 — see the amendment below and \[PF:21\].** |

**Why §1 is not violated:** §1's rule is about the product. `wch-priv` never ships, is
never a dependency of a product crate (gate-asserted), and its `modprobe` subprocess is a
*development* dependency in the same category as the ffprobe oracle §1 explicitly permits.
Shelling out is also the licence-correct choice: the in-process alternative is `libkmod`,
which is LGPL, and §2.8 forbids linking it. A process boundary is not a link edge.

**The shape, and the road not taken.** Two designs were put to the owner:

- a **closed verb vocabulary** — module names as compile-time constants, no caller-supplied
  paths — whose blast radius is "vivid and uvcvideo get loaded and unloaded"; and
- a **generic exec wrapper**, vmcell's model, which grants its capabilities to any program.

The owner chose the wrapper, with the consequence stated plainly in the question
(`wch-priv -- /bin/sh` is a root shell). The deciding argument is real: only a wrapper can
put `CAP_NET_ADMIN` inside a *test process*, and no verb design can do that from outside.
The module verbs were kept anyway, for ergonomics and because they are what a shell history
should show.

**So the security boundary is not a capability boundary.** It is:

1. **The file mode.** `just bless` chmods the blessed copy `0700` *before* `setcap`. This is
   the boundary, and `privileged-helper.sh` re-checks it on every `just ci` because a
   restore or a `chmod -R` can widen it long after the bless.
2. **The path.** `.wch-bin/`, gitignored, outside `target/` — writing a binary strips its
   xattrs, and cargo rewrites `target/` for reasons unrelated to this crate's source.
3. **Who has an account.** Nothing defends against a second session as the same user.

**What the code does to stay defensible inside that:** `#![forbid(unsafe_code)]` (the
`caps` crate owns the `prctl`); two dependencies, neither of them ours, because every link
edge is attack surface *inside* a root boundary; `env_clear()` before every subprocess,
because `modprobe` honours `MODPROBE_OPTIONS` and `AT_SECURE` does **not** scrub it;
absolute utility paths; and an interlock that refuses to unload `uvcvideo` while any
process holds a `/dev/video*` open, because pulling the driver out from under a video call
is the kind of thing a tool does exactly once.

**A deliberate duplication:** `modules::video_holders` walks `/proc/*/fd` and so will the
V4L2 backend's `Busy` diagnosis (D13, P2). They are not one law in two homes — this one
asks "is *any* camera in use", the backend's asks "who holds *this* node" and returns
`schema::Holder` — and merging them would drag the product's crate graph inside the
privileged boundary. Thirty lines is the cheaper half of that trade.

### Reconsider the granted powers when the plan closes (owner ruling, 2026-08-08)

**The powers granted here are deliberately broader than the demonstrated need, and that is
an accepted, time-boxed decision rather than an oversight.** `wch-priv exec` grants
`CAP_SYS_MODULE` to any program; `CAP_NET_ADMIN` was granted on a *prediction* about P4's
uevent socket that nobody has verified. The owner's ruling: on this machine, for the
duration of the implementation plan, that is fine — the cost of a too-narrow tool is a
loop that stalls on a password prompt, and the cost of guessing the boundary early is
guessing it wrong twice.

**The trigger is G6.** When the last phase gate closes, the guesswork ends: P2–P6 will have
established exactly which privileged operations the project actually performs, and the
evidence will be sitting in the justfile recipes and the suites that call them. Revisit
then, with these questions:

1. **Which capabilities were actually spent?** If `CAP_NET_ADMIN` was never needed — if P4's
   uevent socket turns out to bind unprivileged on this kernel, which was never tested —
   drop it. A capability granted "in case" and never used is the easiest thing in this
   whole design to remove and the easiest to forget. **Answered at P4d: it was never
   needed. See the amendment below and \[PF:21\]; question 1 now has a fact instead of a
   condition.**
2. **Was `exec` used for anything but delegating to a test process?** If not, the closed
   verb vocabulary that was offered and declined becomes available at no cost: the one
   argument that defeated it was that only a wrapper can put a capability *inside* a test
   process, and by G6 we will know whether that was ever needed.
3. **Does the loop still need to load modules unattended at all?** After the plan, the R2
   rung runs on demand rather than continuously. If nothing routine needs it, the whole of
   `crates/priv/`, the `bless` recipe, and `privileged-helper.sh` delete together — nothing
   else references them.

Recording the deferral rather than the decision is the point. A broad grant with a named
revisit is a different thing from a broad grant nobody revisits, and the difference is
entirely whether it was written down.

**Retires when:** question 3 above is answered "no" — or, short of that, when questions 1
and 2 have narrowed the grant to what the finished project measurably uses.

**Corrected once already, on the first real bless.** The helper was blessed `+eip` on the
theory that the file's inheritable bit is what lets a capability reach a child. It is not.
The kernel computes `pI' = pI` across `exec` — a process's inheritable set is whatever its
*caller* had — and the file's `fI` only ever appears as the term `pI & fI`, which is empty
when the caller is a shell. `PR_CAP_AMBIENT_RAISE` needs the capability in both `pP` and
`pI`, so `wch-priv exec` could never have worked, while `getcap` showed a perfect-looking
`cap_net_admin,cap_sys_module=eip`. The fix is to raise into `pI` at runtime, which
`capabilities(7)` allows without `CAP_SETPCAP` (`pI' ⊆ (pI | pP)`), and the blessing is now
`+ep` — the inheritable bit was dead weight carrying a false explanation.

Two things follow, both of which outlive the bug:

- **`doctor` performs the ambient raise instead of predicting it.** The old version
  reported a static guess and got it wrong in the one direction that matters; the new one
  runs the same call `exec` does, so a green line means the chain has actually executed.
  `just bless` ends by running it.
- **`just bless` stages the copy** and moves it into place only after `setcap` succeeds. A
  bless that cannot finish used to replace a *working* helper with an un-capped one — it
  failed closed, which is the right direction, but left the machine worse off than before
  the command was run.

**Amend this note if** a verb is added that takes a module name, a path, or anything else
from its caller. That would not *increase* the privilege — `exec` already grants root — but
it would add a second, quieter route to it, one that reads like a safe utility in a shell
history.

### Amendment, 2026-08-10: the `CAP_NET_ADMIN` prediction is disproved

The row above was this note's one *unverified* claim, and it said so. P4d measured it, and
it is wrong: `socket(AF_NETLINK, SOCK_DGRAM, NETLINK_KOBJECT_UEVENT)` and `bind` with
`nl_pid = 0, nl_groups = 1` both succeed on kernel `7.0.0-29-generic` from a process whose
effective capability set is empty, and the same process then received all fifty-six
packets of a `uvcvideo` cycle. `lib/kobject_uevent.c` registers the protocol with
`NL_CFG_F_NONROOT_RECV`, which exempts group membership from the check the prediction
assumed. \[PF:21\] carries the transcripts, the packet shape and the limits.

Three consequences, and only the first two are P4d's:

1. **Nothing in `crates/backends/v4l2/src/sys/uevent.rs` asks for a capability**, and its
   own test asserts the absence of `CAP_NET_ADMIN` before it asserts the bind, so a run
   under a blessed wrapper fails rather than passing without measuring.
2. **P4d's R3 hotplug arm runs unprivileged**, binding its own socket and spawning
   `wch-priv uvcvideo cycle` as a subprocess. There is no managed `wch-priv exec` recipe
   for it, and the argument that bought `exec` — "only a wrapper can put `CAP_NET_ADMIN`
   inside a *test process*" — is not exercised by hotplug. That is evidence for **G6
   question 2** as well as question 1.
3. **The narrowing itself is not done here.** docs/6 §2.13 says "the trigger to narrow or
   delete is G6" and docs/7 P6e owns the execution; P4d records the truth and hands it
   over. When P6e runs, the blessing is `cap_sys_module` — `modprobe` still needs it, and
   nothing measured here touches that half. The one thing that could bring
   `CAP_NET_ADMIN` back is `SO_RCVBUFFORCE`, which does need it; nothing in this project
   uses it, and PF:21 says what would have to change for that to stop being true.

**Absorbed (2026-08-08):** docs/6 §2.13 summarizes this entry and docs/7 P6e carries the G6 reckoning; this entry remains the full record, and the owner rulings live here.

---

## E2 — The R2 rung's first execution, 2026-08-08

Entry E1 recorded that the four `vivid_*` tests had never run, and said plainly that "the
rung reports a counted skip" and "the rung works" are different claims. The privileged
helper (note N8) closed that gap. This is the second claim.

`just rung-vivid`, with `vivid` loaded at `n_devs=1`:

```
    Starting 4 tests across 21 binaries (395 tests skipped)
        PASS vivid_enumeration_groups_nodes_and_classifies_them_by_capability
        PASS vivid_controls_enumerate_and_hold_the_control_model_invariants
        PASS vivid_reads_every_readable_controls_current_value
        PASS vivid_formats_enumerate_with_sizes_and_intervals_nested_under_them
     Summary  4 tests run: 4 passed, 0 skipped
```

**All four passed on first execution**, against a driver the code had never met.

**What R2 covers that R3 cannot.** The virtual driver is a far larger control-model surface
than the seed hardware:

| | Chicony RGB | OBSBOT | vivid |
|---|---|---|---|
| controls | 18 | 24 | **77** |
| formats | 2 | 2 | **83** |
| size entries | 13 | 7 | **747** |
| compound payloads read | 1 | 0 | **10** |

The last row is the one that matters most: the `G_EXT_CTRLS` payload path — a caller-sized
buffer whose length comes from device-supplied `elem_size × elems` (rubric B10) — was
exercised by exactly one control on the real hardware and by ten here. §3.3 item 4 says a
green R2 proves the ioctl plumbing and not device quirks; that remains true, and the
plumbing it proves is materially wider than it was.

**It also found a defect, in a test rather than in the product.**
`hw_a_node_that_implements_no_control_ioctl_answers_empty_rather_than_erroring` asserted
that a node without `VIDEO_CAPTURE` reports no controls. That is false, and vivid is the
counter-example: its **video output** nodes are not capture nodes and carry 77 controls
each. "Not a capture node" and "implements no control ioctl" are different claims, and only
the second is PF:15. The test now asserts the property the finding actually names — that
`controls()` and `formats()` never *fail* on any node — with non-vacuity on both halves (at
least one non-capture node, at least one node answering nothing). It is still red when the
ENOTTY fix is reverted, and now green with vivid loaded as well as without.

That is the R2 rung earning its place on its first run: the bug was in a hardware test that
had passed four times against hardware that could not contradict it.

**Also established:** `just smoke-hw` passes with vivid loaded (10 nodes rather than 6),
so the R3 suite does not depend on the machine having only its real cameras attached.

---

## PF:15 — `ENOTTY` is how a node says "I do not implement that ioctl", and it terminates enumeration

**Measured** 2026-08-08 on kernel 7.0.0-29-generic against the docs/1 §1.2 seed hardware.
Continues that registry; cite it as `[PF:15]`.

V4L2 has no count-first call: every enumeration walks an index until the kernel refuses.
The refusal was assumed to be `EINVAL` throughout. It is not the only one.

Measured on `/dev/video0` (capture) and `/dev/video1` (metadata), same USB interface:

| ioctl | capture node | metadata node |
|---|---|---|
| `VIDIOC_QUERY_EXT_CTRL` | OK | **`ENOTTY`** |
| `VIDIOC_QUERYMENU` | (`EINVAL`, no such index) | **`ENOTTY`** |
| `VIDIOC_ENUM_FRAMESIZES` | OK | **`ENOTTY`** |
| `VIDIOC_ENUM_FRAMEINTERVALS` | OK | **`ENOTTY`** |
| `VIDIOC_G_EXT_CTRLS` | OK | **`ENOTTY`** |
| `VIDIOC_ENUM_FMT` (type `VIDEO_CAPTURE`) | OK | `EINVAL` |

So the *same node* answers differently for different ioctls: `EINVAL` for `ENUM_FMT` ("I
implement that, and there is no format at that index") and `ENOTTY` for the other five ("I
do not implement that call at all"). `ENOTTY` is errno 25, `Inappropriate ioctl for
device`. The split is not arbitrary — a metadata node genuinely has an `ENUM_FMT`
implementation, for the metadata buffer type — but it is not one a caller can predict from
the node's capabilities, so both answers have to be accepted wherever a list ends.

**Consequence:** a build accepting only `EINVAL` reports a metadata node's control set as
`Error::DeviceIo`. That is not hypothetical — `V4l2Camera::open` falls back to a group's
first node when it has no capture node, and a **metadata-only camera is a shape this
project deliberately supports** (`CameraInfo::capture_node` documents it: "the camera is
listed, and streaming it is a typed refusal rather than a surprise"). Such a camera's
`controls` would have failed with a device error instead of returning an empty set.

`sys::ioctl::call_enumerating` now reads both as `Exhausted`. The distinction that
survives: `ENOTTY` from an *enumeration* is a terminator; from `VIDIOC_QUERYCAP` it still
means "not a V4L2 device" and stays an error, because `querycap` does not go through that
path.

**Why it was invisible:** the independent Python probe that produced the byte fixtures
caught bare `OSError` to end each loop, so it recorded "0 controls" for the metadata nodes
and never revealed which errno ended them. A second implementation only catches what it
distinguishes — worth remembering the next time one is used as an oracle.

**Retires when:** never, unless the kernel starts implementing the control ioctls on
metadata nodes. Regression-tested by
`hw_a_node_that_implements_no_control_ioctl_answers_empty_rather_than_erroring`, which
lives in the crate rather than in `tests/` because the bug is only reachable through a node
`open` would never pick on hardware that also has a capture node.

**Absorbed (2026-08-08):** docs/6 §1.2 and §2.5; this entry remains the measurement record.

---

## E1 — G1 hardware evidence, 2026-08-08

docs/2's G1 asks for the dev-machine R3 run to be "recorded as evidence in the notes with
transcripts", because shared CI has no camera and that hole is structural (docs/4's
recorded limits). This is that record. Evidence entries are dated and appended; they are
not amended, because the point of a transcript is that it was true once.

**Host:** kernel 7.0.0-29-generic, x86_64. **Attached:** Chicony `04f2:b83c` (two logical
cameras — RGB on interface `3-4:1.0`, IR on `3-4:1.2`), OBSBOT Tiny 3 `3564:ff02` on
`3-1:1.0`. Six `/dev/video*` nodes, three capture and three metadata.

### R3 — `just smoke-hw`

```
smoke-hw: SKIP 1 — motor-moving suites (hw_motion_*) are excluded; set WCH_ALLOW_MOTION=1 to include them
smoke-hw: 6 capture node(s) present; running test(/^hw_/) - test(/^hw_motion_/)
    Starting 4 tests across 20 binaries (363 tests skipped)
        PASS [   0.004s] (1/4) webcam-handler-v4l2::hardware hw_nodes_group_by_interface_and_capture_nodes_are_found_by_capability
        PASS [   0.004s] (2/4) webcam-handler-v4l2::hardware hw_enumeration_matches_the_committed_profile
        PASS [   0.221s] (3/4) webcam-handler-v4l2::hardware hw_profile_capture_reproduces_the_committed_invariant_section
        PASS [   0.221s] (4/4) webcam-handler-v4l2::hardware hw_controls_enumerate_on_every_node_without_panicking
     Summary [   0.221s] 4 tests run: 4 passed, 363 skipped
smoke-hw: suite run, 1 named skip(s)
```

The `hw_motion_` suite is empty at P1 and the skip is still counted, which is the point:
the exclusion is a standing property of the recipe, not a fact about this run.

**What the four runs establish.** `profile capture` reproduces each committed profile's
invariant section exactly while provenance differs — G1's carve-out, demonstrated rather
than asserted. PF:1's `Region of Interest Rectangle` (type `0x0107`, `elem_size` 16)
enumerates on both Chicony nodes without panicking; the crate whose control layer we
bypass panics on it. PF:13 confirmed live: two cameras report `bus_info`
`usb-0000:00:14.0-4` and are told apart only by the interface path.

### R0 — `just miri`

```
     Summary [   2.720s] 19 tests run: 19 passed, 44 skipped
miri: suite run, 0 named skip(s)
```

All 19 `sys::decode` units, over the captured ioctl replies in
`crates/backends/v4l2/fixtures/`. **Miri cannot cross an ioctl** — this covers the
decoding half only, which is why the decoders take bytes rather than structs.

### R2 — `just rung-vivid`

```
rung-vivid: SKIP 1 — the vivid module is installed but not loaded; run `sudo modprobe vivid` (this script never loads kernel modules on someone's behalf)
rung-vivid: 0 tests run, 1 named skip(s)
```

**The R2 suite has never been executed.** `vivid` is installed on this host and not
loaded, and neither the rung script nor the session that wrote the tests loaded it —
the script's refusal is deliberate and applies to us too. Four `vivid_*` tests exist and
are selected by the recipe; they are unproven code until somebody runs
`sudo modprobe vivid && just rung-vivid`. Recorded here rather than left implicit,
because "the rung reports a counted skip" and "the rung works" are different claims and
only the first is established.

### Amendments after the P1 review

The transcripts above are as-run and stand. Two of the claims they rest on were narrowed
by the adversarial review that followed, and the narrowing belongs next to the evidence:

- **The Miri run above covered no `unsafe` block.** Its selection was `sys::decode`, which
  is entirely safe code; the two Miri-reachable blocks (`Payload::bytes`/`bytes_mut`) were
  outside it. Corrected — the job now runs 23 units including those two, and the other four
  blocks are ioctl calls Miri cannot cross either way.
- **The R3 run above passed while a real defect was present.** Metadata nodes answer
  `ENOTTY`, not `EINVAL` (PF:15), and no test reached a node the public surface never
  opens. The regression test that closes it is red on this machine when the fix is
  reverted; that is what the original four could not have told us.
- **The R2 skip above is retired.** See entry E2: `vivid` has been loaded and the suite
  has run.

### Not established by any of the above

- **Writes.** P1 is the read path; `set`, streaming and hotplug answer
  `Error::Unimplemented` (N6). No control on any attached camera was written, and no
  motor moved.
- **The PF:6 clamp behaviour** on real hardware. The battery probes it against replayed
  profiles; the hardware twin arrives at P2 with the write path.
- **Frame capture.** `PF:9`'s in-process MJPEG capture was demonstrated during the design
  probe, not by this build.

---

## N9 — D4's restore vocabulary gained a fourth outcome, because the common success looked like a failure

**Doc:** design D4 defines snapshot/restore and its ordering; `schema::snapshot`'s
`RestoreOutcome` had three variants — `Restored`, `AlreadyCorrect`, `Unrestorable` — and
`UnrestorableReason::StillInactive` for "the control is still INACTIVE after its automation
partner was handled".

**Repo:** a fourth outcome,

```rust
OwnedByAutomation { control: ControlSlug, automation: Option<ControlSlug> }
```

counted as **complete** by `RestoreReport::is_complete`.

**Why, and the evidence.** The first hardware run of `wch controls --discover-pairs`
measured both of the Chicony's real pairs, recorded `auto_exposure`'s off position by name
rather than by index (PF:2's rule, honoured), put the camera back exactly where it started
— and then said:

```
wch: the probe could not put 2 control(s) back:
     exposure_time_absolute, white_balance_temperature
```

Both were exactly where they started. The reasoning is arithmetic once written down:

1. The snapshot recorded `white_balance_temperature` as INACTIVE, because
   `white_balance_automatic` was on.
2. It recorded `white_balance_automatic` as **on**, because it was.
3. Restore writes the automation control back to on, which re-engages the partner.
4. The partner is INACTIVE again, so the second pass cannot write it.

On any device whose INACTIVE flag follows its automation control's value — which is every
device PF:3 describes — that is the *ordinary* outcome of every guarded write's restore.
`is_complete()` returning false for it would have made the field meaningless: a report that
cries failure on the common success is a report people stop reading, and P3's calibration
sweeps would have produced one on every run.

**Why this is completion rather than re-litigation.** D4's promise is "leave the camera as
you found it", and a control whose owner is back *is* as we found it — its value is that
automation's to choose, exactly as it was at snapshot time. The old vocabulary could only
say "we failed", which was false. The new variant says what happened and names the owner.

`StillInactive` is kept, and now means the thing it always said: a control that was **ours**
when the snapshot was taken and is owned by automation now. That is a real change we could
not undo. Telling the two apart is why `engine::snapshot::restore` defers on the device's
*present* state as well as on the snapshot's record of it.

**What it does not do:** it is not a general "partially restored" channel. A control that
could not be written for any other reason is still `Unrestorable`, and a second benign
outcome would need the same standard of evidence this one had.

**Retires when:** nothing retires it; docs/1 D4 should absorb it at its next revision, as
N4 says of the four error variants.

**Absorbed (2026-08-08):** docs/6 D4 carries the four-outcome vocabulary.

---

## PF:16 — `little_exif` cannot write EXIF into a JPEG that uses restart intervals

**Measured** 2026-08-08 on kernel 7.0.0-29-generic against the docs/1 §1.2 seed hardware,
with `little_exif 0.6.23`. Continues the docs/1 §1.2 registry; cite it as `[PF:16]`.

`wch photo` failed on roughly one Chicony frame in three:

```
wch: stamp EXIF onto JPEG failed: failed to fill whole buffer
```

Forty consecutive frames captured off `/dev/video0` and stamped in a loop: **nine failed**,
at sizes from 26 KB to 101 KB, interleaved with successes. Nothing about the failures was
structural — the failing and succeeding frames carried an identical marker sequence
(`DQT DQT SOF0 DHT×4 DRI SOS`), so the difference was in the compressed data itself.

**The cause.** `little_exif`'s JPEG path (`src/jpg.rs`, `clear_metadata`) walks the **whole
file** byte by byte looking for `0xFF <marker>` pairs, and reads the two bytes after each as
a segment length. That is valid in a JPEG's header and invalid in its scan:

- a literal `0xFF` in entropy-coded data is byte-stuffed as `FF 00`, and
- the Chicony emits a `DRI` segment, so its scan is punctuated with restart markers
  `FF D0`–`FF D7`.

Either way the walker reads a "length" out of the image data. Whether that length happens to
land inside the buffer depends on what the sensor was looking at, which is exactly why the
failure rate varied with the scene rather than with the code.

**Consequence:** `imaging::exif::stamp_jpeg` no longer lets the writer see our file.
`Metadata::as_u8_vec` builds the APP1 segment — which needs no knowledge of the file at all
— and `splice_app1` inserts it after the SOI itself, walking only the header and **stopping
at `SOS`**. The entropy-coded data is copied verbatim and never interpreted, which is E6's
byte-fidelity promise restated as an implementation.

The walk is also the place a camera's bitstream is treated as device data (rubric B10): a
header segment whose length runs past the end of the buffer ends the walk rather than
indexing past it, and the file still gets stamped.

**Regression-tested by** `a_scan_full_of_marker_shaped_bytes_is_stamped_without_being_parsed`
in `crates/imaging/src/exif.rs`, over a hand-built JPEG whose scan contains `FF D0` and
`FF 00 FF FF`. Hand-built rather than committed: the frames that exposed this are camera
frames, and camera frames never enter the repository (rubric A12).

**Retires when:** `little_exif` stops parsing past `SOS` — worth re-checking on any bump,
because our splice would then be a redundancy rather than a fix. It is not obviously worth
removing even then: the splice is thirty lines and it keeps a parse of device-supplied bytes
inside code this project's rules apply to.

**Absorbed (2026-08-08):** docs/6 §1.2 and D6; this entry remains the measurement record.

---

## E3 — G2 hardware evidence, 2026-08-08

docs/2's G2 asks for the dev-machine hardware run to be recorded in the notes, with the same
carve-out G1 used: the recipe existing and selecting tests is the gate criterion, and the run
itself is evidence. This is that record. Evidence entries are dated and appended; they are
not amended.

**Host:** kernel 7.0.0-29-generic, x86_64. **Attached:** Chicony `04f2:b83c` (RGB on
`3-4:1.0`, IR on `3-4:1.2`), OBSBOT Tiny 3 `3564:ff02` on `3-1:1.0`.

### R3 — `just smoke-hw`

```
smoke-hw: SKIP 1 — motor-moving suites (hw_motion_*) are excluded; set WCH_ALLOW_MOTION=1 to include them
smoke-hw: 6 capture node(s) present; running test(/(^|::)hw_/) - test(/(^|::)hw_motion_/)
     Summary [   7.143s] 13 tests run: 13 passed, 497 skipped
```

What the thirteen establish, in the words they printed:

```
cam:…-integrated-c: brightness 128 -> 129 (read back from the device)
cam:obsbot-tiny-3…: brightness 50 -> 51 (read back from the device)
cam:…-integrated-c: PF:6 live — brightness took 255 for a write of 1255,
  warnings [Clamped { requested: 1255, applied: 255, range: 0..=255 }]
cam:obsbot-tiny-3…: PF:6 live — brightness took 100 for a write of 1100
cam:…-integrated-c: PF:3 live — switching white_balance_automatic off freed
  white_balance_temperature
cam:…-integrated-c: privacy is read-only and said so
cam:…-integrated-c: snapshot(15) → perturb brightness → restore, every control back
cam:obsbot-tiny-3…: snapshot(22) → perturb brightness → restore, every control back
cam:…-integrated-c: streamed MJPG at 1280x720 (30 fps), two cycles, 6 frames each
cam:…-integrated-i: streamed GREY at 640x360 (15 fps), two cycles, 6 frames each
cam:obsbot-tiny-3…: streamed MJPG at 1920x1080 (30 fps), two cycles, 6 frames each
cam:…-integrated-c: D5 live — 3x3 negotiated to 1280x720, and reported as adjusted
cam:…-integrated-c: MJPG 1280x720 → 95458 bytes, the camera's own bytes [E6]
cam:…-integrated-i: GREY 640x360 → 16106 bytes, re-encoded
cam:obsbot-tiny-3…: MJPG 1920x1080 → 150962 bytes, the camera's own bytes [E6]
```

**Two P1 open questions are now closed.** The P1 evidence entry listed "the PF:6 clamp
behaviour on real hardware" and "frame capture" under *not established by any of the above*.
Both are above, on two devices each.

Four partial skips, all named and all the same shape: the Chicony IR camera exposes three
controls and none of them is a writable scalar, and neither it nor the OBSBOT has an enabled
non-motorized boolean automation control to toggle. The arms that need one say so rather
than passing quietly.

### R2 — `just rung-vivid-managed`

```
rung-vivid: vivid is loaded; running test(/(^|::)vivid_/)
     Summary [   2.051s] 7 tests run: 7 passed, 503 skipped
8 control write(s) went out and read back through the driver
cam:vivid: two stream cycles through the real ioctl path
1 node(s) refused a second concurrent stream as Busy
```

The three new arms are the P2 half of what E2 said R2 buys: a control surface far wider than
the seed hardware's, met by code that had never seen it.

### Not established by any of the above

- **Motors.** `hw_motion_*` is still empty and still excluded by default. No pan, tilt or
  zoom control has been written on this machine, and §5 keeps it that way until a sweep
  needs it (P3).
- **Hotplug.** `CameraBackend::watch` is the one remaining `Unimplemented` row; the uevent
  socket arrives at P4.
- **A second host.** Everything here is one machine, one kernel, three cameras. The corpus
  and the vivid rung are what stand in for the rest, and neither is a substitute.

---

## N10 — The gate that counts test selections could not count to zero

**Doc:** docs/2's standing conventions require every phase gate to be "named, counted,
re-runnable", and cite the predecessor's defect by name: *a "held" gate whose selection had
silently gone to zero*. `scripts/gates/counted-selections.sh` is the check written against
exactly that.

**Repo, before this note:** it could not report zero for any input.

`cargo nextest list -T json -E <filterset>` lists the **entire workspace** whatever the
filter says, and marks each testcase `matches` or `mismatch`. Its `test-count` is the size
of that whole listing. Measured on cargo-nextest 0.9.138 against this tree:

```
$ cargo nextest list -T json -E 'package(webcam-handler-engine) and test(/^zzz_no_such/)'
  test-count: 143      # the whole workspace
```

The predicate read `test-count`, with a fallback that summed the per-suite `testcases` maps
— which gives 143 as well. So from the day it was written, the gate whose entire subject is
"prove no selection has silently gone to zero" was green by construction. The defect it
exists to prevent, reproduced inside the check for it.

**Why the both-directions selftest did not catch it.** The failing arm used a *stub*
lister, and the stub returned `{"test-count":0,"rust-suites":{}}` for a non-matching filter
— a shape nextest never produces. The stub encoded the author's belief about the tool, the
predicate agreed with the belief, and the two shook hands. This is PF:15's lesson in a
different costume: *a second implementation only catches what it distinguishes.*

**Repo now:** the count is `filter-match.status == "matches"`, and the selftest gained an
arm the stub cannot provide — the **real** tool, over a filter that matches nothing. The
stub still exists, because a seeded criteria table is cheaper to drive than a rebuilt
workspace, but it now answers the way the tool answers, and the real-tool arm is what
notices if a nextest release changes the shape again.

**Retires when:** nothing retires it. The general lesson is worth keeping in front of
whoever writes the next gate: *the inverse arm must be driven by the thing under test, not
by a model of it.* Where a stub is unavoidable, one arm should still run the real thing.

**Absorbed (2026-08-08):** docs/8 rule 6 and docs/9's third structural rule carry the lesson.

---

## E4 — The P2 adversarial review, 2026-08-08

docs/3 Part E asks for a review pass at each phase boundary; P1's found four defects and is
recorded in E1's amendments. This is P2's. Thirty-one candidate findings, each attacked by
an independent skeptic instructed to refute it; **fifteen survived** and are fixed in the
commit that carries this entry. The ones worth remembering:

**Two the code got wrong in ways only hardware or a kernel source would show.**

- `V4l2Camera::set` chose its ioctl by the *caller's* value variant while `read_current`
  chose by the *descriptor's* `HAS_PAYLOAD` flag. A `ControlValue::Bytes` aimed at a scalar
  control therefore reached `set_payload`, which plants a heap address in
  `v4l2_ext_control`'s union — and `uvc_ctrl_set` ignores `size` for a control it does not
  treat as a pointer control, taking the low 32 bits of that address as the value, clamping
  it into range, and reporting an ordinary adjustment. On the OBSBOT's pan that is a motor
  driven to its limit by an allocator. Reachable from `wch restore` with a hand-edited
  snapshot. The fake refused the same input all along, so this was also the E5 resemblance
  claim failing in the direction that matters.
- `StreamRequest::choose` built its candidate sizes from `FrameSize::max_dimensions`, which
  collapses a **stepwise** entry to its largest corner. A device offering 32..1920 in steps
  of two would answer a request for 640×480 — a size it can deliver exactly — with
  1920×1080, reported as an adjustment. No seed camera is stepwise, which is why nothing
  noticed; `FrameSize::largest_within` now asks the range the question.

**Three in the discovery probe**, all of the same family — it treated a menu as a switch:

- one alternative was tried, chosen by numeric index order, so a three-item `auto_exposure`
  resting on `Aperture Priority Mode` could report *no pairs* silently;
- a candidate that could not be undone left residue that the *next* candidate's diff was
  measured against, inventing pairs stamped `Measured`;
- one "off" value was inferred for everything a toggle moved, so a mode that frees one
  control and freezes another recorded the wrong recipe for one of them.

**Two gates that were green while checking less than they claimed** — note N10 for the
worse one, and `ignored-suites-have-recipes.sh`, whose new test-group half read overrides
under *any* nextest profile and matched prefixes anywhere in a filter expression, including
ones the expression subtracts.

**Three tests that could not fail**: the PF:3 hardware arm counted a toggle that moved
nothing as an observation; the `InactiveFlip` fault arm put its ordering assertion inside an
`if let` that the defect itself would skip; and a test named
`..._exif_an_independent_reader_can_read_back` asserted only on the report.

**What the review did not find**, which is worth as much: no unsound `unsafe`, no aliasing
or lifetime defect in the mmap path, and no case where an availability failure had been
converted into a capability answer. Sixteen further candidates were refuted, several of them
by skeptics that built the reviewer's exact device and ran it.

---

## N11 — The state directory's lock file is the one state write that is not atomic

**Doc:** design §2.10 and rubric A5 name `webcam-handler-engine::store::write_json_atomic`
as the single home for state-directory writes, and call a caller that bypasses it "the same
defect as a second copy". D9 says the same thing in prose: "writes go through one audited
`write_json_atomic`".

**Repo:** `<state dir>/lock` is written in place — `open(O_WRONLY|O_TRUNC)`, `write`,
`fsync` — by `store::write_record`. It is the only write in the state directory that does
not go through the home, and it lives inside the home's own module.

**Why:** `write_json_atomic` finishes with a `rename`, and a rename replaces the
destination's **inode**. The advisory lock is an `flock` on an open file description of
that inode. Renaming a new file over the lock file therefore does not update the lock
file; it makes the lock file a *different file* that nobody holds, and the next process to
ask finds it free while the first still believes it is the owner. Atomicity applied to the
lock would delete the lock.

The in-place write is safe for a narrower reason than atomicity: the record is only ever
written by the process that already holds the lock, so there is exactly one writer at a
time. A *reader* — `SessionStore::holder`, and the refusal path in `SessionStore::lock` —
can still catch it half-written, and that case is handled by not trusting it: an
unparsable record yields `StoreLocked { holder: None }`, an honestly unidentified holder,
rather than an invented pid. The record is decoration on a fact the kernel owns; the lock
is the `flock`, and `holder()` asks the kernel first (a shared `try_read`) so a *stale*
record left behind by a process that exited can never be reported as a live holder.

`scripts/gates/atomic-write-home.sh` exempts the store module, so this deviation is not
gate-caught and would not be: the gate's subject is bypasses *outside* the home. It is
recorded here because a reviewer reading `write_record` should find the reasoning before
filing the finding.

**Retires when:** nothing retires it. A lock whose file is replaced is not a lock.

---

## N12 — The store's two `fsync`s are the lines no test in this suite can turn red

**Doc:** rubric rule 2 — "for every test: write the buggy implementation" — and AGENTS.md's
"if a test cannot go red, it is not a test". Design D9 asks for
`tempfile in-dir → sync_all → rename → fsync parent`.

**Repo:** all four steps are there. Twenty-one buggy implementations were seeded against
`engine::store` at workspace scope while P3a was written, and nineteen were caught by a
named test — the destination written before the rename or in place of it, an unparsable
middle line dropped like a torn tail, a torn last line dropped even when a terminator
follows it, the version probe skipped or narrowed to newer-only, the session list sorted
oldest-first, the lock guard dropped instead of held, the holder record never written, the
holder read without asking the kernel, the task slug and the control slug believed instead
of derived, the photo path made absolute. **Two survived, and they are the same two lines:
`temp.as_file().sync_all()` and `fsync_dir(dir)`.** Deleting either leaves the whole
workspace suite green.

**Why they survive:** each `fsync` buys *durability*, not visibility. The temp file's sync
keeps the filesystem from ordering the rename ahead of the contents; the directory's sync
keeps the rename itself from being lost. Both are observable only across a power cut or a
kernel crash on a filesystem that reorders metadata. A hermetic test cannot produce either,
and the alternatives were worse: a test that counts syscalls needs `strace` (a runtime
external binary, which design §2.8 forbids), and a test named for durability that only
proves the call compiles is the "green plumbing test named for the whole" rubric Part C
rejects on sight.

The neighbouring property *is* covered, and the distinction is the point:
`a_write_publishes_a_new_inode_instead_of_overwriting_the_old_one` turns red for an
implementation that overwrote the destination instead of renaming over it, because *which
file is published* is observable where *when it reaches the platter* is not.

**Repo, therefore:** `fsyncing_a_directory_is_supported_here_and_its_failure_is_typed`
proves the two things that *are* checkable — the operation is supported on this filesystem
(some refuse `fsync` on a read-only directory descriptor), and its failure is a typed
`StorageIo` rather than a panic — and its name and its comment both say it does not prove
durability. The two uncovered lines are named here instead of covered by a test that would
lie about them.

**Retires when:** a crash-consistency rung exists that can cut power to a filesystem
(a `dm-log-writes` target replayed at arbitrary write boundaries is the usual shape, and
it needs privileges the test suite does not have), or the mutation floor P3f commissions
records this survivor as a reasoned acceptance and gains a mechanism this note does not
anticipate. This entry belongs in design §3.3's structural-gap register at its next
regeneration; the register is regenerated rather than accreted, so it is recorded here in
the meantime.

### Amendment, 2026-08-09: the mutation floor landed, and it neither retires nor reaches this

The second clause above named P3f. P3f has landed (E7), and the answer is that it does
**not** retire this entry, for two reasons worth separating.

**The claim was reproduced, for the first time since it was written.** Deleting
`fsync_dir(dir)` from `write_json_atomic_scripted` leaves 642 tests run, 642 passed;
deleting the temp file's `sync_all()` leaves 642 tests run, 642 passed. Both at workspace
scope, on the current tree.

**But the tool cannot express either mutation.** cargo-mutants replaces function bodies and
flips operators; it does not delete a statement. `replace fsync_dir -> Result<()> with
Ok(())` is generated and is *caught*, by
`fsyncing_a_directory_is_supported_here_and_its_failure_is_typed` — which asserts the typed
failure and says in its own comment that it does not prove durability. So a green mutation
run over `store.rs` must never be read as a re-confirmation of this note. What the floor
did find in the same module is eight other survivors, seven of them ordinary uncovered
lines with tests now written; that is a fact about the 2026-08-09 seeded-defect campaign's
completeness, not about these two lines.

---

## N13 — The store refuses to write a document it could not read back

**Doc:** design D9 says every JSON file carries `schema_version` from day one and that a
foreign one is a typed refusal, and it says so about the *load*. Nothing in D9 constrains
the write.

**Repo:** `SessionStore::save_session` now refuses a `Session` whose `schema_version` is
not this build's, with the same `Error::SchemaVersionForeign { found, supported }` the
load produces. P3a's version of it wrote whatever it was handed.

**Why.** P3a's permissiveness was deliberate and its reasoning was sound as far as it
went: a store that *corrected* the version would make `SchemaVersionForeign` unreachable,
and the field's whole job is to be believed. But "do not correct it" and "write it
anyway" are different rules, and the second one lets a caller persist a file the tool
cannot read — the tool's own write, refused by the tool's own loader, in a directory
whose entire premise is that an agent or a human can pick it up later. P3b is the first
caller that could do it (a lifecycle path that carried a loaded version forward, or a
test fixture that edited one), so the check lands with the first caller rather than after
a session directory has been made unreadable by the thing that wrote it.

Refusing rather than correcting keeps both halves of the original reasoning: the version
is still believed, and it is still the loader that decides what this build can read —
`save_session` asks the same question with the same constants and gives the same answer.

**What it cost.** Two fixtures were built by handing `save_session` a foreign document,
and they are now built from bytes: `a_foreign_schema_version_is_refused_in_both_directions`
edits `schema_version` in the *serialized* form and publishes it through
`write_json_atomic`, which is where another build's document would differ anyway, and the
`StoreFault::ForeignSchemaVersion` arrangement already bumped the version inside the store
rather than at its caller, so it did not change at all. The fixture is more honest for it:
the loader now meets a file it did not write, which is the case it exists for.

**Retires when:** the tool supports more than one session schema version at once — at
which point "this build's version" stops being a single number and both the guard and the
loader need the same replacement.

---

## N14 — What `SessionConflict` means, and why an unreadable session refuses rather than yields

**Doc:** design D8 says a session belongs to a (camera fingerprint, task) pair. D9's
layout puts every session under `<fingerprint-slug>/<task-slug>/<uuidv7>/`, so one task
slug holds many sessions. D13 lists `SessionConflict` and does not say what conflicts.

**Repo:** `engine::lifecycle` fills the blank in three parts.

1. **A task slug holds a history; at most one of its sessions is open.**
   `lifecycle::is_open` is `queue.is_empty() || !session.is_settled()` — a session with an
   empty queue is open (it was created a moment ago), and one with a queue is open until
   every queued control has reached a D8 terminal state. `Session::is_settled` alone would
   not do: it answers `true` for the empty queue, vacuously, so `calibrate start` twice in
   a row would leave two empty session directories where one session was meant.
2. **`create` conflicts with the open session, and names it.** The refusal carries the
   session's uuid in `SessionConflict::detail`, because "resume that one instead" is only
   actionable if the caller is told which one. A settled slot takes a new session, which is
   what makes the many-uuids-per-task layout mean something.
3. **`resume` looks at the newest session in the slot and no further**, and a document it
   cannot read is a refusal — `SchemaVersionForeign` or `StorageIo` — rather than a reason
   to hand back the one before it.

**Why (3) is the uncomfortable one.** It means a session written by a newer build blocks
`create` for that (camera, task) until it is moved aside, and that is deliberate: the two
failures are not symmetric. Skipping past an unreadable document answers "nothing is open
here" without knowing it, and two live sessions sweeping one camera — each with its own
pre-sweep snapshot, the second's recording the first's mid-calibration state — is exactly
what `SessionConflict` exists to prevent, and what `restore` cannot undo afterwards.
Quietly resuming an *older* session is the same mistake wearing a friendlier face: the
operator asked for the work in progress and got somebody else's finished work. A refusal
that names a version is a thing an operator can act on; a wrong session is not.

The escape is ordinary and does not need a flag at this phase: a different task starts a
different slot, and the session tree is a directory an operator can move. If P3d's
`calibrate start` grows a `--force`, this note is what it has to argue with.

**What is not decided here:** nothing detects a *concurrent* live session inside one open
slot, because nothing needs to — the state lock (D9) is what keeps two processes from
interleaving writes, and a session document records no owning pid. If a future phase wants
"this session is being swept right now, by that process", it is a new fact and needs its
own field, not a reinterpretation of this one.

**Retires when:** D8 grows an explicit close/abandon verb, at which point "open" becomes a
recorded state rather than a derived one and part 1 above is replaced by reading it.

---

## N15 — Which of `persist`'s two writes goes first is checkable; which of the store's two `fsync`s lands is not

**Doc:** rubric rule 2 — for every test, the buggy implementation — and note N12, which
recorded the store's two `fsync`s as the lines no hermetic test can turn red.

**Repo:** `engine::lifecycle::persist` writes `session.json` first and appends to
`log.ndjson` second, and `the_document_goes_down_before_the_line_that_describes_it` turns
red when the two are swapped.

**Why it is recorded.** The ordering has the same *shape* as N12's fsyncs — it only
matters across a crash between two steps — so the honest expectation was another
acceptance. It is not one, and the difference is worth stating because it is the test that
makes it: the two writes fail independently, and a fixture that fails exactly one of them
distinguishes the orders. `log.ndjson` is replaced by a **directory**, so `O_APPEND` on it
is `EISDIR` from the real kernel for every user including root — no `chmod`, no privilege
assumption, no scripted store fault. Correct order: the document is on disk and the log
append refuses. Swapped: the append refuses first and the document never lands, and the
test sees the state before the transition.

The fsyncs stay uncovered for the reason N12 gives — their effect is visible only across a
power cut, and neither is a *second* write whose failure can be arranged. "Unobservable in
a hermetic test" turned out to be a claim about the fault that can be injected, not about
the class of property, and this pair is the counter-example that keeps the claim narrow.

**Retires when:** nothing retires it; it is a note about why a neighbouring line *is*
covered, and it belongs beside N12 when design §3.3's structural-gap register is next
regenerated.

### Amendment, 2026-08-09: the same lesson, twice more, from the mutation floor

E7's triage produced two more instances, both of which began as "no test can reach this"
and neither of which survived the second look.

- `pairing::Planner::emit`'s switch-off loop is bounded by `round > partner_count` — one
  round of slack past the "one round per partner" its own comment claims — and tightening
  it to `round == partner_count` looked unobservable, because consistent pair data converges
  within one round per partner and inconsistent data raises the same `ControlInactive`
  either way. It is observable: a device where clearing one partner *puts another back*
  (the case the loop's comment describes) needs three rounds for two partners, and
  `a_partner_a_nested_guard_puts_back_is_cleared_again_rather_than_refused` is that fixture.
- `LockRecord::for_this_process`'s `-1` fallback could become `1` — `init`'s pid — and no
  test can reach it, because a process cannot choose its own pid. The fault was not
  unreachable; the *seam* was missing. `pid_or_unknown(raw: u32)` takes the value, and the
  branch is one assertion away. "Pure cores take values" is not only a design preference;
  it is the difference between a line nothing can watch and a line one assertion can.

Ten survivors were accepted in that run (N25, N26, N27), and each was written only after
this question had been asked of it.

---

## N16 — A session records the pair set it swept with, and the probe that found it is a log line

**Doc:** design D8 lists what a session holds — goal, criteria, per-control status, sweep
plans, samples, ordering, notes — and D9 lists what its directory holds. Neither mentions
automation pairs. D3 says empirical discovery runs "automatically at calibration start"
and that discovered pairs "are recorded in the device profile".

**Repo:** `schema::session::Session` gained

```rust
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub pairs: Vec<AutomationPair>,
```

and `SessionEvent` gained `PairsDiscovered { measured, skipped }`.
`engine::lifecycle::discover_pairs` runs the probe, merges its findings over the declared
table narrowed to this device, writes the merge onto the document and appends the event.

**Why the session and not only the profile.** D3's sentence is about `controls
--discover-pairs`, whose output is a profile, and it stays true. What a *session* needs is
different: the pair set is an input to two operations that must agree with each other
across a process boundary. A sweep's guarded write switches an automation control off
(D3); the restore that undoes it has to write the automation controls back **first** (D4),
and `snapshot::take` decides which controls those are by asking the pair set. When the
restore is a *recovery* — a second process picking up a crashed session (N9, §6) — that
process has never seen this camera. Its choices are to re-run the probe, which writes to a
camera in the middle of an interrupted calibration, or to read the list the first process
used. Only the second one is a recovery.

Storing the *merge* rather than the two layers is the same reasoning one step on: a
consumer that had to re-merge could merge differently, and "measured beats declared" (E1)
would become a rule two places implement instead of one. Provenance survives the merge, so
nothing is lost — a reader can still tell a nomination from an observation.

**Why an event.** `log.ndjson` is the record of what happened *to the device*, which is
why P3b declined to log queue edits. The probe is not a queue edit: it toggles automation
controls and puts them back, so it is the first thing in most sessions that moves the
camera at all. `skipped` is on the line because a probe silent about what it passed over
reads as a probe that found nothing there — the same reason `Discovery` carries it.

**What it does not do:** it does not make the session the home of pairing. The declared
table is still `schema::pairing`, the merge is still `engine::pairing::merge`, and the
narrowing is still `engine::pairing::applicable`. The session stores an answer those three
produced.

**Retires when:** the device profile becomes something a session references by identity —
at which point the session can name a profile instead of copying its conclusion, and the
two would have to agree about staleness instead of about pairs.

---

## N17 — The progress seam has no fault menu, because emitting cannot fail in a way a sweep may act on

**Doc:** design §2.9 says every seam has a real implementation and a scriptable double
"with an exhaustive-match fault menu", and AGENTS.md repeats it as a rule for writing
code. Rubric Part C asks for every fault-menu variant of every seam to be
exhaustive-match-walked with a test.

**Repo:** `engine::progress::ProgressSink::emit` returns `()`. There is a real
implementation (`ChannelSink`, over a bounded `std::sync::mpsc`), a null one (`Silent`) and
a double (`Recorder`), and no `ProgressFault` enum anywhere.

**Why.** A fault menu exists so a caller can be made to meet every way a seam fails and
answer each one deliberately. This seam's failures are "the subscriber's queue is full" and
"the subscriber went away", and the correct answer to both is the same and is not the
caller's to choose: keep sweeping. A sweep holds a camera, has a pre-sweep snapshot on
disk, and is minutes long; ending it because a progress bar closed would put "the terminal
went away" on the list of things that abandon a calibration. Giving `emit` a `Result` would
make that failure *representable at every call site* — five of them in `engine::calibrate`
— and the only correct handling would be to ignore it, which is the shape rubric B11 calls
a suppression looking for a reason.

What the rule is actually protecting against is a seam whose failures are invisible, and
that half is kept: `ChannelSink` counts what it drops and `dropped()` reports it, with a
test that fills the queue and asserts the count, and another that drops the receiver
entirely. The bound is `limits::PROGRESS_QUEUE_DEPTH`, so "how much progress may pile up"
stays in the one table (rubric A14). A dropped event is a number, not a silence.

**What would change this.** If P4e's subscription needs to distinguish "this client is
slow" from "this client is gone" — to reap it, say — that is a *query* on the sink, not a
failure of `emit`, and it lands as another method. The moment `emit` has a failure a sweep
should act on, it needs the menu; nothing has produced one.

**Retires when:** a consumer appears whose disconnection should end a sweep. The daemon is
not it — P4e's disconnect-mid-sweep semantics are already written down as "the sweep
continues, the subscription is reaped" (docs/7 P4e).

---

## PF:17 — A compound control's element count is not invariant: `vivid`'s pixel array reshapes with the negotiated format

**Measured** 2026-08-08 on kernel 7.0.0-29-generic, against `vivid` (one instance, via the
blessed helper), while landing P3c's R2 sweep arm. Continues the docs/6 §1.2 registry;
cite it as `[PF:17]`.

`U8 pixel array` (`u8_pixel_array`, a `V4L2_CTRL_TYPE_U8` compound control) reports
different dimensions before and after a format is negotiated on the same open file
descriptor:

```
before desc:       elems=300 elem_size=1 dims=[15, 20]
negotiated:        320x180 YUYV
after-stream desc: elems=240 elem_size=1 dims=[12, 20]
write-back:        requested len = 300  applied len = 240  equal = false
```

The grid is `ceil(height/16) × ceil(width/16)`: 240×320 at open gives `[15, 20]`, and
`S_FMT` to 320×180 gives `[12, 20]`. An isolated write-back with no `S_FMT` in between is
byte-exact and warning-free, so this is the *format change* reshaping the control, not an
unstable read.

**What it costs this tool, stated rather than fixed.**

1. **T3's invariant/state split assumes `elems`, `elem_size` and `dims` are invariant.**
   `profile::invariant_control` strips `current` and the volatile flag bits and keeps the
   rest, so two `profile capture` runs against one such device disagree in the *invariant*
   section whenever the negotiated format differed between them.
2. **Snapshot/restore of such a control cannot complete across a format change.** The
   snapshot holds a 300-byte payload; the write-back after streaming returns 240 bytes, so
   the write is `WriteWarning::Adjusted`, the outcome is a non-exact `Restored`, and
   `RestoreReport::is_complete()` is **false**. For a calibration session that means the
   persisted pre-sweep snapshot is never consumed (N9's rule, working correctly — something
   really did not go back).

Neither is a defect introduced by the sweep executor and neither is reachable on the seed
hardware: the Chicony's `Region of Interest Rectangle` is a fixed 16-byte payload and the
OBSBOT exposes no compound control. It is reachable on `vivid`, which is why the R2 sweep
arm asserts that **the control it swept** came back rather than asserting the whole restore
report is complete — asserting completeness there would be asserting a driver bug in our
favour, and asserting incompleteness would pin a behaviour the next kernel may change.

**Retires when:** a device is found on which this does not happen *and* the invariant
split is redefined to name which descriptor fields may move — the honest fix is a
per-control statement, since `elems` is invariant for every other control on this driver.
Until then the finding is that a payload's shape is device state, and code that treats it
as identity is wrong on at least one driver in the tree.

---

## N18 — `log.ndjson` gained `SweepInterrupted`, because "where it stopped" is not "why"

**Doc:** design D9 makes the session directory the inspectable record of a calibration, and
docs/7 P3d makes `calibrate status` the verb that reads it. Note N16 states the rule
`SessionEvent` is governed by: the log is the record of what happened *to the device*,
which is why P3b declined to log queue edits. P3c left the question open in as many words —
`CalibrationProgress::SweepInterrupted` had no durable counterpart, and the recorded
`SampleTaken` lines already bound an interruption.

**Repo:** `schema::session::SessionEvent::SweepInterrupted { control, taken, total, failure,
detail }`, appended by `engine::calibrate::run` on the path that returns the error, through
`lifecycle::note` — an append with no document change, because the samples that survived
were each committed as they were taken and the document already says what it needs to.

**Why.** The bound the samples give is a bound on *when*, and the question `calibrate
status` is asked is *why*. A camera that was unplugged, a sensor that never settled
\[PF:11\] and a filesystem that filled leave byte-identical session directories: three
samples of sixteen, a control left `Sweeping`, and a pre-sweep snapshot still armed. Design
keeps those three apart everywhere else — availability is not capability, and the D13
registry exists so a caller can act on the difference — and the one place they were
collapsed was the record an operator reads after the process that saw the failure is gone.
The live event carries the same fact to whoever was watching; this is the copy that
survives the terminal.

**What the line does not mean.** Its *absence* is not evidence. A process that was killed
leaves no line either, which is exactly the case design §6's persisted snapshot exists for.
A present line means a known reason; an absent one means the reason is not known here, and
nothing reads it as "the sweep is still running".

**Why the append is best-effort, and why that is not a swallowed error.** The note is
attempted and its failure is not returned, because the sweep already has a refusal to
report and the alternative is answering "the disk is full" to somebody whose camera was
pulled out — the one conversion AGENTS rule 7 forbids. The store that could not take this
line cannot take the next operation's either, so nothing is hidden for long: the caller
meets it at its own next write. `emit`'s missing fault menu (N17) is the neighbouring
decision and is not the same one — that seam cannot fail in a way a sweep may act on, and
this one can, but not in a way that may displace the failure it is describing.

**Retires when:** a session gains a durable "this sweep is running, owned by that process"
fact. That would make silence itself readable, and the interruption line would become one
of three states rather than the only one that speaks.

---

## N19 — `calibrate plan` is the draft, and a control the device will not calibrate is recorded rather than omitted

**Doc:** design D8 says a session holds "an ordered control queue the caller may reorder
between sweeps" and lists `Blocked { reason }` in the per-control vocabulary; it does not
say which verb fills either. The skill's step 6 (vendor/v4l2-webcam-skill,
`references/calibrating.md`) says to write a draft covering **all** the setting names, and
step 7 says to verify that it does.

**Repo:** `wch calibrate plan <camera> --task …` with no controls named classifies every
control the camera enumerates: the sweepable ones are queued, and the rest get
`ControlStatus::Blocked` with the device's reason. Naming controls queues those instead;
`--order` treats the named controls as a permutation of the existing queue.
`lifecycle::draft` is the engine half, and a control that already has a status is left
alone — re-drafting a session mid-run is ordinary, and a draft that re-classified a
calibrated control would throw away the value somebody chose.

**Why blocked rather than absent.** A draft that silently omitted the read-only controls
would read as a device that does not have them, and the operator performing the skill's
step 7 — check the draft against the control list — would find the two disagreeing with no
way to tell an omission from a device difference. `BlockedReason` is D8's answer and this
is its first producer; before P3d the variant existed with nothing to write it, which is
the "typed declaration nothing reads" rubric A8 calls a defect.

**Where each reason comes from.** Nothing here re-derives a rule: `DISABLED` is read off
the descriptor because it is a device fact D8 names in its own right and the write planner
deliberately reports it and READ_ONLY as one refusal; "no ordered range" is
`sweep::plan`'s answer; "read-only" and "INACTIVE with nothing to free it" are
`pairing::plan`'s. A classifier that restated any of them would be a second copy of a law,
drifting the first time the original changed. The question is asked with
`allow_motion = true`, because a control that moves motors is a reason a *sweep* needs a
flag (design §5) and not a reason the control cannot be calibrated — blocking every PTZ
control at draft time would refuse the OBSBOT its whole purpose.

**Retires when:** D8 grows an explicit per-control plan on the document (a stored
`SweepSpec` a later `sweep` reads), at which point drafting and planning become two facts
and this verb records both.

---

## N20 — `calibrate apply` does not restore, and does not consume the pre-sweep snapshot

**Doc:** design D4 and AGENTS rule 7: sweeps and guarded operations wrap themselves in
snapshot/restore by default and the tool leaves the camera as it found it unless told to
keep changes. D8 says `calibrate apply` "replays a session's calibrated values … against a
fingerprint-matched camera — the skill's calibration script as data instead of Bash".

**Repo:** `lifecycle::apply` performs the guarded write and stops. Nothing is restored
afterwards, and `Session::pre_snapshot` is left exactly as it was.

**Why.** The two halves of D4's sentence are about different operations. A sweep *borrows*
the camera — it moves a control to take a photograph and has no interest in where it left
it — so it restores, and design §6's persisted snapshot is what makes that survive a crash.
`apply` is the operator saying "leave it like this": it is the skill's step 12, the reason
the session was recorded at all, and a restore afterwards would undo the only thing the
verb does. Reading rule 7 as "every write restores" would make the calibration
unusable by the tool that produced it.

Consuming the snapshot would be the same mistake from the other side. That record describes
the camera **before the calibration**, and it is the only route back to it; `apply` is when
an operator is most likely to want that route. So the record stays, and
`lifecycle::recover` — the same function an ordinary session end and a crash recovery both
run — is what spends it.

**What still holds.** The write is guarded, so D4's *ordering* is honoured: an automation
control that owns a calibrated value is switched off first and the report names it in
`disabled_automation`, because applying a calibration changes more than the controls it
lists and the caller is entitled to hear so.

**Retires when:** `apply` grows a scope — "apply for this command and put it back after" —
which is a different verb with a different lifetime, not a flag on this one.

---

## PF:18 — A PTZ move is acknowledged before it happens: `pan_absolute` reads back the *commanded* position

**Measured** 2026-08-09 on kernel 7.0.0-29-generic, against the OBSBOT Tiny 3
(`3564:ff02`, `/dev/video4`), while landing P3e's R3 motion arm. Continues the docs/6 §1.2
registry; cite it as `[PF:18]`.

`pan_absolute` declares `-468000..=468000` step `3600`. Driving it across 216000 units —
23% of the declared range, and a visible arc on a gimbal — returns from the ioctl in the
same time as a write to a control with nothing mechanical behind it, and the new value is
readable immediately:

```
pan_absolute -> 108000   : write returned in 126 ms   (first open; includes device open)
pan_absolute -> -108000  : write returned in  21 ms
pan_absolute -> 0        : write returned in  26 ms
brightness   -> 60       : write returned in  19 ms   (the no-motor baseline)
brightness   -> 50       : write returned in  22 ms

after pan_absolute=108000, six successive G_EXT_CTRLS polls: 108000 108000 108000
                                                             108000 108000 108000
```

**The head does move** — it is the acknowledgement that is early, not the mechanism that is
absent. Photographed at three pan positions, the scene the frames measure is different at
each:

```
 -108000 -> -108000  luma=0.1160 rms=0.1580 sharp=13.63 shadows=0.4171
       0 ->       0  luma=0.0986 rms=0.1651 sharp=12.67 shadows=0.5432
  108000 ->  108000  luma=0.1267 rms=0.1676 sharp=14.80 shadows=0.4063
```

No head traverses that arc in 21 ms. So `G_EXT_CTRLS` on this control reports where the
camera was told to point, not where it points.

**What it costs this tool, stated rather than fixed.**

1. **For a motorized control, `{requested, applied}` means requested versus *accepted*, not
   requested versus *achieved*.** D3's read-back doctrine is unchanged everywhere else —
   this is the one control class on the seed hardware where the read-back is an
   acknowledgement rather than a measurement, and PF:6's clamping is still visible through
   it because clamping happens in the acknowledgement.
2. **"The motor came back" is a claim about the commanded position.** §5 requires every
   motion arm to restore what it moved and assert it, and `hw_motion_a_bounded_ptz_sweep_…`
   does — against the strongest statement V4L2 offers on this device. Nothing in the
   control surface reports mechanism state, so a stronger assertion would have to be
   invented, and an invented assertion about a motor is worse than an honest narrow one.
3. **A motion sweep's settle counts frames \[PF:11\], not motion.** The P3e arm moves one
   control step per sample and is not exposed to this. A `--all` pan sweep is: the motion
   cap widens the stride to 32400 units — about 3.5% of the range per sample — and a frame
   captured while the head is still travelling would be recorded against a position it had
   not reached. The sample would be labelled with the driver's answer, which would be the
   commanded value, and nothing downstream could tell.

**Not corpus-shaped.** Like PF:6, this is a behaviour under a write rather than a field in
a descriptor: the profile records `pan_absolute`'s range, step and flags, and every one of
them is unchanged. The frames that show the head moving are camera frames and never enter
the repository (design §3.2, §5), so the evidence for it is this transcript. The
registry-completeness walk in `corpus_replay.rs` covers PF:1–14, the v1 registry, and is
left alone here as it was for PF:15–17.

**Retires when:** a device is found whose pan read-back tracks the mechanism — at which
point "accepted" and "achieved" become distinguishable and the tool can say which it has —
or a settle policy learns to wait on frame stability for motion controls, which would make
point 3 a bug rather than a limit.

---

## N21 — A `Calibrated` record does not say whether the metric ranked or merely tied

**Doc:** design D8 says metrics *rank*, they do not decide, and that the `Calibrated`
record names its `selector` — `metric:<name>`, `agent`, or `human` — precisely so nobody
pretends a Laplacian knows what "text legible on the DUT" means.
`engine::session::select_by_metric` implements it: the best-scoring sample wins, ties keep
the earliest.

**Repo:** unchanged. What landed is the *observation*, and one counted skip in the R3 arm.

**Why.** The first real calibration this project ran (E5) produced, on the Chicony, a sweep
whose `clipped_highlights` score was `0.0000` for every one of its five samples. The
selector did exactly what it is specified to do — no sample improved on the first, so the
first won — and wrote

```
brightness: Calibrated { value: 0, score: Some(0.0), selector: Metric { name: ClippedHighlights } }
```

That record is byte-identical in shape to one where a metric genuinely separated five
samples. From the document, from `calibrate status`, and from the `Selected` log line, a
tie-break and a ranking look the same. It is the shape docs/8 Part C names — an answer that
reads as a decision and was not one — and the three ways it arises on real optics are all
ordinary: a scene with no content in the metric's dimension, a control whose range has a
dead zone, and a camera whose lens is covered (E5's Chicony was the third).

**What the R3 arm does about it.** It selects with `rms_contrast`, computes the expected
winner from the recorded scores itself, and then asks whether any sample scored *below* the
winner. If one did, the ranking is asserted to have discriminated. If none did, the arm
prints a named partial skip saying the metric scored every sample the same, so the run
reports "the selection was a tie-break" rather than counting it as a ranking that worked.
Counting is the whole defence: nothing else distinguishes the two.

**What this is not.** It is not an argument for making ties an error. A tie is a real
answer — three exposures that clip identically really are indistinguishable to that metric
— and refusing would leave a session stuck on a control the operator could settle by
looking. The gap is that the *record* keeps only the winner.

**Retires when:** `ControlStatus::Calibrated` carries the spread the ranking saw — the
range of scores, or the runner-up's — so a reader can tell a decision from a coin toss
without the samples in front of them. That is a D8 schema change and a docs/6 amendment,
not a P3e edit.

---

## E5 — G3 hardware evidence: calibration meets real optics, 2026-08-09

docs/7's P3e asks for the R3 evidence run — a real calibration session on the Chicony RGB
over a brightness-class control, and a bounded PTZ sweep on the OBSBOT that restores the
motor position and asserts it — "recorded in the notes with transcripts", under the same
carve-out G1 and G2 used: *the recipe existing and selecting tests is the gate criterion,
and the run itself is evidence*. This is that record. Evidence entries are dated and
appended; they are not amended.

**Host:** kernel 7.0.0-29-generic, x86_64. **Attached:** Chicony `04f2:b83c` (RGB on
`3-4:1.0`, IR on `3-4:1.2`), OBSBOT Tiny 3 `3564:ff02` on `3-1:1.0`. Six `/dev/video*`
nodes.

### R3 — `just smoke-hw`, motors included

```
smoke-hw: motor-moving suites (hw_motion_*) are included — set WCH_NO_MOTION=1 to exclude them
smoke-hw: 6 capture node(s) present; running test(/(^|::)hw_/)
    Starting 15 tests across 28 binaries (622 tests skipped)
     Summary [  21.155s] 15 tests run: 15 passed, 622 skipped
smoke-hw: 8 claim(s) declined by tests that ran — each named above
smoke-hw: suite run, 0 named skip(s) before it started
```

The calibration session, on both cameras that offer a brightness-class control:

```
cam:…-integrated-c: probe measured 2 pair(s), declined 0, left the camera alone: true
cam:…-integrated-c: draft covered 18 of the device's 18 control(s) — 14 queued, 4 blocked:
  camera_controls (NotSweepable { control_type: "control_class" }), privacy (ReadOnly),
  region_of_interest_rectangle (NotSweepable { control_type: "rect" }),
  user_controls (NotSweepable { control_type: "control_class" })
cam:…-integrated-c: swept brightness — sharpness          0:0.0803 63:0.2428 126:0.0076 189:1.7370 252:1.6129
cam:…-integrated-c: swept brightness — clipped_highlights 0:0.0000 63:0.0000 126:0.0000 189:0.0000 252:0.0000
cam:…-integrated-c: swept brightness — clipped_shadows    0:1.0000 63:1.0000 126:1.0000 189:0.0000 252:0.0000
cam:…-integrated-c: swept brightness — mean_luma          0:0.0001 63:0.0006 126:0.0000 189:0.1684 252:0.3215
cam:…-integrated-c: swept brightness — rms_contrast       0:0.0006 63:0.0018 126:0.0001 189:0.0020 252:0.0019
cam:…-integrated-c: apply refused with 13 control(s) pending, and wrote nothing
cam:…-integrated-c: applied brightness=189 (selector metric:rms_contrast, score 0.0020),
  1 write(s), automation off: []
cam:…-integrated-c: calibration session 019fe59c-534e-7682-9e26-3d99d9735684 — 5 sample(s),
  restore complete, 16 control(s) back where the sweep found them

SKIP (partial): cam:…-integrated-i exposes no sweepable brightness-class control

cam:obsbot-tiny-3…: probe measured 4 pair(s), declined 1, left the camera alone: true
cam:obsbot-tiny-3…: draft covered 24 of the device's 24 control(s) — 22 queued, 2 blocked:
  camera_controls, user_controls (both NotSweepable { control_type: "control_class" })
cam:obsbot-tiny-3…: swept brightness — sharpness          0:2.1263 25:9.4992 50:22.1370 75:90.5446 100:195.9731
cam:obsbot-tiny-3…: swept brightness — clipped_highlights 0:0.0000 25:0.0000 50:0.0035 75:0.0038 100:0.0038
cam:obsbot-tiny-3…: swept brightness — clipped_shadows    0:0.5555 25:0.5140 50:0.4211 75:0.0003 100:0.0000
cam:obsbot-tiny-3…: swept brightness — mean_luma          0:0.0502 25:0.0833 50:0.1240 75:0.2567 100:0.3717
cam:obsbot-tiny-3…: swept brightness — rms_contrast       0:0.0763 25:0.1186 50:0.1649 75:0.2543 100:0.3089
cam:obsbot-tiny-3…: apply refused with 21 control(s) pending, and wrote nothing
cam:obsbot-tiny-3…: applied brightness=100 (selector metric:rms_contrast, score 0.3089),
  1 write(s), automation off: []
cam:obsbot-tiny-3…: calibration session 019fe59c-6a7e-76a2-ba60-bb54ae3fcc60 — 5 sample(s),
  restore complete, 22 control(s) back where the sweep found them
```

The PTZ sweep — the first motor this project has ever driven:

```
SKIP (partial): cam:…-integrated-c exposes no writable pan/tilt control, so nothing here has a motor to move
SKIP (partial): cam:…-integrated-i exposes no writable pan/tilt control, so nothing here has a motor to move
cam:obsbot-tiny-3…: pan_absolute declares -468000..=468000 step 3600 — 261 samples at full
  stride, bounded to 29 by the motion cap [limits::MAX_MOTION_SWEEP_SAMPLES]
cam:obsbot-tiny-3…: probe measured 4 pair(s), declined 1, left the camera alone: true
cam:obsbot-tiny-3…: pan_absolute requested -> applied -7200->-7200 -3600->-3600 0->0
  3600->3600 7200->7200
cam:obsbot-tiny-3…: moved pan_absolute through [-7200, -3600, 0, 3600, 7200] — 5 sample(s),
  14400 units of travel (4 step(s)), and back to 0
```

### R3 — `WCH_NO_MOTION=1 just smoke-hw`

Run both ways, because "the knob excludes the motion arms" and "the exclusion is named and
counted" are different claims and only running it proves the second:

```
smoke-hw: SKIP 1 — motor-moving suites (hw_motion_*) are excluded by WCH_NO_MOTION=1; unset it to include them
smoke-hw: 6 capture node(s) present; running test(/(^|::)hw_/) - test(/(^|::)hw_motion_/)
    Starting 14 tests across 28 binaries (623 tests skipped)
     Summary [  17.827s] 14 tests run: 14 passed, 623 skipped
smoke-hw: 6 claim(s) declined by tests that ran — each named above
smoke-hw: suite run, 1 named skip(s) before it started
```

15 with motors, 14 without, and the difference is one named, counted skip rather than a
smaller number nobody noticed.

### What the run establishes

- **The D8 loop closes against a real camera.** Session create, empirical pair discovery
  persisted onto the document (N16), a draft covering every control the device enumerates
  (N19 — 18 of 18 and 24 of 24, with the blocked ones carrying the device's own reason), a
  five-value guarded sweep with one photo scored and stored per sample, a metric selection
  recording its selector and score, `apply` refused without `--partial` and *nothing written*
  to the camera by the refusal, `apply --partial` writing exactly the calibrated set, and
  the pre-sweep snapshot put back with every control asserted to be where the sweep found
  it. The photos never entered the tree: each session lives in a temporary store that is
  removed when the test ends.
- **`--partial` is load-bearing on real hardware**, not a fixture artefact: a draft over
  the whole control set leaves 13 controls pending on the Chicony and 21 on the OBSBOT, so
  the refusal is the ordinary path and the flag is how an operator says they meant it.
- **The motion cap is real on a real range.** `pan_absolute`'s declared range is 261 samples
  at full stride and the planner bounds it to 29, naming
  `limits::MAX_MOTION_SWEEP_SAMPLES` as the cap that did it. The arm then sweeps five
  values — deliberately far under the cap, because §5's ceiling is what a caller may spend
  and a test should spend the least travel that proves the loop.
- **Two open questions from E3 are closed.** E3 listed "Motors — no pan, tilt or zoom
  control has been written on this machine" under *not established*. Pan is now written,
  swept, and returned. And calibration, which had only ever run against the fake, has run
  against two cameras.

### What it does not establish, and one thing it changed

- **Calibration *efficacy* on the Chicony.** That camera photographed a flat field: at every
  brightness its `rms_contrast` stays under 0.002 and its `clipped_shadows` is exactly
  1.0000 for the whole lower half of the range, while the OBSBOT in the same room reads
  0.076–0.309 and 0.555–0.000. Under a forced manual exposure of 1250 the Chicony reaches
  `mean_luma` 0.4980 with `rms_contrast` still 0.0000 — a uniform field lifting as a whole,
  which is a covered lens or a featureless surface at close range, not a scene. So the
  Chicony's transcript demonstrates the *mechanism* end to end and says nothing about
  focusing on a subject; the OBSBOT's monotone sharpness curve (2.1 → 195.9) is the half of
  this evidence with a scene behind it. Design §3.3 item 2 already says calibration efficacy
  on real optics is only demonstrable on R3 — this run demonstrates it on one of the two
  cameras, and names which.
- **PF:18** came out of it: `pan_absolute` acknowledges a move before the head has made it,
  so the read-back is the commanded position rather than a measured one. The motion arm's
  restore assertion is written knowing that, and says so.
- **N21** came out of it: the first selection this project ran on real optics used
  `clipped_highlights`, which scored `0.0000` for all five Chicony samples. The record it
  produced — `Calibrated { value: 0, score: Some(0.0), selector: metric:clipped_highlights }`
  — is indistinguishable from one a real ranking produced. The arm now selects with
  `rms_contrast`, computes the winner itself, and prints a named partial skip when the
  metric failed to separate any two samples, so a tie-break is counted rather than read as a
  decision. With that change the Chicony's own sweep produced a genuine interior optimum
  (189 of 0..=255), which is what a calibration is supposed to look like.
- **A defect the run found in the command surface, fixed here with its gate:**
  `wch calibrate sweep --values -108000,0,108000` was refused with "unexpected argument
  '-1' found", because clap reads a leading minus as a flag. Every PTZ range is centred on
  zero, so this is the ordinary case for the device class the tool exists to drive, not an
  edge one. `--values` and `select --value` are now `allow_hyphen_values`, and
  `a_negative_control_value_survives_the_command_line_in_both_flag_forms` is red without the
  fix in both the `--flag value` and `--flag=value` forms.
- **Still one host, one kernel, three cameras** (design §3.3 item 8), and one of the three
  has nothing in front of it.

### Amendment, 2026-08-09: the Chicony's lens was covered, and it no longer is

The transcripts above are as-run and stand: on 2026-08-09 the Chicony RGB camera really did
photograph a flat field, and every number recorded for it is what it measured. This
amendment records what changed and what the change lets the same run say — appended rather
than folded in, the way E1's amendments are, because the point of a transcript is that it
was true once.

**What changed in the world.** The entry above inferred, from `rms_contrast` never rising
above 0.002 and `clipped_shadows` sitting at exactly 1.0000 across the lower half of the
brightness range, "a covered lens or a featureless surface at close range, not a scene". The
inference was right: the camera had a physical lens blinder over it. The blinder has been
removed. Nothing in the tool, the tests or the corpus changed for this; the sweep below is
the same arm running the same plan against the same control on the same kernel.

A third camera — the Dell U3224KB/A \[PF:19\] — was also attached between the two runs, so
the suite now walks four logical cameras instead of three and ten `/dev/video*` nodes
instead of six.

### R3 — `just smoke-hw`, motors included, three cameras attached

```
smoke-hw: motor-moving suites (hw_motion_*) are included — set WCH_NO_MOTION=1 to exclude them
smoke-hw: 10 capture node(s) present; running test(/(^|::)hw_/)
    Starting 15 tests across 28 binaries (626 tests skipped)
     Summary [  48.851s] 15 tests run: 15 passed, 626 skipped
smoke-hw: 8 claim(s) declined by tests that ran — each named above
smoke-hw: suite run, 0 named skip(s) before it started
```

The Chicony's brightness sweep, with nothing in front of the lens:

```
cam:…-integrated-c: probe measured 2 pair(s), declined 0, left the camera alone: true
cam:…-integrated-c: draft covered 18 of the device's 18 control(s) — 14 queued, 4 blocked:
  camera_controls (NotSweepable { control_type: "control_class" }), privacy (ReadOnly),
  region_of_interest_rectangle (NotSweepable { control_type: "rect" }),
  user_controls (NotSweepable { control_type: "control_class" })
cam:…-integrated-c: swept brightness — sharpness          0:3.9823 63:11.8826 126:16.2084 189:16.3432 252:12.5131
cam:…-integrated-c: swept brightness — clipped_highlights 0:0.0000 63:0.0000 126:0.0000 189:0.0000 252:0.6189
cam:…-integrated-c: swept brightness — clipped_shadows    0:0.7489 63:0.0962 126:0.0004 189:0.0000 252:0.0000
cam:…-integrated-c: swept brightness — mean_luma          0:0.0120 63:0.2179 126:0.4528 189:0.6987 252:0.9414
cam:…-integrated-c: swept brightness — rms_contrast       0:0.0134 63:0.0804 126:0.1170 189:0.1181 252:0.1134
cam:…-integrated-c: apply refused with 13 control(s) pending, and wrote nothing
cam:…-integrated-c: applied brightness=189 (selector metric:rms_contrast, score 0.1181),
  1 write(s), automation off: []
cam:…-integrated-c: calibration session 019fe5ed-ccf5-7a12-80ed-7cf35f0cfa0d — 5 sample(s),
  restore complete, 16 control(s) back where the sweep found them
```

Beside it, the other two, for the comparison the original entry drew:

```
cam:obsbot-tiny-3…: swept brightness — sharpness          0:1.8505 25:8.4847 50:19.6604 75:76.0378 100:133.7970
cam:obsbot-tiny-3…: swept brightness — clipped_highlights 0:0.0000 25:0.0000 50:0.0026 75:0.0027 100:0.0027
cam:obsbot-tiny-3…: swept brightness — clipped_shadows    0:0.4539 25:0.3504 50:0.1525 75:0.0000 100:0.0000
cam:obsbot-tiny-3…: swept brightness — mean_luma          0:0.0439 25:0.0916 50:0.1403 75:0.2883 100:0.4200
cam:obsbot-tiny-3…: swept brightness — rms_contrast       0:0.0558 25:0.1072 50:0.1466 75:0.2248 100:0.2666
cam:obsbot-tiny-3…: applied brightness=100 (selector metric:rms_contrast, score 0.2666)

cam:dell-u3224kb…: swept brightness — sharpness           0:20.8620 63:35.7764 126:55.3860 189:121.0135 252:136.1497
cam:dell-u3224kb…: swept brightness — clipped_highlights  0:0.0000 63:0.0000 126:0.0000 189:0.0000 252:0.0000
cam:dell-u3224kb…: swept brightness — clipped_shadows     0:0.0906 63:0.0756 126:0.0437 189:0.0200 252:0.0059
cam:dell-u3224kb…: swept brightness — mean_luma           0:0.3022 63:0.3866 126:0.4565 189:0.5991 252:0.7059
cam:dell-u3224kb…: swept brightness — rms_contrast        0:0.1404 63:0.1699 126:0.1920 189:0.2396 252:0.2640
cam:dell-u3224kb…: applied brightness=252 (selector metric:rms_contrast, score 0.2640)
```

### R3 — `WCH_NO_MOTION=1 just smoke-hw`, unchanged in kind

```
smoke-hw: SKIP 1 — motor-moving suites (hw_motion_*) are excluded by WCH_NO_MOTION=1; unset it to include them
smoke-hw: 10 capture node(s) present; running test(/(^|::)hw_/) - test(/(^|::)hw_motion_/)
    Starting 14 tests across 28 binaries (627 tests skipped)
     Summary [  34.468s] 14 tests run: 14 passed, 627 skipped
smoke-hw: 6 claim(s) declined by tests that ran — each named above
smoke-hw: suite run, 1 named skip(s) before it started
```

15 with motors and 14 without, as before; the difference is still one named, counted skip.

### What the uncovered run establishes that the covered one could not

- **`rms_contrast` ranks this camera's samples, and the ranking has something behind it.**
  Five distinct scores spanning 0.0134 to 0.1181 — an order of magnitude — where the covered
  run spanned 0.0001 to 0.0020. The selection is an interior optimum at 189 of 0..=255, with
  252 scoring **below** it (0.1134 < 0.1181) rather than beside it. The original entry's
  `rms_contrast` row also happened to peak at 189, and its winning margin over 252 was
  0.0001; that is the difference between a ranking and an ordering of noise, and only the
  second run can tell them apart.
- **The physical claim the arm makes is now a claim about a scene.** `mean_luma` runs 0.0120
  → 0.9414 monotonically, and `clipped_highlights` becomes non-zero (0.6189) at the top of
  the range while `clipped_shadows` falls to 0.0000 — a frame with content in it being
  driven from underexposed to blown out. Under the blinder, `mean_luma` moved too (0.0001 →
  0.3215) because a uniform field lifts as a whole; what it could not do was *clip at one
  end and not the other*.
- **`sharpness` now has an interior maximum** (16.34 at 189, falling to 12.51 at 252), which
  is a lens looking at something. The covered run's sharpness row was 0.0803 / 0.2428 /
  0.0076 / 1.7370 / 1.6129 — two orders of magnitude smaller and not ordered.
- **Design §3.3 item 2 — "calibration efficacy on real optics is only demonstrable on R3" —
  is now demonstrated on three cameras rather than one.** The original entry named which of
  the two had a scene behind it; all three now do.

### What it still does not establish

- **N21's distinction resolved as "ranked" in *both* runs, and that is worth saying plainly.**
  The counter the arm carries asks whether any sample scored below the winner; it fires only
  on an exact tie. A flat field that produces five scores differing in the fourth decimal
  passes it, which is what happened on 2026-08-09 before the blinder came off. So this run
  does not *retire* N21 and does not resolve it in a way the previous run had not — it
  confirms that N21's counter measures tie-versus-not, not signal-versus-noise, and that the
  `Calibrated` record still carries no spread. N21's "retires when" is unchanged.
- **The "fully lightless camera" guard has never fired, on either run.** P3e added it for
  the case `clipped_shadows == 1.0` at *every* sample; the Chicony under its blinder was
  1.0000 at three samples of five and 0.0000 at the other two, so the guard did not fire
  then either, and with the blinder off the metric never reaches 1.0 on any camera. Its
  `else` branch — the `mean_luma` ordering assertion — is the one that has run every time,
  so nothing here is vacuous; what is true is that the guard is a standing allowance for a
  condition this host has never presented, and it is left in place rather than deleted
  because a covered lens is exactly the desk a contributor might run this on. Recorded so
  that "the guard exists" is not mistaken for "the guard was exercised".
- **Nothing about the metrics' agreement with a human.** `rms_contrast` picked 189; whether
  an operator would call that sample correctly exposed is what D8's `selector` field exists
  to keep separate, and no human ranked these samples.
- **Still one host, one kernel** (design §3.3 item 8) — but now three cameras, four logical
  cameras, ten nodes, and none of them with anything in front of it.

---

## PF:19 — One camera can own two capture nodes: a UVC device with two output terminals on one sensor

**Measured** 2026-08-09 on kernel 7.0.0-29-generic, against the Dell U3224KB/A 4K Webcam
(`413c:c03d`, interface `2-3.4.1.1:1.0`), the day it was attached. Continues the docs/6 §1.2
registry; cite it as `[PF:19]`.

PF:7 says nodes group by USB interface, and one USB device can host several logical cameras.
Both halves hold. What does not hold is the inference the codebase had drawn from them —
that a group with two capture nodes is two cameras that grouping failed to separate. This
device is one camera with two capture nodes:

```
/dev/video6  device_caps 0x04200001  capture   NV12/YUYV/MJPG, 13 sizes, up to 3840x2160
/dev/video7  device_caps 0x04a00000  metadata  UVCH + UVCM
/dev/video8  device_caps 0x04200001  capture   NV12 640x480 only, one size
/dev/video9  device_caps 0x04a00000  metadata  UVCH + UVCM

all four hang off ONE interface: /sys/.../2-3.4.1.1/2-3.4.1.1:1.0/video4linux/
```

Its USB descriptors say why. There is a single Camera Sensor input terminal (ID 1) feeding a
single processing unit (ID 3) and extension unit (ID 2), and **two** USB Streaming output
terminals (IDs 4 and 5) taking their input from it. Each output terminal gets its own
VideoStreaming interface (`:1.1` with `bTerminalLink 4`, `:1.2` with `bTerminalLink 5`), and
`uvcvideo` registers a capture node and a metadata node per streaming interface — all of
them children of the one VideoControl interface, which is the grouping key. One sensor, two
streams, four nodes, one control set.

**What it costs this tool, stated rather than fixed.**

1. **"Two capture nodes in a group" was an assertion, and it was wrong.** The R3 arm
   `hw_nodes_group_by_interface_and_capture_nodes_are_found_by_capability` asserted
   `carrying.len() <= 1` with the message "the group is two cameras, not one". It went red
   on this device the first time the rung met it — which is the rung working. The claim that
   replaces it is the one a caller actually depends on: `capture_node()` answers the
   **first** node carrying `VIDEO_CAPTURE`, in node order. The multi-capture case is now
   printed as a live observation and its absence as a named partial skip, the way PF:13's
   counter-example already was.
2. **The tie-break is positional because V4L2 offers nothing else.** Both nodes report
   identical `device_caps` (`0x04200001`), identical `QUERYCAP` card, driver and `bus_info`
   strings, and identical `capabilities`. Only opening each and comparing format trees
   distinguishes the full-resolution stream from the 640×480 secondary, and enumeration does
   not open nodes for their formats. On this device the first node is the full one, and that
   is `uvcvideo` registering streaming interfaces in descriptor order — a convention, not a
   guarantee. The rule is now written down in `CameraInfo::capture_node` and in
   `enumerate::representative` rather than implied by the singular in a doc comment.
3. **The second stream is listed but not reachable.** `CameraInfo::nodes` carries all four,
   so nothing is hidden and `wch info` shows them; but T1/T2 have no vocabulary for "stream
   the other node", so the 640×480 secondary cannot be opened through this tool. Naming a
   node as a capture target is a design change (T1's `open` takes a `CameraId`), not a bug
   fix, and it is not made here.
4. **T3 captures the camera, not each node.** The committed profile records all four nodes
   and one format tree — the one `capture_node()` selected. A profile of this device is
   therefore silent about what `/dev/video8` offers; the raw `ENUM_FMT` walk above is the
   only record of it.

**Corpus.** `corpus/profiles/dell-u3224kb.json`, captured by `wch profile capture` before
anything wrote to the device. `one_camera_with_two_capture_nodes_is_in_the_corpus_as_one_document`
asserts the shape from that document and fails if it is dropped or split;
`four_nodes_with_two_capture_members_are_one_camera_not_two` and
`a_second_capture_node_does_not_become_a_second_camera_alongside_the_others` pin the pure
grouping against the measured topology; `a_group_with_two_capture_nodes_picks_the_first_and_keeps_the_other_listed`
pins the tie-break and its inverse. The registry-completeness walk in `corpus_replay.rs`
covers PF:1–14, the v1 registry, and is left alone here as it was for PF:15–18.

**Retires when:** a device is found whose two capture nodes are distinguishable from their
descriptors alone — at which point "first" can become a rule with a reason — or T1 grows a
way to name a node, at which point the second stream stops being invisible and this becomes
a feature note rather than a limit.

---

## PF:20 — `pan_absolute`, `tilt_absolute` and `zoom_absolute` are not evidence of a motor

**Measured** 2026-08-09 on kernel 7.0.0-29-generic, against the Dell U3224KB/A 4K Webcam
(`413c:c03d`, `/dev/video6`). Continues the docs/6 §1.2 registry; cite it as `[PF:20]`.

A camera built into a monitor bezel enumerates the same three motion controls the OBSBOT
gimbal does:

```
pan_absolute    int  -144000..144000  step 3600  default 0    current 0
tilt_absolute   int  -144000..144000  step 3600  default 0    current 0
zoom_absolute   int      100..500     step 1     default 100  current 100
```

Nothing in the control surface separates these from the OBSBOT gimbal's. The slugs are the
same, the types are the same, the flag words are the same (`0x1000`, `has_which_min_max`
only), and pan and tilt differ from the OBSBOT's only in magnitude — ±144000 arc-seconds
against ±468000 and ±324000. (`zoom_absolute` is the one that looks different: 100..500
against the OBSBOT's 0..100, which reads as a percentage scaled by a hundred. It is a shape,
not a mechanism.)

Both of this workspace's motor predicates therefore fire on it, and they are different
predicates: `testkit::battery::is_motorized` — pan/tilt/zoom/focus/roll, which keeps every
non-motion test arm off all three — and `engine::sweep::is_motion_control`, which is
pan/tilt only and is what makes the product demand `--allow-motion` and apply
`limits::MAX_MOTION_SWEEP_SAMPLES`. So the tool treats this device exactly as it treats a
gimbal: the `hw_motion_` arm swept its pan, and the cap bound its 81-sample full-range plan
to 27.

```
cam:dell-u3224kb…: pan_absolute declares -144000..=144000 step 3600 — 81 samples at full
  stride, bounded to 27 by the motion cap [limits::MAX_MOTION_SWEEP_SAMPLES]
cam:dell-u3224kb…: pan_absolute requested -> applied -7200->-7200 -3600->-3600 0->0
  3600->3600 7200->7200
```

**What is and is not established.** That the controls exist, accept writes, read back what
they were sent \[PF:18\], and restore. **Not** whether anything mechanical moves: this device
is embedded in a display and its pan is far more likely to be a crop inside a 4K sensor than
a head on a bearing, but V4L2 reports no mechanism state (that is PF:18's whole subject), so
the tool cannot tell and neither can this entry. The finding is precisely that it *cannot* —
the control vocabulary UVC gives us describes an effect, not an actuator.

**What it costs this tool, stated rather than fixed.** Design §5's motor rules — the sweep
caps, `--allow-motion`, `WCH_NO_MOTION=1` — are keyed on the slug, so they now apply to a
device that may have nothing to wear out. That is the right direction to be wrong in: the
cost of treating digital PTZ as mechanical is a needless flag and a bounded sweep, and the
cost of the reverse is driving somebody's gimbal into its end stop. No change is made, and
none should be made on the strength of a guess about a lens the tool cannot see.

It does cost `WCH_NO_MOTION=1` some of its meaning: AGENTS names it "for runs where the
camera points at a person", and on this device excluding the `hw_motion_` arm may exclude
nothing physical at all. The knob still does what it says — it excludes the suites that
*could* move a motor — and it cannot do better while the mechanism is unreadable.

**Not corpus-shaped as a *behaviour*, but the descriptors are:** the three controls, their
ranges and their flags are in `corpus/profiles/dell-u3224kb.json`, which is what makes "a
non-gimbal camera declares pan/tilt/zoom" a device answer rather than a claim. What no
profile can carry is the absence of a motor.

**Retires when:** a control, a UVC descriptor field or a kernel property distinguishes
digital windowing from mechanical PTZ — UVC's `CT_DIGITAL_WINDOW_CONTROL` is the obvious
candidate and `uvcvideo` exposes no V4L2 control for it on this kernel — at which point
design §5 could key its motor rules on the mechanism rather than on the name.

---

## N22 — A sample photo's path carries the sweep pass, because D9's name is unique only within one sweep

**Doc:** design D9 fixes the session layout, sample photos included:
`photos/<control-slug>/<value>.jpg|png`. D8 says `precision` is "the final sampling step …
so multi-pass refinement (coarse → fine) is representable", and
`engine::session::begin_sweep` is legal from `Calibrated` for exactly that reason, with a
test (`a_calibrated_control_can_be_swept_again_for_a_finer_pass`) whose comment reads "a
state machine that refused this would make the field a lie".

**Repo:** `photos/<control-slug>/<from>/<requested>.<ext>`, where `<from>` is the number of
samples the control already carried when the sweep began — the index this pass's first
sample takes in the control's history.

**Why.** P3c's naming rule is stated with its scope: a requested value is unique *within a
sweep*, because the planner deduplicates. A control's history is longer than one sweep.
`begin_sweep` resets `done` and leaves `samples`; `record_sample` appends; a refinement pass
refines *around* the coarse winner, so the two plans overlap by construction. Under D9's
literal name the second pass's `128.jpg` lands on the first pass's, and the first pass's
`Sample` — with its own metrics, its own `captured_at`, its own `applied` — keeps naming a
file that no longer holds the frame those numbers describe. The module header states the law
this breaks in as many words: *the frame that is scored is the frame that is stored.* It was
true within a sweep and false across two.

It is not a hypothetical shape. On real optics a refinement pass is run *because* something
changed — the scene, the lighting, an interacting control the operator has since
calibrated — so the two pictures at one value are genuinely different pictures, and the one
an agent opens to check a score is the wrong one. The deterministic fake hides it perfectly:
identical control values produce byte-identical frames, so nothing in `crates/engine/tests`
could see the overwrite in the bytes. The arm that pins it —
`a_refinement_pass_cannot_overwrite_the_frames_the_coarse_pass_scored` — therefore asserts
the *paths*, that no two samples of one control ever name one file, which is a claim the
fake cannot make true by accident.

**Why the sample count rather than a pass counter.** The number is already on the document,
so nothing new is persisted and no schema moves. It is monotone across passes that record
anything, and a pass that records *nothing* reuses its predecessor's directory — which is
correct rather than a hole: there is nothing on the record naming what is in it, and an
interrupted first sample now returns the control to `Untouched` anyway (N24).

**What this does not fix, stated rather than hidden.** The samples of two passes still live
in one `ControlSession`, so `select_by_metric` ranks the union and `sampled_precision`
computes a stride across two sampling grids. Both are defensible when the scene did not
change and neither is when it did, and telling those apart is a D8 question — a pass
boundary on the record — rather than a path question. Recorded here because the P3 review
raised it alongside this defect and it is not closed.

**Retires when:** D8 gives a pass its own identity on the document (a `passes` list, or a
`SweepRun` a `Sample` belongs to), at which point the path is derived from that identity
rather than from a count, and the ranking question above has somewhere to be answered.

---

## N23 — Restoring after a calibration is a verb, not a default, and it is session-scoped

**Doc:** design D4 — "Sweeps and guarded operations wrap themselves in snapshot/restore by
default; the tool leaves the camera as it found it unless told to keep changes (`--keep`)" —
design §5's "every sweep/guarded operation restores state by default", and AGENTS rule 8.
D10's settled method list has seven `calibrate_*` methods. N20 rules that `apply` does *not*
restore and does not consume the pre-sweep snapshot, and justifies both by pointing at
`lifecycle::recover`: "the same function an ordinary session end and a crash recovery both
run — is what spends it".

**Repo, before this note:** no process ran it. `grep -rn recover crates/cli crates/cli-core`
was empty; every caller of `lifecycle::recover` in the workspace was a test. `Session::
pre_snapshot` was written by the product and read only by tests, so `wch calibrate sweep`
exited 0 with the camera holding its last swept value and the record unspent forever — and
because `arm_pre_snapshot` is once per *session*, the next `calibrate start` on that camera
recorded the previous sweep's endpoint as "the camera as the operator found it". The
doctrine's anchor was a record of the tool's own residue. Confirmed by three independent
skeptics in the P3 review, two of them against attached hardware.

**Repo now:** an eighth verb, `wch calibrate restore <camera> --task|--session`, which runs
`lifecycle::recover` — restore in D4's automation-first order, then consume — and answers
with the same `RestoreReport` `wch restore` does. Running it twice is not an error: the
second time there is nothing left to put back and it says so. Every sweep that leaves a
snapshot armed prints, on standard error, the exact command that spends it.

**Why a verb and not a default, which is what D4 says.** Three settled facts point the same
way, and they are all about the snapshot being *session*-scoped:

1. `arm_pre_snapshot` takes it **once per session** and its doc says why — by the second
   sweep the first one's writes are on the device, and a snapshot taken then would restore
   the camera into the middle of a calibration nobody asked for. A per-sweep restore wants a
   per-sweep snapshot, which is a different design and a different crash story (§6).
2. N20 requires the record to survive `apply`: "that record describes the camera **before
   the calibration**, and it is the only route back to it; `apply` is when an operator is
   most likely to want that route." A sweep that consumed it would delete the route the
   moment the last sweep finished. A sweep that restored *without* consuming would restore
   twice and still leave the question of who spends it.
3. Design §5's "motors wear". A session that sweeps pan and then tilt would, under a
   sweep-scoped default, drive the pan head back to where it started and immediately leave
   it there while tilt sweeps — travel spent to reach a state no one observes. On the seed
   PTZ hardware the default D4 words describe is worse for the device than the verb.

So `--keep` has no producer, and that is the honest reading rather than an omission: the
restore is explicit, and declining it is not running the verb. What replaces "by default" is
"and it tells you" — the sweep names the command, because a default nobody is told about and
a verb nobody knows about fail the same way.

**What is not claimed.** The subprocess suite asserts the durable half — that the verb
exists, reports on every control the snapshot held, and *consumes* the record — because the
fake replays a profile into a fresh device per process and cannot show a value surviving
between two `wch` runs. That the camera physically goes back is the engine suites' claim and
the R3 rung's, and both already make it.

### What replaces the stderr line for a client that has no stderr, P4c

The sentence above — "the sweep names the command" — is printed by `wch` on standard error,
and a client on the other end of a socket cannot see standard error. That is the same shape
of gap note N30 wrote `DiscoveryReport` for, so it was checked when P4c routed
`wch_calibrate_sweep` rather than assumed. **The fact is already on the wire and needs
nothing added:** the sweep answers with the `Session` document itself, and
`Session::pre_snapshot` is a field on it that is present exactly when a snapshot is armed
and absent after `calibrate_restore` spends it. `crates/daemon/tests/calibrate_verbs.rs`
asserts both ends of that over a socket.

What is not on the wire is the *rendering*, which is right: a wire field a client reads and
a sentence a person reads are different things, and the sentence belongs to `wchc` (P4f),
built from the same field rather than from a second copy of the rule. Recorded here because
"a verb nobody knows about" is this note's own failure mode, and a client told nothing would
have been the daemon's instance of it.

**Retires when:** either the snapshot becomes sweep-scoped (at which point D4's sentence is
implementable as written and this verb becomes `--keep`'s inverse), or D8 grows the explicit
close/abandon verb N14 names in its own retirement clause — at which point ending a session
and giving the camera back are one act rather than two, and D10's method list settles at
eight for a different reason than this one.

---

## N24 — A sweep that recorded nothing puts the control back, because `Sweeping { done: 0 }` has no exit

**Doc:** design D8's per-control vocabulary runs `Untouched` → `AutoDisabled` → `Sweeping` →
`Calibrated | Deferred | Blocked`; no arrow goes backwards. N18 records that an interruption
leaves "a control left `Sweeping`, and a pre-sweep snapshot still armed", and says nothing
about what the operator does next.

**Repo:** `engine::session::abandon_sweep` returns a control to `Untouched`, and
`engine::calibrate::run`'s interruption path calls it when the sweep recorded **zero**
samples.

**Why.** Leaving a control mid-sweep is right when samples were taken — they happened, and
`select` is the documented way out. With zero samples every exit was closed, and the P3
review walked all of them: `may_begin_sweep` refuses `Sweeping`, so no re-sweep;
`selectable` refuses `no_samples`; `lifecycle::draft` skips anything that is not `Untouched`,
so `plan` is a no-op; `reorder_queue` requires a strict permutation, so the control cannot be
dropped from the queue; no shipped verb produces `Deferred` or `Blocked`; and `Sweeping` is
never terminal, so `is_open` stays true and `calibrate start` refuses that (camera, task)
slot forever. A transient *availability* failure at sample 1 — an unplug, a `SettleTimeout`
\[PF:11\], `ENOSPC` on the first photo write, a `FormatUnsupported` from the first
`start_stream` — was converted into a permanent *capability* refusal for that control. That
is the one conversion AGENTS rule 7 and rubric A4 exist to forbid, applied one layer up from
where they usually watch for it.

The backwards arrow is the smallest correct answer: nothing was recorded, so nothing needs
recording. The alternative — permitting `begin_sweep` from `Sweeping { done: 0 }` — leaves
the session unsettleable, so the (camera, task) slot stays refused even after the control is
calibrated. `abandon_sweep` refuses from every other state, samples included, so it cannot
be the way somebody's work is thrown away.

**What still holds.** The attempt is not erased: `SweepInterrupted` is appended by the same
path (N18), so `calibrate status` still says a sweep was tried and why it stopped. And the
commit is best-effort for N18's reason — a store that cannot take it must not answer "the
disk is full" to somebody whose camera was pulled out.

**Retires when:** D8 gains a durable "this sweep is running, owned by that process" fact —
N18's own retirement trigger. Silence would then be readable, and a zero-sample interruption
would be a recorded state rather than an unreachable one.

---

## E6 — The P3 adversarial review, 2026-08-09

docs/8 Part E asks for a review pass at each phase boundary; P1's is in E1's amendments and
P2's is E4. This is P3's, over the five commits `361bcd8..856170a`. Six independent lenses
raised **31 candidates**, each attacked by three skeptics instructed to refute it. **Twelve
survived**, and they dedupe to **nine distinct defects** — two pairs and one triple were the
same defect seen from different lenses. All nine are fixed in the commit carrying this
entry, each with the test or gate row that turns red without it.

**The one that mattered most, and it is a doctrine failure rather than a bug.** Design D4,
design §5 and AGENTS rule 8 all say a sweep leaves the camera as it found it.
`lifecycle::sweep_write` persists the pre-sweep snapshot before the first write for exactly
that reason — and **nothing a user could type ever spent it**. Every caller of
`lifecycle::recover` in the workspace was a test, so `Session::pre_snapshot` was written by
the product and read only by the suite: rubric A8's "a typed declaration nothing reads",
with a hardware consequence. A skeptic reproduced it on `/dev/video0` (brightness left at
220, operator's 128 unrecovered) and on the OBSBOT (head parked at the last swept pan), and
showed the second half too: because `arm_pre_snapshot` is once per *session*, the next
`calibrate start` recorded the previous sweep's endpoint as "the camera as the operator
found it". The doctrine's anchor had become a record of the tool's own residue. N20 states
as settled fact that `recover` is "the same function an ordinary session end and a crash
recovery both run" — a caller that did not exist; the note was not justifying the gap, it
was assuming it closed. Fixed as an eighth verb (N23), driven against both cameras below.

**Two the fixture choice hid, and they are the review's methodological lesson.**

- `uncalibratable` asks the sweep planner with `allow_motion = true`, and N19 states the
  reason as law: motion is a reason a *sweep* needs a flag, not a reason a control cannot be
  calibrated. Flipping it to `false` left the entire workspace green — 618 tests, zero
  failures — because the one test that drafts a whole device drafts the motor-less Chicony,
  while `obsbot-tiny3` sat in the corpus, loaded by two other tests in the same file. The
  rule that lets this tool calibrate a PTZ camera at all was pinned by nothing.
- A control's sample photos were named `photos/<control>/<value>`, unique within one sweep
  and not across a control's history. D8's `precision` exists so a coarse pass can be
  followed by a fine one; the fine pass refines *around* the coarse winner, so the two plans
  overlap and the second pass's frames land on the first's while the first's samples still
  name them. Invisible on the fake, which produces byte-identical frames at identical
  control values — the test that catches it asserts the *paths* (N22).

**Two gates green while checking less than they claimed** — note N10's family, for the third
and fourth time. `json-validates.sh` derived its verb population from top-level `--help`
only and matched rows by prefix, so a single `calibrate-start` row satisfied the whole
seven-verb subtree: deleting the other six left `PASS json-validates`, exit 0, six fewer
documents validated. `atomic-write-home.sh`'s raw-write pattern omitted `File::options(` and
`File::create_new(`, std's own aliases for two primitives it did catch, so two byte-identical
bypasses got opposite verdicts on how the open was spelled. Both criteria stated the
stronger guarantee in the same words the predicate did not have.

**Two conversions of the kind the rubric watches for, one layer above where it watches.** A
sweep interrupted before its *first* sample left the control in `Sweeping { done: 0 }`, a
state every shipped verb refuses — so an unplug, a settle timeout \[PF:11\] or `ENOSPC` on
the first photo became a permanent capability refusal for that control and a (camera, task)
slot that never settles (N24). And `calibrate plan`/`sweep` accepted `--session <uuid>` for a
camera that was not the session's: the D8 fingerprint law was implemented in `apply` alone,
so a sweep could drive camera B, record the samples in camera A's document, and let `apply`
write them to A with its own check green — and because `arm_pre_snapshot` short-circuits on
A's snapshot, B was moved with **no record of it anywhere on disk**.

**Two graded LOW, both structural rather than observable, both now closed by construction.**
`ChosenByArg::ALL` was a hand-written array driving the `--by` parser, so a variant the
compiler forced you to *map* was never forced into the parser's vocabulary; it is generated
by `closed_vocabulary!` now. And the four mutating calibrate verbs read `session.json`
immediately *before* `store.with_lock` and committed a draft cloned from that pre-lock read,
so only the write half of a read-modify-write was protected — a skeptic hammered it into 9
lost updates in 300 rounds of two concurrent `calibrate plan` processes, both exiting 0. The
read now takes a `&StoreLock` it does not use: the token is proof, and a caller that has not
taken the lock cannot perform the read at all.

**The deferral P3d left and P3e dropped, ruled on.** P3d's commit said
`calibrate::applied_value`'s two producerless refusals were "P3e's call"; P3e made none — no
N-entry, no §3.3 register row, no test. The call is: **they are exercised, not deleted and
not registered.** They are the contract check on a value that arrives from outside the engine
(the T2 seam has two implementations today and a third at P4), and a sweep that took `writes`
on faith would label a sample with a number nobody measured. Reaching them *through* a
backend would mean teaching a double to violate T2, which makes the double a worse model of a
device — so they are driven where they are decidable: `applied_value` is a pure function of a
write report, and the unit test
`a_report_that_does_not_name_the_control_or_answers_with_a_payload_is_refused` hands it
both malformed reports directly, with the conforming twin beside them.

**What the review did not find, which is worth as much.** No unsound `unsafe` and nothing new
in the mmap path (P3 added none). No state write outside D9's home, and no path around
`write_json_atomic` or the fd-lock — the store's own discipline held under six lenses. No
fault-menu variant without a driven inverse. No place where the D8 state machine's refusals
could be reached around, and no auto-selection: "metrics rank, they do not decide" survived
every attempt. No availability error reshaped into a capability answer *at the error layer* —
the one conversion found was in a state machine above it. Nineteen candidates were refuted,
several by skeptics that built the reviewer's exact scenario and ran it.

**Where the review did not look**, recorded so the next one starts there: `engine::progress`
(274 new lines, no lens), `cli-core::render` (777 new lines, 21 tests, none mutation-checked),
`photo.rs`'s `controls_in_effect`/`grab`/`from_capture` split as a PF:16 byte-fidelity path,
`crates/backends/v4l2/tests/vivid.rs`'s sweep arm, `engine/tests/session_lifecycle.rs`, the
three dependencies P3 adopted (`fd-lock`, `tempfile`, `indicatif`) against rubric B9, and the
new session/progress DTOs as serde contracts. The seeded-defect counts in the five commit
messages were not reproduced by anybody; finding 6 above is a survivor those campaigns missed,
which is a data point about how complete they were.

### The fixes against hardware, 2026-08-09

Same host and cameras as E5, plus the Dell: kernel 7.0.0-29-generic, Chicony `04f2:b83c`
(`/dev/video0`), OBSBOT Tiny 3 `3564:ff02` (`/dev/video4`), Dell U3224KB/A `413c:c03d`
(`/dev/video6`).

```
$ just smoke-hw
smoke-hw: motor-moving suites (hw_motion_*) are included — set WCH_NO_MOTION=1 to exclude them
smoke-hw: 10 capture node(s) present; running test(/(^|::)hw_/)
     Summary [  52.835s] 15 tests run: 15 passed, 633 skipped
smoke-hw: 8 claim(s) declined by tests that ran — each named above
```

The opt-out still excludes the motion arm as a named, counted skip and nothing else:

```
$ WCH_NO_MOTION=1 just smoke-hw
smoke-hw: SKIP 1 — motor-moving suites (hw_motion_*) are excluded by WCH_NO_MOTION=1; unset it to include them
smoke-hw: 10 capture node(s) present; running test(/(^|::)hw_/) - test(/(^|::)hw_motion_/)
     Summary [  37.432s] 14 tests run: 14 passed, 634 skipped
```

The eighth verb, driven against the two devices the finding named. Chicony, brightness:

```
$ wch get cam:integrated-camera-integrated-c brightness      -> 128
$ wch calibrate start/plan cam:… --task p3review brightness
$ wch calibrate sweep cam:… --task p3review brightness --values 30,150,220
    brightness  sweeping 3/3
    note: this sweep borrowed the camera and it still holds what the sweep left;
          `calibrate restore cam:integrated-camera-integrated-c --session 019fe650-…` puts it back
$ wch get cam:… brightness                                   -> 220   <- the defect, reproduced
$ wch calibrate restore cam:… --task p3review
    brightness                  restored to 128
    exposure_time_absolute      back under auto_exposure, as it was [PF:3]
    white_balance_temperature   back under white_balance_automatic, as it was [PF:3]
    (12 further controls already correct)
$ wch get cam:… brightness                                   -> 128
$ wch calibrate restore cam:… --task p3review                -> exit 0, 0 outcomes
    note: this session carries no unconsumed pre-sweep snapshot; the camera was not written to
```

OBSBOT, `pan_absolute` — the motor case, bounded to three steps and run with the motors-on
default:

```
$ wch get cam:obsbot-… pan_absolute                          -> 7200
$ wch calibrate sweep cam:obsbot-… --task p3ptz pan_absolute --values -3600,0,3600 --allow-motion
    pan_absolute  sweeping 3/3
$ wch get cam:obsbot-… pan_absolute                          -> 3600  <- the head, parked
$ wch calibrate restore cam:obsbot-… --task p3ptz            -> pan_absolute restored to 7200
$ wch get cam:obsbot-… pan_absolute                          -> 7200
```

\[PF:18\] bounds what that last line means, unchanged: the read-back is the *commanded*
position, and no control on this device reports mechanism state.

**Every camera was left as it was found and it is asserted rather than assumed.** A
`wch snapshot` of all four enumerated cameras was taken before the runs and again after
them, normalised to `{control, value}` pairs and diffed: identical on all four
(Chicony RGB 15 controls, Chicony IR 2, OBSBOT 22, Dell 17), `pan_absolute` at its as-found
7200 included. No camera frame entered the repository; the session trees and the snapshots
live in a scratch directory outside it.

**Still open after this pass**, recorded rather than left implicit: the two passes of a
refinement still share one `ControlSession`, so `select_by_metric` ranks their union and
`sampled_precision` strides across two grids (N22 says so and says why it is a D8 question);
`--keep` has no producer because the restore is a verb rather than a default (N23); and the
coverage gaps listed above are the next review's starting point rather than this one's work.

---

## N25 — Five accepted mutants in the planners are equivalent, not uncovered

**Doc:** rubric rule 2 — for every test, the buggy implementation — and note N12, which
records lines this suite cannot turn red. docs/7 P3f asks every surviving mutant to become
a test or a **reasoned** acceptance, and N15 is the standing warning that "no test can kill
this" is usually a claim about the fault you thought to inject.

**Repo:** five survivors of the first mutation-floor run (E7) are accepted, and all five
are accepted on the *strongest* available ground: no input distinguishes the two programs.
That is a different claim from N12's — N12's `fsync`s make a real difference nothing in a
hermetic test can observe; these make no difference at all.

Four of them are one story: **`sweep::strided` and `sweep::subsample` are written total for
a `limit` of zero, and their only callers pass 256 or 32** (`limits::MAX_SWEEP_SAMPLES`,
`limits::MAX_MOTION_SWEEP_SAMPLES`). Every guard that exists for `limit == 0` is therefore
unreachable, and mutating it changes nothing:

| Mutant | Why it is equivalent |
|---|---|
| `replace && with \|\| in strided` | `capped \|\| limit_count > 0` is always true, and when `capped` is false the factor comes out as exactly 1 — `requested_count` is at least 1, so `div_euclid` is 0 and `rem_euclid` is not, or the two are equal and `div_euclid` is 1. A stride multiplied by 1 is the stride. |
| `replace > with >= in strided` (the `limit_count > 0` guard) | `limit_count` is 256 or 32. Both spellings are true. |
| `replace < with <= in strided` (the `values.len() < ceiling` guard) | The stride widening already bounds the count: `factor = ceil(requested / limit)` gives `limit × factor ≥ requested > span/stride`, so `floor(span / (stride × factor)) + 1 ≤ limit`. The loop always runs out of range before it runs out of ceiling, so the guard never binds. |
| `replace \|\| with && in subsample` | `count <= limit \|\| limit == 0`: with `limit` positive the second disjunct is dead, and when `count <= limit` the loop the mutant falls into re-picks every element in order and `dedup` puts the list back exactly as it was. |

The fifth is the same shape one function along: **`replace > with >= in precision_of`**, and
its twin **`replace > with >= in sampled_precision`** in `engine::session`. Both filter
gaps to the positive ones, and in both the input has already been made duplicate-free —
`session::sampled_precision` sorts and `dedup`s its own input; every `SweepPlan` reaches
`precision_of` with distinct values (`strided` is strictly increasing, `log_spaced`
`dedup`s, `explicit` inserts through a `BTreeSet` and `subsample` `dedup`s). A gap between
two distinct integers is at least 1, so `gap > 0` and `gap >= 0` accept the same set.

**The two structural claims are checked, not asserted.**
`a_strided_plan_is_bounded_by_its_stride_and_never_by_the_ceiling` plans 1 764 sweeps —
seven minima from -468 000 to 4 096, fourteen spans from 0 to 936 000, six device steps,
three specs — and asserts of each that it holds exactly as many values as its own recorded
stride implies (which a truncating ceiling makes impossible) and that no value repeats
(which is what makes the positive-gap filters unreachable). If a future change makes either
claim false, that test goes red before the acceptances do.

**What makes these acceptances rather than gaps** is that the *lines* are covered and only
the unreachable *edges* are not. The same run caught `replace > with ==` and
`replace > with <` on `sampled_precision`'s filter — both make it reject everything, the
recorded precision collapses to zero, and
`precision_comes_from_what_the_camera_held_not_from_the_plan` turns red. It caught
`replace > with >=` on `strided`'s *cap* comparison one line above the guard accepted here,
because `a_sweep_of_exactly_the_cap_is_not_a_capped_sweep` was written for it. The
acceptance register keys survivors by file and description rather than by line, so it
compares **multisets**: two mutants sharing a key need two lines, and one of a pair
regressing turns the job red rather than hiding behind its sibling's entry.

**The alternative considered, and declined.** Making `limit` a `NonZeroU32` would delete
three of these guards and the mutants with them, and deduplicating inside `precision_of`
would delete the other two. Both are changes to shipped code made to satisfy a measurement,
and both remove a total function's defence against a future caller. This repository has the
opposite precedent only where the redundancy was a *second representation of a fact*
(`store::parse_log` retired a flag that could not change an outcome); a guard against a
precondition is not that.

**Retires when:** a caller passes `limit == 0`, or a planner stops deduplicating its output
— at which point these become ordinary uncovered lines and the register's both-direction
check turns `just mutants` red until the entries are deleted and the tests written.

---

## N26 — `sharpness` cannot see its own mean, because a Laplacian response sums to zero

**Doc:** as N25. This is the third kind of accepted survivor: not a guard on an unreachable
input, but arithmetic whose operand is always the identity.

**Repo:** `imaging::metrics::sharpness` is the variance of a 3×3 Laplacian response — it
takes the mean of the response and then the mean of the squared deviations from it. The
mutation floor (E7) reports three survivors inside that calculation:
`sum / n` → `sum * n`, `sum / n` → `sum % n`, and `value - mean` → `value + mean`.

**Why all three are equivalent: the response sums to exactly zero, always.** The Laplacian
kernel sums to zero, and `imageproc`'s border replication distributes the taps so that the
total weight on every input pixel — corner and edge included — is zero as well. So
`sum == 0`, and therefore `0 / n`, `0 * n` and `0 % n` are the same number, and adding zero
is the same as subtracting it.

**This is a claim about a dependency, so it is a test rather than a paragraph.**
`the_laplacian_response_sums_to_zero_whatever_the_image_is` asserts it over the population
that would break it if anything could: a single bright pixel at each of the nine positions
of a 3×3, which asks the border replication the question from every direction, plus the
four fixtures the rest of the module measures. An `imageproc` release that changes the
border handling turns *that* test red, and these three mutants become killable the same
day — at which point the register's both-direction check turns `just mutants` red until
their entries are deleted.

**What is covered, and it is the neighbouring line.** The fourth mutation of the same
calculation — dividing the sum of squares by the sample count the wrong way round — is
*not* accepted: it survives every ordering test in the module (scaling a metric by n²
reorders nothing), and `sharpness_is_the_variance_of_the_laplacian_and_not_some_neighbouring_moment`
was written for it, stating the statistic independently rather than asserting a rank.

**Retires when:** the response stops summing to zero — a different filter, a different
border rule, a different library.

---

## N27 — The store lock's `WouldBlock` arm is the only `flock` failure a test can arrange

**Doc:** AGENTS rule 7 — EBUSY/ENODEV/EPERM/timeout stay distinct from "the camera can't",
and no code or test converts one into the other. `StoreLock::acquire` applies the same rule
to the filesystem: a lock somebody else holds is `Error::StoreLocked`, and any *other*
failure of the lock call is `Error::StorageIo`.

**Repo:** the mutation floor (E7) reports
`replace match guard err.kind() == io::ErrorKind::WouldBlock with true` as a survivor. With
it, every `flock` failure becomes "somebody else holds it" — an availability answer given
for an availability failure of a different kind, which is a smaller error than the ones
that rule usually catches but the same shape.

**Why it survives, and why that is an acceptance rather than a gap.** The arm below it
needs `try_write` to fail with something other than `EWOULDBLOCK`, and a hermetic test
cannot arrange one. `EBADF` needs a descriptor this code owns and has closed; `EINTR` needs
a signal delivered inside the syscall; `ENOLCK` needs the kernel out of lock records; a
filesystem that does not implement `flock` at all needs a mount this suite may not make.
The store's fault menu deliberately arranges the *real* thing for the case it can — a
second `flock` from an independent open file description — and that is `EWOULDBLOCK` by
construction, so the seam that exists cannot produce the fault this line distinguishes.

This is N12's family rather than N25's: the two programs really do differ, and the
difference is invisible because the input that reveals it cannot be produced here.

**What was declined.** A fifth `StoreFault` variant could script a non-`WouldBlock` lock
failure. It is not added, for the reason design §2.9 gives the fault menus generally and E5
gives the fake: a menu entry for a failure nobody has observed is a fake capability, and the
right time to add it is the day a filesystem is seen doing it — as a fault variant with a
fixture, the way a PF entry lands.

**Retires when:** a real filesystem is observed answering `flock` with anything but
`EWOULDBLOCK` here, or the lock acquisition grows a seam for its own syscall.


---

## E7 — The mutation floor's first run, 2026-08-09

docs/7 P3f commissioned a `cargo-mutants`-class job over the pure cores "before G4, not
after", and said the triage rather than the wiring would be the work. It was. This entry
is the first run's record: what the job is, what it cost, every survivor and what became
of it, and the two claims from E6's coverage gaps that it was pointed at.

### The job

`just mutants` runs `scripts/mutants.sh` over `cargo-mutants` 27.1.0. The scope is
`.cargo/mutants.toml` — six files, transcribed from docs/7's sentence: the guarded-write
planner (`engine::pairing`), the sweep planner (`engine::sweep`), the calibration state
machine (`engine::session`), the settle policy (`engine::settle`), the session store
(`engine::store`) and the D8 metric set (`imaging::metrics`). Judgement is the **whole
workspace suite** (`test_workspace = true`), which is AGENTS rule 2's "mutations verify at
workspace scope" rather than a choice: at 643 tests in about three seconds the
honest option is also the cheap one, and a mutant that only its own crate's unit tests
catch is a weaker result than this project claims.

Survivors are compared against `scripts/mutants-accepted.txt` in both directions. An
unlisted survivor fails the job; so does a listed one that has stopped surviving, because
the register would otherwise be exactly the thing N15 warns about — an acceptance nobody
re-checks.

**One correctness hazard, recorded because it costs a day to diagnose.** The build
directories must not share `target/` with the checkout. A test binary bakes in
`CARGO_MANIFEST_DIR`, this repo's corpus loader walks that path's ancestors looking for
`corpus/profiles` (`crates/testkit/src/corpus.rs`), and a binary compiled in a scratch tree
and cached into ours fails every corpus-loading test with "no ancestor directory contains
corpus/profiles" — which looks exactly like a real defect and is not. `copy_target = false`
is the guard; the cure for a cache already poisoned is `cargo clean -p <pkg>`, never a code
change.

### What it cost

**410 mutants in 21 minutes and 17 seconds** of wall clock, on an eight-core machine: five
parallel jobs (what the build root could hold, which the script works out and says), each
one rebuilding the workspace and running all 643 tests. Per mutant that is roughly two to
six seconds of build and nine to fourteen of test — the build is incremental, the test run
is not, and both are paid 410 times. `just ci` on the same machine is minutes. That ratio
is the whole argument for the posture docs/9 records: a rung and a phase-close criterion,
never a CI step.

Three operational findings sit behind that number, and each is in `scripts/mutants.sh`
rather than only here:

- **Debug info off.** `just ci`'s own `target/` on this machine is 34 GiB, almost all of it
  DWARF for the workspace's test binaries, and each job gets a whole copy. The first
  attempt filled a 16 GiB `tmpfs` `/tmp` and had to be abandoned — one build directory had
  reached 6.1 GiB on its own. `CARGO_PROFILE_{DEV,TEST}_DEBUG=0` brings a build directory
  to about 1.5 GiB and makes the links, which are most of the wall clock, much cheaper. It
  cannot change a verdict: it changes what a backtrace can say, not what a test asserts.
- **Build directories on `tmpfs`, not on the disk that holds `target/`.** Measured both
  ways on the same tree: about seven mutants a minute in `$TMPDIR`, under one a minute on
  disk. Concurrent cargo builds are I/O bound long before they are CPU bound.
- **The job count is what the build root can hold, not what the machine has cores.** The
  script does the `df` and says so — on this machine, five jobs rather than eight.

### What it found

**410 mutants generated: 350 caught, 50 unviable, 10 survivors — and every one of the ten
is a recorded acceptance.** `just mutants` exits 0 on that. The 50 unviable are evidence
about the type system rather than about the tests: the mutation did not compile.

One test landed after that run's build directories were copied
(`a_strided_plan_is_bounded_by_its_stride_and_never_by_the_ceiling`, which is evidence for
N25 rather than a killer), so the register was re-checked against the final tree on its own:
`just mutants -F <the ten accepted, by description>` selects fifteen mutants — the ten and
the five that share a key with one of them — and answers *ten missed, five caught*, which
is the register exactly. The five same-key siblings being caught is the point of comparing
multisets rather than sets.

That is the tree *after* the triage, which is the work docs/7 predicted. Across this
session's passes the floor surfaced **forty-eight distinct survivors**: seven in
`pairing` (four, then three more once the first four were killed and the fixtures that
had been covering two rules at once stopped), four in `session`, one in `settle`, eight in
`store`, twenty-three in `sweep` and five in `metrics`. **Thirty-eight became tests and
ten became recorded acceptances.**

**Thirty-eight became tests.** Seventeen new tests and four extended ones, each watched red
on its mutant and green on the clean tree before it was kept. The ones worth naming, because
they are defects rather than thin coverage:

- **An availability failure reported as "nothing here", twice, in the store.** `load_log`
  and `read_dir_or_empty` each return "empty" for exactly one errno — a file or directory
  that is *absent*. Widen either guard to every failure and a state directory that exists
  and cannot be read becomes "this session has done nothing" and "this camera has never
  been calibrated". The second is the worse one: `calibrate start` would then open a second
  session beside the one it could not see, which is what `SessionConflict` exists to
  prevent (N14). Both are now driven by a real kernel refusal rather than a scripted one —
  `log.ndjson` as a directory is `EISDIR`, `sessions/` as a file is `ENOTDIR`, neither
  needing a privilege or a `chmod` (the trick N15 uses, for the same reason).
- **A fault fixture that could stop producing its fault.** `StoreFault::TornLogLine`
  truncates the line to half its length; truncating it to `len % 2` — nothing, or a single
  `{` — leaves every assertion in `a_torn_line_is_dropped_or_refused_by_where_it_is` green,
  because through `load_log` an entry that was never written and an entry that was written
  torn and dropped are the same answer. The test now reads the bytes and asserts the tear
  is the first half of the entry. A fixture that has quietly stopped producing the condition
  it is named for is "skip reads as pass" in a fault-menu costume.
- **The lock record could name nobody, and every message would still read as though it
  did.** Inverting one `!` makes `comm` `None` on every record; the suite could not tell
  "somebody holds the lock" from "wch (pid 1234) holds it", which is the entire reason the
  record exists. And `-1` — the pid fallback that must never name a real process — could
  become `1`. That one needed a change to shipped code to become testable: the conversion
  is now `LockRecord::pid_or_unknown(raw: u32)`, a value rather than a call, because a
  process cannot choose its own pid. It is the engine's own rule (pure cores take values)
  applied to one line, and it is the only product change this session made.
- **The whole capping arithmetic of the sweep planner was unpinned.** Nine mutations of
  `strided` — the sample count off by one either way, the rounding that chooses the widening
  factor, the factor itself, the boundary at which the cap fires — survived a suite that
  asserted the cap's *shape* (under the limit, still spanning the range) and never its
  answer. Five more in `log_spaced`: the shift that moves a zero-crossing range up to 1
  before taking logarithms could be inverted, negated, applied to the wrong end, or the
  ratio inverted, and every one of those still produces something increasing that starts at
  the minimum. What pins them is the answer stated exactly: 10 001 values at step 1 under a
  cap of 256 is a stride of 40 and 251 samples, and a five-point log sweep of [100, 10 000]
  is `[100, 316, 1000, 3162, 10000]`.
- **Three "was it trimmed?" comparisons that reported a trim that never happened.** An
  explicit list that fits was one mutation away from telling every caller it had been
  capped, and `note_dedup`'s `dropped` count was a subtraction that a division agreed with
  on the only fixture that reached it (4 → 3 is one either way; 5 → 2 is three or two).
- **A stuck clock.** `MonotonicClock::now_ms` replaced by `0` left the whole workspace
  green: the only test of it asserted that it does not run *backwards*, which any constant
  satisfies. A settle against a stuck clock never expires, so PF:11's wedged driver would
  hold an actor thread forever instead of raising `SettleTimeout`. It is now compared
  against an independent `Instant`, spun on rather than slept on (N3 bans `sleep`).
- **The guarded-write planner's two bounds and both of its suggestion rules.** A chain of
  automation controls exactly at `MAX_GUARD_DEPTH` had nothing asserting it plans — only a
  cycle, which is refused at any bound. The switch-off loop's one round of slack past "one
  round per partner" had no fixture that needed it, and the fixture that does is the shape
  the loop's own comment describes: clearing one partner puts another back. And
  `suggestions`' two rules — substring containment, and a shared prefix of four or more —
  were both reachable by the one fixture that tested either, so either could be deleted.
- **`ControlStatus::AutoDisabled`'s list could be empty or hold only the last partner**,
  and D4 restores from that list. Two mutations, one test, and the test is the first thing
  in the suite to assert the list's *contents*.
- **A tie under a lower-is-better metric.** "Ties keep the earliest sample" was asserted for
  the higher-is-better comparison only; the second comparison, three lines away, was pinned
  by nothing.

**Ten became recorded acceptances**, in three families and none of them for convenience:
five equivalent mutants in the planners (N25), three in `sharpness` where the operand is
always zero (N26), and one lock-acquisition guard whose distinguishing fault no hermetic
test can inject (N27). N26's argument is a claim about a dependency, so it is carried by a
test that turns red the day it stops being true.

### The G3 headline criterion, mutated at the fixture instead of the code

E6 recorded that nobody had ever mutated the fake's peak and watched G3's headline test go
red. Done here, in both of the two places the claim rests on, at workspace scope.

**The fake's optimum moved.** `fake::frames::focus_optimum` is `default.clamp(min, max)`;
replaced with `(default / 2).clamp(min, max)`, the synthetic camera's frames become
sharpest at 256 where the committed fixture declares 512. **Seven tests go red** out of
625, and the headline one is among them by value rather than by accident:

```
FAIL webcam-handler-engine::sweep a_scripted_session_calibrates_focus_at_the_optimum_the_fixture_declares
  left:  Selected { control: focus_absolute, value: 256, selector: Metric { Sharpness } }
  right: Selected { control: focus_absolute, value: 512, selector: Metric { Sharpness } }
FAIL webcam-handler-engine::sweep the_optimum_wins_and_every_other_sample_loses
FAIL webcam-handler-fake::resemblance frames_are_sharpest_at_the_focus_optimum_stated_by_the_profile
FAIL webcam-handler-fake::resemblance the_focus_optimum_is_the_profiles_declared_default
FAIL webcam-handler-fake frames::tests::sharpness_peaks_at_the_optimum_and_falls_off_either_side
FAIL webcam-handler-fake frames::tests::blur_grows_with_distance_from_the_optimum_in_both_directions
FAIL webcam-handler-fake frames::tests::the_optimum_is_the_declared_default_and_nothing_is_blurred_there
```

So the physics is validated in the direction that matters: a wrong optimum fails, and it
fails at the *selection*, not merely at the fake's own unit tests.

**The fixture's declaration moved.** Editing `focus_absolute`'s `default` from 512 to 600
in the committed `crates/testkit/fixtures/synthetic-basic.json` turns **two** tests red —
`the_fixture_declares_the_optimum_this_suite_states` and testkit's
`the_committed_document_matches_the_constructor` — and leaves the sweep assertions green.
That is not a gap, it is the design working: the fake reads the fixture's default, so both
sides of the physics move together and only the anchor notices. The suite's own header says
so ("edit either side and this fails before any sweep runs"), and this is the run that
checks it rather than believing it. It also bounds what the sweep tests prove: they pin
*that the sweep finds the fake's peak*, and the anchor is the only thing pinning *where
the fake's peak is*.

### The seeded-defect counts in the P3 commit messages

E6 recorded that nobody had reproduced them. This job is the right instrument for exactly
one of the four, and saying which is more useful than producing a number for all of them.

**P3a's claim is the one in scope, and it splits in two.** Its commit message says
"twenty-one buggy implementations were seeded at workspace scope; nineteen were caught",
and names the two survivors as `temp.as_file().sync_all()` and the parent `fsync_dir` —
the pair note N12 records.

- **The survivor half reproduces exactly.** Deleting `fsync_dir(dir)` from
  `write_json_atomic_scripted` leaves **642 tests run, 642 passed**. Deleting the temp
  file's `sync_all()` leaves **642 tests run, 642 passed**. Both were re-run here, at
  workspace scope, on the current tree. N12 stands, independently reproduced for the first
  time since it was written.
- **The completeness half does not.** The same module the campaign was seeded against
  yielded eight survivors to the tool, seven of which are ordinary uncovered lines with
  ordinary tests now written for them — including two conversions of an availability
  failure into a capability answer (`load_log` and `read_dir_or_empty` reporting *no
  history* for a file or directory that exists and cannot be read) and a fault fixture that
  could stop tearing the log line it exists to tear while every assertion around it stayed
  green. Twenty-one hand-seeded defects is a campaign, not a census, and this is what the
  difference looks like.

**The tool cannot express N12's own mutant, and that is worth recording.** cargo-mutants
replaces function bodies and flips operators; it does not delete a *statement*. So
`replace fsync_dir -> Result<()> with Ok(())` is generated and **caught** — by
`fsyncing_a_directory_is_supported_here_and_its_failure_is_typed`, which asserts the typed
failure — while the mutation N12 is actually about, deleting the *call* from
`write_json_atomic`, is not in the tool's vocabulary at all. A green mutation run over
`store.rs` is therefore not a re-confirmation of N12; the two paragraphs above are.

**P3b, P3c and P3d are out of this job's reach**, and the honest answer is that this
session neither confirms nor refutes them. Their seventeen, ten and fourteen seeded
defects were aimed at `engine::lifecycle`, `engine::calibrate`/`engine::progress`, and
`cli-core`'s calibrate verbs — none of which is in the floor's scope
(`.cargo/mutants.toml` says why the imperative shell is out, and says it is a deferral).
Widening the floor to `engine::lifecycle` is the single highest-value next probe: it is
where E6 found P3's largest defect, and it is where P3b's count was claimed.

### What this run does not establish

- **Nothing outside the six files.** The imperative shell — `lifecycle`, `calibrate`,
  `discover`, `capture`, `photo`, `snapshot`, `write` — the CLI renderers, the daemon and
  the V4L2 edge are all unmeasured by it. `.cargo/mutants.toml` says why each is out; the
  shell exclusion is a deferral, and it is the one worth revisiting first.
- **Nothing about mutants the tool does not generate.** cargo-mutants replaces function
  bodies with default values and flips binary operators. A defect of *omission* — a
  missing arm, an unwritten field, a call that should exist and does not — is not in its
  vocabulary, and the seeded-defect campaigns each sub-milestone runs are not made
  redundant by it.
- **Nothing about unviable mutants.** A mutant that does not compile is not evidence about
  the tests; it is evidence that the type system already refused it.

---

## N28 — The T5 trait and its method inventory are one macro declaration, because a Rust trait does not reify its methods

**Doc:** docs/9's method-count-walk row states the constraint and its consequence — "a Rust
trait does not reify its methods, so 'exhaustive match' is the wrong mechanism and this row
says the real one", the real one being the registered `RpcModule`'s `method_names()`. docs/8
Part C repeats it. Neither says what an *emitter* should do, and the plain reading leaves
xtask holding a table of method names beside the trait it describes.

**Repo:** `crates/api/src/wire.rs` declares `wire_surface!`, a `macro_rules!` that takes the
T5 methods once and emits both halves — the `#[rpc(server, client, namespace = "wch")]`
trait the daemon implements and `wchc` consumes, and `pub const METHODS: &[wire::Method]`,
the population xtask walks to write `schemas/webcam-handler-openrpc.json`. `lib.rs`'s trait
*is* that macro's argument. A method cannot reach one half and miss the other, because there
is nowhere to write it twice.

**Why:** `method_names()` is authoritative about names and about nothing else. An OpenRPC
document needs each method's summary, its whole doc comment including the `# Errors` section,
its parameter names in signature order, and the Rust types of its parameters and its result —
so an emitter resting on `method_names()` alone would still carry a second table for all of
that, which is rubric rule 6's banned hand list in a smaller costume. Generating the trait
*removes* the second table rather than checking it. The shape is not new here:
`closed_vocabulary!` already emits an enum and its `ALL` from one source for the same reason,
and says so in as many words ("A `const ALL: &[Self]` written next to an enum *is* a hand
list").

**What the macro does not own is checked as two.** The namespace separator belongs to
jsonrpsee's proc macro, not to us (`jsonrpsee-proc-macros-0.26.0/src/rpc_macro.rs` defaults
it to `_`), so `METHODS` spells a wire name `concat!("wch", "_", <name>)` — our belief —
while the registration spells it jsonrpsee's way. That is the one fact about this surface
genuinely derived twice, and `the_inventory_and_the_registration_describe_the_same_surface`
compares the two off a real `RpcModule` built by `into_rpc()`. Watched red by changing the
separator to `"."`: it fails naming all nineteen, and it is the **only** thing that notices —
the pinned-spelling test reads `method_names()`, so it stays green.

**What this does not pre-empt.** docs/9's row remains P4c's. Its subject is a registered
*daemon* module compared against the integration-test inventory — "a wire method with no
test" — a different claim over a different population from "the emitted document describes
the trait".

**Retires when:** jsonrpsee grows an inventory of its own — a generated method table, or a
macro that emits its registration as data — at which point this layer wraps something
upstream already provides.

---

## N29 — The T5 trait lands at nineteen methods, not twenty-one: the two subscriptions wait for P4e

**Doc:** docs/7 P4a says the trait lands "minus the `record_*` methods, which join at P6 with
their tests" — one subtraction, stated once. D10 counts `subscribe_events` (hotplug) and
`subscribe_calibration` (per-session progress) among the trait's methods, so a literal
reading lands twenty-one at P4a.

**Repo:** nineteen. Both subscriptions are absent along with the three `record_*` methods,
and `crates/api/src/lib.rs`'s "What is not here yet" section names all five and says which
sub-milestone brings each.

**Why:** a subscription declared at P4a breaks the next two sub-milestones in turn. P4b
implements the server half of every method the trait carries the day the trait exists, and
the only stand-in for a subscription whose event source does not exist yet is
`Error::Unimplemented` — the variant P4d deletes (N6), so P4a would be adding the producer
P4d is removing. And P4c's method-count walk fails a registered method with no test, which is
exactly what a subscription nothing can drive would be. Neither cost buys anything: nothing
produces a hotplug event until P4d lands the uevent source, and nothing bridges a
`ProgressEvent` onto a socket until P4e lands the delivery semantics docs/7 gives it
("disconnect-mid-sweep semantics — the sweep continues, the subscription is reaped, both
asserted"). The plan is what is ambiguous here, not the repo: P4a's sentence subtracts one
thing and P4e's sentence lands two more, and only one of those readings survives P4b.

**The accounting this changes, recorded so P4c does not rediscover it.** A `#[subscription]`
registers *two* names, subscribe and unsubscribe. So P4e grows the registered population by
four rather than two while the trait's own count goes from nineteen to twenty-one, and P6c
then adds the three `record_*` for D10's complete twenty-four. The pinned-spelling test in
`crates/api/src/lib.rs` asserts all five absent names by name today, so landing one early is
a red test rather than a silent widening.

**Retires when:** P4e lands the subscriptions with their delivery semantics. Nothing else
should.

### Amendment, 2026-08-10: the stand-in this entry argued against no longer exists

The argument above turns on "the only stand-in for a subscription whose event source does not
exist yet is `Error::Unimplemented` — the variant P4d deletes (N6), so P4a would be adding the
producer P4d is removing". P4d has now deleted it. The reasoning is unchanged and the
conclusion is *stronger*: declaring a subscription before P4e can deliver on it no longer has
even a bad answer available, because the D13 registry's eighteen members contain nothing that
means "not built yet". P4e should read the sentence as history, and should not go looking for
the variant when it lands `subscribe_events` — the event source is the thing that has to exist
first, which is what this entry said all along and what P4d's uevent socket (note N53) now
provides.

---

## N30 — `discover_pairs` is a method on the wire and a flag on the command line, and that is one law with two surfaces

**Doc:** D10 lists `discover_pairs` as its own method beside `controls`. The shipped T4
executor disagrees in writing: `Executor::controls(&mut self, camera, discover_pairs: bool)`,
whose doc comment says "It is a parameter rather than a second method because the answer has
the same shape either way", and the command is `wch controls --discover-pairs`.

**Repo:** both, unchanged. T5 carries `wch_controls` and `wch_discover_pairs` as two methods;
T4 keeps its boolean and its flag.

**Why:** the plan routes them into different sub-milestones — P4b's read verbs are "`list`,
`info`, `controls`, `get`, `calibrate status/list`" and P4c's mutating list names
`discover_pairs` — which is only expressible if they are two methods. That split is not
bookkeeping: the probe **writes to the camera**, toggling automation-shaped controls and
restoring them afterwards, so a single `controls` method would mean "may or may not move your
camera depending on a boolean" to a daemon that has to route, permission and count it. The
CLI flag exists to make exactly that visible at the point a human types it; a method name
does the same job for a caller who sees only names.

The two surfaces answer to different laws, and neither is a copy of the other: T4's is "a verb
exists exactly once", T5's is "one method per operation the daemon routes". The engine still
holds one probe, `engine::discover::pairs`, called from one place in each binary, so design
§2.10 is satisfied where it actually applies.

**Why the answer is not a `ControlReport` either.** `wch_discover_pairs` returns
`schema::report::DiscoveryReport` — the control set after the probe, the candidates it
declined and why, and what putting the camera back achieved. `wch` prints the last two on
standard error after a probe; a client that could not see them would be running a write with
its restoration report withheld, which is what AGENTS rule 8 exists to prevent.
`engine::discover::Discovery::skipped` became `Vec<ProbeSkip>` in the same change, so there is
one spelling of "what the probe passed over" rather than a tuple on one side and a document on
the other.

**Retires when:** the daemon stops routing, permissioning or counting per method — at which
point the split buys nothing and D10's list is worth re-reading.

---

## N31 — A gate selftest arm can go red for the wrong reason, and the harness reports that as green

**Doc:** docs/9's structural rule is "both directions per gate — a predicate with no failing
case fails the selftest, and one with only failing cases fails it too". Nothing says an arm
must pin *which* branch it turned red.

**Repo, and the near-miss that forced it:** `selftest.sh`'s verdict for a `fail_case_*` is the
arm's exit status and nothing else (`if ((status != 0))` → `ok`). Several seeds in
`schema-artifacts-current.cases.sh` are red under more than one branch — a hand-edited
artifact is stale whichever artifact it is, and a file the emitter stopped writing is an
orphan while any hand-edited copy of it is also stale. `fail_case_committed_artifact_nothing_emits`
seeded `schemas/openrpc.json` specifically, which is the filename the OpenRPC document was
about to claim: from the moment xtask emitted it, that arm would have gone on exiting non-zero
while its subject silently became the *stale* branch, and the orphan branch would have had no
arm proving it can fire. That is note N10's family — a gate green while checking less than it
claims — reached without anybody editing the gate.

**Repo now:** the arms assert the message. `_red_because <pattern> <command>` returns the
predicate's status only when it failed *and* said `<pattern>` while failing; a predicate that
stayed green, or that went red about something else, returns **0** and prints what happened —
because 0 is how a `fail_case_*` tells this harness to look at it. Measured in all three
directions: a matching pattern over a red predicate returns its status (3), a wrong pattern
returns 0 with "the predicate went red, but not because of…", and a green predicate returns 0
with "the predicate stayed green…". The emitted document was also named
`webcam-handler-openrpc.json` rather than `openrpc.json`, so the orphan seed keeps a filename
nothing will ever emit — but it is the message assertion, not the name, that makes that a
check rather than a hope.

Nothing was weakened to fit: the two pre-existing tree-seeding arms were strengthened the same
way, and no arm was removed.

**Retires when:** `selftest.sh` learns to take an expected message per arm, at which point the
helper moves into the harness and stops being a per-case-file convention.

---

## N32 — A selftest arm that builds mutated Rust into the repository's `target/` poisons the repository's build

**Doc:** the selftest's own contract is that cases "never mutate the checkout" —
`gate_scratch_tree` copies the tree first, and every scratch copy lands under one directory
the harness removes. E7 records a neighbouring hazard for the mutation floor
(`copy_target = false`, because a test binary bakes in `CARGO_MANIFEST_DIR` and the corpus
loader walks its ancestors); this is a different mechanism with the same shape.

**Repo:** the arm docs/9's P4 row commissions — a method's wire name edited in
`crates/api/src/lib.rs`, the real emitter run over the copy — is the first selftest arm that
compiles *changed Rust*. The convention it inherited was
`CARGO_TARGET_DIR="$(gate_root)/target"`, which is what keeps a JSON-seeding arm at seconds
instead of minutes.

**Why that combination is unsafe, measured.** Cargo decides freshness by mtime, and
`gate_scratch_tree` copies with `tar -cf - | tar -xf -`, which preserves them: a scratch tree's
`crates/api/src/lib.rs` carries the *same* mtime as the checkout's, to the second (verified on
this tree). So a build of the mutated sources landing in the repository's own `target/` is
reused by the next `cargo run` over the pristine checkout, whose files are no newer than the
fingerprint that build just wrote. The seeded defect escapes its arm and becomes the
repository's binary until somebody touches a file — reproduced, and repaired with `touch`,
before it was designed out. A copy that "never mutates the checkout" mutated its build.

**Repo now:** `_isolated_target_dir <arm>` gives every Rust-editing arm
`target/gate-selftest/<arm>` — under `target/` so `gate_find`'s pruning keeps the tree-walking
gates and the scratch copies from paying for it, the same reasoning `.cargo/mutants.toml`
gives `target/mutants.out`. Measured: 27 s cold, about a second warm, roughly 550 MB. Arms
that edit only JSON keep the shared directory, because they change no input cargo looks at.
The selftest carries 101 failing arms after this sub-milestone, up from 97.

**Retires when:** cargo's freshness stops resting on mtimes, or `gate_scratch_tree` starts
stamping its copies with a fresh time — either would make the shared directory safe again, and
the second is the cheaper fix if a third Rust-editing arm ever makes the isolation expensive.

---

## N33 — What jsonrpsee's generated server accepts, measured, and what the OpenRPC document therefore says

**Doc:** D10 fixes the surface (`namespace = "wch"`, one trait) and says nothing about
request shape. docs/7 P4a says nothing either. The choice — named parameters everywhere,
and what the emitted document declares `required` — was made on a belief about jsonrpsee
that turned out to be false, and the correction is worth keeping because the belief is the
kind anybody would form from reading the macro.

**The belief:** that the generated request object carries no serde default, so an
`Option<T>` parameter needs an explicit `"camera": null` rather than an absent key. It was
written into two doc comments and into the emitted document's `required: true`.

**The measurement,** against the real `RpcModule` this crate's own tests build with
`into_rpc()` (jsonrpsee 0.26.0, `wch_calibrate_list`'s `camera: Option<CameraId>`):

| request | answer |
|---|---|
| `"params": {}` | served — `camera` arrives as `None` |
| `"params": {"camera": null}` | served — identical |
| `"params": []` | **refused**, `-32602 "Invalid params" / "No more params"` |
| no `params` key at all | **refused**, the same |
| `wch_info` with `"params": ["cam:x"]` | served |
| `wch_info` with `"params": {}` | refused, ``missing field `camera` `` |

The by-name path decodes into a `#[derive(Deserialize)]` struct
(`jsonrpsee-proc-macros-0.26.0/src/render_server.rs`, `decode_map`), and serde resolves a
missing `Option` field through `serde::__private::de::missing_field`, which visits `None`.
The absent `#[serde(default)]` never mattered. The positional path uses `optional_next`,
which answers `Ok(None)` for a *missing* element but not for an *empty array*: `[]` leaves
the sequence parser looking at `]` with nothing to read.

**Repo:** `"required"` is now `!param.ty.admits_absence()`, and `admits_absence` lives on
`api::wire::TypeRef` — one home, read off the type's own `schemars` output, because the
document and the daemon must not answer this differently.
`an_optional_parameter_may_be_left_out_and_a_required_one_may_not` walks `METHODS` and puts
the document's claim to the generated server directly: a request with no parameters is
served exactly when every parameter of that method admits absence. Both directions are in
one run (`wch_list` and `wch_calibrate_list` are served; the other seventeen are refused),
and the walk asserts it saw some of each so a uniformly-required surface could not pass it.

**And why the document still says `by-name`.** The server accepts positional requests too,
but *not uniformly* — the table above shows `wch_info` served positionally while
`wch_calibrate_list` needs `[null]` rather than `[]`. `"paramStructure": "either"` would
promise a shape one of the document's own methods rejects. So the document commits to the
one that always works, and `wire::Param`'s doc no longer claims a positional client is
"entitled" to anything.

**Retires when:** jsonrpsee's positional path treats an exhausted sequence as an absent
optional, at which point `"either"` becomes true and the ordering rationale comes back.

---

## N34 — Three P4a predicates and one assembly have no consumer until P4c/P4f, and each says so where it lives

**Doc:** rubric A8 — "a typed declaration nothing reads is a defect" — is the row that
convicted `Session::pre_snapshot` at G3 (note N23). P4a lands a wire surface with no server
and no client behind it, so it lands declarations whose readers are two sub-milestones
away, which is the same shape from a distance.

**Repo:** four of them, each with the gap written on the declaration itself rather than
implied by its absence:

- `api::photo::PhotoResponse::bytes_match_the_delivery` — a `PhotoResponse` off a socket
  whose `byte_count` disagrees with its payload. Consumers: the daemon before it sends one
  (P4c), `wchc` before it turns one into a `cli_core::Photograph` (P4f). `crates/client` is
  still an empty `main`, so there is nowhere for the call to go today, and a truncated
  photo is refused by nobody.
- `schema::capture::Sink::is_addressable` — a relative `Sink::ServerPath` arriving over the
  wire. `cli_core::Command::photo_request` resolves against the caller's cwd before sending
  (D10), so `wch` cannot produce one; the daemon's cwd under systemd is `/`, so a
  hand-written client sending `{"kind":"server_path","path":"out.jpg"}` would have
  `/out.jpg` written as the daemon's uid. Consumer: P4c's `photo` routing. It landed as a
  predicate beside the variants rather than as a paragraph in the T5 method's doc, because
  a paragraph is a thing an implementer has to have read.
- `api::codes::typed` — the inverse of the error mapping, whose consumer is `wchc`'s
  decode (P4f). Landed with the mapping deliberately: one home, both directions, in the
  commit that owns the law.
- `schema::report::DiscoveryReport`'s `controls` field — not a predicate but the same
  shape. Two of the three fields come straight from `engine::discover::Discovery`; this one
  is assembled (`camera.controls()` *after* the probe, then
  `pairing::applicable(&controls, &merge(declared_pairs(), measured))`), and today that
  assembly exists once, in `crates/cli`'s `InProcess::controls`. When P4c routes
  `wch_discover_pairs` it becomes a second copy unless the assembly moves into
  `engine::discover` first. **That move is P4c's, and this entry is the obligation.** The
  doc comment used to say the daemon "assembles this field-for-field with nothing
  translated", which was false for exactly the field that carries the work.

**Why not land the consumers now:** P4a's whole scope discipline (docs/7's risk register)
is that a sub-milestone that turns out to be two splits rather than stretching. A daemon
call site requires the daemon (P4b), a client one requires the client (P4f), and inventing
either here to satisfy A8 would be landing two sub-milestones' work to make a doc comment
true.

**Discharged, row by row.** The fourth row — the `DiscoveryReport` assembly — was
discharged at **P4c**, in the step that routed the control-shaped mutating verbs, and
*before* the daemon had a call site rather than after: the assembly is now
`engine::discover::report`, which probes, re-reads the control set the camera is in
afterwards, and merges declared with measured through `pairing::in_effect`. `crates/cli`'s
`InProcess::controls` and `daemon::server::discover_pairs` both call it and neither
assembles anything, and `engine::lifecycle::discover_pairs` — which had been writing
`applicable(controls, &merge(declared_pairs(), …))` by hand, a *third* spelling nobody had
noticed — collapsed onto the same function in the same change. Two tests hold it: the
engine's own, which latches a toggle so a report built from a control set read *before* the
probe describes a device that no longer exists, and the daemon's, which compares
`wch_discover_pairs`'s whole answer against the one engine call. A daemon that assembled it
itself with the measured pairs dropped goes red on both.

The **second and third rows went with `photo`**, in P4c's next step, and they landed in the
two places the entry named rather than in one convenient one:

- `schema::capture::Sink::is_addressable` is called by `daemon::server::addressable`, which
  runs **before** the handler resolves a camera. Refusing early is the whole value —
  `FakeBackend::opens()` is still zero after the refusal, which is what the daemon suite
  asserts, so a request this build was never going to honour costs nobody a descriptor. The
  refusal is `Error::IllegalTransition` naming the path and saying it has to be absolute;
  note **N46** records the pick. Deleting the call turns the daemon suite red on the
  refusal *and* on the descriptor count.
- `api::photo::PhotoResponse::bytes_match_the_delivery` is called by
  `daemon::server::photo_response`, as the last statement before the answer is returned, so
  there is exactly one place it can be skipped. Its own unit test feeds a hand-built
  `Photograph` whose delivery and payload disagree — reachable because `Photograph`'s
  fields are `pub` and nothing else in the workspace can build the disagreement — and the
  daemon suite asserts the predicate again from the *client* side on every photo answer it
  receives, which is the half `wchc` will do at P4f.

~~One row remains, and it still names its sub-milestone: `api::codes::typed`'s client-side
consumer is P4f's `wchc`.~~

**Checked off at the G4 boundary, 2026-08-11 (docs/7 P4g), and verified in the tree rather
than read off the commit that claims it.** The last row is discharged. `api::codes::typed`'s
client-side consumer is `client::remote::refusal` (`crates/client/src/remote.rs:175`, called
at `:159`, `:611` and `:624` — the three places a `jsonrpsee` `ClientError` can reach this
crate, and there is no fourth). It is the whole of the branch: `ClientError::Call(object)`
whose payload `codes::typed` accepts becomes that `schema::Error`, and **everything else**
becomes `Error::StorageIo` naming the socket rather than being dressed up as a camera answer
(E3 — availability is not capability). The reconstructed value is rendered once, at
`crates/client/src/main.rs:36`, through the same `Display` `wch` renders it with, which is
the identity `./scripts/gates/cli-parity.sh` compares byte for byte; the smallest place it
can be watched is `crates/client/tests/wchc.rs:216–227`, where `wchc --json get <cam>
warp_drive` exits 1 with `wchc: no control named "warp_drive"` — a D13 error that crossed the
wire and came back typed, asserted in both directions against a control the camera does have.
**All four rows of this entry are now discharged**, each in the sub-milestone it named: the
`DiscoveryReport` assembly at P4c, `Sink::is_addressable` and
`PhotoResponse::bytes_match_the_delivery` at P4c, and the decode at P4f.

**One thing the check-off found and did not fix, because a declaration is the thing this
entry is about.** `PhotoResponse::bytes_match_the_delivery`'s own doc comment still says
"**One of its two consumers landed at P4c** and the other is still owed … The one still owed
is `wchc` … (P4f); until then a truncated payload is refused by the sender and by nobody on
the receiving end" (`crates/api/src/photo.rs:152–160`, and the field's own comment at
`:134`). The consumer landed: `client::remote` calls it at `crates/client/src/remote.rs:468`
on every photo answer it receives, and E13's hardware arm drives that call against three real
cameras. So the declaration now names a gap that has closed — which is this entry's own
mechanism ("each declaration names which") pointed the wrong way, and is exactly the rot A8's
row exists to prevent one level up. Recorded here, at file:line, for the G4 review to price;
not fixed in the commit that found it.

**Retires when:** ~~P4f lands its call site.~~ Nothing further — every row named its
sub-milestone and every sub-milestone landed. Each declaration named which, so the review
that closes G4 checked them off rather than rediscovering them, which is the whole return on
having written them down.

---

## N35 — `ControlWrite` reaches the shared command surface and stops; the engine keeps its tuple

**Doc:** design §2.10 — one home per law — and the precedent set three times in this same
change: `SessionRef`, `Selection` and `ChosenBy` moved out of `webcam-handler-cli-core`
into `webcam-handler-schema` so the wire and the command line name one type, and
`ProbeSkip` was pushed all the way into `engine::discover` for the same reason.
`ControlWrite` did not get the same treatment, and an asymmetry with no note is
indistinguishable from an oversight.

**Repo:** `ControlWrite` is the shape of a requested write from the command line inward.
`cli_core::Assignment` is a clap newtype over one (the `BackendKindArg(BackendKind)`
pattern), and `cli_core::Executor::set` takes `&[ControlWrite]` — so `wch` and `wchc` hand
their shared surface the same value and P4f's parity gate compares two paths whose input
has one shape. `engine::write::set`, `engine::pairing::plan` and `plan_unguarded` still
take `&[(ControlSlug, ControlValue)]`.

**Amended at P4c, when the boundary got its second crossing.** Routing `wch_set` gave the
conversion a second caller — `crates/cli`'s `InProcess::set` and
`daemon::server::set` — and a four-line `map` written twice is how the two surfaces P4f
compares start to differ. The conversion is now **`ControlWrite::target`**, a method on the
type beside the fields it reads; the entry below is otherwise unchanged, because moving the
boundary itself is still the large mechanical edit it was. So the stop is in the same place
and there is one spelling of where it is.

**Amended again by the P4c review, which found the method was not enough.** `target` gave the
four-line `map` one home and the *three-line assembly the map sits inside* was still written
twice — read the controls, `pairing::in_effect(&controls, Vec::new())`, convert, call
`write::set` — once in `crates/cli`'s `InProcess::set` and once in `daemon::server::set`, with
`snapshot` and `restore` copied the same way beside them. That composition is a rule and not
plumbing: which pair set a write plans against decides whether an automation control is
switched off first, and `Vec::new()` for the measured pairs is itself a decision (measuring
writes to the camera — note N30). Both roots now call `engine::write::set_requested`,
`engine::snapshot::take_in_effect` and `engine::snapshot::restore_in_effect`, which is where
`target` is called from too. The three wrappers are in `write.rs` and `snapshot.rs` rather
than in `pairing.rs` on purpose: `pairing` is a pure core in the mutation floor's scope and
takes values, so a function there that takes a `&mut dyn Camera` would be the wrong kind of
thing in the wrong file.

**Why the stop is there.** The rule §2.10 protects is "one spelling of a fact that crosses
a boundary". The wire is a boundary and the T4 executor is the seam two binaries are
compared across; `engine::pairing` is neither — it serializes nothing, and a named pair
buys it no document. Against that, the migration is not free: `pairing.rs` is one of the
mutation floor's pure cores, its callers build targets inline in about forty places
including two integration suites, and a large mechanical edit through a mutation-floor file
is exactly the kind of thing docs/7's risk register says splits a sub-milestone. The
contrast with `ProbeSkip` is the deciding one: there the tuple had a single producer and a
single consumer, so one spelling cost four lines.

**Retires when:** `engine::write::set` and `engine::pairing::plan` take `&[ControlWrite]`,
at which point `ControlWrite::target` and its two call sites go and this entry with them.
A good moment is whenever `pairing.rs` is being edited for its own reasons — which P4c was
not, and is why the amendment above is a method on the type rather than the migration.

---

## N36 — "A frame may contain a person" has four subjects and four tests, and no walkable population

**Doc:** AGENTS.md's privacy section and rubric A12: "Camera frames never enter the
repository, logs, or error messages." `scripts/gates/no-frame-bytes-in-repo.sh` enforces
the *repository* clause by content-sniffing committed files. Nothing enforces the *logs and
error messages* clause, because it is about a `Debug` impl that does not exist yet on a type
somebody has not written yet.

**Repo:** four types hold raw camera bytes and all four hand-write `Debug` to print a count:
`schema::capture::Frame` (P1), `api::photo::Base64Bytes` (P4a), `engine::photo::Photograph`
and `cli_core::Photograph` (both P2, both found deriving `Debug` over
`returned: Option<Vec<u8>>` by the P4a review). Each has its own
`…_never_reach_a_debug_line` test, driven by real bytes and asserting the first bytes'
decimal rendering is absent, so each can go red on its own.

**Why four tests and not one mechanism.** The population is not walkable: "a type that
holds camera bytes" is not something the compiler, a lint or a `cargo metadata` walk can
enumerate, and a grep gate over `#[derive(…Debug…)]` near a `Vec<u8>` would be a heuristic
with false positives across the whole tree — the exact "check that names locations it can
drift from" docs/9 bans. So the honest statement is that this is four independent tests and
the fifth type will need a fifth, which is a real gap and is why it is written down.

**What would close it, and why it did not happen here.** A `FrameBytes` newtype in
`webcam-handler-schema` with the hidden `Debug`, wrapped by `Frame.bytes`, both
`Photograph.returned` fields and `Base64Bytes`, would make the rule structural: a type
holding frame bytes could not derive `Debug` over them at all, and the population would be
"whoever names `FrameBytes`". It touches `Frame.bytes`, which is `pub Vec<u8>` and is read
directly by `imaging`, `v4l2`, `fake` and their suites — a refactor with its own risk, in a
commit whose subject is the wire surface.

**Retires when:** the newtype lands and the four tests become one property of one type.

---

## N37 — `WireError::source` is an equivalent mutant today, and the register is what will notice when it stops being one

**Doc:** `.cargo/mutants.toml`'s own rule for the floor's scope, and
`scripts/mutants-accepted.txt`'s rule for its exceptions: an entry earns its place either
because no hermetic test can turn the line red, or because the mutant is *equivalent* — no
input distinguishes the two programs.

**Repo:** `crates/api/src/codes.rs` implements `std::error::Error for WireError` with
`fn source(&self) -> Option<&(dyn Error + 'static)> { self.0.source() }`. cargo-mutants
replaces the body with `None` and the whole workspace stays green.

**Why it is equivalent, and why the line stays anyway.** `schema::Error` is a `thiserror`
enum in which no variant carries a `#[source]` or `#[from]` field, so `self.0.source()` *is*
`None` for every value there is; the mutant and the original are the same program. The
delegation is still what the type means: `WireError` is a transparent newtype (`Display`
delegates too, deliberately — a `source()` answering `Some(&self.0)` would make every chain
printer render D13's one sentence twice, which is what
`the_wire_error_adds_no_second_rendering_of_anything` now asserts). Writing `None` instead
would be writing a coincidence where a relationship belongs.

**And the acceptance is a tripwire, not a shrug.** `scripts/mutants.sh` compares survivors
against the register **in both directions**: the day a `schema::Error` variant gains a
source, this mutant becomes killable, the entry stops surviving, and the job fails asking
for the test. That is the second direction doing exactly the job N15 paid for.

**Retires when:** a D13 variant carries a source of its own.

---

## E8 — The mutation floor's second run, over the scope P4a widened, 2026-08-09

E7 records the floor's commissioning over six files. The P4a adversarial review found two
survivors in `webcam-handler-api` **by hand** — `photo.rs`'s `(Path, None) => true` arm
flipped to `false`, and `codes.rs`'s `D13_CODES` range guard deleted, each leaving the
whole workspace green — which is the argument for a widening in one sentence: the floor
exists to find exactly those, and it could not see them because the crate was not in
`examine_globs`.

### The widening

Three lines: `crates/api/src/{codes,photo,wire}.rs`. They belong for the reason the six do —
each takes values and returns values, and `webcam-handler-api` starts no runtime and opens
nothing (note N5's review-held half), so a survivor there is a unit test somebody can write
today. `crates/api/src/lib.rs` is deliberately **not** among them and the scope file says
why: it is a `wire_surface!` invocation and nineteen doc comments, with no expression to
mutate.

### The run

**478 mutants in 21 minutes: 409 caught, 11 survived, 58 unviable, 0 timed out**, judged
by the whole 678-test workspace suite, four parallel jobs on an eight-core machine (about
2-3 s of incremental build and 8-10 s of test per mutant, both paid 478 times). E7's run
was 410 mutants over six files in 21 minutes with five jobs; the sixty-eight new ones are
the wire crate's, and the cost did not move.

The eleven survivors are the ten E7 already triaged (N25's six, N26's three, N27's one) plus
exactly one new: `WireError::source` delegating to an inner error that never has a source,
which is equivalent and is recorded as N37. The register comparison runs clean in both
directions — eleven survivors, eleven acceptances, nothing unexpected and nothing stale.

### What the widening actually bought, measured

Both hand-found survivors are dead, and each was watched red before the fix:
`an_empty_photo_is_an_answer_rather_than_an_absent_one` now calls the predicate on the
`ServerPath` answer, and the range guard was **deleted** rather than tested — it could not
discriminate, because `rpc_code` is total onto `D13_CODES`, so the check it was claimed to
be was already the one below it. The run also found four more the review had not: three on
`Base64Bytes::into_inner` and one on `is_empty` (no test read the payload back), and one on
`SERVER_ERROR_BAND`'s lower bound (`-32099..=-32000` with the minus deleted still contained
every code anybody asserted). All five are now covered by
`the_payload_reports_its_own_size_and_hands_the_same_bytes_back` and by the band's own
both-ends assertion.

**Retires when:** never — this is dated evidence. The next widening writes its own entry.

---

## N38 — The daemon depends on `jsonrpsee-server` by name, because the facade's `server` feature takes down the T6 wall

**Doc:** design §2.8's crate tree gives `daemon/` "jsonrpsee server" among its
dependencies, and §2.8's purity walls give `webcam-handler-api` its own: "**no axum, no
hyper, no tower-http; tokio allowed** \[N5\]". `scripts/gates/dependency-walls.sh` is where
the second one is a fact rather than a wish, and N5 records that the surviving half of the
wall is the one that matters — "only `daemon` links the web stack".

**Repo:** `crates/api` depends on the `jsonrpsee` facade
(`features = ["macros", "client-core", "server-core"]`) and the daemon needs a server.
The obvious spelling — `jsonrpsee = { workspace = true, features = ["server"] }` in
`crates/daemon` — turns the wall red, because `cargo metadata` resolves features **once
per package for the whole workspace** and emits one node per package: the shared
`jsonrpsee` node gains `jsonrpsee-server`, `jsonrpsee-server` reaches `hyper`, and
`webcam-handler-api` therefore *reaches* hyper too. Measured on this tree, not reasoned
about:

```
$ WCH_GATE_METADATA=$scratch/md-facade.json ./scripts/gates/dependency-walls.sh
  FAIL  dependency-walls: webcam-handler-api links hyper; only the daemon links the web stack (T6)
FAIL dependency-walls — 1 violation(s) over 1377 examined items
```

Depending on the **`jsonrpsee-server` package** directly does not: the facade's feature set
is untouched, so `api`'s closure is what it was, and the daemon links exactly what it
serves.

```
PASS dependency-walls — 1432 items examined, 0 named skip(s)
```

**Why this and not the alternatives.** Three ways out, and only one is cheap *and* honest.
Teaching the wall to resolve features per dependent (`cargo tree -e normal -p <crate>`)
would work — it was measured too — but it is a gate rewrite with its own fail arms and a
second injectable seam, and it changes what the gate *means*: package reachability is what
`cargo build -p` links, whereas the metadata union is what `just ci`'s `--workspace` build
actually links, and the union is the stricter reading. Widening the wall to allow
`api → hyper` deletes the half N5 kept; AGENTS is explicit that weakening a posture "is an
owner decision, not a convenience fix". So the daemon names the sub-crate.

It is not a version change (`0.26.0`, the same pin, the same release train) and not a new
upstream project. `RpcModule` comes from `jsonrpsee-core` either way, so the T5 trait's
`into_rpc()` and the server this crate builds agree about the type — there is no
two-facades hazard hiding behind this.

**The tripwire:** the manifest comment in `crates/daemon/Cargo.toml` says all of the above
in three sentences, because the failure mode is a tidy-up. Somebody consolidating "two
jsonrpsee dependencies" into the facade's feature gets a red gate whose message
(`webcam-handler-api links hyper`) names a crate their diff did not touch.

**What this changed about the wall it did not touch.** Wall 1b — "`webcam-handler-api`
links no web stack" — was until P4b quantifying over an empty set: `hyper` was not in the
workspace graph at all. The daemon's `jsonrpsee-server` edge puts it there, so the wall
started saying something the moment this landed, and what it said first was that the
obvious manifest spelling was wrong.

**What `ServerConfig` still decides for us, and what was done about it.** `uds::serve` sets
four of jsonrpsee's thirteen config fields from `schema::limits` (request bytes, response
bytes, batch size, and the per-request connection cap) and enforces a fifth bound —
*accepted connections* — in its own accept loop. The remaining eight are inherited, and two
of them are bounds in AGENTS's sense: `message_buffer_capacity` (a channel depth, which
AGENTS puts in `schema::limits`) and `max_subscriptions_per_connection`. Both govern the
WebSocket surface only, so rather than ship somebody else's numbers behind a transport no
test drives, P4b calls `ServerConfig::http_only()` and
`a_websocket_upgrade_is_declined_until_the_phase_that_brings_its_bounds` pins it. **P4e owns
turning it back on**, with those two constants and the subscription tests that reach them.
`keep_alive_timeout` is inherited and inert here — it is hyper's HTTP/2 setting and this
transport is HTTP/1.1 over `AF_UNIX` — which also means an accepted-but-silent connection
has no server-side timeout, and the accept loop's own connection permit is what bounds it
instead.

**And one measured thing about the cap that shares its name.** `ServerConfig::max_connections`
is acquired inside `TowerService::call` — per in-flight HTTP request, released with the
response (`jsonrpsee-server-0.26.0/src/server.rs`) — so it is not the bound
`limits::DAEMON_MAX_CONNECTIONS`'s doc describes. Measured before the fix: with the cap at
32, 128 idle connections were all accepted and held, each one a descriptor on a process
whose descriptors are also the camera's. The permit is now taken in our accept loop and
held for the connection's life; the config's cap stays set, where it bounds concurrent
requests.

**Retires when:** a jsonrpsee release stops routing `hyper` through the facade's `server`
feature, or `dependency-walls.sh` learns to resolve features per dependent. N5's standing
instruction — re-run the measurement on any jsonrpsee bump — covers the first, and the
inherited-defaults list above is the thing to re-read on the second.

---

## N39 — A leftover socket is stale *because the daemon holds the state lock*, and a wrong directory mode is refused rather than repaired

**Doc:** design D11 — "The Unix socket (`$XDG_RUNTIME_DIR/webcam-handler/wchd.sock`,
directory 0700) is always served", and "filesystem permissions are the auth model". docs/7
P4b — "socket directory 0700 asserted at startup". docs/9's UDS-permissions row —
"startup assertion + test". **Nothing in the series says what happens to a `wchd.sock`
left behind by a dead daemon**, and nothing says whether a wrong directory mode is a
refusal or a repair. Both had to be decided to land the transport, so both are recorded
here rather than left implicit in `crates/daemon/src/uds.rs`.

**Repo:** `daemon::uds::SocketDir` answers each once.

**The stale socket.** `bind(2)` on an existing path is `EADDRINUSE` unconditionally — a
socket file is not a lock and outlives the process that made one — and at P4b *every* exit
is un-drained by design (P4e owns shutdown), so a daemon that never unlinks cannot restart
after a single `Ctrl-C`. The answer needs no new law, only D9's: the daemon holds the state
directory's advisory lock under `LockProtocol::HeldForLifetime`, Linux releases an `flock`
when the holding process dies, so a process that has *taken* that lock has already
established that no other daemon is alive for this user — and therefore that anything at
the socket path is stale. The ordering is lock, then directory, then unlink, then bind, and
`SocketDir::bind` takes `&StoreLock` by parameter so a caller cannot get the order wrong;
it checks the protocol, because `wch`'s per-operation lock is released moments later and
carries none of the argument.

Two things are deliberately *not* done. The folklore alternative — connect to the socket
and treat `ECONNREFUSED` as "stale" — is rejected: it races a daemon that is mid-startup,
it is weaker (a wedged daemon accepts and never answers), and it asks the socket a question
the lock already answers. And only a **socket** is unlinked: a regular file, a directory or
a symlink at that path is refused, because deleting an operator's file to make room is a
data-loss bug wearing a cleanup routine. A reviewer who finds an `unlink` before a `bind`
in any other codebase is looking at a socket-hijacking bug, which is why this paragraph
exists.

**The directory mode.** Create-with-0700 is not enough: `mkdir`'s mode is masked by the
umask (which can only clear bits, so a directory *this* call creates is always 0700), but a
directory that already exists keeps whatever mode it already had — an
`$XDG_RUNTIME_DIR/webcam-handler` left 0755 by an older build, a tmpfiles rule, or an
operator is a camera daemon anyone on the machine can talk to. So the mode is read back and
**refused, not repaired**. A silent `chmod` would hide the fact that the directory was
reachable, and for however long it was reachable it may already have been reached; D11's
posture errs closed and an operator who is not told cannot act. The refusal names the
directory and both modes, and `a_group_readable_socket_directory_is_refused_rather_than_repaired`
asserts that the directory is *unchanged* afterwards, so a helpful repair cannot be added
without a red test.

**Where the assertion is not.** The socket file's own mode is never asserted, and that is
correct rather than an omission: `connect(2)` checks search permission on every directory
component and write permission on the socket inode, and the inode is created with
`0777 & ~umask`, so the 0700 **directory** is the boundary D11 names. Asserting the
socket's mode would be asserting the wrong thing while looking thorough.

**Which makes the check on that one directory carry the whole model — so it is a check on
an inode, not on a name.** The first version of this code checked a *path*, and the P4b
adversarial review measured three holes in it against the real `SocketDir` API:

- `std::fs::DirBuilder::recursive(true).create()` succeeds when the leaf is a **symlink to
  a directory** (`create_dir_all` falls back to `path.is_dir()`, which follows), and
  `std::fs::metadata` then reports the *target's* mode. So
  `$XDG_RUNTIME_DIR/webcam-handler` symlinked at any 0700 directory passed the whole check,
  and `bind` created `wchd.sock` inside the link target. Now the leaf is `lstat`ed and a
  symlink is a refusal.
- `runtime_dir` validates only that `$XDG_RUNTIME_DIR` is set, non-empty and absolute, and
  `recursive(true)` created the entire chain when it named a path that did not exist —
  `XDG_RUNTIME_DIR=/tmp/x/no/such/runtime` produced a served daemon with no complaint. That
  turns "you are not in a login session", which `runtime_dir`'s own doc says the operator
  must be told, into a directory under `/tmp` that nobody promised anything about. The base
  is now verified (exists, is a directory, is not a symlink) and only the `webcam-handler`
  component is ever created.
- `bind` re-resolved the path and re-checked nothing, so `chmod 0777` between `prepare` and
  `bind` left `bind` succeeding. `SocketDir` now carries the checked directory's
  `(st_dev, st_ino)` and `bind` refuses unless the name still leads to that inode at that
  mode.

Why this matters on a real host and not only in a test: the attack needs the *parent* of
the socket directory, and unlink/rename permission is a property of the parent. On the many
setups that export a synthesised `XDG_RUNTIME_DIR=/tmp/runtime-$USER` under sshd, in
containers, or from a `startx` wrapper, whoever creates that directory first owns it.

**What is still open, and it is a dependency decision rather than a defect to fix here.**
The remaining window is between the re-check and `bind(2)` itself. Closing it needs the
directory held as a **descriptor** — `open(O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC)`, `fstat` on
that descriptor, and a bind relative to it — and it is also what would let the daemon
compare `st_uid` against `geteuid()`, which is the check a root-run or setuid-adjacent
daemon wants and which nothing here makes. All three need a syscall wrapper: every crate
outside `crates/backends/v4l2` is `#![forbid(unsafe_code)]`, and adding `rustix` or `libc`
to the daemon is a design §2.8 registry change and an owner's call, not a review's. Until
then the owner check is absent by omission rather than by argument, and the ordinary
non-root case is self-refuting anyway — a 0700 directory belonging to somebody else is one
this daemon cannot traverse, so `bind` fails with `EACCES`. **P4d owns the decision**, where
the unprivileged-bind measurement (design §8 item 10, note N8) already puts a syscall-level
question on the table.

### Amendment, 2026-08-10: it was never a decision, and two of the three facts above were
### already true when this was written

The owner ruled (§2.8) that adopting a crate which clears the licence bar is applying the
bar rather than moving it, so there is no decision here to own — only work. Worse for this
entry: the two facts that would have made that obvious were already in the tree when it was
written. `rustix` 1.1.4 has been in `Cargo.lock` transitively since before P4b, and
`deny.toml` has carried `Apache-2.0 WITH LLVM-exception` since P0 with the comment
"precautionary: rustix offers it as one OR-alternative" — somebody anticipated this exact
adoption and left the door open. So "adding `rustix` or `libc` to the daemon is a §2.8
registry change" was wrong twice over: not a registry change, and not new supply chain.

What survives is the choice between the two, and its ground is the sentence above rather
than licensing: `rustix` is a *safe* wrapper, so `openat`/`fstat`/bind-relative-to-a-dirfd
land without an `unsafe` block in a crate that forbids them and without moving the boundary
`unsafe-scope.sh` asserts. `libc` would give the same syscalls and the same licence and
would need the daemon to stop being `#![forbid(unsafe_code)]`, which is a real design change
and not this one.

**The lesson, which is why this amendment is longer than the fix.** The entry deferred on a
ground it had not checked, and the check was two `grep`s. A deferral is a claim, and an
unverified one outlives whatever created it — this is note N15's family (an acceptance
nobody re-checks) wearing a scheduling costume rather than a testing one.

### Amendment, 2026-08-10: the window is closed, and the literal ask was unreachable

Landed at P4d with `rustix` 1.1.4, three features (`fs`, `net`, `process`), **no `unsafe`
block** and no change to `deny.toml`. `SocketDir::prepare` now opens the directory
`O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC | O_PATH`, `fstat`s *that descriptor*, checks the mode
**and `st_uid` against `geteuid()`**, and holds the descriptor for the daemon's life;
`SocketDir::bind` does its `statat`, its `unlinkat` and its bind through the same
descriptor. `SocketDir` loses `#[derive(Clone)]` — an `OwnedFd` is not `Clone` and nobody
cloned it.

**The literal ask does not exist, and that is a fact about Linux rather than about rustix.**
`bind(2)` takes a `sockaddr_un` whose `sun_path` is resolved from the process's root and
cwd, and there is **no `bindat(2)`** — checked in the pinned rustix source
(`net/socket.rs`'s `bind` takes one address and no `dirfd`; `SocketAddrUnix` has exactly
three constructors, none taking a descriptor) and true of `libc` and of a hand-written
syscall for the same reason. Two things do exist:

- `fchdir(dirfd)` then bind a relative name. **Rejected:** the working directory is
  process-global, so changing it inside a multi-threaded tokio daemon is a data race with
  every other thread's relative path for the duration of the call.
- **`bind("/proc/self/fd/<dirfd>/wchd.sock")`.** procfs `fd` entries are *magic links*:
  resolution through one jumps to the dentry the descriptor holds rather than re-walking a
  stored name. This is the dirfd-relative bind, spelled the way Linux offers it. Measured
  on this host, 2026-08-10, with an independent Python probe rather than with this code:

  ```
  O_RDONLY|O_DIRECTORY   bind(/proc/self/fd/3/wchd.sock) OK; len=25; socket? True; dir st_uid=1000 mode=0700 ino=9041806
  O_PATH|O_DIRECTORY     bind(/proc/self/fd/3/wchd.sock) OK; len=25; socket? True; dir st_uid=1000 mode=0700 ino=9041806
  after the swap: path …/webcam-handler -> ino 9041809, fd still -> ino 9041806
  bind via /proc/self/fd landed in the attacker's dir: False; in the checked inode: True
  bind into a DELETED directory: errno=2 No such file or directory
  ```

  The third line is the whole point: the name was replaced with a *different directory*
  between the check and the bind — this entry's exact scenario — and the socket still landed
  in the inode whose mode was asserted. A checked directory that is **removed** instead
  fails closed with `ENOENT`. The composed path is 25 bytes against `sun_path`'s 108, and
  `O_PATH` (no read permission asked for) is enough.

**What the claim became, which is not what it was.** The substitution scenario is now
*defeated* rather than *detected*, and the tests say so in their names:
`a_socket_directory_substituted_between_the_check_and_the_bind_is_defeated` asserts the
socket landed in the moved-aside checked directory and **not** in the attacker's,
`…_removed_…_fails_closed` covers the unlink case, and
`…_re_permissioned_…_is_refused` is what the surviving `fstat` re-check is for — the
inode's own mode changing under a running daemon, which is a question about the right object
rather than about which object. The owner check is driven as a predicate over a `Stat` with
one field moved, both directions, because arranging a directory owned by another uid needs
privileges this suite must not acquire (note N44's precedent).

**Four consequences, written down rather than discovered:**

1. **A `listen` backlog is now ours.** `tokio::net::UnixListener::bind` chose 1024;
   creating the socket ourselves means naming the number, so
   `limits::DAEMON_LISTEN_BACKLOG` is 64 with its own doc and `bind` reads it. AGENTS'
   "bounded everything" applied to a bound we had been inheriting.
2. **`local_addr()` stopped being the way to ask where the socket is.** It reports the
   address passed to `bind(2)`, which is now `/proc/self/fd/<n>/wchd.sock`. The test that
   used it asks a real `UnixStream::connect` on D11's own path instead — which is the
   claim it was standing in for and is strictly stronger.
3. **`MAX_UNIX_SOCKET_PATH_BYTES` kept its check and changed its meaning.** The bind is 25
   bytes and cannot overflow `sun_path` however deep `$XDG_RUNTIME_DIR` is; the *client*
   connects by the real name, so the 107-byte refusal is now on the client's behalf. Its
   message says so, or the next reviewer reads it as dead code and deletes it.
4. **`(st_dev, st_ino)` demoted from mechanism to diagnostic.** The descriptor is the
   object now; the pair is carried so a refusal can name which inode was checked.

**The honest residual, after all of that:**

1. **It needs `/proc` mounted.** A minimal container without procfs cannot use the magic
   link, and `SocketDir::bind` falls back to binding by name — with a `tracing::warn!` that
   names what is no longer being protected. A silent downgrade of an authentication model
   is worse than the window it hides. Not exercised on this desk (`/proc` is mounted), so
   this is a guard that exists rather than a guard that has fired.
2. **The base directory is still resolved by name, once.** `open` of `$XDG_RUNTIME_DIR`
   walks a path, and an attacker owning *its* parent can swap it between the environment
   read and the open. Closing that needs a component-by-component `openat` walk from `/`
   with `O_NOFOLLOW` at each step, or `openat2(RESOLVE_NO_SYMLINKS | RESOLVE_BENEATH)`
   which rustix exposes as `rustix::fs::openat2`. One syscall of window against a path
   walk; stated rather than done, and `openat2` is the named way to do it if it ever is.
3. **The uid check is `geteuid()`, deliberately.** A root daemon serving a root-owned
   runtime directory passes; a root daemon pointed at a *user's* runtime directory is now
   refused, which is the case this entry said "nothing here makes".
4. **The socket inode's own mode is still unasserted**, and the paragraph above argues at
   length that this is correct rather than an omission. Nothing here changes it.

### Amendment, 2026-08-10: which flag closes the window, and residual 1 driven

Three corrections out of the P4d adversarial review, all inside the amendment above.

**1. `O_NOFOLLOW` was not what refused the symlink, and the file said twice that it was.**
`prepare`'s doc said "`O_NOFOLLOW` makes a symlink `ELOOP` from the kernel", and the
refusal message said the directory is opened "`O_NOFOLLOW`, so a symlink is refused by the
kernel rather than followed". Both are false **for this flag combination**, and `open(2)`
says so outright: with `O_PATH` and `O_NOFOLLOW` together the call *succeeds* and returns a
descriptor referring to the symbolic link. Re-measured on this host, 2026-08-10, with an
independent probe against the C API:

```
O_PATH|O_NOFOLLOW              -> OPENED st_mode=0o120777 islnk=True
O_PATH|O_NOFOLLOW|O_DIRECTORY  -> FAILED errno=20 ENOTDIR
O_NOFOLLOW (no O_PATH)         -> FAILED errno=40 ELOOP
O_PATH|O_DIRECTORY             -> OPENED st_mode=0o40775 islnk=False   (the target)
```

So under `O_PATH` it is **`O_DIRECTORY`** that refuses a symlink, spelled `ENOTDIR` — which
is the errno `a_symlinked_socket_directory_is_refused_however_private_its_target_is` already
asserted, one comment away from the prose that contradicted it. `O_NOFOLLOW` is still
load-bearing: without it the open follows the link and checks the *target*. Neither may be
dropped, and the reason a maintainer would drop `O_DIRECTORY` — a better diagnosis for "the
path exists but is a regular file" — was exactly what the false rationale invited.

The repair is not only prose. The four flags are one named constant, `SOCKET_DIR_OFLAGS`,
whose doc states which flag does which job; and the symlink test now *measures* it, opening
the same link with `SOCKET_DIR_OFLAGS.difference(OFlags::DIRECTORY)` and asserting the open
succeeds on an `S_IFLNK` inode. Dropping `O_DIRECTORY` turns that test red rather than
leaving the mode check (`0o120777` ≠ `0700`) as the last accidental line of defence.

**2. Consequence 4 above is withdrawn: `(st_dev, st_ino)` is gone.** Demoting the pair to a
diagnostic left a comparison between two `fstat`s of *one open descriptor*, which cannot
disagree — an open descriptor pins its inode, and `st_dev` cannot change. It was an arm no
input reaches and no test can turn red, in a module whose subject is that a check nobody
drives is a check nobody has, and the same P4d commit deleted three unreachable arms one
crate over for that reason (`hotplug::refusal`). `SocketDir` loses the field;
`still_the_directory_that_was_checked` is now the `fstat` plus `check_mode_and_owner` that
does have both directions driven. **Substitution is defeated structurally and no longer
detected**, which the module header already said correctly and the field's own doc did not.

**3. Residual 1 is now a branch a test drives.** "A guard that exists rather than a guard
that has fired" was honest and was also unnecessary: nothing could make `relative_address`
return `None` on a host with `/proc` mounted, so the fallback bind and its `warn!` had never
executed anywhere. The address is
now a value — `BindAddress::{ThroughTheDescriptor, ByName}` — chosen by `bind` and *passed*
to `bind_listener`, which is the seam the `Accepting` trait already is one function along.
`a_bind_by_name_still_serves_and_says_that_it_is_the_unprotected_spelling` drives both arms:
the fallback binds a socket a real `UnixStream::connect` reaches by D11's own path and emits
the `WARN`, and the ordinary spelling binds through the descriptor and says nothing.
Silencing the warning turns it red, so "never a silent downgrade" is checked rather than
stated. `MAX_UNIX_SOCKET_PATH_BYTES` needs no second arm: `bind` refuses on the length
*before* it chooses a spelling, so both spellings meet the same check. `/proc` being mounted
is still true of every host this suite runs on, and this is still a branch driven by a test
rather than one that has ever run in anger; what changed is that it is no longer written
blind.

**Retires when:** P4e lands the orderly exit. Unlinking on the way out is still not a
substitute for this — a daemon that is killed never runs its exit path — so the rule stays;
what changes is that the leftover becomes the exception rather than the rule. The
descriptor half of this entry has **retired**: what was "still open" is closed, with
residuals 1 — now a branch tests drive, per correction 3 above — and 2 as the honest
remainder.

### Amendment, 2026-08-11: an **inherited** socket is checked by name, and the unit file is what closes it

P4e-ii gave the daemon a second startup path, and this entry is where the residual belongs
rather than in a new note: the substitution defence is *this* entry's argument, and what
follows is a limit on that argument rather than a new law. (The alternative — a note of its
own — would have put the strong claim and its one exception in two places, which is how a
reader ends up quoting the wrong one.)

**What changed.** `daemon::systemd::Activation::adopt` takes a socket **systemd bound**, from a
`ListenStream=` string, before this process existed. There is nothing to *prepare*: the bind
already happened. So D11's question — "is the directory 0700 and ours" — is asked through the
same `check_mode_and_owner` the self-bound path uses (design §2.10's one home, reached by a new
`check_directory_mode_and_owner`), about the parent of the path `local_addr()` reports. An
abstract or unnamed address is refused outright rather than checked leniently: no directory, no
mode, no owner, reachable by anything in the network namespace that can spell the name, so
there is *nothing* to check rather than something this build cannot verify.

**What is gone on that path, exactly.** The `O_DIRECTORY | O_NOFOLLOW | O_PATH` open still
makes "is a directory, is not a symlink" the kernel's own refusal, and the mode and `st_uid`
are still read off the descriptor this daemon opened. What cannot be reproduced is the
*binding through it*: there is no second bind to perform, so what was checked and what is
served from are one **name** and not provably one inode. A directory swapped between systemd's
bind and this daemon's check is undetected. Substitution is **detected rather than defeated**
on the activated path — which is what the pre-P4d state of the self-bound path was, recorded
here so nobody reads this entry's headline as covering both.

**What closes it is not this daemon.** `packaging/systemd/wchd.socket` sets
`DirectoryMode=0700` (systemd's default is 0755) and `SocketMode=0600`, so the directory the
manager creates is never reachable by another account in the first place, and
`scripts/gates/systemd-units.sh` re-derives both rather than trusting the file. The daemon's
part is to be legible: it says at startup which of the two paths it is on, so an operator
reading the journal can tell a defended bind from a checked one. There is no way to do better
without a `bindat(2)` Linux does not have (the amendment above measured that), and no way at
all when the bind is another process's.

**Also settled by P4e-ii:** this entry's "**Retires when:** P4e lands the orderly exit" has
happened, and the answer is the one the entry predicted rather than the one it hoped for.
`daemon::shutdown` stops the daemon in an order and deliberately adds **no** unlink to it —
the exits that matter run no code at all, so a cleanup only the orderly path performs is one
the failing path cannot rely on. The leftover socket did not become the exception; the rule
stands exactly as written, and `crates/daemon/tests/signals.rs` now asserts the file is still
there after a real `SIGTERM` has been drained.

---

## N40 — D13's `StoreLocked` gained a second field, because the refusal D9 writes turns on a fact the error could not carry

**Doc:** design D9 gives the state directory two locking protocols and one sentence for the
collision — "the daemon holds it exclusively for its lifetime; a daemonless `wch` takes it
per mutating operation; `wch` finding it held reports *daemon owns the state (and likely the
camera) — use wchc* rather than corrupting or blocking (D13)". docs/7 P4b lands "the
state-dir lock held for the daemon lifetime and `wch`'s held-lock refusal". D13 is settled
at nineteen variants and every code is a compatibility contract
(`crates/api/fixtures/d13-rpc-codes.tsv`).

**Repo, before:** the refusal existed and the sentence did not. `Error::StoreLocked
{ holder }` rendered *"the state directory is locked by wchd (pid 909)"*, and the strings
"use wchc" and "daemon owns the state" appeared nowhere outside the two documents. The fact
that would justify the sentence was read and then thrown away: `LockRecord` carries
`protocol: LockProtocol`, `LockRecord::holder()` narrows it to `schema::Holder { pid, comm }`,
which has no such field. So a `wch` could not tell "a daemon holds this and will until
somebody stops it" from "another `wch` is four milliseconds into `calibrate select`" — which
is exactly the distinction `LockProtocol`'s own doc comment says the type exists to record.

**Repo, after:** `LockProtocol` is defined in `webcam-handler-schema`'s error registry and
re-exported from `webcam-handler-engine::store`; `Error::StoreLocked` carries
`protocol: Option<LockProtocol>` beside `holder`; and `LockProtocol::advice` is the one home
of both sentences. The daemon takes the lock through `daemon::state::OwnedState`, whose value
*is* the lifetime.

**Why the fact went on the error rather than anywhere else.** Three homes were possible and
they are not equivalent.

- *In `crates/cli`.* One `wch` renders the sentence from a `store.holder()` it consults on
  the error path. Two homes for one law the moment `wchc` needs it, and a TOCTOU window:
  the holder may have exited between the refusal and the second look.
- *In `cli-core`, keyed on `ErrorKind::StoreLocked`.* One home, and wrong: T4's renderer is
  shared with `wchc` (design §2.7), so `wchc` would advise its user to go and start `wchc`.
  P4f's parity gate compares the two binaries' output byte for byte, which is where that
  would surface — one sub-milestone after the sentence shipped.
- *On the error.* The fact crosses the wire with the refusal, `wch` and `wchc` render the
  same words without either of them containing any, and the sentence is a `const fn` with an
  exhaustive `match` — so a third protocol cannot be added without answering "and what do you
  tell somebody who meets this one?".

**What it cost, exactly.** `LockProtocol` moved crates (`schema` cannot depend on `engine`,
so a wire-carried vocabulary has to live in `schema` — the same argument that put `Holder`
there); `Error::sample(StoreLocked)` gained the field; `just generate` rewrote both artifacts
under `schemas/`. What did **not** move: the code (`-32026`), the fixture that pins it, the
variant count, and every walk anchored on `ErrorKind::ALL`. This is a variant's *shape*
changing, not the registry's membership, so "nineteen variants / nineteen codes" is still
nineteen. The new field is an `Option`, and serde reads a missing `Option` as `None`, so a
document written by a build without it still deserializes.

**Both arms are load-bearing, and both are asserted.** `HeldForLifetime` renders D9's
sentence verbatim, parenthetical and em dash included; `PerOperation` renders "it is held for
one operation and will be free shortly" and must **not** mention `wchc`, because sending
somebody after a daemon that does not exist is worse advice than none. An unreadable lock
record yields `holder: None` *and* `protocol: None` — the two fall together on purpose,
since advising a switch to `wchc` on the strength of a record we could not read would be
inventing the one fact the advice turns on. `crates/daemon/tests/lock.rs` drives both
orderings against a real `wchd` process; `crates/cli/tests/calibrate.rs` drives the same two
through the `wch` binary, where a person reads them.

**The trap this leaves for a reviewer.** The daemon holds the lock for its whole life, so it
must never call `SessionStore::with_lock` — that is D9's *daemonless* protocol and it would
ask for a lock the process already holds. `daemon::state`'s module doc says so, and the
mutating verbs at P4c are to pass `OwnedState::lock` instead. Nothing in the tree can express
the mistake as a type error yet; if P4c finds a way to, take it.

**Retires when:** the registry gets a way to attach advice to a variant generically, or D9
stops distinguishing the two protocols — neither of which is on any plan.

---

## N41 — The camera actor is the engine's, and its reply channel is the caller's

**Doc:** design §2.1 puts the actor in the engine — "the engine owns each open camera
through a dedicated OS thread (the *camera actor*) … the daemon's async tasks and the
direct CLI both talk to the same actor API" — and §2.4 lists "camera actors (D12)" among
the engine's imperative shell. Design §2.8's crate inventory gives the engine
`schema + imaging + tempfile + fd-lock + tracing` and **no runtime**, while the daemon's
line carries tokio. `crates/api/src/lib.rs` states how a request reaches it: "The daemon's
implementation hands the request to the camera's actor thread (D12) and awaits a reply;
`#[method(blocking)]` would put a minutes-long sweep on a tokio blocking thread *and* still
queue it behind the actor, which is two queues for one device."

**The tension, stated plainly.** Those three sentences cannot all be satisfied by an actor
that names a channel type. A `tokio::sync::mpsc` reply channel puts a runtime in the engine,
against §2.8. A `std::sync::mpsc` one is worse than a missing feature: the daemon's handler
would have to park a thread on `recv()` — `spawn_blocking` included — which is the *same*
two-queues-for-one-device that `crates/api`'s doc rejects `#[method(blocking)]` for. A
scratch reading also existed, and one of the contract documents written for this
sub-milestone recommends it: put the actor in `crates/daemon` and let it use tokio freely.
That contradicts §2.1 and §2.4 twice over, and it would put the actor out of reach of `wch`,
which links the engine and not the daemon.

**Repo:** `engine::actor` resolves it by naming no reply channel at all.
`CameraActor::submit` takes `FnOnce(Result<OpenCamera<'_>>) -> Answering + Send + 'static` —
work that is *given* the device and closes over whatever the caller answers through. The
daemon's handler closes over a `tokio::sync::oneshot::Sender` (whose `send` is synchronous
and non-blocking, which is exactly what a blocking thread needs) and awaits the receiver, so
a request waiting on a sweep occupies no thread anywhere. `CameraActor::ask` closes over a
`std` channel for callers that have a thread to spare. One actor API, two transports, and
the engine keeps its dependency list.

The work *returns* its answer rather than sending it, and that is the one piece of ceremony
in the shape. The actor publishes "between commands" between the two, so a caller holding a
reply is holding a status that already accounts for the command that produced it — without
the split, a handler could receive its answer, ask `Cameras::activity`, and be told the
actor was still inside the command it had just answered. That is not cosmetic: it is what
`CameraActor::sweep` reads to decide whether asking the actor would mean waiting for it
(below), and a housekeeping pass that acted on a stale reading would skip a camera that had
in fact gone quiet.

**What that buys, beyond the manifest.** The serialization D12 calls "by construction" is
literally that: the `Box<dyn Camera>` is a local variable inside one thread, the only way to
touch it is a closure that thread runs, and `OpenCamera<'device>` is a borrow the closure
cannot outlive. There is no accessor to misuse and no lock to forget. `engine::actor::Cameras`
is the other half — one actor per `CameraId`, so two requests for one camera reach one
thread rather than two descriptors on one node.

**The type alias is load-bearing, not decoration.** `OpenCamera<'device>` is written
`&'device mut (dyn Camera + 'static)`. Inside a reference the default trait-object lifetime
is the reference's own and `&mut T` is invariant in `T`, so the elided spelling names a type
that a `Box<dyn Camera>` cannot coerce into — and every closure a caller wrote would fail to
compile for a reason that reads like a mistake in the caller.

**Why every command carries the time.** Idle close is a deadline, and a deadline read from a
clock inside the actor is a deadline no test can reach without waiting, which this project
bans in tests as much as anywhere. So the actor reads no clock: the caller stamps each
command, the same doctrine `engine::settle` states for the settle policy ("the caller
supplies both, which turns *the deadline expired between these two frames* from a race into
an argument"). A shared stepped clock was never available anyway —
`engine::settle::SteppedClock` is deliberately not `Sync`, "a stepped clock shared across
threads is a race dressed as a fixture" — so this shape costs that decision nothing.
`Idle::used` takes the *later* of the two readings, because two handlers reading a monotonic
clock concurrently can reach the actor in the other order and the harm is one-directional:
an older stamp winning moves the deadline closer and closes a camera somebody is using.

**What the actor does when its thread dies.** The most popular V4L2 crate panics on a
control type this kernel emits \[PF:1\], so "a backend panicked" is measured, not
hypothetical. A `Liveness` drop guard falls during unwinding, `CameraActor::submit` answers
`Error::DeviceGone` (never `Error::Busy` — a thread that is gone is not a device that is
held, and E3 keeps those apart), and `Cameras::actor` hands out a fresh actor next time. One
camera stops working for one request; the daemon does not.

The guard carries the *published state* as well as the dead flag, and that pairing is the
P4b review's, not the original design's. Unwinding drops the `Box<dyn Camera>` — the
descriptor is gone as the thread starts leaving — but `Thread::publish_closed` is only
reached from a `Sweep` a dead thread will never process, and `Cameras::activity` lists every
actor whether its thread is alive or not. So the first version reported `open: true` about a
camera this process had already released, and went on reporting it until some later request
happened to replace the actor: measured, `closes() == 1` and
`activity() == [CameraActivity { open: true, .. }]` at the same instant. Both flags now fall
in the guard, after the local `Box` (locals unwind before the parameter that owns the guard),
so the claim falls after the fact it describes rather than before it.

**Why the housekeeping pass reads that published state instead of asking.** `Cameras::sweep`
walks the actors one after another, and `CameraActor::sweep` originally enqueued a `Sweep`
command and blocked on the acknowledgement, short-circuiting only when the queue was
*full*. So one camera mid-command held the whole pass: reproduced with two cameras, one
given a command that does not return, and the other — open and past its deadline — still
open a second and a half later, with the pass still inside `recv()`. A device command may
take minutes by design (P4c's calibration sweep) and may never return at all (a `DQBUF`
against a driver that has stopped delivering), so that is an unbounded wait on a
request-driven path. The actor now publishes `busy` alongside `open` and the deadline, and
`CameraActor::sweep` answers `false` from that mutex unless the camera is open, expired
*and* quiescent. The published state is a filter, not the decision — the actor re-checks the
deadline before dropping anything — and the one case it cannot exclude is a command that
arrives in the few instructions between the read and the send, which is a caller who by
definition wants the camera.

**Retires when:** the engine acquires a runtime for another reason, or `wch` stops opening a
camera per invocation and becomes the second consumer §2.1 describes — at which point the
two-transport shape stops being a prediction and starts being load-bearing in two places.

---

## N42 — "Observable via the status surface" is the actor registry, and D12's `wait` flag has no producer until P4c

**Doc:** docs/7 P4b promises "open/idle observable via the status surface and tested";
rubric B6 spells it "both observable via the status API and tested". D12 also says "a second
capture request queues or is refused with `Busy` per its `wait` flag", and rubric B3 makes
that a review item.

**Neither surface exists, and one of them must not be invented here.** T5 has nineteen
methods and none of them is a daemon status; `calibrate_status` is a *session* document;
`schema::report::CameraList`/`CameraInfo` carry no "open" field. Adding `wch_status` would be
a twentieth method, which `the_trait_registers_the_nineteen_wch_methods_and_nothing_else`
(renamed at P4e-i to `the_surface_registers_the_nineteen_methods_and_the_two_subscriptions_and_nothing_else`, note N57)
turns red on purpose (note N29) and which no sub-milestone in docs/7 authorises. A
`sd_notify(STATUS)` camera count is P4e's and could not be asserted from a test anyway.

**Repo:** the surface is `engine::actor::Cameras::activity`, a library accessor answering one
`CameraActivity { camera, open, last_used_ms }` per actor in `CameraId` order. Not a serde
DTO: it is not on the wire, and a `schema::report` type with `JsonSchema` derives would land
in the committed schema bundle as a document no method produces — rubric A8's defect wearing
the costume of thoroughness. `crates/daemon/src/lib.rs` exists "so integration tests can
drive a real server", so the daemon's own tests reach the same value through the registry it
holds.

The claim is asserted twice over, and the second is the one that can go red for the right
reason: the *registry's* bookkeeping says `open: false`, and the *fake backend's*
`FakeBackend::opens()`/`closes()` counters say a descriptor went away. An actor that decided
to close without dropping the handle passes the first and fails the second — which is rubric
B3's "actor shutdown mid-stream releases the device (fd closed — asserted)" in the only form
P4b can take it, since nothing streams yet. The counters are `streams_started`'s argument
applied to a second claim: a caller holding a `Box<dyn Camera>` cannot ask a `FakeCamera`
anything, so an observation of the double is the only place the fact can come from, and an
observation is not a capability (E5).

**The `wait` flag, and what actually landed.** P4b routes no capture verb, and nothing in the
schema is named `wait` — landing the flag here would be a typed declaration nothing reads,
which is the row that convicted `Session::pre_snapshot` at G3 (note N23). What landed is the
half that is real without a consumer: the actor's command queue *is* D12's queue, bounded by
`limits::CAMERA_COMMAND_QUEUE_DEPTH`, and a caller arriving past it is refused with
`Error::Busy` — asserted deterministically, by holding the actor's one thread and filling the
queue behind it. **The obligation P4c inherits** is the flag that chooses between the two:
`wait: true` must wait for room rather than take this refusal. This entry is that obligation,
in note N34's shape, so the G4 review checks it off rather than rediscovering it.

**One thing the `Busy` refusal deliberately does not carry.** Its `holders` list is empty.
The list is filled by the `/proc/*/fd` walk that answers "which *other* processes have this
node open"; here the work in the way is this daemon's own, and the field feeds
`terminate_holder` — naming this process's pid would invite a client to kill the daemon it is
talking to.

**Also not here:** the periodic driver. `Cameras::sweep(at)` is the housekeeping pass and a
test runs it at the millisecond it wants to talk about; the daemon runs it on a cadence from
the composition root that owns a registry, which is the sub-milestone that routes the read
verbs. Until then `limits::CAMERA_IDLE_CLOSE_MS`'s reader is `Cameras::new`, which is the
shipped default rather than a number the daemon repeats.

**Where it stands after P4c routed `wch_photo`, and the flag's re-deferral.** The capture
verb is routed, so the half of D12 that exists is now reachable over the wire and asserted
there: two photo requests in flight against one camera reach one actor, share one
descriptor, and produce two *sequential* streams — the fake refuses a second `start_stream`
while one is running (its own resemblance suite pins that both ways), so "both answered" is
the assertion and a handler that had opened its own descriptor could not pass it.

The **flag itself is re-deferred to P4e**, deliberately and with a reason, because
discharging it here would have been three changes wearing one name:

1. *A committed wire shape.* Nothing in the schema is called `wait`; `PhotoRequest` is
   `{stream, settle, transform, sink}`. Adding a field moves
   `schemas/webcam-handler-schema.json` **and** `schemas/webcam-handler-openrpc.json`, which
   is the class of edit the P4a methods spec told D-4 and D-5 to keep out of a ride-along
   commit — and docs/7 P4c's own "Lands" sentence never names `wait`. Only D12 and this
   entry do.
2. *New actor machinery.* `CameraActor::submit` is a `try_send` on a bounded
   `SyncSender` and has no blocking-with-deadline path at all, so `wait: true` is not a
   branch — it is an enqueue that waits, plus the bound AGENTS requires of anything that
   waits, in `engine::actor`, from a sub-milestone whose subject is a transport.
3. *A command-line spelling, or an argued absence of one.* `wch photo --wait` is a T4 verb
   change, and P4c has no CLI budget; declaring the flag wire-only would be a second
   spelling of a request field, which §2.10 dislikes.

The A8 objection that kept it out of P4b does not reappear at P4e: by then a capture verb
has a producer, and P4e already owns the concurrency semantics of a client that goes away
mid-operation, which is the same question from the other end.

What is *not* deferred is the behaviour a caller meets today, and the two halves of it are
asserted in two different places, which is worth saying exactly rather than in one breath.
**The queue** — a second request reaching the same actor and being served after the first
rather than beside it — is asserted **over the wire**, by the two-photos test above.
**The bound, and the refusal past it** — the ninth request for one camera while a command
holds the thread — is asserted at the **actor**
(`one_camera_runs_one_command_at_a_time_and_says_so_when_the_queue_is_full`) and nowhere
else, because filling a nine-deep queue over a socket needs the actor's one thread held from
inside a handler, which is a fixture for the sub-milestone that owns long-running requests
rather than one to build for a claim the engine already proves at the layer that makes it.
So a client that sends a ninth verb for one camera during a `calibrate_sweep` gets
`Error::Busy` with an empty holder list — availability, not capability (E3), and the empty
list is the paragraph below — and P4c is the first build where anybody can meet it. That
is stated here rather than tested twice.

**Discharged, in part, at P4e-i (2026-08-10).** The `wait` flag landed, with the enqueue
that honours it and the bound that enqueue needs. `schema::capture::PhotoRequest` gained
`#[serde(default)] wait: bool`; `engine::actor::CameraActor::submit_with` takes an
`Enqueue` — `Refuse`, which is what `submit` has always done, or `WaitUntil(Instant)`, which
parks the **caller's** thread until a place comes free; `limits::CAMERA_ENQUEUE_WAIT_MS` is
the shipped budget and `Enqueue::waiting` its one reader; and `daemon::server::enqueueing`
is the one place the field is read. Both artifacts under `schemas/` moved with the field, in
their own commit, which is what item 1 above asked for. Note **N56** carries the mechanism,
its three tests and the two things it does *not* buy.

The **third** item — a command-line spelling — is answered by its permitted alternative, an
argued absence, and the argument is in `cli_core::Command::photo_request` beside the `wait:
false` it writes: `wch` opens its own camera per invocation and runs one verb, so the queue
the flag chooses about is its own and always empty; the consumer where the flag is
meaningful is `wchc`, whose transport is **P4f's** (docs/7:341), and until that lands
nothing on a command line can reach a daemon at all. A `--wait` today would be a flag with
no producer and no reachable consumer, and would move `json-validates.sh`'s `--help`-scraped
population and P4f's parity population on the way (note N48's precedent) for a `--help` line
that had to say it did nothing here. It is a wire field until the surface that can mean it
exists.

**Checked off at the G4 boundary, 2026-08-11 (docs/7 P4g), and verified in the tree.** This
entry asked to be checked off "in note N34's shape", and the obligation it carries — D12's
`wait` flag — is **discharged in all three of its parts**, the third of which landed after
the paragraph above was written and is recorded here rather than left to docs/7 to assert on
this entry's behalf:

1. *The wire shape*: `schema::capture::PhotoRequest::wait`, `crates/schema/src/capture.rs:684`,
   `#[serde(default)]` with the doc at `:661` quoting D12's sentence. Both `schemas/`
   artifacts moved with it at P4e-i, in their own commit.
2. *The actor machinery*: `daemon::server::enqueueing` (`crates/daemon/src/server.rs:1168`) is
   the one place the field is read, at `:1504`, and it is the only producer of an `Enqueue`
   other than `Refuse` in the daemon; `limits::CAMERA_ENQUEUE_WAIT_MS` bounds the wait and
   `limits::CAMERA_ENQUEUE_WAITERS` bounds how many callers may hold that budget (note N59).
3. *The command-line spelling* — **the part this entry's text above still describes as an
   argued absence, and which stopped being one at P4f.** `--wait` is a flag:
   `crates/cli-core/src/lib.rs:589`, on the **shared** T4 root because a verb exists once,
   reaching the request at `:1074`. Its `--help` says out loud that it is inert under `wch`
   (`:584–587`) rather than leaving a user to find out. Three assertions hold it, all in
   `crates/cli-core/src/lib.rs`: `the_wait_flag_reaches_the_request_and_is_absent_from_it_by_default`
   (`:2073`, run green for this check-off), `wch list --wait` refused at `:2104` because the
   queue the flag chooses about is a camera's one thread and no other verb takes a capture
   through it, and `:2116` requiring the string in **both** roots' `--help`. No committed
   artifact moved, which is `schema-artifacts-current.sh`'s finding rather than N42's
   prediction.

**What is left of this entry.** Only the daemon-status half: the actor registry is still
`engine::actor::Cameras::activity` and still not on the wire. Rubric B6's row says "both
observable via the status API and tested", and the surface that answers it is a library
accessor — a deliberate refusal to invent a twentieth T5 method, argued above, and an input
the G4 rubric reconciliation has to price rather than inherit.

**Retires when:** a daemon status reaches the wire — which is a T5 method, so a docs/7
sub-milestone has to want one. The `wait` half retires here.

---

## N43 — The thirteen unrouted T5 methods answer `Unimplemented`, and the producer is pinned the way note N6 pins the first one

**Doc:** docs/7 P4b lands "read-verb routing (`list`, `info`, `controls`, `get`, `calibrate
status/list`)" and docs/7 P4c lands "the mutating half over RPC". D13 keeps
`Error::Unimplemented` for "a method whose phase has not arrived", and note N6 is the entry
that made it a schedule rather than an escape hatch: "`webcam-handler-v4l2::unimplemented_surface()`
is the one list of methods that answer it, and a test pins the list's size and contents".
N6 also schedules the variant's deletion at P4d.

**The gap, stated plainly.** The compiler forces the whole trait — `WchRpcServer` has
nineteen methods and the generated `into_rpc()` registers all nineteen — so on the day P4b
lands, thirteen methods are registered, reachable from the socket, and answering something.
No document in the series says what. Leaving them to answer *nothing in particular* is not
available: whatever they do is what a client sees.

**Repo:** they answer `Error::Unimplemented { operation, arrives_in: "P4c" }`, produced by
`daemon::server::unimplemented` and inventoried by `daemon::server::unrouted()`.

| Candidate | Why it lost |
|---|---|
| **`Error::Unimplemented` (chosen)** | Literally D13's stated purpose. It names the operation and the phase, says *this build* rather than *this device*, and is neither a panic \[PF:1\] nor the kernel's fault (`DeviceIo`) nor a capability lie (`FormatUnsupported`, which E3 forbids here). |
| Route all nineteen at P4b | Contradicts docs/7 P4c and note N30, and P4c is not empty work — sink handling, `is_addressable`, `bytes_match_the_delivery`, `HolderGone`, N34's `DiscoveryReport` move, the method-count walk. Re-scoping the plan is not a sub-milestone's decision. |
| Register only six | Requires bypassing `into_rpc()` or calling `RpcModule::remove_method` — a second registration path, which is the thing D10 exists to prevent, and against `crates/api`'s statement that a real `RpcModule` built by `into_rpc()` is the only authoritative account of the surface. It would also answer `-32601 Method not found` for a method that *is* on this surface, which is a lie a client would cache. |

**Why this is not the producer N6 warned about.** Three things, and the third is the one that
matters:

1. **Chronology.** N6 schedules the variant's deletion at **P4d**; this producer is gone at
   **P4c**, one sub-milestone earlier. Note N29 rejected an earlier stand-in on exactly this
   ground — "P4a would be adding the producer P4d is removing" — and that argument does not
   reach a producer that predeceases the deletion.
2. **It is pinned, in N6's own shape.** `unrouted()` is the second instance of
   `unimplemented_surface()`: one list, size and contents asserted, and P4c cannot land
   without emptying it.
3. **The list is derived, not transcribed.** `ROUTED` is the pin — six wire names, in the
   tradition of `fixtures/d13-rpc-codes.tsv` — and `unrouted()` is `api::METHODS` minus it.
   So the two halves cannot disagree, a twentieth method cannot fall into neither, and the
   population comes from the wire surface's own declaration (note N28) rather than from a
   list somebody remembered to update. `wch_discover_pairs` is on the unrouted side, which is
   note N30's split holding.

**What is asserted, and where.** `daemon::server`'s tests check the partition against
`api::METHODS`, check that every row renders as a D13 refusal naming its operation and its
phase, and call every method still on the unrouted side with arguments that would be valid —
so the refusal comes from the body rather than from parameter parsing, and a body naming the
wrong operation is red. `crates/daemon/tests/read_verbs.rs` then checks that one of them
survives the wire as `-32030` with its payload intact, recovered client-side through
`api::codes::typed`.

**Where it stood.** P4c routed in steps. The first moved the five control-shaped mutating
verbs across (`set`, `snapshot`, `restore`, `discover_pairs`, `profile_capture`); the second
moved `photo`, leaving seven — `wch_terminate_holder` and the six `calibrate_*` verbs that
write. Nothing about the mechanism changed while that lasted: the pin was the routed list,
the map was derived from it, and the assertions walked the whole of whichever half they were
about, so the counts moved in one diff or not at all.

### Retired at P4c, and what replaced the assertion

The third step routed the remaining seven. `unrouted()`, `unimplemented()` and
`ROUTING_PHASE` are **deleted**, and with them the three tests that walked them
(`the_routed_and_unrouted_halves_together_are_the_whole_wire_surface`,
`an_unrouted_method_names_itself_and_the_phase_that_lands_it`, and
`every_unrouted_method_refuses_with_its_own_name`).

**`ROUTED` stays**, and the claim inverted rather than went. While a method could be
unrouted the assertion was a *partition* — this list, plus `api::METHODS` minus this list,
covers the surface and overlaps nowhere — which is what stopped a twentieth method falling
into neither. With the second half empty what is left is the **equality**, and it is the
claim a client actually depends on:
`the_pinned_routing_is_the_whole_wire_surface_and_nothing_answers_unimplemented` asserts
`ROUTED` *is* `api::METHODS`, pinned at the nineteen `crates/api` pins the trait at (note
N29), with the set non-empty so two empty sets cannot compare equal and say nothing. A
twentieth method breaks it just as loudly as it broke the partition.

Two suites moved with it. `read_verbs.rs`'s fifth refusal — a registered method answering
`-32030` with its phase intact — is **gone**, because there is no such method; the claim it
carried is now `calibrate_verbs.rs`'s `no_calibrate_verb_answers_store_locked_or_unimplemented`,
which is the same assertion over a walk of the whole calibrate half rather than over one
verb standing for the rest.

**Retires when:** now. The only place in the **shipped** tree where this variant is still
the answer to a caller is `webcam-handler-v4l2::unimplemented_surface()`'s one row,
`CameraBackend::watch`, which P4d deletes along with the variant — N6's scheduled death,
with both surfaces empty by then as planned.

Two other constructions of the variant exist and are neither producers nor obstacles,
counted here so that the sentence above stays exact rather than approximately true (a grep
finds all three):

- `schema::Error::sample(ErrorKind::Unimplemented)` — the representative value the
  registry walks (`Error::sample`'s own doc: "the RPC code mapping, the CLI renderer, and
  the schema emitter all need a walkable population"). It is a value of every kind by
  construction, not a refusal anybody receives, and it goes when the kind does.
- `engine::profile`'s `StubCamera`, inside that module's `#[cfg(test)]` block, which
  answers every `Camera` method it is not testing with
  `Unimplemented { operation: "StubCamera", arrives_in: "never" }`. A test double saying
  "not this one" is not a phase schedule; `arrives_in: "never"` is the double saying so.
  It predates P4c (it is in the tree at P4b's commit) and P4d will have to give it a
  different refusal when the variant goes.

### Amendment, 2026-08-10: what P4d did with the two constructions above

Both are gone, in the two different ways this entry predicted, and N6's retirement stanza
carries the reasoning. The `sample` arm went with the kind. The `StubCamera` answers
`Error::IllegalTransition { from: "stub_camera", op }` — the family N46 already picked that
variant for, with the method attempted in `op` so the double names *which* thing it does not
do rather than only that it does not.

One sentence in the section above is now history rather than instruction: the claim that
`read_verbs.rs`'s fifth refusal "is now `calibrate_verbs.rs`'s
`no_calibrate_verb_answers_store_locked_or_unimplemented`" no longer holds, because that
half of the walk stopped compiling with the variant. The test is
`no_calibrate_verb_answers_store_locked` and the absence it used to check is now structural.
The transcript above stands as what was true at P4c.

---

## N44 — The other-uid half of the UDS-permissions row is a shell predicate, because a Rust test cannot report a skip that CI counts

**Doc:** docs/9 Part 2's **UDS permissions** row (Phase P4b) reads "startup assertion +
test: socket dir 0700, socket unusable by a scratch other-uid check where CI permits, else
a named skip", and docs/9 Part 1 says compiled predicates — "Rust tests standing in for a
shell gate" — are "deviations to record in the implementation notes where they happen, not
silent exceptions". This entry records the deviation in the other direction: the row's two
halves landed in two different places, and only one of them is a `#[test]`.

**Repo:** the 0700 half is `daemon::uds`'s unit tests, both directions
(`the_socket_directory_is_created_private`, and
`a_group_readable_socket_directory_is_refused_rather_than_repaired`, which also asserts the
directory is unchanged afterwards). The other-uid half is
`scripts/gates/uds-permissions.sh`, with `scripts/gates/cases/uds-permissions.cases.sh`
proving it red four ways.

**Why the split.** A second uid needs a second account and a non-interactive `sudo`, and
neither is a property of the code — they are properties of the host. nextest has no
runtime-skip concept, so a test that declines a claim on a host that cannot arrange it
*passes*, and `just ci` runs `cargo nextest run` without `--success-output final`, which
means even a `println!("SKIP: …")` is invisible in the one run that matters. That is
"skip == pass, in a costume" (docs/8 Part C) and AGENTS rule 3 forbids it. `gate_skip`
prints the reason and `gate_finish` counts it, which is the whole difference. The shipped
precedent is `privileged-helper.sh`'s blessed-copy arm, which declines the same way for the
same kind of reason.

**The trap this predicate walked into, and how it is closed.** The obvious form of the
check — "another account cannot traverse the socket directory" — passes for the wrong
reason on every machine, because `mktemp -d` creates the scratch directory **0700**: the
second uid is stopped one or two levels above the directory under test and the assertion
never reaches its subject. So the gate widens everything it owns above the socket directory
to 0755, and then makes the *reachability of the parent* a precondition it checks first: if
the second account cannot traverse the runtime directory, the arm is a counted skip, because
a refusal one level further down would prove nothing. This is the same defect class as
note N10's — a check that is green while checking less than it claims — found before it
shipped rather than after.

**What the predicate drives, and its one honest limit.** It runs a real `wchd` against a
scratch pair of XDG directories and learns from the daemon's own stderr that it is serving,
which is `crates/daemon/tests/lock.rs`'s synchronisation and not a clock; `timeout` is a
watchdog that turns "hangs" into "fails". It then inspects the directory *after* stopping
the daemon, which is sound precisely because this build never unlinks its socket (note N39):
the directory and the socket file are exactly as the daemon left them, and a mode is a
property of a directory rather than of a process. `$WCH_GATE_WCHD` is the documented seam —
the daemon-shaped program to drive — so the failing arms can be daemons that get D11 wrong
in one way each while `pass_case` still drives the shipped binary (rubric rule 6, note N10).
The limit: on a runner with no second account and no passwordless `sudo` the arm is a skip,
and a counted skip is still not a run — the same sentence docs/9's gaps register already
carries for the Playwright rung.

**Retires when:** never, as a rule; the skip retires on a runner that offers a second uid.
P5a's token-enforcement row is the next thing to check against it, since the TCP listener's
auth model is a token rather than a directory mode and the two must not be conflated.

---

## N45 — The idle deadline is stamped when a command *starts*, which only a command longer than the timeout can tell

**Doc:** design D12 — the daemon "never opens a camera until first use and closes on idle
(configurable)". `schema::limits::CAMERA_IDLE_CLOSE_MS` prices the timeout against the cost
of re-opening: "every `wchc get` in a shell loop pays a fresh `open` and the driver's
first-frame settle \[PF:11\]".

**Repo:** `engine::actor::Thread::device` publishes `Idle::used(at)` before handing the
device to the caller's closure, and nothing records a second use when the work finishes. So
`Idle::last_used_ms` means *when the last command was issued*, not *when the device was
last touched*, and a command that runs for longer than the timeout is idle by its own
deadline the moment it returns.

**Why it is written down rather than fixed at P4b.** Nothing this build routes can take
thirty seconds: the six read verbs are an enumeration, a format walk and two control reads.
The first command that can is **P4c's `wch_calibrate_sweep`**, which is bounded by
`MAX_SWEEP_SAMPLES` settles and photos and is measured in minutes — at which point a sweep
that completes is followed by an idle close within one cadence, and the client's next verb
pays a fresh `open` plus the driver's first-frame settle, every time, for every long
operation.

**What a fix costs, which is the reason it is a decision and not a tidy-up.** The actor
reads no clock by design (note N41, and this module's header states the doctrine at
length), so "when did the work finish" is not a number it has. Re-stamping with the same
`at` changes nothing — `Idle::used` takes the later of the two readings and `at` is the
command's *start*. The two shapes that would work are: give the actor a clock, which
reverses a stated decision and needs `SteppedClock` to become `Sync` (it is deliberately
not — "a stepped clock shared across threads is a race dressed as a fixture"); or have the
actor remember that a `Use` completed since the previous `Sweep` and decline the first
sweep after one, which is clock-free but grants every command one extra cadence of life and
therefore has to be argued against the timeout rather than bolted beside it.

**Owned by:** **P4c**, with the verb that makes it visible, and with the both-directions
test the deadline already has — a command shorter than the timeout still closes on the
first sweep past it, a command longer than the timeout does not close immediately after it.

### Discharged at P4c, by the clock-free shape

**What landed: shape B.** `engine::actor::Live` gained one boolean, `used_since_sweep`.
`Thread::publish` raises it under the same mutex as `busy`, before the work runs;
`CameraActor::sweep`'s published-state filter takes it and declines *once*;
`Liveness::drop` lowers it with `open` and `busy`, for the same reason those fall on an
unwind — a dead actor claiming a command had just finished would postpone one pass on a
camera this process no longer holds.

**Why shape A lost, measured against the tree rather than against the note.** Giving the
actor a clock reverses this module's central decision (N41, and `engine::actor`'s header
spends two paragraphs on it) — but the disqualifying cost is smaller and sharper than that:
`engine::settle::Clock` has no `Send + Sync` bound, `SteppedClock` is **deliberately** not
`Sync` ("a stepped clock shared across threads is a race dressed as a fixture"), and a
`MonotonicClock` the actor constructed itself would put the post-command stamp somewhere no
test can reach. So the fix would have been untestable without either a new thread-safe clock
type and a widened bound at every `Clock` call site, or a test that waits — and waiting is
the rule that produced the caller-supplies-the-time design in the first place. A fix whose
proof needs the thing the design exists to avoid is the wrong fix.

**The ordering inside the filter is the whole reason a short command costs nothing.** The
flag is consulted *before* the deadline, so the ordinary passes that happen between a short
command and its deadline spend the grace and there is none left when the deadline arrives —
`an_idle_camera_closes_and_the_next_use_opens_it_again` keeps passing unchanged, which is
N45's first direction. A long command gets no such passes (every one of them saw `busy`),
so its grace is still there for the pass that follows it, which is the case this entry is
about. The flag is read in the *filter* rather than in the thread because it is published
under the mutex the filter already takes: reading it there costs nothing and does not turn
every long command into a queue round trip, which is the property `CameraActor::sweep`'s own
paragraph exists to preserve.

**The cost, argued against the timeout rather than bolted beside it — and it is one *pass*,
not one timeout.** The first version of this paragraph, and of `CAMERA_IDLE_CLOSE_MS`'s doc,
said the effective close became "30–35 s measured from when a command *ends*". That was
wrong by a factor of six, and the P4c review caught it: `used_since_sweep` is a boolean taken
by `std::mem::take`, so a completed command buys exactly one declined pass — one
`CAMERA_IDLE_SWEEP_MS`, five seconds — and for a command *longer* than the timeout the
deadline is already long past when it returns, so the camera closes on the **second** pass
after the command ends, five to ten seconds later rather than thirty to thirty-five. The
committed test says so and always did (`sweep(9_000)` declines, `sweep(9_001)` closes,
against a 1 000 ms timeout); the prose had drifted away from it in the one paragraph a reader
would go to for the number.

What the shape actually buys, stated so it can be argued: a long command gets **one whole
housekeeping cadence** in which a client that is still working can issue its next verb,
instead of an immediate close on the very next pass. That is the failure this entry was
opened for — "a sweep that completes is followed by an idle close within one cadence" — and
one cadence of grace is what removes it. A client that pauses longer than that still pays a
fresh `open` and the driver's first-frame settle \[PF:11\], exactly as a client that pauses
thirty seconds after a *short* command does; the two are then the same rule with the same
five-second slack rather than two rules.

Buying a whole timeout back was considered and rejected twice over. Making
`used_since_sweep` a counter of `CAMERA_IDLE_CLOSE_MS / CAMERA_IDLE_SWEEP_MS` passes would
put the daemon's *cadence* inside the actor, which is a number the actor is never told —
`Cameras::with_idle_timeout` takes the timeout and nothing takes the cadence — so the grace
would become "however often somebody happens to sweep", which is worse than a small grace:
`an_idle_camera_closes_and_the_next_use_opens_it_again` sweeps twice and would never close.
Re-stamping `Idle` from the declining pass's own `at` is clock-free and exact for a long
command, and it postpones a **short** command's close by a whole timeout as well, for a case
that never needed it. The one-pass shape is the one that costs a short command nothing, and
`CAMERA_IDLE_CLOSE_MS`'s and `CAMERA_IDLE_SWEEP_MS`'s docs now price the two cases
separately rather than in one sentence that is true of neither.

**The tests, both directions, nothing waiting for anything.**
`a_command_longer_than_the_timeout_is_not_closed_by_the_pass_that_follows_it` stamps a
command at 0 and sweeps at 9 000 against a 1 000 ms timeout — nine times the timeout, and
still open — then sweeps at 9 001 and it closes, which is the "one cadence, not a second
timeout" half. `a_long_commands_grace_is_one_sweep_cadence_and_not_a_second_timeout` is the
arm the corrected accounting owes: the same shape driven by the two *shipped* constants, so
the sentence "five to ten seconds, not thirty" is asserted against the numbers it is about
and goes red if either the shape or the constants move under it. `a_pass_that_runs_during_a_long_command_does_not_spend_its_grace` holds the
actor's one thread with a command the test releases by channel and takes four passes while
it is provably held, because a pass that consumed the grace mid-command would leave N45
undischarged on the only verb it is about. Both fail against the unfixed actor, measured by
neutering the filter's take.

Two daemon tests moved with the behaviour and say so where they are:
`an_idle_camera_is_closed_by_a_pass_at_the_millisecond_it_is_given` now asserts the
declining pass before the closing one, and
`the_driver_the_daemon_spawns_closes_an_idle_camera_with_nobody_asking` spends the grace
itself so that what it asserts about the *driver* is still one tick and one close.

**Retires when:** now. The entry stays as the record of which shape was taken and why.

---

## N46 — A photo's sink is refused with `IllegalTransition`, twice, and the rule lives on the type both times

**Doc:** D13 is a **closed** registry — `ErrorKind` and its `ALL` are generated, and "a
twentieth variant does not compile until the round-trip, rendering, and RPC-code walks all
know it" — so a refusal with no obvious variant is a decision to record rather than a
variant to invent. Two of them arrived together when P4c routed `wch_photo`, and no document
in the series says which variant either should be.

**The two refusals.** Both are about a `schema::capture::Sink`, and both exist because a
`Sink` can now be built by something other than `cli_core::Command::photo_request`:

1. **A relative `Sink::ServerPath`.** `schema::capture::Sink::is_addressable` is the
   predicate (note N34); `wch` and `wchc` resolve `-o` against the caller's cwd before
   sending (D10), so only a hand-written client can produce one, and this daemon's working
   directory under systemd is `/`.
2. **An extension this build cannot write.** Debt D-1: `engine::photo::sink_format`
   documented its own correctness as "the CLI refuses it while building the sink", which
   stopped being true the moment a socket could build one — `wchd` links no `cli-core`, so
   `{"kind":"server_path","path":"/tmp/x.webp"}` produced JPEG bytes in a file named
   `.webp` and a `PhotoDelivery::Path` whose extension lied about its contents.

**The pick: `Error::IllegalTransition` for both.** The alternatives and why each lost:

| Candidate | Why it lost |
|---|---|
| **`IllegalTransition` (chosen)** | `cli_core::Command::photo_request` already used it for refusal 2, so this keeps one refusal with one spelling rather than making the same mistake answer differently depending on which surface met it. Its rendering — "cannot {op} from state {from}" — carries both halves a caller acts on: what was typed, and what this build accepts. |
| `FormatUnsupported` | Explicitly forbidden by E3 and by the sentence cli-core already carries: that variant is the *camera* saying what it cannot offer, and `.webp` is not the camera's fault. |
| `StorageIo` | Names a path, but implies the filesystem was consulted. It was not — both refusals happen before anything is opened, which is the point of them. |
| `DeviceIo` | Blames the kernel for a request nobody could honour, which is N6's argument applied here. |
| A new D13 variant | A design change to a closed registry, not an implementation choice, and one that would move the code registry, the round-trip walk and `fixtures/d13-rpc-codes.tsv` for two refusals that fit an existing variant. |

**The honest cost of the pick, stated rather than left to a reviewer.** `IllegalTransition`'s
own doc says "The calibration state machine refused a transition (design D8)", and neither
of these is the D8 machine. So the variant now carries two families: D8's transitions and
"the request names something this build cannot do". That is a widening, and it is the reason
this entry exists; the alternative was a nineteenth-variant edit to a closed registry in a
sub-milestone whose subject is a transport. If a later phase splits the variant, these two
call sites and `cli_core`'s are the population.

**Where each rule lives, which is not the same as where each is asked.** Both rules are on
the type, beside the variants they constrain:

- `Sink::writable_format()` owns the *decision* (`.png` is PNG, no extension at all is a
  JPEG) and the *refusal*, and it replaced both `engine::photo::sink_format` and the inline
  check in `cli_core::Command::photo_request`. Three callers now: `cli-core` while parsing,
  `daemon::server::addressable` before opening a camera, and `engine::photo::from_capture`
  where the answer is actually used. That third one is a backstop and not a duplicate — it
  is what a caller who did neither of the first two gets.
- `Sink::is_addressable()` stays a `bool`, and `daemon::server::addressable` spells the
  refusal. It is asked in exactly one place because it is a *transport* rule: `wch` cannot
  produce a sink that fails it, and putting the check inside `engine::photo` would make the
  daemon's "no camera was opened" assertion pass for the wrong reason.

**Retires when:** never, as a rule — but the widening above retires if D13 ever grows a
variant for "the request asks for something this build does not do", at which point the
three call sites move together.

---

## N47 — A daemon holding one `flock` is not serialized against itself, so the session edit is serialized in the process

**Doc:** design D9 gives the state directory one advisory lock and two protocols over it —
"the daemon holds it exclusively for its lifetime; a daemonless `wch` takes it per mutating
operation". `engine::store` expresses "under the lock" as a `&StoreLock` argument on every
mutating method, and `engine::lifecycle::session_to_update` extends the same proof to the
*read*, because a read-modify-write whose read is outside the hold is only half protected.
The doc on that function records the defect it was written for: "two concurrent `wch`
processes whose windows overlapped could both exit 0 with one of them silently
republishing the other's document without its samples."

**The gap the wire crossing opens, which the in-process path did not have.** `flock` is a
property of an *open file description*, and it does not exclude its holder from itself. A
`wch` process takes the lock per operation, so two of them genuinely serialize. `wchd` takes
one at startup and holds it until it exits, and every request task legitimately has a
`&StoreLock` — so nothing in D9 stops two of them interleaving. The per-camera actor does
not close it either: `wch_calibrate_plan { order: true }` and `wch_calibrate_select` open no
camera at all (reordering a queue is an edit to a document, and the values a selection
chooses between were photographed during the sweep), so they touch no actor and nothing
orders them against anything.

Concretely, and this is the shape the test drives: a `wch_calibrate_sweep` commits a sample
per value from inside the actor closure, each commit publishing a draft cloned from the
sweep's own in-memory `Session`. A `calibrate_plan --order` arriving mid-sweep reads the
document, permutes the queue and publishes — and the sweep's next commit overwrites it with
a document whose queue predates the edit. Both clients get `Ok`; one of them is lied to.

**Repo:** `daemon::server::Inner::sessions` is a `tokio::sync::Mutex<Arc<StoreLock>>`, and
`Wchd::editing_sessions` is the only way to reach the token. That shape rather than a
`Mutex<()>` beside the token is the whole design: the guard *is* the right to edit, so a
handler that wants the lock has already taken the exclusion by the time it can name one.
There is no second path — `daemon::state::OwnedState::token` hands its `Arc` to `Wchd::new`
and to nothing else.

| Candidate | Why it lost |
|---|---|
| **A `tokio::sync::Mutex` holding the token (chosen)** | Structural: the type makes "one session edit at a time" the only spelling of a session edit. Awaiting it parks no thread, so a request behind a minutes-long sweep is a suspended future exactly like one behind a camera's actor. Lock ordering is one-directional — this first, the actor's queue second — so there is no inversion to reason about. |
| Refuse rather than wait, as `wch` does | This is the *parity* answer and it is unavailable: the refusal `wch` gets is `StoreLocked` naming the holder, and the holder here is this daemon. `daemon::state`'s header spends a paragraph on why a client told to "use wchc" by the `wchd` it is talking to is worse than a wait. |
| A compare-and-set on the document | A real fix and a bigger one: `Session` carries no version, so this is a committed schema change plus a new refusal for a caller whose edit lost a race. It would also be the right answer for a *multi-process* future that D9 does not currently have. |
| Nothing, with an argument | Available only if the interleaving were safe, and it is not: two of the verbs never touch an actor. |

**The cost, stated rather than discovered.** `wch_calibrate_sweep` holds the mutex for the
whole sweep — minutes of camera time — so a `wch_calibrate_select` against an *unrelated*
session waits for it. That is a coarser lock than the problem needs (per session, or a
compare-and-set, would both be finer), and it is the same coarseness `wch` has, arrived at
differently: `InProcess::calibrate_sweep` runs the whole sweep inside `store.with_lock`, so
a concurrent `wch` is refused for the same duration. The bound is the sweep's own
(`limits::MAX_SWEEP_SAMPLES`) plus the client's request timeout, which the T5 method's doc
already tells a client to raise or disable.

**The test, and what it does and does not claim.**
`a_queue_edit_cannot_interleave_with_a_sweep_that_is_rewriting_the_same_session` runs a real
sweep through a backend decorator that gates every `Camera::set`, so "the sweep is inside a
write, with its pre-sweep snapshot already on disk" is an observation rather than a duration.
It fires the reorder there, lets exactly one write through, waits for the next — a whole
sample, including the commit that publishes the document — and asserts the reorder has *not*
completed, then that both changes survive.

The green direction is certain: the mutex makes `is_finished()` false, rather than the
scheduler happening not to get to it. The red direction is **measured, not guaranteed** —
replacing the guard with a bare `Arc::clone` of the token turns the run red on the first
assertion, which is how it was checked. The gate is armed only after `calibrate_start`,
because that verb's probe writes to the camera too and gating it would hold the setup rather
than the sweep.

**Retires when:** the session document carries a version and the store does a compare-and-set,
at which point the exclusion can narrow to one session and the refusal for a lost race
becomes representable. Nothing in docs/7 schedules that; this entry is the record that the
coarse answer was chosen on purpose.

---

## N48 — `terminate_holder` reuses the V4L2 backend's `/proc` walk, and its `kill(2)` lives beside it

**Doc:** AGENTS.md — "Killing a process that holds the camera is an explicit command naming
its target, never a fallback." Design §5 — "terminating a holder is the distinct explicit
`terminate-holder` command (D10), which names both the camera and the pid, refuses if the
pid no longer holds the device, and is never a fallback behavior of anything else." D13
gives it `HolderGone { pid }`; `schema::report::TerminationReport` and the one-member
`TerminationSignal` give it an answer shape. What no document says is **where the code
lives**, and P4c had to answer that twice: for the diagnosis and for the signal.

**The diagnosis.** It exists once, as `webcam-handler-v4l2::holders::of` — the `/proc/*/fd`
walk that lets an `Error::Busy` refusal name the process instead of describing the problem.
P4c promoted the module to `pub` and the daemon calls it by name. The alternatives:

| Candidate | Why it lost |
|---|---|
| **Promote `v4l2::holders` to `pub` (chosen)** | Smallest diff, no new dependency edge — `wchd` already links the V4L2 backend at its composition root — and the walk stays in one place. The cost is a layering statement: the daemon reaches past `CameraBackend` for a fact that is about the *host* rather than about a camera, and the module's own header now says so. |
| Move the walk into `webcam-handler-engine` | Cleanest layering on paper — a `/proc` walk is not V4L2 — but it inverts an existing edge: `crates/backends/v4l2` does not depend on `crates/engine`, and adding that is a §2.8 decision with a dependency-wall diff, for a module whose only two callers are a `Busy` refusal and this verb. |
| Widen T1 with a holder method | Makes a `/proc` concept part of the backend contract every backend must implement, including the fake — and a `FakeBackend` that invented a holder would be "a fake capability no real device exhibits", which AGENTS calls a bug in the fake. T1/T2 are also on the settled list. |
| A daemon-local walk | The third copy in the workspace, and the one with no argument behind it. Note N8 already spends the deliberate-duplication budget on `wch-priv::modules::video_holders`, which answers a *different* question ("is any camera in use") and is not merged because merging would drag the product's crate graph inside a root-capable boundary. |

**The signal.** Nothing in this workspace sends one, `kill(2)` has no safe `std` wrapper
(`std::process::Child::kill` signals a process this one forked, which a camera's holder is
not), and running `/bin/kill` is banned (design §1: no runtime external binaries). So it is
`unsafe { libc::kill(pid, SIGTERM) }` in a new `crates/backends/v4l2/src/sys/signal.rs` —
the one directory in the workspace where `scripts/gates/unsafe-scope.sh` permits the token,
and the one already holding the other half of this verb. The alternative was a permissive
syscall crate (`rustix`, Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT; `nix`, MIT),
which would have put the code where it belongs at the price of a §2.8 registry entry, a
`deny.toml` row, a pin, and a transitive tree — for **two** calls. The smell that signalling
is not V4L2 is real and is answered the same way `holders`' own header answers it: the walk
is not V4L2 either, diagnosis and the one action available on a diagnosis belong together,
and splitting them would put the two halves of one verb in two crates.

**The `unsafe` block's obligation is thin, and it is stated thinly.** `kill(2)` takes two
integers by value and returns an integer: no pointer, no borrow, no allocation. The whole of
the obligation is that the kernel is willing to interpret both arguments, which it is — it
answers `ESRCH`/`EINVAL`/`EPERM` rather than misbehaving. The one interpretation that is a
*hazard* rather than an error is refused above the block: `kill(2)` reads `0` as every
process in the caller's process group, `-1` as every process this uid may signal, and
anything below that as a process group, so a non-positive pid off a socket would turn "an
explicit command naming its target" into a way to kill everything the daemon can reach.
`a_pid_that_is_not_one_process_is_refused_before_the_syscall` is that arm.

**The safety properties, and where each one is enforced.**

1. **Naming, not searching.** The request carries `{camera, pid}`; nothing chooses a victim,
   widens to a process group, or picks "the first holder". `holders` has no
   "signal the holders of this node" function, because that is the shape a fallback takes.
2. **Never a fallback.** No other method signals anything when it meets `Error::Busy`.
   Asserted twice, because a test can only witness the verbs it drives:
   `four_verbs_that_want_the_device_meet_a_busy_one_without_signalling_anything` refuses
   `Busy` through four verbs on a camera whose node a child process holds and finds the child
   alive afterwards — four of eighteen, and its name now says so — while
   `scripts/gates/kill-is-never-a-fallback.sh` makes the claim over the whole tree: the
   signal has one home in the backend and exactly one caller outside it. The P4c review is
   why the second exists; a `Busy` retry added to `wch_photo` would have left the first green.
3. **Verified holding, immediately before the signal.** `holders::holds(pid, node)` and the
   `kill` are the two statements of one blocking closure, with no `await` between them.
4. **One signal, chosen by the product.** `SIGTERM` only, from a closed one-member
   vocabulary; `SIGKILL` is not representable and no wire field could ask for it.
5. **Never this daemon.** A camera this daemon has open is a camera this daemon holds, so on
   real hardware its own pid *is* in the walk. `daemon::server::not_this_daemon` refuses it
   with `IllegalTransition` — not `HolderGone`, which would be a lie, and not
   `PermissionDenied`, which would send an operator looking for a privilege problem. The
   other end of the sentence is already in `engine::actor`, whose own `Busy` refusals carry
   an empty holder list so a client never reads the pid out of an answer.
6. **Under-reporting refuses rather than guesses — but the *cap* must not be what refuses.**
   The walk sees only what this uid may see, so a pid that genuinely holds the node can be
   invisible, and the answer is then `HolderGone`: that occasionally refuses a kill somebody
   was entitled to, and it is the correct direction, because the alternative is signalling on
   the strength of a walk that did not see the target. What the P4c review found is that the
   first implementation ran the same argument off the *bounded* list as well: `holders::of`
   stops at `limits::MAX_HOLDERS_REPORTED` so that a `Busy` refusal stays readable, and the
   handler looked the pid up in that list before signalling. A browser holds one node from
   several processes and `/proc`'s iteration order is not stable, so the fifth holder of
   `/dev/video0` was answered `HolderGone { pid }` — false, and with no pid a caller could
   name to free the camera, which is the verb at its least usable in the one situation it
   exists for. `holders::holds`'s own doc had described the opposite arrangement since it was
   written ("a pid past `limits::MAX_HOLDERS_REPORTED` in the walk's order is one the walk
   would not mention and this still finds"), so the name and the call graph disagreed —
   rubric A11. The gate is now `holders::holder(pid, node)`, the per-pid question, which is
   *stronger* evidence than membership in a truncated walk rather than weaker; `of` attaches
   nothing to this verb any more. `a_holder_past_the_reporting_cap_can_still_be_named_and_signalled`
   forks `MAX_HOLDERS_REPORTED + 1` holders, picks the one the walk did not mention, and
   asserts it is signalled and the others are not.
7. **Requested is not applied.** `still_held: true` is a success answer. Nothing escalates on
   it — no second signal, no `SIGKILL`, no retry loop.
8. **`EPERM` stays `PermissionDenied`.** A process this uid may not signal is an availability
   answer, and converting it into "the process is gone" is exactly the conversion E3 and
   AGENTS rule 7 forbid. `ESRCH` *is* `HolderGone`, because that is the pid-reuse race
   landing where the caller already has to handle it.

**The race that remains, and what bounds it.** Between the re-verification and `kill(2)` the
holder can exit, be reaped, and have its pid recycled onto an unrelated process. This cannot
be closed from user space without `pidfd_open(2)` + `pidfd_send_signal(2)`, which signal a
*process* rather than a number and which need a syscall wrapper this workspace does not
link. **Amended 2026-08-10:** the wrapper is reachable and this is schedulable work rather
than a limitation. `rustix` 1.1.4 is already in `Cargo.lock` and already allowlisted, and
`rustix::process::{pidfd_open, pidfd_send_signal}` are both safe functions behind its
`process` feature (verified in the pinned source, not assumed). The owner ruling of
2026-08-09 (§2.8) makes taking that edge routine. It is still not P4c's — this entry's
four bounds are what P4c shipped and they stand — but the reason it was not done is now
"nobody scheduled it", which is a different sentence from "it cannot be done", and only
one of those two is true. Four things bound it, and the method's doc says all four rather than claiming it away:
Linux recycles pids in increasing order up to `pid_max` (2^22 on a modern default), so the
window is "enough processes forked to wrap the counter" rather than "the next fork"; the
re-verification narrows it from a whole request — an enumeration and an ioctl per node — to a
few instructions; `SIGTERM` asks rather than compels, so a wrongly addressed one is
survivable for a process that handles it; and the caller named the number.

**`still_held` is the one place this daemon waits to learn something.** `kill(2)` returns
when the signal is queued, so an immediate re-walk would report almost every process that is
about to exit as still holding the camera — and a field that is nearly always `true` is a
field nobody reads, which defeats the reason it exists. A process this one did not fork
leaves no event a Unix process can wait on, so the mechanism is a bounded poll:
`limits::TERMINATE_RECHECK_MS` of budget spent `limits::TERMINATE_RECHECK_POLL_MS` at a
time, ending the moment the fact changes. It is about the **node**, not about the pid, which
is what the field says — so a camera this daemon has open reports `still_held: true` until
D12's idle close lets go of it.

**The rule this is measured against is AGENTS' "no `sleep` as synchronization, anywhere",**
and the distinction it turns on is what the waiting is *for*. The banned shape waits for
work this process controls — a settle, a deadline, a reply — and every one of those runs on
a caller-supplied `now_ms` or a paused clock, which is why nothing else in this workspace
waits. Here the fact belongs to a process this one did not start and cannot observe, and the
answer being waited for is a **field on the response** rather than a step in a protocol: the
poll is the measurement, not the coordination. Two things keep it from being the banned
shape wearing an argument — it is bounded by a `limits` constant with the number's reasoning
beside it, and it ends on the fact rather than on the clock, so the ordinary cost is one
interval. **`still_held: false` says "nothing this uid can see holds it", which is as far as
`/proc` goes** — the same under-reporting point 6 is about, from the reporting side rather
than the signalling side. The conservative reading is not available here and the asymmetry is
deliberate: a walk of a node nobody holds is *also* empty, so answering `true` on an empty
walk would make the field constant, which is the failure `TERMINATE_RECHECK_MS` exists to
prevent. Point 6 takes the conservative direction because its answer decides an *action*;
this one describes one.

**Which arm the wait's duration is safe for, corrected.** This entry used to say "the test
does not depend on the wait's duration for anything", which is true of the `still_held: true`
arm and was false of the other: over a socket, `false` requires the forked child to have
received `SIGTERM` and left `/proc/<pid>/fd` inside `TERMINATE_RECHECK_MS` of **wall** clock,
which on a loaded runner is a race and would have been the one duration-dependent assertion
in a workspace whose rule is that nothing synchronises on a sleep. The P4c review found it
and the integration suite no longer makes it. Both directions are pinned where the clock is
an argument instead — `daemon::server`'s
`a_still_held_answer_is_immediate_when_nothing_holds_the_node_and_bounded_when_something_does`
runs on a paused clock and asserts the *exact* elapsed reading: zero for a node nobody holds,
`TERMINATE_RECHECK_MS` for one this process holds throughout, and zero again once it lets go.
The `true` direction over the wire stays, because a second holder the suite owns and holds
open for the whole call is arranged rather than raced.

**The walk count the constant states.** `TERMINATE_RECHECK_POLL_MS`'s doc prices the work
against a large process table, and it said "ten walks" while the loop performed eleven — one
immediately and one after each of the ten waits the budget buys. Off by one in the one number
a reader would use, which is rubric B11's class. The loop now runs `recheck_walks()` times,
that function is where the arithmetic lives, and
`the_walks_one_still_held_answer_costs_are_the_number_the_constant_states` asserts it against
both constants so the sentence cannot drift again.

**How it is tested without signalling a stranger.** The fake replays real-looking node paths,
and on a developer's laptop `/dev/video0` is a path a real program may hold — so the suite
doctors one field of the committed profile and points the capture node at a file inside its
own throw-away directory. The holder is a child process the test forks and is willing to
lose (`crates/engine/tests/crash_recovery.rs`'s construction, announcing on a pipe and
blocking on another, so nothing sleeps). The `HolderGone` direction is asserted against **the
test process's own pid** — alive, signallable, and not a holder — which is the only pid it
is safe to be wrong about: a build that signalled it would take the runner down loudly rather
than pass quietly. `still_held: true` is arranged by a second holder the test owns, itself,
rather than by a child that ignores `SIGTERM`, because setting a signal disposition needs
`unsafe` that no test in this workspace may write.

**Not landed, and named so the absence is counted.** `terminate_holder` has **no T4 verb**.
`schema::report`'s own header already says the answer type belongs to a verb "whose verb has
no command-line spelling yet", which is why it is in the OpenRPC document rather than the
JSON Schema bundle. Adding one would move `json-validates.sh`'s population (derived from
`wch --help`) and P4f's parity population, and docs/7 P4c's list is a wire list. So the
method is reachable only by a client that speaks raw JSON-RPC until somebody schedules the
CLI spelling.

**Retires when:** never as a rule. The `unsafe` block retires if this workspace ever adopts a
permissive syscall crate for another reason, at which point `sys/signal.rs` becomes a
two-line forward and `holders` can move to the engine in the same diff.

---

## N49 — The method-count walk drives the surface itself, because a shared test module has to be used in full by every binary that includes it

**Doc:** docs/9 Part 2's **T5 method-count walk** row (P4c): "the registered `RpcModule`'s
`method_names()` … compared against the integration-test inventory; derived from the running
registration, never a hand list", failing on "a wire method with no test". Rubric B6 says the
same from the review side, and docs/7 P4c adds "Every method exercised over the fake". Note
**N28** fixes the population: "a registered *daemon* module compared against the
integration-test inventory … a different claim over a different population from 'the emitted
document describes the trait'".

**What landed.** `crates/daemon/tests/method_surface.rs`. The registered side is
`method_names()` off the `Methods` value the fixture serves, built by the generated
`into_rpc()` over a real `Wchd`. The exercised side is recorded at the transport: a
`Recording` wrapper around the shared `Wire` writes down the method name the **generated
client** handed it, on the way past, whether the call is answered or refused. Neither side
is a list. What is unavoidably written by hand is the *sequence of calls* — a Rust trait does
not reify its methods, so nothing can walk one and invent arguments for it — and that
sequence is exactly what the comparison protects: a twentieth method nobody calls leaves the
recorded set one short.

**The alternative, and why it lost.** The stronger shape is for the census to drive the
*behavioural* suites' own exercise functions (`read_verbs::read_verbs`,
`mutating_verbs::mutating_verbs`, `calibrate_verbs::calibrate_through`), so that "exercised"
means "reached by a call whose answer somebody asserts" rather than "reached by a call". It
lost to a mechanical fact, measured rather than assumed:

> `crates/daemon/tests/support/*.rs` are `#[path]`-included, so every binary that includes
> one compiles **all** of it, and `dead_code` is per item per binary — down to individual
> struct fields. Measured on this toolchain: a `PartialEq` derive counts as a field read and
> a `Debug` derive explicitly does not ("`Deb` has a derived impl for the trait `Debug`, but
> this is intentionally ignored during dead code analysis"). `#[expect(dead_code, reason=…)]`
> cannot paper over it either, because the expectation would be *unfulfilled* in the binaries
> that do use the item, which is itself an error.

So a shared driver module must be used in full — every function, every field — by each of
the four binaries that includes it. Moving the three drivers there would have forced the
census to consume every helper the three suites own (`Targets::read_only`, `Refusals`,
`Shape`, the fingerprint walk) or forced those suites to be restructured around it, which is
a large diff in three carefully-argued files to buy a property the shared *fixture* already
buys most of. The rule bit twice earlier in P4c — it is why `Ask` no longer carries session
references, and why `terminate_holder`'s tests live in `mutating_verbs.rs` rather than in a
binary of their own — and this entry is where it is written down.

**The cost, stated rather than discovered.** The walk counts calls, not depth: a twentieth
method could be landed with a census call, a shallow assertion and no behavioural test. Three
things push back and none of them is a proof.

1. The census's answer record has **one field per method** and every field is read by an
   assertion, so an answer nobody looked at is `dead_code` and does not build. That makes
   "call it and ignore it" a compile failure rather than a review finding.
2. `discriminating_refusals` asks the same client for three things that are not there and
   requires three *different* typed refusals, so a fixture that had degraded into refusing
   everything cannot keep the count green.
3. The behavioural depth has its own `g4` rows — `binary(read_verbs)`,
   `binary(mutating_verbs)`, `binary(calibrate_verbs)` — so "this verb has a test" is a
   claim with a second reader.

The four limits go in docs/9's gaps register beside the struck row, because that is where
this suite records what a green does not mean.

**Measured red, both directions the row names.** A twentieth method added to the trait and
implemented on `Wchd` but not driven fails the comparison naming it
(`… "wch_snapshot", "wch_terminate_holder", "wch_twentieth"` against a set of nineteen). A
daemon whose registration is missing a method the suite drives fails *earlier* — at the
call, as `Call(ErrorObject { code: MethodNotFound })` — because an unregistered method
answers `-32601` rather than an answer, which is why the assertion's second direction is a
sentence about what the equality means rather than a failure anybody will read.

**Retires when:** never as a rule; the walk is the row. It is amended if the four limits
change — in particular if P4e's subscriptions land, because a `#[subscription]` registers
*two* names and a subscription nothing can drive is exactly what this row fails (note N29
says so).

**One arm removed by the P4c review, and it is worth recording why.** The walk shipped with a
loop under the equality that dropped each registered name from the recorded set in turn and
asserted the shortened set differed — sold, in the gate row, as "the comparison proven able to
go red by dropping each registered name from it in turn". It could not fail for any input:
after `assert_eq!(exercised, registered)` the two sets *are* one set, so removing a member
always succeeds and a proper subset is never equal to its superset. A tautology under a
comparison is worse than no arm at all, because it reads as a second guard and a later edit
that weakened the comparison would leave it green. The equality is the row, its red-ness was
demonstrated by deleting a call from `every_method` and watching it name the missing method,
and the non-vacuity that loop was reaching for is now the assertion that the registered set
has nineteen members — the number `crates/api` pins the trait at.

---

## N50 — `CameraId` deserializes through its constructor, because the empty string is a prefix of every camera

**Doc:** D1 makes camera resolution a **prefix** match — "`cam:obsbot` for
`cam:obsbot-tiny-3`" — and says an ambiguous prefix names its candidates rather than choosing
one. The P4a methods contract recorded the hole as **D-4** and docs/7 carried it as a standing
debt: `CameraId` and `ControlSlug` are `#[serde(transparent)]` newtypes whose derived
`Deserialize` accepts `""` while `parse`/`from_slug` refuse it — "on a command line
`CameraArg::id()` catches it, off a socket nothing does".

**Repo, before this.** `schema::camera::resolve_prefix(ids, "")` finds no exact match and then
keeps every id, because every string starts with the empty one. On a **two**-camera host that
is `CameraAmbiguous`; on the single-webcam laptop that is this product's ordinary deployment
it is `PrefixMatch::Unique` — the emptiest possible prefix *chooses*, which is the one thing
D1 says a prefix may not do. P4c is the commit that put it on `wch_set`, `wch_photo`,
`wch_terminate_holder`, `wch_profile_capture` and the eight `calibrate_*` verbs, i.e. on every
path that writes to a camera or opens a session against one.

**Why it landed here rather than staying deferred, which is a correction to docs/7.** The
standing debt bundled this with `PixelFormat`'s wire spelling and deferred both with one
reason: "each moves a committed bundle, so each wants its own commit and a gate diff rather
than a ride-along". That is true of `PixelFormat` and **false of this half**, which the P4c
review noticed and which is cheap to disprove rather than argue: `Deserialize` and
`JsonSchema` are separate derives over the same attributes, so replacing the derived one with
a hand impl changes which strings are *accepted* and not what the type *emits*.
`./scripts/gates/schema-artifacts-current.sh` is green with both committed artifacts
byte-identical, which is the disproof.

One thing had to move to keep that true, and it is worth knowing: `schemars` publishes a
type's doc comment as the `description` of its node in the bundle, so the paragraph arguing
this belongs on the `impl` and not on `CameraId`. Writing it above the struct moved
`schemas/webcam-handler-schema.json` and the gate said so immediately.

**What did not land, deliberately.** `ControlSlug`'s empty case is left as it is. docs/7 calls
it honest and it is: an empty slug lands on `ControlUnknown`, which names the control the
caller did not give and suggests the ones this camera has. Nothing chooses on its behalf,
which is the whole of what made the `CameraId` half a defect.

**The tests, both directions.**
`schema::camera::an_empty_camera_id_is_refused_off_the_wire_rather_than_matching_every_camera`
asserts the wildcard it is about *first* — `resolve_prefix(&ids, "")` really is `Unique` on a
one-camera host — then the refusal, then the two spellings a real client sends and a
round-trip, both derived from the assigned id rather than written down.
`daemon::mutating_verbs::an_empty_camera_id_is_refused_rather_than_naming_the_only_camera`
sends `{"camera": ""}` as **raw JSON over the socket**, because the generated client takes a
typed `CameraId` and cannot produce the request the defect is about — which is the defect, in
one sentence — and asserts `-32602` from three methods with `FakeBackend::opens()` still zero.
Both go red against the derived impl.

**Retires when:** now for this half. docs/7's standing debt keeps the `PixelFormat` row with
the reason that actually applies to it.

---

## N51 — A photo's `ServerPath` must name a regular file, because `std::fs::write` blocks on a fifo inside the actor's one thread

**Doc:** the T5 trait's `wch_photo` states the posture and it is settled: "A `ServerPath` sink
is a write primitive: any client that can call this can write a file anywhere the daemon's uid
can. That is deliberate and it is exactly what D11's authentication model covers." AGENTS is
equally settled about the other side: "Camera frames never enter the repository, logs, or
error messages", and "Bounded everything".

**Repo, before this.** `engine::photo::write_photo` is `std::fs::write(path, bytes)`, which
opens with `O_WRONLY|O_CREAT|O_TRUNC` and no `O_NONBLOCK`. On a fifo that `open` blocks until
a reader appears. `daemon::server::addressable` asked whether the path was absolute and whether
its extension named an encoding this build writes; neither question is *what the path is*. So
P4c re-checked the extension a socket can send and did not re-check the **type of file** a
socket can name — which is debt D-1's own shape one layer down.

**What that costs, measured rather than argued.** The write runs inside the closure
`Wchd::on_camera_with_state` hands to the camera's actor thread, and nothing bounds a submitted
command: `CameraActor::submit` is a `try_send` and the daemon awaits the oneshot with no
timeout. A blocked command leaves `Live::busy` raised, and `CameraActor::sweep` reads `busy`
first, so D12's idle close can never fire either. One `mkfifo /tmp/x.jpg` and one `wch_photo`
therefore park that camera's thread for the life of the process: the request never answers,
the next `CAMERA_COMMAND_QUEUE_DEPTH` requests queue and every one after that is `Busy`
forever, the descriptor is held open, and the operator's webcam is unusable by any other
application until `wchd` is restarted. Reproduced by disabling the check and running the new
test, which does not return.

The second shape is quieter and worse: `/dev/stdout` is an absolute path ending in nothing a
`PhotoFormat` refuses, and under systemd it is the journal. A camera frame in the logs is not
a bound this project trades away.

**The refusal, and where it lives.** `addressable` grew a third rule: a `ServerPath` whose
existing target is not a regular file is `Error::IllegalTransition`, naming the path and what
it is. `IllegalTransition` for note N46's reason — it is the request naming a destination this
build will not write, in the same family as the other two sink refusals — even though it is the
one that does stat a path. It follows symlinks (`metadata`, not `symlink_metadata`), because a
symlink to a regular file is an ordinary destination and it is the *target* that decides
whether the write blocks; that reading is also what refuses `/dev/stdout`. A path that does not
exist is fine: `std::fs::write` creates a regular file, which is the case the rule protects.
Because it stats, the handler runs it on the blocking pool rather than on a runtime worker —
a `stat` is fast until the path is on a hung mount.

**Where it does *not* live, and why.** Not in `engine::photo`. `write_photo`'s own doc argues
that a temp-file-plus-rename would "silently break the case where that path is a fifo or
`/dev/stdout`", and for `wch` that is a real feature: a person typed the path, and Ctrl-C
exists. What changed is the caller, not the engine — the daemon has no Ctrl-C, no timeout on a
submitted command, and a journal. So the rule is the *transport's*, beside the other two rules
only a socket can break, and `wch photo -o /dev/stdout` is untouched.

**What is left, and who owns it.** The check is a `stat` followed by an `open`, so a client
that replaces the path between them wins a race and still parks the thread. Closing that needs
the `open` itself to be non-blocking — `O_NONBLOCK` through `OpenOptionsExt::custom_flags`
plus an `fstat` on the descriptor — and the flag's numeric value is not the same on every
Linux architecture, so getting it honestly means a `libc` edge in a crate that has none or a
constant this project would be guessing. **Amended 2026-08-10:** there is a third way this
missed, and it is the good one. `rustix::fs::OFlags::NONBLOCK` carries the per-architecture
value for us and `rustix` is already in `Cargo.lock` and already allowlisted, so the flag
needs neither a guess nor an `unsafe` block — the objection above was to *guessing a
constant*, and a safe wrapper that knows it answers the objection completely. The owner
ruling of 2026-08-09 (§2.8) makes the edge routine. The deferral stands on its remaining
ground alone, which is the honest one: closing this race properly wants the actor-command
bound too, and shipping half of it would trade a wedge for a leak. It is **P4e's**, with the bound on a submitted
command that N42 already deferred there: "an enqueue that waits with a bound where
`CameraActor::submit` has only a `try_send`" is the same missing mechanism, and an actor
command that cannot outlive a deadline makes the residual race a refusal rather than a wedge.
docs/7's standing debts carry it.

**The test.** `a_server_path_that_is_not_a_regular_file_is_refused_before_the_camera_is_touched`
sends a photo to a fifo (made with `mkfifo(1)`, because this crate is
`#![forbid(unsafe_code)]` and `libc` is confined to the V4L2 backend) and to a directory,
asserts the typed refusal names both the path and what it is, asserts `FakeBackend::opens()`
is still zero, and then takes a photo to a real path through the same camera — which is the
assertion the wedge would break, and the line a blocked build never reaches.

**Discharged at P4e-i (2026-08-10), and the layered answer is two sentences rather than
one.** The race is closed by making the *descriptor* the destination:
`daemon::server::open_destination` opens the path with
`rustix::fs::OFlags::WRONLY | CREATE | NONBLOCK | CLOEXEC`, `fstat`s what it got, and
refuses anything that is not a regular file — all of it **before** a camera is resolved or
opened, so `FakeBackend::opens()` is still zero after a refusal and the assertion this
entry's test makes is unchanged. The open `File` is then carried into the actor's closure by
`daemon::server::OpenedAhead`, a `engine::photo::Destination`; `wch` keeps the blocking open
it wants (`engine::photo::WhereverTheCallerSaid`), because a person typed that path, `-o
/dev/stdout` is a feature and Ctrl-C exists. There is no window left: the name is resolved
once, and nothing a client does to it afterwards can redirect the bytes.

**The two flags do different halves, and the note owes that precision** (the summary
sentence above was imprecise about it). `O_NONBLOCK` removes the *cause* — no open on this
path can wait, so a fifo answers `ENXIO` where it used to park the thread. The bounded
enqueue N42 landed beside it removes the *consequence class* — a command that parks the
thread for some other reason no longer turns every later request for that camera into an
unbounded queue. A deadline on the daemon's await would have done neither: it answers the
*caller*, and cannot unwind a thread already inside a blocking `open(2)`, so `Live::busy`
would stay raised and the camera would stay parked. That is why P4e-i landed the
non-blocking open rather than a timeout, and why "an actor command that cannot outlive a
deadline makes the residual race a refusal rather than a wedge" was only ever half true.

**Two ordering decisions worth having written down.** `O_TRUNC` is deliberately *not* in the
open: truncating a destination before the capture has happened would empty an operator's
existing photo on the way to reporting that the camera failed, so
`engine::photo::write_to_open_file` sets the length after `write_all`. And the `stat` in
`describe_unopenable` is not the `stat` this entry is about: `ENXIO` has already decided the
refusal, and that call only supplies the noun — a client that swaps the path between the
failed open and it gets a less accurate sentence rather than a parked camera.

**What remains, honestly.** A *regular file* on a hung mount still blocks in `write(2)`
inside the actor's thread. `O_NONBLOCK` does not help there and must not: a short write on a
regular file is a truncated photo, which is a worse bug than a slow one.

**Amended 2026-08-10 (note N59), because the sentence that stood here was wrong and wrong in
the flattering direction.** It said "a wedged filesystem costs one camera one command rather
than the camera". It costs the camera. `engine::actor::Thread::run` calls `work(...)` inline,
so a `write(2)` that never returns means `finished()` never runs, `Live::busy` stays raised
for ever, `close_if_idle` is never reached and `inbox.recv()` is never called again — every
later command on that camera is refused for the life of the process, which is bit for bit the
wedge this entry was opened to measure with a hung mount as the trigger instead of a fifo.
What the bounded enqueue changes is only *how quickly other callers are told no*: a
`wait: true` caller gives up at `limits::CAMERA_ENQUEUE_WAIT_MS` with `Busy` rather than
holding a pool thread indefinitely, and a `wait: false` caller was already refused. Ending
such a command outright would need a cancellable device thread, which nothing in D12
provides — that part was right, and it is the actual retirement condition.

**And a gate moved with it, because the workspace learned a new way to write a file.**
`atomic-write-home.sh`'s raw-write population is a hand list of spellings — it has to be;
there is no walkable population of "ways to obtain a writable descriptor" — and until this
change every entry was `std`'s. A `rustix` open turned into a `std::fs::File` matched none
of them, so a state-directory bypass spelled the way `open_destination` is spelled would
have been invisible to the gate: the third instance of note N10's family, and the second
one in this predicate (the P3 review added `File::options(` and `File::create_new(` for
the same reason). The pattern now also matches the write-shaped **flags**,
`OFlags::(WRONLY|RDWR|CREATE|TRUNC|APPEND)`, and deliberately not `rustix::fs::open(`
itself: `daemon::uds::SocketDir` opens the runtime directory `O_PATH | O_DIRECTORY` with
that same function while naming `XDG_RUNTIME_DIR`, and calling that a bypass would be a
false positive with an obvious workaround. Both directions are in
`cases/atomic-write-home.cases.sh` — a seeded bypass through `rustix` that the old pattern
was measured not to match, and a read-only `rustix` open beside the runtime directory that
must stay green.

**The tests.** `a_server_path_that_is_not_a_regular_file_is_refused_before_the_camera_is_touched`
is unchanged and now asserts a stronger mechanism through the same wire; the new
`a_path_swapped_after_the_check_cannot_redirect_the_photo_or_park_the_camera` renames the
checked file away mid-capture and puts a fifo in its place, and finds the photo in the inode
the daemon approved; `daemon::server`'s
`a_destination_is_opened_from_its_descriptor_and_a_fifo_never_waits_for_a_reader` asserts the
four refusals apart from one another and that an existing file survives the open.

**Retires when:** nothing. It records the layered answer.

## N52 — The mutation floor's verdict moved with `nproc`, and the branch that did it had never fired

**Believed:** that `just mutants` measures the tests. The job's whole claim is that a
surviving mutant is a missing test, and `scripts/mutants.sh` compares survivors against
`scripts/mutants-accepted.txt` in both directions so neither an unlisted survivor nor a
stale acceptance can pass unnoticed (E7).

**True:** it measures the tests *and the machine*, and until P4c nobody could have known,
because the branch that mixes them in had never executed. `scripts/mutants.sh` counts a
timed-out mutant as a survivor — deliberately, and correctly, since a mutant that hangs is
not a mutant that was proven killed; the concatenation of `missed.txt` and `timeout.txt` is
right there in the script. But P3f's first run produced **zero** timeouts (E7: 410 mutants)
and P4a's widened run produced zero as well (E8: 478). A branch with no observations behind
it is a claim, and this one was load-bearing.

cargo-mutants times each mutant at `baseline × timeout_multiplier`, floored by
`minimum_test_timeout` (default 20s). This workspace's baseline suite is about 3s, so
`3 × 5 = 15` sat under the floor and every mutant got 20 seconds. The baseline is timed
**once, alone**. Every mutant afterwards runs beside `jobs - 1` concurrent cargo builds on
one disk. Those are different conditions, and the distance between them grows with every
test the workspace gains — which P4b and P4c grew by a daemon, four integration suites and
real sockets.

**Measured**, same tree, same commit, one variable changed:

| jobs | test-timeout floor | timeouts | survivors | acceptances | verdict |
|---|---|---|---|---|---|
| 8 | 20s (tool default) | 34 | 42 | 11 | **FAIL** — 31 with no acceptance |
| 4 | 180s | 0 | 11 | 11 | **PASS** — register clean both ways |

All thirty-one were in `imaging/src/metrics.rs`, and every one of them was caught once it
was given time to finish. Two things make that the worse direction of failure rather than
the harmless one. First, it is the *inverse* of what the floor exists to detect: the job
reported thirty-one missing tests that were not missing, over a file whose tests P3f had
already triaged. Second, a gate that cries wolf does not get believed — it gets re-run at
`-j1` until it agrees, and the run after that is the one where a real survivor is waved
through as "probably the timeout thing again". This entry exists so that reflex has a
written answer.

**Changed:** `.cargo/mutants.toml` pins `minimum_test_timeout = 180.0`, with the table
above in the comment beside it. A floor and not a `timeout_multiplier`, because the
multiplier scales the *baseline*, and the baseline is exactly the measurement that does not
know about contention — no multiple of an unloaded 3s describes a loaded one. The timeout
still counts as a survivor, so an infinite-loop mutant is caught by the same mechanism as
before; it now costs three minutes to catch instead of twenty seconds, which is affordable
in a job that runs for half an hour and is the right side to be wrong on.

**Not changed, and why.** `scripts/mutants.sh`'s `per_job_gib=3` survived scrutiny: one
build tree was measured at 2.5 GiB during this triage, so the estimate is sound. The P4c
run that died with "Disk quota exceeded" did so because the build root was a 16 GiB `tmpfs`
that other work filled *after* the script's one-shot `df` — the check samples free space
once at start and cannot see what arrives later. Left alone rather than padded on a guess;
`WCH_MUTANTS_BUILD_ROOT` already exists for exactly this and pointing it at a real
filesystem is the fix an operator has.

**Doc:** AGENTS.md "no skip that reads as pass" has a sibling this entry names — no
*failure* that reads as a finding. Both are the same rule: a gate's output must mean what
it says, and an environmental verdict wearing a defect's clothes costs the suite its
credibility just as surely as the reverse.

**Retires when:** never by disproof; it retires as history if the floor ever stops counting
a timeout as a survivor, which would need its own argument.

---

## PF:21 — The uevent socket needs no privilege: `NETLINK_KOBJECT_UEVENT` binds *and delivers* to an unprivileged process

**Measured** 2026-08-10 on kernel `7.0.0-29-generic` (x86_64), as uid 1000 with
`CapEff: 0000000000000000` — no effective capability at all. Continues the docs/6 §1.2
registry; cite it as `[PF:21]`. This is design §8 item 10's question, answered, and it is
the measurement docs/7 P4d schedules ("bind `NETLINK_KOBJECT_UEVENT` *unprivileged first*
on this kernel and record the answer in the notes").

Taken with an independent Python probe against the C API, not with this project's own
socket module — the house rule that a fixture produced by the code under test proves
nothing (PF:15 was measured the same way).

### The bind

```
socket(AF_NETLINK, SOCK_DGRAM | SOCK_CLOEXEC, NETLINK_KOBJECT_UEVENT)

kernel 7.0.0-29-generic  uid=1000 euid=1000
  CapInh: 0000000800000000     (CAP_WAKE_ALARM, inherited from the shell)
  CapPrm: 0000000000000000
  CapEff: 0000000000000000
  CapBnd: 000001ffffffffff
  bind(nl_pid=0, nl_groups=1): OK, getsockname=(1863593, 1)
  bind(nl_pid=0, nl_groups=2): OK, getsockname=(2582951618, 2)
  bind(nl_pid=0, nl_groups=3): OK, getsockname=(2332564595, 3)
  SO_RCVBUF default: 212992
  unprivileged multicast sendto(group=1): errno=1 (EPERM) Operation not permitted
```

**The bind is free**, for the kernel's own broadcast group (1), for udev's rebroadcast
group (2), and for both. The kernel side of why: `lib/kobject_uevent.c` registers the
protocol with `NL_CFG_F_NONROOT_RECV`, which is precisely the flag that exempts group
membership from `netlink_bind`'s `CAP_NET_ADMIN` check. `NONROOT_SEND` is **not** set,
which the last line measures from userspace: a non-root local user cannot forge a uevent
into this socket, by multicast or by unicast to another netlink port.

### The delivery, which is a different claim

Binding is a permission fact; receipt needs an event. One `wch-priv uvcvideo cycle` with
every camera closed, with the same unprivileged listener bound to group 1:

```
capture: uid=1000 CapEff=0000000000000000 sockname=(1895300, 1)
[0001] +    0.00ms  290B            usb unbind   …/2-3.4.1.1:1.2
[0003] +    0.54ms  294B    video4linux remove   …/2-3.4.1.1:1.0/video4linux/video6
…
[0052] +  332.68ms  288B    video4linux add      …/2-3.4.1.1:1.0/video4linux/video9
[0056] +  336.42ms   87B         module add      /module/uvcvideo
capture: 56 packet(s); burst width 336.42ms
```

56 packets, all of them received by a process with an empty capability set, none lost
(`ENOBUFS` never fired inside the default 212992-byte receive buffer). By subsystem and
action:

| subsystem | remove/unbind | add/bind |
|---|---|---|
| `video4linux` | 10 | 10 |
| `usb` | 9 | 9 |
| `module` | 4 | 4 |
| `media` | 4 | 4 |
| `drivers` | 1 | 1 |

**Ten `video4linux` removes and ten adds for four cameras**, which is PF:19's arithmetic
generalised and is why a subsystem filter and a debounce are both load-bearing: thirty-six
of the fifty-six packets are not ours, and the twenty that are describe four cameras.

Two numbers P4d's debounce is sized against rather than guessed at:

- **The whole burst is 336 ms wide.** The largest gap between consecutive *packets* is
  93.8 ms; the largest gap between consecutive `video4linux` packets is **119 ms** — the
  pause between the last `remove` (188.94 ms) and the first `add` (307.90 ms), which is
  `modprobe` being re-run. A quiet-window shorter than that fires *between* the removes
  and the adds and reports a machine with no cameras on it, which is the flapping the
  debounce exists to prevent. **It is one sample and not a bound** — see the second cycle
  below, which measured the same gap 20 ms shorter.
- **Node numbers survived this cycle** (`video0`…`video9`, same interfaces, same four
  cameras) but nothing may depend on that: ten minors are released at once and the kernel
  re-allocates in registration order.

### A second cycle, and the one number that moved

Run again the same day, same host, same four cameras, with a fresh unprivileged listener
and no code between the socket and the log. It is here because a single timing sample that
a constant is about to be sized against is a number nobody has seen vary:

```
capture: uid=1000 CapEff=0000000000000000 sockname=(2113773, 1)
[0001] +    0.00ms  294B    video4linux remove   …/2-3.4.1.1:1.0/video4linux/video6
[0020] +  133.25ms  234B    video4linux remove   …/3-4/3-4:1.0/video4linux/video1
[0033] +  231.78ms  228B    video4linux add      …/3-4/3-4:1.0/video4linux/video0
[0056] +  294.68ms   87B         module add      /module/uvcvideo
capture: 56 packet(s); burst width 294.68ms; ENOBUFS 0
capture: largest gap between consecutive packets 93.37ms
capture: video4linux packets 20; largest gap between consecutive video4linux packets 98.53ms
capture: last remove +133.25ms -> first add +231.78ms = 98.53ms
```

What repeated exactly: **56 packets**, `ENOBUFS` 0, and the per-subsystem census
(`video4linux` 10/10, `usb` 9/9, `module` 4/4, `media` 4/4, `drivers` 1/1) — the same
table, packet for packet. The packet shape repeated too, 228 bytes and trailing NUL, with
only `SEQNUM` advanced (89104 against 89028). The four cameras came back on the same four
interfaces and the same ten minors.

What moved: the **burst width, 294.68 ms against 336.42 ms**, and with it the
remove-to-add pause, **98.53 ms against 119 ms**. Both are `modprobe` being re-run, and
`modprobe` is not a real-time system. So the debounce's quiet window is chosen against the
*spread* — two samples 20 ms apart on an idle desk say a window in the 150–300 ms range
clears both with room, and a window near 100 ms would have straddled this run. Sizing it
at the larger sample would have been sizing it at a coincidence.

A **third** cycle, run only to write the packets to disk so the fixture work would not need
a fourth, delivered the same 56 packets — 12953 bytes of payload, `ENOBUFS` 0 — and the
same census again. Three cycles, three identical packet counts and three identical
subsystem tables: the *shape* of a `uvcvideo` cycle on this host is stable and only its
timing is not. Feeding those captured bytes through this workspace's own
`hotplug::trigger` confirms the two claims the synthetic test packets cannot: a real
kernel packet **ends in a NUL** (which `kobject-uevent` skips, because a segment with no
`=` is not a field), and a real `video4linux` `add`/`remove` decodes to
`NodeChanged`, while the `media`, `usb` and `module` neighbours decode to
`NotOurs(OtherSubsystem)`.

### The packet shape on this kernel, verbatim

```
add@/devices/pci0000:00/0000:00:14.0/usb3/3-4/3-4:1.0/video4linux/video0|ACTION=add|
DEVPATH=/devices/pci0000:00/0000:00:14.0/usb3/3-4/3-4:1.0/video4linux/video0|
SUBSYSTEM=video4linux|MAJOR=81|MINOR=0|DEVNAME=video0|SEQNUM=89028|
```

(`|` is a NUL.) 228 bytes; the largest `video4linux` packet in the burst was 294. Note the
**trailing NUL**, which `kobject-uevent`'s own committed fixture does not have, and note
that `DEVNAME` is a bare name with no `/dev/` prefix.

### Which document was wrong

Two files in this repository predicted opposite answers and neither had measured:

- **`docs/implementation-notes.md` N8** — "Binding `NETLINK_KOBJECT_UEVENT` needs
  `CAP_NET_ADMIN`. *Unverified* — the probe was blocked — so this capability is granted
  ahead of proof". **Disproved.** N8 carries the amendment.
- **`docs/research/crates-v4l2.json`** — "Needs no privileges beyond an AF_NETLINK
  socket." Correct, and it was a flat assertion with nothing behind it. The same file's
  risk 5 hedge ("verify container/sandbox environments permit AF_NETLINK uevent sockets")
  is the part that survives: see the limits below.

### What this does and does not license

- **`CAP_NET_ADMIN` was never spent on the receive path.** G6's narrowing (design §2.13,
  docs/7 P6e) can drop it, leaving `cap_sys_module` — which `modprobe` still needs and
  which nothing here touches. **P4d does not re-bless**: docs/6:1045 and docs/7 P6e own
  the narrowing, and this entry hands them the fact.
- **The R3 hotplug arm needs no privileged wrapper for its socket.** It binds its own
  listener as an ordinary test process and spawns `wch-priv uvcvideo cycle` as a
  subprocess — the shape measured above. The `exec` wrapper's original argument (N8: "only
  a wrapper can put `CAP_NET_ADMIN` inside a *test process*") is therefore not exercised
  by hotplug.
- **`SO_RCVBUFFORCE` still needs `CAP_NET_ADMIN`**, and is the one way the capability
  could come back. It is not needed here, and the honest form of that claim is *measured
  three times, never fired* rather than an arithmetic margin: all three cycles delivered
  all 56 packets with `ENOBUFS` never raised. The margin is smaller than a payload sum
  suggests — the whole burst is **12953 bytes** of payload against a 212992-byte buffer,
  about 16×, and netlink charges the buffer `skb->truesize` rather than payload, so the
  real headroom is some fraction of that. A busier machine could plausibly reach it, which
  is why `Received::Overrun` is a modelled outcome and not an error. `SO_RCVBUF` (which
  needs no capability) is the first thing to reach for if it ever does; if a future arm
  reaches past it for `SO_RCVBUFFORCE`, G6 must hear about it — the grant would then be for
  a reason N8 never predicted.
- **One host, one kernel, four cameras** (design §3.3 item 8). This says nothing about a
  container with a restricted netlink policy, an LSM that filters `AF_NETLINK`, or a
  network namespace with no uevent broadcaster. `docs/research`'s risk 5 remains open, and
  a build that cannot bind gets the typed `Error::DeviceIo` `sys::uevent::open` produces
  rather than a panic.

**Regression-tested by** `sys::uevent::tests::the_uevent_socket_binds_with_no_cap_net_admin_which_is_what_pf21_measured`,
which asserts the effective capability set does not contain `CAP_NET_ADMIN` *before* it
asserts the bind succeeds — so a run that held the capability fails rather than passing
vacuously, and "we did not try unprivileged" cannot be spelled the same way as
"unprivileged works" (AGENTS rule 7, applied to us rather than to the device).

**Retires when:** a kernel or a deployment target refuses the unprivileged bind, at which
point the test above goes red and the grant N8 removed has to be argued back on evidence
rather than on prediction.

---

## N53 — "Re-enumerated on event" is a *diff*, not a call to `enumerate`, and the debounce turns on direction rather than on the clock

**The question docs/7 left open.** Design §2.5 lists "filtered to subsystem `video4linux`,
debounced, **re-enumerated on event**" inside P4d's deliverable, on the V4L2 backend
(docs/6:710-713, docs/7:294-295). But `schema::backend::HotplugEvent`'s own doc says the
opposite thing about ownership — "**Re-enumeration decides what camera it belongs to** —
the event names a node, never a camera, because grouping is not a node property" — which
reads as the *consumer's* job, and the fake's watch enumerates nothing at all. Both
sentences are in the tree and they point different ways. This entry is the decision.

**Decided: the watch re-reads the node list itself, and the events it hands out are the
difference between successive readings.** It does *not* call `CameraBackend::enumerate`,
and the distinction is the whole answer:

| | `sysfs::nodes` — what the watch calls | `probe_nodes` → `enumerate` — what it does not |
|---|---|---|
| reads | `/sys/class/video4linux`, a kernel-maintained symlink farm | the same, **and opens every node for `QUERYCAP`** |
| holds | nothing | every `/dev/video*`, transiently |
| answers | which nodes exist | which *cameras* exist, grouped by USB interface \[PF:7, PF:19\] |

Three reasons the heavier one is out, each of which would otherwise be a defect somebody
discovers later:

1. **A watch that opened nodes would be a camera holder.** `wch-priv uvcvideo cycle`
   refuses to unload the driver while any process holds a `/dev/video*` open (design
   §2.13), so a watch that re-enumerated inside `next_event` would make P4d's own R3 arm
   fail against its own interlock — self-inflicted `Error::InUse`, arriving as "hotplug is
   broken". It would also contradict D12's "the daemon never opens a camera until first
   use": subscribing to events is not a use.
2. **`Ok(None)` at a deadline is the contract (E3), and the battery bounds it** at
   `deadline + 2 s`. A full sysfs-plus-`QUERYCAP` scan of ten nodes inside `next_event`
   spends that budget on work the caller did not ask for.
3. **A half-populated group is `enumerate`'s problem and not the watch's.** `probe_nodes`
   drops a whole group when one member is unreadable, deliberately (E3: a busy node must
   not be answered as a missing capability) — so re-enumerating *mid-burst* would produce
   cameras appearing and disappearing that nothing had plugged in. Diffing node paths has
   no such failure mode: a node is in the directory or it is not, and the events say
   nothing about cameras, which is exactly what `HotplugEvent`'s doc asks for.

So both sentences are satisfied. The design's "re-enumerated on event" is honoured — the
tree *is* read again on every burst — and the trait's "re-enumeration decides what camera
it belongs to" stays the consumer's, because a node path is all the watch ever emits.

**What a diff-based watch cannot report, stated rather than discovered.** Its events can
never be invented and can never leave the watch out of step with the tree, which is what
makes every dropped packet affordable (a lost packet is a lost *trigger*, and the next
trigger produces the full difference anyway). The price is the mirror image: **a node that
leaves and returns entirely between two readings is not reported at all**, because between
those two readings nothing changed. No diff-based watch closes that window.

### The debounce is a second settle-shaped fold, and that is not a second home

Design D5's settle policy is a pure fold on a caller-supplied clock in `engine::settle`,
and "one home per law" (design §2.10) would ordinarily make a second one a defect. It is
not one here, for two reasons that should be read together rather than either alone:

- **They are different laws.** `engine::settle` decides when a *control* has stopped
  moving, from frames and read-backs. `hotplug::Debounce` decides when the *kernel* has
  stopped talking, from packet arrivals. Neither could be expressed in the other's terms.
- **The DAG forbids sharing even if they were the same.** `webcam-handler-engine` is a
  `[dev-dependencies]` entry of `webcam-handler-v4l2` and `scripts/gates/dependency-walls.sh`
  keeps it that way. What is actually shared is the *shape* — an `Instant` parameter
  instead of a clock read, which is also what `sys::wait::until_readable` already takes —
  and a shape is not a home.

### The turn rule, and why the quiet window is not sized against `modprobe`

The obvious debounce is "fire when the socket has been quiet for N milliseconds", and the
obvious N is one that separates a driver cycle's removes from its adds. **That number does
not exist on this host.** From the PF:21 captures:

| gap | measured |
|---|---|
| largest gap *inside* the remove phase | **93.6 ms** (the Dell goes, then ~94 ms later the USB3 devices) |
| largest gap between two nodes of the *same* camera | 30.2 ms |
| last remove → first add, cycle 1 | 119.0 ms |
| last remove → first add, cycle 2 | **98.5 ms** |

93.6 and 98.5 are five milliseconds apart, and both are `modprobe` timings rather than
bounds. A window below them costs extra readings; a window above them coalesces a whole
`uvcvideo` cycle into one reading whose diff is *empty*, because the same ten nodes come
back — so the watch would report **nothing happened** for a cycle that took every camera
away, and docs/7's R3 arm ("one `uvcvideo` cycle … produces remove+add events through
`watch`") could not be satisfied at all.

So the window does not try. `limits::HOTPLUG_QUIET_MS` is 250 ms — chosen to coalesce
*any* same-direction burst on this desk with room over the 30.2 ms it actually has to
bridge — and what ends a burst early is the **turn rule**: a trigger whose direction
reverses the burst being coalesced ends that burst immediately, because the tree has passed
through a state the caller must be given the chance to see. With it, the same window is
right at 50 ms and at 2 s, which is what a constant sized against two samples of `modprobe`
is not. `crate::watch::Watch::drain` stops mid-drain when the turn fires; what is left stays
queued on the socket, so nothing is lost by stopping early.

`limits::HOTPLUG_MAX_DEFERRAL_MS` (2 s) is the other end: a failing hub in an add/remove
loop produces a trigger stream with no pause in it, and a quiet-only rule would defer the
reading forever — a hang wearing a debounce costume.

**One consequence for the R3 arm** (P4d's next step): because the turn fires on the *first*
add, the reading it forces already sees that node back — and every other node the kernel
managed to register between the packet arriving and the reading finishing. Those nodes are
not reported as having left. **Measured, twice, at two of ten on this host** (note E9): the
Chicony RGB interface's `video0` and `video1` were both back by the time the turn's reading
read the tree, so the run's headline number is 8 removals rather than 10. The count is a
race with `modprobe` and not a property, so the arm asserts *shapes* — at least one
`Removed` naming a path from the pre-cycle set, then at least one `Added` — never an exact
count and never a node by name. What it *can* assert exactly is the accounting: the
pre-cycle node listing with every delivered event applied reproduces the kernel's own
listing afterwards, which is this note's claim stated as an outcome rather than as a
mechanism.

**Proven by** `hotplug::tests::{a_burst_is_finished_when_the_socket_goes_quiet_or_when_the_ceiling_arrives,
a_trigger_that_reverses_the_burst_ends_it_whatever_the_clock_says,
out_of_order_instants_cannot_make_a_finished_burst_unfinished, a_lost_packet_costs_a_trigger_and_never_a_change,
a_failed_reading_does_not_spend_the_trigger_that_asked_for_it}` and
`watch::tests::{a_burst_of_node_removals_costs_one_reading_and_still_reports_every_node,
without_the_debounce_the_same_burst_costs_one_reading_per_packet,
a_drain_stops_at_the_turn_so_a_driver_cycle_is_not_coalesced_into_nothing}` — the second of
which is the inverse arm: `Debounce::new(ZERO, ZERO)` reproduces the undebounced build and
asserts ten readings for ten packets, so "the debounce coalesces" is a claim with a red
version. No test here sleeps; every instant is invented by the test and nothing waits on
one.

### Amendment, 2026-08-10: six corrections from the P4d adversarial review

All inside the watch this entry describes, and the first two changed behaviour.

**1. An unreadable packet lost the change it announced.** `Tracker::observe_packet` armed
the debounce only for a `NodeChanged`; `Trigger::Unreadable` bumped a counter and returned.
`Tracker::observe_lost`, one function below, did the opposite for a packet whose bytes never
arrived — `observe(None, at)`, which forces a reading. That asymmetry falsified this crate's
load-bearing sentence, "a dropped, oversized or unparsable packet costs a *trigger* and
never desynchronises the watch": for an unparsable one the cost was the whole event. The
refusals in `trigger` are deliberately liberal — `kobject-uevent` validates the entire
buffer, so one non-UTF-8 byte in a field this build never reads refuses a camera's `remove`
whole — and the safety net that makes liberal refusal affordable was not wired to the
refusing path. On a single-node camera, or when the corrupted packet is a burst's last, the
subscriber was told nothing at all. **Fixed:** `Unreadable` arms the debounce, and so does
`NotOurs::UnmodelledAction`, whose subsystem is unknowable by construction (the parser
refuses on `ACTION` before it reads `SUBSYSTEM`). `OtherSubsystem` and `OtherAction` still
do not — those are packets this build positively knows are not news, and they were 36 of 56
in a measured cycle \[PF:21\]. The cost is bounded by the debounce itself: one reading per
quiet window under any flood, which is the bound `Lost` already accepted. Driven by
`hotplug::tests::a_packet_this_build_could_not_read_still_costs_a_trigger_rather_than_the_change`,
whose three arms kill all four mutants of the match.

**2. The subsystem filter was unconstrained, and the fixture named as pinning it could not
pin it.** `VIDEO_SUBSYSTEM`'s doc forbids filtering on `DEVPATH` text, and two places named
`uevent-add-media-node.bin` as the proof. That fixture's `DEVPATH` is
`…/3-4:1.0/media0` — no `video4linux` substring — so a substring filter drops it correctly,
and **no** packet in the 56-datagram cycle has a non-`video4linux` `SUBSYSTEM` with
`video4linux` in its path. Measured on this tree: replacing `event.subsystem !=
VIDEO_SUBSYSTEM` with `!event.devpath.to_string_lossy().contains(VIDEO_SUBSYSTEM)` left all
22 hotplug and watch tests green, so the documented rule could be deleted with `just ci`
green. The doc also had the containment backwards — a `usb` interface's path is a *prefix*
of its `video4linux` child's, so the failure direction is a device hanging *below* a node,
not above it. **Fixed:** one synthetic packet, `SUBSYSTEM=input` with
`…/video4linux/video0/input1` as its path, plus its mirror (`SUBSYSTEM=video4linux` with no
`video4linux` in the path). Synthetic and said so: nothing on this desk emits either. The
mutant now dies; `fixtures/README.md` and the test comment say what the media packet
actually pins.

**3. `Trigger::NodeChanged::devpath` was a typed declaration nothing read.** Its doc named
three consumers — the debounce's diagnostics, a log line, the R3 arm's transcript — and none
exists: `Debounce::observe` takes a direction and an instant, this crate has no logging
dependency (which is what `Counts` is *for*), and `Trigger` is `pub(crate)` and unreachable
from `tests/hardware.rs`. Replacing the initialiser with `String::new()` left every
production path in the workspace identical and only this module's own unit tests noticed.
Rubric A8, and worse than inert: a heap copy of attacker-influenced kernel text kept alive
past the parse is what made "no packet text becomes a path" a convention rather than a fact
about the type. **Deleted.** `NodeChanged` carries a direction and nothing else, and
`a_hostile_devpath_never_becomes_a_slash_dev_path_because_nothing_here_makes_one` now
asserts the two triggers are *equal* — a benign packet and one whose every path-shaped field
is hostile produce the same value.

**4. The watch's node source read forty files it threw away.** `SysfsNodes::list` called
`sysfs::nodes`, which `canonicalize`s each node's `device` link and reads `idVendor`,
`idProduct` and `serial` under it, and kept only `dev_path` — ten nodes × four operations
per rescan, on the path a hotplug burst provokes, against a tree the driver is still moving.
Two docs said "opens nothing" and "opens no node"; the second was true and the first was
not. **Fixed:** `sysfs::node_paths` is the `read_dir` on its own, sharing `node_names_in`
with `nodes_in` so the `video` filter and the numeric sort keep one home —
`the_cheap_node_listing_and_the_full_one_agree_on_the_population` is what goes red if either
grows a second copy.

**5. `MAX_UEVENTS_PER_DRAIN` documented a bound on `next_event` that nothing enforced.**
The constant is read by `drain`, and `next_event` calls `drain` inside a loop whose exits
are an event, an error, or the deadline — so a storm of `NotOurs` packets never arms the
debounce and the call reads for as long as the caller was willing to wait. The behaviour is
right (datagrams nobody reads become `ENOBUFS`, so draining a storm is work the watch has to
do) and the **doc** was wrong. Corrected to say the bound is one pass and the caller's
deadline is what bounds the call, and
`watch::tests::a_flood_of_other_peoples_packets_is_all_read_and_still_answers_at_the_deadline`
now drives the claim at the layer it is made at: every queued packet read, nothing dropped,
deadline still honoured.

**6. `Watch`'s derived `Debug` printed the 8 KiB receive buffer.** `Counts`' own doc offers
a holding layer's `Debug` as the production read path for the counters, and taking that
invitation emitted ~30 KB per line whose live prefix was the last datagram — a neighbouring
subsystem's `PRODUCT=`, `MODALIAS=` and serial strings, recorded by a daemon nobody asked to
record them. Hand-written now: socket, tracker, and the buffer's *length*, which is the
shape `sys::mmap` already uses.

**The mutation floor is owed a re-run, and this says so rather than letting it be
assumed.** These corrections edit `hotplug.rs`, which is inside `.cargo/mutants.toml`'s
`examine_globs`, so the floor's population moved with them (still 36 mutants for the file,
last in a queue of 515). Two attempts on this desk did not produce a usable verdict: the
first used four jobs against a 14 GiB tmpfs and turned 267 mutants into build failures —
340 "unviable" against the 73 the P4d run recorded, and nine acceptances reported as no
longer surviving purely because they were never tested; the second was correctly configured
(`/dev/shm`, three jobs) and was on track but could not be run to completion here. What
*is* established is narrower and was watched rather than inferred: the four hand-applied
mutants of exactly the logic these corrections changed — the subsystem comparison replaced
by a `DEVPATH` substring, `Unreadable` not arming the debounce, `UnmodelledAction` not
arming it, and `OtherSubsystem`/`OtherAction` arming it — each go red, and the two
`daemon::uds` edits were watched the same way (dropping `O_DIRECTORY`, silencing the
downgrade warning). `just mutants` at the commit boundary is the remaining obligation, and
`crates/api/src/codes.rs:160` was the only survivor either attempt reached, which is the
one acceptance N37 already carries.

One correction outside this entry's subject but found with it: `Fd::open` passed
`libc::O_RDWR` alone, and `v4l::v4l2::open` hands its flags to `open(2)` unchanged (checked
in the pinned 0.14.0 source), so every `/dev/video*` this crate opened was inherited across
`exec` — while P4d's netlink socket set `CLOEXEC` and said why. The camera node is the more
valuable descriptor by that argument: it is D12's exclusive-access capability and the thing
`wch-priv`'s unload interlock counts holders of, and P4d is the first commit in this crate
to spawn a child. `libc::O_CLOEXEC` is set, pinned by
`sys::tests::a_device_descriptor_is_not_inherited_by_anything_this_process_execs` off
`F_GETFD` rather than off the flags argument (AGENTS rule 5: requested is not applied).

**Retires when:** something makes the node-path diff the wrong answer — a kernel that
recycles a node name for a different device inside one burst would do it, and would need
the event to carry more than a path, which is a `schema::backend` change and not a backend
one.

---

## E9 — G4 hardware evidence: hotplug through the real backend, 2026-08-10

docs/7's P4d asks for the R3 hotplug run — "one `uvcvideo` cycle via the blessed helper
with every camera closed produces remove+add events through `watch` (the interlock
honored)" — and docs/9's netlink row says that arm "is evidence-recorded". This is that
record, under the carve-out G1, G2 and G3 already used: *the recipe existing and selecting
tests is the gate criterion, and the run itself is evidence, not CI-gating*. Evidence
entries are dated and appended; they are not amended.

**Host:** kernel 7.0.0-29-generic, x86_64, uid 1000. **Attached:** Chicony `04f2:b83c`
(RGB on `3-4:1.0`, IR on `3-4:1.2`), OBSBOT Tiny 3 `3564:ff02` on `3-1:1.0`, Dell
U3224KB/A on `2-3.4.1.1:1.0` \[PF:19\]. Four logical cameras, ten `/dev/video*` nodes.
**Helper:** `.wch-bin/wch-priv`, mode `700`, `blessing: cap_sys_module,cap_net_admin+ep`,
`can delegate to a child: yes (ambient raise verified)`.

### The measurement that came first, and is this entry's headline

Design §8 item 10 asked whether the uevent socket needs `CAP_NET_ADMIN`, and P4d took the
answer before writing a line of the arm: **it does not**. `socket(AF_NETLINK, SOCK_DGRAM |
SOCK_CLOEXEC, NETLINK_KOBJECT_UEVENT)` + `bind(nl_pid=0, nl_groups=1)` succeeds as uid 1000
with `CapEff: 0000000000000000`, and three separate `uvcvideo` cycles then **delivered all
56 packets** to that unprivileged listener with `ENOBUFS` never raised. The full transcript,
the per-subsystem census and the packet shape are \[PF:21\]; N8's row predicting the
capability is disproved there and carries the amendment. The consequence for this arm is
mechanical and worth stating in one sentence: **the test process binds its own socket as an
ordinary process and spawns the helper as a subprocess** — no `wch-priv exec` wrapper, no
managed recipe, and the hotplug arm therefore spends no capability of its own. The helper is
still needed, for `modprobe` and `cap_sys_module`, which nothing here narrows.

### R3 — `just smoke-hw`, motors included

```
smoke-hw: motor-moving suites (hw_motion_*) are included — set WCH_NO_MOTION=1 to exclude them
smoke-hw: 10 capture node(s) present; running test(/(^|::)hw_/)
    Starting 16 tests across 34 binaries (835 tests skipped)
     Summary [  53.806s] 16 tests run: 16 passed, 835 skipped
smoke-hw: 7 claim(s) declined by tests that ran — each named above
smoke-hw: suite run, 0 named skip(s) before it started
```

Fifteen arms before this sub-milestone, sixteen now. The new one's own transcript, verbatim:

```
before: 4 camera(s) on 10 node(s): 2-3.4.1.1:1.0, 3-1:1.0, 3-4:1.0, 3-4:1.2
  event: Removed { path: "/dev/video2" }
  event: Removed { path: "/dev/video3" }
  event: Removed { path: "/dev/video4" }
  event: Removed { path: "/dev/video5" }
  event: Removed { path: "/dev/video6" }
  event: Removed { path: "/dev/video7" }
  event: Removed { path: "/dev/video8" }
  event: Removed { path: "/dev/video9" }
  event: Added { path: "/dev/video2" }
  event: Added { path: "/dev/video3" }
  event: Added { path: "/dev/video4" }
  event: Added { path: "/dev/video5" }
  event: Added { path: "/dev/video6" }
  event: Added { path: "/dev/video7" }
  event: Added { path: "/dev/video8" }
  event: Added { path: "/dev/video9" }
  cycle: uvcvideo: cycled; 10 node(s) before, 10 after
cycle seen through watch: 8 removal(s), 8 arrival(s) — /dev/video2 then /dev/video2
after: 4 camera(s) back on the same 4 bus path(s)
```

The helper printed neither of its two `warning:` lines, so the unload took and the nodes
were all back inside its settle deadline. **Eight and not ten**, in both directions of the
knob and in every run taken today: the turn rule ends the burst on the *first* `add`, and by
the time that reading finishes reading `/sys/class/video4linux` the Chicony RGB interface's
`video0` and `video1` are already registered again. Note N53 predicted one node of ten; the
measurement says two, and N53 now says so.

### R3 — `WCH_NO_MOTION=1 just smoke-hw`

Both directions are run because "the knob excludes the motion arms" and "the exclusion is
named and counted" are different claims and only running it proves the second.

```
smoke-hw: SKIP 1 — motor-moving suites (hw_motion_*) are excluded by WCH_NO_MOTION=1; unset it to include them
smoke-hw: 10 capture node(s) present; running test(/(^|::)hw_/) - test(/(^|::)hw_motion_/)
    Starting 15 tests across 34 binaries (836 tests skipped)
     Summary [  38.365s] 15 tests run: 15 passed, 836 skipped
smoke-hw: 5 claim(s) declined by tests that ran — each named above
smoke-hw: suite run, 1 named skip(s) before it started
```

16 with motors and 15 without; 7 declined claims and 5. The difference is one named,
counted skip and the two partial skips that belong to the excluded arm, rather than a
smaller number nobody noticed. The hotplug arm's transcript is byte-identical between the
two runs — it does not move a motor and does not care about the knob.

### The skips are exercised, not described

A hardware arm's skip is the thing most likely to be written once and never run, and a skip
that reads as pass is the failure `smoke-hw`'s counter exists for. Two of this arm's four
declines were driven for real on this host:

```
$ mv .wch-bin/wch-priv .wch-bin/wch-priv.hidden && cargo nextest run … -E 'test(/hw_hotplug_/)'
SKIP: no blessed helper at .wch-bin/wch-priv; run `just bless` (sudo once) — this arm cannot cycle a driver without it
```

```
$ python3 -c "import os,time; fd=os.open('/dev/video0', os.O_RDWR); time.sleep(25)" &
$ .wch-bin/wch-priv uvcvideo status --json
{"module":"uvcvideo","loaded":true,"holders":[{"pid":3729222,"comm":"python3","node":"/dev/video0"}]}
$ cargo nextest run … -E 'test(/hw_hotplug_/)'
before: 4 camera(s) on 10 node(s): 2-3.4.1.1:1.0, 3-1:1.0, 3-4:1.0, 3-4:1.2
SKIP: 1 process(es) hold a camera open ([{"comm":"python3","node":"/dev/video0","pid":3729222}]); the uvcvideo interlock refuses a cycle and this arm does not force it
```

The second is the interlock honored rather than fought: a real other-process holder was
present, the arm asked the helper, declined, and **did not cycle the driver** — no
`--force`, which is an operator affordance and never a test one. The helper was restored
intact after the first (`blessing: cap_sys_module,cap_net_admin+ep`, unchanged); the holder
released `/dev/video0` on its own and the desk was back to `0 camera holder(s)`.

### The arm was watched failing, twice, against real hardware

An `#[ignore]`d evidence arm is exactly where a decorative assertion hides, so both of its
claims were driven red by hand-written mutants on this desk before it was called done —
each caught by a *different* assertion, which is the point of having two.

| mutant | what went red |
|---|---|
| `Debounce::observe`: `self.turned = true` → `false` (the turn rule deleted) | `a uvcvideo cycle produced 0 event(s) through the real watch, which is not a removal followed by an arrival: []` — after 6.0 s of a 6 s budget. The whole cycle coalesced into one reading whose diff was empty, which is precisely the failure note N53 argues the turn rule exists to prevent, now observed rather than reasoned about. |
| `Tracker::rescan`: `self.known.difference(&now)` → `self.known.iter()` (every known node reported gone) | the ordering assertion still passed — 12 removals then 8 arrivals — and the **accounting identity** caught it: `applying the watch's 20 event(s) to the pre-cycle node list does not reproduce the kernel's own listing`, `left` short of `/dev/video0` and `/dev/video1`. |

Both mutants were reverted and the arm re-run green. The second is the reason the arm does
more than count events: a defect can produce a plausible-looking removal-then-arrival
sequence and still leave a subscriber holding a tree the kernel does not have.

### What the run establishes

- **`CameraBackend::watch` is real on the V4L2 backend, and a kernel proved it.** The
  socket, the group-1 bind, the `SUBSYSTEM=video4linux` filter, the debounce, the turn rule
  and the node-list diff are one working path from a `modprobe` to a `HotplugEvent`. Until
  today the v4l2 half of that path had only ever been driven by committed packets; the fake
  had passed the battery's `HotplugWatch` arm since P0, and parity now means the same thing
  for both backends.
- **The events are re-enumeration, not packet contents** (note N53), demonstrated as an
  outcome: the pre-cycle node listing with all 16 delivered events applied reproduces the
  kernel's own listing afterwards. Nothing a packet said became a path — a hostile
  `DEVNAME` has no route to a `HotplugEvent` here, which is rubric B10's requirement
  discharged by construction rather than by validation.
- **The debounce is load-bearing on real timings and not only on invented ones.** One cycle
  is 56 packets, 20 of them ours, arriving across ~300 ms \[PF:21\]; the watch answered with
  two readings of the tree.
- **The interlock is designed around.** The arm holds no `/dev/video*` descriptor for its
  whole duration — its enumeration opens and closes each node before the watch is opened,
  and the watch's own descriptor is an AF_NETLINK socket, which is the fact that makes the
  arm possible at all. It never streams a frame; no camera image existed at any point.
- **The desk was left as it was found** (AGENTS rule 8, applied to the driver): four cameras
  on the same four bus paths, ten nodes on the same ten minors, `uvcvideo: loaded; 0 camera
  holder(s)`, after every one of the seven cycles this session ran — the two mutant runs
  included, each of which cycled the driver before failing its assertion.

### What it does not establish

- **Mid-stream device loss stays the fake's, by design and not by omission.** Design §3.3
  item 9 says a camera that dies while a stream is running is "modeled, not measured", and
  the helper's interlock makes it unarrangeable: unloading `uvcvideo` requires every node
  closed, and a stream holds one. So this arm proves the half the interlock permits —
  add/remove with every camera closed — and `Fault::DeviceGone` remains scripted. That is
  the split the design drew, honored literally; the arm's doc comment says which half is
  which so a future reader does not read the gap as a hole.
- **A driver cycle is not a physical unplug.** The USB device never left the bus, so the
  `usb` subsystem's own removal traffic in a real unplug — and whatever a hub or a cable
  fault would add — is unmeasured. What a cycle reproduces faithfully is the
  `video4linux` half, which is the half `watch` filters on.
- **Node renumbering is untested.** Every cycle taken today returned the same ten minors in
  the same order, so the arm's insistence on fingerprints and on shapes rather than names is
  *correct by argument* and has never been exercised by a kernel that disagreed. A guard
  that exists and has not fired.
  **Fired since (2026-08-11), see \[PF:22\] and note N63:** a later reload rotated three of
  the four cameras through each other's minors. This arm's fingerprint-and-shape discipline
  held; the R3 *enumeration* arm, which asserted the names, did not.
- **The receive-buffer overrun path has never fired.** `Received::Overrun` is a modelled
  outcome and the flood claim is a unit test against a socket, not a kernel: three
  independently captured cycles fitted the default 212992-byte buffer \[PF:21\], and no run
  on this desk has raised `ENOBUFS`. The guard exists; the machine has not been busy enough
  to test it.
- **The debounce's ceiling has never fired either.** `limits::HOTPLUG_MAX_DEFERRAL_MS`
  exists for a failing hub in an add/remove loop, and no hardware here loops.
- **Still one host, one kernel, four cameras** (design §3.3 item 8) — and one driver:
  everything above is `uvcvideo`. A `vivid` node's uevents are the same subsystem and would
  take the same path, which is an argument and not a measurement.

---

## E10 — The mutation floor's third run, over the scope P4d widened, 2026-08-10

E8 ends "the next widening writes its own entry", so this is that entry. E7 commissioned
the floor over six pure cores; P4a widened it to nine with `crates/api/src/{codes,photo,
wire}.rs` (E8). P4d makes it ten, and the tenth is the first from a **backend**.

### The widening, and the argument that had to be written down

One line: `crates/backends/v4l2/src/hotplug.rs`. `.cargo/mutants.toml`'s header rules out
silence about a new file, so the choice was made out loud rather than left to whoever reads
the globs next. It belongs for the same reason the other nine do — `hotplug::trigger` takes
a byte slice and returns a value, and `Debounce`/`Tracker` fold over `Instant`s the caller
supplies. It opens nothing, reads no clock and makes no syscall.

Its two neighbours stay out, and that split is the point of the module boundary rather than
an accident of it:

- `sys/uevent.rs` is the socket, excluded with the rest of `src/sys/` — "only decidable
  against a device". Putting the packet *decision* in `hotplug.rs` instead of in the socket
  module is what kept a pure fold inside the floor's reach (note N53).
- `watch.rs` is the blocking loop, excluded with its own reason: what is left in it after
  the folds moved down is a clock, a `poll` and a `recv`, so its mutants are **timing**
  survivors — the class note N52 records this floor being bad at.

### The run

**515 mutants in 33 minutes: 431 caught, 11 survived, 73 unviable, 0 timed out**, judged by
the whole 828-test workspace suite, three parallel jobs on an eight-core machine. E8's run
was 478 mutants in 21 minutes at four jobs; the thirty-seven new ones are `hotplug.rs`'s
(P4d's deletion of `Unimplemented` also took a few of `codes.rs`'s with it), and the extra
wall clock is the job count, not the scope.

The register comparison runs clean in both directions — **eleven survivors, eleven
acceptances, nothing unexpected and nothing stale**. All eleven are the ones E7 and E8
already triaged (N25's six, N26's three, N27's one, N37's one). `hotplug.rs` contributed
none.

### What the widening bought, measured — and a note on how it was found

It bought one real defect, on the first run over the file: **`Tracker::next_deadline`
replaced with `None` survived the entire suite.**

`Debounce::next_deadline` is asserted four ways one module down, so the fold itself was
covered. What nothing asserted was that the `Tracker` *forwards* it, and — the part that
matters — what forwarding is for. `Watch::next_event` waits for whichever comes first,
something to read or the burst going quiet; with `None` the second half disappears and the
budget is always the caller's deadline. The events are not lost and the tree never falls out
of step (note N53's whole argument), so every existing test still passed: a settled burst is
simply handed over when the *caller* gives up rather than when the *burst* does. On a
subscriber polling with a generous deadline that is the difference between a quarter of a
second and however long it asked for.

It is neither equivalent nor unkillable, so it is not in `scripts/mutants-accepted.txt`.
`watch::tests::a_settled_burst_is_reported_when_it_settles_and_not_when_the_caller_gives_up`
kills it: one remove packet queued on a datagram pair, a 20 ms quiet window, a ten-second
deadline, and the assertion that the answer came back in well under half of it. Watched
failing against a hand-applied copy of the mutant — **10.010212057s**, the whole deadline —
then green at 23 ms with the mutant reverted, then confirmed by a narrowed re-run over the
file alone: 36 mutants, 24 caught, 12 unviable, **0 missed**.

Two things this says beyond the fix. The floor found a class the nine hand-written mutants
of P4d step 2 did not: those were all about *what* the watch reports, and this one is about
*when*. And the survivor sat exactly on the seam between two modules — the fold was tested,
the loop was tested, and the delegation between them was the gap — which is the shape a
file-by-file reading of a diff is worst at seeing.

### What it does not establish

- **Three jobs, not four or five.** Note N52's measurement stands: the verdict moved with
  `nproc` until `minimum_test_timeout = 180.0` was pinned. It is pinned, 0 mutants timed
  out, and the register was clean — but this run does not re-measure N52's claim, it rests
  on it. (The first attempt of this run died at 5 jobs on `ENOSPC`, a `tmpfs` build root too
  small for five trees at about 3 GiB each. That is an environment fact, not a verdict.)
- **`watch.rs` and `sys/uevent.rs` are still unexamined by this job**, deliberately and with
  their reasons in the scope file. Their claims rest on the hand-written mutants of P4d step
  2 and on the R3 arm (note E9), not on this floor.

### Re-run at the sub-milestone boundary, and N52's claim measured rather than rested on

The run above was judged by the suite as it stood mid-sub-milestone; the P4d review then
added tests for seventeen findings, so the tree that ships is not the tree that was judged.
A floor result that predates the code it certifies is the same species of stale as an
acceptance nobody re-checks (N15), so the whole job was run again against the committed
tree, from a clean build root:

**515 mutants — 431 caught, 11 missed, 73 unviable, 0 timed out; 11 survivors, 11 recorded
acceptances, register clean both ways. PASS.**

Identical in every count to the run above, and that identity is the second measurement this
entry can offer. The section above says "this run does not re-measure N52's claim, it rests
on it" — the boundary re-run **does** re-measure it, because it used **eight** jobs where the
first used three. Same tree, same scope, 3 jobs and 8 jobs, one verdict. Before
`minimum_test_timeout = 180.0` that same comparison produced FAIL-with-31-unaccepted against
PASS (N52's table). The pin holds at a scope 35 mutants larger than the one it was measured
on, which is the property that matters: it was fixed for the workspace, not for a run.

The 828-test figure above is therefore left as written rather than corrected — it is what
that run was judged by, and the boundary re-run's 835 is what this commit is judged by. An
evidence entry that quietly updates its own numbers stops being evidence.

**Retires when:** never — this is dated evidence. The next widening writes its own entry.

## N54 — P4d was two sub-milestones wearing one name, and the review's *falling* false-positive rate is how you can tell

**Believed:** that a sub-milestone is the right size when it is one session's work, and that
docs/7's "split-don't-stretch" rule is enough to keep it there. The plan's own risk register
says so: "The residual risk is a sub-milestone mis-sized anyway; the notes record splits so
sizing improves." This entry is that record, written because P4d did not split and should
have.

**True:** P4d was mis-sized at the moment it was *written into the plan*, not at the moment
it was executed, and the reason is visible in its own sentence. Its "Lands" clause carries
four items:

1. the AF_NETLINK uevent socket, `kobject-uevent` parsing, subsystem filter, debounce,
   re-enumeration, and `CameraBackend::watch` on the real backend;
2. **the measurement** — bind `NETLINK_KOBJECT_UEVENT` unprivileged and record whether N8's
   `CAP_NET_ADMIN` grant was ever needed;
3. **the deletion** of `Error::Unimplemented`, a cross-cutting change touching the schema,
   the error registry, the wire code block, a backend's pinned surface, two committed
   artifacts, three documents and a standing debt;
4. and — inherited, from a different sub-milestone — note N39's socket-directory hardening.

Those four share a *topic*: they are all kernel-facing, or near enough to look it. They do
not share a *story*. Contrast the two before it. P4b was "a daemon skeleton" and every
clause served it; P4c was "route the whole surface" and every clause served that. P4d is
"the things left over that touch the kernel", which is an inventory, not a milestone. **A
sub-milestone assembled by what the code touches will always look coherent to its author
and behave like two to whoever runs it**, because the coupling that made it one bullet is a
coupling between files rather than between decisions.

### What it cost, measured

Orchestration wall-clock across the four P4 sub-milestones, same shape of harness each
time, same reviewer count:

| | P4a | P4b | P4c | **P4d** |
|---|---|---|---|---|
| agent hours | 4.7 | 5.3 | 5.5 | **7.3** |
| tool calls | 1309 | 1334 | 1426 | **1663** |
| findings | 25 | 24 | 23 | **17** |
| findings rejected as not-real | 3 | 1 | 1 | **0** |

P4d is a third longer than the longest of the other three, and that is the *cheap* half of
the cost. The expensive half is below.

### The tell, which points the other way from intuition

**P4d's review produced the fewest findings and rejected none of them.** Every one of the
seventeen was real. Across P4a-P4c the same reviewer harness had produced more findings and
been wrong about several — a reviewer with a small diff reaches for marginal claims, and
some of those claims are wrong.

The tempting reading is "P4d was reviewed better". The likelier reading is the opposite: a
reviewer facing a diff of 48 files, a new unsafe-adjacent edge, a wire-contract deletion and
a hardware rung **never runs out of real material**, so it never has to reach — and it also
never gets to the *end*. A zero false-positive rate on a large diff is not a quality signal;
it is a **saturation** signal, and it says the review stopped for lack of budget rather than
for lack of defects. The three sub-milestones that produced wrong findings were the three
whose reviewers had read everything and were down to guessing.

That is falsifiable and nobody has falsified it: it predicts that splitting a large
sub-milestone in two and reviewing each half raises the total finding count *and* the
false-positive rate. Worth measuring the next time a split happens, rather than believing
this paragraph.

### The other costs, named so the next plan can price them

- **Terminal verification multiplied.** P4d ended owing `just ci`, `just smoke-hw`,
  `just rung-vivid-managed` and the mutation floor — because it touched the daemon, the
  hardware path, the sysfs walk and a floor-scoped file. Two agents tried the floor and
  neither finished; one reported it unrun while an earlier agent had in fact completed it
  and written E10 with real numbers. Nobody was lying and the disagreement was structural:
  a sub-milestone with four terminal rungs has four chances to end in an ambiguous state.
- **Inherited debt rode along.** N39's hardening had nothing to do with hotplug. It was
  scheduled into P4d because P4d was "the one that needs syscalls", which is again a
  file-shaped reason. Debts should land where their *subject* lands, or in a sub-milestone
  of their own.
- **Contract-file claims propagated.** Two statements written into the scratch contract
  during P4d were wrong and reached a draft: a debounce window sized off a single timing
  sample, and "three orders of magnitude to spare" on a receive buffer that measurement put
  at about sixteen times. Both were caught, both by re-measuring rather than by re-reading.
  A larger sub-milestone gives a wrong intermediate claim more room to travel before
  anything meets it.

### What the next plan revision should do

Not "make sub-milestones smaller" — that is unfalsifiable advice. Specifically:

1. **Size by story, not by subsystem.** If a sub-milestone's "Lands" clause needs the word
   "and" between two things that a reviewer would hold in mind separately, it is two. P4d
   splits cleanly at the seam: *hotplug* (the socket, the parse, the filter, the debounce,
   the watch, the fixtures, the R3 arm) and *the deletion* (`Unimplemented`, its surfaces,
   its wire code, its documents). The measurement belongs with hotplug because hotplug is
   what needs the socket; N39 belongs wherever the socket directory is next opened.
2. **Count terminal rungs at planning time.** A sub-milestone owing more than two of
   {`just ci`, `smoke-hw`, `rung-vivid-managed`, the mutation floor, an R3 evidence run} is
   a sub-milestone whose *ending* is as expensive as its middle, and the plan should say so
   in the "Proves" clause rather than discovering it.
3. **Never schedule an inherited debt into a sub-milestone for a file-shaped reason.**

**Doc:** docs/7's "Milestones are session-sized" convention and its risks section are the
subjects; this entry is the observation they asked for. The gate letters and criteria are
untouched — this is about how work is *cut*, not about what it must prove.

**Retires when:** never by disproof; it is superseded if a later plan revision adopts the
sizing rule above and a subsequent sub-milestone still behaves this way, which would mean
the rule is wrong and the cause is elsewhere.

## N55 — The session-GC trigger fired, and firing it proved the trigger cannot tell what fired it

**Believed:** docs/7's post-plan trigger table commissions session garbage collection on
**"a full disk"** (design §8.8, §7's deferral). The reasoning was sound and deliberately
lazy: calibration sessions accumulate photos, nobody knows the real growth rate, and
guessing a retention policy before anybody has a full disk is how you get a policy that
deletes the wrong thing. So the trigger waits for the world to answer.

**True — the trigger fired twice, and it should not have.** Both events are recorded here
because "a full disk" is exactly what happened, and a trigger that fires is a trigger that
gets recorded whatever the reader thinks of it afterwards:

- **2026-08-09, during P4c.** `scripts/mutants.sh` aborted with `Disk quota exceeded`
  writing to `/tmp`. The build root was a 16 GiB `tmpfs` and cargo-mutants gives each job a
  whole tree with its own build directory.
- **2026-08-10, during P4d.** The mutation floor's first attempt died on `ENOSPC` at five
  jobs, same build root, same cause.

**Neither was session data, and this is the part worth the entry.** Measured on the machine
that filled, the same day:

| | size |
|---|---|
| `~/.local/state/webcam-handler` (the entire session store, five sessions) | **904 KiB** |
| `target/` | 79 GiB |
| root filesystem | 85% used, 102 GiB free |

Session GC, run at its most aggressive, would have freed **904 kilobytes** against a
shortfall measured in gigabytes. The disk that filled was a `tmpfs` scratch directory that
the product does not write to and the design does not model.

### The finding: the trigger names a symptom, so it cannot name its own cause

"A full disk" is an observation about a *machine*. Session growth is a fact about *this
program*. Any cause fills a disk — a build tree, a core dump, someone else's container
images — so a symptom-shaped trigger fires on all of them and discriminates none. It has
two failure modes and this is the first one: it fires spuriously, and whoever reads it has
to do the measurement the trigger should have described. The second is worse and follows
from the first: a trigger that has cried wolf gets read past, so the one time session data
*is* the cause, the firing looks like the last two.

**And the measurement that would settle it does not exist.** Nothing in the product reports
the size of the session store — not `wch calibrate list`, not the store module, not a gate.
That is the real gap this firing exposed. If session data ever does fill a disk, nobody will
be able to tell that it did, because the quantity the trigger is about is not one anything
measures. The deferral was reasonable; deferring the *instrumentation* with it was not, and
that is a cheaper thing to land than a retention policy.

### What this does and does not commission

It does **not** commission session GC. Two spurious firings are not evidence that sessions
grow, and building a retention policy on this record would be building it on a `tmpfs` that
had nothing to do with us — the exact mistake §7 deferred the work to avoid.

What it argues for, for the owner and the next plan revision:

1. **Re-phrase the trigger in terms of the quantity it means**: "the session store exceeds
   *N*", or "a session store larger than the media it holds" — something a program can
   evaluate, not something a human has to attribute after the fact.
2. **Land the measurement before the policy.** A size in `calibrate list`'s answer, or a
   store method with a test, costs little and turns the trigger from a story into a number.
   Until then the trigger is unevaluable in the direction that matters.
3. Keep the row in docs/7 — it is still uncommissioned. It now carries a pointer here, so
   the next firing starts from a measurement rather than from this paragraph.

**Doc:** docs/7's post-plan trigger table, row "Session GC"; design §7 and §8.8.

**Retires when:** the trigger is re-phrased against a measurable quantity, or session GC is
commissioned on evidence that sessions are what filled something.

---

## N56 — The bounded enqueue is one mechanism wearing three names, and it is the caller's thread that waits

**Doc:** D12 says "a second capture request queues or is refused with `Busy` per its `wait`
flag" (docs/6:524-528) and rubric B3 makes it a review row. AGENTS says "Bounded
everything… constants live in `webcam-handler-schema::limits` and something reads each one",
and "No `sleep` as synchronization, anywhere, including tests". docs/7's standing debts
carry the flag and the race as two entries; notes **N42** and **N51** are those entries.

**They were never two.** N42's second item — "`CameraActor::submit` is a `try_send` on a
bounded `SyncSender` and has no blocking-with-deadline path at all, so `wait: true` is not a
branch, it is an enqueue that waits, plus the bound AGENTS requires of anything that waits"
— is the same missing mechanism N51's amendment names from the other side: "closing this
race properly wants the actor-command bound too, and shipping half of it would trade a wedge
for a leak". P4e-i lands them in one commit for a story-shaped reason rather than a
file-shaped one (note **N54**'s third rule): the story is *nothing a client does can wedge
the daemon*, and half of it is a leak.

### Repo: what landed

- **`engine::actor::Enqueue`** — `Refuse`, or `WaitUntil(std::time::Instant)`.
  `CameraActor::submit` is `submit_with(_, Refuse, _)` and is byte-for-byte the behaviour
  every existing caller had. `submit_with` is the new door.
- **`engine::actor::Room`** — a `Mutex<Seats>` and a `Condvar` beside the existing
  `sync_channel`. The queue's bound is still the channel's
  (`limits::CAMERA_COMMAND_QUEUE_DEPTH`); what `std::sync::mpsc` is missing is only the
  *wake-up*, because `SyncSender` offers a send that blocks forever and one that never
  blocks and `send_timeout` is unstable on the pinned toolchain. A permit pool was
  considered and rejected: a second count of the same eight seats is a second answer to "is
  there room" that can drift from the first.
- **`limits::CAMERA_ENQUEUE_WAIT_MS = 10_000`**, priced against two other constants in the
  same module — a caller that waits must be able to outlast the worst case of the command in
  front of it (`DEFAULT_SETTLE_DEADLINE_MS + FRAME_DEADLINE_MS`, seven seconds), and must
  not pretend to outlast a sweep, which is minutes. A `const` assertion checks the first
  relation where all three numbers are. Its one reader is `Enqueue::waiting`.
- **`schema::capture::PhotoRequest::wait`**, `#[serde(default)]` like its three siblings, so
  no request written before it exists became invalid. Both `schemas/` artifacts moved with
  it and `required` did not.
- **`daemon::server::enqueueing`** — the one place the field is read, and
  `Wchd::on_resolved_camera_queueing` the one place it is honoured.

### Why the waiting happens where it does

**The actor still reads no clock.** The only `Instant::now()` is on the thread that *chose*
to wait, computing how much of its own budget is left — which is the same doctrine
`engine::settle` states ("the caller supplies both, which turns *the deadline expired between
these two frames* from a race into an argument"), not an exception to it. A `Millis` from
`crate::settle::Clock` could not have carried this: `SteppedClock` is deliberately not
`Sync`, and a wait is the one thing here that spans two threads.

**The daemon parks a blocking-pool thread, never a runtime worker.** `submit_with` with a
deadline blocks by construction, so the waiting arm goes through `Wchd::offload`. A request
that did not ask to wait pays nothing at all — no lock, no pool thread, no extra `await` —
which is what keeps the flag from being a tax on the ordinary path.

**Amended 2026-08-10 (note N59): the sentence that stood here — "the number of parked pool
threads is bounded by `limits::DAEMON_MAX_CONNECTIONS` (32) against tokio's own pool" — was
not a bound this build had.** It was arithmetic about HTTP/1.1, which answers one request per
connection; P4e-i lifted `ServerConfig::http_only()` in the same sub-milestone, and
jsonrpsee's WebSocket transport `tokio::spawn`s a task per inbound message with no
per-connection concurrency cap at all (`jsonrpsee-server-0.26.0/src/transport/ws.rs`,
`ServerConfig` — `max_connections` bounds connections, `max_subscriptions_per_connection`
bounds subscriptions, nothing bounds calls). One connection could therefore hold thousands of
`wch_photo {"wait": true}` in flight, the real ceiling being tokio's 512-thread blocking pool
with an unbounded queue behind it — the same pool every `offload` in the daemon draws on, so
`wch_list` from an unrelated client queued behind the flood. The bound now exists rather than
being inherited: `limits::CAMERA_ENQUEUE_WAITERS`, a permit pool in `daemon::server`, sized to
`DAEMON_MAX_CONNECTIONS` precisely so the sentence above is true by construction instead of by
an accident of which transport a client chose. A request past it takes the enqueue a request
that never asked to wait takes, so the refusal vocabulary does not grow.

**A waiter whose thread dies is `DeviceGone`, not `Busy`** (E3). `Liveness::drop` bumps the
counter and notifies *after* it lowers `alive`, and `send_waiting` re-reads `is_alive` after
each wake — because the drop guard runs before the inbox `Receiver` is dropped, so for one
moment a dying actor still answers `Full`. Without that ordering a waiter would spend its
whole budget waiting for room that is never coming and then be told the camera was busy.

### The tests, and how each one goes red

- `a_full_queue_refuses_a_caller_that_will_not_wait_and_one_whose_deadline_has_passed` — the
  refusing half, and the arm that makes the deadline assertable without a clock: a spent
  deadline and a budget that runs out mid-wait leave `send_waiting` by the *same* line. Both
  refusals are compared against each other, so a build that changed what the flag says rather
  than when it says it goes red.
- `a_caller_that_waits_takes_the_place_the_running_command_frees` — the waiting half. The
  determinism is a signal, not a duration: `Seats::waiting` is raised for the whole of a
  wait and `CameraActor::awaited_by` (test-only) blocks until the subject says it is parked,
  so the held thread is released *after* the waiter has provably met a full queue. The
  sixty-second deadline is a bound nothing reaches. Watched red against a hand-applied mutant
  that rewires the waiting arm to `send`: the run becomes a named nextest `TIMEOUT` rather
  than a hang, which is the shape `.config/nextest.toml` exists to give.
- `a_caller_waiting_on_a_thread_that_dies_is_told_the_device_is_gone_and_not_that_it_is_busy`
  — E3, arranged by having the *held* command panic on release, so the thread dies without
  serving what is queued behind it and the queue stays full across the death.
- `the_shipped_wait_budget_is_the_one_constant_and_nothing_repeats_it`, and the daemon's
  `d12s_wait_flag_chooses_between_the_two_enqueues_and_nothing_else` — the constant's single
  reader, and the field's two directions, each asserted where it lives.
- `d12s_wait_flag_crosses_the_wire_both_ways_and_neither_spelling_changes_the_photo` — the
  wire half. **Rewritten after the P4e-i review (note N59)**: as first written it sent both
  spellings behind one held command, where eight seats were free, so both were simply
  enqueued and `wch_photo` ignoring the field entirely passed the whole workspace. It now
  fills the queue first — reading exactly the refusals a flood of twice the seats must
  produce, which is an observation and not a guess — and then compares two *outcomes* at one
  instant: `wait: false` refused `Busy`, `wait: true` served once the held command lets go.
- `a_flood_of_waiting_captures_is_bounded_and_never_the_daemon` — the bound on how *many*
  callers may wait, which this entry originally claimed the connection count supplied. See
  note **N59**.

### What this deliberately does not do, and why

**The bound is not driven to its refusal over the socket.** ~~Filling a nine-deep queue
through a transport means knowing when eight requests have been *enqueued*, which nothing
outside the daemon can observe — so the assertion would be a fact about the scheduler wearing
a fact about the queue.~~ **Retracted 2026-08-10 (note N59), and the retraction is the reason
the wire test could not tell the two spellings apart.** "Nothing outside the daemon can
observe it" was true only because nothing published it. Two things are observable and both
are ordinary refusals or ordinary counts: a `Busy` answer *is* the daemon saying the queue is
full, so reading `CAMERA_COMMAND_QUEUE_DEPTH` of them from a flood of twice that many is an
observation rather than a guess; and `Wchd::watch_waiting_captures` — which
`limits::CAMERA_ENQUEUE_WAITERS` needed a reader for anyway — is the daemon saying a caller
is parked. Neither costs the shared fixture an item, because both hang off the `Wchd` handle
`Fixture` already holds. The wire tests drive both today.

**`calibrate_sweep`'s per-sample photos are `wait: false` and blocking-open**, deliberately:
they run *inside* the actor's thread, so there is no queue in front of them, and their
destination is the session tree this process made rather than a path a client named.

**Retires when:** a caller appears that needs a wait bound other than the one constant — at
which point the deadline is already an argument and only the daemon's spelling moves — or the
actor grows a cancellable command, which would let N51's remaining residual (a regular file on
a hung mount) be ended rather than merely bounded.

---

## N57 — One declaration, two generated traits: D10's sentence bends where jsonrpsee's client is, and the transports say why

**Doc:** D10 (docs/6:491-509) says "the whole daemon API is one `#[rpc(server, client)]`
trait" and lists `subscribe_events` and `subscribe_calibration` among its methods. Note
**N28** is the property that keeps the trait and its inventory one declaration; note **N29**
did the arithmetic for what a subscription would cost the counts; note **N38** named the two
constants P4b deferred with the WebSocket surface it turned off. This entry is P4e-i's, and
it records the decisions those three left open.

**P4e is two sub-milestones**, and the register for that is note **N58**, not this entry:
everything below is P4e-i's, and the shutdown clauses named there are P4e-ii's.

### The split that costs D10 a sentence

**Believed:** that "one trait" and "one wire surface" are the same claim, so subscriptions
would simply join `WchRpc`.

**True:** one `#[subscription]` anywhere in a trait re-bounds the whole **generated client**,
and the bound it lands on is one no type of ours can satisfy.
`jsonrpsee-proc-macros-0.26.0/src/render_client.rs` picks the client supertrait once per
trait — `SubscriptionClientT` if the trait carries any subscription, `ClientT` otherwise —
and `SubscriptionClientT::subscribe` answers `jsonrpsee_core::client::Subscription`, whose
only constructor is **private, over two private types**
(`jsonrpsee-core-0.26.0/src/client/mod.rs`). So no transport outside `jsonrpsee-core` can
implement it. Measured, as an `E0599` on a scratch tree:

```
error[E0599]: the method `list` exists for struct `Wire`, but its trait bounds were not satisfied
    = note: the following trait bounds were not satisfied:
            `Wire: SubscriptionClientT`
```

`crates/daemon/tests/support/wire.rs`'s `Wire` is `ClientT`-only *because it is two
transports* — an in-memory `Methods` and a `POST` on a `UnixStream` — and four integration
suites drive the T5 client twice, once per pipe, which is the comparison they exist to make.
Folding the subscriptions in would have cost all four one of their two pipes, or adopted
`jsonrpsee/async-client` (and `futures-timer`, which is not in `Cargo.lock` and does not
resolve offline) to serve a client `wchc` will not use — P4f's transport is a separate piece
by design (design §2.6).

**Decided: two `#[rpc]` traits out of one `wire_surface!` invocation** — `WchRpc` with
`METHODS`, `WchEvents` with `SUBSCRIPTIONS` — merged into one `Methods` by
`daemon::server::mount`, which the shipped binary and every integration fixture both go
through.

It is not only a compilation dodge, and that matters for whether the sentence should have
been written differently in the first place: **calls and subscriptions really are two
capabilities over this socket.** jsonrpsee's HTTP path builds `RpcServiceCfg::OnlyCalls`, so
a `wch_subscribe_*` sent as a `POST /` is answered `-32603` — not a D13 code, not
`MethodNotFound`. A subscription needs the *upgrade*. One client trait would have hidden
that from the one consumer that has to build against it.

What D10 is actually protecting is **one source**, and it survives intact: both traits come
out of one declaration, so a subscription still cannot reach a trait and miss an inventory
(N28), and `Methods::merge` is the one place a name that collided *across* the two halves is
caught — the proc macro's own `check_name` looks inside one trait only. That merge is
asserted, in `crates/api` and again at `mount`.

### Are subscriptions methods? Two consumers, two answers

**The count walks say yes.** Their population is a real `RpcModule`'s `method_names()`,
which is `callbacks.keys()`, and jsonrpsee registers the `unsubscribe` callback under its own
key (`rpc_module.rs::verify_and_register_unsubscribe`). N29's arithmetic holds exactly: the
registered population goes nineteen to **twenty-three** while D10's own method count goes
nineteen to twenty-one, and those are two numbers about two things. Every consumer
*partitions* by `wire::Subscription::names()` rather than excluding a spelling by hand —
`crates/api`'s registration test, `daemon::server`'s `routed_subscriptions`, and
`crates/daemon/tests/method_surface.rs`, whose exercised set is now *calls* while
`tests/subscriptions.rs` walks the *subscribes*. A third subscription joins both walks by
existing, which is the property that made the partition worth more than a filter.

**Amended 2026-08-10 (note N59): that was true of the two count walks and false of a third
rule this entry stated in the same breath.** `xtask::bundle`'s "a subscription's item type is
a root of the JSON Schema bundle" was written as two hand-registered lines beside a
`SUBSCRIPTIONS` walk the same file already performs 130 lines further down — so a third
subscription reached `x-subscriptions` and reached neither `x-roots` nor `$defs`, with every
xtask test green and `schema-artifacts-current.sh` green after regeneration, because that
predicate only diffs emitted against committed. The walk is derived now and
`every_subscriptions_payload_is_a_root_of_the_bundle_and_not_only_of_the_document` states the
law from the other end. The lesson is the general one this entry was already about: a rule
that *has* a walkable population and does not use it is a rule with one obedient instance,
not a mechanism.

**The OpenRPC document says no**, because saying yes would publish something false. OpenRPC
1.3.2 has no notion of a server-initiated stream. A subscribe call emitted as a `method`
would be true about the call and silent about the payload, which is the only interesting part
of it; its `unsubscribe` sibling emitted as a `method` would be **false**, because that
callback is `params.one::<RpcSubscriptionId>()` — positional — and every method in this
document declares `"paramStructure": "by-name"`. So `methods` stays exactly the call surface
and the subscriptions are a top-level `x-subscriptions` array carrying both wire names, the
notification name and the **item schema**, resolving into the same `components/schemas` every
other `$ref` does. Complete about the payload, honest about being an extension. `xtask`
asserts both directions: every row described, and no subscription spelling among the methods.
`schemas/webcam-handler-schema.json` gains `HotplugEvent` as a root beside `ProgressEvent`,
for the reason that comment already gave.

**N5's wall is intact and cost nothing to keep.** A `#[subscription]` with even one parameter
makes the generated server call `tokio::spawn` on its params-decoding error path
(`render_server.rs`'s `error_ret`), which would put a task spawn in the crate whose header
says "Nothing here runs; it declares". Both subscriptions are parameterless, so no such call
is generated — and `crates/api` needed no tokio dev-dependency either, because
`AnswersNothing`'s two bodies return without awaiting and the registration tests read
`method_names()` rather than driving a subscription.

### `subscribe_calibration` is per *client*, and D10's parenthetical is a filter

D10 says "per-session progress"; `crates/schema/src/progress.rs` says the opposite in as many
words, and it is the side with the committed shape behind it: the session id rides on **every
event** "because P4e's subscription is per *client* and a client may watch a daemon running
more than one session". **Decided: per client, filtered by the consumer on
`ProgressEvent::session`.** A `SessionRef` parameter would resolve against a store lock the
subscribe path has no business taking, and a `SessionRef::Task` subscription would silently
follow whichever session occupied the slot next — besides costing N5's sentence above.
`subscribe_events` carries `HotplugEvent` **verbatim**, nodes and not cameras, for the reason
its own doc and note **N53** give: grouping is not a node property, and re-enumeration is
live every time (E2).

### What a subscriber that falls behind is told, and why the two streams differ

Not one policy, and the difference is in the payload rather than in the transport:

- **`subscribe_events` ends the stream, naming the count.** A `HotplugEvent` is a *delta*,
  the vocabulary is closed, and there is no variant meaning "you missed some". A gap leaves a
  consumer's picture of the node tree wrong in a way it cannot detect, so ending is the only
  answer that is not a quiet lie. The count reaches the client as a **typed** payload
  (`{"lagged": n}`), because jsonrpsee's blanket `impl<T: ToString> From<T> for
  SubscriptionError` would have flattened it into prose a client has to parse.
- **`subscribe_calibration` counts it and carries on.** Every in-flight
  `CalibrationProgress` variant carries `index`/`total` — put there so "a subscriber that
  connects mid-sweep has no earlier events to count" — so a gap is self-healing and the next
  event repaints a correct bar. Ending a client's view of a twenty-minute sweep because it
  was briefly slow would be the transport inventing a failure the payload already handles.

The decision is a fold, `daemon::events::lag_verdict`, with both arms and the payload unit-
tested — because forcing a real fan-out lag means keeping a subscription's task from running
while `SUBSCRIPTION_BROADCAST_DEPTH` events go past it, which is a fact about the scheduler
wearing a fact about the queue. At the *other* two hops there is one answer and it is note
**N17**'s and `engine::progress::ChannelSink`'s: **drop, and count.** Never block — the
producers are a camera actor's own thread and the hotplug watch's, and either one waiting on
a subscriber is the wedge this sub-milestone exists to make unrepresentable.

**Events emitted with nobody subscribed are dropped and counted**, which is P4e-i's decision
rather than an accident. `broadcast::Sender::send` answers `Err` exactly when
`receiver_count() == 0`, and `Fanout::unheard` turns that into a number. Nothing is buffered
for a client that has not arrived: a parked long-lived `Receiver` would hold a whole sweep's
events for nobody, which is the unbounded growth `limits::PROGRESS_QUEUE_DEPTH` rejects one
crate down. The sweep is on disk either way (D9), and `schema::progress` already documented
the posture — an event "is allowed to be dropped when nobody is listening".

### The hotplug watch runs while somebody is listening, and not before

`CameraBackend::watch` can fail — a container without `NETLINK_KOBJECT_UEVENT`, an LSM, a
backend with no watch to give. **Decided: start it at the first subscription, on its own OS
thread, and end it when the last subscriber goes.** Eagerly at startup, the same failure
would refuse to *start a daemon* on a host where enumeration works perfectly, which is the
availability-versus-capability conversion E3 forbids; started lazily it is a D13 refusal of
the subscription that asked for it, which is what a refusal means everywhere else on this
surface. Ending it with its last reader is also why P4e-ii's teardown does not have to reach
it: a thread parked in `poll(2)` for nobody has to be told to stop, and one that stops when
its last reader leaves never does.

It is an OS thread rather than a task because `HotplugWatch` is `Send` and **not** `Sync` and
`next_event` takes `&mut self` — a single-consumer, exclusively-owned, blocking object — and
this daemon runs nothing that can block on a runtime worker. That thread reads
`Instant::now()`, which is not an exception to "the caller stamps each deadline": **it is the
caller.**

### The fake's watch was wrong, and P4e-i is what made it visible

`FakeWatch::next_event` returned `Ok(None)` immediately whatever deadline it was given, with
the argument that "a fake that slept would be scheduling a flake". The argument was about
`sleep` and the conclusion was one step too far. `HotplugWatch`'s contract is *block until an
event or until the deadline*; a watch that answers instantly and forever has no honest
consumer except one polling on a cadence of its own, and P4e-i's daemon is not that caller —
it loops, which against the old behaviour was a **spin at 100% of a core in every daemon
integration test**. AGENTS reads both ways: a fake capability no real device exhibits is a
bug in the fake, and so is a real behaviour the fake refuses to exhibit.

**Corrected with a `Condvar` and no sleep.** The wait ends when a test scripts a fault
(`FakeBackend::queue_fault` notifies), so an event a test asks for arrives when it asks; what
is left is the caller's own deadline, which is a bound the trait declares rather than
synchronisation. `testkit::battery`'s "the deadline is honored" arm was vacuous until now.

### The WebSocket surface, and the residual left switched off

P4b's `ServerConfig::http_only()` is lifted and both of note N38's numbers are this
project's: `limits::WS_MESSAGE_BUFFER_CAPACITY` (64) bounds what one subscription may hold
unwritten, and `limits::RPC_MAX_SUBSCRIPTIONS_PER_CONNECTION` (8) bounds how many streams one
connection may open — refused as a `-32006` answer to the *subscribe call*, before any
handler runs, so connect-and-abandon costs a client its own slots and nobody else's.
`limits::SUBSCRIPTION_BROADCAST_DEPTH` (256) is the fan-out's, deliberately deeper than one
connection's private buffer so that the hop which refuses is the private one and a slow
subscriber never costs another its events; a `const` assertion checks that relation where
both numbers are, beside the one that keeps `set_message_buffer_capacity` from panicking on
zero. `tests/uds.rs`'s upgrade assertion is **inverted rather than deleted**, which is the
whole record of what changed about the transport.

**The residual, named rather than fixed:** `ping_config` stays `None`, so a peer that opens a
WebSocket, subscribes, and never reads again is not reaped by an inactivity timer. It is
bounded — `DAEMON_MAX_CONNECTIONS` × `WS_MESSAGE_BUFFER_CAPACITY`, with a fan-out in front
that never waits on any of it — and turning `enable_ws_ping` on would add two constants whose
behavioural half cannot be asserted without waiting out a timer, which is the shape AGENTS
bans. Left off on purpose.

### What the tests can and cannot force

The suite's determinism is **signals, never durations**: the gate announcing that a sweep is
inside a write, `Wchd::watch_subscribers()` announcing that a subscription was reaped, and
`Wchd::watch_losses()` announcing that a bound has refused an event. That last one is why the
slow-subscriber arithmetic is exact rather than a measurement of the scheduler: the reading
subscriber taking the last event says the *producer* finished, and the loss count reaching
the overflow says the *stalled subscriber's own task* finished — reading it before that would
free a slot per read and let its task send what it would otherwise have dropped. It is note
N17's pre-authorised "query on the sink" and it is an operator-visible number, not a test
hook. `Attached`'s live-count decrement is a field declared **after** the receiver, so drop
order publishes "nobody is subscribed" only once the receiver is gone; that ordering is
structural and no test can observe it from outside, which the test that depends on it says.

Two reds were watched rather than argued: a third row in `wire_surface!` makes the
subscription walk fail at its fallback naming the row, and swapping `daemon::events`'
`try_send` for the `send().await` beside it turns the backpressure test into a named nextest
`TIMEOUT` at 180s.

### Amended 2026-08-10 — the hostile directions, and the two of them this wire cannot express

*Nothing a client does can wedge the daemon* is worth exactly what the list of things a
client can do is worth, so P4e-i walks that list, one test apiece in
`crates/daemon/tests/subscriptions.rs`: a client that subscribes and vanishes; one that names
a session at subscribe time; a sweep of a session that does not exist; a client that
subscribes twice to one sweep; a subscription that outlives its session; a session that ends
under a watcher; more subscriptions than the per-connection bound; a connection that dies in
the middle of a message.

**Every one of them ends by asking the daemon a verb** — over the connection that did the
damage wherever the damage left one, over a connection opened afterwards always, and over the
plain `POST /` the upgrade shares a listener with. That is not belt and braces. A daemon that leaked a permit, a task or a lock
keeps serving the client that already holds a connection while refusing the next one, and one
whose accept loop has gone does the opposite; the two have nothing in common except that a
wedged daemon fails to *return* rather than failing an assertion. The question is a list
**and** a refusal, for `method_surface.rs`'s `discriminating_refusals` reason — a daemon that
had degraded into one blanket error would satisfy "it answered" — and the expected code comes
from `api::rpc_code` rather than from a literal.

**Two of the eight have no wire form, and what was asserted instead is the finding.** Neither
subscription takes a parameter (above), so "subscribe to a session that does not exist" and
"subscribe twice to *the same session*" cannot be sent:

- A client that sends a `session` key anyway is **accepted and the key is ignored** —
  jsonrpsee generates no params decoding at all for a parameterless subscription, so there is
  nothing there to refuse with. Pinned rather than assumed, because a client that believed it
  had a server-side filter would drop every other session's events and never know it. The
  session-shaped refusal is asserted where a session is actually named — the *sweep*, which
  answers `IllegalTransition` — together with the claim that the refusal is the sweep's
  alone: the stream is neither closed nor fed a phantom event, which the next real sweep's
  first event is what proves, by arriving first.
- "Twice to the same session" is two subscriptions on one connection watching one sweep,
  which is the interesting half anyway: two ids, both fed the same events, one connection's
  bookkeeping counting both and giving both back.

**A terminal event is not a close.** `CalibrationProgress::is_terminal` is per control and
per sweep, and `schema::progress`'s own test warns a *consumer* against closing on it; the
transport must not either, and nothing in `daemon::events::forward` can, because it is
generic over the payload. Asserted from both sides anyway: one test runs a whole D8 arc —
sweep, select, apply, restore — under a watcher and asserts that only the session ended,
and another puts a second session down the same subscription afterwards, one that **fails**,
and asserts the `SweepInterrupted` discriminant it carries is the one that session's caller
was refused with.

Three more reds were watched for these, plus one re-demonstration. `daemon::events::forward`
returning after its first delivery: the two session-lifetime tests become named `TIMEOUT`s
and two siblings fail outright. `Counted::drop` not decrementing: every reaping wait becomes
a named `TIMEOUT` — which is the answer to why a reap is *waited for* on a `watch` and never
read off a counter, since a read would have passed against that mutant on whichever schedule
happened to look right. `still_answers` expecting any other D13 code: all seven hostile
directions and the disconnect test fail in milliseconds, which is what says the second
question is answered by the daemon rather than by the helper. And `method_surface.rs`'s walk
was re-demonstrated after the two-trait change — deleting one call from `every_method` fails
the equality naming `wch_calibrate_restore`, with the four subscription spellings correctly
on neither side of it.

**Retires when:** jsonrpsee makes `Subscription` constructible outside `jsonrpsee-core`, at
which point one trait would carry both halves and the split above becomes a cost with nothing
buying it; or OpenRPC gains a shape for a server-initiated stream, at which point
`x-subscriptions` becomes a `$ref` to a standard one.

---

## N58 — P4e split into P4e-i and P4e-ii, and the seam is that shutdown's proof needs subscriptions' fixture

**Doc:** docs/7's "Milestones are session-sized" convention — "a sub-milestone that turns out
to be two splits — **recorded in the notes** — rather than stretching past what one session
can carry" — names the notes as the register, so this entry is the split rather than a report
of it. Note **N54** is the precedent and the rule: *size by story, not by subsystem*, written
after P4d had been mis-sized in exactly this way. Nothing here changes a gate letter or a
criterion; this is about how the work is cut.

**P4e was written as one sub-milestone**, "Subscriptions and shutdown", and its "Lands"
clause failed N54's own test the moment it was read against it: it needs the word *and*
between two things a reviewer holds in mind separately. The two are

- **P4e-i — subscriptions and backpressure.** *A client can watch, and nothing a client does
  can wedge the daemon.* `subscribe_events` and `subscribe_calibration`; the WebSocket half
  of the Unix socket that P4b deliberately turned off, back on with note N38's two bounds and
  the tests that reach them; the fan-out and its lag policies; disconnect-mid-sweep; and the
  three debts note **N56** discharges, which are one mechanism.
- **P4e-ii — shutdown and systemd.** *The daemon stops the way the init system expects.*
  SIGTERM ≡ SIGINT through `CancellationToken` teardown, the drain, ordered store-lock
  release, `sd_notify` READY/STATUS/STOPPING, `listenfd` socket activation, the journald
  layer under systemd, and never self-daemonizing. The tree already names those deferrals
  where they sit — `uds.rs`'s "not a drain and not a signal handler", `state.rs`'s ordering
  sentence, `server.rs`'s ordered end for the idle-sweep driver, and `tests/uds.rs`'s socket
  file that survives a stop *deliberately*.

### Why this is a seam and not a cut through the middle of one

The two halves are **sequential, not parallel**, and the reason is in docs/9's own
commissioned row for P4e: "one test per signal (SIGTERM, SIGINT), real delivery, drain
asserted **with open subscription + mid-flight sweep**". That row is **P4e-ii's**, and it is
the only gate row docs/9 commissions for P4e — which means shutdown's proof is stated in
terms of the thing P4e-i builds. Ordered the other way, P4e-ii would have had to build a
subscription fixture in order to assert a drain, and P4e-i would then have rebuilt it; a
seam that makes one half's proof cheaper and neither half's proof weaker is a seam rather
than an incision. `crates/daemon/tests/subscriptions.rs`'s disconnect test is left inheritable
on purpose: it already holds a sweep inside a device write, already holds an open
subscription across it, and already ends by asserting `ServerHandle::stopped()` resolves —
which is the assertion a leaked bridging task turns into a named `TIMEOUT`.

The seam is also a story seam in N54's sense rather than a file seam. The two halves *do*
share files — `daemon::server`, `daemon::uds`, `Inner` — so a cut made by what the code
touches would have refused to make it. What they do not share is a claim: "nothing a client
does can wedge the daemon" is about a running daemon and a hostile peer, and "the daemon
stops the way the init system expects" is about a daemon that is ending and a well-behaved
init. A reviewer holding both at once is holding two.

### What the split cost, and the prediction it is the first chance to measure

P4e-i's own criteria are new `g4` rows in `scripts/gates/phase-criteria.tsv`, added with the
things they prove; P4e-ii's is docs/9's row above, and it is deliberately **not** written
yet — a criterion is a row added in the same commit as the thing it proves, and nothing here
proves a signal.

N54 ended with a falsifiable prediction and asked for it to be measured "the next time a
split happens". This is that split, and it is the *good* case for the prediction rather than
the fair one: P4e was split **before** either half was written, so there is no un-split P4e
to compare against, and the numbers this produces are two reviews of two halves with nothing
to hold them beside. Recorded so the next reader knows the comparison is unavailable rather
than unfavourable. What can be said now: P4e-i alone owes `just ci` and the mutation floor
(it edits `crates/api/src/wire.rs`, which is inside the floor's `examine_globs`) — two
terminal rungs, which is N54's second rule met rather than broken.

### The second half landed, 2026-08-11, in three commits rather than one

The register is complete, so it is written here rather than left to the log. **P4e-ii is
`ffa1ff7`** (the shutdown discipline and the `Notifying` seam with nobody on the other end of
it), **`bb63e8a`** (the systemd half: `sd_notify` past that seam, the watchdog, `listenfd`
socket activation with D11 asked of a socket this daemon did not bind, the journald layer, and
two shipped unit files with `systemd-units.sh` and `socket-activation.sh` keeping them
agreeing with the binary), and **`add421c`** (docs/9's commissioned signal-parity suite).

Two things about that shape are worth the register's space.

**The third commit was not planned, and it is where the sub-milestone earned its keep.** The
plan had two halves and a suite riding along with the second; what happened is that the suite
became its own commit because it found a defect in the first half — the cancel-then-stop race,
note **N61** — and a commit that lands a test and the fix it forced is a different reviewable
thing from one that lands a feature. That is N54's sizing rule applying one level down: the
unit is the *story*, and "the ending a client is promised actually reaches it" is a story.

**Every deferral this entry listed is discharged, and one is deliberately not.** `uds.rs`'s
"not a drain and not a signal handler", `state.rs`'s ordering sentence and `server.rs`'s
ordered end for the idle-sweep driver are all discharged in the module that named them, with a
`g4` row selecting the three that live outside `daemon::shutdown` itself. The fourth —
`tests/uds.rs`'s socket file that survives a stop *deliberately* — was never a debt and stays:
P4e-ii sharpened its argument rather than reversing it, and `tests/signals.rs` now asserts the
file is there after a real signal. Seven `g4` rows landed with the sub-milestone, taking the
gate from 23 rows to 30.

**And the prediction this entry could not measure stays unmeasured, honestly.** N54 asked for
the cost of a split to be compared against an un-split equivalent; there is still no un-split
P4e to compare against. What can be added is one observation, weaker than a measurement: the
second half's own review surface was *not* smaller than the first's, because the systemd
protocols are four wire formats somebody else defined and each needed its own arm. A reader
looking for the split's payoff should look at the seam instead — P4e-ii's proof reused P4e-i's
fixture exactly as this entry predicted it would, and `tests/support/wchd.rs` (note N49) is the
third suite's worth of reuse that followed.

**Retires when:** never by disproof — it is a record of a decision. It is superseded if a
later plan revision re-merges the two halves, which would need the docs/9 row above to stop
depending on a subscription.

---

## N59 — P4e-i's adversarial review: the bound that was arithmetic, the stream that was told nothing, and four rules with one obedient instance each

**Doc:** AGENTS rule 1 ("every anticipated or discovered defect class becomes a lint, a CI
job, or a test that can go red"), rule 2 ("both directions"), rubric A8 ("for every constant,
ask what *reads* it and what goes red when it stops being read"), rubric A4's amendment (a
transient failure must not leave a client in a state no verb can leave), rubric B11 (a stated
number wants something that can go red when the arithmetic moves under it), and design §2.10
("one home per law"). Four hostile reviews went over the uncommitted P4e-i change; this
entry records what they found that was real, what the tree does about it, and the two
findings that were *not* real in the shape they were reported.

### 1. The bound that was arithmetic about a transport this sub-milestone removed

`daemon::server::on_resolved_camera_queueing`'s doc and note **N56** both stated that "the
number of parked pool threads is bounded by `limits::DAEMON_MAX_CONNECTIONS` (32) against
tokio's own pool". That was never a bound this build enforced. It was an inference from
HTTP/1.1 answering one request per connection — and P4e-i lifts `ServerConfig::http_only()`
in the same sub-milestone. jsonrpsee's WebSocket transport `tokio::spawn`s one task per
inbound message and never awaits it (`jsonrpsee-server-0.26.0/src/transport/ws.rs`), and
`ServerConfig` has no per-connection concurrent-call cap at all: `max_connections` bounds
connections, `max_subscriptions_per_connection` bounds subscriptions, nothing bounds calls.
So **one** connection could hold arbitrarily many `wch_photo {"wait": true}` in flight, each
parking a blocking-pool thread inside `CameraActor::send_waiting` for up to
`CAMERA_ENQUEUE_WAIT_MS`; the real ceiling was tokio's 512-thread pool with an unbounded
queue behind it, a number this project neither chose nor names. Every other verb draws on
that same pool through `Wchd::offload` — `wch_list`, `resolve`, `addressable`,
`open_destination`, even `subscribe_events` — so a flood of waiting captures put every other
client behind it. That is the exact failure the sub-milestone's story is named against, and
it was untestable by construction: the wire test said in as many words that it "deliberately
does not fill the queue over the socket", and `engine::actor`'s suite drives one waiter at a
time.

**What landed.** `limits::CAMERA_ENQUEUE_WAITERS`, enforced by a permit pool
(`daemon::server::Waiters`) around the waiting arm. Sized to `DAEMON_MAX_CONNECTIONS`, with a
`const` assertion that the two agree, precisely so the sentence above becomes true by
construction rather than by an accident of which transport a client chose. A caller past it
is **not** made to wait for permission to wait — that is a second unbounded queue — it takes
the enqueue a caller that never asked to wait takes: served if there is room right now,
`Error::Busy` if there is not. So the flag degrades to its own `false` under load and D13
grows no nineteenth variant meaning "too many waiters" (the registry is closed).

An async wake-up in `engine::actor::Room` was the other candidate and is not available: it
would mean a `tokio::sync::Notify` in `crates/engine`, which `dependency-walls.sh` names in
`$pure` and P4b's own argument (note N41) rests on. The permit pool is also the right *home*
on the merits — what is being bounded is the daemon's blocking pool, which is a fact about
the process hosting the actor rather than about the actor. It is not the "second count of the
same eight seats" N56 rejected: these permits count parked *callers*, and the queue's own
bound is still the `sync_channel`'s.

**How it goes red.** `a_flood_of_waiting_captures_is_bounded_and_never_the_daemon` holds a
camera's actor thread with a real sweep (the `Gate` decorator `calibrate_verbs.rs` already
uses), fills the command queue from **one** WebSocket connection — `CAMERA_COMMAND_QUEUE_DEPTH * 2`
requests meeting that many free seats, so exactly half are refused and reading that many
`Busy` answers is an *observation* that the queue is full — parks `CAMERA_ENQUEUE_WAITERS`
waiters and waits for the daemon to publish that they arrived, then sends four more and reads
four immediate refusals. Watched red against a hand-applied `Semaphore::new(4096)`: the four
park instead, the run takes a whole `CAMERA_ENQUEUE_WAIT_MS`, and it fails at `36` parked
where this daemon bounds `32` (measured, 10.01 s).

### 2. Three rules with one obedient instance each, and the fourth that had none

Rubric A8 read as a question — *what reads this, and what goes red when it stops being read?*
— caught three numbers and one property:

- **`WS_MESSAGE_BUFFER_CAPACITY`.** Its only production reader was `uds::serve`, and the
  suite that claimed to drive it drove `Methods::subscribe`, whose buffer is an *argument*.
  Measured: `.set_message_buffer_capacity(1024)` — jsonrpsee's own default, i.e. the exact
  regression note N38 turned the surface off to prevent — passed all 109 daemon tests. Fixed
  by publishing what a real connection actually gave the subscription
  (`SubscriptionSink::max_capacity`, recorded as `events::StreamActivity::buffer` at accept),
  because `ServerConfig` cannot be read back and the sink is the one place the configured
  number becomes visible. `a_real_connections_message_buffer_is_the_number_this_project_chose`
  asserts it over a real WebSocket and asserts the in-memory arm reports the *different*
  number it was handed, so the field is an observation rather than a second copy of `limits`.
- **`HOTPLUG_WATCH_DEADLINE_MS`.** Its property — `Hotplug`'s "ended when the last subscriber
  goes" — was unobservable: `running` was private and `StreamActivity::subscribers` counts
  receivers rather than threads. A 3600× change left the whole workspace green (measured).
  Fixed by making the flag a `watch::Sender<bool>` *inside* the mutex that already guarded it
  — one fact in one place, decided under exclusion and awaitable — surfaced as
  `Wchd::watch_hotplug`. `a_hotplug_watch_runs_only_while_somebody_is_listening` deliberately
  scripts no fault after the last unsubscribe, so the thread's turn comes from the deadline
  and from nothing else; with the constant at an hour the test is a named nextest `TIMEOUT`
  at 180 s (measured).
- **`PhotoRequest::wait`.** The wire test named for D12's flag could not tell the two
  spellings apart: it sent both behind one held command, where the queue is eight deep, so
  both were simply enqueued. Measured: `let how = enqueueing(false);` — D12's flag deleted —
  passed all 861 tests in the workspace. N56 had argued the gap was unavoidable ("nothing
  outside the daemon can observe" an enqueue); that is retracted there. It was unobservable
  only because nothing published it, and the two signals needed are ordinary: a `Busy` answer
  *is* the daemon saying the queue is full, and `Wchd::watch_waiting_captures` — which
  `CAMERA_ENQUEUE_WAITERS` needed a reader for anyway — is the daemon saying a caller is
  parked. The rewritten test compares two *outcomes* at one instant (`wait: false` refused,
  `wait: true` served after release) rather than two timings, and the deleted-flag mutant is
  now a named `TIMEOUT` at workspace scope.
- **`xtask::bundle`'s subscription roots.** Stated as a law in a comment and implemented as
  two hand-written lines beside a `api::SUBSCRIPTIONS` walk the same file already performs.
  Measured: a third subscription reached the OpenRPC document's `x-subscriptions` and reached
  neither `x-roots` nor `$defs`, with all eight xtask tests green. Derived now, with
  `every_subscriptions_payload_is_a_root_of_the_bundle_and_not_only_of_the_document` stating
  the law from the other end — verified red against the hand list plus a third subscription.
  N57 is amended: its "a third subscription joins both walks by existing" was true of the two
  count walks and false of this third rule.

### 3. The hotplug watch's error arm did the opposite of what it said

`Hotplug::watching`'s `Err` arm logged "the hotplug watch stopped; **subscribers were told**"
and cleared its flag. Nothing told them. `forward`'s only end-of-stream arm is
`broadcast::error::RecvError::Closed`, which tokio produces when the `Sender` is dropped —
and the `Sender` lives in `Fanout::events` inside `Events` inside `Inner`, for the daemon's
whole life. Every open `wch_subscribe_events` stream therefore stayed accepted, stayed
counted in `SubscriptionActivity::live`, and silently delivered nothing for the rest of the
process, with no error, no close and no count: rubric A4's shape one layer up, behind a log
line asserting the opposite, and precisely the "quiet lie" this module refuses twenty lines
earlier when it ends a *lagging* hotplug stream. A second facet, same arm: it cleared
`running` without consulting `receiver_count()`, so a subscriber taking the lock in the
window returned attached to a stream with no thread behind it — falsifying `Hotplug::attach`'s
own stated argument.

**What landed.** `Fanout` carries `Feed<T>` — an event, or `Feed::Ended(&'static str)`. The
terminal travels *in* the channel rather than beside it, because a subscriber that is behind
must get what it already has before it is told the source stopped, and a signal raced against
the queue would end a stream with deliverable events still in it; `broadcast` gives a sender
no close it can perform without dropping, so the value carries what the channel cannot. Both
exits of the watch thread are one function (`Hotplug::give_up`) that takes the lock once, and
`Hotplug::attach` now takes that lock **first** and its receiver **last**, which closes the
third interleaving as well: a subscriber arriving while a failing thread ends its readers
attaches after the terminal was sent, so it is not handed somebody else's ending.

**And the fault menu grew the two variants that make both directions reachable.**
`fake::Fault::WatchUnavailable` (a host with no watch to give) and `Fault::WatchFails` (a
watch that stops) — the exhaustive-match menu whose whole point is that "a fault the compiler
cannot force the fake to script is a fault nobody tests", which had eight variants and
neither of these. Two tests:
`a_backend_that_cannot_watch_refuses_the_subscribe_call_rather_than_accepting_it` (the D13
refusal before the accept, which is what makes the lazy-start decision worth making) and
`a_watch_that_stops_ends_the_streams_reading_it_and_names_why` (delivery, then the ending,
then the reaping, then the retry). Watched red against the arm that only cleared the flag:
a named `TIMEOUT` at 180 s — which is what a stranded subscriber looks like from outside.

### 4. Two findings that were reported and are not defects

- **"A permit pool is a second count of the same seats."** N56 rejected a permit pool and was
  right about the pool it rejected — one that replaced the `sync_channel`'s bound. The one
  that landed counts something else (parked callers, in the process that hosts them), so the
  two are not the same proposal and the entry is amended rather than contradicted.
- **"`WS_MESSAGE_BUFFER_CAPACITY` should be driven past its bound over a real socket."** It
  should not, and the suite says why where it declines to: an `AF_UNIX` socket puts the
  kernel's own send buffer between the daemon and a reader that has stopped, so "the
  connection is full" over that transport is a fact about `SO_SNDBUF` rather than about this
  constant. The exactness lives on the in-memory dispatch, where the connection buffer is the
  only queue in the path; what the real socket now pins is the *configured value*, which is
  the half that was actually missing.

### The smaller corrections, recorded so they are not re-found

`WS_MESSAGE_BUFFER_CAPACITY`'s sizing argument said "a whole quarter of the longest sweep"
from premises (two events per sample × 256 samples) that give an eighth; restated in samples,
which is the unit with one meaning. `crates/api`'s pinned-spelling test was renamed at P4e-i
and three citations of the old name were left behind — the two live ones
(`daemon/tests/method_surface.rs`, docs/9's method-count row, which AGENTS designates the
authoritative statement of that mechanism) now name the current test, and N42's historical
citation gained a parenthetical rather than a rewrite. `Fixture::start`'s baseline comment
promised three assertions and wrote two; the third (`FakeBackend::opens() == 0`, which is
D12's own invariant and the one a fixture change could quietly break) is written now. And
`soketto` joined design §2.8's inventory: the adoption itself needed no escalation under the
2026-08-09 ruling — `Apache-2.0 OR MIT`, pinned, already in the lock through
`jsonrpsee-server`, `cargo deny` and `dependency-walls.sh` green — but §2.8 *is* the registry
AGENTS points a licence audit at, and a crate the daemon's test binaries link that the
registry does not know about is the registry being wrong.

**Retires when:** nothing. It records what a review found and what the tree does about it.
The one thing in it that could be disproved is the sizing of `CAMERA_ENQUEUE_WAITERS`: if a
real deployment shows thirty-two parked captures is either too few to be useful or too many
for the pool, the number moves and the `const` assertion tying it to
`DAEMON_MAX_CONNECTIONS` moves with it — at which point the sentence it exists to make true
has to be rewritten rather than deleted.

## N60 — The floor said an acceptance had become a lie, and the acceptance was telling the truth

**Believed:** that the acceptance register's second direction — "a listed mutant that has
stopped surviving fails the job too" — reports one thing: that a mutant somebody argued was
unkillable has become killable, so the argument needs revisiting. E7 commissioned it as
N15's lesson mechanised, and until now it had never fired.

**True:** it fires on *any* run in which the mutant's test pass fails, and a mutant's test
pass can fail for reasons that have nothing to do with the mutant. The first time it ever
fired, at the P4e-i boundary, it was wrong.

**What it said.** 525 mutants, 443 caught, 9 survivors against 11 acceptances, and:

    FAIL — 2 recorded acceptance(s) no longer survive; the mutant became killable
      crates/imaging/src/metrics.rs: replace / with % in sharpness
      crates/imaging/src/metrics.rs: replace / with * in sharpness

Both are N26's, whose argument is an *equivalence*: the Laplacian response sums to exactly
zero, so `0/n`, `0*n` and `0%n` are the same number. N26 even predicted this firing and
named its cause — "an `imageproc` release that changes the border handling" — which made
the report initially credible. That cause did not apply: `imageproc` had not moved, and
P4e-i does not touch `crates/imaging` at all (`git diff --stat` over it is empty).

**What was measured, in order, before anything was changed.**

1. Both mutants applied by hand to the working tree: **867/867 tests pass**, twice. The
   mutants survive.
2. The three tests the floor's per-mutant logs name as failing, run directly against each
   mutant: **pass**, at 1.4s where the floor recorded 5.5-5.9s.
3. The same three tests, unmutated, under eight CPU spinners: **pass**.
4. The mutant re-applied in a *fresh* tree with cargo-mutants' own environment
   (`CARGO_PROFILE_DEV_DEBUG=0`, its own target directory): **passes**.
5. N26's equivalence claim re-measured directly rather than trusted, over the population
   most likely to break it — a horizontal gradient, an interior bright pixel, a bright
   pixel **on the border**, a bright **corner**, and a hard vertical edge, all 64x48:
   every Laplacian response sums to **exactly 0**. The samples are `i16` widened to `f64`,
   which is exact, so the sum is an exact integer and `0/n == 0*n == 0%n` is arithmetic
   rather than tolerance.

(5) is a proof, not a hypothesis, and it settles the rest: **the two programs are
identical, and identical programs cannot make a test fail.** So whatever failed those runs,
it was not the mutation.

**What did fail, from the floor's own per-mutant log:**

    thread 'an_interrupted_sweep_says_where_it_stopped_and_keeps_what_it_took' panicked
    assertion `left == right` failed: the device's own answer was reshaped on its way out:
      frames did not settle within 5303 ms (11 frames seen)
      left: SettleTimeout   right: DeviceGone

The test scripts a device that vanishes and asserts `DeviceGone`. Under eight concurrent
cargo builds the frames did not arrive inside the settle deadline, so the sweep answered
`SettleTimeout` — a different, entirely correct typed error — and the assertion failed.
`crates/engine/tests/sweep.rs` builds its context with `MonotonicClock::new()`, a **real**
clock, where AGENTS.md's convention is "settle logic runs on a stepped clock in tests".

The apparent specificity that made the report convincing — two mutants, three failures, all
in the tests that consume `sharpness` — dissolves the same way: those are the *slowest*
engine tests, so they are the ones a contention threshold reaches first. Scoring frames is
what makes them slow and what makes them consume the metric; the correlation is real and
the causation runs the other way.

**Changed:** nothing in the register. Both lines stay, and N26 stands with its argument
strengthened by (5) — it had been asserted over nine 3x3 positions and four fixtures, and
is now measured over borders, corners and hard edges at a realistic size.

**The real defect, which is not the floor's:** two engine tests can be handed a different
typed error by a loaded machine. That is a determinism defect in the suite, it predates
P4e-i (the tests are P3's), and P4e-i only exposed it by making every mutant's test pass
longer. Scheduled rather than fixed here, because the principled repair is a stepped clock
on a path where `SteppedClock` is deliberately not `Sync` (note N45) — a scoped piece of
work, not a line. docs/7's standing debts carry it.

**And the thing this entry exists to say.** N52 recorded that the floor's verdict once moved
with `nproc`, and warned about the reflex it creates: "a gate that cries wolf does not get
believed — it gets re-run at `-j1` until it agrees, and the run after that is the one where
a real survivor is waved through". This firing is that warning's second instance, in the
one direction nobody had exercised. **The floor was deliberately not re-run to obtain a
green**, because a re-run that agrees proves nothing about which of the two answers was
right; what settled it was applying the mutants by hand and measuring the arithmetic they
claim to change. A register whose second direction can be tripped by an unrelated flake is
only as trustworthy as the suite's determinism — so the determinism is the thing to fix,
and until it is, a second-direction failure means "investigate", never "delete the line".

**Retires when:** the settle path in `engine`'s integration tests runs on a stepped clock,
at which point this entry keeps only its last paragraph.

---

## N61 — A `cancel()` that does not *wait* buys the cheaper half of its own ordering, and only a test that stops a real process can see it

**Doc:** AGENTS' daemon rule — "open MJPEG/WS streams are cancelled, never awaited, on
shutdown" — design §2.6's shutdown sentence, docs/7 P4e-ii, and docs/9's commissioned
signal-parity row ("one test per signal, real delivery, drain asserted with open subscription
+ mid-flight sweep"). Note **N58** is why that row is this half's. This entry is P4e-ii's real
finding: the commissioned test was red on the *shipped* build, and it was right.

**Believed:** that step 3 of the teardown — every open subscription ends carrying
`events::SHUTTING_DOWN`, and it ends *before* the transport carrying it stops — was bought by
writing the two statements in that order. `daemon::shutdown`'s header said so in as many
words ("reversing steps 3 and 4 would silently buy the cheaper half"), and eleven unit tests
over a recording double agreed with it: in every one, the token was cancelled before the
transport was stopped, and the step's own test even stamped the transport-stop with how many
subscriptions were still open when it happened.

**True:** the ordering is *necessary and not sufficient*, because the thing being ordered is
not the thing the client sees. `CancellationToken::cancel` wakes a subscription's task; that
task then has to be scheduled, return, and have jsonrpsee put its close frame on the
connection. jsonrpsee 0.26 sends a cancelled subscription's close frame **from a task it
spawns after the subscription body returns**, and it holds no connection open for one — so a
transport stopped in the meantime closes the connection with the ending still in flight. What
the client gets is a socket that went away, which is exactly the half step 3 exists to refuse,
bought by a race rather than by an ordering.

**Why the whole workspace was blind to it, which is the part worth keeping.** Every test that
had ever exercised this path was **in-process**: the fixture's server and the fixture's client
live in one process, so the transport's own socket outlives the "daemon" no matter what the
teardown does, and a frame still in flight arrives anyway. The two seams the unit tests drive
are a recording `Notifying` and a scriptable `Stopping`, and neither can be late — they record
synchronously, which is what makes an order assertable at all and also what makes them unable
to reproduce a scheduler. `crates/daemon/tests/signals.rs` is the first test in this project
that stops a real **process**, and the difference is not a matter of realism: when the process
exits, everything it had not yet written is gone. That is the whole gap, and it is a gap no
amount of in-process rigour closes.

**What was measured.**

- Against the shipped build (`ffa1ff7` + `bb63e8a`), the two signal tests failed **about half
  the time** — the client read a closed connection where it expected a `SHUTTING_DOWN`
  payload. Half is what a race between "the spawned close-frame task is scheduled" and "the
  transport stops" looks like when nothing orders them.
- With the wait in place: **0 failures in 80 runs under eight CPU spinners**, and **0 in 5
  full-workspace runs**.
- The residual, below, is **1 failure in 60** with the subscription on an *idle* connection
  and eight spinners.

**Changed.** Step 3 now cancels *and then waits for the live subscription count to reach
zero*, on `Wchd::watch_subscribers` — the `watch::Receiver` P4e-i published for a different
reason — so this is a wait on an event and never a poll of a counter (AGENTS bans the
alternative by name). Two properties of the wait are decisions rather than details:

- **It is one deadline, not two.** `limits::DAEMON_SHUTDOWN_DRAIN_MS` is taken once as a
  `tokio::time::Instant` at the top of the teardown and *shared* between step 3's wait and
  step 5's drain, so a teardown whose subscriptions were slow to end has that much less drain
  and the daemon's worst case stays this number rather than a multiple of it. A fresh timeout
  per step would have put the worst case within reach of the `TimeoutStopSec` this constant
  was chosen to stay under by a factor — and the pair that would then have been wrong together
  is exactly the pair `scripts/gates/systemd-units.sh` exists to keep honest. The bound is on
  the *stop*, not on each of its parts.
- **Expiry is a `warn!` with its own sentence**, different from the drain's, because it is a
  different failure: a subscription that did not end is a client that will be told nothing,
  where a drain that expired is a request that will not be answered. Never a silence (AGENTS
  rule 3).

**The residual, stated rather than hidden.** The live count drops one step *before* jsonrpsee
queues the frame — the subscription's task decrements on its way out, and the frame is the
spawned task's business afterwards — so the wait is a very good proxy and not the fact itself.
Under heavy load an **idle** connection's subscription can therefore still lose its ending:
measured 1 in 60 with eight spinners. `signals.rs` does not paper over it; it rides its
subscription on **the connection carrying the in-flight sweep**, where graceful shutdown drives
that call to completion and the close frame has the whole drain window behind it. That is a
property of the fixture and it is written where the fixture is, so nobody later "simplifies"
the suite by opening a second connection for the subscription. Closing the residual for real
needs a signal from jsonrpsee that the frame has reached the transport, which 0.26 does not
offer — the same shape as note **N57**'s missing transport signal, and recorded in docs/7's
standing debts rather than worked around. docs/6 §2.6's shutdown sentence is unaffected: it
says cancel, drain, release, and all three still happen in that relation.

**`biased;` in `events::forward`'s `select!` was tried and reverted.** The idea was to make
the `Shutdown::cancelled` arm win over a queued event so the ending is produced sooner. It is
not a fix and it costs a property. Not a fix, because the window is not inside the body: the
frame's fate is decided *after* the body returns, in a task jsonrpsee spawns, so arm order
changes only which microsecond the body returns in and nothing about whether the frame is on
the connection when the transport stops. And it costs the thing N59 landed: putting
`Feed::Ended` **in** the channel was deliberate, so that a subscriber which is behind gets
what it already has before it is told the source stopped; an arm that always wins over a
deliverable event is that ending raced against the queue again, spelled as a scheduling hint.
The wait is where the ordering belongs, because the wait is about the thing that is actually
late.

**Retires when:** jsonrpsee (or whatever transport succeeds it) offers a way to observe that a
subscription's close frame has reached the connection. Then step 3 waits on *that* instead of
on the live count, the 1-in-60 residual closes, and this entry keeps its second paragraph as
the reason the wait exists at all.

---

## N62 — The fifo is the external hold: a subprocess sweep can be wedged only because a sample photo is written with a **blocking** `std::fs::write`

**Doc:** AGENTS' "No `sleep` as synchronization", docs/9's signal-parity row (a drain asserted
"with open subscription + mid-flight sweep"), and note **N51**, which is the entry this one
depends on in the direction that matters — N51 made a photo's *destination* a non-blocking
open, and the reason this technique works is that `calibrate.rs`'s sample writes deliberately
did **not** move with it. Numbered separately from N61 rather than folded into it because its
retirement condition is about a different file and a different change: N61 retires on a
transport signal, this one on `calibrate.rs` moving a write.

**Believed:** implicitly, that "hold a sweep mid-flight and then signal the process" was
arrangeable the way the in-process suites arrange it. It is not. The in-process fixture arms a
`Gate` decorator and releases it over a channel; a subprocess `wchd` has no such door — the
fake has no timing knob reachable from a profile or an environment variable, `fake::Fault` is
in-process only, and nothing in the fake or the engine sleeps. Without an answer, docs/9's row
degrades to signalling an idle daemon, which asserts the drain of nothing.

**True:** the daemon already contains one blocking call on the sweep's own path, and it is
there on purpose. Sweep sample photos go through `photo::WhereverTheCallerSaid`'s
`std::fs::write` — deliberately *not* N51's `WRONLY | CREATE | NONBLOCK | CLOEXEC` open, which
`calibrate.rs` argues for the session's own files. So a **fifo at the second sample's photo
path** wedges the sweep inside `open(2)` until the test opens the read end: a hold with no
timer in it, released by an action rather than by a duration, which is the only kind this
workspace allows. The path is derived from the `Session` the daemon answered with, so nothing
is transcribed; and the bytes crossing the fifo are read and asserted, which is what says the
wedge was the *sweep* and not the test's idea of where the sweep would be.

**What was measured:** with the fifo in place the daemon is provably mid-sweep when the signal
arrives (one sample answered, one in flight), and after the drain the samples the sweep
answered with and the samples on disk agree at 2 — the drain claim a loaded machine cannot
otherwise decide. The seeded "stop that does not drain" produces 1 of 2, on disk and in
flight, which is the failure this arrangement exists to be able to see.

**Changed:** `crates/daemon/tests/signals.rs` holds the technique, and the harness three
suites now share (`tests/support/wchd.rs`, note **N49**'s boundaries) is what made a third
hand-written spawn unnecessary.

**Retires when:** `calibrate.rs` moves sample-photo writes onto a non-blocking path — which is
a legitimate change, and N51's own argument points at it. On that day this suite's wedge stops
working, and the requirement is that the tests are **re-armed rather than deleted**: the claim
they make (a real signal, a real drain, a sweep provably in flight) is docs/9's commissioned
row and does not become less true because its hold moved. The next hold has to be another
*action*-released one; a sleep would be a worse test wearing the same name.

---

## E11 — G4 evidence: socket activation and the journal, against a real service manager, 2026-08-11

E9 is the shape this follows: a dated run against something this project does not control. The
two claims below cannot be produced by any in-process fixture or by
`crates/daemon/tests/systemd.rs`, which binds a notify socket of its own — a descriptor passed
in through `LISTEN_FDS` and a stderr that *is* the journal are things only a service manager
does.

**Host:** the P4d/P4e workstation, kernel `7.0.0-29-generic`, systemd user manager running.

### Socket activation, compared by inode

    $ ./scripts/gates/socket-activation.sh
      note  the socket this gate has systemd bind is $XDG_RUNTIME_DIR/webcam-handler/wchd.sock
      note  the daemon served inode 859855 at /tmp/wch-socket-activation.MHc70iZ6/adopting/webcam-handler/wchd.sock — the one systemd bound, not one of its own
      note  the daemon's readiness line reached the journal as a structured entry (_TRANSPORT=journal), so the journald layer replaced the stderr formatter rather than joining it
      ok    socket-activation: checked 1 activated daemon(s) serving the very socket systemd bound, compared by inode
      ok    socket-activation: checked 1 startup refusal over an abstract-namespace socket passed in through LISTEN_FDS
      ok    socket-activation: checked 1 startup refusal over two descriptors passed in through LISTEN_FDS
      ok    socket-activation: checked 1 log line(s) from a daemon whose stderr is a real journal, checked for the transport that says which layer wrote it
    PASS socket-activation — 4 items examined, 0 named skip(s)

`systemd-socket-activate` bound the socket and started `wchd` on it. The inode is read twice —
once when the activator says it is listening, once when the daemon says it is serving — so
"never binds its own" is **asserted** rather than assumed: a daemon that unlinked the inherited
socket and bound its own would leave a different inode at the same path, and every client that
had already connected would be talking to nothing. The two refusals are the other half of D11
asked of a socket this daemon did not bind: an abstract address has no directory, no mode and
no owner, and two descriptors is a guess an operator cannot see.

### The journald layer replaces the fmt layer

The fourth claim ran under a transient `systemd-run --user` unit, and the discriminator is
`_TRANSPORT`: an entry the journald layer wrote is `journal`, and the same line rendered to a
stderr that systemd captured is `stdout`. It came back `journal`, which is the claim design
§2.6 makes ("journald layer … under systemd") and the reason `daemon::logging` installs it
**instead of** the fmt layer rather than beside it — both would put every line in the journal
twice.

### The unit files, re-derived rather than read

    $ ./scripts/gates/systemd-units.sh
      note  derived from the tree: socket $XDG_RUNTIME_DIR/webcam-handler/wchd.sock, shutdown drain 20000ms
      note  packaging/systemd/wchd.service: TimeoutStopSec=45000ms > DAEMON_SHUTDOWN_DRAIN_MS=20000ms, so the daemon's own bound is what fires
      ok    systemd-units: checked 1 service unit(s) checked for Type=notify, no fork, no PIDFile, NotifyAccess and Restart
      ok    systemd-units: checked 1 socket unit(s) checked for SocketMode 0600, DirectoryMode 0700, and a ListenStream ending in webcam-handler/wchd.sock
      ok    systemd-units: checked 1 (TimeoutStopSec, DAEMON_SHUTDOWN_DRAIN_MS) pair(s), each re-derived from the crate rather than transcribed
    PASS systemd-units — 3 items examined, 0 named skip(s)

The second note is the pair that can only be wrong together, printed with both numbers so the
verdict is legible rather than trusted.

### What this run establishes, and what it does not

**Establishes:** on a host with systemd, `wchd` adopts the socket a service manager binds and
serves that inode; it refuses the two inherited shapes D11 cannot authenticate; and its log
reaches the journal as structured entries from the journald layer, not as captured stderr.
**Every arm ran for real and no skip was taken** — the named counted skips exist and were
exercised green by an arm of their own (`pass_case_a_host_that_cannot_pass_a_socket_in_declines_in_a_way_that_is_counted`),
so a host without these tools declines in a way a reader of CI output can see.

**Does not establish:** that systemd would *accept* the shipped units (`systemd-analyze
verify` is not run — docs/9's gaps register says why); that the units behave under the *system*
manager, since `wchd.service` is a **user** unit and this run used the user manager; or
anything about the substitution window on the inherited path, which is closed by the unit's
`DirectoryMode=0700` and not by this daemon (N39's amendment of the same date).

---

## PF:22 — `/dev/videoN` is probe-order bookkeeping: a `uvcvideo` reload renumbered three of four cameras and changed nothing about any of them

**Measured** 2026-08-11 on kernel `7.0.0-29-generic` (x86_64), four cameras attached — the
same host and hardware as PF:19 and E9. Continues the docs/6 §1.2 registry; cite it as
`[PF:22]`.

**Design §1.2 gets no new bullet, following the convention PF:17–21 already set.** That
section's bullets run PF:1–16, and its own header states the rule: a new finding "always
lands first" here, "before a revision of this document absorbs it". Five entries have
landed that way since; this is the sixth, and the next v2 revision absorbs them together
or not at all. §1.2 also already carries the sentence this entry measures — *"Node
numbering (`/dev/video0` vs `video1`) is never load-bearing"* — so what changed is not the
design's claim but the evidence for it, and the fact that the code disagreed.

The reload is not hypothetical here. This project's own R3 hotplug arm (E9, docs/7 P4d)
unloads and reloads `uvcvideo` through `wch-priv` as part of its evidence run — so the
event that renumbers the nodes is one the test suite *performs*, several times a session.

### The measurement

`./target/debug/wch list --json` against `corpus/profiles/*.json`, matched by fingerprint:

| profile | committed nodes | live nodes | `card` | `bus_info` |
|---|---|---|---|---|
| `chicony-rgb` | `/dev/video0,1` | `/dev/video2,3` | unchanged | unchanged |
| `chicony-ir` | `/dev/video2,3` | `/dev/video4,5` | unchanged | unchanged |
| `obsbot-tiny3` | `/dev/video4,5` | `/dev/video0,1` | unchanged | unchanged |
| `dell-u3224kb` | `/dev/video6,7,8,9` | unchanged | unchanged | unchanged |

Three of four cameras moved. The Dell did not, and the `bus_info` column says why without
having to be a separate measurement: it hangs off a different PCI root
(`usb-0000:00:0d.0-3.4.1.1`, the dock) from the three on `usb-0000:00:14.0`, and it kept
the four highest minors — the shape you get when a reprobe reorders one controller's
devices and leaves another's alone. Do not read the Dell's row as "docked cameras are
stable"; read it as "one controller was reprobed and one was not". The three that did move
moved as a **rotation** — the OBSBOT went first this time — which is exactly what "probe
order" means, and exactly what no re-capture can pin down.

Nothing else moved. For the OBSBOT the two sides are byte-identical but for the path:

```
left:  [DeviceNode { path: "/dev/video4", kind: VideoCapture, device_caps: 69206017, capabilities: 2225078273 }, DeviceNode { path: "/dev/video5", kind: MetaCapture, … }]
right: [DeviceNode { path: "/dev/video0", kind: VideoCapture, device_caps: 69206017, capabilities: 2225078273 }, DeviceNode { path: "/dev/video1", kind: MetaCapture, … }]
```

`card`, `bus_info`, the node count, and every node's `kind`, `device_caps` and
`capabilities` survived on all four. So did every fingerprint: `bus_path` is the sysfs USB
*interface* path (`3-1:1.0`) and not a minor number [PF:13], which is why all four profiles
still found their camera to be compared against at all.

### The transcript, from the arm that used to assert the names

`just smoke-hw`, same session, after the comparison moved. The rung prints every path that
moved precisely because it no longer asserts it — a field a test silently ignores is one
nobody can audit, and this is where the next reading of this finding will come from:

```
obsbot-tiny3: enumeration matches the committed profile; its node paths were reassigned by the kernel and are not identity [PF:22]: /dev/video4 → /dev/video0, /dev/video5 → /dev/video1
chicony-rgb:  enumeration matches the committed profile; its node paths were reassigned by the kernel and are not identity [PF:22]: /dev/video0 → /dev/video2, /dev/video1 → /dev/video3
chicony-ir:   enumeration matches the committed profile; its node paths were reassigned by the kernel and are not identity [PF:22]: /dev/video2 → /dev/video4, /dev/video3 → /dev/video5
dell-u3224kb: enumeration matches the committed profile
3 of 4 matched camera(s) sit at different /dev/videoN paths than when their profile was captured, and none of them changed

Summary [  53.484s] 16 tests run: 16 passed, 910 skipped
smoke-hw: 7 claim(s) declined by tests that ran — each named above
smoke-hw: suite run, 0 named skip(s) before it started
```

The Dell prints the unadorned line, because its four paths did not move. Sixteen of sixteen
green with the other three still rotated, motor arms included (owner ruling, 2026-08-08),
and the seven partial skips are the Chicony IR camera's usual control-poverty declines —
the same seven as before this change.

### What it means for the code

Node numbering can be *displayed*, *opened*, and *recorded as provenance*. It can never be
**asserted as identity** against anything captured on another boot — which the R3
enumeration arm was doing, and which is what made it red on this date about a machine on
which nothing had happened. The comparison moved; the corpus did not. Note **N63** carries
that argument and `CameraInfo::differing_fields` carries it in the code.

**Retires when:** a kernel is measured that assigns `/dev/videoN` from something stable
across a driver reload — a device property rather than a probe counter. Nothing in
`uvcvideo` or in the v4l2 core suggests one is coming; minors come from
`video_register_device`'s first free slot.

**Adjacent:** E9's "What it does not establish" lists *"Node renumbering is untested …
a guard that exists and has not fired"*. It has now fired, on the arm above it rather than
inside E9's own cycle; E9 carries a pointer.

---

## N63 — The profile's section is called `invariant` and one field in it is not: `/dev/videoN` moved, so the comparison moved rather than the corpus

**Believed:** that `ProfileInvariant` earned its name field by field — that "the part of a
profile that should not change unless the device or the kernel does" could be compared with
`==`, as `DeviceProfile::invariant_matches` did, and that `CameraInfo`'s node list was a
description of the device. The R3 enumeration arm believed it twice over, asserting
`profile.invariant.info.nodes == info.nodes` directly.

**True:** `CameraInfo::nodes` is two claims wearing one type. Per node, `kind`,
`device_caps` and `capabilities` say what the device *is*; `path` says what the kernel
happened to call it on the boot the capture was taken. The first is invariant. The second
is a counter.

**What was measured:** PF:22, 2026-08-11. One `uvcvideo` unload/reload — the cycle this
project's *own* R3 hotplug arm performs as evidence (E9) — rotated three of the four
attached cameras through each other's node numbers with `card`, `bus_info`, node counts and
every caps word unchanged. `hw_enumeration_matches_the_committed_profile` went red for all
three. `hw_profile_capture_reproduces_the_committed_invariant_section` had the identical
defect and never got to show it: `engine::profile::capture` copies `camera.info()` verbatim
into the invariant section, node paths included, so `self.invariant == other.invariant`
compared them too — but nextest fails fast by default and the whole `hw_` suite runs
single-threaded in the `exclusive-device` group, so the enumeration arm's failure ended the
run before the capture arm was reached. One defect, two arms, one report. Both are green
now with the same four cameras attached and the paths still rotated.

**Changed:** the comparison, in one place. `CameraInfo::differing_fields` joins
`CameraFingerprint::differing_fields` in `crates/schema/src/camera.rs` with the same
signature idiom and the same sentence — "the fields where `self` and `other` disagree, in a
stable order". It compares the fingerprint (through that neighbour, so PF:8's
absent-serial rule is not re-spelled), `card`, `driver`, `bus_info`, `backend`, the node
**count**, and per node `kind`/`device_caps`/`capabilities`. It does not compare `path`.
`invariant_matches` calls it for the `info` half and keeps `==` for formats, controls and
pairs; the R3 enumeration arm calls it directly. Both sides destructure rather than
field-access, so a new field on either struct stops compiling until somebody decides which
half it belongs to — the one home (design §2.10) enforced by the compiler rather than by
memory.

### Why the schema did not change

`DeviceNode.path` stays. It is capture-time provenance, it is what the fake backend replays
(a statement about the machine a profile came from, not a claim about this one), and it is
what a refusal has to name when a node cannot be opened. Deleting it to make a comparison
correct would be fixing the wrong artifact — and it would silently move
`schemas/webcam-handler-schema.json` and every committed profile.

### Why a re-capture is not the fix

This is the sentence worth writing down, because re-capturing is the obvious move and it is
wrong. A re-capture bakes *today's* arbitrary numbering into the corpus and produces a
green run that means nothing; the next reload breaks it again, and this project performs
that reload deliberately. Worse, it would teach the habit that corpus red means "re-capture
until green", which is the exact failure the arm's own message warns about ("neither is
fixed by re-capturing without saying why"). The corpus was never stale. Profiles are
immutable once committed (AGENTS: "re-capture replaces wholesale") and there was nothing to
replace: the four documents describe the four cameras correctly, and did on both sides of
the reload.

### Why this is a test defect and not a data loss

`CameraFingerprint` — `bus_path`/`usb_id`/`card`/`driver`/`serial` — holds no node path,
and `CameraFingerprint::slug` is what keys D9's session directories. A calibration session
recorded before a reload still finds its camera afterwards, and `calibrate apply`'s
conservative match never consulted a minor number. Nothing persisted was ever keyed on the
thing that moved; only an assertion was.

### `CameraId` is excluded too, and that one is unmeasured

`assign_ids` hands out collision ordinals over the card names of *every* attached camera in
enumeration order, and enumeration order is node order — the thing PF:22 says the kernel
reassigns. So on a host with two identically-named cameras, a reload could swap which is
`cam:webcam` and which is `cam:webcam-2` with neither device changing: the same defect in a
second costume. `CameraId`'s own doc already says it is "never persisted as identity". It
costs nothing to drop from the comparison, because the only device-derived input to an id
is `card`, which *is* compared. Unlike the path finding this one has not been observed — no
two attached cameras collide on this host — so it is named here rather than claimed as
measured, and it is written down because the next reviewer will otherwise re-derive it.

### Both directions

The hardware arm is `#[ignore]`d and needs a camera, so the population comparison is
exercised over values in `crates/schema`, which is where the both-directions proof lives:
`a_renumbered_node_is_not_drift_because_dev_video_n_is_probe_order`,
`a_camera_that_changed_shape_still_goes_red_at_every_field_that_describes_it`,
`identity_still_has_to_match_and_the_report_names_which_half_moved`,
`a_camera_id_is_not_compared_because_it_is_derived_from_the_whole_topology`, and
`renumbering_the_nodes_does_not_read_as_corpus_drift_either` for the second consumer.

All eight checks the function spells — `card`, `driver`, `bus_info`, `backend`,
`nodes.len`, and per node `kind`, `device_caps`, `capabilities` — were removed one at a
time and the suite watched to go red on that check alone (AGENTS rule 2); the ninth,
`fingerprint`, is delegated and carries its neighbour's own arms. Two whole implementations
are worth naming because they are the two ways to get this wrong: **the defect as it
stood**, `nodes` compared with `==`, which the renumbering arm catches; and **the
over-correction**, the node set dropped entirely, which the shape arms catch and which
would have made R3 green and worthless.

The enumeration arm also *prints* every path that moved rather than passing over it in
silence, because a rung that quietly ignores a field is one nobody can audit — and that
line is where PF:22's next transcript will come from.

**Retires when:** PF:22 does, or when `/dev/videoN` becomes load-bearing somewhere the
comparison would have to follow.

---

## N64 — The runtime path went to the schema and the state path stayed, because the line is what a directory is *for*; five gate files proved they were derived by going red

**Doc:** design §2.10 ("one home per law"), T4 ("a verb exists once"), T6 and
`scripts/gates/dependency-walls.sh`'s Wall 3 (the thin client links no engine and no
backend), D11 (the socket's directory is the whole authentication model), and docs/9's
derived-population rule. Note **N2** is the entry whose path citation this one amends —
`directories` is still not a dependency and the count is still two paths in ~thirty lines;
what changed is that they now have one home each.

**Believed:** implicitly, that "the XDG paths" were one subject with one owner, so
`engine::paths` could hold both because the engine was the first thing that needed either.

**True:** they are two subjects, and P4f is where the difference stopped being academic.
`wchc` has to resolve `$XDG_RUNTIME_DIR/webcam-handler/wchd.sock` to connect —
`schema::limits::DAEMON_SOCKET_FILE` already sat in the schema saying so in as many words
("here rather than in the daemon because `wchc` has to resolve the same path to connect
(P4f)") — but the other two thirds of that path, `APP_DIR` and `runtime_dir`, were in the
engine, and Wall 3 forbids the client from linking it. **One string was reachable and the
rule that composes it was not**, which is a decision left half-finished rather than a
dependency problem.

**The line, and it is not "who wrote it first".** A **runtime** directory is a *transport*
fact: the daemon binds a socket in it and a client connects to that socket, and neither
should have to link a session store to find a file descriptor. A **state** directory is a
*storage* fact: D9's session tree, touched only by things that already link the engine. So
`schema::paths` takes `Env`, `SystemEnv`, `MapEnv`, `APP_DIR`, `runtime_dir` and
`usable_dir`; `engine::paths` keeps `state_dir`. One home each, and deliberately **no
re-export shim** — a second name for one item is exactly the drift this move exists to
prevent, and a shim would have made the move invisible to the gates below, which is the
half of the value.

**One item's home was decided by a dependency instead of by that line, and it says so.**
`TempRuntimeDir` is a `tempfile::TempDir`. Moving it with the function it makes fixtures
for would put `tempfile` in the *schema's* shipping edges, to serve tests — so it stays in
`engine::paths`, and `crates/client`'s tests dev-depend on the engine, which Wall 3 permits
because it counts shipping edges only. Both module headers carry that sentence rather than
leaving a reader to wonder why the fixture and the function parted company. It is worth
naming as a *class*: a boundary drawn by a law can still be bent by a dependency, and the
honest response is to say which one decided, not to pretend the law did.

**The fact worth keeping, and it is the derived-population rule paying for itself.** Five
gate files `sed` `APP_DIR` out of the crate that defines it, and **all five went red the
moment it moved** — `uds-permissions.sh`, `systemd-units.sh`, `socket-activation.sh` and
the case files for the first and the third, which write stub daemons that have to land
their sockets where the real one would. (P4f's own `cli-parity.sh` and its cases make it
seven; they read `crates/schema/src/paths.rs` from the day they were written.) That is the
rule working exactly as `json-validates.sh`'s header argues
for it: a gate that had transcribed `"webcam-handler"` would have stayed green and started
lying, checking a directory nothing binds in against a name nothing uses. A red gate on a
pure move is not a cost of the move; it is the receipt.

**`Program` is a program identity, not a second surface.** The other half of the same
commit. `cli_core::Program` is a closed vocabulary (`Wch`, `Wchc`) and the name is a
**parameter of the parse**: `Program::command()` is `Cli::command().name(self.as_str())`,
so `--help`, `--version` and the usage block of every clap error come off **one** tree with
the right binary's name on them. `#[command(name = "wch")]` is gone from the derive, so a
caller reaching for `Parser::parse` can no longer put `wch`'s name in `wchc`'s mouth by
accident. The error prefix is the same question and so it is the same value:
`Program::error_line` owns `{program}: {error}`, read by `wch`'s root and by the two
`report_probe` diagnostics that also printed it. Forking the tree instead would have been
the one thing T4 forbids — and it would have made the parity gate a comparison of a surface
with itself, which `Program`'s own doc records.

**Measured rather than asserted, because a `--help` change is a gate population change.**
`json-validates.sh` scrapes `wch --help`, and P4f's parity gate scrapes it at two levels, so
a byte of drift there moves two populations. A 781-line dump — root `--help`, `--version`,
all ten verbs, every subcommand, and three error paths including clap's usage block — was
built from the parent commit in a throwaway worktree and diffed against the same dump after
the change. Same SHA-256 both sides.

**The two new tests are anchored rather than `contains`-based**, and the reason is
specific: `wch` is a prefix of `wchc`, so `assert!(stderr.contains("wch"))` could never go
red on the defect it is aimed at. Both were watched failing on a hand-applied inverse.
`the_command_tree_is_well_formed` now `debug_assert`s once per root, so a tree that is
malformed under only one of the two names is still caught.

**Retires when:** nothing retires it. It is a boundary, and the entry exists so the next
person to reach for a re-export shim reads why there is not one.

---

## N65 — The client's runtime has one thread and its sweep has no drain, both measured; and the subscribe-before-call ordering is argued, not proved

**Doc:** design D10 and D12, T5, note **N57** (one declaration, two generated traits; the
per-client calibration stream and what a subscriber that falls behind is told), note
**N38** (the way a dependency adoption is re-measured), AGENTS' "bounded everything" and
its ban on waiting out a timer, and docs/6 §2.8 (the dependency registry).

**Believed** — three things, each of which would have been reasonable to write down without
checking, and each of which is checked here instead.

**1. That the client's runtime shape is a preference.** It is not, and the difference is
visible from outside the process. `wchc` runs *one verb per invocation*, so its only
concurrency is a call and the connection's background task, and a current-thread runtime
drives both inside the `block_on` that is already there. **Measured:** `wchc list` against a
real daemon, reading `/proc/self/status` at the moment the client is built, is `Threads: 1`
as shipped and `Threads: 9` on the same 8-core host with `Builder::new_multi_thread` — eight
worker threads spawned to serve a program that issues one request. Reaching for them at all
needs `tokio/rt-multi-thread`, a feature this crate deliberately does not enable, so the
cheap choice is also the one the manifest makes visible. What it **costs** is stated rather
than assumed: the background task makes progress only inside `block_on`, so between two
`Executor` calls this client is not reading its socket. Nothing depends on it doing so —
pings are off, the daemon sends nothing unsolicited except on a subscription, and the one
subscription this binary opens is drained inside the same `block_on` as the call it belongs
to.

**2. That the sweep needs a drain after its `select!` loop.** A counting drain *was*
written, and then measured: **zero events on every one of five runs**, so it was deleted.
The absence is a consequence of (1) rather than an omission — on a current-thread runtime
the connection's background task can only run while this future is awaiting, so nothing can
be pushed onto the subscription *between* the two polls of one `select!` turn, and the
`biased;` ordering polls the events first. A poll after the break could only ever find the
queue empty. An event the *daemon* writes after its response is not waited for either, and
must not be: waiting for a terminal event would hang forever whenever one was dropped, and
dropping is a thing `wch_subscribe_calibration` is explicitly allowed to do (N57). This is
the shape a deleted mechanism should leave behind — a measurement in the doc where the code
used to be, so the next reader does not re-add it on the same intuition.

**3. That subscribing before calling is proved by the test that exercises it. It is not,
and this is the honest half.** The ordering matters — the daemon buffers nothing for a
client that has not arrived (N57: "a parked long-lived `Receiver` would hold a whole
sweep's events for nobody"), so a subscribe-*after*-call would silently drop the start of
every sweep. But a subscribe-after-call **mutant stayed green**: the sweep opens a camera
and settles a sensor before its first event, which is far longer than the round trip a late
subscribe costs, so the race never lost. The test says so where it is written rather than
claiming credit for a property it did not establish. Closing it for real needs a daemon
that emits an event *before* it touches hardware, or a fault seam that holds the first
emission — neither exists, and inventing one for this would be a fixture asserting the
thing it was built from. Recorded in docs/7's standing debts.

**The dependency N57 declined to pay, paid here and re-measured.** `jsonrpsee`'s
`async-client` feature is the only route to the generated *subscription* client —
`SubscriptionClientT` is unimplementable outside `jsonrpsee-core` — and it drags
`futures-timer`, which was in neither the lock nor the local cache. N57 named that cost at
P4e-i and left it; this is the consumer that needs it. Re-measured the way N38 did:
**exactly one package joins the graph** (`futures-timer 3.0.4`, MIT OR Apache-2.0),
`webcam-handler-api`'s closure goes 112 → 115, and **no web-stack crate** enters. The
closure figures are `3992d88`'s, taken at the time; the half of that measurement anyone can
re-take later is the lockfile, and it agrees — `git diff ed51d18..3992d88 -- Cargo.lock`
adds exactly one `name =` line, `futures-timer`. Recorded because a package count is only as
re-checkable as the command that produced it, and a lockfile diff needs no command to be
remembered.

**Whether `futures-timer` earns an entry of its own: no, and here is the decision.** It is
an *inert* `[workspace.dependencies]` row — no crate in this workspace names it, and the
lock is what pins it. An N-entry is a justified deviation or a piece of case law, and there
is no deviation here: the crate cleared the standing bar (permissive, pinned at adoption,
no git source), which the owner's 2026-08-09 ruling says is not an escalation. What it
needed was for the registry row to *say* that nobody names it and that the lock rather than
that line is what pins the version — an inert row that could otherwise be read as a
guarantee it does not provide. That sentence is in the manifest and the version is in
docs/6 §2.8. A second home for it here would be the duplication §2.10 exists to refuse.

**Two smaller decisions, recorded because each had a worse available answer.**
`Transport::send_ping` is *implemented* rather than defaulted: jsonrpsee's default answers
`Ok(())` without writing a frame, which is a false claim even while pings are off, and a
false claim that becomes load-bearing the day somebody turns them on.
`Incoming::Closed` becomes a refusal rather than a skipped message, so in-flight calls end
rather than wait for an answer that is not coming.

**And `--backend` reads provenance, not value.** The flag carries `default_value = "v4l2"`,
so `cli.backend` always holds one and a value check would accept `wchc --backend v4l2` while
the daemon replayed a profile — a client agreeing with a claim it cannot check. The refusal
reads clap's `ValueSource` and names `wchd --backend` as where that decision lives. It is
N42's shape inverted: there, a flag with no consumer stayed absent; here, a flag on the
shared surface exists once (T4) and the consumer that cannot mean it says so.

**Retires when:** (1) and (2) retire together if `wchc` ever grows a second concurrent
verb, at which point both measurements are re-taken rather than trusted. (3) retires when
the daemon can be made to emit a calibration event before it opens a camera, so the
subscribe-after-call mutant can be watched failing.

---

## E12 — G4 evidence: `wch` and `wchc` byte-identical against the real cameras, 2026-08-11

E9 and E11 are the shape this follows: a dated run against something this project does not
control. The parity **gate** runs over the fake, deliberately and permanently — the shape of
an answer must not depend on what is plugged in, and a gate that needed a camera could not
run in CI. This entry is the other question, asked once: does the claim hold when the
document on both sides is describing hardware?

**Host:** the P4d/P4e/P4f workstation, kernel `7.0.0-29-generic` (x86_64), four cameras
attached — the same host and hardware as PF:19, PF:22, E9 and E11. `/dev/video0`
through `/dev/video9`.

### The run

    $ wchd --backend v4l2 &
    2026-08-11T13:49:40Z  INFO wchd: wchd is serving socket=…/webcam-handler/wchd.sock backend="v4l2"

    $ for each verb: wch --json <verb> vs wchc --json <verb>
    list                                    exit 0/0   3871 bytes  sha256:2da17403f98e5e9a…  identical
    info cam:obsbot-tiny-3-obsbot-tiny-3-st exit 0/0   8415 bytes  sha256:8aa88aa4b961d615…  identical
    controls cam:obsbot-tiny-3-…            exit 0/0  12974 bytes  sha256:b31f53457d08347c…  identical
    get cam:obsbot-tiny-3-… brightness      exit 0/0    378 bytes  sha256:99db3b3cd0ee4616…  identical
    calibrate list cam:obsbot-tiny-3-…      exit 0/0     21 bytes  sha256:655fa99a5156e03d…  identical

    $ kill -TERM <wchd>
    2026-08-11T13:49:40Z  INFO daemon::shutdown: wchd is stopping signal="SIGTERM"
    wchd exit=0

The cameras `list` enumerated, from the same run:

    cam:obsbot-tiny-3-obsbot-tiny-3-st   uvcvideo   OBSBOT Tiny 3: OBSBOT Tiny 3 St
    cam:integrated-camera-integrated-c   uvcvideo   Integrated Camera: Integrated C
    cam:integrated-camera-integrated-i   uvcvideo   Integrated Camera: Integrated I
    cam:dell-u3224kb-a-4k-webcam         uvcvideo   Dell U3224KB/A 4K Webcam

### What this run establishes

All five compared verbs are **byte-identical** across the two roots over the real v4l2
backend — one of them a PTZ device whose control set the fake's profiles were captured from,
and one of them the 4K monitor webcam. The daemon **exits 0 on the SIGTERM that ends the
run**, which is P4e-ii's teardown discipline under its first real client rather than under a
test harness.

Two things are worth naming because they are load-bearing and easy to read past.
**`wch` and `wchd` had the same devices open at the same time**, and neither refused: a
control read does not stream, and exclusive streaming is the constraint (D12), so
"availability is not capability" (E3) has an everyday shape here — two processes reading one
camera's controls is not contention. And **`calibrate list` answered 21 bytes from both
roots**, which is the empty-session document; a comparison of two empty answers proves less
than the others and is recorded as the weakest row rather than counted as an equal.

### What it does not establish

That any *write* verb agrees across the two roots against real hardware — none was compared,
for the reason the gate's own table gives: `wch` and `wchd` would drive two different opens
of one device and a comparison would be of two states. That the parity holds for the four
verbs the gate exempts as `device`, which have no real-hardware comparison anywhere. That it
holds on a machine with no cameras, where both roots answer an empty list and the comparison
is vacuous — which is the case CI would have run had this been a gate. And nothing about the
`wchc` sweep's progress rendering, which needs a terminal to draw anything and was not part
of this run.


---

## N66 — The mutation floor spent the whole filesystem and spelled the shortfall as a FAIL, which is N52's finding in a second dimension

**Doc:** AGENTS rule 3 ("CI executes what it claims … never silence"), rule 1 (a discovered
defect class lands with its gate), docs/9's mutation-floor row, and notes **N52** (the
floor's verdict once moved with `nproc`) and **N60** (what a floor that cries wolf costs:
"it gets re-run at `-j1` until it agrees, and the run after that is the one where a real
survivor is waved through"). Found at the **P4f** boundary, running `just gate-g4`.

**Believed:** that `scripts/mutants.sh`'s job trimming — divide the build root's free space
by a measured per-job figure — was the conservative half of the script. It reads that way:
the figure was measured rather than guessed (3 GiB per tree, with `CARGO_PROFILE_DEV_DEBUG=0`
already applied for exactly this reason), and the trim prints what it did, out loud.

**True:** dividing by the per-job figure spends **all** of it. On this host that was
`16 / 3 = 5` jobs at 3 GiB in a 16 GiB `tmpfs` — 15/16 of the filesystem, with the run's own
`target/mutants.out`, the nextest artifacts and everything else on `/tmp` expected to fit in
the remainder. It had been true for as long as a build tree stayed under three gigabytes.
P4f added a crate — a library, a binary and a subprocess integration suite, with
`jsonrpsee/async-client` and `soketto` on a new shipping edge — and the tree went a little
over.

**What was measured.** The run died **fifteen minutes in**, with 23 mutants caught and 502
untested:

    ERROR Worker thread failed: failed to overwrite "/tmp/cargo-mutants-webcam-handler-zT14Sf.tmp/crates/imaging/src/metrics.rs"
    Caused by:
        Disk quota exceeded (os error 122)
    mutants: cargo-mutants exited 137 after 15m0s
    mutants: FAIL — cargo-mutants could not complete (exit 137); see target/mutants.out

**And that last line is the defect, not the disk.** A filesystem that filled is a
resource fact and a slow, annoying, entirely legible one. What this job did with it was
report **FAIL**, which is the same word a surviving unaccepted mutant gets and the same
non-zero `just gate-g4` gets — so the floor's verdict was, for those fifteen minutes, a
function of how much room `/tmp` happened to have. That is precisely N52's shape ("the same
tree answered FAIL at 8 jobs and PASS at 4") in a second dimension: N52 was about *time* and
pinned `minimum_test_timeout`; this one is about *space*. Two dimensions is enough to call it
a class rather than two accidents, and the class is: **a floor whose budget is derived from
the machine can spell the machine's shortfall as a statement about the code.**

**Changed:** one line of arithmetic and the paragraph that argues it. `per_job_gib` stays the
measurement it is — it was measured and it should not be inflated to buy headroom, because
then it is no longer a measurement — and a **`reserve_gib`, one job's worth, is held back
from the budget** before the division. On this host that is `(16 - 3) / 3 = 4` jobs rather
than five, and the two printed lines say what was held back, so a reader of CI output can
see the difference between "this host runs four jobs" and "this host ran out". Re-run at the
P4f boundary: 4 jobs, no disk error.

**What this deliberately does not do.** It does not make the floor's exit code distinguish a
resource failure from a survivor. That is the deeper fix and it is a bigger one — the exit
would have to be a third outcome that `phase.sh` understands, and a third outcome is a thing
a gate table can get wrong in its own way. The reserve removes the instance; the class stays
open, and it stays open **on the record** rather than in somebody's memory of a bad
afternoon.

**Retires when:** either the floor grows that third outcome — a resource shortfall reported
as a named, counted refusal rather than as a verdict — or the build root stops being a
`tmpfs` sized in the same order as one build tree. Until then, re-read this entry before
raising `per_job_gib`: the figure and the budget are two different numbers, and conflating
them is how this happened.

## N67 — The repair N60 scheduled was blocked on a question the broken path never asks, and the defect was wider than the entry that found it

**Doc:** AGENTS' testing rule, which said "settle logic runs on a stepped clock in tests" and
now names two shapes. Discharges the standing debt N60's last section opened, and the one
docs/7 bound to the G4 boundary.

**Believed:** that the repair was "a stepped clock on a path where `SteppedClock` is
deliberately not `Sync` (note N45)" — scoped work, gated on a `Sync` question.

**True:** the `Sync` question never arises on the path that was broken.
`crates/engine/tests/sweep.rs` and `engine::calibrate`'s own unit tests call `calibrate::run`
on the test's own thread; nothing crosses a boundary, and `crates/engine/tests/faults.rs` had
been passing `&SteppedClock::new(0)` on that same path since P3. What those tests need is not
a clock they *step* — not one of them has anything to say about a duration — but a clock that
cannot reach the deadline at all. That is a weaker thing and a different type:
[`engine::settle::FrozenClock`], which holds no state and is therefore `Sync` **by
construction**, so N45's argument is untouched rather than worked around. N45 forbids sharing
a clock two threads can *move*; a clock that cannot move shares nothing mutable. The
alternative — `SteppedClock` with its `Cell` swapped for an atomic — is precisely what N45
forbids, wearing a different type.

**The defect was wider than the entry that found it.** N60 named two integration tests. Under
sixty-four spinners the old code fails **six of seventeen** per run, and one of them is
`engine::calibrate`'s own unit test, at `SettleTimeout { waited_ms: 5030, frames_seen: 7 }`.
The floor's per-mutant log could only name the test that happened to fail *first*, so the
scope recorded in N60 was an artefact of which test lost the race. **A determinism defect
found through a symptom should have its population measured before it is scoped** — the
population here was ten sites in `calibrate.rs`, six in `tests/sweep.rs`, and one in the
daemon's `mutating_verbs.rs`, against the two the symptom named.

**Measured.** Fixed: 50 runs, 0 failures (30 at eight spinners, 20 at sixty-four). Unfixed at
sixty-four: **20 of 20 fail**, carrying N60's exact signature —
`left: SettleTimeout   right: DeviceGone`.

**Eight spinners does not reproduce it**: 20 unfixed runs, 0 failures. That is N60's own step
3 repeated with the same result, and it is the half worth writing down — the reproduction
condition was never CPU contention, it was eight concurrent `cargo` *builds*. A repro recipe
that under-loads the machine reads as "cannot reproduce", which is how a real defect comes to
be recorded as a flake.

**The reading is 1, and E7 is why.** `settle.rs` is inside the mutation floor's scope and
`cargo-mutants` generates exactly one mutant for the impl — `now_ms` replaced by
`Default::default()`. A frozen clock reading `0` would have *been* that mutant, identically: a
new survivor with no argument, on the one file whose credibility N60 is about. E7 already
recorded that a clock stuck at zero left the whole workspace green. The mutant was
hand-applied and watched dying (`left: 0, right: 1`).

**A caveat N60 did not have to think about:** `FrozenClock` pairs with a *frame-counted*
settle. `SettleSpec::SkipFrames` converges without consulting the clock; `SettleFor`
converges by elapsed time and on a frozen clock would run to `MAX_SETTLE_ROUNDS` instead.
Every site changed here uses `SkipFrames`, and the doc beside the type says so.

**What it does not cover.** `daemon::server`'s `Inner` builds its own `MonotonicClock` and
`calibrate_sweep` builds another inside the actor closure, so a daemon test that takes a real
photo still runs its settle on a clock no test can reach — 5 runs at sixty-four spinners, 0
failures, so it is *exposed* rather than observed. Making it injectable needs a `Send + Sync`
clock, which `SteppedClock` cannot be and `FrozenClock` already is; that is the shape of the
next repair, and it is not blocked on N45 either.

**Retires when:** nothing. N60 keeps its last paragraph, as it said it would.

---

## N68 — The floor had one word for three things, and the third was a run whose input moved while it was reading it

**Doc:** AGENTS rule 1 (a discovered defect class lands with its gate), rule 2 (every gate
predicate proves both directions), rule 3 ("never silence"), docs/9's mutation-floor row,
and notes **N52** (the floor's verdict moved with `nproc`), **N60** (what a floor that cries
wolf costs), **N66** (the verdict moved with the free space on `/tmp`) and **N25** (what an
"equivalent" acceptance claims). Found at the G4 boundary, running `just gate-g4`.

**Believed:** that `scripts/mutants.sh` reports two things — the floor is green, or the
floor has a finding. N66 had already recorded that a resource shortfall was being spelled as
the second one, and left the repair open on the record.

**True:** it reports **three** things and had one word for all of them. The third is not a
resource fact and it is not an interruption: it is a run **whose input moved underneath it**.
The floor reads the working tree for the better part of an hour, one mutant at a time, and
its verdict is only ever about the tree it read.

**What was measured, 2026-08-11.** A `just gate-g4` started at **08:13:22** and finished at
**09:18:54** (its own `target/mutants.out/lock.json` and `outcomes.json` mtimes). At about
**09:19** — while it was still running — an agent edited `crates/engine/tests/sweep.rs`,
`crates/engine/src/calibrate.rs` and `crates/engine/src/settle.rs`: one inside the floor's
scope, two feeding the suite that judges every mutant. The run then said:

    mutants: FAIL — 3 recorded acceptance(s) no longer survive; the mutant became killable
    mutants:   crates/engine/src/sweep.rs: replace && with || in strided
    mutants:   crates/engine/src/sweep.rs: replace < with <= in strided
    mutants:   crates/engine/src/sweep.rs: replace > with >= in strided
    mutants: 525 generated — 435 caught, 8 missed, 0 timed out, 82 unviable

All three are **N25 "equivalent" acceptances** (`scripts/mutants-accepted.txt:30-32`): the
argument on each line is that the mutant is the *same program* given the callers'
preconditions — `limit` is never 0, the values reaching the comparison are distinct, the
widened stride already bounds the count. **A test cannot distinguish identical programs.**
That is the proof pattern N60 used to settle its own false positive (five measurements, the
last of which showed the Laplacian response sums to exactly zero, so `0/n`, `0*n` and `0%n`
are one number). So this verdict is not "true" and it is not "false" — it is **void**, and
nothing in the tooling could say so. `just gate-g4` printed `FAIL`, exit 1, the same as a
real missing test.

Two corroborations of the reading, both from the run's own recorded output, which is still
on disk:

- its `missed.txt` holds 8 rows; the register holds 11; the three it lacks are **exactly**
  the three sweep.rs lines above, and nothing survived that the register does not name. A
  run in which only the equivalences "became killable" is a run in which something other
  than the code changed;
- the commit the tree now sits at was made at **09:45:59**, after the run ended. The tree
  that produced this verdict is therefore not any committed tree, which is why the state
  recorded below is HEAD **and** `git status --porcelain` rather than either alone.

**The class, now at three.** N52's verdict moved with **time** (`nproc`, through the test
timeout: same tree, FAIL at 8 jobs and PASS at 4). N66's moved with **space** (a 16 GiB
`tmpfs`, `Disk quota exceeded` fifteen minutes in, exit 137, printed FAIL). This one moved
with a **moving input**. Three dimensions is not three accidents: *a floor that derives
anything from the machine it runs on can spell the machine's condition as a statement about
the code.* And N60 prices it — "a gate that cries wolf does not get believed, it gets re-run
at `-j1` until it agrees, and the run after that is the one where a real survivor is waved
through".

**Changed.**

1. **Three outcomes, three exit codes.** Green is 0. A finding — an unaccepted survivor, or
   an acceptance that stopped surviving — is 1, worded exactly as before, because that
   outcome was never the ambiguous one. **No verdict is `$GATE_NO_VERDICT` = 75**, which is
   `EX_TEMPFAIL` from `sysexits.h` ("temporary failure; the user is invited to retry") rather
   than an invented number; `scripts/gates/lib.sh` carries the argument and the rejected
   alternatives (0 would make an unproven criterion read as proven — rule 3's "skip that
   reads as pass" wearing an exit code; 1 is the finding; 2 is already `phase.sh`'s usage
   error and cargo-mutants' "some survived"). It covers the pre-flight space refusal, a
   killed or interrupted cargo-mutants, a red baseline, a result set whose rows and summary
   disagree, and a tree that moved. A missing scope file, an empty scope and a run that
   generated zero mutants stay **findings**: those are statements about the tree, and a
   floor that has been quietly disarmed is what this job is for.
2. **`phase.sh` renders and propagates the distinction**, because a fix that stopped at the
   floor would have moved the ambiguity out one layer rather than removed it — `just
   gate-g4`'s summary line is what anybody actually reads. A `command` row exiting 75 is
   reported as `NO VERDICT`, counted separately, and the gate exits 75 when that is all that
   happened. A finding outranks it when both occur, and the unanswered criteria are still
   named on the line above.
3. **The floor records the tree before the run and compares it after**, `git rev-parse HEAD`
   plus `git status --porcelain` (which covers untracked files). Recorded and compared rather
   than asserted clean, for `selftest.sh`'s reason: running the floor on a dirty tree is
   ordinary, and "you had uncommitted work" is not a finding — what is one is that the tree
   is not the one the run started on. The message lists the changed lines, so "which files"
   is answered where it is asked. A tree git cannot describe skips the check, named and
   counted.
4. **That logic has one home.** `scripts/gates/lib.sh` gained `gate_tree_watchable`,
   `gate_tree_state` and `gate_tree_changes`; `selftest.sh`'s own tree-watch (added earlier
   the same day, for the arm that wrote a `.seeded` file into the checkout) now calls them,
   and gained HEAD-awareness for free. `scripts/mutants.sh` **now sources `lib.sh`**, which
   it did not before — deliberately, and for two functions and one constant, keeping its own
   `mutants:` reporting vocabulary. The rejected alternative was two copies of an
   eight-line law, which AGENTS.md names a defect; they would have drifted immediately,
   because the floor needs the commit and the selftest did not.
5. **The verdict logic gained a documented seam, and a gate predicate that drives it both
   ways.** `scripts/mutants.sh` is a `g4` criterion *command*, not a predicate under
   `scripts/gates/`, so `selftest.sh` never reached it and it had no case file — which is
   why this defect survived three encounters and is the part of this entry that generalises.
   cargo-mutants writes its result as a directory of text files (`caught.txt`, `missed.txt`,
   `timeout.txt`, `unviable.txt`, one mutant per line, beside `outcomes.json`), so a recorded
   run *is* a fixture: `WCH_MUTANTS_CLASSIFY=<dir>` makes the shipped script classify one and
   exit, building nothing. A fixture may carry its own register (`accepted.txt`) and the tree
   state its run started from (`tree-before.txt`) — the only way to exercise "the input
   moved" without editing somebody's checkout, which no gate here may do.
   `scripts/gates/mutation-verdict.sh` + its case file drive **that mode of the real script**
   (rubric rule 6, paid for by N10): what is proved is what runs, and the mutants are the
   only recorded part. Its fixtures are *derived* from `scripts/mutants-accepted.txt`, so if
   its reading of the register ever drifts from the floor's, the clean fixture stops
   classifying clean and the predicate goes red.

**Measured after the change.** Six recorded result sets classified in about a second: clean →
0, unaccepted survivor → 1, stale acceptance → 1, rows-and-summary-disagree → 75, no result
set → 75, moved tree → 75 (naming the file). Three phase-gate runs: a finding row → exit 1
saying FAIL and not `NO VERDICT`, a no-verdict row → exit 75 saying `NO VERDICT` and not
FAIL, one of each → exit 1 saying both. `selftest.sh` goes from 20 predicates / 32 pass arms
/ 143 fail arms to **21 / 34 / 150**, and every one of the seven new failing arms was watched
red with the reason it was written for — including the two that are dated defects replayed
(`truncated=finding` is N66, `moved=finding` is what the floor did until this commit) and the
two that guard the new outcome from becoming a hiding place (a real survivor answered as
"no verdict", and a floor that refuses a tree nobody touched).

**The cross-check was validated against real runs rather than reasoned about**, because a
truncation detector that cries wolf on a 40-minute job would be worse than the defect it
replaces. In today's recorded output `total_mutants` 525 = 435 + 8 + 0 + 82 and each file's
line count equals its census entry exactly; E10's two runs give 515 = 431 + 11 + 73 + 0. Two
independent runs, one of them the very run this entry is about — and that one was **replayed
through the new classifier** (`WCH_MUTANTS_CLASSIFY=target/mutants.out`), which reproduced
its census and its three-line verdict exactly. So the seam is not only a fixture format: the
directory a real run leaves behind is a fixture, and the first thing it proved is that the
new refusals do not fire on real cargo-mutants output.

**One small thing worth writing down**, because it is the shape of the bug this whole entry
is about, in miniature: the predicate's first wording check forbade the *substring* `FAIL` in
a no-verdict report, and immediately went red on the floor's own trailer — which names its
exit code `EX_TEMPFAIL`. The check is now word-bounded. A test that reads a word as a
substring is the same error as a gate that reads a machine's condition as a verdict: not
looking at what was actually said.

**Does N66's clause retire?** N66's "Retires when" offers two disjuncts, and the first —
*"the floor grows that third outcome: a resource shortfall reported as a named, counted
refusal rather than as a verdict"* — is **satisfied, and that clause retires**. Both routes a
shortfall takes now end in a named refusal: the pre-flight `df` budget refuses with 75 before
building anything, and a cargo-mutants killed part way (exit 137, which is the shape N66's
own failure took) is 75 as well, counted by `phase.sh` as a criterion that could not answer.
**The rest of N66 does not retire and is not history.** Its measurement stands (`per_job_gib`
is a measurement, `reserve_gib` is a budget, and conflating them is how it happened), its
reserve arithmetic is still the thing that stops the shortfall occurring rather than merely
being reported honestly, and its closing warning — re-read the entry before raising
`per_job_gib` — is untouched. An entry retires on empirical disproof; nothing here disproves
N66. Only the repair it explicitly deferred has landed. **N52 and N60 are untouched**: N52's
pin is still the reason a timeout is not a verdict about the machine, and N60's last
paragraph — a second-direction failure means "investigate", never "delete the line" — is now
also printed by the floor itself, beside every stale-acceptance report, because that is where
it is needed.

**What this deliberately does not do.**

- **It does not teach `run-all.sh` the third outcome.** No gate predicate produces one today,
  and a branch with no observations behind it is a claim — which is N52's own lesson about
  the timeout arm that had never fired. When a predicate needs it, it lands with the arm that
  exercises it.
- **It does not run the real floor from a gate.** The 40-minute run does exactly what it did
  before; only what it *says* changed. What is proved in seconds is the classification, which
  is the half that was wrong.
- **A moved tree voids the whole run, not part of it.** The floor cannot say which mutants
  were judged against which tree, and pretending otherwise would be inventing precision.
- **Two samples are not a watch.** A file edited and reverted inside the run's hour leaves
  both recordings identical and is invisible here. Recording every mutant's tree state would
  cost an `fsync`-heavy walk per mutant across a 40-minute job; the cheap check catches the
  case that happened and the expensive one is not obviously worth it.
- **The other rungs keep their own vocabulary.** `miri.sh`, `rung-vivid.sh` and `smoke-hw.sh`
  report named, counted skips and none of them currently has a way to fail environmentally;
  `$GATE_NO_VERDICT` is there when one of them does.

**Retires when:** never by disproof — it is a defect with a date. It retires as history if
the floor ever stops reading the working tree during its run (a floor that mutated a frozen
export could not have this defect), at which point claim 3 above becomes unnecessary rather
than wrong.

---

## N69 — The event a sweep loses has not arrived yet, so the drain N65 measured could only ever count zero

**Doc:** notes **N65** (the client's runtime has one thread and its sweep has no drain, both
measured — this entry supersedes the *conclusion* of its §2 and leaves its measurement
standing), **N57** (the per-client calibration stream, and that dropping an event is a thing
it is allowed to do), **N60** ("a second-direction failure means investigate, never delete
the line"), **N25** (what an "equivalent" acceptance claims), **N67** (a repro recipe that
under-loads the machine reads as "cannot reproduce"), AGENTS' "bounded everything" and
`schema::limits`' rule that something reads every number. Found by `just mutants` on a
stable tree at `4a76b1d`.

### Provenance: the register worked, and what it caught was not what it said

The floor reported one recorded acceptance as no longer surviving —
`crates/engine/src/session.rs: replace > with >= in sampled_precision`. That acceptance is
an N25 equivalence and it is **correct**: `sampled_precision` sorts and dedups before it
takes windows, so every gap is at least 1, and the `i64::try_from(…).ok()` in the
`filter_map` drops an overflowing difference rather than yielding 0. `gap > 0` and
`gap >= 0` admit the same elements; identical programs cannot be told apart by a test. The
register was not touched. Re-measured by N60's own method rather than argued: the mutant
**hand-applied to this tree, with the fix below in place, passes the workspace suite —
935/935, four runs of five**.

What actually failed under that mutant was
`webcam-handler-client::wchc a_sweep_delivers_its_progress_…`, carrying all seven of the
sweep's events except the terminal one. That is N60's pattern for the third time
(N60 itself, N68, this), and N60's rule is the only reason the finding exists at all: the
failing test was investigated instead of the line being deleted, and **the flake was a
product defect**.

**And the fifth of those five runs is the pattern happening again while this entry was being
written**: with the mutant applied, one run failed
`webcam-handler-v4l2 sys::uevent::tests::a_quiet_socket_answers_at_its_deadline_rather_than_erroring_or_hanging`,
which asserts `started.elapsed() < 1s` on a real clock after a 20 ms deadline, on a machine
still carrying this entry's own load. Named rather than fixed, because it is one more
instance of the class N60 scheduled and N67 discharged for `engine`'s settle path — a real
clock in a test that has nothing to say about a duration — and it belongs to whoever takes
that population next.

### The defect

**Believed** (N65 §2): that `Remote::calibrate_sweep` needs no drain after its `select!`
loop, because on a current-thread runtime nothing can be pushed onto the subscription
between the two polls of one turn and `biased` polls the events first. Measured at the time
as "zero events on every one of five runs", and the drain was deleted on that measurement.

**True: the argument is right, the measurement is right, and the conclusion is wrong.** The
event a sweep loses is not one this client was holding — it is one that had not *arrived*.
`wch_calibrate_sweep`'s answer and its `SweepFinished` leave the daemon on two different
tasks: the method call, and the forward task `daemon::events` runs per subscription (the
sweep emits into a `broadcast` from the actor's own thread, and the task that reads it has to
be scheduled). They reach one connection's writer in whichever order that daemon's runtime
put them, and when the answer wins, the client's loop breaks on it and the terminal event
lands microseconds later on a socket nobody is reading any more.

It is the one event a bar cannot recover from a later one. `cli_core::Bar` prints the
sweep's closing line — `brightness: 3 sample(s) taken; nothing selected yet` — from
`is_terminal()` and from nothing else, and then `finish()` clears the bar. What a person sees
is a progress bar that stops one short of finishing and then vanishes without a word.

### Measured, and every row says what it is a measurement of

Eight-core workstation, tree at `4a76b1d`. "Four suites" is four concurrent
`cargo nextest run --locked --offline --workspace` in a loop — the mutation floor's shape,
which is four concurrent jobs each running the whole suite.

| What | Load | Result |
|---|---|---|
| the integration test, unfixed | quiet | 0 failures / 20 runs |
| " | 8 spinners | 0 / 20 |
| " | 64 spinners | 0 / 20 |
| " | **four suites** | **2 / 150** |
| " , fixed | four suites | **0 / 150** |
| 60 sweeps in one process, fresh daemon each | four suites | 0 / 60 |
| events left in *this client's own queue* when the loop breaks | four suites | **0**, every run of 30 |
| the same, after 512 `tokio::task::yield_now` turns | four suites | **0**, every run of 30 |
| the terminal event's arrival, relative to the answer | four suites | **+34 µs**, on the run that lost the race |

Four of those rows are the design, and each of them says something the others do not.

- **Spinners do not reproduce it**, at eight or at sixty-four, which is N67's finding
  repeated on a different defect: the condition is not CPU starvation, it is a machine with
  four other build-and-test jobs on it. A recipe that under-loads reads as "cannot
  reproduce".
- **Zero events in this client's queue, every run, is N65's number re-taken — and it is not
  evidence for N65's conclusion.** It is what N65's argument predicts and the argument is
  sound; what neither could see is that they were counting the one queue that is provably
  empty at that instant. A drain that reads only what is already buffered can be *proved* to
  find nothing here, which makes "we measured zero" and "there is nothing to collect" two
  different statements that happened to share a number.
- **512 `yield_now` turns find nothing either**, and that is a fact about tokio rather than
  about this daemon: the current-thread `block_on` loop re-polls a main future whose waker
  has already fired without ever parking on the I/O driver, so a drain that spins on yields
  never lets the connection's read task touch the socket. Recorded because "yield instead of
  waiting" is the obvious way to write a non-blocking drain and it cannot work on this
  runtime.
- **34 µs** is why a bound of a quarter second is a bound and not a wait.

### Changed

`Remote::calibrate_sweep`'s state machine moved into `sweep_and_watch`, a free function
generic over the answer's type, and grew a fourth step: **a bounded tail**, entered only when
the sweep's terminal event is actually outstanding.

- `drain_tail` reads until this sweep's terminal event, the end of the stream, or
  `schema::limits::CLIENT_SWEEP_DRAIN_MS` (250 ms), whichever comes first. `timeout_at`
  polls the event before the clock, so the bound is on *waiting* and never on reading: even a
  zero budget delivers what is already in hand.
- The guard is the half that keeps it off the ordinary path — a sweep whose terminal event
  beat its answer, which is nearly all of them, pays nothing. **The first shape of this fix
  had no guard and drained unconditionally, and it waited the full bound on every sweep.**
  What caught it was the integration test's own duration going 0.47 s → 0.77 s, which is
  a quarter of a second wearing a number; a fix whose cost is invisible in a suite that
  takes four seconds is a fix nobody would have questioned. It is now asserted by counting
  what the scripted stream was *asked*.
- The events source is a one-method trait (`ProgressSource`) with a scripted double, so the
  ordering that costs a bar its last line — an event that arrives after the answer — is a
  **unit test that fails every run** rather than a race that fails one in a hundred. Six
  buggy implementations were watched failing before this was called done: no tail (the code
  as it shipped), a tail with no guard, a tail on an ended stream, a tail that does not stop
  at its own terminal event, a tail that ignores the session filter, and a tail with no bound
  — the last of which does not fail with an assertion but with nextest's timeout, which is
  N65's objection reproduced exactly.

**Why this is not the thing N65 refused.** N65 refused *waiting for a terminal event*, on
the grounds that "waiting for a terminal event would hang forever whenever one was dropped,
and dropping is a thing `wch_subscribe_calibration` is explicitly allowed to do". Every word
of that stands and this tail cannot hang: it waits only when the daemon has not yet said its
last word, it stops the instant the word arrives, and it is bounded when the word was
dropped. The distinction is between a wait and a bound — and, separately, between the queue
N65 counted and the socket the event was still on.

**What this deliberately does not do.** It does not make the daemon order the two writes.
The honest fix on that side would be a sweep's answer that waits for its own fan-out, and a
call that waits on a subscriber is exactly what P4e-i's "nothing a client does can wedge the
daemon" forbids. It does not add a wire method to use as a flush barrier — a round trip
would cost more than the tail and would still be racing the same two tasks. And it does not
report a lost terminal event to the operator: `wch` cannot lose one (its sink is
synchronous), so a line on `wchc`'s stderr would be a divergence in the one place the parity
gate does not look, for a case whose whole rendering is "the bar stops one short", which is
what N57 already says a dropped event looks like.

### The recurrence worth naming, and it is now three

Three entries in the register now turn on **a measurement taken on an unloaded machine that
was read as proof**:

- **N65**: a counting drain, five runs, zero events — a quiet machine, and the mechanism it
  was looking for needs a loaded one to appear at all;
- **N67**: eight spinners, 20 runs, no failures, where sixty-four and concurrent `cargo`
  builds failed 20 of 20 — "a repro recipe that under-loads the machine reads as *cannot
  reproduce*, which is how a real defect gets recorded as a flake";
- **this one**: 0 of 20 quiet, 0 of 20 at eight spinners, 0 of 20 at sixty-four, 0 of 60 in a
  warm-daemon harness — and 2 of 150 under four concurrent workspace suites. Four of those
  five load levels would have closed the investigation.

The rule the three of them add up to: **for a defect that is an ordering, a green run is
evidence only if the run's load is stated, and the load has to be the one the defect was
seen under.** N67 named the condition for its own defect (concurrent builds, not spinners);
this one's is different again (four concurrent test suites), which is the point — the level
is a property of the race, not a house constant, so it gets measured per defect and written
down beside the rate. A measurement with no load stated is not evidence about a race, and
"zero events on five runs" is the shape that sentence takes when it goes wrong.

**The same race has a second loser, found at P5e and recorded as note N87.** This entry repaired
the client that breaks its loop on the *answer* and loses the event; `crates/daemon/tests/support/
ws.rs` was breaking its loop on the *event* and losing the answer, in four suites and over two
transports, and it cost a 180-second `TIMEOUT` in `just ci`. The measurement above is the one N87
cites rather than re-taking — what it adds is that a duplex connection has two readers and this
entry only ever looked at one of them.

**Retires when:** the daemon can no longer answer a sweep before its own terminal event has
left the process — at which point the tail has nothing to collect and both the drain and
`CLIENT_SWEEP_DRAIN_MS` retire together. Until then, re-read this entry before deleting the
tail on the strength of a quiet run: that is precisely how it came to be missing.

---

## PF:23 — The OBSBOT Tiny 3 stopped advertising 3840×2160 and 120 fps, and nothing on our side of the cable moved

**Measured** 2026-08-11 on kernel `7.0.0-29-generic` (x86_64), against the OBSBOT Tiny 3
(`3564:ff02`) at `3-1:1.0` — the same host, the same port, the same kernel and the same
firmware string as the capture it contradicts. Continues the docs/6 §1.2 registry; cite it
as `[PF:23]`.

**Design §1.2 gets no new bullet**, following the convention PF:17–22 already set: a new
finding lands here first, and the next v2 revision absorbs the accumulated entries together
or not at all; this is the seventh waiting. §1.2 *is* touched, though, in a way the
earlier six were not — **PF:9's bullet contains a sentence this entry falsifies**, and the
distinction between its rule and its example is the first thing to get right, so it is dealt
with below rather than left for a reader to notice.

### The measurement

`corpus/profiles/obsbot-tiny3.json` as committed on 2026-08-08 against a fresh
`wch --backend v4l2 profile capture` on 2026-08-11. The `CameraInfo` half is identical —
`CameraInfo::differing_fields` answers `[]` — and so is the control set: all 24 controls,
byte for byte, in the invariant section. **Only the format tree moved.**

| | committed 2026-08-08T16:19:01Z | fresh 2026-08-11T21:54:09Z |
|---|---|---|
| frame sizes | 7 | 6 |
| interval entries | 48 | 32 |
| controls | 24 | 24 |
| `CameraInfo` fields differing | — | none |
| file | 23,659 bytes | 21,364 bytes |

Size by size, in the tree's own nesting:

| format | size | rates, committed | rates, fresh |
|---|---|---|---|
| MJPG | 1920×1080 | 120, 60, 59.94, 50, 30, 29.97, 25, 24, 20, 15 | 60, 59.94, 50, 30, 29.97, 25, 24, 20, 15 |
| MJPG | **3840×2160** | 30, 29.97, 25, 24, 20, 15 | **not offered** |
| MJPG | 1280×720 | 120, 60, 59.94, 50, 30, 29.97, 25, 24, 20, 15 | 60, 59.94, 50, 30, 29.97, 25, 24, 20, 15 |
| MJPG | 1280×960 | 30, 29.97, 25, 24, 20, 15 | unchanged |
| MJPG | 1920×1440 | 30, 29.97, 25, 24, 20, 15 | unchanged |
| YUYV | 640×360 | 30, 25, 24, 20, 15 | 30 |
| YUYV | 640×480 | 30, 25, 24, 20, 15 | 30 |

Sixteen interval entries went: six with the 4K mode, one each from the two sizes that
offered 120 fps, and four each from the two uncompressed sizes. **The loss is at two
different depths of the same tree** — a whole `FrameSizeInfo` at 3840×2160, and intervals
inside `FrameSizeInfo`s that survived — and that shape matters for what can notice it, which
the last section is about. The 4K mode is the headline and the uncompressed collapse from
five rates to one is the part a summary loses; both are the device, and neither is a rounding
of the other.

### The device is the authority, and this evidence does not pass through our code

AGENTS rule 4 says the device is the only authority on itself, so the finding is not allowed
to rest on the tool that produced the disagreement. `lsusb -d 3564:ff02 -v` reads the
descriptors off the wire and agrees exactly:

```
FORMAT_MJPEG          bFormatIndex 1   bNumFrameDescriptors 4
  FRAME_MJPEG   1 1920x1080  bFrameIntervalType 9   fastest dwFrameInterval 166666  (60.0 fps)
  FRAME_MJPEG   2 1280x720   bFrameIntervalType 9   fastest dwFrameInterval 166666  (60.0 fps)
  FRAME_MJPEG   3 1280x960   bFrameIntervalType 6   fastest dwFrameInterval 333333  (30.0 fps)
  FRAME_MJPEG   4 1920x1440  bFrameIntervalType 6   fastest dwFrameInterval 333333  (30.0 fps)
FORMAT_UNCOMPRESSED   bFormatIndex 2   bNumFrameDescriptors 2   guid {32595559-…} (YUY2)
  FRAME_UNCOMPRESSED 1 640x360  bFrameIntervalType 1   dwFrameInterval 333333  (30.0 fps)
  FRAME_UNCOMPRESSED 2 640x480  bFrameIntervalType 1   dwFrameInterval 333333  (30.0 fps)
```

Six frame descriptors, largest 1920×1440, thirty-two `dwFrameInterval` entries, and the
shortest interval anywhere is 166666 (100 ns units) — 60 fps. There is no 3840×2160
descriptor and no 83333 to be found. **The kernel is not filtering a list the device sent;
the device is not sending it.** That distinction is the whole finding: an ENUM_FRAMESIZES
walk that came up short could be `uvcvideo` declining to expose something, and a
`bNumFrameDescriptors` of 4 could not.

### It is not a link-speed story, and the note must not become one

The obvious reading — "it fell back to USB 2 and dropped the modes it cannot feed" — is
wrong, and it is worth refusing explicitly because it is the reading a future reader will
reach for. Both captures record `bus_path: "3-1:1.0"`. `/sys/bus/usb/devices/3-1/speed` is
`480` and `/sys/bus/usb/devices/usb3` is a 480 Mbps, USB 2.00 bus, so the device is at high
speed *now* — and the kernel log says it was at high speed then too. Every enumeration of
this device on record, on both sides of the change, reads `new high-speed USB device` and
`bcdDevice= 5.10`. `bNumConfigurations` is 1, so there is no second configuration for it to
have switched into. **The camera advertised 3840×2160 over a 480 Mbps link on 2026-08-08 and
declines to advertise it over the same 480 Mbps link today.**

PF:9's bullet glosses the MJPG/YUYV gap as "(USB bandwidth)". That gloss is about why
*uncompressed* stops early and it is not disturbed here; what is disturbed is the idea that
the bus explains the *compressed* ceiling, because the bus did not move and the ceiling did.

### It changed at one of four re-enumerations, and the record cannot say which

The committed capture was taken at 2026-08-08 09:19:01 PDT, under the enumeration of
2026-08-08 03:04:36. The journal has every enumeration of this device since:

| enumeration | boot | speed / `bcdDevice` | relative to the captures |
|---|---|---|---|
| 2026-08-07 14:37:37 | −2 | high / 5.10 | before |
| 2026-08-07 16:41:54 | −2 | high / 5.10 | before |
| **2026-08-08 03:04:36** | −2 | high / 5.10 | **in force for the committed capture (4K present)** |
| 2026-08-08 11:16:50 | −2 | high / 5.10 | between |
| 2026-08-10 17:40:55 | −2 | high / 5.10 | between |
| 2026-08-10 21:08:59 | −1 | high / 5.10 | between (clean reboot) |
| **2026-08-11 12:57:10** | 0 | high / 5.10 | **in force for the fresh capture (4K absent)** |

Four re-enumerations sit between the two captures, and the kernel log cannot tell them apart:
same port, same speed, same device-release string, no configuration change. Nothing in the
record identifies which of the four the device came back different from.

The **power outage** is the last of them. Boot −2 ends with a recorded power-off sequence;
boot −1's journal has none at all — it stops mid-minute at 12:50:05 on 2026-08-11 and boot 0
begins 7 minutes later at 12:57:09, which is what an unplanned power loss looks like in a
journal. So the outage is corroborated as an event, and it is the salient one: it
power-cycled the camera rather than merely re-probing it, and a camera that lost its rail is
a camera whose firmware re-ran whatever decides what to advertise.

**That is an explanation, not a measurement, and it is not proved.** Three of the four
re-enumerations are equally available as the moment it changed, and nothing was captured
between 2026-08-08 and today that would narrow it. `bcdDevice` did not move, which rules out
a firmware version change to the extent that field tracks one — and the device could have
changed what it advertises without changing that field, so even the ruling-out is bounded.
Do not upgrade this paragraph on re-reading: what is known is that the device advertises
less, that it did so at some point in a window containing four re-enumerations, and that one
of them was a power loss.

### Why this is not N63's case, and why that entry does not govern here

N63 carries a section headed **"Why a re-capture is not the fix"**, and it is emphatic: a
re-capture "bakes *today's* arbitrary numbering into the corpus and produces a green run that
means nothing", and it "would teach the habit that corpus red means 're-capture until
green'". Both sentences are right. Neither applies to this finding, and a reader who takes
them as a general rule against re-capture will read the two entries as contradicting each
other. They do not, and the difference is not one of degree:

- **There, the corpus was not stale.** N63's own words: "The four documents describe the four
  cameras correctly, and did on both sides of the reload." The device had not changed; an
  assertion had over-reached, and the artifact at fault was the comparison. **Here, the
  corpus is stale in the strict sense** — it says the device offers 3840×2160, and the
  device's own frame descriptors say it does not. A document that describes the device
  wrongly is the one thing a corpus may not be.
- **There, a re-capture would have recorded an arbitrary number.** `/dev/videoN` is a probe
  counter; whatever a capture wrote down would be falsified by the next `uvcvideo` reload,
  which this project *performs* as part of E9. **Here a re-capture records the device's own
  answer** — six frame descriptors, thirty-two intervals — which is the same answer `lsusb`
  gets independently and which nothing in this project can perturb.
- **There, re-capturing would have been the fix for the wrong artifact.** Here it is the
  sanctioned one: AGENTS rule 4, "new device behavior lands as a profile in `corpus/` + a
  note, **the day it is seen**", and the §3.2 convention that profiles are immutable and a
  re-capture replaces wholesale. Rule 4 does not have an exception for behavior that is a
  loss.
- **The habit N63 warns about is intact.** "Corpus red means re-capture until green" is a
  habit about *not saying why*; the arm's own message says "neither is fixed by re-capturing
  without saying why", and the operative clause is the last four words. This entry is the
  why. What would have been the N63 failure is re-capturing on 2026-08-11 with a one-line
  commit message, and that is not what happened.

There is a small piece of evidence that the two entries fit together rather than merely
coexisting. The re-capture also re-recorded the node paths, at today's `/dev/video0`,
`/dev/video1` rather than the captured `/dev/video4`, `/dev/video5`. Under N63 that field is
no longer compared, so re-baking it can neither help nor harm — and the transcript shows it:
after this re-capture the OBSBOT prints the *unadorned* enumeration line while the two
Chicony cameras still print PF:22's renumbering annotation. Nothing about the kernel changed
between those three lines; one document was rewritten and two were not. That is what "inert
data" looks like from the outside, and it is why the objection N63 raised to re-capturing
cannot be raised against this one.

### What the corpus loses, and where it survives

The committed record of a 3840×2160 mode and of 120 fps at 1920×1080 and 1280×720 is **gone
from `corpus/profiles/`**, and it survives in exactly two places: the table at the top of
this entry, and git history at `9c8b46a:corpus/profiles/obsbot-tiny3.json`. The owner
accepted that trade on 2026-08-11 (below). It is a real loss — a test can load a profile and
cannot load a table — and the honest accounting is that the 4K/120 record is now prose,
which §3.2 says is the weaker form.

What the re-capture does **not** cost is any of the probe findings this document carries for
the registry walk in `crates/backends/fake/tests/corpus_replay.rs`, which was the live risk
of replacing a document wholesale. Checked, before and after:

- **PF:2** — `auto_exposure`'s sparse menu, indices {1, 3}: present in both.
- **PF:4** — `zoom_continuous` reading 245 against a declared `-100..=100`: present in both,
  and this profile is the corpus's **only** carrier of PF:4.
- **PF:5** — `power_line_frequency` defaulting to 3 against a declared `0..=2`: present in
  both, and again the only carrier.
- **PF:8** — a camera reporting no serial: present in both.
- **PF:9** — a compressed format reaching a larger size than an uncompressed one: present in
  both, because the predicate is the *rule* and not the number (see below).
- **PF:3** — live INACTIVE coupling: **newly** carried by this document. The 2026-08-08
  capture caught the camera with `white_balance_automatic` off; today's caught it on, so
  `blue_balance`, `red_balance` and `white_balance_temperature` all carry `0x10` in the state
  block. Two other profiles already carried PF:3, so this is a gain rather than a rescue.

The state block moved as the T3 split promises it may: `pan_absolute` 0 → −28800 and
`tilt_absolute` 0 → −75600 (the motors are where P3e's sweep left them), `red_balance`
147 → 143, `white_balance_automatic` 0 → 1. None of that is drift and none of it is
compared; it is here so nobody re-derives it from the diff.

**PF:9's example, and only its example, is retired.** The bullet reads "The OBSBOT offers
MJPG up to 3840×2160 while YUYV stops at 640×480 (USB bandwidth) — frame-size enumeration
must be nested under pixel format." The rule is untouched and is still asserted from a real
document rather than from that sentence: `corpus_replay.rs`'s PF:9 row is "a compressed
format reaching a larger size than an uncompressed one", which the OBSBOT satisfies today at
1920×1440 against 640×480, and which two other profiles satisfy independently. The two doc
comments that spelled the old numbers as present tense — `FormatInfo` in
`crates/schema/src/camera.rs` and `V4l2Camera::sizes_for` in
`crates/backends/v4l2/src/lib.rs` — now give both readings and cite both entries, which moved
one line each in `schemas/webcam-handler-schema.json` and `schemas/webcam-handler-openrpc.json`.
`crates/testkit/src/fixtures.rs` keeps 3840×2160: that document is a hand-built fixture
carrying the *shape* of each edge and is not a claim about an attached device, which is why
§3.2 keeps it out of `corpus/`.

### The ruling

**Re-capture, replacing wholesale.** Owner's decision, 2026-08-11, taken with the loss above
stated: `corpus/profiles/obsbot-tiny3.json` is replaced by
`wch --backend v4l2 profile capture`, not hand-edited, with provenance naming why
("victor@costan.us (re-capture: the device stopped advertising 3840x2160 and 120 fps,
PF:23)"). The diff is 11 insertions and 100 deletions.

### What now notices this, and what deliberately does not change

The class this finding names — **a device that silently stops offering a mode** — is now
known to be real, and a green corpus arm says nothing about it. Three things changed and one
thing was considered and refused.

**No new gate, and the argument is that the detector already exists and worked.** The class
is observable only against the device, so no predicate on a machine without one can see it;
`just ci` runs on a host with no camera by design. The sensor is
`hw_profile_capture_reproduces_the_committed_invariant_section`, it went red on 2026-08-11
for exactly the right reason, and its message said in advance that a red here is a finding
rather than a chore. A second sensor for the same signal would be two answers to one question
(design §2.10), and the corpus's green after a sanctioned re-capture is not a weakened claim
— **the corpus is the assertion**, and it now asserts something true where it previously
asserted something false. What the day exposed is not a missing detector but three gaps
around it:

1. **The comparison's constraint on the format tree was untested below the top level.**
   `invariant_matches` compares `formats` with `==`, and the only existing arm exercising that
   pushed a whole new `FormatInfo`. `a_mode_the_device_stopped_advertising_reads_as_drift` in
   `crates/schema/src/profile.rs` now pins both of today's depths: a size removed from a
   format that still exists, and an interval removed from a size that still exists — the
   second built so the size lists are *identical* on both sides, which is what makes it the
   arm a shallower comparison would pass. Two buggy implementations were watched failing at
   workspace scope (AGENTS rule 2): `formats` compared as a list of pixel formats, and — the
   over-correction in N63's sense — `formats` compared down to sizes with the intervals
   dropped. Each turned exactly one of 936 tests red, and it was this one, which is the
   measurement that says nothing else in the workspace was holding that level.
2. **The failure message named two hypotheses and today's cause was a third.** It said "the
   corpus is stale or the kernel changed behaviour". Neither happened: the device changed what
   it advertises, and no amount of reading our output distinguishes that from the other two.
   The message now names three and points at `lsusb -v` on the frame descriptors, because that
   is the step actually performed to reach this ruling and it is the only one whose answer does
   not come through this code.
3. **A green run left no record of what the device offered.** Every run before today printed
   `obsbot-tiny3: a fresh capture reproduces the committed invariant section` — true, and
   useless as a "before" column, which is why the shrink had to be reconstructed out of
   committed JSON by hand. The arm now prints the shape it matched, the way the enumeration
   arm prints the node paths it no longer asserts:

   ```
   obsbot-tiny3: a fresh capture reproduces the committed invariant section; it offers MJPG [1920x1080 9 rate(s) to 60fps, 1280x720 9 rate(s) to 60fps, 1280x960 6 rate(s) to 30fps, 1920x1440 6 rate(s) to 30fps] YUYV [640x360 1 rate(s) to 30fps, 640x480 1 rate(s) to 30fps], 24 control(s)
   ```

And one gap that is not about the format tree at all, found while answering "did anything else
drift". Both corpus arms walk *attached cameras* and ask the corpus about each, so a committed
profile whose device is not on the bus is never visited and was never mentioned: today three of
four profiles were compared and `dell-u3224kb` — whose monitor is off the bus — went unnamed in
a transcript that read as full coverage. That is AGENTS rule 3's "named, counted skip" missing
in one direction while present in the other, and it matters *because* of this finding: the
device that changed did so while nobody was looking at it, so "which profiles did this run
actually check" is the number that bounds the claim. Both arms now end with

```
SKIP (partial): 1 committed profile(s) match no camera attached to this host, so this arm did not check them against a device: dell-u3224kb
```

### What would make the next one provable, and is not built here

Nothing in a `DeviceProfile` could have decided between the four re-enumerations, because the
document records nothing about the device that a re-enumeration could change. `bcdDevice`, the
negotiated link speed, `bNumConfigurations` and the boot the capture was taken under are all
absent, and all four had to be read out of `lsusb` and the journal by hand for this entry. A
provenance block carrying them would have turned "the cause is unproved" into a narrower
statement, and possibly into a measurement. It is **not** done here: `ProfileProvenance` is a
wire type, so a new field moves `schemas/`, the OpenRPC document and every committed profile at
once, and that is a schema change with an owner's decision in front of it rather than a
tail-end of a corpus fix. Recorded so the next person with this problem does not re-derive the
gap.

**Retires when:** the OBSBOT advertises 3840×2160 or 120 fps again on a later enumeration —
which would itself be the next finding, and a bigger one, because a capability that comes back
is a device with a mode this project cannot predict and a corpus that cannot be trusted to be
current between two captures. Also retires, differently, if a firmware release is identified
that changed the descriptor set, at which point the cause stops being unproved and this becomes
a dated record of one device's two firmwares rather than an open question.

**Adjacent:** PF:22 and note N63 (the other 2026-08-11 corpus finding, and the entry whose
"why a re-capture is not the fix" this one has to be read against); PF:9, whose example this
retires and whose rule it leaves standing; PF:3, PF:4 and PF:5, whose only or partial carrier
this document is; AGENTS rule 4, which is the whole authority for the ruling.

### Amendment, 2026-08-13: the modes came back, exactly, and this entry's retirement condition is met

Seen while landing \[PF:28\], by the arm this entry exists for going red the other way round.
`hw_profile_capture_reproduces_the_committed_invariant_section` now fails because a fresh capture
offers **more** than the corpus: 3840×2160 is back, 120 fps is back at 1920×1080 and 1280×720, and
the two uncompressed sizes are back to five rates each.

Read off the wire with this entry's own instrument, so the claim does not pass through our code —
`lsusb -d 3564:ff02 -v`, same port, same 480 Mbps link, `bcdDevice= 5.10` unchanged:

```
FORMAT_MJPEG          bNumFrameDescriptors 5      (was 4)
  1920x1080  bFrameIntervalType 10  dwFrameInterval(0)  83333   (120.0 fps)
  3840x2160  bFrameIntervalType  6                              (was absent)
  1280x720   bFrameIntervalType 10  dwFrameInterval(0)  83333   (120.0 fps)
  1280x960   bFrameIntervalType  6
  1920x1440  bFrameIntervalType  6
FORMAT_UNCOMPRESSED   bNumFrameDescriptors 2
  640x360    bFrameIntervalType  5                              (was 1)
  640x480    bFrameIntervalType  5                              (was 1)
```

**Seven frame sizes and forty-eight interval entries — the 2026-08-08 numbers exactly.** Not "some
modes returned": the descriptor set this entry recorded as lost is back whole, and the corpus that
was re-captured to record the loss is now stale in the opposite direction.

**And there is an event in the journal this time**, which is what the original could not produce.
The device was gone from the bus for fourteen minutes overnight:

```
Aug 13 02:47:35 kernel: usb 3-1: USB disconnect, device number 2
Aug 13 03:01:42 kernel: usb 3-1: new high-speed USB device number 29 using xhci_hcd
Aug 13 03:01:43 kernel: usb 3-1: New USB device found, idVendor=3564, idProduct=ff02, bcdDevice= 5.10
```

A `uvcvideo` cycle produces no such line. The same journal holds twenty-six
`registered new interface driver uvcvideo` lines for that day — this project's own probes reloading
the driver over and over — and exactly one enumeration of this device, the one above. A driver
cycle does not re-enumerate a camera; a replug does. So this is a **replug or a power cycle**, and
it lands on the same side as the power outage this entry named
as its favoured explanation: the capability set is decided when the camera's rail comes up, and it
has now been seen to go both ways. That is a stronger reading than the entry could support on
2026-08-11 and it is still a reading — nothing here identifies *what* the firmware decides on, and
"it was unplugged and came back different" is a correlation with one instance in each direction.

**What this costs, and what is deliberately not done here.** `just smoke-hw` has one red — this
arm, on this device, for this reason. It is a device finding rather than a defect, and the session
that met it was landing PF:28's fix, so the corpus is left as committed: a re-capture replaces a
document wholesale (§3.2) and this entry's own long argument about when that is right is the thing
a re-capture would have to be weighed against, in a session that has read it. What the next session
owes is a decision, not a command: re-capture and record which of the device's two descriptor sets
the corpus now claims to describe, or leave the corpus at the 2026-08-11 state and carry the red as
a known device finding. **This entry is retired as an open question** — its retirement clause names
this exact observation, and the answer to "does the capability come back" is now *yes* — and what
replaces it is the sharper question it predicted: a device whose advertised capability set is not
stable across power cycles, which no corpus can be current for by construction.

---

## E13 — G4 evidence: the daemon at the real cameras — a photo and a calibrate sweep over the socket, 2026-08-11

E9, E11 and E12 are the shape this follows: a dated run against something this project does
not control, recorded once and not amended. This is the R3 half of docs/7's **P4g** — "the
daemon against the real cameras: a photo over UDS, a calibrate sweep over UDS with live
`wchc` progress" — landed as commit **`9c8b46a`**, whose two `#[ignore]`d arms live in
`crates/client/tests/hardware.rs`. The run transcribed below was taken at 14:11 local on the
working tree that became that commit (`06489e3` plus the new files); the final confirmation
at the bottom is the tree as committed.

**The gap it fills was total.** All sixteen `hw_` arms in the tree before this one lived in
the V4L2 backend and drove a camera **in process**. Nothing anywhere drove `wchd`. E12,
taken on this same desk five hours earlier, compared `wch` and `wchc` against these cameras
and named the hole in its own "what it does not establish": `photo` is one of the four
`device`-bucket exemptions with "no real-hardware comparison anywhere", and "nothing about
the `wchc` sweep's progress rendering". Two arms, one for each half.

**The hotplug cycle is not re-run here, and that is P4g's instruction rather than an
omission**: "a transcript written twice is two transcripts nobody can tell apart". G4's
hotplug evidence is **E9** — the `uvcvideo` cycle through the real `watch`, the
eight-removals-then-eight-arrivals reading and why it is eight and not ten, the accounting
identity, the interlock honored against a real other-process holder that made the arm
decline rather than force, and the two hand-written mutants that arm was watched failing.
Cited, not repeated.

### The fixture, and it is narrower than E12's

**Host:** the P4d/P4e/P4f workstation, kernel `7.0.0-29-generic` (x86_64), `rustc 1.97.1`,
`cargo-nextest 0.9.138`, eight cores. **Attached: three cameras on six nodes** —

| camera | `card` | nodes | link |
|---|---|---|---|
| `cam:obsbot-tiny-3-obsbot-tiny-3-st` | OBSBOT Tiny 3: OBSBOT Tiny 3 St | `/dev/video0,1` | 480 Mbps, USB 2.00 |
| `cam:integrated-camera-integrated-c` | Integrated Camera: Integrated C | `/dev/video2,3` | 480 Mbps, USB 2.01 |
| `cam:integrated-camera-integrated-i` | Integrated Camera: Integrated I | `/dev/video4,5` | 480 Mbps, USB 2.01 |

**The Dell U3224KB/A that E12 enumerated is not attached** — its monitor is off the bus —
so this run's fixture is *three* cameras where E9, E11, E12 and PF:22 all had **four on ten
nodes**. That is stated here rather than left for a reader to assume parity, because a
run's fixture is part of its evidence and the difference is load-bearing twice over: the
4K monitor webcam is the one camera in this house whose formats nobody here has driven
through a socket, and PF:23's corpus arms had to grow a counted skip on this very day
precisely because a committed profile whose device is off the bus is a profile nobody
visited. This run is the same shape from the other side.

**And the format tree this run negotiated against is not the one E12 saw.** PF:23 landed
the same day: the OBSBOT stopped advertising 3840×2160 and 120 fps, with nothing on our
side of the cable moved, and `corpus/profiles/obsbot-tiny3.json` was re-captured at
**`1a51c81`** — *after* this run. So at 14:11 the device had already shrunk while the
committed document still claimed 4K, and the MJPG 1920×1080 the photo arm negotiated is the
top of the tree the camera actually had. Nothing in either arm asserts a mode by name, which
is why the shrink cost this suite nothing; a reader comparing this fixture with E12's five
hours earlier should still know the device moved between them.

**The daemon is real and the client is the shipped one.** Both arms spawn
`wchd --backend v4l2` into a private pair of XDG directories, discover cameras through the
tool's own `list` — no `/dev/videoN` is named anywhere, which is PF:22 and note N63 obeyed
— and stop the daemon with a real `SIGTERM` through `rustix` (not `Child::kill`'s SIGKILL),
asserting its status, so a daemon that died mid-run cannot leave a green test behind. Every
capture goes to the fixture's temporary `$XDG_STATE_HOME` and is deleted with it: **no frame
reaches the tree** (AGENTS "Hardware and privacy").

The two arms observe differently and the choice is not a preference. The **photo** arm
drives the shipped `wchc` **binary** as a subprocess, because `-o` resolution, the exit code
and the `--json` document are process facts. The **sweep** arm drives
`client::remote::Remote` with a recording `cli_core::SweepWatcher`, because the shipped
watcher is an indicatif bar that draws nothing when standard error is not a terminal — "the
events arrived" is a question only answerable inside the process, and it is the seam
`crates/client/src/lib.rs`'s header has said since P4f that the library exists for.

### A photo over UDS — three cameras, three photos

```
cam:obsbot-tiny-3-obsbot-tiny-3-st: MJPG 1920x1080 → 253417 bytes to a file, 240135 bytes through base64, settled 10 frame(s), the camera's own bytes [E6]
cam:integrated-camera-integrated-c: MJPG 1280x720 → 221486 bytes to a file, 257262 bytes through base64, settled 10 frame(s), the camera's own bytes [E6]
cam:integrated-camera-integrated-i: GREY 640x360 → 23119 bytes to a file, 20113 bytes through base64, settled 10 frame(s), re-encoded
wchd stopped on SIGTERM with exit status: 0 after 3 photo(s)
```

| camera | negotiated | size | to file | base64 | rendering |
|---|---|---|---|---|---|
| OBSBOT Tiny 3 | MJPG, 1/30 s | 1920×1080 | 253 417 B | 240 135 B | verbatim \[E6\] |
| Chicony RGB | MJPG, 1/30 s | 1280×720 | 221 486 B | 257 262 B | verbatim \[E6\] |
| Chicony IR | GREY, 1/15 s | 640×360 | 23 119 B | 20 113 B | `converted_and_encoded` |

Each photo was written by the **daemon's** process to a path the **client** resolved (D10),
decoded independently by `image` at the size the report claims, and counted against the
report's own `byte_count` through `api::PhotoResponse::bytes_match_the_delivery` — the
predicate whose client-side consumer note N34 had been owed since P4a. All three settled ten
frames, which is `limits::DEFAULT_SETTLE_SKIP_FRAMES` and not a number the test chose.

**The two MJPG cameras are the first time E6's byte fidelity has crossed a socket.** The IR
sensor is a GREY source and is the arm's non-compressed case, where `is_verbatim` must be
*false* and the pass-through assertion is deliberately not made — a rule with only obedient
instances is a rule nobody has tested.

The `-o` half and the base64 half are **two separate captures of the same scene**, which is
why their byte counts differ and why they differ again on the confirmation run
(261 177 / 249 212 B on the OBSBOT). A JPEG's size is a property of the frame, and no two
frames off a live sensor are the same size. Anything asserting equality there would be
asserting that the world holds still.

### A calibrate sweep over UDS, with its progress live

| camera | control | range | stride | samples | events | wall |
|---|---|---|---|---|---|---|
| OBSBOT Tiny 3 | `brightness` | `0..=100` | 25 | 5 | 12 | 2.84 s |
| Chicony RGB | `brightness` | `0..=255` | 63 | 5 | 12 | 3.15 s |

Twelve events is `1 SweepStarted + 5 ValueSet + 5 SampleTaken + 1 SweepFinished`, every one
of them delivered to the watcher **inside the `calibrate_sweep` call** and every one carrying
this session's id. The `Uniform` stride is derived from each device's own declared range,
which is why it is 25 on one camera and 63 on the other.

**The live-progress claim is the arrival times, not the count.** On the OBSBOT the five
samples reached the watcher at 0.52 / 1.02 / 1.61 / 2.18 / 2.84 s — spread across the sweep
rather than bunched at its end, which is the only difference between a progress stream and a
report delivered late. Each event crosses the camera actor's thread, a `broadcast`, the
per-client subscription note **N57** describes, a serialization and a socket while the call
it belongs to is still outstanding.

Every sample's `{requested, applied}` agreed on both cameras: no clamping, no step
alignment, so \[PF:6\] had nothing to report here — which is a fact about `brightness` on
these two cameras and not a weakening of the rule.

**Restoration is asserted, not reported.** 22 non-volatile controls on the OBSBOT and 15 on
the Chicony RGB were re-read over the same socket after the session and compared against a
snapshot taken over that socket before it opened; every one matched, and `brightness` went
back to `Int(50)` and `Int(128)`. Why a *second* read rather than the report: see M5 below.

### The named, counted skip — availability is not capability

```
SKIP (partial): cam:integrated-camera-integrated-i exposes no sweepable brightness-class
control among its 3, so this arm declines it — which is a fact about this sensor's control
set and not about the socket
```

The Chicony IR sensor enumerates three controls (`user_controls`,
`region_of_interest_rectangle`, `region_of_interest_auto_ctrls`) and none is
brightness-class. It declines on **every** run of this suite, naming the camera and the size
of the set that was examined, and `scripts/smoke-hw.sh` greps and counts that line. A camera
without the control is not a failure; what it must not be is silent (AGENTS rule 3, rule 7).

### The arms were watched failing, six times over

An `#[ignore]`d evidence arm is exactly where a decorative assertion hides. Each mutant below
was applied to the tree, built, run against the whole non-ignored workspace suite **and**
against the two arms on the real cameras, then reverted. Line numbers are as printed; a later
comment pass moved them, and each assertion is named by its message instead.

| mutant | workspace | the arm |
|---|---|---|
| **M1** `engine::photo`: the report transposes its own dimensions | 1 failure (`daemon::mutating_verbs`) | "the photo's size disagrees with its own report" |
| **M2** `imaging::photo`: the `verbatim` short-circuit disabled (E6 broken) | 2 failures (`cli::photo`, over the fake) | "an MJPG source must pass through, not re-encode \[E6\]: `DecodedAndEncoded { source: PixelFormat([77, 74, 80, 71]), target: Jpeg }`" |
| **M3** `engine::photo`: the sink writes two bytes fewer than the report counts | 1 failure (`cli::photo`) | "the report's byte count is not the file's" |
| **M4** `client::remote`: `watch.event(&event)` deleted from `sweep_and_watch`'s select | 1 failure (`remote::tests`) | "no progress reached the watcher, so the subscription and the call did not overlap" |
| **M5** `engine::snapshot`: `restore_one`'s read-before-write short-circuit made unconditional | 1 failure (`daemon::calibrate_verbs`) | "brightness is `Some(Int(100))` and the session found it at `Int(50)`" |
| **M7** the three brightness-class names replaced with names no camera has | — | **PASS in 0.224 s and four counted `SKIP` lines** |

Two of those rows carry more than a tick.

**M5 is why the sweep arm re-reads the device instead of believing the answer.**
`RestoreReport::is_complete()` was **still true** under a restore that reported
`AlreadyCorrect` for every control and wrote nothing. The report passed and the camera had
not moved back. A restoration assertion built on the report alone would have been green with
the OBSBOT sitting at brightness 100.

**M7 is the skip path exercised rather than described.** Forcing *every* camera to decline
gives a test that passes in a fifth of a second, and the run still says what it did not
claim — per camera, with the size of the control set it looked at — through the same
accounting `scripts/smoke-hw.sh` applies to the v4l2 rung:

```
smoke-hw: 4 claim(s) declined by tests that ran — each named above
smoke-hw:   SKIP (partial): cam:obsbot-tiny-3-obsbot-tiny-3-st … among its 24 …
smoke-hw:   SKIP (partial): cam:integrated-camera-integrated-c … among its 18 …
smoke-hw:   SKIP (partial): cam:integrated-camera-integrated-i … among its 3 …
smoke-hw:   SKIP: no attached camera offered a capture node and a sweepable
            brightness-class control, so no sweep ran over the socket
```

One mutant was **not** run and the reason is recorded rather than papered over:
"subscribe after the call rather than before it" is not expressible as a small mutation of
`Remote::calibrate_sweep`, because the call is a future nothing polls until
`sweep_and_watch`'s select loop, so the subscribe necessarily precedes the request going
out. That is note **N65**'s standing debt — the ordering is argued, not proved — and it is
argued *more* strongly here, since a sweep on real hardware opens a camera and settles a
sensor before its first event.

### N69 on real hardware, and this is the headline

**M6 — the bounded tail deleted**, i.e. N69's fix reverted:
`if watching && !ended { drain_tail(…) }` disabled in `crates/client/src/remote.rs`.

N69 fixed a lost terminal event using a scripted double and measured the integration test at
**2 failures in 150** against the fake, under four concurrent workspace suites. The question
that entry could not answer is whether a *real* daemon driving a *real* camera loses it often
enough for an integration arm to see. Measured under N69's own load condition, which that
entry requires be stated rather than assumed — four concurrent
`cargo nextest run --locked --offline --workspace` loops on this eight-core workstation:

| what | load | result |
|---|---|---|
| the sweep arm, tail deleted (M6) | four suites | **2 failures / 10 runs** |
| the sweep arm, tail present (as shipped) | four suites | 0 / 10 |
| the sweep arm, tail deleted | quiet | 0 / 3 |
| the scripted-double unit test, tail deleted | any | fails immediately, every run |

**2 in 10 against the fake's 2 in 150 — and the loss is *wider*.** Both failing runs lost the
final `SampleTaken` as well as `SweepFinished`; the recorded event list ends at
`ValueSet { index: 5, total: 5 }`. One failing run's tail, verbatim, times relative to the
call:

```
( 5.468 ms) SweepStarted { control: brightness, plan: Uniform { step: 25 }, total: 5 }
(31.524 ms) ValueSet    { index: 1, requested: 0,   applied: 0 }
(543.4 ms)  SampleTaken { index: 1, requested: 0,   applied: 0,   mean_luma 0.0589 }
(560.7 ms)  ValueSet    { index: 2, requested: 25,  applied: 25 }
(1.063 s)   SampleTaken { index: 2, requested: 25,  applied: 25,  mean_luma 0.1021 }
(1.088 s)   ValueSet    { index: 3, requested: 50,  applied: 50 }
(1.591 s)   SampleTaken { index: 3, requested: 50,  applied: 50,  mean_luma 0.1514 }
(1.729 s)   ValueSet    { index: 4, requested: 75,  applied: 75 }
(2.180 s)   SampleTaken { index: 4, requested: 75,  applied: 75,  mean_luma 0.3175 }
(2.289 s)   ValueSet    { index: 5, requested: 100, applied: 100 }
-- and nothing else.
```

**N69 predicted exactly this in writing and could not observe it**: "the last sample's event
is one hop further from the answer than the terminal one — the daemon commits the session
durably in between — so it is the same defect with a wider window rather than a property that
was safe." That sentence is now a measurement.

**Why real hardware makes it worse rather than better**, which is the counter-intuitive half
and the reason this entry exists. Every sample here is a settle and a capture — about 500 ms
— so the daemon's forward task has been **idle for half a second** when the last sample's two
events and the call's answer are all queued within a few hundred microseconds of each other.
The window the answer has to win is unchanged (N69 measured the terminal event arriving
**+34 µs** after the answer on the run that lost the race); what changed is the number of
events sitting behind it, which is **two rather than one**. A slower producer does not make
this race rarer, it makes the pile-up at the end larger.

What a person would have seen is what N69 says a person sees, now on a real camera: a bar
that stops at 4 of 5 and vanishes without its closing line.

The assertion that fired is `"the sweep's terminal event never reached the watcher [N69]"`,
`crates/client/tests/hardware.rs:668`.

### The whole `hw_` suite, and the one red that is not this run's

```
$ cargo nextest run --locked --offline --workspace --run-ignored all --no-tests=fail \
    -E 'test(/(^|::)hw_/)'
  Starting 18 tests across 40 binaries (943 tests skipped)
   Summary [  28.147s] 17/18 tests run: 16 passed, 1 failed, 943 skipped
```

Sixteen arms before this sub-milestone, eighteen now, 28.1 s on one thread — the two new arms
joined the `exclusive-device` nextest group and `just smoke-hw` **by being named `hw_*`**, and
neither `.config/nextest.toml` nor `scripts/smoke-hw.sh` was edited.

**Read again 2026-08-11, and the heading above overstates what happened:** this was not the
whole suite. The summary line says `17/18 tests run`, not `18 tests run`, because nextest's
default is fail-fast and the run was **cancelled at the first failure** — one arm never
started. The transcript stays as it was printed; the amendment at the end of this entry says
which arm, what it costs, and what it cost this entry's own arithmetic.

The single failure is `hw_profile_capture_reproduces_the_committed_invariant_section`, red at
HEAD as well as under this change, and it is **PF:23** — the OBSBOT's shrinking format tree,
diagnosed the same afternoon and closed at `1a51c81` by a sanctioned re-capture. It is
recorded here because it was seen here, and because a reader of this transcript needs to know
that 17 of 18 was the honest count at 14:11 ~~and 18 of 18 is the count at the commit this
entry sits behind~~. **Withdrawn 2026-08-11 by the G4 adversarial review: nobody ever took the
second count. See the amendment below.**

Three more things were seen and are named rather than fixed:

- **`sys::uevent::tests::a_quiet_socket_answers_at_its_deadline_rather_than_erroring_or_hanging`
  failed once and passed on the next run** (`crates/backends/v4l2/src/sys/uevent.rs:289`,
  "a quiet machine broadcast a uevent during this test"). This is the population N69 and N67
  both name — a real clock in a test that has nothing to say about a duration — and a machine
  that has just run a hotplug arm is not the quiet machine that assertion assumes.
- **A hardware arm that fails *between* its sweep and its restore leaves the camera moved.**
  Seen three times here, deliberately, while running M4/M5/M6, each time putting the OBSBOT's
  brightness back by hand. It is the property `crates/backends/v4l2/tests/hardware.rs` has had
  since P3e — restoration is on the success path — and the daemon's persisted pre-sweep
  snapshot is what makes it recoverable rather than lost. Recorded so a reader of a red
  hardware run knows to check the camera.
- **`wchd` refuses to serve when the composed socket path exceeds 107 bytes**, and says so
  precisely: "…is 130 bytes long and a Unix socket path may be at most 107 — `$XDG_RUNTIME_DIR`
  is too deep for a client to reach a socket under. The daemon binds through a descriptor and
  would not have tripped on the length itself; this refusal is on behalf of the `wchc` that
  would have to connect by this name and could not." Met while poking by hand from a deep
  scratch directory. Not a defect — recorded because it is a good refusal, and because it is
  why the by-hand parts of this session ran from `mktemp -d /tmp/wchhw.XXXXXX`.

### What this run establishes

- **A photo crosses the socket, end to end, and the document agrees with the file.** Three
  cameras, three photos, each negotiated on the daemon's thread, written by the daemon's
  process to a path the client resolved, decoded independently at the reported size and
  counted against the reported `byte_count`. This is P4g's first deliverable and the first
  time any write verb has been driven against real hardware through `wchd` at all.
- **Byte fidelity survives the daemon.** Two MJPG cameras delivered the camera's own bytes
  through the socket, `is_verbatim` true, with the GREY sensor as the negative instance.
  E6's byte-fidelity path had never been exercised anywhere but in process.
- **A calibrate sweep crosses the socket with its progress live.** Twelve events per session,
  all inside the call, spread across the sweep, each carrying the session's id. This is
  P4g's second deliverable, and it is the first assertion anywhere that N57's per-client
  subscription delivers a running sweep's events to a client that is still waiting for the
  answer.
- **The camera is left as it was found, proved by re-reading it.** 22 and 15 non-volatile
  controls compared against a snapshot taken over the same socket, not against a report —
  which M5 shows is the difference between an assertion and a formality (AGENTS rule 8).
- **The daemon exits 0 on a real SIGTERM in both arms**, after 3 photos and after 2 sweeps —
  P4e-ii's teardown discipline under a client that has actually driven hardware.
- **N69's defect is worse on real hardware than the fake could show, and by how much is now
  a number**: 2 in 10 rather than 2 in 150, losing two events rather than one. The fix is
  load-bearing on this rung, and the register now carries the measurement N69 asked for and
  could not take.
- **The suite's skips are exercised, not described** (M7), and the two new arms cost the
  existing sixteen nothing.

### What it does not establish

- **Nothing about the Dell U3224KB/A.** It is off the bus, so the one camera in this house
  whose formats have never been driven through a socket still has not been. E12's fixture was
  four cameras and this one is three; the two entries are not interchangeable, and a later
  reader assembling "what has run against hardware" must intersect them rather than union
  them.
- **Nothing about the progress *rendering*.** This arm asserts that events reach a
  **watcher**; it does not draw a bar. `cli_core::Bar` is an indicatif bar that renders
  nothing when standard error is not a terminal, which is precisely why the sweep arm drives
  the library rather than the binary. E12 said the same sentence about the same claim, and it
  is still true: the last unasserted step between a `SweepFinished` and a human is the
  drawing, and it needs a terminal no test in this workspace owns.
- **The sweep result is two cameras, not three.** The IR sensor declined, counted and named.
  So "a sweep over the socket works" rests on two `brightness` controls on two UVC cameras —
  one PTZ, one integrated — and on nothing else. No other control class was swept, no
  non-`Uniform` plan was run, and no motor moved in either arm.
- **Nothing about a machine with no cameras**, which is the case CI would run if this were a
  gate: both arms would decline, counted, and the comparison would be vacuous. This rung is
  evidence and not CI-gating, under the same carve-out G1, G2 and G3 used.
- **This is `wchc` alone; it is not a parity run.** No answer here was compared against
  `wch`. E12 is the comparison, and E12's is five read verbs. A `photo` taken by `wch` and a
  `photo` taken by `wchc` against one camera remain two states of one device rather than two
  renderings of one answer, which is the reason `cli-parity.sh` puts `photo` in the `device`
  bucket in the first place.
- **Nothing about the TCP or WebSocket surfaces.** Everything above is `AF_UNIX`. The daemon
  was started without `--http`, and the subscription rode the UDS connection.
- **The dropped-event path stayed unexercised in the green runs.** With the tail in place no
  sweep lost an event, so N57's "a subscription is allowed to drop" remains asserted by the
  scripted double and by nothing on this desk. The two M6 failures are the only real losses
  measured, and they were arranged by deleting the fix.
- **The N69 rate is one load level on one host, and 10 runs is a coarse instrument.** "2 in
  10" and "2 in 150" are not the same measurement taken twice; they share a load condition
  and nothing else. N69's own rule applies to this entry as much as to that one — a green run
  is evidence only if its load is stated — which is why the quiet-machine row (0 / 3) is in
  the table and is worth exactly what it says.
- **The hotplug half of G4's R3 evidence is E9's**, taken on a four-camera fixture that no
  longer exists on this desk. Nothing here re-measures it, and nothing here should be read as
  having re-measured it.
- **Still one host, one kernel, one driver** (design §3.3 item 8): three UVC cameras on
  `uvcvideo`, on a machine whose two USB cameras both negotiate High Speed. A `vivid` node or
  a SuperSpeed camera would take the same code path, which is an argument and not a
  measurement.

### Amendment, 2026-08-11: "18 of 18" was never counted, and the run it was extrapolated from was itself truncated

This entry's opening says an E-entry is "recorded once and not amended", and it means it —
the transcript above is untouched, every number in it is what was printed, and this section
is appended rather than folded in for the reason E1's amendments give: the point of a
transcript is that it was true once. What is corrected here is not the transcript. It is a
**sentence of this entry's own prose that reported a measurement nobody took**, found by the
G4 adversarial review, and this project has no worse defect than that one.

**The sentence.** "17 of 18 was the honest count at 14:11 and 18 of 18 is the count at the
commit this entry sits behind." The first half is a reading of the summary line printed
above it. The second half is an **inference** — the one red arm was PF:23, PF:23 was closed
by the re-capture at `1a51c81`, therefore the eighteen would all be green — written in the
grammar of a count, in an entry whose own header says it was transcribed from the 14:11 run
rather than recalled. **No run of the full `hw_` suite has ever reported eighteen of
eighteen.** The 14:11 log was kept, along with every transcript taken after it, and no
eighteen-of-eighteen appears in any of them.

Why that is the serious kind of wrong rather than a rounding of the truth: E-entries are
what later work cites when it does not want to spend a camera. **N70** already cites this
one for a hardware number ("a real sample at a real camera is about 500 ms"), and the whole
apparatus of AGENTS rule 4 and docs/6 §1.2 — `declared` data until a probe makes it
`measured`, and measured wins — depends on a reader being able to tell which of the two a
sentence is. An inference in the register of a measurement is `declared` data wearing
`measured` clothes, in the one place in this repository that exists to keep them apart.

**And the 14:11 count was over a truncated run.** Chasing the first error found a second.
`scripts/smoke-hw.sh` invoked `cargo nextest run` with no `--no-fail-fast`, and
`.config/nextest.toml` sets no `fail-fast` policy either, so nextest's default applied and
the run was cancelled at the first red arm. Under the last arm the log carries

```
warning: 1/18 tests were not run due to test failure (run with --no-fail-fast to run all tests, or run with --max-fail)
error: test run failed
```

and the arm that never started is
**`hw_switching_an_automation_control_moves_its_partners_inactive_bit`** — the D3 pairing
arm, which has two `SKIP (partial)` paths of its own and printed neither, because it did not
run. So the entry's "17 of 18" is not seventeen greens out of an eighteen-arm suite that
finished; it is seventeen arms that started. That defect and its repair are note **N71**.

**What is measured, and it is exactly this:**

- **17 arms started at 14:11 on the tree that became `9c8b46a`; 16 passed and 1 failed.** The
  failure is `hw_profile_capture_reproduces_the_committed_invariant_section`, and it is
  PF:23 — diagnosed the same afternoon, closed at `1a51c81` by a sanctioned re-capture.
- **After `1a51c81`, two arms were re-run individually and were green**:
  `hw_profile_capture_reproduces_the_committed_invariant_section` — the arm PF:23 turned red,
  so this is the re-capture doing what the re-capture was for — and its sibling
  `hw_enumeration_matches_the_committed_profile`. Both compare the committed profile against
  the device's own descriptors; neither opens a stream and neither takes a frame, which is
  why they could be run on a desk where the integrated camera faces a person.

**What is unmeasured, stated as plainly as this entry can manage:**

- **The full 18-arm suite has never been run at HEAD.** Not at `1a51c81`, not at `5faa4ee`
  where this entry landed, not at `f61b2ae`. The count at HEAD is **unknown**, and no
  sentence anywhere may say otherwise until a run prints one.
- Sixteen of the eighteen arms have not been executed since `9c8b46a` at all, including the
  one that never started.
- The two arms in `crates/client/tests/hardware.rs` were green at 14:11 against a
  `crates/client/src/remote.rs` that **no longer exists**: `f61b2ae` rewrote the sweep's tail
  and its guard (N70, 697 lines changed in that one file). The sweep arm is the arm that
  proves N69's fix is load-bearing, so its last green is against the version of the fix N70
  found three defects in. That is the arm whose absence of a fresh run is most worth a
  reader's attention.

**Standing: what closes this, and what it must not cost.** On the machine with these three
cameras, `just smoke-hw` — the rung now runs every arm (`--no-fail-fast`) and refuses to
report a truncated run as a complete one, so from N71 forward the number it prints is a
count rather than a subtraction. Where the integrated camera faces a person, the motor
exclusion is not the relevant knob (`WCH_NO_MOTION=1` excludes only `hw_motion_*`, and the
photo and sweep arms take frames regardless): that is an owner's call about the room, not a
flag. When it is run, the result lands as a new dated E-entry, or as a further amendment
here, with the summary line quoted verbatim — because the lesson of this amendment is that a
count in this repository is a line copied out of a run, never a subtraction performed on
one.

### Second amendment, 2026-08-11: the count exists now, and it was taken on a **wider** fixture than this entry's

The standing condition above is discharged. The owner reattached the Dell U3224KB monitor and
said to use it, so `just smoke-hw` ran at **`b436e62`** against **four cameras on ten nodes**
— the fixture E9, E11, E12 and PF:22 had, and one camera wider than the run this entry is
about. Quoted verbatim, which is what the paragraph above asks for:

```
smoke-hw: motor-moving suites (hw_motion_*) are included — set WCH_NO_MOTION=1 to exclude them
smoke-hw: 10 capture node(s) present; running test(/(^|::)hw_/)
     Summary [  73.338s] 18 tests run: 18 passed, 952 skipped
smoke-hw: 8 claim(s) declined by tests that ran — each named above
smoke-hw: 18 of 18 selected test(s) ran — the suite is complete
smoke-hw: suite run, 0 named skip(s) before it started
```

Exit 0. **Eighteen of eighteen, counted rather than inferred**, and the difference between
this sentence and the one withdrawn above is the whole of what the amendment was for.

Four things this run settles that the first one could not.

- **N71's census is exercised on hardware.** It had been driven over recorded logs and a
  throwaway crate outside the workspace, and its own report said the hardware path "has never
  printed *the suite is complete*". It has now, on the run whose predecessor was the truncated
  one that motivated it. `18 of 18` is the census agreeing with nextest's own summary rather
  than a second opinion about it.
- **All four committed profiles are compared against a device.** The `SKIP (partial): 1
  committed profile(s) match no camera attached to this host … dell-u3224kb` that PF:23 added
  is **absent from this run's eight declines**, because the camera it names is on the bus.
  That skip existed for one afternoon and did exactly the job it was added for.
- **The Dell has not drifted, which is a real answer to PF:23's open question and not an
  absence of news.** Its own descriptors advertise all six committed sizes including
  3840×2160, on the bus path the profile records, and both corpus arms pass. PF:23 could not
  test it and said so.
- **The arm that never started at 14:11 ran**, and
  `hw_switching_an_automation_control_moves_its_partners_inactive_bit` is green with its own
  declines now printed among the eight.

**One thing it does not settle, and it is the interesting one.** The Dell is on a **5000 Mbps
SuperSpeed** link and kept its 4K; the OBSBOT is on **480 Mbps** and lost its. Read alone that
looks like the link-speed explanation PF:23 refuses. It is not, and PF:23's refusal stands
unamended: the kernel log has the OBSBOT enumerating at high speed on 2026-08-08 as well, when
it *did* advertise 3840×2160, so the counterexample is inside the same device at the same
speed. What the Dell adds is a second device measured on the same afternoon that did not
change at all — which narrows the population without naming the cause, and a correlation
across two devices is not a mechanism. The cause stays unproved.

**Retires this amendment:** nothing. It is the measurement the first amendment asked for, and
what it replaces is not a wrong number but the absence of one.

---

## N70 — The tail N69 built was guarded by a filter that could not tell whose sweep had ended, bounded by a number nothing read, and ended by a payload that had not ended anything

**Doc:** notes **N69** (the bounded tail this entry repairs — its measurements and its
reading of N65 stand; three things it did not check did not), **N65** (a drain that refuses
to wait collects nothing, and why), **N57** (the per-client calibration stream, that dropping
an event is allowed, and that the daemon counts what it drops), **N60** (a second-direction
failure means investigate), **N25** (what an equivalent acceptance claims), **E13** (a real
sample at a real camera is about 500 ms), AGENTS rule 2 ("construct the buggy implementation
first and watch it fail, at workspace scope") and `schema::limits`' rule that something reads
every number. Found by the **G4 adversarial review** of N69's own machinery, 2026-08-11, on
the tree at `5faa4ee` — `just ci` green at 936 tests, the mutation floor passed that morning
(526 mutants, 442 caught, 11 accepted survivors, 0 timeouts).

**This entry does not amend N69.** That entry is the record of what was believed and measured
when, and every measurement in it re-runs true: the 34 µs, the 2-in-150, the 0-of-30 empty
queue, the 512 yields. What it shipped with is three holes around the edges of a correct
middle, and the shape of all three is the same — **the fix was tested against the mechanism it
was designed for and not against the world it runs in**.

### Provenance: three findings, one entry, and why they are not three

They land together because they are one review of one sub-system, they were repaired in one
commit, and their common failure is a single sentence (below). Splitting them would repeat
the provenance three times and hide the pattern, which is the part worth keeping.

### F1 — the guard trusted a filter its own doc calls imprecise

**Believed:** that `ended |= event.progress.is_terminal()`, over everything
`SweepFilter::admits` let through, means "the daemon has said its last word about this sweep",
so the tail can be skipped.

**True:** `admits` is *session-only* under `--session <UUID>` and *control-only* under
`--task <TEXT>`. Neither precision asks whether a terminal event belongs to **this sweep**,
and a sweep is a session **and** a control. `wch_subscribe_calibration` fans out from one
`Fanout<ProgressEvent>` for the whole daemon (`crates/daemon/src/events.rs`) and camera actors
are one thread per camera, so two sweeps genuinely run at once and both put their last words
on this socket:

- **two cameras, `--task framing --control brightness` on each.** B's `SweepFinished` matches
  A's control, so it is admitted, draws a spurious closing line on A's bar — and sets A's
  `ended`. A's own terminal event then loses the 34 µs race, the guard skips the tail, and the
  event is lost. **Exactly the defect N69 exists to prevent, arranged by N69's own guard.**
- **one `--session S`, two controls.** The camera actor queues rather than refuses, so the
  earlier sweep's `SweepFinished` carries the same session id and disarms the later client's
  tail. The same collapse inside `drain_tail`, too: it stopped at *any* admitted terminal
  event, one short of the one it came for.

The existing test could not reach either. `another_sweeps_terminal_event_neither_draws_nor_
ends_this_tail` uses `Uuid::from_u128(2)` — a **different session**, which the filter already
rejects — so it pins the case that works and cannot express the case that does not.

**Changed.** The filter now answers two questions with two precisions, and they err in
opposite directions on purpose:

- `SweepFilter::admits` still decides what is **drawn** and still errs toward showing;
- `SweepFilter::is_mine_terminal` decides when the daemon has said its **last word** and
  requires both halves — this sweep's control, and a session this process cannot tell apart
  from its own.

The second half of the pair is a fact `--task` does not have while the call is in flight, and
**the answer supplies it**: `wch_calibrate_sweep` replies with the `Session`, one step before
the tail needs it, and `SweepFilter::with_answer` is where it arrives (`SweepAnswer` is the
one-method trait that keeps `sweep_and_watch` generic over an answer whose type is otherwise
none of its business). So the loop records the *session id* of the last terminal event that
could have been this sweep's — an id rather than a flag, because before the answer the
question has no answer and a flag would have to guess one — and the guard is decided once, at
the moment this process knows the most.

**One id and not a set**, deliberately: the only error it can make is the safe one. If this
sweep's own last word is followed by a neighbour's, the recorded id is the neighbour's, the
tail is entered for a sweep that had already ended, and the cost is the bound once at the end
of a sweep that took camera-minutes. A set would be a queue whose length is a property of how
many sweeps a daemon ran, which is the unbounded shape AGENTS' "bounded everything" refuses.

**Drawing was considered and deliberately not tightened**, which is the half worth arguing.
Under `--session S` it would have been possible — the id is known from the start, so requiring
the control too would stop another control's events repainting this bar. It is not done for
two reasons. The first is that the two errors do not cost the same thing: an event drawn that
was somebody else's costs a repainted bar, and a terminal event *credited* to this sweep that
was somebody else's costs this sweep its last event, so one predicate cannot be right for
both. The second is the failure mode of being wrong: `admits` is the only thing standing
between a bar and no bar at all, and a client whose control spelling ever failed to match the
daemon's would, under a tightened `admits`, draw **nothing** — where under this shape it draws
everything and pays the bound. A committed assertion says the id wins over the control
(`a_sweep_named_by_id_admits_that_sessions_events_and_no_others`); it was not inverted, and
this paragraph is why.

`SweepFilter`'s own residual paragraph is re-worded rather than left standing. Its claim was
"Nothing is lost but a bar's accuracy" — **N69 falsified that sentence** and this change makes
it true again: under `--task` a neighbour's events are still drawn while the call is in
flight, and that is now the whole of the residual, because the same event can no longer end
this sweep early.

**Watched failing** (three tests, against the guard as N69 shipped it):

| test | failure |
|---|---|
| `a_second_sweep_in_this_session_does_not_disarm_this_ones_tail` | `another sweep's last word disarmed this sweep's tail` — left `["focus_absolute", "brightness"]`, right `["focus_absolute", "brightness", "focus_absolute"]` |
| `another_cameras_sweep_of_this_control_does_not_disarm_this_ones_tail` | `another camera's sweep disarmed this sweep's tail` — left `[…0001, …0002]`, right `[…0001, …0002, …0001]` |
| `another_control_in_this_session_is_drawn_but_does_not_end_this_tail` | left `["sweep_finished"]`, right `["sweep_finished", "sweep_finished"]` |

### F2 — nothing could go red on `CLIENT_SWEEP_DRAIN_MS`, and the assert beside it described a different number

**(a) The value was read by nothing that could fail.** All eight of N69's tail tests pass
`Duration::ZERO` — which is the honest way to drive `drain_tail`'s arms without a clock, and
which is why it looked complete — and nothing asserted that `calibrate_sweep` passes the
constant at all. **Measured, because "nothing reads it" is a claim: with
`CLIENT_SWEEP_DRAIN_MS = 0` hand-applied, the workspace suite is 939 passed, 0 failed.** Zero
is not a smaller bound, it is a deleted one: this client's queue is provably empty when its
call answers (N65's measurement, N69's reading of it), so the event a tail exists for is one
that has not arrived and only *waiting* collects it. The fix N69 landed could have been
reverted to the shape its own doc says "cannot work" by editing one digit, and every gate in
this repository would have stayed green. The mutation floor could not see it either —
`.cargo/mutants.toml`'s `examine_globs` covers neither `crates/schema` nor `crates/client`.

**Changed.** `remote::SWEEP_DRAIN_BUDGET` is the number as a `Duration`, in one place, and two
tests are about it. `the_tail_waits_the_moment_out_and_the_budget_is_what_pays_for_it` delivers
this sweep's terminal event **a hundred milliseconds behind the answer** on a paused
`tokio::time` clock — AGENTS' `SteppedClock` shape, "a deadline that is the subject" — and
asserts the bar gets it under the real constant, *and* that the tail ended on the event rather
than on the timer (the "it is a bound and not a wait" sentence, which nothing had asserted
either; the regression it guards against has already happened once, N69's 0.47 s → 0.77 s).
`a_tail_with_no_budget_is_the_fix_deleted_and_that_is_visible_here` is the inverse arm: the
same script at zero loses the same event.

**(b) The const-assert did not check the sentence above it.** What stood there was
`CLIENT_SWEEP_DRAIN_MS < FRAME_DEADLINE_MS`, under a comment claiming "a tail must cost less
than the smallest piece of work the sweep it follows does". `FRAME_DEADLINE_MS` is 2000 and it
is an upper bound on *waiting for one frame*, not the cost of one — a real sample at a real
camera is about 500 ms \[E13\] — so the assert admitted every value up to 1999 while its
comment described a bound of a different order. **An assert whose comment misdescribes it is
worse than no assert**, because it reads to the next person as a checked relation. Replaced by
the two relations that are true: `CLIENT_SWEEP_DRAIN_MS > 0`, the mechanical floor, and
`CLIENT_SWEEP_DRAIN_MS <= HOTPLUG_QUIET_MS`, the ceiling the doc actually derives it from
(`<=` and not `==`: the derivation is a ceiling, and the day 34 µs is re-measured on faster
hardware this number may fall on its own).

**Watched failing**, in three stages, because the floor and the test catch different things:

| what was hand-applied | what went red |
|---|---|
| `CLIENT_SWEEP_DRAIN_MS = 0`, before the new test | **nothing** — 939 passed. This is the finding. |
| `= 0`, with the new test | `the tail refused to wait 100ms for the event it exists to collect, under a budget of 0ns` — left `["sweep_started"]`, right `["sweep_started", "sweep_finished"]` |
| `= 50`, which the const-assert cannot see | the same assertion, `under a budget of 50ms` |
| `= 0`, with the const-assert | `error[E0080]: evaluation panicked: assertion failed: CLIENT_SWEEP_DRAIN_MS > 0` — the whole workspace stops compiling |

### F3 — one undecodable notification was read as the end of the stream, and jsonrpsee disagrees

**Believed** (`ProgressSource`'s own doc): "a stream the daemon closed and a payload this build
cannot decode are both *nothing further is coming*", implemented as
`Some(Err(_)) | None => None`.

**True, and read out of the dependency rather than assumed:** `jsonrpsee-core` 0.26's
`impl Stream for Subscription` (`src/client/mod.rs:429`) sets `is_closed` **only** on the arm
where its receiver yields `None`. A `serde_json::from_str` failure is `Some(Err(_))` on a
subscription that stays open and keeps delivering, with the rest of the queue behind it
decodable.

`CalibrationProgress` is an internally-tagged enum with no `#[serde(other)]` arm, so the
condition has a name: **a `wchd` newer than the `wchc` talking to it**, emitting one variant
this build has never heard of. What that cost was everything after it — `watching = false`,
every remaining event discarded while sitting readable in the queue, and the tail skipped too
(`watching` is its other guard). The bar freezes mid-sweep and the sweep's closing line never
comes: **N69's symptom, from a cause N69 did not consider.**

The seam could not express the fault. `next_event` answered `Option<ProgressEvent>` and the
scripted double's fault menu had `Delivery::Ended` documented as covering "a lag close, a
shutdown, or an undecodable payload" — so the collapse was asserted by a test that had been
told to assert it. **A double's fault menu is a claim about the thing it stands in for, and
this one was a claim nobody had checked against the crate it doubles.**

**Changed.** `next_event` answers a three-valued `Arrival` — `Event`, `Undecodable`, `Ended` —
the loop and the tail skip an `Undecodable` and read on, and `Delivery::Undecodable` joins the
scripted menu. An undecodable payload does not extend the tail's deadline either: the deadline
is an instant, so a daemon sending nothing this client understands still ends the tail at the
bound.

**They are not counted onto anything a person sees**, and that is N69's decision applied
rather than a new one. `wch` cannot produce this condition at all — its sink is synchronous
and nothing is serialized — so a line on `wchc`'s stderr would be a divergence between the two
roots in the one place the parity gate does not look, for a rendering that is already what a
dropped event looks like (N57). What `wchc` has to say to the operator, it says as a bar
missing a line; the daemon is the hop that counts, and N57 already has it counting. Named here
because the alternative was considered: a `cli_core` renderer beside `report_probe`, reachable
from one root only. If the owner wants "your daemon is newer than your client" said out loud,
that is where it goes and it is a product decision, not this repair.

**Watched failing** — the collapse hand-applied to the widened type (`Arrival::Undecodable =>
watching = false` in `sweep_and_watch`, `=> return Tail::Ended` in `drain_tail`), which is the
shipped behaviour re-expressed:

| test | failure |
|---|---|
| `a_payload_this_build_cannot_read_is_skipped_and_the_stream_read_on` | `one unreadable notification ended a sweep's progress` — left `["sweep_started"]`, right `["sweep_started", "sweep_finished"]` |
| `a_payload_this_build_cannot_read_does_not_disarm_the_tail` | `an unreadable notification disarmed the tail` — same two lists |
| `a_payload_this_build_cannot_read_does_not_end_a_tail_already_running` | left `Ended`, right `Terminal` |

### What the three have in common, and it is the entry's point

**Each was a test asserting the assumption that produced the code, in a shape that could not
express the assumption being wrong.**

- F1's other-sweep test used a session the filter already rejects, so it pinned the case that
  works and could not reach the case that does not.
- F2's eight tail tests passed `Duration::ZERO` — correct for the arms they drive, and
  collectively an assertion that the number does not matter.
- F3's fault menu had four variants and the missing fifth was the defect; its `Ended` variant's
  doc *stated* the collapse, so the double agreed with the code by construction.

N60's rule is "a second-direction failure means investigate, never delete the line". This is
its neighbour: **a fault menu, a fixture id and a parameter value are each a claim about the
world, and a test built from them can only ever be as true as the claim.** The three questions
that would have caught all three of these are the same question asked of a double, a fixture
and a constant — *what would this look like if it were wrong?* — and none of them needs a
loaded machine, which is what separates this entry from N67 and N69.

**What is left open, named rather than fixed:** `crates/client/src/remote.rs` is outside the
mutation floor's `examine_globs` and this repair does not widen it. `SweepFilter` is now
exactly the shape the floor is good at — a fold over values with unit tests beside it — but
the file it lives in also owns a runtime, a socket and a `select!`, so widening to the file
imports the triage `.cargo/mutants.toml` refuses for `engine::actor`, and splitting the filter
into its own module is a decision somebody should make on purpose rather than as a side effect
of a bug fix. It is the strongest candidate the next widening has.

**Retires when:** N69 retires — the tail, its bound and its guard go together, and the
condition is the same one: a daemon that can no longer answer a sweep before its own terminal
event has left the process. Until then, the three tests named above are the reason each half
of the guard exists, and a build that deletes one of them has deleted a defect's only witness.

---

## N71 — The rung counted the declined claims of tests that never started, and the one drift this repository anticipates in writing had nothing to catch it

**Doc:** AGENTS rule **1** ("every anticipated or discovered defect class becomes a lint, a CI
job, or a test that can go red"), rule **3** ("CI executes what it claims: counted selections
… and every auto-skipping rung reports a **named, counted skip** — never silence"), rule
**2** (construct the buggy implementation and watch it fail), note **N10**'s family — a gate
that stayed green while checking less than it claimed — **E13**'s amendment of the same date,
which is where the first of these was found and which the first of these had already
corrupted, docs/9's derived-population rule, and `scripts/mutants.sh`'s
`$WCH_MUTANTS_CLASSIFY` (the precedent for the seam added here). Found by the **G4
adversarial review**, 2026-08-11, on the tree at `f61b2ae`, `just ci` green.

**Two findings and one entry, because they are one shape.** Each is a claim this repository
makes about itself with nothing in the tree able to contradict it. One is a number — "4
claim(s) declined by tests that ran" — printed by a rung whose skip accounting could silently
lose members. One is a sentence — "the deployed copy tracks this file" — written into the
rules every agent here reads, with no gate, no case and no recipe behind it. A number nobody
can falsify and a rule nobody can enforce fail the same way: they are believed.

### 1. `scripts/smoke-hw.sh` ran fail-fast, so its counted skip could lose members without saying so

`cargo nextest run` defaults to fail-fast, `.config/nextest.toml` sets no policy against it,
and this rung's invocation asked for nothing else. It fired on the run E13 transcribes, which
carries under its last arm

```
warning: 1/18 tests were not run due to test failure (run with --no-fail-fast to run all tests, or run with --max-fail)
```

and in which `hw_switching_an_automation_control_moves_its_partners_inactive_bit` never
started.

**Two consequences, both real and both this rung's own subject.**

The first is the one that makes it a defect rather than an inconvenience. The accounting
greps `SKIP` lines out of the log and reports "N claim(s) declined by tests that ran". An arm
that never ran prints no `SKIP` lines, so the arms cancelled by the fail-fast are *silently
absent from a named, counted skip* — and the arm this run dropped has two `SKIP (partial)`
paths of its own. Rule 3 exists because a skip that reads as a pass is invisible; a **count**
that quietly shrinks is worse, because a reader who sees a number believes somebody looked at
all of them. It is N10's family with the subject changed from a selection to a census.

The second is that a truncated run read as a complete one. Nothing parsed the cancellation
warning, so "the suite ran and one arm failed" and "the suite stopped after seventeen
eighteenths" produced the same shape of output. E13's summary sentence was written from such
a run, and its amendment is the bill.

**The fix is two things, and the second is not redundant with the first.**

`--no-fail-fast`, so every arm runs. Between it and `--max-fail=all` there is no behavioural
difference — nextest treats the older spelling as the newer one — so the choice is about the
reader: `--no-fail-fast` is the spelling nextest's *own* cancellation warning offers first,
which means whoever arrives at this script from that warning finds the words they were
handed, and it is also cargo-test's spelling, so it does not quietly raise the nextest
version this rung requires. A tool floor is a dependency, not a convenience. A numeric
`--max-fail=N` was rejected outright: a middle setting is a truncation with a nicer name, and
eighteen arms in twenty-eight seconds is not a budget in need of a stop-early switch.

Then a **census**, because the flag cannot go red. A flag is a statement of intent; delete it
in an edit two phases from now and this defect returns with nothing to notice. It also does
not cover every truncation: a SIGINT, a binary that aborts before its siblings start, the
per-test `slow-timeout … terminate-after` in `.config/nextest.toml`. So the run is made to
account for itself in its own numbers — `Starting N tests` against the summary's `N tests
run`, which nextest prints as `17/18` only when they differ — and a shortfall is a loud
failure with a non-zero exit. Same discipline `counted-selections.sh` applies to the gate
table: the claim is not that the selection is right, it is that the run measured what it says
it measured.

The comparison is on the **fact** (how many ran) and not on the **reason** (the warning
line): the reason is English prose that changes between tool versions and only exists for the
causes somebody anticipated, while the two counts are printed by every run and disagree for
all of them. When nextest does give a reason it is quoted underneath, as context. A run whose
census cannot be parsed at all is a failure too — if a future nextest stops printing either
line this rung goes red saying so, which costs one commit; the alternative is a rung that
silently stops checking, which is the whole subject of this entry.

**Watched failing, and the buggy implementation here is the shipped one.** The rung cannot be
run at these cameras — the laptop's integrated camera may be pointing at a person and the
photo and sweep arms take frames — so the accounting was exercised the way `mutants.sh`
exercises its classifier: over recorded logs, through a documented seam. `$WCH_SMOKE_HW_ACCOUNT`
points the script at a saved run, and it accounts for it and stops; nothing is built, no node
is opened, no camera is touched, and it announces itself in capitals for the reason
`$WCH_MUTANTS_CLASSIFY` does.

Over **E13's own 14:11 log**, unedited:

```
smoke-hw: 8 claim(s) declined by tests that ran — each named above
  … eight SKIP lines …
smoke-hw: FAIL — 17 of 18 selected test(s) ran, so 1 arm(s) never started and every claim
above is over the ones that did; a truncated run must not read as a full one
smoke-hw:   warning: 1/18 tests were not run due to test failure (…)
-> exit 1
```

That is the census refusing the run this entry's amendment is about, on the log it was
written from.

Then both directions on a **live nextest**, because a recorded log proves the parser and not
the flag. A throwaway five-test crate outside the workspace, one test failing, one of the
others printing a `SKIP (partial)` line:

| run | census | declined claims counted | exit |
|---|---|---|---|
| default (fail-fast) — the shipped behaviour | `2/5 tests run` | **0** — the `SKIP` line's test never started | 1 |
| `--no-fail-fast` — the fix | `5 tests run` | **1** | 0 |

The left column is the defect reproduced in miniature and the right column is it closed:
same crate, same seeded failure, and the difference is whether the declining arm got to
decline. Three more paths were driven: a real complete workspace run (`15 of 15 … the suite
is complete`), an empty log (`FAIL — this run printed no census this script could read`), and
a log captured with `--color always`, which the escape-stripping reads correctly — a census
defeatable by a colour flag is not a census.

**What is not proven, named rather than left for the next reader to discover.** The rung's
own hardware path has not been executed: no camera was opened by anything in this change, so
"the suite is complete" has never been printed by a real `hw_` run. The seam and the throwaway
crate exercise the accounting, which is a pure function of a text file; they do not exercise
`cargo nextest`'s behaviour inside `run_suite`. And `smoke-hw.sh` is a **rung, not a gate
predicate**, so `scripts/gates/selftest.sh` never sees it and nothing re-runs the eight checks
above — they are a session's evidence, not a standing arm. What would close that is a small
predicate driving `$WCH_SMOKE_HW_ACCOUNT` over two committed fixture logs, one truncated and
one complete; it is not built here because the fixtures are transcripts of a hardware run and
committing them is a decision about what belongs in the tree, not a side effect of a bug fix.

### 2. `AGENTS.md` and docs/10 had no gate, and the file says the drift is expected

AGENTS.md's opening declares its own deployment: "Deploy at the repository root as
`AGENTS.md`; the deployed copy tracks this file (one-directional; when they drift, reconcile
deliberately and record which side was wrong)." Read the parenthesis again. It does not say
they cannot drift — it says **what to do when they do**. A reconciliation procedure written
into a document is the project naming an anticipated defect class in its own words, and rule
1 says a named class becomes something that can go red. This one had nothing:
`grep -rn 'docs/10\|10-claude-fable-agents' scripts/` returned empty, no selftest case existed,
no recipe knew the two files were a pair.

They are byte-identical today — 15115 bytes each, same digest at `f61b2ae` — and ten commits
have moved them since the v2 series was issued at `f6bc5d9`, every one moving both. Ten for
ten by hand is a good record and it is also the exact shape of a rule nobody has needed yet.
The eleventh commit is the one that edits the root copy because that is the path an agent has
open, or the doc because that is where the series lives, and the cost is not cosmetic: the
root copy is what every agent working in this tree reads and the doc is what a review reads,
so a divergence puts two different sets of non-negotiable rules in force at once with no
reader able to tell.

`scripts/gates/agents-md-current.sh` is the gate. Eight failing arms and three green ones in
`cases/agents-md-current.cases.sh`; it joined `run-all.sh` and `selftest.sh` by existing,
which is what those two derive their population for.

**Byte-identical, with no allowance, and that was decided by reading rather than assumed.**
The source doc's first line is `# AGENTS.md — webcam-handler (v2)`: it is written *in the
deployed copy's voice*, names itself by the deployed filename, and its second sentence is the
deploy instruction. There is nothing a root copy would need added and nothing it would need
stripped. An allowance — "ignore a leading front-matter block", say — was considered and
rejected as a hole with a nice name: the moment the predicate tolerates one line it cannot
see, a paragraph fits through the same door, and the failure this gate exists to prevent
arrives wearing the gate's approval. If a real divergence ever becomes necessary it lands in
the predicate as a named exception with an argument and its own arm, decided once.

**Nothing is transcribed.** The source is whichever `docs/*.md` **says it deploys**, and the
deployed filename is read out of that same sentence — the trick `schema-artifacts-current.sh`
uses when it reads `ARTIFACT_DIR` out of xtask's source. A v3 reissue or a renumbering
follows the document instead of requiring an edit here, and a green arm proves it by renaming
the source doc. `docs/historical/` is not scanned: `docs/historical/5-claude-fable-agents-v1.md`
is v1 of this same file and still carries its own deploy sentence, and a superseded
document's instruction is not an instruction.

**The predicate's first version went red on the row that documents it, and the fix is the
interesting part.** `pass_case` failed on the shipped tree the moment docs/9's predicate table
gained the row describing this gate — because that row *quotes the rule it documents*. Two
sources, said the predicate, looking at one declaration and one piece of prose about it. A
gate that cannot tell those apart forbids writing about itself, which is an absurd tax on the
documentation this project runs on. So the search is each document's **preamble**, everything
above its first `##`: a statement about where a document deploys is a statement it makes about
*itself*, and this series makes those in its opening block, where a reader meets them on
opening the file; below the first heading a document is discussing the world. A green arm now
holds that line from the other side by seeding the quoted sentence into another document's
body. This was measured, not designed — which is the second time in this entry that chasing a
claim found the claim's own machinery.

**A symlink is a finding, not a shortcut.** If the root copy were a link to the doc the
comparison would be a file against itself: unfalsifiable, PASS over a population of one,
proving nothing while the tree looked right. That is `gate_require_nonzero`'s defect hiding
somewhere it is harder to see, so the predicate reports it, and an arm seeds it.

**Adding eleven arms ran the machine out of room, which is N66 for the fourth time — and it
is fixed here rather than named.** Every arm is a `gate_scratch_tree` copy of the checkout,
26 MiB on this tree, and `selftest.sh` kept every one of them until its `EXIT` trap. At 21
predicates the run peaked a little over 12 GiB; at 22 it went over the user quota on this
host's `/tmp` tmpfs and said

```
tar: ./README.md: Cannot write: Disk quota exceeded
PROBLEM systemd-units pass_case_a_stop_timeout_written_in_systemds_other_spellings exited 1;
        the predicate is red on a shape it must allow
```

— a filesystem's ceiling reported as a predicate being wrong about the tree, in whichever arm
happened to be running when the room ran out. **`just ci` was red for it twice**, and it is
precisely N52's, N66's and N68's finding in a fourth dimension: a verdict that moves with the
machine, spelled with the word a real finding gets.

The repair is `reclaim_scratch`, four lines: an arm's seeded trees are dead weight the moment
its verdict is in hand, so they go before the next arm starts. Nothing carries across arms —
each case seeds its own copy in its own subshell, and the build directories that *are* shared
live under the checkout's `target/` (`_shared_target_dir`), so a scratch wipe costs no
rebuild. Sampled every two seconds through a full run, `/tmp`'s high-water mark went from
over 12.1 GiB to **9.5 GiB**, and — the point — the term that grew with every predicate this
suite gains is gone rather than smaller. A budget check and a `no_verdict`, `mutants.sh`'s
answer, was rejected because this harness does not *need* the room it was holding, and a
resource a suite does not use beats a resource it asks permission for.

Holding one entry at a time also **identified** the remaining 9.5 GiB instead of leaving it a
mystery, and it is one arm: `counted-selections.sh`'s real-lister arm points
`$WCH_GATE_ROOT` at a scratch copy, and `scripts/gates/counted-selections.sh:40` runs
`cargo nextest list --workspace` with that copy as its working directory and no
`CARGO_TARGET_DIR` — so cargo compiles the whole workspace into `<copy>/target`, 9.7 GiB and
a cold build, deleted seconds later. Named and deliberately not touched: it is another arm's
rubric-rule-6 claim, and changing where it builds is a decision about that claim rather than
a side effect of this one.

This is a change to the harness, which is the one part of the suite the selftest cannot
self-test (docs/9's recorded bootstrap limit). What stands in for that here is that the run's
own counts are unchanged across it — 22 predicates, 37 pass arms, 158 fail arms, before and
after — so the sweep demonstrably removed storage and not coverage.

**Why it gets no `g4` row of its own.** `run-all.sh`'s g4 row already exists for exactly this:
its text says the population is derived rather than transcribed and names the predicates no
row mentions. A `g4` row is for a criterion a phase commissioned, and docs/7 commissioned
nothing here — this invariant has held since `f6bc5d9`, proves nothing P4 built, and a row
claiming otherwise would inflate a count that is supposed to mean something. It arrives
through `run-all.sh` and `selftest.sh` at g0, g1, g2, g3 **and** g4, which is the right reach
for a rule that was never phase-scoped. The row's own arithmetic was updated with it: twenty-two
predicates, ten of them named by no g4 row.

**Retires when:** neither retires as a rule. The census retires if nextest ever refuses to
start a run it cannot finish, which is not a thing a test runner can promise. The AGENTS gate
retires if doc 10 stops being deployed — at which point its own preamble stops saying so and
the predicate goes red until somebody says what replaced it, which is the correct handover.

---

## N72 — One decline stood for four different findings, and the assertion that should have been a decline fired after the camera had been written to

**Doc:** AGENTS rule **7** ("availability is not capability … no code or test converts one
into the other"), rule **3** (a named, counted skip is the rung's account of what it did not
claim), rule **6** (represent the unknown), rule **8** (leave the camera as you found it),
rule **2** (construct the buggy implementation first and watch it fail, at workspace scope),
design **§2.10** (one home per law) and **§5** (motors wear), **D3** (the pairing planner
exists to clear an INACTIVE partner), **E13** — the run these two arms were written for, its
`SKIP (partial)` line quoted verbatim in its own transcript, and its standing gap "a hardware
arm that fails *between* its sweep and its restore leaves the camera moved" — **N70** (a test
that asserts the assumption that produced the code, in a shape that cannot express the
assumption being wrong), **N71** (a claim this repository makes about itself with nothing able
to contradict it), and \[PF:3\], \[PF:4\]. Found by the **G4 adversarial review**, 2026-08-11,
on the tree at `9142b81` — `just ci` green at 944 tests and 22 predicates, `just smoke-hw`
18 of 18 at `b436e62`.

**Two findings, one entry, and it is not an amendment to E13.** That entry is a dated
transcript of a run plus the reasoning around it, and both of its amendments correct
*sentences E13 itself wrote* — a count nobody took, and a run that was truncated. Nothing
here touches a number E13 printed or a claim E13 made: the sweep arm did what that entry says
it did, on the cameras it says it did it on. What is repaired is **code that landed with it**,
and the precedent for that is one commit old in each direction — N70 repaired N69's machinery
without amending N69, and N71 repaired `smoke-hw.sh` without amending the entry whose
transcript exposed it. An E-entry that grew a third amendment every time a later review found
a defect in the code it exercised would stop being a transcript and start being a changelog.

The two findings share a subject, which is why they are one entry: **`crates/client/tests/hardware.rs`
answered "this camera is not taking part" for four different reasons and said the same
sentence for all of them, and answered "this camera cannot be swept usefully" by panicking on
a real device after writing to it.** Both are the same mistake about what a hardware suite is
for. A rung against hardware nobody controls is an instrument, and an instrument that reports
"different" as "broken" — or reports four different readings with one word — is not measuring.

### F4 — an `assert!` turned a device shape into a red run, twenty lines above the restore

**The shape.** The sweep arm derived its stride from the descriptor,
`stride = (span / 4).max(desc.range.effective_step())`, handed it to the daemon as
`SweepSpec::Uniform`, ran the sweep on the camera, and then asserted

```
assert!(total >= 3, "{control}: {total} sample(s) is too few to say anything about a sweep");
```

in `check_progress` — reached from `sweep_one_camera` **before** `calibrate_restore`.

**What that costs on a device this desk does not have.** A `brightness` declaring `0..=64`
with a step of 64 plans exactly two values, and so does one declaring `0..=1`. Both clear the
target predicate — writable, active, integer-typed, maximum above minimum — so the arm
*selects* such a control, opens a session, writes to the sensor five hundred milliseconds at
a time, and then panics. The camera is left at the last value the sweep wrote, because the
restore is twenty lines further down and restoration is on the success path.

**This is a lesson the sibling rung already carries in writing**, in
`crates/backends/v4l2/tests/hardware.rs`'s enumeration arm: "A host whose cameras are not in
the corpus is a host this arm has nothing to say about, which the module doc promises and an
`assert!(matched > 0)` broke: it turned 'different hardware' into a red run." The new file
re-introduced the shape one rung over, and added the half the older one did not have — the
older assert fires before anything is written.

**Changed.** The planned size is asked of `engine::sweep::plan` — **the same pure core the
daemon runs a moment later**, design §2.10's one home per law, not a second planner with a
second opinion — and it is asked from the `ControlDesc` alone, so the answer exists before a
session exists. `sweep_for` answers `Sweep::Planned { spec, samples }` or
`Sweep::Declined(String)`, and the decline is a named `SKIP (partial)` line taken **before**
`calibrate_start`.

**Before `calibrate_start` and not, as the review suggested, after `calibrate_plan`**, and
the difference is a write. `wch_calibrate_plan` answers a `Session` whose per-control status
is `Untouched` or `Blocked`; it carries no sample count, so the number is not there to read.
And one call earlier, `wch_calibrate_start` runs **D3's empirical pair probe (N16), which
writes to the camera and puts it back** — `crates/daemon/src/server.rs`'s own comment says
so. So the last moment that is genuinely before any write is before the session opens, which
is where the guard went. A guard against "a failure between the sweep and the restore" that
sits after the first write would have been the finding wearing the fix's clothes.

The `assert!(total >= 3)` did not survive the move, because it could no longer fail — an
assertion whose false branch is unreachable is decoration, which is the same rule as "no
assertion inside a conditional whose false branch cannot go red". What replaced it is a claim
only this rung can make:

```
assert_eq!(total, expected,
    "{}: {control}: this client planned {expected} sample(s) from the device's declared \
     range and the daemon's plan is {total}", info.id);
```

The client planned from the descriptor it read over the socket; the daemon planned from the
descriptor its own camera actor read, through the same core, and answered a count that
crossed a serialization to get here. A build that dropped `Uniform`'s step on the wire — the
plan silently becoming `All`, every one of a `0..=255` range's 256 values — is red here
rather than a sweep that takes two hundred and fifty-one extra photos and passes.

`MAX_SWEEP_SAMPLES` is still checked and still the schema's. `MIN_SAMPLES` is this suite's
own, argued in its doc comment as a fact about what *these assertions* can see rather than
about what the product allows — two samples is a perfectly good sweep for an operator, and
two of anything cannot tell a progress stream from a report delivered at the end, which is
the one property the arm exists to observe. A `const` assertion holds it under the schema's
ceiling, which is N70's F2 discipline applied to the one number this file owns.

### F5 — a counted decline asserted a fact its predicate could not support

**The sentence, as `scripts/smoke-hw.sh` printed it and E13 transcribed it:**

```
SKIP (partial): cam:… exposes no sweepable brightness-class control among its 3, so this arm
declines it — which is a fact about this sensor's control set and not about the socket
```

**The predicate behind it** was a conjunction of five terms over three names, and only the
first is about a control *set*. `testkit::battery::is_perturbable` alone requires
`desc.current` to be `Some(Int(v))`, in range and step-aligned, plus writable, plus scalar,
plus not volatile, plus not WRITE_ONLY; the conjunction then adds `!is_inactive()`,
`control_type == Integer` and `range.max > range.min`. So the same sentence was printed for
all of these:

- **a `gain` that is present but INACTIVE** because `auto_exposure` is engaged \[PF:3\] — a
  *state*, and D3's pairing planner exists precisely to clear it, so the next run with
  automation off would sweep the control this line said the camera does not have;
- **a `brightness` reporting a current outside its own declared range, or off its own step**
  — the represented-unknown class AGENTS rule 6 names, a PF-class device finding of exactly
  the kind \[PF:4\] was raised for, reported as an absent control;
- **a read-only, DISABLED, VOLATILE or menu-typed one** — *capabilities*, and each a different
  one.

And `{N}` was `report.controls.len()`, which carries no information about which term failed.
Rule 7 says no test converts availability into capability; a predicate that answers a bare
`false` has already done the converting, because the caller is left holding a bool where the
interesting half was **which term said no**.

**Why nothing caught it.** E13's M7 mutant exercised the skip path — it replaced the three
control names with names no camera has, every camera declined, the arm passed in 0.224 s and
printed four counted `SKIP` lines. That mutant drives the **name-absent** arm, which is the
one arm the sentence was true for. Every state arm is a shape no camera on this desk can
produce: the three cameras E13 ran against, and the four attached today, either have a wide
usable `brightness` or have no brightness-class control at all. It is N70's population
exactly — a test asserting the assumption that produced the code, in a shape that cannot
express the assumption being wrong — and it is N71's too, in the register that entry cares
about: a sentence a rung prints about itself with nothing in the tree able to contradict it.

**Where the predicate now lives, and the argument.** It moved to
**`testkit::battery`**, beside `is_motorized` and `is_perturbable`, and both hardware rungs
now ask it. Three reasons, in the order they decided it.

1. **It was already written twice.** `crates/backends/v4l2/tests/hardware.rs` and
   `crates/client/tests/hardware.rs` each carried a private `brightness_class_target` with
   the same three names and the same five terms. The client copy's doc comment defended that
   — "the workspace has nowhere shared to put a *selection* that is only ever made by a test"
   — and the defence was wrong on its own terms: this crate is exactly that place, it is
   dev-only by gate (`testkit-is-dev-only.sh`), and the two answers this same pair of files
   already share live in this same module for this same reason. Moving one copy and leaving
   the other would have been half a repair.
2. **A private helper inside an ignored binary is a rule nothing can unit-test**, and that is
   most of why this went unnoticed. In `testkit::battery` the predicate is a fold over
   `ControlDesc` values with sixteen tests beside it, run by every `just ci` on a machine with
   no camera attached. **A unit test over values is worth more here than another hardware
   run**, because the shapes that matter are shapes this desk cannot produce — and "I could
   not reproduce it on my hardware" is the reason the test is necessary, not a reason to skip
   it (N67's and N69's lesson in a third costume).
3. **The distinction belongs in a type, not in a `println!`.** What the old line asserted by
   being typed, `Decline::is_a_fact_about` now answers as a value, and a test reads it.

**The shape it took.** `why_not_perturbable` answers `Option<Disqualifier>` — fifteen variants
naming one term each, with a `Display` that renders the phrase a `SKIP` line uses
(`read-only`, `DISABLED`, `INACTIVE — an automation partner owns it [PF:3]`,
`current 300 outside 0..=255 [PF:4]`, `type is Menu`, `range 5..=5 holds one value`). Three
consequences worth naming:

- **`is_perturbable` is now *defined* as that answering nothing**, rather than written a
  second time beside it. A bool and a reason kept side by side are two rules that agree until
  somebody edits one, and a test asserts the equivalence over every shape anyway.
- **The verdict on writability stays with the schema and only the diagnosis is here.**
  `ControlDesc::is_writable` folds READ_ONLY, DISABLED and the class header into one `false`;
  the diagnosis takes them apart again and ends in a payload-carrying fallback arm —
  "not writable, by a term this suite does not name yet" — because rule 6 applies to this
  project's own vocabularies as much as to the kernel's, and the day somebody reads that
  sentence in a transcript is the day the enum gets its next variant.
- **The motor question is asked first**, before any term that would have refused anyway,
  because design §5 is a law about hardware wear rather than a consequence of how three
  controls happen to be spelled. It is unreachable through `BRIGHTNESS_CLASS` today and
  asserted anyway; the day somebody adds `focus_absolute` to that list, it is the term that
  must fire.

`brightness_class_target` then answers `SweepTarget::Found` or `SweepTarget::Declined`, and
the decline is one of two things: `NoneNamed { examined }`, a fact about the control set, or
`NoneUsable(Vec<(ControlSlug, Disqualifier)>)`, a fact about a control the sensor *has* —
**every** named one, not just the first, so a transcript does not send a reader hunting a
`gain` fault the same run already diagnosed. The old sentence survives verbatim for the one
case it was true for, which is the Chicony IR sensor and is what the rung still prints.

### Watched failing, both findings, at workspace scope

Rule 2's shape for a *new* predicate is the shipped one re-expressed, and that is what was
built first in each case.

**F5** — `why_not_perturbable` implemented as `if is_perturbable(desc) { None } else {
Some(Disqualifier::NotWritable) }` (one word for every term) and `brightness_class_target`
answering `NoneNamed` for every failure (the shipped conjunction). Full workspace suite,
`--no-fail-fast`, because N71 is one commit old:

```
Summary [   4.596s] 960 tests run: 949 passed, 11 failed, 26 skipped
```

The eleven, and the two that carry the finding rather than a tick:

| test | failure |
|---|---|
| `an_inactive_gain_is_a_fact_about_a_state_and_not_about_a_control_set` | left `NoneNamed { examined: 1 }`, right `NoneUsable([(ControlSlug("gain"), Inactive)])` |
| `every_disqualified_control_is_a_fact_about_a_state_and_never_about_a_control_set` | `assertion left == right failed: exposes none of brightness, gamma, gain among its 1 control(s)` — left `"this sensor's control set"`, right `"the state of a control this sensor has"` |
| `a_read_only_brightness_names_the_flag_that_refused_it` | `exposes none of brightness, gamma, gain among its 1 control(s)` |
| `a_disabled_brightness_is_not_reported_as_a_read_only_one` | the same line |
| `a_menu_typed_brightness_is_declined_by_its_type` | the same line |
| `a_compound_typed_brightness_is_declined_before_its_payload_is_read` | the same line |
| `a_one_value_brightness_range_is_declined_by_its_range` | the same line |
| `a_volatile_brightness_and_a_write_only_one_name_their_own_flags` | the same line |
| `a_brightness_whose_current_is_outside_its_own_range_names_the_reading` | the same line |
| `a_brightness_sitting_off_its_own_step_names_the_step` | the same line |
| `every_named_control_is_reported_and_not_only_the_first` | `exposes none of brightness, gamma, gain among its 2 control(s)` |

That every one of them fails with the *same sentence* is the finding printed eleven times.
Five arms stayed green under the buggy implementation and are there to keep it honest: a
plain `brightness` is still found, a genuinely name-absent sensor still declines about its
control set, and `is_perturbable` still agrees with its reason function.

**F4** — the floor deleted (`if samples < MIN_SAMPLES` neutered), which is the shipped
behaviour:

```
Summary [   4.669s] 967 tests run: 964 passed, 3 failed, 26 skipped
```

| test | failure |
|---|---|
| `a_brightness_whose_step_is_its_whole_range_is_declined_rather_than_swept` | `ControlDesc { … range: ControlRange { min: 0, max: 64, step: 64 } … } was planned as 2 sample(s) of Uniform { step: 64 } rather than declined` |
| `a_two_valued_brightness_is_declined_rather_than_swept` | the same, `min: 0, max: 1, step: 1`, `2 sample(s) of Uniform { step: 1 }` |
| `a_single_valued_range_is_declined_and_the_planner_is_not_asked_to_invent_a_second_value` | the same, `min: 50, max: 50`, `1 sample(s)` |

Three more arms hold the other direction, and one of them is the reason the floor is a `<`
and not a `<=`: `the_ranges_the_attached_cameras_declare_plan_the_five_samples_e13_transcribed`
pins the two real ranges that entry recorded (`0..=100` at a stride of 25 and `0..=255` at a
stride of 63, five samples each), `the_floor_is_a_boundary_and_a_plan_that_just_clears_it_is_swept`
pins a three-sample plan running, and `the_count_is_the_planners_and_not_arithmetic_repeated_here`
pins the one case where the two differ — a control whose own step is 7 cannot take a stride of
25, so the planner rounds to 28 and answers four samples where arithmetic over the stride
would say five. That last one is what would go red if the count here were ever re-derived
instead of asked for.

### Hardware

`just smoke-hw` at four cameras on ten nodes, on the tree this entry lands with. The census,
verbatim, which is what N71 requires of any number this rung reports:

```
smoke-hw: motor-moving suites (hw_motion_*) are included — set WCH_NO_MOTION=1 to exclude them
smoke-hw: 10 capture node(s) present; running test(/(^|::)hw_/)
     Summary [  72.295s] 18 tests run: 18 passed, 975 skipped
smoke-hw: 8 claim(s) declined by tests that ran — each named above
smoke-hw: 18 of 18 selected test(s) ran — the suite is complete
smoke-hw: suite run, 0 named skip(s) before it started
```

Eight declines, the same eight as at `b436e62`, and the two this change rewrote now read

```
SKIP (partial): cam:integrated-camera-integrated-i exposes none of brightness, gamma, gain
among its 3 control(s), so this arm declines it — which is a fact about this sensor's control
set and not about the socket
SKIP (partial): cam:integrated-camera-integrated-i exposes none of brightness, gamma, gain
among its 3 control(s), so this arm declines it — which is a fact about this sensor's control
set and not about the backend
```

— the same claim as before, now made by a predicate entitled to make it, and made by **both**
rungs through one piece of code. The two affected arms were also run alone and both are green:
`hw_a_sweep_over_the_socket_delivers_its_progress_live_and_leaves_the_camera_where_it_found_it`
swept and restored `brightness` on the OBSBOT, the Chicony RGB and the Dell (5 samples, 12
events each, 22 / 15 / 17 non-volatile controls compared against the opening snapshot), and
`hw_a_calibration_session_sweeps_a_brightness_control_selects_applies_and_restores` did the
same in process.

**What the hardware run does not establish, and this is F5's own point turned on this entry:**
not one of the four attached cameras produced a `NoneUsable` decline, because not one of them
has a brightness-class control in a disqualified state. The line these arms print on this desk
is the same line they printed before the repair. **The repair is proved by the sixteen unit
tests and by nothing on this desk**, which is exactly why it is where it is.

### Named and deliberately not fixed

- **`crates/backends/v4l2/tests/hardware.rs:1573`** — `assert!(outcome.samples.len() >= 3,
  "{control}: {} sample(s) is too few to show an ordering")`, in the in-process calibration
  arm. This is F4's shape in the sibling rung, complete with the second half: it fires after
  the sweep has run on the camera and before the session's restore. A camera whose
  `brightness` declares `0..=64` step 64 turns that arm red and leaves the control at 64. It
  is not fixed here because it needs the same treatment on a different arm's terms — that
  suite plans through `engine::calibrate` rather than through a `SweepSpec` it built itself —
  and because a repair to an arm this change is not otherwise touching should be its own
  decision.
- **`crates/backends/v4l2/tests/hardware.rs:2060`** — `assert!(values.len() >= 3, "…too few to
  move a motor and come back")`, the same shape in `hw_motion_*` and the milder instance:
  everything above it is planner arithmetic, so a device with a narrow pan range makes the arm
  red without leaving a motor anywhere it should not be.
- **`scripts/gates/ignored-suites-have-recipes.sh:191`** — the awk arm
  `/#\[[[:space:]]*ignore/ { pending = 1 }` reads the attribute wherever it appears, comments
  and doc comments included, and then reports the file's next `fn` as an ignored test with no
  suite. Three sentences of this change's own prose tripped it: the gate named
  `why_not_perturbable`, `driving_the_hardware` and `brightness` as ignored tests. It fails
  **closed**, so nothing was ever wrong on the tree — the cost is that a file may not write
  the attribute's name in prose, which is a real tax on a project whose comments are essays
  and cite attributes by name. The declaration half of this same script already guards against
  precisely this ("the gate must not read its own documentation as a declaration") and the
  ignored-test half does not. The prose here was reworded instead, and `scripts/gates/` is
  outside this change's remit.

### Retires when

Neither retires. F4's guard retires only if a hardware rung stops asserting anything that
needs several samples, and F5's split retires only if `SKIP` lines stop being this project's
account of what a run did not claim — which is AGENTS rule 3, and it is not going anywhere.
The sixteen unit tests are the reason each term of the predicate exists, and a build that
deletes one of them has deleted a disqualifier's only witness on a desk where no camera can
produce it.

### Amendment, 2026-08-11: the two instances this entry named in the sibling rung are closed

The section above ends by naming three defects it found and did not fix. Two of them are
**F4's own shape in `crates/backends/v4l2/tests/hardware.rs`** — the class this entry opened,
left open twice in the file this entry's own comments cite as the place the lesson was already
written down. They are closed here. The third is a gate and is **N73**, for the reason argued
at the head of that entry.

**Why an amendment and not a new entry, which is the opposite of what F4 and F5 got.** N72
argued itself into being an N-entry rather than an E13 amendment on the ground that E13 is a
*transcript* and its amendments correct sentences E13 itself wrote. An N-entry is not a
transcript; it is the case law for one defect class, and its "Named and deliberately not
fixed" list is that class's outstanding docket. Closing a docketed item changes nothing this
entry claims and needs no argument this entry has not already made — the two arms below are
repaired *on their own terms*, which is exactly the reason given above for not repairing them
at the time, and that reason is a fact about scheduling rather than about the finding. A
reader who arrives at `crates/backends/v4l2/tests/hardware.rs:1573` should meet one entry, not
two that agree. Recording it separately would also break the thing N72 is useful for: **the
"Retires when" section below is the class's, and a class whose instances are recorded in three
places retires in three places.**

The gate is the other way round on every one of those tests, which is why it is not here.

#### F4a — the in-process calibration arm, the full shape

`hw_a_calibration_session_sweeps_a_brightness_control_selects_applies_and_restores` derived
`stride = (span / 4).max(desc.range.effective_step())` after opening a session, ran
`engine::calibrate::run` over it, and then asserted

```
assert!(outcome.samples.len() >= 3, "{control}: {} sample(s) is too few to show an ordering");
```

twenty lines below the sweep and **three hundred above `lifecycle::recover`**. A `brightness`
declaring `0..=64` with a step of 64 plans two values, clears every term of
`brightness_class_target`, and so was selected, photographed at each of its two values, and
panicked on — the camera left at 64, because restoration is on the success path. It is F4
with a longer fall.

**Changed** the way F4 was: the planned size is asked of `engine::sweep::plan` through
`testkit::battery::sweep_for`, from the `ControlDesc` alone, and a plan under the floor is a
named `SKIP (partial)` taken **before `start_session`**. Before, and not after, for the reason
F4 records one rung over and this rung confirms in its own transcript: `start_session` calls
`lifecycle::discover_pairs`, whose printed line is `left the camera alone: true` — a claim
about *restoration*, not about abstinence. The last moment genuinely before any write is
before the session opens.

The floor that stood there did not survive, because it can no longer fail. What replaced it is
this arm's own claim, and it is not the client rung's claim in different words:

```
assert_eq!(outcome.plan.total(), planned,
    "{}: {control}: this arm priced {planned} sample(s) from the descriptor the device \
     enumerated before the session opened, and the executor's own re-read plans {}");
```

There, two plans are separated by a serialization. **Here they are separated by a second
enumeration of the same device, across a write.** This arm prices from the descriptor
`camera.controls()` answered at the top; `calibrate::run` prices from the one its own
`describe` re-reads, and between them the pair probe wrote to the sensor and put it back. A
camera that re-declares a control's range after being written to is the class \[PF:23\] was
raised for — a device changing what it advertises while nobody was looking — and it is red
here rather than a sweep that quietly takes a different number of photographs than the arm
reports.

#### F4b — the motion arm, and the "milder" was checked rather than taken on trust

The claim above was that
`assert!(values.len() >= 3, "…too few to move a motor and come back")` is the same shape
without the second half, because everything above it is planner arithmetic. **Read rather than
believed, and it holds**: `backend.open` and `camera.controls()` read; the two
`engine::sweep::plan` calls that assert §5's "never implicit" and "the cap is real on this
device's range" are folds over a `ControlDesc`; `bounded_motion_values` is arithmetic over
five offsets; and the first write in the arm is `start_session`'s pair probe, twenty lines
*below*. No motor was ever left anywhere by this assertion.

That is the whole of the mildness, and it decided the shape of the repair rather than excusing
it. A PTZ camera whose pan range holds two steps — or one parked where only two of the five
offsets land inside its range — still turned a *device shape* into a red run, which is AGENTS
rule 7 and the class rule 1 says becomes a test. Because the arithmetic was already in the one
place where nothing is turning, the repair is **only** a decline: `bounded_motion_values`
answers `Motion::Planned { values }` or `Motion::Declined(String)`, the caller prints a named
`SKIP (partial)` and continues, and nothing moved.

**The decline is taken after the two descriptor-only claims and not before them**, and the
order is design §5's rather than tidiness. Those claims cost no travel, so a camera too narrow
for this arm's trajectory still has "the product refuses a motion sweep without
`--allow-motion`" and "the motion cap binds on this device's own range" asserted against it.
Declining earlier would throw away two free claims to avoid a cost of zero.

What replaced the floor is the claim the number was standing in for:

```
assert_eq!(outcome.plan.values, values, …);
assert!(outcome.plan.adjustments.is_empty(), …);
```

**The motor went exactly where this arm bounded it, and nowhere else.** `SweepSpec::Explicit`
clamps a value outside the range, aligns one off the step, drops a duplicate and subsamples a
list over the cap — each recorded as a `SweepAdjustment`, none of them visible in
`outcome.samples`, and every one of them a motor going somewhere this arm did not choose. It
holds today because `battery::is_perturbable` already refuses a control whose current sits off
its own step \[PF:4\]; the day that stops being true, this says so instead of spending the
difference.

#### `sweep_for` moved, and that is the third copy that did not get written

N72's F5 moved a predicate because it was already written twice. `sweep_for` was written
**once**, in `crates/client/tests/hardware.rs`, and the calibration arm above needed the
identical arithmetic against the identical planner with an identical floor. Writing it again
is how a rule becomes two rules, so it is now `testkit::battery::sweep_for`, beside
`brightness_class_target` and for the same three reasons that entry gives. Both rungs ask it;
what stays in each rung is the one thing that is genuinely its own, which is the floor.

**The floor is not shared and the mechanism is**, and that distinction is a type.
`SampleFloor { count, because }` carries an arm's number *and its argument for the number*
into the `SKIP` line, because the two rungs decline at the same count for unrelated reasons —
the client arm needs enough progress events to tell a live stream from a report delivered at
the end (N69), the calibration arm needs enough samples for a metric to *rank* rather than
compare two endpoints (N21) — and a transcript that printed "fewer than the 3 this arm needs"
for both would make one number look like a law. `a_floor_of_zero_declines_nothing` is there
because a caller's parameter should not have a hole at the bottom of itself. Each rung keeps
its own `const _: () = assert!(…)` under the schema's ceiling, which is N70's F2 discipline
where the number is.

`ShortSweep` is the decline as a value rather than a `String`: `Refused` when the planner
refused the range outright, `UnderFloor` when the plan is legal and small. The two carry
*different* steps on purpose — `Refused` reports the step **as declared**, because a device
declaring 0 is \[PF:4\] and a transcript that printed 1 would hide it, while `UnderFloor`
reports the effective one, because that is the number the count was computed against.

**The cost is one dependency edge and it is worth naming.** `webcam-handler-testkit` now has a
normal dependency on `webcam-handler-engine`, which dev-depends on it. Cargo permits the cycle
because the return edge is a dev-dependency and dev-dependencies do not link;
`dependency-walls.sh` counts only normal edges and passed unchanged at 1699 items; the wall
this could have touched — "`webcam-handler-client` links no engine" — is unmoved, because that
crate's edge to the testkit is a dev one too. The alternative was for each rung to call
`engine::sweep::plan` itself, which is the copy this move exists to avoid.

#### Watched failing, both, at workspace scope

Rule 2's shape for a guard is the shipped behaviour re-expressed, which is the guard neutered.

**F4a and the shared floor** — `if samples < floor.count` replaced by `if false` in
`battery::sweep_for`, which is what both rungs did before N72 and what the calibration arm did
until today. One neuter, three crates red, which is the point of the move:

```
Summary [   4.747s] 980 tests run: 973 passed, 7 failed, 26 skipped
```

| test | failure |
|---|---|
| `battery::tests::a_range_under_the_floor_declines_as_a_value_and_names_the_count_the_planner_gave_it` | left `Planned { spec: Uniform { step: 64 }, samples: 2 }`, right `Declined(UnderFloor { control: ControlSlug("brightness"), min: 0, max: 64, step: 64, stride: 64, samples: 2, floor: SampleFloor { count: 3, because: "say anything about an arrival profile" } })` |
| `battery::tests::the_two_rungs_decline_at_the_same_count_and_do_not_print_the_same_sentence` | `assertion left != right failed`, both sides `Planned { spec: Uniform { step: 1 }, samples: 2 }` |
| `battery::tests::the_declared_step_reaches_the_refusal_and_the_effective_one_reaches_the_count` | the `UnderFloor` half planned instead of declining |
| `v4l2::hardware the_calibration_arms_floor_declines_the_range_that_used_to_be_swept_and_then_panicked_on` | `a range that plans two samples is the shape this arm swept and then panicked on` |
| the three client arms N72 wrote (`a_brightness_whose_step_is_its_whole_range_…`, `a_two_valued_brightness_…`, `a_single_valued_range_…`) | as that entry records them |

That the client's three go red under a neuter made in another crate is the repair's own
witness: there is one floor comparison in the workspace now, and seven arms across two rungs
are looking at it.

**F4b** — `if values.len() < MIN_MOTION_VALUES` replaced by `if false`, which is the shipped
`bounded_motion_values`:

```
Summary [   4.516s] 980 tests run: 977 passed, 3 failed, 26 skipped
```

| test | failure |
|---|---|
| `a_pan_range_that_holds_two_positions_is_declined_rather_than_driven` | `ControlDesc { … range: ControlRange { min: 0, max: 1, step: 1 } … } was planned as the trajectory [0, 1] rather than declined` |
| `a_step_that_is_most_of_the_range_leaves_a_motor_two_places_to_be` | the same, `min: 0, max: 100, step: 60`, `the trajectory [0, 60]` |
| `a_single_position_range_is_declined_and_no_motor_is_asked_to_move` | the same, `min: 50, max: 50`, `the trajectory [50]` |

Three arms hold the other direction and one of them is why the floor is a `<`:
`the_pan_range_the_obsbot_declares_plans_five_positions_around_home` pins the two real motion
ranges in `corpus/profiles/` (the OBSBOT Tiny 3's `-468000..=468000` step 3600 and the Dell
U3224KB's `-144000..=144000`) at five positions each,
`a_motor_parked_at_the_bottom_of_its_range_visits_only_the_positions_above_it` pins the
asymmetric case *and* the difference between filtering the out-of-range offsets and clamping
them — clamping answers `[0, 0, 0, 1, 2]`, which `dedup` shortens to three *different* values
and a motor visits two of them twice — and
`the_motion_floor_is_a_boundary_and_a_trajectory_that_just_clears_it_is_driven` pins exactly
three.

#### One more decline this change made dishonest, and repaired in passing

Both arms end with a summary line for the case where nothing ran, and both named a reason.
The calibration arm's said "no attached camera offered a brightness-class control **and a
capture node**" — one sentence for two facts, which became one sentence for **three** the
moment "a declared range too narrow to assert over" joined them. That is F5's shape arriving
through the door F4's repair opened, in the same commit. Both tails now say only that nothing
ran and point at the per-camera lines above, which are named and counted and are the only
place a reason belongs.

#### Hardware

`just smoke-hw` at four cameras on ten nodes, on the tree this amendment lands with. The
census, verbatim:

```
smoke-hw: motor-moving suites (hw_motion_*) are included — set WCH_NO_MOTION=1 to exclude them
smoke-hw: 10 capture node(s) present; running test(/(^|::)hw_/)
     Summary [  77.494s] 18 tests run: 18 passed, 988 skipped
smoke-hw: 8 claim(s) declined by tests that ran — each named above
smoke-hw: 18 of 18 selected test(s) ran — the suite is complete
smoke-hw: suite run, 0 named skip(s) before it started
```

Eight declines, the same eight as at `2a3a58c`, and **not one of them is new**. The
calibration arm ran three sessions (the OBSBOT, the Chicony RGB and the Dell), five samples
each, restore complete on 22 / 16 / 17 controls; the motion arm moved the OBSBOT's
`pan_absolute` through `[0, 3600, 7200, 10800, 14400]` and the Dell's through
`[-7200, -3600, 0, 3600, 7200]`, five samples and four steps of travel each, and both heads
are back where they started.

**What the hardware run does not establish, which is this entry's own point for the second
time:** no camera on this desk exercised either new decline, and none can. Every attached
camera with a `brightness` declares a range that plans five samples, and both motion ranges
hold two hundred and sixty steps. The two guards are proved by the ten unit tests over values
and by nothing on this desk — which is why the shared half is in a crate a unit test can
reach, and why the two arms' own floors are pinned in the files that own them.

`just ci` green at **980 tests** (from 967) and 22 predicates; `scripts/gates/selftest.sh`
reports 38 pass arms and 160 fail arms (from 37 and 158), the two new pass/fail arms belonging
to N73.

### Named and deliberately not fixed, by the amendment

- **`crates/backends/v4l2/tests/hardware.rs:2344`** — `assert!(held.len() > 1, "every sample
  came back as the same position … the motor did not move")`, which fires between the motion
  sweep and its restore and leaves the head where the sweep put it. It is **not** in the class
  closed here and cannot be moved: a driver that reports the same read-back for every
  commanded position is a finding about the device \[PF:18 is the neighbouring one\], and
  nothing in the descriptor predicts it. It is E13's standing gap in its irreducible form —
  "a hardware arm that fails between its sweep and its restore leaves the camera moved" — and
  what it wants is a restoring wrapper around the whole arm, which is a change to how every
  hardware arm is written rather than to this line.
- **`crates/backends/v4l2/tests/hardware.rs:2130` and `:990`** — two `capture_node().is_none()`
  arms that `continue` in silence, in the motion arm and in
  `hw_a_stream_honours_a_size_the_camera_offers_and_reports_one_it_does_not`. Their three
  siblings in the same file (`:882`, `:1193`, `:1466`) print a named `SKIP (partial)` for the
  identical condition. AGENTS rule 3 wants the skip named and counted, and the cost of these
  two is visible in the sentence repaired above: the motion arm's tail can only speak for the
  cameras it *examined*, because the ones it passed over said nothing.

## N73 — A gate read a sentence about an attribute as the attribute, and the guard it needed was already in the other half of the same file

**Doc:** AGENTS rule **1** (a discovered defect class becomes a lint, a CI job, or a test that
can go red), rule **2** (both directions, proven in `scripts/gates/selftest.sh`, and the
inverse arm driven by the thing under test), rule **3**, the "Docs and dependencies" rule that
this project's comments are essays that cite their own vocabulary, docs/9's gate suite, and
**N72** — which met this three times in one commit, named it, and left it because
`scripts/gates/` was outside that change's remit. Found by the **G4 adversarial review**,
2026-08-11, and repaired on the tree at `2a3a58c`.

**Not an amendment to N72, and the two halves of that judgement are worth keeping apart from
the amendment above.** What N72 owns is a class about *hardware arms*: an assertion that turns
a device shape into a red run. Its docket named three items; two of them are that class and
are closed in the section above, and this one is not that class at all. It is a **gate**
reading prose as code, its population is every `.rs` file in the tree rather than two test
binaries, its verification is a pair of selftest arms rather than a unit test over
`ControlDesc` values, and its retirement condition has nothing to do with sweeps. It also
carries a lesson N72 does not: **a guard that exists in one half of a script is not a guard
the other half has**. Filing it under N72 would make that entry's "Retires when" answer for a
fact about awk.

**The defect.** `scripts/gates/ignored-suites-have-recipes.sh:191`:

```awk
/#\[[[:space:]]*ignore/ { pending = 1 }
pending && match($0, /fn[[:space:]]+[A-Za-z0-9_]+/) { … }
```

The token was matched wherever it appeared. A `//`, `///` or `//!` line that named it set
`pending`, and the file's next `fn` — of any kind, `#[ignore]`d or not, a helper or a test —
was then reported as an ignored test matching no declared suite prefix.

**It fails closed, so nothing was ever wrong on a tree because of it.** The cost is a tax, and
the tax is on exactly the thing this repository's rubric asks for. N72 paid it three times in
one commit: the gate named `why_not_perturbable`, `driving_the_hardware` and `brightness` as
unrouted ignored tests, and three sentences were reworded to appease it. Two module docs still
carried a parenthesis apologising for naming the token, one of them a whole paragraph
(`crates/backends/v4l2/tests/hardware.rs`: "the attribute is named around rather than written
out, because …").

**And the guard was already in the file.** The declaration half of this same script reads
`grep -rHn '^# wch-suite:'`, anchored at column zero, and the header says why in as many
words: "the indented copy above is prose, and the gate must not read its own documentation as
a declaration". One half of the script knew that a sentence about a marker is not a marker.
The other half did not use it.

**Changed.** Each line is reduced to its **code** before either rule looks at it: string
literals first, then whichever of `//` and `/*` comes earliest in what is left, with block
comments carried across lines. The order is load-bearing in both directions and neither
direction is hypothetical:

- **Strings before comments**, because a reason string may contain `//` —
  `#[ignore = "see http://…"]` is a legal attribute and truncating at the `//` would eat the
  closing bracket.
- **Line comments before block comments**, because a doc comment may contain `/*`:
  `crates/backends/v4l2/src/holders.rs` writes `` `/proc/<pid>/fd/*` `` in its module doc, and
  stripping block comments first would swallow the remaining twelve hundred lines of the file
  and every attribute in them.

**The tolerance cannot hide a real test, and that is the property the repair had to keep.** An
attribute reaches an item at the head of its own line, with nothing before it for either
stripper to trip over. What a reducer *can* do is go quietly wrong in the other direction, so
every tolerated mention is counted and reported — `gate_checked "$prose"` — rather than
dropped. On the tree the numbers are 26 ignored tests (unchanged from before the repair, which
is the measurement that says no attribute was eaten) and 2 prose mentions.

**Watched failing, at workspace scope, and the arm that proves it is not a stub.** Three
selftest arms, and the pair is the point: a reducer that reads prose correctly and code
incorrectly is a *worse* gate than the one it replaced, because the pass arm alone would go
green and nothing would notice.

- `pass_case_the_attribute_is_named_in_prose_and_not_read_as_one` seeds one file holding all
  six shapes — module doc, doc comment, trailing comment, one-line block comment, multi-line
  block comment, string literal — each followed by a plain `fn`. Against the **shipped**
  predicate it is red, 8 violations over 38 examined items: the six seeded shapes plus the two
  sentences this change restored in the tree. Against the repaired one it is green with those
  8 counted as prose.
- `fail_case_a_real_ignored_test_hidden_among_prose_about_the_attribute` seeds the same prose
  with one genuine unrouted `#[ignore]`d test underneath it. Red, naming
  `hidden_in_the_prose_and_nobody_runs_it`.
- `fail_case_an_attribute_sharing_a_line_with_a_block_comment_that_closed` seeds a file whose
  only unrouted test is declared on a line beginning `/* R3, one day */`. It is the only arm
  that can say the reducer *resumes* after `*/` rather than dropping the rest of the line, and
  it was checked by breaking exactly that: with the resume removed, the arm exits 0 and the
  harness reports it as a problem.

**Proved on real prose rather than asserted.** Two sentences were restored to the form the
gate had refused — `crates/client/tests/hardware.rs`'s "Both arms above are `#[ignore]`d",
which is one of N72's three, and the whole apologetic parenthesis in
`crates/backends/v4l2/tests/hardware.rs`'s module doc, which predates it. Against the shipped
predicate the tree is red with

```
FAIL  ignored-suites-have-recipes: webcam-handler-v4l2's ignored test 'attached' … matches no declared suite prefix
FAIL  ignored-suites-have-recipes: webcam-handler-client's ignored test 'driving_the_hardware' … matches no declared suite prefix
FAIL ignored-suites-have-recipes — 2 violation(s) over 32 examined items
```

— `driving_the_hardware` being the same false finding N72 recorded by name. Against the
repaired one it is `PASS — 32 items examined, 0 named skip(s)`, with 26 real ignored tests
still found. N72's other two reworded sentences are left as that entry wrote them; they read
well, and one restored sentence per direction is what proves the gate rather than three.

`scripts/gates/selftest.sh` now reports **22 predicates, 38 pass arm(s), 160 fail arm(s)**,
from 37 and 158.

### Retires when

Never, in the sense that matters: the rule "an `#[ignore]`d test that no recipe runs is a test
that will never run again" is not going anywhere, and a gate that reads Rust will always have
to know which characters are code. The *reducer* retires the day this script parses Rust
properly rather than lexically — which it will not, for the reason `unsafe-scope.sh` writes
down for its own token: a rule a reader can verify by eye beats one that needs a parser. That
gate makes the opposite trade on the two halves this one now handles, deliberately and with
the argument recorded; it is a different predicate over a different population and its
decision is left where it is.

---

## E14 — The P4 adversarial review, 2026-08-11

docs/8 Part E asks for a review pass at each phase boundary and for the review's own record
as a dated evidence entry. P1's findings are in E1's amendments, P2's is **E4**, P3's is
**E6**. This is P4's, and it is the first one written *after* its own reconciliation instead
of before it. docs/7's P4g commissions four things — "the adversarial review; fixes;
**evidence entry**; rubric reconciliation" — and three of them landed on the day: the review
ran against the P4g diff, seven of its eight findings were repaired across four commits
(`f61b2ae`, `b436e62`, `2a3a58c`, `e69ffba`), and G4's reconciliation is the fourth block of
docs/8's record at `4b2a7dd`. The entry was missed. The reconciliation says so in its own
opening, records it in P4g's text rather than retro-fitting it, and Part E now asks for the
entry by name; this is that entry, and writing it is the standing requirement discharged.

**What lives here and in no other file.** N70, N71, N72 with its amendment and N73 each
carry one finding or one pair, and each argues why it is not an amendment to something else
— N72 says why it is not an E13 amendment, N73 says why it is not an N72 one — while E13
carries two amendments of its own. Every one of those is locally right, and none of them
holds the review whole. What falls between them is the census, the false-positive arithmetic
**N54** reads a sub-milestone's sizing from, the mutation-floor run that certified the tree
the review read, the three hardware runs the repairs were driven against, and the absence
list. That is this entry, and it is deliberately not a re-telling: where a finding's argument
already exists, this entry cites it and states the number instead.

### The census, and the one number that was never taken

**Eight findings, every one reported with an attempted refutation already written.** docs/8
Part E requires adversarial verification before a Critical/High finding is reported, and this
pass applied it to all eight rather than to the top band alone. **Seven fixed, one ruled on
and left** — F7, kept deliberately, with the reasoning in `4b2a7dd`. By severity: **2 HIGH, 1
MEDIUM-HIGH, 3 MEDIUM, 1 LOW/MEDIUM, 1 LOW**. **Nothing was rejected as not-real after it was
reported**, so N54's "findings rejected as not-real" column reads **0 of 8** for this review.

**And the candidate count cannot be recovered, so this entry states none.** E4 counts thirty-
one candidates against fifteen survivors and sixteen refutations; E6 counts thirty-one
against twelve, deduplicating to nine distinct defects, and nineteen refuted. Both numbers
exist because those reviews raised candidates through several lenses and *then* handed each
one to skeptics, which makes the candidate population an artifact somebody can count. This
pass ran the refutation **inside** the find pass, before reporting, which is what Part E asks
for and which has the side effect that a candidate that died left nothing behind. The find
pass did produce a substantial **"checked and found sound" list** — negative results, each
naming where it looked — but it was *reported* and not committed, so the only parts of it
that survive are the absence paragraphs of docs/8's G4 reconciliation and the section below,
both written from it. **A count nobody wrote down is a count this repository does not have**,
which is E13's own lesson of this session arriving one entry later and pointed the other way:
E13's amendment had to withdraw a number that was inferred rather than measured, and the
honest answer here is the absence of a number rather than a reconstruction of one.

**So the rate is not comparable, and this entry adds no G4 row to N54's table.** The column
would read 0 of 8 beside four rows counting a different denominator, and a table whose rows
are not the same measurement is worse than a table with a gap. N54 reads a *falling*
false-positive rate as a saturation signal — a reviewer facing a diff it "never runs out of
real material" in never has to reach for a marginal claim — and predicts that splitting a
sub-milestone raises both the finding count *and* the false-positive rate. This review cannot
test that prediction in either direction: its
denominator is reported findings and N54's is raised candidates, its harness is one agent
where N54's table is four sub-milestones of the same multi-lens harness, and neither its
agent hours nor its tool calls were recorded. The prediction stays unfalsified and untested,
and the next review that wants to settle it has to keep its candidate list.

**Where the review was wrong while its finding stood**, which is the part of the
false-positive question that *is* recoverable, because two repair sessions wrote it down:

- `2a3a58c` records that the instruction it was handed said to read the planned sample size
  from what `calibrate_plan` answers. It is not there — that verb returns a `Session` whose
  per-control status is `Untouched`/`Blocked` and carries no sample count — and one call
  earlier `wch_calibrate_start` runs D3's empirical pair probe, which **writes to the
  camera**. F4 stood exactly as reported; the last genuinely pre-write moment was one call
  earlier than the route described, which is where the guard went.
- `4b2a7dd` records five statements in the reconciliation's brief that were wrong and were
  corrected in the files rather than smoothed over, two of them arithmetic (it is **twelve**
  predicates that double, not thirteen, because `selftest.sh` sits in `GATE_HARNESS_FILES`
  and outside `run-all.sh`'s population) and one of them a provenance correction that this
  entry inherits: `run-all.sh`'s missing g4 row was **P4g's row-accretion pass**, not an
  adversarial-review finding, and it is not counted among the eight.

**Three defects surfaced by the repairs rather than by the find pass**, and they are not in
the eight either. Two are F4's own class still open in the sibling rung
(`crates/backends/v4l2/tests/hardware.rs:1573` and `:2060`), named in N72's docket and closed
by its amendment at `e69ffba` under AGENTS rule 1 — a class enforced in one of two files is a
decoration. The third is **N73**, a gate reading prose as code, met three times while F4 and
F5 were being written and closed in the same commit. The session that repairs a review's
findings is itself a review, and in this register it found three more.

### The eight findings

| # | what it was | severity | landed |
|---|---|---|---|
| **F1** | `wchc`'s sweep tail armed `ended` from `SweepFilter::admits`, which is session-only under `--session` and control-only under `--task`, so a neighbouring sweep's terminal event disarmed this sweep's tail and lost the event N69 exists to save | HIGH | `f61b2ae` (N70) |
| **F2** | `CLIENT_SWEEP_DRAIN_MS` was read by nothing that could go red — zero passes the suite — and the const-assert beside it compared it against a bound of a different order | HIGH | `f61b2ae` (N70) |
| **F3** | an undecodable notification was read as the end of the subscription; `jsonrpsee-core` 0.26 closes only on `None`, so a `wchd` newer than its `wchc` silenced a stream that was still delivering | MEDIUM | `f61b2ae` (N70) |
| **F4** | `assert!(samples.len() >= 3)` in the R3-over-UDS calibration arm fired **after** the sweep and **before** the restore, so a two-value `brightness` became a red run with the control left where the sweep put it | MEDIUM | `2a3a58c` (N72); the same shape closed in the sibling rung at `e69ffba` |
| **F5** | one counted decline claimed "a fact about this sensor's control set" for a predicate that also refused an INACTIVE `gain` \[PF:3\] and a current outside its own declared range \[PF:4\] — AGENTS rule 7, in a test's prose | MEDIUM | `2a3a58c` (N72) |
| **F6** | `smoke-hw.sh` counted the declined claims of arms that never started, so a truncated run read as a complete one — and E13's "18 of 18" was an inference written in the register of a measurement | MEDIUM-HIGH | `b436e62` (N71); the count itself taken at `9142b81` |
| **F7** | twelve of the twenty-two predicates run twice per `just gate-g4`, once through their own row and once inside `run-all.sh`'s derived population | LOW | ruled on and kept, `4b2a7dd` |
| **F8** | AGENTS.md declares its own deployment from docs/10 and says what to do "when they drift", and nothing in `scripts/` could tell — rule 1 against a class the project names in its own prose | LOW/MEDIUM | `b436e62` (N71), gate `agents-md-current.sh`, docs/9 row |

Two things about that table's severity column, because a grade is a claim like any other.
**F1's and F2's HIGH and F4's MED band are the only ones pinned in a committed document**
before this entry — docs/8's G4 block states them in those words. The other five are the
review's own grades, recorded here because no other file holds them, and a later reader who
wants to re-grade them has the finding's own note to do it from.

**What the eight cost and bought, in the numbers the commits state.** `just ci` goes from
**936 tests at `5faa4ee` to 980 at `e69ffba`** — `f61b2ae` +8, `2a3a58c` +23, `e69ffba` +13,
with `b436e62` and `9142b81` adding none — one new gate predicate (**22 from 21**), and
`scripts/gates/selftest.sh` from **34 pass / 150 fail arms to 38 / 160**, eleven of those arms
F8's and three N73's. Four commits, four trees: each note records the tree its repair session
started from, which is why one review cites `5faa4ee` (N70), `f61b2ae` (N71), `9142b81` (N72)
and `2a3a58c` (N73) for findings that were all reported at once.

### The two HIGH findings, and both were in code four hours old

Both are in `wchc`'s sweep tail, which is **N69**, which landed at **13:06:57** on the same
day the review read it at **17:53:15** — four hours and forty-six minutes by the commit
timestamps, which docs/8's reconciliation rounds up to forty-seven from the clock faces. N69
is not a sketch: it ships an argued entry, **eight tests** over the tail's arms, a measured
2-in-150 on the fake under four concurrent workspace suites, and — by 14:43 at `9c8b46a`, an
hour and a half later — a hardware measurement of 2 in 10 at the real cameras (E13). That is
the standard of care this project asks for, and it is what the two findings were found in.

**F1 is the defect N69 exists to prevent, arranged by N69's own guard.** `admits` is
session-only under `--session` and control-only under `--task`; neither precision asks whose
sweep ended, and a sweep is a session **and** a control. One `Fanout<ProgressEvent>` per
daemon and one actor thread per camera means two sweeps genuinely share the socket, so
`--task framing --control brightness` on two cameras lets B's `SweepFinished` disarm A's tail
and lose A's last event. The committed test for exactly this case
(`another_sweeps_terminal_event_neither_draws_nor_ends_this_tail`) names its other sweep with
`Uuid::from_u128(2)`, a session the filter already rejects — it pins the case that works. G3's
reconciliation had *added* Part C's smell "a test whose fixture cannot exercise the rule it
pins" one gate earlier; the row was in force, in writing, and did not fire, and docs/8 now
carries three reasons why that is not a checklist somebody skipped.

**F2 is the fix deleting itself by one character.** With `CLIENT_SWEEP_DRAIN_MS = 0`
hand-applied the workspace is **939 passed, 0 failed** — that 939 is not the tree the review
read (936 at `5faa4ee`) but the tree mid-repair, F1's three tests in and F2's two not yet,
and `f61b2ae`'s eight new tests are three for F1, two for F2 and three for F3. Zero is not a
smaller bound but a deleted one: this client's queue is provably empty when its call answers
(N65's measurement, N69's reading of it), so the event a tail exists for is one that has not
arrived, and only waiting collects it. Nothing in the suite, and nothing in any gate, could
say so. **Neither could the mutation floor**, whose `examine_globs` name ten files in
`crates/{api,engine,imaging}` and `crates/backends/v4l2/src/hotplug.rs` — and therefore
neither `crates/schema`, where the constant lived, nor `crates/client`, where it is read.
Both HIGH findings lived in the two crates the only mechanical proxy for their class cannot
see, which is the reason docs/8 declines to call that row prevention.

### The mutation floor's run at `5faa4ee`, which has no dated entry of its own

E7, E8 and E10 each record a run; this one was taken during the review and recorded only as a
parenthesis in N70 and a clause in docs/8, so its arithmetic goes here.

**526 mutants: 442 caught, 11 missed, 73 unviable, 0 timed out. 42 m 07 s of wall clock, exit
0.** All eleven survivors match recorded acceptances in `scripts/mutants-accepted.txt` and the
register is checked both ways — N25's six, N26's three, N27's one, N37's one — so nothing is
unaccounted for and no acceptance has stopped surviving. The scope has not moved since P4d
(ten `examine_globs` lines, `.cargo/mutants.toml` unchanged), so the eleven mutants between
E10's 515 and this run's 526 are what five of those ten files gained *since* P4d —
`api/{codes,photo,wire}.rs`, `engine/settle.rs` and `engine/store.rs` are the files that
moved, and `wire.rs` and N67's work in `settle.rs` are most of that diff.

**It was taken on a deliberately frozen tree, and that is why the entry can quote it.** N68's
finding is that a run whose input moves while it is reading it is a third outcome and not a
verdict; the tree was held at `5faa4ee` for the duration. **Zero timeouts is worth its own
sentence.** N52's loaded run produced 34 timeouts and 31 false survivors on the same
workspace, and the pin (`minimum_test_timeout = 180.0`) plus the freeze is what buys a clean
column here; the two numbers side by side are the whole argument for both disciplines. (N70
and docs/8 both place these counts "that morning"; the run is this one, on the tree committed
at 16:11, so *morning* there is the day and not the clock.)

**It settles a question N69 raised.** The earlier floor run, on the stable tree at `4a76b1d`,
reported the acceptance `crates/engine/src/session.rs: replace > with >= in
sampled_precision` as no longer surviving — which under N60's rule means investigate rather
than delete, and investigating it is how N69 exists at all. N69 argues the acceptance is
correct and equivalent (the input is sorted and deduplicated, so every gap is at least 1) and
re-measures it by hand at 935/935 over four runs of five, concluding that what actually failed
was a `wchc` arm losing its terminal event. **This run confirms it from the tool's own side:
the mutant is among the eleven survivors.** The register was right, the report was the flake,
and the flake was a product defect — N60's pattern for the third time, now closed at both
ends.

**What the run does not establish** is what E7 and E10 already say and this entry does not
weaken: nothing outside the ten files, nothing about mutants the tool does not generate (a
defect of omission is not in its vocabulary), and nothing about the two crates that held F1
and F2. N70 leaves the widening candidate named — `SweepFilter` is now exactly the fold over
values the floor is good at, but `remote.rs` also owns a runtime, a socket and a `select!`,
so splitting the filter into its own module is a decision to take on purpose rather than as a
side effect of a bug fix.

### The hardware runs, at a fixture that grew mid-session

Three `just smoke-hw` runs carry this review's repairs, all **four cameras on ten nodes** —
E9/E11/E12/PF:22's fixture — and all **18 of 18, exit 0, motors included, eight declines
each**:

| tree | wall | census line |
|---|---|---|
| `b436e62` | 73.338 s | `18 tests run: 18 passed, 952 skipped` / `18 of 18 selected test(s) ran — the suite is complete` |
| `2a3a58c` | 72.295 s | `18 tests run: 18 passed, 975 skipped` / the same census line |
| `e69ffba` | 77.494 s | `18 tests run: 18 passed, 988 skipped` / the same census line |

**The four-camera fixture was available only for the later runs, and that is a fact about the
desk rather than about the code.** E13's own run is **three** cameras on six nodes — the Dell
U3224KB/A was off the bus — and the owner reattached it mid-session, which is why the count
E13's first amendment declared unmeasured could be taken at all. The count itself is E13's
second amendment (`9142b81`), and the sentence that matters there is that eighteen matching
the withdrawn inference **does not retroactively justify having asserted it**.

Three things those runs establish beyond "green". N71's census is exercised on hardware for
the first time, on the successor to the very run whose silent truncation motivated it, and it
agrees with nextest's own summary rather than offering a second opinion. All four committed
profiles are compared against a device, so PF:23's one-afternoon `SKIP (partial)` for an
unattached `dell-u3224kb` is absent from the eight declines. And both motor heads come back:
the OBSBOT's `pan_absolute` through `[0, 3600, 7200, 10800, 14400]` to **7200**, the Dell's
through `[-7200, -3600, 0, 3600, 7200]` to **0**, five samples and four steps of travel each,
restore asserted rather than assumed (AGENTS rule 8).

**And the eight declines are the same eight across all three runs — not one of them is new**,
which is the point F5's repair keeps making about itself. No camera on this desk produced a
`NoneUsable` decline, no attached `brightness` plans fewer than five samples, and both motion
ranges hold 260 steps, so **neither of F5's fifteen typed disqualifiers nor either of F4's two
sample floors was exercised by any camera here**. They are proved by unit tests over
`ControlDesc` values — sixteen for F5's `Disqualifier`, ten for the two floors as N72's
amendment counts them — and by nothing on this desk. That is the whole reason both predicates
were moved into `testkit::battery`, where a unit test can reach them.

### What the review did not find, each tied to what would have caught it

E4 and E6 both hold that this section is worth as much as the findings, and G4's absences have
had nowhere to live but docs/8's reconciliation. Verified against the tree rather than
asserted:

- **No unsound `unsafe`, over the phase that put a second kernel socket in the tree.** The
  four repair commits add and remove no `unsafe` token in product code — every line in their
  diff containing the word is prose *about* the word, in a module doc and in these notes.
  `sys::uevent` carries no `unsafe` block at all (its `sockaddr_nl` is `linux-raw-sys`'
  bindgen output through `rustix`), and the residual register stands at **eleven blocks and
  one `unsafe impl`**, one obligation each, reconciled against the tree by `unsafe-scope.sh`'s
  third claim — which got its own `g4` row at P4g because no phase row at any letter had
  named it.
- **No state write outside D9's home.** No repair touches `engine::store`, `write_json_atomic`
  or the fd-lock; `atomic-write-home.sh` is green, with the raw-write population widened at
  P4e-i to see `rustix`'s spelling of an open.
- **No availability-to-capability conversion in the product, and the distinction is the
  finding.** The one AGENTS rule 7 finding is **F5**, and it is in a *test's prose* — a `SKIP`
  line claiming "a fact about this sensor's control set" for a predicate that was a
  conjunction of capability *and* state terms. No shipped code converts one into the other;
  what did was a sentence a rung prints verbatim as its account of what it declined, which is
  rule 3's surface and rule 7's subject meeting in one line.
- **The mutation floor clean**, 526/442/11/73/0 above, register checked both ways.
- **No wire-surface and no schema-artifact change.** Nothing under `crates/api/` or
  `schemas/` moved. The one `webcam-handler-schema` edit is `limits.rs`, and it is a doc
  comment, one deleted const-assert and two added ones — `CLIENT_SWEEP_DRAIN_MS` is still
  **250**. `schema-artifacts-current.sh` is the mechanism that would have said otherwise and
  it is green, which matters because AGENTS' "Done means" records that editing prose in that
  crate can move a committed artifact.
- **No surface change at all: the review added no verb and no flag.** `crates/cli-core`,
  `crates/cli` and `crates/daemon` are untouched by all four commits. The only new switch
  anywhere is `$WCH_SMOKE_HW_ACCOUNT`, a documented dev seam in a rung script modelled on
  `mutants.sh`'s `$WCH_MUTANTS_CLASSIFY` and incapable of touching a camera. What the review
  *did* add is one gate predicate, a rung's decline vocabulary, and forty-four tests.
- **No new PF-class finding.** Four N-entries and two amendments; no device behavior was seen
  that the corpus did not already carry. PF:23 belongs to the sub-milestone before this one.
- **And one absence is now known to be worth less than it reads.** G3's "no fault-menu variant
  without a driven inverse" held again — every variant of `ProgressSource`'s menu had its
  driven inverse — and **F3 is the variant that was not in the menu**. An exhaustive walk over
  a fault menu cannot see a fault the menu does not have; docs/8 carries that as a corollary
  now, and this entry records that the absence claim was satisfied and was the wrong question.

### What the review did not cover, which matters more than the absences

E12 and E13 established that a run states its own fixture before it states its result; a
review owes the same.

**One agent, one pass, over the P4g diff only.** The find pass read `4a76b1d..5faa4ee` — from
N68's close to E13's landing — which is P4g and nothing else. P4a through P4f were reviewed at
their own sub-milestones, so this pass is not a review of P4 and no sentence here should be
read as one. It is also **one** lens where E6's had six and N54's table counts a multi-lens
harness per sub-milestone, which is the second reason the census above cannot be compared with
E4's and E6's.

**It ran read-only, with compilation forbidden, because the mutation floor was running
concurrently on the same host.** Every one of the eight was therefore settled *by reading* —
the dependency's source for F3 (`jsonrpsee-core-0.26.0/src/client/mod.rs:429`), the filter's
own doc for F1, the const-assert's operands for F2, the predicate's conjunction for F5 — and
several were confirmed by execution only afterwards, in the repair sessions, which is where
the 939-passed measurement and every watched-failing table in N70–N73 come from.

What that buys: it is the only posture under which a review and a 42-minute floor run share a
machine without either one's verdict being a function of the other's load, which is N52's,
N66's, N68's and N71's finding four times over and the reason the floor's zero-timeout column
is trustworthy. What it costs: **a finding that needs a loaded machine to see cannot be found
this way.** N70's three all happen to be findings that need no load at all — a fixture id, a
constant, a fault menu, each a claim about the world that a reading can check — and that is
luck about this diff rather than a property of the method. The load class (N65, N67, N69,
E13) is precisely what a read-only pass is blind to, and docs/8 now carries it as a Part C
smell and a Part E step for exactly that reason.

### The residuals this session named and nobody owns

Seven items were named and deliberately not fixed across this session's commits, each in a
note about something else. They are gathered here for the first time, because a residual
recorded inside another entry's argument is a residual the next reader finds by accident:

- **`crates/backends/v4l2/src/sys/uevent.rs:289`** — `assert!(!ready, "a quiet machine
  broadcast a uevent during this test")`, in
  `a_quiet_socket_answers_at_its_deadline_rather_than_erroring_or_hanging`. E13 names this
  assertion; N69 names the *other* real-clock assertion in the same test five lines below
  (`started.elapsed() < Duration::from_secs(1)`, `:294`), so the test has two ways to be about
  a machine's quietness rather than about the socket. It **fired again during this session's
  own `just ci` under concurrent load** — observed and not transcribed, so it is recorded here
  as an occurrence and not as a rate. It is the population N67 discharged for `engine`'s
  settle path and nobody has taken for this one.
- **`crates/backends/v4l2/tests/hardware.rs:2344`** — `assert!(held.len() > 1, …)` between the
  motion sweep and its restore. N72's amendment argues it is **not** in the class that
  amendment closed: a driver reporting one read-back per commanded position is a device
  finding \[PF:18 is the neighbouring one\] and nothing in a descriptor predicts it. What it
  wants is a restoring wrapper around the whole arm, which is a change to how every hardware
  arm is written.
- **`crates/backends/v4l2/tests/hardware.rs:2130` and `:990`** — two `capture_node().is_none()`
  guards that `continue` in silence where three siblings (`:882`, `:1193`, `:1466`) print a
  named `SKIP (partial)` for the identical condition. AGENTS rule 3, and the cost is visible:
  the motion arm's tail can speak only for the cameras it examined.
- **`scripts/gates/counted-selections.sh:40`** — `cargo nextest list --workspace` with a
  scratch copy as cwd and no `CARGO_TARGET_DIR`, so it cold-builds the workspace into
  `<copy>/target`: **9.7 GiB**, deleted seconds later, and the whole of `selftest.sh`'s
  remaining 9.5 GiB peak after N71's `reclaim_scratch`. Moving it touches that arm's rule-6
  claim, so it is a decision rather than a cleanup.
- **docs/9's predicate table has no row for `mutation-verdict.sh`**, which has had a `g4` row
  in `scripts/gates/phase-criteria.tsv` since N68. The repo's files are authoritative and
  docs/9 records deltas, so this is a missing delta rather than a missing gate.
- **`crates/cli-core/src/lib.rs:1448`** — "The progress bar is suspended for the duration of
  the rendering below", written above a `watcher.finish()` that runs first, about a
  `ProgressBar::suspend` that **exists nowhere in the tree**. B7·3's clause is therefore
  unasserted *and* its one piece of prose describes a mechanism that is not there.
- **`crates/client/src/remote.rs` is outside the mutation floor's `examine_globs`**, named by
  N70 and unchanged by its repair. It is the strongest candidate the next widening has, and
  it is where a HIGH finding hid from the only mechanical proxy its class has.

### What this entry does not establish

- **Nothing about P4a–P4f.** The find pass read one sub-milestone's diff.
- **Nothing about the candidate population.** Eight reported and eight confirmed is a
  numerator over a denominator nobody wrote down, and no arithmetic in this entry pretends
  otherwise.
- **Nothing about the cost of the review as N54 prices it.** Agent hours and tool calls were
  not recorded, so the sizing question P4g could have answered stays open.
- **Nothing new about hardware.** The three runs above are the repairs' verification, not a
  probe; the fixture, the eight declines and both motor ranges are what earlier entries
  already establish, and the new predicates were exercised by unit tests rather than by a
  camera.

**Retires when:** never — this is dated evidence. What it hands forward is three standing
items: the residual list above, the candidate-count discipline (a review that wants a
comparable false-positive rate has to commit its candidate list, not just its findings), and
the widening `remote.rs` is waiting for.

---

## N74 — The token gate admits a request only when *every* credential it presents verifies, and first-wins and last-wins are refused by name

**Doc:** design **D11** — TCP "requires a bearer token" — and docs/7 P5a's "401-without/200-with".
Both sentences describe a gate that admits a correct credential and refuses a missing one.
Neither says what a request carrying **two** credentials means, and that is the question a
token gate is actually got wrong on.

**Repo:** `daemon::http::gate` states the rule once, in its header, and implements it in one
function over values: **every credential the request presents must verify, and it must present
at least one.** No precedence, no fallback between the two forms, no "the header beats the
query".

- **Both forms are read**, and both are load-bearing. `Authorization: Bearer <token>` is what
  code sends — the page's own requests, P5b's WebSocket and preview. `?token=<token>` is what a
  **navigation** can carry: an operator opening a link performs a navigation, a navigation
  carries the headers the browser chose, and there is no way to attach an `Authorization` to one
  short of an extension. `Token::ready_to_open_url` writes exactly that form.
- `headers.get_all(AUTHORIZATION)`, not `get`: two `Authorization` headers is a well-formed HTTP
  request, and a gate that read the first would be the first-wins gate wearing a different hat.
- The query string is **hand-parsed rather than deserialized**, which is why axum's `query`
  feature is off in the daemon's manifest and why the manifest says so. A deserializer into a map
  answers the duplicate-parameter question by picking one, silently, and that question is this
  entry.
- **The fold does not short-circuit**: `&=` over every credential rather than `Iterator::all`'s
  early exit. The count of credentials a request carries is public and no timing claim lives here
  — that is `Token::verify`'s, note **N78** — but a loop that stops at the first *failure* is one
  rewrite away from stopping at the first *success*, which is the gate this entry refuses.

**Why: a first-wins gate and a last-wins gate are different gates, and the difference is somebody
else's to choose.** HTTP parameter pollution is a live technique precisely because the layers of a
stack disagree about duplicates — a proxy, a browser, a URL library and a server-side
deserializer each pick their own end of the string. A gate whose answer depends on that
convention is a gate whose answer depends on which piece of software parsed the URL last, and the
attacker's move is cheap, because getting one more `&token=…` appended to a URL an operator is
about to open needs no access to this daemon at all. **This gate has no convention to disagree
with**: if two `token=` parameters disagree, no answer is right, so the request is refused; if
they agree, whichever one any layer picks is the same verified token. The argument applies letter
for letter to a header beside a query parameter, and to two `Authorization` headers.

A credential that is *malformed* is counted as one that **failed**, not as none at all. A request
saying `Authorization: Basic …` has presented something, and answering it as though it had
presented nothing would let a request with a wrong credential and a right query parameter in
through a different path than the rule — which is the shape of every "fall back to the other
form" bug.

**The cost, stated rather than hidden.** A request carrying one good credential and one bad one is
refused. No client this project ships sends two, and a request presenting two different
credentials is a request whose author does not know what it is presenting — but a third-party
client that appended its own `?token=` to a URL that already carried one would meet a 401 it would
find surprising. That is the trade, and it errs closed, which is where D11 errs.

**What it does not claim.** Nothing about *authorization*: there is one token and it opens
everything the listener serves, because what it protects is a single-user machine's camera rather
than a multi-tenant surface (D11). Nothing about the layers underneath — the HTTP parser that
split these headers and the router that would have matched the path have timing of their own. And
**no percent-decoding and no `+`-for-space**: the token is 64 lowercase hex digits, every one of
them unreserved in a URL, so a decoder would only be a second way to spell the secret, in the one
place in this daemon where "more ways" is the wrong direction.

**Watched failing.** Each of the three distinguishing cases asserts with the *refused* gate named
in its own failure message — "a first-wins gate serves this request", "a last-wins gate serves
this request", "a header-wins gate serves this request" — and each is driven at both altitudes:
over a `HeaderMap` and a query string in `gate.rs`'s own suite, and over a real socket in
`crates/daemon/tests/http.rs`. Identical copies of one credential are asserted to be admitted, so
the rule is "every one must verify" rather than "more than one is suspicious".

**Retires when:** a client this project ships acquires a reason to send two different credentials
at once, which would turn the cost above from theoretical into real. It does **not** retire on a
client that sends one credential twice.

---

## N75 — In D11's token-less cell the gate is *absent*, not permissive, and it wraps the fallback so an anonymous 404 is a 401

**Doc:** D11's bind × token matrix has exactly one cell where TCP is served without a token
(loopback, behind `--http-insecure-loopback`). The natural reading of "the gate checks the token
unless the posture says otherwise" is one middleware, always installed, answering yes in that
cell.

**Repo:** `daemon::http::listener::router` decides at **composition**, in one `match` over the
posture. The three gated cells get `routes.layer(from_fn_with_state(token, gate::check))`; the
token-less cell gets `routes`, with no middleware at all. `daemon::http::gate` itself has no
branch that can admit a request which did not present the token. The two *disagreeing*
arrangements are refused rather than resolved, in both directions — a token-requiring posture
with nothing minted is a gate with nothing to check, and a token-less posture with a token minted
is a secret printed in a URL and never checked — and the same test asserts that the two agreeing
arrangements **do** get a router, so the refusals are about the disagreement rather than about a
function that refuses everything.

It is `Router::layer` and **not** `route_layer`. `layer` maps over `path_router`,
`fallback_router` and `catch_all_fallback`, so the gate covers a request for a path that does not
exist.

**Why the absence rather than a permissive gate.** A bypass branch inside the one function whose
entire job is to say no is where an inverted condition serves a live camera to anybody who asks.
Here the branch is at composition — read once at startup, over a value a reviewer can read —
rather than inside the hot path, and the thing that would have to go wrong to open the listener is
a whole `match` arm rather than a negation. The failure mode this removes is not hypothetical in
shape; it is the most common way an auth middleware ships open.

**Why the fallback is inside the wall.** `route_layer` would leave an anonymous request for
`/anything` answered by the 404 handler, which tells a stranger which paths this daemon has — the
surface, for free, to somebody who has not authenticated, on the transport that carries a camera.
So an anonymous request for a path this build does not serve is **401**, and the same path
presented with the token is **404**. Both directions are asserted, because the 401 alone would
also be true of a listener that had stopped serving anything.

**The cost.** A client that mistypes a path while authenticated gets a useful 404; one that
mistypes it while unauthenticated cannot tell "wrong path" from "wrong token". That asymmetry is
the point — the distinction is only useful to somebody who already holds the credential. The
refusal body is deliberately the same in both cases and carries no `error=` parameter beside its
`WWW-Authenticate: Bearer` challenge, for the same reason.

**Retires when:** never, as a rule. The mechanism retires if axum changes which routers `layer`
maps over, which is why `listener.rs`'s header names the three by name rather than saying
"everything".

### Amendment, 2026-08-12: the fallback is outside the wall now, and the first half is untouched

The owner ruled that static assets are served without authentication and that only the
resources which carry or drive the camera stay gated (note **N82**). This entry has two halves
and the ruling lands on exactly one of them.

**"The absence rather than a permissive gate" stands, unchanged and unweakened.** The
token-less cell still gets `routes` with no middleware at all, the decision is still one
`match` arm at composition over a value a reviewer can read, and `daemon::http::gate` still has
no branch that can admit a request which presented nothing. Nothing about the ruling touches
that argument, and the same test asserts the same four cells.

**"Why the fallback is inside the wall" is superseded, and the mechanism inverts with it.** The
gate is `Router::route_layer` now — the tool this entry named as the wrong one, for the reason
that made it wrong then and makes it right now: it maps over `path_router` alone, so the asset
fallback is outside the gate and the routes are inside it. An anonymous request for
`/nothing-here` is therefore **404 and not 401**, which is the property this entry argued for,
priced, and now gives up.

**What that property was worth, restated honestly, because the price was paid twice.** The
argument was that a 404 tells a stranger which paths this daemon has. That is true and it
stopped mattering: the paths are `crates/web/assets/` and two `pub const`s in a public
repository, so the surface it protected was already published, and what was left was the cost
this entry recorded — a client that mistypes a path while unauthenticated cannot tell "wrong
path" from "wrong token". The asymmetry is gone in both directions. The refusal body and the
absent `error=` parameter are unchanged, because those are about the *token* and not about the
path.

The mechanism's dependence on axum moves with it: what retires this half is a change to which
routers `route_layer` maps over, and `listener.rs`'s header names that one too.

---

## N76 — The token rides the URL, and a browser does not carry a query string to a document's subresources

**Doc:** D11 — the token is "generated per run and printed as a ready-to-open URL". Design §2.7 —
the web client is "Vanilla ES modules, no build step, no npm, no CDN (assets embed; external
fetches would violate both the offline posture and the license inventory)". Both sentences are
individually satisfiable, and **together they are a constraint neither of them names.**

**The constraint, which is a fact about browsers rather than about this code.** An operator opens
`http://127.0.0.1:34567/?token=<64 hex digits>`. The navigation carries the token, the gate admits
it, the page is served. The page then asks for its own subresources — a stylesheet, a module, an
icon — and a browser resolves those against the document's URL **without its query string**. So
`<link rel="stylesheet" href="app.css">` is fetched as `GET /app.css`: no query, no
`Authorization`, no credential in either form the gate reads. The gate refuses it, **and the gate
is right** — that request presented nothing (note N74). What an operator sees is a page that
renders unstyled in all three token-gated cells of D11's matrix, and a module that never runs.

**Repo, at P5a:** `webcam-handler-web`'s skeleton is **one self-contained file** whose styles are
inline. That is not tidiness, and it is not a placeholder — it is the only shape that works under
the constraint without deciding anything. Both `crates/web/src/lib.rs`'s header and
`daemon::http::listener`'s header carry the finding: once beside the asset crate that had to be
shaped around it, and once beside the gate that produces it.

**Why this cannot be left for P5c to discover.** §2.7's client is vanilla ES **modules**, which
are subresources by definition: a module graph is fetched by the browser, one request per module,
and no amount of inlining collapses it — inlining the entry point still leaves every `import` it
names as a credential-less `GET`. So the client P5c is commissioned to build cannot be built at
all until this is answered, and the worst moment to find that out is halfway through writing it.

**The two candidate answers. This entry deliberately chooses neither.**

1. **A cookie set on the gated navigation.** The document request carries the token and is
   admitted; its response sets a session cookie; the browser then attaches that cookie to every
   same-origin subresource automatically, module graph included. What it costs: a second
   credential shape for the gate to check, and therefore a second answer to note N74's question;
   decisions about `SameSite`, `HttpOnly` and `Secure` on a plain-HTTP loopback origin; and — the
   part that needs real thought — a credential the browser will now send on requests **the page
   did not make**, which is the ground CSRF lives on, against a daemon that drives a camera. What
   it buys: subresources need no code at all, and P5c writes ordinary `import` statements.
2. **A page that fetches its own modules with the `Authorization` header.** The entry point is
   inline script that reads the token out of `location.search` and fetches each module with an
   explicit header, instantiating the graph itself. What it costs: a hand-rolled module loader,
   which spends the "no build step" simplicity §2.7 exists to protect on precisely the wrong
   thing, and which interacts badly with `import` statements the browser would otherwise resolve
   natively. What it buys: the gate keeps exactly one credential model, and no credential is ever
   sent by the browser on its own initiative.

There is a third that is **not** a candidate, named here so it is not re-proposed: **putting the
token in every asset URL the page emits.** It works, and it writes the secret into the browser's
history, into referrer headers, and into any log the page's own requests reach — which is the cost
`ready_to_open_url` already accepts *once*, for one navigation, with its eyes open, and must not
accept N times per page load.

**What P5a must not do, and did not.** Ship a page whose stylesheet 401s and call the listener
finished. The listener is finished; the client's authentication is not, and the boundary is
written down here rather than left to whoever adds the second file to `assets/`.

**Retires when:** P5b or P5c makes the choice on purpose and records it — including, if it is the
cookie, what the gate now accepts and what that costs. Until then, a second file in `assets/` is
the signal that this entry was not read: `no-external-fetch-in-web.sh`'s population is one file
today, and every file added to it lands under this constraint.

### Retired by the owner's ruling of 2026-08-12 — the premise dissolved rather than the question answered

> "How about just exposing the ES module code without requiring any authentication? The modules
> aren't the secret — this software is open source anyway."

Neither candidate was chosen. The **constraint** was removed: static assets are served without
authentication, so a document's subresources present no credential and need none, and P5c's
module graph is ordinary `import` statements. Note **N82** is the ruling, what it cost and what
paid for it.

**It retires on its own condition, by a decider this entry did not anticipate.** The clause
above says "P5b or P5c makes the choice on purpose and records it", and the choosing was the
owner's rather than a sub-milestone's — which is the one authority above the clause. This is
worth spelling out because the file's rule is that an entry retires on empirical disproof: no
measurement here was wrong, and nothing above has been shown false. What went is the question.

**What survives, and is the reason the ruling is right rather than merely permitted.** The fact
this entry measured — a browser resolves a document's subresources against its URL *without the
query string*, so `<link href="app.css">` on a page opened at `/?token=…` is a credential-less
`GET /app.css` — is a fact about browsers and stands. It is now the argument for the ruling: a
gated asset table is a client that cannot load itself, and the two ways out were a credential
the browser attaches by itself (a cookie, on a daemon that drives a camera and answers
WebSocket handshakes that are not subject to CORS — `daemon::http::rpc`'s header keeps that
finding) or a hand-rolled module loader spending §2.7's "no build step" on the wrong thing.
Both were costs paid to protect a secret that does not exist.

**What stays refused.** The third answer this entry named as *not* a candidate — putting the
token in every asset URL the page emits — is still refused, and is now unreachable rather than
merely declined: an asset URL carries no credential at all, so there is no secret to write into
the browser's history N times per page load.

**One standing item this hands to P5c, in place of the constraint.** A second file in `assets/`
is now an ordinary file rather than a signal, but everything in that directory is served to
whoever can reach the port — including, in D11's two non-loopback cells, whoever can route to
it. `webcam-handler-web` embeds its **committed** files (note N77's `debug-embed`, asserted per
asset), so nothing about the machine `wchd` runs on can reach that table today, and anything
generated *would* be a route and therefore gated. The rule that keeps it that way: the client's
files say nothing about this host, and a page that had to would be a handler rather than a file.

---

## N77 — `debug-embed` is on, so a debug `wchd` does not serve the camera's control panel out of its author's source tree

**Doc:** design §2.7 — "no build step, no npm, no CDN (**assets embed**; external fetches would
violate both the offline posture and the license inventory)". "Assets embed" is the property the
design asks for; it does not say which `rust-embed` feature makes it true, and the default does
not.

**Repo:** `crates/web/Cargo.toml` enables `debug-embed`, and it is that crate's only feature.

**Why.** Without it, `rust-embed` embeds in **release** builds and reads `assets/` **from the
filesystem** in debug ones, at the absolute path the crate was compiled from. Three consequences,
and the third is what decided it.

1. A debug `wchd` serves the camera's control panel out of its author's source tree: it works on
   the machine that built it and serves nothing anywhere else.
2. **The traversal question reopens.** With the feature on, `web::get` is a lookup in a table of
   names fixed at compile time, so `../../etc/passwd` is not a traversal to be caught but a name
   that is not in the table — the question is closed by the shape rather than by a check somebody
   has to remember to keep. With it off, the lookup reaches a filesystem, and the daemon's single
   leading-slash strip becomes the only thing between a request path and a directory.
3. **Every test in `crates/daemon/tests/http.rs` would be exercising a different mechanism from
   the one a release binary ships.** In this project the suite *is* the argument, and a suite that
   proves a debug-only path proves nothing about the product. It is rubric rule 6's "the inverse
   arm is driven by the thing under test, not by a model of it", one layer out: the *whole* suite
   would have been driving a model.

**Asserted rather than configured.** `the_assets_are_embedded_rather_than_read_from_this_source_tree`
walks every asset and asserts its bytes are `Cow::Borrowed` — a slice of the binary — rather than
`Cow::Owned`, which is a `Vec` that was just read from a directory that may not exist on the
machine running `wchd`. Over **every** asset rather than a sample, because the feature is per-build
and one sample would pass on a tree where somebody had added a file the walk could not reach.

**The residual, stated rather than hidden:** editing `assets/` now needs a rebuild before `wchd`
serves the change, because the bytes are in the binary. That is the cost of the property, it is
one `cargo run` away, and P5c's client is developed against a daemon that is rebuilt anyway.

**Adjacent — the rest of the feature posture, declined with reasons rather than by omission.**
rust-embed 8 declares no `default` feature, so the one line in the manifest is the whole posture
rather than half of it, and there is no `default-features = false` beside it because there is
nothing to turn off. Left off: `axum-ex` and the other four framework integrations, which would
give this crate its own axum and tokio edges — the edge `dependency-walls.sh`'s asset wall now
makes impossible rather than merely argued against; `compression`, which keeps a second copy of
every asset behind a decompression edge, for a page measured in kilobytes; `mime-guess`, a thousand-entry
MIME table where four extensions plus a test that walks the embedded assets is the forcing
function the table alone would not be; `interpolate-folder-path`, which lets an environment
variable decide what gets embedded and is the exact opposite of the property above; and
`include-exclude` and `deterministic-timestamps`, which have nothing to say about a directory this
crate owns outright. (`mime_guess` still appears in `Cargo.lock`: it is a non-optional dependency
of `rust-embed-impl`, which is a **proc macro**, so it runs at compile time on the host and is not
linked into `wchd`.)

**Retires when:** rust-embed changes its debug behaviour, or the client acquires a development
mode that deliberately serves from disk — which would need a note of its own, because it would
reintroduce all three consequences above on purpose.

---

## N78 — `==` for `Token::verify` survives every test in this workspace, so the answer is a gate over the shape rather than a test over the clock

**Doc:** AGENTS rule **1** — "every anticipated or discovered defect class becomes a lint, a CI
job, or a test that can go red" — and rule **2**, both directions. Design §2.10, one home per law.
D11 makes the bearer token the whole of the TCP transport's auth model.

**The finding.** P5a's second commit applied sixteen mutants to `daemon::http` by hand; fourteen
went red. One of the two survivors is this entry's subject: replacing `Token::verify`'s
constant-time body with `self.expose_secret() == presented` **leaves every test in this workspace
passing.** It is not a gap a better test would close. The two implementations answer identically
for every input; the only thing that changes is how long the daemon takes to say no, and a timing
assertion would be a benchmark pretending to be a test — on a shared runner it would be a flake,
and rubric rule 2's other half is that an assertion which cannot go red is worse than an argument
that admits it is one. `token.rs`'s own doc had already written that down, as a limit rather than
as a promise, which is what made the survivor a commissioning note rather than a surprise.

**Repo:** `scripts/gates/token-comparison-has-one-home.sh` — the twenty-third predicate the suite
has, with sixteen fail arms and two green ones. It cannot measure a clock, so it holds the
**shape** the timing argument stands on, in four claims:

1. **The secret has one reader.** `Token::expose_secret` is the one rendering that yields it, and
   outside `token.rs` the accessor appears only inside `#[cfg(test)]`. The token gate is what this
   is chiefly about: a comparison written in `gate.rs` needs the secret in hand, so `expose_secret`
   in that file's product code is the defect arriving, spelled out, one line before the `==`.
2. **The type refuses `==`.** No `PartialEq`, `Eq` or `Hash` for `Token` anywhere in the workspace,
   derived or hand-written, and no `Display` — `str`'s `PartialEq` compares lengths and then bytes
   and returns at the first difference, and a `Display` is the other way a secret reaches a
   comparison or a log line without anybody typing the word `expose`.
3. **`Debug` is hand-written and names no field**, with `Token`'s field names read out of the
   declaration rather than typed into the predicate, so a renamed field is still a field this
   refuses to see printed. Half of this claim is the gate's and half is a test's: *what* the
   hand-written impl prints is asserted by
   `the_debug_rendering_redacts_the_secret_and_the_named_rendering_yields_it`, which can go red,
   and what the gate adds is the half that test cannot see — a derive would satisfy nothing there
   and would still have to be caught before it printed.
4. **Something still compares.** `verify` is declared in the home and `gate.rs`'s product code
   calls it. Every claim above is true of a tree where the gate stopped checking the token, which
   is a worse defect than any of them; this is `kill-is-never-a-fallback.sh`'s "the only caller
   went away" arm, about a different absence.

**Two decisions inside the predicate worth keeping.** Line comments are stripped before matching,
so **prose does not count** — the opposite of `kill-is-never-a-fallback.sh`'s choice, and
deliberately, because what defends the timing claim *is* the argument beside the code, `token.rs`
names the accessor in prose while making that argument, and a gate that turned writing about the
secret into a violation would push the argument out of the modules that need it. And a file whose
product/test boundary the predicate cannot read — two `#[cfg(test)]` markers, or a marker that
opens something other than a `mod` — is a **failure and not a pass**, which is `unsafe-scope.sh`'s
price for a count it cannot read, charged here for a boundary it cannot find.

**What it does not claim, and this is the entry's honest half.** It checks shape, and shape is not
timing. **An early `return false` written *inside* `verify` passes all four claims**, because from
outside it is the same function with the same name called from the same place. Nothing in this
suite can go red on that. What defends it is the argument beside the code — `verify`'s doc states
the claim, states that the length is deliberately public and why, and states that Rust makes no
*guarantee* the accumulate-then-compare shape compiles without branches — and the person reading
the diff. Saying so is better than a green that implies more.

**The other survivor from the same pass, so the arithmetic is complete.** The `Serving` join in
`main.rs`. No suite can reach it, because `main.rs` is a binary an integration test cannot call.
It is mitigated rather than killed: the composition moved into `daemon::http::open`, which the
suite *does* drive, and `Serving` is `#[must_use]` with a message that makes the naive drop a
build failure rather than a detached task. The join statement itself stays unproven, which is the
honest state of it.

**A third mutant survived the first run and was killed rather than accepted**, and it is recorded
because it is the ordinary outcome this entry's subject was denied: `starts_with` for `verify`,
which serves any **prefix** of the token. Every negative case in the suite was equal-length, so
nothing noticed. It now has a test at both altitudes — a truncated candidate in `gate.rs`'s suite,
and a truncated token in the query string over a real socket.

**Retires when:** the workspace adopts a constant-time comparison crate whose type makes `==`
unavailable by construction, at which point claims 1–3 become the compiler's and only claim 4 has
work left to do. It does **not** retire on a `verify` that looks obviously fine: the whole finding
is that looking fine and being fine are indistinguishable to this suite.

---

## N79 — D11's "unless configured" has no surface, and `Token` has one constructor

**Doc:** D11, in the sentence that commissions the token: TCP "requires a bearer token:
**generated per run and printed as a ready-to-open URL unless configured**". The clause promises
an operator some way to supply a token of their own; otherwise it describes nothing.

**Repo:** there is no such way. `daemon::http::token::Token` has a single constructor, `mint`,
which reads 32 bytes from `getrandom(2)`. `wchd` has two web flags, `--http` and
`--http-insecure-loopback`, and neither carries a token. Nothing reads an environment variable or
a file for one. Every run of `wchd --http` prints a fresh URL with a fresh secret in it.

**This entry does not invent the flag.** P5a's remit was the listener and the gate, and adding a
configuration surface for a credential is a security decision with a shape of its own: where the
value comes from (argument, environment, file), what happens when it is too short or not hex,
whether a configured token suppresses the printed URL, and whether an operator supplying one
across restarts is a feature or a way to leave a long-lived camera credential in a shell history.
None of that is a line of code, and all of it is D11's or a later sub-milestone's.

**Which of the two readings this project thinks is right**, because an open question with no
opinion in it is a question nobody closes. **The clause is a dangling promise, and the honest
repair is to strike it from D11 rather than to build a flag for it.** Three reasons, in order of
weight.

1. **The token's value is that it is per-run, and "configured" is the negation of that.**
   `ready_to_open_url`'s doc accepts a real cost — a secret in the browser's history, in omnibox
   suggestions, and in the terminal scrollback of whoever started the daemon — and prices it
   explicitly against the token's lifetime: one run of one daemon, "which is the reason D11 makes
   it per-run rather than persisted". A configured token is by construction not per-run. It
   survives restarts, it lives wherever it was configured, and it turns every one of those
   exposures from bounded into permanent. The clause and the argument in the same paragraph pull
   against each other, and the argument is the one carrying weight.
2. **Nothing in the plan consumes it.** P5b, P5c and P5d are a WebSocket, a client and a browser
   rung, and each is handed the token by the page it is already running in. No reader is
   scheduled, and a flag with no reader is the rubric A8 shape this project refuses everywhere
   else — the same test `wchd` applies to `tower-http`, which is *not* in its manifest yet because
   the thing that reads it is P5b's.
3. **The use it would serve already has a better answer.** The case for a fixed token is
   scripting: something automating `wchd` that cannot read a startup line. But that consumer's
   transport is the **Unix socket**, which is always served, whose auth model is the filesystem
   (D11's own first sentence), and for which `wchc` exists. A configured TCP token would be a
   second and weaker path to a surface that already has one, which is what §2.10 is about.

**What would change this ruling:** a named consumer that must reach the daemon over TCP, cannot
read its startup output, and cannot use the Unix socket. That is a real shape — a container that
publishes a port, a reverse proxy in front of the daemon — and if one is commissioned then the
clause is a promise to keep rather than to strike, and keeping it needs the four decisions listed
above rather than a `--http-token` bolted on.

**Retires when:** D11 is amended in one direction or the other. Until then this entry is what
stops the clause being read as a feature that exists, and what stops a review reporting its
absence as an oversight rather than as an open question against the design.

---

## N80 — Two answers `daemon::http::posture` gives that look wrong from outside and are deliberate

**Doc:** D11's bind × token matrix, and its "additionally prints a warning naming what it exposes
(a live camera)"; docs/7 P5a repeats the warning requirement.

**Why one entry rather than two.** Both are answers `Posture` gives *about an address*; both look
like defects to a reader comparing the module against the machine it runs on; and both come from
one principle — **the posture describes what the operator typed and what the kernel actually
routes, never an address this module could have invented.** Filing them apart would put one
review-bait pair in two places, and a review that re-reports either is making the same mistake
twice.

### 1. `Posture::warning()` names the **requested** address, not the bound one

**Repo:** `Posture` is decided from the address `--http` was given, before anything is bound, and
it carries that address. So `wchd --http 0.0.0.0:0` warns about `0.0.0.0:0` while the URL printed
by the same startup says `http://0.0.0.0:34567/…` — two lines, two ports, one of them a zero.

**Why it is left alone.** The warning is about **reach**, and reach is a property of the address
rather than of the port: `0.0.0.0` is what makes a live camera reachable from every interface this
host has, and which port the kernel happened to pick changes nothing about that. Echoing what the
operator typed is also what makes the line findable — somebody who bound a daemon an hour ago is
looking for the string they wrote. And the alternative is worse in a specific, structural way:
`Posture` would have to be constructed *after* the bind in order to know the port, which is
exactly the ordering this module exists to avoid, since deciding the security posture once the
socket is already open is deciding it too late.

The port is not lost. `Serving::ready_to_open_url` is built from `local_addr()` and nothing else,
so the line an operator copies carries the bound port, and `Serving::bound()` is the accessor for
it. The two lines say two different things on purpose: one is "this is what you exposed", the
other is "this is where to open it".

### 2. The deprecated IPv4-**compatible** form is not unwrapped, so `::127.0.0.1` is not loopback

**Repo:** `Reach::of` unwraps IPv4-**mapped** addresses with `to_ipv4_mapped` and asks the IPv4
question of what it finds. That closes the real hole, since `Ipv6Addr::is_loopback` is `::1` and
nothing else and answers **false** for `::ffff:127.0.0.1` — the form a dual-stack listener sees for
every IPv4 client, and the one whose misclassification would fire D11's warning on the default
case, which is how operators learn to ignore a warning. `to_ipv4_mapped` answers `None` for the
IPv4-compatible form (`::127.0.0.1`, no `ffff`), so that address is classified `BeyondLoopback`.

**Why it is left alone.** RFC 4291 deprecated the IPv4-compatible format, Linux does not route it,
and `to_ipv4_mapped` declines it by design. Treating it as loopback would be this module inventing
a loopback address the kernel does not have. Declining it errs **closed** — the token is required
and a warning is printed, for an address nothing can reach anyway — which is the direction D11
chose for everything else in the paragraph. An operator who somehow bound it meets a stricter
posture than they expected, never a weaker one.

**What neither of these is: an untested corner.** The warning's contents are asserted by name in
`posture.rs` (it names the bind address and it names the camera, and in the fourth cell it names
the flag that did nothing and says it did nothing) and again over a real socket in
`crates/daemon/tests/http.rs`. The mapped form has a test of its own which additionally asserts
that `std` still answers `false` for `::ffff:127.0.0.1`, so the day `std` changes its mind the test
says its subject has moved rather than quietly passing.

**Retires when:** for (1), a reason appears to prefer the bound address in the warning — which
would mean deciding the posture after the bind, and would need `posture.rs`'s header answered
first. For (2), Linux starts routing IPv4-compatible addresses.

---

## N81 — The web listener has no accept-failure policy and no bound on an in-flight response, deliberately unlike the Unix socket beside it

**Doc:** AGENTS — "Bounded everything: settle deadlines, sweep caps, recording caps, channel
depths, shutdown drains". Design §2.6 — "an open MJPEG tab must not hang shutdown".
`daemon::uds::serve` gives up after `schema::limits::MAX_CONSECUTIVE_ACCEPT_FAILURES` consecutive
accept failures, and that refusal reaches `main`'s exit code.

**Repo:** `daemon::http::serve` does neither. A fatal accept error ends the server task, which says
so at `error!` at the instant it happens and does not reach the process's exit code; and the
graceful stop puts no bound on a response that is already being written.

**Why the accept policy differs from the transport one module along.** The Unix socket is the
daemon's **always-on** transport, and a daemon that has stopped accepting on it has stopped being a
daemon — which is why its give-up becomes an exit code and therefore a systemd `Restart=on-failure`.
The TCP listener is opt-in, the Unix socket is unaffected by anything that happens to it, and
axum's own accept loop backs off and retries. Making the two match would mean this daemon exits
non-zero, and asks a service manager to restart it, because a browser transport it was asked to add
as an extra went away — taking the working transport down with it. So the difference is stated
rather than removed, and the failure is reported **when it happens** rather than at the next
teardown, because a listener that stopped accepting at 03:00 must not first be mentioned by a stop
at 09:00.

**Why the response bound is absent, and what makes that affordable at P5a.** Everything this
sub-milestone serves is a file of a few kilobytes, so the graceful stop is bounded by a `write` to
a socket — a property of *what is served*, not a guarantee this module provides, and
`listener.rs`'s header says exactly that. The response that does not end on its own is **P5b's
MJPEG preview** (`multipart/x-mixed-replace`, which by construction runs until the client goes
away), and that is where §2.6's requirement becomes a thing to build rather than a thing to
inherit: meeting it needs the preview's own stream to watch the cancellation token. A bound written
now would be a bound with nothing to bound (rubric A8), written against a guess at the shape of a
stream that does not exist yet.

**What *is* claimed, so the absence above is not read as an absence of lifecycle.** The listener
watches the daemon's one `Shutdown` token — the same clone the subscriptions and the idle-sweep
driver watch — so it begins stopping at **step 3** of `crate::shutdown`'s order rather than at a
step of its own; `axum::serve(..).with_graceful_shutdown(..)` is how that token reaches hyper. And
`Serving::stopped` is **joined** by the composition root, so "the web listener ended" is a fact the
process waited for rather than a consequence of the runtime being dropped at the end of `main`.
`Serving` is `#[must_use]` with a message naming that, because a dropped handle drops the
`JoinHandle` and detaches the task — and that is the one mitigation the unreachable `main.rs` join
mutant has (note **N78**).

**Retires when:** P5b lands the preview and with it the bound §2.6 requires, at which point this
entry's second half becomes a statement about a past tree and the first half stands alone. The
accept-policy difference does not retire while the web listener is opt-in.

---

## N82 — The token is for the camera, not for the client's own source, and "every route is gated" stopped being a property of the composition

**The ruling (owner, 2026-08-12), verbatim:**

> "How about just exposing the ES module code without requiring any authentication? The modules
> aren't the secret — this software is open source anyway. The only resources that need
> authentication are the WS that talks to the daemon and the camera images."

**Doc:** design **D11**, whose sentence made the bearer token a property of the *transport* —
TCP "serves the web client (static assets + WS JSON-RPC + MJPEG preview `<img>` endpoint), and
requires a bearer token". D11 now carries a dated amendment: the token is required for the two
resources that carry or drive the camera and not for the static assets. The bind × token matrix
and the sentence that justifies it — "a camera is a privacy-sensitive device; the daemon's
exposure posture errs closed" — are unchanged, and now apply precisely to the two routes that
*are* the camera.

**Repo:** one word in `daemon::http::listener::router`. `Router::layer` became
`Router::route_layer`, which maps over `path_router` and neither fallback, so the gate covers
`/rpc` and `/preview` and the asset fallback is outside it. Beside it, one layer that was not
there: every response carries `Referrer-Policy: no-referrer`. **The gate itself did not change**
— not one line of `daemon::http::gate`'s code, only the paragraphs that described what it was
installed over — and that is the shape of the thing: the credential model is N74's, the
token-less cell's absent middleware is N75's, and no cookie is read, written or accepted
anywhere in this daemon. A ruling implemented as an `if request.uri().path() == …` inside
`check` would have put the whole of it in the one function whose job is to say no.

### What it cost, which is the whole of why this entry is long

**"Every route is gated" was true by construction and is now true by list.** `Router::layer`
wrapped the routes, the fallback router and the catch-all, so a request could not reach a
handler without meeting the gate; there was nothing to keep current and nothing to forget.
After the ruling, gating is a property of *where a route is registered*: `route_layer` wraps
the routes that exist when it is called, and a route merged after that line — P5c's, P6's,
anybody's — is served open. A camera-bearing route added that way is a live camera served to
strangers, and it would be **green in this workspace**, because a test can only ask for a path
somebody named.

That is a defect class this change created. AGENTS rule 1 has no exemption for one that arrives
with a ruling, so it becomes two things that can go red, and neither implies the other:

1. **`scripts/gates/web-routes-are-gated.sh`** — the structural half, and the 24th predicate.
   Its claims: `CAMERA_BEARING_PATHS` exists, is non-empty and names *constants* rather than
   literals, each declared in the daemon's `http` tree; every `.route(`/`.route_service(`/
   `.nest(`/`.nest_service(` in the workspace's product code passes one of those constants,
   from inside those modules; `.fallback(`/`.fallback_service(` appears once, in the
   composition, naming the asset handler — the door claim 2 does not watch, since a fallback is
   what this listener now serves *without* the token; and the composition still installs
   `gate::check` exactly once with exactly one `route_layer(`, because every claim above is true
   both of a tree that stopped gating anything and of a tree that put the gate back over
   everything. Sixteen fail arms and four green ones — the shipped tree, a comment that names
   a route without registering one, a test building a router of its own, and a declaration
   rustfmt wrapped onto two lines, which is a formatting event and must never be a finding.
2. **`every_camera_bearing_route_is_behind_the_gate`** (`crates/daemon/tests/preview.rs`) — the
   behavioural half, driven over a real socket against a real camera, four claims per path:
   nothing is `401` with RFC 6750's challenge; a near miss is `401` in **both** credential
   forms, so the gate is comparing rather than looking for a parameter; the token gets past to
   an answer that is neither the gate's `401` nor the asset table's `404`; and the population is
   not empty. Beside them, the ruling's own requirement in the same run: `GET /` with no
   credential is the page.

**Together they are a partition and that is the point.** The gate says every route is named; the
test says every name is gated. A route added without a name fails the first; a name that lost
its gate, or that never had a route, fails the second.

### "Carries or drives the camera", as something a predicate can decide

The phrase is a reason, not a test — nothing can read a handler and know whether a camera is
behind it. What this repository encodes is the decidable question that is exact today and errs
closed tomorrow: **a route is gated; the only thing served without the token is a lookup in the
embedded asset table.** The only reason this listener has a route at all is the camera — `/rpc`
drives one, `/preview` carries one — and everything else it serves is a file compiled into the
binary from a committed directory (note **N77**). A future route with no camera in it is red at
the gate above, deliberately: that is where the argument has to be made, in a diff, rather than
in a router.

### `Referrer-Policy: no-referrer`, which is part of the ruling rather than of its implementation

The token rides the document's URL, so the URL a browser holds for this page **is** the key to a
camera, and a `Referer` header is that URL handed to whatever the page links out to. Same-origin
leakage back to the daemon is harmless — the credential came from there. A link out is not. It
is applied outermost, over both halves of the router, so it lands on the page, the assets, the
preview's frames, the `404` and the gate's own `401`; a header stamped on the half somebody
remembered would be the second list this composition spends its argument refusing to keep. The
page P5a ships has no links; P5c's may, and that is the wrong order in which to discover a
header. `Token::ready_to_open_url`'s doc priced three exposures of the URL form and now records
this one as **closed** rather than accepted.

### Four consequences, stated because they are consequences rather than intentions

1. **An anonymous request for an unknown path is `404` and not `401`.** That was note N75's
   deliberate property — a stranger cannot map the surface — and it is retired in that entry's
   own amendment. The surface it protected is a directory in a public repository, so what it
   bought was already published; what it cost (an unauthenticated client cannot tell "wrong
   path" from "wrong token") is gone with it.
2. **The pre-authentication attack surface widened**, and this is the consequence with the most
   in it. Before the ruling an unauthenticated request reached the token gate and nothing else.
   It now reaches the router, the asset lookup, one allocation per request, and the compression
   layer. That is three pure functions and a table fixed at compile time, and the traversal
   question stays closed by shape rather than by a check (note **N77** again) — but it is a
   widening, and on D11's two non-loopback cells the reachable-from set is "whoever can route to
   the port" rather than "whoever is on this host".
3. **On a non-loopback bind, the client page is now served to that whole network.** D11 warns
   there that what is exposed is "a live camera"; the camera is still behind the token and the
   *page* is not. An operator who read the warning as "nothing here answers without the token"
   will find otherwise. This is the sentence in this entry most worth disagreeing with, so it is
   the one written plainest: nothing in `assets/` says anything about the host it is served
   from, and anything that had to would be a handler — a route — and therefore gated.
4. **docs/7 P5d's Playwright criterion changes meaning.** "Anonymous requests are refused" was
   written when every request was gated; a browser rung asserting it of `/` would now assert
   something false, and one asserting it of the preview and the WebSocket asserts what the
   ruling actually protects. That line is amended in the plan rather than left for P5d to
   discover halfway through a Chromium suite.

### What this retires and what it does not

It retires note **N76** — the two candidate answers for authenticating the client's ES modules —
by dissolving the premise rather than choosing between them; that entry carries the retirement
and what survives of it. It amends note **N75**'s second half (the fallback inside the wall) and
leaves its first half (absent rather than permissive) exactly as written. It touches **N74** not
at all: the rule is still that every credential a request presents must verify and it must
present at least one, in both forms, on the routes where a credential is asked for. **N78**'s
gate is unaffected — the token still has one comparison and one reader.

**Retires when:** never, as a ruling. The mechanism retires if axum changes which routers
`route_layer` maps over — the same standing risk N75 recorded about `layer`, moved one method
along — or if a route this listener serves acquires a genuine reason to be ungated, at which
point `web-routes-are-gated.sh` is the file where that reason has to be written down.

---

## N83 — A photo suspends a live preview instead of being refused by it, and the sequence is one operation on the thread that owns the device

**The ruling (owner, 2026-08-12):**

> "Build suspend/resume in the engine. The photo command pauses the preview stream, takes the
> frame, and resumes it, all inside the actor that already owns the device — so exclusivity
> stays enforced by construction and no client has to sequence anything. The pause window drops
> preview frames, which the watch channel already models. `wch` and `wchc` get it too, because
> it lives below the wire."

**Doc:** design **D12**, which now carries a dated amendment beside the sentence it does not
contradict. Exclusive streaming is still the rule and is still the actor's; what changed is
that it is enforced by a *sequence* rather than by a refusal.

### What the old behaviour was, and why it was pinned

V4L2 allows one streamer per node. `engine::capture::grab` starts a stream of its own, so a
`wch_photo` taken while a preview held the stream met `Error::Busy` **from the device** — E3's
distinction exactly (availability is not capability), and the same refusal a second application
on the host would have met. P5b's preview half reported that honestly rather than building
ahead of the plan: `engine::preview`'s header named the fix, declined it on rubric A8 ("a
mechanism built before the case it serves is a bound with nothing to bound"), and
`crates/daemon/tests/preview.rs`'s
`a_photo_while_a_preview_is_running_is_refused_by_the_device_and_the_preview_survives` pinned
the refusal so that the day it changed, something went red.

**It went red on purpose.** That is the case this file's retirement rule is for: an entry
retires on empirical disproof *or* an owner ruling, and nothing above was measured wrong — the
declining clause was "there is no client that trips it yet", and docs/7 P5c's photo trigger is
that client. The test is replaced rather than deleted, by four in the same file, and the two
alternatives the owner rejected are recorded here so they are not re-proposed: surfacing `Busy`
in the UI with a "stop the preview" affordance, and having the client tear its preview down and
re-open it — which puts device-exclusivity sequencing in JavaScript, the layer least able to
enforce it, and fails outright with a second tab previewing.

### Where the sequence lives, and the two guesses the old header made about that

`engine::preview::while_suspended`, called by `engine::photo::take` around **its capture and
nothing else**. Three properties, and each is a decision:

1. **It is one function that cannot be half-used.** There is no public `suspend` and no public
   `resume`: a suspend any caller can invoke separately is a suspend somebody forgets to
   resume, and what they would forget is a camera left dark for a tab that is still open. The
   only way to reach the stop is to supply the work.
2. **It is indivisible because of where it runs, not because of what it locks.** `photo::take`
   runs inside one `engine::actor` command on the thread that owns the `Box<dyn Camera>`, and
   that thread takes one command at a time in arrival order — so a second photo, a preview turn
   and a control write all queue behind it by construction. Nothing here takes a lock and there
   is nothing to take one on.
3. **It is inside the photo pipeline rather than beside it**, so `wch`, `wchd` and any later
   caller get the same behaviour without arranging anything (§2.10: one home per law).

The old header guessed at two things and was wrong about both, which is worth recording because
both guesses put the mechanism one layer above the fact:

- "needs a suspend protocol on the daemon's feed" — it needs nothing on the feed.
  `schema::backend::Camera::streaming` asks the **device** whether it is streaming (AGENTS rule
  4), which is the only answer that cannot drift from the ioctl; consulting `daemon::preview`'s
  registry would have been a question one layer above its answer and one race away from it.
- "a photo path that knows previews exist" — the photo path knows only that *something* was
  streaming when its command began, and inside one actor there is exactly one thing that can
  be: a preview, because every other stream in this engine starts and stops inside a single
  command (`capture::grab`'s `StreamGuard`). The invariant does the knowing; no caller passes a
  flag.

### Only the capture is inside the window

The sink check, the control read, the render, the EXIF stamp and the file write are all outside
it. None of them touches the stream, and a preview held down for the length of a 4K PNG
re-encode and a write to a slow disk would be a pause bounded by the filesystem. What is inside
is `capture::grab`, whose length is the settle deadline the request carries — which is the
number the bound is checked against.

### The bound, and why it is a budget rather than a measurement

`limits::PREVIEW_SUSPEND_MAX_MS` (ten seconds), read by `while_suspended` and by nothing else.
Nothing can interrupt a `DQBUF` the actor's thread is already inside — the engine has no
runtime and there is no thread left to notice — so a limit consulted *after* the work would be
a number in a log wearing a bound's name. What is checked is the request's own settle deadline,
**before** the stream is stopped, so a photo too expensive to serve costs the viewers nothing at
all; it is refused with `Error::IllegalTransition`, which is D13's existing "the state you are
in forbids this operation as asked" and leaves the registry closed at eighteen. The constant is
asserted against `DEFAULT_SETTLE_DEADLINE_MS + FRAME_DEADLINE_MS` where it is declared, because
a bound that refused the *default* photo would be a mechanism that only ever fires on the case
it was built for.

### What happens when the capture fails, and when the resume does

The stream comes back. A photo that errored and left the preview dead would be AGENTS rule 8
broken by the code that exists to honour it, so the restart is on every path out of the work —
the error path, and the unwinding path a panicking backend produces \[PF:1\], which is why the
resume is owned by a drop guard (`Resuming`) rather than written twice. It is `capture`'s
`StreamGuard` mirrored: that one stops a stream its scope started, this one starts a stream its
scope stopped.

**A resume that itself fails has no good answer, and the least bad one is split.** Returning it
to the photo's caller would throw away a picture that cannot be retaken — the frame is gone —
and returning `Ok` in silence would leave a `watch` channel nobody will ever publish to again.
So: the work's answer is the answer (`StreamGuard`'s doctrine — "the caller is already holding
either a frame or the error that matters"), and the refusal rides beside it in
`engine::preview::Gap::resumed`, where `daemon::preview::interrupted` counts it and logs one
`warn` naming the camera and the device's own words. The feed is **not** withdrawn there: its
driver's next turn asks a camera that is no longer streaming for a frame, gets the device's
refusal and takes the path this module already has for a device that refused — `Ended::Refused`,
the feed withdrawn, its readers' streams ended — within one `limits::PREVIEW_FRAME_WAIT_MS`. A
second withdrawal path would be a second home for "how a preview ends" (§2.10) rather than a
faster answer.

### What a viewer sees, counted rather than silent

The pause is invisible to every counter this daemon already keeps, and that is the trap it was
worth building an observable for: **nothing is published during it, so nothing is dropped**.
`Fanout::published` stops and continues; no reader falls behind; `Fanout::skipped` — which
counts frames a *reader* missed — must not move, because folding a suspension into it would
tell an operator their socket was slow when their camera was busy being a camera (E3's habit,
applied to a count instead of to an error). So the pause has a count of its own,
`Previews::watch_interrupted`, bumped on **both** photo paths: `engine::photo::Taken` carries
the gap beside the outcome precisely so that a capture which *failed* still reports the
interruption it caused.

On the wire a client sees three things: frames stop and start; `X-Wch-Frame-Index` gets no hole
from the pause (a jump there is still the reader's own slowness, which is what that field has
always meant); and `X-Wch-Frame-Sequence` **starts over at zero**, because `STREAMON` resets the
driver's counter. That reset is the only signal a client can read the restart off, and it is
passed through exactly as the device reported it — that field's contract is that it is the
device's number (D5), and rewriting it into a continuation would make a restart
indistinguishable from a device that never stopped. What no client can conclude is *why* the
stream restarted; from a viewer's side "a photo was taken" and "the device restarted" are the
same event, and the daemon's own count is where the difference is recorded.

### What this does not cover, deliberately

**A calibration sweep still meets `Busy`.** A sweep is minutes of photos, so suspending a
preview for one would be a preview that is *off* rather than paused — and the bound that makes
the photo's pause defensible is exactly what a sweep cannot fit inside. `engine::calibrate`
reaches `capture::grab` and `photo::from_capture` directly rather than `photo::take`, so it does
not acquire the mechanism by accident; that is a property of where the sequence was placed, and
it is the reason this entry records the placement rather than only the behaviour.

**`wch` gets the mechanism and can never exercise it.** It links the engine directly, opens a
camera per invocation, takes one photo and closes it, so nothing in that process is ever
previewing and `Camera::streaming` always answers `None` — the ruling's "`wch` … gets it too" is
true and vacuous. `wchc` genuinely gets it for free: it links no engine and asks the daemon,
whose answer changed. Neither binary needed a line.

### What can go red

- `crates/engine`'s `preview::tests` — seven, over the scriptable double: the sequence with a
  real `capture::grab` inside it (the double refuses a second `STREAMON` exactly as V4L2 does,
  so a build that skipped the suspend fails there with `Busy` rather than passing a test that
  counted stops); the resume asking for the *suspended* stream field for field rather than for
  a fresh negotiation; the error path; the panic path; a resume the device refuses; the bound in
  both directions; and a capture with no preview touching no stream at all.
- `crates/engine`'s `photo::tests` — the same sequence through the verb, over the fake replaying
  a committed profile: the photo is still **verbatim**, the resumed stream's first frame is
  sequence 0 again, and a settle budget past the bound is refused with the preview left running.
- `crates/daemon/tests/preview.rs` — four over a real socket, replacing the retired one: a photo
  during a preview with the interruption *count* read rather than inferred; a photo with no
  preview, unchanged; a capture that fails mid-photo with the preview still streaming; and two
  tabs with a photo between them, still one streamer.

**One instrument choice worth recording**, because the obvious one is wrong: the daemon-level
failure is driven by a settle deadline of zero rather than by `Fault::FrameTimeout` or
`Fault::DeviceGoneMidStream`. Those are consumed by whichever `next_frame` reaches them first,
and while a preview is running there is a `next_frame` in flight continuously — so a queued
frame fault there is a race between the preview's turn and the photo's capture. The fault-menu
injection lives in the engine suite, where the double is exclusive and the injection is exact
(`ScriptedCamera::starts_refused_after`, which is new and is what makes a failing `STREAMON`
reachable at all).

**Retires when:** never, as a ruling. The *mechanism* is up for revision if a caller appears
that needs a stream held across more than one capture — a sweep beside a live preview is the
obvious one, and it is a different question rather than a longer bound, because the answer there
is either an unbounded pause or a preview that is told to stop. `PREVIEW_SUSPEND_MAX_MS` moves
if a real camera is measured needing longer than ten seconds to settle, which would be a PF
entry rather than a preference.

---

## PF:24 — An INACTIVE control's current value is the automation's, and `VOLATILE` is not how a device says so

**Measured** 2026-08-12 on kernel `7.0.0-29-generic` (x86_64), five cameras attached, against
the **Logitech BRIO** (`046d:085e`, interface `2-3.4.2.4:1.0`) the day it was attached, with the
other four cameras as the control group. Continues the docs/6 §1.2 registry; cite it as
`[PF:24]`.

PF:3 measured that INACTIVE tracks auto/manual pairing live and in both directions. It says
nothing about the *value* underneath the flag, and this workspace inherited an assumption there:
`engine::snapshot` records `was_volatile` from `V4L2_CTRL_FLAG_VOLATILE` and every "the camera is
back where we found it" comparison exempts exactly that set and nothing else. On this device the
exemption misses, because the device does not set the flag on a control whose value it writes
itself.

### The measurement

Read with `wch get <cam> white_balance_temperature --json`, moved with
`wch photo <cam> -o <scratch>`, switched with `wch set <cam> white_balance_automatic=0|1 --json`
— the shipped binary throughout, nothing bespoke.

`white_balance_temperature` on `cam:logitech-brio`: range `2000..=7500` step 10, flag word
**`0x1010`** — `INACTIVE | HAS_WHICH_MIN_MAX`. `VOLATILE` is `0x0080` and **is not set**.

Idle, with `white_balance_automatic=1` as found, eight consecutive `wch get` reads inside 0.1 s
all answered `3620`. The value moves when the sensor *streams* — one `wch photo` between reads:

```
wbt = 3620 → photo → 3680 → photo → 3650 → photo → 3630
```

The inverse arm is the half that makes it a finding rather than a noisy control. With
`white_balance_automatic=0`, which clears INACTIVE (flag word `0x1000`), the same three photos:

```
wbt = 3630 → photo → 3630 → photo → 3630 → photo → 3630
```

Restoring `white_balance_automatic=1` puts INACTIVE back and the drift with it. The control
group, three photos each, AWB on: **Chicony RGB held `4600` and the Dell U3224KB/A held `5000`**,
unmoved by their own captures. So the drift is this device's, not the class's — which is the
whole reason it is a PF entry: the flag word is identical on all four cameras and the behaviour
underneath it is not.

**The same class, on the OBSBOT, arriving by a different route \[PF:25\].** After a `uvcvideo`
cycle that device's `red_balance`, `blue_balance` and `white_balance_temperature` — the three
controls `white_balance_automatic` owns — all read `0` where they had read `143`, `156` and
`4500`, and stayed there. Zero is below `white_balance_temperature`'s own declared minimum of
2000, so it is also PF:4 with a second and much larger instance than `zoom_continuous`'s 245.
That reading was taken *after* a cycle and is not an AWB-drift measurement; what it shares with
the BRIO's is the underlying fact, which is that the value of a control under automation is a
**read-back of an algorithm** rather than a setting, and UVC gives the driver no obligation to
advertise that with the one flag V4L2 has for it.

### What it costs this tool, stated rather than fixed

**Two R3 arms went red on it, and they were the only two reds in the 2026-08-12 `just smoke-hw`
run** (16 of 18 passed, the suite complete, census clean):

```
crates/backends/v4l2/tests/hardware.rs:1972
  cam:logitech-brio: white_balance_temperature is Some(Int(3680)) and the sweep found it at 3610
crates/client/tests/hardware.rs:651
  cam:logitech-brio: white_balance_temperature is Some(Int(3610)) and the session found it at Int(3630)
```

Both arms sweep `brightness`, both call `engine::lifecycle::recover`, and in both the restore
**reported itself complete** — `restore.is_complete()` passed. The engine did what it was asked;
the device then moved a value nobody wrote, between the restore and the re-read, because the
sweep's own photographs are what drive the AWB. The failure is real and it is not the engine's.

**No code is changed here, and that is a decision.** The obvious repair — exempt INACTIVE
controls as well as VOLATILE ones — is a change to what AGENTS rule 8 *means*, not a bug fix: it
would stop asserting restoration for every control an automation currently owns, including ones a
sweep legitimately moved and put back. The narrower repair — exempt a control the device reports
INACTIVE **at both ends** of the comparison — is defensible and is still a change to
`engine::snapshot`'s one home for that rule. Either way it is a design decision with a rung to
prove it, and this entry is the evidence it would be made on.

**What must not happen is a re-capture or a tolerance.** The corpus is right, the BRIO is right,
and a comparison that allowed "close enough" on a white-balance readback would allow it on
everything else in the same loop.

**Retires when:** a device is measured that sets `VOLATILE` on a control whose value its own
automation writes — at which point the flag becomes usable for the question this workspace asks
it — or when the restore comparison stops keying on `VOLATILE` alone, at which point this entry
becomes the reason rather than the defect.

**Adjacent:** PF:3 (the flag, not the value), PF:4 (currents outside the declared range — the
OBSBOT's `0` is a second instance), PF:25 (the OBSBOT half of the same measurement).

### Amendment, 2026-08-13: both arms were green in the dark, which is the same finding from the other side

Recorded so that a reader comparing two runs does not conclude this was fixed. The
2026-08-13 `just smoke-hw` run that landed \[PF:28\]'s fix (note **N86**) had **both** of the arms
above passing — `hardware.rs`'s brightness sweep and the client rung's session, on the same BRIO,
with no code touching `engine::snapshot`'s VOLATILE exemption. The run happened after dark, and the
room was dark enough that the sensor's own frames were nearly black: measured on this device's
neighbour with the shipped `wch photo`, mean luma 2.58/255 with a maximum of 10.

Nothing about the finding changes; if anything it sharpens. This entry's mechanism is that "the
value moves when the sensor *streams*" — the AWB algorithm reacting to a scene — and a scene with
no light in it is a scene the algorithm has nothing to react to. **That the darkness is what made
the arms green is a reading and not a measurement**: no controlled comparison was run, the room was
not the variable anybody was changing, and the honest statement is that these two arms are green on
some scenes and red on others, which is exactly what a device-driven read-back looks like from a
suite. It also means the suite's red count is not a stable number to quote: the run in \[E15\] had
two, the run on 2026-08-13 had one, and the one it had was neither of these two.

### Amendment, 2026-08-13 (second): the arms were red in daylight, and the repair was a value three of four suites were not reading

The amendment above records both arms **green in the dark** and says plainly that green-on-some-
scenes-and-red-on-others is what a device-driven read-back looks like from a suite. This is the
other side, measured the same day in a lit room, and it is what the repair was driven from.

**The measurement.** `just gate-g3`'s `smoke-hw` row, and then the two arms alone, three times:

| run | arm | snapshot | found |
|---|---|---|---|
| gate-g3 | the client rung's session | 4880 | 4720 |
| gate-g3 | `hardware.rs`'s brightness sweep | 4750 | 4880 |
| alone, at `ef8748f` | the client rung's session | 4850 | 4640 |
| alone, at `ef8748f` | `hardware.rs`'s brightness sweep | 4860 | 4850 |

**A different pair of numbers every time**, which is the part the dark-room amendment could not
supply: a stale constant would repeat, and this does not. All four are `cam:logitech-brio`'s
`white_balance_temperature`. The three cameras beside it were re-probed with the shipped binary the
same afternoon and do not move at all — Chicony RGB 4600, Dell 5000, OBSBOT 4500, each unchanged
across `get` → `photo` → `get`. So the drift is this device's, on this scene, and the control group
is PF:24's own probe reproduced two days later.

**The repair is the narrow one this entry named, and it needed no new engine code.**
`schema::snapshot::RestoreOutcome::OwnedByAutomation` — produced by `engine::snapshot::restore`'s
second pass, and meaning INACTIVE at snapshot time *and* INACTIVE now — already is the predicate
this entry asked for. What was missing is that **three of the four suites that compare a restore
were not reading it**, and filtered on `was_volatile` instead, which PF:24 exists to say is the
wrong flag. One arm (the hotplug one) had it right and had had it right since P1. So this entry's
"either way it is a design decision" was over-priced: the decision had been taken, and only one
caller had noticed. `testkit::battery::restoration_claim` is now the one home and all four arms
read it.

**A fourth arm was in the class and had never been red.**
`hw_a_snapshot_perturb_restore_round_trip_leaves_every_control_where_it_started` makes the identical
comparison behind the identical wrong filter. It has stayed green because it takes no photo, and
this entry's mechanism needs the sensor to be *streaming* — so it is a latent instance rather than
a lucky one, and a device that drifted while idle would find it identically.

**And rule 3 came apart at a `println!`.** The one arm that already declined these controls printed
its `[PF:24]` line without a `SKIP` prefix, so `scripts/smoke-hw.sh`'s census — which counts lines
matching `^[[:space:]]*SKIP` — never counted it. Named but not counted is how a count quietly stops
meaning anything: the rung reported **9** declined claims across the suite and now reports **21**,
and twelve of that difference are declines it was already making. The new twelve are not new
behaviour; they are behaviour that had been invisible to the accounting AGENTS rule 3 asks for.

**One device fact worth keeping, free from the same run.** The partner named for the same control
differs between arms, and both answers are right: the hotplug arm restores through
`restore_in_effect`, whose pair discovery is shape-only, so the OBSBOT's `red_balance` and
`blue_balance` print *"no partner in this device's pair set"*; the calibration arms restore against
the session's **D3-measured** pairs and print `(white_balance_automatic)`. Same device, same run,
two vocabularies — a compact argument for why D3's empirical probe exists beside the declared table.

**The honest limit on this repair's evidence, and it is a real one.** The BRIO was **unplugged from
this desk partway through the session**, between the runs tabulated above and the run that verified
the fix — the rig went from five logical cameras to four, and `hw_enumeration_matches_the_committed_profile`
began declining `logitech-brio` by name as a profile matching no attached device. So **the fix was
never observed turning that red green.** What was done instead: the drift was *staged*, by injecting
a move on exactly the controls the report names `OwnedByAutomation`, and the arms were watched
failing without the repair, passing with it, and **still failing when a claimed control was moved
instead** — which is the assertion that says nothing was weakened. That is a stronger demonstration
than one lit-room run, because it does not depend on the weather; it is not the same claim, and the
difference is exactly the kind this entry's first amendment was written to keep visible.

**Retired as an open question by this amendment.** The clause above says this entry retires "when
the restore comparison stops keying on `VOLATILE` alone, at which point this entry becomes the
reason rather than the defect". That is what happened, and the reason is now cited by four arms and
five unit tests over values.

---

## PF:25 — A `uvcvideo` cycle re-parks the OBSBOT Tiny 3's gimbal and re-initialises its processing unit; the three cameras beside it keep everything

**Measured** 2026-08-12 on kernel `7.0.0-29-generic` (x86_64), four USB devices carrying five
logical cameras, three `uvcvideo` cycles through `wch-priv` \[N8\]. Continues the docs/6 §1.2
registry; cite it as `[PF:25]`. The control group below is four of the five: the Chicony IR
sensor has three controls and none of them is comparable, which is the same control poverty it
declines seven R3 claims for.

**Amended 2026-08-13, and the title's first clause does not survive it** — photographed against a
calibrated metric, the gimbal does not move across a cycle; the read-back does. Read the amendment
at the end of this entry before citing the title, and \[PF:28\] for the hazard that replaces it.

PF:22 measured what a `uvcvideo` reload does to *node numbering* and concluded, correctly, that
it "changed nothing about any of them". That conclusion was drawn over `card`, `bus_info`, node
kinds and caps words — the invariant section of a profile. It does not extend to the **control
state**, and on the one camera in this house with a motor it is false there in the way that
matters most: the cycle moves where the camera is pointing.

### The measurement, three cycles

Positions read with `wch controls <cam> --json` before and after; the cycle performed by
`.wch-bin/wch-priv uvcvideo cycle`, which is the only path to it \[N8\] and which reported
`cycled; 14 node(s) before, 14 after` each time; the restore issued with
`wch set <cam> pan_absolute=… tilt_absolute=…`.

| cycle | how it was performed | OBSBOT `pan_absolute` | OBSBOT `tilt_absolute` |
|---|---|---|---|
| 1 | somewhere inside `just smoke-hw`, whose `hw_hotplug_*` arm cycles the driver | 28800 → **36000** | −46800 → **−43200** |
| 2 | `wch-priv uvcvideo cycle`, nothing else running | 28800 → **43200** | −46800 → **−298800** |
| 3 | `wch-priv uvcvideo cycle`, with a control group (below) | 28800 → 28800 | −46800 → **−298800** |

**Row 1 is a net change across a whole suite run and rows 2 and 3 are the isolated event**, which
is the order the finding was made in and is stated that way rather than tidied: the suite's
before/after is what raised the question, and the two hand-run cycles are what answer it. Nothing
else in the suite writes pan or tilt except the motion arm, which restores to the value it read
\[PF:18\] and whose own transcript shows it starting from 36000.

Pan is not deterministic; tilt landed on **−298800** twice, which is 92% of the way to its
declared minimum of −324000 — the head hanging down. Nothing wrote either control across the two
isolated cycles: no write was issued to that camera between the read and the cycle, and repeated
reads either side are stable, so the value is not noise.

**It does not come back on its own.** After cycle 2 the position was re-read twice, then a
`wch photo` was taken — which opens the node and streams, the event a "sleeping" camera would
wake on — and re-read again: `−298800` all four times. A write puts it back exactly
(`pan_absolute=28800 tilt_absolute=-46800`, `requested → applied` equal, stable across three
subsequent reads), so nothing about the axis is broken; the state is simply gone.

### The processing unit goes with it

After cycles 2 and 3 the OBSBOT answered its five User-Controls-class integers with the **same
out-of-range number**, and its three white-balance read-backs with zero:

```
brightness 50 → 5912    contrast 65 → 5912    saturation 60 → 5912
hue 50 → 5912           sharpness 70 → 5912   (every one of them declares 0..=100)
red_balance 143 → 0     blue_balance 156 → 0  white_balance_temperature 4500 → 0
gain 1 → 1              backlight_compensation 9 → 9   (unchanged)
```

`5912` is `0x1718`, it is identical across five controls with five different meanings, and it
persists across re-opens and across a streaming photo. The three zeros are PF:24's class — those
controls are INACTIVE under `white_balance_automatic=1` and their value is the algorithm's. The
five `5912`s take writes and read back correctly the moment one is issued, so this is a *state*
the device came up in, reported as measured and never corrected \[AGENTS rule 6\].

### The control group, which is what makes this the device's

Cycle 3 was run with the two digital-PTZ cameras deliberately moved off centre first, so that
"held its value" could not be confused with "was at its default":

| camera | written before the cycle | read after the cycle |
|---|---|---|
| Dell U3224KB/A \[PF:20\] | pan 7200, tilt −3600, zoom 100 | **7200, −3600, 100** |
| Logitech BRIO \[PF:20, PF:24\] | pan 7200, tilt −3600, zoom 320 | **7200, −3600, 320** |
| Chicony RGB | brightness 128, contrast 32, saturation 64, sharpness 3 | **unchanged** |
| OBSBOT Tiny 3 | pan 28800, tilt −46800 | 28800, **−298800** |

One event, one host, one kernel, four cameras: the three without a motor kept every value,
including the two whose `pan_absolute`/`tilt_absolute` PF:20 established are windows rather than
actuators. So this is not "a driver reload loses cached control values" — `uvcvideo` demonstrably
does not — it is this device re-parking a head and re-initialising a control block when its
driver goes away.

### What it costs this tool

1. **`just smoke-hw` does not leave a PTZ camera where it found it, and nothing in it can
   notice.** The hotplug arm (E9, docs/7 P4d) performs the cycle as evidence; the motion arm
   `hw_motion_a_bounded_ptz_sweep_returns_the_motor_to_where_it_started` runs **after** it and
   reads `home` from `desc.current` at its own start — which is the post-cycle number. Its
   transcript from the 2026-08-12 run says so in full:

   ```
   cam:obsbot-…: moved pan_absolute through [28800, 32400, 36000, 39600, 43200]
     — 5 sample(s), 14400 units of travel (4 step(s)), and back to 36000
   ```

   The arm passed, correctly: it *did* return the motor to where it started. Where it started was
   36000, and the session had begun at 28800. The Dell's line in the same run reads "back to 0"
   and its home was 0, so on that camera the two coincide and nothing is visible — which is
   exactly why this went unseen until a session measured the camera from outside the suite.

2. **Expected usage item 7 is the cost, and it is stated there already.** "A sweep that leaves it
   pointing somewhere else has invalidated every photograph taken afterwards, not only the one
   being taken." A `uvcvideo` cycle is not a sweep, and the rule does not care.

3. **No fix is made here and two are named.** The suite could snapshot pan/tilt across the
   hotplug arm and put them back — which asserts a restoration V4L2 gives no way to *verify*,
   since `pan_absolute` reads back the commanded position \[PF:18\], so it would restore the
   number and hope about the head. Or the hotplug arm could be excluded from runs where a PTZ
   camera is aimed, the way `WCH_NO_MOTION=1` already excludes the motor arms — which is an owner
   decision about what `just smoke-hw` costs, not a convenience fix. Both are larger than this
   entry.

**Retires when:** a firmware or a kernel is measured on which this device's gimbal survives a
driver unload with its aim, or on which the tilt park position is not reached; or when V4L2 grows
a way to read a mechanism's *actual* position, which would let a restore be verified rather than
issued \[PF:18\].

**Adjacent:** PF:22 (the same event, the half that is harmless), PF:18 (why "restored" is a claim
about a command), PF:20 (why the control group is the right control group), PF:24 (the three
zeros), E9 (the arm that performs the cycle), E15 (this run).

### Amendment, 2026-08-13: photographed, the gimbal does not move — the read-back does, and the hazard inverts

Everything above stands as the transcript of what was read. What it *concluded* does not, and the
correction is not a softening: the title's first clause — "re-parks the OBSBOT Tiny 3's gimbal" —
was inferred from a control's value, and a control's value is the one thing on this axis that is
not evidence of a mechanism \[PF:18\]. Photographed, five cycles across two sessions moved the aim
by less than the noise floor of a metric that resolves 2°. **What moves is the number.** The finding that replaces
it is \[PF:28\], and this amendment says how it was reached, what it leaves unexplained, and what
landed on the strength of it (note **N86**).

**The metric had to be repaired before it could mean anything, and that is the first result.** The
opening attempt — photograph before, cycle, photograph after, compare — was worthless, because auto
white balance moves the picture between two shots of a scene where nothing has changed. That is
\[PF:24\]'s mechanism, met on this entry's own device rather than on the BRIO. So AWB was pinned
off, the image controls were held fixed for the duration, the photographs were downscaled to
320×180 greyscale and compared by mean absolute difference — and then, because a difference with no
scale under it cannot say "nothing moved", the metric was calibrated against moves of a known size:

| comparison — OBSBOT, room scene, nothing else moving | mean abs diff |
|---|---|
| same commanded position, two shots — **the noise floor** | **0.40** |
| commanded 2° away (pan 28800 → 36000) | **10.97** |
| commanded 2° away and back | **0.42** |

A 1–2° move is **26× the noise floor**, and commanding an absolute position returns the aim to
within noise. Only with those three numbers in hand does the experiment below have a verdict rather
than a reading.

**The experiment.** Three consecutive `wch-priv uvcvideo cycle`s from a settled position, each
preceded by commanding pan 28800 / tilt −46800:

| trial | reported tilt, before → after | image difference |
|---|---|---|
| 1 | −46800 → **−43200** | 0.735 |
| 2 | −46800 → **−43200** | 0.401 |
| 3 | −46800 → **−43200** | 0.464 |

Every image difference sits at the noise floor, 26× below what a real 1–2° move looks like, while
the reported tilt moves by exactly one step every time.

**A second session the same day re-measured it with different instruments, and the finding survives
while its arithmetic does not.** The scene was dark by then, so the metric was rebuilt at
`gain=128`, `exposure_time_absolute=2500` — held fixed across every shot, including across the
cycle, since \[PF:25\]'s own body says a cycle can re-initialise the processing unit and an
exposure change would swamp an aim change — and re-calibrated. Three photographs per position with
the last two compared, because a photograph taken the instant a write returns catches the head in
flight (PF:18 item 3, met in practice: a 40° command scored 24.8 and the same command *back* scored
25.6 against the origin, both of them the head still travelling):

| comparison — same device, dark scene, high gain | mean abs diff |
|---|---|
| two settled shots, nothing commanded — the noise floor | 1.02 |
| commanded 2° away | 7.93 |
| commanded 2° away and back | 1.09 |
| **a whole `uvcvideo` cycle, nothing written to pan or tilt** | **1.04** |

Same verdict on a different scene with a worse floor: the cycle costs the aim nothing measurable,
where 2° costs it eight times the floor.

**What that second session did *not* reproduce is the arithmetic.** "+3600, deterministically,
every time" is true of trials 1–3 and false as a rule. Six further cycles, each with a value
commanded and nothing streamed between the write and the cycle:

| tilt commanded before the cycle | reported after it |
|---|---|
| −46800 | −43200 |
| −46800, then two more cycles with nothing written between them | −43200, then −43200 (stable) |
| −43200 | −43200 |
| −36000 | −43200 |
| −100800 | −50400 |
| 0 | −46800 |

The offset is not a constant and not a direction. What is stable is the *destination*: whatever was
commanded, the post-cycle reading lands in a narrow band around 12–14° down, and a second and third
cycle do not move it again. And in the one cycle where **every** control on all five cameras was
compared — the R3 hotplug arm's own run, once N86 gave it a snapshot to compare against — nothing
changed at all: 22 OBSBOT controls, 51 across the other four cameras, tilt 0 before and 0 after.
Neither half of this entry's title reproduced in that run, the processing unit's `5912`s included.

**The narrowed claim, then.** A `uvcvideo` cycle *is* followed by a control reading that differs
from the one before it, on this device, on the axis with a motor and (per the body above) on its
processing unit. It is not established that anything mechanical moves; on the seven cycles measured
photographically or by full control comparison, nothing did.

**And the hazard inverts, which is why this is worth an entry of its own.** This entry's item 3
proposed snapshotting pan/tilt across the hotplug arm and worried that it would "restore the number
and hope about the head". The real exposure is the opposite and is worse: a snapshot taken **after**
a cycle records −43200 for a camera nobody moved, so restoring *from* it would introduce a 1° error
rather than fail to correct one. The ordering is the whole fix, and a build with the two steps
swapped is invisible from outside. \[PF:28\] carries that hazard for the audience that needs it —
whoever next writes a snapshot/restore path — and note **N86** carries the fix and what can go red.

**It contradicts this entry's own −298800, and that is left standing rather than explained.** Two
hand-run cycles took tilt to −298800, 92% of the way to its declared minimum, and the entry records
the head hanging down. Nothing measured on 2026-08-13, in either session, came near it: every
post-cycle reading landed between −43200 and −50400, or did not move at all. Either those two cycles found a different device state — they followed a full
`just smoke-hw`, with streaming and motor arms behind them, where today's did not — or −298800 was
itself a read-back interpreted as a position. **Two regimes may exist and only one of them is
characterised.** The one that is characterised is the one this desk meets every run; the one that
is not is the one where somebody reported a head hanging down, and no amount of arithmetic on
today's numbers settles it. It is named here so that the next person to see −298800 knows they are
looking at the unexplained half rather than at a new bug.

**One guess at the second regime was tested and did not produce it.** These cameras park face down
when they sleep, so "the head was asleep" is the obvious candidate for −298800, and idleness is the
obvious trigger. Measured: head streamed to tilt 0, then **fifteen minutes with nothing streaming
at all**, then a read (an open is not a stream — 0), then a cycle, then a read (0), then
photographs against the pre-idle reference — **1.10, against that shot's own floor of 1.03**. So
fifteen idle minutes park nothing on this device, and that is one more cycle where neither the
number nor the aim moved. A longer idle, a suspend, or a run with the motor arms behind
it are all untested, and one of them is where the other regime lives if it lives anywhere.

**Retires this amendment:** a measurement that reaches the −298800 regime deliberately — the
obvious protocol is a full `just smoke-hw`, then hand-run cycles with the photographic metric
running — or a firmware or kernel on which the post-cycle reading equals the pre-cycle command.
Nothing above retires the entry: the readings in its body were real and the control group is
untouched.

---

## PF:26 — The BRIO's first-enumerated format is YUYV at 640×480, so D5's default photograph is a re-encoded VGA from a camera that offers verbatim 4096×2160

**Measured** 2026-08-12, against the Logitech BRIO on the day it was attached; the Dell
U3224KB/A is the second instance and was already in the corpus. Continues the docs/6 §1.2
registry; cite it as `[PF:26]`.

`StreamRequest::choose` (design D5, `crates/schema/src/capture.rs`) resolves an unspecified
request to "the device's first" format at its first size entry's maximum, and its doc comment
argues the case: "the order `VIDIOC_ENUM_FMT` returns is the driver's own preference, and
second-guessing it is how a tool ends up defaulting to a mode the camera is worse at." That
argument is sound and this is the device that costs it something.

### The measurement

`corpus/profiles/logitech-brio.json`, captured by `wch profile capture` before anything wrote to
the device. The format list, in the driver's order:

```
YUYV  19 sizes, first entry 640x480          (largest 1920x1080)
MJPG  20 sizes, first entry 640x480          (largest 4096x2160; 3840x2160 also present)
NV12   4 sizes, first entry 640x480          (largest 1920x1080)
```

So both halves of the default land on the smallest useful thing the camera has, and the two R3
photo arms print the consequence for all five cameras in one place:

```
cam:obsbot-…:                MJPG 1920x1080 → 191680 bytes, the camera's own bytes [E6]
cam:integrated-camera-…-c:   MJPG 1280x720  → 248606 bytes, the camera's own bytes [E6]
cam:integrated-camera-…-i:   GREY 640x360   →  32999 bytes, re-encoded
cam:dell-u3224kb-a-…:        NV12 640x480   →  36926 bytes, re-encoded
cam:logitech-brio:           YUYV 640x480   →  46742 bytes, re-encoded
```

Two of five cameras deliver the camera's own JPEG bytes by default. The other three do not, and
two of those three — the Dell and the BRIO — **have** an MJPG branch with 4K in it and are not
being asked for it. The Chicony IR sensor is the honest case: it has no compressed format at all.

### What it costs this tool, stated rather than fixed

Expected usage item 3 names the consumer: "the agent may diff two photographs or feed them to a
vision model, and a re-encode inserts differences the device under test did not make. A pipeline
that silently re-encodes is a pipeline that fabricates evidence in a test." On this device the
default does both things that item warns about — it re-encodes, and it does so at **3.5% of the
pixels** the camera can produce (307 200 against 8 847 360) — and it does them *silently* in the
sense that matters: the
`rendering` field says `converted_and_encoded` and the negotiated size is reported, so the
document is honest, but nothing in the answer says "this camera also offers 4096×2160 MJPG and
you did not ask for it".

Nothing is changed. `--size` and `--format` reach the right modes today (`largest_within` picks
the exact entry when the caller names one), the D5 default is a documented decision with a
recorded argument, and changing it — "prefer a compressed format when one exists", or "prefer the
largest size" — is a design change against §7's settled alternatives, not a bug fix. What this
entry establishes is that the default's cost is now measured on real hardware rather than assumed
to be zero, and that the two cameras it costs the most are the two **4K** ones.

### The rest of what the BRIO's tree is, because it is the largest real one this project has met

- **43 size entries and 295 frame intervals**, against the previous corpus maximum of 13 sizes
  (Dell) and 32 intervals (OBSBOT); the whole four-camera corpus before it held **33**. Every one
  of the 76 is `V4L2_FRMSIZE_TYPE_DISCRETE`, so `FrameSize::largest_within`'s recorded claim that
  "no camera this project has met is stepwise" still holds, now over 76 entries rather than 34.

  **That count in `crates/schema/src/camera.rs` says 34 and has been stale since PF:23**, which
  is worth a sentence because of how it got that way: the OBSBOT's re-capture at `1a51c81`
  removed its 3840×2160 entry, the total went 34 → 33, and nothing anywhere reads that number,
  so nothing noticed. It is prose in a doc comment and the claim it supports — *discrete, not
  stepwise* — is still true and now better supported. It is left alone here because that comment
  is an input to `schemas/webcam-handler-schema.json` (AGENTS "Done means"), so touching it moves
  a committed artifact, and this session changes no product code.
- **4096×2160** — DCI 4K, 256:135, the first non-16:9, non-4:3 large mode in the corpus, and the
  first 4K mode any attached camera has advertised since the OBSBOT stopped \[PF:23\].
- **Two square modes, 340×340 and 440×440, in YUYV only**, each with exactly **one** frame
  interval where its nineteen siblings have seven. A size present in one format and absent from
  another, with a per-size interval list of a different length, is the shape PF:9 records; this
  is the sharpest instance of it in the corpus. `340×340` is also the *only* size this device's
  second capture node offers \[PF:27\], which is the most economical reading of why it is here.
- **120 fps** at MJPG 640×480, 90 at 1280×720, 60 at 1920×1080 — high rates that only exist at
  particular sizes, again per-size rather than per-format.
- **The control set is slug-for-slug identical to the Dell U3224KB/A's** — nineteen controls, no
  new vocabulary, no `RECT` and no `BITMASK` — so the BRIO adds no unknown control type. Its
  `pan_absolute`/`tilt_absolute` (±36000, step 3600) and `zoom_absolute` (100..500) are PF:20's
  second instance, and PF:20's finding is unchanged and strengthened: nothing in the control
  surface distinguishes them from a gimbal's, and PF:25 now shows the difference from outside.
- **The motion cap is not exercised on it**, and the R3 arm says so as a named partial skip:
  "pan_absolute's whole range is 21 samples, under the motion cap of 32".

**Retires when:** D5's default is changed, or a device is measured whose enumeration order makes
the default's argument false in the other direction (a driver listing a mode it is *worse* at
first), which is the case that argument was written against and which nothing here has seen.

**Acted on (owner ruling, 2026-08-13; note N85):** the first of those two conditions is met —
D5's default is re-ranked and this entry is the measurement that forced it. The entry is **not**
retired, because nothing here has been disproved: the BRIO still enumerates YUYV first at 640×480
and the Dell still starts at NV12, and those two facts are what a reader of N85 needs in order to
believe it. What is no longer current is the closing paragraph's "nothing is changed"; the table of
what each camera now defaults to is in N85, and the second retirement condition above is unchanged
and still unmet.

**Corpus:** `corpus/profiles/logitech-brio.json`, captured 2026-08-12 by `wch profile capture`
against `cam:logitech-brio` before any write, replayed by
`every_committed_profile_replays_through_the_conformance_battery` on the corpus walk.

---

## PF:27 — The BRIO's second capture node is a *different sensor*, not a second stream off the same one, and this tool cannot name it

**Measured** 2026-08-12 on kernel `7.0.0-29-generic` (x86_64), by a raw `VIDIOC_QUERYCAP` /
`VIDIOC_ENUM_FMT` / `VIDIOC_ENUM_FRAMESIZES` walk over each node — the same instrument PF:19 used
on the Dell, and the only one available, since T1 has no vocabulary for "open the other node"
(PF:19 item 3). The walk was a throwaway `fcntl.ioctl` script in a gitignored scratch directory,
read-only: no `S_FMT`, no `STREAMON`, no control write. It is not committed, for the reason PF:19
did not commit its own: the *finding* is the artifact, and a probe that opens a node this tool
refuses to open is not a thing to keep in the tree. Continues the docs/6 §1.2 registry; cite it
as `[PF:27]`.

PF:19 established that a group with two capture nodes can be **one** camera, and its title records
the mechanism it found: "a UVC device with two output terminals **on one sensor**". That
mechanism is the Dell's. It is not the BRIO's, and the difference is visible without descriptors
because the two nodes do not offer the same *kind* of picture.

### The measurement

All four BRIO nodes hang off the single VideoControl interface
`…/2-3.4.2.4/2-3.4.2.4:1.0/video4linux/`, so PF:7's grouping puts them in one camera and
`capture_node()` takes the first (PF:19's positional tie-break). What each capture node offers:

```
/dev/video10  device_caps 0x04200001  YUYV 19 sizes, MJPG 20 sizes (to 4096x2160), NV12 4 sizes
/dev/video12  device_caps 0x04200001  GREY '8-bit Greyscale', ONE size: 340x340
```

Beside the two cameras this house already had, walked the same day with the same instrument:

| node | card | formats |
|---|---|---|
| `/dev/video8` (Dell, second capture node) | Dell U3224KB/A 4K Webcam | `NV12` 640×480, one size — **unchanged from PF:19**, 2026-08-09 |
| `/dev/video4` (Chicony IR, *its own camera*) | Integrated Camera: Integrated I | `GREY` 640×360, one size |
| `/dev/video12` (BRIO, second capture node) | Logitech BRIO | `GREY` 340×340, one size |

The BRIO's second node reads like the **Chicony's IR camera**, not like the Dell's secondary
stream. **What is measured is the pixel format**: `GREY` appears nowhere in the first node's three
formats, and a second output terminal fed by a colour sensor produces colour — which is exactly
what the Dell's second node does. **That it is an infrared sensor is an inference** and is marked
as one: a greyscale-only square stream at a face-authentication-shaped resolution on a consumer
webcam is a strong hint and it is not a measurement, and the USB descriptors that would settle it
were **not** read here (PF:19 read the Dell's; nothing equivalent was done for this device).

The square `340×340` is the strand that ties it together: it is the **only** size this node has,
and it also appears — along with `440×440`, each with one frame interval where every sibling has
seven \[PF:26\] — in the *first* node's YUYV list. Two sensors' worth of geometry reaching one
enumeration is the most economical reading of that oddity, and it is why PF:26 records the square
modes as an anomaly rather than a curiosity. It is a reading, not a measurement, and the
descriptor walk that would confirm it is the obvious next probe.

### What it costs this tool, stated rather than fixed

**The same hardware capability gets two different answers depending on USB topology.** The
Chicony's greyscale sensor sits behind its own VideoControl interface (`3-4:1.2`), so PF:7's
grouping makes it `cam:integrated-camera-integrated-i` — a camera with an id, a profile, a place
in `wch list` and a row in every hardware arm, declining seven claims by name because its control
set is poor. The BRIO's sits behind the shared one, so it has no id, no profile, no row, and
nothing in this tool can photograph it. Neither answer is wrong about the bus; both are answers to
a question about the bus rather than about the device.

**PF:19's item 4 gets sharper and its wording gets a caveat.** "T3 captures the camera, not each
node" was recorded as a *limit* on the Dell, where the unreachable stream is a 640×480 version of
a picture the tool can already take. On this device the unreachable stream is a picture the tool
**cannot take at all**, so `corpus/profiles/logitech-brio.json` is silent about a whole sensor
rather than about a redundant resolution. That is a bigger silence than PF:19 anticipated, and it
is the argument a future "name a node as a capture target" design change would be made on — a
design change (T1's `open` takes a `CameraId`), not a bug fix, and not made here.

**Nothing about PF:19 is retired.** Its measurement was of the Dell and is confirmed unchanged
today at the same node with the same walk. What this entry adds is that "two capture nodes in one
group" has at least two mechanisms behind it; that **the format list is what separates them, and
`QUERYCAP` is not** — which sharpens PF:19's item 2, where the tie-break had to be positional
because the two nodes were indistinguishable *before* opening, and that is still true here even
though what is behind them is not the same thing at all; and that PF:19's choice of the first node
in node order happens to be right on both devices, still "a convention, not a guarantee", and now
right for two different reasons.

**Retires when:** T1 grows a way to name a capture node, at which point the BRIO's IR stream stops
being invisible; or when a device is met whose two capture nodes are distinguishable from their
`QUERYCAP` output alone, which neither of these two is — both report identical
`device_caps 0x04200001`, identical card, driver and `bus_info`, and differ only once opened.

**Adjacent:** PF:7 and PF:13 (the grouping key), PF:19 (the first instance and the tie-break),
PF:26 (the square modes and the format tree this reads back onto), E15 (this walk).

---

## E15 — Hardware validation of P5's surface at five real cameras, and of AGENTS rule 8 from outside the suite, 2026-08-12

E13 and E14 are the shape this follows: a dated run against something this project does not
control, recorded once and not amended. This one is **not** a phase gate's evidence. It was asked
for by the owner as measurement over the tree at `522f45f` — everything P5 built had only ever
run against the fake backend, deliberately (docs/7 P5a–P5c) — and it is recorded here because
four of its results are device findings that outlive it (PF:24, PF:25, PF:26, PF:27) and one of
those is a failure of a rule this project claims to honour.

Nothing in the tree was changed to produce it. The only file it adds is
`corpus/profiles/logitech-brio.json`.

### The fixture

**Host:** the P4/P5 workstation, kernel `7.0.0-29-generic` (x86_64). **Attached: five logical
cameras on fourteen nodes**, one more camera than any previous entry's fixture —

| camera | `card` | live nodes | committed profile's nodes |
|---|---|---|---|
| `cam:obsbot-tiny-3-obsbot-tiny-3-st` | OBSBOT Tiny 3: OBSBOT Tiny 3 St | `/dev/video0,1` | `/dev/video0,1` |
| `cam:integrated-camera-integrated-c` | Integrated Camera: Integrated C | `/dev/video2,3` | `/dev/video0,1` |
| `cam:integrated-camera-integrated-i` | Integrated Camera: Integrated I | `/dev/video4,5` | `/dev/video2,3` |
| `cam:dell-u3224kb-a-4k-webcam` | Dell U3224KB/A 4K Webcam | `/dev/video6,7,8,9` | `/dev/video6,7,8,9` |
| `cam:logitech-brio` | Logitech BRIO | `/dev/video10,11,12,13` | *first sight* |

The **Logitech BRIO** (`046d:085e`, serial `19908110`, interface `2-3.4.2.4:1.0`, four nodes on
the dock's PCI root) had never been seen by this repository. It landed as a profile the day it
was seen (AGENTS rule 4) and its findings are PF:26 and PF:24.

### 1. Identity survived renumbering, and the brief's premise was half right

`./target/debug/wch list --json` matched against `corpus/profiles/*.json` by fingerprint
(`bus_path`, `usb_id`, `card`, `driver`, `serial`):

- **Four of four committed profiles resolved to exactly one live camera each**, with no
  ambiguity and no camera matching two profiles. The two Chicony logical cameras, which share a
  `usb_id`, a `card` prefix and the serial `0001` \[PF:8\], separate on `bus_path` (`3-4:1.0` vs
  `3-4:1.2`) \[PF:13\].
- **Two of four sit at different `/dev/videoN` than when their profile was captured** — both
  Chicony nodes, each moved by two. `card`, `driver`, `bus_info`, `backend`, node count, and
  every node's `kind`/`device_caps`/`capabilities` are identical on all four, and every
  `CameraId` is unchanged including after a fifth camera joined the enumeration.
- `hw_enumeration_matches_the_committed_profile` and
  `hw_profile_capture_reproduces_the_committed_invariant_section` are both green and the rung
  prints the moves rather than ignoring them, which is N63's design working:

  ```
  obsbot-tiny3: enumeration matches the committed profile
  chicony-rgb:  enumeration matches the committed profile; its node paths were reassigned by
                the kernel and are not identity [PF:22]: /dev/video0 → /dev/video2,
                /dev/video1 → /dev/video3
  chicony-ir:   … /dev/video2 → /dev/video4, /dev/video3 → /dev/video5
  dell-u3224kb: enumeration matches the committed profile
  logitech-brio: enumeration matches the committed profile
  2 of 5 matched camera(s) sit at different /dev/videoN paths than when their profile was
  captured, and none of them changed
  ```

**This session's brief said all four cameras had moved since the corpus was captured; two read as
moved and the difference is a capture date, not a kernel.** The OBSBOT *did* move — that is
PF:22's own table, `/dev/video4,5` → `/dev/video0,1` — but
`corpus/profiles/obsbot-tiny3.json` was replaced at `1a51c81` on 2026-08-11 for PF:23's reason,
*after* that reload, so the committed document records today's numbering and the comparison finds
nothing to report. The Dell has never moved and PF:22 says why (a different PCI root). That
re-capture was not a corpus re-captured to make a comparison green — N63 argues against exactly
that, and this one was for a format-tree shrink — but it is a reader trap worth naming: **a
profile's node paths are provenance about the boot it was captured on, so "moved" and "did not
move" are as much facts about capture dates as about kernels.** Two further `uvcvideo` cycles were
performed during this session (PF:25) and neither renumbered anything, which is the same finding
from the other side: probe order is *arbitrary*, not *unstable*.

### 2. The Logitech BRIO, captured and compared

`wch profile capture cam:logitech-brio --capturer "victor@costan.us (fourth camera, landed on
first sight)" -o corpus/profiles/logitech-brio.json`, before any write reached the device;
`engine::profile::capture` is read-only and says so at its `measured_pairs` field.

What the comparison against the other four found is **PF:26** in full: the largest real format
tree in the corpus (3 formats, 43 sizes, 295 intervals, against 33 size entries for the previous
four cameras *together*), 4096×2160, two square single-interval YUYV modes, 120 fps at VGA — and
a control set that is slug-for-slug the Dell's, so the BRIO introduces no new control vocabulary
at all. Its `white_balance_temperature` is **PF:24**. Its `pan/tilt/zoom` are PF:20's second
instance.

The corpus walk picked it up with no test edited and nothing went red: `just ci` is green over
five profiles (1100 tests run, 1100 passed, 26 skipped), `corpus-floor.sh` counts five, and
`every_committed_profile_replays_through_the_conformance_battery` replays the new document
through the conformance battery on the strength of the directory walk alone — which is the
property that gate's claim 2 prefers over a named list, exercised.

### 3. `just smoke-hw` — 18 of 18 arms ran, 16 passed, 2 failed on the new camera

```
smoke-hw: motor-moving suites (hw_motion_*) are included — set WCH_NO_MOTION=1 to exclude them
smoke-hw: 14 capture node(s) present; running test(/(^|::)hw_/)
…
FAIL ( 2/18) webcam-handler-client::hardware  hw_a_sweep_over_the_socket_delivers_its_progress_live_and_leaves_the_camera_where_it_found_it
FAIL ( 4/18) webcam-handler-v4l2::hardware    hw_a_calibration_session_sweeps_a_brightness_control_selects_applies_and_restores
smoke-hw: 9 claim(s) declined by tests that ran — each named above
smoke-hw: 18 of 18 selected test(s) ran — the suite is complete
```

Both failures are one defect on one control on the new camera and both are **PF:24** — the
device's own AWB moving `white_balance_temperature` between a restore that reported itself
complete and the re-read that checks it. The census the rung grew for E13's sake did its job:
`--no-fail-fast` kept the other sixteen arms running and the `18 of 18` line certifies that a
truncated run did not read as a full one.

Of the nine declined claims, seven are the Chicony IR sensor's usual control poverty and one is
the Chicony RGB declining the motion arm for having no pan/tilt; the ninth is new and correct:
`cam:logitech-brio: pan_absolute's whole range is 21 samples, under the motion cap of 32, so the
cap was not exercised on this device`.

### 4. The OBSBOT did **not** come back to its exact aim, and the suite could not have told us

This is the result that outranks the rest of this entry, and it was found only because the aim
was read *outside* the suite, before and after, as the brief for this session required.

```
before `just smoke-hw`:  pan_absolute  28800   tilt_absolute  −46800
after  `just smoke-hw`:  pan_absolute  36000   tilt_absolute  −43200
```

Two of twenty-four controls differed and they were the two that decide where the camera points.
The cause is **PF:25**: the suite's own hotplug arm cycles `uvcvideo`, that cycle re-parks this
gimbal, and the motion arm runs afterwards and reads `home` from the device it finds. Its
transcript records the whole thing without knowing it — *"moved pan_absolute through \[28800,
32400, 36000, 39600, 43200\] … and back to 36000"* — a plan centred on 36000 by an arm that
believed 36000 was where the session started. It passed. It was right about its own claim and
the camera was 2° off in pan and 1° off in tilt.

Isolating it took three `uvcvideo` cycles and a control group of three cameras that kept every
value across the identical event; PF:25 carries the measurement. The camera was put back to
`28800 / −46800` by hand, verified over three reads, and is there now.

**What this says about a rule this project claims to honour.** AGENTS rule 8 and Expected usage
item 7 are about the validity of a development run, and both are honoured by every arm that
writes a control. Neither covers an arm that removes the *driver*. The gap is not in the restore
machinery — `engine::snapshot` and `engine::lifecycle` did nothing wrong all day — it is that
"leave the camera as you found it" has a scope, and the scope stops at the process boundary the
hotplug arm deliberately crosses.

### 5. The vivid rung — 8 of 8, no skips

`just rung-vivid-managed`, run after the real hardware, module loaded and unloaded by the blessed
helper \[N8\]:

```
Starting 8 tests across 44 binaries (1118 tests skipped)
cam:vivid: 77 control(s) enumerated
cam:vivid: 83 format(s), 747 size entr(ies)
cam:vivid: 4 node(s), bus_path vivid.0
10 compound payload(s) read
Summary [ 8.151s] 8 tests run: 8 passed, 1118 skipped
rung-vivid: suite run, 0 named skip(s) before it started
vivid: unloaded; 4 node(s) went away: video14, video15, video16, video17
```

Green including the write arm, the streaming arm, the `EBUSY`-on-second-stream arm and the
calibration sweep through the real ioctl path. It found nothing, which is the honest result: this
session changed no code, and the rung's subject is ioctl plumbing rather than device quirks
(design §3.3 item 4).

### 6. P5's surface at a real driver — the part that had never met one

A real `wchd --backend v4l2 --http 127.0.0.1:0` on loopback, twice, against the real cameras.

**The assets are ungated and the two camera-bearing routes are not** (N82, N74, D11). All ten
embedded assets, over eleven URLs, answered `200` **anonymously** — `/` and `/index.html`,
`/app.css`, and the eight ES modules — with the content types the client expects; and both
`CAMERA_BEARING_PATHS` entries, `/rpc` and `/preview?camera=…`, answered `401` anonymously and
`200` with the run's token. That is the first time the 2026-08-12 ruling has been exercised
against a daemon holding real cameras rather than a fake.

**Real MJPEG frames over `/preview`.** 280 parts from the BRIO and 337 from the Chicony RGB,
every one a complete JPEG (`FFD8` … `FFD9`) decoding at the negotiated size, 13 749–63 800 bytes,
read off the multipart stream by an out-of-process reader. No frame byte was written anywhere:
the parts were measured and discarded (AGENTS "Hardware and privacy").

**N83's suspend/resume meets a real V4L2 driver, and it works.** A `wchc photo` was taken over
the UDS while the HTTP preview was running, on two cameras:

| camera | photo | pause in the preview | `X-Wch-Frame-Index` across it | `X-Wch-Frame-Sequence` across it |
|---|---|---|---|---|
| Chicony RGB | MJPG verbatim | **0.727 s** | 40 → 41 → 42 → 43, no hole | 40, 41, 42, **0**, 2, 3, 4 |
| Logitech BRIO | YUYV → JPEG \[PF:26\] | **2.632 s** | 100 → 101 → 102 → 103, no hole | 38, 39, 40, **0**, 1, 2 |

Every claim N83 makes about what a viewer sees held against a real `STREAMOFF`/`STREAMON` cycle:
the photo succeeded, the preview resumed, the frames after it are real frames from the device,
the daemon's publication index has **no hole** because nothing was published during the pause,
and the driver's own sequence number **restarts at zero** — the one signal a client can read the
restart off, passed through rather than rewritten. The difference between the two pauses is the
re-encode PF:26 describes. Neither number was compared against
`limits::PREVIEW_SUSPEND_MAX_MS` by anything, and N83 is explicit about why — the bound is a
budget checked against the request's settle deadline *before* the stream is stopped, and a limit
consulted afterwards would be "a number in a log wearing a bound's name". What these two
measurements are is the first evidence that ten seconds is a plausible budget on real hardware:
the worst observed pause is a quarter of it, on the camera that re-encodes.

**One unplanned confirmation, and it is the better half of that table.** On the Chicony the
sequence after the restart runs `0, 2, 3` while the index runs `43, 44, 45` — a frame the
*kernel* dropped, beside an index with no gap in it. That is exactly the distinction the two
headers exist to carry ("a gap here is frames the kernel dropped … and a gap there is frames this
daemon published and the reader was too slow for"), and until this run it had only ever been
asserted over a fake.

**SIGTERM with preview tabs open** (the bound added at `29ecb82`). Two concurrent readers on one
camera's feed, both mid-stream, then a real `SIGTERM`: the daemon was **already gone at the first
poll, 9 ms later**, with **exit status 0**; both streams ended after draining what was already
buffered, and the log's last line is
`daemon::shutdown: wchd is stopping signal="SIGTERM"`. Run twice, same result. The streams were
cancelled and not awaited, which is what the 9 ms says — a daemon that waited for two MJPEG
readers to finish would not have one.

### What this run establishes

- **Identity is not the node path, measured against a five-camera population** — four profiles,
  four unambiguous resolutions, two of them at nodes that moved. PF:22 and N63 hold.
- **New hardware landed the day it was seen**, with a profile the tool captured and three PF
  entries (PF:24, PF:26, PF:27) derived from comparing it field by field against the four that
  were already there rather than from looking at it — which is why the findings are "this device
  differs *here*" rather than "this device is big".
- **"Two capture nodes in one group" has two mechanisms, not one** (PF:27), confirmed by walking
  the Dell's second node and the Chicony's IR camera with the same instrument on the same day.
- **The hardware suite is complete and its two failures are the device's, not the code's.** 18 of
  18 arms ran; the two reds are one control on one camera and are PF:24.
- **AGENTS rule 8 has a hole and it is now measured** (PF:25), together with the reason no arm in
  the suite can see it.
- **P5's whole new surface works against a real driver**: the token posture, the ungated assets,
  a real MJPEG preview, N83's suspend/resume with the frame-sequence restart it predicted, and a
  clean shutdown under open readers.
- **The vivid rung is green over the tree that shipped P5.**

### What it does not establish

- **Nothing about the BRIO under load, over time, or in a browser.** The preview was read by a
  Python client, not by Chrome; R1-web was not run in this session, and no page rendered any of
  these frames. "A browser behavior verified only through the JSON the page consumes is not
  verified" applies to the preview `<img>` exactly as it applies to everything else.
- **Nothing about what the BRIO's second capture node can *do*.** The raw walk says it offers
  `GREY` 340×340 and nothing else \[PF:27\]; no frame was taken from it, because this tool has no
  way to open it, and the inference that it is an infrared sensor is an inference. Whether it
  streams, what its frame rate is, and whether the colour node's square modes come from it are
  all unmeasured.
- **Nothing about 4K.** No photograph in this session was taken above 640×480 except through the
  two cameras whose first format is MJPG (1920×1080 and 1280×720). PF:26 is a finding about the
  *default*; nobody drove `--size 4096x2160` at the BRIO, so this run says nothing about whether
  that mode works, only that it is advertised.
- **The suspend/resume measurement is two cameras and one shape.** One photo per preview, no
  second photo inside the pause, no two-tab photo over HTTP, and no failing capture — the engine
  suite's scripted double owns those arms and this run did not reproduce them on hardware.
- **The `uvcvideo` finding is one device and one firmware.** Three cycles is a small sample; pan
  moved differently in each and only tilt repeated. Nothing here says what other gimbals do, and
  nothing says whether the OBSBOT's companion firmware could be told not to park.
- **Nothing about motors beyond the bounded arm.** No calibration sweep moved a motor, the motion
  arm's five positions are its whole travel, and the BRIO's motion cap was never exercised
  because its range is under it.
- **One cell of D11's matrix, and one only.** Loopback with a token, and anonymous against the
  same listener. `--http-insecure-loopback` was not exercised, no non-loopback bind was attempted,
  and nothing here says anything about N79's reverse-proxy shape.
- **No mutation floor and no parity run.** `just mutants` was not run and `wch`/`wchc` were not
  compared; E12 remains the parity evidence and it predates the BRIO.

### Two repository findings, recorded because each cost this session something

**One: the privacy gate and `.gitignore` disagree about `scratch/`.**

`.gitignore` says test captures land in `/scratch/` — "a camera frame never enters the repository
(design §5)" — and `scripts/gates/no-frame-bytes-in-repo.sh` walks the worktree through
`gate_find`, which prunes `target`, `.git` and `vendor` and **nothing else**. Eight JPEGs written
into `scratch/` during this session therefore made `just ci` red with eight lines of the form

```
FAIL no-frame-bytes-in-repo: scratch/…/brio-1.jpg is a committed jpeg image;
     images live only in corpus/images/ (design §5: a frame may contain a person)
```

The gate is not wrong to be strict about a frame in the worktree, and its message is wrong about
one word: an ignored path is not "committed" and cannot become so. The two instructions collide
for anyone who follows `.gitignore`'s own sentence, and the collision is invisible until a
hardware session takes a photograph by hand — the `hw_` suites never trip it because
`engine::store::TempStore` puts everything under `$TMPDIR`. Resolved for this run by moving the
frames out of the tree; the choice between teaching `gate_find` about `.gitignore` and deleting
that sentence from `.gitignore` is somebody's to make deliberately.

**Two: `gate_scratch_tree`'s fallback has no owner, and 76 copies of this repository were in
`/tmp` before this session started.**

```
scripts/gates/lib.sh:366
  dest="$(mktemp -d "${WCH_GATE_SCRATCH:-${TMPDIR:-/tmp}}/wch-tree.XXXXXXXX")"
```

`selftest.sh` exports `WCH_GATE_SCRATCH` into a directory its own `EXIT` trap removes, so the
whole-suite path cleans up after itself. **Every other caller falls through to `/tmp` and nothing
ever deletes the result** — which is what running a single `cases/*.sh` arm by hand does, the
ordinary move when a gate is being developed. Measured on this host at the start of this session:
**76 `/tmp/wch-tree.*` directories, 1 968 MB, all timestamped 11:37 — eight hours before this
session began**, so they are somebody else's runs and not this one's. Each is a `tar` copy of the
checkout minus `.git` and `target`.

This is not a correctness defect and the fallback is the right default (a gate must run without
the selftest around it); what it lacks is a trap. It is worth writing down because the cost lands
somewhere unrelated: `/tmp` here is a 16 GB tmpfs, the leak plus a live selftest took it to 80%
full, and the symptom is not "the gate failed" but *the shell intermittently refusing to run
anything*, which is a very long way from the line that caused it.

### The camera was put back

Every device this session wrote to was read afterwards and compared against the values it held at
the start: the OBSBOT at `pan 28800 / tilt −46800 / zoom 0 / focus 66` and its five
processing-unit integers at `50 / 65 / 60 / 50 / 70`; the Dell at `pan 0 / tilt 0 / zoom 100`; the
BRIO at `pan 0 / tilt 0 / zoom 320` and `white_balance_automatic=1`. The three values that do not
compare are the OBSBOT's `red_balance`, `blue_balance` and `white_balance_temperature`, which
PF:24 explains and which no write can restore while the automation that owns them is on.

---

## E16 — The R1-web browser rung's first run, and the two client defects it found, 2026-08-13

The first execution of the pinned Playwright + Chromium rung (design §3.1 R1-web, docs/7 P5d),
recorded once and not amended. It is dated evidence rather than case law: nothing below is a
deviation from a doc, and the two defects it names were fixed in the same working tree rather
than justified.

Its whole reason for existing is one sentence of rubric B7, which `crates/daemon/tests/
web_client.rs` quotes about itself in its own header: **a browser behavior verified only
through the JSON the page consumes is not verified.** P5c landed ten files of hand-written
HTML, CSS and ES modules and asserted, deliberately, nothing about whether any of them render.
This is the run that asked.

### The fixture

**Host:** the P4/P5 workstation, kernel `7.0.0-29-generic` (x86_64), on the tree at `8c27a81`
plus this sub-milestone's working tree. **Backend: the fake** — `testkit::fixtures::
synthetic_basic` with its MJPEG modes rewritten to 160×120, `web_client.rs`'s fixture and its
argument — so **no camera was opened and no frame anything captured could contain a person**;
every artifact the browser wrote went to the gitignored `scratch/r1-web/`.

**node** v24.18.0 · **@playwright/test** 1.62.1, exact, with `package-lock.json` committed
beside it · **Chromium** build 1234, `Google Chrome for Testing 151.0.7922.34`, launched from
Playwright's own download and asserted by the rung to be that build and that version.

```
Running 10 tests using 1 worker
  ✓  1 a sparse menu becomes a select carrying the device's own indices (191ms)
  ✓  2 an INACTIVE control is still usable and names what owns it (167ms)
  ✓  3 a current outside its declared range renders with no min and no max (163ms)
  ✓  4 a control type this build cannot name shows its raw discriminant (140ms)
  ✓  5 a clamp snaps the slider back and says both numbers (179ms)
  ✓  6 the preview paints successive frames and keeps painting across a photo (642ms)
  ✓  7 the calibration view tracks a sweep it did not start (191ms)
  ✓  8 the client loads with no credential and the camera is still refused (123ms)
  ✓  9 the page reports a lost socket and works again on the next one (269ms)
  ✓ 10 the browser this rung drove is the pinned one (1ms)
  10 passed (2.5s)
r1-web: 10 browser claim(s) and 79 assertion(s) ran in Chromium 1234
```

The last line is the harness, not the runner: `tests/browser/claims-reporter.mjs` counts one
entry per test and one assertion per top-level `expect` from inside Playwright, and
`web_browser.rs` compares that against `tests/browser/claims.json` on every green run. The two
numbers exist because the *decline* has to report them — a skip that cannot say what it did not
do is the skip that reads as a pass — and a number nothing checks is a number that rots.

### 1. The calibration view's status line was written by two views, and the second one won

**Found on the first run**, as `#calibration-status` reading `no calibration sessions recorded
for this camera` where the rung expected `watching every sweep this daemon runs`.

`app.js` handed the same element to `calibration.watchSweeps` and to
`calibration.showSessions`. The first is opened once at startup; the second runs on every
camera selection, milliseconds later, and overwrites it. So the sweep subscription's line was
never readable — including **the daemon refused a sweep subscription** and **the sweep stream
ended: …**, which are the two sentences that tell an operator the calibration view has stopped
being live. A view that had silently died looked exactly like a healthy one, which is the
"skip reads as pass" defect class wearing a UI.

Fixed by giving the subscription its own element: `#sweep-status` in `index.html`, `sweepStatus`
in `app.js`. Two elements rather than one because the two *lifetimes* differ — the session list
is re-read per camera and the subscription outlives every switch — and that is the property
that made one element wrong rather than merely crowded.

**Only a browser could find this.** Both writes are correct calls to a correct function with
correct arguments; nothing about the JSON is wrong, and no protocol test has an element for the
second write to overwrite.

### 2. A page opened with no token said the connection had closed, instead of saying what was wrong

**Found on the same run**, as `#connection` reading `the connection to wchd closed; reload the
URL wchd printed` on a page opened at `/` with no credential.

`rpc.js` registers its `close` handler before awaiting the open. A refused handshake fires
`error` **and then** `close`, so `connect` rejected — and `app.js` wrote its careful two-candidate
sentence, *"either the token this page was opened with is not this run's, or nothing is listening
on this port any more"* — and then the `close` handler fired `onClose` for a connection that had
never opened and replaced it. The page also disabled its photo button and tore down a preview it
never had.

That is the most common failure this client has: an operator opens `/` by hand after reading the
port off a log, or opens a stale URL from yesterday's run. The diagnosis was written and then
destroyed.

Fixed in `rpc.js` with a two-line `opened` flag: `onClose` means "the connection I gave you has
ended", and a connection that never started has not ended. `connect` rejects on the same event,
so the caller is told once, by the path that knows what happened.

### 3. Not a defect: the pair provenance a browser shows is `declared`, and it should be

The rung expected `(D3, measured)` on the INACTIVE control's note, because the fixture records
that pair as measured. It read `(D3, declared)`, and the expectation was wrong.
`engine::pairing::in_effect` is handed no measured pairs by any read verb — measuring pairs
writes to the camera and is its own operation (note **N30**) — so what the page renders is the
UVC table's claim, labelled as one. A page that said `measured` there would be claiming a probe
nobody ran. Recorded because the first reading of it was "the browser found a provenance bug",
and it took reading `in_effect`'s own doc comment to see that the browser had found the design.

### 4. Recorded, not fixed: a live preview and a sweep on one camera collide

Driving `wch_calibrate_sweep` from a second socket while the page previewed the same camera was
refused:

```
{"code":-32013,"message":"/dev/video0 is busy: held by an unidentified process",
 "data":{"kind":"busy","path":"/dev/video0","holders":[]}}
```

`engine::preview::while_suspended` is what lets a capture interleave with a live preview (note
**N83**), and `engine::photo::take` is its **only** caller — so a photo suspends the preview and
a *sweep* meets `Busy`. The refusal is correct in D13's vocabulary and correct about the device;
what is worth writing down is that the "Expected usage" section's own deployment produces it.
The owner "uses the web client from time to time … to calibrate them at the beginning of a
development run", and the client deliberately starts no sweeps — so the sweep comes from `wchc`
while the owner is watching the preview, which is exactly the arrangement above.

Nothing was changed for it. Suspending a preview for the duration of a sweep is minutes of black
picture rather than one frame's pause, so whether N83's mechanism should extend to a sweep, extend
per *sample*, or stay where it is, is a design decision with an owner in it and not a fix. It is
named here so P5e's review meets it as a recorded finding rather than as a surprise. The rung's
calibration claim aborts the preview request to get a free camera, and says so in the file.

### 5. What the platform does that the design assumed otherwise

**Chrome fires exactly one `load` event for a `multipart/x-mixed-replace` `<img>`** and replaces
every later part silently. The obvious way to assert "the preview paints successive frames" —
count `load` events — measures zero after the first, and measuring zero looks identical to a
preview that stopped. What the rung does instead is write `brightness` through the panel and
watch the picture's **mean luma** follow the fake's declared gain (`fake::frames`: brightness is
a monotonic gain from 25% to 200%), with the element's identity checked at the end so a page that
re-opened its request cannot pass. That is an *ordering* over a device model rather than a pixel
value, which is the rule the hardware suites already run under.

**`BrowserContext.setOffline(true)` does not close an established WebSocket** in this Chromium,
which is worth recording because it looks as though it should. The reconnect claim proxies the
socket with `page.routeWebSocket` and drops it from the middle instead, so the page's socket
really ends while the daemon is still up and still holding its side.

### The decline, driven

The rung runs on every push where the host has node, and declines everywhere else — so its
decline is the line standing between "this was checked" and "nobody knows", and it was exercised
rather than trusted:

```
SKIP r1-web: no usable node at `/nonexistent/node` — install Node.js, or point WCH_E2E_NODE at
one — node is a test-host convenience here and never a build dependency, so every crate still
builds without it; 10 browser claim(s) and 79 assertion(s) were not run
r1-web:   - a sparse menu becomes a select carrying the device's own indices (5 assertions) — …
[ten lines, one per claim]
rung-web: SKIPPED — the rung declined, 1 time(s), each naming what was missing and what it
therefore did not claim
```

Three preconditions, three different sentences, each with its own remedy, all three driven by
`a_declined_rung_names_what_was_missing_and_counts_what_it_did_not_run`. `.config/nextest.toml`
gives the binary `success-output = "final"` so the decline is visible in a plain `just ci`
rather than printed into a void, which is the difference between a counted skip and a silent one.

### What this run does not establish

- **Nothing about a real camera.** It drives the fake, on purpose: the rung is deterministic and
  needs no device, and design §3.1 says so. What a browser does with a *real* MJPEG stream at a
  real frame rate is unexercised.
- **Nothing about Firefox or Safari** (owner ruling, design §2.7; docs/9's gaps list already
  carries this).
- **Nothing about a second tab.** Two browsers previewing one camera is D12's arrangement and
  `preview.rs` asserts it at the protocol level; no browser claim opens two contexts.
- **Nothing about the page under a proxy or over TLS.** `credential.js` upgrades `http:` to
  `ws:` and `https:` to `wss:`, and only the first half has ever run.

---

## N84 — Test scratch has one home and it is under `target/`, where an exclusion stopped being an optimisation and became the thing that stops a copy containing itself

**The ruling (owner, 2026-08-12):**

> "Given the repeated failures coming from tmpfs size limits, let's move all the temporary data
> generated by our tests under a subdirectory of `target/`, which is also marked as a cache /
> temporary directory."

**Doc:** none. This is a repository fact, not a design one — no D-number changes, and design §5's
privacy rule is *strengthened* by it rather than amended (see "what this resolves" below).

**Repo:** `scripts/gates/lib.sh` gains `gate_checkout`, `gate_scratch_root`,
`gate_socket_scratch_root` and `gate_scratch_sweep`; `schema::paths` gains `scratch_root` and
`SCRATCH_DIR`; `engine::paths` gains `scratch_dir`; `scripts/gates/scratch-has-one-home.sh` and
its thirteen arms are new; `just scratch-sweep` is new; the `/scratch/` line is gone from
`.gitignore`; `webcam-handler-testkit` loses its `tempfile` dependency, which nothing in it names
any more.

### What forced it was a lifecycle bug, and the distinction is the whole design

The measurement is E15's second finding: **76 `/tmp/wch-tree.*` directories, 1 968 MB**, all
timestamped eight hours before that session began, in a 16 GB tmpfs. `gate_scratch_tree` copies
the checkout so a selftest arm can mutate a copy and run the *real* predicate against it;
`selftest.sh` exports `$WCH_GATE_SCRATCH` into a directory its own `EXIT` trap removes, and
**every other caller fell through to `/tmp` and nothing ever deleted the result** — which is
precisely what running one `cases/*.sh` arm by hand does, the ordinary move while a gate is being
written.

Three numbers say why the move is safe and why it is not the fix on its own:

- `gate_scratch_tree` already copied with `--exclude=.git --exclude=target`;
- the worktree minus those is **46 MB** — one copy;
- **`target/` alone is 233 GB.**

So nothing was ever copying too much. 76 × 46 MB is a leak, not a size problem, and a leak is fixed
by something deleting what nobody came back for — never by a larger filesystem, which only changes
how long it takes to notice. What the ruling fixes is the *blast radius*: the same
leak on the disk that holds `target/` is 1.9 GB out of 156 GB free instead of 1.9 GB out of a
16 GB tmpfs that the shell was also trying to run in. What fixes the leak is `gate_scratch_sweep`,
below.

`target/` earns it three times over and only the first is the ruling's own wording: cargo writes
`target/CACHEDIR.TAG`, so the directory *declares* itself regenerable to anything that reads that
file; `/target/` is gitignored; and `gate_find` prunes `target`, so nothing under it is visible to
a gate that walks the tree.

### The trap: `--exclude=target` changed jobs

Before the ruling the destination was in `/tmp` and could not be part of the source, so
`--exclude=target` was a size optimisation — remove it and you get a slow, wasteful copy. **With
the destination inside the source tree the exclusions are what stop a copy from containing
itself.** Nothing tested it in either job.

Both failure modes were measured on a synthetic tree rather than reasoned about, and the reasoning
turns out to be half right:

- `--exclude=target` removed, the derived exclusion still there: the copy comes out holding
  `target/` and **no** nesting — 233 GB per arm, around two hundred arms per selftest run.
  Ruinous, and bounded.
- both removed: the copy came out holding one nested `…/target/wch-scratch/wch-tree.…` of itself.
  How deep it goes is a race between `tar`'s directory read and the extraction writing into it, so
  "it recurses forever" is an over-claim and "it recurses" is not.

Two things hold it, and they are deliberately of different kinds:

1. **A derived exclusion.** `gate_scratch_tree` now also excludes the scratch root it is copying
   into, computed from where the copy is actually going rather than written down, so it survives
   an edit to the transcribed exclusions above it. It is unconditional — when the root is
   elsewhere the strip leaves an absolute path and the pattern matches nothing — because an `if`
   around it is a line whose body somebody can delete and leave valid shell behind.
2. **`scripts/gates/scratch-has-one-home.sh`**, which *performs* the copy: it builds a synthetic
   tree with a `target/` in it and the scratch root inside it, runs the shipped
   `gate_scratch_tree` over it, and asserts the result contains the tree's two files, no `target`,
   no nested scratch root, and lands under the root it was given.

**And a static half beside the dynamic one, which is a real concession.** The honest dynamic test
for "somebody deleted the exclusion" is to run the shipped copier without it and look — and that
copies 233 GB into the tree. A gate does not get to perform the defect it checks for, so the
*removal* is caught by reading the copier in the tree under test, which is exactly what a selftest
arm can seed. `fail_case_the_copier_stops_excluding_target` and
`fail_case_the_copier_stops_excluding_the_scratch_root` are both red without ever running a
copier.

### The one exception, measured rather than assumed: `AF_UNIX`

`sockaddr_un::sun_path` is 108 bytes on Linux, 107 once the NUL is counted
(`schema::limits::MAX_UNIX_SOCKET_PATH_BYTES`). Measured for the deepest socket any gate binds,
`<scratch>/<gate>/<runtime>/webcam-handler/wchd.sock`, with the selftest's own directory in the
middle:

```
 94 bytes   /tmp/wch-selftest.XXXXXXXX/wch-socket-activation.XXXXXXXX/journal-run/…/wchd.sock
146 bytes   <checkout>/target/wch-scratch/wch-selftest.XXXXXXXX/wch-socket-activation.XXXXXXXX/…
 41 bytes   /tmp/wchXXXXXXXX/webcam-handler/wchd.sock                     (TempRuntimeDir)
 93 bytes   <checkout>/target/wch-scratch/wchXXXXXXXX/webcam-handler/wchd.sock
```

146 against a bound of 107, and **37 of those bytes are `/home/pwnall/workspace/webcam-handler`**
— a number that is a property of where somebody cloned this repository. Moving these under
`target/` would not merely break three gates on this machine; it would make their verdict a
function of the checkout path, which is N52, N66 and N68 for the fourth time (a verdict that moves
with the machine, wearing the word a real finding gets). Note that even the one line that *fits*,
`TempRuntimeDir` at 93, fits with fourteen bytes of headroom that belong to somebody's home
directory.

So: **the bulk moves, and a directory whose job is to hold a socket keeps the shortest path
available.** `gate_socket_scratch_root` (used by `socket-activation.sh`, `uds-permissions.sh` and
`cli-parity.sh`) and `engine::paths::TempRuntimeDir` are that decision, made twice because there
are two languages and argued in full in both. What is on the short root is a socket, a fifo and an
empty state tree — kilobytes, and not what filled anything. `$WCH_GATE_SCRATCH` deliberately does
*not* redirect it: that variable's job is "put the bulk on another filesystem", and honouring it
here would import a caller's path depth into a kernel constant.

`uds-permissions.sh` was checked separately for the thing it actually asserts — it `chmod 0755`s
its own scratch on purpose, so that the daemon's 0700 socket directory is the only barrier left
standing — and that is unchanged: it still owns the directory it widens, and it is still the only
thing above the socket directory.

### What reclaims after `kill -9`

`gate_scratch_sweep`, called by `selftest.sh` at the start of every run with a day's grace, and by
a person as `just scratch-sweep` with none. This is the half the 76 copies prove is necessary: a
`trap`, a `Drop` and `reclaim_scratch` are all cleanup that happens *when a run finishes*, and the
runs that leak are exactly the ones that did not. Only the next run can reclaim an abandoned
directory.

It takes entries named `wch*` in both roots, older than the age given. What it does **not** cover,
stated rather than discovered later:

- **cargo-mutants' own build trees.** They are `cargo-mutants-*`, its name and not ours, and they
  live under a root this project does not choose.
- **The mutation floor's build root itself**, which is the one thing the ruling deliberately did
  not move. E7 measured about seven mutants a minute with build directories on tmpfs against under
  one a minute on the disk that holds `target/`; moving them would be a knowing 7× regression, the
  floor already has the budget check and the `NO VERDICT` exit N66 gave it, and it is a rung rather
  than a `just ci` step. It carries a `wch-scratch-exempt:` marker naming that measurement, and the
  gate prints it on every run.
- **A caller's own `$WCH_GATE_SCRATCH`** pointed at a third filesystem: the sweep asks
  `gate_scratch_root`, so it sweeps wherever the *sweeping* process is pointed, which is not
  necessarily where the leaking one was.
- **Anything a person named something other than `wch…`.** That is a floor and not tidiness:
  `target/wch-scratch/r1-web/` holds the browser rung's traces and screenshots, which exist to be
  opened *after* a failing run, and `hwval-2026-08-12/` holds a hardware session's transcripts. A
  sweep that took those is a sweep nobody dares run.
- **A live run's directories younger than the age.** `just scratch-sweep` defaults to 0 and would
  take a concurrent run's scratch; the automatic call from `selftest.sh` uses a day, which is
  longer than anything here and shorter than an accumulation nobody notices.

### What this resolves, and it is E15's first finding

E15 recorded a collision: `.gitignore` said test captures land in `/scratch/`, and
`no-frame-bytes-in-repo.sh` walks the worktree through `gate_find`, which prunes `target`, `.git`,
`vendor` and `node_modules` and **not** `scratch`. Eight JPEGs written into `scratch/` by hand
during hardware validation therefore made `just ci` red with eight lines calling gitignored files
"committed". E15 left the choice open — teach `gate_find` about `.gitignore`, or delete the
sentence from `.gitignore` — and the ruling takes a third option that is better than both: **one
directory that is ignored by git and pruned by the walk.** Teaching the walk about `.gitignore`
was the tempting one and it is the wrong one, for the reason `lib.sh` already records about
`git ls-files`: a frame written into the tree and not yet committed is precisely what that gate is
for.

So `/scratch/` is gone from `.gitignore`, the R1-web rung's traces move from `scratch/r1-web/` to
`target/wch-scratch/r1-web/` (P5d's choice, superseded), and the two directories that were sitting
in `scratch/` on this workstation moved with them. **No gate was weakened to get there** — the
privacy walk still sniffs every file it finds, still refuses to ask git what is committed, and now
sees strictly less because the only thing that left its view is a directory full of machine output
that `target/` already covered.

### Two tests changed what they check, and it is a strengthening rather than a retreat

Said loudly because a test that softens its claim under pressure from the change it is testing is
how a suite stops meaning anything. `webcam-handler-cli`'s `a_photo_never_lands_in_the_repository`
and `nothing_a_calibration_writes_lands_in_the_repository` both asserted
`!path.starts_with(repo_root)` — "the scratch directory really is outside the tree" — and this
ruling makes that false on purpose. What replaced it:

- both are renamed `…_lands_where_the_repository_can_see_it`, and both assert the **positive**: the
  photo, and the whole session tree with its sample photos, are under `schema::paths::scratch_root()`
  and nowhere else. The old wording could only ever say where a frame is *not*, which is the weaker
  of the two things to know about a frame.
- the general law moved to where the choice is made, as `schema::paths`'s own
  `the_scratch_root_is_inside_a_build_directory_that_gitignores_itself`: the root exists, its parent
  is `target/`, and the `.gitignore` beside `target/` **names `/target/`** — read out of the file
  rather than remembered, because "a frame written here cannot be committed" is true because of that
  one line and for no other reason. **Nothing asserted that before**, in either language: deleting
  the line was a silent change until this note.

Three assertions where there were two, one of them about a fact nothing checked, and none of them
about a directory being somewhere else.

### What can go red

`scripts/gates/scratch-has-one-home.sh`, over 58 shell scripts and 149 Rust files, with thirteen
arms: four green (the shipped tree; a pristine copy; prose naming every forbidden spelling in both
languages; an exempted line with its reason) and nine red (a script reaching for `$TMPDIR`; a
`mktemp` with no directory; `tempfile::tempdir()`; `TempDir::new()`; the shell home renamed; the
Rust home renamed; the two languages naming different directories; and the two exclusions, each
deleted from the copier).

**It is a new gate rather than an arm of `atomic-write-home.sh`**, which was the near miss: same
shape of claim ("one home, and here is the count of everything that bypasses it"), different
subject entirely — that gate's population is the `write_json_atomic` call graph and its prose
argues about `fsync` and `rename`. A gate whose subject is two unrelated laws has a failure message
that has to ask which one you broke.

**It has one hole and it is named in the file:** the predicate and its own case file are exempt
from the spelling scan, because both have to hold every forbidden spelling on purpose. A genuine
second reach for `$TMPDIR` hidden in the case file that exists to seed second reaches would be
invisible. The alternative was an exemption marker on each of four seeding lines, which buys the
same blindness one line at a time.

**No `g5` row.** The g5 criteria are what P5 — the web client — establishes, and this establishes
nothing about a browser; the closest it comes is moving where that rung's traces land. It is
covered where infrastructure is covered: every phase g0–g4 carries a
`./scripts/gates/run-all.sh` row, and the new predicate is in that population by existing, which
is the arrangement docs/9's derived-population rule exists for.

**Retires when:** never, as a ruling. The *arrangement* is up for revision if the socket exception
ever becomes reachable from a checkout path deep enough that even `/tmp` does not fit — the honest
answer then is an abstract socket or a `/run/user/<uid>` directory, not a shorter prefix — or if
`target/` stops being a directory this project may write to, which would be cargo changing what a
build directory is.

---

## N85 — The formats are re-ranked and the driver's enumeration order is demoted to the last tiebreak, with the lossiness tiebreak coupled to the sink

**The ruling (owner, 2026-08-13),** in answer to \[PF:26\]:

> "Let's re-rank the formats offered by the device and ignore the ordering. Our intended usage
> benefits from higher-quality photos, even if they cost more bandwidth or latency. So, let's
> re-rank — higher-resolution formats are preferred to lower-resolution formats, and less lossy
> encodings are preferred to more lossy encodings."

**Doc:** design **D5** says format negotiation "prefers the requested format/resolution/rate", and
`StreamRequest::choose` implemented the unspecified case as *the device's first enumerated format
at its first size entry's maximum*, arguing: "the order `VIDIOC_ENUM_FMT` returns is the driver's
own preference, and second-guessing it is how a tool ends up defaulting to a mode the camera is
worse at."

**Repo:** `schema::camera` gains `Lossiness` and `SinkFidelity`; `schema::capture` gains
`rank_formats`, `ChoiceReason`, `ChosenFormat::reason`, `StreamRequest::sink_fidelity`,
`StreamRequest::for_sink` and `PhotoRequest::stream_for_sink`; `engine::photo::take` and
`engine::calibrate` derive the sink's answer before the request reaches a device. D5 is amended in
the design, in the design's voice, with the amendment ledger row beside it.

### This is not a re-litigation, and the distinction matters

D5's argument was not wrong when it was written and it is not refuted now. What happened is that a
consumer it did not know about was written down. The Expected-usage statement of 2026-08-12 named
an **agent that photographs a device under test and diffs the photographs**, and item 3 said what a
re-encode costs it: "a pipeline that silently re-encodes is a pipeline that fabricates evidence in
a test." Against that consumer, a driver's own idea of its preferred mode is not evidence about
which photograph is better — it is evidence about which mode the driver would rather serve, which
is a different question and one nobody in this deployment is asking. The old rule was sound
reasoning from the facts available; the ruling overrules it with a fact that was not.

\[PF:26\] is the measurement that charges for it, and it is where the numbers live: the BRIO's
default photograph was a re-encoded 640×480 YUYV frame from a camera offering verbatim 4096×2160 —
**3.5% of its pixels** — and the Dell's was a re-encoded 640×480 NV12 frame from a camera with a
3840×2160 MJPG branch. Two of five cameras delivered the camera's own bytes by default.

### What each camera defaults to, before and after

Measured by running the five committed profiles through the chooser, which is what
`every_committed_profile_resolves_an_unspecified_request_to_its_largest_mode` does on every run:

| camera | before | after | pixels |
|---|---|---|---|
| chicony-rgb | MJPG 1280×720, verbatim | **MJPG 2592×1944**, verbatim | 0.9 MP → 5.0 MP |
| chicony-ir | GREY 640×360, re-encoded | GREY 640×360, re-encoded | unchanged |
| dell-u3224kb | NV12 640×480, re-encoded | **MJPG 3840×2160**, verbatim | 0.3 MP → 8.3 MP |
| logitech-brio | YUYV 640×480, re-encoded | **MJPG 4096×2160**, verbatim | 0.3 MP → 8.8 MP |
| obsbot-tiny3 | MJPG 1920×1080, verbatim | **MJPG 1920×1440**, verbatim | 2.1 MP → 2.8 MP |

Four of the five moved, three of them by more than an order of magnitude, and the count of cameras
delivering the camera's own bitstream by default goes from two to four. The fifth is the Chicony IR
sensor, which has no compressed format at all and one size — the honest case, and the reason the
"after" column has a re-encode in it.

**On every camera this project has met, resolution-first selects MJPG, which is also the verbatim
path.** So the ruling and AGENTS' "verbatim camera JPEG when the sink allows — byte fidelity is the
product" agree everywhere there is evidence, and the ruling costs nothing on known hardware. That
is worth saying plainly, because it means the two decisions below are about cases that are
currently hypothetical, and a reader should know which parts of this entry are measured and which
are argued. `the_ruling_costs_nothing_on_the_hardware_this_project_has_met` asserts the agreement
over the corpus, and names the IR sensor as the one camera the claim is vacuous for.

### Decision 1: resolution is the primary key, lossiness the tiebreak

The owner's sentence lists resolution first and lossiness second, and it admits the other reading —
"less lossy, and among equally lossy the largest" is a grammatical reading of the same words. The
reading taken is **resolution primary, lossiness secondary**, and it is written down here because a
future reader deserves to know which was taken rather than to re-derive it from a `max_by_key`.

The reason is the deployment. Item 2 makes repeatability beat prettiness, and both readings are
equally repeatable; item 3 makes byte fidelity the product, and that argues for lossiness; but the
thing the agent is doing with the photograph is *looking at a device under test*, and 8.8
megapixels of a slightly quantised display beats 0.3 megapixels of an unquantised one at telling
whether a driver drew the right thing. Resolution is the key that changes what is legible in the
picture; lossiness is the key that changes how faithfully the legible thing is recorded.

The **Dell exercises the tiebreak for real**: NV12 and YUYV stop at the same 1920×1080, and 4:2:0
chroma subsampling keeps a quarter of the chroma where 4:2:2 keeps half, so YUYV wins that pair —
from second place in the driver's order, which is the demotion in one line.
`the_dells_two_uncompressed_formats_tie_and_the_less_subsampled_one_wins` asserts it from the
committed document. It is a pair the *default* never has to decide, because MJPG beats both
outright on the whole tree — which is itself the shape of this ruling: the tiebreak is real and
rarely reached.

### Decision 2: the tiebreak is coupled to the sink

The case the ruling leaves open: a device offering an uncompressed format at the **same** maximum
resolution as a compressed one. No camera in `corpus/` has it. "Less lossy" alone picks the
uncompressed one, which must then be **encoded** — so on a `.jpg` sink the ruling would throw away
the camera's own bitstream to hand our encoder a frame it had no reason to touch, which is exactly
what Expected usage item 3 warns about and exactly what E6 exists to prevent.

The resolution is that **lossiness is measured over the whole path from sensor to file, not over
the driver's buffer**:

- into a **JPEG** sink, the compressed format wins. It arrives byte for byte (E6, and the EXIF
  stamp is a header splice \[PF:16\]), so nothing this program did is in the answer; the
  uncompressed candidate would acquire artefacts the camera's own encoder did not make.
- into a **PNG or PPM** sink, the uncompressed format wins. That sink adds no loss to whatever
  arrives, so an uncompressed source reaches the file with the sensor's own samples, while the
  compressed candidate arrives having already been through the camera's quantiser and would have
  that loss written into a lossless container.

This is one rule with two answers rather than an exception, and it is why `SinkFidelity` exists.
Three consequences are part of the decision rather than of its implementation:

- **Determinism survives.** The choice is a pure function of (format tree, sink), so two photographs
  an hour apart with the same command still differ only where the device does — Expected usage item
  2 is intact. What is *not* guaranteed is that `-o a.jpg` and `-o a.png` stream the same mode, and
  they should not: they are different products of the same camera.
- **`SinkFidelity` is derived, never sent.** `PhotoRequest::stream_for_sink` is the one place it is
  written, from the sink the same request already carries, at the moment the request reaches a
  device. The field on `StreamRequest` is `#[serde(skip)]`, so it does not cross the wire: a client
  that could set it would be able to say `png` in one field and `.jpg` in another, and the sink is
  the authority on what the sink can carry. It rides on the request rather than beside it because
  T1's `start_stream` takes one `StreamRequest` and the backends are what call `choose` — that is
  the only seat from which the chooser can be told, and the alternative is a parameter on the
  backend trait for a bit only one of its callers has. `just generate` regenerates both committed
  artifacts unchanged, which is the evidence that the field stays out of the wire vocabulary.
- **`wch` and `wchc` cannot disagree.** Because the derivation happens where the photo is taken
  rather than where it is typed, the in-process CLI and the daemon compute it from the same sink;
  the wire carries nothing new and the parity gate has nothing new to compare.

### The third key, which is not in the ruling's words: an unrecognised FourCC ranks last

The ruling says higher resolution wins. Taken literally that prefers an `H264` or `HEVC` mode —
formats offered at a device's *largest* sizes, precisely — which `imaging::decode` cannot decode, so
the photograph would be a `FormatUnsupported` instead of a picture. The hazard existed before (the
old rule would have taken an unknown format that happened to be enumerated first) and **the ruling
makes it likelier**, because hardware encoders advertise the big modes.

So a FourCC this build cannot name ranks behind every one it can, ahead of resolution, and the
argument is stated rather than assumed: this is not a claim that an unknown encoding is bad, it is
the observation that nothing here can claim it is *good*, and that the ruling is about which
photograph is better rather than which mode number is biggest. It is **ranked rather than
filtered** — AGENTS rule 6 — so a device offering nothing else still resolves to something
deterministic at that format's largest size, and the caller gets the same typed refusal it would
have got anyway.

That key rests on the schema's five named FourCCs being the same five `imaging::decode::SourceFormat`
can read. Nothing structural holds those two sets together, so
`the_format_ranking_never_prefers_a_format_this_crate_cannot_decode` holds them, in the crate that
owns the decoder, in both directions.

### What did not move

**An explicit request still wins.** A caller that names a format and a size gets them, or a typed
refusal; the ranking is only for the request that named neither, and
`an_explicit_request_beats_the_ranking_in_both_of_its_halves` asserts it with its own inverse
beside it. This is also why PF:26 could close with "nothing is changed" on the day it was measured:
`--size` and `--format` reached the right modes then and reach them now.

**The negotiated result is still surfaced whenever it differs from the request.** The ranking
chooses; `NegotiatedStream::diff` and `Adjustment` describe. The amendment moves the first and does
not touch the second, and the daemon's
`the_negotiated_format_and_size_are_surfaced_when_they_differ_from_the_request` is unchanged and
still green.

### What it broke, including one thing nobody asked for

- **A fixture stopped describing the device it was written for.** `crates/daemon/tests/preview.rs`
  shrank the synthetic camera's MJPG list to 160×120 and left YUYV at 640×480, because under the old
  rule MJPG won for being *first* whatever its sizes said. Under the ruling that profile is a camera
  whose best photograph is uncompressed — correctly — and two preview tests went red on the verbatim
  claim. The fixture now shrinks both formats (320×240 MJPG over 160×120 YUYV) and says why. **This
  is the general shape of what the ruling breaks**: any device or fixture whose compressed format is
  its *smallest* now defaults to an uncompressed one, which is the ruling working rather than
  failing, and is the case PF:26's own retirement condition anticipated from the other side.
- **The test suite got three times slower, and that was worth fixing rather than accepting.**
  `just test` went 29 s → 86 s. The cause is not the product: the fake *synthesises* frames in
  software, and `testkit::fixtures::synthetic_basic` offers 3840×2160 MJPG, so every default-request
  photo in a calibration-sweep test became an 8-megapixel render and JPEG encode — 21 of them per
  sweep. The sweep suites now name a size, which is the mechanism the ruling explicitly preserves,
  and the runtime is back to 28.7 s. The fixture is deliberately not shrunk: a 4K mode in a shared
  fixture is *more* valuable now that the ranking is what picks it.
- **A calibration sweep of a 4K camera is now a sweep at 4K**, on real hardware as well as in tests,
  because the sweep asks the device the same question the photo verb asks (deliberately — a sweep
  whose modes differed would produce samples nobody could reproduce with `wch photo`). Twenty-one
  captures, decodes and writes at 8.3 megapixels instead of at 0.3 is a real cost to a
  start-of-run activity a human is waiting on (Expected usage item 9). Nothing is changed for it
  here: the owner accepted "more bandwidth or latency", `--size` is one flag away, and the honest
  time to decide whether a sweep should default smaller than a photograph is when somebody has run
  one and been annoyed.

### What can go red

Seven hand-applied mutants, each watched red at workspace scope over all 1 119 tests and restored:

| mutant | tests red |
|---|---|
| the ordering reversed (`max_by_key` → `min_by_key`) | 23 |
| the explicit request ignored | 5, plus the Chromium rung timing out |
| the size rule reverted to the format's *first* entry | 6 |
| the tiebreak dropped (fidelity out of the key) | 4 |
| the tiebreak made primary (fidelity above pixels) | 3 |
| the negotiated-format report suppressed | 2 |
| an unknown format sorted to the front | 1 |

Two of those numbers are worth reading rather than counting. **The tiebreak mutants are invisible
to the corpus**: on all five committed profiles, ranking by fidelity first and by resolution first
give the same answer, because MJPG is both the largest and the compressed format on every one of
them. That is the same fact as "the ruling costs nothing on known hardware", seen from the mutation
side, and it is why the tiebreak's evidence is fixtures rather than devices. **The unknown-format
key is held by one test** — the key this entry argues hardest for, and the one nothing else in the
workspace happens to cover, because a camera with an `H264` 4K mode beside an `MJPG` 1080p one is a
device nobody here owns. The negotiated-format mutant was held by one test until this session added
the second; suppressing the pixel-format arm of `diff` left every size-adjustment assertion in the
workspace green, which is a reminder that "the difference is surfaced" is three claims and each
needs its own.

**No `g5` rows.** The g5 criteria are what P5 — the web client — establishes, and this establishes
nothing about a browser: it changes which mode a camera is asked for, one crate below anything the
page can see. It is a D5 change and its evidence is where D5's evidence is — the schema's unit
tests, the corpus walk in `webcam-handler-fake`, and the engine's photo tests, all of which are
already in g0's and g1's counted populations by package. A g5 row here would claim P5 asked for it.

**Retires when:** a device is measured whose largest mode is one the tool photographs *worse* —
which is the case D5's original argument was written against and which nothing has yet seen — or
when the owner rules again. The sink coupling retires separately and on its own evidence: the first
camera that offers an uncompressed format at the same maximum resolution as a compressed one makes
Decision 2 measurable instead of argued, and it should be re-read against that device rather than
trusted.

---

## PF:28 — A snapshot taken *after* a `uvcvideo` cycle records a position nobody commanded: the reload swaps the driver's acknowledgement for the device's own answer

**Measured** 2026-08-13 on kernel `7.0.0-29-generic` (x86_64), five cameras attached, against the
OBSBOT Tiny 3 (`3564:ff02`, interface `3-1:1.0`) — the one device in this house with a motor — by
two sessions the same day with different instruments: a photographic metric, and the R3 hotplug
arm comparing every control on every camera. Continues the docs/6 §1.2 registry; cite it as
`[PF:28]`. \[PF:25\]'s amendment of the same date carries the transcripts and the calibration;
this entry is the *hazard*, written for whoever next puts a snapshot and a restore around
something that takes a device away.

\[PF:18\] established that `pan_absolute` reads back the **commanded** position rather than the
mechanism's, and drew the consequence for a write: `{requested, applied}` means requested versus
accepted. This is the consequence for a *reload*, and it is sharper than the one PF:18 drew,
because it turns a control's value into a claim about two different things depending on when it
was read.

### The measurement, in one line each

- Before a cycle, `tilt_absolute` reads what was last written to it. After the cycle, on the same
  camera, with nothing written in between, it reads something else — seen repeatedly in both
  sessions, tabulated in PF:25's amendment.
- The difference is **not** a constant: from a commanded −46800 it reads −43200 (three trials,
  identically), from −36000 it also reads −43200, from −100800 it reads −50400, from 0 it reads
  −46800. What is stable is the destination band — 12–14° down — not the offset.
- **The aim does not move with it.** Photographed against a metric calibrated so that a commanded
  2° move scores 26× the noise floor, three cycles scored 0.40–0.74 — the floor. On a second scene
  with a worse floor (1.02, against 7.93 for 2°) a cycle scored 1.04, and a cycle after fifteen
  minutes with nothing streaming scored 1.10.
- It is not every cycle. In the run where all 73 controls on five cameras were compared across one
  cycle, nothing changed on any of them, tilt included — and the cycles that *did* move the number
  were the ones where a value had been written with nothing streamed since. Whether streaming is
  what commits a commanded position on this device is a reading and not a measurement; what is
  measured is that a written-then-cycled control reads differently and a streamed-then-cycled one
  did not.

**The reading, marked as a reading.** The pre-cycle number is the driver's acknowledgement of a
command and the post-cycle number is the device's own answer, which the reload forces it to give
because the acknowledgement did not survive the unload. Two facts support it and neither proves
it: the post-cycle value is insensitive to *what* was commanded, and it is stable under further
cycles once reached. Which of the two numbers describes the head is exactly what V4L2 will not
say (PF:18), so this stays a reading. What is measured — and all the fix below needs — is that
**the two numbers differ, and a reader cannot tell from the value which one they are holding.**

### The hazard, which is the inverse of the one that was expected

\[PF:25\] worried that restoring pan and tilt across a driver cycle would "restore the number and
hope about the head". The exposure runs the other way and it is worse:

> A snapshot taken **after** the cycle records −43200 for a camera nobody moved. Restoring from
> that snapshot does not fail to correct an error — it *introduces* one, by commanding a position
> the operator never chose, and reports a complete restore while doing it.

Two properties make it dangerous rather than merely wrong. It is **silent**: `engine::snapshot`'s
report for the swapped ordering is `AlreadyCorrect` — nothing was written, because nothing needed
writing, because the snapshot recorded exactly the state it was supposed to undo. And it is
**invisible in a diff**: snapshot-then-disturb-then-restore and disturb-then-snapshot-then-restore
are the same three calls, and a build with them the wrong way round looks like a build with them
the right way round. It is the shape docs/8 Part C keeps naming from other directions — a check
that passes for a reason unrelated to what it is about. Here the assertion "the camera is where the
snapshot found it" passes because the snapshot and the camera agree about a state neither of them
should be in.

### What it costs this tool, and what landed

1. **Any snapshot/restore path around a driver reload, a replug, or a device reset needs an
   ordering it can assert**, not an ordering it intends. `engine::snapshot::restore_across` is that
   door: it takes the instant the disturbance began and refuses a snapshot stamped after it, before
   anything is written. The refusal is `IllegalTransition` in `crate::refusal`'s convention, naming
   both stamps, and its inverse is a unit test that goes red at every `just ci`.
2. **The R3 hotplug arm now snapshots before its cycle and restores after it** (note **N86**), which
   is fix 1 of the two \[PF:25\] named, chosen by the owner. It is the first arm in that file to
   write a motorized control outside `hw_motion_*`, and the argument for that is this entry's
   measurement rather than a convenience: what it writes is the aim the operator had, and the aim
   is what the arm's cycle was leaving wrong.
3. **What the fix does not claim.** It restores a *command*. V4L2 offers no way to verify a
   mechanism's position \[PF:18\], so "the head is back" is believed, not measured. What licenses
   believing it is the calibration above: on this device, on two scenes, commanding an absolute
   position returns the picture to within the noise floor of a metric that resolves 2°.
4. **One regime is characterised and one is not.** PF:25 records two cycles that took tilt to
   −298800 with the head hanging down, and nothing measured on 2026-08-13 came near that. Anyone
   who meets it is in the half nobody has measured; see PF:25's amendment for what would settle it.

**Not corpus-shaped**, for PF:18's reason: this is a behaviour under an event, not a field in a
descriptor. The profile records `tilt_absolute`'s range, step and flags and every one of them is
unchanged across a cycle. The evidence is these transcripts and the arm's own printed line — which
is the point of it printing what a cycle moved, in the run's own output, instead of leaving the
next reader to reconstruct it from outside the suite as PF:25 had to.

**Retires when:** a kernel or a firmware is measured on which a control's post-reload reading
equals the command the driver was holding — at which point the two numbers stop being different
questions and the ordering stops mattering — or when V4L2 grows a way to read a mechanism's actual
position, which would let the restore be verified instead of issued \[PF:18\].

**Adjacent:** PF:18 (the read-back is the command; this is its family), PF:25 (the entry this came
out of, and its amendment), PF:24 (the AWB drift that had to be removed before the metric meant
anything, on this device as well as on the BRIO), PF:22 (the same event, the half that changes
nothing), N86 (the fix and what can go red), E9 (the arm that performs the cycle).

---

## N86 — The hotplug arm snapshots before its cycle and restores after it, and the ordering is a value the act produces rather than the order of two lines

**Doc:** AGENTS rule 8 — "Leave the camera as you found it. Snapshot before, restore after… tests
assert restoration." Design **D4** is the machinery; \[PF:25\] found `just smoke-hw` breaking the
rule at the one camera with a motor, named two possible fixes and made neither.

**The ruling (owner, 2026-08-13):** fix 1 — "the hardware suite's `hw_hotplug_*` arm must snapshot
the camera's controls before the cycle and restore them after". The alternative it was chosen over
was excluding the hotplug arm from runs where a PTZ camera is aimed, which buys the rule by giving
up the evidence.

**Repo:** `engine::snapshot::restore_across`; three helpers and a restore in
`crates/backends/v4l2/tests/hardware.rs`'s hotplug arm; four unit tests in `engine::snapshot`; the
`hardware.rs` module header, whose claim about motorized controls this change made untrue as
written.

### The load-bearing part is the ordering, and it is the part a diff cannot see

Snapshot, cycle, restore and cycle, snapshot, restore are the same three calls. The second is not a
weaker version of the first — on this device it is actively wrong, because the post-cycle reading
is a number nobody commanded \[PF:28\], so a snapshot taken then records the defect and restoring
from it writes the defect back. Worse, it does so *quietly*: the restore reports `AlreadyCorrect`,
which is what a camera that needed nothing looks like.

So the ordering is not asserted by reading the source and it is not asserted by a comment. Each of
the two acts produces its own stamp, and the comparison of the two stamps is the assertion:

- `snapshot_every_camera` reads the clock in the same expression as it reads the device.
  `snapshot::take_in_effect` takes `now` as an argument precisely because the engine reads no
  clock, so somebody must; doing it there makes the stamp a fact about when the camera was read.
- `spawn_the_cycle` reads the clock and spawns the helper, in that order, in one function. The
  stamp is taken *before* the spawn on purpose: an instant earlier than the true start can only
  refuse a snapshot the arm should not have trusted, while a later one would admit a snapshot taken
  *during* the cycle, which is the reading PF:28 says is a lie.
- `engine::snapshot::restore_across` refuses a snapshot whose `taken_at` is after the disturbance's
  stamp, before writing anything.

Move the snapshot below the cycle and its stamp moves with it, because the stamp comes from the
read and not from a bookkeeping line beside it. That is the difference between this and the two
alternatives considered. A recorded step log (`Snapshotted`, `Cycled`, `Restored`) is only as
truthful as where its `push` calls sit, and they travel with the code they record. A shell gate
over the source would be asserting that one call appears above another in a file — which is
`ignored-suites-have-recipes.sh`'s N72/N73 lesson from the wrong side: a gate reading prose as
code, one refactor away from being wrong in either direction.

**What can go red, and where.** `restore_across`'s inverse is a unit test in `engine::snapshot`
that runs at every `just ci` on a machine with no camera: a snapshot stamped after the disturbance
must be refused **and must write nothing**. Watched failing with the comparison neutered to
`if false`, where it reports `AlreadyCorrect` — the silent wrong answer this entry is about. The
boundary has its own test (a snapshot stamped in the disturbance's own millisecond restores, since
stamps are milliseconds and a `uvcvideo` cycle takes about a second, so a tie is a quick caller
rather than a swapped one), and so does the belt-and-braces case where the fingerprint also moved.

The hardware half cannot be a unit test — the cycle needs a driver — so what the arm carries is the
assertion, not a second copy of the law: swapping its two steps makes `restore_across` refuse, and
the arm's `unwrap_or_else` turns that into a red run with PF:28 named in the message. Watched on
this desk, 2026-08-13, by moving the snapshot call below the cycle and running the arm against five
real cameras:

```
FAIL hw_hotplug_a_uvcvideo_cycle_arrives_as_removals_then_arrivals_through_the_real_watch
  cam:integrated-camera-integrated-c: the restore across the cycle was refused: cannot restore
  3-4:1.0 from state snapshot_after_the_disturbance(taken_at=2026-08-13T11:51:31.585107871Z,
  disturbance_began=2026-08-13T11:51:30.331954912Z). A snapshot younger than the cycle records
  the cycle — see PF:28
```

1.25 seconds apart, which is the cycle. The camera named is the first one walked rather than the
one with the motor: the ordering is wrong for every camera at once, and the first refusal is the
one that stops the run.

### Three decisions inside it that are not obvious

**It restores every camera, not the one with the motor.** The cycle is a driver-wide event, the
control group in PF:25 is four cameras, and an arm that guarded only the device it expected to be
disturbed would have nothing to say the day a second one is. On a desk where nothing moved, the
cost is five reads and no writes — `restore_one` does not write a control that is already correct.

**It restores motorized controls, which every other arm in that file refuses to touch.** The module
header's claim — "every other arm excludes motorized controls by asking `is_motorized`" — was true
and this makes it false, so the header is amended rather than left to rot. The argument is that
`is_motorized` guards against a suite *pointing a camera somewhere new*, and this writes back the
position the operator had; PF:28 measured that the aim does not detectably move across a cycle at
all, so what the restore issues is a command to the aim the camera already has. It runs under
`WCH_NO_MOTION=1` with the rest of the arm, and that is the honest side of the trade rather than an
oversight: **the alternative is a run that leaves the camera reporting an aim it does not have**,
which is exactly the defect. It is flagged here because it is the owner's to overrule — the rung's
motor policy is an owner ruling (2026-08-08), and this is the first thing to write a motor outside
`hw_motion_*` since it.

**The restore happens as early as the driver allows** — immediately after the cycle's output is
collected, ahead of every assertion about the event stream. A desk should not keep a wrong aim
because a debounce disagreed with a kernel. The two skips above that point need no restore for the
same reason they are skips: the interlock refused the unload, or the unload did not take, so
nothing was re-initialised.

### What the arm asserts, and the one thing it only prints

Three assertions and a transcript. The restore is refused if the snapshot is younger than the
cycle. The report must be *complete*. Every control the report says it wrote or found correct must
read back at its recorded value — with the controls the report names `OwnedByAutomation` excluded
**by the report rather than by a list here**, because their value is the automation's \[PF:24\] and
demanding a number from an algorithm is asserting the device is not what it says.

What is printed and not asserted is *what the cycle moved*. A cycle that disturbs nothing is a
legitimate answer — it is what this desk produced on the day this landed — and asserting that the
device misbehaves would make the arm fail on hardware that behaves. That line is also the repair
for how PF:25 was found: from outside the suite, by hand, because every green run had printed
nothing about it.

### What the run said, 2026-08-13

`just smoke-hw`, five cameras, 18 of 18 arms run, 9 claims declined by name. The hotplug arm's new
lines, in full:

```
before the cycle: 15 control(s) recorded for cam:integrated-camera-integrated-c
before the cycle: 2 control(s) recorded for cam:integrated-camera-integrated-i
before the cycle: 17 control(s) recorded for cam:dell-u3224kb-a-4k-webcam
before the cycle: 17 control(s) recorded for cam:logitech-brio
before the cycle: 22 control(s) recorded for cam:obsbot-tiny-3-obsbot-tiny-3-st
…
cam:obsbot-tiny-3-obsbot-tiny-3-st: 22 control(s) recorded, none of them changed by the cycle
  3 control(s) left to their automation [PF:24]: blue_balance, red_balance, white_balance_temperature
```

**The cycle disturbed nothing that day, on any of the five cameras**, and the arm says so instead
of implying it — which is the transcript PF:25 had to be rebuilt by hand for want of. It is also
why the arm prints this rather than asserting it: a green run here is a device that behaved, and a
suite that failed when the device behaved would be unusable on anyone else's desk.

Independent of the suite's own claim, the OBSBOT's aim was checked photographically either side of
a cycle in the same session — 1.04 against a noise floor of 1.02, where a commanded 2° scores 7.93
\[PF:28\]. The suite says it put the camera back; the photographs say the camera never left.

**The suite has one red and this change did not add it.** It is
`hw_profile_capture_reproduces_the_committed_invariant_section` on the OBSBOT, which is
\[PF:23\]'s retirement condition arriving: the device advertises 3840×2160 and 120 fps again, whole,
after an overnight replug. PF:23 carries the evidence and what the next session owes. The two reds
this run was expected to have — E15's PF:24 pair — were both **green**, in a dark room; PF:24's
amendment of the same date says why that is the same finding from the other side and not a repair.

**No gate rows, and no change to any count.** `scripts/gates/phase-criteria.tsv` is the population
of what a *phase* proved, and this proves nothing about a phase boundary: it is a defect fix in a
closed phase's suite, and a row here would claim P4d had asked for it. Rule 1's "a fix lands with
its gate" is met by the four unit tests, which `just ci` already selects by package. `just smoke-hw`
still runs the same eighteen arms — no arm was added, renamed or split — so the suite's counted
selection and its census are untouched, and `ignored-suites-have-recipes.sh` sees the same
`hw_hotplug_*` name it saw before.

**Amend this note if** the arm learns to skip the motorized half of the restore under
`WCH_NO_MOTION=1`, or if a second suite needs `restore_across` — at which point the stamp-taking
helpers stop being two functions in a test file and become somebody's module.

**Retires when:** V4L2 can report a mechanism's actual position, which would let the arm assert the
restoration it currently issues \[PF:18, PF:28\]; or when a kernel makes a control's post-reload
reading equal the command it was holding, at which point the ordering stops carrying anything.

## N87 — The helper's own comment said it kept both halves of a duplex connection, and one of the two readers threw its half away

**Believed:** that `crates/daemon/tests/support/ws.rs` already held whatever a reader was not
asked for. The `queued` field says so in as many words, and has since P4e-i built it — "**Queued,
not discarded**, and that is the difference between a suite that tests the daemon and one that
tests the scheduler: on a duplex connection an answer and a notification are in flight together,
so a helper that dropped whichever lost the race would make a delivered event look like an
undelivered one — the test would hang, and it would hang for a reason on this side of the socket."
The paragraph is right about the stakes and it names the failure exactly. It was a description of
the intent.

**True:** only one of the two readers implemented it. `Ws::answer` searched `queued` for an answer,
read a frame if there was none, and pushed a notification back — correct, and the shape the
paragraph describes. `Ws::notification` read a frame, returned it if it carried `params`, and
otherwise **went round the loop again**, so every answer it stepped over was consumed and dropped.
Its own doc comment said "Answers to calls are skipped rather than treated as the end of anything
… which is the whole reason `Ws::queued` exists", and *skipped* is the defect written down as
though it were the design: the frame was skipped permanently rather than set aside.

**Repo:** `Ws::notification` in `crates/daemon/tests/support/ws.rs`, now written as `Ws::answer`'s
mirror; the `queued` field's doc, which had to be corrected rather than kept; and
`an_answer_queued_before_a_notification_survives_the_read_that_passes_over_it` in
`crates/daemon/tests/web_rpc.rs`.

### How it surfaced, and why nothing before this session had a chance to

`just ci` on the committed tree at `84ce8f3` — the first one this session ran, and the entry point
for everything below:

```
TIMEOUT [ 180.011s] (1123/1123) webcam-handler-daemon::web_rpc a_subscription_delivers_over_the_tcp_websocket
     Summary [ 196.529s] 1123 tests run: 1122 passed, 1 timed out, 26 skipped
```

Two numbers in that line are the whole diagnosis and both are easy to read past. **`(1123/1123)`
says it finished last**, so it was still waiting after everything else had stopped — a hang on a
machine that had gone idle, not a slow test on a busy one. And the same test alone takes
**3.061 s**, which is a factor of sixty and not a factor a loaded machine produces.

The process was still alive when it was found, and the three readings that mattered were taken from
`/proc` before it was reaped: every thread parked (`futex_do_wait`, one reactor in `ep_poll`), the
camera actor thread parked, and **both queues empty on the established TCP connection**. Nothing
was in flight, nothing was running, and the two ends were still connected. A daemon that had wedged
would not look like that; a client waiting for something already delivered would.

**Only one caller in the workspace could see it.** `Ws::notification` loses an answer only when a
call is outstanding *while* a notification is read, and
`a_subscription_delivers_over_the_tcp_websocket` is the one test that arranges exactly that — it
puts the sweep on the wire with `Ws::write` and collects the answer with `Ws::answer` after the
events, "which is the only shape in which the notifications and the answer are in flight
together". That sentence is the test's own, and it is why the suite that found this is the suite
that had to.

### The ordering is a race this repository has already measured, from the other side

**The race is N69's and it is not re-derived here.** That entry establishes it in as many words —
"`wch_calibrate_sweep`'s answer and its `SweepFinished` leave the daemon on two different tasks:
the method call, and the forward task `daemon::events` runs per subscription … They reach one
connection's writer in whichever order that daemon's runtime put them" — and measured the gap at
**+34 µs on the run that lost it**. `engine::calibrate::run` emitting `SweepFinished` before it
returns is a fact about the engine that does not survive the next hop, and N69 is where that is
written down.

**What is new is the direction.** N69's loser was `wchc`: the client broke its loop on the
*answer* and the terminal event landed microseconds later on a socket nobody was reading any more,
costing a progress bar its last line. Its repair is client-side — `sweep_and_watch`'s fourth step,
a bounded tail entered only when the terminal event is actually outstanding. This entry's loser is
the **test helper**, and it is the mirror image: the loop breaks on the *event* and the answer is
the frame nobody reads again. Nothing in N69's fix could have covered it, because N69 repaired a
client that reads events and this is a helper that reads both.

So the two entries are siblings over one race, and the pair is the lesson: a duplex connection has
two readers and either of them can be the one that discards. N69 asked what a client owes a
terminal event; this asks what a reader owes the frame it was not looking for.

**The load that shows it is not N69's load.** That entry records that spinners do not reproduce its
defect at eight or at sixty-four and that four concurrent suites do. This one reproduces under six
busy-loop hogs on eight cores — **three hangs in twenty-five runs of the suite, roughly one in
eight** — because the arrangement is not spinners alone: nextest is running the binary's eight
tests concurrently underneath them, which is scheduling pressure on the daemon's own runtime rather
than on the CPU. Stated rather than compared, because a rate measured under one load is not
evidence about another (rubric Part E).

The instrumented run is what closed it. With a heartbeat task and a line per frame, a hung run
printed the sweep's events **including `sweep_finished`**, and then forty-three heartbeats — 86
seconds — while the test sat in the `Ws::answer` that follows the loop. The events arrived, the
terminal arrived, the runtime was healthy, and the frame the test was waiting for had been read and
discarded by the loop that preceded it.

### The test does not reproduce the race, and that is the point

A test that needed the scheduler to lose would be a test that passes for the wrong reason. This one
puts an answer in `queued` **before any notification can exist**: `wch_list` is written raw with an
id of its own, a following `Ws::call` steps over that answer and queues it — which is `Ws::call`'s
documented behaviour and not a trick — and only then is the sweep requested. Against the repaired
helper it passes in 3.0 s; against the old one it is a `TIMEOUT` on an idle machine, which is where
it was watched failing before the repair was written (AGENTS "Writing tests", rule 2).

The rate was re-taken afterwards at the same load that produced it — **0 red in 40 runs of the
repaired binary under six busy-loop hogs on eight cores**, against three in twenty-five before. That
number is corroboration and not the proof: forty green runs of a one-in-eight race is a result the
old code would have reached with probability under one in two hundred thousand, but it is still a
statement about a distribution, and the deterministic test above is the thing that can go red on
the next person's change.

**No gate row was added and none is owed.** `binary(web_rpc)` is already a `g5` row and its
population is the whole target, so the new claim arrives counted; a row naming this test would be
transcribing a population the table already derives.

### What this cost, stated as the lesson rather than as an apology

**A comment asserting a property of the code beside it is not the property.** This one was better
than most — specific, motivated, and correct about the consequence — and it had been read by
whoever wrote each of the four suites that include this module. What none of them could see is that
the sentence was true of `Ws::answer`, which they were looking at, and false of `Ws::notification`,
which was thirty lines away and looked the same. The two readers agreed **by resemblance**, and
that is the failure mode a pair of mirror functions has: they are written together, they are read
together, and only one of them has to drift.

**And the deadline that named it could not name why.** `.config/nextest.toml`'s header argues that
"a deadline that turns a hang into a named failure is not synchronisation" and that it exists "so
that when a test stops finishing, the run says which one". It did exactly that and the claim
stands. What is worth recording beside it is the size of the gap between *which one* and *why*:
the name pointed at a subscription over a WebSocket, and the defect was in a queue discipline in a
test helper shared by four suites and reached over two transports. Three of the four instruments
that closed it — the `/proc` reading of a live hang, the heartbeat, the per-frame trace — were
built during the session and thrown away.

**Amend this note if** a third reader is added to `Ws` — the argument above says a pair drifts, and
a triple drifts faster; the answer then is one search parameterised by a predicate rather than
three loops that resemble each other.

**Retires when:** nothing retires it. The race it is about is a property of a duplex transport with
two writers, and the repair is the discipline rather than a bound on the race.

## N88 — A sweep that announced itself ends the stream it opened, and the guarantee is one `match` rather than four `return`s

**Doc:** **D8** (the sweep's execution order and its terminal vocabulary); **N18** (why the
durable `SweepInterrupted` append is best-effort rather than a swallowed error); **N24**
(`Sweeping { done: 0 }` is a state with no exit); **N69**/**N70** (the per-client stream, and
the bounded tail that ends on `is_terminal`); AGENTS rule 7 and "Who runs this, and why".

**Believed:** that the sweep's interruption path was the sample loop's error arm, which emits
`CalibrationProgress::SweepInterrupted`, writes the durable `SessionEvent::SweepInterrupted`
best-effort, and returns the refusal. That arm is correct and has been since P3c. What was
believed without being written down is that it was the *only* way out after the sweep had
announced itself.

**True:** it was one of four, and the other three were wrong. Between
`stream.emit(SweepStarted)` and the two terminal emits, `engine::calibrate::run` had three
fallible steps whose `?` returned with the stream still open — the photo directory's **name**
(`SessionStore::photo_dir_rel`), the directory's **creation** (`create_photo_dir`), and the
**closing commit** that writes `SessionEvent::SweepFinished`, which ran *before* the live
`SweepFinished` was emitted. Nothing closed them one layer up either:
`crates/daemon/src/server.rs` calls `engine::calibrate::run(…)?` and the refusal goes to the
RPC answer.

**Repo:** `engine::calibrate::execute` and `engine::calibrate::interrupted`, both private; the
law is stated in `calibrate.rs`'s module header as "One start, one end"; one `g3` row over
three tests.

### Who is actually hurt, which is not the caller

Whoever made the call learns of a refusal from its own answer, so for them a missing event is
untidy rather than harmful. The stream's other readers have nothing else to read. P4e's
subscription is per *client*; the web client's calibration view exists to track a sweep it did
not start; an agent watching a session another process drives is in the same position. To all
of them **a start with no end is indistinguishable from a sweep still running**, and stays
that way for the life of the process. `wchc` pays from the other side: N69 and N70's bounded
tail ends on `is_terminal`, so an ending that is never coming costs the whole budget and then
renders a sweep that stops mid-sentence.

AGENTS' "who runs this" is what makes that decisive rather than a matter of taste. The primary
consumer is an unattended agent harness with no hands, and a progress stream is the only thing
it can watch.

### The N24 half, which is the part that was worse than a missing event

`abandon_sweep` lived **inside** the loop's error arm. So a sweep stopped by either photo
directory failure left the control at `Sweeping { done: 0 }` on disk — precisely the state
note N24 walks exit by exit to forbid — with no `SessionEvent` line saying why. That control
could never be swept again and the (camera, task) slot never settled.

N24's *reasoning* was right and its *placement* is what leaked: it reasoned about the exits
the sample loop takes, and the sweep had exits the sample loop does not. Folding the arm into
one place repairs that half by construction rather than by a second call.

### Why the shape, and not three fixes

Three of four exits were wrong, which says the review that would have caught the fourth is the
review that missed these three. So the obligation is discharged where it can be discharged
once: everything between the two events is a private `execute` returning `Result`, every
refusal arrives at one `match` in `run`, and one private `interrupted` turns an `Err` into the
terminal event, the `abandon_sweep` and the durable note. A `?` added inside that body is
covered the day it is written rather than the day somebody notices it.

**The closing commit moved *inside*, and that is a semantic change rather than a mechanical
one.** `SweepFinished` now means "the document says this sweep finished" and not "the executor
believes it did". A refused close is reported honestly as `SweepInterrupted { taken: total,
total }` carrying the store's `errno` — because the alternative announces a state that never
landed, and leaves the watching client disagreeing with the next process to read the session.

### The fixtures, and why the store's own fault menu is not one of them

`StoreFault::DiskFull` cannot reach any of these holes: a blanket refusal fails the
`begin_sweep` commit, which is deliberately on the *near* side of the announcement, so the
sweep never starts. Nor can the menu be re-armed mid-sweep — `TempStore::arrange` takes
`&mut` and the sweep holds the store shared. So each hole is driven by a real arrangement:

- **the name** — the scripted double with a control whose slug is `---`. It has to be the
  double rather than the fake: no device enumerates such a name and `ControlSlug::from_name`
  refuses it, so a fake that exhibited it would be a fake claiming something no profile
  replays (E5, AGENTS "The fake resembles"). `parse` is how one reaches the engine — a
  hand-edited session file, or an RPC request. Its twin sweeps a real control on the same
  double and gets one step further, which is what proves the refusal came from the naming.
- **the directory** — a **dangling symlink** where the pass's photo directory goes, so
  `create_dir_all` meets `EEXIST` and the `is_dir` retry follows the link to nothing. A plain
  file was tried first and `atomic-write-home.sh` correctly called it a bypass; the symlink
  needs no write primitive and tells a truer story anyway (photos aimed at a disk that is not
  mounted).
- **the closing write** — a progress sink that, on the last `SampleTaken`, replaces
  `session.json` with a **directory**, so the only remaining store write fails at `rename`
  with `EISDIR`. Chosen over a chmod because it is deterministic for every user including
  root. The log is a separate file, so the durable `SweepInterrupted` still lands, and the
  test asserts both that and the absence of a `SweepFinished` line.

### The honest limit

This is a guarantee about `Result` exits and nothing more. A panic inside the sweep, or a
process that dies mid-sweep, still emits nothing — that is design §6's crash story and the
actor's liveness handling, and it is named here so the law is not read as wider than it is.
The durable record is what covers that case, which is why `interrupted` writes one.

**Amend this note if** a fallible step is ever added *above* the `SweepStarted` emit that
ought to announce itself — the line between "never started" and "started and stopped" is
drawn at that emit, and moving anything across it moves the law with it.

**Retires when:** nothing retires it. It is a property of a stream with readers who did not
make the call.

## N89 — A camera's advertised support may change at each plug event, so the format tree is invariant *within a connection* and nowhere else

**Doc:** AGENTS rule 4 ("the device is the only authority on itself"; measured wins; new
hardware behaviour lands as corpus + a note the day it is seen); design **T3** (the profile's
two sections and their differing comparison semantics); \[PF:23\], whose second amendment
predicted this exact question; \[PF:9\], whose retired example is involved.

**The ruling (owner, 2026-08-13):** *"we need to be prepared for the fact that a camera's
advertised support may change each time the camera is plugged in. I don't think we need to
worry about support changing while the camera is connected."* Asked to choose between
re-capturing the stale profile and changing what the arm treats as invariant, the owner
answered **both**.

**Repo:** `DeviceProfile::invariant_difference` and `InvariantDifference` in
`webcam-handler-schema`; the three-way match in
`hw_profile_capture_reproduces_the_committed_invariant_section`; a re-captured
`corpus/profiles/obsbot-tiny3.json`; one row of `RANKED_DEFAULT` in
`crates/backends/fake/tests/corpus_replay.rs`.

### Why `formats` and only `formats`, which is the load-bearing part

The narrowness is **measured twice, in opposite directions**. When the OBSBOT stopped
advertising 3840×2160 and 120 fps, PF:23 recorded that its `CameraInfo` half was identical
(`differing_fields` answered `[]`) and its control set was identical, "all 24 controls, byte
for byte" — only the format tree moved. When the whole tree came back two days later it came
back the same way, and the re-capture confirmed the control set byte-identical again, with the
only `info` difference the `/dev/videoN` paths note **N63** already made inert. Two
observations, opposite directions, exactly one of the four sections moving in each.

So `formats` gets a predicate and the other three do not. **The day a control set is measured
moving across a plug event is the day this shape is wrong**, and that is this decision's
retirement condition rather than a hypothetical.

### The predicate fails closed, and its obvious spelling fails open

`is_only_the_format_tree` is `self.sections() == [FORMATS]`. The spelling that reads
identically in review — `formats && info.is_empty() && !controls && !measured_pairs` — keeps
answering `true` when a *fifth* section is added later, which would silently extend the
owner's ruling to something nobody was asked about. The equality answers `false` for anything
it does not recognise. This was written the wrong way first and caught by re-reading a comment
against the code it described; it is recorded because the two spellings are indistinguishable
at a glance and only one of them is safe to add a field beside.

### What the decline costs, and it is not free

This is **the first `SKIP (partial)` in the hardware suite that hides the corpus being wrong
rather than the hardware lacking something.** Every other decline in that suite is "no
compound control", "no motor", "no perturbable control" — facts about a device's shape.
This one says a committed document does not describe the device in front of it, and passes.

Nothing bounds how long a stale corpus may sit behind that line. The only defence is that the
line is loud: it names the extent (`7 size(s)/48 rate(s) fresh against 6/32 committed`), it
uses the word *stale* about our own corpus, and it prints both trees underneath. **A session
that meets this decline owes a re-capture with a reason; leaving it is a decision and not a
default.** The abrasive wording is deliberate — the softer phrasing ("the device changed")
would let a corpus nobody re-captured read as healthy indefinitely, which is the failure this
whole design exists to prevent.

### The plug event reached the product, once, visibly

`RANKED_DEFAULT`'s OBSBOT row moved `MJPG 1920×1440` → `MJPG 3840×2160`, so **an unflagged
`wch photo` on this camera returns a 4K frame today and returned a 1920×1440 one last week,
because the camera was unplugged and plugged back in.** D5 and note N85 were not touched: the
ranking rule asked the same question and the device gave a different answer. This is the first
measured instance of a plug-event capability change propagating through the format ranking into
what an agent gets back, and it is the concrete form of the thing \[PF:26\] is about.

### What is now owed, and it is a schema decision rather than a note

PF:23's "what would make the next one provable" gap asks for a `ProfileProvenance` carrying
`bcdDevice`, the negotiated link speed and the boot, and defers it as a wire change needing an
owner's decision. **Under this ruling that stops being a nice-to-have.** If advertised support
changes at every plug event, a profile that cannot say *which enumeration it was taken under*
is structurally unable to answer "is this document stale, and since when" — which is precisely
the question the decline above leaves open. Recorded here as the successor question, not
answered.

Measured this session and worth having in one place when it is: ten enumerations of this
device since 2026-08-08, every one `high-speed` (480 Mbps, USB 2.00) and every one
`bcdDevice 5.10`. **Neither link speed nor the USB-reported device version distinguishes the
two capability sets**, so a provenance field carrying those two alone would not have answered
it; what separates the observations is the enumeration itself. A future session must not treat
`bcdDevice` as a firmware witness on this device — the owner updated its firmware in the
02:47→03:01 window on 2026-08-13 and the field did not move.

**Amend this note if** a device is measured changing its advertised support *without* a plug
event, which is the half the owner's ruling explicitly sets aside — that would make the
decline above unsound rather than merely loud.

**Retires when:** a control set, an identity or a measured pair set is seen moving across a
plug event, at which point the one-section predicate stops describing the hardware.

## N90 — Two requests from the owner, 2026-08-13: every binary carries the full name, and the README says how to install and where to start

**These are the owner's, stated in his own words on 2026-08-13, and recorded here before either
was implemented** so that the request and its execution are separately auditable. Both are
changes to things this repository had already decided; neither is a defect report.

> 1. All binaries and crates must have names that start with `webcam-handler`. Example:
>    `webcam-handler-daemon`.
> 2. README sections: installing all the binaries via `cargo install --path` commands; usage
>    overview recommending the daemon + CLI client with a brief tutorial on the calibration
>    process.

He added: implement both "as soon as it's convenient (no negative interactions with other
ongoing efforts)" — which is a scheduling instruction and is why this entry exists rather than
a commit alone.

### Request 1, and what it actually changes

**The crates already comply.** All fourteen workspace packages carry the full prefix —
`webcam-handler-api`, `-cli`, `-cli-core`, `-client`, `-daemon`, `-engine`, `-fake`,
`-imaging`, `-priv`, `-schema`, `-testkit`, `-v4l2`, `-web`, `-xtask`. What does not comply is
the **four binaries**, and AGENTS.md states the old rule in as many words: "Packages carry the
full `webcam-handler-` prefix; directories are short (`crates/engine/`); lib names are bare;
binaries are `wch`/`wchd`/`wchc`." That last clause is what this request overturns.

The mapping is not a choice once the request is read against the example given — the owner's
`webcam-handler-daemon` **is** the daemon's package name, so binary name becomes package name
throughout:

| was | becomes | package |
|---|---|---|
| `wch` | `webcam-handler-cli` | `webcam-handler-cli` |
| `wchd` | `webcam-handler-daemon` | `webcam-handler-daemon` |
| `wchc` | `webcam-handler-client` | `webcam-handler-client` |
| `wch-priv` | `webcam-handler-priv` | `webcam-handler-priv` |

It also makes request 2 answerable in one line each: `cargo install --path crates/daemon`
installs a binary named after the crate it came from, which is the property a reader of an
install command expects and did not have.

**Directories are not in scope, clarified by the owner the same day:** *"the names of the
`crates/` directories can remain short."* So `crates/cli/`, `crates/daemon/` and the rest keep
their names, and AGENTS.md's "directories are short (`crates/engine/`)" stays true — it is
only that sentence's final clause, "binaries are `wch`/`wchd`/`wchc`", that this request
overturns. Worth stating because the install command above reads `crates/daemon` and produces
`webcam-handler-daemon`, so the short directory and the long binary are visibly different
strings on the same line and a later reader could otherwise take one of them for a mistake.

**What it costs, measured rather than guessed:** 112 files mention one of the four names.
Four `[[bin]]` targets carry them; the rest are call sites, gate predicates, nextest
selections (`binary(wchd)` in `scripts/gates/phase-criteria.tsv` is a *criterion* keyed on a
binary name), systemd units, packaging, the browser rung's harness, and prose.

**The one judgement this entry fixes before the work starts: history is not rewritten.**
`docs/implementation-notes.md`'s existing entries and `docs/historical/` say `wchd` because
that is what the binary was called on the day each was written. They are append-only case law
and a dated record that silently acquires a name nobody used at the time is a worse document,
not a tidier one. So the rename touches live code, live configuration, live scripts and the
*current* documents; earlier entries keep their vocabulary, and this entry is the pointer that
explains the discontinuity to whoever meets it.

**And the ergonomic cost is real and is the owner's to accept, which he has:** `wchc` is four
keystrokes and `webcam-handler-client` is twenty-one. Nothing in this repository shortens it
back — no alias, no symlink, no second `[[bin]]` — because a second name for one program is
the thing the request exists to remove.

### The cost nobody predicted: `comm` is fifteen bytes, and all four names now collide in it

`TASK_COMM_LEN` is 16 including the NUL, so the kernel hands out **`webcam-handler-`** for
every one of the four programs. Measured against a live process rather than reasoned from the
header. D9's lock record carries `/proc/self/comm` verbatim, so three things moved:

- `crates/daemon/tests/lock.rs` asserted `Some("wchd")` and now asserts `Some("webcam-handler-")`;
- `Error::sample(StoreLocked)` carries the same string, and that sample **ships** — it is the
  example a client author reads in `schemas/webcam-handler-openrpc.json`;
- and the field **can no longer tell a CLI holder from a daemon holder**, because all four
  truncate to the same fifteen bytes.

That last one is a real loss of diagnostic value and it is worth being plain about: before the
rename, "who holds the lock" could be answered by `comm` alone. It cannot now. What carries
the answer instead is the `pid` beside it, and — for the *advice* rather than the identity —
`LockProtocol`, which is what the refusal always turned on. Nothing was weakened that a
program branches on; what was lost is what a human reads first.

It is not a reason to reverse the request, and no workaround was invented: a program that set
its own `comm` to something other than its name would be a second name for one program, which
is exactly what request 1 removes. Recorded so the next person to read
`"comm": "webcam-handler-"` in the shipped schema knows it is the kernel's answer and not a
truncation bug in this project.

### Two things left undone, both the owner's

- **`xtask` is a fifth binary and does not comply.** `webcam-handler-xtask` declares
  `[[bin]] name = "xtask"`. The ruling's own mapping names four, so inventing a fifth row was
  declined rather than assumed. It is one line if wanted: no `.cargo/config.toml` alias exists
  (`just generate` runs `cargo run -p webcam-handler-xtask --`) and the gate rows select it by
  `package(…)` rather than by `binary(xtask)`.
- **`wch-priv`'s blessing is stale and needs a `sudo`.** `.wch-bin/wch-priv` (mode 0700,
  capped, blessed 2026-08-08) is still on disk and nothing reads it: `just bless`,
  `just priv-doctor`, `just rung-vivid-managed` and `privileged-helper.sh` all name
  `.wch-bin/webcam-handler-priv`, which does not exist. So `privileged-helper.sh` takes its
  documented named, counted skip, and **the R2 vivid rung is unavailable until `just bless` is
  re-run**. `just bless` will do the right thing (its stamp check requires the stable path to
  exist, which it does not), but it will not remove the old root-capable file — that deletion
  is the owner's.

### Request 2, and what it is not

An installation section and a usage overview, with the recommendation being **the daemon plus
the CLI client** rather than the direct CLI, and a short calibration tutorial. Worth recording
that this is a documentation request with a *product* opinion inside it: `wch` (now
`webcam-handler-cli`) links a backend and drives the device in-process, and the recommended
path is instead the daemon with a thin client over a socket. That matches AGENTS' "Who runs
this" — an unattended agent harness and an occasional human at the same cameras — and it is
the first time the README has been asked to say which of the two a new reader should reach
for.

It is **not** the agent usage guide. That is docs/7's **P6e**, generated by xtask from the T4
command core so it cannot drift, and it remains P6e's. This is the human-facing front page,
and the two are allowed to overlap on the calibration walkthrough.

**Amend this note if** the binaries acquire short aliases after all, which would mean the
ergonomic cost was underestimated here.
