# Design Document: webcam-handler — Architecture (v3)

Doc 12 in the webcam-handler series, **v3 — second revision**. Status: **adopted** at docs/13
P7a (commit `796babb`, 2026-08-18); supersedes docs/6 (v2) upon adoption, whose section and registry numbering this
revision preserves — v2 citations in the implementation notes, the commit history and the
gate scripts stay resolvable. Companion documents: the phased implementation plan
(docs/13, v3 — docs/7's P0–P6 ledger is closed and carries forward by reference), the code
review rubric (docs/14, v3), the automated quality gates (docs/15, v3), and AGENTS.md
(docs/16, v3 — deployed at the repository root at adoption).

**Adoption was one sub-milestone and it was the first one** (docs/13 P7a, commit `796babb`).
Because two mechanisms read the v2 documents by name, issuing this revision superseded nothing
until that session executed the swap: move docs/6, 7, 8, 9 and 10 under `docs/historical/`;
repoint the one predicate that reads the design by literal path (`wire-surface-sync.sh`,
`design_path=`) **and the two selftest case files that seed the old paths**
(`cases/wire-surface-sync.cases.sh`, `cases/agents-md-current.cases.sh`) — verified the
complete set; `agents-md-current.sh` and `agent-guide-current.sh` derive their subjects
and follow on their own; move docs/16's deploy and redirect sentences into its preamble
and copy it to the root, byte-identical, in the same commit docs/10 leaves (the gate's
second-claimant arms forbid any ordering with two claimants of either declaration); and
run `just ci`. Until that lands, docs/6 v2 remains the document of record
and this one is the successor it names.

**What this document was built from.** The v2 inputs stand (the 2026-08-07 probes, the case
law, the vendored skill, the license audits); three inputs are new, in decreasing order of
authority:

1. **The G6 whole-tree review and its reconciliation** — `docs/11` (2026-08-15/17, against
   `3a7b9fa`): 290 candidates, 79 findings, 78 closed by eleven repair commits, plus the
   post-reconciliation case law through **N255** (2026-08-18). Where a finding moved a rule,
   this revision absorbs the rule as repaired, not as reviewed.
2. **The sibling-consumer ledger** —
   `../usb-teleporter/docs/feature-requests-webcam-handler.md` (verified against
   webcam-handler HEAD 2026-08-11; every "Today" claim re-verified against `799ee73` for
   this revision): six requests, FR-W1–FR-W6, from the first consumer of this tool that is
   neither the owner nor the owner's agent harness. §1.3 carries the ledger; D14–D19 are
   the decisions it produced.
3. **Standing owner inputs addressed to "the next design revision"** — N91 (the short
   name), N103 (WebM), N79 (D11's dangling token clause), N158 (§2.7's self-count) — each
   discharged or explicitly carried in §7 and §8 — plus the owner's instruction of **2026-08-18**
   commissioning this revision: implement the sibling ledger, revise the plan alongside the
   design, and give the web client a design pass for the operator's real use — preview
   beside the controls with no scrolling between them, and calibration drivable from the
   page (**D20**).

## Changes from v2

Every change absorbs measured evidence or an owner instruction; none re-litigates a settled
decision. Each row names its source:

| Change | Where | Source |
|---|---|---|
| A third consumer joins the expected-usage statement: a sibling project's HIL harness (usb-teleporter) consuming this tool as a pinned library and as a `--json` subprocess, to prove camera forwarding | §1, §1.3 | the FR ledger |
| Camera selection grows from one spelling to five — id prefix, `/dev` node path, `bus:` interface path, `usb:` VID:PID, `serial:` — through one parser and the one existing resolver; selection never filters enumeration; zero new error kinds | **D14** (new) | FR-W1 |
| The device profile gains a stated identity/device partition and a masked compare with a named-diff answer, promoting the corpus-replay test's private mask into the product | **D15** (new), T3 | FR-W2; corpus_replay precedent |
| `Frame.sequence` and `Frame.timestamp_us` become contract; a pure bounded aggregator computes dropped frames, interval percentiles and jitter; the recording answer carries the stats | **D16** (new), D7 | FR-W4 |
| An A/B comparison primitive: pure metric deltas over two photographs plus an SSIM corroborator that represents its own unavailability instead of refusing | **D17** (new) | FR-W3 |
| The blessed in-process composition becomes a public facade the direct CLI is rebuilt on, plus a stability table naming which seams embedders may hold | **D18** (new), §2.7 | FR-W5 |
| Mid-stream device loss gets its expected-behavior contract stated in advance, and a contributed-evidence protocol for the partner rig that can uniquely produce the event | **D19** (new), §3.3 | FR-W6 |
| D5 as repaired: `choose` refuses in one shared home (`Result`, both backends inherit); a named size narrows the ranking device-wide and refuses with `SizeRefusal`, never a substitution | D5 | docs/11 H1/H1b; N134, N138; owner 2026-08-16 |
| D9 as repaired: the session tree is private by mode (0700/0600, refused-not-repaired, owner-checked); a torn log tail heals at append time; `write_json_atomic`'s contract is the `Reach` classification | D9 | docs/11 M9–M11; N140–N142, N150 |
| D11 absorbs the provenance layer (browser-foreign requests refused before credentials), the journald token redaction, and strikes the "unless configured" clause N79 called a dangling promise | D11 | N93, N95, N180, N182; N79 executed |
| D12 absorbs the recording interlocks as rules: one take per camera; a photo during a take is `Busy` with an `Occupation`; every claim on a camera is a value whose release is its own; the stop's worst case is a four-wait table | D12, §2.10 | N114, N118, N169–N177 |
| D13 absorbs the `Busy.this_process` occupation payload, the three exclusive `FormatUnsupported` doors, the client's three-way `Received`, and the instruction-last `IllegalTransition` template | D13 | N217/N221, N138/N211, N215, N212 |
| T2 declares nine methods — `streaming` joined with D12's suspend ruling and no revision had absorbed it | §2.3 | N132 |
| §2.8's registry becomes a parseable table (one row per direct third-party dependency) so a reconciler gate can hold it; the four recorded drifts are corrected; `image-compare` is adopted conditionally for D17 | §2.8 | N133, N164; docs/15 commissions the reconciler |
| §1.2's probe registry extends to PF:28, one absorbed line each; transcripts stay in the notes | §1.2 | notes, PF:17–PF:28 |
| §2.10 gains two laws: backend-contract refusals live in shared resolvers, and a claim on a camera comes back with its value | §2.10 | docs/11 §9.1; N169 |
| The structural-gap register is regenerated; item 9's ending changes (the partner rig is the named path out); a cross-machine-comparison item joins it | §3.3 | rubric rule 4; FR-W6 |
| WebM looked at, as the owner asked, and declined as a measurement path; the ingestion-need shape is recorded | §7 | N103 |
| The short-name question is presented for ruling with a recommendation; nothing renames until the owner rules | §8 | N91 |
| §2.7's RPC-helper self-count is corrected to the measured shape and stops being a number prose asserts | §2.7 | N158 |
| The web client becomes the operator's workbench: preview and controls side by side with no scrolling between them, live tuning through the existing guarded writes, and **human-driven calibration** — the flow §1 promised the owner and P5 delivered only a viewer for. One new token-gated route (`/session-photo`) joins `CAMERA_BEARING_PATHS` | **D20** (new), §2.7, D11 | owner, 2026-08-18; §1's expected-usage statement; E16's recorded collision |

## TL;DR

- **Architecture is unchanged**: one schema crate serving four masters; a pure engine over
  T1/T2 backends; the V4L2 vertical and the profile-replaying fake; one wire surface; one
  command surface; the daemon's per-camera actors. v3 adds consumers and contracts, not
  layers.
- **The v3 surface, in one sentence**: two document verbs (`profile compare`, `photo
  diff`), one widened positional (camera selectors), one widened answer (`record` carries
  stream stats), one public composition (the embedding facade), one new token-gated HTTP
  route (`/session-photo`, D20), **zero new wire methods, zero new error kinds, zero new
  crates**, and one conditionally adopted dependency (`image-compare`, D17).
- **The device is the only authority on itself (E2)** — unchanged, and D14 keeps it:
  selection resolves against the live enumeration, never against a filter that would hide
  part of the machine or move D1's collision ordinals.
- **Comparability across time is the product** — and v3 makes it a verb: D15 answers "is
  the forwarded camera the same device?" and D17 answers "are these two photographs
  peers?", both as documents an unattended agent can branch on.
- **A contract is stated once, where both sides inherit it.** The G6 review's costliest
  finding was a rule enforced by the fake and violated by the real backend, green on both
  (docs/11 H1). The repair pattern — the refusal moved into the shared resolver — is now a
  §2.10 law, and every v3 contract names the population that walks it on both backends.
- **Security posture unchanged** (D11): UDS always; TCP opt-in, loopback + token;
  provenance before credentials; the token gates the camera-bearing routes — the list, which
  D20 gives a third entry, rather than a count of it — and not the client's own open-source
  assets.
- **Licenses**: permissive only, enforced by cargo-deny; the ban list is unchanged and
  `dssim` stays on it — D17's SSIM is MIT `image-compare` or owned code, never AGPL.

## 1. Scope

**Expected usage (owner, 2026-08-12; extended at this revision).** `webcam-handler-daemon`
runs on a computer whose cameras are pointed at a **device under test**. Three consumers,
shaped three ways:

- **An AI agent harness** (Claude Code or similar) drives the client to photograph the
  device under test to check its own work, and wants video to validate animations.
  Primary, continuous, unattended, and **without hands**: a verb needing a call sequence
  or a failure that reads as prose is a defect for the consumer that matters most.
- **The owner at the web client**, occasionally: check on the cameras, calibrate at the
  start of a run. Interactive, supervisory — and at this revision the second half of that
  sentence stops being aspirational: v2 said "calibrate them at the start of a development
  run" while the shipped page could only *watch* a session the CLI drove. D20 is the debt,
  named and paid: side-by-side preview and controls, and a calibration flow a human can
  drive end to end from the page.
- **A sibling project's HIL harness** (usb-teleporter, §1.3 — new at v3): consumes this
  tool as a **library** (git dependency pinned by rev, in-process calls for its tight
  loops) and as a **subprocess** (`--json` against the committed schemas), to prove that a
  camera forwarded over USB/IP still honors the whole T1/T2 contract. It is the first
  consumer that compares this tool's answers **across machines and kernels**, which is why
  identity, profile comparison and frame timing each grew a decision at this revision.

The consequences the first two consumers settled stand unchanged — the error vocabulary is
read unsupervised; repeatability beats image quality; the two consumers meet on one camera
as the ordinary case (N83). The third consumer adds one: **an answer about a device must
survive the device moving to another bus, another kernel, another machine** — which is a
claim about which fields are identity and which are description, and D15 is where it is
settled rather than implied. The full statement lives at the top of
`docs/implementation-notes.md`.

**Goals (v3).** Everything the v1 goals list (enumerate, describe, drive controls, photos,
video, calibration, four consumers of one library), all shipped and closed by docs/7's
ledger, **plus**: select a camera by any stable spelling a caller already holds (D14);
compare two profiles at the device level with a named diff (D15); account for a stream's
delivery health from its own frames (D16); compare two photographs (D17); embed the
blessed composition without reverse-engineering a CLI (D18); state, in advance, what a
mid-stream device loss must look like so a partner rig can measure it (D19); and give the
owner a workbench — tune a camera while watching it, and calibrate it by eye, from the
page (D20).

**The in-process constraint (owner's requirement)** — unchanged. No external binaries at
runtime; build-time bindgen and test-time oracles are fine.

**Non-goals (v3).** The v1 list stands: no audio (§8 item 2), no transcoding on the
measurement path (§8 item 1, §7 — and N103's WebM look sharpened it rather than reversing
it), no non-V4L2 product backend, no remote/multi-host daemon operation, no TLS, no
tracking loops, no cross-browser constraint. Two additions, both declined *verbs*:

- **No tool-side two-camera capture orchestration.** D17 compares photographs; it does not
  add a verb that captures from two cameras "simultaneously", because the tool cannot
  deliver simultaneity (one streamer per node; USB contention unmodeled, §3.3 item 3) and
  a verb that pretends to is a measurement lying about its own arrangement. The harness
  owns scene timing; two `photo` calls are its instrument.
- **No session/aggregate store for stream stats.** D16 computes and reports; trending over
  time is the consumer's, exactly as metric history already is.

### 1.1 The skill-to-operation map

Unchanged from v2 — the vendored skill's operations all have first-class homes, and
`docs/agent-guide.md` (generated from the T4 tree) is the skill's successor. The v3 verbs
extend past the skill's scope: the skill had no profile-comparison or photo-diff story to
replace, so D15 and D17 map to nothing in it and are recorded here rather than in the map.

### 1.2 The probe record (PF registry)

PF:1–12 were measured 2026-08-07 during design; PF:13–16 during P0–P2; **PF:17–28 during
P3–P5 on the same host** (the OBSBOT, the Dell U3224KB and the Logitech BRIO joining the
two Chicony logical cameras). Full transcripts live in the implementation notes, which is
where a new finding always lands first; this registry absorbs each finding's one-line law
at each revision. PF:1–16 are restated in docs/6 v2 §1.2 verbatim and are not repeated
here beyond their titles; PF:17–28 are absorbed at this revision.

- **PF:1** — the `v4l` crate panics on modern control types (the reason the control loop
  is ours). **PF:2** — menu indices are sparse. **PF:3** — INACTIVE tracks pairing live,
  both directions. **PF:4** — current values can sit outside the declared range. **PF:5**
  — defaults can too. **PF:6** — out-of-range writes are silently clamped. **PF:7** — one
  USB device can host multiple logical cameras; nodes group by interface. **PF:8** —
  serial numbers are unreliable identity. **PF:9** — in-process capture works; format
  lists are per-pixel-format. **PF:10** — bindgen's build deps. **PF:11** — early frames
  are unsettled. **PF:12** — read-only controls exist; the flag set grows. **PF:13** —
  `bus_info` is per-USB-device; only the interface path tells logical cameras apart.
  **PF:14** — a UVC VideoStreaming interface never has a V4L2 binding. **PF:15** —
  `ENOTTY` joins `EINVAL` as an enumeration terminator. **PF:16** — `little_exif` cannot
  write into a restart-interval JPEG (the header-only splice).
- **PF:17 (vivid)** — **a compound control's element count is not invariant**: vivid's
  `u8_pixel_array` reshapes with the negotiated format, so payload shape is device
  *state*, not identity, and a mis-sized payload is *applied truncated*, not refused —
  the fake resembles that now [N136].
- **PF:18 (OBSBOT)** — **a PTZ move is acknowledged before it happens**: `pan_absolute`
  reads back the commanded position in ~21 ms while the head is still traveling.
  `{requested, applied}` means *accepted*, not *achieved*, for motor controls; settle
  counts frames, never motion.
- **PF:19 (Dell)** — **one camera can own two capture nodes** (one sensor, two USB
  streaming terminals). The capture node is the *first* capture-capable member in node
  order — a uvcvideo convention, not a guarantee — and T1 deliberately has no per-node
  capture vocabulary, so the second stream is listed and unreachable.
- **PF:20 (Dell)** — **`pan_absolute` is not evidence of a motor**: a bezel camera
  enumerates the same PTZ controls and flags as a gimbal. Motor rules key on slug and
  treat digital PTZ as mechanical — the right direction to be wrong.
- **PF:21** — **the uevent socket needs no capability**: `NETLINK_KOBJECT_UEVENT` binds
  and delivers to a process with an empty capability set. Disproved N8's prediction and
  narrowed the privileged helper (§2.13).
- **PF:22** — **`/dev/videoN` is probe-order bookkeeping**: one `uvcvideo` reload rotated
  three cameras through each other's minors. Node numbering may be displayed, opened and
  recorded, never asserted as identity — and D14's `NodePath` selector is therefore an
  *address*, resolved per call, not an identity (its own paragraph says so).
- **PF:23 (OBSBOT; retired 2026-08-13)** — a camera's advertised capability set moved
  across power events and moved back. Retired as an open question when the modes
  returned; what it left behind is the sharper rule N89 absorbed into T3: **the format
  tree is invariant within a connection and nowhere else**.
- **PF:24 (BRIO)** — **an INACTIVE control's current value is the automation's and
  `VOLATILE` is not set**: `white_balance_temperature` drifts under AWB with no volatile
  flag. Restoration claims key on `OwnedByAutomation`, never on the volatile bit.
- **PF:25 (OBSBOT)** — **a `uvcvideo` cycle disturbs control read-back**: post-cycle
  reads land in a regime nobody commanded while the aim is photographically unmoved
  (PF:28 carries the inverted hazard). The −298800 regime is uncharacterised and recorded
  as such.
- **PF:26 (BRIO, Dell)** — **the driver's first-enumerated format is not the best
  photograph**: YUYV 640×480 enumerated first on a camera offering verbatim 4096×2160
  MJPG. The measurement behind D5's re-ranking ruling (owner, 2026-08-13; N85).
- **PF:27 (BRIO)** — **a second capture node can be a different sensor** (GREY 340×340
  beside the RGB node): "two capture nodes in one group" has at least two mechanisms, and
  format lists — not QUERYCAP — separate them.
- **PF:28** — **a snapshot taken after a `uvcvideo` cycle records a position nobody
  commanded**: restoring from it *introduces* an error and reports `AlreadyCorrect`.
  `restore_across` refuses a snapshot stamped after the disturbance (N86).

### 1.3 The sibling-consumer ledger (usb-teleporter)

usb-teleporter forwards USB devices over a mutual-TLS tunnel; a consumer machine
materializes a forwarded camera as `/dev/videoN` through mainline `vhci_hcd`. Its Tier 4
proof is this tool's own conformance battery and photo pipeline run against the *forwarded*
camera and compared with the direct-attached original — passing proves the whole T1/T2
contract survives forwarding. Its request ledger
(`../usb-teleporter/docs/feature-requests-webcam-handler.md`) is phrased as standalone
capabilities with the use case as rationale, and every request names a fallback in force —
nothing here was extracted under duress. The map:

| Request | What it asked | Decision here |
|---|---|---|
| FR-W1 | select one camera by node path, bus path, usb id or serial | **D14** |
| FR-W2 | identity-masked profile compare with a named diff — "the single highest-value request in this file" | **D15** |
| FR-W3 | A/B photo comparison, permissive SSIM | **D17** |
| FR-W4 | frame-rate/latency aggregation, or the frame fields blessed as contract | **D16** (both) |
| FR-W5 | an embedding facade or a supported-composition contract | **D18** (both) |
| FR-W6 | (offer) real mid-stream device-loss evidence from the rig that can produce it | **D19** |

Two properties of the ledger shaped the decisions. Its requests are *capabilities that
stand alone* — so every D-item below is designed for any caller on a multi-camera machine,
with the forwarding rationale cited as the motivating instance, never the specification.
And its fallbacks all compose public pieces — which is why several decisions are
promotions of a composition into a home (D15 promotes the corpus-replay mask, D18 promotes
the `InProcess` assembly) rather than new machinery: **the request is evidence that the
pieces were right and the assembly was missing.**

## 2. Architecture

### 2.1 System overview

Unchanged from v2 in every box and arrow; restated so this document stands alone:

```
                      ┌────────────────────────────  one command surface (T4)  ───────────────────┐
                      │                                                                           │
   webcam-handler-cli ──▶ engine::facade (D18) ──▶ CameraBackend trait (T1/T2) ──▶ webcam-handler-v4l2 ──▶ /dev/video*
                      │        ▲                                               └─▶ webcam-handler-fake ──▶ device-profile corpus
webcam-handler-client ──▶ jsonrpsee client ─┐                                                 (captured from real cameras)
           web client ──▶ WS / HTTP ────────┴─▶ webcam-handler-daemon: jsonrpsee server ──▶ engine (same trait objects)
                                                  │  UDS always · TCP opt-in (D11)
                                                  └─▶ axum: static web assets · MJPEG preview · /session-photo (D20) · WS
```

The crate table, request lifecycle and concurrency model are v2's, with two v3 notes. The
direct CLI's in-process executor is now a thin wrapper over `engine::facade` (D18) — the
same composition, promoted to a public home the CLI consumes, so the diagram's first arrow
names the thing an embedder holds. And the daemon's per-camera **actor** registry is
deliberately *not* the facade's consumer: the facade is the one-shot, caller-owns-lifetime
composition; the actor registry is the long-lived, daemon-owns-lifetime one; both sit on
`engine::resolve`, which stays the single resolution home (D14).

**Concurrency model (D12)** — one OS thread per open camera (the camera actor), command
channel in front, exclusive streaming by construction, latest-frame fan-out for the
preview. Unchanged; D12 below carries the rules the G6 review added.

### 2.2 The domain model — decisions

**D1 — Camera identity and grouping.** A *camera* is a group of device nodes sharing a USB
*interface*, keyed on the sysfs interface path — never on `QUERYCAP bus_info`, which both
Chicony logical cameras report identically [PF:7, PF:13]. The capture node is the **first**
group member in node order whose `device_caps` contain `VIDEO_CAPTURE` [PF:19 — one camera
can own two capture nodes, and the second is listed but unreachable; PF:27 — it can even be
a different sensor]; `META_CAPTURE` nodes are recorded but never streamed. Identity has two
tiers:

- `CameraId` — the name RPC calls and CLI arguments use. Grammar: `cam:<card-slug>[-<n>]`,
  where `card-slug` is the querycap card name through the slug transform (D2's, `-`
  separator) and `-<n>` (n ≥ 2) is appended on collision in enumeration order. Stability
  scope: reproducible across runs while the attached-device topology is unchanged; stable
  for one engine instance's lifetime across replug (the id follows the fingerprint); never
  persisted as identity. A natural slug always wins its own name; a collision ordinal
  increments until it collides with nothing, naturals included; and the `camera-<index>`
  fallback for a card that slugs to nothing is contested against reserved naturals too —
  the collision the comment called impossible happened (docs/11 L3, N226). Commands accept
  any unambiguous prefix.
- `CameraFingerprint` — best-effort stable across replug/reboot: the USB *interface* path
  [PF:13], VID:PID, card name, driver, serial *when the device provides a distinguishing
  one* [PF:8]. Calibration sessions record the full fingerprint and match conservatively;
  a mismatch on `apply` is a refusal naming the differing fields.

Node numbering is never load-bearing **as identity** — one `uvcvideo` reload rotated three
cameras through each other's minors [PF:22] — but it is a perfectly good *address*, and
D14's `NodePath` selector uses it as exactly that: resolved against the live listing at
each call, never recorded as who the camera is. An empty enumeration is diagnosed, not
shrugged at: the per-*device* driverless-USB-camera scan [PF:14] and the `NodeUnreadable`
hint reach clients through T1's `diagnose` — and the hints describe the same probe pass
that produced the listing they explain, stamped and handed over rather than re-read
(docs/11 M7; N193, N198).

**D2 — The control model: represent, don't reject.** Unchanged from v2, restated: a
descriptor carries numeric id, name, slug (the pinned transform — `Zoom, Continuous` →
`zoom_continuous`), type, range `{min, max, step}` as i64, default, flags (raw u32 plus
the decoded known set [PF:12], the decoded set compared against bindgen's own constants
rather than hand-copied — thirteen bits had drifted from nothing checking [docs/11 L11,
N228]), sparse menu map [PF:2], element count and size for array/compound controls
(element count is device *state*, not identity [PF:17]), and the current value *as read,
unvalidated* [PF:4]. Control types are a closed enum with `Unknown { raw }` carrying
payload size [PF:1]. Out-of-range currents and defaults are reported as measured, flagged,
never corrected [PF:4, PF:5]. The enumeration loop is ours (raw `QUERY_EXT_CTRL` +
`QUERYMENU` tolerating holes); `v4l::query_controls` is lint-banned [PF:1]. Two rules the
G6 review added: **one control the device declines to read is carried valueless rather
than ending the walk** — and the tolerance is `EBUSY` alone, because folding EPERM, EIO
and timeouts into "no value" converts three of rule 7's four classes (docs/11 M6; N192,
N196) — and **a declined value is a visible absence**: snapshots record it, profile
capture refuses on it, restore neither invents it nor reports complete over it (N195).

**D3 — Writes read back; guarded writes handle automation.** Unchanged from v2: `set`
returns `{requested, applied}`; a clamp [PF:6] is a warning-carrying success; a guarded
set resolves and disables automation partners first, reporting every change. For motor
controls, `applied` means *accepted*, not *achieved* [PF:18]. Pairing resolution is
layered — the declared table (data, `webcam-handler-schema`) under empirical discovery
(`--discover-pairs`, and at calibration start), with measured pairs recorded in the
profile and trumping the table (E1). The three probe rules stand, each paid for at P2: a
menu is not a switch; residue is isolated; "off" is recorded per freed control by
menu-item name [PF:2]. And the probe's *own* restore is a full citizen: it snapshots
through the pairing-aware path so its restoration ordering is D4's, not alphabetical
(docs/11 M13; N143).

**D4 — Snapshot and restore.** Unchanged from v2: automation first, manual second,
two-pass INACTIVE handling; four outcomes with `OwnedByAutomation` counting as complete
[N9, PF:24 — the volatile flag is not the tell; the outcome is]; sweeps and guarded
operations wrap themselves in snapshot/restore by default. Two v3-absorbed rules:
restore-after-calibration is a *verb* (`calibrate restore`), not a default — the snapshot
is session-scoped and every sweep prints the restoring command [N23, N20] — and a
snapshot stamped after a disturbance the driver caused is refused by `restore_across`
rather than replayed, because a post-cycle snapshot records a position nobody commanded
and restoring it *introduces* an error that reports `AlreadyCorrect` [PF:25, PF:28, N86].

**D5 — The capture pipeline.** The v2 statement stands — settle policy explicit and
bounded [PF:11], mmap'd buffers, verbatim MJPG to `.jpg` sinks (E6), the negotiated result
always reported — with the 2026-08-13 re-ranking ruling and the G6 repairs folded into one
clean statement of resolution:

- **An unspecified request re-ranks the device's formats** (owner ruling, 2026-08-13;
  [PF:26], N85): resolution is the primary key (a format's resolution is the largest its
  size list offers, and the chosen format streams at that size), lossiness the tiebreak,
  **coupled to the sink** — into a JPEG sink a compressed format wins (it arrives byte for
  byte, E6); into a PNG/PPM sink the uncompressed one does. A FourCC this build cannot
  decode ranks behind every one it can name (ranked, not filtered); the driver's
  enumeration order is the tiebreak of last resort. `SinkFidelity` is derived and never on
  the wire; the chooser's answer carries a `ChoiceReason` naming the rule that fired.
- **An explicit request wins or is refused, and the refusal has one home.** The G6 review
  found this sentence honoured by the fake and violated by the real backend, green on both
  (docs/11 H1) — so the rule now lives where both backends inherit it:
  `StreamRequest::choose` answers `Result`, a named-but-absent format is
  `FormatUnsupported` from inside the shared resolver, and the fake's own pre-filter is
  deleted rather than kept as a second copy [N134]. **A named size narrows the ranking's
  candidate set device-wide** and, when nothing delivers it, refuses with a
  `SizeRefusal` naming the size and the deliverable sizes — never a substitution, and
  never a veto by a format the ranking happened to pick first (owner ruling, 2026-08-16;
  H1b, N138). Stepwise entries still answer `largest_within` — the closest deliverable
  size, never the maximum corner.
- The conformance battery's `ExplicitRequest` arm is the population that walks this
  contract on **both** backends (§3.3 item 10) — the arm that would have caught H1 the day
  the fake grew its guard.

**D6 — Photo outputs.** Unchanged from v2: sources closed at MJPG, YUYV, GREY, NV12;
`.jpg` = verbatim camera bytes when negotiated MJPG, `.png`/`.ppm` decoded and encoded
in-process; orientation transforms are an EXIF tag on the verbatim sink and pixel-domain
elsewhere — and every one of the six transforms is asserted over `Transform::ALL`, the
population that was four-sixths unwalked at G6 (docs/11 L21, N207). EXIF is the
header-only APP1 splice [PF:16], with device-derived text bounded: descriptions shorten to
`MAX_EXIF_TEXT_BYTES` on a whole-control boundary and say so, and the segment length is
checked against `MAX_EXIF_APP1_BYTES` rather than truncated by the library's u16 (docs/11
L5, N203). Raw-format decoders take the buffer `plane_bytes` says a driver owes — a
padding-free final row is admitted by all three, not two (docs/11 M16; N201).

**D7 — Video recording: the license-layered strategy.** L0 ships and is the whole v1–v3
story: MJPEG → AVI, our muxer, no encoder, no patent surface, no copyleft; Y4M as the raw
escape hatch for YUYV/GREY/NV12, with the chroma siting carried as a capture-time input
(`ChromaSiting`, owner ruling 2026-08-16 — `C420` *is* a siting claim and now it is a
stated one, oracle-read back [N200, N210]). Honest sizes, re-priced at P6 and dated to
N99's measurement: the muxer 568 code lines, its independently derived re-parser 795 —
D7's "~300 lines" was the muxer alone and the license argument survives unchanged [N99]. AVI is constant-frame-rate
and cameras are not: the header's rate is rewritten at close to the measured mean
interval; Y4M's fixed-width rational is patched in place the same way, so `Measured` is
reachable in both containers [N106]; the declared-vs-wall-clock bound is two-sided and
asserted on real hardware [N120, E17]. A recording deliberately does **not** settle —
early frames are visible, and discarding them would move the take's start [N111]. A
recording's bytes go to a path: `record` refuses `ReturnBytes`, because a capped recording
is 32–43× the RPC response cap and the rule lives on the request type so socket-built
requests meet it too [N110]. The recording answer carries D16's stream stats. The deferred
layers (L1 UVC H.264 → MP4, L2 rav1e, L3 openh264) and the rejections stand as v2 recorded
them; §7 carries N103's WebM disposition.

**D8 — The calibration model.** Unchanged from v2 in shape: a session belongs to (camera
fingerprint, task); goal and ordered criteria; per-control status through the closed
vocabulary `Untouched → Sweeping { plan, done, total } → Calibrated { value, precision,
score, selector } | Deferred | Blocked`; sweep plans `All`/`Uniform`/`Log`/`Explicit`
planned from the *measured* range; scoring by the built-in metrics, which **rank and never
decide** — the `Calibrated` record names its selector (`metric:<name>`, `agent`, or
`human`), and D20 is where `human` finally gets a producer with eyes on the photographs.
Absorbed rules: `AutoDisabled` keeps **no product producer** — automation is disabled by
the guarded write at the first sample and the event (`AutomationDisabled`) is appended
from the *write report*, after the device did the thing, because a log line is a claim
about a camera (docs/11 L15; N229, N233). A sweep that recorded zero samples returns its
control to `Untouched` (`abandon_sweep`) whether the process lived [N24] or died —
`lifecycle::recover` frees stranded `Sweeping` states on every recovery arm, appending
`SweepInterrupted` whose `failure` is `Option<ErrorKind>` because a later process must not
invent a refusal nobody measured (docs/11 H2; N139, N149). Sample paths carry the sweep
pass so a refinement never overwrites the coarse pass's evidence [N22]. `SweepAdjustment`
— what the planner changed about what you asked — reaches the answer, the durable log and
the live event, written by one transition [docs/11 M14; N145].

**D9 — Persistence: inspectable files, atomic writes, one lock.** The v2 layout stands
(`session.json`, `log.ndjson`, `photos/<control>/<pass>/<value>.<ext>`, relative camino
paths, `schema_version` from day one). Absorbed at v3, each from a G6 finding:

- **The session tree is private by mode and by decision**: directories 0700, files 0600,
  owner-checked (`st_uid == geteuid`), the mask excluding only group/other bits so an
  inherited `S_ISGID` is not refused [N150]; a pre-existing wide tree is **refused, never
  repaired** (N39's ruling), with the remedy printed — a session tree holds photographs
  and a frame may contain a person (docs/11 M11; N142).
- **A torn `log.ndjson` tail is healed at the next append**, under the lock already held —
  terminate a parseable tail, truncate an unparsable one back to the last newline — so a
  crash plus one later append can no longer manufacture the interior corruption
  `load_log` refuses forever (docs/11 M9; N140). The *refusal* of interior corruption is
  settled law and untouched [N12]; and a repair that copies a guard copies the guard's
  test — the heal's own `NotFound` arm re-created a closed defect until N253.
- **`write_json_atomic`'s contract is the truth**: the parent fsync runs after the rename
  publishes, so its failure is classified (`Reach::{Untouched, Published}`) and
  `lifecycle::persist` matches on it instead of believing a sentence the code could not
  keep (docs/11 M10; N141).
- The lock-file write is the one recorded non-atomic state write [N11]; a daemon holding
  one `flock` serializes against itself on a session mutex whose guard *is* the right to
  edit [N47]; on-disk changes a `#[serde(default)]` can absorb do not bump
  `SESSION_SCHEMA_VERSION`, and the rule for ones that cannot is written beside the
  constant [N151]. Session writes are quadratic in sample count and stay so on a
  measurement: 0.2 s of a 512 s sweep, priced against the crash-safety the "fix" would
  cost [N146].

**D10 — One wire surface, one home (T5).** The whole daemon API is one `wire_surface!`
declaration in `webcam-handler-api` over `webcam-handler-schema` DTOs (one declaration,
two generated traits since P4e-i — N57 — which is the "one trait" sentence's honest v3
spelling: one *source*). The daemon implements the server half; `webcam-handler-client`
consumes the generated client; the direct CLI reaches the same verbs through T4's executor
over the D18 facade — a verb exists exactly once. Methods (namespace `wch`): `list`,
`info`, `controls`, `discover_pairs`, `get`, `set`, `snapshot`, `restore`, `photo`,
`record_start`, `record_status`, `record_stop`, `profile_capture`, `terminate_holder`,
`calibrate_start`, `calibrate_plan`, `calibrate_sweep`, `calibrate_status`,
`calibrate_select`, `calibrate_apply`, `calibrate_restore`, `calibrate_list`,
`subscribe_events`, and `subscribe_calibration`. jsonrpsee joins the namespace to the name
with `_`, so the wire spelling of `list` is `wch_list`; that prefix is a **wire break**
and is never renamed in passing (note N91 — and §8's naming question explicitly excludes
it). Every name above is written out because `wire-surface-sync.sh` reconciles this
sentence against the macro member by member, and a shorthand is a member the reconciler
cannot see. **v3 adds no method and no subscription**: D14 widens the grammar of the
existing camera string parameter (the selector spellings, parsed by the one selector home
in the schema crate, on the daemon side); D15's and D17's verbs are document verbs that
never touch a socket (T4's new clause, §2.7); D16 rides the existing `record_stop` answer;
D20 drives the eight calibrate verbs that already exist and fetches sample bytes over
HTTP (D20's route), not RPC. Binary results cross as the two-variant sink DTO
(`ReturnBytes` base64 / `ServerPath` camino path; `record` refuses the first — N110). A
`PixelFormat` crosses as its FourCC string with the prefix-free escape [N109]. Errors
cross as the D13 registry; DTOs derive `schemars::JsonSchema`; the committed JSON Schema
bundle and OpenRPC document are generated artifacts, and the prose inside them speaks to a
reader with no Rust toolchain — rustdoc links are rewritten out at emission, in every
spelling the sweep found [docs/11 M22; N148, N218, N222, N249].

**D11 — Transports and the security posture.** The Unix socket
(`$XDG_RUNTIME_DIR/webcam-handler/wchd.sock`, directory 0700, dirfd-held and bound through
`/proc/self/fd` so substitution is defeated, not detected [N39]) is always served. TCP is
**opt-in** (`--http [addr]`, default `127.0.0.1:0` → report the bound port), serves the
web client, and requires a bearer token generated per run and printed as a ready-to-open
URL. The bind × token matrix, stated once: loopback + token is the default; token-less
loopback exists only behind `--http-insecure-loopback`; non-loopback **always** requires
the token and warns naming the exposure. ~~generated per run and printed unless
configured~~ — the "unless configured" clause is **struck at this revision**, executing
N79: it promised a surface that never existed and that nothing consumed for three phases.
The named re-trigger survives the strike: a consumer that needs TCP, cannot read startup
output and cannot use UDS gets a 0600 credential file under `$XDG_RUNTIME_DIR`, designed
then, not before (N79, N182).

Three absorbed amendments complete the posture:

- **The token is for the camera, not for the client's own source** (owner ruling,
  2026-08-12; N82): the bearer token gates exactly the routes that carry or drive the
  camera — `daemon::http::CAMERA_BEARING_PATHS`, which names them,
  and which D20 makes **three** when `/session-photo` lands (docs/13 P9b) — and not the
  static assets, which are this project's own open-source code. "Every route is gated" is a property of that list, and
  the two halves that go red on a route added without a gate are
  `web-routes-are-gated.sh` and `every_camera_bearing_route_is_behind_the_gate`. Served
  responses carry `Referrer-Policy: no-referrer`; an anonymous request for an unserved
  path is 404, the ruling's stated price.
- **Provenance runs before credentials** (owner rulings, 2026-08-13; N93, N95): requests
  a browser marks foreign are refused 403 over the whole listener — `Sec-Fetch-Site` as
  the primary signal (`/preview` carries no `Origin` at all — measured), `Origin`
  corroborating against the request's own `Host` authority, absence admitted because
  every non-browser consumer sends neither, unrecognised values refused, and disagreeing
  duplicate `Host` lines yielding no authority at all [N180]. The DNS-rebinding residual
  is named and accepted (N93), and it is still the only way in that twenty live request
  shapes found (docs/11 §7.2) — with N250's postscript: the twenty-first shape, a bare
  `?token` beside a verifying bearer, was covered by the every-credential-must-verify fold
  [N74] and is now pinned by the test the mutation floor demanded.
- **The journald sink redacts the token; the terminal does not** (owner ruling,
  2026-08-16; docs/11 M25, N182): the ready-to-open URL is the one place the secret is
  written down on purpose, an operator's terminal still gets it whole, and the persistent
  indexable sink gets it stripped by shape.

Loopback + token because loopback alone is not an auth boundary on a multi-user machine; a
camera is a privacy-sensitive device; the posture errs closed.

**D12 — Concurrency and ownership.** One actor per camera serializes device access by
construction; exclusive streaming; cameras open on first use and close on idle; control
I/O interleaves with streaming. The amendments and G6 rules, folded:

- **A photo suspends a live preview** rather than being refused by it (owner ruling,
  2026-08-12; N83): stop–capture–restart inside one actor command, bounded by
  `PREVIEW_SUSPEND_MAX_MS`, restart on every exit path. T2's ninth method, `streaming()`,
  exists so the suspension can restore exactly the negotiated stream it interrupted
  [N132]. What still meets `Busy` is a calibration sweep — minutes of photos — and, since
  P6c, a photo during a *recording*, because the suspend mechanism cannot tell whose
  stream it would stop and a gap would corrupt the take's timing silently [N118]; that
  refusal is checked **on the camera's own actor thread**, not three awaits earlier
  (docs/11 M1; N170, N176).
- **One take per camera, and the sequence is total** [N114]: a second `record_start` is
  `Busy`; a start over an uncollected take discards it, counted; `record_stop` on nothing
  is `IllegalTransition`; `record_stop` collects. A take whose camera vanished is still
  collected under the name it started with [docs/11 M4; N173]. The preview during a take
  is fed from the recording's own frames (owner ruling, 2026-08-14; N117).
- **A claim on a camera comes back with its value** (the G6 review's M1/M2/H2 shape;
  N169, N171, N177): a recording slot, a preview's watchers, the device's own `STREAMON`
  — every claim is held by a value whose release is its own (`Drop`, or `#[must_use]`
  where the debt is to *start* something), witnessed by a `Weak` the registry reaps on
  every entry, because a release that depends on a later line running is a camera the
  agent meets as `Busy` forever. `claims-come-back-with-their-values.sh` holds the shape
  over the two claim modules' closed constructor vocabulary.
- **Shutdown is bounded by a table, not a sentence** [docs/11 M27; N174]: every teardown
  step is under the shared drain deadline, the runtime itself ends under
  `shutdown_timeout`, per-connection sockets are shut down when the stop expires [N175],
  and the worst case is written as the sum of four waits beside the systemd
  `TimeoutStopSec` that must exceed it. A panicked idle-sweep pass is reported and the
  driver carries on [docs/11 L7].

**D13 — The error vocabulary.** `webcam-handler-schema` defines the closed typed registry
— **eighteen variants**, each carrying what the caller needs to act: `DeviceGone`,
`Busy { holders, this_process: Option<Occupation> }`, `PermissionDenied { path, hint }`,
`CameraUnknown { requested }`, `CameraAmbiguous { requested, candidates }`,
`ControlUnknown { requested, did_you_mean }`, `ControlReadOnly`,
`ControlInactive { control, automation }`, `FormatUnsupported { requested, available,
size: Option<SizeRefusal>, container: Option<ContainerRefusal> }`, `SettleTimeout`,
`FingerprintMismatch { fields }`, `SessionConflict`, `IllegalTransition { from, op }`,
`SchemaVersionForeign`, `StoreLocked { holder, protocol }`, `HolderGone`,
`DeviceIo { operation, errno, message }`, `StorageIo { path, errno, message }`. Absorbed
at v3:

- **`Busy` says what this process is doing** without naming a pid a client could be
  invited to kill (N48 point 5 stands): `this_process: Option<Occupation>`, a closed
  five-way vocabulary (recording, starting a recording, running commands, streaming,
  streaming a preview) whose only door is `Error::busy_here`, with advice per occupation —
  because "held by an unidentified process" for a holder this daemon knows precisely sent
  agents to a remedy that could not reach the refusal (docs/11 M19; N217, N221).
- **`FormatUnsupported` has three exclusive doors** — a format the device lacks, a size
  nothing delivers (`SizeRefusal`), a container the negotiated format cannot ride
  (`ContainerRefusal { container, negotiated, carried_by }`) — so the message never
  attributes one list to another's owner again [N129, N138, N211].
- **`IllegalTransition` renders `"{from}: cannot {op}"`** — the instruction ends the
  sentence, for all eleven producers whose `op` is one (docs/11 L29; N212).
- **The registry reaches a command line as a document with an exit code per kind** (owner
  ruling, 2026-08-15; N124, N127, N128): a failing `--json` run prints
  `schema::error::Failure` — the D13 kind in the registry's own serde spelling, the
  payload, the message — through the one `cli_core::report_failure` both roots call, with
  `cli_core::exit_code` an exhaustive match onto the contiguous block `10 ..= 27`. No
  answer may carry `FAILURE_MARKER`; a `--json` run prints exactly one schema type, and
  which type says whether it answered.
- **The client keeps a discriminant the wire delivered**: an error object whose code is
  ours but whose payload this build cannot read answers as the *kind*, never as a dead
  socket — `Received::{Refusal, Kind, Foreign}`, derived by walking `ErrorKind::ALL`
  (docs/11 M21; N215).
- **A D13 message is payload** — the part the primary consumer reads first — and the test
  for one asserts its *claim* against the product, never its wording (N129, N211; rubric
  A15). Messages name no flag a surface lacks and no verb a root cannot run [N123, N220].

The registry is errors only (a clamp rides the write result as a warning); `ErrorKind` and
its `ALL` come from `closed_vocabulary!`; a nineteenth variant does not compile until the
round-trip, rendering, RPC-code and exit-code walks all know it. **v3 adds none**: D14's
failures reuse `CameraUnknown`/`CameraAmbiguous` (a selector is a request, and those two
kinds are precisely "resolved to nothing" and "resolved to more than one"); D17's
dimension mismatch is not an error at all (D17 represents it); D19's device-loss contract
is spelled entirely in existing kinds. The one candidate for a nineteenth is §8's
process-failure question (N238), which is the owner's.

**D14 — Camera selection: one resolver, five spellings** *(new at v3; FR-W1).* A caller on
a multi-camera machine usually knows *which* camera it means by something it already holds
— a `/dev` node it just watched appear, a bus position, a VID:PID, a serial — and until v3
this tool made it re-derive the card-name slug from an enumeration, which is exactly the
composition the FR ledger's fallback performs by hand. v3 makes the knowledge a spelling:

- **The vocabulary** (`schema::selector`, a `closed_vocabulary!` so every match walks it):
  `Id` — today's grammar, `cam:` prefix optional, unambiguous-prefix matching unchanged;
  `NodePath` — a string beginning `/`, matched against any of a camera's `nodes[].path`
  (any node, not only the capture node: you address by what you can see in `/dev`);
  `BusPath` — `bus:<interface-path>`, matched exactly against `fingerprint.bus_path`
  (`bus:3-4:1.2`); `UsbId` — `usb:<vid>:<pid>` hex, against `fingerprint.usb_id`;
  `Serial` — `serial:<text>`, against `fingerprint.serial`, where a device that reports no
  serial matches nothing [PF:8]. The scheme table is closed: card slugs cannot contain
  `:` (the slug alphabet is `[a-z0-9-]`), so a colon-delimited scheme this build does not
  know is unambiguously a selector, and it resolves to `CameraUnknown` with a message
  naming the scheme vocabulary — a spelling mistake is a request no camera can ever match,
  which is precisely what that kind means.
- **One parser, one resolver.** `schema::selector::parse` is the only place a spelling
  becomes a `CameraSelector` — both CLI positionals, the wire's `camera` parameter and
  D18's facade go through it — and `engine::resolve::camera` (the existing home) widens to
  take the selector. Zero matches → `CameraUnknown { requested }`; more than one →
  `CameraAmbiguous { requested, candidates }` — **no new error kinds**, because selection
  failures are exactly the two failures resolution has always had, and an agent's dispatch
  table already knows them.
- **Selection never filters enumeration.** The resolver matches over the *full* live
  listing, and `V4l2Backend::new()` keeps its zero parameters. Two reasons, both
  load-bearing: D1's collision ordinals are assigned over the whole machine, so a filtered
  enumeration would move `cam:obsbot-tiny-3-2`-class names depending on the filter — an id
  whose meaning depends on how you asked is not an id; and E2 — the device list is the
  machine's truth, and a backend that pre-narrows it is an authority this design gives
  nobody.
- **Address versus identity, stated on the tin.** `NodePath` is an *address*: node numbers
  are probe-order bookkeeping that one driver reload rotates [PF:22], so the spelling is
  resolved against the live listing at each call and is documented as the session-scoped
  choice for a caller that just materialized a node (the FR's vhci consumer; a udev hook).
  `BusPath`, `UsbId` and `Serial` are fingerprint-tier and survive replug. The guide's
  selector table carries this split, because the primary consumer reads dispositions, not
  design documents.
- **Ambiguity is the normal case somewhere**, so it is a first-class refusal, not an edge:
  the two Chicony logical cameras share one `usb_id` [PF:13] — `usb:04f2:b83c` on the seed
  corpus answers `CameraAmbiguous` with both candidates, and that committed pair is the
  fixture the refusal is pinned on. On the FR's consumer machine, a forwarded camera and a
  local twin of the same model are the same shape; the candidates list carries the
  bus-path difference that tells them apart.
- **Sessions are untouched**: calibration still binds to the full fingerprint (D8), and a
  selector plays no part in `apply`'s conservative matching — a selector finds a camera, a
  fingerprint proves it is the same one.

**D15 — The device projection and the masked compare** *(new at v3; FR-W2 — "the single
highest-value request in this file").* A device profile's invariant carries two kinds of
fact and v3 states the partition instead of implying it: **identity** — `info` (id,
fingerprint, bus strings, node table) — is *where the device is*; **description** —
`formats`, `controls`, `measured_pairs` — is *what the device is*. Across a forwarded bus,
a different port, a reboot or another machine, identity legitimately differs while the
description must not; the shipped whole-invariant compare can only answer "different" there,
for reasons that are the point of forwarding.

- **The projection is a destructuring, not a list.** `ProfileInvariant`'s partition is
  stated by destructuring the struct into named sides (the shape
  `invariant_difference` already uses), so a field added later fails to compile until
  somebody assigns it a side — closed in both directions by construction, which is the
  only mechanical defence this design trusts for a partition (§2.10's lesson from docs/11
  §9.1: the rows that work are the ones with an `ALL` behind them).
- **The answer is a named diff, not a bool.** `DeviceProfile::compare(&other) ->
  ProfileComparison` — a schema DTO, in the committed bundle: `device` (per-section — the
  differing control slugs by name, the format-tree delta, the pair-set delta) and
  `identity` (the existing `differing_fields` vocabulary) as two halves of one document,
  because the FR's consumer needs both answers from one comparison — "same device?" as the
  fidelity assertion and "what identity moved?" as the expected-delta report.
  `device_matches` is the derived bool for callers that only branch.
- **The format-tree caveat rides the answer.** A camera's advertised format tree is
  invariant within a connection and nowhere else (owner ruling, 2026-08-13; N89, PF:23) —
  so the comparison reports the format section separately and `ProfileComparison` carries
  the `is_only_the_format_tree`-shaped distinction, letting a consumer apply the policy
  its situation warrants instead of this tool guessing.
- **One home, and the test becomes a consumer.** The corpus-replay suite has masked
  identity by hand since P1; that mask is promoted into the product and the suite deletes
  its private copy and consumes the projection — the request was evidence the pieces were
  right and the assembly was missing, and a second mask kept "for tests" would be the
  §2.10 defect. The verb is `profile compare <a.json> <b.json>` (a document verb, §2.7),
  and the corpus arm asserts every committed profile compares device-equal to an
  identity-rewritten copy of itself and device-unequal to every *other* profile — both
  directions, over the whole corpus.

**D16 — Stream health is a payload** *(new at v3; FR-W4).* USB-over-IP's characteristic
failures — added latency, isochronous bandwidth collapse, dropped frames — are visible
only in two fields this tool already delivers: `Frame.sequence` ("gaps mean dropped
frames") and `Frame.timestamp_us` (the driver's clock). Thermal throttling and contended
hubs show up the same way on an ordinary rig. v3 does both things the FR offered as
alternatives, because they are cheap together and each covers the other's gap:

- **The fields are contract** (the FR's option b). The two doc comments' semantics become
  stated, tested contract: the committed schemas carry them, the fake's frame synthesis
  honours them, and a scripted `Fault::FrameGap` exists so a consumer's gap-accounting has
  a driven inverse — client-side aggregation over `sequence` + `timestamp_us` is a
  supported, stable use of the API, which is the sentence the FR asked somebody to own.
- **The aggregator is a pure core** (the FR's option a):
  `imaging::stream_stats::Accumulator` — push `(sequence, timestamp_us)` per frame, read
  `StreamStats` at the end: frames delivered, dropped (summed sequence gaps), gap events,
  interval mean/min/max/p50/p99, and jitter as mean absolute deviation, all in
  microseconds, all integer arithmetic. **Bounded by an existing bound**: intervals are
  retained exactly up to `MAX_RECORDING_FRAMES` (16 384 — ~128 KiB, the recording cap that
  already governs every take), and an accumulator pushed past it degrades to
  streaming-moments-only with the truncation stated on the answer — never silently, per
  AGENTS rule 3's spirit one layer down. Percentiles are exact over what was retained; a
  fixed field set (`p50`, `p99`), never a map that invites unbounded vocabulary.
- **The recording answer carries it.** `RecordReport` gains `stats: StreamStats` — the
  subprocess consumer gets delivery health from the verb it already runs, with wall-clock
  skew computed there (`driver span` vs the take's own start/stop stamps — the pure core
  sees no wall clock, so skew is an `Option` field the record path fills; a consumer
  aggregating a live stream in-process fills it from its own stamps or leaves it absent,
  and the field is public precisely so it can). Two home notes: `StreamStats` is a
  `webcam-handler-schema` DTO — it rides a wire answer, and every type serving the four
  masters lives there — while the accumulator's arithmetic lives beside
  `imaging::video::declared_interval`, which P6d already made the one place a mean frame
  interval is computed; the accumulator extends that home rather than founding a second.
- **What this is not**: no live-stream verb, no trending store, no threshold judgment —
  the stats *rank and report* exactly as D8's metrics do, and deciding "healthy" belongs
  to the consumer whose tolerance it is.

**D17 — A/B comparison** *(new at v3; FR-W3).* "Same scene, two paths" — two units of one
model, before/after a firmware update, one camera direct and one forwarded — wants a
comparison record, and everything needed exists as pure pieces:

- **The core is total.** `imaging::compare::photos(a, b) -> Comparison`: per-metric values
  for each side and deltas (the D8 metric set, `MetricName::ALL`-walked, so a sixth metric
  joins the comparison by existing), plus an SSIM corroborator. **A dimension mismatch is
  represented, not refused** (D2's doctrine applied to a comparison): per-metric scalars
  are well-defined on unequal images and stay, while `ssim` answers
  `Unavailable { reason: DimensionsDiffer { a, b } }` — a closed reason vocabulary, so an
  unattended consumer branches on data instead of parsing a refusal, and **no new error
  kind exists**. The record states which comparisons were made; nothing resizes anything,
  ever — a silent resample is a codec artifact smuggled into the loop (E6).
- **SSIM's source is decided, with its condition stated.** `image-compare` 0.5 (MIT,
  verified in the 2026-08-07 adversarial license audit; the crate deny.toml's `dssim` ban
  has named as the permissive answer since P0) is adopted under the 2026-08-09 ruling —
  the licence clears the bar, the crate looks solid — **conditional on one measurement at
  adoption**: its `image` edge must not re-enable the default features this workspace
  turned off, because cargo feature unification is workspace-wide and the `avif` → rav1e
  drag is the trap `feature-posture.sh` exists for. That gate reads the *resolved* graph,
  so the trap cannot arrive silently even if this sentence is forgotten; if the graph is
  dirty, the fallback is named — a grayscale windowed SSIM owned in `imaging::metrics`
  (~a hundred lines over primitives already linked), the same make-vs-take answer the AVI
  muxer and the XDG paths gave [N2]. `dssim` stays banned; metrics still rank, SSIM
  corroborates, and neither decides.
- **The verb is `photo diff <a> <b>`** — a document verb (§2.7) over files this build's
  own decoders read (JPEG/PNG/PPM through the existing `imaging` paths, to luma). The
  two-camera *capture* half is deliberately composed, not built (§1's new non-goal): two
  `photo` calls whose scene timing the caller owns, then one `photo diff`.

**D18 — The embedding facade and the supported-composition contract** *(new at v3;
FR-W5).* The engine has been consumable as a library since P0, and the FR's consumer
found the actual cost: the blessed call order lives in `webcam-handler-cli`'s private
`InProcess` executor, so an embedder reverse-engineers a CLI and re-verifies the
five-module assembly at every upgrade. v3 promotes the assembly, and the promotion is
shaped so drift is structurally impossible:

- **`engine::facade` is the composition** — `Facade::new(backend: Box<dyn CameraBackend>)`,
  with the one-shot verbs an embedder holds: `list()`, `resolve(&CameraSelector)`,
  `open(&CameraSelector)`, `photo(...)` (the settle/negotiate/capture/stamp pipeline
  behind one call), `profile(...)` / `profile_probed(...)`, and `watch()`. It is not a new
  layer: it is the `InProcess` assembly moved into the engine, **and the direct CLI's
  executor is rebuilt on it** — each executor verb is parse-and-render around one facade
  call — so the facade cannot drift from what `webcam-handler-cli` ships, and the CLI
  parity gate transitively pins the facade's answers. One home, one consumer relationship,
  no second copy (§2.10).
- **What the facade deliberately excludes**: calibration and recording lifecycles. Both
  are stateful compositions with a store lock, a session mutex and (in the daemon) an
  actor's thread behind them; an embedder that wants them wants the daemon or the CLI,
  and a facade method that half-owned a session would be a second lifecycle home. The
  boundary is stated in the module doc, with the daemon named as the long-lived
  composition (§2.1).
- **The supported-composition contract** (the FR's option b, delivered beside option a):
  a stability table in the facade's module doc naming what an embedder may hold and what
  it may not. **The table is the list, and this sentence is not a second copy of it.**
  What belongs in the Yes column is stated here as a rule rather than an enumeration: the
  schema types and T1/T2 (the vocabulary all four masters share), the backend crates
  (`Facade::new` takes a `Box<dyn CameraBackend>`, so constructing one is the caller's job
  by construction and no facade method could cover it), the engine's pure cores, the facade
  itself, every engine module a facade *signature* forces on a caller, `imaging`'s pure
  functions — the metrics among them, since metrics are imaging's and the engine has no
  such module — and the conformance suite, which is *for* backend consumers. What belongs
  in the No column: the engine's shell-module internals, the daemon's modules, `cli-core`,
  `web`, and the test oracles. Which modules those rules land on is answered by the table
  and by nothing else, because a prose copy of a contract drifts from it silently and this
  one had — measured, three engine modules and three testkit modules apart, with nothing
  able to compare them (notes **N270**, **N271**). `facade-stability-table-sync.sh` holds
  the table to the crates it names in both directions, so every module of every crate it
  classifies sits in exactly one column, a module added to the engine stops the gate until
  somebody has decided which, and this sentence is held to naming the table rather than
  restating it. Versioning honesty: this workspace is 0.x and consumed
  pinned-by-rev; the contract is that the named seams move *deliberately*, a break gets a
  Changes row and a note, and nothing else is promised.

**D19 — Mid-stream device loss: the contract, stated before the measurement** *(new at
v3; FR-W6 — accepted as offered).* This design records mid-stream loss on real hardware
as unmeasured **by design**: the privileged helper refuses to unload `uvcvideo` under an
open node (§2.13), so the event exists locally only as the fake's scripted fault (§3.3
item 9). The sibling project can produce the real event reproducibly — drop the tunnel,
detach the vhci port — a camera vanishing under the driver exactly as a yanked cable,
with nobody touching hardware. v3 accepts the offer by doing the one thing that makes
contributed evidence worth having: **stating the expected behavior in advance**, so the
partner rig tests this design's claim rather than blessing whatever the code does (the
independence rule this project already applies to its own oracles — the AVI re-parser was
written from the specification before the muxer existed).

The contract, spelled entirely in existing vocabulary:

- **During a photo**: a device that vanishes answers `DeviceGone` — never `SettleTimeout`
  (the deadline did not expire; the device left), never `Busy`, never a capability
  refusal. The actor's liveness guard already owns this answer [N41].
- **During a recording**: the take *finalizes* — a valid AVI/Y4M up to the last complete
  frame (D7's crash story, at last with a producer for its event), a `RecordReport`
  whose end names the device failure, collectable by `record_stop` under the id the take
  started with [N173], stats included (D16 — the gap accounting right up to the loss is
  the rig's streaming-fidelity measurement).
- **During a preview**: the feed ends; viewers' streams close; the camera's slot is
  reaped, not stranded (D12's claims rule — this is the scenario it was written against).
- **Around the loss**: `list` stops naming the camera; a `subscribe_events` watcher gets
  the removal within the hotplug bounds; a later return is a new arrival whose
  fingerprint tells the consumer it is the same device on a different address (D14/D15's
  split doing its job).
- **The protocol**: the recipe is an R3-class `hw_gone_*` suite, `#[ignore]`d,
  recipe-named, self-skipping counted on hosts that cannot arrange the event ("needs an
  arrangeable mid-stream device loss") — written and committed *here*, runnable *there*.
  Evidence lands as a dated E-entry with transcripts and the producing rig named;
  behavior contradicting this contract lands as a PF entry and a fake-fault amendment the
  same day (rule 4). No API change; the fake's `DeviceGoneMidStream` fault stays the
  hermetic stand-in, held to resemblance against the contributed record once it exists
  (E5, at last with something real to resemble).

**D20 — The operator's workbench** *(new at v3; owner, 2026-08-18).* The web client's P5
scope was supervision: look at cameras, watch a sweep the CLI drove. The owner's actual
session at the start of a development run is *tuning* — eyes on the preview, hands on the
controls — and *calibrating by eye*, which is D8's `selector: human` finally given a
producer. Three requirements, from the owner's own sentence: preview beside the controls;
tune and see without scrolling; drive a calibration session from the page.

- **The layout is two independent panes, and the preview never scrolls away.** One
  viewport-height app shell: the preview pane (the MJPEG `<img>`, the camera picker, the
  status line) stays put because **nothing scrolls under it** — the shell is a `100dvh`
  grid with `overflow: hidden` and the pane is a non-scrolling item of it — while the
  control column scrolls on its own axis beside it. `position: sticky` is the *stacked*
  shell's mechanism, where the document is the scroll container; at Chrome's feature
  level, per the §2.7 browser ruling. The
  precise requirement, stated testably: **the preview and the control being adjusted are
  simultaneously visible at every scroll position of the control column**, at the rung's
  pinned viewport size; on a viewport too narrow for two panes the shell stacks with the
  preview sticky at the top. The 77-control vivid case is the sizing fixture, not the
  18-control common case.
- **Live tuning is the existing machinery, arranged.** A slider or menu change issues the
  guarded write the panel already sends; the preview updates because control I/O
  interleaves with streaming (D12) and the stream is already on screen; `{requested,
  applied}` lands back on the widget, so a clamp visibly moves the slider to what the
  device did [PF:6] — the browser rung asserts that today and keeps it. What v3 adds is
  arrangement, not mechanism: the write's round trip must not lose the widget's identity
  (the M32/N154 fences stay), and a write during a suspend (a photo's pause, N83) queues
  on the actor exactly as any command does.
- **Human-driven calibration is the eight verbs the wire already has**, sequenced by the
  page: start (camera, task, goal) → plan → per control: sweep, watched live over the
  existing `subscribe_calibration` → **review the sample photographs** in a grid beside
  their metric scores → select by click, recorded as `selector: human` — the vocabulary
  D8 reserved for exactly this reviewer — → apply, restore-or-keep, with every refusal
  rendered from D13 (the state machine's `IllegalTransition`s are the page's guard rails,
  not duplicated client-side logic). The CLI flow is untouched and stays the agent's;
  the two flows share every verb, which is T4's law paying again one consumer over.
- **Sample review needs the sample bytes, and that is the one new route.**
  `/session-photo` serves a calibration sample image by *reference* — session id, control
  slug, sweep pass, value — with the file path derived server-side through the store's
  own layout rules (D9's slug transform and relative-path discipline; no caller-supplied
  path ever reaches the filesystem, so there is nothing to traverse). It is a GET an
  `<img>` can carry the token to, with a HEAD twin that answers about the route and
  never opens a file (the N179 precedent). **It serves stored camera frames, so it is
  camera-bearing by definition**: it joins `daemon::http::CAMERA_BEARING_PATHS` as the
  third entry, behind the token gate and the provenance layer like its two siblings —
  and its addition is deliberately the first exercise of the defect class N82's ruling
  created, with both halves going red on a route added without its gate
  (`web-routes-are-gated.sh`, `every_camera_bearing_route_is_behind_the_gate`) before
  the route lands. RPC delivery was considered and declined: base64 through
  `wch_calibrate_*` would spend a third of the response budget per photograph to avoid a
  route this posture already knows how to gate, and an `<img>` is the consumer.
- **The sweep/preview collision is resolved by the page, not the actor.** A sweep is
  minutes of exclusive capture, and D12 deliberately leaves it outside the suspend
  mechanism, so whichever streaming operation asks second meets `Busy` (N83's boundary;
  E16 measured the collision — a sweep arriving over a second socket was refused while a
  preview held the stream — and recorded it as a design question; this is the answer).
  The page therefore **ends its own preview request before `calibrate_sweep`**, and
  during the sweep the preview pane *becomes the sweep*: progress from the
  subscription, and the freshest sample rendered through `/session-photo` as each lands,
  so the operator watches what the camera saw at each step — truer than a live preview,
  which would show the settle transients between samples. Feeding sweep frames through
  the preview channel (N117's recording mechanism, one seam over) was considered and
  left as §8's item: it buys smoothness, costs new actor machinery, and the sample-based
  view answers the operator's actual question.
- **What the page still is not**: an agent surface (the guide and the CLIs are), a
  session editor (goal and criteria are set at start; free-text notes stay CLI-side), or
  a second calibration state machine — every transition the page offers is a verb the
  daemon refuses or performs, and the rung's claims drive the page's flow against those
  refusals rather than around them.

### 2.3 The backend contract (T1–T3)

**T1 — `CameraBackend`.** The pluggability seam, unchanged in shape — six methods since
P0/N7 and stable through v3:

```rust
pub trait CameraBackend: fmt::Debug + Send + Sync {
    fn kind(&self) -> BackendKind;                          // the closed vocabulary
    fn name(&self) -> &'static str { self.kind().as_str() } // derived; cannot disagree
    fn enumerate(&self) -> Result<Vec<CameraInfo>>;
    fn open(&self, id: &CameraId) -> Result<Box<dyn Camera>>;
    fn watch(&self) -> Result<Box<dyn HotplugWatch>>;       // add/remove events
    fn diagnose(&self) -> Vec<ListHint> { Vec::new() }      // absence, explained (D1) [N7]
}
```

D14 deliberately adds nothing here: selection is resolution over `enumerate`'s answer, not
a backend capability, so a new backend inherits every selector spelling for free.
`diagnose`'s contract carries the G6 repair: the hints describe the probe pass that
produced the listing they explain, never a second reading of the device (docs/11 M7). The
`HintKind` vocabulary stays closed at two, each of which met the bar of a design
requirement not otherwise satisfiable.

**T2 — `Camera`.** Blocking, object-safe, minimal — **nine methods**, the ninth absorbed
at this revision (N132; it landed with D12's 2026-08-12 suspend ruling, and a backend
written against the v2 sketch did not compile, which is why this was documentation debt
rather than a defect):

```rust
pub trait Camera: fmt::Debug + Send {
    fn info(&self) -> &CameraInfo;
    fn formats(&self) -> Result<Vec<FormatInfo>>;           // sizes × intervals nested [PF:9]
    fn controls(&self) -> Result<Vec<ControlDesc>>;         // D2, never panics [PF:1]
    fn get(&mut self, id: ControlId) -> Result<ControlValue>;
    fn set(&mut self, id: ControlId, v: ControlValue) -> Result<Applied>;  // D3 read-back
    fn start_stream(&mut self, req: &StreamRequest) -> Result<NegotiatedStream>;
    fn streaming(&self) -> Option<NegotiatedStream>;        // what a suspend must restore [N132]
    fn next_frame(&mut self, deadline: Instant) -> Result<Frame>;
    fn stop_stream(&mut self) -> Result<()>;
}
```

The traits' home stays `webcam-handler-schema`; traits take and return schema values only;
a backend never renders, persists, or decides policy. The two v2 contract notes stand —
**dispatch belongs to the descriptor** (`HAS_PAYLOAD`, never the caller's value variant;
the fake now obeys it too, and the array-control fixture is what can tell the two rules
apart — docs/11 M29, N135) and **the traits are total** (the transitional `Unimplemented`
died at P4d as scheduled). One note is new at v3:

- **A contract stated here is enforced where both backends inherit it, or it names the
  battery arm that walks it on both.** The G6 review's three stand-in-versus-real
  findings (H1, M29, M30) were all contracts each side could get wrong alone; the repairs
  moved the explicit-request refusal into the shared resolver (D5), the dispatch rule
  onto a fixture that separates it, and the coupling model onto measured corpus. §3.3
  item 10 keeps the gap named; this note is its standing instruction.

**T3 — The device profile.** The two-section shape stands — the *invariant* description
against the *state* block, provenance outside both — with three absorbed rules and one
addition. Absorbed: a node's `path` is provenance, not invariant — comparison excludes it
[N63, PF:22]; the format tree is invariant within a connection and nowhere else [N89,
PF:23], and the comparison answers per-section so that fact is representable; a compound
control's element count is state, not identity [PF:17]. Added at v3: **the
identity/device partition is a stated projection with a named-diff compare** — D15 —
which is the comparison semantics this section always owed a consumer comparing one
device across two addresses. Profiles are captured by the tool, committed with
provenance, immutable, replaced wholesale; a profile may carry *measured* automation
pairs when the caller asked for the probe and the probe restored (`--discover-pairs`,
owner ruling; N239). E5 applies to the fake's replay of every section, coupling included
— asserted from committed corpus that actually exhibits it (docs/11 M30).

### 2.4 The engine

The engine stays deliberately thin around pure cores — pairing planner, settle policy,
sweep planner, session state machine — with the imperative shell (actors, store,
sinks) around them, every shell seam doubled and fault-menu'd (§2.9). The metrics are
`webcam-handler-imaging`'s and not the engine's, which is where §2.5 puts them and what
the facade's stability table says. v3 adds two pure
cores and one public composition:

- **The stream-stats accumulator** (D16): pure, integer, bounded; lives beside the one
  existing home of interval arithmetic.
- **The comparison core** (D17): pure functions over decoded luma; total; represents its
  own unavailability.
- **`engine::facade`** (D18): not a core — the blessed shell composition, promoted, with
  the direct CLI rebuilt on it so it cannot drift.

`engine::resolve` remains the one resolution home and widens to D14's selector. The
calibration lifecycle keeps its G6-repaired recovery: `lifecycle::recover` restores the
camera *and* frees stranded sweeps on every arm, including the no-snapshot one (docs/11
H2's repair route — the killed-at-first-sample window leaves no snapshot at all, so the
walk lives outside the restore).

### 2.5 The V4L2 backend

Unchanged from v2 in structure — sysfs enumeration grouped by interface path, the raw
QUERY_EXT_CTRL control layer, our mmap streaming, the owned uevent socket, `/proc` busy
diagnosis — with the G6 findings folded where they landed:

- The control walk carries one declined control as valueless (`EBUSY` only) instead of
  ending [N192, N196]; `describe(id)` stops the walk at its target instead of paying the
  77-control sweep per guarded write [docs/11 P1; E18 measured it at four cameras].
- The second-stream refusal names nobody, and the holder walk excludes this process
  *inside* the walk, before the reporting cap [docs/11 M5; N191, N197].
- A device that states no frame interval is `FrameInterval::Unstated` — a fourth answer,
  never a fabricated discriminant [docs/11 M8; N194].
- The unsafe boundary rules stand verbatim (one module says `unsafe`; bindgen layouts
  only; one obligation per SAFETY block, restated where a comment had discharged an
  obligation its ioctls do not have [N190]; device-derived numbers validated — and the
  `bytesused` clamp now has the test whose inverse is a SIGSEGV [N188]; union-arm offsets
  derived through `offset_of!`, never transcribed [N187]). Kernel names the crate asks
  bindgen for are a declared build precondition with a gate, not a failed compile [N228,
  N236].

### 2.6 The daemon

As v2 built it — jsonrpsee 0.26 over the T5 trait, UDS via the owned glue, axum for
assets/WS/preview, tracing with the journald layer under systemd, sd-notify + listenfd
(with the daemon itself asking `SO_ACCEPTCONN`, the fifth check listenfd does not make —
N181) — plus D20's `/session-photo` route in the HTTP composition, gated and named
exactly as its two siblings (D11). Shutdown is D12's bounded table; SIGTERM ≡ SIGINT;
open streams are cancelled and their sockets shut down on expiry, never awaited.

### 2.7 The clients

**The command core (T4).** One clap tree, one rendering, one executor seam, two composition
roots — as v2 refined it (the root's name a parse parameter; `--backend`/`--profile`
refused by the client root with provenance read off `ValueSource`; a failing `--json` run
prints the one `Failure` document through the shared emitter). Two v3 additions:

- **A document verb runs below the executor.** `profile compare` and `photo diff` take
  files and answer documents; they touch no camera, no store and no socket, so they
  execute inside the command core itself, identically on both roots — parity for them is
  a property of there being one implementation rather than a comparison the gate must
  make. `cli-parity.sh`'s bucket vocabulary grows a fifth name (`document`) whose
  exemption argument is exactly that, stated in the header like the other four. The
  boundary is sharp: a verb that needs a backend, a store or a daemon is an executor
  verb, whatever it answers.
- **The selector reaches both roots as the same positional.** Every verb that took a
  camera id takes a selector spelling (D14); the parse lives in schema; clap sees an
  opaque string, so the tree does not fork.

**The web client.** Vanilla ES modules, no build step, no npm, no CDN, embedded via
`rust-embed`; Chrome is the supported target (owner ruling — modern platform APIs freely;
Firefox/Safari best-effort, never a constraint); the browser half is asserted in a real
browser by the pinned Playwright rung (§3.1 R1-web). v3 reshapes the page around D20 —
the two-pane workbench shell, live tuning, and the human-driven calibration flow — and
the module inventory moves accordingly; the prose that counts the files is reconciled by
the test that reads `paths()`, which is what stopped that count rotting [N153]. The RPC
helper carries its owner-ruled liveness (per-call timeout, idle heartbeat — N157), and
its size is whatever the module says it is: the module header's self-count was wrong the
day it was written down (53 said, 71 measured — N158; the v2 budget it overshot is priced
there and still bought), and this revision replaces the number with the rule — **a prose
count of code is a claim something reconciles or a dated measurement citing its entry,
and this document no longer makes any other kind.**

### 2.8 Workspace, dependencies, licenses

The workspace layout, naming convention (full `webcam-handler-` package prefix, short
directories, bare lib names, binaries named after their packages — owner rulings of
2026-08-13, N90; §8 carries the short-name question), dependency edges and purity walls
(T6) stand as v2 recorded them, including `webcam-handler-api`'s measured tokio exemption
[N5] and `webcam-handler-web`'s neither-stack wall. What changes at v3 is the **form of
the registry**, because prose rotted measurably: the G6 review found three adopted crates
the registry never learned, one version it misstated, and one evidence sentence that had
quietly stopped being true [N133], and the L32 pins-with-no-consumer sat beside them
[N164]. A registry a gate can read is the repair.

**The dependency registry.** One row per `[workspace.dependencies]` entry (the
manifest-side fact; this table is the design-side one), plus rows marked **(lock only)**
for edges the walls police that no manifest names; `dependency-registry-sync.sh` —
commissioned in docs/15 — reconciles table against manifest both ways, which is the
reconciler N133 asked for and the check whose absence L32 priced. Both directions in one
sentence: every manifest entry is registered by some row, and every row whose **pin cell
carries no bold parenthetical mark** names a crate the manifest has at the version the
row states. The mark vocabulary is closed and lives in the pin cell, where a version
would otherwise be — **(lock only)** for an edge the walls police that no manifest names,
and **(not yet an edge — …)** for an adoption this revision decided and a later phase
lands; a marked row still registers its crate, so a mark buys a row an absent manifest
entry and never a hidden one, and the gate counts and names every mark it honours. A
row may be a pin without a consumer only if it says so and names its disposition —
carrying one silently is the L32 defect this table exists to end. Versions are pins-at-adoption; licenses as verified at adoption; the *scope*
column names the crates whose edge it is, because an unscoped row is how `caps` went
unregistered inside the one crate whose dependency list most needs reviewing:

| Crate | Pin | License | Scope | Why |
|---|---|---|---|---|
| `v4l` | 0.14 | MIT | v4l2 | ioctl/mmap wrappers + bindgen types only; control and streaming layers are ours [PF:1]; `v4l2r` the named migration target |
| `kobject-uevent` | 0.2 | MIT | v4l2 | uevent packet parser; the socket is ours |
| `rustix` | 1.1.4 | Apache/MIT | v4l2 (net), daemon (fs,net,process,rand), engine (process — N150's euid check; its manifest says "no filesystem, no net"), client (process, dev-only) | safe syscalls so `forbid(unsafe_code)` crates can make them |
| `libc` | 0.2.189 | MIT/Apache | v4l2 | the ioctl numbers' types |
| `tokio` | 1.x | MIT | daemon, client (rt,net), api (via jsonrpsee — N5) | the async runtimes at the two socket roots |
| `tokio-util` | 0.7.19 | MIT | daemon (+ `compat` dev), client (`compat`, ship) | one stop token (`CancellationToken`); `compat` bridges a tokio stream onto soketto's `futures::io` traits |
| `tokio-stream` | 0.1.19 | MIT | daemon | `wrappers::ReceiverStream` and nothing else — the MJPEG preview's `mpsc::Receiver` onto `axum::body::Body::from_stream` (adopted P5b; registered at v3 — N133) |
| `axum` | 0.8.9 | MIT | daemon | the web listener |
| `tower` | 0.5.3 | MIT | daemon | the layer vocabulary axum composes (registered at v3 — N133) |
| `tower-http` | 0.6.11 | MIT | daemon | compression on the asset routes (version corrected at v3 — N133) |
| `jsonrpsee` | 0.26 | MIT | api (macros), client (async-client) | the one wire surface; minor pinned workspace-wide |
| `jsonrpsee-server` | 0.26 | MIT | daemon | the server by its own package name, never `jsonrpsee/server` — a feature unifies workspace-wide and would put hyper in `api`'s tree [N38] |
| `soketto` | 0.8.1 | Apache/MIT | client (ship), daemon (dev) | one WebSocket implementation on both ends |
| `futures-timer` | 3.0.4 | MIT/Apache | (jsonrpsee's) | carried visibly; the lock pins it |
| `hyper` | **(lock only)** | MIT | daemon (via axum/jsonrpsee-server) | never a direct edge; the walls hold it to one crate |
| `rust-embed` | 8 | MIT | web | asset embedding; `debug-embed` on [N77]; framework features declined |
| `serde` / `serde_json` | 1 | MIT/Apache | workspace | the four masters' one serialization |
| `schemars` | 1 | MIT | schema, api | the committed JSON Schema bundle |
| `zune-jpeg` | 0.5 | MIT/Apache/Zlib | imaging | JPEG decode |
| `yuv` | 0.8 | BSD-3/Apache | imaging | YUYV/NV12 conversion [N201's length rule] |
| `image` | 0.25, `default-features=false, features=["png","jpeg"]` | MIT/Apache | imaging | PNG/JPEG encode; the `avif` default stays off (gate-held) |
| `png` | 0.18 | MIT/Apache | imaging | PNG encode path |
| `imageproc` | 0.27 | MIT | imaging | Laplacian and friends (D8 metrics) |
| `image-compare` | 0.5 | MIT | imaging | SSIM corroborator; the conditional adoption's measurement cleared at P8b — its own `image` edge is `default-features = false`, the resolved graph stays clean, and six permissive packages join the lock (note **N260**, which also names the owned-SSIM exit) |
| `little_exif` | 0.6 | MIT/Apache | imaging | APP1 bytes only; our splice writes them [PF:16] |
| `y4m` | 0.8 | MIT | (pin, not linked) | measured and declined at P6b — the module writes its own 51-line sink [N107]; the pin stays by that entry's ruling, removal being the owner's |
| `clap` | 4 | MIT/Apache | cli-core | the one command tree |
| `comfy-table` | 8 | MIT | cli-core | listings |
| `indicatif` | 0.18 | MIT | cli-core | sweep progress |
| `anstream`/`anstyle` | 1.0.0 / 1.0.13 | MIT/Apache | (clap's; pins) | clap 4's own color stack, carried visibly like `futures-timer`; no crate names either directly |
| `humantime` | 2 | MIT/Apache | cli-core | human durations [N113] |
| `thiserror` / `anyhow` | 2 / 1 | MIT/Apache | workspace / roots | error derivation; anyhow never crosses a wall |
| `tracing` + `tracing-subscriber` + `tracing-journald` | 0.3.2 (journald) | MIT | daemon | one logging edge; journald instead of fmt under systemd; the engine still has none by design |
| `sd-notify` | 0.5.0 | MIT/Apache | daemon | READY/STATUS/STOPPING/WATCHDOG |
| `listenfd` | 1.0.2 | Apache-2.0 | daemon | LISTEN_FDS validation, four fifths of it [N181] |
| `tempfile` | 3 | MIT/Apache | engine | the atomic write's temp file |
| `fd-lock` | 4 | MIT/Apache | engine | the one store lock |
| `uuid` | 1 (v7) | MIT/Apache | schema | session ids |
| `jiff` | 0.2 | Unlicense/MIT | schema | timestamps; RFC 3339 on disk |
| `camino` | 1.2.5 | MIT/Apache | schema | UTF-8 paths on the wire and on disk |
| `base64` | 0.23 | MIT/Apache | api only | D10's transport encoding, one home |
| `caps` | 0.5.6 | MIT/Apache | priv (**dev-only**) | ambient-capability handling in the blessed helper (registered at v3 — N133; the crate with the strongest reason to be reviewed is the one that was unlisted) |
| `kamadak-exif` | 0.6 | BSD-2 | imaging (dev) | the independent EXIF read-back oracle |

Dev-only and build-time entries beyond these (nextest, cargo-deny, cargo-mutants,
shellcheck, typos, the pinned Playwright/Chromium, ffprobe/mpv as test oracles, bindgen's
libclang + kernel headers) are tooling, not linkage, and live where their gates document
them. The long adoption arguments — N5's measured exemption, rustix over libc, soketto on
both ends, the two-entry tokio-util story, `directories` dropped for ~30 owned lines [N2]
— stand in docs/6 v2 §2.8 and the notes; this table is the *what*, those are the *why*,
and the reconciler holds the what.

**License allowlist and named bans**: unchanged, and `deny.toml` is the authoritative
population (MIT, Apache-2.0 ± LLVM-exception, BSD-2/3, ISC, Zlib, 0BSD, MIT-0, Unlicense,
CC0-1.0, Unicode-3.0; bans on the LGPL linkage families, the MPL wrappers, GPL codec
stacks, AGPL `dssim`, the IJG trio, and `option-ext`). **Who decides an adoption** stands
as the 2026-08-09 ruling: applying the bar is routine and says what it concluded; moving
the bar is the owner's.

### 2.9 Seams and doubles

The v2 table stands, with the recording seam recorded (it existed since P6b without a
row — N111's honest note) and D16's fault:

| Seam | Real | Double | Fault menu |
|---|---|---|---|
| `CameraBackend`/`Camera` (T1/T2) | `webcam-handler-v4l2` | `webcam-handler-fake` | device-gone mid-stream, EBUSY, clamp-on-write [PF:6], INACTIVE flips [PF:3], control-read-declined [N195], settle-never-converges, frame timeout, **frame gap (v3, D16)**, hotplug add/remove, watch unavailable/fails |
| Session store (D9) | XDG state dir | temp-dir store | full disk, lock held, torn `log.ndjson` line, foreign `schema_version` |
| Recording sink (D7) | files on disk | scripted `Files` double | open refused, write refused, sync refused |
| Clock/settle | real time | `SteppedClock` / `FrozenClock` [N67] | deadline expiry during settle; a deadline that cannot expire |
| RPC transport | UDS/WS | in-memory jsonrpsee | disconnect mid-subscription, undecodable notification [N70] |

Fault menus are exhaustive-match-walked enums; each fault is consumed where it decides
its answer, so a one-shot fault reports once and only when it fired [N232]. A double's
claims are checked against the thing it stands in for — a fake capability no real device
exhibits is a bug in the fake [PF:17, N136], and a fault-menu doc that states a
dependency's behavior is a claim somebody reads the dependency to verify [N70].

### 2.10 Single-copy homes

| Law | Home |
|---|---|
| Control model semantics (types, flags, sparse menus, the slug transform) | `webcam-handler-schema::control` |
| The backend contract (T1/T2 traits, `BackendKind`) | `webcam-handler-schema::backend` |
| **Camera selection — every spelling, one parser, one resolver (v3)** | `webcam-handler-schema::selector` + `webcam-handler-engine::resolve` |
| **The profile's identity/device partition and its compare (v3)** | `webcam-handler-schema::profile` (the destructuring projection) |
| Auto/manual pairing (declared + measured merge) | `webcam-handler-schema` data + `webcam-handler-engine::pairing` |
| Requested-vs-applied write semantics | `Camera::set` contract (T2) |
| **Backend-contract refusals that both backends must make (v3)** | the shared resolver that computes the answer — `StreamRequest::choose` is the exemplar (D5); a per-backend copy of a shared refusal is the H1 defect class |
| **A claim on a camera (v3)** | a value whose release is its own — `Drop` or `#[must_use]`, `Weak`-witnessed, reaped (D12; N169) |
| Atomic state writes | `webcam-handler-engine::store::write_json_atomic` |
| Settle policy | `webcam-handler-engine::settle` |
| Sweep value derivation | `webcam-handler-engine::sweep::plan` |
| **Interval and delivery arithmetic (v3, absorbing P6d's move)** | `webcam-handler-imaging::video` + the D16 accumulator beside it |
| Error registry + RPC codes + exit codes | `webcam-handler-schema::error` + `webcam-handler-api::codes` + `cli_core::exit_code` (three exhaustive matches over one `ALL`) |
| Command surface (verbs, flags, rendering) | the T4 core (`webcam-handler-cli-core`) |
| **The blessed in-process composition (v3)** | `webcam-handler-engine::facade` (D18); the CLI executor is its consumer, never its sibling |
| Wire surface | the T5 declaration in `webcam-handler-api` |
| Capture settle defaults, size caps, path layouts | `webcam-handler-schema::limits` |
| JPEG pass-through vs re-encode (E6) | `webcam-handler-imaging::photo` |
| The unsafe surface | `crates/backends/v4l2/src/sys/` |

A second copy of any of these is a review finding; the gates enforce the mechanically
checkable ones (docs/15).

### 2.11 The backend playbook

Unchanged in its five steps — new crate against schema values only; capture and commit
profiles; add the `BackendKind` variant and the two factory matches; **run the
conformance battery, which is the definition of done**; hardware suites `#[ignore]`d and
restoring. The battery's arm inventory at v3: enumeration, control model, write
read-back, snapshot/restore inverse (Drop-guarded against the mid-arm return that left a
camera moved — docs/11 M31, N137), stream lifecycle, **explicit request** (the H1 arm),
hotplug watch, fault menu — and D16's frame-accounting assertions ride the stream arms.
Step 4's sentence from §3.3 item 10 is now printed here too: **ask of every backend
contract which arm of the battery would fail if one side stopped honouring it.**

### 2.12 The evidence doctrine

E1–E6 stand verbatim (documentation nominates, the device legislates; the device is the
only authority on itself; an answer is evidence and a busy device is not an absent
capability; requested is not applied; a stand-in resembles; verbatim bytes where bytes
are the product). v3 adds no doctrine letter: the two structural principles the G6 review
earned are §2.10 laws (shared refusal homes; claims come back with their values), because
they are about where code lives, not about what counts as evidence. One clarification
earned by D19: **a resemblance claim about an event this rig cannot produce is `declared`
until a rig that can produce it contributes the measurement** — E5 with its provenance
vocabulary applied to the fake's fault menu.

### 2.13 The privileged development helper (`webcam-handler-priv`)

As v2 left it after the G6 reckoning executed (P6e; N125, N126): dev-only,
`cap_sys_module+ep` over a closed verb vocabulary — two module names, compile-time
constants — blessed mode-0700 into `.wch-bin/`, refusing to unload `uvcvideo` under any
open node, never a dependency of a product crate. The boundary is the file mode plus the
account; `privileged-helper.sh` re-checks the mode, the equality of carried capabilities,
and that no other capability-carrying file sits in the directory. The standing
instruction is unchanged and unweakened: never widen the verbs toward caller-supplied
names, paths or programs, and never add a capability, without amending N8 and N125 first.
D19 changes nothing here — the interlock is *why* mid-stream loss is a partner
measurement, and that is the design working, not a gap in it.

## 3. Test architecture

### 3.1 The rung ladder

R0–R3 plus R1-web, unchanged in shape and updated in fact:

- **R0 — pure, hermetic (every push):** schema round-trips, planner/state-machine/metric
  properties, imaging codecs on synthetic fixtures, muxer byte expectations against the
  independently derived re-parser, error-mapping exhaustiveness, Miri over the
  unsafe-adjacent pure units. v3 adds the selector parser (every spelling, both
  directions), the D15 projection (partition-closure by destructuring; corpus
  self-compare and cross-compare), the D16 accumulator (constructed sequence/timestamp
  vectors, gap and jitter cases, the truncation boundary), and the D17 core (metric
  deltas, the SSIM-unavailable representations).
- **R1 — fake-backend integration (every push):** engine + store + calibration end to end
  over profile replay; daemon + client over real sockets; both CLI binaries as
  subprocesses. v3 adds selector resolution through both roots and the wire, the
  `FrameGap` fault driving `RecordReport.stats`, and the document verbs' `--json`
  round-trips.
- **R1-web — the browser rung (per push where the host has node; counted named skip
  elsewhere):** the pinned Playwright/Chromium suite — 24 claims and 206 assertions at
  this revision's baseline, claims manifest-counted both ways. D20 grows it: the
  workbench layout claim (**preview and the adjusted control simultaneously visible at
  every scroll position of the control column**, asserted at the pinned viewport against
  the replayed vivid profile's 77 controls — the sizing fixture, not the friendly case);
  live tuning round-trips (a clamp moves the slider, on screen); the human calibration
  flow end to end — start, plan, sweep watched live, sample grid painted through
  `/session-photo`, a click recording `selector: human`, apply, restore — driven against
  the fake and asserted through the page, with the anonymous and cross-site refusals of
  the new route beside them; and the sweep-time pane swap (progress plus freshest sample
  instead of a dead preview).
- **R2 — kernel-virtual (opportunistic gate):** vivid via the blessed helper; proves
  ioctl plumbing, not device quirks; serialized with R3 in the one-thread
  `exclusive-device` group. The 77-control surface doubles as D20's layout fixture and
  D14's widest resolution population.
- **R3 — real hardware (`#[ignore]`d, recipe-named, on demand):** enumeration matches
  committed profiles, capture decodes, writes read back, snapshot/restore asserted,
  motors bounded and returned. v3 adds `hw_` twins for selector resolution against the
  attached cameras (the shared-`usb_id` ambiguity on the Chicony pair is a live fixture)
  and D19's `hw_gone_*` recipes, which **self-skip counted on every host that cannot
  arrange a mid-stream loss** — written here, runnable on the partner rig, so the
  contract's tests exist before the event does.

### 3.2 Corpus rules

Unchanged: tool-captured, provenance-stamped, immutable, replaced wholesale; profile-shaped
PF findings representable in and asserted from committed profiles; synthetic images only;
fixtures enter tests as bytes. v3 notes: the corpus now carries **measured pairs** on one
profile (chicony-rgb, two pairs — N239, E18) and D15's cross-compare turns the whole
corpus into mutual negative fixtures (every profile device-differs from every other, with
the differing sections named — a population, not a sample).

### 3.3 The structural-gap register (v3, honest)

Regenerated at this revision, not accreted (rubric rule 4):

1. **Real-hardware truth is unautomatable in shared CI.** R3 runs where a camera is
   attached; corpus mitigates; a new kernel × device interaction is invisible until
   someone runs R3 against it.
2. **The fake's physics are a model.** Clamping, coupling and frame response replay from
   profiles; frame synthesis is invented. Calibration *logic* is fully tested;
   calibration *efficacy* on real optics is R3's alone — and D20's human flow inherits
   this: the rung proves the page drives the verbs, not that a human picks well.
3. **USB bandwidth and multi-camera contention are not modeled** — concurrent-stream
   failures on real hubs will look like driver errors, reported honestly [D13], predicted
   by nothing. D16 makes their *symptoms* measurable (gaps, jitter); it does not model
   their cause.
4. **`vivid` fidelity is partial**: the rung proves ioctl plumbing, not device quirks.
5. **The AVI muxer's player-compatibility claim** rests on the ffprobe/mpv oracles where
   the host has them (named, counted decline elsewhere) plus manual checks; the two
   oracles share one FFmpeg build, so they are honestly one parser and a playability
   check [N119]; the CFR-vs-delivery residual is bounded and accepted [N120].
6. **Privacy canary limits**: the frame gate sniffs known formats and walks two
   containers; a frame in an unrecognized envelope passes it; review carries that half.
7. **The browser rung drives Chromium only**, by ruling; Firefox/Safari are unexercised.
8. **All hardware evidence is one machine, one kernel, and the cameras attached to it.**
   Five profiles committed; four cameras plus vivid at E18. The camera count moves; the
   machine count has not, and the machine count is what bounds the claim. **D19 and D15
   are the first designed-for path out** — a second machine, a second kernel, and the
   comparison vocabulary to say what carried over — but until the partner rig runs, that
   is a design, not evidence.
9. **Mid-stream device loss is stated, modeled, and locally unmeasurable.** The helper's
   interlock (§2.13) keeps real cycles camera-closed, so `DeviceGone` mid-stream is the
   fake's scripted fault — **and, new at v3, a stated contract (D19) with committed
   `hw_gone_*` recipes that only a rig arranging real loss can run.** Until such a rig
   contributes its first E-entry, every sentence in D19 is `declared`.
10. **A contract can be asserted over one backend and nowhere else, and nothing says so.**
    The G6 review's H1/M29/M30 family; both instances closed, the gap structural and
    named. The defence is a population walked on both backends — ask of every backend
    contract which battery arm would fail if one side stopped honouring it (§2.11). v3's
    new contracts each name their arm at birth.
11. **Cross-machine comparison is corpus-shaped until it is not** *(new at v3)*: D15's
    masked compare is asserted over identity-rewritten corpus and profile pairs from one
    machine. Its motivating claim — "the forwarded camera describes itself identically" —
    is exactly the claim only the partner rig can measure, and the first contributed
    comparison record is what turns this item's `declared` into `measured`.
12. **Perceptual similarity has no ground truth on this rig** *(new at v3)*: D17's SSIM
    corroborates and is itself corroborated only by the metric deltas beside it; no
    committed fixture can say what similarity score two *cameras* ought to earn. The
    metric-ordering fixtures bound the pure math; the meaning of a score stays the
    consumer's.

## 4. Phased plan

Lives in **docs/13 (implementation plan v3)** — the P0–P6 closure ledger carries forward
by reference to docs/7, and the v3 work is cut into phases P7–P9 with gates G7–G9 as
named, counted, re-runnable criterion sets over `scripts/gates/phase-criteria.tsv`, one
row per criterion, accreting as sub-milestones land. The standing conventions
(session-sized sub-milestones, reviews in their own sessions, notes written the day a
thing is learned) are restated there and unchanged.

## 5. Hardware and privacy discipline

The v2 rules stand unchanged: frames never enter the repository, logs or error messages;
test captures in gitignored scratch; leave the camera as found, with restoration asserted;
motors bounded everywhere, product motion behind `--allow-motion`, test motion on by
default with `WCH_NO_MOTION=1` as the counted opt-out; busy devices belong to someone and
killing a holder is a distinct explicit command; the `Privacy` control is honored, never
worked around. Two v3 clarifications, both from D20:

- **Session photographs leave the machine through exactly one door**, and it is gated:
  `/session-photo` is on `CAMERA_BEARING_PATHS`, behind the bearer token and the
  provenance layer, addressed by reference with the path derived server-side (D9's slug
  rules — caller-supplied text never becomes a filesystem path). The session tree's 0700
  posture (D9) is the same fact one layer down.
- **The workbench changes what the operator can do, not what the daemon records**: the
  page drives verbs the daemon already had; nothing is stored that a CLI session would
  not store; the preview remains served, never written.

## 6. Risks

The v2 register stands (the semi-dormant `v4l` crate behind our narrow use; bindgen's
header-vintage build precondition [N236]; jsonrpsee 0.x churn behind pinned minors;
kernel/driver variance answered by doctrine; the owned muxer, oracle-tested; crashed
sweeps answered by persisted snapshots and the recovery walk; rust-embed governance;
the root-capable helper behind its file mode and gates). v3 adds three:

- **The v3 surface leans harder on one sibling's roadmap.** D19's measurement and §3.3
  item 11's retirement wait on the partner rig existing and caring. Contained: everything
  v3 ships is useful to the first two consumers on its own (selectors, comparison, stats,
  the facade and the workbench all have local users), and the partner-shaped halves are
  contracts and recipes, which cost nothing to hold.
- **The workbench doubles the web client's behavioral surface** while the client stays
  hand-written vanilla JS with a rung, not a framework with a type system. Contained the
  way P5/P6 contained it: every flow is a browser-rung claim with counted assertions, the
  page holds no second state machine (the daemon's refusals are the guard rails), and the
  G6 review's client findings (stale-identity fences, sentinel identities, one writer per
  element) are standing rules the rung already pins.
- **`image-compare` is a single-maintainer dependency for one function.** Contained by
  the adoption condition (a dirty feature graph refuses it), the named owned fallback,
  and the fact that nothing on the measurement path depends on SSIM — deltas and
  orderings come from owned metrics.

## 7. Considered and not adopted

The v2 list stands in full (nokhwa, v4l2r-now, udev, libcamera, shelling out, MP4/MKV as
the v1 container, SQLite/redb/sled, RON/TOML, WASM clients, the axum alternatives, the
jsonrpc families, daemonize/figment/colored/fs2/prettytable, dssim, ndarray/statrs). Three
v3 additions:

- **WebM as a recording output** (the owner asked this revision to look — N103). Declined
  as anything but a future opt-in second output. WebM carries only VP8/VP9/AV1, so it
  means an *encode* — against D7 L0's no-encoder posture and E6's byte-fidelity doctrine
  on the path that matters; Matroska `V_MJPEG` remuxes without an encoder and no major
  vendor ingests it, which sharpens §7's original MKV rejection and retroactively
  justifies AVI with a reason the v1 decision never needed. If model-vendor ingestion
  becomes a real need, the recorded path is L2 (rav1e, AV1-in-WebM) as an explicitly
  lossy *ingestion* output beside the verbatim AVI — never the measurement default. The
  vendor-acceptance table (Gemini: AVI; OpenAI: WebM) is `declared`, dated 2026-08-14.
  N103's own make-`measured` bar was a dated re-read of each vendor's accepted-format
  list; this revision deliberately sharpens it — an actual upload attempt is what makes
  the table `measured`, because a format list is itself a vendor's `declared` claim.
- **A tool-side two-camera simultaneous capture verb** (§1 non-goals; D17's argument).
- **Serving calibration samples over RPC as base64** (D20's argument — a route this
  posture knows how to gate beats spending the response budget to avoid one).

## 8. Open questions

Regenerated at this revision; v2's items 1–10 that remain open carry forward by number,
the discharged ones are marked, and v3's join the list:

1. **IJG — dormant, unchanged**: the default stack is allowlist-clean; the ban list keeps
   IJG arriving only by decision.
2. **Audio — blocked on licensing, unchanged.**
3. **UVC H.264 (D7 L1) — waits on hardware that exhibits it (E2), unchanged.**
4. **Control-change events (`VIDIOC_SUBSCRIBE_EVENT`) — sharpened by D20**: the workbench
   is the consumer this item always named (another client's write should move the panel
   without a reload). Still uncommissioned; the trigger is now concrete — commission it
   when the workbench's stale-panel cost is observed in real use, not before.
5. **Multi-camera USB bandwidth — unchanged**, with D16 as the instrument that will make
   the symptom measurable when it appears.
6. **Metric growth — unchanged**; D17's SSIM entry is the named trigger's first firing,
   and response-curve fitting still waits for a session that needs it.
7. **`webcam-handler-cli` auto-forward to a running daemon — unchanged.**
8. **Session retention/GC — unchanged**, still deliberately uncommissioned; N55's
   re-phrased trigger (a measured quantity, instrumentation before policy) stands, and
   D20 adds mild pressure (a workbench browses sessions, so an unbounded store is now
   *visible*), which is recorded, not acted on.
9. **An MCP server surface — unchanged**: a fourth consumer of the one wire surface,
   uncommissioned until an agent runtime that wants one appears.
10. ~~The uevent capability question~~ — answered and executed (PF:21; N125). Closed.
11. **The short name** *(N91 — the owner asked this revision to take it up)*. The
    evidence: `TASK_COMM_LEN` truncates all four binary names to `webcam-handler-`, so
    D9's lock-holder `comm` and every `ps` line lost their diagnostic value [N90]; the
    socket path reaches 146 bytes against `sun_path`'s 107-byte usable bound on deep
    runtime dirs [N84]; and three of N91's five name categories already answer to `wch`
    (the wire namespace `wch_*`, the `WCH_*` environment variables, and the on-disk
    paths — `wchd.sock`, `.wch-bin/`) while the two that do not are the package names
    and the binary names. **Recommendation**: adopt `wch-` as the package prefix in one sweep
    (`wch-cli`, `wch-daemon`, `wch-client`, `wch-priv`, `wch-schema`, …), keeping N90's
    law (a binary is named after its package — the four names then fit `comm` whole),
    keeping the repository name `webcam-handler`, and **never touching the wire
    namespace, the socket filename, or the `WCH_*` environment variables** — N91's own
    catalogue marks the first a wire break and the others already short. The rename is
    one sub-milestone with the N90/N126 checklists (the orphaned-blessed-copy class) and
    lands only on the owner's ruling; nothing in P7–P9 depends on it either way.
12. **A process-failure kind** *(N238)*: with overflow checks shipped on, a daemon-side
    panic reaches a caller as `DeviceGone` (rule 7 inverted through a panic's only exit)
    and a CLI root exits 101 with no `Failure` document. Closing it needs a D13 kind for
    "this process failed, the device did not" — a wire and exit-code change, so an owner
    ruling; until then the gap is stated here and in N238, deliberately unglossed.
13. **Feeding sweep samples through the preview channel** *(D20)*: the sample-based
    sweep view answers the operator's question today; N117's recording mechanism is the
    named shape if a live sweep feed ever earns its cost. Uncommissioned.
14. **A native selector in `list` output** *(D14 adjacency)*: whether `list` should print
    the selector spellings each camera answers to (it already prints the fields they
    match). Cosmetic; decide from agent-guide feedback.
